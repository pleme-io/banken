;; banken drill paths — the (defdrill) authoring surface.
;;
;; A view's or ward's `:drill-to` names one of these. Before this domain
;; existed, `:drill-to "logs"` resolved against NOTHING — the shipped
;; views.lisp drilled to "logs" and "diagnose", neither of which was a
;; declared anything, so a typo or a rename was a silently dead Enter key.
;; `Catalog::resolve` now rejects a dangling target
;; (SpecError::DanglingDrill).
;;
;; A path must strictly DESCEND the hierarchy
;; (context < namespace < workload < pod < container < terminal). A path that
;; zooms out, revisits a rung, or chains two terminals has no valid value
;; (SpecError::NonDescendingDrill), and an empty path is rejected outright
;; (SpecError::EmptyDrill).

;; k9s's `l` on a pod row: the log pager. Referenced by the `pods` view.
(defdrill
  :name "logs"
  :from "pods"
  :steps ((:level pod :view "pods")
          (:level container :view "containers")
          (:level logs :view "log-pager")))

;; BANKEN.md §V's drill-flow: Enter on a RED ward row walks the symptom→cause
;; path to the load-bearing cause and presents the remedy as the exact
;; release.yaml diff a `postigo` DECLARE would commit. Referenced by both the
;; `ward` view and the `ward` (defward).
(defdrill
  :name "diagnose"
  :from "ward"
  :steps ((:level workload :view "ward")
          (:level diagnose :view "diagnose")))

;; XRay — the dependency tree over owner-refs + endpoints + caixa
;; `:contratos`, with the declared-vs-observed overlay k9s structurally
;; cannot express (BANKEN.md §II/§V).
;;
;; Referenced by no `:drill-to` yet, which is ALLOWED: the `:` command bar is
;; a first-class peer of the ward (§V), so a command-bar-reachable drill
;; needs no referent. `Catalog::resolve` deliberately does not require one.
(defdrill
  :name "xray"
  :from "ward"
  :steps ((:level namespace :view "namespaces")
          (:level workload :view "workloads")
          (:level xray :view "xray")))
