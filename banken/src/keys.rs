//! The awase↔egaku-term chord bridge.
//!
//! banken has two keybinding vocabularies in play and they are **not** the
//! same type:
//!
//! - `banken_spec::chord::ActionChord` (wrapping `awase::Hotkey`) is the
//!   **authored** chord on a `(defk8saction)` — the typed border, the thing
//!   `bindings::build_binding_map` conflict-checks.
//! - `egaku_term::__re::KeyCombo` is the **delivered** chord — what the
//!   terminal event layer builds from a crossterm `KeyEvent`.
//!
//! They agree on the important things — both lowercase a letter and convey
//! uppercase as a separate `shift` modifier (`egaku-term-0.3.1/src/event.rs`
//! `key_name` + `modifier_names`; `awase/src/hotkey.rs` `Key: Display`) —
//! which is exactly why the authored form is `shift+s` and not `S`.
//!
//! [`chord_to_combo`] is the total-where-it-can-be, `None`-where-it-cannot
//! projection between them. It deliberately covers only what the two
//! vocabularies provably agree on (letters, digits, F-keys) and returns
//! `None` for everything else rather than guessing: the two DO diverge on
//! named keys (awase `Escape`/`Return` vs egaku-term `esc`/`enter`), and a
//! silent wrong mapping would make the drift gate in `app.rs` pass while
//! comparing the wrong chords.
//!
//! `pending-banken: keymap-derived-from-catalog` — the app's
//! `default_keymap` still hand-binds its three postigo chords next to the
//! navigation keys, and `app_keymap_agrees_with_the_authored_chords` is the
//! *drift gate* over that hand-list rather than its elimination. Deriving
//! the postigo half straight from `load_actions()` needs `BankenApp::new`
//! to become fallible (a `Result`), since a spec-load failure must not
//! silently fall back to hardcoded chords. That is a deliberate next step,
//! not a claim made here.

use awase::{Key, Modifiers};
use banken_spec::chord::ActionChord;
use egaku_term::__re::KeyCombo;

/// Project an authored [`ActionChord`] onto the `KeyCombo` the terminal
/// event layer would deliver for it.
///
/// Returns `None` when the chord uses a key or modifier the two
/// vocabularies do not provably agree on — never a guessed mapping.
///
/// F13–F20 are refused deliberately: awase declares them, crossterm's
/// `KeyCode::F(n)` renders them as `f13`…`f20` and egaku-term passes that
/// through, but no terminal in the fleet has been observed delivering
/// them, so they stay outside the *proven*-agreement set.
#[must_use]
pub fn chord_to_combo(chord: ActionChord) -> Option<KeyCombo> {
    let hotkey = chord.hotkey();
    let name = key_name(hotkey.key)?;
    let mods = modifier_names(hotkey.modifiers)?;
    Some(KeyCombo::new(&name, mods))
}

/// The egaku-term key name for an awase key, for the subset where the two
/// vocabularies provably agree.
fn key_name(key: Key) -> Option<String> {
    // Letters, digits and F-keys: awase's `Key: Display` and egaku-term's
    // `key_name` produce byte-identical strings ("s", "0", "f5"), so the
    // Display form is the mapping. Everything else diverges (awase
    // "escape"/"return" vs egaku-term "esc"/"enter") and is refused.
    // Written as an explicit variant list rather than a range: awase's
    // `Key` is a plain enum (no range patterns), and spelling the agreeing
    // set out means a future awase variant lands in the `false` arm — the
    // safe side — instead of being swept into a range.
    let agrees = matches!(
        key,
        // Letters
        Key::A
            | Key::B
            | Key::C
            | Key::D
            | Key::E
            | Key::F
            | Key::G
            | Key::H
            | Key::I
            | Key::J
            | Key::K
            | Key::L
            | Key::M
            | Key::N
            | Key::O
            | Key::P
            | Key::Q
            | Key::R
            | Key::S
            | Key::T
            | Key::U
            | Key::V
            | Key::W
            | Key::X
            | Key::Y
            | Key::Z
            // Digits
            | Key::Num0
            | Key::Num1
            | Key::Num2
            | Key::Num3
            | Key::Num4
            | Key::Num5
            | Key::Num6
            | Key::Num7
            | Key::Num8
            | Key::Num9
            // Function keys
            | Key::F1
            | Key::F2
            | Key::F3
            | Key::F4
            | Key::F5
            | Key::F6
            | Key::F7
            | Key::F8
            | Key::F9
            | Key::F10
            | Key::F11
            | Key::F12
    );
    if agrees { Some(key.to_string()) } else { None }
}

