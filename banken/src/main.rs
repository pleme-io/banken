//! banken — the entry point.
//!
//! Runs the `:pods` navigator over a [`ClusterEnv`] source. The default
//! source is the fixture ([`banken::fixture::FixtureClusterEnv`]) — the
//! path proven this session. `--live` selects the live kube backend
//! ([`banken::live::KubeClusterEnv`], feature `live`) against the current
//! kubeconfig context; it is UNTESTED-LIVE this session (no cluster
//! reachable) and only compiled when the `live` feature is enabled.
//!
//! Usage:
//! ```text
//! banken                # :pods over the fixture source (default)
//! banken :pods          # same, explicit view
//! banken --live         # :pods over the live cluster (feature `live`)
//! banken --help         # usage
//! ```

use banken::app::BankenApp;
use banken::fixture::FixtureClusterEnv;
use banken_spec::types::OperatorId;

/// The cluster id the fixture source reports. A `(defguarita)` resolving
/// `(:context cluster)` needs *a* name; naming the fixture is honest, and an
/// empty value would make every recipe refuse.
const FIXTURE_CLUSTER: &str = "fixture";

fn main() {
    // Minimal typed arg handling — no clap, no shell parsing. banken takes
    // an optional view (`:pods`) and an optional `--live` flag.
    let args: Vec<String> = std::env::args().skip(1).collect();
    let want_live = args.iter().any(|a| a == "--live");
    let want_help = args.iter().any(|a| a == "--help" || a == "-h");

    if want_help {
        print_usage();
        return;
    }

    // The only view M0 ships is `:pods`; a different `:view` arg is accepted
    // but routes to :pods for now (the FuzzyPicker command bar is M1).
    let operator = OperatorId("drzzln".into());

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");

    let exit = rt.block_on(async move {
        if want_live {
            run_live(operator).await
        } else {
            run_fixture(operator).await
        }
    });

    if let Err(msg) = exit {
        eprintln!("banken: {msg}");
        std::process::exit(1);
    }
}

/// Run the navigator over the fixture source (the proven path).
async fn run_fixture(operator: OperatorId) -> Result<(), String> {
    // Fallible: the keymap and the table columns come from the authored
    // vocabulary, and a spec failure must SURFACE rather than fall back to
    // hardcoded chords the legend does not describe.
    let mut app = BankenApp::try_new(
        FixtureClusterEnv::new(),
        operator,
        "source: fixture (no cluster reachable this session)",
    )
    .map_err(|e| e.to_string())?
    // The fixture IS the cluster in this mode, and naming it honestly is what
    // lets a `(defguarita)` resolve `(:context cluster)`. It is not a claim
    // that a cluster named "fixture" exists — the status line says
    // "source: fixture" right beside it.
    .with_cluster(FIXTURE_CLUSTER);
    egaku_term::run_async(&mut app)
        .await
        .map_err(|e| e.to_string())
}

/// Run the navigator over the live kube source. Only available with the
/// `live` feature; otherwise a typed message tells the operator how to
/// enable it (never a silent fallback that hides the gap).
#[cfg(feature = "live")]
async fn run_live(operator: OperatorId) -> Result<(), String> {
    use banken::live::KubeClusterEnv;
    // UNTESTED-LIVE this session — pending-banken: live-read.
    let env = KubeClusterEnv::connect()
        .await
        .map_err(|e| format!("live connect failed (VPN/kubeconfig?): {e}"))?;
    // The kubeconfig's `current-context`, when it can be read. An empty value
    // is NOT defaulted away: a `(defguarita)` then refuses to pre-warm a
    // session rather than opening one on whatever context the operator's
    // shell happens to be on.
    let cluster = env.context_name().unwrap_or_default();
    let mut app = BankenApp::try_new(env, operator, "source: LIVE (current kubeconfig context)")
        .map_err(|e| e.to_string())?
        .with_cluster(cluster);
    egaku_term::run_async(&mut app)
        .await
        .map_err(|e| e.to_string())
}

/// Without the `live` feature, `--live` is an explicit typed error — never
/// a silent fall-through to the fixture (which would be a rounded-up claim).
#[cfg(not(feature = "live"))]
async fn run_live(_operator: OperatorId) -> Result<(), String> {
    Err(
        "--live requires building with `--features live` (kube backend); \
         the default binary ships the fixture source only"
            .into(),
    )
}

fn print_usage() {
    // A plain-text usage block — the one place a printf-shaped write is the
    // typed emission surface (stdout of a CLI, not VT into the grid).
    println!("banken 番犬 — the pleme-io-native k9s (observe-first, GitOps-native)");
    println!();
    println!("USAGE:");
    println!("  banken [:view] [--live]");
    println!();
    println!("VIEWS:");
    println!("  :pods            the pod table (default; the only M0 view)");
    println!();
    println!("FLAGS:");
    println!("  --live           read from the live cluster (requires --features live)");
    println!("  -h, --help       print this help");
    println!();
    println!("KEYS (in the :pods table) — read from the authored vocabulary:");
    match banken_spec::load_catalog() {
        Ok(catalog) => {
            // The nav keys, grouped by intent so `down`/`j` read as one row.
            for intent in banken_spec::nav::NavIntent::ALL {
                let chords: Vec<String> = catalog
                    .nav_keys()
                    .iter()
                    .filter(|n| n.intent == *intent)
                    .map(|n| n.keys.canonical())
                    .collect();
                if !chords.is_empty() {
                    println!("  {:<16} {}", chords.join(" / "), intent.label());
                }
            }
            // The postigo actions, each with the gate its keystroke crosses.
            for a in catalog.actions() {
                let unbound = banken::app::unbound_action_names(&catalog).contains(&a.name);
                let note = if unbound { "  (not wired yet)" } else { "" };
                println!(
                    "  {:<16} {} — {}{}",
                    a.keys.canonical(),
                    a.legality.class().label().to_uppercase(),
                    a.name,
                    note,
                );
            }
            // The guaritas — pre-warmed tear sessions. The class printed is
            // the DERIVED one (from the recipe's panes), so a recipe staging a
            // live effect cannot be advertised here as a convenience.
            if !catalog.guaritas().is_empty() {
                println!();
                println!("GUARITAS (pre-warmed tear/mado troubleshooting sessions):");
                let unbound = banken::app::unbound_guarita_names(&catalog);
                for g in catalog.guaritas() {
                    let class = g
                        .legality()
                        .map(|l| l.class().label().to_uppercase())
                        .unwrap_or_else(|_| "INVALID".to_owned());
                    let note = if unbound.contains(&g.name) {
                        "  (launches from another view)"
                    } else {
                        ""
                    };
                    println!(
                        "  {:<16} {} — {} ({} panes, from :{}){}",
                        g.keys.canonical(),
                        class,
                        g.name,
                        g.panes.len(),
                        g.from,
                        note,
                    );
                }
                println!("  The plan is PREVIEWED, not opened: handing it to a live tear-daemon");
                println!("  is the SessionEnv seam's job and banken ships no implementation of it");
                println!("  yet (pending-banken: guarita-live-handoff).");
            }
        }
        // Honest: if the vocabulary does not load, say so rather than print a
        // hand-written key list that might not match what would have run.
        Err(e) => println!("  <the authored vocabulary failed to load: {e}>"),
    }
}
