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
- `pending-banken: pick-on-quit` — `q` in the pod table exits to the shell
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

## Freshness reaches the operator's eye (2026-08-09)

`SyncPhase` was carried all the way to the app and **dropped one step short of
the renderer**: during the initial absorb — one to three seconds on a real
cluster — the status line read `0 pods`, which is indistinguishable from an
empty cluster and is a claim banken had not earned. The type existed precisely
to prevent that (`Absorbing` is "I have not finished looking", not "there is
nothing"). A freshness axis nobody can see is not a freshness axis.

Now: the count carries its claim (`absorbing…` / `83 pods` / `83 pods · STALE —
<cause>`), a `Degraded` replica turns the whole status bar red, and an empty
table says which of the three empties it is in the table's own space. `Synced` is
the unmarked case on purpose — labelling the healthy state trains the eye to skip
the label, which is exactly when the unhealthy one stops being read.

## The NAME column was drawing UIDs — measured live, hidden by three vacuous tests

**69 of 69 rows on `alpha-eks` rendered `10a69bf6-b039-4731-8cce-28ed6e55c534`
under `NAME`.** Cause: the authored column pointed at `egaku::IDENTITY_FIELD`,
and `egaku::TableView::project` short-circuits that field to
`TableRow::identity()` **without ever calling `TableRow::cell`**
(`egaku-0.1.5/src/table.rs:398-404`). banken's `identity()` is the object UID —
correct and load-bearing for `Grip` — so the NAME column drew a uid.

The commit `f2bda73 row: separate display identity from act identity` had made
`Row::cell(IDENTITY_FIELD)` return the name. **That fix was dead code on the
render path**, because nothing calls `cell` for that field. It is the shape of
fix that looks done and is not.

Why three render tests stayed green: every fixture uid was `fixture-{name}` /
`t-{name}`, so `frame.contains("catch-api-7d9f")` passed whether the drawer drew
the name or the uid. **A fixture value that embeds another field cannot
distinguish the two.** Fixture uids are now opaque and name-free.

The fix is `banken_spec::env::DISPLAY_NAME_FIELD` (`"object-name"`) — an ordinary
field, so it projects through `cell`. The authored `:field name` in
`specs/views.lisp` became `:field object-name` with the reason stated in the
file, since a decorative `:field` is the join hazard this repo already fixed
once. Fail-once measured: reverting `pod_columns` to `IDENTITY_FIELD` turns 4
tests red including `the_name_column_draws_the_name_never_the_uid`.

**`pending-banken: egaku-identity-field-projection` — CLOSED UPSTREAM
(2026-08-09), in exactly the shape this row named.** egaku `be62a30` changed
`project` to `row.cell(field).unwrap_or_else(|| row.identity())` and released it
as **0.1.11**; banken now resolves that version.

Measured rather than assumed: reverting `pod_columns` to `IDENTITY_FIELD`
against 0.1.11 leaves all 12 render tests green, including
`the_name_column_draws_the_name_never_the_uid` — so the reserved field now
projects the name correctly and the local workaround is no longer load-bearing.

`DISPLAY_NAME_FIELD` **stays anyway**, and not from inertia: it says on the
authored row that the NAME column shows a *display* name rather than an
identity, which is the distinction the whole bug was about. It is now
defence-in-depth over a fixed upstream instead of a workaround for a broken one,
and retiring it would trade a self-describing field for a silent dependence on
someone else's fallback.

## The help page — the authored vocabulary, rendered back (2026-08-09)

`h` (or `f1`) in the pod table opens a full-screen, scrollable help page; the
status-line legend carries `h:help` so it is discoverable rather than merely
documented. **It is DERIVED, and that is the whole design** — a hand-written
help screen is a second copy of the vocabulary that rots in the one direction
nobody notices: the app grows a chord, the help does not, and the feature is
invisible forever.

`banken_spec::help::HelpPage` is typed data built from the resolved `Catalog`;
`banken::help` is one terminal FACE of it and `--help` is the other, so the two
**render the same value**. `main.rs` used to build its own KEYS block by walking
the catalog a second time — which is exactly how `--help` came to advertise `S`
for a chord the runtime bound as `shift+s`.

Three mechanisms keep it current, at three different tiers — do not collapse them:

| what changes | what happens | tier |
|---|---|---|
| a new `(defk8saction)` / `(defbancada)` / … form | appears on the page with no code change | **truly-unrepresentable** (sections iterate the catalog) |
| a new authored DOMAIN | `error[E0004]` until it has a section — `HelpTopic` is a `closed_catalog!` enum and `HelpPage::section` matches it exhaustively; `help-topic` is also on `REQUIRED_AXES`, so `catalog_covers_every_axis` gates it both ways | **compile-error + CI-caught** |
| a chord the running app cannot dispatch | listed but MARKED `(declared, not wired in this view)`, via a `Wiring` the app supplies | **CI-caught** (`every_authored_chord_is_on_the_help_page`); nothing stops a caller passing an empty `Wiring` |

The load-bearing test is `every_authored_chord_is_on_the_help_page`: the union
of nav + action + bancada chords must each appear in the rendered page. A chord
an operator can press and cannot look up is an undiscoverable feature.

**Help owns the navigation keys while it is up.** `j`/`k` scroll the page and
deliberately do *not* move the table cursor underneath — otherwise closing help
would leave the operator somewhere else. A postigo chord pressed from the page
still ACTS and closes it: a modal that eats every key is one you have to escape
before doing the thing you already decided to do.

**`?` is NOT the help chord, and the reason is measured.** `awase 0.1.6` (what
banken pins) has no `?` in `Key::from_name`, so `(defnavkey :keys "?")` fails to
compile the catalog: `invalid hotkey: unknown key: ?`. And egaku-term's
`to_hotkey` resolves a delivered `Char('?')` through that same function, so even
a version that *parsed* `?` would deliver it as `Key::Slash` with no modifier —
awase models one `Slash` key and cannot distinguish the shifted glyph — making
`?` and `/` literally the same chord and burning the k9s filter key to buy the
k9s help key. `pending-banken: help-question-chord`.

- **`LegalityClass::label_upper()`** replaced five `.label().to_uppercase()`
  sites (legend, action overlay, `--help`, both help sections). Past the
  third-use test, and each one allocated a `String` to say a fixed word.
- **Corrected: `fit_legend` packs GREEDILY, not by priority prefix.** Its doc
  claimed "the most important entries survive"; the loop has never done that —
  it keeps each entry that still fits, so a later, shorter one can appear while
  an earlier, longer one is dropped. Measured at 63 columns:
  `l:OBSERVE  s:DECLARE  shift+s:BREAK-GLASS  g:OBSERVE  h:help …`. The
  behaviour is better than the claim and is kept; the doc now describes it.

## Vim in the context selector — `unsoku` (2026-08-09)

The cluster picker's query was **append-only**: `egaku::FuzzyPicker`'s seven
event arms carry `Type(char)` and `Backspace` and nothing else, so an operator
could type and backspace and could not move a cursor, delete a word, or change
inside quotes. `banken/src/vim.rs` closes that.

**banken owns almost none of it.** What a keystroke *means* is
[`unsoku`](https://crates.io/crates/unsoku) (運足, *footwork*) — motions,
operators, objects, the register, and the eight vim rules it tests. unsoku in
turn declares no vocabulary: `escriba_core::{Motion, Operator, TextObject}`
are published pure enums and `escriba-mode` owns the operator-pending FSM.
banken owns only the **stroke→intent** reading, which is genuinely local: a
picker has TWO cursors on one keyboard, and `j`/`k`/`gg`/`G` address the ROW
LIST while `h`/`l`/`w`/`b` address the text. No editor has an opinion on that.

| stance | keys |
|---|---|
| **INSERT** (the landing) | everything types; `esc` → NORMAL |
| **NORMAL** | `h l 0 ^ $ w b e` · `d c y` + motion/object · `x D C` · `iw aw i" i(` · counts (`3w`, `d2w`) · `p P` · `i a I A` · `j k gg G` (rows) |

**A filter box opens in INSERT**, and `unsoku::Stance::default()` stays Normal.
An editor is not a filter box: the picker's primary act is typing, and an
operator who must press `i` first has been handed a worse chooser than the one
they had. `Vim::for_filter()` is that decision, in one place.

**The stance badge is load-bearing, not decoration.** In INSERT an unrecognised
key costs "a character you can delete"; in NORMAL it costs `q` leaving the
screen or `dw` eating a word. The badge re-buys that property, so it is drawn
on the query line where the eye already is, colour-coded per stance — a badge
read as a word is read too late. The footer is per-stance for the same reason:
it advertised `type: filter` in NORMAL, where typing does not filter.

**Strokes, not chords, and it is forced.** awase's punctuation set is the
unshifted US keycaps only, so `$` and `^` return `None` from `Key::from_name`
and can never be `Hotkey`s. Routing vim through chords would have dropped two
motions from v1 and collided `G` with the authored `shift+g` (pod-break-glass).
Same root cause as `pending-banken: help-question-chord`.

- **Fixed on the way, both caught by the first wire-up.** `esc` recursed —
  `dispatch(Cancel)` → `stroke(Escape)` → `Effect::Cancel` → `dispatch(Cancel)`
  — and overflowed the stack; `apply` now emits the picker event directly and
  never re-enters. And `ci"` did nothing from outside the quotes: vim searches
  FORWARD for a quoted string when the caret is not inside one (`:h v_i"`),
  which for a filter box is the common case, not an edge one. Fixed in unsoku,
  not worked around here.
- **The caret bug is closed.** It was drawn from `query.chars().count()` — the
  query LENGTH — which was invisibly correct only while the caret was always at
  the end. `QueryLine::caret_col()` converts the real byte caret to a character
  column, once.
### What `d` means over a pod — `pending-banken: vim-on-the-pods-table`, ANSWERED

The question looked like a keybinding conflict and is actually the postigo
model doing its job.

**vim's operators do not carry over to the table, and cannot.** `d{motion}`
deletes a range of *text in a buffer you own*, where the act is local and undo
exists. A row is a **projection of remote state you do not own**, and deleting
one is a live, irreversible act on someone else's system. `ClusterEnv` has no
unwitnessed-mutate method — so vim's `d` has **no method to call**. That is not
a limitation to work around; it is the seal, working, at the exact moment
something tried to bring a mutating verb in through a side door.

**Motions carry over completely**, and for the mirror-image reason: they change
*where you are looking*, which is local UI state. That is already what
`(defnavkey)` models, with no legality class — banken's own note says typing
"move the cursor" as `Observe` would make the class stop meaning "this
performed a cluster read".

So the split is: **motions are navigation, operators are acts, and an act needs
a class.** The one vim operator with a clean reading is `y` — but only over the
*rendered* row, where it is a local copy and therefore nav-class, not OBSERVE;
a `y` that fetched the manifest would be a real read and would need the class.

And the vocabulary had already answered: **`d` was authored as `describe`,
OBSERVE, and was the last unwired action.** The legal reading already owned the
key. It is now wired (`Action::ObserveDescribe` → `RowAction::Describe` →
`ClusterEnv::get_resource`), which closes the "(not wired yet)" note the help
page and `--help` were both honestly printing.

- **The describe is the row's own fields, not a `kubectl describe` transcript**,
  and that is deliberate: the transcript has no server API — it is per-kind Go
  presentation logic — so producing it means shelling out to `kubectl`, which
  the NO-SHELL rule keeps out of this path. `pending-banken: describe-fidelity`
  records the gap rather than hiding it.
- `unbound_authored_actions_are_reported_rather_than_silent` would have gone
  **vacuous** the moment the set emptied — an `unbound_action_names` that always
  returned `[]` passes `== []`. It now asserts the emptiness *alongside a
  positive control*: a synthetic catalog carrying an undispatchable action must
  still name it.
- Still open, and now precisely: `pending-banken: vim-motions-on-the-table` —
  the MOTION half (`gg`/`G`/counts over rows) is legal and unbuilt. It needs new
  `(defnavkey)` intents, and `NavIntent`'s projection is exhaustive in both
  screens, so it is a vocabulary change rather than a keymap one.

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

**bancada** (pt-BR: a workbench — tools already laid out, work starts the
instant you sit down). Renamed from `guarita` 2026-07-31: that word was
already double-claimed (`theory/NAMING.md:88`), and on Law 2 it belongs to
the per-request credential check in a host organization's VPN design, whose
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

**And the witness is structural at the seam.** `SessionEnv` has exactly **one**
staging arm — `stage_witnessed(MutatingCommand, &WitnessedAction)`. A read pane
is not staged at all: it is *born* as its command through
`PaneProgram::Observe(&ObservedCommand)`. Both newtypes have private fields and
come only from `PlannedPane::as_observed` / `as_mutating`, which are `Some` on
their own `CommandEffect`, so a mutating command has **no argument value that
can reach either an unwitnessed stage or a spawn**. The second half matters as
much as the first: a spawned program runs immediately, which is exactly the
operator's-Enter step the witnessed arm exists to preserve. A *second* staging
arm being added is CI-caught by `substrate_invariant.rs` (same
only-mitigated→CI-caught tier as the `ClusterEnv` re-add case). Do not round
either up.

Tightened 2026-07-31: the seam used to carry an unwitnessed `stage_observed`
and lean on its argument type. That arm is gone.

**`(:context cluster)` is what makes "the RIGHT cluster" true.** A pane opened
without an explicit `--context` lands on whatever the kubeconfig's current
context happens to be, which is not necessarily what banken is reading. An
unknown cluster is a typed **refusal** (`UnresolvedContextField`), never an
emitted `--context ""`. Same for `namespace` / `container`.

**And the same hazard existed ONE LAYER UP, unguarded, until 2026-07-31.**
`banken --live` rode `Client::try_default()` — i.e. the kubeconfig's
`current-context` — so banken could render a real pod table from an entirely
different estate than the operator meant, and nothing would look wrong: the
read succeeds, the rows are real, only the cluster is wrong. Measured on workstation
that day: current-context was `beta-eks` (a different estate) while the
cluster under inspection was `alpha-eks`. `--context <name>` is now
**REQUIRED** for `--live` (`banken::cli`, `Invocation::Live` carries a
non-optional `String`, so an unnamed live run has no representation past the
parser — **parse-time-rejected**, not truly-unrepresentable: the library's
`KubeClusterEnv::connect()` still exists as the honest in-cluster
constructor). The refusal prints the current-context it would have used and
every available context name, so it costs one keystroke, not a detour.
`connect_with_context` carries the connected name straight into
`context_name`, so the string that selected the apiserver and the string a
`(defbancada)` resolves against **cannot drift**.

**And it happened a THIRD time, one layer over again — the fix above was
necessary and NOT sufficient (corrected 2026-08-08).** `--context <name>` was
presented here as what makes "the RIGHT cluster" true. It is not, on its own:
**`KUBECONFIG` is a `:`-separated MERGE LIST and a context name is not unique
across it.** kube-rs resolves a duplicate **first-wins and says nothing** —
`append_new_named` (`kube-client-0.99.0/src/config/file_config.rs`) filters out
every later entry whose name already exists, with no error and no warning, so
by the time a caller holds the merged `Kubeconfig` the evidence that the name
was ambiguous is **gone**. The flag narrowed the hazard from "whatever
current-context happens to be" to "whichever file sorted first" — the same
false calm, one layer over.

Measured that day: the operator kubeconfig declares `charlie-local` → a LOCAL
apiserver, while `~/.kube/config` declares `charlie-local` →
`charlie-local.example.invalid`, a **remote** cluster. banken read the remote one
and rendered a perfectly healthy table doing it. Four protocol-layer
hypotheses (Ed25519 client certs, ALPN/h2, IPv6, TLS handshake) were chased
and all were wrong; `lsof` showing `SYN_SENT` to a public IP is what exposed
it. **The lesson generalizes past this bug: when a cluster-facing result looks
odd, check the SOCKET before theorising about protocol layers.**

`live::resolve_context` now reads each file in the merge list **separately** —
the only point at which the question is still answerable — and refuses unless
exactly one declares the name (`ContextError::Ambiguous`, naming every
declaring file and which would have silently won). `connect_with_context`
resolves *before* it connects, so an ambiguous name has no code path past that
line. Tier: **parse-time-rejected** — the name can still be written, it just
cannot open a client. `KubeClusterEnv::server()` carries the resolved
apiserver URL, because a name is a label to trust and the URL is what actually
got dialled. Fail-once measured: reverting the `Ambiguous` arm to first-wins
turns `a_context_declared_by_two_files_is_refused` red, showing it silently
choosing one of two real clusters.

**`pending-banken: context-provenance-in-the-status-line` — CLOSED
(2026-08-08), and closing it exposed a blind test.** The status line now
renders the apiserver URL beside the name:
`source: LIVE engenho-probe (https://127.0.0.1:6443) [streaming]`.

The first attempt *looked* done and was not. The label was built inline in
`main.rs` **and again** in `tests/live_read.rs` — so the test asserted against
a label it had constructed itself, and adding the URL to `main.rs` changed
nothing the test could see. It stayed green while proving nothing about what
the binary renders. `absorb::live_source_label` is now the single
constructor both callers use, so the test finally pins the binary's output
(Prime Directive: duplication is a bug — here it was also a *vacuous gate*).

## naturalize(kubectl) — DECLINED, and why (2026-08-08)

An operator ask to `/naturalize kubectl` was taken through recon + design +
an adversarial pass. **The verdict is to decline it as a naturalize**, on the
skill's own bright line rather than on effort:

1. **It would be a WRAP wearing a citizen's name.** Every illegal state the
   design closed was closed by handing **kube-rs** a better-constructed
   `Kubeconfig` — kube-rs's types, kube-rs's resolution, kube-rs's client. The
   naturalize skill's check 1 asks "would vendoring X's implementation satisfy
   a consumer?" Here it plainly would: ~90% of kubectl's verb surface already
   ships in `kube-client` 0.99 (logs, exec, attach, port-forward, evict, scale,
   ephemeral containers, delete-collection, `await_condition`), and the rest is
   one aggregated-discovery GET away.
2. **The fleet already holds the honest posture, and it is NOT a naturalize.**
   The skill's own waiver names it: *"X is genuinely irreplaceable upstream (a
   standard we must speak on the wire) → speak the wire, own the executor —
   that is magma's posture, not a naturalize."* The Kubernetes API **is** that
   wire standard, and `engenho-kube-client/src/config.rs` already implements
   the executor half — a hand-written `Kubeconfig`/`ExecConfig` parser with
   exec-credential support and `resolve_connection`, behind engenho's
   `KubeClient` trait.
3. **Two capabilities are genuinely irreducible**, and neither is ours to
   dissolve: `kubectl describe` has **no server API** — it is per-kind Go
   presentation logic — and `-o go-template` is Go's `text/template`, which is
   implementation-defined rather than specified.

**What IS worth doing, and is the real ask underneath:** close the leak. Five
`:program "kubectl"` forms ship in `banken-spec/specs/bancadas.lisp`, and
`bancada.rs` enshrines `"kubectl"` in the typed border's own doc comment —
while `substrate_invariant.rs:49` already lists `kubectl` among
`FORBIDDEN_SUBSTRINGS` for method names. The repo treats the string as
contraband in one place and authors it in another.

`pending-banken: bancada-kubectl-argv` — a read pane that runs `kubectl` is a
foreign-idiom leak in the one surface an operator watches. The destination is a
pleme-io binary in the pane speaking the API directly (Pillar 8's "no shell
ships either", one layer over). **NOT closable for the mutating pane**, which
needs a real TTY on exec — that stays `kubectl` until an owned executor ships
exec, and saying so is the honest tier.

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

- **`pending-banken: tear-argv-spawn` — CLOSED for READS (2026-07-31).** tear
  `5974375` threaded `args: &[String]` through
  `MultiplexerControl::new_session_with_source_and_size` / `split_pane` /
  `new_window` to the `PtyHandle::spawn` that had accepted one all along. So
  **pane creation and pane program are now one act**: `SessionEnv::open_session`
  / `split` take a `PaneProgram`, and a read pane is *born* as its own argv,
  reaching `execvp` as a vector with no shell in between.
  - **The refusal-to-quote no longer applies to a read pane** — not relaxed,
    *bypassed*: there is no shell in the path to quote for. Measured:
    `-o=jsonpath={.status.phase}` is refused by `stageable` (positive control in
    the same test) and reaches a spawned read pane byte-identical.
  - **The seam got STRICTER, not just rearranged.** `stage_observed` is gone
    from `SessionEnv` outright, so there is exactly **one** staging method and
    it demands a witness. The `substrate_invariant.rs` allowlist and its
    staging-arm count moved with it (`== [stage_witnessed]`, not "exactly two").
- **`pending-banken: tear-argv-witnessed-arm`** — the half that **cannot**
  convert, and this is a property rather than a limitation waiting on upstream.
  A spawned program **runs immediately**; `stage_witnessed` deliberately types
  its argv *without* a newline so the operator's own Enter is the final act. A
  mutating pane therefore stays a `PaneProgram::Shell`, still types, and still
  refuses to quote. Closing it needs a tear surface that can place text on a
  pane's input line *without executing it as that pane's program* — the row
  records the shape, not a promise.
  - That a mutating command cannot be spawned is **compile-enforced**, not
    reviewed: `PaneProgram::Observe` takes `&ObservedCommand`, which only an
    observing `PlannedPane` produces. Fail-once measured by trying to write the
    dangerous line — `PaneProgram::Observe(&pane.as_mutating()…)`:
    `error[E0308]: mismatched types … expected `&ObservedCommand`, found
    `&MutatingCommand``. Tier: truly-unrepresentable *within this authored
    surface*, the same qualified tier as the other four seals.
- **`pending-banken: tear-argv-spawn-live`** — the conversion is **mock-proven
  and unit-proven, NOT re-run against a daemon.** `tests/tear_handoff.rs` was
  rewritten to assert the daemon's own `TearPane.shell` + `.args` record
  (durable, and the exact vector handed to `execvp`) instead of polling the
  rendered grid for an echo — which the spawn model no longer produces. It is
  still `#[ignore]`d and needs a `tear-daemon` built from `5974375` or later.
  **tear ships no protocol version and does not negotiate**, so an older daemon
  accepts the new frame and silently spawns a bare `kubectl`; the
  `!root.args.is_empty()` assertion is exactly the gate on that.
- **A read pane now EXITS when its command does** — the direct consequence of
  spawning, stated rather than discovered. `kubectl describe` finishes and its
  pane goes `PaneState::Exited` where it previously returned a shell prompt.
  tear keeps a watched session's exited panes and their final grid
  (remain-on-exit), so the output stays readable; the operator just cannot type
  in that pane. Both shipped recipes hold a long-running pane (`logs --follow`,
  `get events --watch`), so neither session can fully exit and be reaped.
- **`pending-banken: bancada-tear-feature-nix-unverified` — CLOSED
  (2026-08-07), and the fix was to remove the CAUSE rather than verify the
  symptom.** The row existed because the `tear` feature's dep took banken's
  `Cargo.lock` from zero git sources to four (tear, shikumi, gen, plus a
  second `ishou-tokens` 0.1.4 from git alongside banken-config's registry
  one), which is the crate2nix-vs-`fetchgit` base32-vs-SRI drift class, and
  `nix build` had never been run against it. Both deps were **published on
  crates.io the whole time** (`tear-client` / `tear-types` 0.1.12, checked
  against the sparse index) — the git pin bought nothing and cost the drift
  class plus a `branch = "main"` float that pins nothing. Repointed at the
  registry: the lock now carries **zero** git sources, transitive ones
  included. `nix build .#banken` is green with `default = ["live", "tear"]`
  — it compiled `kube 0.99` and `tear-client 0.1.12` through the substrate
  path — and the resulting store artifact answers `--live` by reading the
  kubeconfig rather than by refusing. `hashfix` was never needed.
  **Generalize: a git dep on a sibling pleme-io crate is a defect, not a
  pin.** It makes the depending crate structurally unreleasable and drags the
  drift class in behind it; the sibling is almost always already published.
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
    is gated on autorevivy's detection stream deploying beyond alpha-shadow
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
    about kube-rs.** BANKEN.md §IX called the informer "unbuilt substrate banken
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
    re-rolled — QUADRO §T14). An enum of `Watched|Polled` in the app would have
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
      zero, and no local type closes it (BANKEN.md C1).
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
    `pending-banken: manual-refresh-mode` — an authored `0` ("never
    auto-refresh") is honoured as `DEFAULT_POLL` because the feed has no
    manual-only mode.
  - `pending-banken: absorb-multi-gvk` — one kind (pods), one watch. Discovery,
    admission bounds and per-kind fidelity are M3/M4.
  - **`pending-banken: absorb-display-fidelity` — OPEN, and it now carries a
    DESIGN CONFLICT this repo created on 2026-08-08.** Measured: `Accept:
    …as=Table` with `includeObject=None` is **170 B/pod vs 18,747** (110×) and
    its cells carry a server-rendered `Age` that would close the `AGE renders -`
    gap for free — but it carries **no uid** (`row.object` is `null`), and uid
    is now REQUIRED on `Row`. `includeObject=Metadata` carries uid at 5,830
    B/pod (3.2×) and still ships `managedFields`. So the three fidelities are
    not a ladder: **Display renders, Metadata identifies, Full describes**, and
    the table's cursor identity (unique at an instant) is a different question
    from the act's identity (unique across recreate). Resolve that before
    building, not during.
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
