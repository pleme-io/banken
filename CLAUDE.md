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

## The authored vocabulary — SIX domains, one resolved catalog

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
| `(defnavkey)` | a navigation chord + its local-UI intent | 7 |

plus `(defbanken)` in `banken-config` (the deployment face). Every domain has
a lisp-roundtrip test and a `catalog.rs` axis on the `REQUIRED_AXES` roster
(★★ CATALOG REFLECTION, gated in **both** directions).

**A nav key is deliberately NOT a `(defk8saction)`** — it carries no legality
class, because typing "move the cursor" as `Observe` would make the class stop
meaning "this performed a cluster read". The two domains share exactly one
thing, the chord namespace, and `Catalog::resolve` checks it.

### Three construction seals (each truly-unrepresentable *within its authored surface* — none fleet-wide)

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
   constructors, so an **unresolved** bundle cannot be held. The six domains
   reference each other by name and every such join was a silent-failure class.

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
  (**79 tests green** — `cargo test -p banken-spec`). The
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
  through `banken_spec::apply`, no live-mutate path). Proof: **149 workspace tests**
  green (`cargo test`; 79 banken-spec + 54 banken + 16 banken-config) incl. a `TestBackend` golden-frame test (asserts via
  `to_lines()`/`cell()`, never `.contains()`) + a postigo-dispatch integration
  test; `cargo fmt --check` clean. **54 tests green** in the binary crate
  (`cargo test -p banken`).
  - **`pending-banken: promote-tableview-to-egaku`** — the `TableView`/`draw::table`
    are built **in banken for now** (egaku 0.1.4 has only `ListView`, egaku-term
    0.3.1 has no `draw::table`; promoting a generic widget needs an egaku +
    egaku-term publish cycle, out of reach without push/nix). Tier-honest interim,
    not a silent fork — collapses to a thin adapter when egaku gains `TableView<Row>`.
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
- **DESIGN / UNTESTED-LIVE:** the live-cluster read (`live::KubeClusterEnv`, feature
  `live`) — a real typed `kube`/`k8s-openapi` apiserver client (NO `kubectl`
  subprocess), wired behind the same `ClusterEnv` seam, **compiles + its pure
  `pod_to_row` projection is tested** (`cargo test -p banken --features live`), but
  its network read is **never exercised** this session (no cluster reachable —
  rio/camelot VPN-gated, local k3s down). `pending-banken: live-read`. Default the
  binary to the fixture; `--live` selects it (and errors clearly without the feature,
  never a silent fixture fallback). "renders from fixtures" is PROVEN; "live cluster
  read" is DESIGN — never conflate.
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
