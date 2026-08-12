//! permit — may this identity actually read that, *here*?
//!
//! # The question ronda could not answer
//!
//! [`crate::ronda`]'s ladder tops out at `Pods`: *this identity may list pods
//! somewhere on this cluster*. That is the right question for a cluster
//! chooser and the wrong one for a view. An operator with namespace-scoped
//! RBAC — the common shape on a shared estate — reaches `Pods` and then opens
//! `:secrets` in `kube-system` and gets an empty table.
//!
//! **An empty table and a forbidden one look identical**, and the difference
//! is the whole answer: one means the namespace is empty, the other means you
//! cannot see. A navigator that renders them the same way sends an operator to
//! debug a workload that is running fine.
//!
//! # Why an authorization query and not a failed read
//!
//! Kubernetes answers this directly. `SelfSubjectAccessReview` is a single
//! POST that asks the apiserver *"may I do `verb` on `resource` in
//! `namespace`?"* — evaluated against the same RBAC the real request would hit,
//! with no side effect and no dependence on anything existing. It is how
//! `kubectl auth can-i` works.
//!
//! Two properties make it the right primitive rather than "just try the read":
//!
//! - It distinguishes **forbidden** from **empty** before a table is drawn, so
//!   a view can say "you cannot read this" instead of showing nothing.
//! - It is safe to ask about a *destructive* verb. Asking "may I delete pods"
//!   changes nothing — which is what lets banken show an operator the shape of
//!   their own access without exercising any of it.
//!
//! # What a permit does NOT prove
//!
//! That the request would succeed. RBAC is one gate; an admission webhook, a
//! quota, a `NetworkPolicy`, or the object simply not existing are others.
//! [`Permit::Allowed`] means *authorization would not be the thing that stops
//! you* — which is a narrower claim than "this will work", and is stated that
//! way in [`Permit::describe`] so nothing downstream can round it up.

use banken_spec::error::SpecError;
use banken_spec::types::ResourceKind;

/// The apiserver's answer to one access question.
///
/// `Unknown` is a distinct variant and not an error, for the same reason
/// [`crate::ronda::Rung::Unknown`] is distinct from `Down`: "the review could
/// not be performed" and "you are not allowed" are opposite facts, and
/// collapsing them would have a navigator grey out a view the operator can
/// actually read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Permit {
    /// RBAC would not be the thing that stops you.
    Allowed,
    /// RBAC would refuse, with the apiserver's own reason when it gave one.
    Denied {
        /// The apiserver's reason, when present. Frequently empty — a bare
        /// deny carries no explanation, and inventing one would be worse.
        reason: String,
    },
    /// banken could not ask. NOT a denial.
    Unknown {
        /// Why the question could not be put.
        why: String,
    },
}

impl Permit {
    /// Whether a view keyed on this permit should render its table.
    ///
    /// `Unknown` renders — the review failing is not evidence the operator
    /// lacks access, and hiding a readable view on an inconclusive answer is
    /// the failure mode this type exists to avoid.
    #[must_use]
    pub fn should_render(&self) -> bool {
        !matches!(self, Self::Denied { .. })
    }

    /// A one-line operator-facing verdict.
    ///
    /// Deliberately says *authorization* rather than *this will work*: RBAC is
    /// one gate among several, and a permit that implied success would be
    /// rounding a narrow claim up to a broad one.
    #[must_use]
    pub fn describe(&self) -> String {
        match self {
            Self::Allowed => "authorization would allow this".to_owned(),
            Self::Denied { reason } if reason.is_empty() => {
                "forbidden by RBAC (the apiserver gave no reason)".to_owned()
            }
            Self::Denied { reason } => format!("forbidden by RBAC: {reason}"),
            Self::Unknown { why } => {
                format!("could not check — this is NOT a denial: {why}")
            }
        }
    }

    /// The stable machine token, for the MCP surface.
    #[must_use]
    pub fn token(&self) -> &'static str {
        match self {
            Self::Allowed => "allowed",
            Self::Denied { .. } => "denied",
            Self::Unknown { .. } => "unknown",
        }
    }
}

/// One access question.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ask {
    /// The verb — `list`, `get`, `watch`, `delete`, …
    pub verb: String,
    /// The resource kind.
    pub kind: ResourceKind,
    /// The namespace, or `None` for cluster-scope / all-namespaces.
    pub namespace: Option<String>,
}

impl Ask {
    /// The read banken actually performs for a view: `list`.
    #[must_use]
    pub fn to_list(kind: ResourceKind, namespace: Option<&str>) -> Self {
        Self {
            verb: "list".to_owned(),
            kind,
            namespace: namespace.map(ToOwned::to_owned),
        }
    }
}

