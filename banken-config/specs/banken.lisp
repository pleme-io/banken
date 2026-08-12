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
  ;; Which stance the cluster picker's filter opens in. `"normal"` ⇒ —
  ;; vim-true: `j`/`k` move rows, `i` starts typing, `esc` never leaves.
  ;; `"insert"` ⇒ open typing;, where the screen opens typing.
  ;;
  ;; A real preference with two defensible answers: a chooser's primary act
  ;; IS typing, and a screen that draws a stance badge must honour the
  ;; stance. NORMAL won because the second argument is about honesty and the
  ;; first is about convenience. Authored rather than hardcoded so the answer
  ;; can change without a rebuild of the argument.
  :picker-stance "normal"
  ;; The typed ishou_tokens::FleetTheme, DERIVED from
  ;; FleetDefaults::prescribed() — never a locally-chosen palette. Note the
  ;; snake_case wire name: FleetTheme's serde repr is rename_all =
  ;; "snake_case", and it is deliberately NOT the preset vocabulary (which
  ;; spells this same palette "nord"). An unknown name has no typed value.
  ;;
  ;; This literal is the ONLY hand-written theme value in banken, and it
  ;; exists only because an authored form must be concrete. It must equal
  ;; `FleetDefaults::prescribed().theme`'s wire name; the
  ;; `prescribed_mirrors_the_authored_lisp` test makes a fleet re-theme a
  ;; LOUD failure here instead of silent drift.
  ;;
  ;; Measured 2026-07-29: the PUBLISHED ishou-tokens 0.1.4 (what banken
  ;; actually compiles against) prescribes `vellum`, while the ishou
  ;; working copy has `PlemeDark` unpublished at the SAME version number.
  ;; This file names what the CONSUMED artifact prescribes.
  :theme "vellum"
  ;; The log-pager cap, in lines. 0 ⇒ unbounded. DERIVED from
  ;; FleetDefaults::scrollback_lines — the fleet 10k floor.
  :scrollback-lines 10000
  ;; ── DECLARE's destination ────────────────────────────────────────
  ;; Which repository a DECLARE opens its pull request against, `owner/name`.
  ;;
  ;; EMPTY on purpose, and the refusal is the feature. There is no
  ;; fleet-wide right answer — which repository owns a resource is a fact
  ;; about ONE estate — and a DECLARE written to a guessed repository opens
  ;; a PR that looks correct and reconciles nothing, which costs a
  ;; reviewer's trust rather than merely failing. So the prescribed value is
  ;; "I do not know", and `banken::declare` turns that into a typed refusal
  ;; naming this field. An operator sets it in their own overlay.
  ;;
  ;; This replaced a BANKEN_GITOPS_REPO environment variable — the untyped,
  ;; unauthored, undiscoverable surface the fleet configuration rule exists
  ;; to eliminate.
  :gitops-repo ""
  ;; The branch the pull request targets. Unlike the repository, this one HAS
  ;; a defensible fleet-wide answer, so it gets a real default.
  :gitops-base "main"
  ;; ── The watchdog's two cadences ──────────────────────────────────
  ;; The cheap TCP round: a VPN that comes up while the operator is reading
  ;; the list should be noticed, and a connect to an open port is one round
  ;; trip with no authentication.
  :ronda-round-ms 15000
  ;; The EXPENSIVE climb. Separate knob because the costs differ by two
  ;; orders of magnitude: everything above `network` runs a credential
  ;; helper, which on EKS is an `aws eks get-token` subprocess plus an STS
  ;; round-trip. Five minutes against four reachable contexts is ~48 helper
  ;; invocations an hour; climbing on the cheap clock against all eighteen
  ;; would be ~4300. That ratio is why there are two knobs.
  :ronda-climb-ms 300000
  ;; *** THE SEAM *** — the one field joining the deployment face to the
  ;; tatara-lisp DOMAIN face. Everything under here is (defk8sview …) /
  ;; (defk8saction …), never a runtime knob.
  :spec-dir "banken-spec/specs")
