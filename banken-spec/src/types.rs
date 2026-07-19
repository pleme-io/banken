//! The typed border of the `postigo` action-legality primitive
//! (BANKEN.md §III.a).
//!
//! `ActionLegality` types every action a banken row exposes into
//! exactly one of three classes — OBSERVE / DECLARE / BREAK-GLASS.
//! `DeclareTarget` enumerates the GitOps lowering rails a DECLARE can
//! aim at; **it has no `Kubectl`/`Apply` arm — a live-mutate action is
//! un-authorable by construction**, which is the whole primitive.
//!
//! Every closed enum here carries an `ALL` const + an exhaustive-match
//! guarantee (★★ CATALOG REFLECTION) so a new variant landing without a
//! catalog row fails the build.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tatara_lisp::DeriveTataraDomain;

// ── Reference newtypes ─────────────────────────────────────────────
//
// Opaque-string references to fleet primitives the DECLARE rails aim
// at. They are typed newtypes (not bare `String`) so a `BandRef`
// cannot be silently used where a `PromessaRef` is meant.

/// A reference to a breathe `*Band` CRD setpoint.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Hash)]
pub struct BandRef(pub String);

/// A reference to a Viggy `(defpromessa)` target.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Hash)]
pub struct PromessaRef(pub String);

/// A reference to a galho typed-IaC stack.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Hash)]
pub struct StackRef(pub String);

/// A reference to an eclusa mergeflow lifecycle.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Hash)]
pub struct MergeflowRef(pub String);

/// A typed operator identity — the BREAK-GLASS witness.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Hash)]
pub struct OperatorId(pub String);

/// A reference to a `RUNBOOK.md` (the forced break-glass log target).
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Hash)]
pub struct RunbookRef(pub String);

// ── ActionLegality — the three-class gate (BANKEN.md §III.a) ───────

/// The `postigo` legality class of one action.
///
/// - `Observe` — a read; always allowed, mutates nothing.
/// - `Declare` — a k9s scale/edit/restart/delete reframed as a GitOps
///   change a reconciler applies (git mutates, never the cluster).
/// - `BreakGlass` — the one sanctioned live-effect arm: witnessed,
///   RUNBOOK-logged, never a default chord.
///
/// Serde is internally tagged on `class` so the Lisp authoring surface
/// round-trips: `:legality (:class observe)` →
/// `{"class":"observe"}` → `Observe`, and `:legality (:class declare
/// :target (…))` → `Declare { target }`. The tag values are kebab-case
/// to match the authored Lisp symbols.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(tag = "class", rename_all = "kebab-case")]
pub enum ActionLegality {
    /// Read: always allowed, mutates nothing.
    Observe,
    /// k9s scale/edit/restart/delete → a GitOps change.
    Declare { target: DeclareTarget },
    /// Wedged-GitOps only; witnessed + RUNBOOK-logged live effect.
    BreakGlass {
        witness: OperatorId,
        runbook: RunbookRef,
    },
}

/// Coarse discriminant of an `ActionLegality`, used for catalog
/// reflection where the payloads are irrelevant (only the *class*
/// matters). Every `ActionLegality` value projects to exactly one.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LegalityClass {
    /// Projects from `ActionLegality::Observe`.
    Observe,
    /// Projects from `ActionLegality::Declare`.
    Declare,
    /// Projects from `ActionLegality::BreakGlass`.
    BreakGlass,
}

impl LegalityClass {
    /// Every legality class — the catalog-reflection surface.
    pub const ALL: &'static [LegalityClass] = &[
        LegalityClass::Observe,
        LegalityClass::Declare,
        LegalityClass::BreakGlass,
    ];

    /// Stable label for telemetry / catalog rows.
    pub fn label(self) -> &'static str {
        match self {
            LegalityClass::Observe => "observe",
            LegalityClass::Declare => "declare",
            LegalityClass::BreakGlass => "break-glass",
        }
    }
}

impl ActionLegality {
    /// Project this action to its coarse legality class.
    pub fn class(&self) -> LegalityClass {
        match self {
            ActionLegality::Observe => LegalityClass::Observe,
            ActionLegality::Declare { .. } => LegalityClass::Declare,
            ActionLegality::BreakGlass { .. } => LegalityClass::BreakGlass,
        }
    }
}

// ── DeclareTarget — the GitOps lowering rails (BANKEN.md §III.a) ───

