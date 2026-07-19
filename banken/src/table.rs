//! The `PodTable` model — banken's minimal in-crate resource table
//! (the `:pods` view model).
//!
//! # pending-banken: promote-tableview-to-egaku
//!
//! BANKEN.md §IV/§VII/§VI name a generic `egaku::TableView<Row>` (+
//! `egaku_term::draw::table`) as the load-bearing NET-NEW *egaku* widget
//! banken's identity capability rides on — "land in egaku (P2), never the
//! app, never ratatui." The published `egaku` 0.1.4 has only a flat
//! `ListView` (`Vec<String>`, no columns/sort/payload) and `egaku-term`
//! 0.3.1 has no `draw::table`; promoting a generic `TableView` into those
//! crates needs a publish cycle (a new egaku + egaku-term release) that is
//! out of reach this session (no push / no nix). So the table model +
//! its cell drawer live **in banken for now**, built directly on
//! egaku-term 0.3.1's typed `Buffer`/`Cell`/`Style` surface (★★ TYPED
//! EMISSION — no `format!()` of VT). This is a tier-honest interim, not a
//! silent fork: when egaku gains `TableView<Row>`, this model collapses
//! into a thin adapter over it. The load-bearing engineering (columns,
//! selection, sort, the postigo dispatch) is here and correct either way.
//!
//! The row payload is [`banken_spec::env::Row`] — the OBSERVE read type
//! from the shipped citizenship primitive, reused verbatim (never a
//! parallel row type).

use banken_spec::env::Row;
use banken_spec::types::{Ordering, ResourceKind, SortKey};

/// One resolved column of the table: a header + the [`Row`] cell key it
/// projects. Mirrors `banken_spec::types::ColumnSpec` but resolved for
/// render (the header is what draws; the field is the `Row.cells` key).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Column {
    /// The header text drawn at the top ("NAME", "READY", …).
    pub header: String,
    /// The `Row.cells` key this column reads (`"READY"`, `"STATUS"`, …).
    /// The synthetic `"NAME"` column reads `Row.name` directly.
    pub field: String,
}

impl Column {
    /// Construct a column from a header + field key.
    pub fn new(header: impl Into<String>, field: impl Into<String>) -> Self {
        Self {
            header: header.into(),
            field: field.into(),
        }
    }
}

/// The canonical `:pods` columns (BANKEN.md §III.b `(defk8sview "pods")`):
/// NAME / READY / STATUS / RESTARTS / AGE.
#[must_use]
pub fn pod_columns() -> Vec<Column> {
    vec![
        Column::new("NAME", "NAME"),
        Column::new("READY", "READY"),
        Column::new("STATUS", "STATUS"),
        Column::new("RESTARTS", "RESTARTS"),
        Column::new("AGE", "AGE"),
    ]
}

/// The `:pods` table view model: the observed rows, the column layout,
/// the selected row index, and the active sort.
///
/// This is a pure state machine — no rendering, no IO. The renderer
/// ([`crate::render::draw_pod_table`]) projects it onto an egaku-term
/// `Buffer`; the app runtime drives selection + sort via the methods
/// below. Selection is clamped to the row set on every mutation, so an
/// out-of-range selected index is unrepresentable after construction.
///
/// (`PartialEq` only — `banken_spec::types::SortKey` is `PartialEq` but not
/// `Eq`, so this cannot derive `Eq`; `PartialEq` is all the tests need.)
#[derive(Debug, Clone, PartialEq)]
pub struct PodTable {
    kind: ResourceKind,
    columns: Vec<Column>,
    rows: Vec<Row>,
    selected: usize,
    sort: SortKey,
}

impl PodTable {
    /// A fresh `:pods` table over `rows`, default-sorted by STATUS desc
    /// (so unhealthy pods surface first — the k9s default and the
    /// BANKEN.md `(defk8sview)` `:default-sort (by "STATUS" desc)`).
    #[must_use]
    pub fn pods(rows: Vec<Row>) -> Self {
        let mut t = Self {
            kind: ResourceKind::Pod,
            columns: pod_columns(),
            rows,
            selected: 0,
            sort: SortKey {
                column: "STATUS".into(),
                order: Ordering::Desc,
            },
        };
        t.apply_sort();
        t
    }

    /// The resource kind this table lists.
    #[must_use]
    pub fn kind(&self) -> ResourceKind {
        self.kind
    }

    /// The resolved columns.
    #[must_use]
    pub fn columns(&self) -> &[Column] {
        &self.columns
    }

    /// The current (already-sorted) rows.
    #[must_use]
    pub fn rows(&self) -> &[Row] {
        &self.rows
    }

    /// The selected row index (always in range, or 0 when empty).
    #[must_use]
    pub fn selected_index(&self) -> usize {
        self.selected
    }

    /// The active sort key.
    #[must_use]
    pub fn sort(&self) -> &SortKey {
        &self.sort
    }

    /// The selected [`Row`], if any (`None` only when the table is empty).
    #[must_use]
    pub fn selected_row(&self) -> Option<&Row> {
        self.rows.get(self.selected)
    }

    /// Replace the observed rows (a watch/poll refresh), preserving the
    /// selection by row *name* when possible so a refresh doesn't jump the
    /// cursor. Re-applies the active sort.
    pub fn set_rows(&mut self, rows: Vec<Row>) {
        let selected_name = self.selected_row().map(|r| r.name.clone());
        self.rows = rows;
        self.apply_sort();
        self.selected = match selected_name {
            Some(name) => self.rows.iter().position(|r| r.name == name).unwrap_or(0),
            None => 0,
        };
        self.clamp_selection();
    }

