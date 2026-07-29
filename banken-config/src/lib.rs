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

use ishou_tokens::{FleetDefaults, FleetTheme, FleetThemedConfig};
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

// NOTE: there is deliberately **no** `DEFAULT_THEME` / `DEFAULT_SCROLLBACK_LINES`
// constant. Those two fields are *visual* and are DERIVED from
// `ishou_tokens::FleetDefaults::prescribed()` through
// [`FleetThemedConfig::from_fleet`] — a local constant would be exactly the
// hand-copied palette that primitive exists to delete
// (`pleme-io/docs/fleet-themed-config.md`). A fleet re-theme reaches banken
// on the next compile; `banken_converges_with_the_fleet` pins it.

/// banken's deployment/runtime configuration.
///
/// Every field here is something an *operator deploying banken* chooses.
/// Nothing here describes a view, a column, a sort order or an action —
/// that is the `.lisp` domain face's job, reached via [`Self::spec_dir`].
///
/// `deny_unknown_fields` on the YAML face means an operator typo is a
/// loud failure rather than a silently-ignored key (★★ CONFIGURATION
/// MANAGEMENT). See the crate docs for the honest Lisp-face asymmetry.
// No `Ord`: `ishou_tokens::FleetTheme` is deliberately unordered (there is no
// meaningful "less-than" between two palettes), so the container cannot be
// either. Equality is what config comparison needs.
#[derive(DeriveTataraDomain, Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
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

    /// The selected theme — the typed `ishou_tokens::FleetTheme`, **not** a
    /// string. An unknown theme name has no typed value, so
    /// `:theme "nord-ish"` is rejected when the form compiles rather than
    /// silently falling back to a renderer default.
    ///
    /// DERIVED from the fleet: [`FleetThemedConfig::from_fleet`] copies
    /// `FleetDefaults::theme`, so a fleet re-theme lands here on the next
    /// compile.
    ///
    /// Lisp face: `:theme "vellum"` — note the **snake_case** wire name.
    /// `FleetTheme`'s serde repr is `rename_all = "snake_case"`
    /// (`ishou-tokens/src/fleet_theme.rs:62`), which is deliberately NOT
    /// the same vocabulary as `FleetTheme::preset_name()` (which spells the
    /// `PlemeDark` palette `"nord"`). The typed field is what makes the two
    /// impossible to confuse.
    ///
    /// **Which value is prescribed depends on the ishou-tokens artifact,
    /// not on reading ishou's source.** Measured 2026-07-29: the PUBLISHED
    /// 0.1.4 that banken compiles against prescribes `Vellum`, while the
    /// `ishou` working copy carries `PlemeDark` unpublished at the *same*
    /// version number. Every assertion here derives the expectation from
    /// `FleetDefaults::prescribed()` for exactly that reason; the one
    /// literal that must be concrete lives in `specs/banken.lisp`.
    pub theme: FleetTheme,

    /// The log-pager scrollback cap, in lines. `0` ⇒ unbounded.
    ///
    /// DERIVED from the fleet ([`FleetDefaults::scrollback_lines`]) — the
    /// same 10k floor mado, hikki and the rest of the fleet land on.
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
    /// kubeconfig), no namespace scope (all), no auto-refresh, the `Bare`
    /// theme, unbounded scrollback, and an empty spec dir (author nothing).
    fn bare() -> Self {
        Self {
            context: String::new(),
            namespace: String::new(),
            refresh_interval_ms: 0,
            theme: FleetTheme::Bare,
            scrollback_lines: 0,
            spec_dir: PathBuf::new(),
        }
    }

    /// Tier 2 — prescribed: what banken actually boots with today.
    ///
    /// The **visual** half is not hand-pinned — it comes from
    /// `FleetDefaults::prescribed()` through
    /// [`FleetThemedConfig::from_fleet`], the mado pattern (`mado`'s own
    /// `prescribed_default` is `from_fleet(prescribed())` plus an overlay).
    /// banken's non-visual prescribed values (the poll cadence, the spec
    /// dir) are the overlay, since `FleetDefaults` has no analogue for them.
    ///
    /// These MIRROR the shipped `specs/banken.lisp` baseline so
    /// `banken config-show default` reports what really runs, and
    /// `prescribed_mirrors_the_authored_lisp` pins the two together.
    fn prescribed_default() -> Self {
        Self {
            refresh_interval_ms: DEFAULT_REFRESH_INTERVAL_MS,
            spec_dir: PathBuf::from(DEFAULT_SPEC_DIR),
            // Visual fields (theme, scrollback) + the empty context/namespace
            // floor come from the fleet-derived base.
            ..<Self as FleetThemedConfig>::from_fleet(&FleetDefaults::prescribed())
        }
    }
}

