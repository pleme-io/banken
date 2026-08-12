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
//! banken's `tatara-lisp = "0.3.3"`: `domain::parse_kwargs`
//! (`tatara-lisp-0.3.3/src/domain.rs:63-78`) inserts every `:keyword`
//! into a map and the derive only ever *reads* the ones it knows, so an
//! unknown kwarg is silently ignored — proven, not assumed, by
//! `lisp_face_silently_ignores_an_unknown_kwarg`. That characterization
//! test is deliberately written as an ASSERTION OF THE CURRENT
//! BEHAVIOUR, so it turns red the moment banken adopts a strict reader
//! and must be flipped to an expect-error.
//!
//! `pending-banken: strict-kwargs-reader` — **corrected 2026-07-30**, and
//! the correction is the point: the previous row claimed the strict reader
//! "lives in tatara-lisp 0.3.x", so adopting 0.3.3 was expected to close
//! this. It did not. Canonical 0.3.3 ships `parse_kwargs` and **no strict
//! variant at all** (verified by reading `domain.rs`, not by trusting the
//! version number), and the 0.3.3 derive emits a call to exactly that
//! lenient function. So the asymmetry is unchanged by the migration and
//! the row stays open, now blocked on the reader being *written* upstream
//! rather than on a release being *published*.

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
/// The prescribed opening stance: NORMAL.
///
/// Operator decision 2026-08-12. The screen draws a stance badge and binds
/// vim keys, so opening in INSERT meant `j` typed a `j` — a screen that
/// advertises a mode and then ignores it.
pub const DEFAULT_PICKER_STANCE: PickerStance = PickerStance::Normal;

