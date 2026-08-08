//! `KubeClusterEnv` — the live-cluster OBSERVE backend (BANKEN.md §VI M0
//! destination-tier / §VII "M0 read for a rio cluster").
//!
//! # Tier: PROVEN LIVE (measured 2026-07-31) — `pending-banken: live-read` CLOSED
//!
//! This backend's read path has been exercised against a real apiserver:
//! `camelot-eks` (EKS v1.33, AWS SSO exec-credential auth) returned **109
//! pods**, matching `kubectl get pods -A` at the same moment, and those rows
//! rendered through the full `PodTable` → `draw_pod_table` pipeline. Two
//! independent runs, both recorded in the commit that closed the row:
//!
//! - `tests/live_read.rs` (`#[ignore]`d, feature `live`) — the whole chain
//!   with an assertion at each joint, ending on a golden-frame `TestBackend`
//!   grid;
//! - the **binary itself** in a real 120×28 PTY
//!   (`banken --live --context camelot-eks`), snapshotted mid-run.
//!
//! **What that measurement cost, and why it is the whole point of the row:**
//! `--features live` had *compiled clean* for weeks while the first live read
//! aborted the process — kube 0.99's `rustls-tls` selects no rustls crypto
//! provider, so the first TLS handshake panicked in
//! `rustls/src/crypto/mod.rs:249`. A green build was never evidence.
//! See [`ensure_crypto_provider`].
//!
//! Still NOT proven and deliberately unchanged: everything below
//! `list_resources` for pods. `get_resource`, `logs`, `events`, `topology`,
//! `health_signals`, `declare` and `break_glass` all still return typed
//! "not yet wired" errors — do not read "banken reads a live cluster" as
//! "banken does anything else live". AGE renders `-` for every row (the
//! `creation_timestamp` derivation needs a clock; a `-` beats a wrong value).
//!
//! **NO SHELL.** This reads pods via the typed `kube` apiserver client
//! against the current kubeconfig context — never a `kubectl` subprocess.
//! The org NO-SHELL rule holds.
//!
//! Compiled only under the `live` feature so the default (fixture) build
//! stays lean. It implements the **same** [`ClusterEnv`] seam the fixture
//! env does — the app runtime is byte-identical against either source.
//!
//! Only the OBSERVE read half (`list_resources` for pods) is implemented;
//! DECLARE opens a branch+PR (M3) and BREAK-GLASS routes through the raw
//! kubernetes MCP (the §IX named interim executor) — neither is a live
//! mutation this backend performs, so both return a typed
//! `SpecError::Interp` "not yet wired" rather than a silent wrong `Ok`.

use std::path::PathBuf;

use banken_spec::env::{
    ChangeRef, ClusterEnv, DeclareChange, DepTree, Event, GlassRecord, HealthReading, LogStream,
    ResourceRef, Row, WatchStream, WitnessedAction, Workload,
};
use banken_spec::error::SpecError;
use banken_spec::types::ResourceKind;

use crate::absorb::{Ev, Folded, ListStrategy, Publisher, Replica, SyncPhase};
use futures::StreamExt;
use k8s_openapi::api::core::v1::Pod;
use kube::api::ListParams;
use kube::config::{KubeConfigOptions, Kubeconfig};
use kube::runtime::{WatchStreamExt, watcher};
use kube::{Api, Client, Config};

/// The kubeconfig's `current-context`, when it can be read.
///
/// Exposed so a caller can *name the hazard* in a refusal: the merged
/// KUBECONFIG on an operator workstation routinely points somewhere other
/// than the estate they mean to look at, and telling them which context they
/// would have silently landed on is what turns a refusal into a one-keystroke
/// fix.
#[must_use]
pub fn kubeconfig_current_context() -> Option<String> {
    Kubeconfig::read()
        .ok()
        .and_then(|kc| kc.current_context)
        .filter(|c| !c.is_empty())
}

/// Install the process-wide rustls crypto provider, once.
///
/// **This is not boilerplate — it is the fix for a measured runtime panic.**
/// kube 0.99's `rustls-tls` feature pulls rustls 0.23 with
/// `log,logging,std,tls12` and selects **no** crypto provider, so rustls has
/// no process default and the first TLS handshake panics inside
/// `rustls/src/crypto/mod.rs:249` rather than returning an error. Measured
/// 2026-07-31: `--features live` compiled clean, and the first live read
/// aborted the process. A clean compile was never evidence the backend reads,
/// and this is precisely why `pending-banken: live-read` was a real row and
/// not paperwork.
///
/// Installing explicitly (rather than relying on rustls's
/// pick-from-crate-features path) makes the fix immune to a future dependency
/// enabling `aws-lc-rs` as well — which would turn the "exactly one provider"
/// inference back into the same panic.
///
/// `install_default` returns `Err` only when a provider is *already*
/// installed, which is the desired end state, so it is deliberately discarded
/// rather than propagated.
fn ensure_crypto_provider() {
    let _already_installed = rustls::crypto::ring::default_provider().install_default();
}