/// The fleet visual derivation — one edit to `FleetDefaults` propagates to
/// banken at the next compile (`pleme-io/docs/fleet-themed-config.md`).
///
/// Hand-written rather than `#[derive(FleetThemed)]`: banken has exactly two
/// fleet-derived fields, so the derive would buy two lines and fail the
/// third-use test. See `Cargo.toml`.
impl FleetThemedConfig for BankenConfig {
    fn from_fleet(fd: &FleetDefaults) -> Self {
        Self {
            theme: fd.theme,
            scrollback_lines: fd.scrollback_lines,
            // Non-visual fields fall back to the zero-opinion floor, per the
            // trait's own guidance — the caller overlays what it prescribes.
            ..<Self as shikumi::TieredConfig>::bare()
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
        assert_eq!(b.theme, FleetTheme::Bare);
        assert_eq!(b.scrollback_lines, 0);
        // The bare floor equals the FLEET bare floor for the visual fields —
        // derived, not coincidence.
        let fleet_bare = FleetDefaults::bare();
        assert_eq!(b.theme, fleet_bare.theme);
        assert_eq!(b.scrollback_lines, fleet_bare.scrollback_lines);
        assert_eq!(b.spec_dir, PathBuf::new());
        // The zero-opinion floor reads all namespaces.
        assert!(!b.is_namespaced());
        assert_eq!(b.read_namespace(), None);
    }

    #[test]
    fn prescribed_is_the_shipped_baseline() {
        let p = <BankenConfig as TieredConfig>::prescribed_default();
        assert_eq!(p.refresh_interval_ms, DEFAULT_REFRESH_INTERVAL_MS);
        assert_eq!(p.spec_dir, PathBuf::from(DEFAULT_SPEC_DIR));
        // Visual fields are DERIVED — asserted against the fleet source, not
        // against a local constant (a local constant would BE the drift).
        let fd = FleetDefaults::prescribed();
        assert_eq!(p.theme, fd.theme);
        assert_eq!(p.scrollback_lines, fd.scrollback_lines);
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
        // FleetTheme's serde repr is snake_case, not the camelCase the
        // container uses. The expected wire name is DERIVED from the fleet
        // rather than written as a literal, so a fleet re-theme does not need
        // this assertion edited.
        let expected_theme = serde_yaml::to_string(&FleetDefaults::prescribed().theme)
            .expect("theme serializes")
            .trim()
            .to_owned();
        assert!(
            yaml.contains(&expected_theme),
            "yaml should carry the fleet theme wire name {expected_theme:?}; yaml was: {yaml}"
        );
        assert!(
            !expected_theme.contains('-'),
            "FleetTheme wire names are snake_case, never kebab: {expected_theme}"
        );
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
theme: pleme_dark
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

    /// **The fleet convergence pin** (`pleme-io/docs/fleet-themed-config.md`).
    ///
    /// One line asserting banken's prescribed visual fields equal
    /// `FleetDefaults::prescribed()`. Same primitive mado
    /// (`mado/src/config.rs:4365`), tobira, namimado, kekkai, hikki, fumi
    /// and escriba-tui/-render already pin with; banken's shape follows
    /// escriba-tui's (`escriba-tui/src/render.rs:588-590`) since a terminal
    /// app owns a theme + a scrollback cap but not a font (the terminal does).
    #[test]
    fn banken_converges_with_the_fleet() {
        let p = <BankenConfig as TieredConfig>::prescribed_default();
        ishou_tokens::convergence::Guard::for_app("banken")
            .expect_theme(p.theme)
            .expect_scrollback_lines(p.scrollback_lines)
            .run();
    }

    /// `from_fleet` is the derivation, and it must actually READ the fleet —
    /// asserted by feeding it the BARE tier and watching the visual fields
    /// follow. A hardcoded body would return the prescribed values here and
    /// fail.
    #[test]
    fn from_fleet_reads_the_fleet_rather_than_hardcoding() {
        let from_bare = <BankenConfig as FleetThemedConfig>::from_fleet(&FleetDefaults::bare());
        assert_eq!(from_bare.theme, FleetTheme::Bare);
        assert_eq!(from_bare.scrollback_lines, 0);

        let from_prescribed =
            <BankenConfig as FleetThemedConfig>::from_fleet(&FleetDefaults::prescribed());
        assert_eq!(from_prescribed.theme, FleetDefaults::prescribed().theme);
        assert_ne!(
            from_prescribed.theme, from_bare.theme,
            "the two fleet tiers must produce different themes, or the \
             derivation is not reading `fd` at all"
        );

        // Non-fleet fields fall back to the zero-opinion floor, per the
        // trait's documented contract — the caller overlays what it prescribes.
        assert_eq!(from_prescribed.refresh_interval_ms, 0);
        assert_eq!(from_prescribed.spec_dir, PathBuf::new());
    }

    /// `FleetDefaults::apply_to` is the ishou-side helper for the same
    /// derivation — it must agree with the direct call.
    #[test]
    fn apply_to_helper_agrees_with_from_fleet() {
        let fd = FleetDefaults::prescribed();
        let via_helper: BankenConfig = fd.apply_to();
        assert_eq!(
            via_helper,
            <BankenConfig as FleetThemedConfig>::from_fleet(&fd)
        );
    }

    /// The typed theme is what makes an unknown palette name unrepresentable
    /// rather than silently defaulted. `FleetTheme`'s serde repr is
    /// snake_case, and it is NOT the preset vocabulary — `"nord"` names the
    /// same palette in `FleetTheme::preset_name()` but is not a valid wire
    /// value, and the typed field is what stops the two being confused.
    #[test]
    fn an_unknown_theme_name_has_no_typed_value() {
        let base = "\
context: ''
namespace: ''
refreshIntervalMs: 1000
scrollbackLines: 10000
specDir: banken-spec/specs
";
        // `nord` is the PRESET vocabulary name for the same palette
        // `FleetTheme` spells `pleme_dark`; `pleme-dark` is the kebab form.
        // Neither is a valid wire value, which is the confusion the typed
        // field removes.
        for bad in ["nord-ish", "pleme-dark", "nord", "PlemeDark", "Vellum"] {
            let mut doc = String::from(base);
            doc.push_str("theme: ");
            doc.push_str(bad);
            doc.push('\n');
            assert!(
                serde_yaml::from_str::<BankenConfig>(&doc).is_err(),
                "`{bad}` is not a FleetTheme wire value and must be rejected"
            );
        }
        // And every real `FleetTheme` wire value IS accepted — derived from
        // the type, so a new variant does not need this list edited.
        for good_theme in [
            FleetTheme::Bare,
            FleetTheme::PlemeDark,
            FleetTheme::Vellum,
            FleetTheme::PolarVeil,
        ] {
            let wire = serde_yaml::to_string(&good_theme)
                .expect("serializes")
                .trim()
                .to_owned();
            let mut good = String::from(base);
            good.push_str("theme: ");
            good.push_str(&wire);
            good.push('\n');
            let c: BankenConfig = serde_yaml::from_str(&good)
                .unwrap_or_else(|e| panic!("wire name {wire:?} must parse: {e}"));
            assert_eq!(c.theme, good_theme);
        }
    }

    #[test]
    fn lisp_face_compiles_the_authored_form() {
        let c = BankenConfig::from_lisp(CANONICAL_BANKEN_LISP).expect("authored form compiles");
        assert_eq!(c.refresh_interval_ms, DEFAULT_REFRESH_INTERVAL_MS);
        assert_eq!(c.spec_dir, PathBuf::from(DEFAULT_SPEC_DIR));
        assert_eq!(
            c.theme,
            FleetDefaults::prescribed().theme,
            "the authored `:theme` must name whatever the CONSUMED \
             ishou-tokens prescribes — measured 2026-07-29: the PUBLISHED \
             0.1.4 prescribes Vellum, while the ishou working copy has \
             PlemeDark unpublished at the same version number. Deriving the \
             expectation is what caught that.",
        );
    }

    #[test]
    fn prescribed_mirrors_the_authored_lisp() {
        // The two faces of ONE surface must agree, or `config-show
        // default` lies about what boots.
        let authored = BankenConfig::from_lisp(CANONICAL_BANKEN_LISP).expect("compiles");
        assert_eq!(
            authored,
            <BankenConfig as TieredConfig>::prescribed_default(),
            "specs/banken.lisp drifted from prescribed_default(). If the THEME \
             is what differs, the fleet re-themed: update the one `:theme` \
             literal in specs/banken.lisp to the new \
             `FleetDefaults::prescribed().theme` wire name. That literal is \
             the only hand-written theme value in banken and this test is \
             what makes the fleet moving under it a loud failure.",
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
  :theme \"pleme_dark\"
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
