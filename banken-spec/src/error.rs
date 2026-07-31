//! Errors produced while loading or interpreting banken specs.
//!
//! Mirrors sui-spec's `SpecError` shape exactly (the canonical
//! TYPED-SPEC + INTERPRETER pattern): a typed `Interp { phase }` for
//! every unimplemented or failing interpreter surface, never a
//! `todo!()`/`unimplemented!()`/`panic!()` in a production path. A
//! silent wrong `Ok` is forbidden — an interpreter gap surfaces
//! mechanically as a typed error the consumer can see.

use thiserror::Error;

/// Every failure the postigo triplet can produce.
#[derive(Error, Debug)]
pub enum SpecError {
    /// The Lisp source failed to read, macroexpand, or compile under
    /// a domain's schema.
    #[error("spec load error: {0}")]
    Load(String),

    /// A typed border failed to compile from its authored form.
    #[error("spec compile error: {0}")]
    Compile(String),

    /// An interpreter phase failed. `phase` names the exact step so
    /// a consumer sees the gap without guessing.
    #[error("interpreter error in {phase}: {message}")]
    Interp { phase: String, message: String },

    /// A DECLARE action could not be lowered to a full manifest —
    /// e.g. the selection has no owning `release.yaml` (the
    /// §IX C-declare-coverage gap). Distinct from `Interp` so
    /// callers can offer break-glass or a reviewed rail instead.
    #[error("no DECLARE lowering target: {0}")]
    NoLoweringTarget(String),