/// Every context name the kubeconfig declares, in file order.
///
/// Empty means the kubeconfig could not be read at all — which is different
/// from "declares no contexts", and both are honest outcomes to report rather
/// than defaults to paper over.
#[must_use]
pub fn kubeconfig_context_names() -> Vec<String> {
    Kubeconfig::read()
        .map(|kc| kc.contexts.into_iter().map(|c| c.name).collect())
        .unwrap_or_default()
}

/// Every kubeconfig file `KUBECONFIG` names, in the order they merge.
///
/// `KUBECONFIG` is a **`:`-separated merge list**, not a single path. That is
/// not a detail — it is the whole reason [`resolve_context`] exists.
#[must_use]
pub fn kubeconfig_files() -> Vec<PathBuf> {
    match std::env::var_os("KUBECONFIG") {
        Some(v) => std::env::split_paths(&v)
            .filter(|p| !p.as_os_str().is_empty())
            .collect(),
        // The single-file default. Reported as a one-element list so callers
        // never branch on "merged or not".
        None => std::env::var_os("HOME")
            .map(|h| vec![PathBuf::from(h).join(".kube").join("config")])
            .unwrap_or_default(),
    }
}

/// A context name that resolved to exactly ONE declaration, and the apiserver
/// that declaration names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedContext {
    /// The context name, as asked for.
    pub name: String,
    /// The `cluster.server` URL the context resolves to. **This is the field
    /// that makes "the right cluster" checkable** — a name is not unique, a
    /// URL is what actually gets dialled.
    pub server: String,
    /// The single kubeconfig file that declared it.
    pub file: PathBuf,
}

/// Why a `--context` could not be turned into exactly one apiserver.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContextError {
    /// **The silent-wrong-cluster class.** Two or more files in the merge list
    /// declare this context name. kube-rs (and client-go) resolve this
    /// first-wins and say NOTHING: `append_new_named`
    /// (`kube-client-0.99.0/src/config/file_config.rs`) filters out every later
    /// entry whose name already exists — no error, no warning. So the operator
    /// asks for a name, gets whichever file happened to sort first, and every
    /// row they read is real. Only the cluster is wrong.
    ///
    /// Measured 2026-08-08: `engenho-local` named a local apiserver in one file
    /// and `engenho-local.quero.cloud` in `~/.kube/config`. banken read the
    /// remote one and looked entirely healthy doing it.
    Ambiguous {
        name: String,
        /// Every file declaring the name, in merge order. The first is the one
        /// that would silently have won.
        files: Vec<PathBuf>,
    },
    /// No file declares it. Carries what IS available, so the refusal costs a
    /// keystroke rather than a detour.
    NotFound {
        name: String,
        available: Vec<String>,
    },
    /// A file in the merge list could not be read or parsed.
    Unreadable { file: PathBuf, message: String },
}

impl std::fmt::Display for ContextError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Ambiguous { name, files } => {
                write!(
                    f,
                    "context `{name}` is declared by {} kubeconfig files",
                    files.len()
                )?;
                for (i, p) in files.iter().enumerate() {
                    write!(f, "\n  {}. {}", i + 1, p.display())?;
                    if i == 0 {
                        write!(f, "   <- would silently win")?;
                    }
                }
                write!(
                    f,
                    "\nRefusing: the name does not identify one cluster. \
                     Point KUBECONFIG at a single file, or rename one context."
                )
            }
            Self::NotFound { name, available } => {
                write!(f, "no kubeconfig context named `{name}`. Available:")?;
                for a in available {
                    write!(f, "\n  {a}")?;
                }
                Ok(())
            }
            Self::Unreadable { file, message } => {
                write!(
                    f,
                    "kubeconfig {} could not be read: {message}",
                    file.display()
                )
            }
        }
    }
}

impl std::error::Error for ContextError {}

/// Resolve a context name against **each kubeconfig file separately**, and
/// refuse unless exactly one declares it.
///
/// # Why this cannot delegate to kube-rs
///
/// `Kubeconfig::read()` merges the whole list *before* anyone can look, and the
/// merge is lossy by design: a duplicate name is dropped silently. By the time
/// a caller holds the merged config, the evidence that the name was ambiguous
/// is gone. The only place the question is answerable is before the merge —
/// which is here.
///
/// # Errors
///
/// [`ContextError::Ambiguous`] when >1 file declares `name` — the whole point.
/// [`ContextError::NotFound`] when none does. [`ContextError::Unreadable`] when
/// a listed file cannot be parsed.
pub fn resolve_context(name: &str) -> Result<ResolvedContext, ContextError> {
    let mut declaring: Vec<(PathBuf, Option<String>)> = Vec::new();
    let mut available: Vec<String> = Vec::new();

    for file in kubeconfig_files() {
        let kc = match Kubeconfig::read_from(&file) {
            Ok(kc) => kc,
            // A missing entry in a merge list is normal (client-go tolerates
            // it); an unreadable one that EXISTS is not.
            Err(e) if !file.exists() => {
                let _absent = e;
                continue;
            }
            Err(e) => {
                return Err(ContextError::Unreadable {
                    file,
                    message: e.to_string(),
                });
            }
        };

        for ctx in &kc.contexts {
            if !available.contains(&ctx.name) {
                available.push(ctx.name.clone());
            }
            if ctx.name == name {
                // Resolve the cluster this context points at, within THIS file
                // — a context and its cluster entry can be split across files,
                // but the common case is co-located and a missing server is
                // reported rather than invented.
                let server = ctx.context.as_ref().and_then(|c| {
                    kc.clusters
                        .iter()
                        .find(|cl| cl.name == c.cluster)
                        .and_then(|cl| cl.cluster.as_ref())
                        .and_then(|cl| cl.server.clone())
                });
                declaring.push((file.clone(), server));
            }
        }
    }

    match declaring.len() {
        0 => Err(ContextError::NotFound {
            name: name.to_owned(),
            available,
        }),
        1 => {
            let (file, server) = declaring.remove(0);
            Ok(ResolvedContext {
                name: name.to_owned(),
                server: server.unwrap_or_else(|| "<no server declared>".to_owned()),
                file,
            })
        }
        _ => Err(ContextError::Ambiguous {
            name: name.to_owned(),
            files: declaring.into_iter().map(|(f, _)| f).collect(),
        }),
    }
}

