//! The picker's modal editing — `unsoku` wired to the query line.
//!
//! # Why banken owns a text model at all
//!
//! `egaku::FuzzyPicker` owns its query as a private `TextInput`, and its seven
//! `PickerEvent` arms carry `Type(char)` and `Backspace` and nothing else. So
//! the query was **append-only**: an operator could type and backspace, and
//! could not move a cursor, delete a word, or change inside quotes.
//!
//! egaku 0.1.12's `query_mut()` guard closes that upstream, and this module is
//! the consumer side of it: [`QueryLine`] is a local type implementing
//! [`unsoku::TextTarget`], which is orphan-rule-clean in both directions (a
//! foreign trait on a local type) and needs no further upstream change.
//!
//! # What is banken's and what is not
//!
//! Everything about *what a keystroke means* is `unsoku`'s — the motions, the
//! operators, the objects, the register, and the eight vim rules it tests.
//! banken owns only the **stroke→intent** reading, which is genuinely local:
//! `j`/`k` address the picker's ROW LIST, not the text, and no editor has an
//! opinion about that.
//!
//! # Strokes, not chords, and that is forced
//!
//! Vim keys arrive as [`Stroke::Char`], never as `awase::Hotkey`s. awase's
//! punctuation set is the unshifted US keycaps only, so `$` and `^` return
//! `None` from `Key::from_name` and can never become chords — verified against
//! awase 0.1.8. Routing vim through chords would have dropped two motions from
//! the agreed v1 alphabet, and would have collided `G` with the authored
//! `shift+g` (pod-break-glass). See `theory/UNSOKU.md` §V.
//!
//! The **erase chords** are the deliberate exception, and they run the other
//! way for the same reason: `ctrl+w` / `ctrl+u` / `ctrl+k` and the `delete`
//! key are the deletions an operator expects in ANY one-line query box, and
//! none of them can be a character —
//! `egaku_term::app::text_char` refuses every CTRL-modified key
//! (`egaku-term/src/app.rs:188`) and `delete` is not a `KeyCode::Char` at all.
//! So they arrive as chords, and [`Stroke`] names each one rather than the key
//! that produced it. Measured 2026-08-12 against the live 0.1.15 binary: all
//! four were INERT — the picker bound `backspace` and `ctrl+c` and nothing
//! else, so the only deletion the first screen had was one character at a
//! time.

use unsoku::{Motion, Operator, TextObject};
use unsoku::{Register, Stance, TextTarget};

/// The picker's query as something `unsoku` can edit.
///
/// A local type so `impl unsoku::TextTarget for QueryLine` is orphan-clean.
/// It mirrors egaku's `TextInput` shape (text + byte caret) because that is
/// what it is copied out of and written back into through `query_mut()`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct QueryLine {
    text: String,
    caret: usize,
}

impl QueryLine {
    /// Adopt the picker's current query, caret at the end — where an
    /// append-only query always left it.
    #[must_use]
    pub fn adopt(text: &str) -> Self {
        Self {
            caret: text.len(),
            text: text.to_owned(),
        }
    }

    /// The text, for writing back into the picker.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// The caret as a **char index**, which is what a drawer needs.
    ///
    /// `unsoku` and `egaku::TextInput` both count BYTES; a terminal column
    /// counts characters. banken drew its caret from `query.chars().count()`
    /// and ignored the real cursor entirely, which was invisible only because
    /// an append-only caret is always at the end. The moment `h` works, a
    /// multi-byte context name puts the caret in the wrong column — so the
    /// conversion lives here, once.
    #[must_use]
    pub fn caret_col(&self) -> usize {
        self.text[..self.caret].chars().count()
    }
}

impl TextTarget for QueryLine {
    fn text(&self) -> &str {
        &self.text
    }
    fn caret(&self) -> usize {
        self.caret
    }
    fn set_caret(&mut self, at: usize) {
        self.caret = at.min(self.text.len());
    }
    fn replace(&mut self, range: std::ops::Range<usize>, with: &str) {
        self.text.replace_range(range, with);
        self.caret = self.caret.min(self.text.len());
    }
}

