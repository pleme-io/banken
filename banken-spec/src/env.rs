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

/// A Kubernetes object identity — `metadata.uid`.
///
/// # Why this exists, and why it is not a `String`
///
/// `Row::identity()` used to be the bare `name`. In the all-namespaces view
/// banken actually ships, two pods called `api` in different namespaces were
/// therefore **one identity** — and `(namespace, name)` is no better, because
/// it is unique only *at an instant*. client-go's own `shared_informer.go`
/// documents the recycle trap verbatim: delete object `X` with uid `A`, create
/// object `X` with uid `B`, and a watcher is notified of **an update from the
/// first to the second**, never of the delete-then-create.
///
/// Every precondition banken will ever mint — a grip on a row before a
/// live-effect action, a `resourceVersion` on a patch, an observed reclaim —
/// is addressed by this value. Keyed on a name, all of them could be minted
/// against the wrong object while every guard stayed green.
///
/// The field is private with a fallible constructor for the same reason
/// [`crate::types::OperatorId`]'s is: a blank identity names nothing, and was
/// constructible by accident.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Uid(String);

impl Uid {
    /// The sole constructor. `None` for a blank identity.
    #[must_use]
    pub fn new(uid: impl Into<String>) -> Option<Self> {
        let uid = uid.into();
        if uid.trim().is_empty() {
            return None;
        }
        Some(Self(uid))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for Uid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// The `:field` an authored view points its NAME column at, so the object's
/// **name** reaches the screen.
///
/// # Why it is not `egaku::IDENTITY_FIELD`
///
/// `egaku::TableView::project` special-cases `IDENTITY_FIELD` and returns
/// [`egaku::TableRow::identity`] without ever consulting
/// [`egaku::TableRow::cell`] (`egaku-0.1.5/src/table.rs:398-404`). banken's
/// `identity()` is the **uid** — a deliberate, load-bearing choice for `Grip`,
/// where an act must address an object across delete-and-recreate — so a NAME
/// column on `IDENTITY_FIELD` draws a uid.
///
/// Measured 2026-08-09 against `alpha-eks`: all 69 rows rendered
/// `10a69bf6-b039-4731-8cce-28ed6e55c534` under `NAME`. Pointing the authored
/// column at an ordinary field routes it through `cell`, which is the whole
/// fix and needs no change to egaku — which banken must not make from this
/// repo (QUADRO T1).
///
/// **The upstream destination**, recorded rather than promised:
/// `pending-banken: egaku-identity-field-projection` — `project` should prefer
/// `row.cell(field)` and fall back to `identity()`, at which point a NAME
/// column on `IDENTITY_FIELD` would resolve correctly and this constant could
/// retire. It is one egaku change; until it lands, the authored field is the
/// honest join.
pub const DISPLAY_NAME_FIELD: &str = "object-name";

/// One row of an observed resource table — an ordered set of typed
/// cells, plus the resource's identity for drill-down.
///
/// # `Default` is deliberately absent
///
/// It used to derive it, which made an all-empty `Row` constructible — so a
/// cache miss returning `Row::default()` renders as a blank **real** row rather
/// than as an absence. A row is something banken OBSERVED; there is no such
/// thing as a default one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// The object's `metadata.uid` — the identity, never the name.
    pub uid: Uid,
    /// Resource name.
    pub name: String,
    /// Namespace, if namespaced.
    pub namespace: Option<String>,
    /// The object's `metadata.resourceVersion` at the moment it was observed.
    ///
    /// Opaque and **not orderable** — k8s only guarantees ordering from
    /// apiserver 1.35, and `alpha-eks` is v1.33. Compare it for EQUALITY to
    /// answer "has this object moved since I looked", never with `<`.
    pub version: Option<String>,
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
#[cfg(test)]
mod table_row_identity_tests {
    use super::*;
    use egaku::TableRow;

    /// The NAME column renders the NAME; `identity()` returns the UID.
    ///
    /// These are different questions and were briefly collapsed: `identity()`
    /// correctly moved to the uid for the `Grip` work, and because nothing
    /// populated the identity FIELD the drawer fell back to it, so the NAME
    /// column rendered `uid-v1-v1-Pod-demo-web-217ffebd52-0-14` — measured
    /// live against engenho on 2026-08-09.
    ///
    /// The live test in place at the time could NOT catch it: it asserted
    /// `line.contains(&row.name)`, and engenho's uid EMBEDS the pod name, so a
    /// uid-rendering column satisfied it. This asserts equality on the cell
    /// instead, which no substring coincidence can satisfy.
    #[test]
    fn the_name_column_renders_the_name_and_identity_stays_the_uid() {
        let row = Row {
            uid: Uid::new("uid-v1-v1-Pod-demo-web-217ffebd52-0-14").expect("non-blank"),
            name: "web-217ffebd52-0".to_owned(),
            namespace: Some("demo".to_owned()),
            version: None,
            cells: vec![("phase".to_owned(), "Running".to_owned())],
        };

        assert_eq!(
            row.cell(egaku::IDENTITY_FIELD),
            Some("web-217ffebd52-0"),
            "the identity FIELD is what the NAME column draws — it must be the \
             name, never the uid",
        );
        assert_eq!(
            row.identity(),
            "uid-v1-v1-Pod-demo-web-217ffebd52-0-14",
            "act identity must remain the uid so a recreate is not mistaken \
             for the same object",
        );
        assert_ne!(
            row.cell(egaku::IDENTITY_FIELD).unwrap(),
            row.identity(),
            "display identity and act identity are different questions",
        );
        // Ordinary cells are unaffected.
        assert_eq!(row.cell("phase"), Some("Running"));
        assert_eq!(row.cell("nope"), None);
    }
}

impl egaku::TableRow for Row {
    /// The **uid**, never the name — see [`Uid`] for why the name was wrong.
    fn identity(&self) -> &str {
        self.uid.as_str()
    }

    fn cell(&self, field: &str) -> Option<&str> {
        // The NAME column renders the object's NAME, while `identity()`
        // returns the UID. Those are two different questions and collapsing
        // them was a real regression: when `identity()` moved from name to uid
        // (for the `Grip` work — an act must address an object across
        // recreate), the NAME column started rendering
        // `uid-v1-v1-Pod-demo-web-…`.
        //
        // **`DISPLAY_NAME_FIELD` is what the authored view actually names, and
        // that is not cosmetic.** `egaku::TableView::project` short-circuits
        // `IDENTITY_FIELD` straight to `row.identity()` and never calls this
        // method for it (`egaku-0.1.5/src/table.rs:398-404`), so answering
        // `IDENTITY_FIELD` here fixed nothing on the render path — measured
        // 2026-08-09 against alpha-eks, where every one of 69 rows drew its
        // uid. A column pointed at an ordinary field is projected through
        // `cell`, which is how the fix reaches the screen without banken
        // editing egaku from this repo. `pending-banken: egaku-identity-field-
        // projection` records the upstream destination.
        //
        // The `IDENTITY_FIELD` arm stays anyway: it is the right answer to the
        // question, it keeps a reader that asks for `"name"` working, and it
        // is what will make the upstream fix a no-op here rather than a second
        // migration.
        if field == DISPLAY_NAME_FIELD || field == egaku::IDENTITY_FIELD {
            return Some(self.name.as_str());
        }
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

    /// Re-read `id` **at the moment of the act** and return a [`Grip`] only if
    /// it is still the same object.
    ///
    /// This is an OBSERVE method — it reads and nothing else — and it exists
    /// because a preview and an act are separated by the operator's think time.
    /// A `(defbancada)` resolved on `g` and opened on `enter` addresses whatever
    /// the name resolves to *then*; with an absorbed cache that window is the
    /// cache's TTL. Re-deriving here is what makes "the object you looked at"
    /// and "the object you act on" the same claim.
    ///
    /// Implementations MUST compare the **uid**, not the name. Returning a grip
    /// because the name still resolves is precisely the recycle bug
    /// ([`GripError::Recycled`]).
    ///
    /// # Errors
    ///
    /// [`GripError::Vanished`] when the object is gone, [`GripError::Recycled`]
    /// when the name now resolves to a different uid, and [`GripError::Blind`]
    /// when the backend could not be read — the last is deliberately distinct
    /// from `Vanished`, because "it is gone" and "I cannot see" must not
    /// authorise an act the same way.
    fn grip(&self, id: &ObjectId) -> Result<Grip, GripError>;

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

    fn grip(&self, id: &ObjectId) -> Result<Grip, GripError> {
        (**self).grip(id)
    }

    fn declare(&self, change: DeclareChange) -> Result<ChangeRef, SpecError> {
        (**self).declare(change)
    }

    fn break_glass(&self, action: WitnessedAction) -> Result<GlassRecord, SpecError> {
        (**self).break_glass(action)
    }
}

// ── Identity at the act: ObjectId + Grip (the postigo × cache seal) ──

/// What a row IS, addressed the way the apiserver addresses it.
///
/// Carried by a preview so the act can re-find the same object. Fields are
/// private and [`ObjectId::of`] is the only constructor, so an id cannot be
/// assembled from a name an operator typed — it is always derived from
/// something banken observed.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ObjectId {
    uid: Uid,
    kind: ResourceKind,
    namespace: Option<String>,
    name: String,
}

impl ObjectId {
    /// Derive an id from an observed row.
    #[must_use]
    pub fn of(kind: ResourceKind, row: &Row) -> Self {
        Self {
            uid: row.uid.clone(),
            kind,
            namespace: row.namespace.clone(),
            name: row.name.clone(),
        }
    }

    #[must_use]
    pub fn uid(&self) -> &Uid {
        &self.uid
    }
    #[must_use]
    pub fn kind(&self) -> ResourceKind {
        self.kind
    }
    #[must_use]
    pub fn namespace(&self) -> Option<&str> {
        self.namespace.as_deref()
    }
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }
}

impl ObjectId {
    /// Decide a grip against a freshly-read row set — **the one place the uid
    /// comparison lives.**
    ///
    /// Every `ClusterEnv` implementation routes through this rather than
    /// comparing for itself. That is deliberate: the comparison IS the seal,
    /// and four copies of it are four chances for one to compare names. A
    /// backend supplies rows; it does not get to decide what counts as the
    /// same object.
    ///
    /// # Errors
    ///
    /// [`GripError::Recycled`] when the name resolves to a different uid, and
    /// [`GripError::Vanished`] when it resolves to nothing.
    pub fn grip_against(&self, rows: &[Row]) -> Result<Grip, GripError> {
        // uid first: the object may have been renamed-around, and identity is
        // the uid, not the (namespace, name) it currently answers to.
        if let Some(row) = rows.iter().find(|r| r.uid == self.uid) {
            return Ok(Grip::hold(self.clone(), row.clone()));
        }
        // The name still resolves, to something else — the recycle case.
        if let Some(other) = rows
            .iter()
            .find(|r| r.name == self.name && r.namespace == self.namespace)
        {
            return Err(GripError::Recycled {
                name: self.name.clone(),
                expected: self.uid.clone(),
                found: other.uid.clone(),
            });
        }
        Err(GripError::Vanished {
            kind: self.kind,
            name: self.name.clone(),
        })
    }
}

/// Why a grip could not be taken.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GripError {
    /// The object is gone. Acting on it would address nothing.
    Vanished { kind: ResourceKind, name: String },
    /// **The name still resolves — to a DIFFERENT object.**
    ///
    /// The name-recycle case, and the reason this whole type exists.
    /// client-go's `shared_informer.go` documents it verbatim: delete `X` with
    /// uid `A`, create `X` with uid `B`, and a watcher sees an *update* from
    /// the first to the second, never the delete-then-create. Anything keyed on
    /// the name would act on `B` believing it is `A`.
    Recycled {
        name: String,
        expected: Uid,
        found: Uid,
    },
    /// The backend could not be read, so banken does not KNOW whether the
    /// object still exists. Distinct from `Vanished` on purpose: "it is gone"
    /// and "I cannot see" must never collapse, or a blind read authorises an
    /// act the same way a clean one does.
    Blind { kind: ResourceKind, message: String },
}

