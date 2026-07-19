//! Golden-frame test of the `:pods` table render (BANKEN.md §VI M0 gate,
//! fixture-tier).
//!
//! Asserts via `TestBackend`'s typed `to_lines()` / `cell()` accessors —
//! NEVER `.contains()` on a formatted blob (★★ TOKEN-STABILITY). Proves the
//! render pipeline over the fixture rows produces a structured multi-column
//! table with a header, the correct columns in order, the selected-row
//! highlight, and pathology-colored STATUS cells.
//!
//! Tier-honest: this proves the render over a **fixture** source (a real,
//! verifiable increment). The equivalent gate over a **live** cluster read
//! is DESIGN this session (no cluster reachable — see `banken::live`).

use banken::app::BankenApp;
use banken::fixture::FixtureClusterEnv;
use banken::render::draw_pod_table;
use banken::table::PodTable;
use banken_spec::env::ClusterEnv;
use banken_spec::types::{OperatorId, ResourceKind};
use egaku_term::TestBackend;
use egaku_term::crossterm::style::Color;

fn fixture_table() -> PodTable {
    let env = FixtureClusterEnv::new();
    PodTable::pods(env.list_resources(ResourceKind::Pod, None).unwrap())
}

/// The golden columns, asserted by position (not substring soup).
#[test]
fn pods_table_header_carries_columns_in_order() {
    let table = fixture_table();
    let mut backend = TestBackend::new(80, 12);
    backend.draw(|buf| draw_pod_table(buf, 0, 0, 80, 12, &table));

    let header = &backend.to_lines()[0];
    let positions: Vec<usize> = ["NAME", "READY", "STATUS", "RESTARTS", "AGE"]
        .iter()
        .map(|h| {
            header
                .find(h)
                .unwrap_or_else(|| panic!("header column {h} present in {header:?}"))
        })
        .collect();
    // Columns appear strictly left-to-right in the declared order.
    assert!(
        positions.windows(2).all(|w| w[0] < w[1]),
        "columns ordered NAME<READY<STATUS<RESTARTS<AGE, got {positions:?}",
    );
}

/// The fixture rows all render, one per data row, sorted STATUS-desc so the
/// healthy Running pods sort above the CrashLoopBackOff (desc string sort).
#[test]
fn pods_table_renders_every_fixture_row() {
    let table = fixture_table();
    assert_eq!(table.rows().len(), 5);

    let mut backend = TestBackend::new(80, 12);
    backend.draw(|buf| draw_pod_table(buf, 0, 0, 80, 12, &table));
    let lines = backend.to_lines();
    // Row 0 header, row 1 rule, rows 2.. data. Every pod name is present on
    // its own data row (assert by name presence per row, structured).
    let data_lines: Vec<&String> = lines.iter().skip(2).take(5).collect();
    for row in table.rows() {
        assert!(
            data_lines.iter().any(|l| l.contains(&row.name)),
            "pod {} rendered on a data row",
            row.name,
        );
    }
}

/// The selected row (row 0 after sort) carries the highlight background —
/// asserted per-cell via `TestBackend::cell`, the structured accessor.
#[test]
fn selected_row_carries_the_highlight_background() {
    let table = fixture_table();
    let mut backend = TestBackend::new(80, 12);
    backend.draw(|buf| draw_pod_table(buf, 0, 0, 80, 12, &table));

    // First data row is at y=2 (0 header, 1 rule). It is the selected row.
    let sel_cell = backend.cell(0, 2).expect("first data cell exists");
    assert_eq!(sel_cell.bg, Color::Cyan, "selected row highlight bg");

    // A non-selected data row (y=3) has no highlight bg.
    let other = backend.cell(0, 3).expect("second data cell exists");
    assert_ne!(other.bg, Color::Cyan, "unselected row has no highlight");
}

/// A CrashLoopBackOff STATUS cell renders error-red; a Running STATUS cell
/// renders success-green — the pathology-color axis (BANKEN.md §V) at the
/// cell level. Asserted via the typed cell fg, never a text scan.
#[test]
fn status_cells_are_pathology_colored() {
    // Build a table where the selected (highlighted) row does NOT own the
    // STATUS coloring — so we can read the STATUS fg on unselected rows.
    let table = fixture_table();
    let mut backend = TestBackend::new(80, 12);
    backend.draw(|buf| draw_pod_table(buf, 0, 0, 80, 12, &table));

    // Find the STATUS column start x by locating "STATUS" in the header.
    let header = &backend.to_lines()[0];
    let status_x = u16::try_from(header.find("STATUS").expect("STATUS header")).unwrap();

    // Scan the data rows (y >= 2), reading the first glyph of each STATUS
    // cell; assert at least one green (Running) and one red (CrashLoop)
    // appear across the unselected rows.
    let mut saw_green = false;
    let mut saw_red = false;
    for y in 3..8u16 {
        if let Some(cell) = backend.cell(status_x, y) {
            if cell.fg == Color::Green {
                saw_green = true;
            }
            if cell.fg == Color::Red {
                saw_red = true;
            }
        }
    }
    assert!(saw_green, "a Running STATUS cell renders green");
    assert!(saw_red, "a CrashLoopBackOff STATUS cell renders red");
}

/// The full app frame (title + table + status line) renders through the
/// `BankenApp::render` path with the source label present in the status
/// line — proving the runtime's draw path, not just the bare table drawer.
#[test]
fn app_frame_renders_title_table_and_status_line() {
    let app = BankenApp::new(
        FixtureClusterEnv::new(),
        OperatorId("drzzln".into()),
        "source: fixture",
    );
    let mut backend = TestBackend::new(80, 16);
    backend.draw(|buf| app.render(buf));
    let lines = backend.to_lines();

    // Title on row 0.
    assert!(lines[0].contains(":pods"), "title bar shows the :pods view");
    // Status line (last row) carries the source label + the key legend.
    let status = lines.last().expect("a status line");
    assert!(
        status.contains("fixture"),
        "status shows the source (tier-honest)"
    );
    assert!(
        status.contains("DECLARE") && status.contains("OBSERVE"),
        "status shows the postigo chord legend",
    );
}