/// A live cluster env reading through the typed `kube` client against the
/// current kubeconfig context. Constructed async (the client resolves the
/// kubeconfig + builds the TLS stack); reads are blocking-from-the-caller's
/// view via a small runtime handle so the sync [`ClusterEnv`] trait is
/// satisfied without changing the shipped seam.
pub struct KubeClusterEnv {
    client: Client,
    handle: tokio::runtime::Handle,
    /// The kubeconfig's `current-context`, when it could be read.
    ///
    /// `kube::Client` does not surface which context it resolved, so this is
    /// read separately from the kubeconfig. `None` means UNKNOWN, and a
    /// `(defbancada)` then REFUSES to pre-warm a session rather than emitting
    /// `--context ""` — which would open the operator's troubleshooting panes
    /// on whatever cluster their shell happens to point at. See
    /// [`banken_spec::bancada`].
    context_name: Option<String>,
    /// The apiserver URL that was actually dialled, when it is known.
    ///
    /// `None` on the [`Self::connect`] path, which never resolves a named
    /// context and so cannot claim one. See [`Self::server`].
    server: Option<String>,
}

impl KubeClusterEnv {
    /// Build a live env from the current kubeconfig context.
    ///
    /// # Errors
    ///
    /// Returns a `SpecError::Interp { phase: "connect" }` if the kubeconfig
    /// cannot be resolved or the client cannot be constructed (the honest
    /// failure when no cluster / VPN is up — never a fake success).
    pub async fn connect() -> Result<Self, SpecError> {
        ensure_crypto_provider();
        let client = Client::try_default().await.map_err(|e| SpecError::Interp {
            phase: "connect".into(),
            message: e.to_string(),
        })?;
        // Best-effort, and its absence is meaningful rather than defaulted:
        // an unreadable kubeconfig means banken does not know which cluster
        // it is on, which is exactly when a pre-warmed session must refuse.
        //
        // Note this is READ SEPARATELY from what `try_default` resolved. The
        // two agree in practice, but nothing here *proves* they do — which is
        // one more reason [`Self::connect_with_context`] is the path the CLI
        // takes: there, the name banken reports and the name banken connected
        // with are the same value.
        let context_name = kubeconfig_current_context();
        Ok(Self {
            client,
            handle: tokio::runtime::Handle::current(),
            context_name,
            // `try_default` does not report the URL it resolved, and inventing
            // one by re-reading the kubeconfig would be the same
            // probably-the-same-thing guess this constructor already carries on
            // `context_name`. `None` is the honest value.
            server: None,
        })
    }

    /// Build a live env against an **explicitly named** kubeconfig context.
    ///
    /// This is the constructor the CLI uses, and the reason it exists is a
    /// measured hazard rather than a convenience: [`Self::connect`] rides
    /// `Client::try_default`, which resolves the kubeconfig's
    /// `current-context` — and on a merged operator KUBECONFIG that is
    /// routinely a *different estate* than the one the operator means to
    /// look at. Reading the wrong cluster silently is the same failure class
    /// [`banken_spec::bancada`] already refuses one layer down (a pane opened
    /// on "whatever context the shell happens to be on"); it is refused here
    /// too, by making the caller name the context.
    ///
    /// The name is carried into [`Self::context_name`] **as the value that
    /// was connected with**, not re-read from the kubeconfig afterwards — so
    /// the `(:context cluster)` a `(defbancada)` resolves is the same string
    /// that selected the apiserver, and the two cannot drift.
    ///
    /// # Errors
    ///
    /// A `SpecError::Interp { phase: "connect" }` when the kubeconfig cannot
    /// be read, when it declares no such context, or when the client cannot
    /// be built from it. A missing context is an error naming what was asked
    /// for — never a silent fall back to the current one, which would defeat
    /// the entire point of the flag.
    pub async fn connect_with_context(context: &str) -> Result<Self, SpecError> {
        ensure_crypto_provider();
        // Resolve BEFORE connecting. An ambiguous name has no path past this
        // line, so "banken read the wrong cluster and looked fine doing it"
        // has no code path rather than a warning nobody reads.
        let resolved = resolve_context(context).map_err(|e| SpecError::Interp {
            phase: "connect".into(),
            message: e.to_string(),
        })?;
        let options = KubeConfigOptions {
            context: Some(context.to_owned()),
            ..Default::default()
        };
        let config = Config::from_kubeconfig(&options)
            .await
            .map_err(|e| SpecError::Interp {
                phase: "connect".into(),
                message: {
                    let mut m = String::from("kubeconfig context `");
                    m.push_str(context);
                    m.push_str("`: ");
                    m.push_str(&Self::error_chain(&e));
                    m
                },
            })?;
        let client = Client::try_from(config).map_err(|e| SpecError::Interp {
            phase: "connect".into(),
            message: Self::error_chain(&e),
        })?;
        Ok(Self {
            client,
            handle: tokio::runtime::Handle::current(),
            context_name: Some(context.to_owned()),
            server: Some(resolved.server),
        })
    }

