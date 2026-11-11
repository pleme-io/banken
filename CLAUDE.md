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

<!-- IN-FLIGHT DEBT: none. Re-measured 2026-08-07 after `default` became
     ["live", "tear"]: 199 tests green by default, `cargo fmt --check`
     clean on the files this touched, `--no-default-features` still
     compiles (both #[cfg] arms live). Cargo.lock now carries ZERO git
     sources — tear-client/tear-types moved to crates.io 0.1.12, which
     also removed the transitive git shikumi / gen / second ishou-tokens
     the old `branch = "main"` pin dragged in. The prior note's "tear
     1c1007d is published and pinned" was two claims at once and only
     the first was true: it was published, and pinned by GIT REV anyway.
     Kept out of the body deliberately: a cleared waiver is deleted,
     not annotated. -->

## Repo law (operator, 2026-11-05)

1. **No subprocess, ever.** banken speaks every backend on the wire: the
   Kubernetes API, the cloud APIs, and the auth flows (AAD device code, AWS
   SSO-OIDC, GCP OAuth) natively. It never runs kubectl, az, aws, gcloud or
   kubelogin, and never goes through a shell. A kubeconfig exec plugin banken
   does not resolve natively is a typed refusal on that entry alone. The gate is
   `banken/tests/no_subprocess.rs`
   (`banken_spawns_no_process_outside_the_external_editor_seam`).
2. **The editor is embedded.** `e` opens an in-process, blue-powered escriba
   pane (`config(:edit, {editor: :embedded})`, the default). `$VISUAL` /
   `$EDITOR` is the opt-in `:external`, spawned as one direct argv call in
   `banken/src/edit.rs`: the only spawn the gate allows.
3. **Blue first, banken composes.** Every capability another tool could use is a
   bidama in `pleme-io/blue/bidamas`: the provider contract, each cloud
   provider, the doctor (`shinsatsu`), the edit → diff → apply loop, the tree
   and path model, the binding table. banken only composes them.
4. **One experience everywhere.** The tree, `i`, `e`, `dd` and the doctor work
   the same over Kubernetes and every cloud (`docs/PROVIDERS.md`).

Where the code does not meet the law yet is a `pending-banken:` row below
or in the docs linked below, never a silent gap.

One-sentence purpose: an **observe-first, GitOps-native cluster-navigator TUI** —
keep k9s's fast keyboard navigation + health surfaces, structurally refuse its
imperative-mutation console.

## The one primitive banken owns — `postigo`

Every cluster action is typed into a three-class **action-legality** gate:
`OBSERVE` (direct read) / `DECLARE` (lowers to a full-manifest GitOps change a
reconciler applies) / `BREAK-GLASS` (witnessed, RUNBOOK-logged). The `ClusterEnv`
interpreter trait has **no unwitnessed-mutate method** — an unwitnessed live
mutation is `E0599` within the authored surface (the re-add case is CI-caught,
per `substrate_invariant.rs`, graded only-mitigated → CI-caught, never rounded up
to truly-unrepresentable).

## The landing — banken opens on a CHOOSER, not on data (2026-08-09)

Bare `banken` opens the **cluster picker** (`banken/src/picker.rs`): every
kubeconfig context, each beside the apiserver URL it resolves to, fuzzy-filtered
by name **or** by estate, `enter` to open. It used to land on the fixture — five
invented pods, labelled `source: fixture` in the status line and otherwise
indistinguishable from a cluster — and the label does not fix that, because the
eye goes to the rows. The fixture is retired from the default, not removed:
`banken --fixture` (★★ MODULARIZE, DON'T DELETE).

`banken --live` with no context used to print eighteen context names and exit 2.
banken knew what the operator wanted, held the exact list, and made them retype
one entry of it; it now opens the picker instead.

**The wrong-estate invariant is not weakened — it is enforced EARLIER and more
strongly.** `Invocation::Live` still carries a non-optional `String`, so an
unnamed live run has no representation past the parser; picking produces that
same value. And picking beats typing on two counts the CLI never could:

1. the row shows the **apiserver URL** before it is dialled — a name is a label
   to trust, a URL is what actually gets connected to;
2. an **ambiguous** name (declared by >1 file in the `KUBECONFIG` merge list —
   the 2026-08-08 `charlie-local` hazard) is marked `⚠ declared by N kubeconfig
   files` and **cannot be chosen**. Typing it was accepted at the CLI and
   refused later by `ContextError::Ambiguous`, i.e. after the operator had
   already committed. Measured with two synthetic kubeconfigs declaring `twin`.

`--context <name>` no longer needs `--live` in front of it: with no fixture
default left for a name to be misattached to, naming a cluster IS asking to read
it. `--fixture` alongside either is `ConflictingSources` — refused, never ranked,
because "are these rows real" is the one question the argv must answer.

**The picker's keys are a DERIVATION of the one authored vocabulary, filtered to
the chords that cannot be typed.** A picker's keyboard belongs to its query, so
`up`/`down`/`return`/`escape` bind and `j`/`k`/`o`/`q` deliberately do not —
otherwise filtering for `jaeger` moves the cursor and quits. `is_control_chord`
mirrors egaku-term's own `text_char` rule and is conservative in the direction of
*not* binding. `backspace` is structural and deliberately not a `(defnavkey)`: it
edits a text query, not a selection. The state is `egaku::FuzzyPicker` — banken
adds no widget (QUADRO T1); only the drawer is local, for the same per-row
semantics reason `render.rs` keeps its own.

Two `run_async` calls in sequence, not one app with a stage flag: `BankenApp` is
generic over its `ClusterEnv`, so the env must exist before the app does. One app
spanning both screens would hold an `Option<E>` and answer "which cluster am I
reading" with `None` for the whole first screen.

**`--no-default-features` did not compile, and had not for some time.** The
`#[cfg(not(feature = "live"))] run_live` stub took two parameters against a
three-argument call — added when `--list-strategy` landed and never built, because
`default = ["live", "tear"]` means no ordinary invocation exercises that arm. A
`#[cfg]`-gated arm nothing builds is a claim nothing checks. Verified broken by
stashing this work (`E0061` at `main.rs:65`); now green, 68 tests.

- **Fail-once measured:** the footer read its chords out of `KeyMode::bindings`,
  a `HashMap`, so with `escape` and `ctrl+c` both bound to Cancel it advertised
  a different chord run to run (`ctrl+c: quit` on the first run of
  `the_footer_advertises_the_bound_chords`). Fixed by ordering authored chords
  before structural ones — a *meaning*, not a tie-break. A length tie-break was
  tried first and silently advertised the escape hatch, since `escape` and
  `ctrl+c` are both six characters.
- **A refusal wraps rather than clips.** Measured in a 120-column PTY: the
  one-line footer cut the ambiguity message mid-path, dropping the second
  declaring file — the half that says which one to remove.
- `pending-banken: pick-on-quit` — CLOSED 2026-11-04 by the `ascend` intent (see
  the tree section). The history below is kept. `q` in the pod table exits to the shell
  rather than back to the picker. Deliberately NOT done with the reconnect loop
  above, and the reason is the legend: `q` is advertised as `q:quit` by a
  status line derived from the authored vocabulary, so re-pointing `q` at
  "back" would make the one surface that tells an operator what a key does
  start lying. Closing it properly means a new authored nav intent (a `back`
  chord) so the legend moves with the behaviour — a vocabulary change, not a
  main-loop change.
- **`pending-banken: reconnect-on-failed-pick` — CLOSED (2026-08-09).** A
  connect that fails after picking used to print the error and exit, throwing
  away the list the operator was choosing from — the retry started from a cold
  prompt. `run_pick` now loops: the failure re-enters the picker carrying the
  reason as a `notice`, so the other seventeen contexts are still on screen and
  the retry costs one keystroke. The notice renders through the refusal path
  because it *is* one; only the screen that produced it differs, and the first
  keystroke clears it for the same reason a refusal is cleared. The loop
  advances only on a successful connect or on the operator declining, so it
  terminates.

## The estate, the credential doctor and the tree (2026-11-04)

**The estate.** `banken/src/estate.rs` holds the pure rules. `live::read_estate`
reads two sources: the target registry (`~/.kube/targets/<target>`, one
kubeconfig per target, the same files the `k-<target>` and `k9s-<target>`
wrappers use) and the `KUBECONFIG` merge list. The picker and `resolve_context`
both read `Estate`, so the list and the verdict cannot disagree.

- A context whose first declaration has the credential helper `nerai-refuse`
  is a refusal stub. Its reason is the helper's argument.
  `ContextError::Refused` carries it, and names the registry target when one
  covers the name.
- A stub that shadows a registry target does not count toward "declared by N
  files". Only real declarations count. A stub after a real declaration
  refuses nothing, because the merge never reaches it.
- An ambient context with the same name and server as a registry target is
  hidden as a duplicate. The same name with a different server stays visible.
- A target file that is not a single-context kubeconfig is refused alone.
- Refused rows are hidden by default and counted in the title.
  `config(:picker, {refused: :dim})` shows them dimmed with the reason.

**The wrong-estate invariant holds and is stronger.** A registry target opens
with its own file only (`config_for_resolved(resolved.context, resolved.file)`).
An ambiguous name still cannot be chosen or connected. A refused name now has
no path past `resolve`, where before it read as an ambiguity.
A bancada pane passes the connection's own file (`--kubeconfig`); see the
2026-11-05 section.

**The credential doctor.** `shinsatsu` is a bidama in `pleme-io/blue/bidamas`
(moved there 2026-11-05; banken loads it through `blue_lang_host::Standard`). It
declares providers (Azure, AWS, Google Cloud, nerai), their failure classes,
and for each class a title, a diagnosis, remediation steps (typed argv) and a
verify step (the kubeconfig's own exec helper). `banken/src/doctor.rs` embeds
it through `blue_lang_host::Embedded` and speaks JSON with it. The climb note,
the picker notice and `--context` errors all show `Diagnosis::headline`.
`banken/src/doctor_screen.rs` shows every command before it runs; `enter`
runs them as child processes inside the panel, shows a device flow's URL and
code (`shinsatsu::prompt_of`), the output and the wait, then the verify step;
a pass reconnects. `esc` stops a running command.
`every_failure_class_is_reachable_by_its_own_marker` and
`every_failure_class_has_a_diagnosis_and_a_remediation` fail when a class has
no test row, no marker or no remediation. The doctor speaks JSON through
blue's `json_read` / `json_write`; `json_write` (added to blue 2026-11-05)
keeps a list of pairs an array, which `json_stringify` does not.

**The tree.** The screens are a path: cluster › namespace › kind › object ›
detail. `NavIntent` gained `ascend`, `descend`, `top`, `bottom`, `page-down`,
`page-up`, `back`, `forward` and `filter`; `confirm` and `dismiss` became
`descend` and `ascend`. `ascend` closes a detail, then clears the filter, then
drops the namespace scope, then sets `BankenApp::ascended`, and `main` returns
to the picker (`Landed::Ascended`). It never quits; `q`, `:q` and `ctrl+c`
quit. `descend` confirms a bancada preview, scopes to a namespace from `:ns`,
or opens the object's describe detail. `:` takes paths (`:ns/kube-system`,
`:pods/ns/web`, `:ns/*`, `:..`, `:q`). `ctrl+o` and `tab` walk the history.
`top` is a sequence: two presses. Logs moved to `shift+l`, help to `shift+h`
and `f1`, and the bancadas to `t` / `shift+t`.
`pending-banken: pick-on-quit` is CLOSED by `ascend`: `q` still quits, and the
legend still says so.

**Configuration is blue.** `banken/src/world.rs` reads `~/.config/banken/banken.b`
(or `$BANKEN_WORLD`) in a `banken blue-world` child, the mado shape. Entries:
`bind(chord, :intent)`, `bind(chord, :off)`, `config(:picker, {refused:,
targets:})`. `Catalog::with_nav_overlay` applies each binding alone, so a bad
chord, an unknown intent or a clash with a postigo chord is refused alone and
shown in the picker notice. The help page and the legend read the overlaid
catalog, so they cannot drift from the keymap. `motenashi::off` refuses an
entry the world did not declare, which is why a default is turned off with
`bind(chord, :off)`.

## Verbs, the edit loop and recipes on targets (2026-11-05)

- `i` inspect (OBSERVE) calls `ClusterEnv::inspect`: facts, labels, the owner
  chain (walked with `kube::discovery::pinned_kind`), conditions and events.
- `dd` delete and `e` edit are BREAK-GLASS `(defk8saction)`s. `break_glass`
  carries a `GlassAct` (`Record`, `Delete`, `Apply`); the ledger records first,
  then the act runs. `ClusterEnv::posture` answers `ReadOnly` for a registry
  target its refusal file shadows (the module only shadows live targets that
  are read-only), and `ReadOnly` refuses both verbs before any prompt and again
  inside `break_glass`. The trait's default posture is `ReadOnly`.
- `banken/src/edit.rs` is the edit loop: `manifest` (managedFields removed),
  `$VISUAL`/`$EDITOR`, `preview_change` (server-side apply, dry run), a unified
  diff, a confirm, then `apply_with(... GlassAct::Apply)`. An error reopens the
  editor with `# banken:` lines on top and the edit kept.
- The `Arc<T>` forward of `ClusterEnv` must forward every method. A default
  method it does not forward silently answers the default for the shared env;
  the verb tests caught exactly that.
- Recipes: `(:optional (:flag "--kubeconfig" :field kubeconfig))` emits both
  words when the connection read one file (a registry target, or the file
  that declares an ambient context) and nothing otherwise.
  `pending-banken: bancada-target-kubeconfig` is CLOSED.
- Other clouds: `docs/PROVIDERS.md` holds the destination and the provider
  contract. Only Kubernetes is built.
- Native auth: kubelogin is resolved in-process (`banken/src/aad.rs`: AAD v2
  device code, a token held per server id, set as the bearer on the kube
  `Config`), so kubelogin never runs. The doctor panel sends the two requests
  itself and shows every field first. The blue world reads in process (no
  `banken blue-world` child). Removed spawn sites: the doctor's `az login` /
  `aws sso login` / `gcloud auth login` runner, its credential-helper check, and
  the blue-world self-spawn.
- `pending-banken: native-aws-gcp-auth`: kube-rs still runs `aws eks get-token`
  and `gke-gcloud-auth-plugin` as exec plugins, inside kube-rs, which the source
  gate cannot see. The destination is SSO-OIDC + GetRoleCredentials + SigV4
  presign and the GCP ADC refresh, as blue bidamas over a host HTTPS word.
- `pending-banken: msal-cache`: an AAD token lives only for the process; the
  MSAL cache is not read or written.
- `pending-banken: embedded-escriba`: the default editor is the escriba pane,
  not built yet, so `e` refuses with the opt-in named.
- `pending-banken: bancada-kubectl`: session recipes stage `kubectl` in tear
  panes. tear spawns them, not banken, and the law still says no.
- `pending-banken: blue-composition`: the tree/path model, the edit loop, the
  binding table and the provider contract are Rust or tatara-lisp here, not
  bidamas yet.

## Screens: freshness, the NAME column, the help page, the picker vim

Before changing the status line, the NAME column, the help page, `fit_legend`,
the picker's vim (`unsoku`) or what `d` means over a row, read
[docs/SCREENS.md](docs/SCREENS.md). It holds the receipts and the open rows
`help-question-chord`, `describe-fidelity` and `vim-motions-on-the-table`.

## The two app seams — `BankenApp<E: ClusterEnv, S: SessionEnv>`

The app is generic over **two** mockable traits, for the same reason each time:
the runtime is byte-identical against a mock, a live backend, or a build with
no adapter compiled in. Rows come from `E`; a confirmed `(defbancada)` opens
through `S`.

Both live adapters are in the **default** build as of 0.1.2 — `default =
["live", "tear"]` — so the `absent` column below describes
`--no-default-features`, not what an operator gets. That matters beyond taste:
substrate's rust tool-release shape reads its cargo features from gen's build
spec (cargo's *default* resolve) and exposes no per-consumer knob, so this
crate's `default` IS `pkgs.banken`, and a lean default meant every fleet node
shipped a navigator that could not navigate.

| seam | mock | live | absent |
|---|---|---|---|
| `ClusterEnv` | `MockClusterEnv` / `FixtureClusterEnv` | `KubeClusterEnv` (feat `live`, DEFAULT) | `--live` is a typed CLI error |
| `SessionEnv` | `MockSessionEnv` | `LazyTearSessionEnv` (feat `tear`, DEFAULT) | `UnwiredSessionEnv` — a typed refusal |

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

A selected row plus one chord resolves a recipe into a pre-warmed tear session;
`enter` opens it. Legality derives from the panes (no `:legality` kwarg), and
`SessionEnv` has one staging arm, `stage_witnessed`. Before changing a recipe,
the session seam, `--context` resolution or anything that runs `kubectl` in a
pane, read [docs/BANCADA.md](docs/BANCADA.md): the tier ledger, the live tear
runs, the wrong-estate history, naturalize(kubectl) declined, and the open
`bancada-*` / `tear-argv-*` rows.

### Four construction seals (each truly-unrepresentable *within its authored surface* — none fleet-wide)

1. **`WardVerdict`** — fields private, `evaluate` the only constructor, so a
   ward verdict claiming `Green` over an absent core metric is
   unconstructible. The BROKEN-METRIC guard is a **structural
   cap** consulting no catalog row, so omitting / filtering / mis-authoring the
   `broken-scrape` rule cannot switch it off. A guard that depends on a catalog
   row is a guard with an off switch. Fail-once measured: deleting the cap line
   turns `a_dead_scrape_can_never_report_green` red with `a blind reading must
   be Unknown, never Green — with catalog []`.
2. **`Attestation::Proven`** — `OutcomeChainRef` has no `Deserialize` that
   succeeds, so `:attested (:kind proven …)` is **parse-time rejected** with an
   error naming C-controller. The "labelled (computed), NEVER
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

banken is a **Quadro** (terminal UI) — the design governs the render; it composes
egaku + egaku-term + moldura (host: mado), shikumi config, ishou theme, awase keys.
It is a **naturalize** worked example — a **half-citizen by design** (naturalizes
k9s's observe half, not its mutate half). Per its own 4-check citizenship test it
is currently a **resident** (native ✓ / no-leak ✓ / proven-on-fixtures ✓ /
proven-live ✗ until a cluster is reachable / known ✓).

## Build status (tier-honest — never round up)

Before claiming a tier, closing a `pending-banken:` row or touching
`banken-config`, the theme, the awase keymap, the live read, the watch plane,
`Grip`, `ListStrategy` or the Armazém, read
[docs/BUILD-STATUS.md](docs/BUILD-STATUS.md). It is the per-component tier
ledger with every receipt and open row.

## Standing rule

Every PR advances a ledger row's tier or leaves a typed
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
