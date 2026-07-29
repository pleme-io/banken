;; banken deployment config — the (defbanken) authoring face.
;;
;; This is the DEPLOYMENT/runtime half of banken's single config surface:
;; which context, which namespace, how often to poll, which theme, and
;; WHERE the domain is authored. It deliberately says nothing about views,
;; columns, sort orders or actions — those are the DOMAIN half, authored as
;; (defk8sview …) / (defk8saction …) under `:spec-dir`.
;;
;; Precedent: escriba-config/src/lib.rs — one struct, two faces
;; (`#[tatara(keyword = "defescriba")]` + `impl shikumi::TieredConfig`),
;; where "the `.lisp` remains the load-bearing prescription … this struct
;; is the operator-facing summary".
;;
;; A test (`prescribed_mirrors_the_authored_lisp`) pins this form to
;; `TieredConfig::prescribed_default()`, so the two faces cannot drift.

(defbanken
  ;; Empty ⇒ follow the kubeconfig's own current-context. Prescribing a
  ;; context here would point banken at a cluster nobody selected.
  :context ""
  ;; Empty ⇒ all namespaces (k9s's landing behaviour).
  :namespace ""
  ;; BANKEN.md §VI M0: a 1 Hz POLL. True watch is M1 unbuilt substrate
  ;; (§IX C-watch) — this knob is not renamed until the informer exists.
  :refresh-interval-ms 1000
  ;; The theme selection. A bare string at this milestone; the typed
  ;; ishou_tokens::FleetTheme projection is the next tier up.
  :theme "pleme-dark"
  ;; The log-pager cap, in lines. 0 ⇒ unbounded.
  :scrollback-lines 10000
  ;; *** THE SEAM *** — the one field joining the deployment face to the
  ;; tatara-lisp DOMAIN face. Everything under here is (defk8sview …) /
  ;; (defk8saction …), never a runtime knob.
  :spec-dir "banken-spec/specs")
