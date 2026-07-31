//! The `PodTable` binding — banken's *pod-specific* half of a generic
//! [`egaku::TableView`].
//!
//! # `pending-banken: promote-tableview-to-egaku` — CLOSED 2026-07-31
//!
//! BANKEN.md §IV/§VII/§VI named a generic `egaku::TableView<Row>` as the
//! load-bearing NET-NEW egaku widget banken's identity capability rides on
//! — "land in egaku (P2), never the app, never ratatui." It landed
//! (egaku `e602369`), lifted from the model that used to live in this file,
//! and this module is now what the token always said it would be: a thin
//! adapter over it.
//!
//! **What moved out** (~167 lines): the columns, the identity-field join,
//! selection, identity-preserving refresh, header→field sort resolution, the
//! direction cycle, and the unresolved-field report — every one of them
//! generic over a row type, none of them about Kubernetes.
//!
//! **What stayed here**, because it is app knowledge and nothing else:
//! [`pod_columns`] / [`pod_default_sort`] (the `:pods` layout), the
//! [`ResourceKind`] this table lists, and [`PodTable::from_view`] — the
//! reader that turns an authored `(defk8sview "pods")` form into that layout.
//!
//! # One invariant got STRICTER on the way out
//!
//! `from_view` used to be the only constructor that checked the default sort
//! resolves to a declared column, so [`PodTable::pods`] could not have been
//! wrong but nothing said so. [`egaku::TableView::new`] performs that check on
//! the **only** constructor, so the local check is gone as redundant rather
//! than merely duplicated. What `from_view` still owns is the check egaku
//! cannot make: that the view's `:source` is a resource kind at all.
//!
//! Tier is unchanged and is worth restating so the collapse does not read as a
//! promotion: parse-time-rejected (a `Result` at the boundary), not
//! truly-unrepresentable. A [`SortKey`] naming a column no view declares can
//! still be *written*; it just cannot be *installed*.
//!
//! The row payload is [`banken_spec::env::Row`] — the OBSERVE read type from
//! the shipped citizenship primitive, reused verbatim (never a parallel row
//! type). Its [`egaku::TableRow`] impl lives in `banken-spec` because the
//! orphan rule puts it there; see `banken_spec::env`.

use banken_spec::env::Row;
use banken_spec::types::{Ordering, ResourceKind, ViewSource};
use banken_spec::{Catalog, SpecError};
use egaku::{Column, SortKey, SortOrder, TableError, TableView};

/// The reserved column field that projects a row's identity
/// ([`Row::name`]) rather than one of its cells.
///
/// A re-export of [`egaku::IDENTITY_FIELD`], not a second constant. banken's
/// authored `(defk8sview)` forms spell it `:field name` and the fixture/live
/// readers omit it from `Row.cells`; that agreement is now with egaku's
/// vocabulary rather than with a private copy of it.
pub use egaku::IDENTITY_FIELD;

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
    SortKey::new("STATUS", SortOrder::Desc)
}

/// Project an authored `banken_spec` sort onto egaku's.
///
/// Two enums for one fact is exactly the drift this repo models away, but the
/// two halves are owned by different crates: `banken_spec::types::Ordering` is
/// what the `(defk8sview)` reader parses, `egaku::SortOrder` is what the
/// widget sorts by. The projection is total in both directions and lives in
/// one place, which is the honest floor when neither crate may own the other's
/// enum.
fn sort_order_of(ordering: Ordering) -> SortOrder {
    match ordering {
        Ordering::Asc => SortOrder::Asc,
        Ordering::Desc => SortOrder::Desc,
    }
}

/// The `:pods` table: the resource kind it lists, plus the generic
/// [`TableView`] that holds the rows, columns, cursor and sort.
///
/// Everything about *this table being a pod table* is the `kind` field and the
/// two constructors. Everything about *being a table* is [`Self::view`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PodTable {
    kind: ResourceKind,
    inner: TableView<Row>,
}

