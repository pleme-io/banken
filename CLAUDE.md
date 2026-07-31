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

<!-- ── IN-FLIGHT DEBT: source changes that outrun their published deps ──
     Each row names the exact chain that clears it. Nothing here is a
     design gap; each is one push away. -->

**`pending-egaku-bump: TableView/TableRow`** — `banken-spec` and `banken` name
`egaku::{TableRow, TableView, Column, SortKey, SortOrder, TableError}`, which
land in egaku `e602369` and are **UNPUSHED**. The published `egaku 0.1.4` this
repo's `Cargo.lock` names predates them, so **HEAD does not compile against the
locked dep** — measured, not assumed:

```
error[E0405]: cannot find trait `TableRow` in crate `egaku`
  --> banken-spec/src/env.rs:69:13
   |
69 | impl egaku::TableRow for Row {
   |             ^^^^^^^^ not found in `egaku`
```

`Cargo.lock` is deliberately left naming the **registry** `egaku 0.1.4` — a lock
naming an unpushed rev is worse than one naming a stale rev. Measured green
against the local sibling with
`cargo test --workspace --config 'patch.crates-io.egaku.path="../egaku"'`
(194 default / 196 `--features live` / 196 `--features tear`). That same E0405
is the run's **positive control**: the code cannot compile against published
egaku, so a green run proves the patch was actually used rather than silently
ignored.

Clearing it is one commit: **push egaku → release `egaku 0.1.5` → bump the pin
in `banken/Cargo.toml` + `banken-spec/Cargo.toml` → `cargo update -p egaku` →
regenerate the crate2nix lock in the SAME commit** (D2 delta-only, or the nix
eval fails). banken carries no `Cargo.gen.lock` today — its `flake.nix` calls
crate2nix on `Cargo.lock` directly — so the third step is `nix build` going
green, not a file to regenerate; keep it in the same commit either way.

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

## The two app seams — `BankenApp<E: ClusterEnv, S: SessionEnv>`

The app is generic over **two** mockable traits, for the same reason each time:
the runtime is byte-identical against a mock, a live backend, or a build with
no adapter compiled in. Rows come from `E`; a confirmed `(defbancada)` opens
through `S`.

| seam | mock | live | absent |
|---|---|---|---|
| `ClusterEnv` | `MockClusterEnv` / `FixtureClusterEnv` | `KubeClusterEnv` (feat `live`) | `--live` is a typed CLI error |
| `SessionEnv` | `MockSessionEnv` | `LazyTearSessionEnv` (feat `tear`) | `UnwiredSessionEnv` — a typed refusal |

`UnwiredSessionEnv` **refuses** rather than returning `Ok(())`: a stub would
make the app report a session it never opened, and the operator would go
looking for panes that do not exist.

`LazyTearSessionEnv` is a `Mutex` wrapper, and that is load-bearing rather than
tidy: `AsyncApp::draw` returns an `impl Future + Send`, so `S: Sync`, and
`TearSessionEnv` holds a `RefCell` and is **not** `Sync`. The wrapper is what
makes the live adapter reachable from the app at all. It connects on first
`open_session` so a missing daemon costs one overlay at the moment the operator
asks, instead of refusing to start banken for a dependency only `g` uses.

Two accessors (`keymap`, `should_quit`) are **inherent** as well as on
`AsyncApp`, because that trait needs both seams `Send + Sync` and a recording
mock is neither. Asking "which action does this chord bind" has nothing to do
with owning a terminal.

## The authored vocabulary — SEVEN domains, one resolved catalog

banken's behaviour is **data**, both the configuration half and the
runtime-manipulation half. `banken_spec::load_catalog()` is the entry point;
the per-domain `load_*` helpers exist for a single-domain consumer but check
nothing across domains.

| Form | Owns | Instances shipped |
|---|---|---|
| `(defk8sview)` | a view: kind, source, columns, default sort, drill target | 3 |
| `(defk8saction)` | one action + its `postigo` legality class | 4 |
| `(defpathology)` | a symptom→cause rule: evidence, severity, remedy rail | 3 |
| `(defward)` | the health landing: Pulses lanes, linter set, headline | 1 |
| `(defdrill)` | a typed drill path (`→logs`, `→diagnose`, `→xray`) | 3 |
| `(defnavkey)` | a navigation chord + its local-UI intent | 8 |
| `(defbancada)` | a pre-warmed tear/mado troubleshooting session | 2 |

plus `(defbanken)` in `banken-config` (the deployment face). Every domain has
a lisp-roundtrip test and a `catalog.rs` axis on the `REQUIRED_AXES` roster
(★★ CATALOG REFLECTION, gated in **both** directions).