/// Which stance a modal filter box opens in.
///
/// Mirrors `unsoku::Stance`'s two inhabitants without depending on it: this
/// crate is the CONFIG face and has no business pulling in an editor, and the
/// use site converts. An unknown spelling has no typed value, so
/// `:picker-stance "norml"` is refused when the form compiles rather than
/// falling back to a default nobody chose.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PickerStance {
    /// Navigate first; `i` starts typing. The prescribed default.
    #[default]
    Normal,
    /// Open typing; `esc` reaches NORMAL.
    Insert,
}

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
    /// Lisp face: `:context "alpha-eks"`.
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

    /// Which stance the cluster picker's filter opens in.
    ///
    /// A knob because it is a genuine preference with two defensible answers
    /// — a chooser's primary act is typing, and a screen that draws a stance
    /// badge must honour the stance — and burying a preference in a
    /// constructor is what made it unaskable. It was
    /// `pending-banken: opening-stance-config`.
    ///
    /// A typed enum rather than a bool, for two reasons. The authoring
    /// surface has no bool atom, so `:picker-opens-in-normal true` is refused
    /// with "expected bool" (measured); and `"normal"` says at the call site
    /// what `true` cannot, so an unknown spelling has no typed value instead
    /// of silently meaning `false`.
    ///
    /// Lisp face: `:picker-stance "normal"`.
    ///
    /// `#[serde(default)]` because an OVERLAY may legitimately omit it: a
    /// `banken-local.yaml` that sets only a namespace must not be required to
    /// restate every field, and without this every partial overlay fails with
    /// `missing field pickerStance`. The default is `Normal`, which is the
    /// same value the prescribed tier carries, so an omission and an explicit
    /// `"normal"` mean the same thing.
    #[serde(default)]
    pub picker_stance: PickerStance,

    /// The selected theme — the typed `ishou_tokens::FleetTheme`, **not** a
    /// string. An unknown theme name has no typed value, so
    /// `:theme "nord-ish"` is rejected when the form compiles rather than
    /// silently falling back to a renderer default.
    ///
    /// DERIVED from the fleet: [`FleetThemedConfig::from_fleet`] copies
    /// `FleetDefaults::theme`, so a fleet re-theme lands here on the next
    /// compile.
    ///
    /// Lisp face: `:theme "vellum"` — note the **`snake_case`** wire name.
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

    /// The `owner/name` of the `GitOps` repository a DECLARE opens its pull
    /// request against. Empty ⇒ banken refuses to declare.
    ///
    /// **Empty is the prescribed value and the refusal is the point.** There
    /// is no fleet-wide right answer — the repository that owns a resource is
    /// a fact about one estate — and a DECLARE written to a *guessed*
    /// repository opens a PR that looks correct and reconciles nothing, which
    /// costs a reviewer's trust rather than merely failing. So the zero value
    /// is "I do not know", and `crate::declare` turns that into a typed
    /// refusal naming the field.
    ///
    /// This replaced a `BANKEN_GITOPS_REPO` environment variable, which is
    /// exactly the untyped, unauthored, undiscoverable surface the fleet
    /// configuration rule exists to eliminate: it had no schema, no default a
    /// reader could find, and no way to be set per-estate alongside the rest
    /// of the config.
    ///
    /// Lisp face: `:gitops-repo "pleme-io/k8s"`.
    #[serde(default)]
    pub gitops_repo: String,

    /// The branch a DECLARE's pull request targets.
    ///
    /// Prescribed `"main"` — unlike the repository, this one HAS a defensible
    /// fleet-wide answer, so it gets a real default rather than a refusal.
    ///
    /// Lisp face: `:gitops-base "main"`.
    #[serde(default = "default_gitops_base")]
    pub gitops_base: String,

    /// How often the watchdog re-walks its cheap TCP rounds, in milliseconds.
    ///
    /// A knob because the right answer depends on the estate: a VPN that comes
    /// up while the operator reads the list should be noticed quickly, but each
    /// round is a real connect against every configured context, and an estate
    /// with connection logging sees them.
    ///
    /// Lisp face: `:ronda-round-ms 15000`.
    #[serde(default = "default_ronda_round_ms")]
    pub ronda_round_ms: u64,

    /// How often the watchdog climbs the EXPENSIVE half of the ladder, in
    /// milliseconds.
    ///
    /// Separate from [`Self::ronda_round_ms`] because the costs differ by two
    /// orders of magnitude: everything above `Rung::Network` runs a credential
    /// helper, which on an EKS context is an `aws eks get-token` subprocess and
    /// an STS round-trip. Five minutes against four reachable contexts is ~48
    /// helper invocations an hour; climbing on the cheap clock against all
    /// eighteen would be ~4300. That ratio is why there are two knobs and not
    /// one.
    ///
    /// Lisp face: `:ronda-climb-ms 300000`.
    #[serde(default = "default_ronda_climb_ms")]
    pub ronda_climb_ms: u64,
}

fn default_gitops_base() -> String {
    "main".to_owned()
}

const fn default_ronda_round_ms() -> u64 {
    15_000
}

