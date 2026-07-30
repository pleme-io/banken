;; banken pathology taxonomy — the (defpathology) authoring surface.
;;
;; Each form is one symptom→cause rule the Popeye lane runs as a READ-ONLY
;; linter (BANKEN.md §V): banken suggests, and every remedy routes through a
;; `postigo` rail — never autorevivy's live actuator, never kubectl. That is
;; why `:remedy` has no `kubectl`/`apply` value to author: it reuses
;; DeclareTargetKind, the same five-rail catalog `(defk8saction)` uses.
;;
;; `:evidence` is a CONJUNCTION — every listed signal must read abnormal for
;; the rule to fire. An empty `:evidence` is an ERROR, not a rule that
;; silently never fires (SpecError::PathologyWithoutEvidence).
;;
;; Tier-honest scope: the autorevivy taxonomy is 35 classes (BANKEN.md §II,
;; verified) and Tendril names more. THREE ship here. This file is the
;; vocabulary and the first three instances of it — not the taxonomy.
;; `pending-banken: pathology-taxonomy-backfill` (the remaining 32 need
;; autorevivy's detection stream deployed beyond camelot-shadow first, per
;; §V's ~15% live-data-reuse note; authoring rules whose evidence nothing
;; supplies would be a catalog of rules that can never fire).

;; ── THE MARQUEE CASE (BANKEN.md §V / TENDRIL §II.9) ────────────────────
;;
;; `up==1` with the core metric series ABSENT is a BROKEN SCRAPE — fix the
;; scrape, not the workload. This rule is the DIAGNOSIS: it tells the
;; operator what to fix and how.
;;
;; *** It is NOT the green gate. *** The "no workload shows GREEN unless
;; up==1 ∧ count(core_metric)>0" invariant lives in
;; `WardVerdict::evaluate` as an unconditional structural cap, precisely so
;; that deleting, renaming, or omitting THIS form from a ward's
;; `:pathologies` cannot switch the guard off. A guard that depends on a
;; catalog row is a guard with an off switch.
(defpathology
  :name "broken-scrape"
  :symptom "the target reports up==1 but its core metric series is absent"
  :cause "the scrape config selects the target but the metrics path/port is wrong, so every metric-derived verdict for this workload is blind"
  :severity critical
  :evidence (metric-presence)
  ;; The scrape config is chart values in git — a full-manifest DECLARE.
  :remedy (:kind declare :rail flux-helm-values))

;; ── Pulses-side: a band outside its bounds ─────────────────────────────
;;
;; A breathe band whose phase is not `Holding` is not converging. Reading
;; ANY unhealthy phase (not a specific dimension) is deliberate and coarse
;; in M0 — HealthReading carries `(band-kind, phase)` pairs but the evidence
;; axis cannot yet name a dimension. `pending-banken:
;; per-dimension-band-evidence`.
(defpathology
  :name "band-not-holding"
  :symptom "a breathe band for this workload reports a phase other than Holding"
  :cause "the band is shadowed, stale, conflicted or mid-carve — its setpoint is not being enforced, so the workload's limits are whatever was last written"
  :severity warning
  :evidence (band-phase)
  ;; A band setpoint is a *Band CRD field — the BreatheBand rail.
  :remedy (:kind declare :rail breathe-band))

;; ── The honest C-declare-coverage arm (BANKEN.md §IX) ──────────────────
;;
;; Some live resources have NO owning release.yaml (a bare pod, an
;; operator-created child, a chartless CRD), so DECLARE has no lowering
;; target for them. The `no-remedy` arm says so out loud instead of
;; pretending a rail applies — the same refusal `SpecError::NoLoweringTarget`
;; makes at the interpreter.
(defpathology
  :name "unowned-resource"
  :symptom "the selected resource has no owning HelmRelease, so its declared source cannot be located"
  :cause "it was created imperatively, by an operator/controller, or as a chartless CRD — nothing in git declares it, so there is nothing to change"
  :severity advisory
  :evidence (detection)
  :remedy (:kind no-remedy
           :why "no owning release.yaml exists; the only exits are a witnessed break-glass or adopting the resource into a chart (BANKEN.md §IX C-declare-coverage)"))
