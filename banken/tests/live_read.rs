//! The LIVE cluster read, against a real apiserver — `pending-banken:
//! live-read`'s gate.
//!
//! **`#[ignore]` by default, on purpose.** This test opens a real TLS
//! connection to a real Kubernetes apiserver (and, on EKS, shells out through
//! whatever exec-credential plugin the kubeconfig names). A plain `cargo test`
//! must never do that. Run it deliberately:
//!
//! ```text
//! BANKEN_LIVE_CONTEXT=camelot-eks \
//!   cargo test -p banken --features live --test live_read -- --ignored --nocapture
//! ```
//!
//! **It refuses to guess a context**, exactly as [`banken::cli`] does: the
//! context comes from `BANKEN_LIVE_CONTEXT` and there is no default. A test
//! that silently fell back to the kubeconfig's `current-context` would be
//! asserting "banken can read *a* cluster", which is not the claim — the claim
//! is "banken reads the cluster it was told to read". On this machine the
//! current-context is an entirely different estate, so a defaulting test would
//! have passed while proving the wrong thing.
//!
//! # What this proves that the fixture cannot
//!
//! `pod_to_row`'s projection is already unit-tested against synthetic
//! `k8s_openapi::Pod` values, and the render is already golden-frame tested
//! against fixture rows. Neither touches:
//!
//! - kubeconfig resolution for a NAMED context,
//! - the exec-credential auth path (AWS SSO on EKS),
//! - the TLS + HTTP round trip to a real apiserver,
//! - `list_resources` returning rows the apiserver actually produced,
//! - and those rows surviving `PodTable::from_view`'s authored-column
//!   resolution into a rendered grid.
//!
//! This test is the whole chain, end to end, with an assertion at each joint.
//!
//! # It is READ-ONLY and it is structurally incapable of being otherwise
//!
//! `ClusterEnv` has no unwitnessed-mutate method, and this test calls
//! `list_resources` and nothing else. There is no cleanup step because there
//! is nothing to clean up.

#![cfg(feature = "live")]

use banken::app::BankenApp;
use banken::live::KubeClusterEnv;
use banken::session::UnwiredSessionEnv;
use banken::table::PodTable;
use banken_spec::env::ClusterEnv;
use banken_spec::types::{OperatorId, ResourceKind};
use egaku_term::TestBackend;

/// The context to read, from the environment. No default — see the module
/// docs.
fn required_context() -> String {
    std::env::var("BANKEN_LIVE_CONTEXT").expect(
        "set BANKEN_LIVE_CONTEXT to the kubeconfig context to read. There is \
         deliberately no default: falling back to the kubeconfig's \
         current-context would assert that banken can read *a* cluster, when \
         the claim under test is that it reads the cluster it was told to.",
    )
}

