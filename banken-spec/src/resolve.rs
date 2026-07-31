//! [`Catalog`] — the resolved, cross-checked banken vocabulary.
//!
//! # Why a resolved bundle is a type
//!
//! banken's vocabulary is five domains that *reference each other by name*:
//! a view drills to a drill, a drill starts from a view or ward, a ward
//! augments a view and runs pathologies, a nav key shares a chord namespace
//! with an action. Every one of those is a **string join**, and every string
//! join is a silent-failure class: a typo, a rename, a deleted form, and the
//! reference resolves to nothing at runtime while every individual form
//! compiles perfectly.
//!
//! [`Catalog`]'s fields are private and [`Catalog::resolve`] /
//! [`crate::load_catalog`] are its only constructors, so **an unresolved catalog
//! is unconstructible**. A consumer cannot hold a bundle of banken forms
//! whose cross-references were never checked — the same construction-seal
//! shape as [`crate::pathology::WardVerdict`] and the same qualified tier:
//! **truly-unrepresentable within this authored surface**. It is not a claim
//! about the individual `load_views()`/`load_actions()` helpers, which stay
//! public because a single-domain consumer legitimately needs them.
//!
//! # What is checked
//!
//! | Check | Error |
//! |---|---|
//! | names unique per axis | [`SpecError::DuplicateName`] |
//! | each drill path descends, non-empty | [`SpecError::NonDescendingDrill`] / [`SpecError::EmptyDrill`] |
//! | a view/ward `:drill-to` resolves | [`SpecError::DanglingDrill`] |
//! | a drill's `:from` resolves | [`SpecError::UnknownDrillSource`] |
//! | a ward's `:view` exists and is `HealthWard` | [`SpecError::WardViewMissing`] / [`SpecError::WardViewKindMismatch`] |
//! | ward lanes ↔ view columns correspond | [`SpecError::WardLaneColumnMismatch`] |
//! | a ward's `:pathologies` resolve | [`SpecError::UnknownPathology`] |
//! | each bancada's shape + WITNESS OBLIGATION | [`SpecError::EmptyBancada`] / [`SpecError::BancadaPlacement`] / [`SpecError::UnwitnessedBancada`] / [`SpecError::UnneededWitness`] |
//! | a bancada's `:from` resolves | [`SpecError::UnknownDrillSource`] |
//! | no two chords collide, across actions AND nav keys AND bancadas | [`SpecError::ChordConflict`] |
//!
//! An **unreferenced** drill is deliberately NOT an error: a drill is also
//! reachable from the `:` command bar (BANKEN.md §V makes the command bar a
//! first-class peer of the ward), so requiring a `drill_to` referent would
//! forbid a legitimate authoring.

use std::collections::BTreeSet;

use crate::{
    bindings::build_binding_map,
    drill::DrillSpec,
    error::SpecError,
    bancada::BancadaSpec,
    nav::NavKeySpec,
    pathology::PathologySpec,
    types::{K8sActionSpec, K8sViewSpec, ViewKind},
    ward::WardSpec,
};

/// The resolved banken vocabulary — every cross-reference checked.
///
/// Construct with [`crate::load_catalog`] (the shipped `specs/*.lisp`) or
/// [`Self::resolve`] (an arbitrary set, for tests and for a consumer reading
/// a custom `spec_dir`).
#[derive(Debug, Clone)]
pub struct Catalog {
    views: Vec<K8sViewSpec>,
    actions: Vec<K8sActionSpec>,
    pathologies: Vec<PathologySpec>,
    wards: Vec<WardSpec>,
    drills: Vec<DrillSpec>,
    nav_keys: Vec<NavKeySpec>,
    bancadas: Vec<BancadaSpec>,
}

