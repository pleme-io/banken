//! The app is DRIVEN by the authored vocabulary, not merely checked against
//! it.
//!
//! Before this, `banken/src/app.rs` hand-bound six navigation chords next to
//! three authored postigo ones and `banken/src/table.rs` hand-listed the five
//! `:pods` columns that `specs/views.lisp` also declares —
//! `app_keymap_agrees_with_the_authored_chords` was a *drift gate over the
//! hand-lists* rather than their elimination (`banken/src/keys.rs` said so, as
//! `pending-banken: keymap-derived-from-catalog`). These tests prove the
//! derivation: change the authored spec, and the runtime moves.

use awase::{Hotkey, Key, MatchContext, Modifiers};
use banken::app::{Action, BankenApp, key_legend, keymap_from_catalog, unbound_action_names};
use banken::fixture::FixtureClusterEnv;
use banken::table::{IDENTITY_FIELD, PodTable, pod_columns};
use banken_spec::env::ClusterEnv;
use banken_spec::env::DISPLAY_NAME_FIELD;
use banken_spec::testing::MockSessionEnv;
use banken_spec::types::{OperatorId, ResourceKind};
use banken_spec::{Catalog, SpecError, load_catalog};

/// The action an authored chord resolves to in the app keymap.
///
/// One lookup helper because the authored chord and the delivered chord are
/// now the SAME type: there is no projection step to assert around.
fn act(km: &awase::KeyMode<Action>, hk: Hotkey) -> Option<&Action> {
    km.find_binding(&hk, &MatchContext::default())
        .map(|b| &b.action)
}

/// An unmodified chord.
fn bare(k: Key) -> Hotkey {
    Hotkey::new(Modifiers::NONE, k)
}

fn catalog() -> Catalog {
    load_catalog().expect("the shipped vocabulary must resolve")
}

fn fixture_rows() -> Vec<banken_spec::env::Row> {
    FixtureClusterEnv::new()
        .list_resources(ResourceKind::Pod, None)
        .expect("fixture read")
}

// ── the keymap is derived ────────────────────────────────────────────────

/// Every binding in the app keymap traces to an authored form — the nav
/// chords from `(defnavkey)`, the postigo chords from `(defk8saction)`.
#[test]
fn the_keymap_is_built_from_the_authored_catalog() {
    let c = catalog();
    let km = keymap_from_catalog(&c).expect("the shipped catalog builds a keymap");

    // Navigation, from specs/navkeys.lisp.
    assert_eq!(act(&km, bare(Key::J)), Some(&Action::SelectNext));
    assert_eq!(act(&km, bare(Key::Down)), Some(&Action::SelectNext));
    assert_eq!(act(&km, bare(Key::K)), Some(&Action::SelectPrev));
    assert_eq!(act(&km, bare(Key::Up)), Some(&Action::SelectPrev));
    assert_eq!(act(&km, bare(Key::O)), Some(&Action::ToggleSort));
    assert_eq!(act(&km, bare(Key::Q)), Some(&Action::Quit));
    // `escape` used to need the ONE measured `escape`→`esc` translation, and
    // this spot carried a second assertion pinning that the AUTHORED spelling
    // was NOT the delivered one. That assertion had nothing to say once the
    // two vocabularies became one type — there is no second spelling to be
    // wrong about. Its disappearance is the clearest single sign the
    // projection is really gone, rather than moved somewhere else.
    assert_eq!(act(&km, bare(Key::Escape)), Some(&Action::Dismiss));

    // postigo, from specs/actions.lisp.
    assert_eq!(act(&km, bare(Key::L)), Some(&Action::ObserveLogs));
    assert_eq!(act(&km, bare(Key::S)), Some(&Action::DeclareScale));
    assert_eq!(
        act(&km, Hotkey::new(Modifiers::SHIFT, Key::S)),
        Some(&Action::BreakGlass),
    );
    assert_ne!(
        act(&km, bare(Key::S)),
        Some(&Action::BreakGlass),
        "bare `s` must be DECLARE, never BREAK-GLASS",
    );
}

