#![cfg(feature = "live")]

use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;

use banken::absorb::ListStrategy;
use banken::armazem::{Fonte, KindArmazem, KindKey};
use banken::kube_source::KubeSource;
use banken_spec::types::ResourceKind;
use bytes::Bytes;
use futures::StreamExt;
use http_body::Frame;
use http_body_util::{StreamBody, combinators::UnsyncBoxBody};
use izumi_armazem::{Action, Armazem, ArmazemConfig, Change, Phase, PolicyEngine, Seq, Since};
use parking_lot::Mutex;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Resume {
    Answers,
    Expired,
    NeverOpens,
}

#[derive(Clone)]
struct Fake {
    seen: Arc<Mutex<Vec<String>>>,
    resume: Resume,
}

fn pod(name: &str, uid: &str, rv: &str) -> serde_json::Value {
    serde_json::json!({
        "apiVersion": "v1",
        "kind": "Pod",
        "metadata": {
            "name": name,
            "namespace": "default",
            "uid": uid,
            "resourceVersion": rv,
        },
        "status": { "phase": "Running" },
    })
}

type Body = UnsyncBoxBody<Bytes, Infallible>;

fn whole(v: &serde_json::Value) -> Body {
    let bytes = Bytes::from(serde_json::to_vec(v).expect("json"));
    UnsyncBoxBody::new(StreamBody::new(futures::stream::iter([
        Ok::<_, Infallible>(Frame::data(bytes)),
    ])))
}

fn events(lines: Vec<serde_json::Value>) -> Body {
    let frames: Vec<Result<Frame<Bytes>, Infallible>> = lines
        .into_iter()
        .map(|v| {
            let mut b = serde_json::to_vec(&v).expect("json");
            b.push(b'\n');
            Ok(Frame::data(Bytes::from(b)))
        })
        .collect();
    UnsyncBoxBody::new(StreamBody::new(
        futures::stream::iter(frames).chain(futures::stream::pending()),
    ))
}

fn query(uri: &http::Uri, key: &str) -> Option<String> {
    uri.query()?.split('&').find_map(|kv| {
        let (k, v) = kv.split_once('=')?;
        (k == key).then(|| v.to_owned())
    })
}

impl Fake {
    fn answer(&self, uri: &http::Uri) -> http::Response<Body> {
        self.seen.lock().push(uri.to_string());
        let watch = query(uri, "watch").as_deref() == Some("true");
        let rv = query(uri, "resourceVersion");
        let body = match (watch, rv.as_deref()) {
            (false, _) => whole(&serde_json::json!({
                "apiVersion": "v1",
                "kind": "PodList",
                "metadata": { "resourceVersion": "200" },
                "items": [pod("b", "u-b", "150")],
            })),
            (true, Some("150")) if self.resume == Resume::Answers => events(vec![
                serde_json::json!({ "type": "MODIFIED", "object": pod("a", "u-a", "151") }),
            ]),
            (true, Some("150")) => events(vec![serde_json::json!({
                "type": "ERROR",
                "object": {
                    "kind": "Status",
                    "apiVersion": "v1",
                    "status": "Failure",
                    "message": "too old resource version: 150 (190)",
                    "reason": "Expired",
                    "code": 410,
                },
            })]),
            (true, _) => events(Vec::new()),
        };
        http::Response::builder()
            .status(200)
            .header("content-type", "application/json")
            .body(body)
            .expect("response")
    }

    fn lists(&self) -> usize {
        self.seen
            .lock()
            .iter()
            .filter(|u| !u.contains("watch=true"))
            .count()
    }

    fn resumed_from(&self, rv: &str) -> bool {
        let want = format!("resourceVersion={rv}");
        self.seen
            .lock()
            .iter()
            .any(|u| u.contains("watch=true") && u.contains(&want))
    }
}

fn store(resume: Resume) -> (KindArmazem, Fake) {
    let fake = Fake {
        seen: Arc::new(Mutex::new(Vec::new())),
        resume,
    };
    let served = fake.clone();
    let service = tower::service_fn(move |req: http::Request<kube::client::Body>| {
        let fake = served.clone();
        async move {
            let hangs = fake.resume == Resume::NeverOpens
                && query(req.uri(), "watch").as_deref() == Some("true")
                && query(req.uri(), "resourceVersion").as_deref() == Some("150");
            if hangs {
                fake.seen.lock().push(req.uri().to_string());
                std::future::pending::<()>().await;
            }
            Ok::<_, Infallible>(fake.answer(req.uri()))
        }
    });
    let client = kube::Client::new(service, "default");
    let cfg = ArmazemConfig {
        linger_ms: 0,
        policy: PolicyEngine::Native,
        ..ArmazemConfig::default()
    };
    let armazem = Armazem::new(
        Fonte::new(KubeSource::new(client, ListStrategy::ListWatch)),
        cfg,
    )
    .expect("config");
    (armazem, fake)
}

