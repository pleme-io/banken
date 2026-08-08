//! `discovery` — the typed RESTMapper banken owns, built from ONE aggregated
//! discovery document.
//!
//! # Why banken owns this instead of enabling it
//!
//! `theory/BANKEN.md`'s **C-discovery** ceiling, measured 2026-08-08. It is the
//! inverse of the C-watch lesson: there, a capability read as absent and was
//! merely *unenabled*; here the capability is genuinely **absent from kube-rs**
//! while being **rich on the wire**.
//!
//! - `kube_core::discovery::ApiResource` carries exactly five fields — group,
//!   version, api_version, kind, plural — and **discards `shortNames`,
//!   `categories` and `singularName`**, which `k8s_openapi`'s own `APIResource`
//!   proves the server sends. So `po` → pods is *not expressible* through
//!   kube-rs's discovery API.
//! - `kube-client` has **zero** aggregated-discovery support; its
//!   `Discovery::run()` costs N+2 sequential requests (68 against camelot-eks)
//!   where one aggregated GET returns 53 groups / 66 group-versions / 221
//!   resources in 243,880 bytes.
//! - `ApiResource::from_gvk` **guesses** the plural from a hardcoded
//!   pluralizer, so an irregularly-pluralized CRD resolves to a wrong URL and
//!   **404s silently**.
//!
//! This module is pure parsing + resolution: **no network**. The fetch lives in
//! the live backend; keeping the mapper pure is what makes every invariant
//! below unit-testable with no cluster.
//!
//! # Gate 0 — the illegal states this vocabulary corners
//!
//! | # | illegal state | how it is cornered | tier |
//! |---|---|---|---|
//! | 1 | a **guessed** plural | no constructor accepts one; entries come only from a parsed server document | truly-unrep *within this surface* |
//! | 2 | an **ambiguous** alias served first-wins | [`MapError::Ambiguous`] naming every candidate | parse-time-rejected |
//! | 3 | an **unknown** alias silently treated as a kind | [`MapError::Unknown`], with near-misses | parse-time-rejected |
//! | 4 | a **subresource** treated as a listable resource | subresources are parsed into their parent, never aliased | truly-unrep *within this surface* |
//! | 5 | a resource **that cannot be listed** offered for absorption | [`ApiResourceEntry::is_listable`] gates it; [`RestMapper::listable`] is the absorber's only door | only-mitigated (C-list-gate: a caller may still read `entries()` directly) |
//!
//! **State 2 is the same class this repo fixed one layer over on the same day.**
//! A `KUBECONFIG` merge list can declare one context name twice, and kube-rs
//! resolves it first-wins and silently (see `banken::live::resolve_context`).
//! Aliases have exactly that shape: **`events` is served by BOTH `core/v1` and
//! `events.k8s.io/v1`**, and `kubectl get events` silently picks one. Picking is
//! the bug; refusing and naming both is the fix.

use std::collections::BTreeMap;

use serde::Deserialize;

use crate::error::SpecError;

// ── the wire shapes (apidiscovery.k8s.io/v2) ──────────────────────────

/// The aggregated discovery document, as served for
/// `Accept: application/json;g=apidiscovery.k8s.io;v=v2;as=APIGroupDiscoveryList`.
#[derive(Debug, Deserialize)]
struct ApiGroupDiscoveryList {
    #[serde(default)]
    items: Vec<ApiGroupDiscovery>,
}

#[derive(Debug, Deserialize)]
struct ApiGroupDiscovery {
    #[serde(default)]
    metadata: GroupMeta,
    #[serde(default)]
    versions: Vec<VersionDiscovery>,
}

#[derive(Debug, Default, Deserialize)]
struct GroupMeta {
    /// Empty string for the **core** group — which is why this is a plain
    /// `String` with a default rather than an `Option`: `""` is a real,
    /// meaningful group name here, not a missing value.
    #[serde(default)]
    name: String,
}

#[derive(Debug, Deserialize)]
struct VersionDiscovery {
    version: String,
    #[serde(default)]
    resources: Vec<ResourceDiscovery>,
}

