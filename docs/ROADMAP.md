# banken — the destination, and the delta to it

> Written destination-first. The phases exist to serve the destination; if a
> phase stops serving it, the phase is wrong, not the destination.

## I. What "best of its kind" means here

**Not "k9s with more views."** k9s is excellent, mature, and already won that
shape. Rebuilding it feature-for-feature would produce a worse k9s and teach the
substrate nothing.

banken's claim is a different one, and it is already half-built:

> **A cluster navigator where every action is typed by its legality — so a live
> mutation is a compile error rather than a policy, and the same interface can
> be driven safely by a human *or* an agent.**

Three properties make that a category rather than a feature list:

1. **The gate is structural.** `ClusterEnv` has no unwitnessed-mutate method.
   Not "banken asks for confirmation" — there is no code path. Every other tool
   in this space is one keystroke from `delete`.
2. **DECLARE is real GitOps.** Changing a workload produces a branch and a full
   manifest, reviewed and reconciled. The cluster is never the thing you edit.
3. **It is agent-addressable.** An agent driving banken inherits exactly the
   human's powers and no more, because both go through the same three classes.
   Nothing in this space offers that today.

Everything below is in service of those three. Where a phase only buys parity,
it says so.

## II. The honest baseline (measured 2026-08-12)

| Surface | State |
|---|---|
| `ClusterEnv` seam | **1 of 10 methods wired live** — `list_resources`, pods only |
| `get_resource`, `logs`, `events`, `topology`, `health_signals`, `watch`, `grip`, `declare`, `break_glass` | typed refusals (never a fake `Ok` — good, and still absent) |
| Authored views | **3** (`pods`, `svc`, `ward`); only `pods` renders |
| Live read | pods via watch (delta traffic, not poll) — **shipped, proven** |
| Legality gate | postigo three-class dispatch — **shipped**, over a mostly-unwired env |
| Landing screen | context picker, vim-modal, ronda access ladder, antessala — **shipped** |
| Session handoff | `(defbancada)` → tear/mado — mock-proven; live open unverified |
| Config | one field (`refresh-interval-ms`) |
| Visual tokens | hand-picked colours; **not** on the fleet `ishou` spine |

The gap between "1 of 10 methods" and the destination is the whole of this
document. The gate is built and has almost nothing to gate.

## III. The delta, in leverage order

### Phase A — the resource plane goes generic *(unlocks everything else)*

Today every layer is pod-shaped: `spawn_pod_absorber`, `PodTable`,
`draw_pod_table`, `ResourceKind::Pod`. That is the single biggest constraint in
the codebase — ~20 views are blocked behind one type parameter.

- Generalise the absorber to `absorb<K: ResourceKind>`, keeping the watch
  producer and the `Despensa` reader unchanged.
- Table columns become a property of the *kind*, authored as `(defview …)` —
  the vocabulary already has the form, with 3 entries and 1 renderer.
- Wire `watch` and `list_resources` for the core kinds: deployments,
  statefulsets, daemonsets, jobs, cronjobs, services, ingresses, configmaps,
  secrets (names only — see the crivo note in VI), nodes, namespaces, events,
  PVCs.
- **Done-predicate:** adding a resource kind is a `(defview …)` entry plus a
  column list. If it needs Rust, phase A is not finished.

*Buys: parity. Costs: the least glamorous work here, and everything waits on it.*

### Phase B — vim everywhere *(the operator's standing ask)*

The picker is modal; the table is not. The vim layer (`unsoku` + `crate::vim`)
already exists and is proven on one surface — the second surface is mostly
wiring, and doing it now while the shape is fresh is far cheaper than later.

- Normal/Insert on the table: `j`/`k`/`gg`/`G`/`ctrl-d`/`ctrl-u`, counts,
  `/` to filter with the same one-line modal editor the picker uses.
- `:` command bar — `:pods`, `:svc`, `:deploy`, `:ns <name>`, `:ctx` — with the
  same erase chords, resolved against the authored view catalog so an unknown
  `:verb` is a typed refusal naming the legal set.
- Promote the modal query line into **egaku** once it has two consumers. Per
  QUADRO T1 widgets live in egaku, never in an app; two consumers is the
  threshold that earns the promotion.
- **Done-predicate:** a keystroke means the same thing on every screen, and
  the stance badge is drawn from one type.

### Phase C — the read surface

`get_resource`, `logs`, `events`, `topology`, `health_signals` are all declared
and all refuse. These are pure OBSERVE, so they need no new legality thinking —
only implementation.

- YAML / describe pane, `d` and `y`, over `get_resource`.
- Log tailing: follow, multi-container, search, wrap — the one thing operators
  spend the most time in.
