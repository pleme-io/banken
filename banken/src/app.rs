//! The banken app runtime — the `:pods` view model + key handling
//! (BANKEN.md §VI M0 / Quadro T3+T4).
//!
//! [`BankenApp`] holds the [`PodTable`] view model, the last postigo
//! [`ActionResult`] panel, and the quit flag. It implements
//! [`egaku_term::AsyncApp`] so `run_async` owns the terminal (alt-screen
//! enter + Drop/panic restore via `egaku_term::Terminal`), pumps the
//! crossterm event stream, and diffs the buffer so only changed cells
//! flush (no full-clear, no flicker on a no-change tick).
//!
//! The render model is a small sum type (`Panel`) so an illegal screen
//! state is unrepresentable (Quadro T4): the app is always showing exactly
//! one of {the table, an action-result overlay, a bancada awaiting
//! confirmation}.
//!
//! # pending-banken: live-watch — the POLL half is closed (2026-08-03)
//!
//! It used to be open in full, and the two prior states of this comment are
//! worth keeping straight because each was wrong in a different way. It first
//! claimed "`run_async` refreshes on every event"; that was corrected to
//! "there is no refresh at all" — `run_async` RE-DREW on every event over
//! rows read once in `try_new`, and `refresh()` had **zero callers**. The
//! correction then concluded that closing this row meant hand-rolling the
//! loop here, "NOT waiting on a tick", because `egaku_term`'s docs advertised
//! an `AsyncApp::tick` that did not exist.
//!
//! That conclusion was right about the phantom method and wrong about the
//! remedy. The load-bearing fix was upstream, not here: `AsyncApp` now has
//! `wake`/`on_wake` and `run_async` selects over them (egaku-term 0.3.4), so
//! banken overrides two methods instead of duplicating a terminal loop —
//! and every other fleet TUI gets the same arm for free rather than
//! hand-rolling its own copy.
//!
//! What is closed: the table now moves without a keystroke, absorbed off-task
//! by [`crate::feed`] over `izumi::refresh`. What is NOT: this is a 1 Hz
//! **poll**, which is what BANKEN.md §VI M0 specifies. A true `kube` watch
//! informer is M1 and unbuilt, so the row stays open under its own name.

use std::sync::Arc;
use std::time::Instant;

use awase::repeat_gate::KeyRepeatGate;
use banken_spec::chord::ActionChord;
use banken_spec::env::{ClusterEnv, Row};
use banken_spec::nav::NavIntent;
use banken_spec::types::{OperatorId, ResourceKind};
use banken_spec::{Catalog, SpecError};
use egaku_term::crossterm::style::Color;
use egaku_term::{__re::KeyMap, AsyncApp, Buffer, Result as TermResult, Style};

use banken_spec::bancada::{BancadaSpec, SessionEnv};

use crate::absorb::{Despensa, SyncPhase};
use crate::action::{
    ActionResult, PendingBancada, RowAction, dispatch, open_bancada, preview_bancada,
    resolve_bancada,
};
use crate::render::{draw_pod_table, sort_label};
use crate::table::PodTable;

/// The one view M0 ships. Named once so the keymap, the table columns and the
/// bancada filter cannot drift onto three different spellings of it.
pub const VIEW_NAME: &str = "pods";

/// The one action the app keymap resolves a key to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Action {
    /// Move the selection down (`down` / `j`).
    SelectNext,
    /// Move the selection up (`up` / `k`).
    SelectPrev,
    /// Cycle the sort direction (`o`).
    ToggleSort,
    /// OBSERVE the selected pod's logs (`l`).
    ObserveLogs,
    /// DECLARE a scale change on the selected pod (`s`).
    DeclareScale,
    /// BREAK-GLASS shell into the selected pod (`shift+s`).
    BreakGlass,
    /// Open the bancada at this index of the app's recipe list (`g`,
    /// `shift+g`) — a pre-warmed tear session plan for the selected row.
    ///
    /// The index is into [`BankenApp::bancadas`], which is
    /// `catalog.bancadas_from(VIEW_NAME)` in order, so the keymap and the
    /// recipe list cannot disagree: [`keymap_from_catalog`] builds both from
    /// the same filtered iteration.
    OpenBancada(usize),
    /// Confirm the previewed action (`enter`).
    ///
    /// The app has exactly one confirmable overlay — a resolved
    /// [`PendingBancada`] — and everywhere else this is a deliberate no-op
    /// rather than an error: pressing `enter` on the table is not a mistake
    /// worth an error panel.
    Confirm,
    /// Dismiss the action-result overlay (`esc`).
    Dismiss,
    /// Quit (`q`).
    Quit,
}

impl Action {
    /// `true` when this action must be debounced against an OS/terminal
    /// key-repeat storm (Quadro T8's `KeyRepeatGate` leg).
    ///
    /// The three `postigo` action chords are gated: each performs real
    /// work (a logs read, a full-manifest lowering, a witnessed record) and
    /// a held key fires one per repeat tick — the mado 2026-05-21
    /// runaway-font shape (25 events in 1.5s) applied to a DECLARE.
    ///
    /// Navigation and lifecycle keys are deliberately **not** gated:
    /// awase's own guidance is that for cursor movement "smooth-feeling
    /// repeat IS the desired behaviour"
    /// (`awase/src/repeat_gate.rs`), and throttling `j`/`k` would make the
    /// table feel broken. Gating what is cheap is not free — it is a
    /// regression.
    #[must_use]
    pub fn is_repeat_gated(self) -> bool {
        matches!(
            self,
            Action::ObserveLogs
                | Action::DeclareScale
                | Action::BreakGlass
                // A bancada resolves a whole recipe against the cluster. A
                // held key firing one per repeat tick is wasteful.
                | Action::OpenBancada(_)
                // And CONFIRM is the expensive one: it opens a real tear
                // session with N panes and stages N commands. A held `enter`
                // firing one session per OS repeat tick is the mado
                // runaway-font shape pointed at the most costly action in
                // the app.
                | Action::Confirm
        )
    }
}

/// Which screen the app currently shows — a sum type so an illegal screen
/// (e.g. "a table AND a modal AND nothing") is unrepresentable.
#[derive(Debug, Clone, PartialEq)]
enum Panel {
    /// The `:pods` table (the default landing).
    Table,
    /// An overlay showing the result of the last postigo action.
    ActionOverlay(ActionResult),
    /// A resolved `(defbancada)` awaiting the operator's `enter`.
    ///
    /// It holds the **typed plan**, not its rendering, which is what makes
    /// the confirm step real: [`Action::Confirm`] opens exactly the plan that
    /// was previewed, not a re-resolution against a table the operator may
    /// have scrolled since. A separate variant rather than a flag on
    /// [`Panel::ActionOverlay`] so "confirmable" is a property of the screen
    /// state rather than something the confirm handler has to infer.
    BancadaPreview(PendingBancada),
}