/// One keystroke, as the vim layer reads it.
///
/// A char, one of the three structural keys, or one of the four **erase
/// chords**. Deliberately not `awase::Hotkey` — see the module docs.
///
/// # Why the erase chords are strokes and not `Char`s
///
/// `ctrl+w` is not a character: [`egaku_term::event::text_char`] refuses every
/// CTRL/ALT/SUPER-modified key, so it can only ever arrive as a *chord*. Naming
/// each one here rather than letting the picker resolve a span itself is what
/// keeps [`Vim::apply`] the single path text leaves the query by — which is the
/// property that makes "every removal fills the register" structural rather
/// than remembered per call site.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stroke {
    /// A printable character.
    Char(char),
    /// `esc` — leaves Insert, and cancels a pending operator in Normal.
    Escape,
    /// `backspace`.
    Backspace,
    /// `return`.
    Enter,
    /// `ctrl+w` — erase the word before the caret.
    EraseWordBack,
    /// `ctrl+u` — erase from the start of the line to the caret.
    EraseToStart,
    /// `ctrl+k` — erase from the caret to the end of the line.
    EraseToEnd,
    /// The `delete` key — erase the character AT the caret (forwards), the
    /// direction `backspace` cannot reach.
    EraseForward,
}

impl Stroke {
    /// The span this stroke erases, or `None` when it is not an erase chord.
    ///
    /// Free of `self.stance` on purpose: the four erase chords mean the same
    /// thing in both stances. That is not a shortcut — `ctrl+w` and `ctrl+u`
    /// are real vim *insert-mode* commands, and readline gives them the same
    /// reading everywhere else an operator meets a one-line query box. A
    /// deletion whose meaning changed with an invisible mode would be the
    /// worst of both models.
    fn erase_span(self, q: &QueryLine) -> Option<std::ops::Range<usize>> {
        let (text, caret) = (q.text.as_str(), q.caret);
        Some(match self {
            // Word boundaries are genuinely unsoku's to know, so this one is
            // resolved rather than sliced. A backward motion returns
            // `target..caret` (unsoku 0.1.71 lib.rs:412), which IS the span.
            Self::EraseWordBack => {
                unsoku::operated_span(text, caret, Operator::Delete, Motion::WordStartPrev, 1)?
            }
            // These three are exact slices for the same reason `D` and `x`
            // already are: there is no boundary rule to get wrong, and a
            // motion would only add a way for the answer to drift.
            Self::EraseToStart => 0..caret,
            Self::EraseToEnd => caret..text.len(),
            Self::EraseForward => caret..unsoku::next_char(text, caret),
            Self::Char(_) | Self::Escape | Self::Backspace | Self::Enter => return None,
        })
    }
}

/// What a stroke did, so the caller knows what to do about the screen.
///
/// The row-list arms exist because a picker has **two cursors** sharing one
/// keyboard: `j`/`k`/`gg`/`G` address rows while `h`/`l`/`w`/`b` address text,
/// and only the caller owns the row cursor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Effect {
    /// Nothing happened — an unbound key in Normal, swallowed rather than
    /// typed. This is the whole reason the stance matters.
    Inert,
    /// The query text changed; refilter.
    Edited,
    /// The caret moved but the text did not; redraw only.
    Moved,
    /// Move the row selection by `n` (negative is up).
    Rows(isize),
    /// Jump the row selection to the first or last row.
    RowEdge { last: bool },
    /// The operator confirmed a row.
    Accept,
    /// The operator left.
    Cancel,
}

/// The picker's modal editor: a stance, a pending count/operator, a register.
///
/// The composition is done here rather than through
/// `escriba_mode::OperatorPending` because that machine's event type is
/// `escriba_core::Action` — an editor's action vocabulary, carrying search
/// prompts and buffer verbs a query line has no reading for. Feeding it would
/// mean inventing `Action`s to throw away. `pending` below is the same
/// two-state idea over the two verbs a single line supports.
#[derive(Debug, Clone, Default)]
pub struct Vim {
    stance: Stance,
    /// The accumulating `{count}` prefix, `None` until a digit arrives.
    count: Option<usize>,
    /// The armed operator (`d`/`c`/`y`), waiting for a motion or object.
    pending: Option<Operator>,
    /// `i`/`a` seen after an operator — awaiting the object key.
    object: Option<bool>,
    /// `g` seen, awaiting the second `g`.
    g: bool,
    register: Register,
}

