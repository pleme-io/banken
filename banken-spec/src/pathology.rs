//! `(defpathology)` — the symptom→cause rule domain, and the health
//! verdict it feeds (BANKEN.md §V + the §V drill-flow).
//!
//! # What this domain is
//!
//! BANKEN.md §V's Popeye lane consumes a *taxonomy*: the Tendril
//! symptom→cause rules (§II/§III) plus autorevivy's 35-class detection
//! stream, rendered as a read-only linter whose every remedy routes through
//! `postigo` DECLARE. That taxonomy was authored **prose** in the theory doc
//! and **hardcoded nowhere** in banken — a rule was a sentence, not a value.
//! `(defpathology …)` makes each rule a typed value: what was observed, what
//! the load-bearing cause is, which typed evidence signals must be abnormal
//! for it to fire, how badly it bites, and which rail fixes it.
//!
//! Third-use test: passed by a wide margin. autorevivy's `MaintenanceJob`
//! taxonomy is 35 classes (BANKEN.md §II, verified), Tendril names more, and
//! three ship authored here on day one.
//!
//! # The BROKEN-METRIC guard is STRUCTURAL, not a catalog row
//!
//! The marquee invariant — *no workload shows GREEN unless
//! `up==1 && count(core_metric)>0`* (BANKEN.md §V / TENDRIL §II.9) — is **not**
//! implemented as a pathology, deliberately. A rule in a catalog can be
//! omitted, mis-authored, or filtered out by a ward's `:pathologies` list,
//! and then the guard silently stops guarding. Instead
//! [`WardVerdict::evaluate`] applies it as an unconditional structural cap:
//! **a would-be `Green` over an absent core metric becomes
//! [`Verdict::Unknown`]**, whatever the catalog says. The
//! `broken-scrape` pathology still ships — it is the *diagnosis* that tells
//! the operator what to fix — but the guard does not depend on it existing.
//!
//! # The seal, tier-honest
//!
//! [`WardVerdict`]'s fields are private and [`WardVerdict::evaluate`] is its
//! **only** constructor, so "a ward verdict claiming GREEN over a dead
//! scrape" has no construction path outside this module — the same
//! qualified tier as `ClusterEnv`'s missing mutate method
//! (**truly-unrepresentable within the authored `WardVerdict` surface**).
//! [`Verdict`] itself stays freely constructible because renderers must
//! match on it; what a caller cannot mint is a *ward's* verdict. The render
//! layer painting a green cell from a bare `Verdict::Green` it computed
//! itself is **only-mitigated** — see `pending-banken: render-green-gate`.

use serde::{Deserialize, Serialize};
use tatara_lisp::DeriveTataraDomain;

use crate::{closed_catalog, env::HealthReading, error::SpecError, types::DeclareTargetKind};

closed_catalog! {
    /// How badly a pathology bites, which is what a firing contributes to
    /// the composed [`Verdict`].
    #[serde(rename_all = "kebab-case")]
    pub enum Severity {
        /// The workload is broken. A firing forces [`Verdict::Red`].
        Critical => "critical",
        /// The workload is degraded. A firing forces [`Verdict::Degraded`].
        Warning => "warning",
        /// Worth telling the operator, but not a health downgrade — a
        /// firing leaves the verdict where it was.
        Advisory => "advisory",
    }
}

impl Severity {
    /// The verdict floor a firing of this severity imposes.
    #[must_use]
    pub fn verdict(self) -> Verdict {
        match self {
            Severity::Critical => Verdict::Red,
            Severity::Warning => Verdict::Degraded,
            Severity::Advisory => Verdict::Green,
        }
    }
}

closed_catalog! {
    /// A typed evidence signal a pathology reads.
    ///
    /// **Deliberately small: exactly the three signals
    /// [`HealthReading`] actually carries.** Adding an evidence kind the
    /// reading cannot supply would make an authored rule look evaluated
    /// while it silently never fires — the "silent wrong `Ok`" this repo's
    /// error discipline forbids. Grow this axis *with* the reading, never
    /// ahead of it.
    #[serde(rename_all = "kebab-case")]
    pub enum EvidenceKind {
        /// The core-metric presence guard (`up==1 ∧ count(metric)>0`) —
        /// abnormal when [`HealthReading::metric_present`] is `false`.
        MetricPresence => "metric-presence",
        /// A breathe band phase — abnormal when ANY band phase on the
        /// reading is outside [`HEALTHY_BAND_PHASES`].
        BandPhase => "band-phase",
        /// An autorevivy detection label — abnormal when the reading's
        /// detections name this pathology.
        Detection => "detection",
    }
}

