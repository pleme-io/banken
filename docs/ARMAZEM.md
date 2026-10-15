# Armazém — lazy absorb, state apart from the screen

**Status: DESIGN (2026-10-15).** Nothing below is built unless a phase says so.
It closes `pending-banken: absorb-multi-gvk` and replaces the per-view
synchronous read.

## What is wrong today, measured

Measured against ryn's engenho on 2026-10-15 with banken 0.1.41:

- **One kind is absorbed.** `main.rs` spawns a single pod watch at startup.
  Every other view is a synchronous `list_resources` on the app task: in
  `switch_view` (`app.rs:683`) and again in `refresh_current_view`
  (`app.rs:717`). That breaks absorb rule 1 ("the read never runs on the app
  task"). A slow apiserver freezes the keyboard on every `:deploy`, and the
  deployments view never moves after it is drawn.
- **The MCP server keeps no state.** Every `banken_list` is a fresh LIST
  (`mcp.rs:477`). An agent pays a full LIST per question, and cannot see what
  the operator's banken holds. "What does banken know" and "what does the
  screen show" are the same thing, so neither can be tested without a
  terminal.
- **What banken knows is tied to what it shows.** Leave the pods view and its
  rows are gone. Come back and banken reads them again from zero.

## The destination

An **Armazém** (warehouse) holds one `Despensa` per absorbed key. A view and
an MCP question are both just *interest* in a key. The screen is one of
several readers. A key stays watched while someone is interested, stays warm
for a linger window after that, and then goes **dormant**: the watch stops,
the last snapshot is kept and labelled dormant, and new interest resumes the
watch from the stored `resourceVersion`.

```
              Interesse (lease)            Interesse (lease)
  TUI view ───────────────┐      ┌──────────────── MCP question
                          ▼      ▼
                    ┌──────────────────┐   demand    ┌───────────────┐
                    │     Armazém      │ ──────────▶ │ zelador vigy  │
                    │ key → Despensa   │ ◀────────── │ (tatara-lisp) │
                    └──────────────────┘ start/park  └───────────────┘
                          ▲   ▲
               watch task │   │ watch task      (one per Live key)
                       apiserver
```

### Types

| Type | Owns | Rule it carries |
|---|---|---|
| `AbsorbKey` | `(ResourceKind, Scope{All, Namespace(..)})` | the cluster is fixed per Armazém. A context switch builds a new one, so no key can mix two estates |
| `Armazem` | `key → Entry{despensa, publisher, task, interest, last_rv}` | the only place a watch task is created or stopped |
| `Interesse` | an RAII lease on one key | taking one is O(1) and never blocks. Dropping it decrements. There is no other way to express demand |
| `SyncPhase` | gains `Dormant { since, resume_rv }` | dormant rows are kept and *labelled*, never shown as live, so a stale table can't pass as a current one |
| `ZeladorAction` | `Start(key)`, `Park(key)`, `Evict(key)` | the reconciler emits these and only the Armazém carries them out |

**The view never reads the env.** `switch_view` takes an `Interesse` and draws
whatever snapshot exists: `Absorbing` with no rows the first time, or the
dormant rows labelled as dormant. The table fills when the publisher's wake
fires. `list_resources` stays on `ClusterEnv` for the MCP fallback and the
grip re-read.

### Why the lifecycle is a vigy

"Every tick, compare demand with what is running, decide, act" is exactly
what vigy was built for. vigy records and broadcasts the actions, and the host
carries them out (`vigy/CLAUDE.md`, "Doesn't interpret reconcile actions").
banken embeds the runtime in memory (`RuntimeHandle::open_in_memory_for("banken")`,
with no SQLite file) and registers one host intrinsic:

- `(banken-demand)` returns one row per key: `interest`, `idle-ms`, `phase`,
  `generation` and `live-watches`.

The default policy ships as `banken-spec/specs/zelador.lisp`:

```lisp
(defzelador
  :linger-ms 30000
  :max-live-watches 12
  :evict-dormant-after-ms 3600000)
```

The vigy program reads `(banken-demand)` and emits:
- `start` for any key with interest that is not live;
- `park` for any live key with zero interest that has been idle past
  `:linger-ms`, or that falls outside the most recent `:max-live-watches`;
- `evict` for any key dormant past `:evict-dormant-after-ms`.

The knobs live in `banken-config`'s overlay tier, so an operator can change
them without a rebuild. The policy itself is data, and a second policy is just
a declaration.

**Resume, not relist.** `kube_runtime::watcher` always opens with a LIST, so
resuming uses `Api::watch(&WatchParams, resume_rv)` directly. A `410 Gone`
(the rv was compacted) falls back to the watcher's relist. Either way the
phase says which path ran.

### MCP reads the Armazém

- Every read tool takes an `Interesse` for its key. It waits until the key is
  `Synced` (or a bound is reached) and answers from the snapshot, stamped with
  `phase`, `generation`, `resourceVersion` and the snapshot's age. A second
  question about the same kind costs nothing on the wire.
- **New tool `banken_absorb`**: lists every key with its phase, interest,
  generation, last rv, idle time and row count. This is what makes "what
  banken knows" testable separately from "what the screen shows".
- **A TUI-hosted MCP endpoint** (`banken --mcp-socket PATH`): the agent reads
  the same Armazém the operator's screen reads. The screen becomes one view
  of the state rather than its owner.

## Conformance against engenho

The k9s work proved "every k9s action works on engenho" as a matrix
(`engenho/docs/K9S-CONFORMANCE.md`). banken gets the same, built from the
other end: **banken's real client code, not request shapes mined from its
source.**

1. **engenho, `tests/client_conformance/`.** The k9s suite's harness (the
   in-process cluster, seed and judge) moves into a shared `tests/support/`
   module, because it now has two consumers. `banken` (crates.io, feature
   `live`) is a dev-dependency, and its `KubeClusterEnv` runs against the
   in-process cluster. Matrix: `BankenAction::ALL`, with an exhaustive
   ordinal and at least one row per action:
   - list of each typed kind under both `ListStrategy` variants;
   - get, grip (including the uid mismatch after recreate), can-i, logs
     (bounded), events, aggregated discovery into the `RestMapper`, and the
     readiness climb;
   - absorb: a Synced snapshot, a MODIFIED reaching the Despensa, and resume
     after park, plus relist on 410.

   This needs one banken constructor, `KubeClusterEnv::from_config(kube::Config)`,
   since the harness has no kubeconfig context.