/// The banken TUI app.
///
/// Generic over **two** seams, for the same reason each time: the runtime is
/// byte-identical against a mock, against a live backend, and against a build
/// that has neither compiled in.
///
/// - `E: ClusterEnv` — where the rows come from
///   ([`FixtureClusterEnv`](crate::fixture::FixtureClusterEnv) /
///   `KubeClusterEnv` / `MockClusterEnv`).
/// - `S: SessionEnv` — where a confirmed `(defbancada)` is opened
///   ([`UnwiredSessionEnv`](crate::session::UnwiredSessionEnv) /
///   `LazyTearSessionEnv` / `MockSessionEnv`).
///
/// The two are independent: banken reads a live cluster and opens sessions in
/// a mock, or reads a fixture and opens real panes, without either seam
/// knowing about the other.
pub struct BankenApp<E: ClusterEnv, S: SessionEnv> {
    /// `Arc` rather than `E` because the background feed
    /// ([`crate::feed::PodFeed`]) needs its own handle to the same backend:
    /// it reads on tokio's blocking pool while the app keeps serving input.
    /// Every constructor takes `impl Into<Arc<E>>`, so a caller still passes
    /// a plain env and nothing at the call sites changed.
    env: Arc<E>,
    /// Where a confirmed bancada is opened. See [`crate::session`].
    session: S,
    operator: OperatorId,
    /// The cluster banken is reading, as the kubeconfig context name.
    ///
    /// **Empty means UNKNOWN, not "the current one".** It is what a
    /// `(defbancada)` resolves `(:context cluster)` against, and an empty
    /// value makes the planner REFUSE rather than emit `--context ""` — which
    /// would pre-warm a session on whatever cluster the operator's kubeconfig
    /// happens to point at. See [`banken_spec::bancada`].
    cluster: String,
    /// The pre-warmed session recipes reachable from this view, in the SAME
    /// order [`keymap_from_catalog`] bound them — which is what makes
    /// [`Action::OpenBancada`]'s index total.
    bancadas: Vec<BancadaSpec>,
    table: PodTable,
    panel: Panel,
    keys: awase::KeyMode<Action>,
    /// Quadro T8: awase's `KeyRepeatGate` debounces the held-key path on
    /// the three `postigo` action chords. See [`Action::is_repeat_gated`]
    /// for why navigation is exempt.
    repeat_gate: KeyRepeatGate<Action>,
    done: bool,
    /// A short label describing where the pod rows came from (fixture vs
    /// live), rendered in the status line — tier-honesty in the UI itself.
    source_label: String,
    /// The key legend entries drawn in the status line, DERIVED from the
    /// authored catalog by [`key_legend_parts`], **most important first**.
    ///
    /// It used to be the literal `"l:OBSERVE  s:DECLARE  S:BREAK-GLASS …"` —
    /// which already disagreed with reality, advertising `S` for a chord the
    /// runtime binds as `shift+s`. The legend is the one surface that *tells
    /// the operator which gate a keystroke crosses*, so it being a hand-written
    /// string was the highest-consequence hand-list in the app.
    ///
    /// Held as PARTS rather than one string because the whole legend does not
    /// always fit: the previous code rendered it only `if legend_w < width`,
    /// so adding the two bancada chords made the entire legend **silently
    /// vanish** at 80 columns. [`fit_legend`] now drops whole trailing entries
    /// and marks the elision, so a narrow terminal costs the *least* important
    /// hints rather than all of them.
    legend_parts: Vec<String>,
    /// The background absorb plane, when one is running.
    ///
    /// `None` is the default and means input-only — which is what every
    /// non-tokio test builds, and what `AsyncApp::wake` lowers to a
    /// `pending()` for. See [`crate::feed`].
    absorb: Option<Despensa>,
    /// Stops the poll absorber when the app is dropped.
    ///
    /// `PodFeed` flipped this in its own `Drop`; keeping that behaviour is not
    /// tidiness — without it an apiserver poll outlives the terminal nobody is
    /// watching. The watch absorber does not need one: its task ends when the
    /// stream does.
    absorb_stop: Option<crate::absorb::PollGuard>,
    /// The absorb generation already folded into the table.
    ///
    /// Starts at `0`, which is `Snapshot::empty()`'s generation, so a freshly
    /// built app whose absorber has not published yet does no work.
    applied_generation: u64,
}

impl<E: ClusterEnv, S: SessionEnv> BankenApp<E, S> {
    /// Build the app over a cluster env, loading the authored vocabulary and
    /// performing the initial OBSERVE read of the pod table.
    ///
    /// **Fallible on purpose.** The keymap and the table columns now come
    /// from `banken_spec::load_catalog()`, and a spec-load or cross-resolution
    /// failure must SURFACE — falling back to hardcoded chords would be the
    /// silent-wrong-behaviour class this repo refuses (and would put the
    /// operator on a keyboard the authored legend does not describe).
    ///
    /// # Errors
    ///
    /// - Any `SpecError` from `load_catalog()` (a spec file that fails to
    ///   compile, or a cross-reference that does not resolve).
    /// - [`SpecError::Binding`] when an authored chord has no egaku-term
    ///   projection.
    pub fn try_new(
        env: impl Into<Arc<E>>,
        session: S,
        operator: OperatorId,
        source_label: impl Into<String>,
    ) -> Result<Self, SpecError> {
        let catalog = banken_spec::load_catalog()?;
        Self::with_catalog(env, session, operator, source_label, &catalog)
    }

    /// [`Self::try_new`] over an explicit catalog — the seam a test or a
    /// consumer reading a custom `spec_dir` uses.
    ///
    /// # Errors
    ///
    /// [`SpecError::Binding`] when an authored chord has no egaku-term
    /// projection, or when the app's dispatch table names an action the
    /// catalog does not declare.
    pub fn with_catalog(
        env: impl Into<Arc<E>>,
        session: S,
        operator: OperatorId,
        source_label: impl Into<String>,
        catalog: &Catalog,
    ) -> Result<Self, SpecError> {
        let env = env.into();
        let keys = keymap_from_catalog(catalog)?;
        // Initial read. A read failure yields an empty table (the app still
        // runs and shows "0 pods"); the read is retried on the next
        // refresh. Never a panic on a read error. A *spec* failure is
        // different in kind and is propagated above, not defaulted away.
        let rows = env
            .list_resources(ResourceKind::Pod, None)
            .unwrap_or_default();
        Ok(Self {
            env,
            session,
            operator,
            cluster: String::new(),
            bancadas: catalog
                .bancadas_from(VIEW_NAME)
                .into_iter()
                .cloned()
                .collect(),
            table: PodTable::from_view(catalog, VIEW_NAME, rows)?,
            panel: Panel::Table,
            keys,
            repeat_gate: KeyRepeatGate::new(),
            done: false,
            source_label: source_label.into(),
            legend_parts: key_legend_parts(catalog),
            absorb: None,
            absorb_stop: None,
            applied_generation: 0,
        })
    }

    /// Name the cluster banken is reading (the kubeconfig context).
    ///
    /// Builder-style rather than a constructor argument because it is
    /// *optional information*, and its absence is meaningful: a banken that
    /// does not know its own cluster refuses to pre-warm a session rather
    /// than opening one somewhere else. See [`Self::cluster`].
    #[must_use]
    pub fn with_cluster(mut self, cluster: impl Into<String>) -> Self {
        self.cluster = cluster.into();
        self
    }

    /// The pre-warmed session recipes this view exposes.
    #[must_use]
    pub fn bancadas(&self) -> &[BancadaSpec] {
        &self.bancadas
    }

    /// The [`SessionEnv`] a confirmed bancada is opened through.
    ///
    /// Exposed so a test can assert against a recording mock — "the app
    /// reported a session" and "the seam was actually called" are different
    /// claims, and only the second one is evidence.
    #[must_use]
    pub fn session(&self) -> &S {
        &self.session
    }

    /// Re-read the pod table from the env, on the calling thread.
    ///
    /// A read failure leaves the current rows in place — a transient
    /// apiserver error must not blank a table an operator is reading.
    ///
    /// **This blocks.** Against `KubeClusterEnv` it is an HTTPS round trip,
    /// so calling it from the task that owns the terminal freezes input for
    /// the duration. That is why the running app does not call it: the
    /// steady-state path is [`Self::with_feed`], which does the same read on
    /// tokio's blocking pool. This stays for a synchronous caller (a test, a
    /// one-shot) that genuinely wants the read inline.
    pub fn refresh(&mut self) {
        if let Ok(rows) = self.env.list_resources(ResourceKind::Pod, None) {
            self.set_rows(rows);
        }
    }

    /// Install `rows` as the table contents. The single write path, so the
    /// inline refresh and the feed's apply cannot drift apart.
    fn set_rows(&mut self, rows: Vec<Row>) {
        self.table.view_mut().set_rows(rows);
    }

    /// Attach a background absorb plane reading the same env every
    /// `interval` (see [`crate::feed`]).
    ///
    /// Builder-style and opt-in: an app without a feed is input-only, which
    /// is what every synchronous test wants and what `AsyncApp::wake` lowers
    /// to `pending()` for. Requires a tokio runtime, which is why it is not
    /// simply done in `try_new`.
    ///
    /// # Panics
    ///
    /// Never directly; `tokio::spawn` panics if called outside a runtime.
    #[must_use]
    pub fn with_feed(mut self, interval: std::time::Duration) -> Self
    where
        E: Send + Sync + 'static,
    {
        let (despensa, publisher) = crate::absorb::channel();
        self.absorb_stop = Some(crate::absorb::spawn_poll_absorber(
            Arc::clone(&self.env),
            ResourceKind::Pod,
            interval,
            publisher,
        ));
        self.absorb = Some(despensa);
        self
    }

    /// Attach an already-running absorber — the path the live build takes, where
    /// the producer is a `kube` watch stream rather than a poll.
    ///
    /// The app takes a [`Despensa`] and never learns which producer filled it.
    /// That is the point: a consumer adapts by reading a declared capability,
    /// never by branching on which backend it happens to have.
    #[must_use]
    pub fn with_absorber(mut self, despensa: Despensa) -> Self {
        self.absorb = Some(despensa);
        self
    }

