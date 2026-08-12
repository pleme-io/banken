//! `KubeClusterEnv` — the live-cluster OBSERVE backend (BANKEN.md §VI M0
//! destination-tier / §VII "M0 read for a bravo cluster").
//!
//! # Tier: PROVEN LIVE (measured 2026-07-31) — `pending-banken: live-read` CLOSED
//!
//! This backend's read path has been exercised against a real apiserver:
//! `alpha-eks` (EKS v1.33, AWS SSO exec-credential auth) returned **109
//! pods**, matching `kubectl get pods -A` at the same moment, and those rows
//! rendered through the full `PodTable` → `draw_pod_table` pipeline. Two
//! independent runs, both recorded in the commit that closed the row:
//!
//! - `tests/live_read.rs` (`#[ignore]`d, feature `live`) — the whole chain
//!   with an assertion at each joint, ending on a golden-frame `TestBackend`
//!   grid;
//! - the **binary itself** in a real 120×28 PTY
//!   (`banken --live --context alpha-eks`), snapshotted mid-run.
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
///
/// Derived from [`enumerate_contexts`] rather than from `Kubeconfig::read()`,
/// so the names a refusal lists and the rows the picker offers come from one
/// read of one set of files. They were two separate reads, and the merged one
/// could not see an ambiguous name at all.
#[must_use]
pub fn kubeconfig_context_names() -> Vec<String> {
    enumerate_contexts()
        .map(|cs| cs.into_iter().map(|c| c.name).collect())
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
    /// Measured 2026-08-08: `charlie-local` named a local apiserver in one file
    /// and `charlie-local.example.invalid` in `~/.kube/config`. banken read the
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

    for (file, declarations) in read_unmerged()? {
        for (ctx_name, server) in declarations {
            if !available.contains(&ctx_name) {
                available.push(ctx_name.clone());
            }
            if ctx_name == name {
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

/// One kubeconfig file's context declarations: `(context name, its cluster's
/// server URL)` pairs, in file order.
type Declarations = Vec<(PathBuf, Vec<(String, Option<String>)>)>;

/// Every kubeconfig file's context declarations, **file by file, unmerged**.
///
/// The one place the pre-merge question is answerable, and therefore the one
/// place either [`resolve_context`] or [`enumerate_contexts`] may read from.
/// They used to carry a copy each of this loop; the copies are what let the
/// picker's list and the resolver's verdict disagree about what a kubeconfig
/// says, which is the exact class of bug the resolver exists to close.
///
/// Each entry is `(file, [(context name, its cluster's server URL)])`. A
/// context whose cluster entry lives in a *different* file resolves to `None`
/// rather than to an invented URL — the common case is co-located, and a
/// missing server is reported, never guessed.
///
/// # Errors
///
/// [`ContextError::Unreadable`] when a file that EXISTS cannot be parsed. A
/// file merely absent from disk is skipped, which is what client-go does.
fn read_unmerged() -> Result<Declarations, ContextError> {
    let mut out: Declarations = Vec::new();
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

        let declarations = kc
            .contexts
            .iter()
            .map(|ctx| {
                let server = ctx.context.as_ref().and_then(|c| {
                    kc.clusters
                        .iter()
                        .find(|cl| cl.name == c.cluster)
                        .and_then(|cl| cl.cluster.as_ref())
                        .and_then(|cl| cl.server.clone())
                });
                (ctx.name.clone(), server)
            })
            .collect();
        out.push((file, declarations));
    }
    Ok(out)
}

/// One kubeconfig context, as something an operator can be asked to CHOOSE.
///
/// The point of the type is the third field. A context *name* is what an
/// operator recognises and is what `--context` takes, but the name is not
/// unique across a merged `KUBECONFIG` — so a chooser that offers names alone
/// offers a choice it cannot honour. Carrying every declaring file makes the
/// ambiguity a visible property of the row at the moment of choosing, instead
/// of a refusal that arrives after the operator has already decided.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextChoice {
    /// The context name, exactly as `--context` would take it.
    pub name: String,
    /// The apiserver the **first** declaration names — i.e. the one kube-rs
    /// would silently have won with. `None` when no declaration names a
    /// server, which is reported rather than papered over.
    pub server: Option<String>,
    /// Every kubeconfig file declaring this name, in merge order. Length 1 is
    /// the healthy case; more is [`ContextError::Ambiguous`] waiting to happen.
    pub files: Vec<PathBuf>,
}

impl ContextChoice {
    /// Whether more than one kubeconfig file declares this name.
    ///
    /// An ambiguous choice is **not selectable**: [`resolve_context`] refuses
    /// it, so offering it as though it were pickable would be an affordance
    /// that cannot be honoured.
    #[must_use]
    pub fn is_ambiguous(&self) -> bool {
        self.files.len() > 1
    }
}

/// Every context the kubeconfig merge list declares, first-seen order, each
/// carrying its apiserver and its declaring files.
///
/// This is the picker's source, and it deliberately does **not** go through
/// `Kubeconfig::read()`: that merges first, and the merge drops a duplicate
/// name without a word (`append_new_named`), so a merged read cannot tell an
/// unambiguous context from an ambiguous one. Reading unmerged is what lets a
/// row say "two files declare this".
///
/// # Errors
///
/// [`ContextError::Unreadable`] when a kubeconfig file that exists cannot be
/// parsed. An empty `Ok` result means the files parsed and declare nothing —
/// a different answer from "could not read", and both are reported honestly.
pub fn enumerate_contexts() -> Result<Vec<ContextChoice>, ContextError> {
    let mut out: Vec<ContextChoice> = Vec::new();
    for (file, declarations) in read_unmerged()? {
        for (name, server) in declarations {
            if let Some(existing) = out.iter_mut().find(|c| c.name == name) {
                // A second declaration does NOT overwrite the server: the
                // first one is what kube-rs resolves to, so showing a later
                // file's URL would describe a cluster banken would not dial.
                existing.files.push(file.clone());
            } else {
                out.push(ContextChoice {
                    name,
                    server,
                    files: vec![file.clone()],
                });
            }
        }
    }
    Ok(out)
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
        // The unobserved connect is the observed one with the reports thrown
        // away, never a second copy of the sequence. A `channel()` here costs
        // one allocation and buys the guarantee that the path a test drives
        // and the path an operator watches cannot drift.
        let (reporter, _watch) = crate::antessala::channel();
        Self::connect_with_context_staged(context, &reporter).await
    }

    /// [`Self::connect_with_context`], reporting each stage as it begins.
    ///
    /// The stages are published at the CALL BOUNDARIES rather than inferred
    /// from inside: `Client::try_from` is one opaque call that both
    /// authenticates and builds the client, so banken can honestly say "I am
    /// about to run the credential helper" and cannot honestly say how far
    /// through it is. Three observations beat an interpolated percentage.
    ///
    /// **This function never publishes [`Stage::Settled`].** The connect is
    /// not the last thing standing between an operator and their first frame —
    /// the initial pod read is — so settling here would close the screen one
    /// stage early and hand the remaining wait back to a blank terminal. The
    /// caller owns the terminal and therefore owns the terminal stage; it
    /// arms an [`crate::antessala::SettleOnDrop`] around the whole build.
    ///
    /// # Errors
    ///
    /// Exactly [`Self::connect_with_context`]'s — this is that function.
    pub async fn connect_with_context_staged(
        context: &str,
        reporter: &crate::antessala::StageReporter,
    ) -> Result<Self, SpecError> {
        use crate::antessala::Stage;

        reporter.reached(Stage::Kubeconfig);
        ensure_crypto_provider();
        // Resolve BEFORE connecting. An ambiguous name has no path past this
        // line, so "banken read the wrong cluster and looked fine doing it"
        // has no code path rather than a warning nobody reads.
        let resolved = resolve_context(context).map_err(|e| SpecError::Interp {
            phase: "connect".into(),
            message: e.to_string(),
        })?;
        reporter.reached(Stage::Configuration);
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
        // THE slow one — this is where the `exec` credential helper runs, with
        // a blocking `cmd.output()` on an AWS SSO round-trip for an EKS
        // context. Announced BEFORE the call, which is the only useful moment:
        // announcing it after would name the stage that already finished.
        reporter.reached(Stage::Credentials);
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

    /// Climb the access ladder above [`crate::ronda::Rung::Network`] for one
    /// context — the expensive half of a round.
    ///
    /// # Why the ladder is climbed with kube and not by hand
    ///
    /// The rungs above `Network` are TLS-against-the-cluster-CA, an apiserver
    /// that speaks Kubernetes, an accepted identity, and permission to list
    /// pods. Every one of those is something `kube` already does correctly,
    /// including the parts that are easy to get subtly wrong — SNI, the CA
    /// bundle, the exec credential protocol, token caching. Hand-rolling a
    /// TLS handshake here to "probe more cheaply" would be a second, worse
    /// implementation of the client banken already ships, and it would drift.
    ///
    /// So the climb is the real thing: build the real client, ask the real
    /// apiserver, and **classify the failure** to learn which rung it stopped
    /// at. That also means a green light is earned by doing exactly what the
    /// navigator will do a moment later, rather than by a proxy for it.
    ///
    /// # What each rung costs
    ///
    /// `Client::try_from` runs the kubeconfig's exec credential helper. On an
    /// EKS context that is an `aws eks get-token` subprocess and an STS
    /// round-trip, which is why [`crate::ronda::DEEP_INTERVAL`] is minutes and
    /// why callers gate this on the cheap probe having already succeeded.
    ///
    /// A `limit=1` list is deliberately the last step rather than a
    /// `SelfSubjectAccessReview`: SSAR asks the cluster's opinion of a verb,
    /// where this asks the actual question banken cares about and gets a real
    /// answer from the real path. It reads one pod, so the cost does not grow
    /// with the size of the estate.
    #[must_use]
    pub async fn climb(context: &str) -> crate::ronda::Standing {
        use crate::ronda::{Rung, Standing};

        ensure_crypto_provider();
        let options = KubeConfigOptions {
            context: Some(context.to_owned()),
            ..Default::default()
        };
        let config = match Config::from_kubeconfig(&options).await {
            Ok(c) => c,
            Err(e) => {
                return Standing::stopped(Rung::Network, Self::error_chain(&e));
            }
        };
        // This is the line that spawns the credential helper.
        let client = match Client::try_from(config) {
            Ok(c) => c,
            Err(e) => {
                let mut m = String::from("credentials: ");
                m.push_str(&Self::error_chain(&e));
                return Standing::stopped(Rung::Network, m);
            }
        };

        // Rung: does an apiserver answer, and does it accept the identity?
        //
        // A 401 is a SUCCESSFUL measurement of a lower rung, not an error to
        // report as a failure: something Kubernetes-shaped answered and said
        // "not you", which is strictly more than `Network` and strictly less
        // than `Identity`.
        match client.apiserver_version().await {
            Ok(_) => {}
            Err(e) => {
                let code = Self::api_status(&e);
                return if code == Some(401) {
                    Standing::stopped(Rung::Serving, "apiserver answered 401 — identity rejected")
                } else {
                    let mut m = String::from("no apiserver answer: ");
                    m.push_str(&e.to_string());
                    Standing::stopped(Rung::Network, m)
                };
            }
        }

        // Rung: may this identity actually list pods here?
        let api: Api<Pod> = Api::all(client);
        match api.list(&ListParams::default().limit(1)).await {
            Ok(_) => Standing::at(Rung::Pods),
            Err(e) => {
                let code = Self::api_status(&e);
                if code == Some(403) {
                    Standing::stopped(Rung::Identity, "authenticated, but not allowed to list pods")
                } else {
                    let mut m = String::from("authenticated; the pod list failed: ");
                    m.push_str(&e.to_string());
                    Standing::stopped(Rung::Identity, m)
                }
            }
        }
    }

    /// The HTTP status an apiserver returned, when the error carries one.
    ///
    /// The distinction the ladder rests on: a transport error means banken
    /// never got an answer, where a 401/403 means it got a very specific one.
    /// Reading the code is what keeps "rejected" from being reported as
    /// "unreachable".
    fn api_status(e: &kube::Error) -> Option<u16> {
        match e {
            kube::Error::Api(response) => Some(response.code),
            _ => None,
        }
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
    /// Verified serving on `alpha-eks` (v1.33) on 2026-08-08.
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
    /// `Discovery::run()` at N+2 sequential requests (68 against alpha-eks).
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
    /// Measured against alpha-eks: `/apis` alone yields 53 groups in which
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

/// `AGE`, k8s-style, from a creation timestamp.
///
/// This column rendered `-` on every row since the live backend landed, with
/// the note that the derivation "needs a clock". It needs a clock the same way
/// every age does; what it actually needed was a decision about which unit to
/// show, and `kubectl`'s answer is the one operators already read: the largest
/// unit that leaves a number, never two units, never a fraction.
///
/// `None` (absent timestamp) still renders `-`, because "banken does not know
/// how old this is" and "this is zero seconds old" are different facts.
fn age_cell(created: Option<&k8s_openapi::apimachinery::pkg::apis::meta::v1::Time>) -> String {
    let Some(t) = created else {
        return "-".into();
    };
    let secs = (k8s_openapi::chrono::Utc::now() - t.0).num_seconds();
    if secs < 0 {
        // A clock skew between here and the apiserver. `-` beats a negative
        // age, which reads as a bug in the cluster rather than in the clock.
        return "-".into();
    }
    let (n, unit) = match secs {
        s if s < 60 => (s, 's'),
        s if s < 3600 => (s / 60, 'm'),
        s if s < 86_400 => (s / 3600, 'h'),
        s => (s / 86_400, 'd'),
    };
    let mut out = n.to_string();
    out.push(unit);
    out
}

/// The common head of every projection: identity, name, namespace, version.
///
/// `None` for an object carrying no `metadata.uid` — see [`pod_to_row`] for
/// why that is a refusal rather than a synthesised identity.
fn row_head<K: kube::Resource>(o: &K) -> Option<(banken_spec::env::Uid, String, Option<String>, Option<String>)> {
    let m = o.meta();
    let uid = banken_spec::env::Uid::new(m.uid.clone().unwrap_or_default())?;
    Some((
        uid,
        m.name.clone().unwrap_or_default(),
        m.namespace.clone(),
        m.resource_version.clone(),
    ))
}

/// Build a [`Row`] from an object plus the cells its authored view names.
fn row_of<K: kube::Resource>(o: &K, cells: Vec<(String, String)>) -> Option<Row> {
    let (uid, name, namespace, version) = row_head(o)?;
    let mut cells = cells;
    cells.push(("age".into(), age_cell(o.meta().creation_timestamp.as_ref())));
    Some(Row {
        uid,
        name,
        namespace,
        version,
        cells,
    })
}

fn service_to_row(s: k8s_openapi::api::core::v1::Service) -> Option<Row> {
    let spec = s.spec.as_ref();
    let ty = spec
        .and_then(|s| s.type_.clone())
        .unwrap_or_else(|| "ClusterIP".into());
    let cluster_ip = spec
        .and_then(|s| s.cluster_ip.clone())
        .unwrap_or_else(|| "-".into());
    let ports = spec
        .and_then(|s| s.ports.as_ref())
        .map(|ps| {
            let mut out = String::new();
            for (i, p) in ps.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(&p.port.to_string());
            }
            out
        })
        .unwrap_or_else(|| "-".into());
    let cells = vec![
        ("type".into(), ty),
        ("cluster-ip".into(), cluster_ip),
        ("ports".into(), ports),
    ];
    row_of(&s, cells)
}

fn deployment_to_row(d: k8s_openapi::api::apps::v1::Deployment) -> Option<Row> {
    let st = d.status.as_ref();
    let desired = d
        .spec
        .as_ref()
        .and_then(|s| s.replicas)
        .unwrap_or(0);
    let ready = st.and_then(|s| s.ready_replicas).unwrap_or(0);
    let up_to_date = st.and_then(|s| s.updated_replicas).unwrap_or(0);
    let available = st.and_then(|s| s.available_replicas).unwrap_or(0);
    let mut ready_cell = ready.to_string();
    ready_cell.push('/');
    ready_cell.push_str(&desired.to_string());
    let cells = vec![
        ("ready".into(), ready_cell),
        ("up-to-date".into(), up_to_date.to_string()),
        ("available".into(), available.to_string()),
    ];
    row_of(&d, cells)
}

fn replicaset_to_row(r: k8s_openapi::api::apps::v1::ReplicaSet) -> Option<Row> {
    let st = r.status.as_ref();
    let desired = r.spec.as_ref().and_then(|s| s.replicas).unwrap_or(0);
    let current = st.map_or(0, |s| s.replicas);
    let ready = st.and_then(|s| s.ready_replicas).unwrap_or(0);
    let cells = vec![
        ("desired".into(), desired.to_string()),
        ("current".into(), current.to_string()),
        ("ready".into(), ready.to_string()),
    ];
    row_of(&r, cells)
}

fn configmap_to_row(c: k8s_openapi::api::core::v1::ConfigMap) -> Option<Row> {
    // KEY COUNT, never the values. A ConfigMap routinely carries credentials
    // that were put there by mistake, and a navigator that renders them into a
    // terminal has published them to a scrollback, a screen share and a
    // recording. The count answers "is it populated"; `:describe` is the
    // deliberate act that shows content.
    let keys = c.data.as_ref().map_or(0, std::collections::BTreeMap::len)
        + c.binary_data.as_ref().map_or(0, std::collections::BTreeMap::len);
    row_of(&c, vec![("keys".into(), keys.to_string())])
}

fn endpoints_to_row(e: k8s_openapi::api::core::v1::Endpoints) -> Option<Row> {
    let addresses: usize = e
        .subsets
        .as_ref()
        .map(|ss| {
            ss.iter()
                .map(|s| s.addresses.as_ref().map_or(0, Vec::len))
                .sum()
        })
        .unwrap_or(0);
    row_of(&e, vec![("endpoints".into(), addresses.to_string())])
}

fn namespace_to_row(n: k8s_openapi::api::core::v1::Namespace) -> Option<Row> {
    let phase = n
        .status
        .as_ref()
        .and_then(|s| s.phase.clone())
        .unwrap_or_else(|| "Unknown".into());
    row_of(&n, vec![("phase".into(), phase)])
}

fn node_to_row(n: k8s_openapi::api::core::v1::Node) -> Option<Row> {
    let st = n.status.as_ref();
    // A node's STATUS is the Ready condition, and `Unschedulable` on the spec
    // is what a cordon looks like — reporting only Ready would show a cordoned
    // node as healthy, which is the one moment an operator is looking at this
    // column on purpose.
    let ready = st
        .and_then(|s| s.conditions.as_ref())
        .and_then(|cs| cs.iter().find(|c| c.type_ == "Ready"))
        .map(|c| {
            if c.status == "True" {
                "Ready".to_owned()
            } else {
                "NotReady".to_owned()
            }
        })
        .unwrap_or_else(|| "Unknown".into());
    let status = if n.spec.as_ref().and_then(|s| s.unschedulable) == Some(true) {
        let mut s = ready.clone();
        s.push_str(",SchedulingDisabled");
        s
    } else {
        ready
    };
    let version = st
        .and_then(|s| s.node_info.as_ref())
        .map(|i| i.kubelet_version.clone())
        .unwrap_or_else(|| "-".into());
    let cells = vec![("phase".into(), status), ("version".into(), version)];
    row_of(&n, cells)
}

fn event_to_row(e: k8s_openapi::api::core::v1::Event) -> Option<Row> {
    let reason = e.reason.clone().unwrap_or_else(|| "-".into());
    let kind = e.type_.clone().unwrap_or_else(|| "Normal".into());
    let message = e.message.clone().unwrap_or_default();
    let object = e.involved_object.name.clone().unwrap_or_else(|| "-".into());
    // COUNT matters more than it looks: an event repeating 400 times is a
    // different situation from one that happened once, and k8s collapses
    // repeats into a single object with a count rather than emitting each.
    let count = e.count.map_or_else(|| "-".to_owned(), |c| c.to_string());
    let cells = vec![
        ("event-type".into(), kind),
        ("reason".into(), reason),
        ("object".into(), object),
        ("count".into(), count),
        ("message".into(), message),
    ];
    row_of(&e, cells)
}

impl KubeClusterEnv {
    /// Read one object by name and project it, using the same per-kind
    /// projection [`Self::list_resources`] uses.
    ///
    /// Sharing the projection is the point: a `describe` that rendered
    /// different fields than the table it was opened from would make the two
    /// disagree about the same object, and the operator would have no way to
    /// know which was right.
    async fn get_ns_scoped<K>(
        &self,
        name: &str,
        ns: Option<&str>,
        project: fn(K) -> Option<Row>,
    ) -> Result<Row, SpecError>
    where
        K: kube::Resource<Scope = kube::core::NamespaceResourceScope>
            + Clone
            + std::fmt::Debug
            + k8s_openapi::serde::de::DeserializeOwned,
        K::DynamicType: Default,
    {
        let api: Api<K> = match ns {
            Some(ns) => Api::namespaced(self.client.clone(), ns),
            None => Api::default_namespaced(self.client.clone()),
        };
        let obj = api.get(name).await.map_err(|e| SpecError::Interp {
            phase: "get-resource".into(),
            message: Self::error_chain(&e),
        })?;
        project(obj).ok_or_else(|| SpecError::Interp {
            phase: "get-resource".into(),
            message: "the apiserver returned an object with no metadata.uid".into(),
        })
    }

    /// The cluster-scoped peer of [`Self::get_ns_scoped`].
    async fn get_cluster_scoped<K>(
        &self,
        name: &str,
        project: fn(K) -> Option<Row>,
    ) -> Result<Row, SpecError>
    where
        K: kube::Resource<Scope = kube::core::ClusterResourceScope>
            + Clone
            + std::fmt::Debug
            + k8s_openapi::serde::de::DeserializeOwned,
        K::DynamicType: Default,
    {
        let api: Api<K> = Api::all(self.client.clone());
        let obj = api.get(name).await.map_err(|e| SpecError::Interp {
            phase: "get-resource".into(),
            message: Self::error_chain(&e),
        })?;
        project(obj).ok_or_else(|| SpecError::Interp {
            phase: "get-resource".into(),
            message: "the apiserver returned an object with no metadata.uid".into(),
        })
    }

    /// Cluster events, newest last (the apiserver's own order).
    async fn list_events(&self, ns: Option<&str>) -> Result<Vec<Event>, SpecError> {
        use k8s_openapi::api::core::v1::Event as K8sEvent;
        let api: Api<K8sEvent> = match ns {
            Some(ns) => Api::namespaced(self.client.clone(), ns),
            None => Api::all(self.client.clone()),
        };
        let list = api
            .list(&ListParams::default())
            .await
            .map_err(|e| SpecError::Interp {
                phase: "events".into(),
                message: Self::error_chain(&e),
            })?;
        Ok(list
            .items
            .into_iter()
            .map(|e| Event {
                kind: e.type_.unwrap_or_else(|| "Normal".into()),
                reason: e.reason.unwrap_or_else(|| "-".into()),
                message: e.message.unwrap_or_default(),
                involved: e.involved_object.name.unwrap_or_else(|| "-".into()),
            })
            .collect())
    }

    /// List a namespaced kind and project each object into a [`Row`].
    ///
    /// The generic that ends the pod-shaped era: `list_pods` was the only
    /// listing, so every kind past `Pod` was a typed refusal. What varies per
    /// kind is exactly the projection — which is why it is the parameter and
    /// the plumbing is not.
    async fn list_ns_scoped<K>(
        &self,
        ns: Option<&str>,
        project: fn(K) -> Option<Row>,
    ) -> Result<Vec<Row>, SpecError>
    where
        K: kube::Resource<Scope = kube::core::NamespaceResourceScope>
            + Clone
            + std::fmt::Debug
            // Reached through k8s-openapi's re-export: `serde` is not a direct
            // dependency of this crate and adding one for a single trait bound
            // would widen the closure for nothing.
            + k8s_openapi::serde::de::DeserializeOwned,
        K::DynamicType: Default,
    {
        let api: Api<K> = match ns {
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
        Ok(list.items.into_iter().filter_map(project).collect())
    }

    /// The cluster-scoped peer. Separate because `Api::namespaced` does not
    /// exist for a cluster-scoped kind — the scope is in the type, so asking
    /// for a namespaced Node is a compile error rather than a runtime 404.
    async fn list_cluster_scoped<K>(
        &self,
        project: fn(K) -> Option<Row>,
    ) -> Result<Vec<Row>, SpecError>
    where
        K: kube::Resource<Scope = kube::core::ClusterResourceScope>
            + Clone
            + std::fmt::Debug
            // Reached through k8s-openapi's re-export: `serde` is not a direct
            // dependency of this crate and adding one for a single trait bound
            // would widen the closure for nothing.
            + k8s_openapi::serde::de::DeserializeOwned,
        K::DynamicType: Default,
    {
        let api: Api<K> = Api::all(self.client.clone());
        let list = api
            .list(&ListParams::default())
            .await
            .map_err(|e| SpecError::Interp {
                phase: "list-resources".into(),
                message: Self::error_chain(&e),
            })?;
        Ok(list.items.into_iter().filter_map(project).collect())
    }
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

    /// Every [`ResourceKind`] reads live.
    ///
    /// **A TOTAL match, and that is the point.** This was `Pod => …, other =>
    /// Err("not yet wired")`, which meant a kind added to the enum compiled
    /// clean and refused at runtime. Listing every arm makes a new kind a
    /// compile error here until someone writes its projection — the gap moves
    /// from an operator's screen to the build.
    fn list_resources(&self, kind: ResourceKind, ns: Option<&str>) -> Result<Vec<Row>, SpecError> {
        use k8s_openapi::api::apps::v1::{Deployment, ReplicaSet};
        use k8s_openapi::api::core::v1::{ConfigMap, Endpoints, Namespace, Node, Service};
        match kind {
            ResourceKind::Pod => self.block(self.list_pods(ns)),
            ResourceKind::Service => {
                self.block(self.list_ns_scoped::<Service>(ns, service_to_row))
            }
            ResourceKind::ConfigMap => {
                self.block(self.list_ns_scoped::<ConfigMap>(ns, configmap_to_row))
            }
            ResourceKind::Endpoints => {
                self.block(self.list_ns_scoped::<Endpoints>(ns, endpoints_to_row))
            }
            ResourceKind::Deployment => {
                self.block(self.list_ns_scoped::<Deployment>(ns, deployment_to_row))
            }
            ResourceKind::ReplicaSet => {
                self.block(self.list_ns_scoped::<ReplicaSet>(ns, replicaset_to_row))
            }
            // Cluster-scoped: `ns` is not applicable and is deliberately not
            // silently accepted-and-ignored.
            ResourceKind::Namespace => {
                self.block(self.list_cluster_scoped::<Namespace>(namespace_to_row))
            }
            ResourceKind::Node => self.block(self.list_cluster_scoped::<Node>(node_to_row)),
            ResourceKind::Event => self.block(
                self.list_ns_scoped::<k8s_openapi::api::core::v1::Event>(ns, event_to_row),
            ),
        }
    }

    /// Read ONE object, by the same total dispatch `list_resources` uses.
    ///
    /// Total for the same reason: a kind added to the enum must be a compile
    /// error here rather than a runtime refusal an operator meets by pressing
    /// `d` on a row that rendered perfectly well.
    fn get_resource(
        &self,
        kind: ResourceKind,
        name: &str,
        ns: Option<&str>,
    ) -> Result<Row, SpecError> {
        use k8s_openapi::api::apps::v1::{Deployment, ReplicaSet};
        use k8s_openapi::api::core::v1::{ConfigMap, Endpoints, Namespace, Node, Pod, Service};
        match kind {
            ResourceKind::Pod => self.block(self.get_ns_scoped::<Pod>(name, ns, pod_to_row)),
            ResourceKind::Service => {
                self.block(self.get_ns_scoped::<Service>(name, ns, service_to_row))
            }
            ResourceKind::ConfigMap => {
                self.block(self.get_ns_scoped::<ConfigMap>(name, ns, configmap_to_row))
            }
            ResourceKind::Endpoints => {
                self.block(self.get_ns_scoped::<Endpoints>(name, ns, endpoints_to_row))
            }
            ResourceKind::Deployment => {
                self.block(self.get_ns_scoped::<Deployment>(name, ns, deployment_to_row))
            }
            ResourceKind::ReplicaSet => {
                self.block(self.get_ns_scoped::<ReplicaSet>(name, ns, replicaset_to_row))
            }
            ResourceKind::Namespace => {
                self.block(self.get_cluster_scoped::<Namespace>(name, namespace_to_row))
            }
            ResourceKind::Node => {
                self.block(self.get_cluster_scoped::<Node>(name, node_to_row))
            }
            ResourceKind::Event => self.block(
                self.get_ns_scoped::<k8s_openapi::api::core::v1::Event>(name, ns, event_to_row),
            ),
        }
    }

    fn logs(&self, _pod: &str, _ns: &str, _follow: bool) -> Result<LogStream, SpecError> {
        // KubeClient has no `log` method (BANKEN.md §II); the interim log
        // source is the kubernetes MCP. Surface the gap, never a fake tail.
        Err(SpecError::Interp {
            phase: "logs".into(),
            message: "live logs via the kubernetes MCP (pending-banken: native-logs)".into(),
        })
    }

    /// Cluster events — the first place an operator looks when a pod will
    /// not start, and previously a typed refusal.
    fn events(&self, ns: Option<&str>) -> Result<Vec<Event>, SpecError> {
        self.block(self.list_events(ns))
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

    /// Climb one context against the operator's REAL kubeconfig and print what
    /// came back.
    ///
    /// `#[ignore]`d because it needs a machine with a kubeconfig, a network,
    /// and possibly a live SSO session — a manual instrument, not a gate.
    /// Kept rather than deleted because the ladder's entire value is that its
    /// rungs match reality, and the only way to check that is against a real
    /// estate:
    ///
    /// ```text
    /// cargo test -p banken --lib -- --ignored --nocapture climb_against
    /// BANKEN_CLIMB_CONTEXT=bravo cargo test … climb_against
    /// ```
    #[tokio::test]
    #[ignore = "needs a real kubeconfig and network"]
    async fn climb_against_the_real_kubeconfig() {
        let context =
            std::env::var("BANKEN_CLIMB_CONTEXT").unwrap_or_else(|_| "alpha-eks".to_owned());
        let started = std::time::Instant::now();
        let standing = KubeClusterEnv::climb(&context).await;
        println!("context : {context}");
        println!("elapsed : {:?}", started.elapsed());
        println!("rung    : {:?} ({})", standing.rung, standing.rung.label());
        println!("note    : {}", standing.note);
    }

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
    /// This is the class that bit us for real on 2026-08-08: `charlie-local`
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
