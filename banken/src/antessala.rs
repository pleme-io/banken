//! antessala — the antechamber: the room before the room.
//!
//! # What was there before
//!
//! One `eprintln!` on the primary screen and then nothing:
//!
//! ```text
//! banken: connecting to `engenho-local` (kubeconfig → auth → apiserver)…
//! ```
//!
//! …followed by a blank terminal for as long as the connect took. On an EKS
//! context that is not a flicker: `Client::try_from` runs the kubeconfig's
//! `exec` credential helper with a **blocking** `cmd.output()`
//! (`live.rs:509`), which on `aws eks get-token` is an AWS SSO round-trip.
//! Seconds of an empty dark window, and the operator cannot tell a slow
//! network from a wedged process from a helper sitting on an expired session.
//!
//! The line was not careless — it names the three stages, and it was
//! deliberately printed *before* the alt-screen so it would survive on the
//! primary screen as a receipt of which estate was read. Both of those are
//! good instincts. What it could not do is **move**, because it was written
//! once, before the work started, from outside the loop.
//!
//! # What this is
//!
//! The wait, made into a place. The connect runs as a task that publishes
//! [`Stage`] transitions; [`ConnectingScreen`] draws them inside the alt
//! screen, one line per stage, with the live one animated and timed. The same
//! three stages the old line named — now observed rather than asserted.
//!
//! Three things fall out that a printed line structurally could not give:
//!
//! - **The slow stage names itself.** `credentials` sitting at 4.2s is a
//!   diagnosis; a blank screen at 4.2s is not.
//! - **The wait is cancellable.** `esc` aborts the connect and returns to the
//!   list. It used to be un-interruptible: an operator who picked the wrong
//!   cluster, or one behind a VPN that is down, waited out kube's default
//!   connect timeout with no way to say "not that one".
//! - **The receipt gets better, not worse.** It moves to the way OUT
//!   (`main.rs`), where it is the durable record of the estate actually read
//!   rather than of the one that was about to be attempted — a distinction
//!   that mattered every time the attempt then failed.
//!
//! # Why the stages are published rather than inferred
//!
//! banken cannot see inside `Client::try_from`; it is one opaque call that
//! both authenticates and builds the client. So [`Stage`] marks what banken
//! is **about to do**, published at each call boundary in
//! [`crate::live::KubeClusterEnv::connect_with_context_staged`]. That is an
//! honest resolution — three observations, not a progress bar interpolating a
//! percentage nobody measured.
//!
//! # The name
//!
//! *antessala*, the antechamber — the room you are in before the room you
//! asked for. Brazilian-Portuguese per the fleet naming laws (Tier-2+
//! places/flows take Portuguese; Japanese is the foundational substrate
//! layer, which is why the navigator itself is `banken` 番犬), and the literal
//! gloss is the job: somewhere to be while the door is being opened.

use std::time::Duration;

use egaku_term::crossterm::style::Color;
use egaku_term::{__re::KeyMap, AsyncApp, Buffer, Result as TermResult, Style};

/// How often the screen repaints while it is waiting.
///
/// 12 fps. Fast enough that the spinner reads as motion rather than as a
/// stutter, slow enough that a screen doing nothing but waiting is not
/// burning a core to say so.
const FRAME: Duration = Duration::from_millis(80);

/// The elapsed time below which no timer is drawn.
///
/// A connect that resolves in 200 ms should not flash a stopwatch on its way
/// past — the timer exists to reassure during a *long* wait, and showing it
/// for a fast one just adds a number that appears and vanishes.
const TIMER_AFTER: Duration = Duration::from_millis(900);

