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
//!
//! ## The full authored vocabulary (seven domains)
//!
//! The `postigo` triplet above is the *citizenship* core. The rest of
//! banken's behaviour is authored too — both the **configuration** half and
//! the **runtime-manipulation** half — so the tool is data, not a Rust
//! program with a config file:
//!
//! | Form | Owns | Module |
//! |---|---|---|
//! | `(defk8sview …)` | a navigable view: kind, source, columns, sort, drill target | [`types`] |
//! | `(defk8saction …)` | one action + its `postigo` legality class | [`types`] |
//! | `(defpathology …)` | one symptom→cause rule: evidence, severity, remedy rail | [`pathology`] |
//! | `(defward …)` | the health landing: Pulses lanes, the linter set, the headline | [`ward`] |
//! | `(defdrill …)` | a typed drill path (`ctx→ns→pod→container`, `→logs`, `→diagnose`) | [`drill`] |
//! | `(defnavkey …)` | a navigation keybinding + its local-UI intent | [`nav`] |
//! | `(defbancada …)` | a pre-warmed tear/mado troubleshooting session | [`bancada`] |
//!
//! plus `(defbanken …)` in the sibling `banken-config` crate, which owns the
//! deployment face (context, namespace, poll cadence, theme, `spec_dir`) —
//! two faces of one surface, the escriba pattern.
//!
//! **[`load_catalog`] is the entry point.** The six domains reference each
//! other by *name*, and every one of those joins is a silent-failure class;
//! [`Catalog`] cannot be constructed without every reference resolving. See
//! [`resolve`] for the full check table.
//!
//! ## Three seals this crate carries beyond the postigo omission
//!
//! - **The BROKEN-METRIC guard** ([`pathology::WardVerdict`]) — a ward
//!   verdict claiming `Green` over an absent core metric is unconstructible,
//!   and the guard is *structural*, so no catalog row can switch it off.
//! - **`(proven)` is un-authorable** ([`ward::Attestation`]) — BANKEN.md §IX
//!   C-controller's ceiling is a parse-time refusal, not a doc note.
//! - **An unresolved catalog is unconstructible** ([`Catalog`]).
//! - **A pre-warmed session cannot lie about its legality**
//!   ([`bancada::SessionPlan`]) — a `(defbancada)` has no `:legality` kwarg,
//!   the class is derived from its panes, and a mutating pane has no
//!   [`bancada::ObservedCommand`] value with which to reach the unwitnessed
//!   staging arm.
//!
//! Each is **truly-unrepresentable within its authored surface** and each is
//! qualified — none is a fleet-wide guarantee. `pending-banken:
//! strict-kwargs-reader` (the reader that would make an unknown kwarg a
//! parse error rather than a silently-ignored key) is unchanged by the
//! 2026-07-30 move to canonical `tatara-lisp = "0.3.3"`: 0.3.3 ships
//! `parse_kwargs` and no strict variant, so the row is still open.

pub mod bindings;
pub mod catalog;
pub mod chord;
pub mod closed;
pub mod drill;
pub mod env;
pub mod error;
pub mod bancada;
pub mod interp;
pub mod loader;
pub mod nav;
pub mod pathology;
pub mod resolve;
pub mod testing;
pub mod types;
pub mod ward;

pub use error::SpecError;
pub use resolve::Catalog;

use crate::{
    drill::DrillSpec,
    bancada::BancadaSpec,
    nav::NavKeySpec,
    pathology::PathologySpec,
    types::{K8sActionSpec, K8sViewSpec},
    ward::WardSpec,
};

/// The canonical authored views.
pub const CANONICAL_VIEWS_LISP: &str = include_str!("../specs/views.lisp");
/// The canonical authored actions.
pub const CANONICAL_ACTIONS_LISP: &str = include_str!("../specs/actions.lisp");
/// The canonical authored pathology taxonomy.
pub const CANONICAL_PATHOLOGIES_LISP: &str = include_str!("../specs/pathologies.lisp");
/// The canonical authored health wards.
pub const CANONICAL_WARDS_LISP: &str = include_str!("../specs/wards.lisp");
/// The canonical authored drill paths.
pub const CANONICAL_DRILLS_LISP: &str = include_str!("../specs/drills.lisp");
/// The canonical authored navigation keys.
pub const CANONICAL_NAVKEYS_LISP: &str = include_str!("../specs/navkeys.lisp");
/// The canonical authored pre-warmed troubleshooting sessions.
pub const CANONICAL_BANCADAS_LISP: &str = include_str!("../specs/bancadas.lisp");

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

/// Compile every authored `(defpathology)` form.
///
/// # Errors
///
/// Returns a `SpecError::Compile` if the Lisp source fails to parse.
pub fn load_pathologies() -> Result<Vec<PathologySpec>, SpecError> {
    loader::load_all::<PathologySpec>(CANONICAL_PATHOLOGIES_LISP)
}

/// Compile every authored `(defward)` form.
///
/// # Errors
///
/// Returns a `SpecError::Compile` if the Lisp source fails to parse — which
/// includes an attempted `:attested (:kind proven …)`, refused with
/// [`ward::UNATTESTABLE`].
pub fn load_wards() -> Result<Vec<WardSpec>, SpecError> {
    loader::load_all::<WardSpec>(CANONICAL_WARDS_LISP)
}

/// Compile every authored `(defdrill)` form.
///
/// # Errors
///
/// Returns a `SpecError::Compile` if the Lisp source fails to parse.
pub fn load_drills() -> Result<Vec<DrillSpec>, SpecError> {
    loader::load_all::<DrillSpec>(CANONICAL_DRILLS_LISP)
}

/// Compile every authored `(defnavkey)` form.
///
/// # Errors
///
/// Returns a `SpecError::Compile` if the Lisp source fails to parse.
pub fn load_nav_keys() -> Result<Vec<NavKeySpec>, SpecError> {
    loader::load_all::<NavKeySpec>(CANONICAL_NAVKEYS_LISP)
}

/// Compile every authored `(defbancada)` form.
///
/// # Errors
///
/// Returns a `SpecError::Compile` if the Lisp source fails to parse. Note
/// this does **not** validate the recipes' shape or witness obligation —
/// [`load_catalog`] does, via [`bancada::BancadaSpec::validate`].
pub fn load_bancadas() -> Result<Vec<BancadaSpec>, SpecError> {
    loader::load_all::<BancadaSpec>(CANONICAL_BANCADAS_LISP)
}

/// Load and **cross-resolve** the whole shipped vocabulary.
///
/// This is the entry point a consumer should reach for: the returned
/// [`Catalog`] is the only way to hold banken's forms as a bundle, and it
/// cannot be constructed without every cross-reference resolving (see
/// [`resolve`] for the full check table). The per-domain `load_*` helpers stay
/// public for a single-domain consumer, but they check nothing across domains.
///
/// # Errors
///
/// Any `SpecError` from compiling one of the six spec files, or from
/// cross-resolution (a dangling `:drill-to`, an unknown `:pathologies` entry,
/// a ward/view column disagreement, a chord collision across the two keyed
/// domains, a duplicate name).
pub fn load_catalog() -> Result<Catalog, SpecError> {
    Catalog::resolve(
        load_views()?,
        load_actions()?,
        load_pathologies()?,
        load_wards()?,
        load_drills()?,
        load_nav_keys()?,
        load_bancadas()?,
    )
}
