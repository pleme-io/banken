//! banken 番犬 — the pleme-io-native k9s (naturalize).
//!
//! This crate is the **runnable** half of banken (BANKEN.md §VI M0): the
//! `:pods` table render + interaction pipeline, built on the shipped
//! [`banken_spec`] citizenship primitive (the `postigo` action-legality
//! triplet). It is an **observe-first, GitOps-native cluster navigator** —
//! it keeps k9s's fast keyboard navigation and routes every action through
//! the three-class gate: OBSERVE reads freely, DECLARE lowers to a
//! full-manifest GitOps change a reconciler applies, BREAK-GLASS is
//! witnessed. There is **no live-mutate path** — the [`banken_spec::env::ClusterEnv`]
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
//! - **DESIGN / UNTESTED-LIVE:** the live-cluster read
//!   ([`KubeClusterEnv`](live::KubeClusterEnv), under the `live` feature).
//!   No cluster was reachable this session (rio/camelot VPN-gated, local
//!   k3s down), so the live read path was never exercised. It is wired
//!   behind the same [`ClusterEnv`](banken_spec::env::ClusterEnv) seam and
//!   lights up the instant a cluster returns. Do NOT conflate the two.
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
//! - [`live`] — `KubeClusterEnv` (feature `live`). `pending-banken: live-read`.
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
//!   The app does **not** call it yet — pressing `g` previews the plan; wiring
//!   the app's overlay to a confirm-then-open is `pending-banken:
//!   bancada-app-open`.

pub mod action;
pub mod app;
pub mod cli;
pub mod fixture;
pub mod keys;
pub mod render;
pub mod table;

#[cfg(feature = "live")]
pub mod live;

#[cfg(feature = "tear")]
pub mod tear_session;
