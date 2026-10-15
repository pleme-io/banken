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

use banken_spec::types::ResourceKind;
use izumi_armazem::ArmazemConfig;
use izumi_mcp::{Refusal, StreamConfig, Streams};
use rmcp::{
    ServerHandler,
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{ServerCapabilities, ServerInfo},
    schemars, tool, tool_handler, tool_router,
};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::absorb::ListStrategy;
use crate::armazem::{Estoque, KindFonte, KindKey, LogFonte, LogKey, LogLine, SharedEnv};

/// The agent-facing server over one cluster env.
///
/// `Arc<dyn ClusterEnv>` rather than a type parameter on purpose: the fixture
/// and live envs must be interchangeable at the *call site* in `main`, where
/// the choice is a runtime argv decision. The trait is object-safe precisely
/// because it has no generic methods — the same smallness that makes it
/// auditable for the absence of a mutate arm.
/// Everything that is TRUE OF ONE CLUSTER, bundled so a switch moves it all at
/// once.
///
/// The four fields here are not merely related, they are mutually consistent or
/// they are wrong, and each pairing is already documented as an invariant
/// elsewhere in this file:
///
/// - `env` + `permits`: "the SAME env answers reads and authorization
///   questions, so `banken_can_i` can never disagree with `banken_list` about
///   which identity is asking".
/// - `cluster` + `glass`: the break-glass ledger is opened per-cluster
///   (`GlassLedger::at(path, cluster)`), so a stale pairing serves one estate's
///   break-glass history under another's name.
/// - `cluster` + `env`: the stamp on every read.
///
/// Swapping them as ONE value is what makes those invariants survive a context
/// switch. Four independently-assigned fields would let a switch land halfway.
struct Served {
    env: SharedEnv,
    cluster: String,
    permits: Option<Arc<dyn crate::permit::PermitEnv + Send + Sync>>,
    glass: Option<crate::glass::GlassLedger>,
    estoque: Estoque,
    rows: Arc<Streams<KindFonte>>,
    logs: Arc<Streams<LogFonte>>,
}