/// The breathe `BandStatus.phase` values banken reads as healthy.
///
/// **Fail-toward-unhealthy is the point.** A phase string not listed here
/// reads as abnormal, so an unfamiliar phase (`NotReady`, `Stale`,
/// `Conflict`, a value breathe adds tomorrow) surfaces loudly rather than
/// being silently treated as fine. Tier: **only-mitigated** — this is a
/// string comparison against a foreign CRD's enum, not a typed join. A
/// typed `BandPhase` shared with breathe would raise it, and is the
/// destination.
pub const HEALTHY_BAND_PHASES: &[&str] = &["Holding"];

/// `true` when `phase` is one banken reads as healthy.
#[must_use]
pub fn is_healthy_band_phase(phase: &str) -> bool {
    HEALTHY_BAND_PHASES.contains(&phase)
}

closed_catalog! {
    /// Coarse discriminant of a [`Remedy`], for catalog reflection.
    #[serde(rename_all = "kebab-case")]
    pub enum RemedyKind {
        /// [`Remedy::Declare`].
        Declare => "declare",
        /// [`Remedy::BreakGlass`].
        BreakGlass => "break-glass",
        /// [`Remedy::NoRemedy`].
        NoRemedy => "no-remedy",
    }
}

/// What fixes a pathology.
///
/// **There is no `Kubectl`/`Apply`/`Patch` arm**, for the same reason
/// [`crate::types::DeclareTarget`] has none: a remedy that live-mutates is
/// un-authorable. A remedy is either a DECLARE onto one of the five `GitOps`
/// rails (reusing [`DeclareTargetKind`] — one catalog, not two), the
/// witnessed BREAK-GLASS arm, or an honest admission that banken cannot fix
/// it.
///
/// Internally tagged on `kind` (kebab-case) so an authored
/// `:remedy (:kind declare :rail flux-helm-values)` round-trips.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Remedy {
    /// Fix it by declaring a change onto a `GitOps` rail.
    Declare {
        /// Which of the five rails carries the fix.
        rail: DeclareTargetKind,
    },
    /// Fix it through the witnessed BREAK-GLASS arm (wedged-GitOps only).
    BreakGlass,
    /// banken cannot fix it — say why rather than pretending a rail exists.
    /// This is the honest arm for the §IX C-declare-coverage gap (a bare
    /// pod, an operator-created child, a chartless CRD).
    NoRemedy {
        /// Why no rail applies.
        why: String,
    },
}

impl Remedy {
    /// Project this remedy to its coarse kind.
    #[must_use]
    pub fn kind(&self) -> RemedyKind {
        match self {
            Remedy::Declare { .. } => RemedyKind::Declare,
            Remedy::BreakGlass => RemedyKind::BreakGlass,
            Remedy::NoRemedy { .. } => RemedyKind::NoRemedy,
        }
    }
}

/// One authored symptom→cause rule.
///
/// Every field is read by [`WardVerdict::evaluate`] or rendered in the
/// diagnose panel; there is no decorative field. (An earlier draft carried a
/// `gates_green: bool` — it was cut, because the green gate became the
/// structural cap in `evaluate` and the flag would have been an unused field
/// masquerading as an invariant.)
#[derive(DeriveTataraDomain, Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[tatara(keyword = "defpathology")]
pub struct PathologySpec {
    /// The rule's stable name. This is ALSO the label an autorevivy
    /// detection must carry for [`EvidenceKind::Detection`] to match, so it
    /// is a join key, not a title.
    pub name: String,
    /// What the operator observes (the surface symptom).
    pub symptom: String,
    /// The load-bearing cause the symptom resolves to — the whole point of
    /// the taxonomy (BANKEN.md §V: *"walks the symptom→cause path to the
    /// load-bearing cause"*).
    pub cause: String,
    /// How badly a firing bites.
    pub severity: Severity,
    /// The evidence signals that must ALL be abnormal for this rule to
    /// fire (a conjunction — see [`PathologySpec::fires`]). Empty is an
    /// authoring error, not a never-firing rule.
    pub evidence: Vec<EvidenceKind>,
    /// How it is fixed — a `GitOps` rail, break-glass, or an honest "not by
    /// banken".
    pub remedy: Remedy,
}

