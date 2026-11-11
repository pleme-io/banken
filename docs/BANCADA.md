# bancada — the banken → tear/mado bridge, and naturalize(kubectl)

Moved verbatim from CLAUDE.md (2026-11-11).

## bancada — the banken → tear/mado bridge (`(defbancada)`)

**bancada** (pt-BR: a workbench — tools already laid out, work starts the
instant you sit down). Renamed from `guarita` 2026-07-31: that word was
already double-claimed, and on Law 2 it belongs to
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
