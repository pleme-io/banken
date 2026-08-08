//! `(defbancada)` — the pre-warmed troubleshooting-session domain.
//!
//! **bancada** (pt-BR: a workbench — the surface with your tools already laid
//! out, so work starts the instant you sit down). It is the bridge between
//! banken (which knows
//! *what is broken and where*) and tear/mado (which own *the panes you fix it
//! in*): from a selected row, one authored chord produces a whole tear session
//! — the right panes, already in the right cluster/namespace/resource context,
//! with the troubleshooting commands staged — so the operator is fixing the
//! problem rather than setting up to fix it.
//!
//! # What this domain closes
//!
//! Without it, "open a shell on this pod's node with the logs alongside" is a
//! sequence the operator retypes every time, differently every time, against
//! whatever context their shell happened to be on. Three failure classes live
//! in that gap and all three are addressed here:
//!
//! 1. **The wrong cluster.** A pane opened without an explicit `--context` is
//!    on the kubeconfig's *current* context, which is not necessarily the one
//!    banken is reading. [`plan`] refuses to resolve a
//!    [`ContextField::Cluster`] reference against an empty cluster id
//!    ([`SpecError::UnresolvedContextField`]) rather than emitting an argument
//!    that silently means "some other cluster".
//! 2. **A mutating command smuggled in as setup.** Staging `kubectl exec` into
//!    a pane the operator is dropped into IS the live-effect path, whatever it
//!    is called. See the legality section below.
//! 3. **A recipe that only exists in muscle memory.** A recipe is a
//!    `(defbancada …)` form — a new one is a declaration, not a code change.
//!
//! # The legality is DERIVED, never authored (the load-bearing invariant)
//!
//! [`BancadaSpec`] has **no `:legality` kwarg**. Its
//! [`legality`](BancadaSpec::legality) is computed from its panes: any pane
//! whose [`StagedCommand::effect`] is [`CommandEffect::Mutates`] makes the
//! whole recipe [`ActionLegality::BreakGlass`]; a recipe of pure observers is
//! [`ActionLegality::Observe`]. There is therefore **no field in which a
//! mutating recipe can claim to be an observe** — the mislabelling class has
//! no representation, rather than being caught by review.
//!
//! A BREAK-GLASS recipe must carry `:witness` + `:runbook`, exactly as
//! [`crate::types::ActionLegality::BreakGlass`] demands; omitting them is
//! [`SpecError::UnwitnessedBancada`], reported by name.
//!
//! # And the witness is structural at the seam too
//!
//! [`SessionEnv`] has **exactly one** staging method, [`SessionEnv::stage_witnessed`],
//! and it takes a [`MutatingCommand`] *plus* a [`WitnessedAction`]. A read-only
//! pane is not staged at all: it is *born* as its command, via
//! [`PaneProgram::Observe`], whose payload is an [`ObservedCommand`]. Both
//! newtypes have private fields and are only obtainable from
//! [`PlannedPane::as_observed`] / [`PlannedPane::as_mutating`], which are
//! `Some` exactly on their own [`CommandEffect`]. So "stage a mutating command
//! through the unwitnessed path" is not a rule — there is no unwitnessed
//! staging path, and there is no `ObservedCommand` a mutating pane can produce
//! to reach the spawn one.
//!
//! **This got stronger on 2026-07-31, not merely rearranged.** The seam used to
//! carry a second, *unwitnessed* staging arm (`stage_observed`) and rely on the
//! argument type to keep a mutating command out of it. Now the arm is gone
//! outright: there is one staging method and it demands a witness.
//!
//! Honest tier, per this repo's standing rule: **truly-unrepresentable within
//! this authored surface** (the same qualified tier as [`crate::Catalog`] and
//! [`crate::pathology::WardVerdict`]), *not* a fleet-wide guarantee. An author
//! extending `SessionEnv` with a second, unwitnessed staging method is
//! CI-caught by `tests/substrate_invariant.rs`, exactly like the
//! [`crate::env::ClusterEnv`] re-add case. Do not round either up.
//!
//! # NO SHELL — a staged command is a typed argv, never a shell string
//!
//! [`StagedCommand`] is `program` + `Vec<CommandArg>`, and a [`CommandArg`] is
//! either a literal or a *typed reference into the context*
//! ([`ContextField`]). [`plan`] resolves it to a `Vec<String>` argv. No
//! quoting, no interpolation, no `format!()` of a command line, and nothing in
//! this crate ever spawns anything — the argv is data handed to the seam.
//!
//! # The tear vocabulary is CONSUMED, not re-invented
//!
//! The three typed axes below are deliberately projections of tear's own
//! types, so the adapter is mechanical rather than a translation layer with
//! opinions:
//!
//! | banken | tear (`tear-types`) | `MultiplexerControl` call |
//! |---|---|---|
//! | [`SessionLayout`] | `layout::LayoutKind` (minus `Custom`) | `apply_layout` |
//! | [`PanePlacement::Root`] | — | `new_session_with_source_and_size` |
//! | [`PanePlacement`] (the four splits) | `direction::Direction` | `split_pane` |
//! | [`PaneProgram::Observe`] | the `shell: &str` + `args: &[String]` a pane spawns | `split_pane` / `new_session` |
//! | [`SessionEnv::focus`] | — | `select_pane` |
//!
//! `LayoutKind::Custom` is deliberately absent: "custom" means *whatever the
//! operator arranged by hand*, which is not something a recipe can declare.
//!
//! # Tier-honest status
//!
//! **The plan is SHIPPED and mock-proven. The live handoff is SHIPPED and
//! PROVEN LIVE. The app keystroke that opens one is NOT.** Three different
//! claims — do not collapse them:
//!
//! - Everything in this module — compiling the forms, deriving the legality,
//!   resolving the context, producing the [`SessionPlan`], and walking it
//!   through a [`SessionEnv`] — is exercised against
//!   [`crate::testing::MockSessionEnv`] with **zero side effects**.
//! - The `SessionEnv` implementation over `tear_client::Client` lives in
//!   `banken::tear_session` (feature `tear`) and was **run against a live
//!   `tear-daemon` on 2026-07-30**: a real three-pane session, with the
//!   pre-warmed `kubectl` line asserted on the first pane's rendered grid.
//! - banken's `g` chord resolves and **previews** the plan; the operator's
//!   `enter` confirms it and the app calls [`open`] through its `SessionEnv`
//!   type parameter (`pending-banken: bancada-app-open`, CLOSED 2026-07-31).
//!   Whether that seam is a live daemon, a recording mock, or a build with no
//!   adapter compiled in is the app's choice, not this module's.
//!
//! The upstream limitation that used to shape this seam is **gone** (tear
//! `5974375`, 2026-07-31): `MultiplexerControl::new_session_with_source_and_size`
//! / `split_pane` / `new_window` now carry `args: &[String]` all the way to the
//! `PtyHandle::spawn` that always accepted one. That is why pane creation takes
//! a [`PaneProgram`] — a read pane's argv reaches `execvp` as a vector, with no
//! shell in between and therefore nothing to quote. What remains, and is stated
//! where it lives (`banken::tear_session`), is that the **witnessed** arm still
//! types its argv at a prompt and still refuses to quote, because a mutating
//! command must sit typed-and-unsubmitted until the operator's own Enter.
//! `pending-banken: tear-argv-witnessed-arm`.

