//! The absorb plane — a watch-fed replica of one resource kind.
//!
//! This replaces [`crate::feed`]'s 1 Hz poll. The difference is not a tuning
//! knob, it is a change of what the network cost is proportional to.
//!
//! # The measurement that motivates it
//!
//! Against `camelot-eks` on 2026-08-08, 191 pods:
//! `Api::list(&ListParams::default())` at `DEFAULT_POLL = 1s` moves **3,580,862
//! bytes per tick** — 3.4 MiB/s decoded, 12 GiB/hour, **96 GiB per 8-hour day,
//! per running instance**. A 30-second `watch` against the same cluster at the
//! same moment moved **0 bytes**.
//!
//! The cluster was quiet, so that zero is not a trick — it is the whole point:
//! **delta traffic is proportional to CHANGE; poll traffic is proportional to
//! STATE SIZE.** banken was paying for the entire cluster, once per second, to
//! learn that nothing had happened.
//!
//! # Two rules inherited from QUADRO §T14, and one added here
//!
//! 1. **The read never runs on the app task.** Each kind's stream owns a
//!    `tokio::spawn`ed task; the app only ever loads a pointer.
//! 2. **The signal never writes.** [`Despensa::changed`] is `&self` and
//!    cancellation-safe; the apply is a separate `&mut self` step. A
//!    half-applied refresh stays unrepresentable.
//! 3. **A publish that changes nothing does not wake the renderer.**
//!    `izumi::refresh::LiveStore::push` bumps its generation unconditionally, so
//!    the old feed redrew ~60 times a minute against a cluster where nothing
//!    moved. Here the generation advances only when the content hash does.
//!
//! # Why `ArcSwap` and not a lock
//!
//! The render path touches this every frame. `ArcSwap` makes a read one atomic
//! load with no contention against the writing task, and it is the fleet's
//! established shape for a read-hot path (20 crates declare `arc-swap`). The
//! published [`Snapshot`] is immutable, so a frame drawn from it cannot tear.
//!
//! # Honest scope of this module (M0)
//!
//! It absorbs and it publishes. It does **not** yet carry identity: a `Row` has
//! no `uid`, so the replica is keyed on `(namespace, name)`, which is unique at
//! an instant but **not across delete-and-recreate**. Acting on a row is
//! therefore still no safer than it was — that is `pending-banken: grip`, the
//! M1 seal, and this module deliberately does not pretend otherwise.

use std::collections::BTreeMap;
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use arc_swap::ArcSwap;
use banken_spec::env::Row;
use tokio::sync::watch;

/// How the replica currently stands with respect to its upstream.
///
/// This is the axis banken has never had. `Vec<Row>` cannot say "these rows are
/// eight seconds stale and my watch is dead", so a stale table and a live one
/// rendered identically — the false-calm class the `WardVerdict` cap forbids one
/// layer up, ungoverned here until now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyncPhase {
    /// The initial stream has not completed. Rows may be partial.
    ///
    /// Distinct from `Synced` with an empty set: "I have not finished looking"
    /// and "I looked and there is nothing" are different answers, and only one
    /// of them means the operator should keep waiting.
    Absorbing,
    /// The initial list completed and the watch is live.
    Synced,
    /// The stream failed. The rows are the last good set, and `cause` says why
    /// they have stopped moving.
    ///
    /// The rows are *kept*, not blanked: a transient apiserver failure must not
    /// erase a table an operator is reading. What changes is the claim made
    /// about them.
    Degraded { cause: String },
}

/// An immutable published view of one kind.
///
/// Fields are private and there is no public constructor other than
/// [`Snapshot::empty`], so a snapshot claiming `Synced` cannot be fabricated by
/// a caller — the plane is the only thing that can mint one. This is the
/// `WardVerdict` shape (private fields, one constructor) applied to freshness.
#[derive(Debug, Clone)]
pub struct Snapshot {
    rows: Arc<[Row]>,
    phase: SyncPhase,
    /// Monotonic counter of *content-changing* publishes. Not a clock — a clock
    /// would make this untestable without sleeping.
    generation: u64,
    /// Hash of `rows`. The wake gate: a publish whose hash matches the previous
    /// one does not advance `generation` and does not notify.
    content: u64,
}