/// The API-group + plural a `ResourceKind` is addressed by in an
/// authorization query.
///
/// Exhaustive on purpose: a new `ResourceKind` must name its wire identity
/// here or this stops compiling. Guessing a plural by lowercasing the label
/// would silently produce `replica_sets`, and an authorization query against a
/// resource that does not exist answers **allowed** — a wrong answer that
/// looks like good news.
#[must_use]
pub fn wire_identity(kind: ResourceKind) -> (&'static str, &'static str) {
    match kind {
        ResourceKind::Pod => ("", "pods"),
        ResourceKind::Service => ("", "services"),
        ResourceKind::ConfigMap => ("", "configmaps"),
        ResourceKind::Endpoints => ("", "endpoints"),
        ResourceKind::Namespace => ("", "namespaces"),
        ResourceKind::Node => ("", "nodes"),
        ResourceKind::Event => ("", "events"),
        ResourceKind::Deployment => ("apps", "deployments"),
        ResourceKind::ReplicaSet => ("apps", "replicasets"),
    }
}

/// The mockable border for an authorization question.
pub trait PermitEnv {
    /// Ask the apiserver whether this identity may perform `ask`.
    ///
    /// # Errors
    ///
    /// Implementations SHOULD return [`Permit::Unknown`] rather than an error
    /// for a review that could not be performed — a failed check is not a
    /// denial, and an `Err` at this seam invites a caller to treat it as one.
    /// The `Result` exists for a caller that genuinely cannot proceed.
    fn may(&self, ask: &Ask) -> Result<Permit, SpecError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **The distinction the module exists for.** A failed check must not read
    /// as a denial — that would hide a view the operator can actually use, and
    /// the operator would have no way to tell.
    #[test]
    fn an_unknown_permit_still_renders_and_a_denial_does_not() {
        assert!(Permit::Allowed.should_render());
        assert!(
            Permit::Unknown {
                why: "the review request timed out".into()
            }
            .should_render(),
            "an inconclusive check must not hide a readable view",
        );
        assert!(
            !Permit::Denied {
                reason: "no".into()
            }
            .should_render(),
        );
    }

    /// The wording must not round a narrow claim up. RBAC is one gate; a
    /// permit that said "this will work" would be wrong the first time an
    /// admission webhook refused.
    #[test]
    fn an_allowed_permit_claims_authorization_not_success() {
        let d = Permit::Allowed.describe();
        assert!(d.contains("authorization"), "{d}");
        assert!(!d.contains("will work"), "{d}");
        assert!(!d.contains("succeed"), "{d}");
    }

    /// An unknown must SAY it is not a denial, in the string a human reads —
    /// not only in the type a program matches on.
    #[test]
    fn an_unknown_permit_says_it_is_not_a_denial() {
        let d = Permit::Unknown {
            why: "no network".into(),
        }
        .describe();
        assert!(d.contains("NOT a denial"), "{d}");
        assert!(d.contains("no network"), "the cause survives: {d}");
    }

    /// A bare deny carries no reason, and inventing one would be worse than
    /// saying so.
    #[test]
    fn a_reasonless_denial_says_the_apiserver_gave_no_reason() {
        let d = Permit::Denied {
            reason: String::new(),
        }
        .describe();
        assert!(d.contains("no reason"), "{d}");
    }

    /// **Every kind must name its wire identity**, and the plurals must be the
    /// real ones. A guessed plural addresses a resource that does not exist,
    /// and an authorization query against a nonexistent resource answers
    /// ALLOWED — a wrong answer wearing the shape of good news.
    #[test]
    fn every_kind_has_a_real_wire_identity() {
        for kind in ResourceKind::ALL.iter().copied() {
            let (group, plural) = wire_identity(kind);
            assert!(!plural.is_empty(), "{kind:?} has no plural");
            assert!(
                !plural.contains('_'),
                "{kind:?} → `{plural}` looks like a lowercased label, not a \
                 Kubernetes plural",
            );
            assert_eq!(plural, plural.to_ascii_lowercase(), "{kind:?}");
            assert!(
                group.is_empty() || group == "apps",
                "{kind:?} names an unexpected group `{group}`",
            );
        }
        assert_eq!(
            wire_identity(ResourceKind::ReplicaSet),
            ("apps", "replicasets")
        );
        assert_eq!(wire_identity(ResourceKind::ConfigMap), ("", "configmaps"));
        assert_eq!(
            wire_identity(ResourceKind::Deployment),
            ("apps", "deployments")
        );
    }

    #[test]
    fn a_list_ask_carries_the_verb_banken_actually_performs() {
        let ask = Ask::to_list(ResourceKind::Pod, Some("kube-system"));
        assert_eq!(ask.verb, "list");
        assert_eq!(ask.namespace.as_deref(), Some("kube-system"));
        assert_eq!(Ask::to_list(ResourceKind::Node, None).namespace, None);
    }

    #[test]
    fn the_tokens_are_distinct_and_stable() {
        assert_eq!(Permit::Allowed.token(), "allowed");
        assert_eq!(
            Permit::Denied {
                reason: String::new()
            }
            .token(),
            "denied",
        );
        assert_eq!(Permit::Unknown { why: String::new() }.token(), "unknown",);
    }
}