    /// The apiserver URL this env actually dialled, when it is known.
    ///
    /// A context NAME does not identify a cluster — two files in a `KUBECONFIG`
    /// merge list may declare the same one. The URL does. Surfacing it is what
    /// lets an operator check "the right cluster" instead of trusting a label.
    #[must_use]
    pub fn server(&self) -> Option<&str> {
        self.server.as_deref()
    }

    /// The kubeconfig context this env is reading, when it is known.
    ///
    /// Built by [`Self::connect_with_context`] it is the name that was
    /// connected with; built by [`Self::connect`] it is a separate read of the
    /// kubeconfig's `current-context` and is therefore only *probably* the
    /// same thing. The CLI takes the former path for exactly that reason.
    #[must_use]
    pub fn context_name(&self) -> Option<String> {
        self.context_name.clone()
    }

    /// Block on `fut` using the captured runtime handle. Used to satisfy the
    /// synchronous [`ClusterEnv`] read methods from an async client.
    fn block<F, T>(&self, fut: F) -> T
    where
        F: std::future::Future<Output = T>,
    {
        tokio::task::block_in_place(|| self.handle.block_on(fut))
    }

    /// Start absorbing pods into `publisher` and return the task handle.
    ///
    /// This is the watch half of BANKEN.md's C-watch ceiling, and the ceiling
    /// turned out to be a fact about banken's Cargo feature list rather than
    /// about kube-rs: `kube-runtime` 0.99 already ships every piece below.
    ///
    /// # Why `streaming_lists()`
    ///
    /// It collapses the initial LIST and the WATCH into one call
    /// (`sendInitialEvents=true` + `resourceVersionMatch=NotOlderThan`,
    /// terminated by an `InitialEventsEnd` bookmark). The apiserver never
    /// buffers a whole list body, so the first row reaches the operator at the
    /// first streamed chunk instead of after a 2.0 s full-list round trip.
    /// Verified serving on `camelot-eks` (v1.33) on 2026-08-08.
    ///
    /// # Why `default_backoff()` is not optional
    ///
    /// A bare `watcher()` has **no backoff at all** — kube-rs's own comment at
    /// `watcher.rs:750-752` is *"This will normally happen immediately"*. When
    /// the AWS SSO *session* expires (8–12 h) every request errors, and an
    /// un-backed-off loop re-invokes the `aws` exec-credential plugin per
    /// attempt: a hot loop spawning a subprocess. This is the single mandatory
    /// line in the function.
    ///
    /// # What it deliberately does NOT do
    ///
    /// It does not carry identity. A `Row` has no `uid`, so the replica is keyed
    /// on `(namespace, name)` — unique at an instant, **not** across
    /// delete-and-recreate. Acting on a row is therefore no safer than before;
    /// that is `pending-banken: grip`, and pretending otherwise here would be
    /// the round-up this repo keeps catching.
    #[must_use]
    pub fn spawn_pod_absorber(
        &self,
        publisher: Publisher,
        strategy: ListStrategy,
    ) -> tokio::task::JoinHandle<()> {
        let api: Api<Pod> = Api::all(self.client.clone());
        self.handle.spawn(async move {
            let mut replica = Replica::default();
            // The strategy is NAMED, never probed. `ListWatch` sends no
            // parameters a minimal apiserver need not implement, which is what
            // lets banken read a conformance-partial backend; `Streaming`
            // collapses LIST+WATCH into one call against a server that
            // negotiates `sendInitialEvents`. There is no third arm that picks
            // for you — see `ListStrategy`.
            let config = match strategy {
                ListStrategy::Streaming => watcher::Config::default().streaming_lists(),
                ListStrategy::ListWatch => watcher::Config::default(),
            };
            let stream = watcher(api, config).default_backoff();
            futures::pin_mut!(stream);

            while let Some(next) = stream.next().await {
                match next {
                    Ok(event) => {
                        let folded = match event {
                            watcher::Event::Init => replica.fold(Ev::Init),
                            watcher::Event::InitApply(p) => match pod_to_row(p) {
                                Some(r) => replica.fold(Ev::InitApply(r)),
                                None => Folded::Quiet,
                            },
                            watcher::Event::InitDone => replica.fold(Ev::InitDone),
                            watcher::Event::Apply(p) => match pod_to_row(p) {
                                Some(r) => replica.fold(Ev::Apply(r)),
                                None => Folded::Quiet,
                            },
                            watcher::Event::Delete(p) => match pod_to_row(p) {
                                Some(r) => replica.fold(Ev::Delete(r)),
                                None => Folded::Quiet,
                            },
                        };
                        // Only a set-changing fold reaches the publisher, and the
                        // publisher gates again on the content hash. Two gates
                        // because they catch different things: this one skips the
                        // work of materialising rows mid-init, that one skips the
                        // wake when a resourceVersion moved but no visible cell did.
                        match folded {
                            Folded::Quiet => {}
                            Folded::Changed | Folded::InitComplete => {
                                publisher.publish(replica.rows(), SyncPhase::Synced);
                            }
                        }
                    }
                    // The rows are KEPT and relabelled, never blanked: a transient
                    // apiserver failure must not erase a table an operator is
                    // reading. What changes is the claim banken makes about them.
                    Err(e) => {
                        publisher.publish(
                            replica.rows(),
                            SyncPhase::Degraded {
                                cause: e.to_string(),
                            },
                        );
                    }
                }
            }
            publisher.mark_stopped();
        })
    }