/// How far a connect has got.
///
/// Ordered, and the order is the screen's: a connect only ever moves forward,
/// so `PartialOrd` is what lets the drawer ask "is this stage behind the
/// current one" instead of holding a second list of which are done.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub enum Stage {
    /// Resolving the named context out of the `KUBECONFIG` merge list —
    /// banken's own unmerged read, the one that refuses an ambiguous name.
    #[default]
    Kubeconfig,
    /// `Config::from_kubeconfig`: parsing, selecting the cluster/user, and
    /// loading the CA bundle off disk. File IO only — this stage does **not**
    /// run the auth plugin, despite its position in the chain.
    Configuration,
    /// `Client::try_from`: where the `exec` credential helper actually runs.
    ///
    /// **The slow one, and the only one that leaves the machine.** On EKS this
    /// is `aws eks get-token`, which on a cold SSO session is an interactive
    /// browser round-trip's worth of latency.
    Credentials,
    /// `BankenApp::try_new`'s initial pod listing.
    ///
    /// **This stage exists because covering only the connect was not enough,
    /// and the difference was measured rather than reasoned.** The first
    /// version of this screen ended at the connect, and against `cid-k3s` the
    /// terminal still went blank for a long time afterwards — the antechamber
    /// had closed correctly (`run_async returned ok=true stage=Settled`) and
    /// the wait had simply moved one stage down, into the synchronous
    /// `env.list_resources` inside `with_catalog` (`app.rs:309`). A transition
    /// that covers all but the last blocking call is not a transition; it just
    /// relocates the blank screen.
    FirstRead,
    /// Everything the first frame needs is ready — or the attempt failed.
    ///
    /// Deliberately ONE terminal stage rather than a `Succeeded`/`Failed`
    /// pair: the outcome is a `Result` on the task handle, and duplicating it
    /// into the stage channel would create two sources for one fact that
    /// could disagree.
    Settled,
}

impl Stage {
    /// Every stage in order — the checklist the screen draws, derived rather
    /// than hand-listed so a new stage cannot be added without appearing.
    pub const ALL: [Self; 5] = [
        Self::Kubeconfig,
        Self::Configuration,
        Self::Credentials,
        Self::FirstRead,
        Self::Settled,
    ];

    /// The short name on the left of the row.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Kubeconfig => "kubeconfig",
            Self::Configuration => "configuration",
            Self::Credentials => "credentials",
            Self::FirstRead => "first read",
            Self::Settled => "ready",
        }
    }

    /// What is happening, in the operator's terms.
    ///
    /// The `credentials` line names the *subprocess*, because that is the
    /// thing that hangs and the thing they can go look at.
    #[must_use]
    pub fn detail(self) -> &'static str {
        match self {
            Self::Kubeconfig => "resolving the context out of the merge list",
            Self::Configuration => "reading the cluster and its CA bundle",
            Self::Credentials => "running the exec credential helper",
            Self::FirstRead => "listing pods on the cluster",
            Self::Settled => "ready",
        }
    }
}

/// Publishes [`Stage::Settled`] when it is dropped.
///
/// **The screen's only non-operator way out is seeing `Settled`, so a path
/// that fails to publish it hangs the terminal.** Holding that as a rule to
/// remember at each `?` is exactly the kind of thing that is right until it
/// is not — so it is a `Drop` instead, and every exit becomes structural:
/// an early return, an error, a panic unwinding through the task, and an
/// `abort()` that drops the future mid-poll all settle the screen.
///
/// This is what a "confirm the stage was reported" test could never buy: the
/// property is now carried by the type rather than checked on the paths
/// somebody thought to check.
#[derive(Debug)]
pub struct SettleOnDrop(StageReporter);

impl SettleOnDrop {
    /// Arm the guard. Announce stages through it for the duration.
    #[must_use]
    pub fn new(reporter: StageReporter) -> Self {
        Self(reporter)
    }

    /// The reporter, for announcing the stages along the way.
    #[must_use]
    pub fn reporter(&self) -> &StageReporter {
        &self.0
    }
}

impl Drop for SettleOnDrop {
    fn drop(&mut self) {
        self.0.reached(Stage::Settled);
    }
}

/// The write half of the stage channel, held by the connect task.
#[derive(Debug, Clone)]
pub struct StageReporter(tokio::sync::watch::Sender<Stage>);

impl StageReporter {
    /// Announce that the connect is now working on `stage`.
    ///
    /// A send failure means the screen is already gone (the operator
    /// cancelled), which is not the connect's problem — it is ignored rather
    /// than propagated, so a cancelled wait cannot turn into a connect error.
    pub fn reached(&self, stage: Stage) {
        let _gone = self.0.send(stage);
    }
}

