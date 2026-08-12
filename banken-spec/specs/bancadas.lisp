;; banken pre-warmed troubleshooting sessions — the (defbancada) authoring
;; surface. A "bancada" (pt-BR) is a workbench: the surface with your tools
;; already laid out, so work starts the instant you sit down.
;;
;; This is the banken → tear/mado bridge. From a selected row, one authored
;; chord produces a whole tear session — the right panes, already on the right
;; cluster/namespace/resource, with the troubleshooting commands staged — so
;; the operator is FIXING the problem rather than setting up to fix it.
;;
;; ─── Three things about this file that are load-bearing ───────────────────
;;
;; 1. THERE IS NO `:legality` KWARG, and that is the point. A recipe's postigo
;;    class is DERIVED from its panes (`BancadaSpec::legality`): any pane whose
;;    `:effect` is `mutates` makes the whole recipe BREAK-GLASS. So a mutating
;;    recipe has no field in which to claim it observes. A BREAK-GLASS recipe
;;    MUST carry `:witness` + `:runbook` (SpecError::UnwitnessedBancada), and a
;;    pure-observe recipe must NOT (SpecError::UnneededWitness).
;;
;; 2. A COMMAND IS A TYPED ARGV, NEVER A SHELL STRING. `:program` plus an
;;    ordered `:args` list, each element either `(:literal "…")` or
;;    `(:context <field>)`. No quoting, no interpolation, no shell. The
;;    `(:context …)` fields are a CLOSED catalog — an unknown field has no
;;    typed value, rather than resolving to an empty string.
;;
;; 3. `(:context cluster)` IS WHAT MAKES "THE RIGHT CLUSTER" TRUE. A pane
;;    opened without an explicit `--context` lands on whatever the kubeconfig's
;;    current context happens to be, which is NOT necessarily the one banken is
;;    reading. When banken does not know its own cluster id, the planner
;;    REFUSES (SpecError::UnresolvedContextField) instead of emitting
;;    `--context ""` — an argument that is silently wrong rather than failing.
;;
;; `:layout` and `:placement` are projections of tear's own `LayoutKind` /
;; `Direction`, so the adapter to `MultiplexerControl` is a rename, not a
;; translation layer with opinions. `LayoutKind::Custom` is deliberately absent:
;; "custom" means whatever the operator arranged by hand, which is not something
;; a recipe can declare.

;; ── OBSERVE — the everyday triage landing ────────────────────────────────
;;
;; Three read panes on the selected pod: a live log tail (the big one), its
;; events, and its full manifest. Nothing here mutates, so the recipe is
;; OBSERVE and carries no witness.
(defbancada
  :name "pod-triage"
  :keys "g"
  :from "pods"
  :layout main-vertical
  :session-prefix "triage"
  :panes ((:role logs
           :placement root
           :command (:program "kubectl"
                     :args ((:literal "--context") (:context cluster)
                            (:literal "-n")        (:context namespace)
                            (:literal "logs")
                            (:literal "--follow")
                            (:literal "--tail=200")
                            (:context resource-name))
                     :effect observes))
          ;; Namespace-scoped rather than object-scoped ON PURPOSE.
          ;; `--field-selector involvedObject.name=<pod>` needs the pod name
          ;; CONCATENATED into one argv element, and `CommandArg` has no join
          ;; arm — deliberately, because a join is one step from string
          ;; interpolation and this domain's whole claim is that a staged
          ;; command is a typed argv. A namespace-scoped event watch is the
          ;; honest thing the vocabulary can express today; the day a second
          ;; recipe needs a joined argument, that is the third-use signal to
          ;; add a typed `(:joined …)` arm rather than to write one here.
          (:role events
           :placement right
           :command (:program "kubectl"
                     :args ((:literal "--context") (:context cluster)
                            (:literal "-n")        (:context namespace)
                            (:literal "get") (:literal "events")
                            (:literal "--watch"))
                     :effect observes))
          (:role describe
           :placement below
           :command (:program "kubectl"
                     :args ((:literal "--context") (:context cluster)
                            (:literal "-n")        (:context namespace)
                            (:literal "describe")
                            (:context resource-kind)
                            (:context resource-name))
                     :effect observes))))

;; ── BREAK-GLASS — the wedged-workload escape hatch ───────────────────────
;;
;; Same log pane, plus an interactive `kubectl exec` into the selected
;; container. That exec is a LIVE EFFECT, so `:effect mutates` — and because
;; the legality is derived, the whole recipe becomes BREAK-GLASS and the
;; `:witness` + `:runbook` below stop being optional. Removing either one
;; fails the catalog by name (SpecError::UnwitnessedBancada), and the
;; witnessed staging arm is the ONLY path an argv with `mutates` can reach
;; (`MutatingCommand` is unconstructible from an observing pane).
;;
;; The chord is "shift+g", not "G": awase::Hotkey::parse folds case, so "G"
;; and "g" are the SAME typed chord and authoring "G" here would collide with
;; `pod-triage` above — silently, before `Catalog::resolve` learned to check
;; all three keyed domains against one namespace. See banken_spec::chord.
(defbancada
  :name "pod-break-glass"
  :keys "shift+g"
  :from "pods"
  :layout main-horizontal
  :session-prefix "glass"
  :witness "drzzln"
  :runbook "clusters/bravo/RUNBOOK.md"
  :panes ((:role logs
           :placement root
           :command (:program "kubectl"
                     :args ((:literal "--context") (:context cluster)
                            (:literal "-n")        (:context namespace)
                            (:literal "logs")
                            (:literal "--follow")
                            (:context resource-name))
                     :effect observes))
          (:role shell
           :placement below
           :command (:program "kubectl"
                     :args ((:literal "--context") (:context cluster)
                            (:literal "-n")        (:context namespace)
                            (:literal "exec") (:literal "-it")
                            (:context resource-name)
                            (:literal "-c") (:context container)
                            (:literal "--") (:literal "/bin/sh"))
                     :effect mutates))))
