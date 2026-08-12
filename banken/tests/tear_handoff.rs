//! The LIVE `(defbancada)` handoff, against a running `tear-daemon`.
//!
//! **`#[ignore]` by default, on purpose.** This test opens a real session on
//! the operator's tear daemon and types into real PTYs; a plain `cargo test`
//! must never do that. Run it deliberately:
//!
//! ```text
//! cargo test -p banken --features tear --test tear_handoff -- --ignored --nocapture
//! ```
//!
//! It cleans up after itself (`kill_opened_session`) whether it passes or
//! fails, so a red run does not leave a stray session behind.
//!
//! What it proves that the mock cannot: that
//! `banken_spec::bancada::SessionEnv`'s four methods really do land on
//! `tear_types::MultiplexerControl`, that the session/split sequence is
//! accepted by the daemon, that the resulting session has the pane count the
//! authored recipe declares, and — since 2026-07-31 — that the read pane's
//! **argv reached the daemon** rather than being dropped.
//!
//! **This test has NOT been re-run since the spawn conversion.** It needs a
//! `tear-daemon` built from tear `5974375` or later; the daemon on this
//! machine at the time of writing predates it and would silently ignore
//! `args`, which is precisely what the `!root.args.is_empty()` assertion is
//! there to catch. `pending-banken: tear-argv-spawn-live`.

#![cfg(feature = "tear")]

use banken::tear_session::{BANCADA_SOURCE, TearSessionEnv};
use banken_spec::{
    bancada::{BancadaContext, open, plan},
    interp::Selection,
    load_bancadas,
    types::ResourceKind,
};
use tear_types::control::MultiplexerControl;

fn ctx() -> BancadaContext {
    BancadaContext {
        cluster: "alpha-eks".into(),
        selection: Selection {
            kind: ResourceKind::Pod,
            name: "banken-bancada-selftest".into(),
            namespace: Some("catch".into()),
            current: Vec::new(),
        },
        container: Some("catch".into()),
    }
}

/// The whole bridge, live: an authored recipe + a selection become a real
/// three-pane tear session on the daemon.
///
/// The spawned commands are `kubectl` invocations against a cluster that may
/// well not be reachable — that is fine and deliberate. What is under test is
/// the HANDOFF (session created, panes split, argv delivered), not kubectl's
/// exit code. A `kubectl` that fails in the pane proves the pane exists.
#[test]
#[ignore = "opens a real session on the operator's tear daemon; run with --ignored"]
fn the_triage_recipe_opens_a_real_three_pane_tear_session() {
    let gs = load_bancadas().expect("specs/bancadas.lisp compiles");
    let triage = gs.iter().find(|g| g.name == "pod-triage").expect("triage");
    let p = plan(triage, &ctx()).expect("plans");
    assert_eq!(p.panes().len(), 3);

    let env = TearSessionEnv::connect_default()
        .expect("a tear-daemon must be running (`tear` / launchd) for this test");

    let result = open(&p, &env);

    // Inspect BEFORE cleaning up, but clean up even if the assertions below
    // would panic — a red run must not leave a session behind.
    let client = tear_client::Client::connect_default().expect("a second client connects");
    let inspected = env
        .session_id()
        .map(|id| client.get_session(id).expect("the session exists"));

    let cleanup = env.kill_opened_session();

    let refs = result.expect("the plan must open against a live daemon");
    assert_eq!(refs.len(), 3, "one pane handle per planned pane");

    let session = inspected.expect("the env recorded the session it opened");
    assert_eq!(
        session.panes.len(),
        3,
        "the daemon really holds three panes — the splits landed",
    );
    assert!(
        session.name.starts_with("triage-alpha-eks-"),
        "the session carries the planned name: {}",
        session.name,
    );
    assert_eq!(
        session.source,
        tear_types::session::SessionSource::Named(BANCADA_SOURCE.to_owned()),
        "banken-opened sessions are tagged so `tear list --source` can triage them",
    );

    // **THE claim under test, and it moved on 2026-07-31.** A read pane is now
    // SPAWNED as its own argv rather than typed into a shell, so the evidence
    // is the daemon's own registry record of what it spawned — `TearPane.shell`
    // + `.args` — not a poll of the rendered grid. That is a strictly better
    // witness: it is durable (the pane can have exited), it is the exact vector
    // handed to `execvp`, and it cannot be satisfied by an echo.
    //
    // It is also the honest gate on a STALE DAEMON. tear ships no protocol
    // version and does not negotiate, so a daemon older than tear `5974375`
    // accepts the new frame and silently ignores `args` — spawning a bare
    // `kubectl`. That failure is invisible on a screen poll and unmissable
    // here: `args` comes back empty.
    let root = session
        .panes
        .values()
        .find(|pane| pane.id == tear_types::id::PaneId(refs[0].0))
        .expect("the daemon holds the root pane this plan opened");
    assert_eq!(
        root.shell, "kubectl",
        "the read pane IS the command — argv[0] is the program the daemon \
         spawned, with no shell in between",
    );
    assert!(
        !root.args.is_empty(),
        "the daemon recorded NO spawn args. Either the argv was dropped, or \
         this daemon predates tear 5974375 and is silently ignoring them — \
         restart it (there is no protocol version to reject on)",
    );
    assert!(
        root.args.contains(&"alpha-eks".to_owned()),
        "the spawned argv targets the cluster banken was reading: {:?}",
        root.args,
    );
    assert!(
        root.args.contains(&"banken-bancada-selftest".to_owned()),
        "and the pod the selection named: {:?}",
        root.args,
    );

    cleanup.expect("the self-test session is killed");
}
