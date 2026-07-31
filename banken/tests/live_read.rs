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
        table.unresolved_fields().is_empty(),
        "every authored column must resolve against the LIVE reader's rows; \
         unresolved: {:?}",
        table.unresolved_fields(),
    );

    // ── the render ──
    let app = BankenApp::try_new(env, OperatorId("drzzln".into()), {
        let mut label = String::from("source: LIVE ");
        label.push_str(&context);
        label
    })
    .expect("the app builds over the live env")
    .with_cluster(context.clone());

    assert_eq!(
        app.table().rows().len(),
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