impl Snapshot {
    /// The pre-absorption state: no rows, and honestly labelled as still
    /// looking rather than as an empty cluster.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            rows: Arc::from(Vec::new()),
            phase: SyncPhase::Absorbing,
            generation: 0,
            content: 0,
        }
    }

    /// The absorbed rows. Cheap: an `Arc` clone, never a `Vec` clone.
    ///
    /// The old feed's `rows()` cloned the whole `Vec<Row>` — every `String` in
    /// every cell — on every frame.
    #[must_use]
    pub fn rows(&self) -> Arc<[Row]> {
        Arc::clone(&self.rows)
    }

    /// How this reading stands. Always available, never inferred by the caller.
    #[must_use]
    pub fn phase(&self) -> &SyncPhase {
        &self.phase
    }

    /// Content-changing publishes so far. `0` means nothing has been absorbed.
    #[must_use]
    pub fn generation(&self) -> u64 {
        self.generation
    }
}

/// Hash a row set for the wake gate.
///
/// Deliberately hashes the *rendered* cells rather than the source objects:
/// what the operator sees is the thing that must not redraw when it has not
/// changed. A `resourceVersion` bump that alters no visible cell is exactly the
/// wake this suppresses.
fn content_hash(rows: &[Row]) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for r in rows {
        r.name.hash(&mut h);
        r.namespace.hash(&mut h);
        for (k, v) in &r.cells {
            k.hash(&mut h);
            v.hash(&mut h);
        }
    }
    h.finish()
}

/// The published store for one kind, plus its change signal.
///
/// Cloneable and cheap; the plane writes through one handle while the app reads
/// through another.
pub struct Despensa {
    snap: Arc<ArcSwap<Snapshot>>,
    rx: watch::Receiver<u64>,
    /// Set when the owning task ends. Surfaced so a status line can distinguish
    /// "quiet" from "stopped".
    stopped: Arc<AtomicU64>,
}

impl Despensa {
    /// The current published snapshot — one atomic load.
    #[must_use]
    pub fn snapshot(&self) -> Arc<Snapshot> {
        self.snap.load_full()
    }

    /// Resolve when the content has changed.
    ///
    /// The `&self` half of the `AsyncApp` wakeup pair: it observes only, so
    /// being dropped un-polled — which happens on every keystroke that wins the
    /// runtime's `select!` — costs nothing and can leave nothing written.
    pub async fn changed(&self) {
        let mut rx = self.rx.clone();
        let _ = rx.changed().await;
    }

    /// Whether the absorbing task has ended.
    #[must_use]
    pub fn stopped(&self) -> bool {
        self.stopped.load(Ordering::SeqCst) != 0
    }
}

/// The write side, held by the absorbing task.
///
/// Split from [`Despensa`] so that the app's handle has no publish method at
/// all — the app cannot fabricate a reading even by accident, for the same
/// reason `ClusterEnv` has no mutate method.
pub struct Publisher {
    snap: Arc<ArcSwap<Snapshot>>,
    tx: watch::Sender<u64>,
    stopped: Arc<AtomicU64>,
    generation: u64,
}

impl Publisher {
    /// Publish a row set, waking subscribers **only if the content changed**.
    ///
    /// Returns whether it woke anyone — the value the wake-gate test asserts on.
    pub fn publish(&mut self, rows: Vec<Row>, phase: SyncPhase) -> bool {
        let content = content_hash(&rows);
        let prev = self.snap.load();
        // A phase change is a content change for the operator's purposes: going
        // Synced -> Degraded must repaint the status line even though not one
        // row moved. Comparing only the hash would leave a dead watch rendering
        // as a healthy one, which is the exact failure this module exists to
        // make visible.
        let changed = content != prev.content || phase != prev.phase;
        if !changed {
            return false;
        }
        self.generation += 1;
        self.snap.store(Arc::new(Snapshot {
            rows: Arc::from(rows),
            phase,
            generation: self.generation,
            content,
        }));
        // A send error means every receiver is gone, i.e. the app has exited.
        // That is a normal shutdown, not a failure to report.
        let _ = self.tx.send(self.generation);
        true
    }