**A nav key is deliberately NOT a `(defk8saction)`** — it carries no legality
class, because typing "move the cursor" as `Observe` would make the class stop
meaning "this performed a cluster read". The two domains share exactly one
thing, the chord namespace, and `Catalog::resolve` checks it.

## bancada — the banken → tear/mado bridge (`(defbancada)`)

**bancada** (pt-BR: a workbench — tools already laid out, work starts the
instant you sit down). Renamed from `guarita` 2026-07-31: that word was
already double-claimed (`theory/NAMING.md:88`), and on Law 2 it belongs to
the per-request credential check in `akeyless-vpn`'s SHAAR design, whose
"gatehouse sentry checking papers per visit" gloss teaches that job exactly.
This domain took the *container* half of the metaphor and dropped the
*checking-papers* half — and the checking here is `postigo`'s, not its own.
From a selected row, one
authored chord resolves a recipe into a **pre-warmed tear session**: the
panes, their splits, and the fully-resolved `kubectl` argv for the cluster +
namespace + pod banken is looking at — so the operator is fixing the problem
rather than setting up to fix it. `g` → `pod-triage` (3 read panes),
`shift+g` → `pod-break-glass` (logs + `kubectl exec`).

**The chord RESOLVES; `enter` OPENS.** The two are separate keystrokes on
purpose, and it is not ceremony: the preview shows the fully-resolved argv and
the cluster it names *before* anything happens, which for a BREAK-GLASS recipe
is the whole difference between staging a live-effect command and being asked
first. `confirm` is an authored `(defnavkey :keys "return" :intent confirm)`,
so the confirm key is data like every other chord; `NavIntent`'s app
projection is exhaustive, so adding the variant was a compile error until it
was handled. `Action::Confirm` is **repeat-gated** — it is the most expensive
action in the app, and a held `enter` would be one session per OS repeat tick.
A confirmed plan is cleared, so a second `enter` cannot open it twice.

**The load-bearing invariant: there is NO `:legality` kwarg.** A recipe's
`postigo` class is *derived* from its panes — any pane whose `:effect` is
`mutates` makes the whole recipe BREAK-GLASS — so a mutating recipe has no
field in which to claim it observes. A BREAK-GLASS recipe must carry
`:witness`/`:runbook` (`UnwitnessedBancada`); a pure-observe one must not
(`UnneededWitness`). Fail-once measured: deleting `:witness "drzzln"` from
`specs/bancadas.lisp` turns three tests red with
`UnwitnessedBancada { bancada: "pod-break-glass", pane_role: "shell" }`.

**Second fail-once, measured 2026-07-31 for the app-open wire.** Reverting
`Action::Confirm`'s arm from `open_bancada(pending, &self.session)` back to the
old preview-only `preview_bancada(pending)` turns exactly one test red:

```text
assertion `left == right` failed: confirming must OPEN the session through
the seam, not merely re-render the plan
  left: 0
 right: 1
```

The assertion is on **what `MockSessionEnv` recorded**, not on what the overlay
says — "banken reported a session" and "the seam was called" are different
claims and only the second is evidence. The app adds **no legality of its
own**: `open_bancada` calls `bancada::open`, so the mutating pane still has no
`ObservedCommand` value with which to take the unwitnessed arm.

**And the witness is structural at the seam.** `SessionEnv` has exactly two
staging arms taking *different types*: `stage_observed(ObservedCommand)` and
`stage_witnessed(MutatingCommand, &WitnessedAction)`. Both newtypes have
private fields and come only from `PlannedPane::as_observed` /
`as_mutating`, which are `Some` on their own `CommandEffect`. Staging a
mutating command unwitnessed is not forbidden — it has **no argument value
that can reach the call**. A *third* staging arm being added is CI-caught by
`substrate_invariant.rs` (same only-mitigated→CI-caught tier as the
`ClusterEnv` re-add case). Do not round either up.

**`(:context cluster)` is what makes "the RIGHT cluster" true.** A pane opened
without an explicit `--context` lands on whatever the kubeconfig's current
context happens to be, which is not necessarily what banken is reading. An
unknown cluster is a typed **refusal** (`UnresolvedContextField`), never an
emitted `--context ""`. Same for `namespace` / `container`.