impl PodTable {
    /// A fresh `:pods` table over `rows`, default-sorted by STATUS desc
    /// (so unhealthy pods surface first — the k9s default and the
    /// BANKEN.md `(defk8sview)` `:default-sort (by "STATUS" desc)`).
    ///
    /// # Panics
    ///
    /// Never in practice, and the `expect` says why: [`pod_default_sort`]
    /// names `STATUS`, which [`pod_columns`] declares, and both are constants
    /// in this file. Making the no-catalog fallback fallible would push a
    /// `Result` through every test for a failure two adjacent literals cannot
    /// produce. `columns_mirror_the_authored_view` is what keeps the pair
    /// honest against the authored view.
    #[must_use]
    pub fn pods(rows: Vec<Row>) -> Self {
        Self {
            kind: ResourceKind::Pod,
            inner: TableView::new(pod_columns(), rows, pod_default_sort())
                .expect("pod_default_sort names STATUS, which pod_columns declares"),
        }
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
    /// - [`SpecError::Binding`] when the view's `:source` is not a resource
    ///   kind (a health/topology view has no resource table to build). This is
    ///   the one check egaku cannot make — it is about the `(defk8sview)`
    ///   vocabulary, not about tables.
    /// - [`SpecError::Binding`] when the view's `:default-sort` names a column
    ///   the view does not declare, mapped from
    ///   [`TableError::UnknownSortColumn`]. That check now happens inside
    ///   [`TableView::new`], on the only constructor there is; previously it
    ///   was performed here and *only* here. Without it the sort resolves to a
    ///   field no row carries, every row projects `""`, every comparison ties,
    ///   and the table comes out in whatever order the reader happened to
    ///   return — a silently arbitrary order that looks sorted.
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
        let sort = SortKey::new(
            view.default_sort.column.clone(),
            sort_order_of(view.default_sort.order),
        );

        let inner = TableView::new(columns, rows, sort).map_err(|e| match e {
            TableError::UnknownSortColumn(col) => binding_error(
                "view `",
                view_name,
                {
                    let mut m = String::from("`'s :default-sort names column `");
                    m.push_str(&col);
                    m.push_str("`, which the view does not declare");
                    m
                }
                .as_str(),
            ),
        })?;

        Ok(Self { kind, inner })
    }

    /// The resource kind this table lists.
    #[must_use]
    pub fn kind(&self) -> ResourceKind {
        self.kind
    }

    /// The generic table state — columns, rows, cursor, sort.
    ///
    /// Deliberately an accessor rather than a wall of forwarding methods: a
    /// forwarder per `TableView` method would re-grow, one call site at a
    /// time, the local model this collapse deleted. `table.view().rows()`
    /// says out loud which half of the type is answering.
    #[must_use]
    pub fn view(&self) -> &TableView<Row> {
        &self.inner
    }

    /// The generic table state, mutably — selection and sort moves.
    pub fn view_mut(&mut self) -> &mut TableView<Row> {
        &mut self.inner
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

    /// The `:pods` binding is a pod table: the kind is what banken adds on top
    /// of a generic view, and it must survive both constructors.
    #[test]
    fn the_fallback_table_lists_pods() {
        assert_eq!(PodTable::pods(vec![]).kind(), ResourceKind::Pod);
    }

    /// The lifted model is REACHED, not merely linked: the fallback's columns
    /// and default sort arrive in the `TableView` and it sorts on them.
    #[test]
    fn the_pod_binding_reaches_the_lifted_model() {
        let t = PodTable::pods(vec![
            row("a", "CrashLoopBackOff"),
            row("b", "Running"),
            row("c", "Pending"),
        ]);
        assert_eq!(t.view().columns(), pod_columns().as_slice());
        assert_eq!(t.view().sort(), &pod_default_sort());
        // STATUS desc on the `phase` cell — the header→field resolution the
        // lift performs. Without it every row projects "" and the order is
        // whatever came in.
        let statuses: Vec<&str> = t
            .view()
            .rows()
            .iter()
            .map(|r| t.view().cell_value(r, &Column::new("STATUS", "phase")))
            .collect();
        assert_eq!(statuses, vec!["Running", "Pending", "CrashLoopBackOff"]);
    }

    /// Both authored `Ordering` values project, and onto different directions
    /// — a projection that collapsed would make `:default-sort … asc` and
    /// `… desc` render identically.
    #[test]
    fn the_authored_ordering_projection_is_total_and_injective() {
        assert_eq!(sort_order_of(Ordering::Asc), SortOrder::Asc);
        assert_eq!(sort_order_of(Ordering::Desc), SortOrder::Desc);
    }
}
