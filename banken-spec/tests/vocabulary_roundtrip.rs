//! Every authored domain round-trips from Lisp into its typed border, and
//! the whole vocabulary cross-resolves.
//!
//! One test per domain (★★ CATALOG REFLECTION's authoring half: a domain
//! that ships without a round-trip proof is a form nobody has driven), plus
//! the `load_catalog()` cross-resolution that is the actual entry point.

use banken_spec::{
    Catalog, SpecError,
    drill::DrillLevel,
    env::HealthReading,
    load_catalog, load_drills, load_nav_keys, load_pathologies, load_wards,
    nav::NavIntent,
    pathology::{EvidenceKind, Remedy, Severity, Verdict, WardVerdict},
    types::DeclareTargetKind,
    ward::{Attestation, BandDimension, LaneSignal, WardScope},
};

// ── (defpathology) ──────────────────────────────────────────────────────

#[test]
fn authored_pathologies_compile_into_typed_values() {
    let ps = load_pathologies().expect("the authored taxonomy must compile");
    assert_eq!(
        ps.len(),
        3,
        "broken-scrape + band-not-holding + unowned-resource"
    );

    let broken = ps
        .iter()
        .find(|p| p.name == "broken-scrape")
        .expect("the marquee rule");
    assert_eq!(broken.severity, Severity::Critical);
    assert_eq!(broken.evidence, vec![EvidenceKind::MetricPresence]);
    assert_eq!(
        broken.remedy,
        Remedy::Declare {
            rail: DeclareTargetKind::FluxHelmValues
        },
        "the scrape config is chart values in git",
    );
    assert!(
        !broken.cause.is_empty(),
        "a rule without a cause is a symptom list"
    );

    let band = ps.iter().find(|p| p.name == "band-not-holding").unwrap();
    assert_eq!(band.severity, Severity::Warning);
    assert_eq!(
        band.remedy,
        Remedy::Declare {
            rail: DeclareTargetKind::BreatheBand
        },
    );

    // The honest C-declare-coverage arm: no rail, and it says why.
    let unowned = ps.iter().find(|p| p.name == "unowned-resource").unwrap();
    match &unowned.remedy {
        Remedy::NoRemedy { why } => assert!(
            why.contains("release.yaml"),
            "the no-remedy arm must say why, got: {why}"
        ),
        other => panic!("expected NoRemedy, got {other:?}"),
    }
}

/// The authored taxonomy actually DRIVES the interpreter: a blind reading
/// through the shipped rules is Red (broken-scrape fires) and, critically,
/// never Green.
#[test]
fn the_authored_taxonomy_drives_the_verdict() {
    let ps = load_pathologies().unwrap();

    let healthy = HealthReading {
        band_phases: vec![("MemoryBand".into(), "Holding".into())],
        metric_present: true,
        detections: Vec::new(),
    };
    let v = WardVerdict::evaluate(&healthy, &ps).expect("evaluates");
    assert_eq!(v.verdict(), Verdict::Green);

    let blind = HealthReading {
        band_phases: vec![("MemoryBand".into(), "Holding".into())],
        metric_present: false,
        detections: Vec::new(),
    };
    let v = WardVerdict::evaluate(&blind, &ps).expect("evaluates");
    assert_eq!(
        v.verdict(),
        Verdict::Red,
        "the authored broken-scrape rule is Critical, so it diagnoses Red"
    );
    assert_ne!(v.verdict(), Verdict::Green);
    assert_eq!(v.fired(), ["broken-scrape"]);

    let carving = HealthReading {
        band_phases: vec![("CpuBand".into(), "Carving".into())],
        metric_present: true,
        detections: Vec::new(),
    };
    let v = WardVerdict::evaluate(&carving, &ps).expect("evaluates");
    assert_eq!(v.verdict(), Verdict::Degraded);
    assert_eq!(v.fired(), ["band-not-holding"]);
}

// ── (defward) ───────────────────────────────────────────────────────────

#[test]
fn authored_wards_compile_into_typed_values() {
    let ws = load_wards().expect("the authored wards must compile");
    assert_eq!(ws.len(), 1);
    let w = &ws[0];
    assert_eq!(w.name, "ward");
    assert_eq!(w.view, "ward");
    assert_eq!(w.scope, WardScope::Fleet);
    assert_eq!(w.drill_to.as_deref(), Some("diagnose"));

    // Lanes: MEM (memory band), CPU (cpu band), STATUS (composed verdict).
    let headers: Vec<&str> = w.lanes.iter().map(|l| l.header.as_str()).collect();
    assert_eq!(headers, vec!["MEM", "CPU", "STATUS"]);
    assert_eq!(
        w.lanes[0].signal,
        LaneSignal::Band {
            dimension: BandDimension::Memory
        },
    );
    assert_eq!(
        w.lanes[1].signal,
        LaneSignal::Band {
            dimension: BandDimension::Cpu
        },
    );
    assert_eq!(
        w.lanes[2].signal,
        LaneSignal::Verdict,
        "the STATUS lane renders the BROKEN-METRIC-gated verdict"
    );

    assert_eq!(
        w.pathologies,
        vec!["broken-scrape", "band-not-holding", "unowned-resource"]
    );
    assert_eq!(w.headline.label, "QUIET");
}

