;; banken canonical actions — the (defk8saction) authoring surface (BANKEN.md §III.b).
;;
;; Each action is typed into exactly one postigo legality class, driven
;; into typed K8sActionSpec values by banken_spec::load_actions. A plugin
;; that tries to live-mutate is UN-AUTHORABLE: DeclareTarget's `:rail`
;; discriminant has no `kubectl`/`apply` value to author into — a
;; live-mutate action has no typed value to lower into (parse-time
;; domain error). A `tests/lisp_roundtrip.rs` proves the round-trip.

;; DECLARE — k9s `s` (scale) lowers to a FULL HelmRelease values change.
(defk8saction
  :name "scale"
  :keys "s"
  :legality (:class declare
             :target (:rail flux-helm-values :release-path "apps/catch/release.yaml"))
  :manifest-scope full)

;; OBSERVE — always allowed, mutates nothing.
(defk8saction
  :name "view-logs"
  :keys "l"
  :legality (:class observe)
  :manifest-scope full)

;; OBSERVE — describe.
(defk8saction
  :name "describe"
  :keys "d"
  :legality (:class observe)
  :manifest-scope full)

;; BREAK-GLASS — exec is a witnessed, RUNBOOK-logged escape hatch,
;; never a default OBSERVE.
(defk8saction
  :name "shell"
  :keys "S"
  :legality (:class break-glass
             :witness "drzzln"
             :runbook "clusters/rio/RUNBOOK.md")
  :manifest-scope full)

;; *** A plugin that tries to live-mutate is UN-AUTHORABLE:            ***
;; *** DeclareTarget's :rail has no (kubectl …)/(apply …) value.        ***