    /// Two `(defk8saction)` forms claim the SAME key chord.
    ///
    /// This is an **error, never last-write-wins**: silently keeping the
    /// last binding is how an operator ends up with a chord that fires a
    /// different legality class than the one the legend advertises — the
    /// worst possible failure for a tool whose whole point is that the
    /// operator knows which gate a keystroke crosses.
    #[error(
        "key chord `{chord}` is claimed by both `{existing}` and `{incoming}` \
         — two actions cannot share one chord ({hint})"
    )]
    ChordConflict {
        /// The canonical chord both actions claim.
        chord: String,
        /// The action that claimed it first.
        existing: String,
        /// The action that collided with it.
        incoming: String,
        /// The named fix (usually: author the uppercase form as `shift+…`).
        hint: &'static str,
    },

    /// An awase binding surface was rejected for a reason other than a
    /// straight duplicate — e.g. `awase::detect_conflicts` reported a
    /// chord-leader colliding with a plain binding.
    #[error("keybinding error: {0}")]
    Binding(String),

    // ── (defpathology) ────────────────────────────────────────────────
    /// A `(defpathology)` declared no `:evidence`.
    ///
    /// An evidence-free rule can never be evaluated. Returning "did not
    /// fire" would be the silent-never-fires failure this crate exists to
    /// refuse, so it is an error at evaluation time and the whole verdict
    /// fails rather than optimistically reporting health.
    #[error(
        "pathology `{0}` declares no evidence — it could never fire, which is an authoring error, not a rule"
    )]
    PathologyWithoutEvidence(String),

    // ── (defdrill) ────────────────────────────────────────────────────
    /// A `(defdrill)` declared no `:steps` — a dead Enter key that looks
    /// authored.
    #[error("drill `{0}` has no steps")]
    EmptyDrill(String),

    /// Two consecutive drill steps do not strictly descend the hierarchy —
    /// a path that zooms out, revisits a rung, or chains two terminals.
    #[error(
        "drill `{drill}` does not descend: step `{from_level}` is followed by \
         `{to_level}`, which is not deeper — a drill descends, never ascends \
         or repeats"
    )]
    NonDescendingDrill {
        /// The offending drill's name.
        drill: String,
        /// The earlier step's level label.
        from_level: &'static str,
        /// The later step's level label.
        to_level: &'static str,
    },

    // ── (defbancada) ──────────────────────────────────────────────────
    /// A `(defbancada)` declared no `:panes` — a chord that looks authored
    /// and opens nothing.
    #[error("bancada `{0}` has no panes — it would open an empty session")]
    EmptyBancada(String),

    /// A `(defbancada)`'s panes are not a valid session shape: the first pane
    /// is not `root`, or a later pane claims to be.
    #[error("bancada `{bancada}` has an invalid pane layout: {detail}")]
    BancadaPlacement {
        /// The offending recipe.
        bancada: String,
        /// What specifically is wrong, and why it cannot be a session.
        detail: &'static str,
    },

    /// A `(defbancada)` stages a MUTATING command but declares no
    /// `:witness` / `:runbook`.
    ///
    /// Staging `kubectl exec` into a pane the operator is dropped into is the
    /// live-effect path whatever it is called, so it carries the same
    /// BREAK-GLASS obligation as a `(defk8saction)` that does. The recipe's
    /// legality is *derived* from its panes, so this is the only way the
    /// obligation can go unmet — it cannot be dodged by mislabelling.
    #[error(
        "bancada `{bancada}` stages a MUTATING command in its `{pane_role}` pane, \
         which makes the whole recipe BREAK-GLASS — declare `:witness` and \
         `:runbook`, or make every pane observe-only"
    )]
    UnwitnessedBancada {
        /// The offending recipe.
        bancada: String,
        /// The role of the first mutating pane.
        pane_role: &'static str,
    },

    /// A `(defbancada)` of pure observers carries a `:witness` / `:runbook`
    /// anyway.
    ///
    /// Rejected in this direction too: a stray witness reads as "somebody
    /// signed off on a live effect" when there is none, which is an over-claim
    /// pointing the way reviewers do not check.
    #[error(
        "bancada `{0}` declares a `:witness`/`:runbook` but stages no mutating \
         command — a witness on a pure-observe recipe claims a sign-off that \
         nothing needed"
    )]
    UnneededWitness(String),

    /// A `(defbancada)` references a context field the planner cannot resolve.
    ///
    /// **Never** substituted with an empty string: `kubectl --context ""` and
    /// `kubectl -c ""` are silently *wrong* rather than failing, which would
    /// open the pre-warmed session on the wrong cluster or the wrong
    /// container — the exact class the domain exists to close.
    #[error(
        "bancada `{bancada}` references `{field}`, which the current selection \
         does not carry — refusing to substitute an empty value, which would \
         silently resolve to a different target"
    )]
    UnresolvedContextField {
        /// The offending recipe.
        bancada: String,
        /// The unresolvable field's label.
        field: &'static str,
    },

    // ── cross-domain resolution (see `crate::resolve`) ────────────────
    /// A view or ward's `:drill-to` names a `(defdrill)` that does not
    /// exist. Previously a silently dead Enter key.
    #[error("`{surface}` drills to `{drill}`, but no (defdrill) declares that name")]
    DanglingDrill {
        /// The view or ward that declared the drill.
        surface: String,
        /// The name it named.
        drill: String,
    },

    /// A `(defdrill)`'s `:from` names neither a declared view nor a
    /// declared ward.
    #[error(
        "drill `{drill}` starts from `{from}`, which is neither a declared (defk8sview) nor a declared (defward)"
    )]
    UnknownDrillSource {
        /// The drill.
        drill: String,
        /// The surface it claimed to start from.
        from: String,
    },

    /// A `(defward)`'s `:pathologies` names a `(defpathology)` that does
    /// not exist — the linter would silently run one rule fewer.
    #[error("ward `{ward}` runs pathology `{pathology}`, but no (defpathology) declares that name")]
    UnknownPathology {
        /// The ward.
        ward: String,
        /// The name it named.
        pathology: String,
    },

    /// A `(defward)`'s `:view` names a `(defk8sview)` that does not exist.
    #[error("ward `{ward}` augments view `{view}`, but no (defk8sview) declares that name")]
    WardViewMissing {
        /// The ward.
        ward: String,
        /// The view it named.
        view: String,
    },

    /// A `(defward)` augments a view that is not a `HealthWard` — the ward
    /// composition only means anything over the health view kind.
    #[error("ward `{ward}` augments view `{view}`, which is kind `{kind}`, not `health-ward`")]
    WardViewKindMismatch {
        /// The ward.
        ward: String,
        /// The view it named.
        view: String,
        /// The view's actual kind label.
        kind: &'static str,
    },

    /// A `(defward)`'s lanes and its view's non-identity columns disagree.
    ///
    /// Two authored files describing one screen is the drift class the
    /// resolver exists to kill: the view owns the geometry, the ward owns
    /// the signals, and the correspondence between them is checked rather
    /// than hoped for.
    #[error(
        "ward `{ward}` and view `{view}` disagree about the ward's columns: \
         {detail}. The view's columns are [identity, ...lane headers]; fix \
         whichever side is wrong."
    )]
    WardLaneColumnMismatch {
        /// The ward.
        ward: String,
        /// The view it augments.
        view: String,
        /// What specifically differs.
        detail: String,
    },

    /// Two forms on the same axis claim the same name. Names are join keys
    /// (a `drill_to`, a ward's `:pathologies` entry, a detection label), so
    /// a duplicate silently shadows one of the two.
    #[error("two `{axis}` forms are both named `{name}` — names are join keys and must be unique")]
    DuplicateName {
        /// Which axis (`"view"`, `"ward"`, `"pathology"`, `"drill"`, …).
        axis: &'static str,
        /// The duplicated name.
        name: String,
    },
}
