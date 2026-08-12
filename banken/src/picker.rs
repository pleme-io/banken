//! The landing screen: **choose a cluster**.
//!
//! # Why banken opens on a picker and not on a table
//!
//! Bare `banken` used to land on the fixture — five invented pods, honestly
//! labelled `source: fixture` in the status line and looking in every other
//! respect exactly like a cluster. A navigator whose default screen is
//! fabricated data is not a navigator, and the label does not fix it: the
//! operator's eye goes to the rows.
//!
//! The alternative on offer was the other half of the same shape: `banken
//! --live` refused, printed eighteen context names, and exited. banken knew
//! what the operator wanted, held the exact list they needed, and made them
//! retype one entry of it. A refusal that hands over the answer and then
//! declines to act on it is friction wearing a safety costume.
//!
//! So the landing is the list itself. **The invariant is not weakened — it is
//! enforced one step earlier and more strongly.** `--context <name>` remains
//! required for a *named* live run, and picking is not a way around it:
//!
//! - the chosen name arrives at [`crate::cli::Invocation::Live`] as the same
//!   non-optional `String` a typed `--context` produces, so there is still no
//!   representation of "read an unnamed estate";
//! - each row shows the **apiserver URL** the name resolves to before it is
//!   dialled, which typing the name never did;
//! - a name declared by more than one kubeconfig file is shown as ambiguous
//!   **and cannot be chosen** — where typing it would have been accepted at
//!   the CLI and refused only later, by
//!   [`crate::live::ContextError::Ambiguous`], after the operator had already
//!   committed to it.
//!
//! That third point is the 2026-08-08 `charlie-local` hazard (one name, a
//! local apiserver in one file and a remote one in another) surfaced at the
//! moment of *choosing* rather than at the moment of connecting.
//!
//! # The widget is egaku's; only the drawing is banken's
//!
//! State is [`egaku::FuzzyPicker`] — the fleet's modal fuzzy chooser, already
//! carrying the query, the ranking and the highlight (QUADRO T1: widgets live
//! in egaku, never in an app). banken adds no widget.
//!
//! The *drawer* is here, for the same reason [`crate::render`] keeps its own
//! table drawer: per-row semantics egaku-term's generic list drawer does not
//! carry. An ambiguous row draws red and refuses; the current-context row is
//! annotated; the apiserver draws dim beside the name. `egaku_term::draw::list`
//! renders `Vec<String>` with one selection style, so adopting it would trade
//! the whole ambiguity signal for a shorter file. Every glyph is still a typed
//! `Buffer` write — no `format!()` of VT (★★ TYPED EMISSION).
//!
//! # Where the keys come from
//!
//! The same authored `(defnavkey)` vocabulary the table uses — **filtered to
//! the chords that cannot be typed**. A picker's keyboard belongs to the
//! query: binding the authored `j` would mean an operator filtering for
//! `jaeger` moved the cursor instead. So `up`/`down`/`return`/`escape` bind
//! and `j`/`k`/`o`/`q` deliberately do not, which is a *derivation* rather
//! than a second key list — re-spelling `(defnavkey :keys "down")` upstream
//! moves this screen's binding too.
//!
//! **That filter is a property of INSERT, and the screen now opens in
//! NORMAL** ([`ContextPicker::OPENING_STANCE`]). The typable chords are not
//! lost — they arrive at [`crate::vim`] instead, which is where a modal screen
//! keeps its command keys: `j`/`k` move rows, `q` leaves, `i` starts typing.
//! One keyboard, two readings, and the keymap above is only the half that
//! must hold in BOTH stances.
//!
//! The erase chords — `backspace`, `ctrl+w`, `ctrl+u`, `ctrl+k`, `delete` —
//! are bound structurally and are deliberately **not** `(defnavkey)`s: they
//! edit a text query, not a selection. The nav vocabulary describes moving
//! around a resource view, and giving it a text-editing intent would make
//! `NavIntent` mean two things.

use awase::{Key, Modifiers};
use banken_spec::Catalog;
use banken_spec::nav::NavIntent;
use egaku::{FuzzyPicker, PickerEffect, PickerEvent, PickerItem};
use egaku_term::crossterm::style::Color;
use egaku_term::{__re::KeyMap, AsyncApp, Buffer, Result as TermResult, Style};

use crate::live::{ContextChoice, ContextError, enumerate_contexts, kubeconfig_current_context};

/// What a keystroke does on the picker screen.
///
/// Deliberately smaller than [`crate::app::Action`]: this screen reads nothing
/// and mutates nothing, so there is no `postigo` class anywhere in it. The one
/// cluster-facing act it can produce is *returning a chosen name to `main`*,
/// which then goes through the ordinary `--context` path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickerAction {
    /// Move the highlight up one row (wraps).
    Up,
    /// Move the highlight down one row (wraps).
    Down,
    /// Commit the highlighted context.
    Accept,
    /// `esc` — routed through the modal layer, which cancels a pending
    /// operator, or leaves Insert, or does nothing.
    ///
    /// **Deliberately not named `Cancel` any more.** It was, and it was bound
    /// to `escape` AND `ctrl+c` at once — so making a resting `esc` inert (the
    /// operator's ask) would have silently taken `ctrl+c` down with it, since
    /// both arrived at one arm that routed into the modal layer. The escape
    /// hatch is now [`Self::Abort`], a separate action that cannot be reached
    /// through a stance and therefore cannot be swallowed by one.
    Escape,
    /// `ctrl+c` — leave, unconditionally, from any stance.
    Abort,
    /// Erase one character backwards from the caret (`backspace`).
    Erase,
    /// `ctrl+w` — erase the word before the caret.
    EraseWord,
    /// `ctrl+u` — erase from the start of the query to the caret.
    EraseToStart,
    /// `ctrl+k` — erase from the caret to the end of the query.
    EraseToEnd,
    /// `delete` — erase the character AT the caret.
    EraseForward,
}

/// Why the picker could not be opened.
#[derive(Debug)]
pub enum PickerError {
    /// The kubeconfig declares no contexts at all — there is nothing to pick.
    ///
    /// A distinct arm from [`Self::Kubeconfig`] because "I read your config and
    /// it names no clusters" and "I could not read your config" call for
    /// different fixes, and collapsing them would send the operator looking in
    /// the wrong place.
    NoContexts,
    /// A kubeconfig file that exists could not be read or parsed.
    Kubeconfig(ContextError),
}

