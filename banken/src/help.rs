//! The help overlay — one terminal FACE of [`banken_spec::help::HelpPage`].
//!
//! The page itself is typed data built from the authored catalog and lives in
//! `banken-spec`; this module only turns it into cells. That split is the
//! whole reason the overlay and `--help` cannot drift: they render the *same
//! value*, so a chord can no longer be documented one way on stdout and
//! another way on screen. (banken's `--help` really did advertise `S` for a
//! chord the runtime bound as `shift+s`, because the two were built
//! separately.)
//!
//! Colour is the one thing added here, and it is added for a reason rather
//! than for decoration: the `postigo` gate column is the single most important
//! glyph on the page — it is what tells an operator whether a key reads or
//! acts — so it draws in the same palette the table's STATUS column uses.
//!
//! Every glyph is a typed `Buffer` write; no `format!()` of VT
//! (★★ TYPED EMISSION).

use banken_spec::help::{HelpEntry, HelpPage};
use egaku_term::crossterm::style::Color;
use egaku_term::{Buffer, Style};

/// Rows the overlay reserves for its own frame: a title line and a footer.
const CHROME: u16 = 2;

/// The colour of a `postigo` gate label — the same three-class palette the
/// STATUS column uses, so "this key acts" reads the same way "this pod is
/// broken" does.
fn gate_style(gate: &str) -> Style {
    match gate {
        "OBSERVE" => Style::default().fg(Color::Green),
        "DECLARE" => Style::default().fg(Color::Yellow),
        "BREAK-GLASS" => Style::default().fg(Color::Red),
        // An unrecognised gate draws neutral rather than green: the failure
        // mode to avoid is a gate banken cannot classify LOOKING like the
        // safe one.
        _ => Style::default().fg(Color::White),
    }
}

/// One rendered line of the page, kept as a typed pair so the drawer never
/// re-parses text it just produced.
enum Line<'a> {
    /// A section heading + its blurb.
    Heading { title: &'a str, blurb: &'a str },
    /// A blank spacer between sections.
    Blank,
    /// One entry.
    Entry(&'a HelpEntry),
}

/// Flatten the page into drawable lines.
///
/// Built fresh per frame rather than cached: it is a few dozen borrows of
/// data the page already owns, and a cache here would be a second copy of the
/// page that could fall out of step with it.
fn lines(page: &HelpPage) -> Vec<Line<'_>> {
    let mut out = Vec::new();
    for section in page.sections() {
        if section.entries.is_empty() {
            continue;
        }
        if !out.is_empty() {
            out.push(Line::Blank);
        }
        out.push(Line::Heading {
            title: section.title,
            blurb: section.blurb,
        });
        out.extend(section.entries.iter().map(Line::Entry));
    }
    out
}

/// How far the page can scroll in a viewport `height` rows tall.
///
/// A pure function of `(page, height)` — there is no stored maximum to fall
/// out of step with the content, which is the same reasoning behind the pod
/// table's bottom-anchored viewport.
#[must_use]
pub fn max_scroll(page: &HelpPage, height: u16) -> usize {
    let body = usize::from(height.saturating_sub(CHROME));
    lines(page).len().saturating_sub(body)
}

/// Draw the help page over the whole frame.
///
/// Full-screen rather than a panel: the page is longer than any half-screen
/// overlay can hold, and a help screen that itself needs scrolling *inside* a
/// box is a worse answer than one that simply owns the screen while it is up.
pub fn draw_help(buf: &mut Buffer, width: u16, height: u16, page: &HelpPage, scroll: usize) {
    if width == 0 || height == 0 {
        return;
    }
    // Opaque: the table underneath must not show through, or the operator
    // reads a mix of two screens.
    for y in 0..height {
        buf.blank(0, y, width, Style::default());
    }

    let all = lines(page);
    let body = usize::from(height.saturating_sub(CHROME));
    // Clamped here as well as by the caller: `max_scroll` moves with the
    // terminal size, and a resize between keystroke and frame must not scroll
    // past the end.
    let first = scroll.min(all.len().saturating_sub(body));

    // ── Title ──
    buf.set_stringn(
        0,
        0,
        "banken 番犬  —  help  (the authored vocabulary)",
        width,
        Style::default().fg(Color::Cyan).bold(),
    );
    let pos = scroll_label(first, all.len().saturating_sub(body));
    let pos_w = u16::try_from(pos.chars().count()).unwrap_or(0);
    if pos_w < width {
        buf.set_stringn(
            width - pos_w,
            0,
            &pos,
            pos_w,
            Style::default().fg(Color::DarkGrey),
        );
    }

    // ── Body ──
    for (i, line) in all.iter().skip(first).take(body).enumerate() {
        let Ok(row) = u16::try_from(i) else { break };
        draw_line(buf, row + 1, width, line);
    }

    // ── Footer ──
    let bar = Style::default().fg(Color::Black).bg(Color::DarkGrey);
    let y = height - 1;
    buf.blank(0, y, width, bar);
    buf.set_stringn(0, y, &footer(page), width, bar);
}

