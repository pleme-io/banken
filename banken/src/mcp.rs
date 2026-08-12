//! The agent surface — banken's reads, and ONLY its reads, over MCP.
//!
//! # Why this is the differentiator
//!
//! Every cluster TUI is a human tool. An agent driving one drives a
//! *terminal*: it types keys at a screen, and whatever the screen can do, the
//! agent can do. That is backwards for the property banken is built around —
//! a human and an agent should not have the same powers, and the difference
//! should be structural rather than a prompt asking nicely.
//!
//! So the agent gets its own front door, and what is behind it is decided by
//! the type system. Every tool below is an OBSERVE: it calls a method on
//! [`ClusterEnv`], which has no unwitnessed-mutate method. There is no
//! `delete`, no `scale`, no `apply`, no `exec` — not disabled, not behind a
//! flag an operator might flip, **absent**. An agent cannot be talked into
//! calling a tool that does not exist.
//!
//! # What is deliberately NOT here, and why that is the design
//!
//! DECLARE and BREAK-GLASS are the other two postigo classes and both are
//! reachable from the TUI. Neither is exposed here:
//!
//! - **DECLARE** lowers to a full-manifest GitOps change. Its honest MCP shape
//!   returns a *proposed manifest and a branch*, never an applied one — which
//!   needs the PR-opening half of `ClusterEnv::declare`, still a preview.
//!   Exposing it early would let an agent believe it had changed something.
//!   `pending-banken: mcp-declare`.
//! - **BREAK-GLASS** is witnessed by construction: a `GlassRecord` names the
//!   operator who authorised it. An agent is not an operator, and inventing a
//!   witness so a tool signature type-checks would make the record a lie —
//!   the one thing a witnessed action may never be.
//!   `pending-banken: mcp-break-glass`.
//!
//! The reads are useful on their own: an agent can triage a failing workload
//! end to end — find the pod, read its status, pull its logs, read the events
//! — and then it must hand back to a human to change anything. That asymmetry
//! is the product, not a limitation of it.
//!
//! # The refusal shape is load-bearing
//!
//! Every arm that cannot answer returns `{"error": …}`, never an empty list.
//! To an agent those are opposite claims: an empty list reads as *the cluster
//! has none of these*, and a triage that concludes "the deployment is gone"
//! from a credential expiry is worse than one that stops. Same reasoning as
//! ronda's `Rung::Unknown` being a distinct value from `Rung::Down`.

use std::sync::Arc;

use banken_spec::env::ClusterEnv;
use banken_spec::types::ResourceKind;
use rmcp::{
    ServerHandler,
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{ServerCapabilities, ServerInfo},
    schemars, tool, tool_handler, tool_router,
};
use serde::Deserialize;
use serde_json::{Value, json};

/// The agent-facing server over one cluster env.
///
/// `Arc<dyn ClusterEnv>` rather than a type parameter on purpose: the fixture
/// and live envs must be interchangeable at the *call site* in `main`, where
/// the choice is a runtime argv decision. The trait is object-safe precisely
/// because it has no generic methods — the same smallness that makes it
/// auditable for the absence of a mutate arm.
#[derive(Clone)]
pub struct BankenMcp {
    env: Arc<dyn ClusterEnv + Send + Sync>,
    cluster: String,
    ronda: crate::ronda::Ronda,
    /// The per-verb authorization check, when the env can perform one.
    ///
    /// `Option` because the fixture has no apiserver to ask, and a fixture
    /// that answered "allowed" would be inventing an authorization result —
    /// the one kind of answer that must never be fabricated.
    permits: Option<Arc<dyn crate::permit::PermitEnv + Send + Sync>>,
    /// The break-glass ledger, exposed READ-ONLY.
    ///
    /// Reading the audit trail is an OBSERVE, and a valuable one: "a human
    /// exec'd into this pod twenty minutes ago" is often the whole answer to
    /// "why does this pod look like that", and it is invisible in every other
    /// read. Exposing the *record* is the opposite of exposing the *action* —
    /// the agent learns that glass was broken and still has no way to break
    /// any.
    glass: Option<crate::glass::GlassLedger>,
    tool_router: ToolRouter<Self>,
}