impl std::fmt::Display for GripError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GripError::Vanished { kind, name } => {
                f.write_str("`")?;
                f.write_str(name)?;
                f.write_str("` (")?;
                f.write_str(kind.label())?;
                f.write_str(") no longer exists — refusing to act on it")
            }
            GripError::Recycled {
                name,
                expected,
                found,
            } => {
                f.write_str("`")?;
                f.write_str(name)?;
                f.write_str("` is a DIFFERENT object now (was uid ")?;
                f.write_str(expected.as_str())?;
                f.write_str(", is uid ")?;
                f.write_str(found.as_str())?;
                f.write_str(") — the name was recycled; refusing to act on it")
            }
            GripError::Blind { kind, message } => {
                f.write_str("cannot re-read the ")?;
                f.write_str(kind.label())?;
                f.write_str(" to confirm it is still the object you selected: ")?;
                f.write_str(message)
            }
        }
    }
}

/// A re-verified hold on ONE live object, valid only inside the dispatch that
/// took it.
///
/// # This is the postigo × cache seal, and it is a seal by ABSENCE
///
/// banken previews a `(defbancada)` on `g` and opens it on `enter`. Between the
/// two, the selected object can be deleted, replaced, or have its name
/// recycled — and with an absorbed cache the window is the cache's TTL rather
/// than a poll interval. The plan the operator saw stays exactly what runs
/// (that is what makes the preview honest); what must be re-established is that
/// the SUBJECT is still the thing they looked at.
///
/// # Why it is `!Send`, and why that is the mechanism rather than a lint
///
/// `PhantomData<*const ()>` makes this type `!Send`. `AsyncApp` requires the
/// app's state to be `Send`, so a `Grip` **cannot be stored in the app between
/// preview and confirm** — the compiler refuses. There is no discipline to
/// forget: carrying a stale authorisation forward is not a thing an author can
/// write.
///
/// Every earlier design in this program tried to carry a freshness *witness*
/// forward instead, and that was built and broken: rows launder through
/// `set_rows` into `egaku::TableView<Row>` — a foreign container that knows
/// nothing about freshness — so a stale reading held beside a live table
/// `cargo check`s clean. Re-deriving at the act dissolves the launder point
/// instead of guarding it.
///
/// # Honest tier
///
/// **Truly-unrepresentable within this authored surface** for *carrying* a grip
/// across the await, and for staging without one (the argument type). It is
/// **only-mitigated** that the object does not change between the grip and the
/// act itself — that window is microseconds rather than the operator's think
/// time, but it is not zero, and no local type can close it. See BANKEN.md C1.
///
/// # The `!Send` seal, asserted
///
/// Rust has no stable negative trait bounds, so this cannot be a normal
/// `#[test]` — an earlier attempt used a method-resolution probe and was
/// VACUOUS: making `Grip` `Send` left all five grip tests green, because
/// fully-qualified trait syntax always resolves to the blanket impl and can
/// never become ambiguous. A `compile_fail` doctest is the real assertion.
///
/// ```compile_fail
/// # use banken_spec::env::Grip;
/// fn requires_send<T: Send>(_: T) {}
/// let g: Grip = unimplemented!();
/// requires_send(g); // Grip is !Send — this must NOT compile
/// ```
#[derive(Debug)]
pub struct Grip {
    row: Row,
    id: ObjectId,
    _not_send: std::marker::PhantomData<*const ()>,
}