impl Vim {
    /// A modal editor opening in the stance the caller names.
    ///
    /// # The stance is the CALLER's, and that is the whole correction
    ///
    /// This used to be `for_filter()`, which hardcoded [`Stance::Insert`] and
    /// argued for it: a picker's primary act is typing a filter, so opening in
    /// Normal makes the operator press `i` before they can type. That argument
    /// is real but it is a *preference*, and burying a preference inside a
    /// constructor is what made it unchangeable — the operator asked for
    /// vim-true behaviour (navigate first, `i` to write) and there was no
    /// surface to ask it through.
    ///
    /// So the stance is a parameter. The prescribed default lives once, at
    /// [`crate::picker::ContextPicker::OPENING_STANCE`], and is the seam a
    /// `(defbanken …)` field writes to.
    #[must_use]
    pub fn opening_in(stance: Stance) -> Self {
        Self {
            stance,
            ..Self::default()
        }
    }

    /// The current stance, for the badge.
    #[must_use]
    pub fn stance(&self) -> Stance {
        self.stance
    }

    /// Whether a multi-key sequence is mid-flight, so the badge can show it.
    ///
    /// An operator who pressed `d` and sees nothing has no way to tell a armed
    /// verb from a dropped keystroke.
    #[must_use]
    pub fn pending_label(&self) -> Option<String> {
        let mut s = String::new();
        if let Some(n) = self.count {
            s.push_str(&n.to_string());
        }
        if let Some(op) = self.pending {
            s.push(match op {
                Operator::Delete => 'd',
                Operator::Change => 'c',
                _ => 'y',
            });
        }
        if let Some(around) = self.object {
            s.push(if around { 'a' } else { 'i' });
        }
        if self.g {
            s.push('g');
        }
        (!s.is_empty()).then_some(s)
    }

    /// Take and clear the pending count, defaulting to 1.
    fn take_count(&mut self) -> usize {
        self.count.take().unwrap_or(1)
    }

    fn reset(&mut self) {
        self.count = None;
        self.pending = None;
        self.object = None;
        self.g = false;
    }

    /// Feed one stroke.
    pub fn stroke(&mut self, s: Stroke, q: &mut QueryLine) -> Effect {
        // The erase chords resolve BEFORE the stance branch, because they mean
        // the same thing in both — see [`Stroke::erase_span`]. They also
        // resolve immediately, so they cancel anything armed: an operator who
        // pressed `d` and then reached for `ctrl+w` wants the word gone, not a
        // `d` still waiting for a motion afterwards.
        if let Some(span) = s.erase_span(q) {
            self.reset();
            if span.is_empty() {
                // At the edge the chord has nothing to take. Inert, never a
                // no-op `Edited` — a refilter on an unchanged query would make
                // the screen flicker for a keystroke that did nothing.
                return Effect::Inert;
            }
            return self.apply(Operator::Delete, span, q);
        }
        match self.stance {
            Stance::Insert => self.insert(s, q),
            Stance::Normal => self.normal(s, q),
        }
    }

    fn insert(&mut self, s: Stroke, q: &mut QueryLine) -> Effect {
        match s {
            Stroke::Char(c) => {
                let at = q.caret;
                q.replace(at..at, &c.to_string());
                q.set_caret(at + c.len_utf8());
                Effect::Edited
            }
            Stroke::Backspace => {
                if q.caret == 0 {
                    return Effect::Inert;
                }
                let from = unsoku::prev_char(&q.text, q.caret);
                q.replace(from..q.caret, "");
                q.set_caret(from);
                Effect::Edited
            }
            Stroke::Escape => {
                self.stance = Stance::Normal;
                // vim steps the caret left on leaving Insert, which is also
                // what the Normal clamp requires.
                let at = unsoku::clamp(&q.text, q.caret.saturating_sub(1), Stance::Normal);
                q.set_caret(at);
                Effect::Moved
            }
            Stroke::Enter => Effect::Accept,
            // Resolved in [`Self::stroke`] before the stance branch — an erase
            // chord reads the same in both stances, so neither arm may claim
            // one. Listed rather than wildcarded so a new [`Stroke`] variant is
            // a compile error here until this stance states what it means.
            Stroke::EraseWordBack
            | Stroke::EraseToStart
            | Stroke::EraseToEnd
            | Stroke::EraseForward => Effect::Inert,
        }
    }