/// The read half, held by the screen.
pub type StageWatch = tokio::sync::watch::Receiver<Stage>;

/// A fresh stage channel, starting at [`Stage::Kubeconfig`].
///
/// The initial value is the first real stage rather than a `Pending` variant,
/// because by the time anyone holds the receiver the task is already resolving
/// the context — a "not started yet" state would be a lie for the whole of its
/// existence.
#[must_use]
pub fn channel() -> (StageReporter, StageWatch) {
    let (tx, rx) = tokio::sync::watch::channel(Stage::Kubeconfig);
    (StageReporter(tx), rx)
}

/// What the operator did while waiting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Waited {
    /// The connect settled on its own; read the outcome off the task handle.
    Through,
    /// The operator gave up. The caller aborts the task and goes back.
    Cancelled,
}

/// The one thing a keystroke can do here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaitAction {
    /// Stop waiting and go back to the list.
    Cancel,
}

/// The antechamber screen.
pub struct ConnectingScreen {
    context: String,
    server: Option<String>,
    stage: Stage,
    watch: StageWatch,
    keys: awase::KeyMode<WaitAction>,
    /// The animation phase. A frame counter, not a clock — it advances once
    /// per repaint, so the spinner cannot appear to stall on a slow terminal
    /// while still turning in the state.
    frame: usize,
    /// How long the wait has run. Held rather than computed from an `Instant`
    /// on every draw so a test can set it directly and assert the rendered
    /// row, which is the thing that was actually wrong before.
    elapsed: Duration,
    started: Option<std::time::Instant>,
    cancelled: bool,
}

impl ConnectingScreen {
    /// Open the antechamber for `context`.
    ///
    /// `server` is the apiserver URL when the caller already knows it — the
    /// picker does, because the row it was chosen from shows it. The
    /// `--context` path does not, and passes `None` rather than re-resolving:
    /// the URL is a *check*, and a screen that has nothing to check is better
    /// than one that resolves the kubeconfig a second time to fill a line.
    #[must_use]
    pub fn new(context: impl Into<String>, server: Option<String>, watch: StageWatch) -> Self {
        // Read before the move: `*watch.borrow()` inside the literal holds a
        // borrow across the field that consumes it.
        let stage = *watch.borrow();
        Self {
            context: context.into(),
            server,
            stage,
            watch,
            keys: wait_keymap(),
            frame: 0,
            elapsed: Duration::ZERO,
            started: Some(std::time::Instant::now()),
            cancelled: false,
        }
    }

    /// Whether the operator gave up.
    #[must_use]
    pub fn outcome(&self) -> Waited {
        if self.cancelled {
            Waited::Cancelled
        } else {
            Waited::Through
        }
    }

    /// The stage currently drawn (for a test).
    #[must_use]
    pub fn stage(&self) -> Stage {
        self.stage
    }

    /// Advance the animation and re-read the stage.
    ///
    /// The whole `&mut self` half of the wakeup: `wake` only sleeps, so
    /// nothing here can be dropped part-way.
    fn tick(&mut self) {
        self.frame = self.frame.wrapping_add(1);
        self.stage = *self.watch.borrow();
        if let Some(started) = self.started {
            self.elapsed = started.elapsed();
        }
    }

