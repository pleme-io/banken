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
    .map_err(|e| e.to_string())?;
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
    let mut app = BankenApp::try_new(env, operator, "source: LIVE (current kubeconfig context)")
        .map_err(|e| e.to_string())?;
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
        }
        // Honest: if the vocabulary does not load, say so rather than print a
        // hand-written key list that might not match what would have run.
        Err(e) => println!("  <the authored vocabulary failed to load: {e}>"),
    }
}
