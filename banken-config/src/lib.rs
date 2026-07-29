//! `banken-config` — banken's ONE typed config surface.
//!
//! # Why one struct carries both faces
//!
//! shikumi and the tatara-lisp `(def…)` forms are **not two competing
//! config systems**; they are two faces of one surface, and the fleet
//! already settled which face owns what. The precedent this crate copies
//! is [`escriba-config`] — `escriba/escriba-config/src/lib.rs` puts
//! `#[tatara(keyword = "defescriba")]` (line 21) and
//! `impl shikumi::TieredConfig for EscribaConfig` (line 143) on the
//! **same struct**, and states the division of labour verbatim in its
//! own comment (lines 128–142):
//!
//! > *"The `.lisp` remains the load-bearing prescription (it carries the
//! > keymaps/modes/highlights this 7-field struct cannot express); this
//! > struct is the operator-facing summary."*
//!
//! Applied to banken:
//!
//! | Owns | Face | Artifact |
//! |---|---|---|
//! | **DEPLOYMENT / runtime** — which context, which namespace, how often to refresh, which theme, *where the authored specs live* | shikumi `TieredConfig` (bare / discovered / prescribed / custom-YAML fold) | this struct |
//! | **DOMAIN authoring** — which views exist, their columns, their default sort, which actions exist and their `postigo` legality class | tatara-lisp `(defk8sview)` / `(defk8saction)` | the `.lisp` files under [`BankenConfig::spec_dir`] |
//!
//! The seam between them is exactly one field: [`BankenConfig::spec_dir`].
//! The deployment face says *where* the domain is authored; the domain
//! face says *what* it is. Neither can express the other's content, so
//! there is nothing to compete over.
//!
//! `(defbanken …)` exists on this struct for the same reason
//! `(defescriba …)` exists on `EscribaConfig`: so the deployment summary
//! is *also* authorable as Lisp data (★★ Tlisp-first authoring), not so
//! that a second config system exists.
//!
//! # Honest asymmetry between the two faces (measured, not assumed)
//!
//! The YAML/shikumi face carries `#[serde(deny_unknown_fields)]`, so a
//! typo'd key is a hard deserialize error (**eval/parse-time-rejected**;
//! see `unknown_yaml_field_is_rejected`). The **Lisp face is LOOSE** at
//! banken's pinned `tatara-lisp = "=0.2.4"`: `domain::parse_kwargs`
//! (`tatara-lisp-0.2.4/src/domain.rs:52-67`) inserts every `:keyword`
//! into a map and the derive only ever *reads* the ones it knows, so an
//! unknown kwarg is silently ignored — proven, not assumed, by
//! `lisp_face_silently_ignores_an_unknown_kwarg`. That characterization
//! test is deliberately written as an ASSERTION OF THE CURRENT
//! BEHAVIOUR, so it turns red the moment banken adopts a strict reader
//! and must be flipped to an expect-error.
//!
//! `pending-banken: tatara-lisp-0.3.x-adoption` — the strict-kwargs
//! reader that closes this asymmetry lives in tatara-lisp 0.3.x, which
//! is unpublished as of 2026-07-29 (crates.io tops out at 0.2.5). The
//! pins here stay `=0.2.4` / `=0.2.2` until that lands; moving them now
//! would collide with the in-flight consolidation.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tatara_lisp::DeriveTataraDomain;

/// The default refresh cadence — BANKEN.md §VI M0's 1 Hz poll.
///
/// M0/M1 are a **poll**, not a watch: the concrete `SharedInformer` that
/// would turn engenho's `ReqwestWatcher` stream into a live cache is
/// trait-doc-only (BANKEN.md §IX C-watch). This knob is the poll period,
/// and is *not* renamed to `watch_*` until that substrate exists.
pub const DEFAULT_REFRESH_INTERVAL_MS: u64 = 1_000;

/// The default authored-spec directory, relative to the repo root.
pub const DEFAULT_SPEC_DIR: &str = "banken-spec/specs";

/// The default log-pager scrollback cap, in lines.
pub const DEFAULT_SCROLLBACK_LINES: usize = 10_000;

/// The default theme selection.
///
/// A bare `String` at this milestone — the typed
/// `ishou_tokens::FleetTheme` projection is the next tier up
/// (`pending-banken: ishou-theme`).
pub const DEFAULT_THEME: &str = "pleme-dark";

