# Armazém — lazy absorb, state apart from the screen

**Status (2026-10-15):** the substrate is built and released in izumi
(`izumi-armazem`, `izumi-mcp`). banken's adoption (phases A1–A6 below) is
not built yet. It closes `pending-banken: absorb-multi-gvk` and replaces the
per-view synchronous read.

## What is wrong today, measured

Measured against ryn's engenho on 2026-10-15 with banken 0.1.41:

- **One kind is absorbed.** `main.rs` spawns a single pod watch at startup.
  Every other view is a synchronous `list_resources` on the app task: in
  `switch_view` and again in `refresh_current_view`. That breaks absorb
  rule 1 ("the read never runs on the app task"). A slow apiserver freezes
  the keyboard on every `:deploy`, and the deployments view never moves after
  it is drawn.
- **The MCP server keeps no state.** Every `banken_list` is a fresh LIST. An
  agent pays a full LIST per question and cannot see what the operator's
  banken holds.
- **MCP answers overflow their client.** One `banken_logs` read returned
  74,623 characters. A breathe raw list is ~430 KB. 0.1.43 caps logs at 32 KiB
  by cutting the oldest lines, so whatever was cut is gone; nothing can be
  paged or followed.
- **What banken knows is tied to what it shows.** Leave the pods view and its
  rows are gone. Come back and banken reads them again from zero.

## Where it lives

The shape is not banken's. Every pleme-io MCP server has the same two
problems: reads that start from zero on every question, and answers too big
for the client. So it is built once, in izumi, and banken is its first
consumer.

| Crate | Owns |
|---|---|
| `izumi-armazem` | `Armazem<S: Source>`: one change-journaled `Shelf` per key. A stream opens on `Interesse`, parks to `Dormant` (rows and resume token kept) after `linger-ms` idle, resumes from that token, and is evicted after `evict-dormant-after-ms`. The start/park/evict decision is the `zelador.tlisp` vigy program each tick; a proptest holds it equal to the native policy. |
| `izumi-mcp` | `Streams<S>`: every read is a page inside `budget-bytes`, with an opaque `next` cursor pinned to one snapshot and a `follow` cursor whose reads return only changes, or a `gap` snapshot when the journal moved past it. An expired cursor is a typed refusal. |
| `vigy` | run history is bounded (newest 1000 per vigy by default), so a 1 s zelador cannot grow a store without end. |

**Configuration.** Lazy, vigy-governed streaming is the default:
`mode: lazy`, `policy: vigy`. `mode: eager` keeps every opened stream live,
`mode: direct` reads per question with no stream, and `policy: native` runs
the same decision without the lisp evaluator. Nothing is removed by a switch;
each is a configured state.

## The data structure: changes and flips are first-class

A `Shelf` is a state *plus* its history.

- **Content-gated sequence.** An upsert with the same fingerprint, a reset to
  the same set, or a repeated phase does not advance `seq`, so a quiet
  cluster costs no wakes and no follow traffic.
- **Phase flips are journaled in order.** `live → dormant → absorbing → live`
  is readable from any cursor, so a reader can tell "the data stopped moving
  because nothing changed" from "the data stopped moving because the stream
  was parked".
- **Bounded journal, typed gap.** A cursor older than the journal gets
  `Since::Gap(snapshot)`, never a silently partial delta.
- **Keyed or appended.** Records with an id are a keyed set (a watch's
  objects). Records without one are a bounded ring (log lines, events). The
  same shelf, cursor and follow serve both.
- **A parked stream cannot write.** The stop flag is checked under the shelf
  lock, so a source that keeps running after park can't flip a dormant shelf
  back to live.

## banken on the Armazém

| Type | Owns | Rule it carries |
|---|---|---|
| `KindKey` | `(ResourceKind, Scope{All, Namespace(..)})` | the cluster is fixed per Armazém. A context switch builds a new one, so no key mixes two estates |
| `KubeSource` | `impl izumi_armazem::Source for KubeClusterEnv`: a kube-runtime watcher per key, feeding `reset`/`upsert`/`remove`, resume token = the last `resourceVersion` | resume uses `Api::watch(&WatchParams, rv)`; a `410 Gone` falls back to the watcher's relist |
| `LogSource` | one followed container log per key, appended as a ring | replaces `banken_logs`'s cut-the-oldest cap with paging and follow |

**The view never reads the env.** `switch_view` takes an `Interesse` and draws
whatever snapshot exists: `Absorbing` with no rows the first time, or the
dormant rows labelled as dormant. The table fills when the shelf's wake fires.

**MCP reads the same Armazém**, through `izumi_mcp::Streams`:
- `banken_list`, `banken_events` and `banken_logs` take an optional `cursor`.
  The first call opens or reuses the key's stream and returns the first
  budgeted page with `next` and `follow`. A second question about the same
  kind costs nothing on the wire.
- **`banken_absorb`** returns `Armazem::inventory()`: every key's phase, seq,
  record count, interest, liveness, idle and dormant time. This is what makes
  "what banken knows" testable separately from "what the screen shows".
- **A TUI-hosted MCP endpoint** (`banken --mcp-socket PATH`): the agent reads
  the same Armazém the operator's screen reads.

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
   - get, grip (including the uid mismatch after recreate), can-i, logs,
     events, aggregated discovery into the `RestMapper`, and the readiness
     climb;
   - absorb: a Live snapshot, a MODIFIED reaching the shelf, park and resume
     from rv, and relist on 410.

   This needs one banken constructor, `KubeClusterEnv::from_config(kube::Config)`,
   since the harness has no kubeconfig context.
2. **banken, `tests/engenho_live.rs`** (`#[ignore]`, `BANKEN_LIVE_CONTEXT`,
   the same no-default rule as `live_read.rs`). Every MCP tool against a real
   engenho, paging a list past the budget and following it, and
   `banken_absorb` showing a key go live → dormant → live. This is the ryn
   session that k9s got, done over state instead of a screen.

## Gaps the first live pass found (2026-10-15)

| Gap | Side | Status |
|---|---|---|
| node status rewritten without a resourceVersion bump (heartbeat advanced, rv stayed 1659760) | engenho | in progress |
| node `status.nodeInfo` absent, so VERSION shows `-` | engenho | in progress |
| CronJob-made Jobs carry no `creationTimestamp` | engenho | in progress |
| server-side Table prints only NAME and a raw-timestamp AGE | engenho | in progress |
| pod AGE hardcoded to `-` | banken | fixed, 0.1.42 |
| `banken_logs` bounded by lines only | banken | capped at 32 KiB in 0.1.43; paging and follow come with A4 |
| readiness reported a climb in flight as settled at `network` | banken | fixed, 0.1.42 |

## Phases

| # | Phase | Closes |
|---|---|---|
| A1 | `KindKey` + `KubeSource`; every typed kind absorbed; no view reads the env | `absorb-multi-gvk` (typed kinds) |
| A2 | resume from rv, relist on 410, dormant rows drawn as dormant | — |
| A3 | the zelador ticking inside banken, its knobs in `banken-config`'s overlay tier | — |
| A4 | MCP through `izumi_mcp::Streams`, `banken_absorb`, `LogSource`, `--mcp-socket` | `mcp-shares-state` |
| A5 | `KubeClusterEnv::from_config`, then engenho's `client_conformance` | — |
| A6 | `engenho_live.rs` against ryn | — |

pending-banken: armazem — this document; each phase above is a row.