/// The egaku-term modifier names for an awase modifier set.
///
/// `None` when a modifier has no egaku-term counterpart — `FN` and
/// `CAPS_LOCK` are real awase modifiers that egaku-term's
/// `modifier_names` cannot express, and dropping them silently would turn
/// `fn+s` into plain `s`.
fn modifier_names(mods: Modifiers) -> Option<Vec<String>> {
    if mods.contains(Modifiers::FN) || mods.contains(Modifiers::CAPS_LOCK) {
        return None;
    }
    let mut out = Vec::new();
    if mods.contains(Modifiers::CTRL) {
        out.push("ctrl".to_owned());
    }
    if mods.contains(Modifiers::ALT) {
        out.push("alt".to_owned());
    }
    if mods.contains(Modifiers::SHIFT) {
        out.push("shift".to_owned());
    }
    // awase CMD ⇔ crossterm SUPER ⇔ egaku-term "super".
    if mods.contains(Modifiers::CMD) {
        out.push("super".to_owned());
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plain_letter_maps_to_a_bare_combo() {
        let combo = chord_to_combo(ActionChord::plain(Key::L)).expect("maps");
        assert_eq!(combo, KeyCombo::key("l"));
    }

    #[test]
    fn a_shifted_letter_maps_to_the_shift_modifier_combo() {
        let combo = chord_to_combo(ActionChord::shifted(Key::S)).expect("maps");
        assert_eq!(combo, KeyCombo::new("s", vec!["shift".to_owned()]));
        // And crucially it is NOT the bare `s` combo — the case-only
        // collision that motivated the typed chord in the first place.
        assert_ne!(combo, KeyCombo::key("s"));
    }

    #[test]
    fn ctrl_and_cmd_map_to_the_crossterm_names() {
        let ctrl_k = ActionChord::parse("ctrl+k").expect("parses");
        assert_eq!(
            chord_to_combo(ctrl_k).expect("maps"),
            KeyCombo::new("k", vec!["ctrl".to_owned()])
        );
        let cmd_k = ActionChord::parse("cmd+k").expect("parses");
        assert_eq!(
            chord_to_combo(cmd_k).expect("maps"),
            KeyCombo::new("k", vec!["super".to_owned()])
        );
    }

    /// A divergent key is REFUSED, not guessed. awase spells it "escape",
    /// egaku-term spells it "esc" — mapping one to the other by Display
    /// would silently produce a combo the event layer never delivers.
    #[test]
    fn a_divergent_key_name_is_refused() {
        let esc = ActionChord::parse("escape").expect("awase parses `escape`");
        assert_eq!(esc.hotkey().key, Key::Escape);
        assert!(
            chord_to_combo(esc).is_none(),
            "awase `escape` vs egaku-term `esc` — refuse rather than guess"
        );
    }

    /// A modifier egaku-term cannot express is REFUSED, not dropped —
    /// dropping `fn` would turn `fn+s` into the plain `s` chord.
    #[test]
    fn an_inexpressible_modifier_is_refused() {
        let fn_s = ActionChord::parse("fn+s").expect("awase parses `fn+s`");
        assert!(chord_to_combo(fn_s).is_none());
    }
}