impl PathologySpec {
    /// Does this rule fire against `reading`?
    ///
    /// The evidence list is a **conjunction**: every listed signal must read
    /// abnormal. A single-signal rule therefore fires on that signal alone,
    /// and a multi-signal rule needs all of them — which is what makes a
    /// narrow rule (`BestEffort QoS *and* a band outside its bounds`)
    /// expressible without a second combinator.
    ///
    /// [`EvidenceKind::BandPhase`] is coarse in M0: it reads "ANY band
    /// phase on the reading is unhealthy", not "the memory band
    /// specifically". Narrowing it needs a per-dimension reading, which
    /// [`HealthReading`] does not carry — `pending-banken:
    /// per-dimension-band-evidence`.
    ///
    /// # Errors
    ///
    /// [`SpecError::PathologyWithoutEvidence`] when `evidence` is empty. An
    /// evidence-free rule can never be evaluated, and returning `false`
    /// would be exactly the silent-never-fires failure this crate's error
    /// discipline forbids.
    pub fn fires(&self, reading: &HealthReading) -> Result<bool, SpecError> {
        if self.evidence.is_empty() {
            return Err(SpecError::PathologyWithoutEvidence(self.name.clone()));
        }
        Ok(self.evidence.iter().all(|e| match e {
            EvidenceKind::MetricPresence => !reading.metric_present,
            EvidenceKind::BandPhase => reading
                .band_phases
                .iter()
                .any(|(_, phase)| !is_healthy_band_phase(phase)),
            EvidenceKind::Detection => reading.detections.iter().any(|d| d == &self.name),
        }))
    }
}

closed_catalog! {
    /// The composed health verdict for one workload.
    ///
    /// Freely constructible on purpose — a renderer must be able to `match`
    /// on it. The *sealed* thing is [`WardVerdict`], which is the only value
    /// a ward is allowed to report.
    #[serde(rename_all = "kebab-case")]
    pub enum Verdict {
        /// Healthy, and **provably observed** — a `Green` can only come out
        /// of [`WardVerdict::evaluate`] when the core metric was present.
        Green => "green",
        /// Something is degraded (a `Warning` pathology fired).
        Degraded => "degraded",
        /// Something is broken (a `Critical` pathology fired).
        Red => "red",
        /// **We cannot see.** The core-metric presence guard failed, so the
        /// workload's health is unknown — never rendered as calm.
        Unknown => "unknown",
    }
}

impl Verdict {
    /// How bad this verdict is, for the worst-of fold. `Unknown` is not
    /// ranked here: it is produced by the structural cap in
    /// [`WardVerdict::evaluate`], never by folding severities.
    fn rank(self) -> u8 {
        match self {
            Verdict::Green => 0,
            Verdict::Degraded => 1,
            Verdict::Red => 2,
            // Ranked at the top so a fold can never *demote* an Unknown.
            Verdict::Unknown => 3,
        }
    }

    /// The worse of two verdicts.
    #[must_use]
    pub fn worse_of(self, other: Verdict) -> Verdict {
        if other.rank() > self.rank() {
            other
        } else {
            self
        }
    }
}

/// A ward's verdict — **the only value a ward may report**.
///
/// Fields are private and [`Self::evaluate`] is the sole constructor, so a
/// caller cannot mint a ward verdict at all, let alone one claiming
/// [`Verdict::Green`] over an absent core metric. That is the BANKEN.md §V
/// BROKEN-METRIC guard realized as a construction invariant rather than a
/// render-time `if`.
///
/// Honest tier: **truly-unrepresentable within this authored surface**
/// (identical to `ClusterEnv`'s missing mutate method). It is NOT a
/// fleet-wide guarantee — a renderer that computes its own bare
/// [`Verdict`] bypasses this, which is `pending-banken: render-green-gate`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WardVerdict {
    verdict: Verdict,
    fired: Vec<String>,
    metric_present: bool,
}

