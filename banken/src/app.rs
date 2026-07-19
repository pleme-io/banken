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

use banken_spec::env::ClusterEnv;
use banken_spec::types::{OperatorId, ResourceKind};
use egaku_term::crossterm::style::Color;
use egaku_term::{
    __re::{KeyCombo, KeyMap},
    AsyncApp, Buffer, Result as TermResult, Style,
};

use crate::action::{ActionResult, RowAction, dispatch};
use crate::render::{draw_pod_table, sort_label};
use crate::table::PodTable;

/// The one action the app keymap resolves a key to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
    /// Dismiss the action-result overlay (`esc`).
    Dismiss,
    /// Quit (`q`).
    Quit,
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
    table: PodTable,
    panel: Panel,
    keys: KeyMap<Action>,
    done: bool,
    /// A short label describing where the pod rows came from (fixture vs
    /// live), rendered in the status line — tier-honesty in the UI itself.
    source_label: String,
}

impl<E: ClusterEnv> BankenApp<E> {
    /// Build the app over a cluster env, performing the initial OBSERVE
    /// read of the pod table.
    ///
    /// # Errors
    ///
    /// Propagates a `SpecError` if the initial `list_resources` read fails.
    pub fn new(env: E, operator: OperatorId, source_label: impl Into<String>) -> Self {
        // Initial read. A read failure yields an empty table (the app still
        // runs and shows "0 pods"); the read is retried on the next
        // refresh. Never a panic on a read error.
        let rows = env
            .list_resources(ResourceKind::Pod, None)
            .unwrap_or_default();
        Self {
            env,
            operator,
            table: PodTable::pods(rows),
            panel: Panel::Table,
            keys: default_keymap(),
            done: false,
            source_label: source_label.into(),
        }
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

    /// Apply an action to the app state. Public so tests can drive the app
    /// without a terminal (the same path `handle` takes).
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

        // Right: the key legend (the postigo chords).
        let legend = "l:OBSERVE  s:DECLARE  S:BREAK-GLASS  o:sort  q:quit";
        let legend_w = u16::try_from(legend.chars().count()).unwrap_or(0);
        if legend_w < width {
            buf.set_stringn(width - legend_w, y, legend, legend_w, bar);
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
        ActionResult::Error(msg) => (
            String::from(" ERROR "),
            Style::default().fg(Color::Red).bold(),
            vec![msg.clone()],
        ),
    }
}

/// The default keymap — the postigo chords (BANKEN.md §III.b / §VIII: the
/// muscle-memory chords transfer from k9s).
fn default_keymap() -> KeyMap<Action> {
    let mut km = KeyMap::new();
    km.bind(KeyCombo::key("down"), Action::SelectNext);
    km.bind(KeyCombo::key("j"), Action::SelectNext);
    km.bind(KeyCombo::key("up"), Action::SelectPrev);
    km.bind(KeyCombo::key("k"), Action::SelectPrev);
    km.bind(KeyCombo::key("o"), Action::ToggleSort);
    km.bind(KeyCombo::key("l"), Action::ObserveLogs);
    km.bind(KeyCombo::key("s"), Action::DeclareScale);
    // Uppercase S arrives as shift+s (event.rs lowercases + adds shift).
    km.bind(KeyCombo::new("s", vec!["shift".into()]), Action::BreakGlass);
    km.bind(KeyCombo::key("esc"), Action::Dismiss);
    km.bind(KeyCombo::key("q"), Action::Quit);
    km
}

// `AsyncApp::draw` takes `&self` and returns `impl Future + Send`, so the
// `&BankenApp<E>` borrow must be `Send` — which requires `E: Sync`.
impl<E: ClusterEnv + Send + Sync> AsyncApp for BankenApp<E> {
    type Action = Action;

    fn keymap(&self) -> &KeyMap<Action> {
        &self.keys
    }

    async fn handle(&mut self, action: &Action) -> TermResult<()> {
        self.apply_action(*action);
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
    use super::*;
    use crate::fixture::FixtureClusterEnv;

    fn app() -> BankenApp<FixtureClusterEnv> {
        BankenApp::new(
            FixtureClusterEnv::new(),
            OperatorId("drzzln".into()),
            "source: fixture",
        )
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
}