/// **THE M0 GATE.** A real apiserver read becomes real rendered pod rows.
///
/// Every assertion is on structure, not on a substring of a blob: the row
/// count, the authored columns resolving against the rows the apiserver
/// returned, and the selected pod's name appearing on a data line of the
/// rendered grid.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "reads a real cluster; run with --ignored and BANKEN_LIVE_CONTEXT set"]
async fn a_named_context_renders_real_pod_rows() {
    let context = required_context();

    let env = KubeClusterEnv::connect_with_context(&context)
        .await
        .expect("connecting to the named context (VPN/SSO session live?)");

    // The apiserver URL that was actually dialled, captured before the env is
    // moved into the app. A context NAME is a label and is not unique across a
    // KUBECONFIG merge list; this is the value an operator can CHECK.
    let env_server = env.server().map(str::to_owned);
    assert!(
        env_server.is_some(),
        "a named-context connection must know the server it dialled — that is \
         the whole point of resolving before connecting",
    );
    let strategy = banken::absorb::ListStrategy::default();

    // The env reports the context it was CONSTRUCTED with — this is the join
    // that makes a `(defbancada)`'s `(:context cluster)` trustworthy.
    assert_eq!(
        env.context_name().as_deref(),
        Some(context.as_str()),
        "the env must report the context it connected with, not a re-read one",
    );

    // ── the read ──
    let rows = env
        .list_resources(ResourceKind::Pod, None)
        .expect("a cluster-wide pod list");
    assert!(
        !rows.is_empty(),
        "the cluster returned zero pods — that is a real answer, but it is not \
         evidence the read works. Point BANKEN_LIVE_CONTEXT at a cluster with \
         workloads.",
    );

    // A row the apiserver produced carries a name and the four authored
    // cells. This is `pod_to_row`'s output on REAL objects rather than
    // synthetic ones.
    for row in &rows {
        assert!(!row.name.is_empty(), "every pod row carries a name");
        for field in ["ready", "phase", "restarts", "age"] {
            assert!(
                row.cells.iter().any(|(k, _)| k == field),
                "row {} carries the authored `{field}` cell, got {:?}",
                row.name,
                row.cells.iter().map(|(k, _)| k).collect::<Vec<_>>(),
            );
        }
    }

    // ── the authored columns resolve against the LIVE rows ──
    // The fixture reader and the live reader key their cells independently;
    // this is the assertion that the live one agrees with `specs/views.lisp`.
    let catalog = banken_spec::load_catalog().expect("the authored vocabulary loads");
    let table = PodTable::from_view(&catalog, banken::app::VIEW_NAME, rows.clone())
        .expect("the authored :pods columns resolve against live rows");
    assert!(
        table.view().unresolved_fields().is_empty(),
        "every authored column must resolve against the LIVE reader's rows; \
         unresolved: {:?}",
        table.view().unresolved_fields(),
    );

    // ── the render ──
    let app = BankenApp::try_new(
        env,
        UnwiredSessionEnv::new(),
        OperatorId::new("drzzln").expect("a literal witness is non-blank"),
        {
            // The SAME function main.rs calls. Building a label here by hand
            // is what previously made this assertion structurally blind to
            // what the binary actually renders.
            banken::absorb::live_source_label(&context, env_server.as_deref(), strategy)
        },
    )
    .expect("the app builds over the live env")
    .with_cluster(context.clone());

    assert_eq!(
        app.table().view().rows().len(),
        rows.len(),
        "the app's own construction read returned the same row count",
    );

    let mut backend = TestBackend::new(120, 24);
    backend.draw(|buf| app.render(buf));
    let lines = backend.to_lines();

    // The literal grid, so a run of this test IS the receipt.
    for line in &lines {
        println!("{line}");
    }

    // Title, header, and the selected pod on a data line.
    assert!(lines[0].contains(":pods"), "the title bar renders");
    let header = &lines[2];
    for column in ["NAME", "READY", "STATUS", "RESTARTS", "AGE"] {
        assert!(
            header.contains(column),
            "header carries {column}, got {header:?}",
        );
    }
    let selected = app
        .table()
        .view()
        .selected_row()
        .expect("a non-empty live table has a selection")
        .name
        .clone();
    assert!(
        lines.iter().skip(4).any(|l| l.contains(&selected)),
        "the selected pod `{selected}` renders on a data line",
    );

    // The status line states the source AND the context — the surface that
    // tells the operator which estate they are looking at.
    let status = lines.last().expect("a status line");
    assert!(
        status.contains(&context),
        "the status line names the cluster being read, got {status:?}",
    );
}

