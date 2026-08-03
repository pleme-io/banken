//! The `ClusterEnv` seam (BANKEN.md §III.c) — the trait with **no
//! unwitnessed-mutate method**.
//!
//! This copies sui-spec's verified `GcEnvironment`/`apply<E>` shape
//! (gc.rs) *exactly* — but where `GcEnvironment` **has**
//! `delete_path(&self, path: &str) -> Result<u64, String>` (gc.rs:148),
//! `ClusterEnv` **deliberately omits the unwitnessed-mutate analogue.**
//! That omission is the whole primitive:
//!
//! - OBSERVE reads (`list_resources`/`get_resource`/`logs`/`events`/
//!   `topology`/`health_signals`/`watch`) are all read-only — they
//!   return typed rows/streams and mutate nothing.
//! - `declare` is the only non-witnessed write path, and it writes
//!   **git**, never the live cluster — a reconciler applies the change.
//! - `break_glass` is the one witnessed live-effect arm.
//!
//! *** There is NO `fn scale/delete/edit/kubectl_apply`. An unwitnessed
//!     live cluster mutation is not expressible — calling one is `E0599`
//!     (a compile error), not a `Result::Err`. `break_glass` IS a typed
//!     witnessed mutate; `declare` mutates git, then a reconciler
//!     applies. The ABSENCE of a mutate method is the citizenship
//!     primitive. ***
//!
//! The re-add case (an author *extending* the trait with a mutate
//! method) is not forbidden by the type system — it is CI-caught by the
//! substrate-invariant guard in `tests/`. Its honest tier is
//! only-mitigated → CI-caught (BANKEN.md §III.d). Do NOT claim
//! "imperative mutation is unrepresentable" unqualified.

use crate::{
    error::SpecError,
    types::{DeclareTarget, ManifestScope, OperatorId, ResourceKind, RunbookRef},
};

// ── Read-path value types (the afferent surface) ───────────────────

/// One row of an observed resource table — an ordered set of typed
/// cells, plus the resource's identity for drill-down.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Row {
    /// Resource name.
    pub name: String,
    /// Namespace, if namespaced.
    pub namespace: Option<String>,
    /// Column-keyed cell values (`"READY" -> "1/1"`, …). Ordered so a
    /// renderer draws deterministically.
    pub cells: Vec<(String, String)>,
}

/// A [`Row`] is an [`egaku::TableRow`] — the identity is the resource name,
/// and a cell lookup is the association-list read every reader already
/// populates.
///
/// # Why the impl lives HERE and not in `banken`
///
/// Not a placement preference: Rust's orphan rule leaves no other legal home.
/// `egaku::TableRow` is foreign to `banken` and so is `banken_spec::env::Row`,
/// so `impl egaku::TableRow for Row` inside `banken` is `E0117`. It belongs to
/// whichever crate owns one of the two halves, and `Row` is ours.
///
/// # `None` vs `Some("")` is load-bearing here
///
/// [`egaku::TableView::unresolved_fields`] reports columns *no* row populates,
/// and it can only do that if "the reader never emitted this field" and "the
/// reader emitted it empty" are different answers. So this returns `None` for
/// an absent key rather than `""` — which is exactly what the association-list
/// `find` already yields, and why the reserved identity field is handled by
/// the view rather than synthesised into `cells`.
impl egaku::TableRow for Row {
    fn identity(&self) -> &str {
        &self.name
    }

    fn cell(&self, field: &str) -> Option<&str> {
        self.cells
            .iter()
            .find(|(k, _)| k == field)
            .map(|(_, v)| v.as_str())
    }
}

/// A single log line stream (a poll-materialized snapshot; a true
/// follow stream lands with the M1 informer).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LogStream {
    /// The pod the logs came from.
    pub pod: String,
    /// The materialized lines.
    pub lines: Vec<String>,
    /// Whether the source is in follow mode.
    pub follow: bool,
}

/// One cluster event.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Event {
    /// `Normal` / `Warning`.
    pub kind: String,
    /// Short reason code.
    pub reason: String,
    /// Human-readable message.
    pub message: String,
    /// The object this event is about.
    pub involved: String,
}

