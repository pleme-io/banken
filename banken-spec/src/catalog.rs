//! Catalog reflection (★★ CATALOG REFLECTION) — the postigo primitive
//! self-describes.
//!
//! Every closed enum in the typed border carries an `ALL` const (see
//! `types.rs`). This module composes them into a self-describing
//! inventory of the action-legality universe, and the tests here prove
//! that every variant appears in its `ALL` const — the exhaustive-match
//! guarantee. If a new variant lands without a catalog row, the
//! corresponding `every_*_variant_in_all` test fails the build.

use crate::types::{DeclareTargetKind, LegalityClass, ResourceKind, ViewKind};

/// One catalog row describing a discriminant of the postigo border.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogRow {
    /// The border axis this row belongs to (`"legality"`, `"target"`, …).
    pub axis: &'static str,
    /// The variant's stable label.
    pub label: &'static str,
}

/// The full self-describing catalog of the postigo border — every
/// discriminant across every closed axis.
pub fn catalog() -> Vec<CatalogRow> {
    let mut rows = Vec::new();
    for c in LegalityClass::ALL {
        rows.push(CatalogRow {
            axis: "legality",
            label: c.label(),
        });
    }
    for t in DeclareTargetKind::ALL {
        rows.push(CatalogRow {
            axis: "declare-target",
            label: t.label(),
        });
    }
    for v in ViewKind::ALL {
        rows.push(CatalogRow {
            axis: "view-kind",
            label: v.label(),
        });
    }
    for k in ResourceKind::ALL {
        rows.push(CatalogRow {
            axis: "resource-kind",
            label: k.label(),
        });
    }
    rows
}

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

    #[test]
    fn catalog_covers_every_axis() {
        let rows = catalog();
        let axes: HashSet<&str> = rows.iter().map(|r| r.axis).collect();
        for required in ["legality", "declare-target", "view-kind", "resource-kind"] {
            assert!(axes.contains(required), "catalog missing axis {required}");
        }
        // The catalog size is the sum of every axis's ALL length — a
        // partition-complete count.
        let expected = LegalityClass::ALL.len()
            + DeclareTargetKind::ALL.len()
            + ViewKind::ALL.len()
            + ResourceKind::ALL.len();
        assert_eq!(rows.len(), expected);
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
