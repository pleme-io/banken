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
//!
//! # The columns are AUTHORED, not hardcoded
//!
//! [`PodTable::from_view`] reads the columns, the default sort and the listed
//! resource kind out of the `(defk8sview "pods")` form; [`PodTable::pods`] is
//! the no-catalog fallback and `columns_mirror_the_authored_view` pins it to
//! the authored source so it cannot drift. The authored `:field` is now the
//! actual `Row.cells` join key — see [`Column::field`] for the divergence that
//! closed.

use banken_spec::env::Row;
use banken_spec::types::{Ordering, ResourceKind, SortKey, ViewSource};
use banken_spec::{Catalog, SpecError};

/// The reserved column field that projects a row's identity
/// ([`Row::name`]) rather than one of its cells.
///
/// One name, one place: the authored `(defk8sview)` spells it `:field name`,
/// every reader omits it from `Row.cells`, and this constant is what joins
/// the two. A string literal repeated in `cell_value` and `apply_sort` is how
/// the identity column silently stops resolving.
pub const IDENTITY_FIELD: &str = "name";

/// One resolved column of the table: a header + the [`Row`] cell key it
/// projects. Mirrors `banken_spec::types::ColumnSpec` but resolved for
/// render (the header is what draws; the field is the `Row.cells` key).
///
/// **The `field` is the authored `:field`, and it IS the `Row.cells` key.**
/// Before the vocabulary landed these were two vocabularies for one thing —
/// the authored view said `:field phase` while every reader emitted a cell
/// keyed `"STATUS"` — so the authored field was decorative and a typo in it
/// was invisible. Now a column that names a field no row carries renders
/// empty *and* is reported by [`PodTable::unresolved_fields`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Column {
    /// The header text drawn at the top ("NAME", "READY", …).
    pub header: String,
    /// The `Row.cells` key this column reads (`"ready"`, `"phase"`, …).
    /// [`IDENTITY_FIELD`] reads [`Row::name`] directly.
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
///
/// **A hardcoded MIRROR of the authored view, kept only as the fallback for
/// [`PodTable::pods`]'s test/no-catalog path.** The production path is
/// [`PodTable::from_view`], which reads the columns and the default sort out
/// of the `(defk8sview "pods")` form — because this function and
/// `specs/views.lisp` were two hand-lists of the same five columns, free to
/// disagree. `columns_mirror_the_authored_view` pins them together so the
/// fallback cannot drift from the authored source it stands in for.
#[must_use]
pub fn pod_columns() -> Vec<Column> {
    vec![
        Column::new("NAME", IDENTITY_FIELD),
        Column::new("READY", "ready"),
        Column::new("STATUS", "phase"),
        Column::new("RESTARTS", "restarts"),
        Column::new("AGE", "age"),
    ]
}