/// Column the gate/subject text starts at, so entries line up down the page.
/// The alignment is what makes the page scannable rather than a wall.
const KEY_COL: u16 = 18;

fn draw_line(buf: &mut Buffer, y: u16, width: u16, line: &Line<'_>) {
    match line {
        Line::Blank => {}
        Line::Heading { title, blurb } => {
            let x = buf.set_stringn(0, y, title, width, Style::default().fg(Color::Cyan).bold());
            let x = buf.set_stringn(x, y, "  —  ", width.saturating_sub(x), Style::default());
            buf.set_stringn(
                x,
                y,
                blurb,
                width.saturating_sub(x),
                Style::default().fg(Color::DarkGrey),
            );
        }
        Line::Entry(e) => {
            // The chord column, bold — it is what the eye scans for.
            let keys = e.keys.as_deref().unwrap_or("");
            let mut x =
                buf.set_stringn(2, y, keys, width.saturating_sub(2), Style::default().bold());
            // Pad to the shared column; a chord longer than it gets one
            // space rather than a wrapped line.
            if x < KEY_COL {
                x = KEY_COL;
            } else {
                x = x.saturating_add(1);
            }
            if let Some(gate) = e.gate {
                x = buf.set_stringn(x, y, gate, width.saturating_sub(x), gate_style(gate));
                x = buf.set_stringn(x, y, " — ", width.saturating_sub(x), Style::default());
            }
            x = buf.set_stringn(x, y, &e.subject, width.saturating_sub(x), Style::default());
            if !e.detail.is_empty() {
                x = buf.set_stringn(x, y, "  ", width.saturating_sub(x), Style::default());
                buf.set_stringn(
                    x,
                    y,
                    &e.detail,
                    width.saturating_sub(x),
                    Style::default().fg(Color::DarkGrey),
                );
            }
        }
    }
}

/// `"12/40"` — where in the page the viewport sits, so a reader knows there
/// is more below rather than assuming the page ends at the fold.
fn scroll_label(first: usize, max: usize) -> String {
    if max == 0 {
        return String::from(" all ");
    }
    let mut s = String::from(" ");
    s.push_str(&first.to_string());
    s.push('/');
    s.push_str(&max.to_string());
    s.push(' ');
    s
}

/// The footer, with its chords read from the page itself.
///
/// The page is derived from the authored catalog, so the keys that close and
/// scroll the help screen are named by the same source that filled it — a
/// help screen whose own footer is a hand-written literal would be the first
/// thing to rot.
fn footer(page: &HelpPage) -> String {
    let mut s = String::from(" ");
    s.push_str(&primary_chord(page, "select-prev"));
    s.push('/');
    s.push_str(&primary_chord(page, "select-next"));
    s.push_str(": scroll  ·  ");
    // Both closers, because they are genuinely different affordances: the
    // help chord is what an operator presses again by reflex, `escape` is
    // what closes every other overlay in banken.
    s.push_str(&primary_chord(page, "help"));
    s.push_str(" / ");
    s.push_str(&primary_chord(page, "dismiss"));
    s.push_str(": close  ·  ");
    s.push_str(&primary_chord(page, "quit"));
    s.push_str(": quit banken ");
    s
}

