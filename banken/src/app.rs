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
//! one of {the table, an action-result overlay}.
//!
//! # pending-banken: live-watch
//!
//! M0's refresh is nominally a 1 Hz poll (BANKEN.md §VI; the true watch
//! informer is M1 unbuilt substrate). `run_async` refreshes on every
//! event, which is sufficient against the static fixture source this
//! session (fixture data does not change, so a poll tick would be a
//! no-op). A periodic `refresh()` call — driven by a `tokio::select!`
//! tick against a live `ClusterEnv::watch` — lands with the live read.

use std::time::Instant;

use awase::repeat_gate::KeyRepeatGate;
use banken_spec::chord::ActionChord;
use banken_spec::env::ClusterEnv;
use banken_spec::nav::NavIntent;
use banken_spec::types::{OperatorId, ResourceKind};
use banken_spec::{Catalog, SpecError};
use egaku_term::crossterm::style::Color;
use egaku_term::{
    __re::{KeyCombo, KeyMap},
    AsyncApp, Buffer, Result as TermResult, Style,
};

use banken_spec::guarita::GuaritaSpec;

use crate::action::{ActionResult, RowAction, dispatch, plan_guarita};
use crate::render::{draw_pod_table, sort_label};
use crate::table::PodTable;

/// The one view M0 ships. Named once so the keymap, the table columns and the
/// guarita filter cannot drift onto three different spellings of it.
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
    /// Open the guarita at this index of the app's recipe list (`g`,
    /// `shift+g`) — a pre-warmed tear session plan for the selected row.
    ///
    /// The index is into [`BankenApp::guaritas`], which is
    /// `catalog.guaritas_from(VIEW_NAME)` in order, so the keymap and the
    /// recipe list cannot disagree: [`keymap_from_catalog`] builds both from
    /// the same filtered iteration.
    OpenGuarita(usize),
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
                // A guarita resolves a whole recipe and (once the live seam
                // lands) opens a tear session with N panes. A held key firing
                // one session per repeat tick is the runaway shape at its
                // most expensive.
                | Action::OpenGuarita(_)
        )
    }
}

/// Which screen the app currently shows — a sum type so an illegal screen
/// (e.g. "a table AND a modal AND nothing") is unrepresentable.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Panel {
    /// The `:pods` table (the default landing).
    Table,
    /// An overlay showing the result of the last postigo action.
    ActionOverlay(ActionResult),
}

/// The banken TUI app. Generic over the [`ClusterEnv`] source so the same
/// runtime drives the fixture env (this session) or the live kube env
/// (under the `live` feature) with no code change.
pub struct BankenApp<E: ClusterEnv> {
    env: E,
    operator: OperatorId,
    /// The cluster banken is reading, as the kubeconfig context name.
    ///
    /// **Empty means UNKNOWN, not "the current one".** It is what a
    /// `(defguarita)` resolves `(:context cluster)` against, and an empty
    /// value makes the planner REFUSE rather than emit `--context ""` — which
    /// would pre-warm a session on whatever cluster the operator's kubeconfig
    /// happens to point at. See [`banken_spec::guarita`].
    cluster: String,
    /// The pre-warmed session recipes reachable from this view, in the SAME
    /// order [`keymap_from_catalog`] bound them — which is what makes
    /// [`Action::OpenGuarita`]'s index total.
    guaritas: Vec<GuaritaSpec>,
    table: PodTable,
    panel: Panel,
    keys: KeyMap<Action>,
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
    /// so adding the two guarita chords made the entire legend **silently
    /// vanish** at 80 columns. [`fit_legend`] now drops whole trailing entries
    /// and marks the elision, so a narrow terminal costs the *least* important
    /// hints rather than all of them.
    legend_parts: Vec<String>,
}