impl WardVerdict {
    /// Evaluate every pathology against one workload's reading and compose
    /// the verdict.
    ///
    /// Two steps, in this order, and the order is load-bearing:
    ///
    /// 1. **Fold** the severities of every firing rule (worst wins).
    /// 2. **Cap** — if the fold said [`Verdict::Green`] but
    ///    [`HealthReading::metric_present`] is `false`, report
    ///    [`Verdict::Unknown`] instead. This step consults **no catalog
    ///    row**, so an empty / filtered / mis-authored pathology list
    ///    cannot disable it.
    ///
    /// # Errors
    ///
    /// Propagates [`SpecError::PathologyWithoutEvidence`] from
    /// [`PathologySpec::fires`] — a malformed catalog yields an error, never
    /// an optimistic `Green`.
    pub fn evaluate(
        reading: &HealthReading,
        pathologies: &[PathologySpec],
    ) -> Result<Self, SpecError> {
        let mut fired = Vec::new();
        let mut folded = Verdict::Green;
        for p in pathologies {
            if !p.fires(reading)? {
                continue;
            }
            fired.push(p.name.clone());
            folded = folded.worse_of(p.severity.verdict());
        }

        // *** THE BROKEN-METRIC GUARD (BANKEN.md §V / TENDRIL §II.9). ***
        // Structural and catalog-independent: a would-be Green over an
        // absent core metric is Unknown. Deleting this line is what the
        // `a_dead_scrape_can_never_report_green` gate catches.
        let verdict = if folded == Verdict::Green && !reading.metric_present {
            Verdict::Unknown
        } else {
            folded
        };

        Ok(Self {
            verdict,
            fired,
            metric_present: reading.metric_present,
        })
    }

    /// The composed verdict.
    #[must_use]
    pub fn verdict(&self) -> Verdict {
        self.verdict
    }

    /// The names of the pathologies that fired, in catalog order.
    #[must_use]
    pub fn fired(&self) -> &[String] {
        &self.fired
    }

