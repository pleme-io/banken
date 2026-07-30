//! The `:pods` table cell drawer — banken's minimal `draw::table`.
//!
//! # pending-banken: promote-tableview-to-egaku
//!
//! This is the cell-drawer half of the [`crate::table`] interim (see that
//! module's note): a generic `egaku_term::draw::table` belongs in
//! egaku-term (P2, sibling of `draw::list`/`draw::split`), but adding it
//! needs an egaku-term publish cycle out of reach this session. So the
//! drawer lives in banken for now, built **only** on egaku-term 0.3.1's
//! published typed surface — `Buffer::set_stringn` / `Buffer::blank` /
//! `Style` / `Cell`. It emits **zero** raw VT and never `format!()`s a
//! styled string (★★ TYPED EMISSION / Quadro P5): every visible glyph is a
//! typed `Cell` write, and the runtime diffs the buffer so only changed
//! cells flush (no full-clear, no flicker on a no-change tick).
//!
//! When egaku-term gains `draw::table`, this collapses to a one-line call.

use banken_spec::types::Ordering;
use egaku_term::crossterm::style::Color;
use egaku_term::{Buffer, Style};

use crate::table::PodTable;

/// Two spaces of gutter between columns.
const COL_GAP: u16 = 2;

/// Compute the render width (in cells) of every column: the max of the
/// header and every projected cell value, so the columns line up.
fn column_widths(table: &PodTable) -> Vec<u16> {
    table
        .columns()
        .iter()
        .map(|col| {
            let mut w = col.header.chars().count();
            for row in table.rows() {
                w = w.max(table.cell_value(row, col).chars().count());
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
/// full-width reverse-video. Returns nothing — pure emission into the
/// typed buffer.
pub fn draw_pod_table(buf: &mut Buffer, x: u16, y: u16, width: u16, height: u16, table: &PodTable) {
    if width == 0 || height == 0 {
        return;
    }

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
    let first_data_row = y + 2;
    let visible_rows = height.saturating_sub(2);
    for (idx, row) in table.rows().iter().enumerate() {
        let row_i = u16::try_from(idx).unwrap_or(u16::MAX);
        if row_i >= visible_rows {
            break;
        }
        let ry = first_data_row + row_i;
        let is_selected = idx == table.selected_index();

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
    let right_edge = x.saturating_add(width);
    let mut cx = x;
    for (col, &cw) in table.columns().iter().zip(widths.iter()) {
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
                let v = table.cell_value(row, col).to_string();
                // Keyed on the HEADER, not the field: the header is the
                // operator-facing name and is stable across readers, while
                // the field is the reader's join key. `pending-banken:
                // column-render-hints` — the destination is a typed
                // `:colorize` hint on the authored `ColumnSpec` so a magic
                // header string is not the selector at all.
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
    let arrow = match table.sort().order {
        Ordering::Asc => "↑",
        Ordering::Desc => "↓",
    };
    let mut s = String::new();
    s.push_str("sort:");
    s.push_str(&table.sort().column);
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

    #[test]
    fn sort_label_shows_column_and_direction() {
        let table = PodTable::pods(vec![row("catch-0", "Running")]);
        let label = sort_label(&table);
        assert!(label.contains("STATUS"));
        assert!(label.contains('↓'), "default is descending");
    }
}