    /// Paint the screen. Public so a `TestBackend` can assert the frame
    /// without a terminal — the same seam [`crate::picker`] uses.
    pub fn render(&self, buf: &mut Buffer) {
        let width = buf.width();
        let height = buf.height();
        if width == 0 || height == 0 {
            return;
        }

        // The block is vertically centred and left-aligned to a soft margin,
        // rather than pinned to the top-left. A wait screen is the one frame
        // with nothing competing for the space, and putting the content where
        // the eye already rests is the whole difference between "elegant" and
        // "a line of text in a dark rectangle".
        let rows = u16::try_from(Stage::ALL.len()).unwrap_or(4);
        let block = rows + 4; // title, server, blank, …stages…, blank, footer
        let top = height.saturating_sub(block) / 2;
        let left = if width > 60 { 6 } else { 1 };

        // ── Who ──
        let mut y = top;
        let x = buf.set_stringn(
            left,
            y,
            "banken 番犬  ",
            width.saturating_sub(left),
            Style::default().fg(Color::DarkGrey),
        );
        buf.set_stringn(
            x,
            y,
            &self.context,
            width.saturating_sub(x),
            Style::default().fg(Color::Cyan).bold(),
        );

        // ── Where. The URL is the check a name cannot give you, so it is on
        // screen while the connect is still deniable, not only after. ──
        if let Some(server) = &self.server {
            y = y.saturating_add(1);
            if y < height {
                buf.set_stringn(
                    left,
                    y,
                    server,
                    width.saturating_sub(left),
                    Style::default().fg(Color::DarkGrey),
                );
            }
        }

        // ── The stages ──
        y = y.saturating_add(2);
        for stage in Stage::ALL {
            if y >= height.saturating_sub(1) {
                break;
            }
            self.draw_stage(buf, left, y, width, stage);
            y = y.saturating_add(1);
        }

        // ── The way out. An un-interruptible wait is what this replaces, so
        // saying it is interruptible is not decoration. ──
        y = y.saturating_add(1);
        if y < height {
            buf.set_stringn(
                left,
                y,
                "escape: cancel and go back to the list",
                width.saturating_sub(left),
                Style::default().fg(Color::DarkGrey),
            );
        }
    }

    fn draw_stage(&self, buf: &mut Buffer, left: u16, y: u16, width: u16, stage: Stage) {
        let (marker, marker_style) = if stage < self.stage {
            ("✓", Style::default().fg(Color::Green))
        } else if stage == self.stage && stage != Stage::Settled {
            (spinner_frame(self.frame), Style::default().fg(Color::Cyan))
        } else if stage == Stage::Settled && self.stage == Stage::Settled {
            ("✓", Style::default().fg(Color::Green))
        } else {
            ("·", Style::default().fg(Color::DarkGrey))
        };

        let live = stage == self.stage;
        let done = stage < self.stage;

        let mut x = buf.set_stringn(left, y, marker, width.saturating_sub(left), marker_style);
        x = buf.set_stringn(x, y, "  ", width.saturating_sub(x), Style::default());

        let label_style = if live {
            Style::default().bold()
        } else if done {
            Style::default().fg(Color::DarkGrey)
        } else {
            Style::default().fg(Color::DarkGrey)
        };
        x = buf.set_stringn(x, y, stage.label(), width.saturating_sub(x), label_style);

        // Only the LIVE row carries its detail. Every row carrying one turns
        // the block into a paragraph, and the point of the block is that the
        // eye lands on the single line that is currently true.
        if live {
            x = buf.set_stringn(x, y, &pad_to(stage.label(), 16), width.saturating_sub(x), Style::default());
            x = buf.set_stringn(
                x,
                y,
                stage.detail(),
                width.saturating_sub(x),
                Style::default().fg(Color::DarkGrey),
            );
            if self.elapsed >= TIMER_AFTER {
                let t = elapsed_label(self.elapsed);
                buf.set_stringn(
                    x.saturating_add(2),
                    y,
                    &t,
                    width.saturating_sub(x.saturating_add(2)),
                    Style::default().fg(Color::Yellow),
                );
            }
        }
    }
}

/// The spinner glyph for a frame.
///
/// Braille, because it is the one animated-glyph set that is a single cell
/// wide in every terminal font the fleet runs and does not shift the row's
/// layout as it turns.
fn spinner_frame(frame: usize) -> &'static str {
    const FRAMES: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
    FRAMES[frame % FRAMES.len()]
}

/// Pad a label out to a column so the details line up.
fn pad_to(s: &str, col: usize) -> String {
    " ".repeat(col.saturating_sub(s.chars().count()).max(1))
}

/// `"4.2s"` — one decimal, because the digit that moves is what says the
/// process is alive, and a second decimal moves too fast to read.
fn elapsed_label(d: Duration) -> String {
    let tenths = d.as_millis() / 100;
    let mut s = (tenths / 10).to_string();
    s.push('.');
    s.push_str(&(tenths % 10).to_string());
    s.push('s');
    s
}