/// The WATCH plane against a real apiserver — the M0 gate.
///
/// `#[ignore]` for the same reason as every other test in this file: it opens a
/// real connection through a real exec-credential plugin.
///
/// ```text
/// BANKEN_LIVE_CONTEXT=camelot-eks \
///   cargo test -p banken --features live --test live_read -- --ignored --nocapture
/// ```
///
/// # What this proves that `list_resources` cannot
///
/// That banken's rows arrive by **streamed delta** rather than by re-reading the
/// world. The distinction is not stylistic: measured against `camelot-eks` on
/// 2026-08-08, the poll moved 3,580,862 B *per second* (96 GiB per 8-hour day)
/// while a 30-second watch over the same cluster moved **0 bytes**.
///
/// The assertions are staged so a failure says which joint broke:
///   1. the stream reaches `Synced` at all — the initial streaming list
///      completed and `InitDone` fired;
///   2. it produced a non-empty replica — the fold actually applied `InitApply`;
///   3. the phase is not `Degraded` — no backoff/relist loop is hiding behind
///      rows that merely *look* fine.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "opens a real apiserver connection; set BANKEN_LIVE_CONTEXT"]
async fn the_watch_plane_absorbs_from_a_real_cluster() {
    let ctx = std::env::var("BANKEN_LIVE_CONTEXT")
        .expect("set BANKEN_LIVE_CONTEXT — this test refuses to guess a context");

    let env = banken::live::KubeClusterEnv::connect_with_context(&ctx)
        .await
        .expect("connect to the named context");

    let (despensa, publisher) = banken::absorb::channel();
    // Named explicitly rather than defaulted: this test asserts the STREAMING
    // path specifically (generation 1 for the whole initial set), and a silent
    // strategy change would make it assert something else while staying green.
    // The strategy is an INPUT, so this test is a differential harness across
    // read paths rather than an assertion about one. Measured 2026-08-08: the
    // same binary reaches `Synced` against camelot-eks on either strategy, and
    // against a local engenho ONLY on `list-watch` — because engenho does not
    // negotiate `sendInitialEvents`, so `streaming` degrades to a live tail
    // whose terminating bookmark never arrives and `InitDone` never fires.
    let strategy = std::env::var("BANKEN_LIST_STRATEGY")
        .ok()
        .and_then(|s| banken::absorb::ListStrategy::parse(&s))
        .unwrap_or(banken::absorb::ListStrategy::Streaming);
    println!("list strategy: {}", strategy.label());
    let _task = env.spawn_pod_absorber(publisher, strategy);

    // Wait for the initial streaming list to complete. Generous, because a cold
    // exec-credential plugin invocation is ~0.7 s on its own.
    let mut synced = false;
    for _ in 0..200 {
        if matches!(
            despensa.snapshot().phase(),
            banken::absorb::SyncPhase::Synced
        ) {
            synced = true;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }

    let snap = despensa.snapshot();
    assert!(
        synced,
        "the streaming list must complete and reach Synced; phase was {:?}",
        snap.phase()
    );
    assert!(
        !snap.rows().is_empty(),
        "a real cluster must yield rows; the fold applied nothing"
    );
    assert!(
        !matches!(snap.phase(), banken::absorb::SyncPhase::Degraded { .. }),
        "the watch must be healthy, not backing off behind plausible rows: {:?}",
        snap.phase()
    );

    println!(
        "absorbed {} pods from `{ctx}` by watch, generation {}",
        snap.rows().len(),
        snap.generation()
    );
}

/// **The discovery gate.** Two aggregated requests build a real RESTMapper,
/// and `po` resolves to Pod against an actual apiserver.
///
/// This is the thing kube-rs structurally cannot do (`ApiResource` discards
/// shortNames), so a green unit test over a fixture document proves the
/// parser and nothing about the wire. Only a real server proves the `Accept`
/// negotiation — which is exactly the class of gap that let `--features live`
/// compile clean for weeks while the first live read panicked.
#[tokio::test]
#[ignore = "reads a real cluster; run with --ignored and BANKEN_LIVE_CONTEXT set"]
async fn discovery_resolves_shortnames_against_a_real_cluster() {
    let ctx = required_context();
    let env = banken::live::KubeClusterEnv::connect_with_context(&ctx)
        .await
        .expect("connect to the named context");

    let mapper = env
        .discover()
        .await
        .expect("the server must serve aggregated discovery (no silent fallback by design)");

    let n = mapper.entries().len();
    assert!(n > 0, "a real cluster describes at least one resource");
    println!("discovered {n} resources from `{ctx}` in TWO requests (/api + /apis)");

    // The payoff: a short name resolves, carrying the SERVER's plural.
    let pods = mapper
        .resolve("po")
        .expect("`po` must resolve on any cluster");
    assert_eq!(pods.kind(), "Pod");
    assert_eq!(pods.plural(), "pods");
    assert!(pods.is_namespaced());
    assert!(pods.is_listable(), "pods must be list+watch-able");

    // And every entry carries a server-stated plural — never a guess.
    for e in mapper.entries() {
        assert!(
            !e.plural().is_empty(),
            "{} carries no plural; a guessed one is what 404s silently",
            e.kind()
        );
    }
}
