//! Errors produced while loading or interpreting banken specs.
//!
//! Mirrors sui-spec's `SpecError` shape exactly (the canonical
//! TYPED-SPEC + INTERPRETER pattern): a typed `Interp { phase }` for
//! every unimplemented or failing interpreter surface, never a
//! `todo!()`/`unimplemented!()`/`panic!()` in a production path. A
//! silent wrong `Ok` is forbidden — an interpreter gap surfaces
//! mechanically as a typed error the consumer can see.

use thiserror::Error;

/// Every failure the postigo triplet can produce.
#[derive(Error, Debug)]
pub enum SpecError {
    /// The Lisp source failed to read, macroexpand, or compile under
    /// a domain's schema.
    #[error("spec load error: {0}")]
    Load(String),

    /// A typed border failed to compile from its authored form.
    #[error("spec compile error: {0}")]
    Compile(String),

    /// An interpreter phase failed. `phase` names the exact step so
    /// a consumer sees the gap without guessing.
    #[error("interpreter error in {phase}: {message}")]
    Interp { phase: String, message: String },

    /// A DECLARE action could not be lowered to a full manifest —
    /// e.g. the selection has no owning `release.yaml` (the
    /// §IX C-declare-coverage gap). Distinct from `Interp` so
    /// callers can offer break-glass or a reviewed rail instead.
    #[error("no DECLARE lowering target: {0}")]
    NoLoweringTarget(String),

    /// Two `(defk8saction)` forms claim the SAME key chord.
    ///
    /// This is an **error, never last-write-wins**: silently keeping the
    /// last binding is how an operator ends up with a chord that fires a
    /// different legality class than the one the legend advertises — the
    /// worst possible failure for a tool whose whole point is that the
    /// operator knows which gate a keystroke crosses.
    #[error(
        "key chord `{chord}` is claimed by both `{existing}` and `{incoming}` \
         — two actions cannot share one chord ({hint})"
    )]
    ChordConflict {
        /// The canonical chord both actions claim.
        chord: String,
        /// The action that claimed it first.
        existing: String,
        /// The action that collided with it.
        incoming: String,
        /// The named fix (usually: author the uppercase form as `shift+…`).
        hint: &'static str,
    },

    /// An awase binding surface was rejected for a reason other than a
    /// straight duplicate — e.g. `awase::detect_conflicts` reported a
    /// chord-leader colliding with a plain binding.
    #[error("keybinding error: {0}")]
    Binding(String),
}
