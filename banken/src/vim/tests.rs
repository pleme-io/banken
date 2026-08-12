//! The picker's modal layer, driven the way an operator drives it: as a
//! sequence of keystrokes, asserting the query and the stance after each.
//!
//! These do not re-test `unsoku` — its eight rules have their own suite.
//! What is banken's, and therefore tested here, is the stroke→intent reading:
//! two cursors on one keyboard, the stance transitions, and the sequences
//! (`3w`, `diw`, `gg`) that only exist as a composition.

use super::*;

/// Drive a literal key sequence. `<esc>` and `<bs>` are the two non-chars a
/// test needs to spell.
fn run(start: &str, keys: &str) -> (Vim, QueryLine, Vec<Effect>) {
    let mut v = Vim::default();
    let mut q = QueryLine::adopt(start);
    // Adopting leaves the caret at the end, which is where an append-only
    // query left it; Normal cannot rest there.
    let at = unsoku::clamp(q.text(), q.caret(), Stance::Normal);
    q.set_caret(at);
    let mut fx = Vec::new();
    let mut it = keys.chars().peekable();
    while let Some(c) = it.next() {
        let stroke = if c == '<' {
            let mut tag = String::new();
            for t in it.by_ref() {
                if t == '>' {
                    break;
                }
                tag.push(t);
            }
            match tag.as_str() {
                "esc" => Stroke::Escape,
                "bs" => Stroke::Backspace,
                "cr" => Stroke::Enter,
                other => panic!("unknown key tag <{other}>"),
            }
        } else {
            Stroke::Char(c)
        };
        fx.push(v.stroke(stroke, &mut q));
    }
    (v, q, fx)
}

fn text(start: &str, keys: &str) -> String {
    run(start, keys).1.text().to_owned()
}

// ── the stance ───────────────────────────────────────────────────────────

/// **THE PROPERTY THE WHOLE LAYER TRADES FOR.** A picker's unbound key used to
/// cost "a character you can delete". In Normal it costs nothing at all — but
/// only if unbound keys are genuinely inert rather than typed.
#[test]
fn an_unbound_key_in_normal_types_nothing() {
    let (v, q, fx) = run("alpha", "zZ%");
    assert_eq!(q.text(), "alpha", "not one character reached the query");
    assert_eq!(v.stance(), Stance::Normal);
    assert!(fx.iter().all(|e| *e == Effect::Inert), "{fx:?}");
}

/// **The opening stance is the CALLER's**, which is the correction this
/// constructor exists for.
///
/// It was `for_filter()`, hardcoding Insert on the argument that a chooser's
/// primary act is typing. That argument is real and it lost: the operator
/// asked for vim-true behaviour on the landing screen, and there was no
/// surface to ask it through because the preference was buried in a
/// constructor. The prescribed default now lives at
/// `ContextPicker::OPENING_STANCE` — one place, and a `(defbanken …)` field
/// can write to it.
#[test]
fn the_opening_stance_is_the_callers_to_choose() {
    assert_eq!(Vim::opening_in(Stance::Normal).stance(), Stance::Normal);
    assert_eq!(Vim::opening_in(Stance::Insert).stance(), Stance::Insert);
    assert_eq!(
        Stance::default(),
        Stance::Normal,
        "and the LIBRARY default is unchanged — this layer never had an opinion",
    );
}

#[test]
fn insert_types_and_escape_returns_to_normal() {
    let (v, q, _) = run("", "iabc<esc>");
    assert_eq!(q.text(), "abc");
    assert_eq!(v.stance(), Stance::Normal);
    assert_eq!(q.caret(), 2, "escape steps left, as vim does");
}

#[test]
fn a_and_capital_a_place_the_caret_where_vim_does() {
    let (_, q, _) = run("ab", "A!");
    assert_eq!(q.text(), "ab!", "A appends at the end");
    let (_, q, _) = run("ab", "I!");
    assert_eq!(q.text(), "!ab", "I inserts at the start");
}

// ── the two cursors ──────────────────────────────────────────────────────

/// `j`/`k` address the ROW LIST and must never touch the text. This is the
/// one thing about a picker that no editor has an opinion on.
#[test]
fn jk_move_rows_and_leave_the_query_alone() {
    let (_, q, fx) = run("alpha", "jjk");
    assert_eq!(q.text(), "alpha");
    assert_eq!(fx, vec![Effect::Rows(1), Effect::Rows(1), Effect::Rows(-1)]);
}

#[test]
fn a_count_applies_to_row_motion_too() {
    let (_, _, fx) = run("x", "5j");
    assert_eq!(fx.last(), Some(&Effect::Rows(5)));
}