use serde::{Deserialize, Serialize};
use tatara_lisp::DeriveTataraDomain;

use crate::{
    chord::ActionChord,
    closed_catalog,
    env::WitnessedAction,
    error::SpecError,
    interp::Selection,
    types::{ActionLegality, K8sActionSpec, ManifestScope, OperatorId, RunbookRef},
};

// ── The closed axes ────────────────────────────────────────────────

closed_catalog! {
    /// What a pane in a pre-warmed session is *for*.
    ///
    /// Semantic, not syntactic: the role is what the operator's eye looks for
    /// ("where are the logs?"), and it is what a future renderer titles the
    /// pane with. It deliberately does NOT imply a command — two recipes may
    /// both have a `Logs` pane running different `kubectl` invocations.
    #[serde(rename_all = "kebab-case")]
    pub enum PaneRole {
        /// A log stream.
        Logs => "logs",
        /// The resource's events.
        Events => "events",
        /// The resource's full description / manifest.
        Describe => "describe",
        /// A live-updating resource table.
        Watch => "watch",
        /// An interactive shell (in the pod, or on the node).
        Shell => "shell",
        /// An editor opened on the GitOps source of truth.
        Editor => "editor",
    }
}

closed_catalog! {
    /// Where a pane sits relative to the one before it.
    ///
    /// [`PanePlacement::Root`] is the session's first pane and has no origin;
    /// the four split directions are `tear_types::direction::Direction`
    /// verbatim, so the adapter is a rename rather than a decision.
    #[serde(rename_all = "kebab-case")]
    pub enum PanePlacement {
        /// The session's first pane. Exactly one pane carries this, and it is
        /// the first — [`BancadaSpec::validate`] refuses anything else.
        Root => "root",
        /// Split to the right of the previous pane (`Direction::Right`).
        Right => "right",
        /// Split below the previous pane (`Direction::Below`).
        Below => "below",
        /// Split to the left of the previous pane (`Direction::Left`).
        Left => "left",
        /// Split above the previous pane (`Direction::Above`).
        Above => "above",
    }
}

closed_catalog! {
    /// The window layout a pre-warmed session settles into — a projection of
    /// `tear_types::layout::LayoutKind`, minus `Custom`.
    #[serde(rename_all = "kebab-case")]
    pub enum SessionLayout {
        /// All panes in one row.
        EvenHorizontal => "even-horizontal",
        /// All panes in one column.
        EvenVertical => "even-vertical",
        /// One large pane on top, the rest along the bottom.
        MainHorizontal => "main-horizontal",
        /// One large pane on the left, the rest stacked right.
        MainVertical => "main-vertical",
        /// An approximate square grid.
        Tiled => "tiled",
    }
}

closed_catalog! {
    /// Whether a staged command reads or has a live effect.
    ///
    /// This is the ONE axis the `postigo` gate reads out of a bancada, and it
    /// is authored per command rather than per recipe — because a recipe is
    /// mixed in practice (three log tails and one `exec`), and the honest
    /// legality of that recipe is the legality of its worst pane.
    #[serde(rename_all = "kebab-case")]
    pub enum CommandEffect {
        /// A read. Mutates nothing in the cluster.
        Observes => "observes",
        /// A live effect (`exec`, `port-forward`, `debug`, `cordon`, …).
        /// Its presence makes the whole recipe BREAK-GLASS.
        Mutates => "mutates",
    }
}

closed_catalog! {
    /// A typed reference into the troubleshooting context, resolved by
    /// [`plan`] against a [`BancadaContext`].
    ///
    /// Closed on purpose: a free-form `${...}` template would let a recipe
    /// name a field the planner has never heard of and get an empty string
    /// for it. An unknown field here has no typed value.
    #[serde(rename_all = "kebab-case")]
    pub enum ContextField {
        /// The kubeconfig context banken is reading — the field that makes
        /// "the RIGHT cluster" true rather than hoped for.
        Cluster => "cluster",
        /// The selected resource's namespace.
        Namespace => "namespace",
        /// The selected resource's kind label (`pod`, `service`, …).
        ResourceKind => "resource-kind",
        /// The selected resource's name.
        ResourceName => "resource-name",
        /// The container inside the selected pod, when one is selected.
        Container => "container",
    }
}