    /// Whether the core-metric presence guard passed. `false` means the
    /// reading was blind, so a non-`Red` verdict carries no confidence.
    #[must_use]
    pub fn metric_present(&self) -> bool {
        self.metric_present
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reading(
        metric_present: bool,
        phases: &[(&str, &str)],
        detections: &[&str],
    ) -> HealthReading {
        HealthReading {
            band_phases: phases
                .iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                .collect(),
            metric_present,
            detections: detections.iter().map(|d| (*d).to_owned()).collect(),
        }
    }

    fn rule(name: &str, severity: Severity, evidence: &[EvidenceKind]) -> PathologySpec {
        PathologySpec {
            name: name.into(),
            symptom: "symptom".into(),
            cause: "cause".into(),
            severity,
            evidence: evidence.to_vec(),
            remedy: Remedy::BreakGlass,
        }
    }

    #[test]
    fn healthy_reading_with_no_firings_is_green() {
        let v = WardVerdict::evaluate(&reading(true, &[("MemoryBand", "Holding")], &[]), &[])
            .expect("evaluates");
        assert_eq!(v.verdict(), Verdict::Green);
        assert!(v.fired().is_empty());
    }

    /// **THE GATE.** The BANKEN.md §V BROKEN-METRIC guard: an absent core
    /// metric can never read as calm — and crucially this holds with an
    /// EMPTY pathology catalog, which is what makes the guard structural
    /// rather than a catalog row that could be omitted.
    #[test]
    fn a_dead_scrape_can_never_report_green() {
        for catalog in [
            Vec::new(),
            vec![rule(
                "irrelevant",
                Severity::Advisory,
                &[EvidenceKind::Detection],
            )],
        ] {
            let v = WardVerdict::evaluate(
                &reading(
                    /* metric_present */ false,
                    &[("MemoryBand", "Holding")],
                    &[],
                ),
                &catalog,
            )
            .expect("evaluates");
            assert_eq!(
                v.verdict(),
                Verdict::Unknown,
                "a blind reading must be Unknown, never Green — with catalog {catalog:?}",
            );
            assert_ne!(v.verdict(), Verdict::Green);
            assert!(!v.metric_present());
        }
    }

    /// The cap only removes GREEN. A blind reading that ALSO shows a
    /// critical firing is still Red — we can see the crashloop even if the
    /// metric is gone, and downgrading Red to Unknown would lose signal.
    #[test]
    fn the_cap_only_removes_green_never_a_real_downgrade() {
        let v = WardVerdict::evaluate(
            &reading(false, &[], &["crashloop"]),
            &[rule(
                "crashloop",
                Severity::Critical,
                &[EvidenceKind::Detection],
            )],
        )
        .expect("evaluates");
        assert_eq!(v.verdict(), Verdict::Red);
    }

    #[test]
    fn severity_folds_worst_wins() {
        let r = reading(true, &[], &["warn-rule", "crit-rule"]);
        let v = WardVerdict::evaluate(
            &r,
            &[
                rule("warn-rule", Severity::Warning, &[EvidenceKind::Detection]),
                rule("crit-rule", Severity::Critical, &[EvidenceKind::Detection]),
            ],
        )
        .expect("evaluates");
        assert_eq!(v.verdict(), Verdict::Red);
        assert_eq!(v.fired(), ["warn-rule", "crit-rule"]);
    }

    #[test]
    fn an_advisory_firing_is_reported_without_downgrading_health() {
        let v = WardVerdict::evaluate(
            &reading(true, &[], &["note"]),
            &[rule("note", Severity::Advisory, &[EvidenceKind::Detection])],
        )
        .expect("evaluates");
        assert_eq!(v.verdict(), Verdict::Green);
        assert_eq!(v.fired(), ["note"], "still surfaced to the operator");
    }

    #[test]
    fn evidence_is_a_conjunction() {
        let both = rule(
            "both",
            Severity::Critical,
            &[EvidenceKind::Detection, EvidenceKind::BandPhase],
        );
        // Detection present but every band healthy → does NOT fire.
        assert!(
            !both
                .fires(&reading(true, &[("MemoryBand", "Holding")], &["both"]))
                .unwrap()
        );
        // Both abnormal → fires.
        assert!(
            both.fires(&reading(true, &[("MemoryBand", "Carving")], &["both"]))
                .unwrap()
        );
    }

    /// An unfamiliar band phase reads as ABNORMAL — fail-toward-unhealthy,
    /// so a phase breathe adds tomorrow surfaces instead of reading calm.
    #[test]
    fn an_unknown_band_phase_is_treated_as_unhealthy() {
        assert!(is_healthy_band_phase("Holding"));
        for unknown in ["Carving", "NotReady", "Stale", "Conflict", "SomethingNew"] {
            assert!(
                !is_healthy_band_phase(unknown),
                "{unknown} must read as abnormal, not silently fine"
            );
        }
    }

    /// **THE GATE.** An evidence-free rule is an ERROR, not a rule that
    /// silently never fires.
    #[test]
    fn an_evidence_free_pathology_is_an_error_not_a_silent_never_fire() {
        let empty = rule("no-evidence", Severity::Critical, &[]);
        let err = empty
            .fires(&reading(true, &[], &[]))
            .expect_err("an evidence-free rule must be rejected");
        assert!(
            matches!(&err, SpecError::PathologyWithoutEvidence(n) if n == "no-evidence"),
            "got: {err}"
        );
        // And it poisons the whole evaluation rather than yielding a
        // cheerful Green over a malformed catalog.
        let err = WardVerdict::evaluate(&reading(true, &[], &[]), &[empty])
            .expect_err("a malformed catalog must not evaluate to Green");
        assert!(err.to_string().contains("no-evidence"), "got: {err}");
    }

    #[test]
    fn remedy_projects_to_its_kind_and_has_no_live_arm() {
        assert_eq!(
            Remedy::Declare {
                rail: DeclareTargetKind::FluxHelmValues
            }
            .kind(),
            RemedyKind::Declare
        );
        assert_eq!(Remedy::BreakGlass.kind(), RemedyKind::BreakGlass);
        assert_eq!(
            Remedy::NoRemedy {
                why: "chartless".into()
            }
            .kind(),
            RemedyKind::NoRemedy
        );
        // Every remedy rail is one of the five GitOps rails — the remedy
        // axis reuses ONE declare-target catalog rather than opening a
        // second one that could grow a live arm independently.
        for k in RemedyKind::ALL {
            assert!(["declare", "break-glass", "no-remedy"].contains(&k.label()));
        }
    }
}
