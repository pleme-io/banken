//! `(defward)` — the health-ward composition domain (BANKEN.md §V).
//!
//! # What this domain is
//!
//! BANKEN.md §V makes the health `ward` banken's *first-class default
//! landing*: Pulses (breathe band phases per workload), Popeye (the
//! autorevivy detection stream as a read-only linter), the QUIET headline,
//! and the BROKEN-METRIC guard. A `(defk8sview :kind HealthWard)` can
//! express the ward's **geometry** (its columns, its sort) and nothing else
//! — which signal feeds which lane, which pathologies the linter runs, and
//! how the headline is labelled were all unexpressible. `(defward …)` is
//! that composition.
//!
//! Third-use test: the lane is the multi-instance unit (four breathe
//! dimensions + presence + detections + the composed verdict = seven lane
//! shapes today), and [`WardScope`] admits the fleet / namespace / node
//! wards §V's drill-flow implies. The form earns its keep on lanes alone.
//!
//! # A ward is NOT a second view — the correspondence is checked
//!
//! `(defward)` deliberately does **not** re-declare the ward's columns. It
//! names the `(defk8sview)` it augments, and
//! [`crate::resolve::resolve_catalog`] asserts that the view's non-identity
//! column headers are exactly the ward's lane headers, in order. Two
//! authored files describing one screen is precisely the drift class this
//! repo's type-strict-modeling rule exists to kill; here the module system
//! cannot *derive* one from the other (they live in different domains), so
//! the honest tier is **eval-caught by the resolver**, and
//! [`crate::resolve`] carries the gate.
//!
//! # `Attestation::Proven` is UN-AUTHORABLE, by construction
//!
//! BANKEN.md §V and §IX C-controller are explicit and repeated: the QUIET
//! headline is labelled **"(computed)"**, *never* "(proven)", until a fleet
//! `PromessaController` + a real OutcomeChain ship — and neither exists in
//! code (autorevivy's `attest()` is a placeholder string). A doc sentence
//! saying "do not round this up" is reviewer discipline. So instead:
//! [`Attestation::Proven`] carries an [`OutcomeChainRef`] whose
//! `Deserialize` impl **always fails**, naming the ceiling. An authored
//! `:attested (:kind proven …)` is therefore **parse-time-rejected** — the
//! over-claim has no typed value.
//!
//! Per ★★ MODULARIZE, DON'T DELETE the variant *stays*: it is the
//! destination, and reviving it when the controller ships is a one-line
//! change (make [`OutcomeChainRef::from_outcome_chain`] `pub`), not a
//! rebuild from memory.

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use tatara_lisp::DeriveTataraDomain;

use crate::closed_catalog;

closed_catalog! {
    /// A breathe band dimension a Pulses lane can render.
    ///
    /// The four **k8s-plane** dimensions whose `BandStatus.phase` BANKEN.md
    /// §II verifies as SHIPPED data (`MemoryBand`/`CpuBand`/`StorageBand`/
    /// `ReplicaBand`). breathe itself declares eleven dimensions including
    /// host-plane and generic-CR ones; banken authors only the four it has
    /// evidence for, rather than mirroring a catalog it cannot read.
    /// `pending-banken: host-plane-band-lanes`.
    #[serde(rename_all = "kebab-case")]
    pub enum BandDimension {
        /// `MemoryBand`.
        Memory => "memory",
        /// `CpuBand`.
        Cpu => "cpu",
        /// `StorageBand`.
        Storage => "storage",
        /// `ReplicaBand`.
        Replica => "replica",
    }
}

impl BandDimension {
    /// The breathe CRD kind name this dimension's phase is read from — the
    /// key `HealthReading::band_phases` is expected to carry.
    #[must_use]
    pub fn band_kind(self) -> &'static str {
        match self {
            BandDimension::Memory => "MemoryBand",
            BandDimension::Cpu => "CpuBand",
            BandDimension::Storage => "StorageBand",
            BandDimension::Replica => "ReplicaBand",
        }
    }
}

closed_catalog! {
    /// Coarse discriminant of a [`LaneSignal`], for catalog reflection.
    #[serde(rename_all = "kebab-case")]
    pub enum LaneSignalKind {
        /// [`LaneSignal::Band`].
        Band => "band",
        /// [`LaneSignal::MetricPresence`].
        MetricPresence => "metric-presence",
        /// [`LaneSignal::Detections`].
        Detections => "detections",
        /// [`LaneSignal::Verdict`].
        Verdict => "verdict",
    }
}

/// What one ward lane renders.
///
/// Internally tagged on `kind` (matching [`Remedy`](crate::pathology::Remedy)
/// and [`Attestation`] — one discriminant keyword across the vocabulary, not
/// three) so an authored `:signal (:kind band :dimension memory)`
/// round-trips.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum LaneSignal {
    /// One breathe band dimension's phase.
    ///
    /// Per BANKEN.md §V a lane shows a **per-dimension phase, not one
    /// joined verdict** — the composed `BreathePlan` is DESIGN, so a
    /// single-verdict band lane would be an over-claim. That is why this
    /// arm carries a dimension rather than being a bare `Bands`.
    Band {
        /// Which dimension.
        dimension: BandDimension,
    },
    /// The core-metric presence guard (`up==1 ∧ count(metric)>0`).
    MetricPresence,
    /// The autorevivy detection labels that fired.
    Detections,
    /// The composed [`crate::pathology::WardVerdict`] — the only lane whose
    /// green state is gated by the BROKEN-METRIC guard.
    Verdict,
}