/// The FIRST chord the page lists for a navigation intent.
///
/// First, not all of them: the page groups an intent's chords (`down / j`),
/// and splicing a whole group into a footer slot produced
/// `up / k/down / j: scroll` — measured in a 120-column PTY. Authored order
/// puts the conventional chord first, so the head of the group is the one to
/// advertise; every chord in the group still works.
fn primary_chord(page: &HelpPage, intent: &str) -> String {
    page.section_for(banken_spec::help::HelpTopic::Navigation)
        .entries
        .iter()
        .find(|e| e.subject == intent)
        .and_then(|e| e.keys.as_deref())
        .and_then(|keys| keys.split(" / ").next())
        .map_or_else(|| String::from("?"), str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;
    use banken_spec::help::Wiring;
    use egaku_term::TestBackend;

    fn page() -> HelpPage {
        let catalog = banken_spec::load_catalog().expect("the shipped vocabulary must resolve");
        HelpPage::build(&catalog, Wiring::default())
    }

    fn frame(w: u16, h: u16, scroll: usize) -> String {
        let p = page();
        let mut backend = TestBackend::new(w, h);
        backend.draw(|buf| draw_help(buf, w, h, &p, scroll));
        backend.to_lines().join("\n")
    }

    /// The overlay shows the authored vocabulary — not a hand-written blurb.
    #[test]
    fn the_overlay_renders_the_authored_forms() {
        let f = frame(110, 60, 0);
        for expected in [
            "NAVIGATION",
            "ACTIONS",
            "view-logs",
            "OBSERVE",
            "select-next",
        ] {
            assert!(f.contains(expected), "missing `{expected}`:\n{f}");
        }
    }

    /// **The gate column must be legible**, because it is what says whether a
    /// key reads or acts. BREAK-GLASS draws red for the same reason
    /// `CrashLoopBackOff` does.
    #[test]
    fn a_break_glass_gate_draws_red() {
        let p = page();
        let mut backend = TestBackend::new(110, 60);
        backend.draw(|buf| draw_help(buf, 110, 60, &p, 0));
        let lines = backend.to_lines();
        let row = lines
            .iter()
            .position(|l| l.contains("BREAK-GLASS"))
            .expect("a BREAK-GLASS entry is on the page");
        let col = u16::try_from(lines[row].find("BREAK-GLASS").unwrap()).unwrap();
        let cell = backend
            .cell(col, u16::try_from(row).unwrap())
            .expect("the gate cell exists");
        assert_eq!(cell.fg, Color::Red, "a live-effect gate must draw red");
    }

    /// Scrolling reaches content the first screen cannot show. Without this
    /// the later sections would exist in the data and be unreachable on a
    /// normal-height terminal.
    #[test]
    fn scrolling_reveals_the_lower_sections() {
        let short = frame(110, 12, 0);
        assert!(!short.contains("PATHOLOGIES"), "not on the first screen");
        let scrolled = frame(110, 12, max_scroll(&page(), 12));
        assert!(
            scrolled.contains("PATHOLOGIES"),
            "the tail must be reachable:\n{scrolled}",
        );
    }

    /// The viewport is clamped, so an over-scroll shows the end rather than
    /// blank rows.
    #[test]
    fn an_overscroll_lands_on_the_last_page_not_on_nothing() {
        let f = frame(110, 12, 10_000);
        assert!(
            f.lines().filter(|l| !l.trim().is_empty()).count() > 4,
            "an over-scrolled page must still show content:\n{f}",
        );
    }

    /// The footer names the keys that scroll and close — read from the page,
    /// so it cannot advertise a chord the vocabulary does not declare.
    #[test]
    fn the_footer_chords_come_from_the_authored_vocabulary() {
        let f = footer(&page());
        assert!(f.contains("escape"), "close chord: {f}");
        assert!(f.contains('q'), "quit chord: {f}");
        assert!(!f.contains('?'), "every advertised chord resolves: {f}");
    }

    /// **A footer slot takes ONE chord.** Splicing a whole intent group into
    /// it produced `up / k/down / j: scroll` — measured in a 120-column PTY,
    /// and unreadable.
    #[test]
    fn the_footer_advertises_one_chord_per_slot() {
        let p = page();
        assert_eq!(primary_chord(&p, "select-next"), "down");
        assert_eq!(primary_chord(&p, "help"), "h");
        // The group really does hold more than one, so this is not vacuous.
        assert!(
            p.section_for(banken_spec::help::HelpTopic::Navigation)
                .entries
                .iter()
                .any(|e| e.keys.as_deref().is_some_and(|k| k.contains(" / "))),
            "the probe needs a multi-chord intent to be meaningful",
        );
    }

    /// A tiny terminal must not panic.
    #[test]
    fn a_tiny_terminal_still_renders() {
        for (w, h) in [(20_u16, 5_u16), (8, 3), (1, 1)] {
            let _ = frame(w, h, 0);
        }
    }
}
