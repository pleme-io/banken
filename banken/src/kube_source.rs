use std::collections::HashMap;
use std::fmt::Debug;

use banken_spec::env::Row;
use banken_spec::types::ResourceKind;
use futures::{AsyncBufReadExt, StreamExt, TryStreamExt};
use izumi_armazem::{ArmazemConfig, BoxFuture, ConfigError, Feed, Source};
use k8s_openapi::serde::de::DeserializeOwned;
use kube::api::{ListParams, LogParams, WatchEvent, WatchParams};
use kube::core::{ClusterResourceScope, NamespaceResourceScope};
use kube::runtime::{WatchStreamExt, watcher};
use kube::{Api, Client, Resource};

use crate::absorb::ListStrategy;
use crate::armazem::{Estoque, Fonte, KindKey, LogKey, LogLine};
use crate::live::KubeClusterEnv;

pub struct KubeSource {
    client: Client,
    strategy: ListStrategy,
}

impl KubeSource {
    #[must_use]
    pub fn new(client: Client, strategy: ListStrategy) -> Self {
        Self { client, strategy }
    }
}

trait KindReader: Send + Sync {
    fn absorb(
        &self,
        client: Client,
        key: KindKey,
        resume: Option<String>,
        strategy: ListStrategy,
        feed: Feed<Row>,
    ) -> BoxFuture<()>;

    fn fetch(&self, client: Client, key: KindKey) -> BoxFuture<Result<Vec<Row>, String>>;
}

struct Typed<K> {
    api: fn(Client, &KindKey) -> Api<K>,
    project: fn(K) -> Option<Row>,
}

fn namespaced<K>(client: Client, key: &KindKey) -> Api<K>
where
    K: Resource<Scope = NamespaceResourceScope>,
    K::DynamicType: Default,
{
    match key.namespace() {
        Some(ns) => Api::namespaced(client, ns),
        None => Api::all(client),
    }
}

fn clustered<K>(client: Client, _key: &KindKey) -> Api<K>
where
    K: Resource<Scope = ClusterResourceScope>,
    K::DynamicType: Default,
{
    Api::all(client)
}

fn reader(kind: ResourceKind) -> Box<dyn KindReader> {
    use crate::live::{
        configmap_to_row, deployment_to_row, endpoints_to_row, event_to_row, namespace_to_row,
        node_to_row, pod_to_row, replicaset_to_row, service_to_row,
    };
    use k8s_openapi::api::apps::v1::{Deployment, ReplicaSet};
    use k8s_openapi::api::core::v1::{ConfigMap, Endpoints, Event, Namespace, Node, Pod, Service};
    match kind {
        ResourceKind::Pod => Box::new(Typed::<Pod> {
            api: namespaced,
            project: pod_to_row,
        }),
        ResourceKind::Service => Box::new(Typed::<Service> {
            api: namespaced,
            project: service_to_row,
        }),
        ResourceKind::ConfigMap => Box::new(Typed::<ConfigMap> {
            api: namespaced,
            project: configmap_to_row,
        }),
        ResourceKind::Endpoints => Box::new(Typed::<Endpoints> {
            api: namespaced,
            project: endpoints_to_row,
        }),
        ResourceKind::Deployment => Box::new(Typed::<Deployment> {
            api: namespaced,
            project: deployment_to_row,
        }),
        ResourceKind::ReplicaSet => Box::new(Typed::<ReplicaSet> {
            api: namespaced,
            project: replicaset_to_row,
        }),
        ResourceKind::Event => Box::new(Typed::<Event> {
            api: namespaced,
            project: event_to_row,
        }),
        ResourceKind::Namespace => Box::new(Typed::<Namespace> {
            api: clustered,
            project: namespace_to_row,
        }),
        ResourceKind::Node => Box::new(Typed::<Node> {
            api: clustered,
            project: node_to_row,
        }),
    }
}

#[derive(Default)]
struct Placas {
    by_name: HashMap<(Option<String>, String), String>,
}

impl Placas {
    fn upsert(&mut self, feed: &Feed<Row>, row: Row) {
        if let Some(v) = &row.version {
            feed.resume_at(v.clone());
        }
        let uid = row.uid.as_str().to_owned();
        if let Some(old) = self
            .by_name
            .insert((row.namespace.clone(), row.name.clone()), uid.clone())
            && old != uid
        {
            feed.remove(old);
        }
        feed.upsert(row);
    }