impl Grip {
    /// The **sole** constructor, and it is crate-private on purpose: a grip
    /// exists only because [`ClusterEnv::grip`] re-read the object and the uid
    /// still matched. Nothing else can mint one.
    pub(crate) fn hold(id: ObjectId, row: Row) -> Self {
        Self {
            row,
            id,
            _not_send: std::marker::PhantomData,
        }
    }

    /// The object as re-read AT THE ACT — not as previewed.
    #[must_use]
    pub fn row(&self) -> &Row {
        &self.row
    }

    /// What was gripped.
    #[must_use]
    pub fn id(&self) -> &ObjectId {
        &self.id
    }
}

#[cfg(test)]
mod grip_tests {
    use super::*;

    fn row(uid: &str, ns: &str, name: &str) -> Row {
        Row {
            uid: Uid::new(uid).expect("non-blank"),
            name: name.to_owned(),
            namespace: Some(ns.to_owned()),
            version: Some("1".to_owned()),
            cells: Vec::new(),
        }
    }

    fn id_of(uid: &str, ns: &str, name: &str) -> ObjectId {
        ObjectId::of(ResourceKind::Pod, &row(uid, ns, name))
    }

    #[test]
    fn a_grip_is_taken_when_the_uid_still_matches() {
        let id = id_of("A", "catch", "api");
        let g = id
            .grip_against(&[row("A", "catch", "api")])
            .expect("same object");
        assert_eq!(g.id().uid().as_str(), "A");
        assert_eq!(g.row().name, "api");
    }