// ── The authored value types ───────────────────────────────────────

/// One argument of a staged command: a literal, or a typed reference into the
/// context.
///
/// Externally tagged + snake_case so the authored Lisp reads
/// `(:literal "logs")` / `(:context resource-name)`.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CommandArg {
    /// A fixed argument (`"logs"`, `"--follow"`, `"-n"`).
    Literal(String),
    /// A reference resolved from the [`BancadaContext`].
    Context(ContextField),
}

/// A command pre-staged into a pane — a typed argv, never a shell string.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct StagedCommand {
    /// The program (`"kubectl"`, `"banken"`, `"nvim"`).
    pub program: String,
    /// Its arguments, in order.
    #[serde(default)]
    pub args: Vec<CommandArg>,
    /// Whether running it reads or mutates. Drives the recipe's derived
    /// legality — see the module docs.
    pub effect: CommandEffect,
}

/// One pane of a pre-warmed session.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct BancadaPane {
    /// What the pane is for.
    pub role: PaneRole,
    /// Where it sits relative to the previous pane.
    pub placement: PanePlacement,
    /// The command staged into it.
    pub command: StagedCommand,
}

/// One authored pre-warmed troubleshooting session.
#[derive(DeriveTataraDomain, Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[tatara(keyword = "defbancada")]
pub struct BancadaSpec {
    /// The recipe's name — a join key, so it must be unique.
    pub name: String,
    /// The chord that opens it. Shares ONE namespace with
    /// `(defk8saction)` and `(defnavkey)`; [`crate::Catalog::resolve`]
    /// conflict-checks all three together.
    pub keys: ActionChord,
    /// The surface it is reachable from: a declared `(defk8sview)` or
    /// `(defward)` name, resolved by [`crate::Catalog::resolve`].
    pub from: String,
    /// The window layout the session settles into.
    pub layout: SessionLayout,
    /// The prefix of the generated session name; the resolved context is
    /// appended so two triage sessions on two pods do not collide.
    pub session_prefix: String,
    /// The panes, in creation order. The first must be
    /// [`PanePlacement::Root`] and no other may be.
    pub panes: Vec<BancadaPane>,
    /// The BREAK-GLASS witness. **Required iff** any pane mutates; a recipe
    /// of pure observers must not carry one (a witness on an observe recipe
    /// reads as "somebody signed off on a live effect" when none exists).
    #[serde(default)]
    pub witness: Option<OperatorId>,
    /// The RUNBOOK the break-glass is logged to. Same requirement as
    /// [`Self::witness`].
    #[serde(default)]
    pub runbook: Option<RunbookRef>,
}

impl BancadaSpec {
    /// `true` when any pane stages a mutating command.
    #[must_use]
    pub fn mutates(&self) -> bool {
        self.panes
            .iter()
            .any(|p| p.command.effect == CommandEffect::Mutates)
    }

    /// The recipe's `postigo` legality — **derived from the panes, never
    /// authored**.
    ///
    /// This is the whole point of the domain's shape: there is no
    /// `:legality` kwarg, so a mutating recipe cannot claim to be an
    /// observe. See the module docs.
    ///
    /// # Errors
    ///
    /// [`SpecError::UnwitnessedBancada`] when the recipe mutates but omits
    /// `:witness` / `:runbook`, and [`SpecError::UnneededWitness`] when a
    /// pure-observe recipe carries one anyway.
    pub fn legality(&self) -> Result<ActionLegality, SpecError> {
        match (self.mutates(), &self.witness, &self.runbook) {
            (true, Some(witness), Some(runbook)) => Ok(ActionLegality::BreakGlass {
                witness: witness.clone(),
                runbook: runbook.clone(),
            }),
            (true, _, _) => Err(SpecError::UnwitnessedBancada {
                bancada: self.name.clone(),
                pane_role: self
                    .panes
                    .iter()
                    .find(|p| p.command.effect == CommandEffect::Mutates)
                    .map_or("<none>", |p| p.role.label()),
            }),
            (false, None, None) => Ok(ActionLegality::Observe),
            (false, _, _) => Err(SpecError::UnneededWitness(self.name.clone())),
        }
    }

    /// Validate the recipe's own shape.
    ///
    /// # Errors
    ///
    /// - [`SpecError::EmptyBancada`] — a recipe with no panes is a chord that
    ///   looks authored and opens nothing.
    /// - [`SpecError::BancadaPlacement`] — the first pane is not
    ///   [`PanePlacement::Root`], or a later pane is.
    /// - whatever [`Self::legality`] rejects.
    pub fn validate(&self) -> Result<(), SpecError> {
        let Some(first) = self.panes.first() else {
            return Err(SpecError::EmptyBancada(self.name.clone()));
        };
        if first.placement != PanePlacement::Root {
            return Err(SpecError::BancadaPlacement {
                bancada: self.name.clone(),
                detail: "the first pane must be placed `root` — a session cannot \
                         begin with a split, there is nothing to split from",
            });
        }
        if self
            .panes
            .iter()
            .skip(1)
            .any(|p| p.placement == PanePlacement::Root)
        {
            return Err(SpecError::BancadaPlacement {
                bancada: self.name.clone(),
                detail: "only the FIRST pane may be placed `root` — a second root \
                         would be a second session, not a split",
            });
        }
        self.legality()?;
        Ok(())
    }

