;; banken — the typed SDLC manifest. One file, one source of truth.
;;
;; Substrate-side renderers consume this and emit:
;;   caixa-publish        → release flow + git tag (crates.io for the spec lib)
;;   caixa-publish-tlisp  → tatara-lisp publishing path (the specs/*.lisp surface)
;;   caixa-validate       → CSE invariant check (layout, slot coverage)
;;   caixa-forge          → repo + workflow regeneration
;;
;; Author edits HERE; everything downstream re-derives from these slots.
;;
;; Kind rationale: banken is a Binario (a pure terminal binary that READS
;; clusters and EMITS git changes). It carries no in-cluster runtime, which
;; is why CLAUDE.md declares skip-urdume / skip-helm-native / skip-gitops /
;; skip-magma / skip-continuous-convergence / skip-platform-mediated.

(defcaixa
  :nome        "banken"
  :versao      "0.1.0"
  :kind        Binario
  :edicao      "2026"
  :descricao   "banken 番犬 (watchdog) — the pleme-io-native k9s. An observe-first, GitOps-native cluster-navigator TUI: live resource tables rendered through egaku-term's typed cell surface, with every action routed through the postigo three-class action-legality gate (OBSERVE reads directly, DECLARE lowers to a full-manifest GitOps change a reconciler applies, BREAK-GLASS is witnessed and RUNBOOK-logged). The ClusterEnv trait carries no unwitnessed-mutate method."
  :repositorio "github:pleme-io/banken"
  :licenca     "MIT"
  :autores     ("pleme-io")
  :etiquetas   ("rust" "tatara-lisp" "kubernetes" "k9s" "tui" "quadro"
                "gitops" "observe-first" "postigo" "egaku" "naturalize"
                "caixa-binario")

  ;; Workspace deps that need to vendor at build time.
  :deps        ()
  :deps-dev    ()

  ;; The wired-up binary is what `feira publish` releases.
  :exe         ("banken")

  ;; banken-spec is the downstream-consumable typed surface — the postigo
  ;; action-legality border + the (defk8sview)/(defk8saction) domains + the
  ;; apply<E: ClusterEnv> interpreter. A consumer that wants the legality
  ;; gate without the TUI depends on this alone.
  :bibliotecas ("banken-spec")
  )