    fn remove(&mut self, feed: &Feed<Row>, row: &Row) {
        if let Some(v) = &row.version {
            feed.resume_at(v.clone());
        }
        let slot = (row.namespace.clone(), row.name.clone());
        if self
            .by_name
            .get(&slot)
            .is_some_and(|u| u == row.uid.as_str())
        {
            self.by_name.remove(&slot);
        }
        feed.remove(row.uid.as_str());
    }

    fn reset(&mut self, feed: &Feed<Row>, rows: Vec<Row>, last: Option<String>) {
        self.by_name = rows
            .iter()
            .map(|r| {
                (
                    (r.namespace.clone(), r.name.clone()),
                    r.uid.as_str().to_owned(),
                )
            })
            .collect();
        feed.reset(rows);
        if let Some(v) = last {
            feed.resume_at(v);
        }
    }
}

const QUIET_IS_LIVE: std::time::Duration = std::time::Duration::from_secs(1);
const RESUME_OPENS_WITHIN: std::time::Duration = std::time::Duration::from_secs(5);

fn watch_error(e: &kube::Error) -> (Option<u16>, String) {
    match e {
        kube::Error::Api(r) => (Some(r.code), r.message.clone()),
        other => (None, other.to_string()),
    }
}

fn relist_cause(rv: &str, code: Option<u16>, message: &str) -> String {
    if code == Some(410) {
        format!("resume from resourceVersion {rv} expired (410 Gone); relisting")
    } else {
        format!("resume from resourceVersion {rv} failed ({message}); relisting")
    }
}

async fn resume<K>(
    api: &Api<K>,
    project: fn(K) -> Option<Row>,
    mut rv: String,
    feed: &Feed<Row>,
) -> String
where
    K: Resource + Clone + DeserializeOwned + Debug + Send + 'static,
{
    let mut placas = Placas::default();
    loop {
        let params = WatchParams::default();
        let stream = match tokio::time::timeout(RESUME_OPENS_WITHIN, api.watch(&params, &rv)).await
        {
            Ok(Ok(s)) => s,
            Ok(Err(e)) => {
                let (code, message) = watch_error(&e);
                return relist_cause(&rv, code, &message);
            }
            Err(_) => {
                return format!(
                    "resume from resourceVersion {rv} did not open within {}s; relisting",
                    RESUME_OPENS_WITHIN.as_secs()
                );
            }
        };
        futures::pin_mut!(stream);
        let mut heard = false;
        let mut vouched = false;
        loop {
            let next = if vouched {
                stream.try_next().await
            } else {
                let Ok(next) = tokio::time::timeout(QUIET_IS_LIVE, stream.try_next()).await else {
                    vouched = true;
                    feed.live();
                    continue;
                };
                next
            };
            heard |= matches!(next, Ok(Some(_)));
            if !vouched && matches!(next, Ok(Some(ref e)) if !matches!(e, WatchEvent::Error(_))) {
                vouched = true;
                feed.live();
            }
            match next {
                Ok(Some(WatchEvent::Added(o) | WatchEvent::Modified(o))) => {
                    if let Some(row) = project(o) {
                        if let Some(v) = &row.version {
                            rv.clone_from(v);
                        }
                        placas.upsert(feed, row);
                    }
                }
                Ok(Some(WatchEvent::Deleted(o))) => {
                    if let Some(row) = project(o) {
                        if let Some(v) = &row.version {
                            rv.clone_from(v);
                        }
                        placas.remove(feed, &row);
                    }
                }
                Ok(Some(WatchEvent::Bookmark(b))) => {
                    rv = b.metadata.resource_version;
                    feed.resume_at(rv.clone());
                }
                Ok(Some(WatchEvent::Error(e))) => {
                    return relist_cause(&rv, Some(e.code), &e.message);
                }
                Ok(None) => break,
                Err(e) => {
                    let (code, message) = watch_error(&e);
                    return relist_cause(&rv, code, &message);
                }
            }
        }
        if !heard {
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        }
    }
}