    /// THE case this whole type exists for.
    ///
    /// The name still resolves — to a different object. Anything keyed on the
    /// name would act on `B` believing it is `A`, which is exactly what
    /// client-go documents a watcher cannot tell you.
    #[test]
    fn a_recycled_name_refuses_and_names_both_uids() {
        let id = id_of("A", "catch", "api");
        let err = id
            .grip_against(&[row("B", "catch", "api")])
            .expect_err("a recycled name must refuse");

        match &err {
            GripError::Recycled {
                name,
                expected,
                found,
            } => {
                assert_eq!(name, "api");
                assert_eq!(expected.as_str(), "A");
                assert_eq!(found.as_str(), "B");
            }
            other => panic!("expected Recycled, got {other:?}"),
        }
        // The message must carry BOTH uids: an operator who is refused needs to
        // see that the name is the same and the object is not.
        let msg = err.to_string();
        assert!(msg.contains("A") && msg.contains("B"), "got: {msg}");
    }

    #[test]
    fn a_vanished_object_refuses() {
        let id = id_of("A", "catch", "api");
        let err = id
            .grip_against(&[row("C", "catch", "other")])
            .expect_err("gone means gone");
        assert!(matches!(err, GripError::Vanished { .. }), "got {err:?}");
    }

    /// A rename is NOT a recycle: same object, different name. Identity is the
    /// uid, so the grip holds — and the row it returns carries the NEW name,
    /// because the act must address the object as it is now.
    #[test]
    fn a_renamed_object_still_grips_and_reports_its_current_name() {
        let id = id_of("A", "catch", "api");
        let g = id
            .grip_against(&[row("A", "catch", "api-v2")])
            .expect("same uid, same object");
        assert_eq!(g.row().name, "api-v2");
    }
}