    /// The attached absorber, if any — for a test asserting the plane is live,
    /// and for the renderer to report its phase.
    #[must_use]
    pub fn absorber(&self) -> Option<&Despensa> {
        self.absorb.as_ref()
    }

    /// How the absorbed reading currently stands, when there is one.
    ///
    /// This is the axis banken has never had: a caller can no longer look at
    /// rows without being able to ask what claim is being made about them.
    #[must_use]
    pub fn sync_phase(&self) -> Option<SyncPhase> {
        self.absorb.as_ref().map(|d| d.snapshot().phase().clone())
    }

    /// Pull whatever has been absorbed into the table.
    ///
    /// The `&mut self` apply half of the wakeup. Separate from
    /// [`Self::refresh`] because it performs **no read** — the read already
    /// happened, off this task. A no-op when there is no absorber.
    /// # Cost, measured — and why the generation gate is here
    ///
    /// `set_rows` takes a `Vec<Row>`, so reaching it from the published
    /// `Arc<[Row]>` costs a **deep clone of every `Row` and every `String` in
    /// every cell**. Measured at N=10 000: `Arc::to_vec()` alone is **10.16 ms
    /// of this function's 14.9 ms, and 4.0 MB of allocation per call**.
    ///
    /// The gate below removes every *redundant* one of those. It does not
    /// remove the copy on a real change — that needs `egaku::TableView` to
    /// accept an `Arc<[Row]>` it can adopt by refcount, which is an **egaku**
    /// change and must not be made from this repo (QUADRO T1: widgets live in
    /// egaku, never in an app). `pending-banken: adopt-arc-rows`.
    ///
    /// The gate is not redundant with the absorb plane's content hash: the hash
    /// decides whether to **wake**, this decides whether to **rebuild the
    /// table**. They are different questions the moment anything other than a
    /// wake calls this — and `on_wake` is not the only caller a future arm can
    /// add.
    ///
    /// # The symptom IS the cost, which is why the test asserts on bytes
    ///
    /// A redundant apply has **no user-visible effect** — `set_rows` preserves
    /// the selection, so an earlier cursor-survival test for this gate **passed
    /// with the gate deleted** and was vacuous. `a_redundant_apply_allocates_nothing`
    /// asserts on a counting allocator instead; deleting the arm below turns it
    /// red with a non-zero byte count (measured: 784 B on the fixture).
    pub fn apply_feed(&mut self) {
        let Some(despensa) = self.absorb.as_ref() else {
            return;
        };
        let snapshot = despensa.snapshot();
        let generation = snapshot.generation();
        // `0` is the pre-absorption generation, and `applied_generation` starts
        // there too — so an absorber that has published nothing yet correctly
        // does no work, rather than rebuilding the table from an empty set.
        if generation == self.applied_generation {
            return;
        }
        self.applied_generation = generation;
        self.set_rows(snapshot.rows().to_vec());
    }

    /// The stop flag for the poll absorber, if one is running.
    #[must_use]
    pub fn absorb_stop(&self) -> Option<&crate::absorb::PollGuard> {
        self.absorb_stop.as_ref()
    }

    /// The current pod table (for tests + the renderer).
    #[must_use]
    pub fn table(&self) -> &PodTable {
        &self.table
    }

    /// The keymap derived from the authored catalog.
    ///
    /// Inherent rather than only on [`AsyncApp`], because that trait needs
    /// both seams `Send + Sync` and a recording mock is neither (it is built
    /// on `RefCell`, exactly like `MockClusterEnv`). Asking "which action does
    /// this chord bind" has nothing to do with running a terminal, and tying
    /// it to the terminal trait would have meant no mock-driven test could
    /// ever ask it.
    #[must_use]
    pub fn keymap(&self) -> &awase::KeyMode<Action> {
        &self.keys
    }

    /// Whether the app has been asked to quit. Inherent for the same reason
    /// as [`Self::keymap`].
    #[must_use]
    pub fn should_quit(&self) -> bool {
        self.done
    }

    /// The dispatcher entry point: consult the repeat gate, then apply.
    ///
    /// Returns `true` when the action was applied and `false` when it was
    /// dropped as a key-repeat storm tick. The gate lives **here** rather
    /// than inside [`Self::apply_action`] so `apply_action` stays a pure
    /// state transition (awase's own guidance: drop "at the dispatcher").
    ///
    /// `now` is explicit so tests are wall-clock-free.
    pub fn dispatch_action_at(&mut self, action: Action, now: Instant) -> bool {
        if action.is_repeat_gated() && !self.repeat_gate.try_pass_at(action, now) {
            return false;
        }
        self.apply_action(action);
        true
    }

    /// [`Self::dispatch_action_at`] at the current instant.
    pub fn dispatch_action(&mut self, action: Action) -> bool {
        self.dispatch_action_at(action, Instant::now())
    }

    /// Apply an action to the app state. Public so tests can drive the app
    /// without a terminal (the pure state transition, ungated).
    pub fn apply_action(&mut self, action: Action) {
        // Any navigation/action dismisses a stale overlay first, except the
        // explicit Dismiss/Quit which are handled below.
        match action {
            Action::SelectNext => {
                self.panel = Panel::Table;
                self.table.view_mut().select_next();
            }
            Action::SelectPrev => {
                self.panel = Panel::Table;
                self.table.view_mut().select_prev();
            }
            Action::ToggleSort => {
                self.panel = Panel::Table;
                self.table.view_mut().toggle_sort_direction();
            }
            Action::ObserveLogs => {
                let r = dispatch(&self.table, RowAction::ViewLogs, &self.operator, &self.env);
                self.panel = Panel::ActionOverlay(r);
            }
            Action::DeclareScale => {
                let r = dispatch(
                    &self.table,
                    RowAction::DeclareScale,
                    &self.operator,
                    &self.env,
                );
                self.panel = Panel::ActionOverlay(r);
            }
            Action::BreakGlass => {
                let r = dispatch(
                    &self.table,
                    RowAction::BreakGlassShell,
                    &self.operator,
                    &self.env,
                );
                self.panel = Panel::ActionOverlay(r);
            }
            Action::OpenBancada(i) => {
                // A bancada index out of range is not possible through the
                // keymap (both come from the same filtered iteration), but
                // reporting it beats an index panic in the operator's TUI.
                self.panel = match self.bancadas.get(i) {
                    Some(spec) => match resolve_bancada(&self.table, spec, &self.cluster) {
                        // Resolving touches nothing. The operator sees the
                        // fully-resolved argv, the cluster it names and the
                        // DERIVED class, and `enter` is what opens it.
                        Ok(pending) => Panel::BancadaPreview(pending),
                        Err(refusal) => Panel::ActionOverlay(refusal),
                    },
                    None => Panel::ActionOverlay(ActionResult::Error(
                        "no bancada is bound at that index — the keymap and the \
                         recipe list disagree"
                            .into(),
                    )),
                };
            }
            Action::Confirm => {
                // The ONLY confirmable screen. Anywhere else `enter` is a
                // deliberate no-op — pressing it on the table is not a
                // mistake worth an error panel.
                if let Panel::BancadaPreview(pending) = &self.panel {
                    // *** RE-GRIP AT THE ACT. ***
                    //
                    // The plan is exactly what was previewed — that is what
                    // makes the preview honest. What is re-established here is
                    // that the SUBJECT is still the object the operator looked
                    // at. Between `g` and `enter` sits the operator's think
                    // time, and the pod may have been deleted, replaced, or had
                    // its name recycled onto a different uid.
                    //
                    // The grip is minted HERE and consumed HERE. It cannot be
                    // taken at preview time and carried: `Grip` is `!Send`, and
                    // `AsyncApp` requires the app's state to be `Send`, so
                    // storing one in `self.panel` does not compile. The
                    // re-check is not a discipline anyone can forget.
                    let r = match self.env.grip(&pending.subject) {
                        Ok(grip) => open_bancada(pending, &grip, &self.session),
                        // A refusal is the RESULT, rendered like any other —
                        // the operator learns the object moved instead of
                        // opening a session onto whatever now answers to that
                        // name.
                        Err(e) => ActionResult::Error(e.to_string()),
                    };
                    self.panel = Panel::ActionOverlay(r);
                }
            }
            Action::Dismiss => self.panel = Panel::Table,
            Action::Quit => self.done = true,
        }
    }