/// banken's deployment/runtime configuration.
///
/// Every field here is something an *operator deploying banken* chooses.
/// Nothing here describes a view, a column, a sort order or an action —
/// that is the `.lisp` domain face's job, reached via [`Self::spec_dir`].
///
/// `deny_unknown_fields` on the YAML face means an operator typo is a
/// loud failure rather than a silently-ignored key (★★ CONFIGURATION
/// MANAGEMENT). See the crate docs for the honest Lisp-face asymmetry.
#[derive(
    DeriveTataraDomain, Serialize, Deserialize, Debug, Clone, PartialEq, Eq, PartialOrd, Ord,
)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[tatara(keyword = "defbanken")]
pub struct BankenConfig {
    /// The kubeconfig context banken reads. Empty ⇒ whatever the
    /// kubeconfig's own `current-context` resolves to (banken never
    /// writes a context, so "follow the kubeconfig" is the safe floor).
    ///
    /// Lisp face: `:context "camelot-eks"`.
    pub context: String,

    /// The namespace to scope reads to. Empty ⇒ all namespaces.
    ///
    /// Lisp face: `:namespace "flux-system"`.
    pub namespace: String,

    /// The `:pods`-table refresh period, in milliseconds. `0` ⇒ never
    /// auto-refresh (manual only) — the zero-opinion floor.
    ///
    /// Lisp face: `:refresh-interval-ms 1000`.
    pub refresh_interval_ms: u64,

    /// The selected theme. Empty ⇒ no opinion (the renderer's own
    /// fallback).
    ///
    /// Lisp face: `:theme "pleme-dark"`.
    pub theme: String,

    /// The log-pager scrollback cap, in lines. `0` ⇒ unbounded.
    ///
    /// Lisp face: `:scrollback-lines 10000`.
    pub scrollback_lines: usize,

    /// **The seam.** Where the authored `(defk8sview)` / `(defk8saction)`
    /// `.lisp` domain files live. This one field is the whole interface
    /// between the deployment face (this struct) and the domain face
    /// (the Lisp forms).
    ///
    /// Lisp face: `:spec-dir "banken-spec/specs"`.
    pub spec_dir: PathBuf,
}

impl BankenConfig {
    /// Resolve the authored views file under [`Self::spec_dir`].
    #[must_use]
    pub fn views_lisp(&self) -> PathBuf {
        self.spec_dir.join("views.lisp")
    }

    /// Resolve the authored actions file under [`Self::spec_dir`].
    #[must_use]
    pub fn actions_lisp(&self) -> PathBuf {
        self.spec_dir.join("actions.lisp")
    }

    /// `true` when reads are scoped to a single namespace.
    #[must_use]
    pub fn is_namespaced(&self) -> bool {
        !self.namespace.is_empty()
    }

    /// The namespace to pass to a `ClusterEnv` read — `None` for
    /// all-namespaces, so the seam takes the typed `Option<&str>` shape
    /// the trait already declares rather than a sentinel empty string.
    #[must_use]
    pub fn read_namespace(&self) -> Option<&str> {
        if self.namespace.is_empty() {
            None
        } else {
            Some(self.namespace.as_str())
        }
    }

    /// Compile a `(defbanken …)` form into a typed value — the Lisp
    /// authoring face (mirrors `EscribaConfig::from_lisp`).
    ///
    /// # Errors
    ///
    /// Returns a `LispError` when the source fails to read, when it holds
    /// no top-level form, or when a required kwarg is missing / ill-typed.
    pub fn from_lisp(src: &str) -> Result<Self, tatara_lisp::LispError> {
        use tatara_lisp::domain::TataraDomain;
        let forms = tatara_lisp::read(src)?;
        let first = forms
            .first()
            .ok_or_else(|| tatara_lisp::LispError::Compile {
                form: "defbanken".into(),
                message: "empty config — expected one top-level (defbanken …) form".into(),
            })?;
        Self::compile_from_sexp(first)
    }

    /// Register the `(defbanken …)` domain with the tatara-lisp registry
    /// (★★ CATALOG REFLECTION — the form is dispatchable by keyword).
    pub fn register_all() {
        tatara_lisp::domain::register::<Self>();
    }

    /// Read + compile a `(defbanken …)` file from disk.
    ///
    /// # Errors
    ///
    /// Returns a `LispError::Compile` naming the path when the file is
    /// unreadable, and propagates any compile error otherwise.
    pub fn from_lisp_file(path: &Path) -> Result<Self, tatara_lisp::LispError> {
        let src = std::fs::read_to_string(path).map_err(|e| tatara_lisp::LispError::Compile {
            form: "defbanken".into(),
            message: {
                let mut m = String::from("reading ");
                m.push_str(&path.display().to_string());
                m.push_str(": ");
                m.push_str(&e.to_string());
                m
            },
        })?;
        Self::from_lisp(&src)
    }
}

/// The canonical authored deployment config — the `(defbanken …)` form
/// that ships with the crate.
pub const CANONICAL_BANKEN_LISP: &str = include_str!("../specs/banken.lisp");