/// The rail a DECLARE lowers onto. Every arm is a git change a
/// reconciler converges on.
///
/// **CRITICAL: there is no `Kubectl`/`Apply` arm.** A live-mutate
/// action is not expressible — a `(defk8saction)` that tries to author
/// one has no typed value to lower into (a parse-time domain error).
/// The absence is deliberate and load-bearing; do not add such an arm.
///
/// Serde is internally tagged on `rail` (kebab-case) so the authored
/// Lisp `(:rail flux-helm-values :release-path "…")` round-trips into
/// the typed struct variant. Field keys are camelCase to match tatara's
/// kebab→camel key mapping (`:release-path` → `releasePath`).
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(tag = "rail", rename_all = "kebab-case")]
pub enum DeclareTarget {
    /// Edit `k8s/clusters/<c>/apps/*/release.yaml` (SHIPPED reconciler).
    FluxHelmValues {
        #[serde(rename = "releasePath")]
        release_path: PathBuf,
    },
    /// Edit a `*Band` setpoint (SHIPPED CRDs).
    BreatheBand { band: BandRef },
    /// Move a `(defpromessa)` target (ASPIRATIONAL backend).
    ViggySetpoint { promessa: PromessaRef },
    /// Open a reviewed galho (DESIGN rail).
    GalhoChange { stack: StackRef },
    /// A reviewed eclusa PR lifecycle (DESIGN rail).
    EclusaMergeflow { flow: MergeflowRef },
    // *** No (kubectl …) / (apply …) arm exists — a live-mutate plugin
    //     cannot be authored. This omission IS the postigo primitive. ***
}

/// Coarse discriminant of a `DeclareTarget`, for catalog reflection.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DeclareTargetKind {
    /// `DeclareTarget::FluxHelmValues`.
    FluxHelmValues,
    /// `DeclareTarget::BreatheBand`.
    BreatheBand,
    /// `DeclareTarget::ViggySetpoint`.
    ViggySetpoint,
    /// `DeclareTarget::GalhoChange`.
    GalhoChange,
    /// `DeclareTarget::EclusaMergeflow`.
    EclusaMergeflow,
}

impl DeclareTargetKind {
    /// Every declare-target kind — the catalog-reflection surface.
    /// Note the deliberate absence of a `Kubectl`/`Apply` kind.
    pub const ALL: &'static [DeclareTargetKind] = &[
        DeclareTargetKind::FluxHelmValues,
        DeclareTargetKind::BreatheBand,
        DeclareTargetKind::ViggySetpoint,
        DeclareTargetKind::GalhoChange,
        DeclareTargetKind::EclusaMergeflow,
    ];

    /// Stable label for telemetry / catalog rows.
    pub fn label(self) -> &'static str {
        match self {
            DeclareTargetKind::FluxHelmValues => "flux-helm-values",
            DeclareTargetKind::BreatheBand => "breathe-band",
            DeclareTargetKind::ViggySetpoint => "viggy-setpoint",
            DeclareTargetKind::GalhoChange => "galho-change",
            DeclareTargetKind::EclusaMergeflow => "eclusa-mergeflow",
        }
    }
}

impl DeclareTarget {
    /// Project this target to its coarse kind.
    pub fn kind(&self) -> DeclareTargetKind {
        match self {
            DeclareTarget::FluxHelmValues { .. } => DeclareTargetKind::FluxHelmValues,
            DeclareTarget::BreatheBand { .. } => DeclareTargetKind::BreatheBand,
            DeclareTarget::ViggySetpoint { .. } => DeclareTargetKind::ViggySetpoint,
            DeclareTarget::GalhoChange { .. } => DeclareTargetKind::GalhoChange,
            DeclareTarget::EclusaMergeflow { .. } => DeclareTargetKind::EclusaMergeflow,
        }
    }
}

// ── View + column model (BANKEN.md §III.a) ─────────────────────────

/// The kind of a `(defk8sview)` — what the view renders.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ViewKind {
    /// A live sortable multi-column resource table (`:pods`, `:svc`).
    ResourceTable,
    /// The health `ward` (Pulses + Popeye + QUIET headline).
    HealthWard,
    /// The XRay dependency tree.
    DependencyTree,
    /// A scrollable log pager.
    LogPager,
}

impl ViewKind {
    /// Every view kind — the catalog-reflection surface.
    pub const ALL: &'static [ViewKind] = &[
        ViewKind::ResourceTable,
        ViewKind::HealthWard,
        ViewKind::DependencyTree,
        ViewKind::LogPager,
    ];

    /// Stable label for telemetry / catalog rows.
    pub fn label(self) -> &'static str {
        match self {
            ViewKind::ResourceTable => "resource-table",
            ViewKind::HealthWard => "health-ward",
            ViewKind::DependencyTree => "dependency-tree",
            ViewKind::LogPager => "log-pager",
        }
    }
}