    #[allow(clippy::too_many_lines)]
    fn normal(&mut self, s: Stroke, q: &mut QueryLine) -> Effect {
        let c = match s {
            Stroke::Enter => return Effect::Accept,
            Stroke::Escape => {
                // A pending sequence is cancelled; a RESTING escape does
                // nothing at all.
                //
                // **`esc` never leaves the screen.** It used to: the first
                // `esc` left Insert and the second quit, so an operator
                // pressing it twice by reflex — the universal "get me out of
                // whatever I'm in" gesture — closed the navigator instead of
                // arriving at a known stance. That is the opposite of what the
                // key means in vim, where `esc` is the way BACK to safety and
                // is idempotent once you are there.
                //
                // Leaving is `q` (below) or `ctrl+c`, both of which say
                // "leave" and nothing else.
                if self.pending.is_some() || self.count.is_some() || self.g || self.object.is_some()
                {
                    self.reset();
                    return Effect::Moved;
                }
                return Effect::Inert;
            }
            Stroke::Backspace => {
                let to = unsoku::prev_char(&q.text, q.caret);
                q.set_caret(to);
                return Effect::Moved;
            }
            Stroke::Char(c) => c,
            // See the matching arm in [`Self::insert`].
            Stroke::EraseWordBack
            | Stroke::EraseToStart
            | Stroke::EraseToEnd
            | Stroke::EraseForward => return Effect::Inert,
        };

        // `{count}` — a leading 0 is the LineStart motion, not a digit.
        if c.is_ascii_digit() && !(c == '0' && self.count.is_none()) {
            let d = c.to_digit(10).unwrap_or(0) as usize;
            self.count = Some(self.count.unwrap_or(0) * 10 + d);
            return Effect::Moved;
        }

        // The object key after `di`/`ca` — `diw`, `daw`, `ci"`.
        if let (Some(op), Some(around)) = (self.pending, self.object) {
            let object = match c {
                'w' => TextObject::Word { around },
                '"' | '\'' => TextObject::Delimited {
                    open: c,
                    close: c,
                    around,
                },
                '(' | ')' => TextObject::Delimited {
                    open: '(',
                    close: ')',
                    around,
                },
                '[' | ']' => TextObject::Delimited {
                    open: '[',
                    close: ']',
                    around,
                },
                '{' | '}' => TextObject::Delimited {
                    open: '{',
                    close: '}',
                    around,
                },
                _ => {
                    self.reset();
                    return Effect::Inert;
                }
            };
            self.reset();
            let Some(span) = unsoku::object_span(&q.text, q.caret, object) else {
                return Effect::Inert;
            };
            return self.apply(op, span, q);
        }

        // `gg` / `g`-prefix.
        if self.g {
            self.g = false;
            if c == 'g' {
                return Effect::RowEdge { last: false };
            }
            return Effect::Inert;
        }

        match c {
            // ── the two cursors: rows ──
            'j' => {
                let n = self.take_count();
                Effect::Rows(isize::try_from(n).unwrap_or(1))
            }
            'k' => {
                let n = self.take_count();
                Effect::Rows(-isize::try_from(n).unwrap_or(1))
            }
            'G' => {
                self.reset();
                Effect::RowEdge { last: true }
            }
            'g' => {
                self.g = true;
                Effect::Moved
            }

            // ── leaving ──
            //
            // Now that `esc` is idempotent, `q` is the key that leaves — and
            // it is not a second key list: `q` is what the authored
            // `(defnavkey :intent quit)` spells, asserted by
            // `the_quit_char_is_the_authored_quit_chord` so a re-spelling
            // upstream fails here rather than silently stranding the operator
            // with no way out but `ctrl+c`.
            //
            // Guarded on `pending`, like the stance arm below: `dq` is not a
            // vim sequence, and an armed `d` must not be able to quit.
            'q' if self.pending.is_none() => {
                self.reset();
                Effect::Cancel
            }

            // ── stance ──
            'i' | 'a' | 'I' | 'A' if self.pending.is_none() => {
                self.stance = Stance::Insert;
                let at = match c {
                    'i' => q.caret,
                    'a' => unsoku::next_char(&q.text, q.caret),
                    'I' => 0,
                    _ => q.text.len(),
                };
                q.set_caret(at);
                self.reset();
                Effect::Moved
            }

            // ── operators ──
            'd' | 'c' | 'y' => {
                let op = match c {
                    'd' => Operator::Delete,
                    'c' => Operator::Change,
                    _ => Operator::Yank,
                };
                // A doubled operator (`dd`) has no single-line reading —
                // `unsoku::object_span` refuses `TextObject::Line`. Clearing
                // the query is what `ctrl+u` is for; pretending `dd` means it
                // would make the register linewise in a model that has no
                // linewise.
                if self.pending == Some(op) {
                    self.reset();
                    return Effect::Inert;
                }
                self.pending = Some(op);
                Effect::Moved
            }
            'i' | 'a' if self.pending.is_some() => {
                self.object = Some(c == 'a');
                Effect::Moved
            }

            // ── the shorthands, which fill the register like any removal ──
            'x' => {
                self.reset();
                let to = unsoku::next_char(&q.text, q.caret);
                if to == q.caret {
                    return Effect::Inert;
                }
                self.apply(Operator::Delete, q.caret..to, q)
            }
            'D' => {
                self.reset();
                let end = q.text.len();
                self.apply(Operator::Delete, q.caret..end, q)
            }
            'C' => {
                self.reset();
                let end = q.text.len();
                let e = self.apply(Operator::Change, q.caret..end, q);
                self.stance = Stance::Insert;
                e
            }
            'p' | 'P' => {
                let n = self.take_count();
                self.reset();
                unsoku::paste(q, &self.register, c == 'p', n);
                Effect::Edited
            }

            // ── motions ──
            _ => {
                let Some(motion) = motion_of(c) else {
                    self.reset();
                    return Effect::Inert;
                };
                let n = self.take_count();
                // An armed operator makes the motion an OPERAND; a bare one
                // moves the caret. Same key, two readings — which is the
                // whole of vim's grammar in one branch.
                if let Some(op) = self.pending.take() {
                    self.reset();
                    let Some(span) = unsoku::operated_span(&q.text, q.caret, op, motion, n) else {
                        return Effect::Inert;
                    };
                    self.apply(op, span, q)
                } else {
                    let Some(at) = unsoku::resolve(&q.text, q.caret, motion, n) else {
                        return Effect::Inert;
                    };
                    q.set_caret(unsoku::clamp(&q.text, at, Stance::Normal));
                    Effect::Moved
                }
            }
        }
    }

