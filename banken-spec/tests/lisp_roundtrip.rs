//! The authored `(defk8sview)`/`(defk8saction)` Lisp specs round-trip
//! into the typed border (BANKEN.md §III.b) — and the authored DECLARE
//! action drives `apply` end-to-end against the mock, proving the Lisp
//! authoring surface and the interpreter agree.
//!
//! This is the third artifact of the TYPED-SPEC + INTERPRETER TRIPLET
//! actually driving the first two — no live cluster.

use banken_spec::{
    interp::{Outcome, Selection, apply},
    load_actions, load_views,
    testing::MockClusterEnv,
    types::{
        ActionLegality, DeclareTarget, LegalityClass, ManifestScope, ResourceKind, ViewKind,
        ViewSource,
    },
};

#[test]
fn authored_views_compile_into_typed_values() {
    let views = load_views().expect("canonical views must compile");
    assert_eq!(
        views.len(),
        10,
        "pods + svc + ward + deploy + rs + no + ns + cm + ep + ev",
    );

    let pods = views.iter().find(|v| v.name == "pods").expect("pods view");
    assert_eq!(pods.kind, ViewKind::ResourceTable);
    assert_eq!(pods.source, ViewSource::Resource(ResourceKind::Pod));
    assert_eq!(pods.columns.len(), 5);
    assert_eq!(pods.default_sort.column, "STATUS");
    assert_eq!(pods.drill_to.as_deref(), Some("logs"));

    let ward = views.iter().find(|v| v.name == "ward").expect("ward view");
    assert_eq!(ward.kind, ViewKind::HealthWard);
    assert_eq!(ward.source, ViewSource::Health);
}

#[test]
fn authored_actions_compile_into_typed_values() {
    let actions = load_actions().expect("canonical actions must compile");
    assert_eq!(actions.len(), 4, "scale + view-logs + describe + shell");

    // Every action has the mandatory Full manifest scope.
    for a in &actions {
        assert_eq!(a.manifest_scope, ManifestScope::Full);
    }

    // The scale action is a DECLARE onto the Flux-values rail.
    let scale = actions
        .iter()
        .find(|a| a.name == "scale")
        .expect("scale action");
    assert_eq!(scale.legality.class(), LegalityClass::Declare);
    match &scale.legality {
        ActionLegality::Declare {
            target: DeclareTarget::FluxHelmValues { release_path },
        } => assert_eq!(release_path.to_string_lossy(), "apps/catch/release.yaml"),
        other => panic!("scale should DECLARE to Flux values, got {other:?}"),
    }

    // The shell action is BREAK-GLASS — witnessed.
    let shell = actions
        .iter()
        .find(|a| a.name == "shell")
        .expect("shell action");
    assert_eq!(shell.legality.class(), LegalityClass::BreakGlass);

    // view-logs + describe are OBSERVE.
    for name in ["view-logs", "describe"] {
        let a = actions.iter().find(|a| a.name == name).unwrap();
        assert_eq!(a.legality.class(), LegalityClass::Observe);
    }
}

#[test]
fn authored_declare_action_drives_apply_to_a_full_manifest() {
    let actions = load_actions().unwrap();
    let scale = actions.iter().find(|a| a.name == "scale").unwrap().clone();

    let sel = Selection {
        kind: ResourceKind::Deployment,
        name: "catch".into(),
        namespace: Some("catch".into()),
        current: vec![
            ("replicas".into(), "2".into()),
            ("dryRun".into(), "false".into()),
        ],
    };
    let env = MockClusterEnv::new();
    let out = apply(&scale, &sel, &env).expect("authored declare applies");
    assert!(matches!(out, Outcome::Committed(_)));

    // The authored action lowered to a FULL manifest carrying every
    // sibling field — the Lisp surface and the interpreter agree.
    let declared = env.declared.borrow();
    assert_eq!(declared.len(), 1);
    assert!(declared[0].full_manifest.contains("dryRun"));
    assert_eq!(declared[0].scope, ManifestScope::Full);
}

/// **Every resource kind the backend can read is reachable from a view.**
///
/// The count assertion above catches a view landing without its catalog
/// entry. This catches the opposite and more likely drift: a `ResourceKind`
/// gaining a live read while no `(defk8sview …)` names it, leaving a kind
/// that the backend serves and nothing can navigate to. That is invisible —
/// everything compiles, every test passes, and the operator simply has no
/// way to ask for it.
///
/// `ResourceKind::ALL` is the denominator, so a kind added to the enum fails
/// here until it is either given a view or explicitly excused below.
#[test]
fn every_resource_kind_is_reachable_from_some_view() {
    let views = load_views().expect("canonical views must compile");
    let mut unreachable = Vec::new();
    for kind in ResourceKind::ALL {
        let reachable = views
            .iter()
            .any(|v| v.source == ViewSource::Resource(*kind));
        if !reachable {
            unreachable.push(kind.label());
        }
    }
    assert!(
        unreachable.is_empty(),
        "these kinds read live but no (defk8sview) names them, so nothing can \
         navigate to them: {unreachable:?}",
    );
}