    /// The `(defbancada)` plan currently awaiting confirmation, if any
    /// (tests, and the renderer).
    #[must_use]
    pub fn pending_bancada(&self) -> Option<&PendingBancada> {
        match &self.panel {
            Panel::BancadaPreview(p) => Some(p),
            Panel::Table | Panel::ActionOverlay(_) => None,
        }
    }

    /// What the overlay currently shows, if one is showing.
    ///
    /// Returns an owned value rather than a borrow because a
    /// [`Panel::BancadaPreview`] stores the typed *plan* and renders it on
    /// demand — there is no stored [`ActionResult`] to lend. Keeping the two
    /// screens behind one accessor is deliberate: a caller asking "what is
    /// the operator looking at" should not have to know which of them it is.
    #[must_use]
    pub fn overlay(&self) -> Option<ActionResult> {
        match &self.panel {
            Panel::ActionOverlay(r) => Some(r.clone()),
            Panel::BancadaPreview(p) => Some(preview_bancada(p)),
            Panel::Table => None,
        }
    }

    /// Draw the whole frame into `buf` (public so golden-frame tests can
    /// render without a terminal via `TestBackend`).
    pub fn render(&self, buf: &mut Buffer) {
        let width = buf.width();
        let height = buf.height();
        if width == 0 || height == 0 {
            return;
        }

        // ── Title bar ──
        let title_style = Style::default().fg(Color::Cyan).bold();
        buf.set_stringn(0, 0, "banken 番犬  :pods", width, title_style);

        // ── The table (rows 2..height-1) ──
        let table_top = 2;
        let table_height = height.saturating_sub(table_top + 1);
        draw_pod_table(buf, 0, table_top, width, table_height, &self.table);

        // ── Status line (last row) ──
        self.draw_status_line(buf, width, height);

        // ── Action overlay (drawn last, on top) ──
        match &self.panel {
            Panel::ActionOverlay(result) => draw_overlay(buf, width, height, result, false),
            // The one screen with a confirm affordance — the footer says so.
            Panel::BancadaPreview(pending) => {
                draw_overlay(buf, width, height, &preview_bancada(pending), true);
            }
            Panel::Table => {}
        }
    }

    fn draw_status_line(&self, buf: &mut Buffer, width: u16, height: u16) {
        let y = height - 1;
        let bar = Style::default().fg(Color::Black).bg(Color::DarkGrey);
        buf.blank(0, y, width, bar);

        let mut left = String::new();
        left.push_str(&self.source_label);
        left.push_str("  ");
        left.push_str(&pod_count_label(self.table.view().len()));
        left.push_str("  ");
        left.push_str(&sort_label(&self.table));
        buf.set_stringn(0, y, &left, width, bar);

        // Right: the key legend, DERIVED from the authored catalog at
        // construction (see `key_legend_parts`) — never a hand-written string
        // that can advertise a chord the runtime does not bind. Fitted to
        // whatever the left half leaves, dropping the least important entries
        // rather than the whole legend.
        let used = u16::try_from(left.chars().count()).unwrap_or(width);
        let available = width.saturating_sub(used).saturating_sub(2);
        let legend = fit_legend(&self.legend_parts, available as usize);
        let legend_w = u16::try_from(legend.chars().count()).unwrap_or(0);
        if legend_w > 0 && legend_w <= width {
            buf.set_stringn(width - legend_w, y, &legend, legend_w, bar);
        }
    }
}

/// A `"N pods"` label without `format!()` of a full styled string (the
/// digits go through a typed integer render, then a plain concat — no VT).
fn pod_count_label(n: usize) -> String {
    let mut s = String::new();
    s.push_str(&n.to_string());
    s.push_str(" pods");
    s
}

/// Draw the action-result overlay as a bordered panel across the lower
/// half of the screen. All writes are typed `Buffer` ops.
fn draw_overlay(
    buf: &mut Buffer,
    width: u16,
    height: u16,
    result: &ActionResult,
    confirmable: bool,
) {
    if width < 10 || height < 8 {
        return;
    }
    let top = height / 2;
    let panel_h = height - top - 1;
    let border = Style::default().fg(Color::Cyan);
    let body = Style::default();

    // Border box.
    let right = width - 1;
    buf.hline(1, top, width - 2, '─', border);
    buf.set_char(0, top, '┌', border);
    buf.set_char(right, top, '┐', border);
    for r in 1..panel_h {
        buf.set_char(0, top + r, '│', border);
        buf.set_char(right, top + r, '│', border);
    }
    let bottom = top + panel_h;
    if bottom < height {
        buf.hline(1, bottom, width - 2, '─', border);
        buf.set_char(0, bottom, '└', border);
        buf.set_char(right, bottom, '┘', border);
    }

    // Title (the postigo class + a summary).
    let (title, title_style, lines) = overlay_content(result);
    buf.set_stringn(2, top, &title, width.saturating_sub(4), title_style);

    // Body lines.
    let inner_w = width.saturating_sub(4);
    for (i, line) in lines.iter().enumerate() {
        let ry = top + 1 + u16::try_from(i).unwrap_or(u16::MAX);
        if ry >= bottom {
            break;
        }
        buf.set_stringn(2, ry, line, inner_w, body);
    }

    // Footer hint. A confirmable screen ADVERTISES the confirm key — an
    // affordance nothing tells the operator about is one nobody uses.
    if bottom > top + 1 {
        let hint = if confirmable {
            " enter: open  ·  esc: dismiss "
        } else {
            " esc: dismiss "
        };
        let hint_w = u16::try_from(hint.chars().count()).unwrap_or(0);
        if hint_w < width {
            buf.set_stringn(2, bottom, hint, hint_w, border);
        }
    }
}

/// The title + colored style + body lines for an action-result overlay.
fn overlay_content(result: &ActionResult) -> (String, Style, Vec<String>) {
    match result {
        ActionResult::Observed { title, lines } => {
            let mut t = String::from(" OBSERVE — ");
            t.push_str(title);
            t.push(' ');
            (t, Style::default().fg(Color::Green).bold(), lines.clone())
        }
        ActionResult::DeclarePreview {
            change_ref,
            full_manifest,
        } => {
            let mut t = String::from(" DECLARE — full-manifest preview (");
            t.push_str(change_ref);
            t.push_str(") ");
            let lines: Vec<String> = full_manifest.lines().map(str::to_string).collect();
            (t, Style::default().fg(Color::Yellow).bold(), lines)
        }
        ActionResult::BreakGlassRecord {
            witness,
            runbook,
            selector,
            record_id,
        } => {
            let t = String::from(" BREAK-GLASS — witnessed record (not executed) ");
            let mut lines = Vec::new();
            let mut w = String::from("witness:  ");
            w.push_str(witness);
            lines.push(w);
            let mut r = String::from("runbook:  ");
            r.push_str(runbook);
            lines.push(r);
            let mut s = String::from("target:   ");
            s.push_str(selector);
            lines.push(s);
            let mut rec = String::from("record:   ");
            rec.push_str(record_id);
            lines.push(rec);
            (t, Style::default().fg(Color::Red).bold(), lines)
        }
        ActionResult::BancadaPlan {
            recipe,
            legality,
            session_name,
            lines,
        } => {
            // The title states the DERIVED gate first — a BREAK-GLASS recipe
            // must not read like a convenience.
            let mut t = String::from(" BANCADA — ");
            t.push_str(legality);
            t.push_str(" — ");
            t.push_str(recipe);
            t.push_str(" (not opened yet) ");
            let mut body = Vec::with_capacity(lines.len() + 2);
            let mut s = String::from("session:  ");
            s.push_str(session_name);
            body.push(s);
            body.push(String::new());
            body.extend(lines.iter().cloned());
            // BREAK-GLASS is red, an observe plan is green — the same colour
            // vocabulary the postigo overlays already use, so the operator
            // reads the gate before the text.
            let style = if legality == "OBSERVE" {
                Style::default().fg(Color::Green).bold()
            } else {
                Style::default().fg(Color::Red).bold()
            };
            (t, style, body)
        }
        ActionResult::BancadaOpened {
            recipe,
            legality,
            session_name,
            pane_count,
            witnessed,
        } => {
            let mut t = String::from(" BANCADA — ");
            t.push_str(legality);
            t.push_str(" — ");
            t.push_str(recipe);
            t.push_str(" (OPENED) ");
            let mut body = Vec::new();
            let mut s = String::from("session:  ");
            s.push_str(session_name);
            body.push(s);
            let mut p = String::from("panes:    ");
            p.push_str(&pane_count.to_string());
            body.push(p);
            if *witnessed {
                body.push(String::new());
                // The asymmetry `TearSessionEnv` builds in, restated where the
                // operator can act on it: the live-effect command is typed and
                // waiting, and their own Enter IN THE PANE is the final act.
                body.push(String::from(
                    "the live-effect command is TYPED but NOT submitted — your own",
                ));
                body.push(String::from(
                    "Enter in that pane is the final act. banken recorded the witness.",
                ));
            }
            let style = if legality == "OBSERVE" {
                Style::default().fg(Color::Green).bold()
            } else {
                Style::default().fg(Color::Red).bold()
            };
            (t, style, body)
        }
        ActionResult::Error(msg) => (
            String::from(" ERROR "),
            Style::default().fg(Color::Red).bold(),
            vec![msg.clone()],
        ),
    }
}

