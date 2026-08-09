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
    let (v, q, fx) = run("camelot", "zZ%");
    assert_eq!(q.text(), "camelot", "not one character reached the query");
    assert_eq!(v.stance(), Stance::Normal);
    assert!(fx.iter().all(|e| *e == Effect::Inert), "{fx:?}");
}

/// **A filter box opens in INSERT.** `Stance::default()` is Normal, which is
/// right for an editor and wrong here: the picker's primary act is typing a
/// filter, and an operator who must press `i` first has been handed a worse
/// chooser than the one they had. `esc` reaches Normal for motions and
/// deletion — the trade fzf's vim mode makes.
#[test]
fn a_filter_box_opens_in_insert_and_escape_reaches_normal() {
    assert_eq!(Vim::for_filter().stance(), Stance::Insert);
    assert_eq!(
        Stance::default(),
        Stance::Normal,
        "the LIBRARY default stays Normal — an editor is not a filter box",
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
    let (_, q, fx) = run("camelot", "jjk");
    assert_eq!(q.text(), "camelot");
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
    let (_, q, fx) = run("camelot-eks", "0dd");
    assert_eq!(q.text(), "camelot-eks", "untouched");
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
/// leaving the picker. `esc` cancels the pending sequence first and only
/// leaves when nothing is armed.
#[test]
fn escape_cancels_a_pending_operator_before_it_cancels_the_screen() {
    let (v, q, fx) = run("alpha", "0d<esc>");
    assert_eq!(q.text(), "alpha", "nothing was deleted");
    assert_eq!(fx.last(), Some(&Effect::Moved), "cancelled, did not leave");
    assert!(v.pending_label().is_none(), "and the badge is clear");

    let (_, _, fx) = run("alpha", "<esc>");
    assert_eq!(fx.last(), Some(&Effect::Cancel), "a resting esc DOES leave");
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