#[test]
fn gg_and_capital_g_jump_to_the_row_edges() {
    assert_eq!(
        run("x", "gg").2.last(),
        Some(&Effect::RowEdge { last: false })
    );
    assert_eq!(
        run("x", "G").2.last(),
        Some(&Effect::RowEdge { last: true })
    );
}

// ── motions over the text ────────────────────────────────────────────────

#[test]
fn word_motions_move_the_caret_without_editing() {
    let (_, q, _) = run("alpha beta gamma", "0w");
    assert_eq!(q.caret(), 6, "`w` to `beta`");
    let (_, q, _) = run("alpha beta gamma", "0ww");
    assert_eq!(q.caret(), 11, "and again to `gamma`");
    let (_, q, _) = run("alpha beta gamma", "0$");
    assert_eq!(q.caret(), 15, "`$` clamps onto the last char in Normal");
}

/// `$` and `^` arrive as CHARACTERS. Neither can be an `awase::Hotkey` — that
/// is why the layer takes strokes, and this is the test that would fail if
/// someone routed vim through chords.
#[test]
fn dollar_and_caret_work_as_plain_characters() {
    let (_, q, _) = run("  padded  ", "^");
    assert_eq!(q.caret(), 2, "`^` to the first non-blank");
    let (_, q, fx) = run("abc", "0$");
    assert_eq!(q.caret(), 2);
    assert!(
        fx.iter().all(|e| *e != Effect::Inert),
        "both were understood"
    );
}

#[test]
fn a_count_multiplies_a_text_motion() {
    let (_, q, _) = run("a b c d", "03w");
    assert_eq!(q.caret(), 6, "`3w` lands on `d`");
}

// ── deletion ─────────────────────────────────────────────────────────────

#[test]
fn dw_deletes_a_word_and_x_deletes_a_character() {
    assert_eq!(text("alpha beta", "0dw"), "beta");
    assert_eq!(text("alpha", "0x"), "lpha");
}

#[test]
fn capital_d_deletes_to_the_end_and_capital_c_changes_to_it() {
    assert_eq!(text("alpha beta", "0wD"), "alpha ");
    let (v, q, _) = run("alpha beta", "0wC");
    assert_eq!(q.text(), "alpha ");
    assert_eq!(v.stance(), Stance::Insert, "`C` leaves you typing");
}

#[test]
fn a_change_operator_enters_insert_and_a_delete_does_not() {
    let (v, _, _) = run("alpha beta", "0cw");
    assert_eq!(v.stance(), Stance::Insert, "`cw` leaves you typing");
    let (v, _, _) = run("alpha beta", "0dw");
    assert_eq!(v.stance(), Stance::Normal);
}

#[test]
fn diw_and_daw_take_the_word_and_the_word_with_its_space() {
    assert_eq!(text("alpha beta", "0diw"), " beta");
    assert_eq!(text("alpha beta", "0daw"), "beta");
}

#[test]
fn ci_quote_changes_inside_quotes() {
    let (v, q, _) = run("name=\"old\"", "0ci\"");
    assert_eq!(q.text(), "name=\"\"");
    assert_eq!(v.stance(), Stance::Insert);
}

/// A doubled operator has no single-line reading and must be INERT, not
/// silently reinterpreted as "clear the query" — that would make the register
/// linewise in a model with no linewise.
#[test]
fn dd_is_refused_rather_than_clearing_the_query() {
    let (_, q, fx) = run("alpha-eks", "0dd");
    assert_eq!(q.text(), "alpha-eks", "untouched");
    assert_eq!(fx.last(), Some(&Effect::Inert));
}

// ── register ─────────────────────────────────────────────────────────────

#[test]
fn a_deleted_word_can_be_pasted_back() {
    let (_, q, _) = run("alpha beta", "0dw$p");
    assert_eq!(q.text(), "betaalpha ", "`p` after the last char");
}

#[test]
fn yank_then_paste_duplicates_without_removing() {
    let (_, q, _) = run("abc", "0yw$p");
    assert_eq!(q.text(), "abcabc");
}

// ── pending sequences ────────────────────────────────────────────────────