- Events on a resource, and the cluster-wide event stream.
- `topology` → the dependency tree (k9s's xray), which the `DepTree` type
  already anticipates.
- **Done-predicate:** every `ClusterEnv` OBSERVE method is live, or documented
  as deliberately absent with a reason.

### Phase D — DECLARE becomes real *(differentiator #1)* — **delivery SHIPPED 2026-08-12**

`crate::declare` is the typed plan, its refusals, a mockable `DeclareEnv` seam,
and a real GitHub forge. `KubeClusterEnv::declare` was a typed refusal and now
opens a branch, writes the whole manifest, and opens a PR.

Three properties worth carrying forward:

- **The head branch is content-addressed.** Re-declaring the same change targets
  the same branch and updates the existing PR, so a retry after a network
  failure is not a duplicate a reviewer must reconcile. The forge treats
  422-already-exists as success on both the branch and the PR for the same
  reason.
- **Order is the safety property** — base sha, branch, file, PR. A failure
  before the file leaves a branch (harmless, reused by the retry) rather than a
  PR describing a file that was never written, which is worse than no PR because
  a reviewer approves it.
- **It refuses rather than guesses.** Only the `flux-helm-values` rail carries
  its own path; the other four name a *reference* whose owning repository banken
  does not hold, and an unset `:gitops-repo` refuses too. A PR opened against a
  guessed repository looks correct and reconciles nothing — worse than an error,
  because it spends a reviewer's trust.

`octocrab` over `gix` deliberately: a DECLARE needs no working tree, and a
cluster navigator is not a git client.

**REMAINING — the visible loop.** Showing reconciliation in the TUI (the PR
merges, the reconciler converges, the row moves) is not built.
`pending-banken: declare-watch-reconciliation`. And four of the five rails still
cannot be routed: `pending-banken: declare-rail-routing`.

- **Done-predicate:** scaling a Deployment in banken produces a reviewed commit,
  and the operator watches the row move without leaving the tool. **The commit
  half is MET; the watch half is not.**

### Phase E — BREAK-GLASS becomes real *(the ledger half SHIPPED 2026-08-12)*

`exec` and `port-forward` are what pull people back to `kubectl`. banken should
have them — as witnessed actions, which is precisely the design's strength.

**SHIPPED — the record.** `crate::glass` is a write-ahead, fsynced, append-only
ledger, and `KubeClusterEnv::break_glass` writes to it. The ordering is a
signature rather than a convention: `GlassLedger::record` is the *only*
constructor of `Witnessed`, and `open_witnessed_session` takes `&Witnessed`, so
opening a break-glass session before the record is durable has no code path.
Tier: parse-time-rejected within the crate (the field is private and there is no
`Default`), **not** unrepresentable fleet-wide — an author editing `glass.rs`
could add a second constructor.

The failing path is covered by construction rather than by remembering to log:
the record is written *before* the effect, so an effect that fails, hangs, or
kills the process cannot leave an unrecorded break-glass. A crash between the
two leaves a record of something that may not have happened — over-recording,
the safe direction. `GlassLedger::unresolved()` surfaces exactly those.

Red-run 2026-08-12: removing the append turned 8 of the 14 ledger tests red,
including both load-bearing ones, so the suite is not blind.

The ledger is also exposed on the MCP surface as `banken_glass_ledger` —
**read-only**. Reading the audit trail is an OBSERVE and often the missing half
of a triage ("a human exec'd into this pod twenty minutes ago" explains a state
no other read can), while `break_glass` itself stays absent from that surface.

**REMAINING — the effect.** `exec` should open through the `(defbancada)`
handoff into tear/mado, so the terminal is a real terminal and banken stays a
navigator. `open_witnessed_session` is the arm and is wired; what is missing is
the TUI chord that builds the plan and the resolve-on-exit call.
`pending-banken: break-glass-exec-chord`.

- **Done-predicate:** a witnessed record exists for every break-glass, and the
  record is written on the failing path too. **MET for the record half; the
  effect half is not yet reachable from the TUI.**

### Phase F — the agent surface *(differentiator #2, the biggest)* — **SHIPPED 2026-08-12**

An MCP server over the same postigo gate. This is the item with no competitor.

`banken mcp --context <name>` (or `--fixture`) serves eight OBSERVE tools over
stdio: `capabilities`, `views`, `list`, `get`, `logs`, `events`, `readiness`,
`glass_ledger`. Every one calls a method on `ClusterEnv`, which has no
unwitnessed-mutate method — so there is no `delete`/`apply`/`scale`/`exec` tool
to call, and `no_mutating_tool_exists` asserts that against the live router
rather than against the source.

DECLARE and BREAK-GLASS are **absent, and `banken_capabilities` says so and says
why.** A DECLARE's honest MCP shape returns a proposed manifest and a branch,
which is the half Phase D still owes; a break-glass record NAMES the operator
who authorised it, and inventing a witness so a tool signature type-checks would
make the record a lie. `pending-banken: mcp-declare` / `mcp-break-glass`. That
is a narrower surface than this phase originally scoped ("DECLARE returns a
proposed manifest and a PR; BREAK-GLASS requires an explicit witness argument"),
and the narrowing is deliberate — both wait on work that has not landed rather
than shipping a shape that would mislead.

Two invariants worth carrying forward, both found by probing the running server
rather than by reading the code:

- **`banken mcp` refuses to guess its estate**, which is *stricter* than the
  TUI. The TUI answers an unnamed run by opening the picker; an MCP server has
  neither a screen to draw one on nor a human to answer it, so riding
  `current-context` would serve some other estate's rows to a reader with no way
  to notice.
- **A refusal is never shaped like an empty result.** To an agent, `[]` and
  "I could not look" are opposite claims, and a triage that concludes "the
  deployment is gone" from a credential expiry is worse than one that stops.

- **Done-predicate:** an agent can triage a failing workload end to end and
  *cannot* fix it live — only propose a change. **MET**, verified against a real
  MCP client over stdio (initialize → tools/list → tools/call).

### Phase G — fleet convergence — **visuals + config SHIPPED 2026-08-12**

- **Visuals: DONE, and narrower than this line originally read.** The ronda
  ramp's five hardcoded RGB triples now come from the fleet theme's own
  error/warning/success via `crate::palette`; three anchors replaced five, so
  the intermediate rungs are genuine interpolations. Two guards assert the ramp
  *follows* the theme rather than merely reading from something named after it.

  **Named ANSI slots deliberately stay slots.** `Color::Green` is not a
  hand-picked value — it is an index into the *operator's* terminal theme, so
  banken's green is their green. Converting those to fleet hexes would look like
  convergence and be a regression. That is the convergence decision, not an
  omission from it.
- **Config: DONE.** `(defbanken …)` grew `:gitops-repo`, `:gitops-base`,
  `:ronda-round-ms`, `:ronda-climb-ms` alongside `:picker-stance`. The GitOps
  target was an environment variable for exactly one commit — the untyped,
  unauthored, undiscoverable surface the fleet config rule exists to eliminate.
  `prescribed_mirrors_the_authored_lisp` caught the struct/Lisp drift the moment
  the fields landed, which is the forcing function working. Remaining:
  per-view columns are still authored in `(defk8sview …)` rather than
  overridable per-deployment, which is correct and not debt.
- **Clippy: 88 warnings → 0**, workspace-wide. The debt was invisible because
  the lint never ran on this machine (the system profile ships only `cargo`, and
  the "no such command" error's ANSI prefix slipped past a `grep "^error"`).
  Every run is now `nix develop --command cargo clippy`.
- **Tests:** adopt the `dojo` facet vocabulary as it lands. Not started —
  `dojo`'s repo is empty (theory only), so there is nothing to adopt yet.

### Phase H — ronda deepens — **per-verb readiness SHIPPED 2026-08-12**

- **Per-namespace and per-verb readiness: DONE.** `crate::permit` asks the
  apiserver directly via `SelfSubjectAccessReview` — one POST, same RBAC the
  real request would hit, no side effect, no dependence on the object existing.
  It separates *forbidden* from *empty* before a table is drawn, which is the
  distinction a navigator otherwise cannot make: the two look identical, and
  rendering them the same way sends an operator to debug a healthy workload.

  `Permit::Unknown` is a variant rather than an error, and `should_render()`
  returns true for it — hiding a readable view on an inconclusive check is the
  failure the type exists to prevent. `describe()` claims *authorization*, never
  success, because RBAC is one gate among several.

  Exposed as `banken_can_i`. Safe for destructive verbs by construction: asking
  "may I delete pods" changes nothing, which is what lets an agent learn the
  shape of its own access without exercising any of it.
- **Ladder rungs for the resource plane** — the primitive is built; wiring a
  view to grey itself out on a `Denied` permit is not.
  `pending-banken: view-greys-out-on-denied`.
- **Warm authenticated client pool** — NOT built, and deliberately not.
  It re-runs each context's credential helper on a timer, which on an EKS estate
  is a recurring `aws eks get-token` per context: real background SSO traffic.
  That is an operator decision, not a default, and it has not been made.
  `pending-banken: ronda-credentials`.

## IV. What this deliberately does NOT chase

- **Plugins / arbitrary shell hooks.** k9s's plugin system is a shell escape,
  and it would drive a hole straight through the legality gate. The authored
  vocabulary is the extension point.
- **In-place YAML editing that applies to the cluster.** That is DECLARE's job.
  An editor that writes to the apiserver is the exact affordance banken exists
  to not have.
- **Benchmarking / sanitizing / cluster-scoring.** Real value, wrong tool —
  those belong to the observability and autorevivy planes.

## V. Sequencing

A is the gate on B–F and is unglamorous; do it first and completely. B is cheap
while the vim work is fresh and is a standing operator request. C is the bulk of
the daily value. D and F are the differentiators and should not start before A
lands, or they will be built pod-shaped and rebuilt.

G runs alongside throughout — it is small, continuous, and rots if deferred.

## VI. Standing constraints

- **Tier-honest labelling.** `Open` is not `Ready`; a preview is not a change; a
  mock-proven handoff is not a live one. Every claim in the UI states what was
  measured. This is the property the whole tool trades on, and it is the easiest
  to lose one convenient adjective at a time.
- **No estate identifiers.** `tests/no_estate_identifiers.rs` gates the tree;
  the repo is public.
- **Secrets.** A `secrets` view lists names and never values without an explicit
  witnessed action — the crivo classification rule applied to a TUI.
- **No shell.** Reads go through the typed `kube` client; orchestration is Rust
  or tatara-lisp.