#[derive(Debug, Deserialize)]
struct ResourceDiscovery {
    /// The PLURAL, as the server states it. Never derived.
    resource: String,
    #[serde(rename = "responseKind")]
    response_kind: Option<ResponseKind>,
    #[serde(default)]
    scope: String,
    #[serde(rename = "singularResource", default)]
    singular_resource: String,
    #[serde(default)]
    verbs: Vec<String>,
    #[serde(rename = "shortNames", default)]
    short_names: Vec<String>,
    #[serde(default)]
    categories: Vec<String>,
    /// Parsed so subresources are ACCOUNTED FOR rather than ignored — but they
    /// are never turned into aliasable entries (Gate-0 state 4).
    #[serde(default)]
    subresources: Vec<SubresourceDiscovery>,
}

#[derive(Debug, Deserialize)]
struct ResponseKind {
    #[serde(default)]
    kind: String,
}

#[derive(Debug, Deserialize)]
struct SubresourceDiscovery {
    subresource: String,
}

// ── the typed surface ─────────────────────────────────────────────────

/// One listable/gettable API resource, exactly as the server described it.
///
/// Every field is **read from the server**. There is no constructor that takes
/// a hand-written plural, which is Gate-0 state 1: the `from_gvk`-style guess
/// that 404s silently has no way to be expressed here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiResourceEntry {
    group: String,
    version: String,
    kind: String,
    plural: String,
    singular: String,
    short_names: Vec<String>,
    categories: Vec<String>,
    namespaced: bool,
    verbs: Vec<String>,
    subresources: Vec<String>,
}

impl ApiResourceEntry {
    /// `""` for the core group.
    #[must_use]
    pub fn group(&self) -> &str {
        &self.group
    }
    #[must_use]
    pub fn version(&self) -> &str {
        &self.version
    }
    #[must_use]
    pub fn kind(&self) -> &str {
        &self.kind
    }
    /// The plural **as the server stated it** — never pluralized locally.
    #[must_use]
    pub fn plural(&self) -> &str {
        &self.plural
    }
    #[must_use]
    pub fn singular(&self) -> &str {
        &self.singular
    }
    #[must_use]
    pub fn short_names(&self) -> &[String] {
        &self.short_names
    }
    #[must_use]
    pub fn categories(&self) -> &[String] {
        &self.categories
    }
    #[must_use]
    pub fn is_namespaced(&self) -> bool {
        self.namespaced
    }
    #[must_use]
    pub fn subresources(&self) -> &[String] {
        &self.subresources
    }

    /// `"v1"` for the core group, `"<group>/<version>"` otherwise.
    #[must_use]
    pub fn api_version(&self) -> String {
        if self.group.is_empty() {
            self.version.clone()
        } else {
            let mut s = String::with_capacity(self.group.len() + 1 + self.version.len());
            s.push_str(&self.group);
            s.push('/');
            s.push_str(&self.version);
            s
        }
    }

    /// Can this resource be absorbed at all?
    ///
    /// **Both** verbs are required, and that is deliberate rather than strict:
    /// banken's absorb plane is a watch feed with a list-shaped bootstrap
    /// (`sendInitialEvents`), so a resource offering only `list` would
    /// silently degrade the whole plane back to polling — the exact
    /// unannounced-downgrade class `ListStrategy` refuses to have an `Auto`
    /// variant for.
    #[must_use]
    pub fn is_listable(&self) -> bool {
        self.has_verb("list") && self.has_verb("watch")
    }

    #[must_use]
    pub fn has_verb(&self, verb: &str) -> bool {
        self.verbs.iter().any(|v| v == verb)
    }

    /// Every name an operator may legitimately type for this resource.
    fn aliases(&self) -> Vec<String> {
        let mut out = Vec::with_capacity(2 + self.short_names.len());
        out.push(self.plural.clone());
        if !self.singular.is_empty() && self.singular != self.plural {
            out.push(self.singular.clone());
        }
        out.extend(self.short_names.iter().cloned());
        out
    }
}

/// Why an operator-typed alias did not resolve to exactly one resource.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MapError {
    /// **Two or more resources answer to this name.** `events` is the live
    /// example on every modern cluster: `core/v1` and `events.k8s.io/v1` both
    /// serve it. Picking one silently is the same false-calm class as a
    /// duplicated kubeconfig context name; the fix is to refuse and name both.
    Ambiguous {
        alias: String,
        /// `group/version Kind`, one per candidate, in a stable order.
        candidates: Vec<String>,
    },
    /// No resource answers to this name. Carries near-misses so the refusal
    /// costs a keystroke rather than a detour.
    Unknown {
        alias: String,
        did_you_mean: Vec<String>,
    },
    /// It resolved, but the server does not offer the verbs the absorb plane
    /// needs. Named rather than silently skipped: "this kind cannot be
    /// watched" and "this kind does not exist" are different answers.
    NotListable { alias: String, kind: String },
}

