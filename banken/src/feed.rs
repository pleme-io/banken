//! The live pod feed — banken's absorb plane (QUADRO T13).
//!
//! Distinct from [`crate::live`], which is the *backend* (`KubeClusterEnv`,
//! where rows come from). This module is the *cadence*: what re-reads that
//! backend on its own, without an operator pressing a key.
//!
//! # What this closes
//!
//! `pending-banken: live-watch`. Before this module, `BankenApp::refresh` had
//! **zero callers**: `egaku_term::run_async` parked on a single
//! `events.next()`, so the only thing that could ever move the table was a
//! keystroke. Against the static fixture that is invisible — the data never
//! changes — but against the live cluster proven in [`crate::live`], the
//! table was simply frozen at whatever `try_new` read once at startup.
//!
//! # Why a feed and not a poll on the app task
//!
//! The obvious fix — sleep a second on the app task, then call `refresh()` —
//! works against the fixture and is wrong against a cluster.
//! `ClusterEnv::list_resources` is a **synchronous** call that, in the `live`
//! build, is an HTTPS round trip to an apiserver. Running it on the task that
//! owns the terminal blocks input for the duration of every read, so a slow
//! or unreachable cluster presents as a wedged UI — and an unreachable
//! cluster is exactly when an operator most needs the keyboard.
//!
//! So the read runs where a blocking read belongs. `izumi::refresh` already
//! owns this shape and is the fleet's one implementation of it:
//! `spawn_interval_refresh` runs the closure on tokio's **blocking pool**,
//! hands the result to a `LiveStore` on the async side, and — the part that
//! matters most here — **preserves the last-known value when a refresh
//! panics** rather than wiping it. banken consumes that; it does not
//! re-implement it. This is izumi's second consumer, and its first outside
//! mado, which is the point: izumi's own docs call it "the generalized
//! extraction of mado's Ctrl-S suggestion plane", and until now the
//! extraction had happened without the redistribution.
//!
//! # The two halves
//!
//! The store is written by the spawned task and read by the app. The handoff
//! is `egaku_term::AsyncApp`'s wakeup pair: [`PodFeed::changed`] is the
//! `&self` signal — cancellation-safe, dropped un-polled on every keystroke —
//! and [`PodFeed::rows`] is what the `&mut self` apply reads. Nothing is
//! written inside the signal, which is what keeps a half-applied refresh off
//! the table.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use banken_spec::env::{ClusterEnv, Row};
use banken_spec::types::ResourceKind;
use izumi::refresh::{LiveStore, StopFlag, spawn_interval_refresh};
use tokio::sync::watch;

/// How often the feed re-reads the cluster when the caller does not say.
///
/// One second is BANKEN.md §VI M0's stated poll cadence. It is a *poll*, and
/// naming it one is deliberate: a true `kube` watch informer is M1, and
/// calling this a watch is precisely the round-up that `pending-banken:
/// live-watch` was written to prevent.
pub const DEFAULT_POLL: Duration = Duration::from_secs(1);

/// A running background read of one resource kind.
///
/// Dropping it stops the task — the [`StopFlag`] is flipped in [`Drop`] — so
/// a feed cannot outlive the app that owns it and leave an apiserver poll
/// running against a terminal nobody is watching.
pub struct PodFeed {
    store: Arc<LiveStore<Vec<Row>>>,
    rx: watch::Receiver<u64>,
    stop: StopFlag,
    /// Set by the panic hook. Surfaced so a status line can say the feed
    /// stopped producing, rather than showing stale rows as if they were
    /// current.
    faulted: Arc<AtomicBool>,
}

impl PodFeed {
    /// Start reading `kind` from `env` every `interval`.
    ///
    /// The first read fires immediately (tokio interval semantics), so rows
    /// appear shortly after start rather than one whole interval later.
    ///
    /// A read returning `Err` leaves the previous rows in place: a transient
    /// apiserver failure must not blank a table an operator is reading. Same
    /// reasoning as `try_new`'s `unwrap_or_default` on the initial read,
    /// applied to the steady state.
    ///
    /// Must be called from within a tokio runtime (`tokio::spawn`'s
    /// requirement).
    #[must_use]
    pub fn start<E>(env: Arc<E>, kind: ResourceKind, interval: Duration) -> Self
    where
        E: ClusterEnv + Send + Sync + 'static,
    {
        let store = LiveStore::new(Vec::<Row>::new());
        let rx = store.subscribe();
        let stop: StopFlag = Arc::new(AtomicBool::new(false));
        let faulted = Arc::new(AtomicBool::new(false));

        let sink_store = Arc::clone(&store);
        let panic_flag = Arc::clone(&faulted);

        spawn_interval_refresh(
            interval,
            Arc::clone(&stop),
            // `Option<Vec<Row>>`, not `Vec<Row>`: a failed read has to stay
            // distinguishable from a genuinely empty cluster. Collapsing them
            // would render "the apiserver is unreachable" identically to
            // "there are no pods here" — and only one of those means the
            // operator should stop trusting the screen.
            Arc::new(move || env.list_resources(kind, None).ok()),
            move |rows: Option<Vec<Row>>| {
                if let Some(rows) = rows {
                    sink_store.push(rows);
                }
            },
            move || panic_flag.store(true, Ordering::SeqCst),
        );

        Self {
            store,
            rx,
            stop,
            faulted,
        }
    }

    /// Resolve when the feed has produced new rows.
    ///
    /// The `&self` signal half of the `AsyncApp` wakeup. It only observes, so
    /// being dropped un-polled — which happens on every keystroke that wins
    /// the runtime's select — costs nothing and can leave nothing written.
    pub async fn changed(&self) {
        let mut rx = self.rx.clone();
        let _ = rx.changed().await;
    }

    /// The most recently absorbed rows.
    #[must_use]
    pub fn rows(&self) -> Vec<Row> {
        self.store.snapshot()
    }

    /// Whether a refresh panicked. [`Self::rows`] then holds the last good set.
    #[must_use]
    pub fn faulted(&self) -> bool {
        self.faulted.load(Ordering::SeqCst)
    }

    /// How many times the feed has published new rows.
    ///
    /// `0` means it has not completed a read yet. Exposed so a test can wait
    /// for the feed to actually produce something rather than sleeping a
    /// guessed duration and hoping — the difference between a deterministic
    /// assertion and a flake.
    #[must_use]
    pub fn generation(&self) -> u64 {
        self.store.generation()
    }
}

impl Drop for PodFeed {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}