    /// The `Accept` header that asks for **aggregated** discovery.
    ///
    /// One request for the whole cluster's resource map, versus kube-rs's
    /// `Discovery::run()` at N+2 sequential requests (68 against camelot-eks).
    /// See `banken_spec::discovery` for why banken owns this at all.
    const AGGREGATED_DISCOVERY_ACCEPT: &'static str =
        "application/json;g=apidiscovery.k8s.io;v=v2;as=APIGroupDiscoveryList";

    /// Fetch the cluster's resource map in ONE request.
    ///
    /// # Errors
    ///
    /// `SpecError::Interp { phase: "discovery" }` when the request fails or
    /// the body is not an aggregated-discovery document.
    ///
    /// **There is deliberately no fallback to legacy per-group discovery.** A
    /// silent downgrade to 68 requests is the same unannounced-degradation
    /// class `ListStrategy` refuses to have an `Auto` variant for: it would
    /// turn "this server is old" into a latency mystery instead of an answer.
    /// A server that does not serve aggregated discovery gets a refusal that
    /// says so.
    /// **TWO** requests, not one, and that is a fact about Kubernetes rather
    /// than a shortcoming here: the core group lives at `/api` and the named
    /// groups at `/apis`, and **`/apis` does not contain core at all**.
    /// Measured against camelot-eks: `/apis` alone yields 53 groups in which
    /// `pods` resolves to `metrics.k8s.io/pods` — the wrong resource — and
    /// `po` does not resolve, because core's short names were never fetched.
    /// Still two requests against kube-rs's 68.
    pub async fn discover(&self) -> Result<banken_spec::discovery::RestMapper, SpecError> {
        let core = self.discover_at("/api").await?;
        let named = self.discover_at("/apis").await?;
        Ok(core.merged(named))
    }

    /// One aggregated-discovery request against a single endpoint.
    async fn discover_at(
        &self,
        uri: &str,
    ) -> Result<banken_spec::discovery::RestMapper, SpecError> {
        let req = http::Request::builder()
            .uri(uri)
            .header(http::header::ACCEPT, Self::AGGREGATED_DISCOVERY_ACCEPT)
            .body(Vec::new())
            .map_err(|e| SpecError::Interp {
                phase: "discovery".into(),
                message: e.to_string(),
            })?;
        let body = self
            .client
            .clone()
            .request_text(req)
            .await
            .map_err(|e| SpecError::Interp {
                phase: "discovery".into(),
                message: Self::error_chain(&e),
            })?;
        banken_spec::discovery::RestMapper::from_aggregated_json(&body)
    }

    /// Flatten an error and **every** error beneath it into one line.
    ///
    /// `kube::Error`'s own `Display` is the outermost wrapper only — a failed
    /// apiserver connection renders as `ServiceError: client error (Connect)`,
    /// which names the layer that noticed and nothing an operator can act on.
    /// The cause (a TLS refusal, a closed port, an unknown authority) is one or
    /// more `source()` hops down and was being discarded at every `map_err` in
    /// this file.
    ///
    /// Measured against a local engenho apiserver: the flattened string is the
    /// difference between `client error (Connect)` and the named TLS failure.
    /// Built with `push_str`, never `format!()` (★★ TYPED EMISSION).
    fn error_chain(e: &dyn std::error::Error) -> String {
        let mut m = e.to_string();
        let mut src = e.source();
        while let Some(cause) = src {
            m.push_str(": ");
            m.push_str(&cause.to_string());
            src = cause.source();
        }
        m
    }