async fn park_after_first_read(armazem: &KindArmazem) -> Seq {
    let mut first = armazem.interest(KindKey::all(ResourceKind::Pod));
    let snap = first.settled(Duration::from_secs(5)).await;
    assert_eq!(snap.phase, Phase::Live);
    assert_eq!(snap.records.len(), 1);
    assert_eq!(snap.resume.as_deref(), Some("150"));
    drop(first);
    assert_eq!(
        armazem.tick().expect("native policy"),
        [Action::Park("pod/*".into())]
    );
    assert!(matches!(
        armazem.inventory()[0].phase,
        Phase::Dormant { resume: Some(ref rv) } if rv == "150"
    ));
    snap.seq
}

fn journal(changes: &[(Seq, Change<banken_spec::env::Row>)]) -> Vec<String> {
    changes
        .iter()
        .map(|(_, c)| match c {
            Change::Reset(_) => "reset".to_owned(),
            Change::Upsert(r) => format!("upsert {}", r.name),
            Change::Remove(id) => format!("remove {id}"),
            Change::Append(_) => "append".to_owned(),
            Change::Phase(Phase::Degraded { cause }) => format!("degraded: {cause}"),
            Change::Phase(p) => p.label().to_owned(),
        })
        .collect()
}

async fn reopened(armazem: &KindArmazem, after: Seq, until: &str) -> Vec<String> {
    let back = armazem.interest(KindKey::all(ResourceKind::Pod));
    for _ in 0..200 {
        let Since::Changes { changes, .. } = back.since(after) else {
            panic!("a cursor taken before the park must still be inside the journal");
        };
        let seen = journal(&changes);
        if seen.iter().any(|c| c == until) {
            return seen;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let Since::Changes { changes, .. } = back.since(after) else {
        panic!("gap");
    };
    journal(&changes)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_reopened_key_resumes_from_its_resource_version_without_a_relist() {
    let (armazem, fake) = store(Resume::Answers);
    let cursor = park_after_first_read(&armazem).await;
    assert_eq!(fake.lists(), 1);

    let seen = reopened(&armazem, cursor, "upsert a").await;
    assert_eq!(
        seen,
        ["dormant", "absorbing", "live", "upsert a"],
        "the resume path: no reset, the dormant row kept, the missed change replayed"
    );
    assert!(fake.resumed_from("150"), "{:?}", fake.seen.lock());
    assert_eq!(
        fake.lists(),
        1,
        "a resume must not relist: {:?}",
        fake.seen.lock()
    );
    let state = &armazem.inventory()[0];
    assert_eq!(state.records, 2);
    assert_eq!(state.phase, Phase::Live);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_expired_resource_version_falls_back_to_a_relist_and_says_so() {
    let (armazem, fake) = store(Resume::Expired);
    let cursor = park_after_first_read(&armazem).await;

    let seen = reopened(&armazem, cursor, "live").await;
    assert_eq!(seen[..2], ["dormant", "absorbing"]);
    assert!(
        seen[2].starts_with("degraded: ")
            && seen[2].contains("410 Gone")
            && seen[2].contains("relisting"),
        "the journal must say the resume expired and a relist ran: {seen:?}"
    );
    assert!(seen.contains(&"live".to_owned()), "{seen:?}");
    assert!(fake.resumed_from("150"));
    assert_eq!(
        fake.lists(),
        2,
        "410 must fall back to a fresh list: {:?}",
        fake.seen.lock()
    );
    assert_eq!(armazem.inventory()[0].phase, Phase::Live);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_resume_watch_that_never_opens_falls_back_to_a_relist() {
    let (armazem, fake) = store(Resume::NeverOpens);
    let cursor = park_after_first_read(&armazem).await;
    let back = armazem.interest(KindKey::all(ResourceKind::Pod));
    let mut seen = Vec::new();
    for _ in 0..100 {
        let Since::Changes { changes, .. } = back.since(cursor) else {
            panic!("gap");
        };
        seen = journal(&changes);
        if seen.last().map(String::as_str) == Some("live") {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert_eq!(seen[..2], ["dormant", "absorbing"]);
    assert!(
        seen[2].contains("did not open") && seen[2].contains("relisting"),
        "a resume that never opens must not hold the key in absorbing: {seen:?}"
    );
    assert_eq!(seen.last().map(String::as_str), Some("live"), "{seen:?}");
    assert_eq!(fake.lists(), 2);
}
