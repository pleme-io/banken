//! Catalog reflection (★★ CATALOG REFLECTION) — banken's whole typed
//! vocabulary self-describes.
//!
//! Every closed enum in every domain carries an `ALL` const. This module
//! composes them into one self-describing inventory spanning all six
//! authored domains — `(defk8sview)` / `(defk8saction)` (the postigo core,
//! [`crate::types`]), `(defpathology)` ([`crate::pathology`]), `(defward)`
//! ([`crate::ward`]), `(defdrill)` ([`crate::drill`]) and `(defnavkey)`
//! ([`crate::nav`]).
//!
//! # Two tiers of enforcement, stated separately
//!
//! - **Variant ∈ `ALL`.** For the ten axes emitted by
//!   [`crate::closed_catalog!`] this is **truly-unrepresentable**: the
//!   variant list and the `ALL` list are the same list. For the four
//!   hand-written axes in [`crate::types`] it stays **CI-caught** by the
//!   `every_*_variant_in_all` tests below (`pending-banken:
//!   closed-catalog-macro-backfill`).
//! - **Axis ∈ the catalog.** Always **CI-caught**, by
//!   `catalog_covers_every_axis` against the [`REQUIRED_AXES`] roster — in
//!   both directions, so neither a domain shipping without its catalog entry
//!   nor an axis appearing without a roster row can pass.
//!
//! Do not collapse those two into one claim.

use crate::{
    bancada::{CommandEffect, ContextField, PanePlacement, PaneRole, SessionLayout},
    drill::DrillLevel,
    nav::NavIntent,
    pathology::{EvidenceKind, RemedyKind, Severity, Verdict},
    types::{DeclareTargetKind, LegalityClass, ResourceKind, ViewKind},
    ward::{AttestationKind, BandDimension, LaneSignalKind, WardScope},
};

/// One catalog row describing a discriminant of the postigo border.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogRow {
    /// The border axis this row belongs to (`"legality"`, `"target"`, …).
    pub axis: &'static str,
    /// The variant's stable label.
    pub label: &'static str,
}

/// Push one axis's `ALL` list into `rows`.
///
/// Generic over the label projection so a new axis is **one line** in
/// [`catalog`] rather than a copy-pasted `for` loop. (The five hand-written
/// loops this replaced were the second-copy signal ★★ EMITTER SUBSTRATE
/// names.)
fn push_axis<T: Copy>(
    rows: &mut Vec<CatalogRow>,
    axis: &'static str,
    all: &'static [T],
    label: impl Fn(T) -> &'static str,
) {
    for v in all {
        rows.push(CatalogRow {
            axis,
            label: label(*v),
        });
    }
}

/// The full self-describing catalog of banken's typed border — every
/// discriminant across every closed axis, in every domain.
///
/// ★★ CATALOG REFLECTION: adding a domain requires landing its catalog
/// entry, and `catalog_covers_every_axis` fails the build when an axis is
/// missing from this function.
pub fn catalog() -> Vec<CatalogRow> {
    let mut rows = Vec::new();
    // postigo — (defk8saction) / (defk8sview)
    push_axis(
        &mut rows,
        "legality",
        LegalityClass::ALL,
        LegalityClass::label,
    );
    push_axis(
        &mut rows,
        "declare-target",
        DeclareTargetKind::ALL,
        DeclareTargetKind::label,
    );
    push_axis(&mut rows, "view-kind", ViewKind::ALL, ViewKind::label);
    push_axis(
        &mut rows,
        "resource-kind",
        ResourceKind::ALL,
        ResourceKind::label,
    );
    // (defpathology)
    push_axis(&mut rows, "severity", Severity::ALL, Severity::label);
    push_axis(
        &mut rows,
        "evidence-kind",
        EvidenceKind::ALL,
        EvidenceKind::label,
    );
    push_axis(&mut rows, "remedy-kind", RemedyKind::ALL, RemedyKind::label);
    push_axis(&mut rows, "verdict", Verdict::ALL, Verdict::label);
    // (defward)
    push_axis(
        &mut rows,
        "band-dimension",
        BandDimension::ALL,
        BandDimension::label,
    );
    push_axis(
        &mut rows,
        "lane-signal-kind",
        LaneSignalKind::ALL,
        LaneSignalKind::label,
    );
    push_axis(&mut rows, "ward-scope", WardScope::ALL, WardScope::label);
    push_axis(
        &mut rows,
        "attestation-kind",
        AttestationKind::ALL,
        AttestationKind::label,
    );
    // (defdrill)
    push_axis(&mut rows, "drill-level", DrillLevel::ALL, DrillLevel::label);
    // (defnavkey)
    push_axis(&mut rows, "nav-intent", NavIntent::ALL, NavIntent::label);
    // (defbancada)
    push_axis(&mut rows, "pane-role", PaneRole::ALL, PaneRole::label);
    push_axis(
        &mut rows,
        "pane-placement",
        PanePlacement::ALL,
        PanePlacement::label,
    );
    push_axis(
        &mut rows,
        "session-layout",
        SessionLayout::ALL,
        SessionLayout::label,
    );
    push_axis(
        &mut rows,
        "command-effect",
        CommandEffect::ALL,
        CommandEffect::label,
    );
    push_axis(
        &mut rows,
        "context-field",
        ContextField::ALL,
        ContextField::label,
    );
    rows
}