impl std::fmt::Display for MapError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Ambiguous { alias, candidates } => {
                write!(f, "`{alias}` is served by {} resources:", candidates.len())?;
                for c in candidates {
                    write!(f, "\n  {c}")?;
                }
                write!(
                    f,
                    "\nRefusing: the name does not identify one resource. \
                     Qualify it as <plural>.<group>."
                )
            }
            Self::Unknown {
                alias,
                did_you_mean,
            } => {
                write!(f, "no API resource named `{alias}`")?;
                if !did_you_mean.is_empty() {
                    write!(f, ". Did you mean:")?;
                    for d in did_you_mean {
                        write!(f, "\n  {d}")?;
                    }
                }
                Ok(())
            }
            Self::NotListable { alias, kind } => write!(
                f,
                "`{alias}` resolves to {kind}, which the server does not offer \
                 both `list` and `watch` on — banken cannot absorb it"
            ),
        }
    }
}

impl std::error::Error for MapError {}

/// The resolved resource map for one cluster.
///
/// Fields are private and [`RestMapper::from_aggregated_json`] is the only
/// constructor, so a mapper holding hand-written entries — the guessed-plural
/// class — cannot be built.
#[derive(Debug, Clone)]
pub struct RestMapper {
    entries: Vec<ApiResourceEntry>,
    /// alias → every entry index answering to it. A `Vec` rather than a single
    /// index **on purpose**: collapsing to one here is precisely the
    /// first-wins bug (Gate-0 state 2), so the ambiguity is preserved in the
    /// data structure and can only be resolved by refusing.
    by_alias: BTreeMap<String, Vec<usize>>,
}

impl RestMapper {
    /// Parse an `APIGroupDiscoveryList` document.
    ///
    /// # Errors
    ///
    /// [`SpecError::Interp`] when the document is not valid JSON or does not
    /// have the aggregated-discovery shape.
    pub fn from_aggregated_json(json: &str) -> Result<Self, SpecError> {
        let doc: ApiGroupDiscoveryList =
            serde_json::from_str(json).map_err(|e| SpecError::Interp {
                phase: "discovery".into(),
                message: {
                    let mut m = String::from("aggregated discovery document: ");
                    m.push_str(&e.to_string());
                    m
                },
            })?;

        let mut entries = Vec::new();
        for group in doc.items {
            for version in group.versions {
                for r in version.resources {
                    // A resource with no responseKind is not a thing banken can
                    // name; skipping is correct and is NOT a silent drop of
                    // something listable — the apiserver omits it exactly for
                    // entries that have no standalone kind.
                    let Some(kind) = r.response_kind.map(|k| k.kind).filter(|k| !k.is_empty())
                    else {
                        continue;
                    };
                    entries.push(ApiResourceEntry {
                        group: group.metadata.name.clone(),
                        version: version.version.clone(),
                        kind,
                        plural: r.resource,
                        singular: r.singular_resource,
                        short_names: r.short_names,
                        categories: r.categories,
                        namespaced: r.scope.eq_ignore_ascii_case("Namespaced"),
                        verbs: r.verbs,
                        // Gate-0 state 4: subresources are RECORDED on their
                        // parent and never become aliasable entries of their
                        // own. `pods/log` has no list path.
                        subresources: r.subresources.into_iter().map(|s| s.subresource).collect(),
                    });
                }
            }
        }

        let mut by_alias: BTreeMap<String, Vec<usize>> = BTreeMap::new();
        for (i, e) in entries.iter().enumerate() {
            for alias in e.aliases() {
                by_alias.entry(alias).or_default().push(i);
            }
        }

        Ok(Self { entries, by_alias })
    }

    /// Every resource the server described.
    #[must_use]
    pub fn entries(&self) -> &[ApiResourceEntry] {
        &self.entries
    }