2. **banken, `tests/engenho_live.rs`** (`#[ignore]`, `BANKEN_LIVE_CONTEXT`,
   the same no-default rule as `live_read.rs`). Every MCP tool against a real
   engenho, and `banken_absorb` showing a key go Live → Dormant → Live. This
   is the ryn session that k9s got, done over state instead of a screen.

## Gaps the first live pass found (2026-10-15)

| Gap | Side | Status |
|---|---|---|
| node status rewritten without a resourceVersion bump (heartbeat advanced, rv stayed 1659760) | engenho | being fixed |
| node `status.nodeInfo` absent, so VERSION shows `-` | engenho | being fixed |
| CronJob-made Jobs carry no `creationTimestamp` | engenho | being fixed |
| server-side Table prints only NAME and a raw-timestamp AGE | engenho | being fixed |
| pod AGE hardcoded to `-` | banken | being fixed |
| `banken_logs` bounded by lines only (74,623 chars came back) | banken | being fixed |
| readiness settles at `network` for a context banken reads fine | banken | being fixed |

## Phases

| # | Phase | Closes |
|---|---|---|
| A1 | `Armazem` + `Interesse` + `AbsorbKey`. Every typed kind is absorbed, and no view reads the env | `absorb-multi-gvk` (typed kinds) |
| A2 | `Dormant` phase, park and resume from rv, relist on 410 | — |
| A3 | the zelador vigy, `(defzelador)` vocabulary and the config knobs | — |
| A4 | MCP reads through the Armazém, `banken_absorb`, `--mcp-socket` | `mcp-shares-state` |
| A5 | `KubeClusterEnv::from_config`, then engenho's `client_conformance` | — |
| A6 | `engenho_live.rs` against ryn | — |

pending-banken: armazem — this document; each phase above is a row.
