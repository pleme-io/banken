//! The `:pods` table cell drawer — banken's `draw::table` **with the
//! pathology color axis**.
//!
//! # This drawer is KEPT ON PURPOSE. `pending-banken: column-render-hints`
//!
//! egaku-term `735d936` shipped a generic `draw::table` / `draw::table_with`
//! over `egaku::TableView<R>`, lifted from this file, and banken's *model*
//! collapsed onto the matching `egaku::TableView` (see [`crate::table`]). This
//! drawer did **not** collapse with it, and the reason is one feature:
//! [`status_style`]. A STATUS cell reading `CrashLoopBackOff` draws red and one
//! reading `Running` draws green — the pathology color axis (BANKEN.md §V) at
//! the cell level, and the single most load-bearing glyph in a k9s-shaped
//! table.
//!
//! That coloring is **app knowledge and was deliberately not lifted**: it is
//! keyed on the magic header `"STATUS"` and on a hardcoded list of Kubernetes
//! phase strings, neither of which a generic table drawer should know. Adopting
//! `draw::table_with` today would trade a real operator-facing signal for
//! nothing, so the honest move is to keep the drawer and keep the token open.
//!
//! **The destination is unchanged and is what closes this:** a typed
//! `:colorize` hint on the authored `ColumnSpec`, carried into
//! `egaku::Column`, honored by `egaku_term::draw::table_with`. Then the magic
//! header string is not the selector at all, this module deletes, and the call
//! becomes one line. That needs an egaku + egaku-term change; banken must not
//! make one from this repo.
//!
//! # What was taken from the lifted drawer anyway
//!
//! The **bottom-anchored viewport**. `TableView` carries no scroll offset — it
//! is the ordered row set plus a cursor — so a drawer with fewer rows of screen
//! than the table has rows must derive its own window. This one now does,
//! exactly as `egaku_term::draw::table_with` does: `first_visible` is a pure
//! function of `(selected_index, height)`, so no new state exists to be wrong
//! in a second place. Before this, banken drew rows from index 0 and stopped at
//! the bottom edge: on a cluster with more pods than the terminal has rows,
//! `j` moved the cursor **off screen** and the operator was navigating blind.
//! That was a live bug against camelot-eks, not a hypothetical.
//!
//! # What was NOT taken, and why that is inert here
//!
//! The lifted drawer measures column widths with `unicode_width`; this one
//! counts `chars()`. The two disagree only on wide characters (CJK, emoji),
//! and **no value banken projects can be one**: `NAME` is a Kubernetes object
//! name (RFC-1123: lowercase alphanumeric, `-`, `.`), and `READY` / `STATUS` /
//! `RESTARTS` / `AGE` are reader-generated ASCII. Adding a second unicode-width
//! implementation to a crate that cannot exercise it would be duplication with
//! no defect behind it — the fix arrives for free when this drawer deletes.
//!
//! Every visible glyph is still a typed `Buffer`/`Cell` write on egaku-term's
//! published surface — zero raw VT, no `format!()` of a styled string
//! (★★ TYPED EMISSION / Quadro P5). The runtime diffs the buffer so only
//! changed cells flush.

use egaku::SortOrder;
use egaku_term::crossterm::style::Color;
use egaku_term::{Buffer, Style};

use crate::table::PodTable;

/// Two spaces of gutter between columns.
const COL_GAP: u16 = 2;

/// Compute the render width (in cells) of every column: the max of the
/// header and every projected cell value, so the columns line up.
fn column_widths(table: &PodTable) -> Vec<u16> {
    let view = table.view();
    view.columns()
        .iter()
        .map(|col| {
            let mut w = col.header.chars().count();
            for row in view.rows() {
                w = w.max(view.cell_value(row, col).chars().count());
            }
            u16::try_from(w).unwrap_or(u16::MAX)
        })
        .collect()
}

/// The status-colored style for a STATUS cell value. Healthy states draw
/// success-green; failing/pending states draw error-red / warning-yellow.
/// This is the pathology color axis (BANKEN.md §V) at the cell level.
fn status_style(value: &str, base: Style) -> Style {
    match value {
        "Running" | "Completed" | "Succeeded" => base.fg(Color::Green),
        "CrashLoopBackOff" | "Error" | "Failed" | "ImagePullBackOff" | "OOMKilled" => {
            base.fg(Color::Red)
        }
        "Pending" | "ContainerCreating" | "Terminating" | "Init" => base.fg(Color::Yellow),
        _ => base,
    }
}