    /// Mark the absorbing task ended.
    pub fn mark_stopped(&self) {
        self.stopped.store(1, Ordering::SeqCst);
    }
}

/// Create a linked reader/writer pair, both starting at [`Snapshot::empty`].
#[must_use]
pub fn channel() -> (Despensa, Publisher) {
    let snap = Arc::new(ArcSwap::from(Arc::new(Snapshot::empty())));
    let (tx, rx) = watch::channel(0u64);
    let stopped = Arc::new(AtomicU64::new(0));
    (
        Despensa {
            snap: Arc::clone(&snap),
            rx,
            stopped: Arc::clone(&stopped),
        },
        Publisher {
            snap,
            tx,
            stopped,
            generation: 0,
        },
    )
}

/// The in-flight replica a watch stream folds into.
///
/// Keyed on `(namespace, name)` for M0. That key is unique at an instant and
/// **not** across delete-and-recreate — see the module note; identity is M1.
#[derive(Default)]
pub struct Replica {
    live: BTreeMap<(Option<String>, String), Row>,
    /// Filled by `InitApply` between `Init` and `InitDone`, then swapped in
    /// whole. Buffering is not an optimization: kube-rs's contract is that any
    /// object previously applied but absent from the init series **has been
    /// deleted**, and only a whole-set swap expresses that. Merging init events
    /// into the live map instead would leave deleted objects immortal.
    building: Option<BTreeMap<(Option<String>, String), Row>>,
}

/// What a folded event did to the replica.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Folded {
    /// The replica changed and should be published.
    Changed,
    /// Nothing observable changed (an init event mid-stream, or a re-apply of
    /// an identical row).
    Quiet,
    /// The initial set completed — publish and flip to `Synced`.
    InitComplete,
}

impl Replica {
    /// Fold one watch event.
    ///
    /// Generic over the event shape rather than taking `kube::runtime::watcher::Event`
    /// so the fold is exercised by a tape of canned events with no cluster and
    /// no `kube` types in the test — the seam that makes the M2 `WatchTape`
    /// possible.
    pub fn fold(&mut self, ev: Ev) -> Folded {
        match ev {
            Ev::Init => {
                self.building = Some(BTreeMap::new());
                Folded::Quiet
            }
            Ev::InitApply(row) => {
                let key = (row.namespace.clone(), row.name.clone());
                match self.building.as_mut() {
                    Some(b) => {
                        b.insert(key, row);
                    }
                    // An InitApply with no preceding Init is a protocol
                    // violation; treat it as a live apply rather than dropping
                    // it, so a row is never silently lost.
                    None => {
                        self.live.insert(key, row);
                    }
                }
                Folded::Quiet
            }
            Ev::InitDone => {
                if let Some(b) = self.building.take() {
                    self.live = b;
                }
                Folded::InitComplete
            }
            Ev::Apply(row) => {
                let key = (row.namespace.clone(), row.name.clone());
                let changed = self.live.get(&key) != Some(&row);
                self.live.insert(key, row);
                if changed {
                    Folded::Changed
                } else {
                    Folded::Quiet
                }
            }
            Ev::Delete(row) => {
                let key = (row.namespace.clone(), row.name.clone());
                if self.live.remove(&key).is_some() {
                    Folded::Changed
                } else {
                    Folded::Quiet
                }
            }
        }
    }

    /// The replica as a published row set, in stable key order.
    #[must_use]
    pub fn rows(&self) -> Vec<Row> {
        self.live.values().cloned().collect()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.live.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.live.is_empty()
    }
}

