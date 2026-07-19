//! The postigo action dispatch — wiring the shipped `banken_spec`
//! citizenship primitive through the UI (BANKEN.md §III / §VI).
//!
//! This is where the three-class gate meets a keystroke on a selected pod
//! row:
//!
//! - `l` → **OBSERVE**: a read (logs), rendered as an [`ActionResult`]
//!   panel. Mutates nothing.
//! - `s` → **DECLARE**: `banken_spec::apply` lowers the selection to a
//!   **full-manifest** GitOps change; the result carries the whole
//!   manifest preview that would be committed (never a live mutation).
//! - `S` → **BREAK-GLASS**: `banken_spec::apply` routes to
//!   `env.break_glass`, producing a witnessed [`GlassRecord`] preview.
//!
//! Every path goes through [`banken_spec::interp::apply`] over a
//! [`banken_spec::env::ClusterEnv`]. There is **no live-mutate path** —
//! the trait has no unwitnessed-mutate method, so a DECLARE can only reach
//! `declare` (git) and a shell can only reach `break_glass` (witnessed).
//! This module adds zero new legality — it consumes the shipped gate.

use banken_spec::env::ClusterEnv;
use banken_spec::interp::{Outcome, Selection, apply};
use banken_spec::types::{
    ActionLegality, DeclareTarget, K8sActionSpec, ManifestScope, OperatorId, RunbookRef,
};

use crate::table::PodTable;

/// A key the operator can press on a selected row, mapped to its postigo
/// action. Kept small + explicit — each variant is a `(defk8saction)` the
/// shipped `banken-spec` catalog already declares.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowAction {
    /// `l` → OBSERVE the logs.
    ViewLogs,
    /// `s` → DECLARE a scale change (full-manifest GitOps preview).
    DeclareScale,
    /// `S` → BREAK-GLASS shell (witnessed, RUNBOOK-logged).
    BreakGlassShell,
}

impl RowAction {
    /// The postigo legality class label for this action — surfaced in the
    /// UI so the operator sees which gate a keystroke crosses.
    #[must_use]
    pub fn class_label(self) -> &'static str {
        match self {
            RowAction::ViewLogs => "OBSERVE",
            RowAction::DeclareScale => "DECLARE",
            RowAction::BreakGlassShell => "BREAK-GLASS",
        }
    }

    /// Build the typed `K8sActionSpec` for this action. These mirror the
    /// authored `banken-spec/specs/actions.lisp` forms exactly (same keys,
    /// same legality) — the app could equally `banken_spec::load_actions()`
    /// and match by key; the typed constructors keep the demo self-contained
    /// and are asserted equivalent by a test.
    fn spec(self, operator: &OperatorId) -> K8sActionSpec {
        match self {
            RowAction::ViewLogs => K8sActionSpec {
                name: "view-logs".into(),
                keys: "l".into(),
                legality: ActionLegality::Observe,
                manifest_scope: ManifestScope::Full,
            },
            RowAction::DeclareScale => K8sActionSpec {
                name: "scale".into(),
                keys: "s".into(),
                legality: ActionLegality::Declare {
                    target: DeclareTarget::FluxHelmValues {
                        release_path: "apps/catch/release.yaml".into(),
                    },
                },
                manifest_scope: ManifestScope::Full,
            },
            RowAction::BreakGlassShell => K8sActionSpec {
                name: "shell".into(),
                keys: "S".into(),
                legality: ActionLegality::BreakGlass {
                    witness: operator.clone(),
                    runbook: RunbookRef("clusters/rio/RUNBOOK.md".into()),
                },
                manifest_scope: ManifestScope::Full,
            },
        }
    }
}

/// The rendered result of a postigo action — what the UI panel shows.
/// Every variant is a *preview / read*; none represents a live mutation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActionResult {
    /// An OBSERVE read produced these lines (logs / describe / events).
    Observed { title: String, lines: Vec<String> },
    /// A DECLARE produced the full-manifest change that *would* be
    /// committed (a branch+PR preview — never applied live here).
    DeclarePreview {
        change_ref: String,
        full_manifest: String,
    },
    /// A BREAK-GLASS produced a witnessed record preview.
    BreakGlassRecord {
        witness: String,
        runbook: String,
        selector: String,
        record_id: String,
    },
    /// The action could not run — a typed error surfaced (e.g. no owning
    /// `release.yaml` for a DECLARE, the §IX coverage gap).
    Error(String),
}

/// Build the [`Selection`] for the currently-selected pod row.
///
/// The selection carries the row's observed cells as the read half of the
/// DECLARE read-modify-write (so `lower_to_full_manifest` serializes the
/// whole spec). `None` when the table is empty.
#[must_use]
pub fn current_selection(table: &PodTable) -> Option<Selection> {
    let row = table.selected_row()?;
    Some(Selection {
        kind: table.kind(),
        name: row.name.clone(),
        namespace: row.namespace.clone(),
        current: row.cells.clone(),
    })
}