/// The local-UI intent a `(defnavkey)` carries, projected onto the app's
/// [`Action`] enum.
///
/// Exhaustive on purpose: adding a [`NavIntent`] variant is a compile error
/// here until it is handled, so an authored intent the app silently ignores —
/// a dead key — is unrepresentable.
impl From<NavIntent> for Action {
    fn from(intent: NavIntent) -> Self {
        match intent {
            NavIntent::SelectNext => Action::SelectNext,
            NavIntent::SelectPrev => Action::SelectPrev,
            NavIntent::ToggleSort => Action::ToggleSort,
            NavIntent::Confirm => Action::Confirm,
            NavIntent::Dismiss => Action::Dismiss,
            NavIntent::Quit => Action::Quit,
        }
    }
}

/// The postigo action names the app can dispatch, and the [`Action`] each
/// maps to.
///
/// This is the ONE remaining hand-written join between the authored catalog
/// and the app, and it is irreducible: a `(defk8saction)` name is a string in
/// a spec file and an `Action` is a Rust variant, so something must relate
/// them. What [`keymap_from_catalog`] guarantees is that the relation is
/// *total in both directions* — an authored action with no row here, or a row
/// here naming an action the catalog does not declare, is an error rather
/// than a chord that silently does nothing.
const DISPATCHABLE_ACTIONS: &[(&str, Action)] = &[
    ("view-logs", Action::ObserveLogs),
    ("scale", Action::DeclareScale),
    ("shell", Action::BreakGlass),
    // `describe` is authored OBSERVE but the app has no describe panel yet —
    // it is deliberately absent, and `keymap_from_catalog` reports it as
    // UNBOUND rather than pretending the chord works.
];

/// Build the app keymap from the authored vocabulary.
///
/// Closes `pending-banken: keymap-derived-from-catalog`. Both keyed domains
/// come from the [`Catalog`], which means:
///
/// - the nav chords and the postigo chords have already been conflict-checked
///   against ONE namespace by `Catalog::resolve` (they used to be two
///   unchecked `km.bind` sequences whose collision resolved by bind order),
///   and
/// - re-spelling a chord in `specs/actions.lisp` or `specs/navkeys.lisp`
///   moves the runtime binding, with no Rust edit.
///
/// # Errors
///
/// - [`SpecError::Binding`] when an authored chord has no egaku-term
///   duplicate chord (see [`bind`]) — a refusal, never a
///   guessed mapping.
/// - [`SpecError::Binding`] when a [`DISPATCHABLE_ACTIONS`] row names an
///   action the catalog does not declare (the hand-list drifted ahead of the
///   spec).
pub fn keymap_from_catalog(catalog: &Catalog) -> Result<awase::KeyMode<Action>, SpecError> {
    let mut km: awase::KeyMode<Action> = awase::KeyMode::typed("default", false);

    // Navigation — the intent projection is exhaustive, so every authored
    // nav key binds.
    for nav in catalog.nav_keys() {
        bind(&mut km, nav.keys, Action::from(nav.intent), &nav.name)?;
    }

    // postigo — only the actions the app can actually dispatch.
    for (name, action) in DISPATCHABLE_ACTIONS {
        let spec = catalog
            .actions()
            .iter()
            .find(|a| a.name == *name)
            .ok_or_else(|| {
                let mut m = String::from("the app binds action `");
                m.push_str(name);
                m.push_str(
                    "` but no (defk8saction) declares it — DISPATCHABLE_ACTIONS \
                     drifted ahead of specs/actions.lisp",
                );
                SpecError::Binding(m)
            })?;
        bind(&mut km, spec.keys, *action, &spec.name)?;
    }

    // bancadas — the pre-warmed tear sessions reachable from THIS view.
    // Bound by position in the same filtered iteration `BankenApp` stores,
    // so `Action::OpenBancada(i)` cannot index a different recipe than the
    // chord names. A recipe launched from another surface is deliberately
    // NOT bound here — `unbound_bancada_names` reports it rather than
    // giving the operator a chord that opens something from a screen they
    // are not on.
    for (i, g) in catalog.bancadas_from(VIEW_NAME).into_iter().enumerate() {
        bind(&mut km, g.keys, Action::OpenBancada(i), &g.name)?;
    }

    Ok(km)
}

/// Bind an authored chord, refusing a duplicate rather than displacing it.
///
/// **This replaced a 199-line projection.** The authored chord used to be an
/// `awase::Hotkey` and the delivered chord an `egaku_term::KeyCombo` — two
/// types, so every binding had to cross a hand-maintained translation with a
/// measured two-row table, an explicit agreeing-variant list, and an outright
/// refusal of `space`. `egaku_term` now delivers `awase::Hotkey`, so the
/// authored value and the delivered value are the same type and there is
/// nothing left to project.
///
/// What is gained beyond the deletion: `try_bind` makes a duplicate chord an
/// error here. The old `KeyMap::bind` was a `HashMap` insert — the second
/// binding silently displaced the first, and only the catalog's separate
/// conflict pass stood between that and a lost chord.
fn bind(
    km: &mut awase::KeyMode<Action>,
    chord: ActionChord,
    action: Action,
    owner: &str,
) -> Result<(), SpecError> {
    use std::fmt::Write as _;
    km.try_bind(awase::Binding::new(chord.hotkey(), action))
        .map_err(|displaced| {
            let mut m = String::from("authored chord `");
            m.push_str(&chord.canonical());
            m.push_str("` on `");
            m.push_str(owner);
            m.push_str("` is already bound to `");
            let _ = write!(m, "{:?}", displaced.action);
            m.push_str("` — one of the two could never fire");
            SpecError::Binding(m)
        })
}

/// Fit as many whole legend entries as `available` columns allow, appending
/// `…` when any were dropped.
///
/// Whole entries only: half of `shift+s:BREAK-GLASS` is worse than none of
/// it. The elision marker is what keeps a narrow terminal honest — the
/// operator can see there are more chords than shown, instead of the legend
/// quietly disappearing (which is what the previous `if legend_w < width`
/// did the moment the bancada chords landed).
#[must_use]
pub fn fit_legend(parts: &[String], available: usize) -> String {
    const SEP: &str = "  ";
    const ELLIPSIS: &str = " …";
    let mut out = String::new();
    let mut dropped = false;
    for part in parts {
        let extra = if out.is_empty() {
            part.chars().count()
        } else {
            SEP.chars().count() + part.chars().count()
        };
        // Reserve room for the marker whenever something might still be cut.
        let budget = available.saturating_sub(ELLIPSIS.chars().count());
        if out.chars().count() + extra <= budget {
            if !out.is_empty() {
                out.push_str(SEP);
            }
            out.push_str(part);
        } else {
            dropped = true;
        }
    }
    // Nothing fit at all: better an honest marker than a blank status line.
    if out.is_empty() {
        return if parts.is_empty() || available == 0 {
            String::new()
        } else {
            String::from("…")
        };
    }
    if dropped {
        out.push_str(ELLIPSIS);
    }
    out
}

/// Build the status-line key legend from the authored catalog.
///
/// Each postigo entry is `<authored chord>:<LEGALITY CLASS>`, so the legend
/// states which gate the keystroke crosses using the *authored* chord and the
/// *typed* class — the two things a hand-written legend can get wrong. The two
/// navigation hints are looked up by [`NavIntent`], not by chord, so re-binding
/// `o` or `q` in `specs/navkeys.lisp` moves the legend with them.
///
/// Typed emission: assembled by concatenation from typed pieces
/// ([`ActionChord::canonical`], [`banken_spec::types::LegalityClass::label`]),
/// never a `format!()` of a layout template.
#[must_use]
pub fn key_legend(catalog: &Catalog) -> String {
    key_legend_parts(catalog).join("  ")
}

