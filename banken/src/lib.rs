//! banken 番犬 — the pleme-io-native k9s (naturalize).
//!
//! This crate is the **runnable** half of banken (BANKEN.md §VI M0): the
//! `:pods` table render + interaction pipeline, built on the shipped
//! [`banken_spec`] citizenship primitive (the `postigo` action-legality
//! triplet). It is an **observe-first, GitOps-native cluster navigator** —
//! it keeps k9s's fast keyboard navigation and routes every action through
//! the three-class gate: OBSERVE reads freely, DECLARE lowers to a
//! full-manifest `GitOps` change a reconciler applies, BREAK-GLASS is
//! witnessed. There is **no live-mutate path** — the
//! [`banken_spec::env::ClusterEnv`]
//! seam has no unwitnessed-mutate method.
//!
//! ## What is PROVEN vs DESIGN (tier-honest — never rounded up)
//!
//! - **PROVEN (this session):** the full render + interaction pipeline over
//!   a **fixture source** ([`fixture::FixtureClusterEnv`]) — the `:pods`
//!   table (columns, selection, sort, pathology colors), the postigo
//!   dispatch through the UI (`l`→OBSERVE, `s`→DECLARE full-manifest
//!   preview, `S`→BREAK-GLASS witnessed record), and golden-frame +
//!   dispatch integration tests. This is the sui-spec `MockEnv` discipline:
//!   a real, verifiable increment whose read is canned data.
//! - **PROVEN LIVE (2026-07-31, feature `live`):** the live-cluster **pod
//!   read** ([`KubeClusterEnv`](live::KubeClusterEnv)). `alpha-eks` returned
//!   109 pods — matching `kubectl get pods -A` at the same moment — and they
//!   rendered through the same pipeline the fixture rows do, both from
//!   `tests/live_read.rs` and from the binary in a real PTY.
//!   **Scope of that claim: `list_resources` for pods, and nothing else.**
//!   `logs`, `events`, `topology`, `health_signals`, `declare` and
//!   `break_glass` still return typed "not yet wired" errors on the live
//!   backend. Do not read the row as broader than it is.
//!
//! ## Module map
//!
//! - [`table`] — the `PodTable` view model (columns/selection/sort).
//!   `pending-banken: promote-tableview-to-egaku`.
//! - [`render`] — the `:pods` cell drawer over egaku-term's typed `Buffer`.
//!   `pending-banken: promote-tableview-to-egaku`.
//! - [`cli`] — the typed argv surface. `--live` **requires** an explicit
//!   `--context <name>`: riding the kubeconfig's `current-context` reads
//!   whichever estate the merged `KUBECONFIG` points at, which is the
//!   wrong-cluster class one layer above the one
//!   [`banken_spec::bancada`] already refuses.
//! - [`fixture`] — `FixtureClusterEnv`, the default OBSERVE source.
//! - [`action`] — the postigo dispatch (consumes `banken_spec::apply`).
//! - [`app`] — the `BankenApp` runtime (`egaku_term::AsyncApp`).
//! - [`live`] — `KubeClusterEnv` (feature `live`). `pending-banken: live-read`
//!   CLOSED 2026-07-31.
//! - [`session`] — the `SessionEnv` impls the app is generic over
//!   (`UnwiredSessionEnv` / `LazyTearSessionEnv`).
//! - [`tear_session`] — `TearSessionEnv` (feature `tear`): the LIVE
//!   `(defbancada)` handoff over a running `tear-daemon`.
//!
//! ## The bancada bridge (banken → tear/mado)
//!
//! `g` / `shift+g` on a selected row resolves an authored
//! `(defbancada)` into a pre-warmed troubleshooting session: the panes, their
//! splits, and the fully-resolved `kubectl` argv for the cluster + namespace +
//! pod banken is looking at. Its `postigo` class is **derived from the
//! recipe's panes**, so a session staging a live effect is BREAK-GLASS and
//! cannot be authored as anything else.
//!
//! Tier-honest, and the two halves are different:
//! - **PROVEN (mock, default build):** compiling the recipes, deriving the
//!   legality, resolving the context, producing the plan, and previewing it in
//!   the app — all against `MockSessionEnv`, zero side effects.
//! - **PROVEN (live, feature `tear`, measured 2026-07-30):** the handoff
//!   itself. `tests/tear_handoff.rs` (`#[ignore]`d — it opens a real session)
//!   ran against the operator's daemon and asserted a real **three-pane**
//!   session whose first pane's rendered grid carries the pre-warmed
//!   `kubectl --context <cluster> … <pod>` line. Fail-once measured: stubbing
//!   `type_into` to send no bytes turns it red on exactly that assertion.
//!   The app reaches it through its `SessionEnv` type parameter: `g`
//!   resolves and previews, `enter` confirms and opens. That wire is
//!   mock-proven with a measured fail-once, and the real-daemon run
//!   (2026-07-31) had every RPC return `Ok` — but `tear list` shows no
//!   session afterwards, so "the operator lands in a pre-warmed session" is
//!   NOT proven. `pending-banken: bancada-app-open-live`; see
//!   [`tear_session`]'s module docs for the exact three-way split.

pub mod absorb;
pub mod action;
pub mod app;
pub mod cli;
pub mod feed;
pub mod fixture;
/// The help overlay — the terminal FACE of `banken_spec::help::HelpPage`.
pub mod help;
pub mod render;
pub mod session;
pub mod table;
/// The picker's modal editing — `unsoku` wired to the query line.
pub mod vim;

/// antessala — the antechamber: the connect, made into a place you can watch
/// and leave. Behind `live` because there is nothing to wait for without it.
#[cfg(feature = "live")]
pub mod antessala;

/// ronda — the watchdog's rounds: a bounded, auth-free reachability probe over
/// every kubeconfig context, so the landing screen says what is actually
/// there. Behind `live` for the same reason [`picker`] is.
#[cfg(feature = "live")]
pub mod ronda;

#[cfg(feature = "live")]
pub mod live;

/// The landing screen — choose a cluster. Behind `live` because a chooser over
/// contexts banken could not then read would be an affordance with nothing
/// behind it.
#[cfg(feature = "live")]
pub mod picker;

/// declare — a DECLARE becomes a branch, a full manifest, and a pull request.
///
/// The delivery half of the postigo gate's git-mutating class: banken writes
/// git and a reconciler converges the cluster, so a change is reviewable and
/// revertable by construction rather than by policy.
pub mod declare;

/// palette — the fleet's colours, for the places that genuinely need RGB.
///
/// Deliberately narrow: named ANSI slots stay slots so the operator's own
/// terminal theme wins, and only interpolated colour (the ronda ramp, which no
/// slot can express) resolves through `ishou` tokens.
pub mod palette;

/// glass — the break-glass ledger, written BEFORE the glass breaks.
///
/// Not feature-gated: the durable record is the whole claim BREAK-GLASS makes,
/// so a build in which it is absent is a build whose witnessed path is not
/// witnessed.
pub mod glass;

#[cfg(feature = "tear")]
pub mod tear_session;

/// mcp — the agent surface. banken's OBSERVE half, and *only* that half,
/// served over MCP on stdio.
///
/// The point is the asymmetry: a human at the TUI can DECLARE and can
/// break-glass; an agent through this door can only look. Not because it is
/// asked not to — because the mutating tools do not exist to call.
#[cfg(feature = "mcp")]
pub mod mcp;
