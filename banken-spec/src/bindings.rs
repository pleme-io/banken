//! The awase binding surface for banken's authored actions.
//!
//! Turns a set of `(defk8saction)` specs into an `awase::BindingMap`, and
//! makes **two actions on one chord an ERROR rather than last-write-wins**.
//!
//! # Where the duplicate is actually caught (read before trusting a name)
//!
//! `awase::detect_conflicts` does **not** catch this class in normal use,
//! and says so itself (`awase/src/conflict.rs:32-37`):
//!
//! > *"Same hotkey bound twice in the same mode (only possible with
//! > external config merging — `HashMap` insert deduplicates in normal
//! > use)"*
//!
//! `KeyMode::add_binding` is `self.bindings.insert(...)`
//! (`awase/src/mode.rs:44-46`), so by the time `detect_conflicts` runs the
//! duplicate has already been silently overwritten. The catch therefore
//! happens **at insert**, on `add_binding`'s returned previous binding —
//! that is the load-bearing check here. `detect_conflicts` is still run,
//! because it *does* own a class this loop cannot see (a chord leader
//! colliding with a plain binding), and folding its report in means the
//! day banken grows a real `awase::KeyChord` leader the guard is already
//! wired.
//!
//! # Honest tier
//!
//! **eval/parse-time-rejected, not truly-unrepresentable.** Nothing in the
//! type system prevents authoring two `(defk8saction)` forms with the same
//! `:keys`; [`build_binding_map`] rejects the pair when the surface is
//! built, and [`crate::load_actions`] is not required to be called. The
//! `authored_actions_have_no_chord_conflict` test is what makes the
//! *shipped* catalog's freedom from conflicts a build-time fact.
//! Do not round this up.

use std::collections::BTreeMap;

use awase::{Action, Binding, BindingMap, KeyMode, detect_conflicts};

use crate::{chord::ActionChord, error::SpecError, types::K8sActionSpec};

/// The single mode banken binds into. banken has no modal layers yet; a
/// named constant keeps the mode string from being repeated as a literal.
pub const BANKEN_MODE: &str = "default";

/// Build the awase binding surface for `actions`.
///
/// Each action becomes an `awase::Binding` whose action is
/// `Action::Command(<action name>)` — **never** `Action::Exec`/`Script`.
/// That is deliberate: `Action::Exec` is documented as "Execute shell
/// command", which would be both a NO-SHELL violation and a live-effect
/// path that bypasses the `postigo` gate entirely. banken's dispatcher
/// resolves the command name back through
/// [`crate::interp::apply`], so every keystroke still crosses the gate.
/// [`assert_no_shell_actions`] pins that.
///
/// # Errors
///
/// - [`SpecError::ChordConflict`] when two actions claim one chord.
/// - [`SpecError::Binding`] when `awase::detect_conflicts` reports a
///   non-clean surface (the chord-leader class).
pub fn build_binding_map(actions: &[K8sActionSpec]) -> Result<BindingMap, SpecError> {
    let mut mode = KeyMode::new(BANKEN_MODE, /* passthrough */ false);

    // Remember which action name claimed each chord so a conflict error can
    // name BOTH sides — `add_binding` only hands back the previous
    // `Binding`, whose `Action::Command` payload is the name we stored.
    let mut claimed: BTreeMap<String, String> = BTreeMap::new();

    for spec in actions {
        let chord = spec.keys;
        let binding = Binding::new(chord.hotkey(), Action::command(spec.name.clone()));
        // *** The load-bearing check: `add_binding` is a HashMap insert, so
        //     the returned Some(prev) IS the duplicate. Dropping it here is
        //     exactly the last-write-wins bug. ***
        if let Some(prev) = mode.add_binding(binding) {
            let existing = claimed
                .get(&chord.canonical())
                .cloned()
                .unwrap_or_else(|| command_label(&prev.action));
            return Err(SpecError::ChordConflict {
                chord: chord.canonical(),
                existing,
                incoming: spec.name.clone(),
                hint: ActionChord::SHIFT_HINT,
            });
        }
        claimed.insert(chord.canonical(), spec.name.clone());
    }

    // The class this loop CANNOT see: a two-step chord leader colliding
    // with a plain binding. banken authors no `awase::KeyChord` yet, so
    // this report is clean today — wiring it now means it stays checked.
    let report = detect_conflicts(&[&mode], &[]);
    if !report.is_clean() {
        let mut msg = String::from("awase detect_conflicts reported ");
        msg.push_str(&report.conflicts.len().to_string());
        msg.push_str(" conflict(s):");
        for c in &report.conflicts {
            msg.push_str("\n  - mode ");
            msg.push_str(&c.mode);
            msg.push_str(" chord ");
            msg.push_str(&c.hotkey.display());
            msg.push_str(": ");
            msg.push_str(&c.existing);
            msg.push_str(" vs ");
            msg.push_str(&c.new);
        }
        return Err(SpecError::Binding(msg));
    }

    let mut map = BindingMap::new();
    map.add_mode(mode);
    map.set_mode(BANKEN_MODE)
        .map_err(|e| SpecError::Binding(e.to_string()))?;
    Ok(map)
}

/// The command name an `awase::Action` carries, for error messages.
fn command_label(action: &Action) -> String {
    match action {
        Action::Command(name) => name.clone(),
        other => {
            let mut s = String::from("<non-command action: ");
            s.push_str(match other {
                Action::Command(_) => "command",
                Action::ModeSwitch(_) => "mode-switch",
                Action::Exec(_) => "exec",
                Action::Script(_) => "script",
                Action::Chain(_) => "chain",
            });
            s.push('>');
            s
        }
    }
}

