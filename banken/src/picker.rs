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
//! That third point is the 2026-08-08 `engenho-local` hazard (one name, a
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
//! `backspace` is bound structurally and is deliberately **not** a
//! `(defnavkey)`: it edits a text query, not a selection. The nav vocabulary
//! describes moving around a resource view, and giving it a text-editing
//! intent would make `NavIntent` mean two things.

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
    /// Leave without choosing.
    Cancel,
    /// Erase one character from the filter query.
    Erase,
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
            current: kubeconfig_current_context(),
            refusal: None,
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
        // this one.
        self.refusal = None;

        let event = match action {
            PickerAction::Up => PickerEvent::NavUp,
            PickerAction::Down => PickerEvent::NavDown,
            PickerAction::Erase => PickerEvent::Backspace,
            PickerAction::Cancel => PickerEvent::Cancel,
            PickerAction::Accept => {
                // The gate. An ambiguous name does not identify one cluster,
                // so accepting it would be an affordance banken cannot honour
                // — `resolve_context` refuses it moments later anyway. Better
                // to say so on the row the operator is looking at.
                match self.picker.selected_item() {
                    Some(item) if item.key.is_ambiguous() => {
                        self.refusal = Some(ambiguity_refusal(&item.key));
                        return true;
                    }
                    // Nothing matches the query: `enter` is inert, not an
                    // error. Typing a filter that matches nothing is a normal
                    // step on the way to one that does.
                    None => return false,
                    Some(_) => PickerEvent::Accept,
                }
            }
        };

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

    /// Type one character into the filter query.
    pub fn type_char(&mut self, c: char) {
        self.refusal = None;
        let _effects = self.picker.on_event(PickerEvent::Type(c));
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
            let caret = 2 + u16::try_from(view.query.chars().count()).unwrap_or(0);
            if caret < width {
                buf.set_char(caret, 1, '▏', Style::default().fg(Color::Cyan));
            }
        }
        if height > 2 {
            buf.hline(0, 2, width, '─', Style::default().fg(Color::DarkGrey));
        }

        // ── Rows ──
        let first_row = 3;
        let footer = height.saturating_sub(1);
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

        // ── Footer ──
        self.draw_footer(buf, width, height);
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
        let Some(refusal) = &self.refusal else {
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
    fn hint(&self) -> String {
        let mut s = String::new();
        s.push_str(&chord_for(&self.advertised, PickerAction::Up));
        s.push('/');
        s.push_str(&chord_for(&self.advertised, PickerAction::Down));
        s.push_str(": move  ·  type: filter  ·  ");
        s.push_str(&chord_for(&self.advertised, PickerAction::Accept));
        s.push_str(": open  ·  ");
        s.push_str(&chord_for(&self.advertised, PickerAction::Cancel));
        s.push_str(": quit");
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
    // Backspace is text editing, not navigation — see the module docs for why
    // it is not a `(defnavkey)`. Ctrl-C is the universal terminal escape
    // hatch, and an operator's last resort must not depend on the vocabulary
    // having loaded the way they expected.
    for (hotkey, action) in [
        (
            awase::Hotkey::new(Modifiers::NONE, Key::Backspace),
            PickerAction::Erase,
        ),
        (
            awase::Hotkey::new(Modifiers::CTRL, Key::C),
            PickerAction::Cancel,
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
/// - `Dismiss` and `Quit` both mean "leave", and both are already `Cancel`;
///   `Quit`'s authored chord is `q`, which is filtered out as typable, so in
///   practice `escape` carries it.
fn picker_action(intent: NavIntent) -> Option<PickerAction> {
    match intent {
        NavIntent::SelectNext => Some(PickerAction::Down),
        NavIntent::SelectPrev => Some(PickerAction::Up),
        NavIntent::Confirm => Some(PickerAction::Accept),
        NavIntent::Dismiss | NavIntent::Quit => Some(PickerAction::Cancel),
        NavIntent::ToggleSort | NavIntent::Help => None,
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
            ctx("camelot-eks", "https://camelot.eks.amazonaws.com"),
            ctx("rio", "https://rio.internal:6443"),
            ctx("jaeger-dev", "https://jaeger.dev:6443"),
        ])
    }

    fn act(km: &awase::KeyMode<PickerAction>, hk: awase::Hotkey) -> Option<PickerAction> {
        km.find_binding(&hk, &awase::MatchContext::default())
            .map(|b| b.action)
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
            (Key::Escape, PickerAction::Cancel),
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
            Some(PickerAction::Cancel),
        );
    }

    #[test]
    fn typing_filters_and_enter_chooses_the_match() {
        let mut p = three();
        for c in "jaeg".chars() {
            p.type_char(c);
        }
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
        for c in "amazon".chars() {
            p.type_char(c);
        }
        assert!(p.dispatch(PickerAction::Accept));
        assert_eq!(p.chosen().map(|c| c.name.as_str()), Some("camelot-eks"));
    }

    /// **THE AMBIGUITY GATE.** This is the 2026-08-08 `engenho-local` hazard —
    /// one name, two real clusters — caught at the moment of *choosing*. Left
    /// unguarded, the picker would hand `main` a name that `resolve_context`
    /// then refuses, i.e. it would offer a choice banken cannot honour.
    #[test]
    fn an_ambiguous_context_cannot_be_chosen() {
        let mut p = picker(vec![ambiguous("engenho-local")]);
        assert!(p.dispatch(PickerAction::Accept));
        assert!(
            p.chosen().is_none(),
            "an ambiguous name must not be committed",
        );
        assert!(!p.should_quit(), "and the screen stays up to say why");
        let refusal = p.refusal().expect("the refusal is shown");
        assert!(refusal.contains("engenho-local"));
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
        let mut p = picker(vec![ambiguous("engenho-local")]);
        p.dispatch(PickerAction::Accept);
        let mut backend = TestBackend::new(80, 14);
        backend.draw(|buf| p.render(buf));
        let frame = backend.to_lines().join("\n");
        for file in ["/home/op/.kube/configs/local", "/home/op/.kube/config"] {
            assert!(frame.contains(file), "`{file}` must be on screen: {frame}");
        }
    }

    /// A refusal describes the keystroke that produced it. Leaving it up after
    /// the next one would make it look like it described that one instead.
    #[test]
    fn the_next_keystroke_clears_a_refusal() {
        let mut p = picker(vec![ambiguous("engenho-local")]);
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
        for c in "zzzz".chars() {
            p.type_char(c);
        }
        assert!(!p.dispatch(PickerAction::Accept));
        assert!(p.chosen().is_none());
        assert!(!p.should_quit());
    }

    #[test]
    fn escape_leaves_without_choosing() {
        let mut p = three();
        assert!(p.dispatch(PickerAction::Cancel));
        assert!(p.should_quit());
        assert!(
            p.chosen().is_none(),
            "leaving is not choosing — `main` must exit quietly, not connect",
        );
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
        assert!(frame.contains("camelot-eks"), "{frame}");
        assert!(
            frame.contains("https://camelot.eks.amazonaws.com"),
            "the apiserver is on the row: {frame}",
        );
    }

    /// An ambiguous row is marked before it is accepted, not only after.
    #[test]
    fn an_ambiguous_row_is_marked_on_screen() {
        let p = picker(vec![
            ctx("camelot-eks", "https://camelot.eks.amazonaws.com"),
            ambiguous("engenho-local"),
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
        let p = three();
        let hint = p.hint();
        assert!(!hint.contains('?'), "every advertised chord binds: {hint}");
        for action in [
            PickerAction::Up,
            PickerAction::Down,
            PickerAction::Accept,
            PickerAction::Cancel,
        ] {
            let chord = chord_for(&p.advertised, action);
            assert!(
                hint.contains(&chord),
                "{action:?} → `{chord}` not in {hint}"
            );
            let hotkey = awase::Hotkey::parse(&chord)
                .unwrap_or_else(|e| panic!("the footer's `{chord}` must be a real chord: {e}"));
            assert_eq!(
                act(p.keymap(), hotkey),
                Some(action),
                "the footer's `{chord}` must actually do what it says",
            );
        }
    }

    /// **A non-determinism gate.** `escape` and `ctrl+c` both cancel, and
    /// `KeyMode::bindings` is a `HashMap` — so taking the first match made the
    /// footer advertise a different chord run to run. Measured on the first
    /// run of the test above, which is why this one exists.
    #[test]
    fn the_advertised_chord_is_stable_across_builds() {
        for _ in 0..8 {
            let p = three();
            assert_eq!(chord_for(&p.advertised, PickerAction::Cancel), "escape");
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
