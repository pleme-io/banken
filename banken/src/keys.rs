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
//! projection between them. It covers three classes and refuses everything
//! else rather than guessing:
//!
//! 1. **Agree by Display** — letters, digits, F1–F12, and the eight
//!    navigation/editing keys whose awase `Key: Display` string is
//!    byte-identical to egaku-term's `key_name` (`up`, `down`, `left`,
//!    `right`, `home`, `end`, `pageup`, `pagedown`, `tab`, `backspace`,
//!    `delete`).
//! 2. **Diverge, with a MEASURED translation** — exactly two:
//!    awase `escape` → egaku-term `esc`, awase `return` → egaku-term
//!    `enter`. Both verified against source
//!    (`awase-0.1.1/src/hotkey.rs:347,346` vs
//!    `egaku-term-0.3.1/src/event.rs:43,42`), not inferred.
//! 3. **Refused** — everything else, including `space` (awase spells it
//!    `space`; crossterm delivers it as `KeyCode::Char(' ')`, so
//!    egaku-term's name is a literal space character) and the `fn`/
//!    `caps_lock` modifiers egaku-term cannot express. A silent wrong
//!    mapping would make the drift gate in `app.rs` pass while comparing the
//!    wrong chords, which is worse than a refusal.
//!
//! Class 2 is what makes the whole keymap derivable from the authored
//! catalog: `(defnavkey :keys "escape")` could not otherwise reach the
//! runtime. `pending-banken: keymap-derived-from-catalog` is CLOSED —
//! [`crate::app::keymap_from_catalog`] builds the app keymap from
//! `banken_spec::load_catalog()`, and `BankenApp::try_new` is fallible
//! precisely so a spec-load failure cannot silently fall back to hardcoded
//! chords.

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

/// The two keys where the vocabularies DIVERGE and the translation is
/// measured rather than inferred.
///
/// Verified 2026-07-30 against the vendored sources:
/// `awase-0.1.1/src/hotkey.rs:347` (`Self::Escape => "escape"`) and `:346`
/// (`Self::Return => "return"`) vs `egaku-term-0.3.1/src/event.rs:43`
/// (`KeyCode::Esc => "esc"`) and `:42` (`KeyCode::Enter => "enter"`).
///
/// This table is deliberately TINY and each row cites its evidence. A key
/// added here without reading both sides would reintroduce exactly the
/// silent-wrong-mapping class the refusal path exists to prevent.
const TRANSLATED: &[(Key, &str)] = &[(Key::Escape, "esc"), (Key::Return, "enter")];

/// The egaku-term key name for an awase key, for the subset where the two
/// vocabularies provably agree or where the divergence is measured.
fn key_name(key: Key) -> Option<String> {
    // Class 2 first: a measured translation wins over the Display form.
    if let Some((_, name)) = TRANSLATED.iter().find(|(k, _)| *k == key) {
        return Some((*name).to_owned());
    }

    // Class 1 — letters, digits, F-keys and the navigation/editing keys
    // whose awase `Key: Display` string is byte-identical to egaku-term's
    // `key_name`, so the Display form IS the mapping.
    //
    // Written as an explicit variant list rather than a range: awase's
    // `Key` is a plain enum (no range patterns), and spelling the agreeing
    // set out means a future awase variant lands in the `false` arm — the
    // safe side — instead of being swept into a range.
    //
    // `Key::Space` is deliberately ABSENT: awase spells it `space`, but
    // crossterm delivers the spacebar as `KeyCode::Char(' ')`, so
    // egaku-term's name for it is a literal space character. That is a
    // divergence with no safe translation (a one-char name collides with
    // nothing but reads as an empty binding), so it stays refused.
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
            // Navigation + editing keys whose Display string is
            // byte-identical on both sides (awase hotkey.rs:353-360 vs
            // egaku-term event.rs:44-56).
            | Key::Up
            | Key::Down
            | Key::Left
            | Key::Right
            | Key::Home
            | Key::End
            | Key::PageUp
            | Key::PageDown
            | Key::Tab
            | Key::Backspace
            | Key::Delete
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

    /// The two MEASURED translations. awase spells them `escape`/`return`,
    /// egaku-term spells them `esc`/`enter`; each row of [`TRANSLATED`]
    /// cites the source line it was read from. Mapping by Display equality
    /// would have produced a combo the event layer never delivers.
    #[test]
    fn a_divergent_key_name_uses_the_measured_translation() {
        let esc = ActionChord::parse("escape").expect("awase parses `escape`");
        assert_eq!(esc.hotkey().key, Key::Escape);
        assert_eq!(
            chord_to_combo(esc).expect("escape translates"),
            KeyCombo::key("esc"),
            "awase `escape` → egaku-term `esc` (measured, not guessed)"
        );

        let ret = ActionChord::parse("return").expect("awase parses `return`");
        assert_eq!(
            chord_to_combo(ret).expect("return translates"),
            KeyCombo::key("enter"),
        );

        // And the translation is NOT the Display form — proving the table is
        // load-bearing rather than decorative.
        assert_ne!(chord_to_combo(esc), Some(KeyCombo::key("escape")));
    }

    /// The navigation keys that agree by Display go through unchanged — this
    /// is what lets `(defnavkey :keys "down")` reach the runtime.
    #[test]
    fn navigation_keys_that_agree_map_by_display() {
        for (authored, expected) in [
            ("down", "down"),
            ("up", "up"),
            ("left", "left"),
            ("right", "right"),
            ("pageup", "pageup"),
            ("pagedown", "pagedown"),
            ("home", "home"),
            ("end", "end"),
            ("tab", "tab"),
            ("backspace", "backspace"),
            ("delete", "delete"),
        ] {
            let chord = ActionChord::parse(authored)
                .unwrap_or_else(|e| panic!("awase parses `{authored}`: {e}"));
            assert_eq!(
                chord_to_combo(chord).unwrap_or_else(|| panic!("`{authored}` must map")),
                KeyCombo::key(expected),
            );
        }
    }

    /// **THE REFUSAL still bites.** `space` diverges with no safe
    /// translation (awase `space` vs crossterm `Char(' ')`), so it stays
    /// refused rather than being swept in with the navigation keys.
    #[test]
    fn space_remains_refused_because_its_divergence_has_no_safe_translation() {
        let space = ActionChord::parse("space").expect("awase parses `space`");
        assert_eq!(space.hotkey().key, Key::Space);
        assert!(
            chord_to_combo(space).is_none(),
            "awase `space` vs egaku-term's literal ' ' — refuse rather than guess"
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