/// An operator who armed `d` by mistake must be able to cancel it without
/// leaving the picker. `esc` cancels the pending sequence — and, resting,
/// does nothing at all.
///
/// **`esc` never leaves.** It used to return `Effect::Cancel` when nothing
/// was armed, so the reflex double-tap closed the navigator. In vim `esc` is
/// the way BACK to a known stance and is idempotent once you are there;
/// leaving is `q`.
#[test]
fn escape_cancels_a_pending_operator_and_otherwise_does_nothing() {
    let (v, q, fx) = run("alpha", "0d<esc>");
    assert_eq!(q.text(), "alpha", "nothing was deleted");
    assert_eq!(fx.last(), Some(&Effect::Moved), "cancelled, did not leave");
    assert!(v.pending_label().is_none(), "and the badge is clear");

    let (_, _, fx) = run("alpha", "<esc><esc><esc><esc>");
    assert!(
        fx.iter().all(|e| *e == Effect::Inert),
        "no number of resting escapes leaves: {fx:?}",
    );
}

/// `q` is what leaves, and only when nothing is armed — `dq` is not a vim
/// sequence and must not be able to quit.
#[test]
fn q_leaves_but_never_out_of_an_armed_operator() {
    let (_, _, fx) = run("alpha", "q");
    assert_eq!(fx.last(), Some(&Effect::Cancel));

    let (v, q, fx) = run("alpha", "0dq");
    assert_eq!(fx.last(), Some(&Effect::Inert), "`dq` is not an exit");
    assert_eq!(q.text(), "alpha");
    assert!(v.pending_label().is_none(), "and it disarmed the `d`");
}

// ── the erase chords ─────────────────────────────────────────────────────

/// A query line with the caret placed exactly, because every erase chord is
/// defined RELATIVE to it and `adopt` always lands at the end — where two of
/// the four have nothing to take.
fn at(text: &str, caret: usize) -> QueryLine {
    let mut q = QueryLine::adopt(text);
    q.set_caret(caret);
    q
}

/// **The four deletions an operator expects in ANY query box.** Measured
/// inert on the live 0.1.15 binary — only `backspace` did anything.
///
/// Driven from INSERT, which is the stance they were reported missing from
/// and the one `apply`'s caret clamp used to be wrong in. The caret sits
/// mid-line (on the `b` of `beta`) so all four directions have something to
/// take — the first draft of this test parked it at the end and asserted
/// `Edited` for `ctrl+k` and `delete`, which have nothing to the right there.
#[test]
fn the_erase_chords_take_what_they_name() {
    const MID: usize = 6; // "alpha |beta"
    for (stroke, expect) in [
        (Stroke::EraseWordBack, "beta"),
        (Stroke::EraseToStart, "beta"),
        (Stroke::EraseToEnd, "alpha "),
        (Stroke::EraseForward, "alpha eta"),
    ] {
        let mut v = Vim::opening_in(Stance::Insert);
        let mut q = at("alpha beta", MID);
        assert_eq!(v.stroke(stroke, &mut q), Effect::Edited, "{stroke:?}");
        assert_eq!(q.text(), expect, "{stroke:?}");
        assert_eq!(v.stance(), Stance::Insert, "erasing is not a mode change");
    }
}

/// They mean the same thing in NORMAL — same caret, same spans. Not a
/// shortcut: a deletion whose meaning changed with an invisible mode is worse
/// than one that has no mode at all.
#[test]
fn the_erase_chords_read_the_same_in_normal() {
    const MID: usize = 6;
    for (stroke, expect) in [
        (Stroke::EraseWordBack, "beta"),
        (Stroke::EraseToStart, "beta"),
        (Stroke::EraseToEnd, "alpha "),
        (Stroke::EraseForward, "alpha eta"),
    ] {
        let mut v = Vim::opening_in(Stance::Normal);
        let mut q = at("alpha beta", MID);
        assert_eq!(v.stroke(stroke, &mut q), Effect::Edited, "{stroke:?}");
        assert_eq!(q.text(), expect, "{stroke:?}");
        assert_eq!(v.stance(), Stance::Normal, "and no mode change here either");
    }
}

/// The one thing that DOES differ between the stances is where the caret is
/// allowed to rest afterwards — Normal cannot sit past the last character,
/// Insert must be able to.
#[test]
fn the_caret_after_an_erase_obeys_the_stance() {
    let mut v = Vim::opening_in(Stance::Insert);
    let mut q = at("alpha beta", 10);
    v.stroke(Stroke::EraseWordBack, &mut q);
    assert_eq!((q.text(), q.caret()), ("alpha ", 6), "Insert rests at the end");

    let mut v = Vim::opening_in(Stance::Normal);
    let mut q = at("alpha beta", 10);
    v.stroke(Stroke::EraseWordBack, &mut q);
    assert_eq!(
        (q.text(), q.caret()),
        ("alpha ", 5),
        "Normal clamps onto the last character",
    );
}