/// Draw the `:pods` table into `buf` within the rectangle
/// `(x, y, width, height)` (cell coordinates). Row `y` is the header;
/// `y + 1` is a rule; rows start at `y + 2`. The selected row is drawn
/// full-width reverse-video, and the visible window is bottom-anchored on the
/// selection so the cursor is always on screen. Returns nothing — pure
/// emission into the typed buffer.
pub fn draw_pod_table(buf: &mut Buffer, x: u16, y: u16, width: u16, height: u16, table: &PodTable) {
    if width == 0 || height == 0 {
        return;
    }

    let view = table.view();
    let widths = column_widths(table);
    let header_style = Style::default().fg(Color::Cyan).bold();

    // ── Header row ──
    draw_row_cells(
        buf,
        x,
        y,
        width,
        table,
        &widths,
        header_style,
        RowContent::Header,
    );

    // ── Rule under the header ──
    if height >= 2 {
        buf.hline(x, y + 1, width, '─', Style::default().fg(Color::DarkGrey));
    }

    // ── Data rows ──
    let visible_rows = usize::from(height.saturating_sub(2));
    if visible_rows == 0 {
        return;
    }
    // Bottom-anchored viewport: keep the selection on screen with no stored
    // offset. See the module docs — this is `egaku_term::draw::table_with`'s
    // derivation, and it is a pure function of (selected, height).
    let first_visible = view
        .selected_index()
        .saturating_sub(visible_rows.saturating_sub(1));
    let first_data_row = y + 2;

    for (offset, row) in view
        .rows()
        .iter()
        .skip(first_visible)
        .take(visible_rows)
        .enumerate()
    {
        let Ok(row_i) = u16::try_from(offset) else {
            break;
        };
        let ry = first_data_row + row_i;
        let is_selected = first_visible + offset == view.selected_index();

        let base = if is_selected {
            // Full-width highlight bar first so padding to the right of the
            // last column carries the selection background.
            let sel = Style::default().fg(Color::Black).bg(Color::Cyan);
            buf.blank(x, ry, width, sel);
            sel
        } else {
            Style::default()
        };

        // A data row draws the pod's cell values. STATUS is pathology-
        // colored ONLY on unselected rows — on the selected row the
        // reverse-video bar owns the whole palette (`colorize_status:
        // false`), so the highlight is not overwritten by a status fg.
        draw_row_cells(
            buf,
            x,
            ry,
            width,
            table,
            &widths,
            base,
            RowContent::Data {
                row,
                colorize_status: !is_selected,
            },
        );
    }
}

/// What one row draws — a typed distinction so "header" and "a selected
/// data row" are never confused (the earlier `Option<&Row>` overloaded
/// `None` to mean both, which drew the header text onto the selected row).
enum RowContent<'a> {
    /// The column-header row: each column's `header` text.
    Header,
    /// A data row: the pod's cell values. `colorize_status` gates the
    /// per-cell STATUS pathology coloring (off for the selected row).
    Data {
        row: &'a banken_spec::env::Row,
        colorize_status: bool,
    },
}

/// Draw one row of cells left-to-right, each padded to its column width
/// with `COL_GAP` between. All writes are typed `Buffer` ops — no
/// `format!()` of VT.
fn draw_row_cells(
    buf: &mut Buffer,
    x: u16,
    y: u16,
    width: u16,
    table: &PodTable,
    widths: &[u16],
    base: Style,
    content: RowContent<'_>,
) {
    let view = table.view();
    let right_edge = x.saturating_add(width);
    let mut cx = x;
    for (col, &cw) in view.columns().iter().zip(widths.iter()) {
        if cx >= right_edge {
            break;
        }
        let remaining = right_edge.saturating_sub(cx);

        let (value, style) = match &content {
            RowContent::Header => (col.header.clone(), base),
            RowContent::Data {
                row,
                colorize_status,
            } => {
                let v = view.cell_value(row, col).to_string();
                // Keyed on the HEADER, not the field: the header is the
                // operator-facing name and is stable across readers, while
                // the field is the reader's join key. `pending-banken:
                // column-render-hints` — the destination is a typed
                // `:colorize` hint on the authored `ColumnSpec` so a magic
                // header string is not the selector at all, and this whole
                // module collapses into `egaku_term::draw::table_with`.
                let s = if *colorize_status && col.header == "STATUS" {
                    status_style(&v, base)
                } else {
                    base
                };
                (v, s)
            }
        };

        buf.set_stringn(cx, y, &value, cw.min(remaining), style);
        cx = cx.saturating_add(cw).saturating_add(COL_GAP);
    }
}

