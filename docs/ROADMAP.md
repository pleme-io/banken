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

### Phase D — DECLARE becomes real *(differentiator #1)*

Today DECLARE renders a full-manifest preview and stops. That preview is the
hard half; the remaining half is delivery.

- Resolve the resource's owning repo + path from the GitOps tree.
- Open a branch, write the **full manifest** (never a patch — the fleet rule),
  open a PR, return a `ChangeRef` the overlay can follow.
- Show reconciliation: the PR merges, the reconciler converges, the row changes.
  Closing that loop *visibly* is what makes GitOps feel faster than `kubectl`
  rather than slower — and slowness is the only reason people reach for `kubectl`.
- **Done-predicate:** scaling a Deployment in banken produces a reviewed commit,
  and the operator watches the row move without leaving the tool.

### Phase E — BREAK-GLASS becomes real

`exec` and `port-forward` are what pull people back to `kubectl`. banken should
have them — as witnessed actions, which is precisely the design's strength.

- `exec` opens through the existing `(defbancada)` session handoff into
  tear/mado, so the terminal is a real terminal and banken stays a navigator.
- Every invocation writes a `GlassRecord` before the session opens, not after.
- **Done-predicate:** a witnessed record exists for every break-glass, and the
  record is written on the failing path too.

### Phase F — the agent surface *(differentiator #2, the biggest)*

An MCP server over the same postigo gate. This is the item with no competitor.

- OBSERVE tools map to the read seam; DECLARE returns a proposed manifest and a
  PR; BREAK-GLASS requires an explicit witness argument or refuses.
- The agent cannot exceed the human because it goes through the same enum. Not
  "we prompt the agent not to" — there is no tool that mutates.
- Pairs with the fleet's agentic-observability doctrine: an agent has no
  preattentive vision, so a typed catalog of queries beats a dashboard.
- **Done-predicate:** an agent can triage a failing workload end to end and
  *cannot* fix it live — only propose a change.

### Phase G — fleet convergence

- **Visuals:** replace hand-picked colours with `ishou` tokens via
  `FleetThemedConfig::from_fleet`, plus a `convergence::Guard` drift test. The
  ronda ramp becomes a token ramp, so one fleet edit moves every app.
- **Config:** grow `(defbanken …)` from one field to the real surface — opening
  stance, ronda intervals, columns per view, theme — on shikumi's tiered fold.
  `pending-banken: opening-stance-config` is already open against this.
- **Tests:** adopt the `dojo` facet vocabulary as it lands.

### Phase H — ronda deepens

- Warm authenticated client pool, so a chosen context opens instantly rather
  than climbing on demand. This is the "maintain connections" half deliberately
  deferred, and it carries real background SSO cost — an opt-in, not a default.
- Per-namespace and per-verb readiness: "may list pods here" is one question;
  "may I read secrets in `kube-system`" is the one that decides whether a view
  will be empty or forbidden **before** the operator opens it.
- Ladder rungs for the resource plane, so a view can grey out what this identity
  cannot reach instead of showing an empty table.

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