    async fn list_pods(&self, ns: Option<&str>) -> Result<Vec<Row>, SpecError> {
        let api: Api<Pod> = match ns {
            Some(ns) => Api::namespaced(self.client.clone(), ns),
            None => Api::all(self.client.clone()),
        };
        let list = api
            .list(&ListParams::default())
            .await
            .map_err(|e| SpecError::Interp {
                phase: "list-resources".into(),
                message: Self::error_chain(&e),
            })?;
        // `filter_map`, not `map`: an object with no `metadata.uid` is a
        // malformed read, and rendering it with a synthesised identity is the
        // failure this type exists to remove. The apiserver always sets uid, so
        // in practice this drops nothing.
        Ok(list.items.into_iter().filter_map(pod_to_row).collect())
    }
}

/// Project a `k8s_openapi` Pod into a banken [`Row`] keyed by the authored
/// `(defk8sview "pods")` fields (`ready`/`phase`/`restarts`/`age`, rendered
/// under the headers READY/STATUS/RESTARTS/AGE). Read-only projection.
/// Returns `None` for an object carrying **no `metadata.uid`**.
///
/// That is not a defensive `unwrap_or_default` — a uid-less object is not a
/// thing banken observed, it is a malformed read, and the apiserver always sets
/// `metadata.uid`. Synthesising one from `(namespace, name)` would reintroduce
/// exactly the non-unique identity this type exists to remove, and would do it
/// invisibly. The caller counts what it skipped rather than rendering it.
fn pod_to_row(pod: Pod) -> Option<Row> {
    let uid = banken_spec::env::Uid::new(pod.metadata.uid.clone().unwrap_or_default())?;
    let version = pod.metadata.resource_version.clone();
    let name = pod.metadata.name.clone().unwrap_or_default();
    let namespace = pod.metadata.namespace.clone();

    let status = pod.status.as_ref();
    let phase = status
        .and_then(|s| s.phase.clone())
        .unwrap_or_else(|| "Unknown".into());

    // READY = ready_containers / total_containers.
    let (ready, total, restarts) = status
        .and_then(|s| s.container_statuses.as_ref())
        .map(|cs| {
            let total = cs.len();
            let ready = cs.iter().filter(|c| c.ready).count();
            let restarts: i32 = cs.iter().map(|c| c.restart_count).sum();
            (ready, total, restarts)
        })
        .unwrap_or((0, 0, 0));

    // Surface a waiting-reason (CrashLoopBackOff, ImagePullBackOff, …) as
    // the STATUS when present — matches k9s's column semantics.
    let display_status = status
        .and_then(|s| s.container_statuses.as_ref())
        .and_then(|cs| {
            cs.iter().find_map(|c| {
                c.state
                    .as_ref()
                    .and_then(|st| st.waiting.as_ref())
                    .and_then(|w| w.reason.clone())
            })
        })
        .unwrap_or(phase);

    let ready_cell = {
        let mut s = String::new();
        s.push_str(&ready.to_string());
        s.push('/');
        s.push_str(&total.to_string());
        s
    };

    Some(Row {
        uid,
        name,
        namespace,
        version,
        // Keyed by the AUTHORED `(defk8sview "pods")` `:field` values, same
        // as the fixture reader — see `banken::table::Column::field`.
        cells: vec![
            ("ready".into(), ready_cell),
            ("phase".into(), display_status),
            ("restarts".into(), restarts.to_string()),
            // AGE derivation from creation_timestamp is a follow-up
            // (needs a clock); shown as "-" rather than a wrong value.
            ("age".into(), "-".into()),
        ],
    })
}

impl ClusterEnv for KubeClusterEnv {
    /// A REAL re-read, at the instant of the act.
    ///
    /// This is the only method on the live backend that runs on the operator's
    /// keystroke rather than on the absorb task, and that is deliberate: the
    /// absorbed snapshot is what the operator LOOKED at, and a cache is exactly
    /// what cannot answer "is this still the same object". One apiserver round
    /// trip per BREAK-GLASS or DECLARE is a price worth paying precisely
    /// because it is per-ACT, not per-tick.
    fn grip(
        &self,
        id: &banken_spec::env::ObjectId,
    ) -> Result<banken_spec::env::Grip, banken_spec::env::GripError> {
        let rows = self
            .list_resources(id.kind(), id.namespace())
            .map_err(|e| {
                // A failed read is `Blind`, never `Vanished`. "It is gone" and "I
                // cannot see" must not authorise an act the same way — collapsing
                // them is the false-calm class this whole seam exists to refuse.
                banken_spec::env::GripError::Blind {
                    kind: id.kind(),
                    message: e.to_string(),
                }
            })?;
        id.grip_against(&rows)
    }

    fn list_resources(&self, kind: ResourceKind, ns: Option<&str>) -> Result<Vec<Row>, SpecError> {
        match kind {
            ResourceKind::Pod => self.block(self.list_pods(ns)),
            // Other kinds land as views are added (BANKEN.md §VI M1). A
            // typed error, never a fake empty `Ok` that hides the gap.
            other => Err(SpecError::Interp {
                phase: "list-resources".into(),
                message: {
                    let mut m = String::from(other.label());
                    m.push_str(" live read not yet wired (pending-banken: live-read M1)");
                    m
                },
            }),
        }
    }