/// A short human label for the active sort — drawn into the status line.
#[must_use]
pub fn sort_label(table: &PodTable) -> String {
    let arrow = match table.view().sort().order {
        SortOrder::Asc => "↑",
        SortOrder::Desc => "↓",
    };
    let mut s = String::new();
    s.push_str("sort:");
    s.push_str(&table.view().sort().column);
    s.push(' ');
    s.push_str(arrow);
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use banken_spec::env::Row;
    use egaku_term::TestBackend;

    fn row(name: &str, status: &str) -> Row {
        Row {
            uid: banken_spec::env::Uid::new(format!("t-{name}")).expect("non-blank"),
            version: None,
            name: name.into(),
            namespace: Some("catch".into()),
            cells: vec![
                ("ready".into(), "1/1".into()),
                ("phase".into(), status.into()),
                ("restarts".into(), "0".into()),
                ("age".into(), "5m".into()),
            ],
        }
    }

    #[test]
    fn header_row_carries_the_column_names() {
        let table = PodTable::pods(vec![row("catch-0", "Running")]);
        let mut backend = TestBackend::new(60, 6);
        backend.draw(|buf| draw_pod_table(buf, 0, 0, 60, 6, &table));
        let lines = backend.to_lines();
        // The header (row 0) contains every column name in order.
        let header = &lines[0];
        let name_at = header.find("NAME").expect("NAME header present");
        let ready_at = header.find("READY").expect("READY header present");
        let status_at = header.find("STATUS").expect("STATUS header present");
        assert!(
            name_at < ready_at && ready_at < status_at,
            "columns ordered"
        );
    }

    #[test]
    fn selected_row_is_reverse_video_via_cell_style() {
        let table = PodTable::pods(vec![row("catch-0", "Running")]);
        let mut backend = TestBackend::new(60, 6);
        backend.draw(|buf| draw_pod_table(buf, 0, 0, 60, 6, &table));
        // Row 0 = header, row 1 = rule, row 2 = first (selected) data row.
        let cell = backend.cell(0, 2).expect("first data cell exists");
        assert_eq!(cell.bg, Color::Cyan, "selected row has the highlight bg");
    }

    #[test]
    fn selected_data_row_shows_the_pod_name_not_the_header() {
        // Regression: an earlier `Option<&Row>` overloaded `None` to mean
        // both "header row" AND "selected data row (no status color)", so
        // the selected row drew the column headers instead of the pod. The
        // selected data row (frame row 2) MUST show the pod's NAME.
        let table = PodTable::pods(vec![row("catch-0", "Running")]);
        let mut backend = TestBackend::new(60, 6);
        backend.draw(|buf| draw_pod_table(buf, 0, 0, 60, 6, &table));
        let data_row = &backend.to_lines()[2];
        assert!(
            data_row.contains("catch-0"),
            "selected data row shows the pod, got {data_row:?}",
        );
        assert!(
            !data_row.starts_with("NAME"),
            "selected data row is NOT the header, got {data_row:?}",
        );
    }

    /// **THE VIEWPORT GATE.** A table taller than its rect must scroll the
    /// selection into view, not clip it. Before the bottom-anchored window was
    /// ported from `egaku_term::draw::table_with`, `j` past the last visible
    /// row moved the cursor somewhere the operator could not see.
    #[test]
    fn the_viewport_scrolls_to_keep_the_selection_visible() {
        // 12 rows, 4 rows of frame => 2 data rows on screen.
        let rows: Vec<Row> = (0..12)
            .map(|i| {
                let mut name = String::from("pod-");
                name.push(char::from(b'a' + i));
                row(&name, "Running")
            })
            .collect();
        let mut table = PodTable::pods(rows);
        for _ in 0..11 {
            table.view_mut().select_next();
        }
        let selected = table.view().selected_row().unwrap().name.clone();

        let mut backend = TestBackend::new(60, 4);
        backend.draw(|buf| draw_pod_table(buf, 0, 0, 60, 4, &table));
        let frame = backend.to_lines().join("\n");
        assert!(
            frame.contains(&selected),
            "the selected row must be on screen, got {frame:?}",
        );
    }

    #[test]
    fn sort_label_shows_column_and_direction() {
        let table = PodTable::pods(vec![row("catch-0", "Running")]);
        let label = sort_label(&table);
        assert!(label.contains("STATUS"));
        assert!(label.contains('↓'), "default is descending");
    }

    /// The pathology color axis is exactly what keeps this drawer alive
    /// instead of collapsing into `egaku_term::draw::table_with`. If this
    /// stops holding, the reason to keep the module is gone.
    #[test]
    fn an_unhealthy_status_draws_red_on_an_unselected_row() {
        let table = PodTable::pods(vec![row("aaa", "CrashLoopBackOff"), row("bbb", "Running")]);
        let mut backend = TestBackend::new(60, 8);
        backend.draw(|buf| draw_pod_table(buf, 0, 0, 60, 8, &table));
        let lines = backend.to_lines();
        // STATUS desc puts "Running" first (selected), CrashLoopBackOff second.
        let crash_row = lines
            .iter()
            .position(|l| l.contains("CrashLoopBackOff"))
            .expect("the unhealthy row is drawn");
        let col = u16::try_from(lines[crash_row].find("CrashLoopBackOff").unwrap()).unwrap();
        let cell = backend
            .cell(col, u16::try_from(crash_row).unwrap())
            .expect("the status cell exists");
        assert_eq!(
            cell.fg,
            Color::Red,
            "CrashLoopBackOff must draw red — this is the whole reason \
             banken keeps its own table drawer",
        );
    }
}