// ── shikumi::TieredConfig — the deployment face ─────────────────────
//
// Operators reach the tiers via the fleet-standard env selector:
//   BANKEN_TIER=bare banken …
//   BANKEN_TIER=default banken …
//
// Precedent: escriba-config (both faces on one struct) + formigueiro-config
// (`deny_unknown_fields` + camelCase on a runtime/deployment struct).
// See `shikumi/src/tiered.rs:345` for the trait contract.

impl shikumi::TieredConfig for BankenConfig {
    /// Tier 0 — bare: the zero-opinion floor. No context (follow the
    /// kubeconfig), no namespace scope (all), no auto-refresh, no theme,
    /// unbounded scrollback, and an empty spec dir (author nothing).
    fn bare() -> Self {
        Self {
            context: String::new(),
            namespace: String::new(),
            refresh_interval_ms: 0,
            theme: String::new(),
            scrollback_lines: 0,
            spec_dir: PathBuf::new(),
        }
    }

    /// Tier 2 — prescribed: what banken actually boots with today. These
    /// MIRROR the shipped `specs/banken.lisp` baseline so
    /// `banken config-show default` reports what really runs; the
    /// `banken.lisp` stays the authored prescription and a test pins the
    /// two together (`prescribed_mirrors_the_authored_lisp`).
    fn prescribed_default() -> Self {
        Self {
            // Empty on purpose: prescribing a context here would make
            // banken read a cluster the operator did not select. The
            // kubeconfig's own current-context is the honest default.
            context: String::new(),
            // Empty on purpose: k9s lands on all-namespaces.
            namespace: String::new(),
            refresh_interval_ms: DEFAULT_REFRESH_INTERVAL_MS,
            theme: DEFAULT_THEME.to_owned(),
            scrollback_lines: DEFAULT_SCROLLBACK_LINES,
            spec_dir: PathBuf::from(DEFAULT_SPEC_DIR),
        }
    }
}

