use std::cmp::Ordering;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use banken_spec::env::{ClusterEnv, Row};
use banken_spec::types::ResourceKind;
use izumi_armazem::{
    Armazem, ArmazemConfig, BoxFuture, ConfigError, Feed, KeyState, Record, Source,
};
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Scope {
    All,
    Namespace(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct KindKey {
    kind: ResourceKind,
    scope: Scope,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyError {
    UnknownKind(String),
    ClusterScoped(ResourceKind),
    BlankNamespace,
    Malformed(String),
}

impl fmt::Display for KeyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownKind(k) => write!(f, "unknown kind `{k}`"),
            Self::ClusterScoped(k) => write!(
                f,
                "`{}` is cluster-scoped, so it has no namespace to narrow to; use `{}/*`",
                k.label(),
                k.label()
            ),
            Self::BlankNamespace => f.write_str("a namespace scope needs a namespace name"),
            Self::Malformed(s) => write!(f, "`{s}` is not `<kind>/*` or `<kind>/<namespace>`"),
        }
    }
}

impl std::error::Error for KeyError {}

impl KindKey {
    /// # Errors
    ///
    /// A blank namespace, or a namespace on a cluster-scoped kind.
    pub fn new(kind: ResourceKind, scope: Scope) -> Result<Self, KeyError> {
        match &scope {
            Scope::Namespace(ns) if ns.trim().is_empty() => Err(KeyError::BlankNamespace),
            Scope::Namespace(_) if !kind.is_namespaced() => Err(KeyError::ClusterScoped(kind)),
            _ => Ok(Self { kind, scope }),
        }
    }

    #[must_use]
    pub fn all(kind: ResourceKind) -> Self {
        Self {
            kind,
            scope: Scope::All,
        }
    }

    /// # Errors
    ///
    /// A blank namespace, or a namespace on a cluster-scoped kind.
    pub fn within(kind: ResourceKind, namespace: Option<&str>) -> Result<Self, KeyError> {
        match namespace {
            None => Ok(Self::all(kind)),
            Some(ns) => Self::new(kind, Scope::Namespace(ns.to_owned())),
        }
    }

    #[must_use]
    pub fn kind(&self) -> ResourceKind {
        self.kind
    }

    #[must_use]
    pub fn scope(&self) -> &Scope {
        &self.scope
    }

    #[must_use]
    pub fn namespace(&self) -> Option<&str> {
        match &self.scope {
            Scope::All => None,
            Scope::Namespace(ns) => Some(ns),
        }
    }
}

impl fmt::Display for KindKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.kind.label())?;
        f.write_str("/")?;
        match &self.scope {
            Scope::All => f.write_str("*"),
            Scope::Namespace(ns) => f.write_str(ns),
        }
    }
}

impl FromStr for KindKey {
    type Err = KeyError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (label, scope) = s
            .split_once('/')
            .ok_or_else(|| KeyError::Malformed(s.to_owned()))?;
        let kind = ResourceKind::ALL
            .iter()
            .copied()
            .find(|k| k.label() == label)
            .ok_or_else(|| KeyError::UnknownKind(label.to_owned()))?;
        match scope {
            "*" => Ok(Self::all(kind)),
            "" => Err(KeyError::BlankNamespace),
            ns if ns.contains('/') => Err(KeyError::Malformed(s.to_owned())),
            ns => Self::new(kind, Scope::Namespace(ns.to_owned())),
        }
    }
}

