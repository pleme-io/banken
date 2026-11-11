# banken screens — freshness, the NAME column, the help page, the picker vim

Moved verbatim from CLAUDE.md (2026-11-11).

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

`shift+h` (or `f1`) in the pod table opens a full-screen, scrollable help
page (it was `h` until `h` became `ascend` on 2026-11-04); the status-line
legend carries `shift+h:help` so it is discoverable rather than merely
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
