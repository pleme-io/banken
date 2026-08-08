//! The `:pods` table moves without a keystroke.
//!
//! This is the concrete predicate for `pending-banken: live-watch`'s poll
//! half. Before egaku-term 0.3.4 the test could not be written at all:
//! `run_async` parked on `events.next()`, `BankenApp::refresh` had zero
//! callers, and there was no non-input path into the loop. Now there is one,
//! and this exercises it end to end — a background read on tokio's blocking
//! pool, published through `izumi::refresh`, applied by `on_wake`, visible in
//! the table.
//!
//! Every assertion here is reached with **zero key events injected**. That is
//! the whole claim; a test that pressed a key would prove nothing new.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use banken::absorb::Despensa;
use banken::app::BankenApp;
use banken::session::UnwiredSessionEnv;
use banken_spec::SpecError;
use banken_spec::env::{
    ChangeRef, ClusterEnv, DeclareChange, DepTree, Event, GlassRecord, HealthReading, LogStream,
    ResourceRef, Row, WatchStream, WitnessedAction, Workload,
};
use banken_spec::types::{OperatorId, ResourceKind};
use egaku_term::AsyncApp;

/// A cluster whose pod list changes on its own, one new pod per read.
///
/// `MockClusterEnv` cannot be used here: it records through `RefCell`, so it
/// is not `Sync`, and the feed reads from a spawned task. That is not a flaw
/// in the mock — a recording double is single-threaded on purpose — it just
/// means a *concurrent* seam needs an atomic one.
struct TickingEnv {
    /// Shared with the test so the read count OUTLIVES the app — which is
    /// the only way to assert that dropping the app actually stopped the
    /// background reads rather than merely losing the handle to them.
    reads: Arc<AtomicUsize>,
}

impl TickingEnv {
    fn new() -> Self {
        Self::sharing(Arc::new(AtomicUsize::new(0)))
    }

    fn sharing(reads: Arc<AtomicUsize>) -> Self {
        Self { reads }
    }

    fn pod(n: usize) -> Row {
        Row {
            uid: banken_spec::env::Uid::new(format!("t-{n}")).expect("non-blank"),
            version: None,
            name: format!("pod-{n}"),
            namespace: Some("default".into()),
            cells: vec![
                ("NAME".into(), format!("pod-{n}")),
                ("READY".into(), "1/1".into()),
                ("STATUS".into(), "Running".into()),
            ],
        }
    }
}

impl ClusterEnv for TickingEnv {
    fn grip(
        &self,
        id: &banken_spec::env::ObjectId,
    ) -> Result<banken_spec::env::Grip, banken_spec::env::GripError> {
        let rows = self
            .list_resources(id.kind(), id.namespace())
            .map_err(|e| banken_spec::env::GripError::Blind {
                kind: id.kind(),
                message: e.to_string(),
            })?;
        id.grip_against(&rows)
    }

    /// Read N returns N pods, so "did the feed run again" is answerable from
    /// the table contents alone rather than from a spy counter.
    fn list_resources(&self, kind: ResourceKind, _ns: Option<&str>) -> Result<Vec<Row>, SpecError> {
        if kind != ResourceKind::Pod {
            return Ok(Vec::new());
        }
        let n = self.reads.fetch_add(1, Ordering::SeqCst) + 1;
        Ok((1..=n).map(Self::pod).collect())
    }

    fn get_resource(
        &self,
        _kind: ResourceKind,
        name: &str,
        _ns: Option<&str>,
    ) -> Result<Row, SpecError> {
        // `..Row::default()` used to close this literal. It cannot now, and
        // that is the point: `Row` no longer derives `Default`, so a row must
        // be spelled out rather than half-invented. Every field here is an
        // explicit test value.
        Ok(Row {
            uid: banken_spec::env::Uid::new(format!("t-{name}")).expect("non-blank"),
            version: None,
            name: name.to_owned(),
            namespace: None,
            cells: Vec::new(),
        })
    }

    // The rest of the OBSERVE surface is not what this test is about. They
    // refuse rather than return a plausible empty value — an unexercised seam
    // answering `Ok` is how a test starts passing for the wrong reason.
    fn logs(&self, _pod: &str, _ns: &str, _follow: bool) -> Result<LogStream, SpecError> {
        Err(unused("logs"))
    }
    fn events(&self, _ns: Option<&str>) -> Result<Vec<Event>, SpecError> {
        Err(unused("events"))
    }
    fn topology(&self, _root: &ResourceRef) -> Result<DepTree, SpecError> {
        Err(unused("topology"))
    }
    fn health_signals(&self, _w: &Workload) -> Result<HealthReading, SpecError> {
        Err(unused("health-signals"))
    }
    fn watch(&self, _kind: ResourceKind, _ns: Option<&str>) -> Result<WatchStream, SpecError> {
        Err(unused("watch"))
    }
    fn declare(&self, _change: DeclareChange) -> Result<ChangeRef, SpecError> {
        Err(unused("declare"))
    }
    fn break_glass(&self, _action: WitnessedAction) -> Result<GlassRecord, SpecError> {
        Err(unused("break-glass"))
    }
}

fn unused(phase: &str) -> SpecError {
    SpecError::Interp {
        phase: phase.to_owned(),
        message: "not exercised by the feed test".to_owned(),
    }
}

/// A counting allocator, so "this call is free" is an assertion rather than a
/// hope. Scoped to this test binary only.
struct Counting;

static ALLOCATED: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

