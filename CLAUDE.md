# banken 番犬 — the pleme-io-native k9s (naturalize)

<!-- Non-applicable ★★ directive waivers (banken is a pure terminal binary that
     READS clusters and EMITS git changes; it has no in-cluster runtime): -->
skip-urdume: terminal-not-a-service
skip-tela: terminal-not-web
skip-helm-native: pure-binary-no-cluster-runtime
skip-gitops: pure-binary-no-cluster-runtime
skip-magma: pure-binary-no-cluster-runtime
skip-continuous-convergence: pure-binary-no-cluster-runtime
skip-platform-mediated: pure-binary-no-cluster-runtime

One-sentence purpose: an **observe-first, GitOps-native cluster-navigator TUI** —
keep k9s's fast keyboard navigation + health surfaces, structurally refuse its
imperative-mutation console.

## Canonical design

The full design, tier-honest ledger, phased path, and Care live in
**`theory/BANKEN.md`** (pleme-io/theory) — read it before any non-trivial change.
Do not restate theory here; cite it.

## The one primitive banken owns — `postigo`

Every cluster action is typed into a three-class **action-legality** gate:
`OBSERVE` (direct read) / `DECLARE` (lowers to a full-manifest GitOps change a
reconciler applies) / `BREAK-GLASS` (witnessed, RUNBOOK-logged). The `ClusterEnv`
interpreter trait has **no unwitnessed-mutate method** — an unwitnessed live
mutation is `E0599` within the authored surface (the re-add case is CI-caught,
per `substrate_invariant.rs`, graded only-mitigated → CI-caught, never rounded up
to truly-unrepresentable).

## Governing methods

banken is a **Quadro** (terminal UI) — QUADRO.md governs the render; it composes
egaku + egaku-term + moldura (host: mado), shikumi config, ishou theme, awase keys.
It is a **naturalize** worked example — a **half-citizen by design** (naturalizes
k9s's observe half, not its mutate half). Per its own 4-check citizenship test it
is currently a **resident** (native ✓ / no-leak ✓ / proven ✗ until M0 / known ✓).

## Build status (tier-honest — never round up)

- **SHIPPED (mock-green, no cluster):** `banken-spec` = the `postigo` TYPED-SPEC +
  INTERPRETER triplet (18 tests green — `cargo test -p banken-spec`). The
  citizenship core, provable without a cluster, per BANKEN.md §III.
- **DESIGN / next arc (M0→M4):** the egaku `TableView`/`TreeView` widgets, the
  `draw::table`/`draw::tree` cell drawers, a live-cluster read (M0's `:pods` table
  + the mado-MCP e2e gate), the watch informer, the health panel, the
  resource→`release.yaml` DECLARE resolver. Not built yet — do not imply otherwise.

## Standing rule

Every PR advances a `theory/BANKEN.md` §VII ledger row's tier or leaves a typed
`pending-banken: <row>` note. The `substrate_invariant.rs` guard stays green (any
`ClusterEnv` mutating-method addition fails the build). No DECLARE rail is
direct-to-main. Toolchain: `tatara-lisp` is exact-pinned (`=0.2.4` + derive
`=0.2.2`, sui's blessed interim pair — a caret range floats to 0.2.5 and breaks
the 0.2.2 derive; keep the pin).
