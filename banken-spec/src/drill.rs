//! `(defdrill)` — the typed drill-path domain (BANKEN.md §IV T1 / §V / §VI).
//!
//! # What this domain closes
//!
//! `K8sViewSpec::drill_to` is an `Option<String>` naming "the view to drill
//! into on Enter". Today that string resolves against **nothing**: the
//! shipped `specs/views.lisp` drills `pods → "logs"` and `ward →
//! "diagnose"`, and *neither target is a declared view*. A typo, a renamed
//! view, or a target that was never authored is a silent dead Enter key.
//!
//! `(defdrill …)` makes the path a value: an ordered sequence of typed
//! steps from a starting surface down to a terminal. Two things become
//! catchable that were not:
//!
//! 1. **A dangling target.** Every `drill_to` must name a declared
//!    `(defdrill)`, and every drill's `from` must name a declared view or
//!    ward — [`crate::resolve::resolve_catalog`] rejects otherwise.
//! 2. **A nonsense path.** Steps must strictly *descend* the resource
//!    hierarchy, so `pod → namespace` (a drill that zooms out) or a repeated
//!    level has no valid value — [`DrillSpec::validate`] rejects it.
//!
//! Honest tier for both: **eval-caught by the resolver**, not
//! truly-unrepresentable. Nothing in the type system stops an author writing
//! `:drill-to "nope"`; what stops it reaching a running banken is the
//! resolver, and [`crate::load_catalog`] is what makes calling it the
//! default rather than an option.
//!
//! Third-use test: passed — `logs` and `diagnose` are already referenced by
//! the shipped views, and §IV/§VI name the `ctx→ns→pod→container` drill as
//! the k9s navigation spine. Three authored instances ship here.

use serde::{Deserialize, Serialize};
use tatara_lisp::DeriveTataraDomain;

use crate::{closed_catalog, error::SpecError};

closed_catalog! {
    /// One rung of the drill hierarchy.
    ///
    /// Ordered by [`DrillLevel::depth`]: a drill descends, never ascends.
    /// The last three are **terminals** — a leaf surface rather than a
    /// container — and share the deepest rank, which is why two terminals
    /// cannot appear in one path (strict descent forbids equal depth).
    #[serde(rename_all = "kebab-case")]
    pub enum DrillLevel {
        /// A kubeconfig context (the outermost rung).
        Context => "context",
        /// A namespace.
        Namespace => "namespace",
        /// A workload (Deployment/StatefulSet/DaemonSet).
        Workload => "workload",
        /// A pod.
        Pod => "pod",
        /// A container inside a pod.
        Container => "container",
        /// TERMINAL — the log pager.
        Logs => "logs",
        /// TERMINAL — the symptom→cause diagnose tree.
        Diagnose => "diagnose",
        /// TERMINAL — the XRay dependency tree.
        Xray => "xray",
    }
}

impl DrillLevel {
    /// How deep this rung sits. Strictly increasing along a valid path.
    ///
    /// The three terminals share the deepest rank on purpose: a path may end
    /// at exactly one of them, and `logs → diagnose` is not a drill, it is
    /// two drills.
    #[must_use]
    pub fn depth(self) -> u8 {
        match self {
            DrillLevel::Context => 0,
            DrillLevel::Namespace => 1,
            DrillLevel::Workload => 2,
            DrillLevel::Pod => 3,
            DrillLevel::Container => 4,
            DrillLevel::Logs | DrillLevel::Diagnose | DrillLevel::Xray => 5,
        }
    }

    /// `true` when this rung is a leaf surface rather than a container.
    #[must_use]
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            DrillLevel::Logs | DrillLevel::Diagnose | DrillLevel::Xray
        )
    }
}

/// One step of a drill path: the rung, and the view rendered at it.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct DrillStep {
    /// Which rung of the hierarchy this step lands on.
    pub level: DrillLevel,
    /// The view name rendered at this rung. A free label at non-terminal
    /// rungs (banken has no `(defk8sview)` for a namespace picker yet);
    /// `pending-banken: drill-step-view-resolution` tracks tightening this
    /// to a declared-view reference the way `from` already is.
    pub view: String,
}

/// One authored drill path.
#[derive(DeriveTataraDomain, Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[tatara(keyword = "defdrill")]
pub struct DrillSpec {
    /// The drill's name — what a view's or ward's `drill_to` names.
    pub name: String,
    /// The surface the drill starts from: a declared `(defk8sview)` or
    /// `(defward)` name. Resolved by [`crate::resolve::resolve_catalog`].
    pub from: String,
    /// The path, outermost rung first.
    pub steps: Vec<DrillStep>,
}