/// A successful read, rendered through [`kotae::Answer`].
///
/// The helpers below used to hand-roll this, and hand-rolling is what the
/// fleet measured 197 times across five MCP servers. What kotae adds beyond
/// deduplication is the `outcome` discriminant: a reader can now tell a found
/// answer from an empty one from a refusal by ONE field, instead of inferring
/// it from which keys happen to be present.
fn ok(v: &Value) -> String {
    kotae::Answer::found_value(v.clone()).render()
}

/// A read that could not be performed.
///
/// `blind`, deliberately — a `SpecError` from the env means banken could not
/// LOOK, which is not a denial and not an absence. That distinction was
/// carried in prose before ("a refusal is never shaped like an empty list");
/// it is now carried by the type, where a reader can act on it.
fn fail(e: &banken_spec::error::SpecError) -> String {
    kotae::Answer::blind(e.to_string()).render()
}

/// The caller named something banken does not know, and here is what it does.
fn refuse_unknown_kind(got: &str) -> String {
    kotae::Answer::refused(format!("unknown kind `{got}`"), legal_kinds()).render()
}

/// Resolve an authored view name or a wire label to a [`ResourceKind`].
///
/// Accepts the operator's own vocabulary (`pods`, `svc`, `deploy`, `no`) as
/// well as the serde wire labels (`config_map`, `replica_set`), because an
/// agent reading `banken_views` gets the latter and reading banken's prose
/// gets the former — and being strict about which would only ever produce a
/// retry, never a better answer.
fn kind_of(name: &str) -> Option<ResourceKind> {
    let n = name.trim().to_ascii_lowercase();
    ResourceKind::ALL
        .iter()
        .find(|k| k.label() == n)
        .copied()
        .or_else(|| {
            Some(match n.as_str() {
                "pod" | "pods" | "po" => ResourceKind::Pod,
                "service" | "services" | "svc" => ResourceKind::Service,
                "deployment" | "deployments" | "deploy" | "deploys" => ResourceKind::Deployment,
                "replicaset" | "replicasets" | "rs" => ResourceKind::ReplicaSet,
                "node" | "nodes" | "no" => ResourceKind::Node,
                "namespace" | "namespaces" | "ns" => ResourceKind::Namespace,
                "configmap" | "configmaps" | "cm" => ResourceKind::ConfigMap,
                "endpoint" | "endpoints" | "ep" => ResourceKind::Endpoints,
                "event" | "events" | "ev" => ResourceKind::Event,
                _ => return None,
            })
        })
}