async fn relist<K>(
    api: Api<K>,
    project: fn(K) -> Option<Row>,
    strategy: ListStrategy,
    feed: &Feed<Row>,
) where
    K: Resource + Clone + DeserializeOwned + Debug + Send + Sync + 'static,
    K::DynamicType: Default + Eq + std::hash::Hash + Clone,
{
    let config = match strategy {
        ListStrategy::Streaming => watcher::Config::default().streaming_lists(),
        ListStrategy::ListWatch => watcher::Config::default(),
    };
    let stream = watcher(api, config).default_backoff();
    futures::pin_mut!(stream);
    let mut placas = Placas::default();
    let mut building: Option<Vec<Row>> = None;
    let mut last: Option<String> = None;
    while let Some(next) = stream.next().await {
        match next {
            Ok(watcher::Event::Init) => building = Some(Vec::new()),
            Ok(watcher::Event::InitApply(o)) => {
                if let Some(row) = project(o) {
                    last.clone_from(&row.version);
                    match building.as_mut() {
                        Some(b) => b.push(row),
                        None => placas.upsert(feed, row),
                    }
                }
            }
            Ok(watcher::Event::InitDone) => {
                if let Some(rows) = building.take() {
                    placas.reset(feed, rows, last.take());
                }
                feed.live();
            }
            Ok(watcher::Event::Apply(o)) => {
                if let Some(row) = project(o) {
                    placas.upsert(feed, row);
                }
                feed.live();
            }
            Ok(watcher::Event::Delete(o)) => {
                if let Some(row) = project(o) {
                    placas.remove(feed, &row);
                }
                feed.live();
            }
            Err(e) => feed.degraded(e.to_string()),
        }
    }
}

impl<K> KindReader for Typed<K>
where
    K: Resource + Clone + DeserializeOwned + Debug + Send + Sync + 'static,
    K::DynamicType: Default + Eq + std::hash::Hash + Clone,
{
    fn absorb(
        &self,
        client: Client,
        key: KindKey,
        from: Option<String>,
        strategy: ListStrategy,
        feed: Feed<Row>,
    ) -> BoxFuture<()> {
        let api = (self.api)(client, &key);
        let project = self.project;
        Box::pin(async move {
            if let Some(rv) = from {
                let cause = resume(&api, project, rv, &feed).await;
                feed.degraded(cause);
            }
            relist(api, project, strategy, &feed).await;
        })
    }

    fn fetch(&self, client: Client, key: KindKey) -> BoxFuture<Result<Vec<Row>, String>> {
        let api = (self.api)(client, &key);
        let project = self.project;
        Box::pin(async move {
            api.list(&ListParams::default())
                .await
                .map(|l| l.items.into_iter().filter_map(project).collect())
                .map_err(|e| e.to_string())
        })
    }
}

impl Source for KubeSource {
    type Key = KindKey;
    type Record = Row;

    fn open(&self, key: KindKey, resume: Option<String>, feed: Feed<Row>) -> BoxFuture<()> {
        reader(key.kind()).absorb(self.client.clone(), key, resume, self.strategy, feed)
    }

    fn fetch(&self, key: KindKey) -> BoxFuture<Result<Vec<Row>, String>> {
        reader(key.kind()).fetch(self.client.clone(), key)
    }
}

pub struct KubeLogSource {
    client: Client,
}

impl KubeLogSource {
    #[must_use]
    pub fn new(client: Client) -> Self {
        Self { client }
    }
}

fn split_stamp(line: &str) -> LogLine {
    match line.split_once(' ') {
        Some((at, text)) if chrono::DateTime::parse_from_rfc3339(at).is_ok() => LogLine {
            at: Some(at.to_owned()),
            text: text.to_owned(),
        },
        _ => LogLine {
            at: None,
            text: line.to_owned(),
        },
    }
}

fn instant(at: &str) -> Option<chrono::DateTime<chrono::Utc>> {
    chrono::DateTime::parse_from_rfc3339(at)
        .ok()
        .map(|t| t.with_timezone(&chrono::Utc))
}

fn follow_params(key: &LogKey, resume: Option<&str>) -> LogParams {
    let since = resume.and_then(instant);
    LogParams {
        container: key.container.clone(),
        follow: true,
        timestamps: true,
        tail_lines: if since.is_some() {
            None
        } else {
            i64::try_from(crate::live::LOG_TAIL_LINES).ok()
        },
        since_time: since,
        ..LogParams::default()
    }
}

impl Source for KubeLogSource {
    type Key = LogKey;
    type Record = LogLine;

    fn open(&self, key: LogKey, resume: Option<String>, feed: Feed<LogLine>) -> BoxFuture<()> {
        let api: Api<k8s_openapi::api::core::v1::Pod> =
            Api::namespaced(self.client.clone(), &key.namespace);
        Box::pin(async move {
            let floor = resume.as_deref().and_then(instant);
            let params = follow_params(&key, resume.as_deref());
            let reader = match api.log_stream(&key.pod, &params).await {
                Ok(r) => r,
                Err(e) => {
                    feed.degraded(e.to_string());
                    feed.until_stopped().await;
                    return;
                }
            };
            feed.live();
            let lines = reader.lines();
            futures::pin_mut!(lines);
            loop {
                match lines.next().await {
                    Some(Ok(raw)) => {
                        let line = split_stamp(&raw);
                        let at = line.at.as_deref().and_then(instant);
                        if let (Some(f), Some(t)) = (floor, at)
                            && t <= f
                        {
                            continue;
                        }
                        if let Some(stamp) = &line.at {
                            feed.resume_at(stamp.clone());
                        }
                        feed.append(line);
                    }
                    Some(Err(e)) => {
                        feed.degraded(e.to_string());
                        break;
                    }
                    None => {
                        feed.degraded(
                            "the log stream ended: the container exited or the apiserver closed the follow",
                        );
                        break;
                    }
                }
            }
            feed.until_stopped().await;
        })
    }

