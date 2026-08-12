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
;;
;; :keys is "shift+s", NOT "S". Two independent reasons, both measured:
;;   (1) awase::Hotkey::parse is case-insensitive (awase/src/hotkey.rs:435),
;;       so "S" and "s" are the SAME typed chord — authoring "S" here made
;;       this action collide with `scale` above, silently.
;;   (2) egaku-term already delivers a held-shift letter AS shift+s (see
;;       banken/src/app.rs default_keymap), so "shift+s" is the form the
;;       runtime actually produces; "S" was the value that disagreed.
;; `bindings::build_binding_map` now rejects the collision as an error
;; rather than last-write-wins.
(defk8saction
  :name "shell"
  :keys "shift+s"
  :legality (:class break-glass
             :witness "drzzln"
             :runbook "clusters/bravo/RUNBOOK.md")
  :manifest-scope full)

;; *** A plugin that tries to live-mutate is UN-AUTHORABLE:            ***
;; *** DeclareTarget's :rail has no (kubectl …)/(apply …) value.        ***