**And the same hazard existed ONE LAYER UP, unguarded, until 2026-07-31.**
`banken --live` rode `Client::try_default()` — i.e. the kubeconfig's
`current-context` — so banken could render a real pod table from an entirely
different estate than the operator meant, and nothing would look wrong: the
read succeeds, the rows are real, only the cluster is wrong. Measured on cid
that day: current-context was `us-east-2-staging-eks` (akeyless) while the
cluster under inspection was `camelot-eks`. `--context <name>` is now
**REQUIRED** for `--live` (`banken::cli`, `Invocation::Live` carries a
non-optional `String`, so an unnamed live run has no representation past the
parser — **parse-time-rejected**, not truly-unrepresentable: the library's
`KubeClusterEnv::connect()` still exists as the honest in-cluster
constructor). The refusal prints the current-context it would have used and
every available context name, so it costs one keystroke, not a detour.
`connect_with_context` carries the connected name straight into
`context_name`, so the string that selected the apiserver and the string a
`(defbancada)` resolves against **cannot drift**.

**A staged command is a typed argv, never a shell string** — `:program` plus
`(:literal …)` / `(:context …)` args. `CommandArg` has no join arm on
purpose; the first recipe needing `--field-selector k=v` is the third-use
signal to add a typed `(:joined …)`, not to interpolate.

### Tier ledger — three separate claims, do not collapse them

| Claim | Tier |
|---|---|
| forms compile, legality derives, plan resolves, plan walks a `SessionEnv` | **SHIPPED, mock-green** (`MockSessionEnv`, zero side effects) |
| the LIVE handoff to a running `tear-daemon` | **SHIPPED + PROVEN LIVE 2026-07-30** — `banken/tests/tear_handoff.rs` (feature `tear`, `#[ignore]`d) opened a real 3-pane session and asserted the pre-warmed `kubectl` line **on the first pane's rendered grid**. Fail-once: stubbing `type_into` to send zero bytes turns it red there. |
| pressing `g` then `enter` in banken OPENS a session | **SHIPPED, mock-proven 2026-07-31** — `pending-banken: bancada-app-open` CLOSED. `BankenApp<E: ClusterEnv, S: SessionEnv>` carries the seam as a second type parameter; `g` resolves + previews, `enter` confirms and calls `bancada::open`. Fail-once measured (below). |
| pressing `enter` opens a session on a REAL `tear-daemon` | **PARTIAL — RUN, and it exposed a tear-side gap. Do not round this up.** See below. `pending-banken: bancada-app-open-live`. |

**The `enter`-against-a-real-daemon run, measured 2026-07-31 — and what it
actually proved.** `banken --features tear` in a real 110×26 PTY, `g` then
`enter` on the fixture's `catch-api-7d9f`, rendered:

```text
┌─ BANCADA — OBSERVE — pod-triage (OPENED) ──────────────────────────┐
│ session:  triage-fixture-catch-catch-api-7d9f                      │
│ panes:    3                                                        │
```

Every RPC returned `Ok` — `connect_default`, `new_session_with_source_and_size`,
`get_session`, `apply_layout`, 2× `split_pane`, 3× `send_keys`, `select_pane` —
and any `Err` would have rendered an ERROR overlay instead, so the daemon really
did accept the whole walk and really did hold that session at that moment.

**And yet `tear list` on the very same socket
(`~/.local/share/tear/tear.sock`, the only one `default_socket_path()` resolves
to on macOS, bound by exactly one daemon pid) reports
`(no sessions …)` immediately afterwards, with `session_count: 0` and
`total_bytes_consumed: 0`.** So the session banken opened is **not one the
operator can attach to**. The same disappearance was observed independently
with sessions created through mado's `tear_new_session` MCP tool: snapshot once,
gone by the next call. That points at a tear-side lifecycle/reaping behaviour,
**not** at banken — and banken must not edit tear from this repo.

Honest tier, three separate claims: *the app→tear call path executes end to end*
— **PROVEN**. *A session exists at the moment banken walks the plan* —
**PROVEN** (`get_session` + three `send_keys` succeeded against it). *The
operator lands in a pre-warmed session they can attach to* — **NOT PROVEN**.
The overlay saying `OPENED` is reporting the seam's own truthful return value;
it is not a claim about durability. `pending-banken: bancada-app-open-live`
stays open on that third claim alone.

Note this also **qualifies** the 2026-07-30 `tear_handoff.rs` row above: that
test asserted a real rendered grid and then killed the session, all inside one
process, which is consistent with a session that exists transiently. It remains
true as written; it was never evidence of durability, and must not be read as
such.