/// The legend's entries, **most important first** — the postigo chords, then
/// the bancada chords (both of which state a *gate*), then the two navigation
/// hints. The order is the drop order when the status line is too narrow, so
/// it is deliberately "what an operator must not misread" first.
#[must_use]
pub fn key_legend_parts(catalog: &Catalog) -> Vec<String> {
    let mut parts: Vec<String> = Vec::new();

    for (name, _) in DISPATCHABLE_ACTIONS {
        if let Some(spec) = catalog.actions().iter().find(|a| a.name == *name) {
            let mut p = spec.keys.canonical();
            p.push(':');
            p.push_str(&spec.legality.class().label().to_uppercase());
            parts.push(p);
        }
    }

    // A bancada's class is its DERIVED one, so the legend states the gate the
    // pre-warmed session would actually cross. A malformed recipe never
    // reaches here (`Catalog::resolve` validates every one), so the fallback
    // label is unreachable in a resolved catalog rather than a quiet default.
    for g in catalog.bancadas_from(VIEW_NAME) {
        let mut p = g.keys.canonical();
        p.push(':');
        p.push_str(
            &g.legality()
                .map(|l| l.class().label().to_uppercase())
                .unwrap_or_else(|_| "INVALID".to_owned()),
        );
        parts.push(p);
    }

    for (intent, label) in [(NavIntent::ToggleSort, "sort"), (NavIntent::Quit, "quit")] {
        if let Some(nav) = catalog.nav_keys().iter().find(|n| n.intent == intent) {
            let mut p = nav.keys.canonical();
            p.push(':');
            p.push_str(label);
            parts.push(p);
        }
    }

    parts
}

/// The bancada recipes the catalog declares but this view cannot launch —
/// the bancadas analogue of [`unbound_action_names`].
///
/// Surfaced as data rather than silence: a recipe whose `:from` names another
/// surface is authored, valid and unreachable from here, and the operator
/// should learn that from `--help` rather than from a chord doing nothing.
#[must_use]
pub fn unbound_bancada_names(catalog: &Catalog) -> Vec<String> {
    catalog
        .bancadas()
        .iter()
        .filter(|g| g.from != VIEW_NAME)
        .map(|g| g.name.clone())
        .collect()
}

/// The postigo action names the authored catalog declares but the app cannot
/// dispatch yet.
///
/// Surfaced as data rather than silence: an authored chord with no app
/// handler is a key the legend implies and nothing performs, and a test pins
/// the current set so growing it is deliberate.
#[must_use]
pub fn unbound_action_names(catalog: &Catalog) -> Vec<String> {
    catalog
        .actions()
        .iter()
        .filter(|a| !DISPATCHABLE_ACTIONS.iter().any(|(n, _)| *n == a.name))
        .map(|a| a.name.clone())
        .collect()
}

// `AsyncApp::draw` takes `&self` and returns `impl Future + Send`, so the
// `&BankenApp<E, S>` borrow must be `Send` — which requires both seams `Sync`.
impl<E: ClusterEnv + Send + Sync, S: SessionEnv + Send + Sync> AsyncApp for BankenApp<E, S> {
    type Action = Action;

    /// Vestigial: banken dispatches through [`Self::hotkey_map`].
    ///
    /// `AsyncApp::keymap` is still a required method, so this returns a
    /// permanently-empty map. It is never consulted — the runtime prefers the
    /// typed path whenever `hotkey_map` is `Some`. A static rather than a
    /// field so the empty map costs the struct nothing.
    fn keymap(&self) -> &KeyMap<Action> {
        static EMPTY: std::sync::OnceLock<KeyMap<Action>> = std::sync::OnceLock::new();
        EMPTY.get_or_init(KeyMap::new)
    }

    /// The typed keymap — banken's real dispatch source.
    ///
    /// The authored chord (`awase::Hotkey`, conflict-checked by the catalog)
    /// and the delivered chord (now also `awase::Hotkey`) are one type, so
    /// this is a direct borrow with no projection in between.
    fn hotkey_map(&self) -> Option<&awase::KeyMode<Action>> {
        Some(BankenApp::keymap(self))
    }

    async fn handle(&mut self, action: &Action) -> TermResult<()> {
        // A dropped repeat tick is not an error — the frame simply does not
        // change, which is exactly the desired no-flicker behaviour.
        let _applied = self.dispatch_action(*action);
        Ok(())
    }

    async fn draw(&self, frame: &mut Buffer) -> TermResult<()> {
        self.render(frame);
        Ok(())
    }

    /// The signal: resolve when the feed has new rows, or never when there is
    /// no feed. `pending()` for the no-feed case is not a placeholder — it is
    /// exactly the pre-feed behaviour, so an app built without one runs
    /// byte-identically to how it ran before this existed.
    async fn wake(&self) {
        match &self.absorb {
            Some(d) => d.changed().await,
            None => std::future::pending().await,
        }
    }

    /// The apply. Runs after the select resolved, so unlike `wake` it is
    /// never dropped part-way and cannot leave the table half-written.
    async fn on_wake(&mut self) -> TermResult<()> {
        self.apply_feed();
        Ok(())
    }

    fn should_quit(&self) -> bool {
        BankenApp::should_quit(self)
    }
}

#[cfg(test)]
mod tests {
    /// The action an authored chord resolves to.
    ///
    /// One helper because the authored chord and the delivered chord are the
    /// same type now — there is no projection to assert around.
    fn act(km: &awase::KeyMode<super::Action>, hk: awase::Hotkey) -> Option<&super::Action> {
        km.find_binding(&hk, &awase::MatchContext::default())
            .map(|b| &b.action)
    }

    use std::time::Duration;

    use awase::repeat_gate::DEFAULT_MIN_INTERVAL;

    use super::*;
    use crate::fixture::FixtureClusterEnv;
    use banken_spec::testing::MockSessionEnv;

    /// The app over the fixture cluster and a **recording** session env, so
    /// every test can assert what a confirm did or did not open. No tear
    /// daemon, no subprocess, no PTY.
    fn app() -> BankenApp<FixtureClusterEnv, MockSessionEnv> {
        BankenApp::try_new(
            FixtureClusterEnv::new(),
            MockSessionEnv::new(),
            OperatorId::new("drzzln").expect("a literal witness is non-blank"),
            "source: fixture",
        )
        .expect("the shipped vocabulary must build an app")
    }

    /// The same app, told which cluster it is reading — what a bancada needs
    /// to pre-warm a session on the RIGHT one.
    fn app_on(cluster: &str) -> BankenApp<FixtureClusterEnv, MockSessionEnv> {
        app().with_cluster(cluster)
    }

    #[test]
    fn app_reads_the_pod_table_on_construction() {
        let a = app();
        assert_eq!(a.table().view().rows().len(), 5);
        assert!(a.overlay().is_none(), "no overlay at rest");
    }

    #[test]
    fn navigation_moves_the_selection() {
        let mut a = app();
        // Start at row 0, move down twice → row 2, up once → row 1.
        assert_eq!(a.table().view().selected_index(), 0);
        a.apply_action(Action::SelectNext);
        a.apply_action(Action::SelectNext);
        assert_eq!(a.table().view().selected_index(), 2);
        a.apply_action(Action::SelectPrev);
        assert_eq!(a.table().view().selected_index(), 1);
    }

    #[test]
    fn declare_action_shows_a_full_manifest_overlay() {
        let mut a = app();
        a.apply_action(Action::DeclareScale);
        match a.overlay() {
            Some(ActionResult::DeclarePreview { full_manifest, .. }) => {
                assert!(full_manifest.contains("flux-helm-values"));
            }
            other => panic!("expected a DeclarePreview overlay, got {other:?}"),
        }
    }

    #[test]
    fn dismiss_clears_the_overlay() {
        let mut a = app();
        a.apply_action(Action::ObserveLogs);
        assert!(a.overlay().is_some());
        a.apply_action(Action::Dismiss);
        assert!(a.overlay().is_none());
    }

    #[test]
    fn quit_sets_done() {
        let mut a = app();
        assert!(!a.should_quit());
        a.apply_action(Action::Quit);
        assert!(a.should_quit());
    }

    #[test]
    fn keymap_binds_the_postigo_chords() {
        let a = app();
        assert_eq!(
            act(
                a.keymap(),
                awase::Hotkey::new(awase::Modifiers::NONE, awase::Key::L)
            ),
            Some(&Action::ObserveLogs)
        );
        assert_eq!(
            act(
                a.keymap(),
                awase::Hotkey::new(awase::Modifiers::NONE, awase::Key::S)
            ),
            Some(&Action::DeclareScale)
        );
        assert_eq!(
            act(
                a.keymap(),
                awase::Hotkey::new(awase::Modifiers::SHIFT, awase::Key::S)
            ),
            Some(&Action::BreakGlass)
        );
    }

