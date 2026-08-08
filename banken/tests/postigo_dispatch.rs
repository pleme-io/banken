//! Integration test of the postigo action dispatch through the UI
//! (BANKEN.md §III / §VI — the citizenship payoff, wired live).
//!
//! Proves that selecting a pod row + pressing a postigo chord routes
//! through the shipped `banken_spec::apply` gate and produces the right
//! typed `Outcome`:
//!  - `s` (DECLARE) → a full-manifest change preview is present.
//!  - `l` (OBSERVE) → a logs read (mutates nothing).
//!  - `S` (BREAK-GLASS) → a witnessed record (never a live mutation).
//! And that **no path performs a live mutation** — structurally impossible,
//! because `ClusterEnv` has no unwitnessed-mutate method.
//!
//! Driven end-to-end through `BankenApp::apply_action` (the same path the
//! event loop takes) over the fixture env — zero terminal, zero cluster.

use banken::action::{ActionResult, RowAction, current_selection, dispatch};
use banken::app::{Action, BankenApp};
use banken::fixture::FixtureClusterEnv;
use banken_spec::testing::MockSessionEnv;
use banken_spec::types::OperatorId;

fn app() -> BankenApp<FixtureClusterEnv, MockSessionEnv> {
    BankenApp::try_new(
        FixtureClusterEnv::new(),
        MockSessionEnv::new(),
        OperatorId::new("drzzln").expect("a literal witness is non-blank"),
        "source: fixture",
    )
    .expect("the shipped vocabulary must build an app")
}

/// Selecting a row + the `s` key produces a DECLARE Outcome whose
/// full-manifest preview is present (the prompt's required proof).
#[test]
fn declare_key_produces_full_manifest_preview_through_the_app() {
    let mut a = app();
    // Move to a specific pod so the selection is deterministic.
    a.apply_action(Action::SelectNext);
    a.apply_action(Action::DeclareScale);

    match a.overlay() {
        Some(ActionResult::DeclarePreview {
            change_ref,
            full_manifest,
        }) => {
            assert!(!change_ref.is_empty(), "a change ref was produced");
            // The FULL manifest is present — the whole selected pod spec,
            // carried through the lowering (never a targeted patch).
            assert!(
                full_manifest.contains("flux-helm-values"),
                "manifest carries the DECLARE rail",
            );
            assert!(
                full_manifest.contains("spec"),
                "manifest carries the whole spec block (full-manifest, not a patch)",
            );
        }
        other => panic!("expected a DECLARE full-manifest preview, got {other:?}"),
    }
}

/// The `l` key produces an OBSERVE read (logs), mutating nothing.
#[test]
fn observe_key_produces_a_read_result() {
    let mut a = app();
    a.apply_action(Action::ObserveLogs);
    match a.overlay() {
        Some(ActionResult::Observed { lines, .. }) => {
            assert!(!lines.is_empty(), "logs read returned lines");
        }
        other => panic!("expected an OBSERVE result, got {other:?}"),
    }
}

/// The `S` (shift+s) key produces a witnessed BREAK-GLASS record — never a
/// live mutation. The witness + runbook are present.
#[test]
fn break_glass_key_produces_a_witnessed_record() {
    let mut a = app();
    a.apply_action(Action::BreakGlass);
    match a.overlay() {
        Some(ActionResult::BreakGlassRecord {
            witness, runbook, ..
        }) => {
            assert_eq!(witness, "drzzln");
            assert!(runbook.contains("RUNBOOK"));
        }
        other => panic!("expected a BREAK-GLASS record, got {other:?}"),
    }
}

/// No dispatch path performs a live mutation. The fixture env exposes only
/// the read + git-declare + witnessed-glass arms; there is no unwitnessed
/// mutate to call (the citizenship invariant). We prove the *observable*
/// consequence: a DECLARE yields a git-change preview and never a
/// BreakGlassRecord, and an OBSERVE yields a read and never either write.
#[test]
fn no_dispatch_path_performs_a_live_mutation() {
    let env = FixtureClusterEnv::new();
    let table = {
        use banken::table::PodTable;
        use banken_spec::env::ClusterEnv;
        use banken_spec::types::ResourceKind;
        PodTable::pods(env.list_resources(ResourceKind::Pod, None).unwrap())
    };
    let op = OperatorId::new("drzzln").expect("a literal witness is non-blank");

    // OBSERVE → a read, never a write of either kind.
    let observe = dispatch(&table, RowAction::ViewLogs, &op, &env);
    assert!(
        matches!(observe, ActionResult::Observed { .. }),
        "OBSERVE is a read",
    );

    // DECLARE → a git-change preview, never a BreakGlassRecord (the only
    // live arm). The selection carries the whole spec (read-modify-write).
    let sel = current_selection(&table).expect("a selected row");
    assert!(
        !sel.current.is_empty(),
        "selection carries the observed spec"
    );
    let declare = dispatch(&table, RowAction::DeclareScale, &op, &env);
    assert!(
        matches!(declare, ActionResult::DeclarePreview { .. }),
        "DECLARE lowers to a git change, never a live mutation",
    );

    // BREAK-GLASS → the one witnessed live arm; still not an *unwitnessed*
    // mutation, and it carries a witness by construction.
    let glass = dispatch(&table, RowAction::BreakGlassShell, &op, &env);
    assert!(
        matches!(glass, ActionResult::BreakGlassRecord { .. }),
        "BREAK-GLASS is witnessed by construction",
    );
}
