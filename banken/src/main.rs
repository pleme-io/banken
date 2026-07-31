//! banken — the entry point.
//!
//! Runs the `:pods` navigator over a [`ClusterEnv`] source. The default
//! source is the fixture ([`banken::fixture::FixtureClusterEnv`]).
//! `--live --context <name>` selects the live kube backend
//! ([`banken::live::KubeClusterEnv`], feature `live`) against an
//! **explicitly named** kubeconfig context.
//!
//! `--context` is required for `--live`, and that is a measured decision, not
//! a style: riding the kubeconfig's `current-context` reads whichever estate
//! the operator's merged `KUBECONFIG` happens to point at, and a pod table
//! from the wrong cluster is indistinguishable from a pod table from the right
//! one. See [`banken::cli`] for the full reasoning and the measurement.
//!
//! Usage:
//! ```text
//! banken                                  # :pods over the fixture source (default)
//! banken :pods                            # same, explicit view
//! banken --live --context camelot-eks     # :pods over that cluster (feature `live`)
//! banken --help                           # usage
//! ```

use banken::app::BankenApp;
use banken::cli::{CliError, Invocation, parse_args};
use banken::fixture::FixtureClusterEnv;
use banken_spec::types::OperatorId;

/// The cluster id the fixture source reports. A `(defbancada)` resolving
/// `(:context cluster)` needs *a* name; naming the fixture is honest, and an
/// empty value would make every recipe refuse.
const FIXTURE_CLUSTER: &str = "fixture";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();

    let invocation = match parse_args(&args) {
        Ok(inv) => inv,
        Err(e) => {
            eprintln!("banken: {e}");
            // The one refusal with useful runtime data to add: name the
            // context banken would otherwise have silently used, and list
            // what is available, so the fix is one keystroke rather than a
            // trip to `kubectl config get-contexts`.
            if e == CliError::MissingContext {
                eprint!("{}", missing_context_hint());
            }
            std::process::exit(2);
        }
    };

    let operator = OperatorId("drzzln".into());

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");

    let exit = rt.block_on(async move {
        match invocation {
            Invocation::Help => {
                print_usage();
                Ok(())
            }
            Invocation::Fixture => run_fixture(operator).await,
            Invocation::Live { context } => run_live(operator, &context).await,
        }
    });

    if let Err(msg) = exit {
        eprintln!("banken: {msg}");
        std::process::exit(1);
    }
}

/// The kubeconfig half of the `--live`-without-`--context` refusal.
///
/// Behind the `live` feature because reading the kubeconfig is that feature's
/// dependency. Without it the refusal is still correct — `--live` itself is
/// unavailable — it simply has nothing extra to say.
#[cfg(feature = "live")]
fn missing_context_hint() -> String {
    let mut s = String::new();
    if let Some(current) = banken::live::kubeconfig_current_context() {
        s.push_str("  the current-context banken would have read: ");
        s.push_str(&current);
        s.push('\n');
    }
    let names = banken::live::kubeconfig_context_names();
    if names.is_empty() {
        return s;
    }
    s.push_str("  available contexts:\n");
    for name in names {
        s.push_str("    ");
        s.push_str(&name);
        s.push('\n');
    }
    s
}

#[cfg(not(feature = "live"))]
fn missing_context_hint() -> String {
    String::new()
}

/// The [`SessionEnv`](banken_spec::bancada::SessionEnv) a confirmed
/// `(defbancada)` opens through.
///
/// With the `tear` feature this is the live adapter, connecting to the daemon
/// **on first use** — so a missing `tear-daemon` costs one overlay saying so
/// at the moment the operator asks for a session, rather than refusing to
/// start banken at all. Without the feature it is the typed refusal, which
/// still prints the fully-resolved plan and says why it cannot open it.
#[cfg(feature = "tear")]
fn session_env() -> banken::session::LazyTearSessionEnv {
    banken::session::LazyTearSessionEnv::new()
}

#[cfg(not(feature = "tear"))]
fn session_env() -> banken::session::UnwiredSessionEnv {
    banken::session::UnwiredSessionEnv::new()
}