/// Every wire label, for a refusal that teaches rather than one that
/// stonewalls.
fn legal_kinds() -> Vec<&'static str> {
    ResourceKind::ALL.iter().map(|k| k.label()).collect()
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ListInput {
    /// The resource kind — `pod`, `service`, `deployment`, `replica_set`,
    /// `node`, `namespace`, `config_map`, `endpoints`, `event`. Short forms
    /// (`po`, `svc`, `deploy`, `no`, `ns`, `cm`, `ep`, `rs`, `ev`) work too.
    pub kind: String,
    /// Namespace to scope to. Omit for all namespaces. Ignored for
    /// cluster-scoped kinds (`node`, `namespace`).
    pub namespace: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct GetInput {
    /// The resource kind (see `banken_list`).
    pub kind: String,
    /// The object's name.
    pub name: String,
    /// Namespace. Omit for cluster-scoped kinds.
    pub namespace: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct LogsInput {
    /// The pod's name.
    pub pod: String,
    /// The pod's namespace.
    pub namespace: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct EventsInput {
    /// Namespace to scope to. Omit for all namespaces.
    pub namespace: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct CanIInput {
    /// The verb — `list`, `get`, `watch`, `create`, `delete`, `patch`. Defaults
    /// to `list`, the read a view actually performs.
    #[serde(default)]
    pub verb: String,
    /// The resource kind (see `banken_list`).
    pub kind: String,
    /// The namespace to ask about. Omit for cluster-scope / all-namespaces.
    pub namespace: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct Empty {}

#[tool_router]
impl BankenMcp {
    /// Build the server over an env and the cluster it reads.
    #[must_use]
    pub fn new(
        env: Arc<dyn ClusterEnv + Send + Sync>,
        cluster: impl Into<String>,
        ronda: crate::ronda::Ronda,
    ) -> Self {
        let cluster = cluster.into();
        Self {
            env,
            permits: None,
            glass: crate::glass::GlassLedger::default_path()
                .map(|p| crate::glass::GlassLedger::at(p, cluster.clone())),
            cluster,
            ronda,
            tool_router: Self::tool_router(),
        }
    }

    /// Point the ledger reader at an explicit path (a test, or an operator
    /// with a non-default state dir).
    #[must_use]
    pub fn with_glass_ledger(mut self, ledger: crate::glass::GlassLedger) -> Self {
        self.glass = Some(ledger);
        self
    }

    /// Attach the authorization checker.
    ///
    /// Absent by default: only a live cluster can answer an authorization
    /// question, and a source that invented one would be fabricating the
    /// single kind of answer an agent must never receive fabricated.
    #[must_use]
    pub fn with_permits(
        mut self,
        permits: Arc<dyn crate::permit::PermitEnv + Send + Sync>,
    ) -> Self {
        self.permits = Some(permits);
        self
    }

    #[tool(
        description = "Which cluster this server reads, and which of the three action classes it does and does not expose. Worth calling first: it states plainly that every tool here is an OBSERVE and that no mutating tool exists, so you do not have to infer your own powers from an absence."
    )]
    pub async fn banken_capabilities(&self, Parameters(_): Parameters<Empty>) -> String {
        ok(&json!({
            "cluster": self.cluster,
            "classes": {
                "observe": "exposed — every tool on this server",
                "declare": "NOT exposed (pending-banken: mcp-declare). A DECLARE \
                            lowers to a full-manifest GitOps change; its honest \
                            shape returns a proposed manifest and a branch, and \
                            that half is still a preview in the TUI.",
                "breakGlass": "NOT exposed (pending-banken: mcp-break-glass). A \
                               break-glass is witnessed by construction — the \
                               record names the operator who authorised it. An \
                               agent is not an operator, and inventing a witness \
                               to satisfy a signature would make the record a lie."
            },
            "toChangeSomething": "Propose a full manifest to the owning GitOps \
                                  repository and let a human review it. There is no \
                                  apply tool here to find.",
            "guarantee": "banken_spec::env::ClusterEnv has no unwitnessed-mutate \
                          method, so the absence above is a property of the types \
                          rather than of this server's tool list."
        }))
    }

    #[tool(
        description = "List every resource kind this server can read, as the exact labels the other tools accept."
    )]
    pub async fn banken_views(&self, Parameters(_): Parameters<Empty>) -> String {
        ok(&json!({ "kinds": legal_kinds() }))
    }

    #[tool(
        description = "List resources of one kind as rows — name, namespace, and the columns banken's authored view renders. The primary read: what pods are in this namespace, which deployments exist, which nodes are NotReady."
    )]
    pub async fn banken_list(&self, Parameters(p): Parameters<ListInput>) -> String {
        let Some(kind) = kind_of(&p.kind) else {
            return refuse_unknown_kind(&p.kind);
        };
        match self.env.list_resources(kind, p.namespace.as_deref()) {
            Ok(rows) => ok(&json!({
                "cluster": self.cluster,
                "kind": kind.label(),
                "count": rows.len(),
                "rows": rows.iter().map(row_json).collect::<Vec<_>>(),
            })),
            Err(e) => fail(&e),
        }
    }

    #[tool(
        description = "Read ONE resource by name — the describe surface. Returns the same projection the table renders, so this and banken's own screen can never disagree about the same object."
    )]
    pub async fn banken_get(&self, Parameters(p): Parameters<GetInput>) -> String {
        let Some(kind) = kind_of(&p.kind) else {
            return refuse_unknown_kind(&p.kind);
        };
        match self.env.get_resource(kind, &p.name, p.namespace.as_deref()) {
            Ok(row) => ok(&json!({
                "cluster": self.cluster,
                "kind": kind.label(),
                "row": row_json(&row),
            })),
            Err(e) => fail(&e),
        }
    }

    #[tool(
        description = "Tail a pod's logs (a bounded snapshot, not a follow). The first thing to read when a workload is failing and its status alone does not say why."
    )]
    pub async fn banken_logs(&self, Parameters(p): Parameters<LogsInput>) -> String {
        // `follow: false` is fixed rather than exposed as a parameter. A follow
        // stream has no terminating read, and a tool call that never returns is
        // how an agent loop wedges — the TUI's pager can hold an open stream
        // because a human can press a key to leave it.
        match self.env.logs(&p.pod, &p.namespace, false) {
            Ok(s) => ok(&json!({
                "pod": s.pod,
                "lineCount": s.lines.len(),
                "lines": s.lines,
                "follow": s.follow,
            })),
            Err(e) => fail(&e),
        }
    }

    #[tool(
        description = "Cluster events — type, reason, involved object, message. Where the reason a pod will not start is usually written down, and the read that most often ends a triage."
    )]
    pub async fn banken_events(&self, Parameters(p): Parameters<EventsInput>) -> String {
        match self.env.events(p.namespace.as_deref()) {
            Ok(evs) => ok(&json!({
                "count": evs.len(),
                "events": evs.iter().map(|e| json!({
                    "type": e.kind,
                    "reason": e.reason,
                    "object": e.involved,
                    "message": e.message,
                })).collect::<Vec<_>>(),
            })),
            Err(e) => fail(&e),
        }
    }

    #[tool(
        description = "The break-glass ledger — every witnessed live action banken has performed, who authorised it, and whether it resolved. READ-ONLY, and often the missing half of a triage: 'a human exec'd into this pod twenty minutes ago' explains a state no other read can. An entry with no outcome is a session that crashed, was killed, or is still open."
    )]
    pub async fn banken_glass_ledger(&self, Parameters(_): Parameters<Empty>) -> String {
        let Some(ledger) = self.glass.as_ref() else {
            return ok(&json!({
                "error": "no break-glass ledger path could be resolved (no \
                          XDG_STATE_HOME and no HOME). This is NOT a claim that \
                          no break-glass has happened — it is banken being \
                          unable to look.",
            }));
        };
        match ledger.entries() {
            // The distinction the whole tool turns on: an EMPTY ledger means no
            // break-glass has been performed, which is a real finding. An
            // unreadable ledger means banken cannot see, which is not. They are
            // different JSON shapes here for exactly that reason.
            Ok(entries) => ok(&json!({
                "ledger": ledger.path().display().to_string(),
                "count": entries.len(),
                "unresolved": ledger.unresolved().map(|u| u.len()).unwrap_or(0),
                "entries": entries.iter().map(|e| json!({
                    "recordId": e.record_id,
                    "atUnixMs": e.at_unix_ms,
                    "cluster": e.cluster,
                    "selector": e.selector,
                    "witness": e.witness,
                    "runbook": e.runbook,
                    "outcome": e.outcome,
                })).collect::<Vec<_>>(),
                "note": "An entry with no `outcome` is UNRESOLVED — banken \
                         recorded the intent and never saw the session finish.",
            })),
            Err(e) => fail(&e),
        }
    }

    #[tool(
        description = "Ask the apiserver whether this identity may perform a verb on a resource kind, optionally in one namespace — `kubectl auth can-i`. Call this BEFORE concluding from an empty list that a workload is missing: an empty table and a forbidden one look identical, and only this tells them apart. Safe for destructive verbs — asking `may I delete pods` changes nothing."
    )]
    pub async fn banken_can_i(&self, Parameters(p): Parameters<CanIInput>) -> String {
        // The CALLER's error first, then the environment's. A typo'd kind is
        // wrong regardless of whether an apiserver is reachable, and reporting
        // the environmental limitation instead would send an agent to debug
        // banken's wiring over its own misspelling.
        let Some(kind) = kind_of(&p.kind) else {
            return refuse_unknown_kind(&p.kind);
        };
        let Some(permits) = self.permits.as_ref() else {
            return ok(&json!({
                "error": "this server has no apiserver to ask (it is serving the \
                          fixture source). This is NOT a denial — it is banken \
                          being unable to check.",
            }));
        };
        let verb = if p.verb.trim().is_empty() {
            "list".to_owned()
        } else {
            p.verb.trim().to_ascii_lowercase()
        };
        let ask = crate::permit::Ask {
            verb: verb.clone(),
            kind,
            namespace: p.namespace.clone(),
        };
        match permits.may(&ask) {
            Ok(permit) => ok(&json!({
                "cluster": self.cluster,
                "verb": verb,
                "kind": kind.label(),
                "namespace": p.namespace,
                "permit": permit.token(),
                "verdict": permit.describe(),
                "note": "`allowed` means AUTHORIZATION would not stop you — not \
                         that the request will succeed. Admission webhooks, \
                         quotas and network policy are separate gates. \
                         `unknown` is not a denial.",
            })),
            Err(e) => fail(&e),
        }
    }

    #[tool(
        description = "How far this identity actually gets against each configured cluster — the access ladder (unknown / down / network / serving / identity / pods) with the reason a climb stopped. Answers 'can I even read that cluster' BEFORE a read is attempted, and separates an expired credential from an unreachable network, which a failed read alone cannot."
    )]
    pub async fn banken_readiness(&self, Parameters(_): Parameters<Empty>) -> String {
        // `standings()`, NOT `positions()`. `positions` is the colour-ramp
        // projection and drops every context no round has measured yet — which
        // made this tool report `covered: 18` beside an empty list, i.e. the
        // eighteen clusters an agent most needs to hear about (we have not
        // looked yet) were the exact eighteen it could not see. Measured
        // against a real MCP client on 2026-08-12.
        let contexts: Vec<Value> = self
            .ronda
            .standings()
            .into_iter()
            .map(|(name, s)| {
                json!({
                    "context": name,
                    // The token to branch on; the label to quote. `label` is
                    // prose written for a human status line and is free to be
                    // reworded, so keying off it would break on a copy edit.
                    "rung": s.rung.token(),
                    "meaning": s.rung.label(),
                    "note": s.note,
                    "settled": s.rung.is_settled(),
                })
            })
            .collect();
        let note = if contexts.is_empty() {
            "No rounds are running. This server was started without a watchdog \
             (the fixture source, or a run that enumerated no contexts) — it is \
             NOT a claim that every cluster is down."
        } else {
            "A rung of `unknown` means no round has reported on that context \
             YET. It is not the same as `down`, and it is not evidence about \
             that cluster in either direction."
        };
        ok(&json!({
            "covered": self.ronda.covered(),
            "contexts": contexts,
            "note": note,
        }))
    }
}