    /// The app keymap and the authored `(defk8saction)` catalog must agree
    /// on every chord — otherwise the legend advertises one gate and the
    /// keypress crosses another. Both sides now go through awase's typed
    /// `Hotkey`, so the comparison is on typed values, not strings.
    #[test]
    fn app_keymap_agrees_with_the_authored_chords() {
        let a = app();
        let authored = banken_spec::load_actions().expect("authored actions load");
        for (name, expected_action) in [
            ("view-logs", Action::ObserveLogs),
            ("scale", Action::DeclareScale),
            ("shell", Action::BreakGlass),
        ] {
            let spec = authored
                .iter()
                .find(|x| x.name == name)
                .unwrap_or_else(|| panic!("authored action `{name}`"));
            // Look the AUTHORED chord up in the app keymap directly — the
            // assertion is on the authored value, not a hand-repeated
            // literal. There is no projection step any more: the authored
            // chord and the delivered chord are the same type.
            assert_eq!(
                a.keymap()
                    .find_binding(&spec.keys.hotkey(), &awase::MatchContext::default())
                    .map(|b| &b.action),
                Some(&expected_action),
                "the app keymap must bind `{name}`'s AUTHORED chord `{}` to {expected_action:?}",
                spec.keys,
            );
        }
        // And `shell` is bound to the SHIFTED chord specifically — the whole
        // point of authoring `shift+s` instead of `S`.
        assert_ne!(
            act(
                a.keymap(),
                awase::Hotkey::new(awase::Modifiers::NONE, awase::Key::S)
            ),
            Some(&Action::BreakGlass),
            "bare `s` must be DECLARE, never BREAK-GLASS",
        );
    }

    /// THE GATE. A held postigo action key is one event per OS repeat tick;
    /// without the gate a held `s` fires N full-manifest lowerings.
    #[test]
    fn a_held_postigo_action_key_is_debounced() {
        let mut a = app();
        let t0 = Instant::now();
        assert!(
            a.dispatch_action_at(Action::DeclareScale, t0),
            "the first press passes"
        );
        // Two OS key-repeat ticks inside the 80ms window: both dropped.
        assert!(
            !a.dispatch_action_at(Action::DeclareScale, t0 + Duration::from_millis(35)),
            "a repeat tick at +35ms is dropped"
        );
        assert!(
            !a.dispatch_action_at(Action::DeclareScale, t0 + Duration::from_millis(70)),
            "a repeat tick at +70ms is dropped"
        );
        // Past the window, an intentional second press passes.
        assert!(
            a.dispatch_action_at(Action::DeclareScale, t0 + DEFAULT_MIN_INTERVAL),
            "a deliberate press after the window passes"
        );
    }

    /// Each gated action has an INDEPENDENT clock — holding `s` must not
    /// swallow a deliberate `shift+s`.
    #[test]
    fn gated_actions_have_independent_clocks() {
        let mut a = app();
        let t0 = Instant::now();
        assert!(a.dispatch_action_at(Action::DeclareScale, t0));
        assert!(
            a.dispatch_action_at(Action::BreakGlass, t0),
            "a different action is not blocked by the first's window"
        );
    }

    // ── bancada: the banken → tear/mado bridge at the keystroke ──────

    #[test]
    fn keymap_binds_the_authored_bancada_chords() {
        let a = app();
        assert_eq!(a.bancadas().len(), 2, "both recipes launch from :pods");
        assert_eq!(
            act(
                a.keymap(),
                awase::Hotkey::new(awase::Modifiers::NONE, awase::Key::G)
            ),
            Some(&Action::OpenBancada(0)),
        );
        assert_eq!(
            act(
                a.keymap(),
                awase::Hotkey::new(awase::Modifiers::SHIFT, awase::Key::G)
            ),
            Some(&Action::OpenBancada(1)),
        );
        // The index and the recipe agree — the whole reason both come from
        // one filtered iteration.
        assert_eq!(a.bancadas()[0].name, "pod-triage");
        assert_eq!(a.bancadas()[1].name, "pod-break-glass");
    }

    /// **THE GATE.** The pre-warmed session plan carries the cluster banken
    /// is reading, the selected pod's namespace, and the selected pod — the
    /// whole point of the bridge.
    #[test]
    fn the_bancada_chord_plans_a_prewarmed_session_on_the_right_cluster() {
        let mut a = app_on("camelot-eks");
        a.apply_action(Action::OpenBancada(0));
        match a.overlay() {
            Some(ActionResult::BancadaPlan {
                recipe,
                legality,
                session_name,
                lines,
            }) => {
                assert_eq!(recipe, "pod-triage");
                assert_eq!(legality, "OBSERVE", "a recipe of pure reads is OBSERVE");
                assert!(
                    session_name.starts_with("triage-camelot-eks-"),
                    "got: {session_name}"
                );
                assert_eq!(lines.len(), 3, "three panes");
                assert!(
                    lines[0].contains("--context camelot-eks"),
                    "the log pane targets the cluster banken is reading: {}",
                    lines[0],
                );
                assert!(lines[0].contains("[root]"), "got: {}", lines[0]);
                assert!(lines[1].contains("[right]"), "got: {}", lines[1]);
            }
            other => panic!("expected a BancadaPlan overlay, got {other:?}"),
        }
    }

    /// **THE GATE.** The break-glass recipe's overlay states BREAK-GLASS. Its
    /// class is derived from the `kubectl exec` pane, so a recipe cannot
    /// present a live-effect session as a convenience.
    #[test]
    fn the_break_glass_bancada_overlay_states_the_gate_it_crosses() {
        let mut a = app_on("camelot-eks");
        a.apply_action(Action::OpenBancada(1));
        match a.overlay() {
            Some(ActionResult::BancadaPlan {
                recipe, legality, ..
            }) => {
                assert_eq!(recipe, "pod-break-glass");
                assert_eq!(legality, "BREAK-GLASS");
            }
            // M0 has no container picker, so `(:context container)` refuses by
            // name — which is itself the honest outcome and must SAY so.
            Some(ActionResult::Error(msg)) => assert!(
                msg.contains("container"),
                "the only acceptable refusal names the unresolved field: {msg}"
            ),
            other => panic!("expected a BancadaPlan or a named refusal, got {other:?}"),
        }
    }

    /// **THE GATE.** A banken that does not know its own cluster REFUSES to
    /// pre-warm a session rather than opening one on whatever the operator's
    /// kubeconfig currently points at. A wrong-cluster session is worse than
    /// no session.
    #[test]
    fn without_a_known_cluster_the_bancada_refuses_by_name() {
        let mut a = app(); // no `with_cluster`
        a.apply_action(Action::OpenBancada(0));
        match a.overlay() {
            Some(ActionResult::Error(msg)) => {
                assert!(
                    msg.contains("cluster"),
                    "the refusal names the field: {msg}"
                );
                assert!(
                    msg.contains("refusing"),
                    "and says it is refusing rather than guessing: {msg}"
                );
            }
            other => panic!("expected a named refusal, got {other:?}"),
        }
    }