    /// Resolve an operator-typed alias to exactly one resource.
    ///
    /// Accepts a bare alias (`po`, `pods`, `pod`) or a group-qualified one
    /// (`events.events.k8s.io`), which is how an ambiguity is *resolved* rather
    /// than merely reported.
    ///
    /// # Errors
    ///
    /// [`MapError::Ambiguous`] when >1 resource answers — never first-wins.
    /// [`MapError::Unknown`] when none does.
    pub fn resolve(&self, alias: &str) -> Result<&ApiResourceEntry, MapError> {
        // Group-qualified form first: `<plural>.<group>`. Checked before the
        // bare lookup so a qualified name can always disambiguate, even when
        // the bare plural is itself a legal alias somewhere.
        if let Some((plural, group)) = alias.split_once('.') {
            if let Some(e) = self
                .entries
                .iter()
                .find(|e| e.plural == plural && e.group == group)
            {
                return Ok(e);
            }
        }

        match self.by_alias.get(alias) {
            None => Err(MapError::Unknown {
                alias: alias.to_owned(),
                did_you_mean: self.near_misses(alias),
            }),
            Some(idxs) if idxs.len() == 1 => Ok(&self.entries[idxs[0]]),
            Some(idxs) => Err(MapError::Ambiguous {
                alias: alias.to_owned(),
                candidates: idxs
                    .iter()
                    .map(|&i| {
                        let e = &self.entries[i];
                        let mut s = e.api_version();
                        s.push(' ');
                        s.push_str(&e.kind);
                        s
                    })
                    .collect(),
            }),
        }
    }

    /// Resolve, and require that banken can actually absorb it.
    ///
    /// # Errors
    ///
    /// Everything [`Self::resolve`] returns, plus [`MapError::NotListable`].
    pub fn listable(&self, alias: &str) -> Result<&ApiResourceEntry, MapError> {
        let e = self.resolve(alias)?;
        if e.is_listable() {
            Ok(e)
        } else {
            Err(MapError::NotListable {
                alias: alias.to_owned(),
                kind: e.kind.clone(),
            })
        }
    }

    /// Cheap prefix/substring near-misses for an unknown alias.
    fn near_misses(&self, alias: &str) -> Vec<String> {
        let mut out: Vec<String> = self
            .by_alias
            .keys()
            .filter(|k| k.starts_with(alias) || alias.starts_with(k.as_str()))
            .take(5)
            .cloned()
            .collect();
        out.sort();
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A realistic slice of a real aggregated-discovery document, including
    /// the `events` collision every modern cluster actually serves.
    const DOC: &str = r#"{
      "kind":"APIGroupDiscoveryList","apiVersion":"apidiscovery.k8s.io/v2",
      "items":[
        {"metadata":{"name":""},"versions":[{"version":"v1","resources":[
          {"resource":"pods","responseKind":{"kind":"Pod"},"scope":"Namespaced",
           "singularResource":"pod","verbs":["get","list","watch","delete"],
           "shortNames":["po"],"categories":["all"],
           "subresources":[{"subresource":"log"},{"subresource":"exec"}]},
          {"resource":"events","responseKind":{"kind":"Event"},"scope":"Namespaced",
           "singularResource":"event","verbs":["get","list","watch"],
           "shortNames":["ev"]},
          {"resource":"nodes","responseKind":{"kind":"Node"},"scope":"Cluster",
           "singularResource":"node","verbs":["get","list","watch"],
           "shortNames":["no"]},
          {"resource":"bindings","responseKind":{"kind":"Binding"},"scope":"Namespaced",
           "singularResource":"binding","verbs":["create"]}
        ]}]},
        {"metadata":{"name":"events.k8s.io"},"versions":[{"version":"v1","resources":[
          {"resource":"events","responseKind":{"kind":"Event"},"scope":"Namespaced",
           "singularResource":"event","verbs":["get","list","watch"],
           "shortNames":["ev"]}
        ]}]},
        {"metadata":{"name":"apps"},"versions":[{"version":"v1","resources":[
          {"resource":"deployments","responseKind":{"kind":"Deployment"},"scope":"Namespaced",
           "singularResource":"deployment","verbs":["get","list","watch"],
           "shortNames":["deploy"],"categories":["all"],
           "subresources":[{"subresource":"scale"},{"subresource":"status"}]}
        ]}]}
      ]}"#;

    fn mapper() -> RestMapper {
        RestMapper::from_aggregated_json(DOC).expect("the fixture document parses")
    }