    /// Project this recipe onto the [`K8sActionSpec`] shape **purely so the
    /// one chord-conflict checker sees all three keyed domains in a single
    /// namespace** — the same device [`crate::nav::NavKeySpec::as_chord_claim`]
    /// uses, and never dispatched.
    ///
    /// The legality carried here is the recipe's DERIVED one when it is
    /// well-formed; a malformed recipe is rejected by [`Self::validate`]
    /// before this is ever reached, so the `Observe` fallback is unreachable
    /// in a resolved catalog rather than a quiet default.
    #[must_use]
    pub fn as_chord_claim(&self) -> K8sActionSpec {
        K8sActionSpec {
            name: self.name.clone(),
            keys: self.keys,
            legality: self.legality().unwrap_or(ActionLegality::Observe),
            manifest_scope: ManifestScope::Full,
        }
    }
}

// ── The planning context + the plan ────────────────────────────────

/// What a recipe is resolved against: which cluster banken is reading, which
/// row is selected, and (optionally) which container inside it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BancadaContext {
    /// The kubeconfig context banken is reading. **Empty means unknown**, and
    /// a recipe referencing [`ContextField::Cluster`] is then refused rather
    /// than resolved to a `--context ""` that silently means something else.
    pub cluster: String,
    /// The selected row.
    pub selection: Selection,
    /// The selected container inside the pod, when one is selected.
    pub container: Option<String>,
}

/// One pane of a resolved plan: the role, where it goes, and the fully
/// resolved argv.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedPane {
    /// What the pane is for.
    pub role: PaneRole,
    /// Where it sits.
    pub placement: PanePlacement,
    /// The resolved argv — program first, every [`CommandArg`] resolved.
    pub argv: Vec<String>,
    /// Whether it reads or mutates.
    pub effect: CommandEffect,
}

impl PlannedPane {
    /// This pane as an unwitnessed-stageable command — `Some` **iff** it
    /// observes.
    #[must_use]
    pub fn as_observed(&self) -> Option<ObservedCommand> {
        match self.effect {
            CommandEffect::Observes => Some(ObservedCommand {
                argv: self.argv.clone(),
            }),
            CommandEffect::Mutates => None,
        }
    }

    /// This pane as a witness-requiring command — `Some` **iff** it mutates.
    #[must_use]
    pub fn as_mutating(&self) -> Option<MutatingCommand> {
        match self.effect {
            CommandEffect::Mutates => Some(MutatingCommand {
                argv: self.argv.clone(),
            }),
            CommandEffect::Observes => None,
        }
    }
}

/// A read-only command a pane may be **spawned as**, needing no witness.
///
/// Fields private, and [`PlannedPane::as_observed`] the only constructor, so a
/// mutating pane cannot produce one. That is what makes [`PaneProgram::Observe`]
/// safe to exist at all: a spawned program runs *immediately*, so the only
/// argv that may reach a spawn is one that reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObservedCommand {
    argv: Vec<String>,
}

impl ObservedCommand {
    /// The argv to spawn.
    #[must_use]
    pub fn argv(&self) -> &[String] {
        &self.argv
    }
}

/// A command whose staging is a live effect, and therefore requires a
/// [`WitnessedAction`].
///
/// Fields private, and [`PlannedPane::as_mutating`] the only constructor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MutatingCommand {
    argv: Vec<String>,
}

impl MutatingCommand {
    /// The argv to spawn.
    #[must_use]
    pub fn argv(&self) -> &[String] {
        &self.argv
    }
}

/// A resolved pre-warmed session: the name, the layout, the derived legality,
/// and the panes with their argv.
///
/// **Fields are private and [`plan`] is the only constructor** — the fourth
/// construction seal in this crate, and for the same reason as the other
/// three: `legality` is *derived from* `panes`, so a public-field struct would
/// let a caller hand-build a plan whose stated legality and actual commands
/// disagree, which is precisely the lie the domain exists to prevent.
#[derive(Debug, Clone, PartialEq)]
pub struct SessionPlan {
    session_name: String,
    layout: SessionLayout,
    legality: ActionLegality,
    panes: Vec<PlannedPane>,
}

impl SessionPlan {
    /// The generated session name.
    #[must_use]
    pub fn session_name(&self) -> &str {
        &self.session_name
    }

    /// The window layout.
    #[must_use]
    pub fn layout(&self) -> SessionLayout {
        self.layout
    }

    /// The derived `postigo` legality of the whole session.
    #[must_use]
    pub fn legality(&self) -> &ActionLegality {
        &self.legality
    }

    /// The resolved panes, in creation order.
    #[must_use]
    pub fn panes(&self) -> &[PlannedPane] {
        &self.panes
    }

    /// The `WitnessedAction` this plan carries, when it is BREAK-GLASS.
    ///
    /// The selector names the session, which is what a RUNBOOK entry needs to
    /// be traceable back to what was opened.
    #[must_use]
    pub fn witnessed_action(&self) -> Option<WitnessedAction> {
        match &self.legality {
            ActionLegality::BreakGlass { witness, runbook } => Some(WitnessedAction {
                selector: self.session_name.clone(),
                witness: witness.clone(),
                runbook: runbook.clone(),
            }),
            ActionLegality::Observe | ActionLegality::Declare { .. } => None,
        }
    }
}

// ── The interpreter: spec + context → plan ─────────────────────────

