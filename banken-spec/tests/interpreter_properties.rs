//! Interpreter properties (BANKEN.md §III.c) — the real proof of the
//! postigo triplet, driven by `MockClusterEnv` with **zero live
//! cluster**. Exactly the sui-spec `MockEnv` seam.

use std::path::PathBuf;

use awase::Key;
use banken_spec::{
    SpecError,
    chord::ActionChord,
    env::{ClusterEnv, Row},
    interp::{Outcome, Selection, apply, lower_to_full_manifest},
    testing::MockClusterEnv,
    types::{
        ActionLegality, DeclareTarget, K8sActionSpec, ManifestScope, OperatorId, ResourceKind,
        RunbookRef,
    },
};

fn scale_action(release: &str) -> K8sActionSpec {
    K8sActionSpec {
        name: "scale".into(),
        keys: ActionChord::plain(Key::S),
        legality: ActionLegality::Declare {
            target: DeclareTarget::FluxHelmValues {
                release_path: PathBuf::from(release),
            },
        },
        manifest_scope: ManifestScope::Full,
    }
}

fn observe_action() -> K8sActionSpec {
    K8sActionSpec {
        name: "view-logs".into(),
        keys: ActionChord::plain(Key::L),
        legality: ActionLegality::Observe,
        manifest_scope: ManifestScope::Full,
    }
}

fn shell_action() -> K8sActionSpec {
    K8sActionSpec {
        name: "shell".into(),
        // `shift+s`, not `S` — awase folds case, so `S` == `s` == the scale
        // action's chord (see banken_spec::chord).
        keys: ActionChord::shifted(Key::S),
        legality: ActionLegality::BreakGlass {
            witness: OperatorId("drzzln".into()),
            runbook: RunbookRef("clusters/rio/RUNBOOK.md".into()),
        },
        manifest_scope: ManifestScope::Full,
    }
}

fn selection_with_spec() -> Selection {
    Selection {
        kind: ResourceKind::Deployment,
        name: "catch".into(),
        namespace: Some("catch".into()),
        current: vec![
            ("replicas".into(), "2".into()),
            ("image".into(), "ghcr.io/pleme-io/catch:amd64-r1-abc".into()),
            // A sibling gate — the field a targeted patch would silently
            // miss. The full manifest MUST carry it.
            ("dryRun".into(), "false".into()),
        ],
    }
}

// ── (a) DECLARE serializes the FULL manifest, never a patch ─────────

#[test]
fn declare_serializes_full_manifest_with_every_sibling_field() {
    let change = lower_to_full_manifest(
        &selection_with_spec(),
        &DeclareTarget::FluxHelmValues {
            release_path: PathBuf::from("apps/catch/release.yaml"),
        },
        ManifestScope::Full,
    )
    .expect("lowering must succeed for a real release path");

    // The whole spec is present — every sibling field, including the
    // one a targeted patch would drop (the 2026-07-12 dryRun incident).
    assert!(change.full_manifest.contains("replicas"));
    assert!(change.full_manifest.contains("image"));
    assert!(
        change.full_manifest.contains("dryRun"),
        "full manifest MUST carry the sibling `dryRun` gate — a patch would drop it",
    );
    // The scope is Full, and the manifest is real YAML.
    assert_eq!(change.scope, ManifestScope::Full);
    let parsed: serde_yaml::Value =
        serde_yaml::from_str(&change.full_manifest).expect("emitted manifest is valid YAML");
    assert!(
        parsed.get("spec").is_some(),
        "full manifest carries a spec block"
    );
}

#[test]
fn apply_declare_records_the_change_on_the_env() {
    let env = MockClusterEnv::new();
    let action = scale_action("apps/catch/release.yaml");
    let out = apply(&action, &selection_with_spec(), &env).expect("declare applies");

    match out {
        Outcome::Committed(change_ref) => {
            assert!(change_ref.0.starts_with("mock-change-"));
        }
        other => panic!("expected Committed, got {other:?}"),
    }

    // Exactly one change recorded, and it carries the full manifest.
    assert_eq!(env.declared_count(), 1);
    let declared = env.declared.borrow();
    assert!(declared[0].full_manifest.contains("dryRun"));
}