/// The data source a view reads from.
///
/// Externally tagged, snake_case, so the authored Lisp round-trips:
/// `:source (:resource pod)` → `{"resource":"pod"}` → `Resource(Pod)`;
/// `:source health` → `"health"` → `Health`.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ViewSource {
    /// A Kubernetes resource kind, listed via a `ClusterEnv` read.
    Resource(ResourceKind),
    /// The composed health signals (band phases + autorevivy + QUIET).
    Health,
    /// The dependency topology of a root resource.
    Topology,
}

/// One column of a resource table.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct ColumnSpec {
    /// The header text ("NAME", "READY", "STATUS", …).
    pub header: String,
    /// The row field this column projects (`name`, `ready`, `phase`, …).
    pub field: String,
}

/// Sort direction.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum Ordering {
    /// Ascending.
    Asc,
    /// Descending.
    Desc,
}

/// A default sort for a table view.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct SortKey {
    /// The column header to sort by.
    pub column: String,
    /// The direction.
    pub order: Ordering,
}

// ── ManifestScope — "full manifest, never a patch" (BANKEN.md §III.a)

/// The scope a DECLARE lowering must serialize.
///
/// `Full` enforces the org "write the whole spec, never a targeted
/// patch" rule (the 2026-07-12 `BreatheCloudPool.dryRun` incident): a
/// narrow patch silently misses sibling gates on the same CR; writing
/// the whole spec forces every field into view.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum ManifestScope {
    /// Serialize the entire spec — the only sanctioned scope.
    Full,
}

// ── ResourceKind — the observe/list catalog ────────────────────────
//
// Mirrors engenho-mcp's `ResourceKind` (idiom-first — the same closed
// catalog banken reads through). A subset sufficient for M0/M1 views;
// extended as views land.

/// Closed catalog of Kubernetes resource kinds banken observes.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum ResourceKind {
    /// core/v1 Pod (namespaced).
    Pod,
    /// core/v1 Service (namespaced).
    Service,
    /// core/v1 ConfigMap (namespaced).
    ConfigMap,
    /// core/v1 Endpoints (namespaced).
    Endpoints,
    /// core/v1 Namespace (cluster-scoped).
    Namespace,
    /// core/v1 Node (cluster-scoped).
    Node,
    /// apps/v1 Deployment (namespaced).
    Deployment,
    /// apps/v1 ReplicaSet (namespaced).
    ReplicaSet,
}

impl ResourceKind {
    /// Every resource kind — the catalog-reflection surface.
    pub const ALL: &'static [ResourceKind] = &[
        ResourceKind::Pod,
        ResourceKind::Service,
        ResourceKind::ConfigMap,
        ResourceKind::Endpoints,
        ResourceKind::Namespace,
        ResourceKind::Node,
        ResourceKind::Deployment,
        ResourceKind::ReplicaSet,
    ];

    /// Stable label for telemetry / catalog rows.
    pub fn label(self) -> &'static str {
        match self {
            ResourceKind::Pod => "pod",
            ResourceKind::Service => "service",
            ResourceKind::ConfigMap => "config_map",
            ResourceKind::Endpoints => "endpoints",
            ResourceKind::Namespace => "namespace",
            ResourceKind::Node => "node",
            ResourceKind::Deployment => "deployment",
            ResourceKind::ReplicaSet => "replica_set",
        }
    }
}

// ── The two authored spec borders (BANKEN.md §III.a) ───────────────

/// A `(defk8sview)` — one navigable view (a resource table, the health
/// ward, an XRay tree, a log pager).
#[derive(DeriveTataraDomain, Serialize, Deserialize, Debug, Clone, PartialEq)]
#[tatara(keyword = "defk8sview")]
pub struct K8sViewSpec {
    /// View name (`"pods"`, `"ward"`, …).
    pub name: String,
    /// What the view renders.
    pub kind: ViewKind,
    /// Where the view reads its data.
    pub source: ViewSource,
    /// The table columns (empty for non-table views).
    #[serde(default)]
    pub columns: Vec<ColumnSpec>,
    /// The default sort (for table views).
    pub default_sort: SortKey,
    /// The view to drill into on Enter, if any.
    #[serde(default)]
    pub drill_to: Option<String>,
}

/// A `(defk8saction)` — one action a row exposes, typed by its
/// `legality`. The `legality` field IS the postigo gate.
#[derive(DeriveTataraDomain, Serialize, Deserialize, Debug, Clone, PartialEq)]
#[tatara(keyword = "defk8saction")]
pub struct K8sActionSpec {
    /// Action name (`"scale"`, `"view-logs"`, `"shell"`, …).
    pub name: String,
    /// The key chord that triggers it (`"s"`, `"l"`, `"S"`, …).
    pub keys: String,
    /// The legality class — the gate.
    pub legality: ActionLegality,
    /// The manifest scope for DECLARE lowering — always `Full`.
    pub manifest_scope: ManifestScope,
}