impl Catalog {
    /// Cross-check a set of authored forms and, if every reference
    /// resolves, produce the resolved catalog.
    ///
    /// # Errors
    ///
    /// Any of the [module-table](self) errors, at the first failure. The
    /// checks run outermost-first (uniqueness, then intra-form shape, then
    /// cross-references) so the reported error is the *cause* rather than a
    /// downstream symptom of it.
    pub fn resolve(
        views: Vec<K8sViewSpec>,
        actions: Vec<K8sActionSpec>,
        pathologies: Vec<PathologySpec>,
        wards: Vec<WardSpec>,
        drills: Vec<DrillSpec>,
        nav_keys: Vec<NavKeySpec>,
        bancadas: Vec<BancadaSpec>,
    ) -> Result<Self, SpecError> {
        // ── 1. Names are join keys; duplicates silently shadow ──
        unique_names("view", views.iter().map(|v| v.name.as_str()))?;
        unique_names("action", actions.iter().map(|a| a.name.as_str()))?;
        unique_names("pathology", pathologies.iter().map(|p| p.name.as_str()))?;
        unique_names("ward", wards.iter().map(|w| w.name.as_str()))?;
        unique_names("drill", drills.iter().map(|d| d.name.as_str()))?;
        unique_names("nav-key", nav_keys.iter().map(|n| n.name.as_str()))?;
        unique_names("bancada", bancadas.iter().map(|g| g.name.as_str()))?;

        // ── 2. Each drill's / bancada's own shape ──
        for d in &drills {
            d.validate()?;
        }
        // A bancada's `validate` also enforces the WITNESS OBLIGATION: a
        // recipe staging a mutating command must carry `:witness`/`:runbook`,
        // because its legality is derived from its panes rather than authored.
        // Running it here is what makes the shipped catalog's compliance a
        // build-time fact rather than a claim.
        for g in &bancadas {
            g.validate()?;
        }

        // ── 3. Cross-references ──
        let drill_names: BTreeSet<&str> = drills.iter().map(|d| d.name.as_str()).collect();
        let view_names: BTreeSet<&str> = views.iter().map(|v| v.name.as_str()).collect();
        let ward_names: BTreeSet<&str> = wards.iter().map(|w| w.name.as_str()).collect();
        let pathology_names: BTreeSet<&str> = pathologies.iter().map(|p| p.name.as_str()).collect();

        for v in &views {
            if let Some(target) = &v.drill_to {
                if !drill_names.contains(target.as_str()) {
                    return Err(SpecError::DanglingDrill {
                        surface: v.name.clone(),
                        drill: target.clone(),
                    });
                }
            }
        }

        for d in &drills {
            if !view_names.contains(d.from.as_str()) && !ward_names.contains(d.from.as_str()) {
                return Err(SpecError::UnknownDrillSource {
                    drill: d.name.clone(),
                    from: d.from.clone(),
                });
            }
        }

        for w in &wards {
            if let Some(target) = &w.drill_to {
                if !drill_names.contains(target.as_str()) {
                    return Err(SpecError::DanglingDrill {
                        surface: w.name.clone(),
                        drill: target.clone(),
                    });
                }
            }
            for p in &w.pathologies {
                if !pathology_names.contains(p.as_str()) {
                    return Err(SpecError::UnknownPathology {
                        ward: w.name.clone(),
                        pathology: p.clone(),
                    });
                }
            }
            check_ward_view_correspondence(w, &views)?;
        }

        // A bancada is reachable from a declared view or ward, exactly like a
        // drill's `:from` — the same silent-failure class (a chord bound to a
        // recipe whose launch surface was renamed away).
        for g in &bancadas {
            if !view_names.contains(g.from.as_str()) && !ward_names.contains(g.from.as_str()) {
                return Err(SpecError::UnknownDrillSource {
                    drill: g.name.clone(),
                    from: g.from.clone(),
                });
            }
        }

        // ── 4. One chord namespace across ALL THREE keyed domains ──
        //
        // A nav key, a postigo action and a bancada share the operator's
        // keyboard, so checking them separately would let `j` (nav) and `j`
        // (a future OBSERVE action) coexist with last-write-wins in the app's
        // keymap. Both non-action domains are projected onto the action shape
        // purely to reuse the one conflict checker — a nav key never gains a
        // legality class that way, and a bancada's is its own derived one.
        let mut keyed = actions.clone();
        keyed.extend(nav_keys.iter().map(NavKeySpec::as_chord_claim));
        keyed.extend(bancadas.iter().map(BancadaSpec::as_chord_claim));
        build_binding_map(&keyed)?;

        Ok(Self {
            views,
            actions,
            pathologies,
            wards,
            drills,
            nav_keys,
            bancadas,
        })
    }

    /// The declared views.
    #[must_use]
    pub fn views(&self) -> &[K8sViewSpec] {
        &self.views
    }

    /// The declared postigo actions.
    #[must_use]
    pub fn actions(&self) -> &[K8sActionSpec] {
        &self.actions
    }

    /// The declared pathology rules.
    #[must_use]
    pub fn pathologies(&self) -> &[PathologySpec] {
        &self.pathologies
    }

