//! [`ActionChord`] — the typed keybinding on a `(defk8saction)`.
//!
//! # What this closes
//!
//! `theory/BANKEN.md` §III.a declares `K8sActionSpec { pub keys: KeyChord }`
//! and §II's reuse map rows the hotkey layer as
//! *"awase `BindingMap`/`KeyChord` + `detect_conflicts` + `KeyRepeatGate`
//! — **SHIPPED**"*. Until now `keys` was a bare `String` and banken
//! consumed **zero** awase. That over-claim is now closed: the field is a
//! typed chord parsed through `awase::Hotkey`, so an unparseable chord is
//! rejected when the `(defk8saction)` form compiles rather than discovered
//! at keypress time.
//!
//! # Correction to §III.a: the right awase type is `Hotkey`, not `KeyChord`
//!
//! `awase::KeyChord` (`awase/src/chord.rs:11-21`) is a **two-step
//! leader→follower** chord: `{ leader: Hotkey, follower: Hotkey,
//! timeout_ms: u32, action: Action }`. banken's actions are single
//! keypresses (`s`, `l`, `shift+s`), and the type awase indexes bindings
//! by is `Hotkey` — `KeyMode.bindings: HashMap<Hotkey, Binding>`
//! (`awase/src/mode.rs:20`). Naming `KeyChord` in §III.a was a type error,
//! not a deliberate design; a one-key action wrapped in a leader/follower
//! chord would need a bogus second key and a timeout that never applies.
//! [`ActionChord`] therefore wraps `Hotkey`, and banken keeps
//! `awase::KeyChord` available for the day it grows a real leader chord.
//!
//! # Why an uppercase letter must be authored as `shift+<letter>`
//!
//! `Hotkey::parse` is documented case-insensitive
//! (`awase/src/hotkey.rs:435`) — `"s"` and `"S"` both resolve to
//! `Hotkey { NONE, Key::S }`. So k9s's `s` (scale) vs `S` (shell) are the
//! SAME typed chord unless the shift modifier is spelled out. That is not
//! a wart to route around: egaku-term already delivers a held-shift
//! letter as `shift+s` (see `banken/src/app.rs`'s `default_keymap`), so
//! `shift+s` is what the runtime actually produces and `"S"` in the
//! authored Lisp was the value that disagreed with it.
//! [`ActionChord::SHIFT_HINT`] is surfaced in the conflict error so the
//! fix is named at the failure site.

use std::fmt;

use awase::{Hotkey, Key, Modifiers};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// The typed key chord that triggers one `(defk8saction)`.
///
/// Authored as a string in Lisp/YAML (`:keys "shift+s"`) and parsed
/// through `awase::Hotkey::parse` at compile/deserialize time, so an
/// unparseable chord has no typed value — **parse-time-rejected**, not a
/// runtime `Result::Err` on the keypress path.
///
/// The wire form is the canonical `awase::Hotkey` display string, so a
/// round-trip normalizes `"S"` → `"s"` and `"SHIFT+S"` → `"shift+s"`.
/// (No `Ord`: `awase::Hotkey` is deliberately not ordered — its
/// `Modifiers` is a bitmask, so any ordering would be arbitrary. Where a
/// stable order is needed, key on [`ActionChord::canonical`].)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ActionChord(Hotkey);

impl ActionChord {
    /// The hint appended to a conflict error — the one fix that resolves
    /// the overwhelmingly common cause (a case-only distinction).
    pub const SHIFT_HINT: &'static str = "awase::Hotkey::parse is case-insensitive, so \"s\" and \"S\" are the SAME chord; \
         author an uppercase letter as \"shift+s\" (which is also what egaku-term delivers)";

    /// Parse an authored chord string.
    ///
    /// # Errors
    ///
    /// Returns the awase parse error message when `s` is not a valid
    /// hotkey (empty, multi-key, or an unknown key name).
    pub fn parse(s: &str) -> Result<Self, String> {
        Hotkey::parse(s).map(Self).map_err(|e| e.to_string())
    }

