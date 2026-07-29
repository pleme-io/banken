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
is currently a **resident** (native ✓ / no-leak ✓ / proven-on-fixtures ✓ /
proven-live ✗ until a cluster is reachable / known ✓).

## Build status (tier-honest — never round up)

- **SHIPPED (mock-green, no cluster):** `banken-spec` = the `postigo` TYPED-SPEC +
  INTERPRETER triplet (18 tests green — `cargo test -p banken-spec`). The
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
  through `banken_spec::apply`, no live-mutate path). Proof: 50 workspace tests
  green (`cargo test`) incl. a `TestBackend` golden-frame test (asserts via
  `to_lines()`/`cell()`, never `.contains()`) + a postigo-dispatch integration
  test; `cargo fmt --check` clean.
  - **`pending-banken: promote-tableview-to-egaku`** — the `TableView`/`draw::table`
    are built **in banken for now** (egaku 0.1.4 has only `ListView`, egaku-term
    0.3.1 has no `draw::table`; promoting a generic widget needs an egaku +
    egaku-term publish cycle, out of reach without push/nix). Tier-honest interim,
    not a silent fork — collapses to a thin adapter when egaku gains `TableView<Row>`.
- **SHIPPED (unit-green):** `banken-config` = the ★★ CONFIGURATION MANAGEMENT
  surface. `BankenConfig` carries **both faces on one struct** —
  `#[tatara(keyword = "defbanken")]` and `impl shikumi::TieredConfig` — exactly
  the `escriba-config` precedent (`escriba/escriba-config/src/lib.rs:21` + `:143`,
  whose own comment at `:128-142` states the division of labour). **shikumi owns
  DEPLOYMENT/runtime** (context, namespace, poll interval, theme, scrollback);
  **tatara-lisp owns DOMAIN AUTHORING** (`(defk8sview)`/`(defk8saction)`); the
  seam between them is exactly one field, `specDir`. `deny_unknown_fields` on the
  YAML face (fail-once proven: a valid doc + one extra key deserializes clean
  without the attribute, `unknown field` with it).
  - **`pending-banken: tatara-lisp-0.3.x-adoption`** — honest asymmetry, measured:
    the YAML face rejects an unknown key; the **Lisp face is LOOSE** at the pinned
    `=0.2.4` (`domain::parse_kwargs`, `tatara-lisp-0.2.4/src/domain.rs:52-67`,
    maps every `:keyword` and the derive reads only known ones, so a typo is
    silently dropped). Pinned by a deliberate CHARACTERIZATION test
    (`lisp_face_silently_ignores_an_unknown_kwarg`) that turns red on adopting a
    strict reader. tatara-lisp 0.3.x is **unpublished** (crates.io tops out at
    0.2.5) and a consolidation is mid-flight upstream — the pins stay as-is.
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
  - **`pending-banken: keymap-derived-from-catalog`** — `app::default_keymap`
    still hand-binds its three postigo chords beside the navigation keys;
    `app_keymap_agrees_with_the_authored_chords` is a *drift gate* over that
    hand-list, not its elimination. Deriving the postigo half from
    `load_actions()` needs `BankenApp::new` to become fallible (a spec-load
    failure must not silently fall back to hardcoded chords).
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
  ward (Pulses/Popeye/QUIET), the `TreeView`/XRay, the DECLARE branch+PR flow + the
  resource→`release.yaml` resolver, the mado-MCP e2e gate over a live read. Not
  built yet — do not imply otherwise.

## Standing rule

Every PR advances a `theory/BANKEN.md` §VII ledger row's tier or leaves a typed
`pending-banken: <row>` note. The `substrate_invariant.rs` guard stays green (any
`ClusterEnv` mutating-method addition fails the build). No DECLARE rail is
direct-to-main. Toolchain: `tatara-lisp` is exact-pinned (`=0.2.4` + derive
`=0.2.2`, sui's blessed interim pair — a caret range floats to 0.2.5 and breaks
the 0.2.2 derive; keep the pin).
