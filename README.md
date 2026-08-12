# banken 番犬

An observe-first, GitOps-native Kubernetes cluster navigator for the terminal.

`banken` keeps the fast keyboard navigation of a TUI cluster browser and routes
every action through a three-class legality gate:

| Class | What it may do |
|---|---|
| **OBSERVE** | reads freely — pods, logs, events |
| **DECLARE** | lowers to a full-manifest GitOps change a reconciler applies |
| **BREAK-GLASS** | witnessed and recorded before it runs |

There is **no unwitnessed live-mutate path**. That is structural rather than
enforced: the `ClusterEnv` seam has no unwitnessed-mutate method, so a live
mutation is a compile error and not a policy someone has to remember.

## The landing screen

`banken` opens on a cluster chooser rather than on a table, because a navigator
whose first screen is invented data is not a navigator. Each row carries the
**apiserver URL** the context resolves to — the field a name cannot give you —
and a name declared by two kubeconfig files is shown as ambiguous and refuses to
be chosen, rather than being accepted and failing later.

### It is modal, and it means it

The chooser opens in **NORMAL**. `j`/`k` move, `i` starts filtering, `esc`
returns to NORMAL and never leaves the screen, `q` quits. In INSERT the query
line is a real one-line vim buffer — `hjkl0$wbe` motions, `d`/`c`/`y` with text
objects (`diw`, `ci"`), `x`/`D`/`C`, counts, and a register — plus the readline
erase chords `ctrl+w` / `ctrl+u` / `ctrl+k` and forward `delete`.

### The rows tell you how far you would actually get

A watchdog walks its **rounds**: every declared apiserver is probed and the row
is lit by an ordered access ladder, from a bounded TCP connect up to "this
identity may list pods here".

```
◌ not probed    ○ nothing answered      ◔ port open, no apiserver reached
◑ apiserver answered, identity rejected ◕ identity accepted   ● may list pods
```

The colour is a continuous ramp over that ladder and eases between rungs, so a
climb reads as progress. The glyph carries the same information, so the row is
readable without colour.

Each rung means only what it measured. `◔` says packets get through and nothing
more — not that your credentials work. When a climb stops early it says why
("credentials: SSO session expired"), which is the half that tells you what to
go and fix.

The cheap probe runs often; the credential climb runs rarely and only against
contexts that already answered, because it spawns your kubeconfig's exec
credential helper.

### The wait is a place

Connecting is a screen, not a blank terminal: it names the stage it is on
(kubeconfig → configuration → credentials → first read), times the slow one, and
`esc` cancels back to the list.

## Usage

```
banken                      choose a cluster, then open :pods on it
banken --context <name>     open :pods on that cluster directly
banken --fixture            explore the interface on canned rows
banken --help
```

`--context` is required for a named live run. Riding the kubeconfig's
`current-context` reads whichever estate a merged `KUBECONFIG` happens to point
at, and a pod table from the wrong cluster looks exactly like one from the right
cluster.

## Built on

[`egaku`](https://github.com/pleme-io/egaku) /
[`egaku-term`](https://github.com/pleme-io/egaku-term) for widget state and the
typed cell surface, [`awase`](https://github.com/pleme-io/awase) for keybindings,
`unsoku` for vim motions, and a `tatara-lisp` vocabulary for the authored
keymap, actions and session recipes — so the chords the legend advertises and
the chords the runtime binds are one derivation rather than two lists.

No `kubectl` subprocess: reads go through the typed `kube` apiserver client.

## License

MIT — see [LICENSE](./LICENSE).
