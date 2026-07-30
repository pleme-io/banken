;; banken health wards — the (defward) authoring surface (BANKEN.md §V).
;;
;; A ward is the health LANDING: Pulses (breathe band phases per workload),
;; Popeye (the pathology linter, read-only), and the QUIET headline. It does
;; NOT re-declare the screen's columns — it names the (defk8sview) it
;; augments, and `Catalog::resolve` asserts that view's non-identity columns
;; are exactly these lanes, in order. The view owns the geometry, the ward
;; owns the signals, and the correspondence is checked rather than hoped for
;; (SpecError::WardLaneColumnMismatch).
;;
;; *** `:attested` may only be `computed`. ***
;; `(:kind proven :outcome-chain "…")` is PARSE-TIME REJECTED: no fleet
;; PromessaController and no real OutcomeChain exist in code (autorevivy's
;; attest() is a placeholder), so BANKEN.md §V/§IX C-controller forbids the
;; label. The refusal is structural — OutcomeChainRef has no Deserialize
;; that succeeds — not a comment asking an author to be careful.

(defward
  :name "ward"
  ;; The (defk8sview :kind HealthWard) this composes over.
  :view "ward"
  :scope fleet
  ;; The lanes, in render order. These MUST equal the `ward` view's columns
  ;; after the identity column (WORKLOAD) — MEM, CPU, STATUS.
  ;;
  ;; Per BANKEN.md §V a band lane shows a PER-DIMENSION phase, never one
  ;; joined verdict: the composed BreathePlan is DESIGN, so a single "bands
  ;; are fine" lane would be an over-claim. Hence one lane per dimension.
  :lanes ((:header "MEM" :signal (:kind band :dimension memory))
          (:header "CPU" :signal (:kind band :dimension cpu))
          ;; The one lane whose GREEN is gated by the BROKEN-METRIC guard —
          ;; it renders the composed WardVerdict, which cannot be Green over
          ;; an absent core metric.
          (:header "STATUS" :signal (:kind verdict)))
  ;; The Popeye linter set. Every name must resolve to a (defpathology) —
  ;; a dangling reference is rejected, never a rule silently not run.
  :pathologies ("broken-scrape" "band-not-holding" "unowned-resource")
  ;; "QUIET: n/m (computed)" — labelled (computed), never (proven).
  :headline (:label "QUIET" :attested (:kind computed))
  ;; Enter on a red ward row walks the symptom→cause path.
  :drill-to "diagnose")