    /// The declared health wards.
    #[must_use]
    pub fn wards(&self) -> &[WardSpec] {
        &self.wards
    }

    /// The declared drill paths.
    #[must_use]
    pub fn drills(&self) -> &[DrillSpec] {
        &self.drills
    }

    /// The declared navigation keys.
    #[must_use]
    pub fn nav_keys(&self) -> &[NavKeySpec] {
        &self.nav_keys
    }

    /// The declared pre-warmed troubleshooting sessions.
    #[must_use]
    pub fn bancadas(&self) -> &[BancadaSpec] {
        &self.bancadas
    }

    /// The bancadas reachable from one surface (a view or ward name).
    ///
    /// Total by construction: [`Self::resolve`] already proved every `:from`
    /// names a declared surface, so a recipe cannot be silently unreachable.
    #[must_use]
    pub fn bancadas_from(&self, surface: &str) -> Vec<&BancadaSpec> {
        self.bancadas.iter().filter(|g| g.from == surface).collect()
    }

    /// The pathology rules one ward runs, resolved to values.
    ///
    /// Total by construction: [`Self::resolve`] already proved every name
    /// resolves, so this cannot silently drop a rule.
    #[must_use]
    pub fn pathologies_for(&self, ward: &WardSpec) -> Vec<PathologySpec> {
        ward.pathologies
            .iter()
            .filter_map(|name| self.pathologies.iter().find(|p| &p.name == name).cloned())
            .collect()
    }
}

/// Assert no two forms on one axis share a name.
fn unique_names<'a>(
    axis: &'static str,
    names: impl Iterator<Item = &'a str>,
) -> Result<(), SpecError> {
    let mut seen = BTreeSet::new();
    for n in names {
        if !seen.insert(n) {
            return Err(SpecError::DuplicateName {
                axis,
                name: n.to_owned(),
            });
        }
    }
    Ok(())
}

/// A ward's lanes must correspond, in order, to its view's non-identity
/// columns — the view owns the geometry, the ward owns the signals, and
/// neither re-declares the other.
fn check_ward_view_correspondence(ward: &WardSpec, views: &[K8sViewSpec]) -> Result<(), SpecError> {
    let Some(view) = views.iter().find(|v| v.name == ward.view) else {
        return Err(SpecError::WardViewMissing {
            ward: ward.name.clone(),
            view: ward.view.clone(),
        });
    };
    if view.kind != ViewKind::HealthWard {
        return Err(SpecError::WardViewKindMismatch {
            ward: ward.name.clone(),
            view: ward.view.clone(),
            kind: view.kind.label(),
        });
    }
    // Column 0 is the identity column (the workload name); the rest are the
    // lanes. A view with no identity column cannot carry lanes at all.
    let Some((_identity, lane_columns)) = view.columns.split_first() else {
        return Err(SpecError::WardLaneColumnMismatch {
            ward: ward.name.clone(),
            view: ward.view.clone(),
            detail: "the view declares no columns, so it has no identity column and no lanes"
                .into(),
        });
    };
    let view_headers: Vec<&str> = lane_columns.iter().map(|c| c.header.as_str()).collect();
    let lane_headers: Vec<&str> = ward.lanes.iter().map(|l| l.header.as_str()).collect();
    if view_headers != lane_headers {
        let mut detail = String::from("view lane columns ");
        detail.push_str(&join_quoted(&view_headers));
        detail.push_str(" vs ward lanes ");
        detail.push_str(&join_quoted(&lane_headers));
        return Err(SpecError::WardLaneColumnMismatch {
            ward: ward.name.clone(),
            view: ward.view.clone(),
            detail,
        });
    }
    Ok(())
}