/// Resolve a recipe against a context into a [`SessionPlan`].
///
/// # Errors
///
/// - whatever [`BancadaSpec::validate`] rejects (shape, placement, witness);
/// - [`SpecError::UnresolvedContextField`] when an authored
///   [`ContextField`] has no value in `ctx`. **Never** substituted with an
///   empty string: `kubectl --context "" …` and `kubectl -c "" …` are both
///   silently *wrong* rather than failing, which is the exact class this
///   crate refuses.
pub fn plan(spec: &BancadaSpec, ctx: &BancadaContext) -> Result<SessionPlan, SpecError> {
    spec.validate()?;
    let legality = spec.legality()?;

    let mut panes = Vec::with_capacity(spec.panes.len());
    for pane in &spec.panes {
        let mut argv = Vec::with_capacity(pane.command.args.len() + 1);
        argv.push(pane.command.program.clone());
        for arg in &pane.command.args {
            match arg {
                CommandArg::Literal(s) => argv.push(s.clone()),
                CommandArg::Context(field) => argv.push(resolve_field(spec, ctx, *field)?),
            }
        }
        panes.push(PlannedPane {
            role: pane.role,
            placement: pane.placement,
            argv,
            effect: pane.command.effect,
        });
    }

    Ok(SessionPlan {
        session_name: session_name(spec, ctx),
        layout: spec.layout,
        legality,
        panes,
    })
}

/// Resolve one context field, or refuse by name.
fn resolve_field(
    spec: &BancadaSpec,
    ctx: &BancadaContext,
    field: ContextField,
) -> Result<String, SpecError> {
    let value: Option<String> = match field {
        // An empty cluster id is "unknown", not "the current one" — see the
        // module docs' failure class 1.
        ContextField::Cluster => Some(ctx.cluster.clone()).filter(|c| !c.is_empty()),
        ContextField::Namespace => ctx.selection.namespace.clone().filter(|n| !n.is_empty()),
        ContextField::ResourceKind => Some(ctx.selection.kind.label().to_owned()),
        ContextField::ResourceName => Some(ctx.selection.name.clone()).filter(|n| !n.is_empty()),
        ContextField::Container => ctx.container.clone().filter(|c| !c.is_empty()),
    };
    value.ok_or_else(|| SpecError::UnresolvedContextField {
        bancada: spec.name.clone(),
        field: field.label(),
    })
}

/// `<prefix>-<cluster>-<namespace>-<name>`, skipping the parts the context
/// does not carry.
///
/// Typed emission: assembled by concatenation from typed pieces, never a
/// `format!()` of a template.
fn session_name(spec: &BancadaSpec, ctx: &BancadaContext) -> String {
    let mut s = String::from(&spec.session_prefix);
    for part in [
        Some(ctx.cluster.as_str()).filter(|c| !c.is_empty()),
        ctx.selection.namespace.as_deref().filter(|n| !n.is_empty()),
        Some(ctx.selection.name.as_str()).filter(|n| !n.is_empty()),
    ]
    .into_iter()
    .flatten()
    {
        s.push('-');
        s.push_str(part);
    }
    s
}

// ── The seam ───────────────────────────────────────────────────────

/// A handle to a pane the session env created.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PaneRef(pub u64);

/// What a pane runs **from birth** — the argument every pane-creating method
/// of [`SessionEnv`] takes.
///
/// # Why creation and program are ONE act
///
/// They always were, in the multiplexer. Splitting them into "make a pane
/// running a shell" then "put a command into it" was a workaround for a tear
/// limitation that no longer exists (`MultiplexerControl` could not carry an
/// argv until tear `5974375`), and the workaround had a cost: the argv had to
/// be *typed at a prompt*, which is precisely where shell quoting lives, so a
/// word containing a space or a `{` had no safe path into a pane at all.
///
/// # The two arms are not symmetric, and that asymmetry is the safety property
///
/// [`Self::Observe`] carries an [`ObservedCommand`], which only an *observing*
/// [`PlannedPane`] can produce — so a mutating argv has no way to reach a
/// spawn. That matters more than it looks: a spawned program **runs
/// immediately**, and running a live-effect command the moment the pane appears
/// is exactly what [`SessionEnv::stage_witnessed`] exists to prevent. A
/// mutating pane therefore gets [`Self::Shell`] and its command is typed in
/// without a newline, waiting for the operator's own Enter.
///
/// So the enum is not "spawn or don't" — it is the type-level statement that
/// **only a read can be auto-run**.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaneProgram<'a> {
    /// The pane **is** this read-only command: `argv[0]` is the program and
    /// the rest is its argument vector, handed to the multiplexer as a vector
    /// and never through a shell.
    Observe(&'a ObservedCommand),
    /// The pane is the operator's own interactive shell. Used for a mutating
    /// pane, whose command [`SessionEnv::stage_witnessed`] types in and
    /// deliberately does not submit.
    Shell,
}

/// Abstract IO for the terminal multiplexer a pre-warmed session is opened
/// in — the mockable `Environment` seam of this triplet.
///
/// Shaped as a projection of tear's `MultiplexerControl` (see the module
/// docs' mapping table) so an adapter over `tear_client::Client` is
/// mechanical. Nothing in this crate implements it against a live daemon;
/// [`crate::testing::MockSessionEnv`] is what the tests drive.
///
/// *** There is exactly ONE staging method and it REQUIRES a witness.
///     `stage_witnessed` takes a [`MutatingCommand`] + a [`WitnessedAction`],
///     and [`MutatingCommand`] is constructible only from a [`PlannedPane`]
///     that mutates. A read pane is never staged — it is spawned as its own
///     command via [`PaneProgram::Observe`], whose [`ObservedCommand`] a
///     mutating pane cannot produce. Staging a mutating command unwitnessed is
///     not a forbidden call; there is no method to make it on. ***
pub trait SessionEnv {
    /// Create the session and its first pane, running `program`.
    ///
    /// # Errors
    /// A `SpecError::Interp { phase: "open-session" }` on failure.
    fn open_session(
        &self,
        name: &str,
        layout: SessionLayout,
        program: PaneProgram<'_>,
    ) -> Result<PaneRef, SpecError>;

