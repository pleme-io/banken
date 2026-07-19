//! banken-spec — the `postigo` action-legality citizenship primitive.
//!
//! This crate is the TYPED-SPEC + INTERPRETER triplet that makes banken
//! (the pleme-io-native k9s) a *citizen, not a clone* (BANKEN.md §III).
//! It copies sui-spec's verified `GcEnvironment`/`apply<E>` shape
//! exactly — but where `GcEnvironment` **has** `delete_path` (gc.rs:148),
//! the [`env::ClusterEnv`] trait **deliberately omits the
//! unwitnessed-mutate analogue**. That omission is the whole primitive.
//!
//! ## The three artifacts (per ★★ TYPED-SPEC + INTERPRETER TRIPLET)
//!
//! 1. **Typed Rust border** — [`types`]: [`types::ActionLegality`]
//!    (OBSERVE / DECLARE / BREAK-GLASS), [`types::DeclareTarget`] (five
//!    GitOps rails, **no `Kubectl`/`Apply` arm**),
//!    [`types::K8sViewSpec`] / [`types::K8sActionSpec`] (the two authored
//!    borders, `#[derive(DeriveTataraDomain)]`).
//! 2. **Authored Lisp spec** — `specs/views.lisp` + `specs/actions.lisp`:
//!    `(defk8sview …)` / `(defk8saction …)` forms declaring the canonical
//!    views + actions as Lisp data. A live-mutate plugin is a parse-time
//!    domain error (`DeclareTarget` has no live arm to author into).
//! 3. **Working interpreter** — [`interp::apply`]: dispatches on
//!    `legality`, lowering a DECLARE to a **full** manifest via
//!    [`interp::lower_to_full_manifest`] (serde, never `format!()`) then
//!    [`env::ClusterEnv::declare`]; routing BREAK-GLASS to
//!    [`env::ClusterEnv::break_glass`]. Proven against
//!    [`testing::MockClusterEnv`] — **zero live cluster**.
//!
//! ## The unrepresentability tier (never rounded up — BANKEN.md §III.d)
//!
//! - A DECLARE keystroke calling live-scale/delete: **truly-unrepresentable
//!   within the authored `ClusterEnv` surface** — the trait has no such
//!   method, calling one is `E0599`.
//! - An author *re-adding* a `fn mutate()` to `ClusterEnv`:
//!   **only-mitigated → CI-caught** by the `tests/substrate_invariant.rs`
//!   guard. Not truly-unrepresentable — do not claim otherwise.
//! - A plugin authoring a live-mutate action: **parse-time-rejected**
//!   (no live `DeclareTarget` arm).

pub mod catalog;
pub mod env;
pub mod error;
pub mod interp;
pub mod loader;
pub mod testing;
pub mod types;

pub use error::SpecError;

use crate::types::{K8sActionSpec, K8sViewSpec};

/// The canonical authored views.
pub const CANONICAL_VIEWS_LISP: &str = include_str!("../specs/views.lisp");
/// The canonical authored actions.
pub const CANONICAL_ACTIONS_LISP: &str = include_str!("../specs/actions.lisp");

/// Compile every authored `(defk8sview)` form.
///
/// # Errors
///
/// Returns a `SpecError::Compile` if the Lisp source fails to parse.
pub fn load_views() -> Result<Vec<K8sViewSpec>, SpecError> {
    loader::load_all::<K8sViewSpec>(CANONICAL_VIEWS_LISP)
}

/// Compile every authored `(defk8saction)` form.
///
/// # Errors
///
/// Returns a `SpecError::Compile` if the Lisp source fails to parse.
pub fn load_actions() -> Result<Vec<K8sActionSpec>, SpecError> {
    loader::load_all::<K8sActionSpec>(CANONICAL_ACTIONS_LISP)
}