/// `escape` and `ctrl+c`, bound structurally.
///
/// Deliberately NOT derived from the `(defnavkey)` vocabulary, and that is a
/// scope statement rather than an oversight: `NavIntent` describes moving
/// around a resource view, and this screen has no view to move around — it has
/// one key and it means "stop waiting". Both chords are unconditional here.
/// There is no stance to route them through, which is exactly why this screen
/// does not need the `Escape`/`Abort` split [`crate::picker`] does.
fn wait_keymap() -> awase::KeyMode<WaitAction> {
    let mut km: awase::KeyMode<WaitAction> = awase::KeyMode::typed("antessala", false);
    for hotkey in [
        awase::Hotkey::new(awase::Modifiers::NONE, awase::Key::Escape),
        awase::Hotkey::new(awase::Modifiers::CTRL, awase::Key::C),
    ] {
        let _prev = km.add_binding(awase::Binding::new(hotkey, WaitAction::Cancel));
    }
    km
}

impl AsyncApp for ConnectingScreen {
    type Action = WaitAction;

    /// Vestigial — dispatch goes through [`Self::hotkey_map`], the same shape
    /// as [`crate::picker::ContextPicker`].
    fn keymap(&self) -> &KeyMap<WaitAction> {
        static EMPTY: std::sync::OnceLock<KeyMap<WaitAction>> = std::sync::OnceLock::new();
        EMPTY.get_or_init(KeyMap::new)
    }

    fn hotkey_map(&self) -> Option<&awase::KeyMode<WaitAction>> {
        Some(&self.keys)
    }

    /// A keystroke here is never text: there is nothing to type into.
    ///
    /// `egaku_term::app::Unclaimed` and not `egaku_term::Unclaimed` — the enum
    /// is `pub` in a `pub mod` but is NOT re-exported at the crate root
    /// (`egaku-term/src/lib.rs:89-97`), so the full path is the only way to
    /// name it. Worth a line because every neighbouring type here IS
    /// re-exported, which makes the absence read as a typo.
    fn unclaimed(&self) -> egaku_term::app::Unclaimed {
        egaku_term::app::Unclaimed::Consume
    }

    async fn handle(&mut self, action: &WaitAction) -> TermResult<()> {
        match action {
            WaitAction::Cancel => self.cancelled = true,
        }
        Ok(())
    }

    /// A plain sleep, which is cancellation-safe — the contract `wake`
    /// documents. The stage is re-read in [`Self::tick`] from `on_wake`,
    /// which always runs to completion, so a stage change cannot be observed
    /// half-applied.
    async fn wake(&self) {
        tokio::time::sleep(FRAME).await;
    }

    async fn on_wake(&mut self) -> TermResult<()> {
        self.tick();
        Ok(())
    }

    async fn draw(&self, frame: &mut Buffer) -> TermResult<()> {
        self.render(frame);
        Ok(())
    }

