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

### Each cluster appears once

The chooser reads two sources. The first is the target registry: one
kubeconfig per target under `~/.kube/targets/<target>`. The second is the
`KUBECONFIG` merge list. A registry target opens with its own file, so no
merge can change which cluster it reaches.

An ambient name that a refusal file answers (a nerai stub, whose credential
helper is `nerai-refuse`) is not a cluster. banken hides these rows and counts
them in the title (`· 10 refused hidden`). Set `config(:picker, {refused: :dim})`
to show them dimmed, each with its reason. A stub that shadows a registry
target does not make the name ambiguous, and the duplicate row does not appear.

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

### A failed login is a panel, not a wall of JSON

When the credential helper fails, banken names the failure in one line, for
example `east: Azure login expired — run az login --use-device-code --scope <id>/.default`.
For a known failure it opens a panel. The panel shows the provider, the cluster,
the apiserver, the tenant and the scope when it knows them, every command it
will run, and the check it runs after. Nothing runs until you press `enter`.

The commands run inside the panel. A device flow (`az login --use-device-code`,
`aws sso login --use-device-code`) shows its URL and code in large type, and a
browser flow (`gcloud auth login`) shows its URL. The panel shows the command's
own output and the time it has waited. When the steps pass, the panel runs the
kubeconfig's own credential helper as the check, and banken connects again.
`esc` stops a running command; a second `esc` returns to the cluster list.

The doctor is the `shinsatsu` bidama in blue's standard distribution
(`pleme-io/blue/bidamas/shinsatsu`), so other tools use the same one. A new
provider or failure is a declaration there.

### The wait is a place

Connecting is a screen, not a blank terminal: it names the stage it is on
(kubeconfig → configuration → credentials → first read), times the slow one, and
`esc` cancels back to the list.

## Navigation is a tree

The screens form a path, the same shape as the Kubernetes API path:
cluster › namespace › kind › object › detail. The title bar shows the path.

| Keys | Action |
|---|---|
| `enter` / `l` / `→` | go down: scope to a namespace, open an object, confirm a session |
| `esc` / `h` / `←` | go up: close a detail, clear the filter, drop the namespace, then return to the cluster list |
| `j` / `k`, `gg` / `G`, `ctrl+d` / `ctrl+u` | move |
| `/` | filter by name |
| `:` | jump by path or name: `:pods`, `:ns/kube-system`, `:pods/ns/web`, `:ns/*`, `:..`, `:q` |
| `ctrl+o` / `tab` | history back / forward |
| `shift+h` / `f1` | help |
| `q`, `:q`, `ctrl+c` | quit |

`esc` never quits. Logs moved to `shift+l`, and the session recipes moved to
`t` / `shift+t`, because the vim keys now navigate.

### Verbs on an object

| Keys | Class | Action |
|---|---|---|
| `i` | OBSERVE | inspect: facts, labels, owner chain, conditions and events |
| `e` | BREAK-GLASS | edit: banken suspends, opens `$VISUAL` / `$EDITOR` on the live YAML (no `managedFields`), shows the server's dry-run diff, and applies only after you confirm. A refused or invalid manifest reopens the editor with the error on top, and your edit stays |
| `dd` | BREAK-GLASS | delete: a prompt names the cluster, the namespace and the object first |

On a read-only registry target (a production target its refusal file guards),
banken refuses `e` and `dd` before any prompt. Every change is witnessed and
recorded in the break-glass ledger before it runs. Describe moved from `d` to
`i` because `dd` is the vim delete.

Session recipes pass the connection's own kubeconfig file (`--kubeconfig`), so
a pane on a registry target reaches the target, not the ambient refusal.

The design for other clouds (AWS, Azure, Google Cloud) on one provider contract
is in [docs/PROVIDERS.md](./docs/PROVIDERS.md).

### Configure it in blue

Every navigation key comes from one authored table (`banken-spec/specs/navkeys.lisp`).
The help page reads the same table. Change it in `~/.config/banken/banken.b`
(or `$BANKEN_WORLD`):

```blue
use("motenashi", %i[bind config])

def banken()
  [
    bind("ctrl+n", :select_next),
    bind("k", :off),
    config(:picker, {refused: :dim, targets: "/home/me/.kube/targets"})
  ]
end
```

A world binding replaces the default on the same chord. `bind(chord, :off)`
turns a default off. banken refuses a bad entry alone and shows the reason; the
other entries still apply. The world runs in a child process with a step budget.

## Usage

```
banken                      choose a cluster, then open :pods on it
banken --context <name>     open :pods on that cluster or registry target directly
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