- **`pending-banken: tear-argv-spawn`** — the upstream limitation the adapter
  is shaped around. `MultiplexerControl` spawns a pane's program with **no
  argv** (`tear-core/src/inproc.rs:667` passes `&[]` to a `PtyHandle::spawn`
  that *does* take `args: &[String]`, `pty.rs:68`), so the argv is **typed
  into** the pane instead of spawned. Consequence: `banken::tear_session`
  **refuses to quote** — an argv word containing whitespace or a shell
  metacharacter is a typed error naming the word, never escaped, because a
  quoting function IS a shell-string builder. The load-bearing fix is to
  thread `args` through `MultiplexerControl` upstream, after which the adapter
  collapses to a direct spawn with no typing and no refusal. Not done here:
  banken must not edit tear from this repo.
- **`pending-banken: bancada-tear-feature-nix-unverified`** — the `tear`
  feature's dep took banken's `Cargo.lock` from **zero** git sources to four
  (tear, shikumi, gen, plus a second `ishou-tokens` 0.1.4 from git alongside
  the registry one banken-config keeps — measured, so banken-config's
  published-0.1.4 theme receipt is unchanged). Cargo resolves the lock
  feature-independently, so crate2nix sees those git crates even though the
  default build never compiles them: that is the documented
  crate2nix-vs-`fetchgit` base32-vs-SRI drift class. `cargo test`/`fmt` are
  green on default, `live` and `tear`; **`nix build` is UNVERIFIED** — run it,
  and reach for `nix run .#hashfix -- loop` if the cascade fires.
- **`pending-banken: bancada-container-selection`** — M0 has no container
  picker, so a recipe referencing `(:context container)` refuses by name.
- The `stage_witnessed` arm sends the argv **without a newline**: the
  live-effect command sits typed and ready, and the operator's own Enter is
  the final act. banken records the witness; the human still takes the step.

### Four construction seals (each truly-unrepresentable *within its authored surface* — none fleet-wide)

1. **`WardVerdict`** — fields private, `evaluate` the only constructor, so a
   ward verdict claiming `Green` over an absent core metric is
   unconstructible. The BANKEN.md §V BROKEN-METRIC guard is a **structural
   cap** consulting no catalog row, so omitting / filtering / mis-authoring the
   `broken-scrape` rule cannot switch it off. A guard that depends on a catalog
   row is a guard with an off switch. Fail-once measured: deleting the cap line
   turns `a_dead_scrape_can_never_report_green` red with `a blind reading must
   be Unknown, never Green — with catalog []`.
2. **`Attestation::Proven`** — `OutcomeChainRef` has no `Deserialize` that
   succeeds, so `:attested (:kind proven …)` is **parse-time rejected** with an
   error naming §IX C-controller. §V/§IX's "labelled (computed), NEVER
   (proven)" was reviewer discipline; it is now a refusal. The variant STAYS
   (★★ MODULARIZE, DON'T DELETE) — revival is making one constructor `pub`.
3. **`Catalog`** — fields private, `resolve`/`load_catalog` the only
   constructors, so an **unresolved** bundle cannot be held. The seven domains
   reference each other by name and every such join was a silent-failure class.
4. **`SessionPlan`** — fields private, `bancada::plan` the only constructor,
   so a plan's `legality` cannot disagree with its `panes` (it is *derived
   from* them). Same shape, same qualified tier as the three above.

### `closed_catalog!` (★★ EMITTER SUBSTRATE)

Ten axes needed the same variants+`ALL`+`label()` triple. The macro makes the
variant list and the `ALL` list the **same list**, so a variant missing from
`ALL` goes from CI-caught to truly-unrepresentable — **for the ten it emits**.
The four hand-written `types.rs` axes stay CI-caught.
**`pending-banken: closed-catalog-macro-backfill`** — `ViewKind`'s PascalCase
serde wire form (`ResourceTable`, what `specs/views.lisp` authors) vs its
kebab `label()` is the one real obstacle; porting it changes nothing until the
authored `:kind` values are re-spelled.

### Killed as over-abstraction (state it, don't re-propose it)

`(defcolumn)` — a named column library would carry `(header, field)`, exactly
what `ColumnSpec` already carries inline: pure indirection, zero invariant.
`(deffixture)` — test data, one instance forever. `PathologySpec.gates_green`
— cut once the green gate became the structural cap; it would have been an
unused field masquerading as an invariant.

## Governing methods