/// A projected watch event — the fold's input, free of `kube` types.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ev {
    Init,
    InitApply(Row),
    InitDone,
    Apply(Row),
    Delete(Row),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(ns: &str, name: &str, status: &str) -> Row {
        Row {
            name: name.to_owned(),
            namespace: Some(ns.to_owned()),
            cells: vec![("STATUS".to_owned(), status.to_owned())],
        }
    }

    #[test]
    fn an_init_series_replaces_the_whole_set() {
        let mut r = Replica::default();
        r.fold(Ev::Apply(row("a", "gone", "Running")));
        assert_eq!(r.len(), 1);

        // A restart re-lists only `kept`. kube-rs's contract says `gone` — applied
        // before, absent from the init series — has been deleted.
        r.fold(Ev::Init);
        r.fold(Ev::InitApply(row("a", "kept", "Running")));
        assert_eq!(r.fold(Ev::InitDone), Folded::InitComplete);

        let names: Vec<_> = r.rows().into_iter().map(|x| x.name).collect();
        assert_eq!(
            names,
            vec!["kept"],
            "an object absent from the init series must not survive it"
        );
    }

    #[test]
    fn a_deleted_pod_leaves_the_replica() {
        let mut r = Replica::default();
        r.fold(Ev::Apply(row("a", "p1", "Running")));
        assert_eq!(
            r.fold(Ev::Delete(row("a", "p1", "Running"))),
            Folded::Changed
        );
        assert!(r.is_empty(), "a deleted pod must leave the table");
    }

    #[test]
    fn re_applying_an_identical_row_is_quiet() {
        let mut r = Replica::default();
        assert_eq!(
            r.fold(Ev::Apply(row("a", "p1", "Running"))),
            Folded::Changed
        );
        assert_eq!(
            r.fold(Ev::Apply(row("a", "p1", "Running"))),
            Folded::Quiet,
            "an unchanged re-apply must not be reported as a change"
        );
    }

    #[test]
    fn same_name_in_two_namespaces_are_two_rows() {
        // The bare-name identity bug one layer down: `TableRow::identity()` is
        // the name alone, so this is the case that proves the replica's key is
        // not making the same mistake.
        let mut r = Replica::default();
        r.fold(Ev::Apply(row("a", "same", "Running")));
        r.fold(Ev::Apply(row("b", "same", "Running")));
        assert_eq!(r.len(), 2);
    }

    #[test]
    fn an_unchanged_absorption_does_not_wake_the_renderer() {
        let (d, mut p) = channel();
        let rows = vec![row("a", "p1", "Running")];

        assert!(
            p.publish(rows.clone(), SyncPhase::Synced),
            "first publish wakes"
        );
        let g1 = d.snapshot().generation();

        assert!(
            !p.publish(rows.clone(), SyncPhase::Synced),
            "an identical row set must not wake the renderer"
        );
        assert_eq!(
            d.snapshot().generation(),
            g1,
            "generation must not advance when nothing changed"
        );
    }

    #[test]
    fn a_phase_change_wakes_even_when_no_row_moved() {
        // Going Synced -> Degraded repaints the status line. Gating on the row
        // hash alone would leave a dead watch rendering as a healthy one.
        let (d, mut p) = channel();
        let rows = vec![row("a", "p1", "Running")];
        p.publish(rows.clone(), SyncPhase::Synced);
        let g1 = d.snapshot().generation();

        assert!(
            p.publish(
                rows,
                SyncPhase::Degraded {
                    cause: "connection reset".into()
                }
            ),
            "a phase change must wake even with identical rows"
        );
        assert!(d.snapshot().generation() > g1);
        assert!(matches!(d.snapshot().phase(), SyncPhase::Degraded { .. }));
    }

    #[test]
    fn an_empty_replica_starts_absorbing_not_synced() {
        // "I have not finished looking" and "I looked and there is nothing" are
        // different answers, and only one means the operator should keep waiting.
        let (d, _p) = channel();
        assert_eq!(*d.snapshot().phase(), SyncPhase::Absorbing);
        assert_eq!(d.snapshot().generation(), 0);
    }
}