unsafe impl std::alloc::GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: std::alloc::Layout) -> *mut u8 {
        ALLOCATED.fetch_add(l.size(), Ordering::Relaxed);
        unsafe { std::alloc::System.alloc(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: std::alloc::Layout) {
        unsafe { std::alloc::System.dealloc(p, l) }
    }
}

#[global_allocator]
static COUNTING_ALLOC: Counting = Counting;

struct CountingHandle;
static COUNTING: CountingHandle = CountingHandle;
impl CountingHandle {
    fn reset(&self) {
        ALLOCATED.store(0, Ordering::SeqCst);
    }
    fn bytes(&self) -> usize {
        ALLOCATED.load(Ordering::SeqCst)
    }
}

fn app() -> BankenApp<TickingEnv, UnwiredSessionEnv> {
    app_counting(Arc::new(AtomicUsize::new(0)))
}

fn app_counting(reads: Arc<AtomicUsize>) -> BankenApp<TickingEnv, UnwiredSessionEnv> {
    BankenApp::try_new(
        TickingEnv::sharing(reads),
        UnwiredSessionEnv::new(),
        OperatorId::new("drzzln").expect("a literal witness is non-blank"),
        "source: ticking",
    )
    .expect("the shipped vocabulary must build an app")
}

/// Wait until the feed has published at least `want` generations, or give up.
///
/// Polls the generation counter rather than sleeping a guessed duration: the
/// difference between an assertion and a flake. Returns whether it got there.
async fn feed_reached(feed: &Despensa, want: u64) -> bool {
    for _ in 0..200 {
        if feed.snapshot().generation() >= want {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    false
}

/// THE test. The table's contents change with no key ever pressed.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_pod_table_moves_without_a_keystroke() {
    let mut app = app().with_feed(Duration::from_millis(20));

    // `try_new`'s own initial read is read #1, so the table starts at 1 pod.
    let before = app.table().view().rows().len();
    assert_eq!(before, 1, "the constructor's inline read seeds one pod");

    let feed = app.absorber().expect("with_feed must attach an absorber");
    assert!(
        feed_reached(feed, 3).await,
        "the background feed must publish on its own cadence; it reached generation {}",
        feed.snapshot().generation()
    );

    // The wakeup pair, exactly as `run_async` drives it — and nothing else.
    tokio::time::timeout(Duration::from_secs(5), app.wake())
        .await
        .expect("wake() must resolve from the feed, with no input");
    app.on_wake().await.expect("on_wake must apply cleanly");

    let after = app.table().view().rows().len();
    assert!(
        after > before,
        "the table must have absorbed newer rows without a keystroke: {before} -> {after}"
    );
}

/// An app with no feed is input-only, byte-for-byte as before. This is what
/// makes the change safe for every synchronous test and every consumer that
/// never opts in.
#[tokio::test]
async fn an_app_without_a_feed_never_wakes() {
    let app = app();
    assert!(app.absorber().is_none(), "no absorber unless asked for one");
    let r = tokio::time::timeout(Duration::from_millis(80), app.wake()).await;
    assert!(
        r.is_err(),
        "without a feed the wakeup must be pending() — input-only, as before"
    );
}

/// Dropping the app stops the background read. A feed that outlived its
/// terminal would keep polling an apiserver for a UI nobody is looking at —
/// and because the task is detached, nothing else would ever stop it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn dropping_the_app_stops_the_feed() {
    let reads = Arc::new(AtomicUsize::new(0));
    {
        let app = app_counting(Arc::clone(&reads)).with_feed(Duration::from_millis(10));
        let feed = app.absorber().expect("absorber attached");
        assert!(feed_reached(feed, 3).await, "feed must start producing");
    } // app — and with it the feed, and with that the StopFlag — dropped here

    let at_drop = reads.load(Ordering::SeqCst);
    // 20 intervals' worth of wall clock. A still-running feed would add ~20
    // reads; a stopped one adds at most the single read already in flight.
    tokio::time::sleep(Duration::from_millis(200)).await;
    let after = reads.load(Ordering::SeqCst);

    assert!(
        after <= at_drop + 1,
        "the feed must stop when the app drops: {at_drop} reads at drop, {after} after 200ms"
    );
}

/// A redundant apply must ALLOCATE NOTHING.
///
/// `apply_feed` reaches `egaku::TableView::set_rows` through a `Vec<Row>`, so
/// every call deep-clones every `Row` and every `String` in every cell —
/// measured elsewhere at **10.16 ms and 4.0 MB per call at N=10 000**. The
/// generation gate removes every redundant one.
///
/// # Why this asserts on BYTES and not on the cursor
///
/// An earlier version of this test moved the selection and asserted it
/// survived. **It passed with the gate deleted** — `set_rows` preserves the
/// selection index, so a redundant apply has no user-visible symptom at all.
/// That test was vacuous: it would have shipped a gate nobody could prove.
///
/// The cost IS the symptom, so the cost is what gets asserted. Deleting the
/// `if generation == self.applied_generation { return }` arm turns this red
/// with a non-zero byte count.
#[tokio::test]
async fn a_redundant_apply_allocates_nothing() {
    let mut app = app().with_feed(Duration::from_millis(20));
    let feed = app.absorber().expect("absorber attached");
    assert!(feed_reached(feed, 2).await, "the feed must produce");

    // First apply does real work — it must, or the measurement below is
    // measuring an app that never absorbed anything.
    COUNTING.reset();
    app.apply_feed();
    let first = COUNTING.bytes();
    assert!(
        first > 0,
        "the first apply must actually build the table; it allocated {first} bytes"
    );

    // Second apply, same generation: the gate must make it free.
    COUNTING.reset();
    app.apply_feed();
    let redundant = COUNTING.bytes();

    assert_eq!(
        redundant, 0,
        "a redundant apply must allocate nothing; it allocated {redundant} bytes \
         (the first, real, apply allocated {first})"
    );
}