/// Run the navigator over the fixture source (the proven path).
async fn run_fixture(operator: OperatorId) -> Result<(), String> {
    // Fallible: the keymap and the table columns come from the authored
    // vocabulary, and a spec failure must SURFACE rather than fall back to
    // hardcoded chords the legend does not describe.
    let mut app = BankenApp::try_new(
        FixtureClusterEnv::new(),
        session_env(),
        operator,
        "source: fixture (no cluster read)",
    )
    .map_err(|e| e.to_string())?
    // The fixture IS the cluster in this mode, and naming it honestly is what
    // lets a `(defbancada)` resolve `(:context cluster)`. It is not a claim
    // that a cluster named "fixture" exists — the status line says
    // "source: fixture" right beside it.
    .with_cluster(FIXTURE_CLUSTER);
    egaku_term::run_async(&mut app)
        .await
        .map_err(|e| e.to_string())
}

/// Run the navigator over the live kube source, against the **named**
/// kubeconfig context.
///
/// The context banken connected with is the context banken reports: it is
/// carried into the status line and into every `(defbancada)`'s
/// `(:context cluster)` from the same `String` that selected the apiserver, so
/// the two cannot drift.
#[cfg(feature = "live")]
async fn run_live(operator: OperatorId, context: &str) -> Result<(), String> {
    use banken::live::KubeClusterEnv;
    // Printed BEFORE the alt-screen is entered, so it survives on the primary
    // screen after banken exits — the durable receipt of which estate was
    // read. The status line carries it live while banken is running.
    eprintln!("banken: reading kubeconfig context `{context}`");

    let env = KubeClusterEnv::connect_with_context(context)
        .await
        .map_err(|e| {
            let mut m = String::from("live connect failed (VPN/kubeconfig?): ");
            m.push_str(&e.to_string());
            m
        })?;
    // The env reports the context it was CONSTRUCTED with, so this is the
    // same value throughout. An empty one is impossible here (the CLI rejects
    // it), which is what makes the bancada `(:context cluster)` resolution
    // trustworthy rather than merely populated.
    let cluster = env.context_name().unwrap_or_default();
    let mut label = String::from("source: LIVE ");
    label.push_str(&cluster);
    let mut app = BankenApp::try_new(env, session_env(), operator, label)
        .map_err(|e| e.to_string())?
        .with_cluster(cluster);
    egaku_term::run_async(&mut app)
        .await
        .map_err(|e| e.to_string())
}

/// Without the `live` feature, `--live` is an explicit typed error — never
/// a silent fall-through to the fixture (which would be a rounded-up claim).
#[cfg(not(feature = "live"))]
async fn run_live(_operator: OperatorId, _context: &str) -> Result<(), String> {
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
    println!("  banken [:view]");
    println!("  banken --live --context <name>");
    println!();
    println!("VIEWS:");
    println!("  :pods            the pod table (default; the only M0 view)");
    println!();
    println!("FLAGS:");
    println!("  --live           read from the live cluster (requires --features live)");
    println!("  --context <name> the kubeconfig context to read — REQUIRED with --live.");
    println!("                   banken will not read \"whatever current-context happens");
    println!("                   to be\": a merged KUBECONFIG routinely points at a");
    println!("                   different estate, and a pod table from the wrong cluster");
    println!("                   looks exactly like one from the right cluster.");
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
            // The bancadas — pre-warmed tear sessions. The class printed is
            // the DERIVED one (from the recipe's panes), so a recipe staging a
            // live effect cannot be advertised here as a convenience.
            if !catalog.bancadas().is_empty() {
                println!();
                println!("BANCADAS (pre-warmed tear/mado troubleshooting sessions):");
                let unbound = banken::app::unbound_bancada_names(&catalog);
                for g in catalog.bancadas() {
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
                println!("  The chord RESOLVES and previews the plan; `enter` confirms it and");
                println!("  opens it through the SessionEnv seam. The gap is deliberate: you see");
                println!("  the resolved argv and the cluster it names before anything opens.");
                println!("  Opening for real needs `--features tear` and a running tear-daemon;");
                println!("  without them `enter` says so rather than reporting a session.");
            }
        }
        // Honest: if the vocabulary does not load, say so rather than print a
        // hand-written key list that might not match what would have run.
        Err(e) => println!("  <the authored vocabulary failed to load: {e}>"),
    }
}