    /// Build from an already-typed hotkey.
    #[must_use]
    pub const fn from_hotkey(hotkey: Hotkey) -> Self {
        Self(hotkey)
    }

    /// A bare, unmodified letter/number key chord.
    #[must_use]
    pub const fn plain(key: Key) -> Self {
        Self(Hotkey::new(Modifiers::NONE, key))
    }

    /// A shifted key chord — the typed form of an "uppercase letter"
    /// binding.
    #[must_use]
    pub const fn shifted(key: Key) -> Self {
        Self(Hotkey::new(Modifiers::SHIFT, key))
    }

    /// The underlying awase hotkey — what `BindingMap` indexes on.
    #[must_use]
    pub const fn hotkey(self) -> Hotkey {
        self.0
    }

    /// The canonical display string (`"s"`, `"shift+s"`).
    #[must_use]
    pub fn canonical(self) -> String {
        self.0.display()
    }
}

impl fmt::Display for ActionChord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

impl Serialize for ActionChord {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.canonical())
    }
}

impl<'de> Deserialize<'de> for ActionChord {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(d)?;
        Self::parse(&raw).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_plain_letter() {
        let c = ActionChord::parse("s").expect("parses");
        assert_eq!(c, ActionChord::plain(Key::S));
        assert_eq!(c.canonical(), "s");
    }

    #[test]
    fn parses_a_shifted_letter() {
        let c = ActionChord::parse("shift+s").expect("parses");
        assert_eq!(c, ActionChord::shifted(Key::S));
        assert_eq!(c.canonical(), "shift+s");
    }

    /// THE fact that forces `shift+s` in the authored Lisp: awase folds
    /// case, so `"s"` and `"S"` are ONE chord. k9s's scale-vs-shell pair
    /// is only distinguishable with the modifier spelled out.
    #[test]
    fn uppercase_and_lowercase_are_the_same_chord() {
        assert_eq!(
            ActionChord::parse("S").expect("parses"),
            ActionChord::parse("s").expect("parses"),
            "awase::Hotkey::parse is case-insensitive — this is why an \
             uppercase binding must be authored as shift+<letter>",
        );
        assert_ne!(
            ActionChord::parse("shift+s").expect("parses"),
            ActionChord::parse("s").expect("parses"),
            "the shifted form IS distinct",
        );
    }

    #[test]
    fn an_unparseable_chord_has_no_typed_value() {
        assert!(ActionChord::parse("").is_err(), "empty");
        assert!(ActionChord::parse("nosuchkey").is_err(), "unknown key");
        assert!(ActionChord::parse("a+b").is_err(), "two keys");
    }

    #[test]
    fn serde_round_trips_and_normalizes() {
        for (authored, canonical) in [
            ("s", "s"),
            ("S", "s"),
            ("shift+s", "shift+s"),
            ("SHIFT+S", "shift+s"),
            ("ctrl+k", "ctrl+k"),
        ] {
            let c: ActionChord = serde_json::from_str(&{
                let mut j = String::from("\"");
                j.push_str(authored);
                j.push('"');
                j
            })
            .unwrap_or_else(|e| panic!("{authored} should deserialize: {e}"));
            assert_eq!(c.canonical(), canonical, "authored {authored}");
            let back = serde_json::to_string(&c).expect("serialize");
            let mut expected = String::from("\"");
            expected.push_str(canonical);
            expected.push('"');
            assert_eq!(back, expected);
            // Canonical form re-parses to the same value (idempotent).
            assert_eq!(ActionChord::parse(canonical).expect("re-parses"), c);
        }
    }

    #[test]
    fn deserialize_rejects_an_unparseable_chord() {
        let err = serde_json::from_str::<ActionChord>("\"nosuchkey\"")
            .expect_err("an unknown key must be rejected at deserialize time");
        assert!(
            err.to_string().contains("nosuchkey"),
            "the error should name the offending chord, got: {err}"
        );
    }
}