/// Assert every binding in `map`'s banken mode carries an
/// `Action::Command` — never an `Exec`/`Script`/`Chain`.
///
/// `awase::Action::Exec` is "Execute shell command"; a banken binding
/// holding one would run a live side effect *outside* the `postigo` gate,
/// which is the exact leak `ClusterEnv`'s missing mutate method exists to
/// forbid. Keeping the binding payload a bare *name* means the only way to
/// act on it is to route back through [`crate::interp::apply`].
///
/// # Errors
///
/// Returns [`SpecError::Binding`] naming the first offending chord.
pub fn assert_no_shell_actions(map: &BindingMap) -> Result<(), SpecError> {
    let mode = map
        .mode(BANKEN_MODE)
        .ok_or_else(|| SpecError::Binding("banken mode missing from the binding map".into()))?;
    for (hotkey, binding) in &mode.bindings {
        if !matches!(binding.action, Action::Command(_)) {
            let mut msg = String::from("binding for chord ");
            msg.push_str(&hotkey.display());
            msg.push_str(" carries ");
            msg.push_str(&command_label(&binding.action));
            msg.push_str(
                " — banken bindings must be Action::Command(<action name>) so \
                          dispatch routes back through the postigo gate",
            );
            return Err(SpecError::Binding(msg));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ActionLegality, ManifestScope, OperatorId, RunbookRef};
    use awase::Key;

    fn observe(name: &str, keys: &str) -> K8sActionSpec {
        K8sActionSpec {
            name: name.into(),
            keys: ActionChord::parse(keys).expect("test chord parses"),
            legality: ActionLegality::Observe,
            manifest_scope: ManifestScope::Full,
        }
    }

    #[test]
    fn distinct_chords_build_a_clean_map() {
        let map = build_binding_map(&[
            observe("view-logs", "l"),
            observe("describe", "d"),
            observe("shell", "shift+s"),
        ])
        .expect("distinct chords are clean");
        let mode = map.mode(BANKEN_MODE).expect("mode present");
        assert_eq!(mode.bindings.len(), 3);
        assert!(
            mode.bindings
                .contains_key(&ActionChord::plain(Key::L).hotkey())
        );
        assert!(
            mode.bindings
                .contains_key(&ActionChord::shifted(Key::S).hotkey())
        );
        assert_no_shell_actions(&map).expect("all bindings are Action::Command");
    }

    /// THE GATE. Two actions on one chord is an ERROR, and the error names
    /// BOTH sides so the operator knows which pair to fix.
    #[test]
    fn duplicate_chord_is_an_error_not_last_write_wins() {
        let err = build_binding_map(&[observe("scale", "s"), observe("suspend", "s")])
            .expect_err("a duplicate chord must be rejected");
        match err {
            SpecError::ChordConflict {
                chord,
                existing,
                incoming,
                ..
            } => {
                assert_eq!(chord, "s");
                assert_eq!(existing, "scale");
                assert_eq!(incoming, "suspend");
            }
            other => panic!("expected a ChordConflict, got {other:?}"),
        }
    }

    /// The case-folding trap, caught: authoring `S` next to `s` is a
    /// conflict because awase folds case. This is why the shipped
    /// `actions.lisp` authors the shell action as `shift+s`.
    #[test]
    fn case_only_distinction_is_caught_as_a_conflict() {
        let err = build_binding_map(&[observe("scale", "s"), observe("shell", "S")])
            .expect_err("`s` and `S` are the same awase chord");
        let msg = err.to_string();
        assert!(msg.contains("shift+s"), "the hint names the fix: {msg}");
    }

    #[test]
    fn shift_disambiguates_the_k9s_scale_vs_shell_pair() {
        let map = build_binding_map(&[
            observe("scale", "s"),
            K8sActionSpec {
                name: "shell".into(),
                keys: ActionChord::shifted(Key::S),
                legality: ActionLegality::BreakGlass {
                    witness: OperatorId("drzzln".into()),
                    runbook: RunbookRef("clusters/rio/RUNBOOK.md".into()),
                },
                manifest_scope: ManifestScope::Full,
            },
        ])
        .expect("s and shift+s coexist");
        assert_eq!(map.mode(BANKEN_MODE).expect("mode").bindings.len(), 2);
    }

    /// The authored catalog itself must be conflict-free — this is what
    /// makes "no two banken actions share a chord" a build-time fact
    /// rather than a claim.
    #[test]
    fn authored_actions_have_no_chord_conflict() {
        let actions = crate::load_actions().expect("authored actions load");
        let map = build_binding_map(&actions).expect("the shipped catalog is conflict-free");
        assert_eq!(
            map.mode(BANKEN_MODE).expect("mode").bindings.len(),
            actions.len(),
            "every authored action landed exactly one binding",
        );
        assert_no_shell_actions(&map).expect("no Exec/Script in the shipped catalog");
    }

    #[test]
    fn a_shell_exec_binding_is_rejected() {
        // Constructed directly (an authored form cannot produce this — the
        // builder only ever emits Action::Command), proving the assertion
        // is non-vacuous rather than checking a shape nothing can violate.
        let mut mode = KeyMode::new(BANKEN_MODE, false);
        mode.add_binding(Binding::new(
            ActionChord::plain(Key::X).hotkey(),
            Action::exec("kubectl delete pod"),
        ));
        let mut map = BindingMap::new();
        map.add_mode(mode);
        let err = assert_no_shell_actions(&map).expect_err("an Exec binding must be rejected");
        assert!(err.to_string().contains("exec"), "got: {err}");
    }
}