    /// Move the selection down one row (saturating at the last row).
    pub fn select_next(&mut self) {
        if self.rows.is_empty() {
            return;
        }
        self.selected = (self.selected + 1).min(self.rows.len() - 1);
    }

    /// Move the selection up one row (saturating at the first row).
    pub fn select_prev(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }

    /// Cycle the sort direction on the current column (asc ⇄ desc) and
    /// re-sort. A real column-picker lands with the egaku `TableView`
    /// promotion; for M0 the sort column is fixed and only the direction
    /// cycles.
    pub fn toggle_sort_direction(&mut self) {
        self.sort.order = match self.sort.order {
            Ordering::Asc => Ordering::Desc,
            Ordering::Desc => Ordering::Asc,
        };
        let selected_name = self.selected_row().map(|r| r.name.clone());
        self.apply_sort();
        if let Some(name) = selected_name {
            self.selected = self
                .rows
                .iter()
                .position(|r| r.name == name)
                .unwrap_or(self.selected);
        }
        self.clamp_selection();
    }

    /// The projected value of `column` for `row` — the synthetic NAME
    /// column reads `row.name`; every other column reads `row.cells`.
    /// Missing cells render as an empty string (never a panic).
    #[must_use]
    pub fn cell_value<'a>(&self, row: &'a Row, column: &Column) -> &'a str {
        if column.field == "NAME" {
            return &row.name;
        }
        row.cells
            .iter()
            .find(|(k, _)| *k == column.field)
            .map_or("", |(_, v)| v.as_str())
    }

    fn clamp_selection(&mut self) {
        if self.rows.is_empty() {
            self.selected = 0;
        } else if self.selected >= self.rows.len() {
            self.selected = self.rows.len() - 1;
        }
    }

    fn apply_sort(&mut self) {
        let field = self.sort.column.clone();
        let synthetic_name = field == "NAME";
        self.rows.sort_by(|a, b| {
            let av = if synthetic_name {
                a.name.as_str()
            } else {
                a.cells
                    .iter()
                    .find(|(k, _)| *k == field)
                    .map_or("", |(_, v)| v.as_str())
            };
            let bv = if synthetic_name {
                b.name.as_str()
            } else {
                b.cells
                    .iter()
                    .find(|(k, _)| *k == field)
                    .map_or("", |(_, v)| v.as_str())
            };
            let base = av.cmp(bv);
            match self.sort.order {
                Ordering::Asc => base,
                Ordering::Desc => base.reverse(),
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use banken_spec::env::Row;

    fn row(name: &str, status: &str) -> Row {
        Row {
            name: name.into(),
            namespace: Some("catch".into()),
            cells: vec![
                ("READY".into(), "1/1".into()),
                ("STATUS".into(), status.into()),
                ("RESTARTS".into(), "0".into()),
                ("AGE".into(), "5m".into()),
            ],
        }
    }

    #[test]
    fn empty_table_has_zero_selection_and_no_selected_row() {
        let t = PodTable::pods(vec![]);
        assert_eq!(t.selected_index(), 0);
        assert!(t.selected_row().is_none());
    }

    #[test]
    fn default_sort_is_status_desc_so_running_beats_crashloop_reversed() {
        // STATUS desc: "Running" > "Pending" > "CrashLoopBackOff" reversed
        // → alphabetically-largest first. Assert order is deterministic.
        let t = PodTable::pods(vec![
            row("a", "CrashLoopBackOff"),
            row("b", "Running"),
            row("c", "Pending"),
        ]);
        let statuses: Vec<&str> = t
            .rows()
            .iter()
            .map(|r| t.cell_value(r, &Column::new("STATUS", "STATUS")))
            .collect();
        // Desc string sort: Running, Pending, CrashLoopBackOff.
        assert_eq!(statuses, vec!["Running", "Pending", "CrashLoopBackOff"]);
    }

    #[test]
    fn selection_saturates_at_both_ends() {
        let mut t = PodTable::pods(vec![row("a", "Running"), row("b", "Running")]);
        t.select_prev();
        assert_eq!(t.selected_index(), 0);
        t.select_next();
        t.select_next();
        t.select_next();
        assert_eq!(t.selected_index(), 1, "cannot exceed last row");
    }

    #[test]
    fn set_rows_preserves_selection_by_name() {
        let mut t = PodTable::pods(vec![row("a", "Running"), row("b", "Running")]);
        // sorted by name-stable content; select the second row.
        t.select_next();
        let selected = t.selected_row().unwrap().name.clone();
        // refresh with the same set in a different order.
        t.set_rows(vec![row("b", "Running"), row("a", "Running")]);
        assert_eq!(
            t.selected_row().unwrap().name,
            selected,
            "selection follows the row by name across a refresh"
        );
    }

    #[test]
    fn toggle_sort_direction_reverses_order() {
        let mut t = PodTable::pods(vec![
            row("a", "Running"),
            row("b", "Pending"),
            row("c", "CrashLoopBackOff"),
        ]);
        let before: Vec<String> = t.rows().iter().map(|r| r.name.clone()).collect();
        t.toggle_sort_direction();
        let after: Vec<String> = t.rows().iter().map(|r| r.name.clone()).collect();
        assert_ne!(before, after, "sort direction cycle changes row order");
    }

    #[test]
    fn cell_value_reads_name_and_cells_and_missing_is_empty() {
        let t = PodTable::pods(vec![row("catch-0", "Running")]);
        let r = &t.rows()[0];
        assert_eq!(t.cell_value(r, &Column::new("NAME", "NAME")), "catch-0");
        assert_eq!(t.cell_value(r, &Column::new("STATUS", "STATUS")), "Running");
        assert_eq!(
            t.cell_value(r, &Column::new("MISSING", "MISSING")),
            "",
            "a missing cell renders empty, never panics"
        );
    }
}