impl DrillSpec {
    /// Validate the path's shape.
    ///
    /// # Errors
    ///
    /// - [`SpecError::EmptyDrill`] when there are no steps. A zero-step
    ///   drill is a dead Enter key that looks authored.
    /// - [`SpecError::NonDescendingDrill`] when two consecutive steps do not
    ///   strictly descend — a path that zooms out, or revisits a rung, or
    ///   chains two terminals.
    pub fn validate(&self) -> Result<(), SpecError> {
        let Some(first) = self.steps.first() else {
            return Err(SpecError::EmptyDrill(self.name.clone()));
        };
        let mut prev = first.level;
        for step in self.steps.iter().skip(1) {
            if step.level.depth() <= prev.depth() {
                return Err(SpecError::NonDescendingDrill {
                    drill: self.name.clone(),
                    from_level: prev.label(),
                    to_level: step.level.label(),
                });
            }
            prev = step.level;
        }
        Ok(())
    }

    /// The path's terminal rung, if it ends at one.
    #[must_use]
    pub fn terminal(&self) -> Option<DrillLevel> {
        self.steps
            .last()
            .map(|s| s.level)
            .filter(|l| l.is_terminal())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn drill(name: &str, levels: &[DrillLevel]) -> DrillSpec {
        DrillSpec {
            name: name.into(),
            from: "pods".into(),
            steps: levels
                .iter()
                .map(|l| DrillStep {
                    level: *l,
                    view: l.label().to_owned(),
                })
                .collect(),
        }
    }

    #[test]
    fn a_descending_path_is_valid() {
        let d = drill(
            "ctx-to-logs",
            &[
                DrillLevel::Context,
                DrillLevel::Namespace,
                DrillLevel::Pod,
                DrillLevel::Logs,
            ],
        );
        d.validate().expect("a descending path is valid");
        assert_eq!(d.terminal(), Some(DrillLevel::Logs));
    }

    /// **THE GATE.** A drill that zooms out is not a drill.
    #[test]
    fn an_ascending_path_is_rejected() {
        let err = drill("backwards", &[DrillLevel::Pod, DrillLevel::Namespace])
            .validate()
            .expect_err("a path that zooms out must be rejected");
        match err {
            SpecError::NonDescendingDrill {
                drill,
                from_level,
                to_level,
            } => {
                assert_eq!(drill, "backwards");
                assert_eq!(from_level, "pod");
                assert_eq!(to_level, "namespace");
            }
            other => panic!("expected NonDescendingDrill, got {other:?}"),
        }
    }

    #[test]
    fn a_repeated_rung_is_rejected() {
        assert!(
            drill("stuck", &[DrillLevel::Pod, DrillLevel::Pod])
                .validate()
                .is_err(),
            "revisiting a rung is not a descent"
        );
    }

    /// Two terminals share the deepest rank, so chaining them is caught by
    /// the same strict-descent rule — no separate check needed.
    #[test]
    fn two_terminals_in_one_path_are_rejected() {
        assert!(
            drill(
                "both",
                &[DrillLevel::Pod, DrillLevel::Logs, DrillLevel::Diagnose]
            )
            .validate()
            .is_err(),
            "logs → diagnose is two drills, not one path"
        );
    }

    /// **THE GATE.** A zero-step drill would look authored and do nothing.
    #[test]
    fn an_empty_path_is_rejected() {
        let err = drill("nothing", &[])
            .validate()
            .expect_err("an empty drill must be rejected");
        assert!(
            matches!(&err, SpecError::EmptyDrill(n) if n == "nothing"),
            "got: {err}"
        );
        assert_eq!(drill("nothing", &[]).terminal(), None);
    }

    #[test]
    fn terminals_are_exactly_the_leaf_rungs() {
        let terminals: Vec<&str> = DrillLevel::ALL
            .iter()
            .filter(|l| l.is_terminal())
            .map(|l| l.label())
            .collect();
        assert_eq!(terminals, vec!["logs", "diagnose", "xray"]);
        // And every terminal shares the deepest depth.
        for l in DrillLevel::ALL.iter().filter(|l| l.is_terminal()) {
            assert_eq!(l.depth(), 5);
        }
    }

    /// A path ending at a container (not a terminal) has no terminal rung —
    /// reported honestly as `None`, not defaulted to one.
    #[test]
    fn a_non_terminal_ending_reports_no_terminal() {
        let d = drill("to-container", &[DrillLevel::Pod, DrillLevel::Container]);
        d.validate().expect("valid descent");
        assert_eq!(d.terminal(), None);
    }
}