    /// **THE gate.** An alias two groups both serve is REFUSED, not picked.
    ///
    /// This is the same class as a duplicated kubeconfig context name, which
    /// this repo fixed one layer over on the same day: kube-rs resolves that
    /// first-wins and silently, and `kubectl get events` picks one of these
    /// two the same way. A first-wins map here would make banken show
    /// `core/v1` Events while the operator meant `events.k8s.io/v1`, with
    /// every row real and only the resource wrong.
    #[test]
    fn an_alias_two_groups_serve_is_refused() {
        let m = mapper();
        match m.resolve("events") {
            Err(MapError::Ambiguous { alias, candidates }) => {
                assert_eq!(alias, "events");
                assert_eq!(candidates.len(), 2, "both servers must be named");
                assert!(candidates.iter().any(|c| c.starts_with("v1 ")));
                assert!(
                    candidates
                        .iter()
                        .any(|c| c.starts_with("events.k8s.io/v1 "))
                );
            }
            other => panic!(
                "an ambiguous alias MUST be refused — picking one is the \
                 silent-wrong-resource class. Got: {other:?}"
            ),
        }
        // And the short name collides identically.
        assert!(matches!(m.resolve("ev"), Err(MapError::Ambiguous { .. })));
    }

    /// The ambiguity is RESOLVABLE, so the refusal costs a keystroke.
    #[test]
    fn a_group_qualified_alias_disambiguates() {
        let m = mapper();
        let e = m
            .resolve("events.events.k8s.io")
            .expect("a group-qualified alias resolves");
        assert_eq!(e.group(), "events.k8s.io");
        assert_eq!(e.kind(), "Event");
        assert_eq!(e.api_version(), "events.k8s.io/v1");
    }

    /// Plural, singular and short name all resolve — the thing kube-rs's
    /// `ApiResource` structurally cannot do, because it discards short names.
    #[test]
    fn plural_singular_and_shortname_all_resolve_to_one_entry() {
        let m = mapper();
        for alias in ["pods", "pod", "po"] {
            let e = m.resolve(alias).expect("pod alias resolves");
            assert_eq!(e.kind(), "Pod");
            assert_eq!(
                e.plural(),
                "pods",
                "the plural is the SERVER's, never guessed"
            );
            assert!(e.is_namespaced());
        }
        let d = m.resolve("deploy").expect("deploy resolves");
        assert_eq!(d.kind(), "Deployment");
        assert_eq!(d.api_version(), "apps/v1");
    }

    /// Cluster-scoped resources are recognised as such — the absorber needs
    /// this to decide whether a namespace even applies.
    #[test]
    fn scope_is_read_from_the_server() {
        let m = mapper();
        assert!(!m.resolve("no").expect("nodes").is_namespaced());
        assert!(m.resolve("po").expect("pods").is_namespaced());
    }

    /// Gate-0 state 4: a subresource is recorded on its parent and is NEVER
    /// an aliasable resource. `pods/log` has no list path of its own.
    #[test]
    fn subresources_are_recorded_but_never_aliasable() {
        let m = mapper();
        let pods = m.resolve("pods").expect("pods");
        assert!(pods.subresources().contains(&"log".to_string()));
        assert!(pods.subresources().contains(&"exec".to_string()));
        assert!(
            matches!(m.resolve("pods/log"), Err(MapError::Unknown { .. })),
            "a subresource must not resolve as a top-level resource",
        );
    }

    /// Gate-0 state 5: a resource the server will not let us watch is refused
    /// BY NAME, not silently skipped. "cannot be watched" and "does not
    /// exist" are different answers and must not collapse.
    #[test]
    fn a_resource_that_cannot_be_watched_is_refused_by_name() {
        let m = mapper();
        // `bindings` is real and create-only — it exists, so `resolve` finds it.
        assert!(m.resolve("bindings").is_ok());
        match m.listable("bindings") {
            Err(MapError::NotListable { kind, .. }) => assert_eq!(kind, "Binding"),
            other => panic!("a create-only resource must be NotListable, got {other:?}"),
        }
        assert!(m.listable("pods").is_ok());
    }

    /// An unknown alias names near-misses rather than just failing.
    #[test]
    fn an_unknown_alias_suggests() {
        let m = mapper();
        match m.resolve("pod") {
            Ok(_) => {}
            other => panic!("`pod` is a real singular: {other:?}"),
        }
        match m.resolve("podz") {
            Err(MapError::Unknown {
                alias,
                did_you_mean,
            }) => {
                assert_eq!(alias, "podz");
                assert!(
                    did_you_mean.iter().any(|s| s == "pod" || s == "pods"),
                    "near-misses should include the obvious one: {did_you_mean:?}"
                );
            }
            other => panic!("an unknown alias must be Unknown, got {other:?}"),
        }
    }
}