/// **THE GATE** (BANKEN.md §V / §IX C-controller). The shipped ward claims
/// `(computed)`, and claiming `(proven)` is not an authoring choice that a
/// reviewer must catch — it has no typed value.
#[test]
fn the_authored_headline_is_computed_and_proven_is_unauthorable() {
    let ws = load_wards().unwrap();
    assert_eq!(ws[0].headline.attested, Attestation::Computed);

    // Author the same ward with `proven` and it is REFUSED at parse time,
    // with an error that names the ceiling.
    let proven = banken_spec::CANONICAL_WARDS_LISP.replace(
        "(:kind computed)",
        "(:kind proven :outcome-chain \"chain-1\")",
    );
    assert!(
        proven.contains("proven"),
        "the substitution must have landed, or this gate proves nothing"
    );
    let err = banken_spec::loader::load_all::<banken_spec::ward::WardSpec>(&proven)
        .expect_err("a proven headline must be rejected at parse time");
    let msg = err.to_string();
    assert!(
        msg.contains("PromessaController"),
        "the refusal must name the ceiling, got: {msg}"
    );
}

// ── (defdrill) ──────────────────────────────────────────────────────────

#[test]
fn authored_drills_compile_into_typed_values() {
    let ds = load_drills().expect("the authored drills must compile");
    assert_eq!(ds.len(), 3, "logs + diagnose + xray");

    let logs = ds.iter().find(|d| d.name == "logs").expect("logs drill");
    assert_eq!(logs.from, "pods");
    let levels: Vec<DrillLevel> = logs.steps.iter().map(|s| s.level).collect();
    assert_eq!(
        levels,
        vec![DrillLevel::Pod, DrillLevel::Container, DrillLevel::Logs]
    );
    assert_eq!(logs.terminal(), Some(DrillLevel::Logs));

    let diagnose = ds.iter().find(|d| d.name == "diagnose").unwrap();
    assert_eq!(diagnose.from, "ward");
    assert_eq!(diagnose.terminal(), Some(DrillLevel::Diagnose));

    // Every authored path descends.
    for d in &ds {
        d.validate()
            .unwrap_or_else(|e| panic!("authored drill `{}` must be valid: {e}", d.name));
    }
}

// ── (defnavkey) ─────────────────────────────────────────────────────────

#[test]
fn authored_nav_keys_compile_into_typed_values() {
    let ns = load_nav_keys().expect("the authored nav keys must compile");
    assert_eq!(ns.len(), 8, "down/j + up/k + o + escape + return + q");

    // The k9s/vi pair: two names, two chords, ONE intent.
    let next: Vec<&str> = ns
        .iter()
        .filter(|n| n.intent == NavIntent::SelectNext)
        .map(|n| n.name.as_str())
        .collect();
    assert_eq!(next, vec!["select-next-arrow", "select-next-vi"]);

    let quit = ns.iter().find(|n| n.intent == NavIntent::Quit).unwrap();
    assert_eq!(quit.keys.canonical(), "q");

    // `escape` is awase's spelling; the ONE verified translation to
    // egaku-term's `esc` lives in `banken::keys::chord_to_combo`.
    let dismiss = ns.iter().find(|n| n.intent == NavIntent::Dismiss).unwrap();
    assert_eq!(dismiss.keys.canonical(), "escape");

    // Every intent the app can perform is actually bound by the shipped
    // catalog — an unbound intent is a capability the operator cannot reach.
    for intent in NavIntent::ALL {
        assert!(
            ns.iter().any(|n| n.intent == *intent),
            "no authored nav key produces intent `{}`",
            intent.label()
        );
    }
}

// ── the whole vocabulary, cross-resolved ────────────────────────────────