fn row_json(r: &banken_spec::env::Row) -> Value {
    let mut cells = serde_json::Map::new();
    for (k, v) in &r.cells {
        cells.insert(k.clone(), Value::String(v.clone()));
    }
    json!({
        "name": r.name,
        "namespace": r.namespace,
        // The uid, not the name — the identity that survives a
        // delete-and-recreate. An agent comparing two reads needs the value
        // that actually distinguishes them.
        "uid": r.uid.as_str(),
        "resourceVersion": r.version,
        "cells": Value::Object(cells),
    })
}

// `router = self.tool_router` is NOT the default and is load-bearing.
//
// rmcp 1.x's `#[tool_handler]` defaults to `Self::tool_router()` — the STATIC
// constructor — so the cached field goes unread and the whole router is rebuilt
// on every `list_tools` and every `call_tool`. Nothing fails; the surface
// behaves identically and pays an allocation per call forever. The only signal
// was clippy's `field `tool_router` is never read`, which is exactly the kind
// of finding a silently-skipped lint run hides.
#[tool_handler(router = self.tool_router)]
impl ServerHandler for BankenMcp {
    // `ServerInfo` is `#[non_exhaustive]` as of rmcp 1.x, so the struct-literal
    // form is E0639 even with `..Default::default()` — field assignment is the
    // only way to build one from outside the crate, which is exactly the case
    // this lint exists to permit.
    #[allow(
        clippy::field_reassign_with_default,
        reason = "ServerInfo is #[non_exhaustive]; the struct-literal form is E0639"
    )]
    fn get_info(&self) -> ServerInfo {
        let mut info = ServerInfo::default();
        info.instructions = Some(
            "banken — observe-first cluster reads. EVERY tool here is an \
                 OBSERVE; there is no mutating tool, because the underlying \
                 `ClusterEnv` has no unwitnessed-mutate method. To change \
                 something, propose a full manifest to the owning GitOps \
                 repository for a human to review — do not look for an apply \
                 tool here, there isn't one. Call `banken_capabilities` for the \
                 exact split, and `banken_readiness` before concluding from a \
                 failed read that a workload is missing rather than that the \
                 cluster was unreachable."
                .into(),
        );
        info.capabilities = ServerCapabilities::builder().enable_tools().build();
        info
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixture::FixtureClusterEnv;

    fn server() -> BankenMcp {
        BankenMcp::new(
            Arc::new(FixtureClusterEnv::new()),
            "fixture",
            crate::ronda::Ronda::inert(),
        )
    }

    fn tool_names() -> Vec<String> {
        BankenMcp::tool_router()
            .list_all()
            .iter()
            .map(|t| t.name.to_string())
            .collect()
    }

    /// **THE PROPERTY THE WHOLE SURFACE EXISTS FOR.** Not "no mutating tool is
    /// enabled" — no mutating tool is DEFINED. An agent cannot be talked into
    /// calling one that is absent, which is a strictly stronger guarantee than
    /// one refused at runtime, because no prompt reaches it.
    ///
    /// Its honest tier is the same as the trait's own: CI-caught, not
    /// unrepresentable. An author *can* add a `#[tool] pub async fn
    /// banken_delete`; this is what fails when they do.
    #[test]
    fn no_mutating_tool_exists() {
        let names = tool_names();
        assert!(!names.is_empty(), "the router must expose something");
        for forbidden in [
            "delete",
            "apply",
            "scale",
            "exec",
            "patch",
            "create",
            "edit",
            "drain",
            "cordon",
            "evict",
            "restart",
            "rollout",
            "port_forward",
            "break_glass",
            "declare",
        ] {
            assert!(
                !names.iter().any(|n| n.contains(forbidden)),
                "`{forbidden}` must have no tool — the absence IS the primitive: {names:?}",
            );
        }
    }

    #[test]
    fn the_reads_are_all_present() {
        let names = tool_names();
        for expected in [
            "banken_capabilities",
            "banken_views",
            "banken_list",
            "banken_get",
            "banken_logs",
            "banken_events",
            "banken_readiness",
            "banken_glass_ledger",
            "banken_can_i",
        ] {
            assert!(
                names.iter().any(|n| n == expected),
                "missing {expected}: {names:?}",
            );
        }
    }

    /// Reading the break-glass ledger is an OBSERVE and is exposed; *breaking*
    /// glass is not. The ledger tool must report what a human did without
    /// giving the agent any way to do it — which is why `no_mutating_tool_exists`
    /// denies `break_glass` while this tool exists alongside it.
    #[tokio::test]
    async fn the_glass_ledger_is_readable_and_an_empty_one_is_a_finding() {
        use crate::glass::{GlassLedger, GlassOutcome};
        use banken_spec::env::WitnessedAction;
        use banken_spec::types::{OperatorId, RunbookRef};

        let dir = std::env::temp_dir().join(format!("banken-mcp-glass-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("tmp");
        let ledger = GlassLedger::at(dir.join("glass.jsonl"), "fixture");
        // Start clean — this path is reused across runs on the same pid.
        let _ = std::fs::remove_file(ledger.path());

        let server = BankenMcp::new(
            Arc::new(FixtureClusterEnv::new()),
            "fixture",
            crate::ronda::Ronda::inert(),
        )
        .with_glass_ledger(ledger.clone());

        // An empty ledger is a REAL answer: no break-glass has happened.
        let s = server.banken_glass_ledger(Parameters(Empty {})).await;
        let v: Value = serde_json::from_str(&s).expect("valid json");
        assert_eq!(v["count"], 0, "{s}");
        assert!(
            v["error"].is_null(),
            "an empty ledger is a finding, not a failure to look: {s}",
        );

        let w = ledger
            .record(&WitnessedAction {
                selector: "pod/api".into(),
                witness: OperatorId::new("drzzln").expect("non-blank"),
                runbook: RunbookRef("R.md".into()),
            })
            .expect("recorded");
        let s = server.banken_glass_ledger(Parameters(Empty {})).await;
        let v: Value = serde_json::from_str(&s).expect("valid json");
        assert_eq!(v["count"], 1, "{s}");
        assert_eq!(v["unresolved"], 1, "recorded and not yet resolved: {s}");
        assert_eq!(v["entries"][0]["witness"], "drzzln");
        assert!(v["entries"][0]["outcome"].is_null(), "{s}");

        ledger.resolve(&w, GlassOutcome::Opened).expect("resolved");
        let s = server.banken_glass_ledger(Parameters(Empty {})).await;
        let v: Value = serde_json::from_str(&s).expect("valid json");
        assert_eq!(v["unresolved"], 0, "{s}");
    }

    /// The capability statement must SAY that declare and break-glass are
    /// absent, and why. An agent that has to infer its own powers from an
    /// empty tool list will guess, and guessing wrong here means reporting
    /// having fixed something it could not touch.
    #[tokio::test]
    async fn capabilities_names_what_is_missing_and_why() {
        let s = server().banken_capabilities(Parameters(Empty {})).await;
        let v: Value = serde_json::from_str(&s).expect("valid json");
        let declare = v["classes"]["declare"].as_str().expect("declare");
        let glass = v["classes"]["breakGlass"].as_str().expect("breakGlass");
        assert!(declare.contains("NOT exposed"), "{declare}");
        assert!(glass.contains("NOT exposed"), "{glass}");
        assert!(glass.contains("witness"), "and says why: {glass}");
        assert!(
            v["toChangeSomething"]
                .as_str()
                .is_some_and(|s| !s.is_empty()),
            "a refusal must name the road that IS open: {s}",
        );
    }

    #[tokio::test]
    async fn listing_a_known_kind_returns_rows() {
        let s = server()
            .banken_list(Parameters(ListInput {
                kind: "pods".into(),
                namespace: None,
            }))
            .await;
        let v: Value = serde_json::from_str(&s).expect("valid json");
        assert_eq!(v["kind"], "pod");
        assert!(v["count"].as_u64().unwrap_or(0) > 0, "{s}");
        assert!(
            v["rows"][0]["uid"].as_str().is_some_and(|u| !u.is_empty()),
            "a row must carry the identity that survives a recreate: {s}",
        );
    }

    /// **An unknown kind names the legal set** rather than returning an empty
    /// list. An empty list reads as "the cluster has none of these", which
    /// would have an agent conclude a workload is gone when it merely
    /// misspelled the kind.
    #[tokio::test]
    async fn an_unknown_kind_is_refused_with_the_legal_set() {
        let s = server()
            .banken_list(Parameters(ListInput {
                kind: "poddz".into(),
                namespace: None,
            }))
            .await;
        let v: Value = serde_json::from_str(&s).expect("valid json");
        // The DISCRIMINANT is the assertion now. Before kotae a reader had to
        // infer "this was refused" from which keys happened to be present;
        // `outcome` says it in one field, so a refusal and an empty read can
        // no longer render alike even by accident.
        assert_eq!(v["outcome"], "refused", "{s}");
        assert!(
            v["because"].as_str().unwrap_or_default().contains("poddz"),
            "{s}",
        );
        assert!(v["legal"].as_array().is_some_and(|a| !a.is_empty()), "{s}");
        assert!(
            v["rows"].is_null() && v["count"].is_null(),
            "a refusal must not be shaped like an empty read: {s}",
        );
    }

    /// The operator's own vocabulary works — an agent handed `deploy` in prose
    /// must be able to pass it straight back — and so does every wire label
    /// `banken_views` hands out, which is the round-trip that matters most.
    #[test]
    fn every_advertised_label_round_trips_and_the_short_forms_resolve() {
        for label in legal_kinds() {
            assert!(
                kind_of(label).is_some(),
                "`banken_views` advertises `{label}` — it must be accepted",
            );
        }
        for (name, kind) in [
            ("deploy", ResourceKind::Deployment),
            ("svc", ResourceKind::Service),
            ("no", ResourceKind::Node),
            ("ev", ResourceKind::Event),
            ("cm", ResourceKind::ConfigMap),
            ("rs", ResourceKind::ReplicaSet),
            ("  PODS  ", ResourceKind::Pod),
        ] {
            assert_eq!(kind_of(name), Some(kind), "{name}");
        }
    }

    /// A ronda that has reported nothing must say so, rather than let an empty
    /// context list read as "every cluster is down" — the same distinction
    /// `Rung::Unknown` exists to carry.
    #[tokio::test]
    async fn an_unreported_readiness_is_not_a_claim_that_everything_is_down() {
        let s = server().banken_readiness(Parameters(Empty {})).await;
        let v: Value = serde_json::from_str(&s).expect("valid json");
        assert_eq!(v["covered"], 0);
        assert_eq!(v["contexts"].as_array().map(Vec::len), Some(0));
        let note = v["note"].as_str().expect("note");
        assert!(note.contains("NOT a claim"), "{note}");
    }

    /// **The regression the live probe caught.** Eighteen contexts seeded and
    /// none measured yet must be reported as eighteen *unknown* contexts — not
    /// as `covered: 18` beside an empty list, which is what `positions()`
    /// produced and which hides the clusters an agent most needs to hear about.
    #[tokio::test]
    async fn seeded_but_unmeasured_contexts_are_listed_as_unknown_not_omitted() {
        let (ronda, publisher) = crate::ronda::channel();
        publisher.seed(&["alpha-eks".to_owned(), "bravo-eks".to_owned()]);
        let s = BankenMcp::new(Arc::new(FixtureClusterEnv::new()), "fixture", ronda.clone())
            .banken_readiness(Parameters(Empty {}))
            .await;
        let v: Value = serde_json::from_str(&s).expect("valid json");

        assert_eq!(v["covered"], 2);
        let listed = v["contexts"].as_array().expect("contexts");
        assert_eq!(
            listed.len(),
            2,
            "the count and the list must describe the SAME set: {s}",
        );
        assert!(
            listed.iter().all(|c| c["rung"] == "unknown"),
            "an unmeasured context is `unknown`, never absent and never `down`: {s}",
        );
        // The projection that caused the bug still behaves as designed — this
        // is not a regression in the drawer's ramp, it is a second reader.
        assert!(
            ronda.positions().is_empty(),
            "positions() must still drop unmeasured contexts — a ramp may not \
             paint `not looked at yet` as a colour",
        );
    }

    /// **`banken_can_i` ASKS, it does not DO.** It accepts a destructive verb
    /// as an argument — "may I delete pods" — and that is safe by
    /// construction: a `SelfSubjectAccessReview` is a request/response
    /// envelope the apiserver answers and stores. The tool NAME carries no
    /// mutating verb, which is what `no_mutating_tool_exists` checks, and the
    /// distinction is worth pinning: an agent may learn the shape of its own
    /// access without exercising any of it.
    #[tokio::test]
    async fn can_i_asks_about_a_destructive_verb_without_performing_it() {
        // With no permit env (the fixture), it REFUSES rather than answering —
        // and says the refusal is not a denial, which is the whole point.
        let s = server()
            .banken_can_i(Parameters(CanIInput {
                verb: "delete".into(),
                kind: "pods".into(),
                namespace: Some("kube-system".into()),
            }))
            .await;
        let v: Value = serde_json::from_str(&s).expect("valid json");
        let err = v["error"].as_str().expect("a refusal");
        assert!(err.contains("NOT a denial"), "{err}");
        assert!(
            v["permit"].is_null(),
            "a refusal must not look like a verdict: {s}"
        );
    }

    /// An unknown kind is refused here too — and must not be mistaken for a
    /// denial, which would tell an agent it lacks access it may well have.
    #[tokio::test]
    async fn can_i_refuses_an_unknown_kind_without_calling_it_denied() {
        let s = server()
            .banken_can_i(Parameters(CanIInput {
                verb: "list".into(),
                kind: "poddz".into(),
                namespace: None,
            }))
            .await;
        let v: Value = serde_json::from_str(&s).expect("valid json");
        // The DISCRIMINANT is the assertion now. Before kotae a reader had to
        // infer "this was refused" from which keys happened to be present;
        // `outcome` says it in one field, so a refusal and an empty read can
        // no longer render alike even by accident.
        assert_eq!(v["outcome"], "refused", "{s}");
        assert!(
            v["because"].as_str().unwrap_or_default().contains("poddz"),
            "{s}"
        );
        assert_ne!(v["permit"], "denied", "{s}");
    }

    /// Whatever the env answers, the response is a typed shape — never
    /// silence, and never a success-shaped empty.
    #[tokio::test]
    async fn a_read_is_either_a_result_or_a_typed_refusal_never_silence() {
        let s = server()
            .banken_logs(Parameters(LogsInput {
                pod: "nonexistent".into(),
                namespace: "default".into(),
            }))
            .await;
        let v: Value = serde_json::from_str(&s).expect("valid json");
        assert!(
            v.get("error").is_some() || v.get("lines").is_some(),
            "either a typed refusal or a real read: {s}",
        );
    }
}