impl std::fmt::Display for PickerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoContexts => f.write_str(
                "no kubeconfig context to choose from. banken reads the KUBECONFIG merge \
                 list (or ~/.kube/config); none of those files declares a context. \
                 Run `banken --fixture` to explore the interface without a cluster.",
            ),
            Self::Kubeconfig(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for PickerError {}

/// The cluster-chooser screen.
pub struct ContextPicker {
    picker: FuzzyPicker<ContextChoice>,
    /// How many contexts exist in total. `FuzzyPicker` exposes only the
    /// *visible* count, and "3 contexts" and "3 of 18" are different claims —
    /// the second is the one that tells an operator their query is hiding
    /// something.
    total: usize,
    keys: awase::KeyMode<PickerAction>,
    /// The chords the footer advertises, authored-first — see [`chord_for`].
    advertised: Advertised,
    /// The modal editor over the query, and the query itself.
    ///
    /// banken holds the text because `FuzzyPicker` cannot express editing it —
    /// seven event arms, `Type(char)` and `Backspace` and nothing else. The
    /// authoritative copy lives here; egaku 0.1.12's `query_mut()` guard is
    /// how it is written back, refiltering on drop.
    vim: crate::vim::Vim,
    query: crate::vim::QueryLine,
    /// What the watchdog's rounds have found — a marker per row.
    ///
    /// Inert by default, so a picker built without one draws no markers rather
    /// than a column of optimistic dots. See [`crate::ronda`] for what a
    /// marker does and does not claim.
    ronda: crate::ronda::Ronda,
    /// Where each row's light currently IS, as opposed to where the ladder
    /// says it belongs.
    ///
    /// The light eases toward its rung instead of jumping, which is what makes
    /// the climb legible: a row that walks red → orange → amber → green over a
    /// second reads as *progress being made*, where four instant recolours
    /// read as flicker and tell the operator nothing about direction.
    lights: std::collections::HashMap<String, f32>,
    /// The kubeconfig's `current-context`, annotated on its row.
    ///
    /// Shown, never pre-selected. Pre-selecting it would rebuild "whatever
    /// current-context happens to be" as a *default choice*, which is the
    /// habit `--context` was made required to break; annotating it is the
    /// useful half without the hazard.
    current: Option<String>,
    /// Set when the operator tried to accept a row that cannot be honoured.
    /// Cleared by the next keystroke.
    refusal: Option<String>,
    /// A message carried IN from the previous screen — a connect that failed,
    /// so the operator lands back on the list with the reason rather than on a
    /// shell prompt. Rendered exactly like a refusal, because it is one: the
    /// difference is only which screen produced it.
    notice: Option<String>,
    chosen: Option<ContextChoice>,
    quit: bool,
}

impl ContextPicker {
    /// Build the picker over every context the kubeconfig declares.
    ///
    /// # Errors
    ///
    /// [`PickerError::Kubeconfig`] when a kubeconfig file cannot be parsed,
    /// and [`PickerError::NoContexts`] when they parse and declare nothing.
    pub fn try_new(catalog: &Catalog) -> Result<Self, PickerError> {
        let contexts = enumerate_contexts().map_err(PickerError::Kubeconfig)?;
        if contexts.is_empty() {
            return Err(PickerError::NoContexts);
        }
        Ok(Self::with_contexts(catalog, contexts))
    }

    /// The stance the picker opens in.
    ///
    /// **NORMAL, by operator decision (2026-08-12).** It opened in Insert, on
    /// the argument that a chooser's primary act is typing a filter and that
    /// making someone press `i` first is a worse chooser. That argument is
    /// real, and it lost to a stronger one: the screen advertises itself as
    /// vim, so `j` and `k` must move. An operator who presses `j` expecting
    /// the next row and gets the letter `j` in their filter has been told the
    /// screen is modal and then handed a screen that is not.
    ///
    /// One constant, one call site — and the value a
    /// `(defbanken :picker-stance …)` field overrides. It stays as the
    /// PRESCRIBED default rather than being deleted: a picker built without
    /// config (every test, `--fixture`) still needs an answer, and reading
    /// the config from inside a constructor would make that answer depend on
    /// what is on disk. `pending-banken: opening-stance-config` — CLOSED.
    pub const OPENING_STANCE: unsoku::Stance = unsoku::Stance::Normal;

    /// The same picker over a supplied context list — the seam a test drives,
    /// so every behaviour below is provable without a kubeconfig on disk.
    ///
    /// Infallible, and that is worth stating rather than hedging with a
    /// speculative `Result`: the keymap is *derived* from an already-resolved
    /// `Catalog` (whose chord namespace `Catalog::resolve` conflict-checked),
    /// and the picker's filter can only ever bind a subset of it. The one
    /// thing that can go wrong on this screen — the kubeconfig — belongs to
    /// [`Self::try_new`].
    #[must_use]
    pub fn with_contexts(catalog: &Catalog, contexts: Vec<ContextChoice>) -> Self {
        let total = contexts.len();
        let items = contexts
            .into_iter()
            .map(|c| {
                // The fuzzy match runs over the label, so putting the server
                // in it means `banken` + "eks.amazonaws" narrows by *estate*,
                // not only by whatever the context happened to be named.
                let mut label = c.name.clone();
                if let Some(server) = &c.server {
                    label.push(' ');
                    label.push_str(server);
                }
                PickerItem::new(c, label)
            })
            .collect();

        let mut picker = FuzzyPicker::new(items);
        // The picker is the whole screen, so it is open from the first frame.
        // (`FuzzyPicker` starts `Closed`, where every event but `Open` is
        // inert — a closed picker would eat every keystroke silently.)
        let _opened = picker.on_event(PickerEvent::Open);

        let (keys, advertised) = keymap_from_catalog(catalog);
        Self {
            picker,
            total,
            keys,
            advertised,
            vim: crate::vim::Vim::opening_in(Self::OPENING_STANCE),
            query: crate::vim::QueryLine::default(),
            ronda: crate::ronda::Ronda::inert(),
            lights: std::collections::HashMap::new(),
            current: kubeconfig_current_context(),
            refusal: None,
            notice: None,
            chosen: None,
            quit: false,
        }
    }

    /// The context the operator committed to, if they committed to one.
    ///
    /// `None` means they left without choosing, which is not an error —
    /// `main` exits quietly rather than treating a deliberate `esc` as a
    /// failure.
    #[must_use]
    pub fn chosen(&self) -> Option<&ContextChoice> {
        self.chosen.as_ref()
    }

    /// The refusal currently displayed, if any (for a test).
    #[must_use]
    pub fn refusal(&self) -> Option<&str> {
        self.refusal.as_deref()
    }

    /// Re-open the picker carrying a message from the screen that just closed.
    ///
    /// The failed-connect path: an operator who picks a cluster the VPN cannot
    /// reach used to be dropped at a shell prompt with an error, having lost
    /// the list they were choosing from. Now they land back on it, with the
    /// reason on screen and the other seventeen contexts still there.
    #[must_use]
    pub fn with_notice(mut self, notice: impl Into<String>) -> Self {
        self.notice = Some(notice.into());
        self
    }

    /// Show what the watchdog's rounds have found beside each row.
    ///
    /// The rounds are owned by the CALLER and outlive this picker, which is
    /// what lets a failed connect re-enter the list without restarting them —
    /// eighteen contexts would otherwise be re-probed from scratch on every
    /// retry, and the markers would blink back to `probing` at exactly the
    /// moment the operator is looking for the one that is up.
    /// Open in the stance the operator authored.
    ///
    /// Builder-style, like every other optional input here: a caller that has
    /// read a `(defbanken …)` passes it, and one that has not keeps
    /// [`Self::OPENING_STANCE`].
    #[must_use]
    pub fn opening_in(mut self, stance: unsoku::Stance) -> Self {
        self.vim = crate::vim::Vim::opening_in(stance);
        self
    }

    #[must_use]
    pub fn with_ronda(mut self, ronda: crate::ronda::Ronda) -> Self {
        self.ronda = ronda;
        self
    }

    /// Ease every light one step toward the rung its context has reached.
    ///
    /// Exponential, not linear: a light moves fastest when it is furthest from
    /// where it belongs, so a big jump (nothing → full access) reads as a
    /// decisive climb and a small correction settles without a visible crawl.
    ///
    /// Runs in `on_wake`, which is the `&mut self` half of the wakeup and
    /// therefore never dropped part-way — a light cannot be left stranded
    /// between two colours by a keystroke arriving mid-ease.
    fn ease_lights(&mut self) {
        /// How much of the remaining distance to close per frame.
        const RATE: f32 = 0.22;
        /// Below this, snap. Chasing a limit forever costs a repaint per frame
        /// to move by a fraction of a colour step nobody can see.
        const SNAP: f32 = 0.004;

        for (name, target) in self.ronda.positions() {
            let light = self.lights.entry(name).or_insert(target);
            let delta = target - *light;
            if delta.abs() <= SNAP {
                *light = target;
            } else {
                *light += delta * RATE;
            }
        }
    }

    /// Whether any light is still moving — the other half of the repaint
    /// cadence, so the picker keeps painting through a transition even after
    /// the round itself has settled.
    fn lights_settled(&self) -> bool {
        self.ronda
            .positions()
            .into_iter()
            .all(|(name, target)| {
                self.lights
                    .get(&name)
                    .is_some_and(|l| (l - target).abs() <= 0.004)
            })
    }

    /// How long to wait before repainting, or `None` for "only on a key".
    ///
    /// Fast while answers are still landing, slow once the round has settled,
    /// and **never** when there is no round — a picker that repaints five
    /// times a second forever because it *might* learn something is a busy
    /// loop with a good excuse.
    fn repaint_after(&self) -> Option<std::time::Duration> {
        if self.ronda.covered() == 0 {
            return None;
        }
        // A moving light keeps the fast cadence even after the round settled
        // — otherwise the ease would be drawn at 1 fps and the "smooth" change
        // would be five visible steps, which is exactly what easing is for.
        Some(if self.ronda.all_settled() && self.lights_settled() {
            std::time::Duration::from_millis(1000)
        } else {
            std::time::Duration::from_millis(60)
        })
    }

    /// The typed keymap — exposed inherently as well as through [`AsyncApp`]
    /// so a test can ask "which action does this chord bind" without owning a
    /// terminal (the same reason [`crate::app::BankenApp`] does it).
    #[must_use]
    pub fn keymap(&self) -> &awase::KeyMode<PickerAction> {
        &self.keys
    }

    /// Apply one action. Returns `true` when it changed anything — the shape
    /// [`crate::app::BankenApp::dispatch_action`] uses, so a test can assert
    /// an inert key really was inert.
    pub fn dispatch(&mut self, action: PickerAction) -> bool {
        // Any keystroke clears a stale refusal: it described the previous
        // attempt, and leaving it up would make it look like it described
        // this one. The carried-in notice goes the same way and for the same
        // reason — it described the screen before this one.
        self.refusal = None;
        self.notice = None;

        // Everything that touches the QUERY or the STANCE goes through the
        // modal layer; only the arrows and `enter` reach the picker directly
        // (an arrow is not a vim key and has one meaning in both stances).
        //
        // Backspace and escape are stance-dependent — in Insert backspace
        // deletes, in Normal it moves left; escape cancels a pending operator
        // and otherwise rests. The four erase chords are NOT stance-dependent
        // and still route here, because [`crate::vim::Vim::apply`] is the one
        // path text may leave the query by.
        //
        // ONE total match, every arm returning. It used to be two matches with
        // the first falling through, which left the second carrying arms no
        // keystroke could reach — and one of them (`Cancel => Backspace`) was
        // already wrong. A total match means a new `PickerAction` cannot be
        // added without this screen stating what it does.
        match action {
            PickerAction::Erase => self.stroke(crate::vim::Stroke::Backspace),
            PickerAction::EraseWord => self.stroke(crate::vim::Stroke::EraseWordBack),
            PickerAction::EraseToStart => self.stroke(crate::vim::Stroke::EraseToStart),
            PickerAction::EraseToEnd => self.stroke(crate::vim::Stroke::EraseToEnd),
            PickerAction::EraseForward => self.stroke(crate::vim::Stroke::EraseForward),
            PickerAction::Escape => self.stroke(crate::vim::Stroke::Escape),
            // Straight to the event, never through the modal layer. That is
            // the whole point of it being a separate action: an escape hatch
            // routed through a stance is an escape hatch a stance can eat.
            PickerAction::Abort => self.emit(PickerEvent::Cancel),
            PickerAction::Up => self.emit(PickerEvent::NavUp),
            PickerAction::Down => self.emit(PickerEvent::NavDown),
            PickerAction::Accept => {
                // The gate. An ambiguous name does not identify one cluster,
                // so accepting it would be an affordance banken cannot honour
                // — `resolve_context` refuses it moments later anyway. Better
                // to say so on the row the operator is looking at.
                match self.picker.selected_item() {
                    Some(item) if item.key.is_ambiguous() => {
                        self.refusal = Some(ambiguity_refusal(&item.key));
                        true
                    }
                    // Nothing matches the query: `enter` is inert, not an
                    // error. Typing a filter that matches nothing is a normal
                    // step on the way to one that does.
                    None => false,
                    Some(_) => self.emit(PickerEvent::Accept),
                }
            }
        }
    }

    /// Feed one keystroke to the modal editor.
    ///
    /// Every printable arrives here, because the picker's keymap binds only
    /// chords that cannot be typed. In Insert the character lands in the
    /// query; in Normal it is a command, and an unbound one is INERT rather
    /// than typed — which is the property the whole layer trades for.
    pub fn stroke(&mut self, s: crate::vim::Stroke) -> bool {
        self.refusal = None;
        self.notice = None;
        let effect = self.vim.stroke(s, &mut self.query);
        self.apply(effect)
    }

    /// Type one character — the [`Stroke::Char`] path, kept as a name because
    /// `AsyncApp::on_text` hands over a `char`.
    pub fn type_char(&mut self, c: char) {
        let _changed = self.stroke(crate::vim::Stroke::Char(c));
    }

    /// Route one [`crate::vim::Effect`] onto the picker.
    ///
    /// This is where the picker's TWO cursors are kept apart: a text effect
    /// writes the query back through egaku's guard, a row effect drives the
    /// selection, and neither can be mistaken for the other.
    fn apply(&mut self, effect: crate::vim::Effect) -> bool {
        use crate::vim::Effect;
        match effect {
            Effect::Inert => false,
            Effect::Moved => true,
            Effect::Edited => {
                // egaku 0.1.12: the guard refilters on drop, so the view can
                // never fall out of step with the text banken just wrote.
                let mut q = self.picker.query_mut();
                q.select_all();
                q.delete_selection();
                for c in self.query.text().chars() {
                    q.insert_char(c);
                }
                true
            }
            Effect::Rows(n) => {
                for _ in 0..n.unsigned_abs() {
                    let _ = self.picker.on_event(if n < 0 {
                        PickerEvent::NavUp
                    } else {
                        PickerEvent::NavDown
                    });
                }
                true
            }
            Effect::RowEdge { last } => {
                // `PickerEvent` has no absolute-row arm, so `gg`/`G` walk —
                // but they walk an EXACT distance read from the view, never a
                // full lap. `move_up`/`move_down` wrap, so a lap would return
                // to where it started and `gg` would silently do nothing.
                let at = self.picker.view().selected;
                let n = self.picker.visible_count();
                let (event, steps) = if last {
                    (PickerEvent::NavDown, n.saturating_sub(1).saturating_sub(at))
                } else {
                    (PickerEvent::NavUp, at)
                };
                for _ in 0..steps {
                    let _ = self.picker.on_event(event.clone());
                }
                true
            }
            // Straight to the event, NEVER back through `dispatch` — that
            // path routes Cancel into `stroke(Escape)`, which produced
            // `Effect::Cancel` again and overflowed the stack on the first
            // `esc`. Caught by escape_leaves_without_choosing.
            Effect::Accept => self.emit(PickerEvent::Accept),
            Effect::Cancel => self.emit(PickerEvent::Cancel),
        }
    }

    /// Feed one event straight to the picker, collecting its effects.
    ///
    /// The single non-re-entrant path into `FuzzyPicker`, so a keystroke can
    /// never route back into the layer that produced it.
    fn emit(&mut self, event: PickerEvent) -> bool {
        let effects = self.picker.on_event(event);
        let changed = !effects.is_empty();
        for effect in effects {
            match effect {
                PickerEffect::Accepted { key } => {
                    self.chosen = Some(key);
                    self.quit = true;
                }
                PickerEffect::Cancelled => self.quit = true,
                PickerEffect::Opened
                | PickerEffect::Filtered { .. }
                | PickerEffect::Moved { .. } => {}
            }
        }
        changed
    }

    /// The stance badge and any pending sequence, for the status line.
    #[must_use]
    pub fn stance_badge(&self) -> String {
        let mut s = String::from(self.vim.stance().label());
        if let Some(p) = self.vim.pending_label() {
            s.push_str("  ");
            s.push_str(&p);
        }
        s
    }

    /// Paint the screen. Public so a `TestBackend` can assert the frame
    /// without a terminal.
    pub fn render(&self, buf: &mut Buffer) {
        let width = buf.width();
        let height = buf.height();
        if width == 0 || height == 0 {
            return;
        }
        let view = self.picker.view();

        // ── Title ──
        buf.set_stringn(
            0,
            0,
            "banken 番犬  —  choose a cluster",
            width,
            Style::default().fg(Color::Cyan).bold(),
        );
        let count = matched_label(view.rows.len(), self.total);
        let count_w = u16::try_from(count.chars().count()).unwrap_or(0);
        if count_w < width {
            buf.set_stringn(
                width - count_w,
                0,
                &count,
                count_w,
                Style::default().fg(Color::DarkGrey),
            );
        }

        // ── Query line: a prompt the operator can see they are typing into ──
        if height > 1 {
            buf.set_stringn(0, 1, "› ", width, Style::default().fg(Color::Cyan));
            buf.set_stringn(2, 1, view.query, width.saturating_sub(3), Style::default());
            // A block caret, so an empty query still reads as "type here"
            // rather than as a dead line.
            //
            // Drawn from the REAL caret, converted to a character column.
            // This used to be `view.query.chars().count()` — the query
            // LENGTH — which was invisibly correct only because an
            // append-only caret is always at the end. The moment `h` works it
            // is wrong on every keystroke, and wrong by a whole character on
            // every multi-byte name.
            let caret = 2 + u16::try_from(self.query.caret_col()).unwrap_or(0);
            if caret < width {
                buf.set_char(caret, 1, '▏', Style::default().fg(Color::Cyan));
            }
        }
        if height > 2 {
            buf.hline(0, 2, width, '─', Style::default().fg(Color::DarkGrey));
        }

        // ── Rows ──
        //
        // The standing line costs the list one row, and earns it: a marker
        // says how far the climb got, and only prose can say *why it stopped
        // there*. "port open, no apiserver reached — credentials: SSO session
        // expired" is a fix; an orange dot is a mood.
        let standing_line = if self.ronda.covered() > 0 { 1 } else { 0 };
        let first_row = 3;
        let footer = height.saturating_sub(1 + standing_line);
        let visible = usize::from(footer.saturating_sub(first_row));
        if visible > 0 {
            // Bottom-anchored viewport, the same pure function of
            // `(selected, height)` the table drawer uses — no stored offset to
            // fall out of step with the selection.
            let first_visible = view.selected.saturating_sub(visible.saturating_sub(1));
            for (offset, item) in view
                .rows
                .iter()
                .skip(first_visible)
                .take(visible)
                .enumerate()
            {
                let Ok(row_i) = u16::try_from(offset) else {
                    break;
                };
                self.draw_row(
                    buf,
                    first_row + row_i,
                    width,
                    &item.key,
                    first_visible + offset == view.selected,
                );
            }
        }

        // ── Stance badge ──
        //
        // Load-bearing, not decoration. In Insert an unrecognised key costs "a
        // character you can delete"; in Normal it costs `q` leaving the screen
        // or `dw` eating a word. The badge is what re-buys the property the
        // modal layer trades away, so it is drawn where the eye already is —
        // on the query line — and in a colour that differs per stance, because
        // a badge read as a word is read too late.
        if height > 1 {
            let badge = self.stance_badge();
            let w = u16::try_from(badge.chars().count()).unwrap_or(0);
            let style = match self.vim.stance() {
                unsoku::Stance::Insert => Style::default().fg(Color::Black).bg(Color::Green),
                unsoku::Stance::Normal => Style::default().fg(Color::Black).bg(Color::Cyan),
            };
            if w + 1 < width {
                buf.set_stringn(width - w - 1, 1, &badge, w + 1, style);
            }
        }

        // ── The selected row's standing ──
        if standing_line == 1 && height > 2 {
            self.draw_standing(buf, width, height.saturating_sub(2));
        }

        // ── Footer ──
        self.draw_footer(buf, width, height);
    }

    /// What the highlighted context's climb reached, and why it stopped.
    fn draw_standing(&self, buf: &mut Buffer, width: u16, y: u16) {
        let Some(item) = self.picker.selected_item() else {
            return;
        };
        let standing = self.ronda.standing(&item.key.name);
        let style = Style::default().fg(Color::DarkGrey);

        let mut x = buf.set_stringn(0, y, "  ", width, style);
        x = buf.set_stringn(
            x,
            y,
            standing.rung.marker(),
            width.saturating_sub(x),
            match standing.rung.position() {
                Some(p) => {
                    let (r, g, b) = crate::ronda::ramp(p);
                    Style::default().fg(Color::Rgb { r, g, b })
                }
                None => style,
            },
        );
        x = buf.set_stringn(x, y, " ", width.saturating_sub(x), style);
        x = buf.set_stringn(x, y, standing.rung.label(), width.saturating_sub(x), style);
        if !standing.note.is_empty() {
            x = buf.set_stringn(x, y, " — ", width.saturating_sub(x), style);
            buf.set_stringn(x, y, &standing.note, width.saturating_sub(x), style);
        }
    }

    fn draw_row(&self, buf: &mut Buffer, y: u16, width: u16, ctx: &ContextChoice, selected: bool) {
        let base = if selected {
            let sel = Style::default().fg(Color::Black).bg(Color::Cyan);
            buf.blank(0, y, width, sel);
            sel
        } else {
            Style::default()
        };

        let marker = if selected { "▶ " } else { "  " };
        let mut x = buf.set_stringn(0, y, marker, width, base);

        // ── What the rounds found, before the name ──
        //
        // First on the row because it is the field that decides whether the
        // rest of the row is worth reading. Colour AND glyph carry it: the
        // glyph is what survives a colour-blind operator and a `TestBackend`
        // frame, so neither is load-bearing alone.
        if self.ronda.covered() > 0 {
            let rung = self.ronda.rung(&ctx.name);
            let style = if selected {
                // The selected row is already inverted; a ramp colour on top of
                // it fights the highlight and reads as a rendering fault.
                base
            } else {
                // The light is drawn from the EASED position, not from the
                // rung — that is the whole difference between a colour that
                // slides up the ramp and five presets that blink.
                match self.lights.get(&ctx.name).copied() {
                    Some(p) => {
                        let (r, g, b) = crate::ronda::ramp(p);
                        base.fg(Color::Rgb { r, g, b })
                    }
                    // Not probed: absence of a measurement, never the bottom
                    // of the ramp. Deep red here would say "this cluster is
                    // down" about a cluster banken has not looked at yet.
                    None => base.fg(Color::DarkGrey),
                }
            };
            x = buf.set_stringn(x, y, rung.marker(), width.saturating_sub(x), style);
            x = buf.set_stringn(x, y, " ", width.saturating_sub(x), base);
        }

        // The name. Red when ambiguous — the row is not choosable, and the
        // colour is the first thing that says so.
        let name_style = if ctx.is_ambiguous() && !selected {
            base.fg(Color::Red)
        } else {
            base
        };
        x = buf.set_stringn(x, y, &ctx.name, width.saturating_sub(x), name_style);

        // The apiserver — dim, because it is the *check*, not the choice.
        // This is the field a name cannot give you, so it is never elided
        // while anything else is still on the line.
        let detail_style = if selected {
            base
        } else {
            base.fg(Color::DarkGrey)
        };
        let pad = name_column_pad(&ctx.name);
        x = buf.set_stringn(x, y, &pad, width.saturating_sub(x), base);
        let server = ctx.server.as_deref().unwrap_or("<no server declared>");
        x = buf.set_stringn(x, y, server, width.saturating_sub(x), detail_style);

        // Annotations, in descending order of how much the operator needs
        // them before pressing enter.
        if ctx.is_ambiguous() {
            let note = ambiguity_note(ctx);
            let style = if selected { base } else { base.fg(Color::Red) };
            x = buf.set_stringn(x, y, &note, width.saturating_sub(x), style);
        }
        if self.current.as_deref() == Some(ctx.name.as_str()) {
            // Short because it is last in the row and therefore first to be
            // clipped: measured at 120 columns beside a full EKS apiserver
            // URL, `(current-context)` cut to `(current-con`. It is the least
            // important field on the line, so it gets the smallest budget
            // rather than a bigger one.
            buf.set_stringn(x, y, "  (current)", width.saturating_sub(x), detail_style);
        }
    }

    fn draw_footer(&self, buf: &mut Buffer, width: u16, height: u16) {
        // A refusal owns the footer while it is up: it is the answer to the
        // key just pressed, and a hint line beside it would read as though the
        // keystroke had worked.
        let Some(refusal) = self.refusal.as_ref().or(self.notice.as_ref()) else {
            let style = Style::default().fg(Color::Black).bg(Color::DarkGrey);
            let y = height - 1;
            buf.blank(0, y, width, style);
            buf.set_stringn(0, y, &self.hint(), width, style);
            return;
        };

        // **Wrapped, not clipped.** A refusal that names two kubeconfig paths
        // does not fit on one 120-column line, and clipping it drops the
        // second path — i.e. exactly the half that makes it actionable.
        // Measured: the message stopped mid-path at the screen edge.
        let style = Style::default().fg(Color::White).bg(Color::Red);
        let lines = egaku_term::draw::wrap_text(refusal, width.saturating_sub(1));
        let shown = u16::try_from(lines.len()).unwrap_or(u16::MAX).min(height);
        let top = height - shown;
        for (i, line) in lines.iter().take(usize::from(shown)).enumerate() {
            let y = top + u16::try_from(i).unwrap_or(0);
            buf.blank(0, y, width, style);
            buf.set_stringn(1, y, line, width.saturating_sub(1), style);
        }
    }

    /// The footer hint, with the chords read from the authored vocabulary
    /// rather than hand-typed — a legend that can advertise a chord the
    /// runtime does not bind is the defect
    /// [`crate::app::key_legend_parts`] exists to prevent, and this screen is
    /// no different.
    /// **Per stance**, because the same key does different things in each and
    /// a fixed hint would be wrong in one of them. It advertised
    /// `type: filter` in NORMAL, where typing does not filter — the footer is
    /// the surface an operator reads to learn the screen, so a hint that is
    /// wrong half the time is worse than no hint.
    fn hint(&self) -> String {
        let mut s = String::new();
        s.push_str(&chord_for(&self.advertised, PickerAction::Up));
        s.push('/');
        s.push_str(&chord_for(&self.advertised, PickerAction::Down));
        s.push_str(": move  ·  ");
        match self.vim.stance() {
            unsoku::Stance::Insert => {
                s.push_str("type: filter  ·  ");
                // Derived, never spelled: these are exactly the chords an
                // operator reported missing, so the footer that would have
                // taught them is the footer that has to be right.
                for (i, action) in [
                    PickerAction::EraseWord,
                    PickerAction::EraseToStart,
                    PickerAction::EraseToEnd,
                ]
                .into_iter()
                .enumerate()
                {
                    if i > 0 {
                        s.push('/');
                    }
                    s.push_str(&chord_for(&self.advertised, action));
                }
                s.push_str(": erase  ·  ");
                s.push_str(&chord_for(&self.advertised, PickerAction::Escape));
                s.push_str(": NORMAL  ·  ");
                s.push_str(&chord_for(&self.advertised, PickerAction::Accept));
                s.push_str(": open");
            }
            unsoku::Stance::Normal => {
                s.push_str("hjkl0$wbe: move  ·  d/c/y + x D C: edit  ·  i/a: INSERT  ·  ");
                s.push_str(&chord_for(&self.advertised, PickerAction::Accept));
                // `q`, NOT escape. The footer said `escape: quit` while escape
                // quit, and both halves changed together — an operator whose
                // only advertised way out no longer worked would be stranded
                // on the landing screen with nothing on screen to tell them
                // `q`. See `the_hint_names_the_key_that_actually_leaves`.
                s.push_str(": open  ·  q: quit");
            }
        }
        s
    }
}

/// `"12 of 18"` — or just the total when nothing is filtered out.
fn matched_label(shown: usize, total: usize) -> String {
    let mut s = String::new();
    if shown == total {
        s.push_str(&total.to_string());
        s.push_str(" contexts ");
    } else {
        s.push_str(&shown.to_string());
        s.push_str(" of ");
        s.push_str(&total.to_string());
        s.push(' ');
    }
    s
}

/// Pad a context name out to a column so the apiserver URLs line up. Names
/// longer than the column get a single space rather than a truncated URL —
/// the URL is the checkable half and must not be the thing that is cut.
fn name_column_pad(name: &str) -> String {
    const NAME_COL: usize = 32;
    let used = name.chars().count();
    " ".repeat(NAME_COL.saturating_sub(used).max(1))
}

/// The inline row marker for an ambiguous context.
fn ambiguity_note(ctx: &ContextChoice) -> String {
    let mut s = String::from("  ⚠ declared by ");
    s.push_str(&ctx.files.len().to_string());
    s.push_str(" kubeconfig files");
    s
}

/// The footer message when an ambiguous row is accepted. Names the files, so
/// the fix is visible from the screen the operator is already on.
fn ambiguity_refusal(ctx: &ContextChoice) -> String {
    let mut s = String::from("`");
    s.push_str(&ctx.name);
    s.push_str("` does not identify one cluster — declared by ");
    for (i, f) in ctx.files.iter().enumerate() {
        if i > 0 {
            s.push_str(" and ");
        }
        s.push_str(&abbreviate_home(f));
    }
    s.push_str(". Rename one, or narrow KUBECONFIG to the file you mean.");
    s
}

/// A path with `$HOME` collapsed to `~`.
///
/// Every kubeconfig path an operator sees elsewhere is written `~/.kube/…`, and
/// on a real machine the expanded form is long enough to push the *second*
/// declaring file off a wrapped refusal. Shortening the part they already know
/// keeps the part they need.
fn abbreviate_home(path: &std::path::Path) -> String {
    let Some(home) = std::env::var_os("HOME") else {
        return path.display().to_string();
    };
    let home = std::path::PathBuf::from(home);
    match path.strip_prefix(&home) {
        Ok(rest) => {
            let mut s = String::from("~/");
            s.push_str(&rest.display().to_string());
            s
        }
        Err(_not_under_home) => path.display().to_string(),
    }
}

/// The chords the footer offers, in the order they were bound: every authored
/// one first, then the structural fallbacks.
///
/// A `Vec` and not a lookup into [`awase::KeyMode`], because that one is a
/// `HashMap` — see [`chord_for`].
type Advertised = Vec<(PickerAction, String)>;

/// The chord to advertise for `action`. `"?"` when nothing binds it, which is
/// honest rather than a guess — and is what a printable authored chord would
/// produce if one ever reached here.
///
/// **Several chords legitimately share an action** — `escape` and `ctrl+c`
/// both cancel — and `KeyMode::bindings` is a `HashMap`, so reading the footer
/// out of it made the same build advertise a different chord run to run.
/// Measured on the first run of `the_footer_advertises_the_bound_chords`:
/// `ctrl+c: quit`.
///
/// The order here is the fix, and it is a *meaning* rather than a tie-break.
/// (A length tie-break was tried first and is why this comment exists:
/// `escape` and `ctrl+c` are both six characters, so it silently advertised
/// the escape hatch.) An authored chord is what the operator's own vocabulary
/// says the key is; `ctrl+c` is a last resort that exists whether or not
/// anything authored it. Advertising the second over the first would teach the
/// wrong key. Every bound chord still works either way — this decides only
/// what the footer *says*.
fn chord_for(advertised: &Advertised, action: PickerAction) -> String {
    advertised
        .iter()
        .find(|(a, _)| *a == action)
        .map_or_else(|| "?".to_owned(), |(_, chord)| chord.clone())
}

/// Build the picker keymap from the authored `(defnavkey)` vocabulary.
///
/// Two rules, and both are derivations rather than lists:
///
/// 1. **Only a chord that cannot be typed may bind.** A picker's keyboard
///    belongs to its query — see the module docs. [`is_control_chord`] is the
///    predicate, and it is deliberately conservative in the direction of
///    *not* binding: an unrecognised key falls to the query, where the worst
///    case is a character the operator can delete, rather than to a silent
///    command they did not mean to issue.
/// 2. **Every intent is projected exhaustively**, so adding a `NavIntent`
///    upstream is a compile error here until this screen decides what it
///    means.
///
/// `ctrl+c` is bound structurally on top: it is not authorable as a
/// `(defnavkey)` (it is not a navigation concept) and an operator's
/// last-resort escape must not depend on the vocabulary being loaded
/// correctly.
fn keymap_from_catalog(catalog: &Catalog) -> (awase::KeyMode<PickerAction>, Advertised) {
    let mut km: awase::KeyMode<PickerAction> = awase::KeyMode::typed("picker", false);
    let mut advertised: Advertised = Vec::new();

    for nav in catalog.nav_keys() {
        let hotkey = nav.keys.hotkey();
        if !is_control_chord(hotkey) {
            continue;
        }
        let Some(action) = picker_action(nav.intent) else {
            continue;
        };
        advertised.push((action, hotkey.display()));
        // A displaced binding is dropped rather than raised: two authored nav
        // keys legitimately share one intent (`down` and `j`), and the filter
        // above can leave a pair on one action. What must never happen is two
        // *different* actions on one chord, and that is already impossible —
        // `Catalog::resolve` conflict-checks the whole chord namespace before
        // this function ever sees it.
        let _displaced = km.add_binding(awase::Binding::new(hotkey, action));
    }

    // The structural chords, appended AFTER the authored ones so the footer
    // prefers what the vocabulary declared (see `chord_for`).
    //
    // **The five erase chords are text editing, not navigation**, which is why
    // none of them is a `(defnavkey)` — see the module docs. `NavIntent`
    // describes moving around a resource view, and giving it a text-editing
    // intent would make it mean two things.
    //
    // All five are chords rather than characters because they cannot be
    // anything else: `egaku_term::app::text_char` refuses every CTRL-modified
    // key, and `delete` is not a `KeyCode::Char`. Verified deliverable in
    // egaku-term 0.3's `to_hotkey` (`src/event.rs:139` maps `Char(c)` through
    // `Key::from_name`, `:147` maps `KeyCode::Delete`), which is what makes
    // binding them here sufficient — there is no upstream gap behind this.
    //
    // Ctrl-C is the universal terminal escape hatch, and an operator's last
    // resort must not depend on the vocabulary having loaded the way they
    // expected — nor on which stance they happen to be in.
    for (hotkey, action) in [
        (
            awase::Hotkey::new(Modifiers::NONE, Key::Backspace),
            PickerAction::Erase,
        ),
        (
            awase::Hotkey::new(Modifiers::CTRL, Key::W),
            PickerAction::EraseWord,
        ),
        (
            awase::Hotkey::new(Modifiers::CTRL, Key::U),
            PickerAction::EraseToStart,
        ),
        (
            awase::Hotkey::new(Modifiers::CTRL, Key::K),
            PickerAction::EraseToEnd,
        ),
        (
            awase::Hotkey::new(Modifiers::NONE, Key::Delete),
            PickerAction::EraseForward,
        ),
        (
            awase::Hotkey::new(Modifiers::CTRL, Key::C),
            PickerAction::Abort,
        ),
    ] {
        advertised.push((action, hotkey.display()));
        let _prev = km.add_binding(awase::Binding::new(hotkey, action));
    }

    (km, advertised)
}

/// What a navigation intent means on the picker screen.
///
/// A total match, so a new `NavIntent` cannot be added upstream without this
/// screen stating its answer. `None` is a deliberate answer for the two
/// intents a chooser has no analogue for, not an omission:
///
/// - `ToggleSort` — the picker's order is the fuzzy ranking, which is a
///   function of the query. A sort toggle would have to fight it.
/// - `Help` — the picker's entire vocabulary is four chords and the footer
///   names all four. A help overlay here would document less than the screen
///   already shows, and its chord (`h`) is a letter an operator needs for
///   typing `helm-controller` into the filter.
/// - `Dismiss` is `escape`, which on this screen means "step back a stance",
///   never "leave" — see [`PickerAction::Escape`].
/// - `Quit` is `None` here, and that is a statement rather than an omission.
///   Its authored chord is `q`, a typable character, so it is filtered out of
///   this keymap by rule 1 and reaches [`crate::vim`] as a NORMAL-stance
///   command instead. Mapping it to a `PickerAction` would be the drift:
///   `escape` would claim the quit intent while no longer quitting.
///   `the_quit_char_is_the_authored_quit_chord` is the gate on the handoff.
fn picker_action(intent: NavIntent) -> Option<PickerAction> {
    match intent {
        NavIntent::SelectNext => Some(PickerAction::Down),
        NavIntent::SelectPrev => Some(PickerAction::Up),
        NavIntent::Confirm => Some(PickerAction::Accept),
        NavIntent::Dismiss => Some(PickerAction::Escape),
        NavIntent::Quit | NavIntent::ToggleSort | NavIntent::Help => None,
    }
}

/// Whether a chord is one the terminal delivers as a *command* rather than as
/// text.
///
/// Mirrors `egaku_term`'s own `text_char` rule, which is the thing that
/// actually decides: a key event becomes text when it is a `KeyCode::Char`
/// with no CTRL/ALT/SUPER modifier. So anything modified by one of those is a
/// chord, and among the unmodified keys only the named control/navigation set
/// is. Everything else — letters, digits, punctuation — is text and must stay
/// text.
fn is_control_chord(hotkey: awase::Hotkey) -> bool {
    if hotkey.modifiers.contains(Modifiers::CTRL)
        || hotkey.modifiers.contains(Modifiers::ALT)
        || hotkey.modifiers.contains(Modifiers::CMD)
    {
        return true;
    }
    matches!(
        hotkey.key,
        Key::Return
            | Key::Escape
            | Key::Tab
            | Key::Backspace
            | Key::Delete
            | Key::Up
            | Key::Down
            | Key::Left
            | Key::Right
            | Key::Home
            | Key::End
            | Key::PageUp
            | Key::PageDown
    )
}

impl AsyncApp for ContextPicker {
    type Action = PickerAction;

    /// Vestigial — dispatch goes through [`Self::hotkey_map`]. Same shape as
    /// [`crate::app::BankenApp`].
    fn keymap(&self) -> &KeyMap<PickerAction> {
        static EMPTY: std::sync::OnceLock<KeyMap<PickerAction>> = std::sync::OnceLock::new();
        EMPTY.get_or_init(KeyMap::new)
    }

    fn hotkey_map(&self) -> Option<&awase::KeyMode<PickerAction>> {
        Some(&self.keys)
    }

    async fn handle(&mut self, action: &PickerAction) -> TermResult<()> {
        let _changed = self.dispatch(*action);
        Ok(())
    }

    /// Every printable no binding claimed is query text. This is the half that
    /// makes the screen a *filter* rather than a menu, and it is why the
    /// keymap above refuses printable chords: whatever binds cannot be typed.
    async fn on_text(&mut self, c: char) -> TermResult<()> {
        self.type_char(c);
        Ok(())
    }

    /// Repaint when the rounds may have learned something.
    ///
    /// A plain sleep, which is the cancellation-safe shape `wake` requires:
    /// the findings are read straight off the `ArcSwap` in [`Self::render`],
    /// so there is no state to drop half-applied and `on_wake` stays the
    /// default no-op.
    ///
    /// `pending()` when there is no round at all, which is what keeps a
    /// ronda-less picker exactly as input-only as it was before.
    async fn wake(&self) {
        match self.repaint_after() {
            Some(d) => tokio::time::sleep(d).await,
            None => std::future::pending().await,
        }
    }

    /// Advance the lights one step. The `&mut self` half of the wakeup, so an
    /// ease can never be left half-applied by a keystroke winning the select.
    async fn on_wake(&mut self) -> TermResult<()> {
        self.ease_lights();
        Ok(())
    }

    async fn draw(&self, frame: &mut Buffer) -> TermResult<()> {
        self.render(frame);
        Ok(())
    }

    fn should_quit(&self) -> bool {
        self.quit
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egaku_term::TestBackend;
    use std::path::PathBuf;

    fn ctx(name: &str, server: &str) -> ContextChoice {
        ContextChoice {
            name: name.into(),
            server: Some(server.into()),
            files: vec![PathBuf::from("/home/op/.kube/config")],
        }
    }

    fn ambiguous(name: &str) -> ContextChoice {
        ContextChoice {
            name: name.into(),
            server: Some("https://127.0.0.1:6443".into()),
            files: vec![
                PathBuf::from("/home/op/.kube/configs/local"),
                PathBuf::from("/home/op/.kube/config"),
            ],
        }
    }

    fn picker(contexts: Vec<ContextChoice>) -> ContextPicker {
        let catalog = banken_spec::load_catalog().expect("the shipped vocabulary must load");
        ContextPicker::with_contexts(&catalog, contexts)
    }

    fn three() -> ContextPicker {
        picker(vec![
            ctx("alpha-eks", "https://alpha.eks.amazonaws.com"),
            ctx("bravo", "https://bravo.internal:6443"),
            ctx("jaeger-dev", "https://jaeger.dev:6443"),
        ])
    }

    fn act(km: &awase::KeyMode<PickerAction>, hk: awase::Hotkey) -> Option<PickerAction> {
        km.find_binding(&hk, &awase::MatchContext::default())
            .map(|b| b.action)
    }

    /// Type a filter the way an operator now has to: `i` first.
    ///
    /// The screen opens in NORMAL, so a bare `type_char('j')` moves a row.
    /// Every test that means "the operator filtered for X" goes through here,
    /// which is also what keeps them honest about the extra keystroke the
    /// stance flip costs.
    fn filter(p: &mut ContextPicker, query: &str) {
        p.type_char('i');
        assert_eq!(p.stance_badge(), "INSERT", "`i` must start a filter");
        for c in query.chars() {
            p.type_char(c);
        }
    }

    /// **THE GATE that makes the screen a filter.** The authored vocabulary
    /// binds `j`/`k`/`o`/`q` in the table. If any of them bound here, an
    /// operator filtering for `jaeger` would move the cursor on the `j` and
    /// quit on the `q` — the query would be unusable for most real cluster
    /// names.
    #[test]
    fn a_typable_authored_chord_is_never_bound() {
        let p = three();
        for key in [Key::J, Key::K, Key::O, Key::Q] {
            assert_eq!(
                act(p.keymap(), awase::Hotkey::new(Modifiers::NONE, key)),
                None,
                "{key:?} must stay available as query text",
            );
        }
    }

    /// The other half: the authored chords that *cannot* be typed do bind, so
    /// this screen's keys are a derivation of the one vocabulary and not a
    /// second list.
    #[test]
    fn the_authored_control_chords_bind() {
        let p = three();
        for (key, expected) in [
            (Key::Down, PickerAction::Down),
            (Key::Up, PickerAction::Up),
            (Key::Return, PickerAction::Accept),
            (Key::Escape, PickerAction::Escape),
            (Key::Backspace, PickerAction::Erase),
        ] {
            assert_eq!(
                act(p.keymap(), awase::Hotkey::new(Modifiers::NONE, key)),
                Some(expected),
                "{key:?}",
            );
        }
        assert_eq!(
            act(p.keymap(), awase::Hotkey::new(Modifiers::CTRL, Key::C)),
            Some(PickerAction::Abort),
        );
    }

    /// **THE MEASURED GAP, 2026-08-12.** All four were inert on the live
    /// 0.1.15 binary: only `backspace` deleted anything, so the first screen's
    /// entire editing vocabulary was one character at a time. egaku-term
    /// delivers every one of them (`event.rs:139`/`:147`) — they were simply
    /// never bound.
    #[test]
    fn the_erase_chords_bind() {
        let p = three();
        for (mods, key, expected) in [
            (Modifiers::CTRL, Key::W, PickerAction::EraseWord),
            (Modifiers::CTRL, Key::U, PickerAction::EraseToStart),
            (Modifiers::CTRL, Key::K, PickerAction::EraseToEnd),
            (Modifiers::NONE, Key::Delete, PickerAction::EraseForward),
        ] {
            assert_eq!(
                act(p.keymap(), awase::Hotkey::new(mods, key)),
                Some(expected),
                "{mods:?}+{key:?} must erase",
            );
        }
    }

    /// And they must actually erase — a binding that resolves to an action the
    /// query never feels is the same defect one level in.
    ///
    /// Driven in INSERT, which is the stance the operator reported them
    /// missing from, and where `apply`'s caret clamp had to stop being
    /// hardcoded to Normal.
    #[test]
    fn the_erase_chords_edit_the_query_in_insert() {
        for (chord, expect) in [
            (PickerAction::EraseWord, "alpha-"),
            (PickerAction::EraseToStart, ""),
            // The caret is at the end after typing, so the two forward
            // deletions have nothing to take — inert, and the query is
            // untouched. That is the honest expectation, not a weaker one.
            (PickerAction::EraseToEnd, "alpha-eks"),
            (PickerAction::EraseForward, "alpha-eks"),
        ] {
            let mut p = three();
            filter(&mut p, "alpha-eks");
            assert_eq!(p.query.text(), "alpha-eks", "typed");
            p.dispatch(chord);
            assert_eq!(p.query.text(), expect, "{chord:?}");
        }
    }

    /// **The caret must survive the erase.** `Vim::apply` clamped to
    /// `Stance::Normal` unconditionally, which was invisible while only
    /// Normal could reach it. In Insert it drags the caret one character back
    /// off the end, so the next keystroke lands before the space `ctrl+w`
    /// just stopped at — "foo b" instead of "foo x".
    #[test]
    fn ctrl_w_leaves_the_caret_where_the_next_character_belongs() {
        let mut p = three();
        filter(&mut p, "foo bar");
        p.dispatch(PickerAction::EraseWord);
        assert_eq!(p.query.text(), "foo ");
        p.type_char('x');
        assert_eq!(p.query.text(), "foo x", "the caret stayed past the space");
    }

    /// An erase chord at the edge takes nothing and reports so, rather than
    /// refiltering an unchanged query.
    #[test]
    fn an_erase_chord_with_nothing_to_take_is_inert() {
        let mut p = three();
        filter(&mut p, "");
        for chord in [
            PickerAction::EraseWord,
            PickerAction::EraseToStart,
            PickerAction::EraseToEnd,
            PickerAction::EraseForward,
        ] {
            assert!(!p.dispatch(chord), "{chord:?} on an empty query");
            assert_eq!(p.query.text(), "");
        }
    }

    #[test]
    fn typing_filters_and_enter_chooses_the_match() {
        let mut p = three();
        filter(&mut p, "jaeg");
        assert!(p.dispatch(PickerAction::Accept));
        assert_eq!(
            p.chosen().map(|c| c.name.as_str()),
            Some("jaeger-dev"),
            "the filtered match is what enter commits",
        );
        assert!(p.should_quit(), "committing leaves the screen");
    }

    /// The query matches the apiserver too, so an operator who remembers the
    /// estate but not the context name can still get there.
    #[test]
    fn the_query_matches_the_apiserver_not_only_the_name() {
        let mut p = three();
        filter(&mut p, "amazon");
        assert!(p.dispatch(PickerAction::Accept));
        assert_eq!(p.chosen().map(|c| c.name.as_str()), Some("alpha-eks"));
    }

    /// **THE AMBIGUITY GATE.** This is the 2026-08-08 `charlie-local` hazard —
    /// one name, two real clusters — caught at the moment of *choosing*. Left
    /// unguarded, the picker would hand `main` a name that `resolve_context`
    /// then refuses, i.e. it would offer a choice banken cannot honour.
    #[test]
    fn an_ambiguous_context_cannot_be_chosen() {
        let mut p = picker(vec![ambiguous("charlie-local")]);
        assert!(p.dispatch(PickerAction::Accept));
        assert!(
            p.chosen().is_none(),
            "an ambiguous name must not be committed",
        );
        assert!(!p.should_quit(), "and the screen stays up to say why");
        let refusal = p.refusal().expect("the refusal is shown");
        assert!(refusal.contains("charlie-local"));
        assert!(
            refusal.contains("/home/op/.kube/config"),
            "and it names the files, so the fix is on screen: {refusal}",
        );
    }

    /// **A refusal must stay actionable ON SCREEN, not merely be correct in a
    /// `String`.** Measured in a real 120-column PTY: the one-line footer
    /// clipped the message mid-path, dropping the second declaring file — the
    /// half that says which one to remove. Both paths must survive the frame.
    #[test]
    fn a_long_refusal_wraps_instead_of_clipping() {
        let mut p = picker(vec![ambiguous("charlie-local")]);
        p.dispatch(PickerAction::Accept);
        let mut backend = TestBackend::new(80, 14);
        backend.draw(|buf| p.render(buf));
        let frame = backend.to_lines().join("\n");
        for file in ["/home/op/.kube/configs/local", "/home/op/.kube/config"] {
            assert!(frame.contains(file), "`{file}` must be on screen: {frame}");
        }
    }

    /// **`pending-banken: reconnect-on-failed-pick`.** A connect that fails —
    /// VPN down, expired credentials — used to print an error and exit,
    /// throwing away the list the operator was choosing from. The notice is
    /// what lets `main` re-enter the picker with the reason on screen and the
    /// other contexts still there.
    #[test]
    fn a_carried_notice_is_shown_on_re_entry() {
        let p = three().with_notice("live connect failed (VPN/kubeconfig?): timed out");
        let mut backend = TestBackend::new(100, 12);
        backend.draw(|buf| p.render(buf));
        let frame = backend.to_lines().join("\n");
        assert!(
            frame.contains("VPN/kubeconfig"),
            "the reason must be on the screen the operator lands on:\n{frame}",
        );
        assert!(
            frame.contains("alpha-eks"),
            "and the list must still be there — that is the whole point:\n{frame}",
        );
    }

    /// It describes the screen BEFORE this one, so the first keystroke here
    /// clears it — the same rule a refusal follows.
    #[test]
    fn the_first_keystroke_clears_a_carried_notice() {
        let mut p = three().with_notice("connect failed");
        p.type_char('c');
        let mut backend = TestBackend::new(100, 12);
        backend.draw(|buf| p.render(buf));
        assert!(!backend.to_lines().join("\n").contains("connect failed"));
    }

    /// A refusal describes the keystroke that produced it. Leaving it up after
    /// the next one would make it look like it described that one instead.
    #[test]
    fn the_next_keystroke_clears_a_refusal() {
        let mut p = picker(vec![ambiguous("charlie-local")]);
        p.dispatch(PickerAction::Accept);
        assert!(p.refusal().is_some());
        p.type_char('e');
        assert!(p.refusal().is_none());
    }

    /// `enter` on a query that matches nothing is inert, not an error —
    /// over-filtering is a normal step, not a mistake worth a panel.
    #[test]
    fn enter_with_no_match_is_inert() {
        let mut p = three();
        filter(&mut p, "zzzz");
        assert!(!p.dispatch(PickerAction::Accept));
        assert!(p.chosen().is_none());
        assert!(!p.should_quit());
    }

    /// **The landing screen opens in NORMAL** (operator decision,
    /// 2026-08-12). It opened in INSERT, so `j` typed a `j` instead of moving
    /// — a screen that advertises a stance badge and then ignores the stance's
    /// keys.
    #[test]
    fn the_picker_opens_in_normal_and_j_moves_a_row() {
        let mut p = three();
        assert_eq!(p.stance_badge(), "NORMAL");
        assert_eq!(p.picker.view().selected, 0);

        p.type_char('j');
        assert_eq!(p.picker.view().selected, 1, "`j` moved a row");
        assert_eq!(p.query.text(), "", "and typed nothing");

        p.type_char('i');
        assert_eq!(p.stance_badge(), "INSERT", "`i` starts writing");
        p.type_char('j');
        assert_eq!(p.query.text(), "j", "and now `j` IS a character");
    }

    /// **`esc` NEVER leaves the screen** (operator: "hitting escape multiple
    /// times shouldn't eventually exit the app, it should at most leave you in
    /// normal mode").
    ///
    /// It used to: the first `esc` left Insert and the second quit — so the
    /// universal "get me out of whatever I'm in" reflex closed the navigator
    /// instead of arriving somewhere known. Held over MANY presses, because
    /// the failure mode is specifically a repeated one.
    #[test]
    fn escape_never_leaves_the_screen_however_many_times_it_is_pressed() {
        let mut p = three();
        p.type_char('i');
        assert_eq!(p.stance_badge(), "INSERT");

        for press in 0..12 {
            p.dispatch(PickerAction::Escape);
            assert!(!p.should_quit(), "escape #{press} left the screen");
            assert_eq!(p.stance_badge(), "NORMAL", "escape #{press}");
        }
        assert!(p.chosen().is_none());
    }

    /// The other half of the same decision: something still has to leave, and
    /// it is `q` in NORMAL — the key the authored vocabulary already spells
    /// for the quit intent.
    #[test]
    fn q_leaves_from_normal_and_is_a_character_in_insert() {
        let mut p = three();
        p.type_char('q');
        assert!(p.should_quit(), "`q` in NORMAL leaves");
        assert!(p.chosen().is_none(), "leaving is not choosing");

        let mut p = three();
        p.type_char('i');
        p.type_char('q');
        assert!(!p.should_quit(), "`q` in INSERT is text, not an exit");
        assert_eq!(p.query.text(), "q");
    }

    /// **THE ANTI-DRIFT GATE on the handoff.** `q` is handled in
    /// [`crate::vim`], not in this screen's keymap, so the two could disagree
    /// silently. This asserts the character vim treats as quit IS the chord
    /// the authored `(defnavkey :intent quit)` declares — re-spell it upstream
    /// and this fails rather than stranding the operator with no advertised
    /// way out.
    #[test]
    fn the_quit_char_is_the_authored_quit_chord() {
        let catalog = banken_spec::load_catalog().expect("the shipped vocabulary must load");
        let quit: Vec<String> = catalog
            .nav_keys()
            .iter()
            .filter(|n| n.intent == NavIntent::Quit)
            .map(|n| n.keys.hotkey().display())
            .collect();
        assert!(
            quit.iter().any(|c| c == "q"),
            "vim.rs treats `q` as quit; the vocabulary declares {quit:?}",
        );
    }

    /// **`ctrl+c` must work from EVERY stance.** This is the trap the
    /// `Escape`/`Abort` split exists for: both used to be one `Cancel` action
    /// routed through the modal layer, so making a resting escape inert would
    /// have silently made `ctrl+c` inert too — taking the last resort out with
    /// the reflex key.
    #[test]
    fn ctrl_c_leaves_from_every_stance() {
        let mut p = three();
        assert!(p.dispatch(PickerAction::Abort), "from NORMAL");
        assert!(p.should_quit());

        let mut p = three();
        p.type_char('i');
        for c in "half-typed".chars() {
            p.type_char(c);
        }
        assert!(p.dispatch(PickerAction::Abort), "from INSERT mid-word");
        assert!(p.should_quit());
        assert!(p.chosen().is_none());
    }

    /// **The row must show the apiserver.** It is the whole reason picking is
    /// stronger than typing the name: a name is a label to trust, a URL is
    /// what actually gets dialled.
    #[test]
    fn a_row_renders_the_name_and_the_apiserver() {
        let p = three();
        let mut backend = TestBackend::new(100, 10);
        backend.draw(|buf| p.render(buf));
        let frame = backend.to_lines().join("\n");
        assert!(frame.contains("alpha-eks"), "{frame}");
        assert!(
            frame.contains("https://alpha.eks.amazonaws.com"),
            "the apiserver is on the row: {frame}",
        );
    }

    /// An ambiguous row is marked before it is accepted, not only after.
    #[test]
    fn an_ambiguous_row_is_marked_on_screen() {
        let p = picker(vec![
            ctx("alpha-eks", "https://alpha.eks.amazonaws.com"),
            ambiguous("charlie-local"),
        ]);
        let mut backend = TestBackend::new(110, 10);
        backend.draw(|buf| p.render(buf));
        let frame = backend.to_lines().join("\n");
        assert!(
            frame.contains("declared by 2 kubeconfig files"),
            "the ambiguity is visible before enter: {frame}",
        );
    }

    /// The footer chords are read from the vocabulary. A legend advertising a
    /// chord the runtime does not bind is the defect `key_legend_parts`
    /// exists to prevent, and a landing screen is the worst place for it.
    ///
    /// Every advertised chord is asserted to actually resolve — which is what
    /// makes this a gate rather than a spelling test.
    #[test]
    fn the_footer_advertises_the_bound_chords() {
        // Per stance, because the footer is per stance — and the INSERT half
        // is where the erase chords are advertised, i.e. exactly the claim an
        // operator would have called a lie a day ago.
        let mut p = three();
        for (stance, actions) in [
            (
                "NORMAL",
                vec![
                    PickerAction::Up,
                    PickerAction::Down,
                    PickerAction::Accept,
                ],
            ),
            (
                "INSERT",
                vec![
                    PickerAction::Up,
                    PickerAction::Down,
                    PickerAction::Accept,
                    PickerAction::Escape,
                    PickerAction::EraseWord,
                    PickerAction::EraseToStart,
                    PickerAction::EraseToEnd,
                ],
            ),
        ] {
            assert_eq!(p.stance_badge(), stance);
            let hint = p.hint();
            assert!(!hint.contains('?'), "every advertised chord binds: {hint}");
            for action in actions {
                let chord = chord_for(&p.advertised, action);
                assert!(
                    hint.contains(&chord),
                    "{stance}: {action:?} → `{chord}` not in {hint}"
                );
                let hotkey = awase::Hotkey::parse(&chord)
                    .unwrap_or_else(|e| panic!("the footer's `{chord}` must be a real chord: {e}"));
                assert_eq!(
                    act(p.keymap(), hotkey),
                    Some(action),
                    "{stance}: the footer's `{chord}` must actually do what it says",
                );
            }
            p.type_char('i');
        }
    }

    /// **The advertised way out must be the way out.** The NORMAL footer said
    /// `escape: quit` while escape quit; both changed in the same commit, and
    /// an operator left with a footer naming a key that no longer leaves would
    /// be stranded on the landing screen.
    #[test]
    fn the_hint_names_the_key_that_actually_leaves() {
        let mut p = three();
        let normal = p.hint();
        assert!(normal.contains("q: quit"), "NORMAL: {normal}");
        assert!(
            !normal.contains("escape: quit"),
            "escape does not quit any more: {normal}",
        );
        // And it is true: the advertised key leaves.
        p.type_char('q');
        assert!(p.should_quit());
    }

    /// The footer must describe the stance it is in. It advertised
    /// `type: filter` in NORMAL, where typing does not filter — and the footer
    /// is what an operator reads to learn a screen they have just met.
    #[test]
    fn the_hint_describes_the_stance_it_is_in() {
        let mut p = three();
        let normal = p.hint();
        assert!(
            !normal.contains("type: filter"),
            "NORMAL must not claim typing filters: {normal}"
        );
        assert!(
            normal.contains("d/c/y"),
            "and must name the verbs: {normal}"
        );

        p.type_char('i'); // -> INSERT
        let insert = p.hint();
        assert!(insert.contains("type: filter"), "INSERT: {insert}");
        assert!(
            insert.contains("erase"),
            "and must name the erase chords, which is how they get found: {insert}",
        );
        assert_ne!(insert, normal);
    }

    /// **A non-determinism gate.** `escape` and `ctrl+c` both cancel, and
    /// `KeyMode::bindings` is a `HashMap` — so taking the first match made the
    /// footer advertise a different chord run to run. Measured on the first
    /// run of the test above, which is why this one exists.
    #[test]
    fn the_advertised_chord_is_stable_across_builds() {
        for _ in 0..8 {
            let p = three();
            assert_eq!(chord_for(&p.advertised, PickerAction::Escape), "escape");
            assert_eq!(chord_for(&p.advertised, PickerAction::Abort), "ctrl+c");
        }
    }

    /// The current-context is annotated and **not** pre-selected: showing it
    /// is useful, defaulting to it would rebuild the habit `--context` was
    /// made required to break.
    #[test]
    fn the_selection_starts_at_the_top_regardless_of_current_context() {
        let p = three();
        assert_eq!(p.picker.view().selected, 0);
    }

    /// **The ladder reaches the row.** Choosing used to be a guess: eighteen
    /// contexts, all equally choosable-looking, and the only way to find out
    /// how far you would actually get was to pick one and wait.
    #[test]
    fn the_ladder_marks_the_rows() {
        use crate::ronda::{Rung, Standing};
        let (ronda, publisher) = crate::ronda::channel();
        publisher.seed(&["alpha-eks".into(), "bravo".into(), "jaeger-dev".into()]);
        publisher.report("alpha-eks", Standing::at(Rung::Pods));
        publisher.report("bravo", Standing::at(Rung::Down));

        let p = three().with_ronda(ronda);
        let mut backend = TestBackend::new(110, 12);
        backend.draw(|buf| p.render(buf));
        let lines = backend.to_lines();

        let row = |name: &str| {
            lines
                .iter()
                .find(|l| l.contains(name))
                .unwrap_or_else(|| panic!("no row for {name}"))
                .clone()
        };
        assert!(row("alpha-eks").contains(Rung::Pods.marker()), "{}", row("alpha-eks"));
        assert!(row("bravo").contains(Rung::Down.marker()), "{}", row("bravo"));
        // Not looked at yet, and saying so — never silently optimistic.
        assert!(
            row("jaeger-dev").contains(Rung::Unknown.marker()),
            "{}",
            row("jaeger-dev"),
        );
    }

    /// **The light is a function of the ladder, drawn from the EASED position
    /// — not from the rung.** That is the difference between a colour that
    /// slides up the ramp and five presets that blink.
    #[test]
    fn the_light_eases_toward_the_rung_rather_than_jumping() {
        use crate::ronda::{Rung, Standing};
        let (ronda, publisher) = crate::ronda::channel();
        publisher.seed(&["alpha-eks".into()]);
        publisher.report("alpha-eks", Standing::at(Rung::Pods));

        let mut p = three().with_ronda(ronda);
        p.ease_lights();
        let first = p.lights.get("alpha-eks").copied().expect("a light");
        assert!(
            (first - 1.0).abs() < f32::EPSILON,
            "a light with no history starts AT its rung, not at zero — a new \
             row must not animate up from `down`, which would read as a climb \
             that never happened",
        );

        // Now a real climb: from the floor to the top.
        publisher.report("alpha-eks", Standing::at(Rung::Down));
        p.lights.insert("alpha-eks".into(), 0.0);
        publisher.report("alpha-eks", Standing::at(Rung::Pods));

        let mut seen = Vec::new();
        for _ in 0..40 {
            p.ease_lights();
            seen.push(p.lights.get("alpha-eks").copied().expect("a light"));
        }
        assert!(seen[0] > 0.0 && seen[0] < 1.0, "it moved, partway: {}", seen[0]);
        assert!(
            seen.windows(2).all(|w| w[1] >= w[0]),
            "and only ever upward: {seen:?}",
        );
        assert!(
            (seen.last().copied().unwrap_or_default() - 1.0).abs() < f32::EPSILON,
            "and it arrives rather than chasing the limit forever",
        );
    }

    /// A moving light holds the FAST cadence even after the round settled —
    /// otherwise the ease would be drawn at 1 fps and the "smooth" change
    /// would be a handful of visible steps.
    #[test]
    fn a_moving_light_keeps_the_fast_cadence() {
        use crate::ronda::{Rung, Standing};
        let (ronda, publisher) = crate::ronda::channel();
        publisher.seed(&["a".into()]);
        let mut p = three().with_ronda(ronda);
        let unsettled = p.repaint_after().expect("a round repaints");

        publisher.report("a", Standing::at(Rung::Pods));
        p.lights.insert("a".into(), 0.0); // mid-transition
        assert_eq!(
            p.repaint_after(),
            Some(unsettled),
            "the round settled but the light has not",
        );

        for _ in 0..60 {
            p.ease_lights();
        }
        let settled = p.repaint_after().expect("still repaints, more slowly");
        assert!(
            settled > unsettled,
            "settled ({settled:?}) must be slower than moving ({unsettled:?})",
        );
    }

    /// **The selected row's standing says WHY the climb stopped.** A marker
    /// gives the level; only prose gives the fix. "port open, no apiserver
    /// reached — credentials: SSO session expired" is actionable; an orange
    /// dot is a mood.
    #[test]
    fn the_standing_line_says_why_the_climb_stopped() {
        use crate::ronda::{Rung, Standing};
        let (ronda, publisher) = crate::ronda::channel();
        publisher.seed(&["alpha-eks".into()]);
        publisher.report(
            "alpha-eks",
            Standing::stopped(Rung::Network, "credentials: SSO session expired"),
        );

        let p = three().with_ronda(ronda);
        let mut backend = TestBackend::new(110, 12);
        backend.draw(|buf| p.render(buf));
        let frame = backend.to_lines().join("\n");
        assert!(frame.contains(Rung::Network.label()), "{frame}");
        assert!(frame.contains("SSO session expired"), "{frame}");
    }

    /// **A picker with no rounds draws no markers**, rather than a column of
    /// dots that mean "banken has no idea" but read as a verdict.
    #[test]
    fn without_rounds_no_marker_column_is_drawn() {
        let p = three();
        let mut backend = TestBackend::new(110, 10);
        backend.draw(|buf| p.render(buf));
        let frame = backend.to_lines().join("\n");
        for rung in crate::ronda::Rung::ALL {
            assert!(
                !frame.contains(rung.marker()),
                "`{}` ({rung:?}) must not appear without a round:\n{frame}",
                rung.marker(),
            );
        }
    }

    /// **The redraw cadence is OFF without a round.** A picker waking many
    /// times a second forever because it might learn something is a busy loop
    /// with a good excuse.
    #[test]
    fn without_a_round_the_picker_is_input_only() {
        assert_eq!(three().repaint_after(), None, "no round, no repaint");
    }

    /// A narrow terminal must not panic or drop the screen.
    #[test]
    fn a_tiny_terminal_still_renders() {
        let p = three();
        for (w, h) in [(20_u16, 5_u16), (8, 3), (1, 1)] {
            let mut backend = TestBackend::new(w, h);
            backend.draw(|buf| p.render(buf));
        }
    }
}

#[cfg(test)]
mod stance_config_tests {
    use super::*;

    /// The authored stance actually reaches the screen — a config field
    /// nothing reads is the exact trap this workspace already paid for once,
    /// when `:refresh-interval-ms` was authored, tested and ignored for the
    /// whole life of the crate.
    #[test]
    fn the_builder_overrides_the_prescribed_stance() {
        let catalog = banken_spec::load_catalog().expect("the shipped vocabulary must load");
        let ctxs = vec![ContextChoice {
            name: "alpha-eks".into(),
            server: Some("https://k8s.example.invalid:6443".into()),
            files: vec![std::path::PathBuf::from("/home/op/.kube/config")],
        }];

        let p = ContextPicker::with_contexts(&catalog, ctxs.clone());
        assert_eq!(p.stance_badge(), "NORMAL", "the prescribed default");

        let p = ContextPicker::with_contexts(&catalog, ctxs).opening_in(unsoku::Stance::Insert);
        assert_eq!(
            p.stance_badge(),
            "INSERT",
            "an authored `:picker-stance \"insert\"` must reach the screen",
        );
    }
}