/// **THE GATE.** The derivation is real: re-spell a chord in the AUTHORED
/// source and the runtime binding moves, with no Rust edit. A hand-list
/// would ignore the edit entirely.
#[test]
fn respelling_an_authored_chord_moves_the_runtime_binding() {
    // Move `toggle-sort` from `o` to `z` in the authored nav keys.
    let rebound = banken_spec::CANONICAL_NAVKEYS_LISP.replace(":keys \"o\"", ":keys \"z\"");
    assert!(
        rebound.contains(":keys \"z\""),
        "the substitution must have landed, or this gate proves nothing"
    );
    let nav = banken_spec::loader::load_all::<banken_spec::nav::NavKeySpec>(&rebound)
        .expect("the rebound nav keys compile");
    let c = Catalog::resolve(
        banken_spec::load_views().unwrap(),
        banken_spec::load_actions().unwrap(),
        banken_spec::load_pathologies().unwrap(),
        banken_spec::load_wards().unwrap(),
        banken_spec::load_drills().unwrap(),
        nav,
        banken_spec::load_bancadas().unwrap(),
    )
    .expect("resolves");

    let km = keymap_from_catalog(&c).expect("builds");
    assert_eq!(
        act(&km, bare(Key::Z)),
        Some(&Action::ToggleSort),
        "the AUTHORED chord drives the runtime binding"
    );
    assert_eq!(
        act(&km, bare(Key::O)),
        None,
        "and the old chord is gone — proving nothing hardcoded it"
    );
}

/// **INVERTED, and that inversion is the point.**
///
/// This test used to assert that `space` was REFUSED. awase spelled it
/// `space`, crossterm delivers `Char(' ')`, egaku-term named that a literal
/// one-character string, and no safe translation existed — so banken refused
/// the chord by name rather than guess. The refusal was correct given two
/// vocabularies; it was still an authored chord an operator could not use.
///
/// With typed delivery there is one vocabulary and `space` is an ordinary
/// key. The gate now asserts the opposite, and the old refusal path has no
/// remaining input: **banken's unprojectable set is empty.**
#[test]
fn space_is_now_an_ordinary_authored_chord() {
    let respelled = banken_spec::CANONICAL_NAVKEYS_LISP.replace(":keys \"o\"", ":keys \"space\"");
    assert!(respelled.contains(":keys \"space\""), "substitution landed");
    let nav = banken_spec::loader::load_all::<banken_spec::nav::NavKeySpec>(&respelled)
        .expect("`space` is a valid awase chord, so it PARSES");
    let c = Catalog::resolve(
        banken_spec::load_views().unwrap(),
        banken_spec::load_actions().unwrap(),
        banken_spec::load_pathologies().unwrap(),
        banken_spec::load_wards().unwrap(),
        banken_spec::load_drills().unwrap(),
        nav,
        banken_spec::load_bancadas().unwrap(),
    )
    .expect("and it cross-resolves");

    let km = keymap_from_catalog(&c).expect("`space` binds — it is just a key now");
    assert_eq!(
        act(&km, bare(Key::Space)),
        Some(&Action::ToggleSort),
        "the authored `space` reaches the runtime"
    );
    // …and it is genuinely deliverable, not merely bindable: the probe drives
    // synthetic events through the same function the runtime uses.
    egaku_term::event::testing::assert_all_deliverable(&[bare(Key::Space)]);
}

/// A duplicate authored chord is still refused by name — the check moved from
/// the projection into `try_bind`, it did not disappear with it.
#[test]
fn a_duplicate_authored_chord_is_refused_by_name() {
    // Point `toggle-sort` at `q`, which `quit` already claims.
    let clashing = banken_spec::CANONICAL_NAVKEYS_LISP.replace(":keys \"o\"", ":keys \"q\"");
    assert!(clashing.contains(":keys \"q\""), "substitution landed");
    let Ok(nav) = banken_spec::loader::load_all::<banken_spec::nav::NavKeySpec>(&clashing) else {
        // The catalog's own conflict pass may reject it first, which is an
        // even earlier refusal and equally acceptable.
        return;
    };
    let Ok(c) = Catalog::resolve(
        banken_spec::load_views().unwrap(),
        banken_spec::load_actions().unwrap(),
        banken_spec::load_pathologies().unwrap(),
        banken_spec::load_wards().unwrap(),
        banken_spec::load_drills().unwrap(),
        nav,
        banken_spec::load_bancadas().unwrap(),
    ) else {
        return;
    };

    match keymap_from_catalog(&c) {
        Err(SpecError::Binding(msg)) => {
            assert!(
                msg.contains("could never fire"),
                "the refusal explains the consequence: {msg}"
            );
        }
        Err(other) => panic!("expected a Binding error, got {other:?}"),
        Ok(_) => panic!("a duplicate chord must not silently displace a binding"),
    }
}