    /// Run an operator over a resolved span. The one place text leaves the
    /// query, so "every removal fills the register" is structural.
    fn apply(&mut self, op: Operator, span: std::ops::Range<usize>, q: &mut QueryLine) -> Effect {
        match op {
            Operator::Yank => {
                unsoku::yank(q, span, &mut self.register);
                Effect::Moved
            }
            Operator::Change => {
                unsoku::take(q, span, &mut self.register);
                self.stance = Stance::Insert;
                Effect::Edited
            }
            _ => {
                unsoku::take(q, span, &mut self.register);
                // Clamped for the stance we are ACTUALLY in, not for Normal.
                //
                // A hardcoded `Stance::Normal` was invisibly correct while
                // `apply` was reachable only from `normal()`. The erase chords
                // make it reachable from Insert, where it is wrong in a way
                // that costs a character: `ctrl+w` over "foo bar" leaves
                // "foo " with the caret at 4, and a Normal clamp drags it back
                // to 3 — so the next key an operator types lands BEFORE the
                // space they just stopped at.
                let at = unsoku::clamp(&q.text, q.caret, self.stance);
                q.set_caret(at);
                Effect::Edited
            }
        }
    }
}

/// The motion a bare key names, `None` when it names none.
///
/// `$` and `^` are here as CHARACTERS, which is the whole reason strokes are
/// chars: neither can be an `awase::Hotkey`.
fn motion_of(c: char) -> Option<Motion> {
    Some(match c {
        'h' => Motion::Left,
        'l' => Motion::Right,
        'w' => Motion::WordStartNext,
        'b' => Motion::WordStartPrev,
        'e' => Motion::WordEndNext,
        '0' => Motion::LineStart,
        '^' => Motion::LineFirstNonBlank,
        '$' => Motion::LineEnd,
        _ => return None,
    })
}

#[cfg(test)]
mod tests;
