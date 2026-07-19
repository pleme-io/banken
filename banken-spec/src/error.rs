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
}
