# banken build status — the tier ledger

Moved verbatim from CLAUDE.md (2026-11-11). Tier-honest: never round up.

## Build status (tier-honest — never round up)

- **SHIPPED (mock-green, no cluster):** `banken-spec` = the `postigo` TYPED-SPEC +
  INTERPRETER triplet, plus the four health/diagnosis/navigation domains below
  (**105 tests green** — `cargo test -p banken-spec`). The
  citizenship core, provable without a cluster.
- **SHIPPED (fixture-green, no cluster):** the `banken` **binary crate** — a
  runnable `:pods` navigator (`cargo run -p banken`, `cargo run -p banken -- --help`).
  The full render + interaction pipeline over a **fixture source**
  (`FixtureClusterEnv`, the sui-spec `MockEnv` discipline): the `:pods` table
  (`table::PodTable` model + `render::draw_pod_table` cell drawer over egaku-term
  0.3.1's typed `Buffer`/`Cell`/`Style` — **no `format!()` of VT**), arrow-key
  selection + sort, alt-screen enter/Drop-restore via `egaku_term::AsyncApp` +
  `run_async`, and the **postigo dispatch wired through the UI** (`l`→OBSERVE logs,
  `s`→DECLARE full-manifest preview, `S`→BREAK-GLASS witnessed record — every path
  through `banken_spec::apply`, no live-mutate path). Proof: **224 workspace tests**
  green (`cargo test`, and identically under `--features live`)
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
      `pending-banken: column-render-hints` below — `status_style` (the
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
  over-claim. The design rowed `awase BindingMap/KeyChord + detect_conflicts +
  KeyRepeatGate` as **SHIPPED** while `K8sActionSpec.keys` was a bare `String`
  and banken consumed **zero** awase. Now: `banken_spec::chord::ActionChord`
  wraps `awase::Hotkey` (parse-time-rejected: an unparseable chord has no typed
  value), `bindings::build_binding_map` makes two actions on one chord a
  `SpecError::ChordConflict` naming both sides — **never last-write-wins** — and
  `awase::KeyRepeatGate` debounces the three postigo action chords at the
  dispatcher (navigation deliberately exempt).
  - **Correction to the design:** the right awase type is **`Hotkey`, not
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
    is gated on autorevivy's detection stream deploying beyond alpha-shadow
    (~15% live-data-reuse note).
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
  **`pending-banken: live-read` CLOSED.** `alpha-eks` (EKS v1.33, AWS SSO
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
    value).
- **SHIPPED + PROVEN LIVE (2026-08-08): the WATCH plane.
  `pending-banken: live-watch` CLOSED.** `banken/src/absorb.rs` +
  `KubeClusterEnv::spawn_pod_absorber` replace the 1 Hz poll with
  `kube::runtime::watcher` under `Config::streaming_lists()` +
  `.default_backoff()`. Measured on `alpha-eks`: **absorbed 83 pods by watch,
  generation 1, matching `kubectl get pods -A` at the same moment (83)**
  (`tests/live_read.rs::the_watch_plane_absorbs_from_a_real_cluster`,
  `#[ignore]`d, refuses to guess a context). 224 workspace tests green with and
  without `--features live`.
  - **The C-watch ceiling was a fact about this crate's Cargo features, not
    about kube-rs.** the design called the informer "unbuilt substrate banken
    must build"; `kube-runtime` 0.99 — the version already resolved — ships
    `Config::streaming_lists()`, `bookmarks: true` by default, `metadata_watcher`
    and the whole resourceVersion/410 machine. Enabling the `runtime` feature was
    the entire acquisition. **Generalize: before grading a capability ABSENT,
    check whether it is merely unenabled.**
  - **What it replaced, measured:** the poll moved **3,580,862 B per tick** at
    1 Hz — 3.4 MiB/s decoded, **96 GiB per 8-hour day, per instance**. A 30 s
    watch on the same cluster moved **0 bytes**. Poll cost is proportional to
    state SIZE; delta cost to CHANGE.
  - **One reader, two producers.** The app holds a `Despensa` and never learns
    which producer filled it: `--live` attaches the watch stream, the fixture
    attaches `spawn_poll_absorber` over `izumi::refresh` (consumed, not
    re-rolled). An enum of `Watched|Polled` in the app would have
    put the transport in the app's own type.
  - **The wake is content-gated, and a PHASE change also wakes.**
    `LiveStore::push` bumped its generation unconditionally, so the old feed
    repainted ~60×/min against a still cluster. Gating on the row hash *alone*
    would have been the opposite error — a `Synced → Degraded` transition must
    repaint the status line even though not one row moved, or a dead watch
    renders as a healthy one.
  - **`pending-banken: grip` — CLOSED (2026-08-08).** `Row` carries a `Uid`
    (`metadata.uid`) and a `version`; `TableRow::identity()` is the uid, never
    the name; `Row` lost `Default`; `pod_to_row` is fallible (a uid-less object
    is a malformed read, not a row). `ClusterEnv::grip(&ObjectId) -> Result<Grip,
    GripError>` **re-reads at the act**, and `open_bancada` takes a `&Grip`, so
    staging without a re-verified subject has no argument form.
    - **The mechanism is the `!Send` marker, not a convention.** `AsyncApp`
      requires app state to be `Send`, so a `Grip` cannot be stored between the
      `g` preview and the `enter` confirm — the compiler refuses. Carrying a
      stale authorisation forward is not a thing an author can write.
    - **Why not a carried witness** (every earlier design tried it): rows
      launder at `app.rs`'s `set_rows` into `egaku::TableView<Row>`, a foreign
      container that knows nothing about freshness, so a stale reading held
      beside a live table `cargo check`s clean. Re-deriving at the act dissolves
      the launder point instead of guarding it.
    - `grip_against` is the ONE place the uid comparison lives; a backend
      supplies rows and never decides what counts as the same object. A failed
      read is `Blind`, never `Vanished` — "it is gone" and "I cannot see" must
      not authorise an act the same way.
    - **The replica still keys on `(namespace, name)` and that is correct** —
      k8s guarantees it unique at any instant, and a recycle arrives as an
      update with NO delete, so uid-keying would strand the dead object. The uid
      belongs on IDENTITY.
    - Fail-once, both measured: uid→name compare turns 2 tests red; making
      `Grip` `Send` fails a `compile_fail` doctest. **The first `!Send` test was
      VACUOUS** (a method-resolution probe — fully-qualified trait syntax always
      resolves to the blanket impl and can never become ambiguous), caught by
      running the fail-once.
    - `pending-banken: grip-window` — the object can still change between the
      grip and the act itself. Microseconds rather than think-time, but not
      zero, and no local type closes it (C1).
  - **SHIPPED: a typed `ListStrategy{Streaming, ListWatch}`** — named, never
    probed, and **no `Auto` variant**: an unannounced downgrade to a weaker read
    path is the silent-degradation class banken refuses. A typo REFUSES and the
    refusal names the legal values; `--help`'s list is derived from
    `ListStrategy::ALL`. This is also what lets banken read a
    conformance-partial apiserver: `ListWatch` sends zero params a minimal
    server need not implement (`watcher.rs:412-413`), where `Streaming` against
    a server that does not negotiate `sendInitialEvents` stalls in `Absorbing`
    with no rows and no error.
  - **SHIPPED: `banken-config` is finally wired into the binary**, with a
    writable overlay tier (`discover_all` + `load_merged`). It was NOT a
    dependency: `main.rs` hardcoded the interval, so every authored knob was
    born dead with the config crate's 16 tests green. And 4 of 4 fleet app
    configs are `/nix/store` symlinks (41 under `~/.config`), so a single-file
    loader gives an operator no way to change a value short of a rebuild.
    **`pending-banken: manual-refresh-mode` — CLOSED (2026-10-15):** an
    authored `0` now means the env-backed source reads once and stays live; a
    live cluster is watched, so the period never applied to it.
- **The Armazém, A1–A4 of `docs/ARMAZEM.md` (2026-10-15).** Every view reads
  an `izumi_armazem` shelf through an `Interesse`; `switch_view` and the filter
  re-apply never call `list_resources` (proven by
  `app::tests::no_view_reads_the_env_on_the_app_task`, red with one inline
  read). `KindKey` (`pod/*`, `pod/<ns>`; cluster-scoped kinds take only `*`) +
  `KubeSource` (one generic watch per key, a total `match` over
  `ResourceKind`, `ListStrategy` + `default_backoff()`), resume from the
  stored rv with a bounded open and a journalled relist on 410 or no-open
  (`tests/armazem_resume.rs`, a fake apiserver). The zelador ticks in the
  binary; its knobs are `armazem:` and the MCP page knobs `streams:` in the
  overlay tier, izumi's defaults (lazy + vigy). Dormant rows render
  `DORMANT`, never as live. MCP reads the same kind of Armazém through
  `izumi_mcp::Streams`: `banken_list`/`banken_events`/`banken_logs` page and
  follow by `cursor`, `banken_absorb` is the inventory, a still-absorbing
  first read is `blind`, and `--mcp-socket PATH` serves the screen's own
  store. **`pending-banken: absorb-multi-gvk` — CLOSED for the typed kinds**;
  discovery-driven kinds remain M3/M4.
  - `pending-banken: resume-token-after-relist` — after a relist the token is
    the last listed object's rv, because kube-runtime's watcher does not
    surface the list or bookmark rv. On engenho that rv was old enough to
    answer 410, so the resume relisted (correctly, and journalled). A resume
    lands when a watch event arrived after the list.
  - `pending-banken: permit-read-on-app-task` — `switch_view`'s
    SelfSubjectAccessReview still runs on the app task; the rows no longer do.
  - `pending-banken: log-follow-ends` — a followed container log that ends
    (exit, restart) stays `degraded` with its lines; it is not re-followed.
  - `pending-banken: armazem` — A5 (`KubeClusterEnv::from_config` +
    engenho's `client_conformance`) and A6 (`engenho_live.rs`) remain.
  - **`pending-banken: absorb-display-fidelity` — OPEN, and it now carries a
    DESIGN CONFLICT this repo created on 2026-08-08.** Measured: `Accept:
    …as=Table` with `includeObject=None` is **170 B/pod vs 18,747** (110×) and
    its cells carry a server-rendered `Age` (the pod `AGE renders -` gap it
    would have closed is closed client-side since 2026-10-15) — but it carries **no uid** (`row.object` is `null`), and uid
    is now REQUIRED on `Row`. `includeObject=Metadata` carries uid at 5,830
    B/pod (3.2×) and still ships `managedFields`. So the three fidelities are
    not a ladder: **Display renders, Metadata identifies, Full describes**, and
    the table's cursor identity (unique at an instant) is a different question
    from the act's identity (unique across recreate). Resolve that before
    building, not during.
- **Live read of engenho-ryn, 2026-10-15 — three defects closed.** Pod `AGE`
  now goes through `row_of` like every other kind (`pod_to_row` had
  hardcoded `-`). `banken_logs` is bounded in bytes as well as lines: the
  apiserver read carries `limitBytes` (1 MiB) beside `tailLines` (500, also
  enforced client-side), the MCP answer keeps the newest lines within 32 KiB,
  and every bound that fired is named in `bounds` with `truncated: true`
  (a 509-line read had returned 74,623 chars unbounded). The readiness
  ladder's `network, settled: true` for a context banken could read was a
  climb that had not landed: a port that opened after the first deep round
  waited a full `DEEP_INTERVAL` for its climb, and the waypoint read as
  settled meanwhile. The cheap round now publishes `Reach::Climbing` when a
  climber exists, a `Climbing` context is climbed on the next round, it is
  not `settled` until the climb answers, and the cheap floor no longer
  erases a ceiling measured on the same rung (an expired-SSO diagnosis).
  The kubeconfig/client-cert hypothesis was measured false: `climb` already
  builds from the declaring file and reaches `pods` on engenho.
  - `pending-banken: climb-wedge-stalls-rounds` — the round loop awaits every
    climb inline, so one wedged credential helper stalls the cheap rounds and
    every later climb. Not measured live; the fix is a per-climb deadline.
  - `pending-banken: limit-bytes-holds-the-head` — when `limitBytes` fires the
    apiserver has returned the OLDEST part of the tail window. banken says so
    in the bound's meaning rather than recovering the newest lines.
- **DESIGN / next arc (M1→M4):** the watch informer (poll→true watch), the health
  ward's **RENDER** (its `(defward)` vocabulary + the `WardVerdict` evaluator are
  SHIPPED mock-green; no ward panel is drawn yet — do not read the vocabulary as
  a shipped screen), the `TreeView`/XRay, the DECLARE branch+PR flow + the
  resource→`release.yaml` resolver, the mado-MCP e2e gate over a live read. Not
  built yet — do not imply otherwise.