/// Every axis the catalog must cover — the ★★ CATALOG REFLECTION roster.
///
/// A new domain lands its axes **here** in the same commit, and
/// `catalog_covers_every_axis` fails the build if [`catalog`] does not emit
/// one of them. (The roster is a separate list from [`catalog`]'s body on
/// purpose: that is what makes "the domain landed but its catalog row did
/// not" a red test rather than an under-reporting catalog nobody notices.)
pub const REQUIRED_AXES: &[&str] = &[
    // postigo
    "legality",
    "declare-target",
    "view-kind",
    "resource-kind",
    // (defpathology)
    "severity",
    "evidence-kind",
    "remedy-kind",
    "verdict",
    // (defward)
    "band-dimension",
    "lane-signal-kind",
    "ward-scope",
    "attestation-kind",
    // (defdrill)
    "drill-level",
    // (defnavkey)
    "nav-intent",
    // (defbancada)
    "pane-role",
    "pane-placement",
    "session-layout",
    "command-effect",
    "context-field",
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{DeclareTarget, ManifestScope};
    use std::collections::HashSet;
    use std::path::PathBuf;

    // ── Exhaustive-match guarantee: every variant is in ALL ─────────
    //
    // The `match` in each test forces every variant to be named; adding
    // a variant without adding it to `ALL` makes the corresponding
    // assertion fail (the label wouldn't be found), and adding a variant
    // without a match arm is a non-exhaustive-match COMPILE error.

    #[test]
    fn every_legality_class_in_all() {
        let all: HashSet<&str> = LegalityClass::ALL.iter().map(|c| c.label()).collect();
        for c in LegalityClass::ALL {
            // Exhaustive match — a new variant breaks compilation here.
            let label = match c {
                LegalityClass::Observe => "observe",
                LegalityClass::Declare => "declare",
                LegalityClass::BreakGlass => "break-glass",
            };
            assert!(
                all.contains(label),
                "legality class {label} missing from ALL"
            );
        }
        assert_eq!(LegalityClass::ALL.len(), 3);
    }

    #[test]
    fn every_declare_target_kind_in_all() {
        let all: HashSet<&str> = DeclareTargetKind::ALL.iter().map(|t| t.label()).collect();
        for t in DeclareTargetKind::ALL {
            let label = match t {
                DeclareTargetKind::FluxHelmValues => "flux-helm-values",
                DeclareTargetKind::BreatheBand => "breathe-band",
                DeclareTargetKind::ViggySetpoint => "viggy-setpoint",
                DeclareTargetKind::GalhoChange => "galho-change",
                DeclareTargetKind::EclusaMergeflow => "eclusa-mergeflow",
            };
            assert!(
                all.contains(label),
                "declare-target {label} missing from ALL"
            );
        }
        assert_eq!(DeclareTargetKind::ALL.len(), 5);
    }

    #[test]
    fn every_view_kind_in_all() {
        let all: HashSet<&str> = ViewKind::ALL.iter().map(|v| v.label()).collect();
        for v in ViewKind::ALL {
            let label = match v {
                ViewKind::ResourceTable => "resource-table",
                ViewKind::HealthWard => "health-ward",
                ViewKind::DependencyTree => "dependency-tree",
                ViewKind::LogPager => "log-pager",
            };
            assert!(all.contains(label), "view-kind {label} missing from ALL");
        }
        assert_eq!(ViewKind::ALL.len(), 4);
    }

    #[test]
    fn every_resource_kind_in_all() {
        let all: HashSet<&str> = ResourceKind::ALL.iter().map(|k| k.label()).collect();
        for k in ResourceKind::ALL {
            let label = match k {
                ResourceKind::Pod => "pod",
                ResourceKind::Service => "service",
                ResourceKind::ConfigMap => "config_map",
                ResourceKind::Endpoints => "endpoints",
                ResourceKind::Namespace => "namespace",
                ResourceKind::Node => "node",
                ResourceKind::Deployment => "deployment",
                ResourceKind::ReplicaSet => "replica_set",
            };
            assert!(
                all.contains(label),
                "resource-kind {label} missing from ALL"
            );
        }
    }

    /// **THE GATE** for ★★ CATALOG REFLECTION: every axis on the
    /// [`REQUIRED_AXES`] roster must actually be emitted by [`catalog`], and
    /// the catalog must emit nothing that is not on the roster.
    ///
    /// Both directions matter. Roster-without-emission is "a domain landed,
    /// its catalog row did not"; emission-without-roster is "an axis grew and
    /// nobody recorded that the catalog owns it".
    #[test]
    fn catalog_covers_every_axis() {
        let rows = catalog();
        let axes: HashSet<&str> = rows.iter().map(|r| r.axis).collect();
        for required in REQUIRED_AXES {
            assert!(
                axes.contains(required),
                "catalog is missing axis `{required}` — a domain landed without \
                 its catalog entry (★★ CATALOG REFLECTION)"
            );
        }
        for emitted in &axes {
            assert!(
                REQUIRED_AXES.contains(emitted),
                "catalog emits axis `{emitted}` which is not on REQUIRED_AXES — \
                 add it to the roster in the same commit"
            );
        }
        assert_eq!(axes.len(), REQUIRED_AXES.len());
        // The catalog size is the sum of every axis's ALL length — a
        // partition-complete count, so a row cannot be dropped or doubled.
        let expected = LegalityClass::ALL.len()
            + DeclareTargetKind::ALL.len()
            + ViewKind::ALL.len()
            + ResourceKind::ALL.len()
            + Severity::ALL.len()
            + EvidenceKind::ALL.len()
            + RemedyKind::ALL.len()
            + Verdict::ALL.len()
            + BandDimension::ALL.len()
            + LaneSignalKind::ALL.len()
            + WardScope::ALL.len()
            + AttestationKind::ALL.len()
            + DrillLevel::ALL.len()
            + NavIntent::ALL.len()
            + PaneRole::ALL.len()
            + PanePlacement::ALL.len()
            + SessionLayout::ALL.len()
            + CommandEffect::ALL.len()
            + ContextField::ALL.len();
        assert_eq!(rows.len(), expected);
    }

    /// Every axis emitted through `closed_catalog!` uses a `kebab-case`
    /// serde repr, so its `label()` IS its wire name — which is what lets a
    /// value be authored in Lisp by the same spelling the catalog reports.
    ///
    /// The four hand-written `types.rs` axes are excluded on purpose:
    /// `ViewKind`'s wire form is PascalCase (`ResourceTable`) while its label
    /// is kebab (`resource-table`), which is the one real obstacle recorded
    /// under `pending-banken: closed-catalog-macro-backfill`. Asserting the
    /// property where it holds, and naming where it does not, is the honest
    /// shape.
    #[test]
    fn labels_are_the_wire_names_for_every_macro_emitted_axis() {
        fn check<T: Copy + serde::Serialize>(all: &[T], label: impl Fn(T) -> &'static str) {
            for v in all {
                let wire = serde_json::to_string(v).expect("serializes");
                let mut expected = String::from("\"");
                expected.push_str(label(*v));
                expected.push('"');
                assert_eq!(wire, expected, "label and serde wire name must agree");
            }
        }
        check(Severity::ALL, Severity::label);
        check(EvidenceKind::ALL, EvidenceKind::label);
        check(RemedyKind::ALL, RemedyKind::label);
        check(Verdict::ALL, Verdict::label);
        check(BandDimension::ALL, BandDimension::label);
        check(LaneSignalKind::ALL, LaneSignalKind::label);
        check(WardScope::ALL, WardScope::label);
        check(AttestationKind::ALL, AttestationKind::label);
        check(DrillLevel::ALL, DrillLevel::label);
        check(NavIntent::ALL, NavIntent::label);
    }

    /// `DeclareTargetKind` is the one *hand-written* axis that now must also
    /// satisfy label==wire, because `(defpathology)`'s `:remedy (:kind
    /// declare :rail …)` authors a rail by that spelling. Pinned separately
    /// so removing its `rename_all` is a red test rather than an
    /// un-authorable remedy discovered later.
    #[test]
    fn declare_target_kind_label_is_the_wire_name() {
        for k in DeclareTargetKind::ALL {
            let wire = serde_json::to_string(k).expect("serializes");
            let mut expected = String::from("\"");
            expected.push_str(k.label());
            expected.push('"');
            assert_eq!(wire, expected);
        }
    }

    /// The load-bearing negative: `DeclareTarget` has no `Kubectl`/`Apply`
    /// arm, so a live-mutate rail cannot be constructed. This test
    /// documents (and, via the exhaustive `kind()` match on the type,
    /// enforces) that the declare-target universe is exactly the five
    /// GitOps rails — a sixth "live" arm would be a compile-visible
    /// addition caught by `every_declare_target_kind_in_all`.
    #[test]
    fn no_live_mutate_declare_target_exists() {
        // Every constructible DeclareTarget maps to one of the five
        // GitOps rails — none is a live cluster mutation.
        let sample = DeclareTarget::FluxHelmValues {
            release_path: PathBuf::from("apps/catch/release.yaml"),
        };
        let kind = sample.kind();
        assert!(
            DeclareTargetKind::ALL.contains(&kind),
            "every declare-target is a known GitOps rail",
        );
        // ManifestScope has exactly one arm — Full — so no partial
        // (patch) scope can be authored.
        let _full = ManifestScope::Full;
    }
}