    fn fetch(&self, key: LogKey) -> BoxFuture<Result<Vec<LogLine>, String>> {
        let api: Api<k8s_openapi::api::core::v1::Pod> =
            Api::namespaced(self.client.clone(), &key.namespace);
        Box::pin(async move {
            let params = LogParams {
                container: key.container.clone(),
                timestamps: true,
                tail_lines: i64::try_from(crate::live::LOG_TAIL_LINES).ok(),
                ..LogParams::default()
            };
            api.logs(&key.pod, &params)
                .await
                .map(|text| text.lines().map(split_stamp).collect())
                .map_err(|e| e.to_string())
        })
    }
}

impl KubeClusterEnv {
    /// # Errors
    ///
    /// The Armazém configuration is invalid.
    pub fn estoque(
        &self,
        strategy: ListStrategy,
        cfg: &ArmazemConfig,
    ) -> Result<Estoque, ConfigError> {
        Estoque::new(
            Fonte::new(KubeSource::new(self.client.clone(), strategy)),
            Fonte::new(KubeLogSource::new(self.client.clone())),
            cfg,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(uid: &str, name: &str) -> Row {
        Row {
            uid: banken_spec::env::Uid::new(uid).expect("non-blank"),
            name: name.to_owned(),
            namespace: Some("catch".to_owned()),
            version: Some(format!("rv-{uid}")),
            cells: vec![("STATUS".to_owned(), "Running".to_owned())],
        }
    }

    struct Recycled;

    impl Source for Recycled {
        type Key = KindKey;
        type Record = Row;

        fn open(&self, _key: KindKey, _resume: Option<String>, feed: Feed<Row>) -> BoxFuture<()> {
            Box::pin(async move {
                let mut placas = Placas::default();
                placas.upsert(&feed, row("A", "api"));
                placas.upsert(&feed, row("B", "api"));
                placas.upsert(&feed, row("C", "web"));
                placas.remove(&feed, &row("C", "web"));
                feed.live();
                feed.until_stopped().await;
            })
        }

        fn fetch(&self, _key: KindKey) -> BoxFuture<Result<Vec<Row>, String>> {
            Box::pin(async { Ok(Vec::new()) })
        }
    }

    #[tokio::test]
    async fn a_recycled_name_replaces_the_dead_object_and_a_delete_removes_by_uid() {
        let armazem = izumi_armazem::Armazem::new(Fonte::new(Recycled), ArmazemConfig::default())
            .expect("config");
        let mut lease = armazem.interest(KindKey::all(ResourceKind::Pod));
        let snap = lease.settled(std::time::Duration::from_secs(5)).await;
        let uids: Vec<&str> = snap.records.iter().map(|r| r.uid.as_str()).collect();
        assert_eq!(
            uids,
            ["B"],
            "a recreate can arrive as an update with no delete; the dead uid must not linger"
        );
        assert_eq!(snap.resume.as_deref(), Some("rv-C"));
    }

    #[test]
    fn a_stamped_line_splits_and_an_unstamped_one_is_kept_whole() {
        let l = split_stamp("2026-10-15T13:00:00.123456789Z hello world");
        assert_eq!(l.at.as_deref(), Some("2026-10-15T13:00:00.123456789Z"));
        assert_eq!(l.text, "hello world");
        let l = split_stamp("no stamp here");
        assert_eq!(l.at, None);
        assert_eq!(l.text, "no stamp here");
    }

    #[test]
    fn a_resumed_follow_starts_at_the_last_stamp_and_a_fresh_one_at_the_tail() {
        let key = LogKey {
            namespace: "ns".into(),
            pod: "p".into(),
            container: None,
        };
        let fresh = follow_params(&key, None);
        assert!(fresh.follow && fresh.timestamps);
        assert_eq!(fresh.since_time, None);
        assert!(fresh.tail_lines.is_some());
        let resumed = follow_params(&key, Some("2026-10-15T13:00:00.5Z"));
        assert!(resumed.since_time.is_some());
        assert_eq!(resumed.tail_lines, None);
    }
}