    /// **THE GATE for `pending-banken: bancada-app-open`.** Resolving a
    /// recipe and CONFIRMING it opens a real session through the
    /// [`SessionEnv`] seam — asserted on what the env RECORDED, not on what
    /// the overlay says. "banken reported a session" and "the seam was
    /// called" are different claims and only the second one is evidence.
    ///
    /// Fail-once measured (2026-07-31): replacing the `Action::Confirm` arm's
    /// `open_bancada(pending, &self.session)` with the old
    /// preview-only behaviour turns this test red with
    ///
    /// ```text
    /// assertion `left == right` failed: confirming must OPEN the session
    ///           through the seam, not merely re-render the plan
    ///   left: 0
    ///  right: 1
    /// ```
    ///
    /// so it is checking the wire, not a shape nothing can violate.
    #[test]
    fn confirming_a_bancada_opens_it_through_the_session_seam() {
        let mut a = app_on("camelot-eks");

        // Resolving alone touches NOTHING. This half is what makes the
        // confirm step meaningful rather than decorative.
        a.apply_action(Action::OpenBancada(0));
        assert!(
            a.pending_bancada().is_some(),
            "the chord resolves a plan and awaits confirmation"
        );
        assert_eq!(
            a.session().sessions.borrow().len(),
            0,
            "resolving a bancada must not open anything",
        );

        // The name the operator was SHOWN, captured before confirming, so the
        // next assertion compares the opened session against the previewed
        // one rather than against a literal that could drift from both.
        let previewed = a
            .pending_bancada()
            .expect("just resolved")
            .plan
            .session_name()
            .to_owned();
        assert!(
            previewed.starts_with("triage-camelot-eks-"),
            "the session name carries the cluster banken reads: {previewed}",
        );

        // Confirming opens it.
        a.apply_action(Action::Confirm);

        let sessions = a.session().sessions.borrow().clone();
        assert_eq!(
            sessions.len(),
            1,
            "confirming must OPEN the session through the seam, not merely \
             re-render the plan",
        );
        assert_eq!(
            sessions[0].0, previewed,
            "and it opens the session the PREVIEW named",
        );
        // Three panes: one root + two splits, each born as its own command.
        assert_eq!(a.session().splits.borrow().len(), 2);
        assert_eq!(a.session().spawned.borrow().len(), 3);
        assert_eq!(
            a.session().witnessed_count(),
            0,
            "a pure-observe recipe witnesses nothing",
        );
        // The spawned argv is the pre-warmed one, on the cluster banken reads.
        let spawned = a.session().spawned.borrow();
        assert!(
            spawned[0].1.contains(&"camelot-eks".to_string()),
            "the first pane targets the cluster banken is reading: {:?}",
            spawned[0].1,
        );

        // And the overlay flips from preview to opened.
        match a.overlay() {
            Some(ActionResult::BancadaOpened {
                recipe,
                pane_count,
                witnessed,
                ..
            }) => {
                assert_eq!(recipe, "pod-triage");
                assert_eq!(pane_count, 3, "the env's ACTUAL pane count");
                assert!(!witnessed);
            }
            other => panic!("expected a BancadaOpened overlay, got {other:?}"),
        }
        assert!(
            a.pending_bancada().is_none(),
            "a confirmed plan is no longer pending — a second enter must not \
             open it twice",
        );
    }

    /// **THE GATE.** The BREAK-GLASS recipe's mutating pane goes through the
    /// WITNESSED arm, carrying the authored witness + runbook. The app adds
    /// no legality of its own — this is `bancada::open`'s discipline reaching
    /// the keystroke intact.
    #[test]
    fn confirming_a_break_glass_bancada_stages_through_the_witnessed_arm() {
        let mut a = app_on("camelot-eks");
        a.apply_action(Action::OpenBancada(1));

        // M0 has no container picker, so a recipe naming `(:context
        // container)` refuses by name — an honest outcome that must not be
        // read as "it opened".
        let Some(pending) = a.pending_bancada() else {
            match a.overlay() {
                Some(ActionResult::Error(msg)) => {
                    assert!(msg.contains("container"), "named refusal: {msg}");
                    return;
                }
                other => panic!("expected a plan or a named refusal, got {other:?}"),
            }
        };
        assert_eq!(pending.legality_label(), "BREAK-GLASS");

        a.apply_action(Action::Confirm);

        let witnessed = a.session().witnessed.borrow().clone();
        assert_eq!(
            witnessed.len(),
            1,
            "the exec pane MUST take the witnessed arm — it has no \
             ObservedCommand value to take the other one with",
        );
        let (_, argv, action) = &witnessed[0];
        assert!(argv.contains(&"exec".to_string()), "got {argv:?}");
        assert_eq!(action.witness.as_str(), "drzzln");
        assert!(action.runbook.0.contains("RUNBOOK"));

        match a.overlay() {
            Some(ActionResult::BancadaOpened {
                legality,
                witnessed: w,
                ..
            }) => {
                assert_eq!(legality, "BREAK-GLASS");
                assert!(w, "the overlay states that a live-effect was staged");
            }
            other => panic!("expected a BancadaOpened overlay, got {other:?}"),
        }
    }

    /// `enter` anywhere but a bancada preview is a deliberate no-op — it must
    /// not open anything, and it must not raise an error panel either.
    #[test]
    fn confirm_outside_a_preview_opens_nothing() {
        let mut a = app_on("camelot-eks");
        a.apply_action(Action::Confirm);
        assert_eq!(a.session().sessions.borrow().len(), 0);
        assert!(a.overlay().is_none(), "and it raises no panel");

        // Nor on a postigo result overlay.
        a.apply_action(Action::DeclareScale);
        a.apply_action(Action::Confirm);
        assert_eq!(a.session().sessions.borrow().len(), 0);
    }

    /// Dismissing a preview must not open it. The escape hatch has to be a
    /// real escape hatch.
    #[test]
    fn dismissing_a_preview_opens_nothing() {
        let mut a = app_on("camelot-eks");
        a.apply_action(Action::OpenBancada(0));
        a.apply_action(Action::Dismiss);
        assert!(a.pending_bancada().is_none());
        assert_eq!(a.session().sessions.borrow().len(), 0);
        // And a confirm after the dismissal has nothing to act on.
        a.apply_action(Action::Confirm);
        assert_eq!(a.session().sessions.borrow().len(), 0);
    }

    /// The authored `return` chord reaches the runtime as `enter` — the ONE
    /// measured awase→egaku-term translation this binding depends on. Without
    /// it the confirm key would be authored and unreachable.
    #[test]
    fn the_authored_confirm_chord_binds_to_enter() {
        let a = app();
        assert_eq!(
            act(
                a.keymap(),
                awase::Hotkey::new(awase::Modifiers::NONE, awase::Key::Return)
            ),
            Some(&Action::Confirm),
        );
    }

    /// A bancada opens a whole session — the most expensive thing a held key
    /// could repeat.
    #[test]
    fn opening_a_bancada_is_repeat_gated() {
        assert!(Action::OpenBancada(0).is_repeat_gated());
        let mut a = app_on("camelot-eks");
        let t0 = Instant::now();
        assert!(a.dispatch_action_at(Action::OpenBancada(0), t0));
        assert!(
            !a.dispatch_action_at(Action::OpenBancada(0), t0 + Duration::from_millis(35)),
            "a repeat tick must not plan a second session"
        );
    }

    // ── the legend must not vanish when it does not fit ──────────────

    /// **THE GATE, and it is a REGRESSION gate.** The status line used to draw
    /// the legend only `if legend_w < width`, so adding the two bancada chords
    /// made the WHOLE legend silently disappear at 80 columns — the one
    /// surface that says which gate a keystroke crosses. Now the least
    /// important entries drop and the elision is marked.
    #[test]
    fn a_narrow_legend_drops_entries_rather_than_vanishing() {
        let parts: Vec<String> = ["l:OBSERVE", "s:DECLARE", "shift+s:BREAK-GLASS", "q:quit"]
            .iter()
            .map(|s| (*s).to_owned())
            .collect();

        // Everything fits: no marker.
        let wide = fit_legend(&parts, 100);
        assert_eq!(wide, "l:OBSERVE  s:DECLARE  shift+s:BREAK-GLASS  q:quit");
        assert!(!wide.contains('…'));

        // Too narrow: the FIRST (most important) entries survive, marked.
        let narrow = fit_legend(&parts, 24);
        assert!(narrow.starts_with("l:OBSERVE"), "got: {narrow}");
        assert!(narrow.ends_with('…'), "the elision is visible: {narrow}");
        assert!(
            narrow.chars().count() <= 24,
            "and it still fits: {} chars",
            narrow.chars().count()
        );
        // Whole entries only — never half a chord label.
        assert!(!narrow.contains("s:DEC "), "no partial entry: {narrow}");

        // Nothing fits at all: an honest marker, never a blank line.
        assert_eq!(fit_legend(&parts, 3), "…");
        assert_eq!(fit_legend(&parts, 0), "");
        assert_eq!(fit_legend(&[], 40), "");
    }

    /// Navigation is deliberately NOT gated — throttling `j`/`k` would make
    /// the table feel broken, and gating what is cheap is a regression.
    #[test]
    fn navigation_is_not_repeat_gated() {
        let mut a = app();
        let t0 = Instant::now();
        for i in 0..4u64 {
            assert!(
                a.dispatch_action_at(Action::SelectNext, t0 + Duration::from_millis(i * 10)),
                "navigation tick {i} must pass through"
            );
        }
        assert_eq!(
            a.table().view().selected_index(),
            4,
            "all four navigation ticks moved the selection"
        );
        assert!(!Action::SelectNext.is_repeat_gated());
        assert!(!Action::Quit.is_repeat_gated());
        assert!(Action::DeclareScale.is_repeat_gated());
    }
}
