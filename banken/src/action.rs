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
//! - `g` / `shift+g` → a **guarita**: a `(defguarita)` resolved against the
//!   selected row into a pre-warmed tear session plan ([`plan_guarita`]). Its
//!   postigo class is DERIVED from the recipe's panes, so the overlay states
//!   the gate the session would cross rather than taking the author's word.
//!
//! Every path goes through [`banken_spec::interp::apply`] over a
//! [`banken_spec::env::ClusterEnv`]. There is **no live-mutate path** —
//! the trait has no unwitnessed-mutate method, so a DECLARE can only reach
//! `declare` (git) and a shell can only reach `break_glass` (witnessed).
//! This module adds zero new legality — it consumes the shipped gate.

use awase::Key;
use banken_spec::chord::ActionChord;
use banken_spec::env::ClusterEnv;
use banken_spec::guarita::{GuaritaContext, GuaritaSpec, PlannedPane};
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
                keys: ActionChord::plain(Key::L),
                legality: ActionLegality::Observe,
                manifest_scope: ManifestScope::Full,
            },
            RowAction::DeclareScale => K8sActionSpec {
                name: "scale".into(),
                keys: ActionChord::plain(Key::S),
                legality: ActionLegality::Declare {
                    target: DeclareTarget::FluxHelmValues {
                        release_path: "apps/catch/release.yaml".into(),
                    },
                },
                manifest_scope: ManifestScope::Full,
            },
            RowAction::BreakGlassShell => K8sActionSpec {
                name: "shell".into(),
                // `shift+s`, not `S` — awase folds case (so `S` would BE
                // the scale chord) and egaku-term delivers held-shift as
                // `shift+s` anyway. See `banken_spec::chord`.
                keys: ActionChord::shifted(Key::S),
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
    /// A `(defguarita)` resolved into a pre-warmed tear session PLAN — the
    /// panes, their placement, and the fully-resolved argv each would be
    /// staged with, plus the recipe's DERIVED postigo class.
    ///
    /// A **preview**, exactly like [`Self::DeclarePreview`]: producing the
    /// plan touches nothing. Handing it to a live `tear-daemon` is the
    /// [`banken_spec::guarita::SessionEnv`] seam's job, and banken ships no
    /// implementation of that seam yet — `pending-banken: guarita-live-handoff`.
    GuaritaPlan {
        /// The recipe's name.
        recipe: String,
        /// Its derived legality class label, uppercased ("OBSERVE" /
        /// "BREAK-GLASS"). Derived from the panes; there is no field in
        /// which a recipe could claim otherwise.
        legality: String,
        /// The generated tear session name.
        session_name: String,
        /// One rendered line per pane.
        lines: Vec<String>,
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

/// Resolve a `(defguarita)` against the selected row into a session plan.
///
/// This is the banken → tear/mado bridge at the keystroke: the selected row
/// plus the cluster banken is reading become a [`GuaritaContext`], and
/// [`banken_spec::guarita::plan`] turns the authored recipe into concrete
/// panes with concrete argv.
///
/// Every failure is surfaced as [`ActionResult::Error`] carrying the typed
/// message — in particular the **unknown-cluster refusal**, which is the one
/// an operator must see rather than have papered over: a pre-warmed session
/// on the wrong cluster is worse than no pre-warmed session.
#[must_use]
pub fn plan_guarita(table: &PodTable, spec: &GuaritaSpec, cluster: &str) -> ActionResult {
    let Some(selection) = current_selection(table) else {
        return ActionResult::Error("no row selected".into());
    };
    let ctx = GuaritaContext {
        cluster: cluster.to_owned(),
        selection,
        // M0 has no container picker; a recipe referencing `(:context
        // container)` therefore refuses BY NAME rather than guessing the
        // first container. `pending-banken: guarita-container-selection`.
        container: None,
    };
    match banken_spec::guarita::plan(spec, &ctx) {
        Ok(p) => ActionResult::GuaritaPlan {
            recipe: spec.name.clone(),
            legality: p.legality().class().label().to_uppercase(),
            session_name: p.session_name().to_owned(),
            lines: p.panes().iter().map(render_pane).collect(),
        },
        Err(e) => ActionResult::Error(e.to_string()),
    }
}

/// `"logs      [root ]  kubectl --context camelot-eks …"` — assembled by
/// concatenation from typed pieces (★★ TYPED EMISSION), never a `format!()`
/// of a layout template.
fn render_pane(pane: &PlannedPane) -> String {
    let mut s = String::new();
    s.push_str(pane.role.label());
    while s.chars().count() < 10 {
        s.push(' ');
    }
    s.push('[');
    s.push_str(pane.placement.label());
    s.push_str("]  ");
    s.push_str(&pane.argv.join(" "));
    s
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
            // `shift+s`, not `S` — see `banken_spec::chord`.
            (RowAction::BreakGlassShell, "shift+s"),
        ] {
            let spec = action.spec(&op);
            let expected = ActionChord::parse(key).expect("test chord parses");
            assert_eq!(spec.keys, expected);
            let found = authored
                .iter()
                .find(|a| a.keys == expected)
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
