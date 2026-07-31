//! The `(defbancada)` bridge, proven end-to-end against the **shipped**
//! `specs/bancadas.lisp` — not against hand-built fixtures.
//!
//! Unit tests in `src/bancada.rs` prove the algebra over constructed values;
//! this file proves that the recipes banken actually ships compile from Lisp,
//! resolve into the catalog, produce the right session/pane/argv plan for a
//! concrete cluster + selection, and walk through the [`SessionEnv`] seam with
//! the witnessed arm used exactly where a live effect is staged.
//!
//! **Zero side effects, by construction**: [`MockSessionEnv`] has no socket,
//! no subprocess and no PTY. The LIVE handoff over a real `tear-daemon` is
//! proven separately and deliberately elsewhere —
//! `banken/tests/tear_handoff.rs`, `#[ignore]`d behind the `tear` feature,
//! because a plain `cargo test` must never open a session on the operator's
//! machine.

use banken_spec::{
    bancada::{BancadaContext, CommandEffect, PanePlacement, PaneRole, SessionLayout, open, plan},
    interp::Selection,
    load_bancadas, load_catalog,
    testing::MockSessionEnv,
    types::{ActionLegality, LegalityClass, ResourceKind},
};

/// A concrete broken-pod context: camelot-eks, namespace `catch`, one pod.
fn ctx() -> BancadaContext {
    BancadaContext {
        cluster: "camelot-eks".into(),
        selection: Selection {
            kind: ResourceKind::Pod,
            name: "catch-7d9f4c".into(),
            namespace: Some("catch".into()),
            current: Vec::new(),
        },
        container: Some("catch".into()),
    }
}

/// (a) The authored forms compile from Lisp into typed values.
#[test]
fn the_authored_bancadas_compile_into_typed_values() {
    let gs = load_bancadas().expect("specs/bancadas.lisp must compile");
    assert_eq!(gs.len(), 2, "two recipes ship");

    let triage = gs
        .iter()
        .find(|g| g.name == "pod-triage")
        .expect("pod-triage");
    assert_eq!(triage.keys.canonical(), "g");
    assert_eq!(triage.from, "pods");
    assert_eq!(triage.layout, SessionLayout::MainVertical);
    assert_eq!(triage.panes.len(), 3);
    assert_eq!(triage.panes[0].role, PaneRole::Logs);
    assert_eq!(triage.panes[0].placement, PanePlacement::Root);
    assert_eq!(triage.panes[1].placement, PanePlacement::Right);
    assert_eq!(triage.panes[2].placement, PanePlacement::Below);

    let glass = gs
        .iter()
        .find(|g| g.name == "pod-break-glass")
        .expect("pod-break-glass");
    // `shift+g`, not `G` — awase folds case, so `G` WOULD BE `g` and would
    // collide with pod-triage above. Same trap the `shell` action hit.
    assert_eq!(glass.keys.canonical(), "shift+g");
    assert_eq!(glass.panes.last().expect("last pane").role, PaneRole::Shell);
}

/// (c) THE GATE. A recipe staging a mutating command is BREAK-GLASS; an
/// observe-only one is not. Neither can say otherwise — there is no
/// `:legality` kwarg on `(defbancada)`.
#[test]
fn a_mutating_recipe_is_break_glass_and_an_observing_one_is_not() {
    let gs = load_bancadas().expect("compiles");

    let triage = gs.iter().find(|g| g.name == "pod-triage").expect("triage");
    assert!(!triage.mutates());
    assert_eq!(
        triage.legality().expect("legal"),
        ActionLegality::Observe,
        "a recipe of pure reads is OBSERVE",
    );

    let glass = gs
        .iter()
        .find(|g| g.name == "pod-break-glass")
        .expect("glass");
    assert!(glass.mutates(), "it stages `kubectl exec`");
    match glass.legality().expect("legal") {
        ActionLegality::BreakGlass { witness, runbook } => {
            assert_eq!(witness.0, "drzzln");
            assert!(runbook.0.contains("RUNBOOK"));
        }
        other => panic!("a recipe staging a live effect must be BREAK-GLASS, got {other:?}"),
    }

    // The derivation is REAL, not decorative: flip the one authored pane's
    // effect back to `observes` and the class flips with it — with the
    // witness/runbook then becoming the error, not the legality.
    let mut demoted = glass.clone();
    for p in &mut demoted.panes {
        p.command.effect = CommandEffect::Observes;
    }
    assert!(
        !demoted.mutates(),
        "with no mutating pane the recipe no longer mutates",
    );
    assert!(
        demoted.validate().is_err(),
        "and its now-pointless witness is itself rejected",
    );
}

/// (b) THE GATE. The plan is the pre-warming: the right cluster, the right
/// namespace, the right pod, already resolved onto every pane's command line.
#[test]
fn the_authored_triage_recipe_plans_the_right_session() {
    let gs = load_bancadas().expect("compiles");
    let triage = gs.iter().find(|g| g.name == "pod-triage").expect("triage");
    let p = plan(triage, &ctx()).expect("plans");

    assert_eq!(p.session_name(), "triage-camelot-eks-catch-catch-7d9f4c");
    assert_eq!(p.layout(), SessionLayout::MainVertical);
    assert_eq!(p.legality().class(), LegalityClass::Observe);
    assert!(p.witnessed_action().is_none());

    let logs = &p.panes()[0];
    assert_eq!(
        logs.argv,
        vec![
            "kubectl",
            "--context",
            "camelot-eks",
            "-n",
            "catch",
            "logs",
            "--follow",
            "--tail=200",
            "catch-7d9f4c",
        ],
        "the log pane opens on the cluster banken is reading, not on whatever \
         the kubeconfig's current context happens to be",
    );

    let describe = &p.panes()[2];
    assert!(
        describe.argv.contains(&"describe".to_string())
            && describe.argv.contains(&"pod".to_string())
            && describe.argv.contains(&"catch-7d9f4c".to_string()),
        "the describe pane carries the resource KIND and NAME: {:?}",
        describe.argv,
    );
}