// ── (b) zero network — the mock has no socket; a Declare touches      ─
//        nothing live, only the recorded git-change log ──────────────

#[test]
fn declare_touches_no_live_cluster_only_git() {
    let env = MockClusterEnv::new();
    let action = scale_action("apps/catch/release.yaml");
    apply(&action, &selection_with_spec(), &env).unwrap();

    // A DECLARE recorded a git change and performed ZERO break-glass
    // (the only live arm). The mock has no network client at all — the
    // absence of a socket is the structural proof of "zero network".
    assert_eq!(env.declared_count(), 1, "git change recorded");
    assert_eq!(env.glass_count(), 0, "no live effect from a DECLARE");
}

// ── (c) a DECLARE never calls a live mutate — there is none to call ──
//
// This is enforced structurally: `ClusterEnv` has no unwitnessed-mutate
// method, so `apply`'s Declare arm CAN ONLY reach `declare` (git) — the
// compiler would reject any other call. The observable proof is that a
// DECLARE produces a git change and never a `GlassRecord`.

#[test]
fn observe_action_mutates_nothing() {
    let env = MockClusterEnv::new().with_row(
        ResourceKind::Deployment,
        Row {
            name: "catch".into(),
            namespace: Some("catch".into()),
            cells: vec![("READY".into(), "2/2".into())],
        },
    );
    let out = apply(&observe_action(), &selection_with_spec(), &env).unwrap();
    match out {
        Outcome::Observed(rows) => assert_eq!(rows.len(), 1),
        other => panic!("expected Observed, got {other:?}"),
    }
    assert_eq!(env.declared_count(), 0);
    assert_eq!(env.glass_count(), 0);
}

#[test]
fn break_glass_is_the_only_live_arm_and_is_witnessed() {
    let env = MockClusterEnv::new();
    let out = apply(&shell_action(), &selection_with_spec(), &env).unwrap();
    match out {
        Outcome::GlassLogged(record) => {
            assert_eq!(record.action.witness, OperatorId("drzzln".into()));
            assert_eq!(
                record.action.runbook,
                RunbookRef("clusters/rio/RUNBOOK.md".into())
            );
            assert!(record.action.selector.contains("catch"));
        }
        other => panic!("expected GlassLogged, got {other:?}"),
    }
    // A break-glass records exactly one witnessed action and no git change.
    assert_eq!(env.glass_count(), 1);
    assert_eq!(env.declared_count(), 0);
}

// ── coverage gap: no owning release.yaml → typed error, not silent Ok ─

#[test]
fn declare_with_no_owning_release_yaml_is_a_typed_error() {
    let action = scale_action(""); // empty release path = no owning release.yaml
    let env = MockClusterEnv::new();
    let err = apply(&action, &selection_with_spec(), &env)
        .expect_err("a resource with no owning release.yaml has no DECLARE target");
    match err {
        SpecError::NoLoweringTarget(msg) => assert!(msg.contains("catch")),
        other => panic!("expected NoLoweringTarget, got {other:?}"),
    }
    // Nothing recorded — the coverage gap surfaced cleanly.
    assert_eq!(env.declared_count(), 0);
}

// ── OBSERVE reads freely ────────────────────────────────────────────

#[test]
fn observe_reads_return_seeded_rows() {
    let env = MockClusterEnv::new()
        .with_row(
            ResourceKind::Pod,
            Row {
                name: "catch-0".into(),
                namespace: Some("catch".into()),
                cells: vec![],
            },
        )
        .with_row(
            ResourceKind::Pod,
            Row {
                name: "catch-1".into(),
                namespace: Some("catch".into()),
                cells: vec![],
            },
        );
    let rows = env
        .list_resources(ResourceKind::Pod, Some("catch"))
        .unwrap();
    assert_eq!(rows.len(), 2);
    // The health seam is a pure read too.
    let watch = env.watch(ResourceKind::Pod, None).unwrap();
    assert_eq!(watch.rows.len(), 2);
}