impl LaneSignal {
    /// Project this signal to its coarse kind.
    #[must_use]
    pub fn kind(&self) -> LaneSignalKind {
        match self {
            LaneSignal::Band { .. } => LaneSignalKind::Band,
            LaneSignal::MetricPresence => LaneSignalKind::MetricPresence,
            LaneSignal::Detections => LaneSignalKind::Detections,
            LaneSignal::Verdict => LaneSignalKind::Verdict,
        }
    }
}

/// One ward lane: a header plus the signal that fills it.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct LaneSpec {
    /// The lane's header — must match the augmented view's corresponding
    /// column header (checked by [`crate::resolve::resolve_catalog`]).
    pub header: String,
    /// The signal rendered in this lane.
    pub signal: LaneSignal,
}

closed_catalog! {
    /// What a ward is scoped to.
    #[serde(rename_all = "kebab-case")]
    pub enum WardScope {
        /// Every workload the current context exposes.
        Fleet => "fleet",
        /// One namespace's workloads.
        Namespace => "namespace",
        /// One node's workloads.
        Node => "node",
    }
}

/// A reference to a Viggy OutcomeChain — the witness an *attested* QUIET
/// headline would need.
///
/// **This type cannot be deserialized.** [`Self::deserialize`] always
/// fails, naming BANKEN.md §IX C-controller, because no fleet
/// `PromessaController` and no real OutcomeChain exist in code. That makes
/// [`Attestation::Proven`] un-authorable from Lisp or YAML —
/// **parse-time-rejected**, not a doc note asking reviewers to be careful.
///
/// It serializes normally, so a value constructed in Rust the day the
/// controller ships round-trips *out*. Reviving the arm is one line: make
/// [`Self::from_outcome_chain`] `pub`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct OutcomeChainRef(String);

/// The error an attempted `(:kind proven …)` authoring produces. Named so
/// the failure site teaches the ceiling rather than just refusing.
pub const UNATTESTABLE: &str = "an attested (proven) QUIET headline is not authorable: no fleet \
     PromessaController and no real OutcomeChain exist in code (autorevivy's \
     attest() is a placeholder). BANKEN.md §V/§IX C-controller: the headline \
     is labelled (computed), never (proven), until that substrate ships. \
     Author `:attested (:kind computed)`.";

impl OutcomeChainRef {
    /// Build a reference from a chain id.
    ///
    /// Deliberately **crate-private**: outside this crate
    /// [`Attestation::Proven`] is unconstructible, so no consumer can
    /// declare a proven headline in Rust either. Make this `pub` when a
    /// real OutcomeChain ships — that one keyword is the whole revival.
    ///
    /// `allow(dead_code)`: only the in-crate tests call it today, and that
    /// is the *point* — ★★ MODULARIZE, DON'T DELETE keeps the destination
    /// declaration alive rather than deleting the arm and re-deriving it
    /// later. Removing the constructor to silence the lint would delete the
    /// revival path.
    #[must_use]
    #[allow(dead_code)]
    pub(crate) fn from_outcome_chain(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// The referenced chain id.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.0
    }
}

impl Serialize for OutcomeChainRef {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for OutcomeChainRef {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        // Consume the input first so the error the caller sees is OURS
        // (naming the ceiling) rather than a confusing type mismatch from
        // an un-consumed deserializer.
        let _ = serde::de::IgnoredAny::deserialize(d)?;
        Err(serde::de::Error::custom(UNATTESTABLE))
    }
}

closed_catalog! {
    /// Coarse discriminant of an [`Attestation`], for catalog reflection.
    ///
    /// `Proven` appears here — the catalog describes the *universe*,
    /// including the destination arm no authored form can reach today. What
    /// is sealed is authoring, not the vocabulary.
    #[serde(rename_all = "kebab-case")]
    pub enum AttestationKind {
        /// [`Attestation::Computed`] — the only authorable arm today.
        Computed => "computed",
        /// [`Attestation::Proven`] — DESTINATION, un-authorable
        /// (see [`UNATTESTABLE`]).
        Proven => "proven",
    }
}

/// How much confidence the headline claims.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Attestation {
    /// Computed live from band phases + detections + the presence guard.
    /// Advisory reading (BANKEN.md §IX C-external-world), never a theorem.
    Computed,
    /// Attested on a real OutcomeChain by a fleet `PromessaController`.
    ///
    /// **Un-authorable today** — [`OutcomeChainRef`] refuses to
    /// deserialize. ASPIRATIONAL, kept per ★★ MODULARIZE, DON'T DELETE.
    Proven {
        /// The attesting chain.
        ///
        /// camelCase on the wire, matching the kebab→camel key mapping the
        /// tatara derive applies to *nested* values (the same reason
        /// `DeclareTarget::FluxHelmValues` renames `release_path` to
        /// `releasePath`). Without it an authored
        /// `:attested (:kind proven :outcome-chain "…")` would be rejected
        /// with a bland "missing field" instead of [`UNATTESTABLE`], and the
        /// failure site would teach nothing.
        #[serde(rename = "outcomeChain")]
        outcome_chain: OutcomeChainRef,
    },
}