    /// Split `origin` in the direction `placement` names, returning the new
    /// pane running `program`. `placement` is never [`PanePlacement::Root`] on
    /// this path — the root is [`Self::open_session`]'s job.
    ///
    /// # Errors
    /// A `SpecError::Interp { phase: "split-pane" }` on failure.
    fn split(
        &self,
        origin: PaneRef,
        placement: PanePlacement,
        program: PaneProgram<'_>,
    ) -> Result<PaneRef, SpecError>;

    /// Stage a live-effect command into a pane, against a witness.
    ///
    /// The pane is a [`PaneProgram::Shell`] by construction — see that
    /// variant's docs for why a mutating command is never spawned.
    ///
    /// # Errors
    /// A `SpecError::Interp { phase: "stage-witnessed" }` on failure.
    fn stage_witnessed(
        &self,
        pane: PaneRef,
        cmd: &MutatingCommand,
        witness: &WitnessedAction,
    ) -> Result<(), SpecError>;

    /// Focus a pane (the one the operator lands on).
    ///
    /// # Errors
    /// A `SpecError::Interp { phase: "focus-pane" }` on failure.
    fn focus(&self, pane: PaneRef) -> Result<(), SpecError>;
}

/// Open a planned session against a [`SessionEnv`].
///
/// Walks the plan in order: the root pane opens the session, each subsequent
/// pane splits from the one before it, and each pane is created running the
/// [`PaneProgram`] its [`CommandEffect`] admits — a read pane **is** its
/// command, a mutating pane is a shell whose command is then staged against the
/// witness. The operator lands on the **last** pane — the deepest one, which is
/// where the recipe's author put the thing to act on.
///
/// # A read pane is spawned, not typed — and that is the whole shape
///
/// The `as_observed()` projection is computed **once**, before the pane exists,
/// and both decisions read it: it is what fills [`PaneProgram::Observe`] and
/// its absence is what selects the witnessed arm. There is no second place to
/// classify the pane, so the two cannot disagree about what a pane is.
///
/// # Errors
///
/// - [`SpecError::Interp`] propagated from the env;
/// - [`SpecError::UnwitnessedBancada`] if a mutating pane is reached on a plan
///   with no witness. Unreachable through [`plan`] (the legality is derived
///   from the same panes) — it is the honest typed floor rather than an
///   `unwrap`, per this crate's no-silent-`Ok` rule.
pub fn open<E: SessionEnv>(plan: &SessionPlan, env: &E) -> Result<Vec<PaneRef>, SpecError> {
    let witness = plan.witnessed_action();
    let mut refs: Vec<PaneRef> = Vec::with_capacity(plan.panes.len());

    for pane in plan.panes() {
        let observed = pane.as_observed();
        let program = match observed.as_ref() {
            Some(cmd) => PaneProgram::Observe(cmd),
            None => PaneProgram::Shell,
        };

        let handle = match pane.placement {
            PanePlacement::Root => env.open_session(plan.session_name(), plan.layout(), program)?,
            split => {
                let origin = *refs.last().ok_or_else(|| SpecError::Interp {
                    phase: "split-pane".into(),
                    message: "a split pane has no origin — the plan's first pane was not root"
                        .into(),
                })?;
                env.split(origin, split, program)?
            }
        };

        if observed.is_none() {
            // Not observed ⇒ mutating: the two projections partition
            // CommandEffect. The `ok_or_else` is the honest typed floor for a
            // case `plan` cannot construct, never an `unwrap`.
            let mutating = pane.as_mutating().ok_or_else(|| SpecError::Interp {
                phase: "stage".into(),
                message: "a planned pane projected to neither an observed nor a \
                          mutating command"
                    .into(),
            })?;
            let w = witness
                .as_ref()
                .ok_or_else(|| SpecError::UnwitnessedBancada {
                    bancada: plan.session_name().to_owned(),
                    pane_role: pane.role.label(),
                })?;
            env.stage_witnessed(handle, &mutating, w)?;
        }
        refs.push(handle);
    }

    if let Some(last) = refs.last() {
        env.focus(*last)?;
    }
    Ok(refs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{testing::MockSessionEnv, types::ResourceKind};

    fn selection() -> Selection {
        Selection {
            kind: ResourceKind::Pod,
            name: "catch-7d9f".into(),
            namespace: Some("catch".into()),
            current: Vec::new(),
        }
    }

    fn ctx() -> BancadaContext {
        BancadaContext {
            cluster: "camelot-eks".into(),
            selection: selection(),
            container: Some("catch".into()),
        }
    }

    fn logs_pane(placement: PanePlacement) -> BancadaPane {
        BancadaPane {
            role: PaneRole::Logs,
            placement,
            command: StagedCommand {
                program: "kubectl".into(),
                args: vec![
                    CommandArg::Literal("--context".into()),
                    CommandArg::Context(ContextField::Cluster),
                    CommandArg::Literal("-n".into()),
                    CommandArg::Context(ContextField::Namespace),
                    CommandArg::Literal("logs".into()),
                    CommandArg::Literal("-f".into()),
                    CommandArg::Context(ContextField::ResourceName),
                ],
                effect: CommandEffect::Observes,
            },
        }
    }

    fn exec_pane(placement: PanePlacement) -> BancadaPane {
        BancadaPane {
            role: PaneRole::Shell,
            placement,
            command: StagedCommand {
                program: "kubectl".into(),
                args: vec![
                    CommandArg::Literal("exec".into()),
                    CommandArg::Literal("-it".into()),
                    CommandArg::Context(ContextField::ResourceName),
                ],
                effect: CommandEffect::Mutates,
            },
        }
    }

    fn observe_recipe() -> BancadaSpec {
        BancadaSpec {
            name: "pod-triage".into(),
            keys: ActionChord::parse("g").expect("chord"),
            from: "pods".into(),
            layout: SessionLayout::MainVertical,
            session_prefix: "triage".into(),
            panes: vec![
                logs_pane(PanePlacement::Root),
                logs_pane(PanePlacement::Right),
            ],
            witness: None,
            runbook: None,
        }
    }

    fn glass_recipe() -> BancadaSpec {
        BancadaSpec {
            name: "pod-break-glass".into(),
            keys: ActionChord::parse("shift+g").expect("chord"),
            from: "pods".into(),
            layout: SessionLayout::MainHorizontal,
            session_prefix: "glass".into(),
            panes: vec![
                logs_pane(PanePlacement::Root),
                exec_pane(PanePlacement::Below),
            ],
            witness: Some(OperatorId::new("drzzln").expect("a literal witness is non-blank")),
            runbook: Some(RunbookRef("clusters/rio/RUNBOOK.md".into())),
        }
    }

    // ── The legality derivation ────────────────────────────────────

    /// **THE GATE.** A recipe that stages a mutating command IS break-glass,
    /// and one that only reads is not. There is no `:legality` kwarg for
    /// either to lie in — this is the derivation, asserted.
    #[test]
    fn legality_is_derived_from_the_panes_not_authored() {
        assert_eq!(
            observe_recipe().legality().expect("observe recipe"),
            ActionLegality::Observe,
        );
        match glass_recipe().legality().expect("glass recipe") {
            ActionLegality::BreakGlass { witness, runbook } => {
                assert_eq!(witness.as_str(), "drzzln");
                assert!(runbook.0.contains("RUNBOOK"));
            }
            other => panic!("a mutating recipe must be BREAK-GLASS, got {other:?}"),
        }
        // And the derivation reads the PANES: flipping one pane's effect
        // flips the class, with nothing else changed.
        let mut flipped = observe_recipe();
        flipped.panes[1].command.effect = CommandEffect::Mutates;
        flipped.witness = Some(OperatorId::new("drzzln").expect("a literal witness is non-blank"));
        flipped.runbook = Some(RunbookRef("R.md".into()));
        assert_eq!(
            flipped.legality().expect("flipped").class(),
            crate::types::LegalityClass::BreakGlass,
        );
    }

    /// **THE GATE.** A mutating recipe with no witness is refused, by name.
    #[test]
    fn a_mutating_recipe_without_a_witness_is_rejected() {
        let mut r = glass_recipe();
        r.witness = None;
        let err = r
            .validate()
            .expect_err("an unwitnessed break-glass must be rejected");
        match err {
            SpecError::UnwitnessedBancada { bancada, pane_role } => {
                assert_eq!(bancada, "pod-break-glass");
                assert_eq!(pane_role, "shell", "the error names the offending pane");
            }
            other => panic!("expected UnwitnessedBancada, got {other:?}"),
        }
    }

    /// The converse: a witness on a pure-observe recipe is ALSO rejected. A
    /// stray witness reads as "somebody signed off on a live effect" when
    /// there is none — an over-claim in the direction reviewers never check.
    #[test]
    fn a_witness_on_a_pure_observe_recipe_is_rejected() {
        let mut r = observe_recipe();
        r.witness = Some(OperatorId::new("drzzln").expect("a literal witness is non-blank"));
        r.runbook = Some(RunbookRef("R.md".into()));
        assert!(matches!(r.validate(), Err(SpecError::UnneededWitness(n)) if n == "pod-triage"),);
    }

    // ── Shape validation ───────────────────────────────────────────

    #[test]
    fn a_recipe_with_no_panes_is_rejected() {
        let mut r = observe_recipe();
        r.panes.clear();
        assert!(matches!(r.validate(), Err(SpecError::EmptyBancada(n)) if n == "pod-triage"));
    }

    #[test]
    fn a_session_cannot_begin_with_a_split() {
        let mut r = observe_recipe();
        r.panes[0].placement = PanePlacement::Right;
        let err = r.validate().expect_err("a leading split must be rejected");
        assert!(
            matches!(&err, SpecError::BancadaPlacement { detail, .. } if detail.contains("root")),
            "got: {err}"
        );
    }

    #[test]
    fn a_second_root_pane_is_rejected() {
        let mut r = observe_recipe();
        r.panes[1].placement = PanePlacement::Root;
        assert!(matches!(
            r.validate(),
            Err(SpecError::BancadaPlacement { .. })
        ));
    }

    // ── Planning ───────────────────────────────────────────────────

    /// **THE GATE.** The plan is the pre-warming: the right cluster, the
    /// right namespace, the right resource, already on the command line.
    #[test]
    fn the_plan_resolves_the_context_into_the_argv() {
        let p = plan(&observe_recipe(), &ctx()).expect("plans");
        assert_eq!(p.session_name(), "triage-camelot-eks-catch-catch-7d9f");
        assert_eq!(p.layout(), SessionLayout::MainVertical);
        assert_eq!(p.panes().len(), 2);
        assert_eq!(
            p.panes()[0].argv,
            vec![
                "kubectl",
                "--context",
                "camelot-eks",
                "-n",
                "catch",
                "logs",
                "-f",
                "catch-7d9f",
            ],
        );
        assert_eq!(p.panes()[0].placement, PanePlacement::Root);
        assert_eq!(p.panes()[1].placement, PanePlacement::Right);
    }

    /// **THE GATE.** banken reading cluster A must never open a pane that
    /// silently lands on the kubeconfig's cluster B. An unknown cluster is a
    /// refusal, not an empty `--context`.
    #[test]
    fn an_unknown_cluster_is_refused_rather_than_emitted_empty() {
        let mut c = ctx();
        c.cluster = String::new();
        let err = plan(&observe_recipe(), &c).expect_err("an unknown cluster must be refused");
        match err {
            SpecError::UnresolvedContextField { bancada, field } => {
                assert_eq!(bancada, "pod-triage");
                assert_eq!(field, "cluster");
            }
            other => panic!("expected UnresolvedContextField, got {other:?}"),
        }
    }

    #[test]
    fn an_absent_namespace_is_refused_by_name() {
        let mut c = ctx();
        c.selection.namespace = None;
        assert!(matches!(
            plan(&observe_recipe(), &c),
            Err(SpecError::UnresolvedContextField {
                field: "namespace",
                ..
            })
        ));
    }

    #[test]
    fn an_absent_container_is_refused_by_name() {
        let mut r = observe_recipe();
        r.panes[1].command.args = vec![CommandArg::Context(ContextField::Container)];
        let mut c = ctx();
        c.container = None;
        assert!(matches!(
            plan(&r, &c),
            Err(SpecError::UnresolvedContextField {
                field: "container",
                ..
            })
        ));
    }

    // ── The staging-projection seal ────────────────────────────────

    /// **THE SEAL.** A mutating pane has NO `ObservedCommand` value, so the
    /// unwitnessed staging arm has no argument that can reach it.
    #[test]
    fn a_mutating_pane_cannot_produce_an_unwitnessed_command() {
        let p = plan(&glass_recipe(), &ctx()).expect("plans");
        let shell = p
            .panes()
            .iter()
            .find(|x| x.role == PaneRole::Shell)
            .expect("the shell pane");
        assert!(
            shell.as_observed().is_none(),
            "a mutating pane must not project to an ObservedCommand",
        );
        assert!(shell.as_mutating().is_some());

        let logs = p
            .panes()
            .iter()
            .find(|x| x.role == PaneRole::Logs)
            .expect("the logs pane");
        assert!(logs.as_observed().is_some());
        assert!(
            logs.as_mutating().is_none(),
            "an observing pane must not project to a MutatingCommand",
        );
    }

    // ── Opening against the mock seam ──────────────────────────────

    #[test]
    fn opening_an_observe_recipe_spawns_every_pane_and_witnesses_nothing() {
        let p = plan(&observe_recipe(), &ctx()).expect("plans");
        let env = MockSessionEnv::new();
        let refs = open(&p, &env).expect("opens");

        assert_eq!(refs.len(), 2);
        assert_eq!(
            env.sessions.borrow().as_slice(),
            &[(
                "triage-camelot-eks-catch-catch-7d9f".to_string(),
                SessionLayout::MainVertical
            )],
        );
        assert_eq!(
            env.splits.borrow().as_slice(),
            &[(refs[0], PanePlacement::Right)],
        );
        assert_eq!(
            env.witnessed_count(),
            0,
            "an observe recipe witnesses nothing"
        );
        assert_eq!(env.spawned.borrow().len(), 2);
        assert!(
            env.shells.borrow().is_empty(),
            "an observe recipe needs no shell — every pane IS its command",
        );
        assert_eq!(env.focused.borrow().as_slice(), &[refs[1]]);
    }

    /// **THE GATE.** Every mutating pane of a break-glass recipe goes through
    /// the WITNESSED arm, carrying the authored witness + runbook.
    #[test]
    fn opening_a_break_glass_recipe_witnesses_the_mutating_pane() {
        let p = plan(&glass_recipe(), &ctx()).expect("plans");
        let env = MockSessionEnv::new();
        open(&p, &env).expect("opens");

        let spawned = env.spawned.borrow();
        assert_eq!(
            spawned.len(),
            1,
            "only the logs pane is spawned as its argv"
        );
        // *** THE SAFETY PROPERTY, checked rather than asserted in prose. ***
        // The mutating pane got a SHELL, so its command was typed and left
        // unsubmitted. Had it been spawned, `kubectl exec` would have run the
        // instant the pane appeared — with the witness recorded but the
        // operator's own Enter skipped.
        let shells = env.shells.borrow();
        assert_eq!(shells.len(), 1, "the mutating pane is a shell, not a spawn");
        assert!(
            !spawned.iter().any(|(pane, _)| *pane == shells[0]),
            "a mutating pane must never appear in the spawn log",
        );
        let witnessed = env.witnessed.borrow();
        assert_eq!(witnessed.len(), 1);
        let (_, argv, action) = &witnessed[0];
        assert_eq!(argv[0], "kubectl");
        assert!(argv.contains(&"exec".to_string()));
        assert_eq!(action.witness.as_str(), "drzzln");
        assert!(action.runbook.0.contains("RUNBOOK"));
        assert_eq!(
            action.selector, "glass-camelot-eks-catch-catch-7d9f",
            "the RUNBOOK entry names the session that was opened",
        );
    }

    /// The plan is a construction seal: `SessionPlan`'s fields are private and
    /// [`plan`] is its only constructor, so its `legality` cannot disagree
    /// with its `panes`.
    #[test]
    fn a_plans_legality_always_matches_its_panes() {
        for spec in [observe_recipe(), glass_recipe()] {
            let p = plan(&spec, &ctx()).expect("plans");
            let any_mutating = p.panes().iter().any(|x| x.effect == CommandEffect::Mutates);
            assert_eq!(
                any_mutating,
                matches!(p.legality(), ActionLegality::BreakGlass { .. }),
                "the plan's legality must be the panes' legality",
            );
            assert_eq!(any_mutating, p.witnessed_action().is_some());
        }
    }

    #[test]
    fn the_chord_claim_carries_the_derived_legality() {
        let claim = glass_recipe().as_chord_claim();
        assert_eq!(claim.keys, ActionChord::parse("shift+g").expect("chord"));
        assert_eq!(
            claim.legality.class(),
            crate::types::LegalityClass::BreakGlass
        );
    }
}