/// The `:pods` default sort — a hardcoded mirror of the authored view's
/// `:default-sort`, for the same fallback reason as [`pod_columns`].
#[must_use]
fn pod_default_sort() -> SortKey {
    SortKey {
        column: "STATUS".into(),
        order: Ordering::Desc,
    }
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
            sort: pod_default_sort(),
        };
        t.apply_sort();
        t
    }

    /// Build a table from an **authored** `(defk8sview)` — the production
    /// path.
    ///
    /// The columns, the default sort and the listed resource kind all come
    /// from the spec, so re-spelling a column in `specs/views.lisp` moves the
    /// rendered table with no Rust edit. [`Self::pods`] remains as the
    /// no-catalog fallback, pinned to this by
    /// `columns_mirror_the_authored_view`.
    ///
    /// # Errors
    ///
    /// - [`SpecError::Binding`] when `view_name` names no declared view.
    /// - [`SpecError::Binding`] when the view's `:default-sort` names a column
    ///   the view does not declare. Previously that sorted by a cell key no
    ///   row carries — every row compared equal, so the table came out in
    ///   whatever order the reader happened to return. A silently arbitrary
    ///   sort is worse than a refusal.
    /// - [`SpecError::Binding`] when the view's `:source` is not a resource
    ///   kind (a health/topology view has no resource table to build).
    pub fn from_view(
        catalog: &Catalog,
        view_name: &str,
        rows: Vec<Row>,
    ) -> Result<Self, SpecError> {
        let view = catalog
            .views()
            .iter()
            .find(|v| v.name == view_name)
            .ok_or_else(|| binding_error("no (defk8sview) is named `", view_name, "`"))?;

        let ViewSource::Resource(kind) = view.source else {
            return Err(binding_error(
                "view `",
                view_name,
                "` does not read a resource kind, so it has no resource table",
            ));
        };

        let columns: Vec<Column> = view
            .columns
            .iter()
            .map(|c| Column::new(c.header.clone(), c.field.clone()))
            .collect();

        // The default sort names a HEADER; it must resolve to a declared
        // column or the sort silently degenerates.
        if !columns.iter().any(|c| c.header == view.default_sort.column) {
            return Err(binding_error(
                "view `",
                view_name,
                {
                    let mut m = String::from("`'s :default-sort names column `");
                    m.push_str(&view.default_sort.column);
                    m.push_str("`, which the view does not declare");
                    m
                }
                .as_str(),
            ));
        }

        let mut t = Self {
            kind,
            columns,
            rows,
            selected: 0,
            sort: view.default_sort.clone(),
        };
        t.apply_sort();
        Ok(t)
    }

    /// The column fields no observed row carries — a declared column that
    /// will always render empty.
    ///
    /// Reported as data rather than silence. It is deliberately NOT an error:
    /// a legitimately-absent cell exists (the live reader emits `AGE` as `-`
    /// only because it has no clock, and a kind-specific column may be absent
    /// on some rows), so the honest surface is a *report* the caller can show
    /// or assert on, not a refusal that would make banken unusable against a
    /// partially-populated read.
    #[must_use]
    pub fn unresolved_fields(&self) -> Vec<&str> {
        self.columns
            .iter()
            .filter(|c| c.field != IDENTITY_FIELD)
            .filter(|c| {
                !self
                    .rows
                    .iter()
                    .any(|r| r.cells.iter().any(|(k, _)| *k == c.field))
            })
            .map(|c| c.field.as_str())
            .collect()
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

    /// The projected value of `column` for `row` — the [`IDENTITY_FIELD`]
    /// column reads `row.name`; every other column reads `row.cells` by the
    /// authored field. Missing cells render as an empty string (never a
    /// panic); [`Self::unresolved_fields`] is what surfaces a column that is
    /// *always* empty.
    #[must_use]
    pub fn cell_value<'a>(&self, row: &'a Row, column: &Column) -> &'a str {
        if column.field == IDENTITY_FIELD {
            return &row.name;
        }
        row.cells
            .iter()
            .find(|(k, _)| *k == column.field)
            .map_or("", |(_, v)| v.as_str())
    }

    /// The authored field the active sort projects, resolved through the
    /// declared columns (`sort.column` is a HEADER, `Row.cells` is keyed by
    /// FIELD — resolving is what joins the two).
    fn sort_field(&self) -> String {
        self.columns
            .iter()
            .find(|c| c.header == self.sort.column)
            .map_or_else(|| self.sort.column.clone(), |c| c.field.clone())
    }

    fn clamp_selection(&mut self) {
        if self.rows.is_empty() {
            self.selected = 0;
        } else if self.selected >= self.rows.len() {
            self.selected = self.rows.len() - 1;
        }
    }

    fn apply_sort(&mut self) {
        let field = self.sort_field();
        let synthetic_name = field == IDENTITY_FIELD;
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

/// A [`SpecError::Binding`] built from three typed pieces — a typed join, not
/// a `format!()` of a message template (★★ TYPED EMISSION).
fn binding_error(prefix: &str, name: &str, suffix: &str) -> SpecError {
    let mut m = String::from(prefix);
    m.push_str(name);
    m.push_str(suffix);
    SpecError::Binding(m)
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
                ("ready".into(), "1/1".into()),
                ("phase".into(), status.into()),
                ("restarts".into(), "0".into()),
                ("age".into(), "5m".into()),
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
            .map(|r| t.cell_value(r, &Column::new("STATUS", "phase")))
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
        assert_eq!(
            t.cell_value(r, &Column::new("NAME", IDENTITY_FIELD)),
            "catch-0"
        );
        assert_eq!(t.cell_value(r, &Column::new("STATUS", "phase")), "Running");
        assert_eq!(
            t.cell_value(r, &Column::new("MISSING", "missing")),
            "",
            "a missing cell renders empty, never panics"
        );
    }
}