    fn get_resource(
        &self,
        _kind: ResourceKind,
        _name: &str,
        _ns: Option<&str>,
    ) -> Result<Row, SpecError> {
        Err(SpecError::Interp {
            phase: "get-resource".into(),
            message: "live get not yet wired (pending-banken: live-read M1)".into(),
        })
    }

    fn logs(&self, _pod: &str, _ns: &str, _follow: bool) -> Result<LogStream, SpecError> {
        // KubeClient has no `log` method (BANKEN.md §II); the interim log
        // source is the kubernetes MCP. Surface the gap, never a fake tail.
        Err(SpecError::Interp {
            phase: "logs".into(),
            message: "live logs via the kubernetes MCP (pending-banken: native-logs)".into(),
        })
    }

    fn events(&self, _ns: Option<&str>) -> Result<Vec<Event>, SpecError> {
        Err(SpecError::Interp {
            phase: "events".into(),
            message: "live events not yet wired (pending-banken: live-read M1)".into(),
        })
    }

    fn topology(&self, _root: &ResourceRef) -> Result<DepTree, SpecError> {
        Err(SpecError::Interp {
            phase: "topology".into(),
            message: "XRay tree is M2 (pending-banken: xray)".into(),
        })
    }

    fn health_signals(&self, _w: &Workload) -> Result<HealthReading, SpecError> {
        Err(SpecError::Interp {
            phase: "health-signals".into(),
            message: "health ward is M2 (pending-banken: health-panel)".into(),
        })
    }

    fn watch(&self, kind: ResourceKind, ns: Option<&str>) -> Result<WatchStream, SpecError> {
        // A poll-materialized batch (the true informer is M1 unbuilt
        // substrate — pending-banken: live-watch).
        Ok(WatchStream {
            kind_label: kind.label().to_string(),
            rows: self.list_resources(kind, ns)?,
        })
    }

    fn declare(&self, _change: DeclareChange) -> Result<ChangeRef, SpecError> {
        // DECLARE opens a branch+PR against the release.yaml owner (M3,
        // never direct-to-main). Not wired here; a typed error, never a
        // silent live effect.
        Err(SpecError::Interp {
            phase: "declare".into(),
            message: "DECLARE branch+PR flow is M3 (pending-banken: declare-rail)".into(),
        })
    }

    fn break_glass(&self, _action: WitnessedAction) -> Result<GlassRecord, SpecError> {
        // BREAK-GLASS exec routes through the kubernetes MCP `k8s-pod-exec`
        // (the §IX named interim executor), not this backend. A typed error
        // until that arm is wired.
        Err(SpecError::Interp {
            phase: "break-glass".into(),
            message: "break-glass exec via the kubernetes MCP (pending-banken: break-glass)".into(),
        })
    }

    // *** No unwitnessed-mutate method — ClusterEnv has none. ***
}

#[cfg(test)]
mod tests {
    use super::*;
    use k8s_openapi::api::core::v1::{
        ContainerState, ContainerStateWaiting, ContainerStatus, PodStatus,
    };

    fn pod_with(name: &str, phase: &str, waiting_reason: Option<&str>, ready: bool) -> Pod {
        let mut pod = Pod::default();
        // A real apiserver ALWAYS sets these two, so a test double that omits
        // them is not modelling a cluster — it is modelling a malformed read.
        // (It briefly did: both projection tests failed on
        // `pod_to_row(..).expect("the fixture pod carries a uid")` the moment
        // uid became load-bearing, which is the seal doing its job.)
        pod.metadata.uid = Some(format!("uid-{name}"));
        pod.metadata.resource_version = Some("1".into());
        pod.metadata.name = Some(name.into());
        pod.metadata.namespace = Some("catch".into());
        let mut cs = ContainerStatus {
            name: "main".into(),
            ready,
            restart_count: 3,
            ..Default::default()
        };
        if let Some(reason) = waiting_reason {
            cs.state = Some(ContainerState {
                waiting: Some(ContainerStateWaiting {
                    reason: Some(reason.into()),
                    ..Default::default()
                }),
                ..Default::default()
            });
        }
        pod.status = Some(PodStatus {
            phase: Some(phase.into()),
            container_statuses: Some(vec![cs]),
            ..Default::default()
        });
        pod
    }

    // Pure projection test — no cluster needed. Proves pod_to_row maps the
    // typed k8s Pod into the canonical banken columns correctly. (The LIVE
    // read itself is untested this session; this exercises the projection.)
    #[test]
    fn pod_to_row_projects_canonical_columns() {
        let row = pod_to_row(pod_with("catch-api", "Running", None, true))
            .expect("the fixture pod carries a uid");
        assert_eq!(row.name, "catch-api");
        let get = |k: &str| {
            row.cells
                .iter()
                .find(|(kk, _)| kk == k)
                .map(|(_, v)| v.as_str())
        };
        assert_eq!(get("ready"), Some("1/1"));
        assert_eq!(get("phase"), Some("Running"));
        assert_eq!(get("restarts"), Some("3"));
    }