/// An authored action the app cannot dispatch is reported, not silently
/// bound to nothing. `describe` is authored OBSERVE with no describe panel
/// yet; pinning the set makes growing it deliberate.
#[test]
fn unbound_authored_actions_are_reported_rather_than_silent() {
    let c = catalog();
    assert_eq!(
        unbound_action_names(&c),
        vec!["describe".to_owned()],
        "the ONE authored action with no app handler"
    );
}

/// **THE GATE.** The status-line legend is the one surface that tells the
/// operator which gate a keystroke crosses, and it used to be a hand-written
/// literal that *already disagreed with reality* — it advertised `S` for a
/// chord the runtime binds as `shift+s`. Derived, it cannot.
#[test]
fn the_legend_states_the_authored_chords_and_their_legality_classes() {
    let c = catalog();
    let legend = key_legend(&c);
    assert_eq!(
        legend,
        "l:OBSERVE  s:DECLARE  shift+s:BREAK-GLASS  \
         g:OBSERVE  shift+g:BREAK-GLASS  h:help  o:sort  q:quit",
        "the legend is derived from the authored chords + typed legality \
         classes — INCLUDING the bancada chords, whose class is derived from \
         their panes rather than authored"
    );
    // The specific lie the old literal told: `S` is NOT the chord.
    assert!(
        !legend.contains("S:BREAK-GLASS"),
        "the runtime binds `shift+s`, so the legend must not advertise `S`"
    );

    // And it MOVES with the authored source.
    let rebound = banken_spec::CANONICAL_ACTIONS_LISP.replace(":keys \"l\"", ":keys \"v\"");
    assert!(rebound.contains(":keys \"v\""), "substitution landed");
    let actions = banken_spec::loader::load_all::<banken_spec::types::K8sActionSpec>(&rebound)
        .expect("compiles");
    let moved = Catalog::resolve(
        banken_spec::load_views().unwrap(),
        actions,
        banken_spec::load_pathologies().unwrap(),
        banken_spec::load_wards().unwrap(),
        banken_spec::load_drills().unwrap(),
        banken_spec::load_nav_keys().unwrap(),
        banken_spec::load_bancadas().unwrap(),
    )
    .expect("resolves");
    let moved_legend = key_legend(&moved);
    assert!(moved_legend.starts_with("v:OBSERVE"), "got: {moved_legend}");
    assert!(!moved_legend.contains("l:OBSERVE"));
}

// ── the table is derived ────────────────────────────────────────────────

/// The `:pods` columns and default sort come from `(defk8sview "pods")`.
#[test]
fn the_pods_table_is_built_from_the_authored_view() {
    let c = catalog();
    let t = PodTable::from_view(&c, "pods", fixture_rows()).expect("the authored view builds");

    let headers: Vec<&str> = t
        .view()
        .columns()
        .iter()
        .map(|col| col.header.as_str())
        .collect();
    assert_eq!(headers, vec!["NAME", "READY", "STATUS", "RESTARTS", "AGE"]);
    let fields: Vec<&str> = t
        .view()
        .columns()
        .iter()
        .map(|col| col.field.as_str())
        .collect();
    assert_eq!(
        fields,
        vec![DISPLAY_NAME_FIELD, "ready", "phase", "restarts", "age"]
    );
    // **And it must NOT be the reserved field.** `egaku::TableView` projects
    // `IDENTITY_FIELD` straight to `TableRow::identity()`, which here is the
    // object UID — so a NAME column authored on it renders uids. Measured
    // 2026-08-09: 69 of 69 rows on camelot-eks. See
    // `banken_spec::env::DISPLAY_NAME_FIELD`.
    assert_ne!(
        fields[0], IDENTITY_FIELD,
        "the NAME column must project through `cell`, not through `identity()`",
    );
    assert_eq!(t.view().sort().column, "STATUS");
    assert_eq!(t.kind(), ResourceKind::Pod);
}