impl Attestation {
    /// Project this attestation to its coarse kind.
    #[must_use]
    pub fn kind(&self) -> AttestationKind {
        match self {
            Attestation::Computed => AttestationKind::Computed,
            Attestation::Proven { .. } => AttestationKind::Proven,
        }
    }
}

/// The ward's top-level headline (BANKEN.md §V's `QUIET: 34/37` line).
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct HeadlineSpec {
    /// The headline's label (`"QUIET"`).
    pub label: String,
    /// How much confidence it claims.
    pub attested: Attestation,
}

/// One authored health ward — the composition BANKEN.md §V describes.
#[derive(DeriveTataraDomain, Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[tatara(keyword = "defward")]
pub struct WardSpec {
    /// The ward's name.
    pub name: String,
    /// The `(defk8sview)` this ward augments. Must name a view of kind
    /// `HealthWard`, and that view's non-identity columns must be exactly
    /// this ward's lane headers — both checked by
    /// [`crate::resolve::resolve_catalog`].
    pub view: String,
    /// What the ward covers.
    pub scope: WardScope,
    /// The Pulses lanes, in render order.
    pub lanes: Vec<LaneSpec>,
    /// The `(defpathology)` names the Popeye linter runs for this ward.
    /// Every name must resolve — a dangling reference is caught by the
    /// resolver, never silently skipped.
    pub pathologies: Vec<String>,
    /// The headline.
    pub headline: HeadlineSpec,
    /// The `(defdrill)` a red ward row drills into.
    #[serde(default)]
    pub drill_to: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn band_dimensions_name_the_breathe_crd_kinds() {
        assert_eq!(BandDimension::Memory.band_kind(), "MemoryBand");
        assert_eq!(BandDimension::Replica.band_kind(), "ReplicaBand");
        assert_eq!(BandDimension::ALL.len(), 4);
    }

    #[test]
    fn lane_signals_project_to_their_kinds() {
        assert_eq!(
            LaneSignal::Band {
                dimension: BandDimension::Cpu
            }
            .kind(),
            LaneSignalKind::Band
        );
        assert_eq!(LaneSignal::Verdict.kind(), LaneSignalKind::Verdict);
        assert_eq!(LaneSignalKind::ALL.len(), 4);
    }

    #[test]
    fn a_computed_headline_round_trips() {
        let h = HeadlineSpec {
            label: "QUIET".into(),
            attested: Attestation::Computed,
        };
        let yaml = serde_yaml::to_string(&h).expect("serializes");
        let back: HeadlineSpec = serde_yaml::from_str(&yaml).expect("deserializes");
        assert_eq!(back, h);
        assert_eq!(back.attested.kind(), AttestationKind::Computed);
    }

    /// **THE GATE.** BANKEN.md §V/§IX C-controller: the QUIET headline may
    /// never claim "(proven)". A doc sentence is reviewer discipline; this
    /// is a parse-time refusal, and the error names the ceiling.
    #[test]
    fn a_proven_headline_cannot_be_deserialized() {
        for doc in [
            "label: QUIET\nattested:\n  kind: proven\n  outcomeChain: chain-1\n",
            "label: QUIET\nattested:\n  kind: proven\n  outcomeChain: ''\n",
        ] {
            let err = serde_yaml::from_str::<HeadlineSpec>(doc)
                .expect_err("a proven headline must be rejected");
            let msg = err.to_string();
            assert!(
                msg.contains("PromessaController"),
                "the refusal must name the ceiling, got: {msg}"
            );
            assert!(msg.contains("(computed)"), "and the fix, got: {msg}");
        }
        // Sanity: the SAME document with `computed` is accepted, so the
        // rejection above is attributable to the attestation arm alone and
        // not to a malformed document.
        let ok: HeadlineSpec = serde_yaml::from_str("label: QUIET\nattested:\n  kind: computed\n")
            .expect("the computed arm parses");
        assert_eq!(ok.attested, Attestation::Computed);
    }

    /// The variant is KEPT (★★ MODULARIZE, DON'T DELETE) and is genuinely
    /// constructible in-crate + serializable — so the day a real
    /// OutcomeChain ships, reviving it is making one constructor `pub`, not
    /// re-deriving the shape.
    #[test]
    fn the_proven_arm_still_exists_and_serializes() {
        let proven = Attestation::Proven {
            outcome_chain: OutcomeChainRef::from_outcome_chain("chain-7"),
        };
        assert_eq!(proven.kind(), AttestationKind::Proven);
        let yaml = serde_yaml::to_string(&proven).expect("serializes out");
        assert!(yaml.contains("chain-7"), "yaml was: {yaml}");
        // …and reading it back is still refused, which is the seal.
        assert!(serde_yaml::from_str::<Attestation>(&yaml).is_err());
    }
}