/// Every removal fills the register (unsoku rule 8) — structural here,
/// because the chords go through `Vim::apply` rather than editing directly.
#[test]
fn an_erase_chord_fills_the_register() {
    let mut v = Vim::opening_in(Stance::Insert);
    let mut q = QueryLine::adopt("alpha beta");
    v.stroke(Stroke::EraseWordBack, &mut q);
    assert_eq!(q.text(), "alpha ");
    v.stroke(Stroke::Escape, &mut q); // -> NORMAL
    v.stroke(Stroke::Char('p'), &mut q);
    assert_eq!(q.text(), "alpha beta", "`p` put it back");
}

/// **The caret clamp had to stop being hardcoded to Normal.** In Insert a
/// Normal clamp drags the caret one character back off the end, so the next
/// key lands before the space the deletion stopped at.
#[test]
fn an_erase_chord_leaves_an_insert_caret_past_the_end() {
    let mut v = Vim::opening_in(Stance::Insert);
    let mut q = QueryLine::adopt("foo bar");
    v.stroke(Stroke::EraseWordBack, &mut q);
    assert_eq!(q.text(), "foo ");
    assert_eq!(q.caret(), 4, "at the end, not clamped onto the space");
    v.stroke(Stroke::Char('x'), &mut q);
    assert_eq!(q.text(), "foo x");
}

/// An erase chord resolves immediately, so it disarms anything pending — an
/// operator who pressed `d` and then reached for `ctrl+w` wants the word
/// gone, not a `d` still waiting afterwards.
#[test]
fn an_erase_chord_disarms_a_pending_operator() {
    let mut v = Vim::opening_in(Stance::Normal);
    let mut q = QueryLine::adopt("alpha beta");
    v.stroke(Stroke::Char('d'), &mut q);
    assert!(v.pending_label().is_some());
    v.stroke(Stroke::EraseToStart, &mut q);
    assert!(v.pending_label().is_none(), "the armed `d` is gone");
}

/// At the edge there is nothing to take, and the answer is Inert — never an
/// `Edited` that refilters an unchanged query for a keystroke that did
/// nothing.
#[test]
fn an_erase_chord_at_the_edge_is_inert() {
    let mut v = Vim::opening_in(Stance::Insert);
    let mut q = QueryLine::default();
    for stroke in [
        Stroke::EraseWordBack,
        Stroke::EraseToStart,
        Stroke::EraseToEnd,
        Stroke::EraseForward,
    ] {
        assert_eq!(v.stroke(stroke, &mut q), Effect::Inert, "{stroke:?}");
        assert_eq!(q.text(), "");
    }
}

/// Multi-byte text must survive every chord — the same guard the vim verbs
/// already carry, extended to the new path.
#[test]
fn the_erase_chords_never_panic_on_multi_byte_text() {
    for stroke in [
        Stroke::EraseWordBack,
        Stroke::EraseToStart,
        Stroke::EraseToEnd,
        Stroke::EraseForward,
    ] {
        for start in ["héllo wörld ünïcode", "é", "", "  "] {
            let mut v = Vim::opening_in(Stance::Insert);
            let mut q = QueryLine::adopt(start);
            let _ = v.stroke(stroke, &mut q);
        }
    }
}

/// A pending sequence must be visible. An operator who pressed `d` and sees
/// nothing cannot tell an armed verb from a dropped keystroke.
#[test]
fn a_pending_sequence_is_shown() {
    let (v, _, _) = run("alpha", "2d");
    assert_eq!(v.pending_label().as_deref(), Some("2d"));
    let (v, _, _) = run("alpha", "di");
    assert_eq!(v.pending_label().as_deref(), Some("di"));
    let (v, _, _) = run("alpha", "0dw");
    assert_eq!(v.pending_label(), None, "cleared once it resolves");
}

// ── the caret column ─────────────────────────────────────────────────────

/// **The latent bug this closes.** `unsoku` and `TextInput` both count BYTES;
/// a terminal column counts characters. banken drew its caret from
/// `query.chars().count()` and ignored the real cursor, which was invisible
/// only because an append-only caret is always at the end.
#[test]
fn the_caret_column_is_chars_not_bytes() {
    let (_, q, _) = run("héllo", "0ll");
    assert_eq!(q.caret(), 3, "byte offset — `é` is two bytes");
    assert_eq!(q.caret_col(), 2, "but the COLUMN is two characters in");
}

#[test]
fn editing_a_multi_byte_query_never_panics() {
    for keys in ["0dw", "0x", "0daw", "0wD", "$p", "0ciw"] {
        let _ = text("héllo wörld ünïcode", keys);
    }
}