/// A reference to a specific resource (an XRay root).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ResourceRef {
    /// The resource kind.
    pub kind: ResourceKind,
    /// The resource name.
    pub name: String,
    /// Namespace, if namespaced.
    pub namespace: Option<String>,
}

/// A dependency tree (the XRay data).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DepTree {
    /// The root of the tree.
    pub root: String,
    /// `(parent, child)` owner-ref / endpoint edges.
    pub edges: Vec<(String, String)>,
}

/// A named workload for the health surface.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Workload {
    /// The workload name.
    pub name: String,
    /// Its namespace.
    pub namespace: String,
}

/// A composed per-workload health reading — band phases + autorevivy
/// detection + QUIET leaves. Advisory (C2 external-world observation),
/// never a proof.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct HealthReading {
    /// Per-dimension band phase (`("MemoryBand", "Holding")`, …).
    pub band_phases: Vec<(String, String)>,
    /// Whether the core-metric presence guard passed
    /// (`up==1 ∧ count(metric)>0`) — the BROKEN-METRIC guard.
    pub metric_present: bool,
    /// autorevivy detection labels that fired for this workload.
    pub detections: Vec<String>,
}

/// A live watch stream token (a poll-materialized batch in M0/M1; a
/// true informer-backed stream lands with the M1 `SharedInformer`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct WatchStream {
    /// The kind being watched.
    pub kind_label: String,
    /// The current materialized rows.
    pub rows: Vec<Row>,
}

// ── Write-path value types ─────────────────────────────────────────

/// A DECLARE change to be emitted to git — the *whole* lowered
/// manifest, never a targeted patch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeclareChange {
    /// Which rail this change lands on (Flux values / band / …).
    pub target: DeclareTarget,
    /// The manifest scope (`Full`).
    pub scope: ManifestScope,
    /// The whole serialized manifest (YAML). Serialized from a typed
    /// value via serde — never `format!()` of syntax.
    pub full_manifest: String,
}

/// A reference to a committed DECLARE change (a git ref / PR handle).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangeRef(pub String);

/// The witnessed live-effect action carried through break-glass.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WitnessedAction {
    /// A human-readable description of the selected target.
    pub selector: String,
    /// The witnessing operator.
    pub witness: OperatorId,
    /// The RUNBOOK the action is logged to.
    pub runbook: RunbookRef,
}

/// A durable record of a break-glass action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlassRecord {
    /// The action that ran.
    pub action: WitnessedAction,
    /// The AnomalyChain record id (a placeholder handle until a real
    /// chain ships — labelled, never rounded up).
    pub record_id: String,
}

// ── The seam ───────────────────────────────────────────────────────

/// Abstract IO for a cluster banken observes and declares against.
///
/// Pattern parallel to sui-spec's `GcEnvironment` — **minus** the
/// unwitnessed-mutate method. Every method below is either read-only,
/// a git write, or the one witnessed live arm.
pub trait ClusterEnv {
    // ── OBSERVE (the whole afferent surface) — all read-only ──

    /// List every resource of `kind` in `ns` (all namespaces if `None`).
    ///
    /// # Errors
    /// A `SpecError::Interp { phase: "list-resources" }` on read failure.
    fn list_resources(&self, kind: ResourceKind, ns: Option<&str>) -> Result<Vec<Row>, SpecError>;

    /// Get one resource by name.
    ///
    /// # Errors
    /// A `SpecError::Interp { phase: "get-resource" }` on read failure.
    fn get_resource(
        &self,
        kind: ResourceKind,
        name: &str,
        ns: Option<&str>,
    ) -> Result<Row, SpecError>;

    /// Read a pod's logs (optionally following).
    ///
    /// # Errors
    /// A `SpecError::Interp { phase: "logs" }` on read failure.
    fn logs(&self, pod: &str, ns: &str, follow: bool) -> Result<LogStream, SpecError>;

    /// List cluster events (namespace-scoped if `ns` is `Some`).
    ///
    /// # Errors
    /// A `SpecError::Interp { phase: "events" }` on read failure.
    fn events(&self, ns: Option<&str>) -> Result<Vec<Event>, SpecError>;

