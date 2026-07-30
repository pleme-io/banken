//! The LIVE `(defguarita)` handoff, against a running `tear-daemon`.
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
//! `banken_spec::guarita::SessionEnv`'s five methods really do land on
//! `tear_types::MultiplexerControl`, that the session/split/stage sequence is
//! accepted by the daemon, and that the resulting session has the pane count
//! the authored recipe declares.

#![cfg(feature = "tear")]

use banken::tear_session::{GUARITA_SOURCE, TearSessionEnv};
use banken_spec::{
    guarita::{GuaritaContext, open, plan},
    interp::Selection,
    load_guaritas,
    types::ResourceKind,
};
use tear_types::control::MultiplexerControl;

fn ctx() -> GuaritaContext {
    GuaritaContext {
        cluster: "camelot-eks".into(),
        selection: Selection {
            kind: ResourceKind::Pod,
            name: "banken-guarita-selftest".into(),
            namespace: Some("catch".into()),
            current: Vec::new(),
        },
        container: Some("catch".into()),
    }
}

/// The whole bridge, live: an authored recipe + a selection become a real
/// three-pane tear session on the daemon.
///
/// The staged commands are `kubectl` invocations against a cluster that may
/// well not be reachable — that is fine and deliberate. What is under test is
/// the HANDOFF (session created, panes split, keys delivered), not kubectl's
/// exit code. A `kubectl` that fails in the pane proves the pane exists.
#[test]
#[ignore = "opens a real session on the operator's tear daemon; run with --ignored"]
fn the_triage_recipe_opens_a_real_three_pane_tear_session() {
    let gs = load_guaritas().expect("specs/guaritas.lisp compiles");
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

    // Did the staged argv actually REACH THE SCREEN? "the daemon accepted
    // send_keys" and "the operator sees a pre-warmed command" are different
    // claims, and only the second one is the feature. Poll the pane's
    // rendered grid — the shell needs a moment to echo, so retry rather than
    // sleep-and-hope.
    let echoed = result.as_ref().ok().and_then(|refs| {
        let pane = tear_types::id::PaneId(refs[0].0);
        for _ in 0..40 {
            if let Ok(snap) = client.pane_snapshot(pane) {
                let text = snap.to_text();
                if text.contains("kubectl") {
                    return Some(text);
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        None
    });

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
        session.name.starts_with("triage-camelot-eks-"),
        "the session carries the planned name: {}",
        session.name,
    );
    assert_eq!(
        session.source,
        tear_types::session::SessionSource::Named(GUARITA_SOURCE.to_owned()),
        "banken-opened sessions are tagged so `tear list --source` can triage them",
    );

    // **THE claim under test.** The pre-warmed command is on the operator's
    // screen, carrying the cluster and the pod the selection named.
    let text = echoed.expect(
        "the staged argv must appear on the pane's rendered grid — without \
         this the handoff only proves the daemon accepted bytes, not that the \
         operator lands on a pre-warmed command",
    );
    assert!(
        text.contains("camelot-eks"),
        "the pre-warmed line targets the cluster banken was reading",
    );
    assert!(
        text.contains("banken-guarita-selftest"),
        "and the pod the selection named",
    );

    cleanup.expect("the self-test session is killed");
}
