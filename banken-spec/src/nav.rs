//! `(defnavkey)` — the navigation keybinding domain.
//!
//! # What this domain closes
//!
//! banken's keymap was **half data, half Rust**. The three `postigo` chords
//! (`l`/`s`/`shift+s`) are authored in `specs/actions.lisp`; the six
//! navigation chords (`down`/`j`, `up`/`k`, `o`, `esc`, `q`) were hardcoded
//! in `banken/src/app.rs`'s `default_keymap()`, with
//! `app_keymap_agrees_with_the_authored_chords` acting as a *drift gate over
//! the hand-list* rather than its elimination — `banken/src/keys.rs` says so
//! itself under `pending-banken: keymap-derived-from-catalog`.
//!
//! Two consequences of that split, both real:
//!
//! 1. **One keyboard, two unchecked namespaces.** A nav chord and a postigo
//!    chord could collide, and the collision resolved by *whichever
//!    `km.bind` ran last* — silently, in the exact tool whose entire point
//!    is that the operator knows which gate a keystroke crosses.
//!    [`crate::resolve::Catalog::resolve`] now conflict-checks both domains
//!    against one namespace.
//! 2. **Half the runtime-manipulation surface was unauthorable.** The
//!    operator asked for the configuration *and* runtime-manipulation
//!    domains as `(def…)` forms; the keys that drive the runtime were the
//!    half that was still Rust.
//!
//! # Why a nav key is NOT a `(defk8saction)`
//!
//! A `(defk8saction)` carries an [`crate::types::ActionLegality`] — it
//! crosses the postigo gate. A nav key mutates **local UI state only**
//! (which row the cursor sits on, which way the sort points). Giving it a
//! legality class would mean typing "move the cursor" as `Observe`, which
//! reads as "this performed a cluster read" — a small lie that makes the
//! legality class stop meaning anything. The two domains stay separate and
//! share exactly one thing: the chord namespace, which is checked.
//!
//! [`NavKeySpec::as_chord_claim`] exists **only** to reuse the one conflict
//! checker; the projection it produces is never dispatched.

use serde::{Deserialize, Serialize};
use tatara_lisp::DeriveTataraDomain;

use crate::{
    chord::ActionChord,
    closed_catalog,
    types::{ActionLegality, K8sActionSpec, ManifestScope},
};

closed_catalog! {
    /// What a navigation key does to local UI state.
    ///
    /// Closed and small: an intent the app cannot perform has no business
    /// being authorable, and an authored intent the app ignores would be a
    /// silently dead key. The app's `From<NavIntent>` projection is
    /// exhaustive, so adding a variant here is a compile error there until
    /// it is handled.
    #[serde(rename_all = "kebab-case")]
    pub enum NavIntent {
        /// Move the selection one row down.
        SelectNext => "select-next",
        /// Move the selection one row up.
        SelectPrev => "select-prev",
        /// Cycle the sort direction on the active column.
        ToggleSort => "toggle-sort",
        /// Dismiss the action-result overlay.
        Dismiss => "dismiss",
        /// Confirm the previewed action in the current overlay.
        ///
        /// Today the app has exactly one confirmable overlay — a
        /// [`crate::bancada::SessionPlan`] awaiting the operator's go-ahead —
        /// and everywhere else this is a no-op. It is authored as an
        /// *intent* rather than as a `(defk8saction)` for the same reason
        /// every other nav key is: confirming a preview mutates local UI
        /// state, and the thing it then performs carries its own DERIVED
        /// `postigo` class. Typing the confirm key itself as `Observe` would
        /// misdescribe the BREAK-GLASS case.
        Confirm => "confirm",
        /// Open the help page — the authored vocabulary, rendered.
        ///
        /// An *intent*, not a `(defk8saction)`, for the usual reason: it
        /// touches local UI state and reads nothing from a cluster, so it
        /// carries no `postigo` class. What makes it worth a variant rather
        /// than a hardcoded chord in the app is that the page it opens is
        /// itself derived from this catalog — so the key that shows the
        /// vocabulary is part of the vocabulary it shows.
        Help => "help",
        /// Quit banken.
        Quit => "quit",
    }
}

/// One authored navigation keybinding.
#[derive(DeriveTataraDomain, Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[tatara(keyword = "defnavkey")]
pub struct NavKeySpec {
    /// The binding's name — a join key, so it must be unique.
    ///
    /// Two bindings may share an *intent* (`down` and `j` both select the
    /// next row, the k9s/vi pair), which is why the name and the intent are
    /// separate fields.
    pub name: String,
    /// The chord, typed through `awase::Hotkey` — an unparseable chord has
    /// no value (see [`crate::chord`]).
    pub keys: ActionChord,
    /// What pressing it does.
    pub intent: NavIntent,
}

impl NavKeySpec {
    /// Project this nav key onto the [`K8sActionSpec`] shape **purely so the
    /// one chord-conflict checker can see both keyed domains in a single
    /// namespace**.
    ///
    /// This is not a legality assignment and the result is never dispatched:
    /// [`crate::resolve::Catalog::resolve`] feeds it to
    /// [`crate::bindings::build_binding_map`] and drops it. The `Observe`
    /// arm is the only legality that mutates nothing at all, which makes it
    /// the honest filler for a value whose legality is *not applicable* —
    /// and the alternative (a second parallel conflict checker over a second
    /// chord map) is the duplication that would let the two namespaces drift
    /// apart again.
    #[must_use]
    pub fn as_chord_claim(&self) -> K8sActionSpec {
        K8sActionSpec {
            name: self.name.clone(),
            keys: self.keys,
            legality: ActionLegality::Observe,
            manifest_scope: ManifestScope::Full,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_intent_has_a_label_and_a_wire_name_that_agree() {
        assert_eq!(NavIntent::ALL.len(), 7);
        for i in NavIntent::ALL {
            let wire = serde_json::to_string(i).expect("serializes");
            let mut expected = String::from("\"");
            expected.push_str(i.label());
            expected.push('"');
            assert_eq!(wire, expected, "label and wire name must agree");
        }
    }

    #[test]
    fn two_bindings_may_share_one_intent() {
        // The k9s/vi pair: `down` and `j` both select the next row. This is
        // why name and intent are distinct fields.
        let a = NavKeySpec {
            name: "select-next-arrow".into(),
            keys: ActionChord::parse("down").expect("awase parses `down`"),
            intent: NavIntent::SelectNext,
        };
        let b = NavKeySpec {
            name: "select-next-vi".into(),
            keys: ActionChord::parse("j").expect("chord"),
            intent: NavIntent::SelectNext,
        };
        assert_eq!(a.intent, b.intent);
        assert_ne!(a.keys, b.keys);
    }

    #[test]
    fn the_chord_claim_carries_the_chord_and_nothing_meaningful_else() {
        let n = NavKeySpec {
            name: "quit".into(),
            keys: ActionChord::parse("q").expect("chord"),
            intent: NavIntent::Quit,
        };
        let claim = n.as_chord_claim();
        assert_eq!(claim.keys, n.keys);
        assert_eq!(claim.name, "quit");
        // The projection is a chord claim, not a legality assignment — it is
        // fed to the conflict checker and dropped.
        assert_eq!(claim.legality, ActionLegality::Observe);
    }
}