/// The shipped vocabulary cross-resolves. This is the test that proves the
/// six spec files agree with each other, not merely that each parses.
#[test]
fn the_shipped_vocabulary_cross_resolves() {
    let c = load_catalog().expect("the shipped vocabulary must cross-resolve");
    assert_eq!(c.views().len(), 3);
    assert_eq!(c.actions().len(), 4);
    assert_eq!(c.pathologies().len(), 3);
    assert_eq!(c.wards().len(), 1);
    assert_eq!(c.drills().len(), 3);
    assert_eq!(c.nav_keys().len(), 8);

    // The ward's pathologies resolve to values, totally.
    let rules = c.pathologies_for(&c.wards()[0]);
    assert_eq!(rules.len(), 3, "every named rule resolved");
}

/// **THE GATE.** `pods` drills to `logs` and the `ward` view drills to
/// `diagnose`. Before `(defdrill)` those strings resolved against nothing.
/// Break one and the whole catalog refuses to resolve.
#[test]
fn a_broken_drill_reference_fails_the_whole_catalog() {
    let broken_views =
        banken_spec::CANONICAL_VIEWS_LISP.replace(":drill-to \"logs\"", ":drill-to \"lgos\"");
    assert!(
        broken_views.contains("lgos"),
        "the substitution must have landed, or this gate proves nothing"
    );
    let views = banken_spec::loader::load_all::<banken_spec::types::K8sViewSpec>(&broken_views)
        .expect("a typo'd drill target still PARSES — that is the point");
    let err = Catalog::resolve(
        views,
        banken_spec::load_actions().unwrap(),
        load_pathologies().unwrap(),
        load_wards().unwrap(),
        load_drills().unwrap(),
        load_nav_keys().unwrap(),
        banken_spec::load_bancadas().unwrap(),
    )
    .expect_err("a dangling drill target must fail resolution");
    match err {
        SpecError::DanglingDrill { surface, drill } => {
            assert_eq!(surface, "pods");
            assert_eq!(drill, "lgos");
        }
        other => panic!("expected DanglingDrill, got {other:?}"),
    }
}

/// **THE GATE.** The ward↔view correspondence over the REAL shipped files:
/// drop a lane from the authored ward and resolution fails, naming both
/// sides. This is what keeps `wards.lisp` and `views.lisp` from disagreeing
/// about one screen.
#[test]
fn dropping_an_authored_lane_fails_resolution_against_the_view() {
    let broken = banken_spec::CANONICAL_WARDS_LISP
        .replace("(:header \"CPU\" :signal (:kind band :dimension cpu))", "");
    assert!(
        !broken.contains(":dimension cpu"),
        "the substitution must have landed, or this gate proves nothing"
    );
    let wards = banken_spec::loader::load_all::<banken_spec::ward::WardSpec>(&broken)
        .expect("a ward with one fewer lane still PARSES");
    let err = Catalog::resolve(
        banken_spec::load_views().unwrap(),
        banken_spec::load_actions().unwrap(),
        load_pathologies().unwrap(),
        wards,
        load_drills().unwrap(),
        load_nav_keys().unwrap(),
        banken_spec::load_bancadas().unwrap(),
    )
    .expect_err("a lane/column disagreement must fail resolution");
    match err {
        SpecError::WardLaneColumnMismatch { ward, view, detail } => {
            assert_eq!(ward, "ward");
            assert_eq!(view, "ward");
            assert!(
                detail.contains("CPU"),
                "the detail names the missing lane: {detail}"
            );
        }
        other => panic!("expected WardLaneColumnMismatch, got {other:?}"),
    }
}

/// **THE GATE.** One chord namespace across BOTH keyed domains: rebind a
/// nav key onto the `scale` chord and the catalog refuses, rather than the
/// app's keymap silently resolving it by bind order.
#[test]
fn a_nav_key_on_a_postigo_chord_fails_the_whole_catalog() {
    let broken = banken_spec::CANONICAL_NAVKEYS_LISP.replace(":keys \"o\"", ":keys \"s\"");
    assert!(
        broken.contains(":keys \"s\""),
        "the substitution must have landed, or this gate proves nothing"
    );
    let nav = banken_spec::loader::load_all::<banken_spec::nav::NavKeySpec>(&broken)
        .expect("a colliding nav key still PARSES on its own");
    let err = Catalog::resolve(
        banken_spec::load_views().unwrap(),
        banken_spec::load_actions().unwrap(),
        load_pathologies().unwrap(),
        load_wards().unwrap(),
        load_drills().unwrap(),
        nav,
        banken_spec::load_bancadas().unwrap(),
    )
    .expect_err("a nav key on a postigo chord must fail resolution");
    match err {
        SpecError::ChordConflict {
            chord,
            existing,
            incoming,
            ..
        } => {
            assert_eq!(chord, "s");
            assert_eq!(existing, "scale", "the postigo action claimed it first");
            assert_eq!(incoming, "toggle-sort");
        }
        other => panic!("expected ChordConflict, got {other:?}"),
    }
}
