# Resource providers: one tree for every cloud

## Destination

banken gives one experience for every estate: Kubernetes, AWS, Azure and Google
Cloud. The same keys work everywhere: `enter`/`l` go down, `esc`/`h` go up, `i`
inspects, `e` edits the document, `dd` deletes, `:` jumps by path. When a
credential fails, the same doctor panel opens. Every change goes through postigo.
A refused account is visible with its reason and has no path to a change.

Each part is a blue bidama, so mado, arnes and other tools can import it:

| Bidama | Job | State (2026-11-05) |
|---|---|---|
| `shinsatsu` | the credential doctor: provider, failure classes, diagnosis, steps with a flow, verify | SHIPPED in `blue/bidamas`; banken consumes it |
| resource-provider contract | the words below, and the all-variants matrix | DESIGN |
| tree / path model | path segments, ascend, descend, `:` jump, history, breadcrumb | DESIGN; in Rust today (`banken/src/app.rs`) |
| edit loop | fetch document, edit, server-side diff, confirm, apply, reopen on error | DESIGN; in Rust today (`banken/src/edit.rs`) |
| binding table | the default keys and the operator overlay | DESIGN; tatara-lisp today (`banken-spec/specs/navkeys.lisp`), overlay in blue (`banken.b`) |
| `aws`, `azure`, `gcp` providers | the contract over Cloud Control, ARM and Cloud Asset Inventory | DESIGN |

## The contract

A provider is a set of words over a path. A path is a list of segments, and the
first segment names the estate.

| Word | Answer |
|---|---|
| `children(path)` | the next segments under a path, each with a kind, a name and a refusal when the provider refuses it |
| `document(path)` | the object as a document (a map), with server bookkeeping removed |
| `preview(path, document)` | the server-side diff of a proposed document, and whether it replaces the object |
| `apply(path, document, witness)` | the change, after postigo witnessed it |
| `delete(path, witness)` | the delete, after postigo witnessed it |
| `verbs(kind)` | the verbs legal for a kind: `inspect`, `edit`, `delete`, and imperative verbs such as `reboot` or `rotate` |
| `facts(failure)` | the doctor facts for an auth failure, so `shinsatsu::diagnose` can answer |
| `posture(path)` | `read-only` or `mutate` for the account, subscription, project or cluster |

Rules:

- A field that the schema marks create-only or immutable makes `preview` answer
  `replace`. The panel shows a replace with a stronger confirm.
- A kind the uniform API does not cover is read-only, and `verbs` says why.
- An imperative action is a verb, never an edit.
- A refused estate (nerai retired, blocked or read-only) answers `children` with
  the refusal, and `apply` and `delete` have no path for it.
- Every kind × every verb has a row in the provider's all-variants matrix.
  Tests run against recorded API fixtures. No test calls a real cloud.

## The providers

| Provider | Path | List | Document | Diff | Apply | Delete |
|---|---|---|---|---|---|---|
| Kubernetes (shipped) | context › namespace › kind › object | watch/list | GET, `managedFields` removed | server-side apply dry run | server-side apply | DELETE |
| AWS | account › region › type › identifier | Cloud Control `ListResources` | `GetResource` | schema read-only / create-only properties, JSON Patch | `UpdateResource` (JSON Patch) | `DeleteResource` |
| Azure | subscription › resource group › provider/type › resource | ARM list | ARM GET at the resource id | ARM what-if | ARM PUT | ARM DELETE |
| Google Cloud | project › service › type › resource | Cloud Asset Inventory | the service's REST GET | field-mask diff | the service's REST PATCH | the service's REST DELETE |

## Recon (2026-11-05)

A codesearch over pleme-io found no uniform CRUD model for any cloud: pangea,
iac-forge and the crossplane providers generate Terraform-shaped resources, not
a read/patch surface over the live API, and the vendor Terraform providers in
the org cover one product each. The provider bidamas start from the cloud APIs
above.

## Kubernetes today

`ClusterEnv` carries the contract's Kubernetes half: `inspect`, `manifest`
(the document), `preview_change`, `posture`, and `break_glass` with a
`GlassAct::Delete` or `GlassAct::Apply`. A read-only registry target answers
`posture` with `ReadOnly`, so `dd` and `e` are refused before any prompt.