impl Default for BankenConfig {
    /// Delegates to the prescribed tier so `BankenConfig::default()` and
    /// `TieredConfig::prescribed_default()` cannot drift.
    fn default() -> Self {
        <Self as shikumi::TieredConfig>::prescribed_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shikumi::{ConfigTier, TieredConfig};

    #[test]
    fn bare_is_zero_opinion() {
        let b = <BankenConfig as TieredConfig>::bare();
        assert_eq!(b.context, "");
        assert_eq!(b.namespace, "");
        assert_eq!(b.refresh_interval_ms, 0);
        assert_eq!(b.theme, "");
        assert_eq!(b.scrollback_lines, 0);
        assert_eq!(b.spec_dir, PathBuf::new());
        // The zero-opinion floor reads all namespaces.
        assert!(!b.is_namespaced());
        assert_eq!(b.read_namespace(), None);
    }

    #[test]
    fn prescribed_is_the_shipped_baseline() {
        let p = <BankenConfig as TieredConfig>::prescribed_default();
        assert_eq!(p.refresh_interval_ms, DEFAULT_REFRESH_INTERVAL_MS);
        assert_eq!(p.theme, DEFAULT_THEME);
        assert_eq!(p.scrollback_lines, DEFAULT_SCROLLBACK_LINES);
        assert_eq!(p.spec_dir, PathBuf::from(DEFAULT_SPEC_DIR));
        // A prescribed context/namespace would silently point banken at a
        // cluster the operator never chose — both stay empty by design.
        assert_eq!(p.context, "");
        assert_eq!(p.namespace, "");
        assert_ne!(p, <BankenConfig as TieredConfig>::bare());
    }

    #[test]
    fn default_delegates_to_prescribed() {
        assert_eq!(
            BankenConfig::default(),
            <BankenConfig as TieredConfig>::prescribed_default()
        );
    }

    #[test]
    fn resolve_tier_dispatches() {
        assert_eq!(
            <BankenConfig as TieredConfig>::resolve_tier(ConfigTier::Bare),
            <BankenConfig as TieredConfig>::bare()
        );
        assert_eq!(
            <BankenConfig as TieredConfig>::resolve_tier(ConfigTier::Default),
            <BankenConfig as TieredConfig>::prescribed_default()
        );
    }

    #[test]
    fn spec_dir_is_the_seam_to_the_lisp_domain_face() {
        let p = BankenConfig::default();
        // The deployment face names WHERE the domain is authored; the
        // domain face (defk8sview/defk8saction) says WHAT it is.
        assert_eq!(
            p.views_lisp(),
            PathBuf::from("banken-spec/specs/views.lisp")
        );
        assert_eq!(
            p.actions_lisp(),
            PathBuf::from("banken-spec/specs/actions.lisp")
        );
    }

    #[test]
    fn read_namespace_projects_to_the_cluster_env_shape() {
        let mut c = BankenConfig::default();
        assert_eq!(c.read_namespace(), None, "empty ⇒ all namespaces");
        c.namespace = "flux-system".into();
        assert!(c.is_namespaced());
        assert_eq!(c.read_namespace(), Some("flux-system"));
    }

    #[test]
    fn yaml_round_trips_through_the_deployment_face() {
        let p = BankenConfig::default();
        let yaml = serde_yaml::to_string(&p).expect("serialize");
        // camelCase on the wire — the fleet convention.
        assert!(yaml.contains("refreshIntervalMs"), "yaml was: {yaml}");
        assert!(yaml.contains("specDir"), "yaml was: {yaml}");
        let back: BankenConfig = serde_yaml::from_str(&yaml).expect("deserialize");
        assert_eq!(back, p);
    }

    /// THE GATE. `deny_unknown_fields` is what turns an operator typo
    /// into a loud failure instead of a silently-dropped knob.
    ///
    /// The input is a **completely valid** config plus ONE extra key, so
    /// the only thing that can reject it is `deny_unknown_fields` — a
    /// permissive deserializer parses this happily. (An earlier draft
    /// omitted the real key instead; that version went red even without
    /// the attribute, on `missing field`, which would have made the gate
    /// prove the wrong thing.)
    #[test]
    fn unknown_yaml_field_is_rejected() {
        let bad = "\
context: ''
namespace: ''
refreshIntervalMs: 1000
theme: pleme-dark
scrollbackLines: 10000
specDir: banken-spec/specs
watchIntervalMs: 500
";
        // Sanity: strip the extra key and the same document IS valid, so
        // the rejection below is attributable to that key alone.
        let good = bad.replace("watchIntervalMs: 500\n", "");
        serde_yaml::from_str::<BankenConfig>(&good)
            .expect("the same document without the extra key must deserialize");

        let err = serde_yaml::from_str::<BankenConfig>(bad)
            .expect_err("deny_unknown_fields must reject an unknown key");
        let msg = err.to_string();
        assert!(
            msg.contains("unknown field"),
            "expected an unknown-field error, got: {msg}"
        );
    }

    #[test]
    fn lisp_face_compiles_the_authored_form() {
        let c = BankenConfig::from_lisp(CANONICAL_BANKEN_LISP).expect("authored form compiles");
        assert_eq!(c.refresh_interval_ms, DEFAULT_REFRESH_INTERVAL_MS);
        assert_eq!(c.spec_dir, PathBuf::from(DEFAULT_SPEC_DIR));
    }

    #[test]
    fn prescribed_mirrors_the_authored_lisp() {
        // The two faces of ONE surface must agree, or `config-show
        // default` lies about what boots.
        let authored = BankenConfig::from_lisp(CANONICAL_BANKEN_LISP).expect("compiles");
        assert_eq!(
            authored,
            <BankenConfig as TieredConfig>::prescribed_default(),
            "specs/banken.lisp drifted from prescribed_default()"
        );
    }

    #[test]
    fn lisp_face_rejects_a_missing_required_kwarg() {
        // The Lisp face IS strict about *required* kwargs (only about
        // *unknown* ones is it loose — see the next test).
        let err = BankenConfig::from_lisp("(defbanken :context \"camelot-eks\")")
            .expect_err("a missing required kwarg must fail");
        assert!(
            err.to_string().contains("required"),
            "expected a required-but-not-provided error, got: {err}"
        );
    }

    /// CHARACTERIZATION, not aspiration. At `tatara-lisp = "=0.2.4"`,
    /// `domain::parse_kwargs` (domain.rs:52-67) is LOOSE: it maps every
    /// `:keyword` and the derive reads only the ones it knows, so an
    /// unknown kwarg is silently dropped. The YAML face rejects the same
    /// class loudly (`unknown_yaml_field_is_rejected`).
    ///
    /// This asserts the CURRENT behaviour on purpose, so adopting the
    /// strict reader turns it red and forces the flip to an
    /// expect-error. `pending-banken: tatara-lisp-0.3.x-adoption`.
    #[test]
    fn lisp_face_silently_ignores_an_unknown_kwarg() {
        let src = "\
(defbanken
  :context \"\"
  :namespace \"\"
  :refresh-interval-ms 1000
  :theme \"pleme-dark\"
  :scrollback-lines 10000
  :spec-dir \"banken-spec/specs\"
  :refresh-interval-MS 500)
";
        let c = BankenConfig::from_lisp(src)
            .expect("0.2.4 parse_kwargs is loose — the typo is ignored, not rejected");
        assert_eq!(
            c.refresh_interval_ms, DEFAULT_REFRESH_INTERVAL_MS,
            "the typo'd kwarg was silently dropped, the real field kept its authored value",
        );
    }
}