    /// Read the dependency topology rooted at `root` (XRay data).
    ///
    /// # Errors
    /// A `SpecError::Interp { phase: "topology" }` on read failure.
    fn topology(&self, root: &ResourceRef) -> Result<DepTree, SpecError>;

    /// Read composed health signals for one workload (band phase +
    /// autorevivy detection + QUIET leaves).
    ///
    /// # Errors
    /// A `SpecError::Interp { phase: "health-signals" }` on read failure.
    fn health_signals(&self, w: &Workload) -> Result<HealthReading, SpecError>;

    /// Open a watch stream for `kind` (a poll-materialized batch until
    /// the M1 informer lands).
    ///
    /// # Errors
    /// A `SpecError::Interp { phase: "watch" }` on read failure.
    fn watch(&self, kind: ResourceKind, ns: Option<&str>) -> Result<WatchStream, SpecError>;

    // ── DECLARE (the only NON-witnessed write path) ──
    //    Emits a git change, never a live cluster mutation.

    /// Emit a DECLARE change to git. The change carries the **whole**
    /// lowered manifest; a reconciler (FluxCD / pangea-operator) then
    /// applies it. This mutates *git*, never the live cluster.
    ///
    /// # Errors
    /// A `SpecError::Interp { phase: "declare" }` on commit failure.
    fn declare(&self, change: DeclareChange) -> Result<ChangeRef, SpecError>;

    // ── BREAK-GLASS (the WITNESSED live-effect arm) ──
    //    Forced RUNBOOK write + attestation. Never a default chord.

    /// Perform a witnessed, RUNBOOK-logged live action — the one
    /// sanctioned live-effect path.
    ///
    /// # Errors
    /// A `SpecError::Interp { phase: "break-glass" }` on failure.
    fn break_glass(&self, action: WitnessedAction) -> Result<GlassRecord, SpecError>;

    // *** THERE IS NO `fn scale/delete/edit/kubectl_apply`.            ***
    // *** An UNWITNESSED live cluster mutation is not expressible —    ***
    // *** calling one is E0599 (a compile error), not a Result::Err.   ***
    // *** `break_glass` IS a typed witnessed mutate; `declare` mutates ***
    // *** git, then a reconciler applies. The absence is the primitive.***
}

/// A shared handle to a cluster IS a cluster.
///
/// Needed the moment a surface reads the same backend from two places at
/// once — an app serving input while a background feed re-reads on a
/// cadence. Without this, holding the env as `Arc<E>` forces every call site
/// that passes it along to spell `&*env`, and each one is a chance to forget.
///
/// It forwards rather than re-implements, so the three-class postigo gate is
/// structurally identical through an `Arc`: there is no `scale`/`delete` here
/// for the same reason there is none above — nothing to forward to.
impl<T: ClusterEnv + ?Sized> ClusterEnv for std::sync::Arc<T> {
    fn list_resources(&self, kind: ResourceKind, ns: Option<&str>) -> Result<Vec<Row>, SpecError> {
        (**self).list_resources(kind, ns)
    }

    fn get_resource(
        &self,
        kind: ResourceKind,
        name: &str,
        ns: Option<&str>,
    ) -> Result<Row, SpecError> {
        (**self).get_resource(kind, name, ns)
    }

    fn logs(&self, pod: &str, ns: &str, follow: bool) -> Result<LogStream, SpecError> {
        (**self).logs(pod, ns, follow)
    }

    fn events(&self, ns: Option<&str>) -> Result<Vec<Event>, SpecError> {
        (**self).events(ns)
    }

    fn topology(&self, root: &ResourceRef) -> Result<DepTree, SpecError> {
        (**self).topology(root)
    }

    fn health_signals(&self, w: &Workload) -> Result<HealthReading, SpecError> {
        (**self).health_signals(w)
    }

    fn watch(&self, kind: ResourceKind, ns: Option<&str>) -> Result<WatchStream, SpecError> {
        (**self).watch(kind, ns)
    }

    fn declare(&self, change: DeclareChange) -> Result<ChangeRef, SpecError> {
        (**self).declare(change)
    }

    fn break_glass(&self, action: WitnessedAction) -> Result<GlassRecord, SpecError> {
        (**self).break_glass(action)
    }
}