impl<E: ClusterEnv> BankenApp<E> {
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
        env: E,
        operator: OperatorId,
        source_label: impl Into<String>,
    ) -> Result<Self, SpecError> {
        let catalog = banken_spec::load_catalog()?;
        Self::with_catalog(env, operator, source_label, &catalog)
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
        env: E,
        operator: OperatorId,
        source_label: impl Into<String>,
        catalog: &Catalog,
    ) -> Result<Self, SpecError> {
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
            operator,
            cluster: String::new(),
            guaritas: catalog
                .guaritas_from(VIEW_NAME)
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
    pub fn guaritas(&self) -> &[GuaritaSpec] {
        &self.guaritas
    }

    /// Re-read the pod table from the env (the poll refresh). Preserves the
    /// selection by name across the refresh.
    pub fn refresh(&mut self) {
        if let Ok(rows) = self.env.list_resources(ResourceKind::Pod, None) {
            self.table.set_rows(rows);
        }
    }

    /// The current pod table (for tests + the renderer).
    #[must_use]
    pub fn table(&self) -> &PodTable {
        &self.table
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
                self.table.select_next();
            }
            Action::SelectPrev => {
                self.panel = Panel::Table;
                self.table.select_prev();
            }
            Action::ToggleSort => {
                self.panel = Panel::Table;
                self.table.toggle_sort_direction();
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
            Action::OpenGuarita(i) => {
                // A guarita index out of range is not possible through the
                // keymap (both come from the same filtered iteration), but
                // reporting it beats an index panic in the operator's TUI.
                let r = match self.guaritas.get(i) {
                    Some(spec) => plan_guarita(&self.table, spec, &self.cluster),
                    None => ActionResult::Error(
                        "no guarita is bound at that index — the keymap and the \
                         recipe list disagree"
                            .into(),
                    ),
                };
                self.panel = Panel::ActionOverlay(r);
            }
            Action::Dismiss => self.panel = Panel::Table,
            Action::Quit => self.done = true,
        }
    }

    /// The last action result overlay, if the overlay is showing (tests).
    #[must_use]
    pub fn overlay(&self) -> Option<&ActionResult> {
        match &self.panel {
            Panel::ActionOverlay(r) => Some(r),
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
        if let Panel::ActionOverlay(result) = &self.panel {
            draw_overlay(buf, width, height, result);
        }
    }

    fn draw_status_line(&self, buf: &mut Buffer, width: u16, height: u16) {
        let y = height - 1;
        let bar = Style::default().fg(Color::Black).bg(Color::DarkGrey);
        buf.blank(0, y, width, bar);

        let mut left = String::new();
        left.push_str(&self.source_label);
        left.push_str("  ");
        left.push_str(&pod_count_label(self.table.rows().len()));
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
fn draw_overlay(buf: &mut Buffer, width: u16, height: u16, result: &ActionResult) {
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

    // Footer hint.
    if bottom > top + 1 {
        let hint = " esc: dismiss ";
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
        ActionResult::GuaritaPlan {
            recipe,
            legality,
            session_name,
            lines,
        } => {
            // The title states the DERIVED gate first — a BREAK-GLASS recipe
            // must not read like a convenience.
            let mut t = String::from(" GUARITA — ");
            t.push_str(legality);
            t.push_str(" — ");
            t.push_str(recipe);
            t.push_str(" (plan only, not opened) ");
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
///   projection (see [`crate::keys::chord_to_combo`]) — a refusal, never a
///   guessed mapping.
/// - [`SpecError::Binding`] when a [`DISPATCHABLE_ACTIONS`] row names an
///   action the catalog does not declare (the hand-list drifted ahead of the
///   spec).
pub fn keymap_from_catalog(catalog: &Catalog) -> Result<KeyMap<Action>, SpecError> {
    let mut km = KeyMap::new();

    // Navigation — the intent projection is exhaustive, so every authored
    // nav key binds.
    for nav in catalog.nav_keys() {
        let combo = project(nav.keys, &nav.name)?;
        km.bind(combo, Action::from(nav.intent));
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
        let combo = project(spec.keys, &spec.name)?;
        km.bind(combo, *action);
    }

    // guaritas — the pre-warmed tear sessions reachable from THIS view.
    // Bound by position in the same filtered iteration `BankenApp` stores,
    // so `Action::OpenGuarita(i)` cannot index a different recipe than the
    // chord names. A recipe launched from another surface is deliberately
    // NOT bound here — `unbound_guarita_names` reports it rather than
    // giving the operator a chord that opens something from a screen they
    // are not on.
    for (i, g) in catalog.guaritas_from(VIEW_NAME).into_iter().enumerate() {
        let combo = project(g.keys, &g.name)?;
        km.bind(combo, Action::OpenGuarita(i));
    }

    Ok(km)
}

/// Project an authored chord onto the delivered combo, or refuse by name.
fn project(chord: ActionChord, owner: &str) -> Result<KeyCombo, SpecError> {
    crate::keys::chord_to_combo(chord).ok_or_else(|| {
        let mut m = String::from("authored chord `");
        m.push_str(&chord.canonical());
        m.push_str("` on `");
        m.push_str(owner);
        m.push_str(
            "` has no egaku-term projection — the awase and egaku-term key \
             vocabularies diverge on it and banken refuses to guess a mapping \
             (see banken::keys)",
        );
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
/// did the moment the guarita chords landed).
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
/// the guarita chords (both of which state a *gate*), then the two navigation
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

    // A guarita's class is its DERIVED one, so the legend states the gate the
    // pre-warmed session would actually cross. A malformed recipe never
    // reaches here (`Catalog::resolve` validates every one), so the fallback
    // label is unreachable in a resolved catalog rather than a quiet default.
    for g in catalog.guaritas_from(VIEW_NAME) {
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

/// The guarita recipes the catalog declares but this view cannot launch —
/// the guaritas analogue of [`unbound_action_names`].
///
/// Surfaced as data rather than silence: a recipe whose `:from` names another
/// surface is authored, valid and unreachable from here, and the operator
/// should learn that from `--help` rather than from a chord doing nothing.
#[must_use]
pub fn unbound_guarita_names(catalog: &Catalog) -> Vec<String> {
    catalog
        .guaritas()
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
// `&BankenApp<E>` borrow must be `Send` — which requires `E: Sync`.
impl<E: ClusterEnv + Send + Sync> AsyncApp for BankenApp<E> {
    type Action = Action;

    fn keymap(&self) -> &KeyMap<Action> {
        &self.keys
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

    fn should_quit(&self) -> bool {
        self.done
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use awase::repeat_gate::DEFAULT_MIN_INTERVAL;

    use super::*;
    use crate::fixture::FixtureClusterEnv;

    fn app() -> BankenApp<FixtureClusterEnv> {
        BankenApp::try_new(
            FixtureClusterEnv::new(),
            OperatorId("drzzln".into()),
            "source: fixture",
        )
        .expect("the shipped vocabulary must build an app")
    }

    /// The same app, told which cluster it is reading — what a guarita needs
    /// to pre-warm a session on the RIGHT one.
    fn app_on(cluster: &str) -> BankenApp<FixtureClusterEnv> {
        app().with_cluster(cluster)
    }

    #[test]
    fn app_reads_the_pod_table_on_construction() {
        let a = app();
        assert_eq!(a.table().rows().len(), 5);
        assert!(a.overlay().is_none(), "no overlay at rest");
    }

    #[test]
    fn navigation_moves_the_selection() {
        let mut a = app();
        // Start at row 0, move down twice → row 2, up once → row 1.
        assert_eq!(a.table().selected_index(), 0);
        a.apply_action(Action::SelectNext);
        a.apply_action(Action::SelectNext);
        assert_eq!(a.table().selected_index(), 2);
        a.apply_action(Action::SelectPrev);
        assert_eq!(a.table().selected_index(), 1);
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
            a.keymap().lookup(&KeyCombo::key("l")),
            Some(&Action::ObserveLogs)
        );
        assert_eq!(
            a.keymap().lookup(&KeyCombo::key("s")),
            Some(&Action::DeclareScale)
        );
        assert_eq!(
            a.keymap().lookup(&KeyCombo::new("s", vec!["shift".into()])),
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
            // Project the AUTHORED chord into the delivered-combo vocabulary
            // and look it up in the app keymap — so the assertion is on the
            // authored value, not a hand-repeated literal.
            let combo = crate::keys::chord_to_combo(spec.keys).unwrap_or_else(|| {
                panic!(
                    "authored chord `{}` for `{name}` has no egaku-term combo",
                    spec.keys
                )
            });
            assert_eq!(
                a.keymap().lookup(&combo),
                Some(&expected_action),
                "the app keymap must bind `{name}`'s AUTHORED chord `{}` to {expected_action:?}",
                spec.keys,
            );
        }
        // And `shell` is bound to the SHIFTED chord specifically — the whole
        // point of authoring `shift+s` instead of `S`.
        assert_ne!(
            a.keymap().lookup(&KeyCombo::key("s")),
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

    // ── guarita: the banken → tear/mado bridge at the keystroke ──────

    #[test]
    fn keymap_binds_the_authored_guarita_chords() {
        let a = app();
        assert_eq!(a.guaritas().len(), 2, "both recipes launch from :pods");
        assert_eq!(
            a.keymap().lookup(&KeyCombo::key("g")),
            Some(&Action::OpenGuarita(0)),
        );
        assert_eq!(
            a.keymap().lookup(&KeyCombo::new("g", vec!["shift".into()])),
            Some(&Action::OpenGuarita(1)),
        );
        // The index and the recipe agree — the whole reason both come from
        // one filtered iteration.
        assert_eq!(a.guaritas()[0].name, "pod-triage");
        assert_eq!(a.guaritas()[1].name, "pod-break-glass");
    }

    /// **THE GATE.** The pre-warmed session plan carries the cluster banken
    /// is reading, the selected pod's namespace, and the selected pod — the
    /// whole point of the bridge.
    #[test]
    fn the_guarita_chord_plans_a_prewarmed_session_on_the_right_cluster() {
        let mut a = app_on("camelot-eks");
        a.apply_action(Action::OpenGuarita(0));
        match a.overlay() {
            Some(ActionResult::GuaritaPlan {
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
            other => panic!("expected a GuaritaPlan overlay, got {other:?}"),
        }
    }

    /// **THE GATE.** The break-glass recipe's overlay states BREAK-GLASS. Its
    /// class is derived from the `kubectl exec` pane, so a recipe cannot
    /// present a live-effect session as a convenience.
    #[test]
    fn the_break_glass_guarita_overlay_states_the_gate_it_crosses() {
        let mut a = app_on("camelot-eks");
        a.apply_action(Action::OpenGuarita(1));
        match a.overlay() {
            Some(ActionResult::GuaritaPlan {
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
            other => panic!("expected a GuaritaPlan or a named refusal, got {other:?}"),
        }
    }

    /// **THE GATE.** A banken that does not know its own cluster REFUSES to
    /// pre-warm a session rather than opening one on whatever the operator's
    /// kubeconfig currently points at. A wrong-cluster session is worse than
    /// no session.
    #[test]
    fn without_a_known_cluster_the_guarita_refuses_by_name() {
        let mut a = app(); // no `with_cluster`
        a.apply_action(Action::OpenGuarita(0));
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

    /// A guarita opens a whole session — the most expensive thing a held key
    /// could repeat.
    #[test]
    fn opening_a_guarita_is_repeat_gated() {
        assert!(Action::OpenGuarita(0).is_repeat_gated());
        let mut a = app_on("camelot-eks");
        let t0 = Instant::now();
        assert!(a.dispatch_action_at(Action::OpenGuarita(0), t0));
        assert!(
            !a.dispatch_action_at(Action::OpenGuarita(0), t0 + Duration::from_millis(35)),
            "a repeat tick must not plan a second session"
        );
    }

    // ── the legend must not vanish when it does not fit ──────────────

    /// **THE GATE, and it is a REGRESSION gate.** The status line used to draw
    /// the legend only `if legend_w < width`, so adding the two guarita chords
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
            a.table().selected_index(),
            4,
            "all four navigation ticks moved the selection"
        );
        assert!(!Action::SelectNext.is_repeat_gated());
        assert!(!Action::Quit.is_repeat_gated());
        assert!(Action::DeclareScale.is_repeat_gated());
    }
}