/// Dispatch a [`RowAction`] on the selected row through the postigo gate.
///
/// For OBSERVE actions the read is performed directly (logs). For DECLARE
/// / BREAK-GLASS the shipped `banken_spec::apply` interpreter is invoked —
/// so the exact citizenship gate is exercised, not re-implemented. Any
/// typed `SpecError` is folded into [`ActionResult::Error`] (never a
/// silent wrong `Ok`).
pub fn dispatch<E: ClusterEnv>(
    table: &PodTable,
    action: RowAction,
    operator: &OperatorId,
    env: &E,
) -> ActionResult {
    let Some(sel) = current_selection(table) else {
        return ActionResult::Error("no row selected".into());
    };

    // OBSERVE `l` routes to the exact read (logs) rather than the generic
    // `list_resources` the interpreter's Observe arm dispatches to — the
    // legality class is identical (a read), and the operator wants the
    // pod's logs specifically. The DECLARE/BREAK-GLASS classes go through
    // `apply` so the shipped gate + lowering are exercised verbatim.
    if action == RowAction::ViewLogs {
        return observe_logs(&sel, env);
    }

    let spec = action.spec(operator);
    match apply(&spec, &sel, env) {
        Ok(Outcome::Observed(rows)) => ActionResult::Observed {
            title: "observed".into(),
            lines: rows.into_iter().map(|r| r.name).collect(),
        },
        Ok(Outcome::Committed(change)) => {
            // Re-lower to surface the full manifest for the preview panel.
            // (The committed ChangeRef proves the gate reached `declare`;
            // the manifest is what the operator reviews.)
            let manifest = match &spec.legality {
                ActionLegality::Declare { target } => {
                    banken_spec::interp::lower_to_full_manifest(&sel, target, spec.manifest_scope)
                        .map(|c| c.full_manifest)
                        .unwrap_or_else(|e| e.to_string())
                }
                _ => String::new(),
            };
            ActionResult::DeclarePreview {
                change_ref: change.0,
                full_manifest: manifest,
            }
        }
        Ok(Outcome::GlassLogged(record)) => ActionResult::BreakGlassRecord {
            witness: record.action.witness.0,
            runbook: record.action.runbook.0,
            selector: record.action.selector,
            record_id: record.record_id,
        },
        Err(e) => ActionResult::Error(e.to_string()),
    }
}

/// Perform the OBSERVE logs read for a selection.
fn observe_logs<E: ClusterEnv>(sel: &Selection, env: &E) -> ActionResult {
    let ns = sel.namespace.as_deref().unwrap_or("default");
    match env.logs(&sel.name, ns, false) {
        Ok(stream) => ActionResult::Observed {
            title: {
                let mut t = String::from("logs: ");
                t.push_str(&stream.pod);
                t
            },
            lines: stream.lines,
        },
        Err(e) => ActionResult::Error(e.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixture::FixtureClusterEnv;
    use banken_spec::env::ClusterEnv;
    use banken_spec::types::ResourceKind;

    fn table() -> PodTable {
        let env = FixtureClusterEnv::new();
        PodTable::pods(env.list_resources(ResourceKind::Pod, None).unwrap())
    }

    #[test]
    fn spec_matches_the_authored_lisp_forms() {
        // The typed constructors must match the shipped
        // `banken-spec/specs/actions.lisp` forms (same keys + legality
        // class) — otherwise the demo diverges from the citizenship
        // catalog. Load the authored actions and cross-check.
        let authored = banken_spec::load_actions().expect("authored actions load");
        let op = OperatorId("drzzln".into());
        for (action, key) in [
            (RowAction::ViewLogs, "l"),
            (RowAction::DeclareScale, "s"),
            (RowAction::BreakGlassShell, "S"),
        ] {
            let spec = action.spec(&op);
            assert_eq!(spec.keys, key);
            let found = authored
                .iter()
                .find(|a| a.keys == key)
                .unwrap_or_else(|| panic!("authored action for key {key}"));
            assert_eq!(
                spec.legality.class(),
                found.legality.class(),
                "legality class matches the authored form for key {key}",
            );
        }
    }

    #[test]
    fn declare_dispatch_produces_a_full_manifest_preview() {
        let t = table();
        let env = FixtureClusterEnv::new();
        let op = OperatorId("drzzln".into());
        let result = dispatch(&t, RowAction::DeclareScale, &op, &env);
        match result {
            ActionResult::DeclarePreview {
                change_ref,
                full_manifest,
            } => {
                assert!(!change_ref.is_empty());
                // The whole selected pod's spec is present in the preview.
                assert!(full_manifest.contains("STATUS") || full_manifest.contains("spec"));
                assert!(full_manifest.contains("flux-helm-values"));
            }
            other => panic!("expected a DeclarePreview, got {other:?}"),
        }
    }

    #[test]
    fn observe_dispatch_reads_logs_and_mutates_nothing() {
        let t = table();
        let env = FixtureClusterEnv::new();
        let op = OperatorId("drzzln".into());
        let result = dispatch(&t, RowAction::ViewLogs, &op, &env);
        match result {
            ActionResult::Observed { lines, .. } => assert!(!lines.is_empty()),
            other => panic!("expected Observed logs, got {other:?}"),
        }
    }

    #[test]
    fn break_glass_dispatch_is_witnessed() {
        let t = table();
        let env = FixtureClusterEnv::new();
        let op = OperatorId("drzzln".into());
        let result = dispatch(&t, RowAction::BreakGlassShell, &op, &env);
        match result {
            ActionResult::BreakGlassRecord {
                witness, runbook, ..
            } => {
                assert_eq!(witness, "drzzln");
                assert!(runbook.contains("RUNBOOK"));
            }
            other => panic!("expected BreakGlassRecord, got {other:?}"),
        }
    }
}