banken is a **Quadro** (terminal UI) — QUADRO.md governs the render; it composes
egaku + egaku-term + moldura (host: mado), shikumi config, ishou theme, awase keys.
It is a **naturalize** worked example — a **half-citizen by design** (naturalizes
k9s's observe half, not its mutate half). Per its own 4-check citizenship test it
is currently a **resident** (native ✓ / no-leak ✓ / proven-on-fixtures ✓ /
proven-live ✗ until a cluster is reachable / known ✓).

## Build status (tier-honest — never round up)

- **SHIPPED (mock-green, no cluster):** `banken-spec` = the `postigo` TYPED-SPEC +
  INTERPRETER triplet, plus the four health/diagnosis/navigation domains below
  (**105 tests green** — `cargo test -p banken-spec`). The
  citizenship core, provable without a cluster, per BANKEN.md §III.
- **SHIPPED (fixture-green, no cluster):** the `banken` **binary crate** — a
  runnable `:pods` navigator (`cargo run -p banken`, `cargo run -p banken -- --help`).
  The full render + interaction pipeline over a **fixture source**
  (`FixtureClusterEnv`, the sui-spec `MockEnv` discipline): the `:pods` table
  (`table::PodTable` model + `render::draw_pod_table` cell drawer over egaku-term
  0.3.1's typed `Buffer`/`Cell`/`Style` — **no `format!()` of VT**), arrow-key
  selection + sort, alt-screen enter/Drop-restore via `egaku_term::AsyncApp` +
  `run_async`, and the **postigo dispatch wired through the UI** (`l`→OBSERVE logs,
  `s`→DECLARE full-manifest preview, `S`→BREAK-GLASS witnessed record — every path
  through `banken_spec::apply`, no live-mutate path). Proof: **194 workspace tests**
  green (`cargo test`; 196 under `--features live` and under `--features tear`)
  incl. a `TestBackend` golden-frame test (asserts via
  `to_lines()`/`cell()`, never `.contains()`) + a postigo-dispatch integration
  test; `cargo fmt --check` clean. Counts moved 195 → 194 with the
  `TableView` adoption below: six `PodTable` model tests left with the model
  they tested (egaku owns them now, at 172 green there), and five landed here —
  three on the pod *binding* + the viewport gate + the status-color gate.
  - **`pending-banken: promote-tableview-to-egaku` — CLOSED (2026-07-31).**
    egaku `e602369` landed `Selectable` + `TableView<R: TableRow>`, lifted from
    this very file, and `banken/src/table.rs` collapsed onto it exactly as the
    token said it would: **492 → 290 lines** (330 removed in the diff), and what
    is left is the pod *binding* — `pod_columns`, `pod_default_sort`, the
    `ResourceKind`, and `from_view`'s `(defk8sview)` reader — over a
    `TableView<Row>` reached through `PodTable::view()` / `view_mut()`.
    `impl egaku::TableRow for Row` lives in **banken-spec**, not banken: the
    orphan rule leaves no other legal home (`E0117` otherwise).
    - **One invariant got STRICTER on the way out.** `from_view`'s own
      default-sort check is gone as *redundant*, not merely moved:
      `TableView::new` performs it on the **only** constructor, so
      `PodTable::pods` is covered too. `TableError::UnknownSortColumn` maps to
      `SpecError::Binding` with the same message shape, and
      `a_default_sort_naming_an_undeclared_column_fails_the_catalog` still
      pins it. Tier is unchanged: **parse-time-rejected**, not
      truly-unrepresentable — a `SortKey` naming a nonexistent column can be
      written, just not installed.
    - **The DRAWER did not collapse with the model, on purpose.** See
      `pending-banken: column-render-hints` below — `status_style` (the §V
      pathology color axis) is app knowledge that egaku-term's lifted
      `draw::table_with` deliberately does not carry, and trading a red
      `CrashLoopBackOff` for CJK column alignment banken can never exercise is
      a bad trade. The **bottom-anchored viewport** was ported across anyway,
      which fixed a live bug: banken drew from row 0 and clipped, so on a
      cluster with more pods than the terminal has rows `j` moved the cursor
      off screen (`the_viewport_scrolls_to_keep_the_selection_visible`).
    - Fail-once measured on the adoption's load-bearing seam — `TableRow::cell`
      rewritten to ignore its `field` and return the first cell:
      **2 red / 51 green**,
      `table::tests::the_pod_binding_reaches_the_lifted_model`
      `left: ["1/1", "1/1", "1/1"] / right: ["Running", "Pending", "CrashLoopBackOff"]`
      and `render::tests::an_unhealthy_status_draws_red_on_an_unselected_row`
      `the unhealthy row is drawn`. Restored.
- **SHIPPED (unit-green, 16 tests):** `banken-config` = the ★★ CONFIGURATION MANAGEMENT
  surface. `BankenConfig` carries **both faces on one struct** —
  `#[tatara(keyword = "defbanken")]` and `impl shikumi::TieredConfig` — exactly
  the `escriba-config` precedent (`escriba/escriba-config/src/lib.rs:21` + `:143`,
  whose own comment at `:128-142` states the division of labour). **shikumi owns
  DEPLOYMENT/runtime** (context, namespace, poll interval, theme, scrollback);
  **tatara-lisp owns DOMAIN AUTHORING** (all six `(def…)` forms above); the
  seam between them is exactly one field, `specDir`. `deny_unknown_fields` on the
  YAML face (fail-once proven: a valid doc + one extra key deserializes clean
  without the attribute, `unknown field` with it).
  - **`pending-banken: strict-kwargs-reader`** — honest asymmetry, measured:
    the YAML face rejects an unknown key; the **Lisp face is LOOSE** at
    `tatara-lisp = "0.3.3"` (`domain::parse_kwargs`,
    `tatara-lisp-0.3.3/src/domain.rs:63-78`, maps every `:keyword` and the derive
    reads only known ones, so a typo is silently dropped). Pinned by a deliberate
    CHARACTERIZATION test (`lisp_face_silently_ignores_an_unknown_kwarg`) that
    turns red on adopting a strict reader.
    **Corrected 2026-07-30 — the row was renamed because its premise was wrong.**
    It used to be called `tatara-lisp-0.3.x-adoption`, i.e. it asserted that
    adopting 0.3.x *is* the fix. banken now runs canonical 0.3.3 and the
    characterization test **stayed green**: 0.3.3 ships `parse_kwargs` and **no
    strict variant at all**, and its derive emits a call to exactly that lenient
    function. Verified by reading the supplier's `domain.rs`, not by inferring
    capability from a version number — the version bump was never the blocker.
    The row is now blocked on the strict reader being *written* upstream.
- **SHIPPED (unit-green):** the **ishou fleet theme derivation** (Quadro T7).
  `BankenConfig.theme` is the typed `ishou_tokens::FleetTheme`, not a string, and
  `prescribed_default()` derives its visual half from
  `FleetDefaults::prescribed()` via `FleetThemedConfig::from_fleet` — the mado
  pattern (`mado/src/config.rs:3468`). **No local palette constant exists**; a
  fleet re-theme reaches banken on the next compile. Pinned by
  `ishou_tokens::convergence::Guard::for_app("banken")` (`.expect_theme` +
  `.expect_scrollback_lines`), shaped after `escriba-tui/src/render.rs:588-590`
  since a terminal app owns a theme + a scrollback cap but not a font.
  `from_fleet` is hand-written, not `#[derive(FleetThemed)]`
  (pleme-fleet-themed-derive 0.1.1 IS published) — two fleet-derived fields do
  not meet the third-use test.
  - **★ Verify-against-the-artifact receipt (2026-07-29).** The *published*
    `ishou-tokens` 0.1.4 that banken compiles against prescribes **`Vellum`**;
    the local `ishou` working copy has `FleetTheme::prescribed_default() ==
    PlemeDark` **unpublished at the same version number**. Authoring
    `:theme "pleme_dark"` from reading ishou's source failed 3 tests. Every
    assertion now derives its expectation from `FleetDefaults::prescribed()`;
    the ONE concrete literal is `:theme` in `banken-config/specs/banken.lisp`,
    and `prescribed_mirrors_the_authored_lisp` makes a fleet re-theme a loud
    failure there instead of silent drift.
- **SHIPPED (unit-green):** the **awase keybinding layer** — closes a documented
  BANKEN.md over-claim. §II rowed `awase BindingMap/KeyChord + detect_conflicts +
  KeyRepeatGate` as **SHIPPED** while `K8sActionSpec.keys` was a bare `String`
  and banken consumed **zero** awase. Now: `banken_spec::chord::ActionChord`
  wraps `awase::Hotkey` (parse-time-rejected: an unparseable chord has no typed
  value), `bindings::build_binding_map` makes two actions on one chord a
  `SpecError::ChordConflict` naming both sides — **never last-write-wins** — and
  `awase::KeyRepeatGate` debounces the three postigo action chords at the
  dispatcher (navigation deliberately exempt).
  - **Correction to BANKEN.md §III.a:** the right awase type is **`Hotkey`, not
    `KeyChord`** — `awase::KeyChord` (`awase/src/chord.rs:11-21`) is a two-step
    leader→follower chord; `KeyMode.bindings` is keyed on `Hotkey`
    (`awase/src/mode.rs:20`). Naming `KeyChord` was a type error.
  - **Correction to `specs/actions.lisp`:** the shell action is `:keys "shift+s"`,
    not `"S"`. `Hotkey::parse` is case-insensitive (`awase/src/hotkey.rs:435`), so
    `"S"` WAS the `scale` chord — a real, silent collision, now CI-red (measured:
    reverting to `"S"` fails 3 tests with `ChordConflict { chord: "s", existing:
    "scale", incoming: "shell" }`). egaku-term already delivers held-shift as
    `shift+s`, so `"S"` was also the value disagreeing with the runtime.
  - **Where the duplicate is actually caught:** at `KeyMode::add_binding`'s
    returned previous binding, **not** by `awase::detect_conflicts` — whose own
    doc (`awase/src/conflict.rs:32-37`) says the same-hotkey-twice class is
    "only possible with external config merging — `HashMap` insert deduplicates
    in normal use". `detect_conflicts` is still run for the chord-leader class it
    does own. Honest tier: **eval/parse-time-rejected**, not
    truly-unrepresentable — two `(defk8saction)` forms *can* be authored with one
    `:keys`; the builder rejects the pair and
    `authored_actions_have_no_chord_conflict` makes the shipped catalog's
    cleanliness a build-time fact.
  - **`pending-banken: keymap-derived-from-catalog` — CLOSED (2026-07-30).**
    `app::keymap_from_catalog`, `table::PodTable::from_view`,
    `app::key_legend` and `--help`'s KEYS block all read
    `banken_spec::load_catalog()`; `BankenApp::try_new` is **fallible** exactly
    so a spec failure surfaces instead of falling back to chords the legend
    does not describe. Proof that the derivation is real, not decorative:
    `respelling_an_authored_chord_moves_the_runtime_binding` re-spells `o`→`z`
    in the authored source and asserts the old chord is *gone*.
    - **The legend already LIED.** The status-line literal advertised `S` for a
      chord the runtime binds as `shift+s`. It is the one surface that tells
      the operator which gate a keystroke crosses, so it was the
      highest-consequence hand-list in the app; it is now derived from the
      authored chord + the typed `LegalityClass::label()`.
    - **`chord_to_combo` grew a MEASURED translation table** — awase
      `escape`→egaku-term `esc`, `return`→`enter`, verified against
      `awase-0.1.1/src/hotkey.rs:346-347` vs
      `egaku-term-0.3.1/src/event.rs:42-43` — plus the eleven nav/editing keys
      that agree by `Display`. Without it `(defnavkey :keys "escape")` could
      not reach the runtime at all. **`space` stays REFUSED**: awase spells it
      `space`, crossterm delivers `Char(' ')`, no safe translation.
    - **THE FIELD JOIN, fixed at its cause.** The authored view said
      `:field phase` while every reader emitted a cell keyed `"STATUS"` — two
      vocabularies for one thing, so the authored `:field` was **decorative**
      and a typo in it was invisible. Readers now key cells by the authored
      field; `table::IDENTITY_FIELD` names the one reserved identity field in
      one place; `PodTable::unresolved_fields()` reports any column whose field
      no row carries. Fail-once measured: reverting ONE reader key turns
      `every_authored_column_resolves_against_the_readers_rows` red with
      `left: ["phase"]`.
    - Two new refusals where a silent wrong answer lived: a `:default-sort`
      naming an undeclared column (it previously sorted by a cell key no row
      carries — every row compared equal, so the table came out in whatever
      order the reader returned), and an unprojectable authored chord (refused
      *by name*, naming both the chord and the form that authored it).
    - `describe` is authored OBSERVE with no app panel;
      `app::unbound_action_names` reports it and `--help` prints
      `(not wired yet)` rather than advertising a dead chord.
  - **`pending-banken: column-render-hints`** — `render.rs` still selects the
    pathology-colored column by the magic header `"STATUS"`. The destination is
    a typed `:colorize` hint on the authored `ColumnSpec` so a magic string is
    not the selector at all.
    **This row is now also what keeps `render.rs` alive at all (2026-07-31).**
    egaku-term `735d936` shipped a generic `draw::table_with` over
    `egaku::TableView<R>`, lifted from this drawer, and banken did **not**
    adopt it — the lift deliberately left `status_style` behind as app
    knowledge, so adopting today would delete the red `CrashLoopBackOff` for
    nothing. Closing this row is what makes the swap free: `:colorize` on the
    authored `ColumnSpec` → `egaku::Column` → `egaku_term::draw::table_with`,
    after which `banken/src/render.rs` deletes and the call is one line.
    Two egaku-side changes; banken must not make either from this repo.
  - **`pending-banken: pathology-taxonomy-backfill`** — three rules ship; the
    autorevivy taxonomy is 35 classes. Authoring rules whose evidence nothing
    supplies would be a catalog of rules that can never fire, so the backfill
    is gated on autorevivy's detection stream deploying beyond camelot-shadow
    (BANKEN.md §V's ~15% live-data-reuse note).
  - **`pending-banken: per-dimension-band-evidence`** /
    **`pending-banken: drill-step-view-resolution`** /
    **`pending-banken: render-green-gate`** — `EvidenceKind::BandPhase` reads
    "ANY band unhealthy" rather than a named dimension; a `DrillStep.view` is a
    free label where `DrillSpec.from` is a resolved reference; a renderer
    computing its own bare `Verdict::Green` bypasses the `WardVerdict` seal
    (**only-mitigated**).
- **SHIPPED + PROVEN LIVE (2026-07-31):** the live-cluster **pod read**
  (`live::KubeClusterEnv`, feature `live`) — a real typed `kube`/`k8s-openapi`
  apiserver client (NO `kubectl` subprocess) behind the same `ClusterEnv` seam.
  **`pending-banken: live-read` CLOSED.** `camelot-eks` (EKS v1.33, AWS SSO
  exec-credential auth) returned **109 pods**, matching `kubectl get pods -A` at
  the same moment, rendered through the whole pipeline. Two runs:
  `banken/tests/live_read.rs` (`#[ignore]`d; refuses to guess a context —
  `BANKEN_LIVE_CONTEXT`, no default) and the **binary in a real 120×28 PTY**.
  Default the binary to the fixture; `--live --context <name>` selects the live
  read (and errors clearly without the feature, never a silent fixture fallback).
  - **★ The measurement immediately paid for its own tier (read this before
    ever writing "compiles, therefore works").** `--features live` had compiled
    clean for weeks; the first real read **aborted the process** —
    `Could not automatically determine the process-level CryptoProvider from
    Rustls crate features` (`rustls-0.23.42/src/crypto/mod.rs:249`). kube 0.99's
    `rustls-tls` pulls rustls with `log,logging,std,tls12` and selects
    **neither** `ring` nor `aws-lc-rs`, so there is no process default and the
    first TLS handshake panics rather than erroring. The DESIGN tier on this row
    was *exactly right* and rounding it up would have shipped a panic. Fixed at
    the cause: `rustls` is a named `live` dep with the `ring` provider (`ring`
    over `aws-lc-rs` — the latter is FFI to a C library), plus an explicit
    `ensure_crypto_provider()` at both connect paths so a future crate enabling
    the other provider is a no-op rather than a fresh panic.
  - **Scope of the live claim, do not widen it: `list_resources` for pods, and
    nothing else.** `get_resource`, `logs`, `events`, `topology`,
    `health_signals`, `declare`, `break_glass` all still return typed
    "not yet wired" errors on the live backend. AGE renders `-` on every row
    (the `creation_timestamp` derivation needs a clock; `-` beats a wrong
    value). The watch informer is still a poll —
    **`pending-banken: live-watch`**, and `BankenApp::refresh()` still has no
    caller, so a live table does not update until a keystroke.
- **DESIGN / next arc (M1→M4):** the watch informer (poll→true watch), the health
  ward's **RENDER** (its `(defward)` vocabulary + the `WardVerdict` evaluator are
  SHIPPED mock-green; no ward panel is drawn yet — do not read the vocabulary as
  a shipped screen), the `TreeView`/XRay, the DECLARE branch+PR flow + the
  resource→`release.yaml` resolver, the mado-MCP e2e gate over a live read. Not
  built yet — do not imply otherwise.

## Standing rule

Every PR advances a `theory/BANKEN.md` §VII ledger row's tier or leaves a typed
`pending-banken: <row>` note. The `substrate_invariant.rs` guard stays green (any
`ClusterEnv` mutating-method addition fails the build). No DECLARE rail is
direct-to-main. Toolchain: **`tatara-lisp = "0.3.3"`, and it is the ONLY tatara
dependency** — never add `tatara-lisp-derive` back. The derive macros arrive
RENAMED through the lib (`tatara_lisp::DeriveTataraDomain` /
`DeriveKeywordSexp`, `tatara-lisp/src/lib.rs:54`), and note
`tatara_lisp::TataraDomain` is the **trait** while `DeriveTataraDomain` is the
**macro**. This replaces the old two-crate exact pin (`=0.2.4` + derive `=0.2.2`)
whose whole purpose was to stop the two versions drifting into a derive that
called a runtime symbol the lib didn't export. Naming one dep removes that
failure mode by construction — 0.3.3's lib pins its own derive at `=0.3.3`, so
the pair cannot desynchronize and there is nothing left for a caret range to
break. Crates now resolve from canonical `pleme-io/tatara-lisp`; the retired
`pleme-io/tatara` sibling is `publish = false`, so never source them from it.