/// **THE GATE.** The authored `:field` is now the actual `Row.cells` join
/// key, so every declared column resolves against real reader output. Before
/// the rekeying the view said `:field phase` while every reader emitted
/// `"STATUS"` — this assertion would have reported all four data fields
/// unresolved, which is exactly how invisible that divergence was.
#[test]
fn every_authored_column_resolves_against_the_readers_rows() {
    let c = catalog();
    let t = PodTable::from_view(&c, "pods", fixture_rows()).expect("builds");
    assert_eq!(
        t.view().unresolved_fields(),
        Vec::<&str>::new(),
        "an authored column whose field no row carries would render forever empty"
    );
    // Non-vacuous: a column naming a field nothing supplies IS reported.
    let mut with_ghost = t.clone();
    // (Rebuild through the public path with an extra ghost column is not
    // expressible — so assert the reporter itself on a table whose rows are
    // empty, where every data field is trivially unresolved.)
    with_ghost.view_mut().set_rows(Vec::new());
    assert_eq!(
        with_ghost.view().unresolved_fields().len(),
        5,
        "object-name/ready/phase/restarts/age — NAME counts too now that it \
         projects through `cell` rather than through `identity()`"
    );
}

/// **THE GATE.** `PodTable::pods` is the no-catalog fallback and must mirror
/// the authored view exactly, or the fallback silently renders a different
/// table than the spec declares.
#[test]
fn the_fallback_columns_mirror_the_authored_view() {
    let c = catalog();
    let authored = PodTable::from_view(&c, "pods", Vec::new()).expect("builds");
    let fallback = PodTable::pods(Vec::new());
    assert_eq!(
        fallback.view().columns(),
        authored.view().columns(),
        "pod_columns() is a MIRROR of specs/views.lisp — it drifted"
    );
    assert_eq!(fallback.view().sort().column, authored.view().sort().column);
    assert_eq!(fallback.view().sort().order, authored.view().sort().order);
    // And `pod_columns()` is what the fallback used.
    assert_eq!(fallback.view().columns(), pod_columns().as_slice());
}

/// A `:default-sort` naming a column the view does not declare is REFUSED.
/// Previously it sorted by a cell key no row carries — every row compared
/// equal, so the table came out in whatever order the reader returned.
#[test]
fn a_default_sort_naming_an_undeclared_column_is_refused() {
    let bad = banken_spec::CANONICAL_VIEWS_LISP.replace(":column \"STATUS\"", ":column \"STATUZ\"");
    assert!(bad.contains("STATUZ"), "substitution landed");
    let views = banken_spec::loader::load_all::<banken_spec::types::K8sViewSpec>(&bad)
        .expect("a typo'd sort column still PARSES");
    // The ward view's lanes still correspond, so this resolves — the defect
    // is intra-view and surfaces at table construction.
    let c = Catalog::resolve(
        views,
        banken_spec::load_actions().unwrap(),
        banken_spec::load_pathologies().unwrap(),
        banken_spec::load_wards().unwrap(),
        banken_spec::load_drills().unwrap(),
        banken_spec::load_nav_keys().unwrap(),
        banken_spec::load_bancadas().unwrap(),
    )
    .expect("cross-resolution is unaffected");
    let err = PodTable::from_view(&c, "pods", fixture_rows())
        .expect_err("an undeclared sort column must be refused");
    assert!(
        err.to_string().contains("STATUZ"),
        "the refusal names the bad column: {err}"
    );
}

/// A health/topology view has no resource table — refused rather than built
/// as an empty one.
#[test]
fn a_non_resource_view_cannot_build_a_resource_table() {
    let c = catalog();
    let err = PodTable::from_view(&c, "ward", Vec::new())
        .expect_err("the ward reads health, not a resource kind");
    assert!(
        err.to_string().contains("resource"),
        "the refusal says why: {err}"
    );
    assert!(
        PodTable::from_view(&c, "nosuchview", Vec::new()).is_err(),
        "an unknown view name is refused"
    );
}

// ── the whole app ───────────────────────────────────────────────────────

/// `try_new` is FALLIBLE on purpose: the keymap and the columns come from the
/// spec, so a spec failure must surface rather than fall back to hardcoded
/// chords the authored legend does not describe.
#[test]
fn the_app_builds_from_the_shipped_vocabulary() {
    let app = BankenApp::try_new(
        FixtureClusterEnv::new(),
        MockSessionEnv::new(),
        OperatorId::new("drzzln").expect("a literal witness is non-blank"),
        "source: fixture",
    )
    .expect("the shipped vocabulary must build an app");
    assert_eq!(app.table().view().rows().len(), 5);
    assert_eq!(app.table().view().unresolved_fields(), Vec::<&str>::new());
}
