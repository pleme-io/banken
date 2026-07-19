//! The interpreter (BANKEN.md §III.c) — `apply<E: ClusterEnv>`
//! dispatching on `action.legality`, plus `lower_to_full_manifest`.
//!
//! - OBSERVE → a read method on the env.
//! - DECLARE → `lower_to_full_manifest(sel, target)` (serialize the
//!   **whole** spec, never a targeted patch) then `env.declare(..)`.
//! - BREAK-GLASS → `env.break_glass(..)`.
//!
//! The lowering serializes a typed manifest value via `serde_yaml`
//! (★★ TYPED EMISSION — never `format!()` of YAML syntax). `ManifestScope`
//! is `Full`, and the serialized document is the entire spec, so the
//! "full manifest, never a patch" org rule holds by construction for the
//! fields banken types.

use serde::Serialize;

use crate::{
    env::{ClusterEnv, DeclareChange, Row, WitnessedAction},
    error::SpecError,
    types::{ActionLegality, DeclareTarget, K8sActionSpec, ManifestScope, ResourceKind},
};

/// What the operator has selected in the current view — the subject of
/// an action. A minimal typed handle in M0/M1 (a real one carries the
/// full observed `Row`); enough to lower a DECLARE and to describe a
/// break-glass target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Selection {
    /// The selected resource kind.
    pub kind: ResourceKind,
    /// The selected resource name.
    pub name: String,
    /// Namespace, if namespaced.
    pub namespace: Option<String>,
    /// The current observed value of the resource, as key/value pairs
    /// (the read half of the read-modify-write). Empty for a bare
    /// selection.
    pub current: Vec<(String, String)>,
}

impl Selection {
    /// A human-readable description of the selection, used to log a
    /// break-glass target.
    pub fn describe(&self) -> String {
        match &self.namespace {
            Some(ns) => {
                let mut s = String::new();
                s.push_str(self.kind.label());
                s.push('/');
                s.push_str(&self.name);
                s.push_str(" (ns=");
                s.push_str(ns);
                s.push(')');
                s
            }
            None => {
                let mut s = String::new();
                s.push_str(self.kind.label());
                s.push('/');
                s.push_str(&self.name);
                s
            }
        }
    }
}

/// The typed value that `lower_to_full_manifest` serializes. This is a
/// *whole-spec* document (not a patch); serde renders every field, so a
/// sibling field cannot be silently dropped the way a targeted
/// `kubectl patch` drops it (the 2026-07-12 `BreatheCloudPool.dryRun`
/// incident).
#[derive(Serialize, Debug, Clone, PartialEq, Eq)]
pub struct FullManifest {
    /// The rail this manifest lands on (label form, for auditability).
    pub rail: String,
    /// The declared target kind (label form).
    pub target: String,
    /// The resource kind being declared against.
    pub kind: String,
    /// The resource name.
    pub name: String,
    /// The namespace, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,
    /// The **entire** observed spec, carried through so the whole
    /// document — every sibling field — is present in the emitted
    /// manifest. This is the "full manifest, never a patch" guarantee
    /// for the fields banken types.
    pub spec: std::collections::BTreeMap<String, String>,
}

/// The result of applying an action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// An OBSERVE action produced rows.
    Observed(Vec<Row>),
    /// A DECLARE action committed a git change.
    Committed(crate::env::ChangeRef),
    /// A BREAK-GLASS action was witnessed and logged.
    GlassLogged(crate::env::GlassRecord),
}

/// Lower a selection + declare target into a **full** manifest.
///
/// Serializes the *whole* selection spec via `serde_yaml` (typed
/// emission, no `format!()`). Returns `SpecError::NoLoweringTarget` when
/// a `FluxHelmValues` target has no owning `release_path` — the
/// §IX C-declare-coverage gap surfaced as a typed error, not a silent
/// wrong `Ok`.
///
/// # Errors
///
/// - `SpecError::NoLoweringTarget` if the DECLARE has no lowering target.
/// - `SpecError::Interp { phase: "lower-manifest" }` if serialization fails.
pub fn lower_to_full_manifest(
    sel: &Selection,
    target: &DeclareTarget,
    scope: ManifestScope,
) -> Result<DeclareChange, SpecError> {
    // `Full` is the only sanctioned scope; the enum has no other arm,
    // so this match is exhaustive and the "whole spec" contract holds.
    let ManifestScope::Full = scope;

    // For the FluxHelmValues rail, a resource with no owning
    // release.yaml has no lowering target — surface it as a typed error
    // (offer break-glass or a reviewed rail upstream), never guess.
    if let DeclareTarget::FluxHelmValues { release_path } = target {
        if release_path.as_os_str().is_empty() {
            return Err(SpecError::NoLoweringTarget(format!(
                "{} has no owning release.yaml",
                sel.describe()
            )));
        }
    }

    let manifest = FullManifest {
        rail: target.kind().label().to_string(),
        target: target.kind().label().to_string(),
        kind: sel.kind.label().to_string(),
        name: sel.name.clone(),
        namespace: sel.namespace.clone(),
        // Carry the ENTIRE observed spec — every sibling field — so the
        // emitted manifest is whole, never a patch.
        spec: sel.current.iter().cloned().collect(),
    };

    let full_manifest = serde_yaml::to_string(&manifest).map_err(|e| SpecError::Interp {
        phase: "lower-manifest".into(),
        message: e.to_string(),
    })?;

    Ok(DeclareChange {
        target: target.clone(),
        scope,
        full_manifest,
    })
}

/// Apply an action against a cluster env, dispatching on its legality
/// class (BANKEN.md §III.c).
///
/// # Errors
///
/// Propagates the env's typed `SpecError` per class, or a
/// `SpecError::Interp`/`NoLoweringTarget` from the lowering step.
pub fn apply<E: ClusterEnv>(
    action: &K8sActionSpec,
    sel: &Selection,
    env: &E,
) -> Result<Outcome, SpecError> {
    match &action.legality {
        ActionLegality::Observe => {
            // Dispatch to a read method. The canonical OBSERVE action
            // lists the selected kind; a real UI routes to the exact
            // read (logs/events/topology) by action name, but the
            // legality class is what the gate cares about.
            let rows = env.list_resources(sel.kind, sel.namespace.as_deref())?;
            Ok(Outcome::Observed(rows))
        }
        ActionLegality::Declare { target } => {
            let change = lower_to_full_manifest(sel, target, action.manifest_scope)?;
            let change_ref = env.declare(change)?;
            Ok(Outcome::Committed(change_ref))
        }
        ActionLegality::BreakGlass { witness, runbook } => {
            let record = env.break_glass(WitnessedAction {
                selector: sel.describe(),
                witness: witness.clone(),
                runbook: runbook.clone(),
            })?;
            Ok(Outcome::GlassLogged(record))
        }
    }
}