/// `["A", "B"]` rendered as `[A, B]` — a typed join, no `format!()` of a
/// message template.
fn join_quoted(items: &[&str]) -> String {
    let mut s = String::from("[");
    for (i, item) in items.iter().enumerate() {
        if i > 0 {
            s.push_str(", ");
        }
        s.push_str(item);
    }
    s.push(']');
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        drill::{DrillLevel, DrillStep},
        nav::NavIntent,
        pathology::{EvidenceKind, Remedy, Severity},
        types::{ColumnSpec, Ordering, SortKey, ViewSource},
        ward::{Attestation, BandDimension, HeadlineSpec, LaneSignal, LaneSpec, WardScope},
    };

    fn health_view(headers: &[&str]) -> K8sViewSpec {
        K8sViewSpec {
            name: "ward".into(),
            kind: ViewKind::HealthWard,
            source: ViewSource::Health,
            columns: headers
                .iter()
                .map(|h| ColumnSpec {
                    header: (*h).to_owned(),
                    field: (*h).to_lowercase(),
                })
                .collect(),
            default_sort: SortKey {
                column: "STATUS".into(),
                order: Ordering::Desc,
            },
            drill_to: None,
        }
    }

    fn ward(view: &str, lanes: &[&str]) -> WardSpec {
        WardSpec {
            name: "ward".into(),
            view: view.into(),
            scope: WardScope::Fleet,
            lanes: lanes
                .iter()
                .map(|h| LaneSpec {
                    header: (*h).to_owned(),
                    signal: LaneSignal::Band {
                        dimension: BandDimension::Memory,
                    },
                })
                .collect(),
            pathologies: Vec::new(),
            headline: HeadlineSpec {
                label: "QUIET".into(),
                attested: Attestation::Computed,
            },
            drill_to: None,
        }
    }

    fn pathology(name: &str) -> PathologySpec {
        PathologySpec {
            name: name.into(),
            symptom: "s".into(),
            cause: "c".into(),
            severity: Severity::Warning,
            evidence: vec![EvidenceKind::Detection],
            remedy: Remedy::BreakGlass,
        }
    }

    fn bancada(name: &str, keys: &str, from: &str) -> crate::bancada::BancadaSpec {
        use crate::bancada::{
            CommandArg, CommandEffect, BancadaPane, BancadaSpec, PanePlacement, PaneRole,
            SessionLayout, StagedCommand,
        };
        BancadaSpec {
            name: name.into(),
            keys: crate::chord::ActionChord::parse(keys).expect("test chord"),
            from: from.into(),
            layout: SessionLayout::Tiled,
            session_prefix: name.into(),
            panes: vec![BancadaPane {
                role: PaneRole::Logs,
                placement: PanePlacement::Root,
                command: StagedCommand {
                    program: "kubectl".into(),
                    args: vec![CommandArg::Literal("logs".into())],
                    effect: CommandEffect::Observes,
                },
            }],
            witness: None,
            runbook: None,
        }
    }

    fn nav(name: &str, keys: &str, intent: NavIntent) -> NavKeySpec {
        NavKeySpec {
            name: name.into(),
            keys: crate::chord::ActionChord::parse(keys).expect("test chord"),
            intent,
        }
    }

    fn resolve_with(
        views: Vec<K8sViewSpec>,
        pathologies: Vec<PathologySpec>,
        wards: Vec<WardSpec>,
        drills: Vec<DrillSpec>,
        nav_keys: Vec<NavKeySpec>,
    ) -> Result<Catalog, SpecError> {
        Catalog::resolve(
            views,
            Vec::new(),
            pathologies,
            wards,
            drills,
            nav_keys,
            Vec::new(),
        )
    }

    #[test]
    fn a_consistent_catalog_resolves() {
        let c = resolve_with(
            vec![health_view(&["WORKLOAD", "MEM"])],
            vec![pathology("broken-scrape")],
            vec![{
                let mut w = ward("ward", &["MEM"]);
                w.pathologies = vec!["broken-scrape".into()];
                w
            }],
            Vec::new(),
            vec![nav("select-next", "j", NavIntent::SelectNext)],
        )
        .expect("a consistent catalog resolves");
        assert_eq!(c.views().len(), 1);
        assert_eq!(c.wards().len(), 1);
        assert_eq!(c.pathologies_for(&c.wards()[0]).len(), 1);
    }

    /// **THE GATE.** A view drilling to a name nothing declares was a
    /// silently dead Enter key.
    #[test]
    fn a_dangling_drill_target_is_rejected() {
        let mut v = health_view(&["WORKLOAD", "MEM"]);
        v.drill_to = Some("nope".into());
        let err = resolve_with(
            vec![v],
            Vec::new(),
            vec![ward("ward", &["MEM"])],
            Vec::new(),
            Vec::new(),
        )
        .expect_err("a dangling drill target must be rejected");
        match err {
            SpecError::DanglingDrill { surface, drill } => {
                assert_eq!(surface, "ward");
                assert_eq!(drill, "nope");
            }
            other => panic!("expected DanglingDrill, got {other:?}"),
        }
    }

    #[test]
    fn a_drill_from_an_unknown_surface_is_rejected() {
        let d = DrillSpec {
            name: "logs".into(),
            from: "nosuchview".into(),
            steps: vec![DrillStep {
                level: DrillLevel::Logs,
                view: "logs".into(),
            }],
        };
        let err = resolve_with(
            vec![health_view(&["WORKLOAD", "MEM"])],
            Vec::new(),
            vec![ward("ward", &["MEM"])],
            vec![d],
            Vec::new(),
        )
        .expect_err("an unknown drill source must be rejected");
        assert!(
            matches!(&err, SpecError::UnknownDrillSource { from, .. } if from == "nosuchview"),
            "got: {err}"
        );
    }

    /// **THE GATE.** A ward's `:pathologies` naming a rule that does not
    /// exist would silently run one rule fewer — a linter quietly blind to
    /// a class.
    #[test]
    fn an_unknown_pathology_reference_is_rejected() {
        let mut w = ward("ward", &["MEM"]);
        w.pathologies = vec!["never-authored".into()];
        let err = resolve_with(
            vec![health_view(&["WORKLOAD", "MEM"])],
            vec![pathology("broken-scrape")],
            vec![w],
            Vec::new(),
            Vec::new(),
        )
        .expect_err("an unknown pathology reference must be rejected");
        assert!(
            matches!(&err, SpecError::UnknownPathology { pathology, .. } if pathology == "never-authored"),
            "got: {err}"
        );
    }

    /// **THE GATE.** The ward↔view correspondence: two authored files must
    /// not disagree about one screen's columns.
    #[test]
    fn ward_lanes_must_match_the_views_non_identity_columns() {
        let err = resolve_with(
            vec![health_view(&["WORKLOAD", "MEM", "CPU"])],
            Vec::new(),
            vec![ward("ward", &["MEM"])], // missing the CPU lane
            Vec::new(),
            Vec::new(),
        )
        .expect_err("a lane/column disagreement must be rejected");
        match err {
            SpecError::WardLaneColumnMismatch { detail, .. } => {
                assert!(
                    detail.contains("MEM, CPU"),
                    "detail names both sides: {detail}"
                );
                assert!(
                    detail.contains("[MEM]"),
                    "detail names the ward side: {detail}"
                );
            }
            other => panic!("expected WardLaneColumnMismatch, got {other:?}"),
        }
        // Order matters too — the lanes render left to right.
        assert!(
            resolve_with(
                vec![health_view(&["WORKLOAD", "MEM", "CPU"])],
                Vec::new(),
                vec![ward("ward", &["CPU", "MEM"])],
                Vec::new(),
                Vec::new(),
            )
            .is_err(),
            "a reordered lane set is a mismatch"
        );
    }

    #[test]
    fn a_ward_over_a_non_health_view_is_rejected() {
        let mut v = health_view(&["WORKLOAD", "MEM"]);
        v.kind = ViewKind::ResourceTable;
        let err = resolve_with(
            vec![v],
            Vec::new(),
            vec![ward("ward", &["MEM"])],
            Vec::new(),
            Vec::new(),
        )
        .expect_err("a ward over a resource table must be rejected");
        assert!(
            matches!(&err, SpecError::WardViewKindMismatch { kind, .. } if *kind == "resource-table"),
            "got: {err}"
        );
    }

    #[test]
    fn a_ward_over_a_missing_view_is_rejected() {
        let err = resolve_with(
            Vec::new(),
            Vec::new(),
            vec![ward("nosuchview", &["MEM"])],
            Vec::new(),
            Vec::new(),
        )
        .expect_err("a ward over a missing view must be rejected");
        assert!(matches!(err, SpecError::WardViewMissing { .. }),);
    }

    #[test]
    fn duplicate_names_are_rejected_because_names_are_join_keys() {
        let err = resolve_with(
            vec![health_view(&["WORKLOAD", "MEM"])],
            vec![pathology("dup"), pathology("dup")],
            vec![ward("ward", &["MEM"])],
            Vec::new(),
            Vec::new(),
        )
        .expect_err("duplicate pathology names must be rejected");
        match err {
            SpecError::DuplicateName { axis, name } => {
                assert_eq!(axis, "pathology");
                assert_eq!(name, "dup");
            }
            other => panic!("expected DuplicateName, got {other:?}"),
        }
    }

    /// **THE GATE.** Nav keys and postigo actions share ONE chord
    /// namespace. Checking them separately is how `s` ends up navigating on
    /// a build where a nav key was added last.
    #[test]
    fn a_nav_key_colliding_with_a_postigo_action_is_rejected() {
        use crate::types::{ActionLegality, ManifestScope};
        let scale = K8sActionSpec {
            name: "scale".into(),
            keys: crate::chord::ActionChord::parse("s").expect("chord"),
            legality: ActionLegality::Observe,
            manifest_scope: ManifestScope::Full,
        };
        let err = Catalog::resolve(
            vec![health_view(&["WORKLOAD", "MEM"])],
            vec![scale],
            Vec::new(),
            vec![ward("ward", &["MEM"])],
            Vec::new(),
            vec![nav("sort", "s", NavIntent::ToggleSort)],
            Vec::new(),
        )
        .expect_err("a nav key on a postigo chord must be rejected");
        match err {
            SpecError::ChordConflict { chord, .. } => assert_eq!(chord, "s"),
            other => panic!("expected ChordConflict, got {other:?}"),
        }
    }

    /// **THE GATE.** The bancada chord joins the SAME namespace. A recipe
    /// bound to a chord a `(defnavkey)` already claims would otherwise
    /// resolve by bind order — in the tool whose whole point is that the
    /// operator knows which gate a keystroke crosses.
    #[test]
    fn a_bancada_colliding_with_a_nav_chord_is_rejected() {
        let err = Catalog::resolve(
            vec![health_view(&["WORKLOAD", "MEM"])],
            Vec::new(),
            Vec::new(),
            vec![ward("ward", &["MEM"])],
            Vec::new(),
            vec![nav("sort", "o", NavIntent::ToggleSort)],
            vec![bancada("triage", "o", "ward")],
        )
        .expect_err("a bancada on a nav chord must be rejected");
        match err {
            SpecError::ChordConflict { chord, .. } => assert_eq!(chord, "o"),
            other => panic!("expected ChordConflict, got {other:?}"),
        }
    }

    /// A bancada whose `:from` names no declared surface is rejected — the
    /// same class as a dangling drill source (a chord bound to a recipe that
    /// can never be reached).
    #[test]
    fn a_bancada_from_an_unknown_surface_is_rejected() {
        let err = Catalog::resolve(
            vec![health_view(&["WORKLOAD", "MEM"])],
            Vec::new(),
            Vec::new(),
            vec![ward("ward", &["MEM"])],
            Vec::new(),
            Vec::new(),
            vec![bancada("triage", "g", "nosuchview")],
        )
        .expect_err("an unreachable bancada must be rejected");
        assert!(
            matches!(&err, SpecError::UnknownDrillSource { from, .. } if from == "nosuchview"),
            "got: {err}"
        );
    }

    /// **THE GATE.** `Catalog::resolve` runs each recipe's witness obligation,
    /// which is what makes the SHIPPED catalog's compliance a build-time fact
    /// rather than a claim in a comment.
    #[test]
    fn an_unwitnessed_mutating_bancada_fails_the_whole_catalog() {
        let mut g = bancada("triage", "g", "ward");
        g.panes[0].command.effect = crate::bancada::CommandEffect::Mutates;
        let err = Catalog::resolve(
            vec![health_view(&["WORKLOAD", "MEM"])],
            Vec::new(),
            Vec::new(),
            vec![ward("ward", &["MEM"])],
            Vec::new(),
            Vec::new(),
            vec![g],
        )
        .expect_err("an unwitnessed mutating recipe must fail the catalog");
        assert!(
            matches!(&err, SpecError::UnwitnessedBancada { bancada, .. } if bancada == "triage"),
            "got: {err}"
        );
    }

    /// An unreferenced drill is ALLOWED — the `:` command bar is a
    /// first-class peer of the ward (BANKEN.md §V), so requiring a
    /// `drill_to` referent would forbid a legitimate authoring.
    #[test]
    fn an_unreferenced_drill_is_allowed() {
        let d = DrillSpec {
            name: "xray".into(),
            from: "ward".into(),
            steps: vec![DrillStep {
                level: DrillLevel::Xray,
                view: "xray".into(),
            }],
        };
        resolve_with(
            vec![health_view(&["WORKLOAD", "MEM"])],
            Vec::new(),
            vec![ward("ward", &["MEM"])],
            vec![d],
            Vec::new(),
        )
        .expect("a command-bar-reachable drill needs no drill_to referent");
    }
}