/// THE GATE, again where it bites hardest: if banken does not know which
/// cluster it is reading, the recipe is REFUSED rather than resolved to a
/// `--context ""` that silently opens on a different cluster.
#[test]
fn planning_without_a_known_cluster_is_refused_not_guessed() {
    let gs = load_bancadas().expect("compiles");
    let triage = gs.iter().find(|g| g.name == "pod-triage").expect("triage");
    let mut c = ctx();
    c.cluster = String::new();

    let err = plan(triage, &c).expect_err("an unknown cluster must be refused");
    let msg = err.to_string();
    assert!(
        msg.contains("cluster") && msg.contains("refusing"),
        "the refusal must name the field and say why: {msg}",
    );
}

/// The full walk: plan → open. The observe recipe stages every pane through
/// the UNWITNESSED arm, and lands the operator on the last pane.
#[test]
fn opening_the_triage_recipe_builds_the_split_session_with_no_witness() {
    let gs = load_bancadas().expect("compiles");
    let triage = gs.iter().find(|g| g.name == "pod-triage").expect("triage");
    let p = plan(triage, &ctx()).expect("plans");

    let env = MockSessionEnv::new();
    let refs = open(&p, &env).expect("opens");

    assert_eq!(refs.len(), 3);
    // One session, two splits — the root is the session, not a split.
    assert_eq!(env.sessions.borrow().len(), 1);
    assert_eq!(env.sessions.borrow()[0].1, SessionLayout::MainVertical);
    assert_eq!(
        env.splits.borrow().as_slice(),
        &[
            (refs[0], PanePlacement::Right),
            (refs[1], PanePlacement::Below),
        ],
        "each split hangs off the pane before it",
    );
    assert_eq!(env.spawned.borrow().len(), 3);
    assert!(
        env.shells.borrow().is_empty(),
        "a pure-read recipe spawns every pane as its own argv — no shell, so \
         nothing is typed at a prompt and nothing needs quoting",
    );
    assert_eq!(
        env.witnessed_count(),
        0,
        "an observe-only recipe must witness nothing",
    );
    assert_eq!(env.focused.borrow().as_slice(), &[refs[2]]);
}

/// THE GATE. The break-glass recipe's `kubectl exec` pane goes through the
/// WITNESSED arm — carrying the authored witness, the RUNBOOK, and a selector
/// naming the session that was opened.
#[test]
fn opening_the_break_glass_recipe_routes_the_exec_through_the_witnessed_arm() {
    let gs = load_bancadas().expect("compiles");
    let glass = gs
        .iter()
        .find(|g| g.name == "pod-break-glass")
        .expect("glass");
    let p = plan(glass, &ctx()).expect("plans");

    let env = MockSessionEnv::new();
    open(&p, &env).expect("opens");

    assert_eq!(
        env.spawned.borrow().len(),
        1,
        "only the log pane is spawned as its own argv",
    );
    assert_eq!(
        env.shells.borrow().len(),
        1,
        "the exec pane is a shell, so its command waits for the operator's \
         own Enter rather than running the moment the pane appears",
    );
    let witnessed = env.witnessed.borrow();
    assert_eq!(witnessed.len(), 1, "the exec pane is the witnessed one");
    let (_, argv, action) = &witnessed[0];
    assert!(
        argv.contains(&"exec".to_string()) && argv.contains(&"catch".to_string()),
        "the witnessed argv is the resolved exec: {argv:?}",
    );
    assert_eq!(action.witness.0, "drzzln");
    assert_eq!(action.runbook.0, "clusters/rio/RUNBOOK.md");
    assert_eq!(action.selector, "glass-camelot-eks-catch-catch-7d9f4c");
}

/// The recipes join the ONE resolved catalog: reachable from a declared view,
/// on chords nothing else claims.
#[test]
fn the_shipped_bancadas_join_the_resolved_catalog() {
    let c = load_catalog().expect("the shipped vocabulary must cross-resolve");
    assert_eq!(c.bancadas().len(), 2);

    let from_pods = c.bancadas_from("pods");
    assert_eq!(
        from_pods.len(),
        2,
        "both recipes launch from the :pods view"
    );
    assert!(
        c.bancadas_from("nosuchview").is_empty(),
        "and none from a surface that does not exist",
    );

    // One chord namespace across all three keyed domains — proven by counting
    // distinct canonical chords against the total number of claims.
    let mut chords: Vec<String> = Vec::new();
    chords.extend(c.actions().iter().map(|a| a.keys.canonical()));
    chords.extend(c.nav_keys().iter().map(|n| n.keys.canonical()));
    chords.extend(c.bancadas().iter().map(|g| g.keys.canonical()));
    let total = chords.len();
    chords.sort();
    chords.dedup();
    assert_eq!(
        chords.len(),
        total,
        "actions, nav keys and bancadas share ONE chord namespace",
    );
}