    fn should_quit(&self) -> bool {
        self.cancelled || self.stage == Stage::Settled
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egaku_term::TestBackend;

    fn screen() -> (StageReporter, ConnectingScreen) {
        let (tx, rx) = channel();
        let s = ConnectingScreen::new(
            "camelot-eks",
            Some("https://camelot.gr7.us-east-2.eks.amazonaws.com".into()),
            rx,
        );
        (tx, s)
    }

    fn frame_of(s: &ConnectingScreen) -> String {
        let mut backend = TestBackend::new(100, 20);
        backend.draw(|buf| s.render(buf));
        backend.to_lines().join("\n")
    }

    /// **The identity is on screen before anything is dialled.** The old line
    /// named the context and nothing else; the URL is the field a name cannot
    /// give you, and the wait is exactly when there is room for it.
    #[test]
    fn the_screen_names_the_cluster_and_the_apiserver() {
        let (_tx, s) = screen();
        let frame = frame_of(&s);
        assert!(frame.contains("camelot-eks"), "{frame}");
        assert!(frame.contains("eks.amazonaws.com"), "{frame}");
    }

    /// Every stage is listed from the first frame, so the operator can see
    /// how much is left rather than only what is happening.
    #[test]
    fn every_stage_is_listed_from_the_first_frame() {
        let (_tx, s) = screen();
        let frame = frame_of(&s);
        for stage in Stage::ALL {
            assert!(frame.contains(stage.label()), "{}: {frame}", stage.label());
        }
    }

    /// **The live stage names what is happening; the others do not.** This is
    /// the property that makes the block readable at a glance — one true line,
    /// not four competing ones.
    #[test]
    fn only_the_live_stage_carries_its_detail() {
        let (tx, mut s) = screen();
        tx.reached(Stage::Credentials);
        s.tick();
        let frame = frame_of(&s);
        assert!(
            frame.contains(Stage::Credentials.detail()),
            "the live stage says what it is doing: {frame}",
        );
        assert!(
            !frame.contains(Stage::Configuration.detail()),
            "a finished stage does not: {frame}",
        );
    }

    /// A finished stage is ticked, a pending one is not — read off the SAME
    /// ordering the type carries, so the two cannot disagree.
    #[test]
    fn finished_stages_are_ticked_and_pending_ones_are_not() {
        let (tx, mut s) = screen();
        tx.reached(Stage::Credentials);
        s.tick();
        let frame = frame_of(&s);
        assert_eq!(frame.matches('✓').count(), 2, "kubeconfig + configuration: {frame}");
        assert!(frame.contains('·'), "apiserver is still pending: {frame}");
    }

    /// The spinner has to actually turn — a static glyph on a wait screen is
    /// indistinguishable from a wedged process, which is the whole complaint
    /// this screen answers.
    #[test]
    fn the_spinner_advances_with_the_frame() {
        let (_tx, mut s) = screen();
        let first = frame_of(&s);
        s.tick();
        assert_ne!(first, frame_of(&s), "the frame must change while waiting");
    }

    /// **The timer appears only once the wait is long enough to need it.** A
    /// stopwatch that flashes past on a 200 ms connect is noise.
    #[test]
    fn the_timer_appears_only_after_the_wait_gets_long() {
        let (_tx, mut s) = screen();
        s.started = None; // drive `elapsed` by hand
        s.elapsed = Duration::from_millis(200);
        s.tick();
        assert!(!frame_of(&s).contains("0.2s"), "too early for a timer");

        s.elapsed = Duration::from_millis(4200);
        assert!(frame_of(&s).contains("4.2s"), "{}", frame_of(&s));
    }

    #[test]
    fn the_elapsed_label_reads_in_tenths() {
        assert_eq!(elapsed_label(Duration::from_millis(4249)), "4.2s");
        assert_eq!(elapsed_label(Duration::from_millis(12_000)), "12.0s");
        assert_eq!(elapsed_label(Duration::ZERO), "0.0s");
    }

    /// **The wait ends when the connect settles**, and that is the ONLY
    /// non-operator way out — so a task that forgets to publish `Settled`
    /// hangs the screen. That is why the reporter publishes it on every exit
    /// path in `connect_with_context_staged`, not just the happy one.
    #[test]
    fn the_screen_closes_when_the_connect_settles() {
        let (tx, mut s) = screen();
        assert!(!s.should_quit());
        tx.reached(Stage::Credentials);
        s.tick();
        assert!(!s.should_quit(), "still working");
        tx.reached(Stage::Settled);
        s.tick();
        assert!(s.should_quit());
        assert_eq!(s.outcome(), Waited::Through);
    }

    /// **The guard settles on EVERY exit, including the ones nobody writes a
    /// test for.** The screen's only non-operator way out is `Settled`, so a
    /// build path that returns early — or panics, or is aborted mid-poll —
    /// would otherwise leave the terminal on a spinner forever.
    #[test]
    fn the_guard_settles_when_the_build_returns_early() {
        let (tx, mut s) = screen();
        {
            let settle = SettleOnDrop::new(tx);
            settle.reporter().reached(Stage::Credentials);
            s.tick();
            assert_eq!(s.stage(), Stage::Credentials);
            assert!(!s.should_quit(), "still working");
            // …and here the build gives up, without reporting anything.
        }
        s.tick();
        assert_eq!(s.stage(), Stage::Settled, "the drop settled it");
        assert!(s.should_quit());
    }

    /// The same guarantee under a PANIC unwinding through the guard, which is
    /// the case an explicit `reporter.reached(Settled)` at the end of the body
    /// would miss entirely.
    #[test]
    fn the_guard_settles_when_the_build_panics() {
        let (tx, mut s) = screen();
        let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _settle = SettleOnDrop::new(tx);
            panic!("the credential helper blew up");
        }));
        assert!(caught.is_err(), "the panic really happened");
        s.tick();
        assert_eq!(s.stage(), Stage::Settled);
        assert!(s.should_quit(), "and the operator is not stranded on a spinner");
    }

    /// **The wait covers the FIRST READ, not just the connect.** Measured: the
    /// connect-only version returned `stage=Settled` correctly and the
    /// terminal still went blank afterwards, through `try_new`'s synchronous
    /// pod listing. A transition that stops one call short just moves the
    /// blank screen.
    #[test]
    fn the_first_read_is_one_of_the_stages() {
        let (tx, mut s) = screen();
        tx.reached(Stage::FirstRead);
        s.tick();
        assert!(!s.should_quit(), "the first read is still part of the wait");
        let frame = frame_of(&s);
        assert!(frame.contains("first read"), "{frame}");
        assert!(frame.contains(Stage::FirstRead.detail()), "{frame}");
        assert!(
            Stage::Credentials < Stage::FirstRead && Stage::FirstRead < Stage::Settled,
            "and it sits between the connect and the frame",
        );
    }

    /// **The wait is cancellable**, which it was not: a connect against a
    /// cluster behind a VPN that is down held the terminal until kube's own
    /// timeout, with no key that meant "not that one".
    #[tokio::test]
    async fn escape_cancels_the_wait() {
        let (_tx, mut s) = screen();
        s.handle(&WaitAction::Cancel).await.expect("infallible");
        assert!(s.should_quit());
        assert_eq!(s.outcome(), Waited::Cancelled);
    }

    #[test]
    fn both_escape_chords_cancel() {
        let (_tx, s) = screen();
        for hk in [
            awase::Hotkey::new(awase::Modifiers::NONE, awase::Key::Escape),
            awase::Hotkey::new(awase::Modifiers::CTRL, awase::Key::C),
        ] {
            assert_eq!(
                s.keys
                    .find_binding(&hk, &awase::MatchContext::default())
                    .map(|b| b.action),
                Some(WaitAction::Cancel),
                "{hk:?}",
            );
        }
    }

    /// A keystroke on this screen is never text — there is nothing to type
    /// into, so an unbound key must be consumed rather than routed to
    /// `on_text` and silently dropped there.
    #[test]
    fn unbound_keys_are_consumed_not_typed() {
        let (_tx, s) = screen();
        assert_eq!(s.unclaimed(), egaku_term::app::Unclaimed::Consume);
    }

    /// The `--context` path has no URL to show and must not invent one.
    #[test]
    fn a_screen_without_a_known_apiserver_still_renders() {
        let (_tx, rx) = channel();
        let s = ConnectingScreen::new("rio", None, rx);
        let frame = frame_of(&s);
        assert!(frame.contains("rio"), "{frame}");
        assert!(frame.contains("kubeconfig"), "{frame}");
    }

    /// A narrow or tiny terminal must not panic or drop the screen — the same
    /// guard the picker carries.
    #[test]
    fn a_tiny_terminal_still_renders() {
        let (_tx, s) = screen();
        for (w, h) in [(20_u16, 5_u16), (8, 3), (1, 1), (0, 0), (200, 60)] {
            let mut backend = TestBackend::new(w, h);
            backend.draw(|buf| s.render(buf));
        }
    }

    /// The stage ordering IS the checklist ordering. If a stage is added out
    /// of order the screen would tick a row that has not run.
    #[test]
    fn the_stage_order_is_the_checklist_order() {
        let mut sorted = Stage::ALL;
        sorted.sort_unstable();
        assert_eq!(sorted, Stage::ALL);
        assert!(Stage::Kubeconfig < Stage::Credentials);
        assert_eq!(Stage::default(), Stage::Kubeconfig);
    }
}