    #[test]
    fn pod_to_row_surfaces_waiting_reason_as_status() {
        let row = pod_to_row(pod_with(
            "catch-worker",
            "Pending",
            Some("CrashLoopBackOff"),
            false,
        ))
        .expect("the fixture pod carries a uid");
        let status = row
            .cells
            .iter()
            .find(|(k, _)| k == "phase")
            .map(|(_, v)| v.as_str());
        assert_eq!(
            status,
            Some("CrashLoopBackOff"),
            "a waiting reason takes precedence over the phase (k9s semantics)"
        );
        let ready = row
            .cells
            .iter()
            .find(|(k, _)| k == "ready")
            .map(|(_, v)| v.as_str());
        assert_eq!(ready, Some("0/1"));
    }

    // ── context resolution ──
    //
    // These drive the REAL `KUBECONFIG` env var, so they must not run
    // concurrently with each other. `cargo test` threads within a binary, so
    // they share one mutex rather than trusting luck.
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// A scratch directory that removes itself. Hand-rolled rather than
    /// pulling `tempfile` in: a dev-dependency here would move `Cargo.lock`
    /// and drag the gen build-spec regeneration behind it, which is a lot of
    /// blast radius for two fixture files.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(tag: &str) -> Self {
            let mut name = String::from("banken-ctx-test-");
            name.push_str(tag);
            name.push('-');
            name.push_str(&std::process::id().to_string());
            let dir = std::env::temp_dir().join(name);
            let _fresh = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).expect("create scratch dir");
            Self(dir)
        }
        fn path(&self) -> &std::path::Path {
            &self.0
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _best_effort = std::fs::remove_dir_all(&self.0);
        }
    }

    fn write_kubeconfig(dir: &std::path::Path, file: &str, ctx: &str, server: &str) -> PathBuf {
        let path = dir.join(file);
        let mut s = String::from("apiVersion: v1\nkind: Config\nclusters:\n- name: ");
        s.push_str(ctx);
        s.push_str("\n  cluster:\n    server: ");
        s.push_str(server);
        s.push_str("\nusers:\n- name: u\n  user: {}\ncontexts:\n- name: ");
        s.push_str(ctx);
        s.push_str("\n  context:\n    cluster: ");
        s.push_str(ctx);
        s.push_str("\n    user: u\n");
        std::fs::write(&path, s).expect("write fixture kubeconfig");
        path
    }

    /// **THE gate.** Two files in the merge list declaring one context name is
    /// refused — never resolved first-wins.
    ///
    /// This is the class that bit us for real on 2026-08-08: `engenho-local`
    /// named a local apiserver in one file and a remote one in `~/.kube/config`,
    /// and banken read the remote cluster while looking perfectly healthy. The
    /// rows were real; only the cluster was wrong.
    #[test]
    fn a_context_declared_by_two_files_is_refused() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = Scratch::new("ambig");
        let a = write_kubeconfig(dir.path(), "a.yaml", "shared", "https://127.0.0.1:6443");
        let b = write_kubeconfig(
            dir.path(),
            "b.yaml",
            "shared",
            "https://remote.example:6443",
        );

        let joined = std::env::join_paths([&a, &b]).expect("join");
        // SAFETY: serialized by ENV_LOCK; restored below.
        unsafe { std::env::set_var("KUBECONFIG", &joined) };
        let got = resolve_context("shared");
        unsafe { std::env::remove_var("KUBECONFIG") };

        match got {
            Err(ContextError::Ambiguous { name, files }) => {
                assert_eq!(name, "shared");
                assert_eq!(files.len(), 2, "both declaring files must be named");
                assert_eq!(
                    files[0], a,
                    "the file that would have silently won comes first"
                );
            }
            other => panic!(
                "an ambiguous context MUST be refused — kube-rs would have \
                 resolved it first-wins and said nothing. Got: {other:?}"
            ),
        }
    }

    /// The unambiguous case still resolves, and reports the URL — the field
    /// that makes "the right cluster" checkable rather than a label to trust.
    #[test]
    fn a_uniquely_declared_context_resolves_to_its_server() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = Scratch::new("unique");
        let a = write_kubeconfig(dir.path(), "a.yaml", "only-here", "https://127.0.0.1:6443");
        let b = write_kubeconfig(dir.path(), "b.yaml", "elsewhere", "https://other:6443");

        let joined = std::env::join_paths([&a, &b]).expect("join");
        unsafe { std::env::set_var("KUBECONFIG", &joined) };
        let got = resolve_context("only-here");
        let missing = resolve_context("nope");
        unsafe { std::env::remove_var("KUBECONFIG") };

        let r = got.expect("a uniquely-declared context must resolve");
        assert_eq!(r.server, "https://127.0.0.1:6443");
        assert_eq!(r.file, a);

        match missing {
            Err(ContextError::NotFound { available, .. }) => assert!(
                available.contains(&"elsewhere".to_string()),
                "the refusal must name what IS available, so it costs a \
                 keystroke rather than a detour: {available:?}"
            ),
            other => panic!("an absent context must be NotFound, got {other:?}"),
        }
    }
}