const fn default_ronda_climb_ms() -> u64 {
    300_000
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

    /// The `GitOps` repository a DECLARE targets, or `None` when unset.
    ///
    /// `None` rather than an empty string so the refusal happens at the one
    /// place that can explain it, instead of an empty `owner/name` reaching
    /// the forge and failing as a parse error about a string nobody typed.
    #[must_use]
    pub fn gitops_target(&self) -> Option<(&str, &str)> {
        if self.gitops_repo.trim().is_empty() {
            None
        } else {
            Some((self.gitops_repo.as_str(), self.gitops_base.as_str()))
        }
    }

    /// The cheap-round interval as a `Duration`.
    #[must_use]
    pub fn ronda_round(&self) -> std::time::Duration {
        std::time::Duration::from_millis(self.ronda_round_ms)
    }

    /// The credential-climb interval as a `Duration`.
    #[must_use]
    pub fn ronda_climb(&self) -> std::time::Duration {
        std::time::Duration::from_millis(self.ronda_climb_ms)
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
            // NORMAL is the zero-opinion value, not a preference: it is
            // `unsoku::Stance::default()`. Opening in INSERT would be this
            // tier expressing exactly the opinion it exists not to have.
            picker_stance: PickerStance::Normal,
            theme: FleetTheme::Bare,
            scrollback_lines: 0,
            spec_dir: PathBuf::new(),
            // No repository — so a DECLARE from a bare-tier banken REFUSES.
            // That is the zero-opinion answer: this tier has no idea which
            // estate it is pointed at, and guessing one would be an opinion.
            gitops_repo: String::new(),
            gitops_base: default_gitops_base(),
            // Zero rounds at the bare tier would leave every marker `unknown`
            // forever, which is not zero-opinion — it is a broken watchdog. The
            // cadence is not an opinion about the estate, so the floor keeps it.
            ronda_round_ms: default_ronda_round_ms(),
            ronda_climb_ms: default_ronda_climb_ms(),
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
            picker_stance: DEFAULT_PICKER_STANCE,
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
    /// `snake_case`, and it is NOT the preset vocabulary — `"nord"` names the
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
        let err = BankenConfig::from_lisp("(defbanken :context \"alpha-eks\")")
            .expect_err("a missing required kwarg must fail");
        assert!(
            err.to_string().contains("required"),
            "expected a required-but-not-provided error, got: {err}"
        );
    }

    /// CHARACTERIZATION, not aspiration. At `tatara-lisp = "0.3.3"`,
    /// `domain::parse_kwargs` (domain.rs:63-78) is LOOSE: it maps every
    /// `:keyword` and the derive reads only the ones it knows, so an
    /// unknown kwarg is silently dropped. The YAML face rejects the same
    /// class loudly (`unknown_yaml_field_is_rejected`).
    ///
    /// This asserts the CURRENT behaviour on purpose, so adopting the
    /// strict reader turns it red and forces the flip to an
    /// expect-error. It stayed GREEN across the 0.2.4 → 0.3.3 migration,
    /// which is precisely how we learned 0.3.3 does not carry the strict
    /// reader after all. `pending-banken: strict-kwargs-reader`.
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

// ── The discovery loader — the writable-overlay tier ────────────────

impl BankenConfig {
    /// Load the effective config from every discovered layer, lowest priority
    /// first.
    ///
    /// # Why `discover_all` + `load_merged` and not `discover` + `load`
    ///
    /// Measured on this machine 2026-08-08: **4 of 4** sampled fleet app
    /// configs (`mado.yaml`, `tear.yaml`, `tend/config.yaml`,
    /// `escriba/rc.lisp`) are symlinks into `/nix/store`, and `~/.config`
    /// carries **41** such symlinks. `touch` on the store target is
    /// `Permission denied`. So the single-file path (`discover()` → `load()`)
    /// resolves to a **read-only** file for essentially every deployed app —
    /// an operator cannot change a value without a rebuild, and any editing
    /// surface built over it would refuse 100% of the fleet.
    ///
    /// `ConfigDiscovery::discover_all` already returns a merge-ordered path
    /// list **including `{app}-*.yaml` partials** (`discovery.rs:1682`,
    /// `:1814`), and `ConfigStore::load_merged` is documented as its
    /// counterpart. So the writable tier needs no new shikumi machinery: the
    /// nix-owned `banken.yaml` stays the base, and an **unmanaged**
    /// `banken-local.yaml` beside it is a writable overlay that wins.
    ///
    /// # Errors
    ///
    /// Propagates discovery and merge failures untouched. A **missing** config
    /// is not an error — it yields [`shikumi::TieredConfig::prescribed_default`],
    /// because banken must start on a machine that has never configured it.
    pub fn discover_effective() -> Result<Self, shikumi::ShikumiError> {
        let paths = shikumi::ConfigDiscovery::new("banken")
            .hierarchical()
            .discover_all()?;

        if paths.is_empty() {
            return Ok(<Self as shikumi::TieredConfig>::prescribed_default());
        }
        Ok(
            shikumi::ConfigStore::<Self>::load_merged(&paths, "BANKEN_")?
                .get()
                .as_ref()
                .clone(),
        )
    }

    /// Every layer discovery would fold, lowest priority first.
    ///
    /// Exposed because *which file supplied a value* is the question an
    /// operator actually has, and the answer is not derivable from the folded
    /// value. A surface that shows an effective value without being able to
    /// name its source is the provenance hole this loader exists inside of.
    ///
    /// # Errors
    ///
    /// A `shikumi::ShikumiError` when the discovery walk itself fails — a home
    /// directory that cannot be resolved, or a layer path that cannot be
    /// stat'ed. An *absent* layer is not an error: not having a config file is
    /// the common case, and the fold's whole job is to be correct without one.
    pub fn layers() -> Result<Vec<std::path::PathBuf>, shikumi::ShikumiError> {
        shikumi::ConfigDiscovery::new("banken")
            .hierarchical()
            .discover_all()
    }
}

#[cfg(test)]
mod overlay_tests {
    use super::*;

    /// The COMPLETE base, as home-manager renders it. Completeness is not
    /// incidental: `BankenConfig` carries `deny_unknown_fields` and no serde
    /// field defaults, and figment merges every source THEN extracts once — so
    /// the merged document must be whole. In deployment the nix-owned base is
    /// whole by construction and only the overlay is partial, which is exactly
    /// the shape these tests exercise.
    const BASE_YAML: &str = "context: \"\"\nnamespace: \"\"\nrefreshIntervalMs: 1000\ntheme: vellum\nscrollbackLines: 10000\nspecDir: specs\n";

    /// THE claim the writable tier rests on: a `banken-*.yaml` partial beside
    /// the base file **wins**, so a nix-owned read-only base can be overridden
    /// by a writable overlay without touching it.
    ///
    /// Measured on this machine 2026-08-08: 4 of 4 sampled fleet app configs
    /// are `/nix/store` symlinks and `~/.config` holds 41 of them; `touch` on
    /// the target is Permission denied. So without this precedence there is no
    /// way for an operator to change a value at all short of a rebuild — and
    /// any editing surface built over the single-file path would refuse 100%
    /// of the deployed fleet.
    ///
    /// This drives `ConfigStore::load_merged` over an explicit path list rather
    /// than `discover_effective()` because the latter walks up from the process
    /// CWD, and a test that mutates CWD races every other test in the binary.
    /// The merge ORDER is the thing under test, and it is the same order
    /// `ConfigDiscovery::discover_all` produces.
    #[test]
    fn a_local_overlay_wins_over_the_base_file() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let base = dir.path().join("banken.yaml");
        let local = dir.path().join("banken-local.yaml");

        // The nix-owned base: what home-manager renders, read-only in reality.
        std::fs::write(&base, BASE_YAML).expect("base");
        // The unmanaged overlay: the file an operator can actually write.
        std::fs::write(&local, "refreshIntervalMs: 250\n").expect("overlay");

        let store = shikumi::ConfigStore::<BankenConfig>::load_merged(
            &[base, local],
            "BANKEN_OVERLAY_TEST_",
        )
        .expect("merge");

        let cfg = store.get();
        assert_eq!(
            cfg.refresh_interval_ms, 250,
            "the writable overlay must win over the read-only base — without this \
             an operator cannot change a value without a rebuild"
        );
    }

    /// The overlay is a PARTIAL: a field it does not mention keeps the base's
    /// value rather than reverting to the type's default. Without this the
    /// overlay would be a whole-file replacement and an operator changing one
    /// knob would silently reset every other one.
    #[test]
    fn an_overlay_does_not_erase_fields_it_omits() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let base = dir.path().join("banken.yaml");
        let local = dir.path().join("banken-local.yaml");

        std::fs::write(
            &base,
            BASE_YAML.replace("namespace: \"\"", "namespace: flux-system"),
        )
        .expect("base");
        std::fs::write(&local, "refreshIntervalMs: 250\n").expect("overlay");

        let store = shikumi::ConfigStore::<BankenConfig>::load_merged(
            &[base, local],
            "BANKEN_OVERLAY_PARTIAL_TEST_",
        )
        .expect("merge");

        let cfg = store.get();
        assert_eq!(cfg.refresh_interval_ms, 250, "the overlay's field wins");
        assert_eq!(
            cfg.namespace, "flux-system",
            "a field the overlay omits must survive from the base"
        );
    }
}