impl PartialOrd for KindKey {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for KindKey {
    fn cmp(&self, other: &Self) -> Ordering {
        self.kind
            .label()
            .cmp(other.kind.label())
            .then_with(|| self.scope.cmp(&other.scope))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct LogKey {
    pub namespace: String,
    pub pod: String,
    pub container: Option<String>,
}

impl fmt::Display for LogKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "log/{}/{}", self.namespace, self.pod)?;
        if let Some(c) = &self.container {
            write!(f, "/{c}")?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LogLine {
    pub at: Option<String>,
    pub text: String,
}

impl Record for LogLine {
    fn id(&self) -> Option<&str> {
        None
    }

    fn fingerprint(&self) -> u64 {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        self.at.hash(&mut h);
        self.text.hash(&mut h);
        h.finish()
    }
}

pub struct Fonte<K, R>(Arc<dyn Source<Key = K, Record = R>>);

impl<K, R> Clone for Fonte<K, R> {
    fn clone(&self) -> Self {
        Self(Arc::clone(&self.0))
    }
}

impl<K, R> Fonte<K, R>
where
    K: Clone + Ord + fmt::Display + Send + Sync + 'static,
    R: Record,
{
    pub fn new(source: impl Source<Key = K, Record = R>) -> Self {
        Self(Arc::new(source))
    }
}

impl<K, R> Source for Fonte<K, R>
where
    K: Clone + Ord + fmt::Display + Send + Sync + 'static,
    R: Record,
{
    type Key = K;
    type Record = R;

    fn open(&self, key: K, resume: Option<String>, feed: Feed<R>) -> BoxFuture<()> {
        self.0.open(key, resume, feed)
    }

    fn fetch(&self, key: K) -> BoxFuture<Result<Vec<R>, String>> {
        self.0.fetch(key)
    }
}

pub type KindFonte = Fonte<KindKey, Row>;
pub type LogFonte = Fonte<LogKey, LogLine>;
pub type KindArmazem = Armazem<KindFonte>;
pub type LogArmazem = Armazem<LogFonte>;
pub type SharedEnv = Arc<dyn ClusterEnv + Send + Sync>;

pub struct EnvSource {
    env: SharedEnv,
    every: Option<Duration>,
}

impl EnvSource {
    #[must_use]
    pub fn new(env: SharedEnv, every: Option<Duration>) -> Self {
        Self { env, every }
    }
}

async fn read_rows(env: SharedEnv, key: KindKey) -> Result<Vec<Row>, String> {
    tokio::task::spawn_blocking(move || {
        env.list_resources(key.kind(), key.namespace())
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

impl Source for EnvSource {
    type Key = KindKey;
    type Record = Row;

    fn open(&self, key: KindKey, _resume: Option<String>, feed: Feed<Row>) -> BoxFuture<()> {
        let env = Arc::clone(&self.env);
        let every = self.every;
        Box::pin(async move {
            loop {
                match read_rows(Arc::clone(&env), key.clone()).await {
                    Ok(rows) => {
                        feed.reset(rows);
                        feed.live();
                    }
                    Err(cause) => feed.degraded(cause),
                }
                let Some(every) = every else { break };
                tokio::select! {
                    () = tokio::time::sleep(every) => {}
                    () = feed.until_stopped() => return,
                }
            }
            feed.until_stopped().await;
        })
    }

    fn fetch(&self, key: KindKey) -> BoxFuture<Result<Vec<Row>, String>> {
        Box::pin(read_rows(Arc::clone(&self.env), key))
    }
}

pub struct EnvLogSource {
    env: SharedEnv,
}

impl EnvLogSource {
    #[must_use]
    pub fn new(env: SharedEnv) -> Self {
        Self { env }
    }
}

async fn read_log(env: SharedEnv, key: LogKey) -> Result<Vec<LogLine>, String> {
    tokio::task::spawn_blocking(move || {
        env.logs(&key.pod, &key.namespace, false)
            .map(|s| {
                s.lines
                    .into_iter()
                    .map(|text| LogLine { at: None, text })
                    .collect()
            })
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

impl Source for EnvLogSource {
    type Key = LogKey;
    type Record = LogLine;

    fn open(&self, key: LogKey, _resume: Option<String>, feed: Feed<LogLine>) -> BoxFuture<()> {
        let env = Arc::clone(&self.env);
        Box::pin(async move {
            match read_log(env, key).await {
                Ok(lines) => {
                    feed.reset(lines);
                    feed.live();
                }
                Err(cause) => feed.degraded(cause),
            }
            feed.until_stopped().await;
        })
    }

    fn fetch(&self, key: LogKey) -> BoxFuture<Result<Vec<LogLine>, String>> {
        Box::pin(read_log(Arc::clone(&self.env), key))
    }
}

#[derive(Clone)]
pub struct Estoque {
    pub rows: KindArmazem,
    pub logs: LogArmazem,
}

impl Estoque {
    /// # Errors
    ///
    /// The Armazém configuration is invalid.
    pub fn new(rows: KindFonte, logs: LogFonte, cfg: &ArmazemConfig) -> Result<Self, ConfigError> {
        Ok(Self {
            rows: Armazem::new(rows, cfg.clone())?,
            logs: Armazem::new(logs, cfg.clone())?,
        })
    }

    /// # Errors
    ///
    /// The Armazém configuration is invalid.
    pub fn over_env(
        env: SharedEnv,
        every: Option<Duration>,
        cfg: &ArmazemConfig,
    ) -> Result<Self, ConfigError> {
        Self::new(
            Fonte::new(EnvSource::new(Arc::clone(&env), every)),
            Fonte::new(EnvLogSource::new(env)),
            cfg,
        )
    }

    #[must_use]
    pub fn spawn_zeladores(&self) -> [tokio::task::JoinHandle<()>; 2] {
        [self.rows.spawn_zelador(), self.logs.spawn_zelador()]
    }

    #[must_use]
    pub fn inventory(&self) -> Vec<KeyState> {
        let mut all = self.rows.inventory();
        all.extend(self.logs.inventory());
        all
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_key_round_trips_through_its_display() {
        for kind in ResourceKind::ALL {
            let all = KindKey::all(*kind);
            assert_eq!(all.to_string().parse::<KindKey>().unwrap(), all);
            if kind.is_namespaced() {
                let ns = KindKey::new(*kind, Scope::Namespace("pangea-system".into())).unwrap();
                assert_eq!(ns.to_string().parse::<KindKey>().unwrap(), ns);
            }
        }
        assert_eq!(KindKey::all(ResourceKind::Pod).to_string(), "pod/*");
        assert_eq!(
            KindKey::within(ResourceKind::Pod, Some("pangea-system"))
                .unwrap()
                .to_string(),
            "pod/pangea-system"
        );
    }

    #[test]
    fn a_cluster_scoped_kind_takes_only_all() {
        assert_eq!(
            KindKey::new(ResourceKind::Node, Scope::Namespace("x".into())),
            Err(KeyError::ClusterScoped(ResourceKind::Node))
        );
        assert_eq!(
            "namespace/kube-system".parse::<KindKey>(),
            Err(KeyError::ClusterScoped(ResourceKind::Namespace))
        );
        assert!(KindKey::within(ResourceKind::Node, None).is_ok());
    }

    #[test]
    fn malformed_keys_are_refused_by_reason() {
        assert_eq!(
            "pod".parse::<KindKey>(),
            Err(KeyError::Malformed("pod".into()))
        );
        assert_eq!("pod/".parse::<KindKey>(), Err(KeyError::BlankNamespace));
        assert_eq!(
            "pods/*".parse::<KindKey>(),
            Err(KeyError::UnknownKind("pods".into()))
        );
        assert_eq!(
            "pod/a/b".parse::<KindKey>(),
            Err(KeyError::Malformed("pod/a/b".into()))
        );
    }
}