impl Served {
    fn new(env: SharedEnv, cluster: String, estoque: Estoque, streams: &StreamConfig) -> Self {
        Self {
            rows: Arc::new(Streams::new(estoque.rows.clone(), streams.clone())),
            logs: Arc::new(Streams::new(estoque.logs.clone(), streams.clone())),
            glass: crate::glass::GlassLedger::default_path()
                .map(|p| crate::glass::GlassLedger::at(p, cluster.clone())),
            env,
            cluster,
            permits: None,
            estoque,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Plano {
    pub armazem: ArmazemConfig,
    pub streams: StreamConfig,
    pub strategy: ListStrategy,
}

#[derive(Clone)]
pub struct BankenMcp {
    served: Arc<std::sync::RwLock<Served>>,
    ronda: crate::ronda::Ronda,
    plano: Plano,
    pinned: bool,
    tool_router: ToolRouter<Self>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct UseContextInput {
    /// A kubeconfig context name, as listed by `banken_readiness`.
    pub context: String,
}

/// A successful read, rendered through [`kotae::Answer`].
///
/// The helpers below used to hand-roll this, and hand-rolling is what the
/// fleet measured 197 times across five MCP servers. What kotae adds beyond
/// deduplication is the `outcome` discriminant: a reader can now tell a found
/// answer from an empty one from a refusal by ONE field, instead of inferring
/// it from which keys happen to be present.
/// A refusal, for the one case that is neither found nor empty nor a cluster
/// error: the served-cluster lock is poisoned, so banken does not know which
/// estate it would be reading.
///
/// A `refused` outcome rather than an empty `found`: kotae exists so a reader
/// can tell those apart in one field, and "I cannot safely say" is not "there
/// is nothing there".
fn refuse_poisoned() -> String {
    kotae::Answer::refused_flatly(
        "the served-cluster lock is poisoned — a context switch panicked \
         mid-swap. banken will not read a cluster it cannot name; restart the \
         server.",
        // No alternatives: there is no other cluster to suggest, because the
        // failure is precisely not knowing which one this is.
    )
    .render()
}

fn ok(v: &Value) -> String {
    kotae::Answer::found_value(v.clone()).render()
}

/// A read that could not be performed.
///
/// `blind`, deliberately — a `SpecError` from the env means banken could not
/// LOOK, which is not a denial and not an absence. That distinction was
/// carried in prose before ("a refusal is never shaped like an empty list");
/// it is now carried by the type, where a reader can act on it.
fn fail(cluster: &str, e: &banken_spec::error::SpecError) -> String {
    // ★ The cluster is named on the FAILURE path too, and this is where it
    // matters most. "the read failed" without "against which cluster" is
    // unplaceable once the served context can change under the caller — and
    // this server's own tool description tells an agent to distinguish "the
    // workload is missing" from "the cluster was unreachable". It cannot do
    // that if the failure does not say where it happened.
    //
    // Found by driving the live server: after switching context, a failed
    // `banken_logs` came back `outcome=blind` with no `cluster` field at all,
    // while every SUCCESSFUL read was stamped. The stamp test only covered the
    // Ok arms.
    // TIER-HONEST: this is a PREFIX, not a field. `kotae::Answer::blind` takes
    // only a string, so a `cluster` key on a blind answer is not expressible
    // through today's kotae API — a JSON-parsing reader gets the name inside
    // `because`, not beside it. Good enough to place a failure, weaker than
    // the successful reads' stamp, and the load-bearing fix belongs in kotae
    // (a target/context field on the non-found arms would serve every fleet
    // MCP server, not just this one).
    kotae::Answer::blind(format!("[{cluster}] {e}")).render()
}

/// The caller named something banken does not know, and here is what it does.
fn refusal(r: &Refusal) -> String {
    kotae::Answer::refused(r.error.clone(), [r.legal.clone()]).render()
}

fn page(cluster: &str, key: &str, first: bool, settle_ms: u64, answer: Value) -> String {
    if answer["outcome"] == "blind" {
        let why = answer["error"].as_str().unwrap_or("the read failed");
        return kotae::Answer::blind(format!("[{cluster}] {why}")).render();
    }
    if answer["key"] != key {
        return kotae::Answer::refused(
            format!(
                "that cursor belongs to `{}`, not `{key}`",
                answer["key"].as_str().unwrap_or("another read")
            ),
            ["pass a cursor with the same arguments that produced it, or omit it to start over"],
        )
        .render();
    }
    if first && answer["phase"]["phase"] == "absorbing" && answer["total"] == 0 {
        return kotae::Answer::blind(format!(
            "[{cluster}] `{key}` absorbed nothing within {settle_ms} ms and is still absorbing. \
             This is not an empty result: ask again, or watch it in `banken_absorb`. A \
             `streaming` list against an apiserver that does not negotiate \
             sendInitialEvents stalls here; `--list-strategy list-watch` does not."
        ))
        .render();
    }
    if first && answer["phase"]["phase"] == "degraded" && answer["total"] == 0 {
        let cause = answer["phase"]["cause"]
            .as_str()
            .unwrap_or("the stream failed");
        return kotae::Answer::blind(format!("[{cluster}] {cause}")).render();
    }
    let mut answer = answer;
    if let Some(o) = answer.as_object_mut() {
        o.insert("cluster".into(), Value::String(cluster.to_owned()));
    }
    ok(&answer)
}

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
    /// A `next` or `follow` value from a previous answer to the same question.
    /// `next` pages through the snapshot the first answer was cut from; `follow`
    /// returns only what changed since. Omit to start a fresh read.
    #[serde(default)]
    pub cursor: Option<String>,
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
    /// The container to follow. Omit for a single-container pod.
    #[serde(default)]
    pub container: Option<String>,
    /// A `next` or `follow` value from a previous answer to the same question.
    /// `next` pages through the snapshot the first answer was cut from; `follow`
    /// returns only what changed since. Omit to start a fresh read.
    #[serde(default)]
    pub cursor: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct EventsInput {
    /// Namespace to scope to. Omit for all namespaces.
    pub namespace: Option<String>,
    /// A `next` or `follow` value from a previous answer to the same question.
    /// `next` pages through the snapshot the first answer was cut from; `follow`
    /// returns only what changed since. Omit to start a fresh read.
    #[serde(default)]
    pub cursor: Option<String>,
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
    ///
    /// # Panics
    ///
    /// Never in a build with izumi-armazem's `vigy` feature, which banken
    /// always enables; the default configuration is otherwise invalid.
    #[must_use]
    pub fn new(env: SharedEnv, cluster: impl Into<String>, ronda: crate::ronda::Ronda) -> Self {
        Self::over_env(env, cluster, ronda, Plano::default())
            .expect("the default Armazém configuration is valid in a build with the vigy policy")
    }

    /// # Errors
    ///
    /// The Armazém configuration is invalid.
    pub fn over_env(
        env: SharedEnv,
        cluster: impl Into<String>,
        ronda: crate::ronda::Ronda,
        plano: Plano,
    ) -> Result<Self, izumi_armazem::ConfigError> {
        let estoque = Estoque::over_env(Arc::clone(&env), None, &plano.armazem)?;
        let _ = estoque.spawn_zeladores();
        Ok(Self::assemble(
            env,
            cluster.into(),
            estoque,
            ronda,
            plano,
            false,
        ))
    }

    /// # Errors
    ///
    /// The Armazém configuration is invalid.
    pub fn over_kube(
        env: Arc<crate::live::KubeClusterEnv>,
        ronda: crate::ronda::Ronda,
        plano: Plano,
    ) -> Result<Self, izumi_armazem::ConfigError> {
        let estoque = env.estoque(plano.strategy, &plano.armazem)?;
        let _ = estoque.spawn_zeladores();
        let cluster = env.context_name().unwrap_or_default();
        let permits = Arc::clone(&env) as Arc<dyn crate::permit::PermitEnv + Send + Sync>;
        Ok(Self::assemble(env, cluster, estoque, ronda, plano, false).with_permits(permits))
    }

    #[must_use]
    pub fn sharing(
        env: SharedEnv,
        cluster: impl Into<String>,
        estoque: Estoque,
        ronda: crate::ronda::Ronda,
        plano: Plano,
    ) -> Self {
        Self::assemble(env, cluster.into(), estoque, ronda, plano, true)
    }

    fn assemble(
        env: SharedEnv,
        cluster: String,
        estoque: Estoque,
        ronda: crate::ronda::Ronda,
        plano: Plano,
        pinned: bool,
    ) -> Self {
        Self {
            served: Arc::new(std::sync::RwLock::new(Served::new(
                env,
                cluster,
                estoque,
                &plano.streams,
            ))),
            ronda,
            plano,
            pinned,
            tool_router: Self::tool_router(),
        }
    }

    fn row_streams(&self) -> Option<(Arc<Streams<KindFonte>>, String)> {
        self.served
            .read()
            .ok()
            .map(|g| (Arc::clone(&g.rows), g.cluster.clone()))
    }

    fn log_streams(&self) -> Option<(Arc<Streams<LogFonte>>, String)> {
        self.served
            .read()
            .ok()
            .map(|g| (Arc::clone(&g.logs), g.cluster.clone()))
    }

    async fn read_kind(
        &self,
        key: KindKey,
        cursor: Option<&str>,
        render: &(dyn Fn(&banken_spec::env::Row) -> Value + Sync),
    ) -> String {
        let Some((streams, cluster)) = self.row_streams() else {
            return refuse_poisoned();
        };
        let name = key.to_string();
        match streams.read(key, cursor, render).await {
            Ok(answer) => page(
                &cluster,
                &name,
                cursor.is_none(),
                self.plano.streams.settle_ms,
                answer,
            ),
            Err(r) => refusal(&r),
        }
    }

    #[must_use]
    pub fn with_glass_ledger(self, ledger: crate::glass::GlassLedger) -> Self {
        if let Ok(mut g) = self.served.write() {
            g.glass = Some(ledger);
        }
        self
    }

    /// Attach the authorization checker.
    ///
    /// Absent by default: only a live cluster can answer an authorization
    /// question, and a source that invented one would be fabricating the
    /// single kind of answer an agent must never receive fabricated.
    #[must_use]
    pub fn with_permits(self, permits: Arc<dyn crate::permit::PermitEnv + Send + Sync>) -> Self {
        if let Ok(mut g) = self.served.write() {
            g.permits = Some(permits);
        }
        self
    }

    /// The cluster every read is stamped with.
    ///
    /// An accessor rather than direct field access at eight call sites: the
    /// served cluster becomes swappable at runtime (`banken_use_context`), and
    /// when it moves behind a lock this is the only line that changes. A stamp
    /// that silently went stale after a switch would be worse than no stamp —
    /// it would be a confident wrong answer about which estate replied.
    #[must_use]
    pub fn served_cluster(&self) -> String {
        self.cluster()
    }

    fn cluster(&self) -> String {
        self.served
            .read()
            .map(|g| g.cluster.clone())
            // A poisoned lock means a previous switch panicked mid-swap. Naming
            // that is far better than a plausible-looking cluster name: the one
            // thing a stamp must never do is be confidently wrong.
            .unwrap_or_else(|_| "<unknown: served-cluster lock poisoned>".into())
    }

    /// The env to read from, cloned out so no lock is held across an `.await`.
    fn env(&self) -> Option<SharedEnv> {
        self.served.read().ok().map(|g| Arc::clone(&g.env))
    }

    /// The authorization checker for the CURRENTLY served cluster.
    fn permits(&self) -> Option<Arc<dyn crate::permit::PermitEnv + Send + Sync>> {
        self.served.read().ok().and_then(|g| g.permits.clone())
    }

    /// The break-glass ledger for the CURRENTLY served cluster.
    fn glass(&self) -> Option<crate::glass::GlassLedger> {
        self.served.read().ok().and_then(|g| g.glass.clone())
    }

    #[tool(
        description = "Switch which cluster this server reads, by kubeconfig context \
                       name. Returns the context now served. Every subsequent read is \
                       stamped with it, so a switch can never silently mislead: check \
                       the `cluster` field of the next answer. Call `banken_readiness` \
                       for the context names available. This is an OBSERVE-plane \
                       operation — it changes what this server READS and nothing in \
                       any cluster."
    )]
    pub async fn banken_use_context(&self, Parameters(p): Parameters<UseContextInput>) -> String {
        let want = p.context.trim();
        if want.is_empty() {
            return kotae::Answer::refused_flatly(
                "a context name is required. Refused rather than defaulted: falling \
                 back to `current-context` would serve whichever estate the merged \
                 KUBECONFIG points at, to a reader who cannot see the mistake.",
            )
            .render();
        }
        if self.pinned {
            return kotae::Answer::refused(
                format!(
                    "this endpoint reads the same store as the banken screen serving `{}`; \
                     switching it here would split the agent from the operator",
                    self.cluster()
                ),
                ["switch the cluster in the banken screen, or run `banken mcp --context <name>` for a separate server"],
            )
            .render();
        }
        let previous = self.cluster();
        if want == previous {
            // Not an error, and not silence either: a caller that re-asserts the
            // current context deserves to hear that nothing moved, or it cannot
            // tell a no-op from a successful switch.
            return ok(&json!({
                "cluster": previous,
                "switched": false,
                "note": "already serving that context; nothing was reconnected",
            }));
        }

        // Connect BEFORE taking the write lock, and hold no lock across the
        // await. Two reasons, both load-bearing: a std RwLock guard is not
        // Send, and — more importantly — a failed connect must leave the
        // PREVIOUS cluster intact and readable. A switch that half-applied
        // would leave the server stamping a cluster it cannot read.
        // The plain connect, not `connect_with_context_staged`: staging exists to
        // paint progress onto a SCREEN, and this server has none — it speaks
        // JSON-RPC on stdio. The never-block-the-screen rule is a TUI concern;
        // borrowing its machinery here would add a reporter nothing reads.
        match crate::live::KubeClusterEnv::connect_with_context(want).await {
            Ok(env) => {
                let name = env.context_name().unwrap_or_else(|| want.to_string());
                let estoque = match env.estoque(self.plano.strategy, &self.plano.armazem) {
                    Ok(e) => e,
                    Err(e) => {
                        return kotae::Answer::refused_flatly(format!(
                            "the Armazém configuration is invalid ({e}). Still serving \
                             `{previous}` — nothing was switched."
                        ))
                        .render();
                    }
                };
                let _ = estoque.spawn_zeladores();
                let shared = Arc::new(env);
                let mut next = Served::new(
                    shared.clone() as SharedEnv,
                    name.clone(),
                    estoque,
                    &self.plano.streams,
                );
                next.permits = Some(shared as Arc<dyn crate::permit::PermitEnv + Send + Sync>);
                match self.served.write() {
                    Ok(mut g) => {
                        *g = next;
                        ok(&json!({
                            "cluster": name,
                            "switched": true,
                            "previous": previous,
                        }))
                    }
                    Err(_) => refuse_poisoned(),
                }
            }
            // The previous cluster is still served, and saying so is the point:
            // otherwise a caller cannot tell whether it is now reading the new
            // cluster, the old one, or nothing.
            Err(e) => kotae::Answer::refused_flatly(format!(
                "could not connect to `{want}`: {e}. Still serving \
                     `{previous}` — nothing was switched."
            ))
            .render(),
        }
    }

    #[tool(
        description = "Which cluster this server reads, and which of the three action classes it does and does not expose. Worth calling first: it states plainly that every tool here is an OBSERVE and that no mutating tool exists, so you do not have to infer your own powers from an absence."
    )]
    pub async fn banken_capabilities(&self, Parameters(_): Parameters<Empty>) -> String {
        ok(&json!({
            "cluster": self.cluster(),
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
        // Stamped like every other read. The kind list is not obviously
        // cluster-specific TODAY, but a reader that has switched context needs
        // every answer to say which cluster it describes — an unstamped answer
        // is one an agent cannot place, and "it happens not to vary" is a fact
        // about M0, not a contract.
        ok(&json!({ "cluster": self.cluster(), "kinds": legal_kinds() }))
    }

    #[tool(
        description = "List resources of one kind as rows — name, namespace, and the columns banken's authored view renders. The primary read: what pods are in this namespace, which deployments exist, which nodes are NotReady."
    )]
    pub async fn banken_list(&self, Parameters(p): Parameters<ListInput>) -> String {
        let Some(kind) = kind_of(&p.kind) else {
            return refuse_unknown_kind(&p.kind);
        };
        let namespace = p.namespace.as_deref().filter(|_| kind.is_namespaced());
        let key = match KindKey::within(kind, namespace) {
            Ok(k) => k,
            Err(e) => {
                return kotae::Answer::refused(
                    e.to_string(),
                    ["name a non-blank namespace, or omit it for all namespaces"],
                )
                .render();
            }
        };
        self.read_kind(key, p.cursor.as_deref(), &row_json).await
    }

    #[tool(
        description = "Read ONE resource by name — the describe surface. Returns the same projection the table renders, so this and banken's own screen can never disagree about the same object."
    )]
    pub async fn banken_get(&self, Parameters(p): Parameters<GetInput>) -> String {
        let Some(kind) = kind_of(&p.kind) else {
            return refuse_unknown_kind(&p.kind);
        };
        let Some(env) = self.env() else {
            return refuse_poisoned();
        };
        match env.get_resource(kind, &p.name, p.namespace.as_deref()) {
            Ok(row) => ok(&json!({
                "cluster": self.cluster(),
                "kind": kind.label(),
                "row": row_json(&row),
            })),
            Err(e) => fail(&self.cluster(), &e),
        }
    }

    #[tool(
        description = "Tail a pod's logs (a bounded snapshot, not a follow). The first thing to read when a workload is failing and its status alone does not say why."
    )]
    pub async fn banken_logs(&self, Parameters(p): Parameters<LogsInput>) -> String {
        let Some((streams, cluster)) = self.log_streams() else {
            return refuse_poisoned();
        };
        let key = LogKey {
            namespace: p.namespace.clone(),
            pod: p.pod.clone(),
            container: p.container.clone(),
        };
        let name = key.to_string();
        let first = p.cursor.is_none();
        let answer = match streams.read(key, p.cursor.as_deref(), &line_json).await {
            Ok(a) => a,
            Err(r) => return refusal(&r),
        };
        let body = page(&cluster, &name, first, self.plano.streams.settle_ms, answer);
        let mut v: Value = match serde_json::from_str(&body) {
            Ok(v) => v,
            Err(_) => return body,
        };
        if v["outcome"] != "found" {
            return body;
        }
        if let Some(o) = v.as_object_mut() {
            o.insert("pod".into(), Value::String(p.pod));
            o.insert("container".into(), json!(p.container));
            o.insert(
                "bounds".into(),
                Value::Array(
                    log_bounds(self.plano.armazem.journal_cap)
                        .iter()
                        .map(|b| {
                            json!({
                                "bound": b.token(),
                                "limit": b.limit(),
                                "meaning": b.meaning(),
                            })
                        })
                        .collect(),
                ),
            );
        }
        ok(&v)
    }

    #[tool(
        description = "Cluster events — type, reason, involved object, message. Where the reason a pod will not start is usually written down, and the read that most often ends a triage."
    )]
    pub async fn banken_events(&self, Parameters(p): Parameters<EventsInput>) -> String {
        let key = match KindKey::within(ResourceKind::Event, p.namespace.as_deref()) {
            Ok(k) => k,
            Err(e) => {
                return kotae::Answer::refused(
                    e.to_string(),
                    ["name a non-blank namespace, or omit it"],
                )
                .render();
            }
        };
        self.read_kind(key, p.cursor.as_deref(), &event_json).await
    }

    #[tool(
        description = "What this server has absorbed: every stream's key (`pod/*`, `deployment/flux-system`, `log/<ns>/<pod>`), its phase (absorbing / live / degraded / dormant / direct), change sequence, record count, how many readers hold it, whether its stream runs, and how long it has been idle or dormant. Separates 'what banken knows' from 'what was asked': a dormant key still holds its last rows and resumes from them."
    )]
    pub async fn banken_absorb(&self, Parameters(_): Parameters<Empty>) -> String {
        let Ok(g) = self.served.read() else {
            return refuse_poisoned();
        };
        ok(&json!({
            "cluster": g.cluster,
            "keys": g.estoque.inventory(),
            "config": self.plano.armazem,
            "zelador": {
                "rows": g.estoque.rows.zelador_error(),
                "logs": g.estoque.logs.zelador_error(),
            },
            "openCursors": g.rows.open_streams() + g.logs.open_streams(),
        }))
    }

    #[tool(
        description = "The break-glass ledger — every witnessed live action banken has performed, who authorised it, and whether it resolved. READ-ONLY, and often the missing half of a triage: 'a human exec'd into this pod twenty minutes ago' explains a state no other read can. An entry with no outcome is a session that crashed, was killed, or is still open."
    )]
    pub async fn banken_glass_ledger(&self, Parameters(_): Parameters<Empty>) -> String {
        let Some(ledger) = self.glass() else {
            return ok(&json!({
                "cluster": self.cluster(),
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
                // The ledger is per-cluster (`GlassLedger::at(path, cluster)`),
                // so an unstamped answer invites reading one estate's
                // break-glass history as another's.
                "cluster": self.cluster(),
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
            Err(e) => fail(&self.cluster(), &e),
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
        let Some(permits) = self.permits() else {
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
                "cluster": self.cluster(),
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
            Err(e) => fail(&self.cluster(), &e),
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
                    // `Standing::meaning()`, NOT `rung.label()`: label cannot
                    // tell "we did not ask" from "we asked and got nothing",
                    // and this tool reported a healthy engenho as
                    // "no apiserver reached" for exactly that reason.
                    "meaning": s.meaning(),
                    "reach": format!("{:?}", s.reach()).to_lowercase(),
                    "note": s.note,
                    "settled": s.is_settled(),
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

fn log_bounds(ring: usize) -> [banken_spec::env::LogBound; 2] {
    [
        banken_spec::env::LogBound::TailLines {
            limit: crate::live::LOG_TAIL_LINES,
        },
        banken_spec::env::LogBound::RingLines { limit: ring },
    ]
}

fn line_json(l: &LogLine) -> Value {
    json!({ "at": l.at, "text": l.text })
}

fn event_json(r: &banken_spec::env::Row) -> Value {
    let cell = |k: &str| {
        r.cells
            .iter()
            .find(|(key, _)| key == k)
            .map(|(_, v)| v.clone())
    };
    json!({
        "type": cell("event-type"),
        "reason": cell("reason"),
        "object": cell("object"),
        "count": cell("count"),
        "message": cell("message"),
        "name": r.name,
        "namespace": r.namespace,
        "uid": r.uid.as_str(),
    })
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
/// # Errors
///
/// The path exists and is not a socket, is a socket another process serves,
/// or cannot be bound.
pub async fn serve_socket(server: BankenMcp, path: &std::path::Path) -> Result<Servindo, String> {
    use rmcp::ServiceExt;
    use std::os::unix::fs::FileTypeExt;
    if let Ok(meta) = std::fs::symlink_metadata(path) {
        if !meta.file_type().is_socket() {
            return Err(format!(
                "`{}` exists and is not a socket; banken will not replace it",
                path.display()
            ));
        }
        if tokio::net::UnixStream::connect(path).await.is_ok() {
            return Err(format!(
                "`{}` is already served by another process",
                path.display()
            ));
        }
        std::fs::remove_file(path).map_err(|e| format!("{}: {e}", path.display()))?;
    }
    let listener =
        tokio::net::UnixListener::bind(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let handle = tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            let server = server.clone();
            tokio::spawn(async move {
                if let Ok(running) = server.serve(stream).await {
                    let _ = running.waiting().await;
                }
            });
        }
    });
    Ok(Servindo {
        handle,
        path: path.to_path_buf(),
    })
}

#[must_use = "dropping it stops serving and removes the socket"]
pub struct Servindo {
    handle: tokio::task::JoinHandle<()>,
    path: std::path::PathBuf,
}

impl Drop for Servindo {
    fn drop(&mut self) {
        self.handle.abort();
        let _ = std::fs::remove_file(&self.path);
    }
}

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
    use banken_spec::env::ClusterEnv;

    fn server() -> BankenMcp {
        BankenMcp::new(
            Arc::new(FixtureClusterEnv::new()),
            "fixture",
            crate::ronda::Ronda::inert(),
        )
    }

    struct LongLines(FixtureClusterEnv);

    impl ClusterEnv for LongLines {
        fn grip(
            &self,
            id: &banken_spec::env::ObjectId,
        ) -> Result<banken_spec::env::Grip, banken_spec::env::GripError> {
            self.0.grip(id)
        }
        fn list_resources(
            &self,
            kind: ResourceKind,
            ns: Option<&str>,
        ) -> Result<Vec<banken_spec::env::Row>, banken_spec::error::SpecError> {
            self.0.list_resources(kind, ns)
        }
        fn get_resource(
            &self,
            kind: ResourceKind,
            name: &str,
            ns: Option<&str>,
        ) -> Result<banken_spec::env::Row, banken_spec::error::SpecError> {
            self.0.get_resource(kind, name, ns)
        }
        fn logs(
            &self,
            pod: &str,
            _ns: &str,
            follow: bool,
        ) -> Result<banken_spec::env::LogStream, banken_spec::error::SpecError> {
            Ok(banken_spec::env::LogStream {
                pod: pod.to_owned(),
                lines: (0..509)
                    .map(|i| {
                        let mut l = format!("line {i:04} ");
                        l.push_str(&"{\"k\":\"v\"}".repeat(13));
                        l
                    })
                    .collect(),
                follow,
                ..Default::default()
            })
        }
        fn events(
            &self,
            ns: Option<&str>,
        ) -> Result<Vec<banken_spec::env::Event>, banken_spec::error::SpecError> {
            self.0.events(ns)
        }
        fn topology(
            &self,
            root: &banken_spec::env::ResourceRef,
        ) -> Result<banken_spec::env::DepTree, banken_spec::error::SpecError> {
            self.0.topology(root)
        }
        fn health_signals(
            &self,
            w: &banken_spec::env::Workload,
        ) -> Result<banken_spec::env::HealthReading, banken_spec::error::SpecError> {
            self.0.health_signals(w)
        }
        fn watch(
            &self,
            kind: ResourceKind,
            ns: Option<&str>,
        ) -> Result<banken_spec::env::WatchStream, banken_spec::error::SpecError> {
            self.0.watch(kind, ns)
        }
        fn declare(
            &self,
            change: banken_spec::env::DeclareChange,
        ) -> Result<banken_spec::env::ChangeRef, banken_spec::error::SpecError> {
            self.0.declare(change)
        }
        fn break_glass(
            &self,
            action: banken_spec::env::WitnessedAction,
        ) -> Result<banken_spec::env::GlassRecord, banken_spec::error::SpecError> {
            self.0.break_glass(action)
        }
    }

    fn tight(env: SharedEnv, budget: usize) -> BankenMcp {
        BankenMcp::over_env(
            env,
            "fixture",
            crate::ronda::Ronda::inert(),
            Plano {
                streams: StreamConfig {
                    budget_bytes: budget,
                    ..StreamConfig::default()
                },
                ..Plano::default()
            },
        )
        .expect("default config")
    }

    fn logs_of(cursor: Option<String>) -> Parameters<LogsInput> {
        Parameters(LogsInput {
            pod: "pangea-operator-0".into(),
            namespace: "pangea-system".into(),
            container: None,
            cursor,
        })
    }

    #[tokio::test]
    async fn a_long_log_pages_inside_the_budget_and_the_pages_reassemble_every_line() {
        let s = tight(Arc::new(LongLines(FixtureClusterEnv::new())), 8 * 1024);
        let mut seen: Vec<String> = Vec::new();
        let mut v: Value = serde_json::from_str(&s.banken_logs(logs_of(None)).await).unwrap();
        assert_eq!(v["total"], 509, "{v}");
        let mut pages = 0;
        loop {
            pages += 1;
            assert_eq!(v["outcome"], "found", "{v}");
            assert_eq!(v["cluster"], "fixture");
            let items = serde_json::to_vec(&v["items"]).unwrap().len();
            assert!(items <= 8 * 1024 + 2, "a page over budget: {items} bytes");
            assert!(
                v["bounds"]
                    .as_array()
                    .is_some_and(|b| b.iter().any(|x| x["bound"] == "ringLines")),
                "{v}"
            );
            for it in v["items"].as_array().unwrap() {
                seen.push(it["text"].as_str().unwrap()[..9].to_owned());
            }
            let Some(next) = v["next"].as_str().map(str::to_owned) else {
                break;
            };
            assert_eq!(v["truncated"]["by"], "budget-bytes");
            v = serde_json::from_str(&s.banken_logs(logs_of(Some(next))).await).unwrap();
        }
        assert!(pages > 1, "a 509-line log must not fit one 8 KiB page");
        let want: Vec<String> = (0..509).map(|i| format!("line {i:04}")).collect();
        assert_eq!(
            seen, want,
            "nothing is cut: the oldest line is reachable by paging"
        );
    }

    #[tokio::test]
    async fn a_list_pages_and_its_follow_returns_only_changes() {
        let s = tight(Arc::new(FixtureClusterEnv::new()), 600);
        let list = |cursor: Option<String>| ListInput {
            kind: "pods".into(),
            namespace: None,
            cursor,
        };
        let first: Value =
            serde_json::from_str(&s.banken_list(Parameters(list(None))).await).unwrap();
        assert_eq!(first["outcome"], "found", "{first}");
        assert_eq!(first["cluster"], "fixture");
        assert_eq!(first["key"], "pod/*");
        assert_eq!(first["total"], 5);
        assert!(
            first["count"].as_u64().unwrap() < 5,
            "600 bytes holds fewer than five rows: {first}"
        );
        assert!(
            first["items"][0]["uid"]
                .as_str()
                .is_some_and(|u| !u.is_empty())
        );
        let mut seen = first["count"].as_u64().unwrap();
        let mut next = first["next"].as_str().map(str::to_owned);
        while let Some(cursor) = next {
            let v: Value =
                serde_json::from_str(&s.banken_list(Parameters(list(Some(cursor)))).await).unwrap();
            assert_eq!(v["outcome"], "found", "{v}");
            seen += v["count"].as_u64().unwrap();
            next = v["next"].as_str().map(str::to_owned);
        }
        assert_eq!(seen, 5);
        let follow = first["follow"].as_str().unwrap().to_owned();
        let quiet: Value =
            serde_json::from_str(&s.banken_list(Parameters(list(Some(follow)))).await).unwrap();
        assert_eq!(quiet["outcome"], "found", "{quiet}");
        assert_eq!(
            quiet["changes"],
            json!([]),
            "a still fixture changes nothing: {quiet}"
        );
    }

    #[tokio::test]
    async fn a_second_question_about_the_same_kind_reads_the_same_stream() {
        let s = server();
        for _ in 0..3 {
            s.banken_list(Parameters(ListInput {
                kind: "po".into(),
                namespace: None,
                cursor: None,
            }))
            .await;
        }
        let v: Value = serde_json::from_str(&s.banken_absorb(Parameters(Empty {})).await).unwrap();
        let keys = v["keys"].as_array().expect("keys");
        let pods: Vec<&Value> = keys.iter().filter(|k| k["key"] == "pod/*").collect();
        assert_eq!(
            pods.len(),
            1,
            "one stream per key, however often it is asked: {v}"
        );
        assert_eq!(pods[0]["phase"]["phase"], "live", "{v}");
        assert_eq!(pods[0]["records"], 5, "{v}");
        assert_eq!(v["cluster"], "fixture");
        assert_eq!(v["config"]["mode"], "lazy");
        assert_eq!(v["config"]["policy"], "vigy");
    }

    #[tokio::test]
    async fn an_unknown_or_foreign_cursor_is_refused_never_an_empty_list() {
        let s = server();
        for bad in ["garbage", "p999.0.0", "f42.7"] {
            let v: Value = serde_json::from_str(
                &s.banken_list(Parameters(ListInput {
                    kind: "pods".into(),
                    namespace: None,
                    cursor: Some(bad.into()),
                }))
                .await,
            )
            .unwrap();
            assert_eq!(v["outcome"], "refused", "{bad}: {v}");
            assert!(v["legal"].as_array().is_some_and(|l| !l.is_empty()), "{v}");
            assert!(v["items"].is_null() && v["changes"].is_null(), "{v}");
        }
        let pods: Value = serde_json::from_str(
            &s.banken_list(Parameters(ListInput {
                kind: "pods".into(),
                namespace: None,
                cursor: None,
            }))
            .await,
        )
        .unwrap();
        let follow = pods["follow"].as_str().unwrap().to_owned();
        let crossed: Value = serde_json::from_str(
            &s.banken_list(Parameters(ListInput {
                kind: "deploy".into(),
                namespace: None,
                cursor: Some(follow),
            }))
            .await,
        )
        .unwrap();
        assert_eq!(
            crossed["outcome"], "refused",
            "a pods cursor cannot answer deployments: {crossed}"
        );
    }

    async fn call(
        lines: &mut tokio::io::Lines<tokio::io::BufReader<tokio::net::unix::OwnedReadHalf>>,
        write: &mut tokio::net::unix::OwnedWriteHalf,
        id: u64,
        tool: &str,
        args: Value,
    ) -> Value {
        use tokio::io::AsyncWriteExt;
        let req = json!({"jsonrpc": "2.0", "id": id, "method": "tools/call",
            "params": {"name": tool, "arguments": args}});
        write
            .write_all(format!("{req}\n").as_bytes())
            .await
            .expect("write");
        loop {
            let line = lines.next_line().await.expect("read").expect("an answer");
            let v: Value = serde_json::from_str(&line).expect("json-rpc");
            if v["id"] == id {
                let text = v["result"]["content"][0]["text"].as_str().expect("text");
                return serde_json::from_str(text).expect("tool json");
            }
        }
    }

    #[tokio::test]
    async fn the_socket_endpoint_reads_the_screens_own_store_and_will_not_switch_it() {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
        let env: SharedEnv = Arc::new(FixtureClusterEnv::new());
        let estoque =
            Estoque::over_env(Arc::clone(&env), None, &ArmazemConfig::default()).expect("config");
        let mut screen = estoque
            .rows
            .interest(KindKey::all(ResourceKind::Deployment));
        screen.settled(std::time::Duration::from_secs(5)).await;

        let dir = std::env::temp_dir().join(format!("banken-sock-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        let path = dir.join("mcp.sock");
        let server = BankenMcp::sharing(
            Arc::clone(&env),
            "fixture",
            estoque.clone(),
            crate::ronda::Ronda::inert(),
            Plano::default(),
        );
        let _served = serve_socket(server, &path).await.expect("bind");

        let stream = tokio::net::UnixStream::connect(&path)
            .await
            .expect("connect");
        let (read, mut write) = stream.into_split();
        let mut lines = tokio::io::BufReader::new(read).lines();
        let init = json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
            "protocolVersion": "2025-03-26", "capabilities": {},
            "clientInfo": {"name": "probe", "version": "0"}}});
        write
            .write_all(format!("{init}\n").as_bytes())
            .await
            .expect("init");
        lines
            .next_line()
            .await
            .expect("read")
            .expect("initialize answer");
        let done = json!({"jsonrpc": "2.0", "method": "notifications/initialized"});
        write
            .write_all(format!("{done}\n").as_bytes())
            .await
            .expect("initialized");

        let absorb = call(&mut lines, &mut write, 2, "banken_absorb", json!({})).await;
        let keys: Vec<&str> = absorb["keys"]
            .as_array()
            .expect("keys")
            .iter()
            .filter_map(|k| k["key"].as_str())
            .collect();
        assert!(
            keys.contains(&"deployment/*"),
            "the agent must see the key the screen opened, in the same store: {absorb}"
        );
        let switched = call(
            &mut lines,
            &mut write,
            3,
            "banken_use_context",
            json!({"context": "somewhere-else"}),
        )
        .await;
        assert_eq!(switched["outcome"], "refused", "{switched}");
        drop(screen);
        drop(_served);
        assert!(!path.exists(), "a dropped endpoint removes its socket");
        let _ = std::fs::remove_dir_all(&dir);
    }

    struct Silent;

    impl izumi_armazem::Source for Silent {
        type Key = KindKey;
        type Record = banken_spec::env::Row;

        fn open(
            &self,
            _key: KindKey,
            _resume: Option<String>,
            feed: izumi_armazem::Feed<banken_spec::env::Row>,
        ) -> izumi_armazem::BoxFuture<()> {
            Box::pin(async move { feed.until_stopped().await })
        }

        fn fetch(
            &self,
            _key: KindKey,
        ) -> izumi_armazem::BoxFuture<Result<Vec<banken_spec::env::Row>, String>> {
            Box::pin(async { Ok(Vec::new()) })
        }
    }

    #[tokio::test]
    async fn a_read_still_absorbing_is_blind_not_an_empty_list() {
        let env: SharedEnv = Arc::new(FixtureClusterEnv::new());
        let estoque = Estoque::new(
            crate::armazem::Fonte::new(Silent),
            crate::armazem::Fonte::new(crate::armazem::EnvLogSource::new(Arc::clone(&env))),
            &ArmazemConfig::default(),
        )
        .expect("config");
        let s = BankenMcp::sharing(
            env,
            "fixture",
            estoque,
            crate::ronda::Ronda::inert(),
            Plano {
                streams: StreamConfig {
                    settle_ms: 20,
                    ..StreamConfig::default()
                },
                ..Plano::default()
            },
        );
        let v: Value = serde_json::from_str(
            &s.banken_list(Parameters(ListInput {
                kind: "pods".into(),
                namespace: None,
                cursor: None,
            }))
            .await,
        )
        .unwrap();
        assert_eq!(v["outcome"], "blind", "{v}");
        assert!(
            v["because"]
                .as_str()
                .unwrap_or_default()
                .contains("still absorbing"),
            "{v}"
        );
        assert!(v["items"].is_null() && v["total"].is_null(), "{v}");
    }

    #[tokio::test]
    async fn an_expired_cursor_is_refused() {
        let s = BankenMcp::over_env(
            Arc::new(FixtureClusterEnv::new()),
            "fixture",
            crate::ronda::Ronda::inert(),
            Plano {
                streams: StreamConfig {
                    cursor_ttl_ms: 1,
                    ..StreamConfig::default()
                },
                ..Plano::default()
            },
        )
        .expect("config");
        let q = |cursor| ListInput {
            kind: "pods".into(),
            namespace: None,
            cursor,
        };
        let first: Value = serde_json::from_str(&s.banken_list(Parameters(q(None))).await).unwrap();
        let follow = first["follow"].as_str().unwrap().to_owned();
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        let v: Value =
            serde_json::from_str(&s.banken_list(Parameters(q(Some(follow)))).await).unwrap();
        assert_eq!(v["outcome"], "refused", "{v}");
        assert!(
            v["because"]
                .as_str()
                .unwrap_or_default()
                .contains("expired"),
            "{v}"
        );
    }

    #[tokio::test]
    async fn a_failed_read_names_the_cluster_it_failed_against() {
        let s = server();
        let here = s.cluster();
        // The fixture has no such pod, so this takes the failure path.
        let body = s
            .banken_logs(Parameters(LogsInput {
                pod: "no-such-pod-anywhere".into(),
                namespace: "default".into(),
                container: None,
                cursor: None,
            }))
            .await;
        assert!(
            body.contains(&here),
            "a failed read must name the cluster it failed against, got:\n{body}"
        );
    }

    /// Re-asserting the served context is a reported NO-OP, not silence.
    ///
    /// A caller that cannot tell "already there" from "switched" cannot tell a
    /// working switch from a broken one either.
    #[tokio::test]
    async fn switching_to_the_current_context_reports_no_op() {
        let s = server();
        let here = s.cluster();
        let body = s
            .banken_use_context(Parameters(UseContextInput {
                context: here.clone(),
            }))
            .await;
        assert!(body.contains("\"switched\": false"), "{body}");
        assert!(
            body.contains(&here),
            "the answer must name the cluster: {body}"
        );
    }

    /// An empty context is REFUSED, never defaulted to `current-context`.
    ///
    /// Defaulting is the entire hazard: it would serve whichever estate the
    /// merged KUBECONFIG points at, to a reader who cannot see the mistake.
    #[tokio::test]
    async fn switching_to_an_empty_context_is_refused() {
        let s = server();
        let before = s.cluster();
        for bad in ["", "   "] {
            let body = s
                .banken_use_context(Parameters(UseContextInput {
                    context: bad.into(),
                }))
                .await;
            assert!(
                body.contains("refused"),
                "an empty context must be refused, got: {body}"
            );
        }
        assert_eq!(
            s.cluster(),
            before,
            "a refusal must not move the served cluster"
        );
    }

    /// A FAILED switch leaves the previous cluster served AND readable.
    ///
    /// The half-applied switch is the dangerous failure: a server that stamped
    /// the new name while still holding the old env would be confidently wrong
    /// about which estate replied. The connect therefore happens before the
    /// write lock is taken, and this proves the consequence.
    #[tokio::test]
    async fn a_failed_switch_keeps_serving_the_previous_cluster() {
        let s = server();
        let before = s.cluster();
        let body = s
            .banken_use_context(Parameters(UseContextInput {
                context: "no-such-context-anywhere".into(),
            }))
            .await;
        assert!(body.contains("refused"), "{body}");
        assert!(
            body.contains(&before),
            "the refusal must say which cluster is STILL served, or the caller \
             cannot tell what it is now reading: {body}"
        );
        assert_eq!(
            s.cluster(),
            before,
            "a failed switch must not move the stamp"
        );

        // And the old cluster must still ANSWER, not merely be named.
        let listed = s
            .banken_list(Parameters(ListInput {
                kind: "pod".into(),
                namespace: None,
                cursor: None,
            }))
            .await;
        assert!(
            listed.contains(&before),
            "reads must still work against the previous cluster: {listed}"
        );
    }

    /// EVERY single-cluster read names the cluster it came from.
    ///
    /// This is the invariant that makes runtime context switching safe. banken
    /// refuses to ride `current-context` because an agent has no peripheral
    /// vision for a title bar naming the wrong estate — a stamp on every
    /// answer is what replaces that vision. Once reads are stamped, a switch
    /// cannot silently mislead: the very next answer says which cluster
    /// replied.
    ///
    /// Measured 2026-08-28, BEFORE this test existed: 4 of 9 tools did not
    /// stamp. `banken_logs` was the worst of them — log lines carry no
    /// intrinsic hint of origin and are the read most likely to be quoted back
    /// or reasoned over, so unstamped logs after a switch are unplaceable.
    ///
    /// `banken_readiness` is deliberately exempt and it is the one honest
    /// exemption: it reports on EVERY context at once and stamps each row with
    /// its own name, so a single top-level cluster field would be a false
    /// claim that the whole answer came from one place.
    #[tokio::test]
    async fn every_single_cluster_read_stamps_its_cluster() {
        let s = server();
        let answers: Vec<(&str, String)> = vec![
            (
                "capabilities",
                s.banken_capabilities(Parameters(Empty {})).await,
            ),
            ("views", s.banken_views(Parameters(Empty {})).await),
            (
                "glass_ledger",
                s.banken_glass_ledger(Parameters(Empty {})).await,
            ),
        ];
        for (name, body) in answers {
            assert!(
                body.contains("\"cluster\""),
                "`banken_{name}` returned an answer that does not name its \
                 cluster; after a context switch a reader cannot place it:\n{body}"
            );
        }

        // The read most in need of the stamp gets it checked by NAME, not just
        // swept up in the loop above — a future refactor that drops it should
        // fail on a line that says "logs".
        let logs = s
            .banken_logs(Parameters(LogsInput {
                pod: "banken-0".into(),
                namespace: "default".into(),
                container: None,
                cursor: None,
            }))
            .await;
        assert!(
            logs.contains("\"cluster\""),
            "banken_logs must name its cluster — log lines carry no intrinsic \
             hint of which estate produced them:\n{logs}"
        );
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
                cursor: None,
            }))
            .await;
        let v: Value = serde_json::from_str(&s).expect("valid json");
        assert_eq!(v["key"], "pod/*");
        assert!(v["count"].as_u64().unwrap_or(0) > 0, "{s}");
        assert!(
            v["items"][0]["uid"].as_str().is_some_and(|u| !u.is_empty()),
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
                cursor: None,
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
            v["items"].is_null() && v["count"].is_null(),
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
                container: None,
                cursor: None,
            }))
            .await;
        let v: Value = serde_json::from_str(&s).expect("valid json");
        assert!(
            v.get("because").is_some() || v.get("items").is_some(),
            "either a typed refusal or a real read: {s}",
        );
    }
}
