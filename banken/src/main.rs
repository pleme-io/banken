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

    let operator = OperatorId::new("drzzln").expect("a literal witness is non-blank");

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
            Invocation::Live { context, strategy } => run_live(operator, &context, strategy).await,
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

/// What `--help` says about `--live`, DERIVED from whether the backend is
/// actually compiled in rather than asserted by hand.
///
/// The hand-written form ("requires --features live") outlived its own truth
/// the moment `live` joined `default` in Cargo.toml: the help text told every
/// operator to rebuild for a capability the binary already had. A usage line
/// that states a build fact is a claim, and a claim that nothing re-derives
/// rots silently — so the two `#[cfg]` arms below ARE the derivation, and the
/// stale case has no way to be written.
#[cfg(feature = "live")]
fn live_availability() -> &'static str {
    ""
}

#[cfg(not(feature = "live"))]
fn live_availability() -> &'static str {
    " (unavailable — rebuild with `--features live`)"
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
    .with_cluster(FIXTURE_CLUSTER)
    // Attached even here, where the fixture never changes and the feed can
    // therefore never show anything new. That is deliberate: running the
    // exact same absorb path in both modes is what keeps the fixture a real
    // rehearsal of the live one. A feed that only existed under `--live`
    // would be a feed nobody ever exercised until it mattered.
    .with_feed(refresh_interval());
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
async fn run_live(
    operator: OperatorId,
    context: &str,
    strategy: banken::absorb::ListStrategy,
) -> Result<(), String> {
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
    // The strategy is part of the receipt, not a hidden default: which read
    // path was used decides what a missing table MEANS (a `Streaming` read
    // against a server that does not negotiate it stalls in `Absorbing` with no
    // error), so the operator must be able to see it without guessing.
    label.push_str(" [");
    label.push_str(strategy.label());
    label.push(']');
    // The WATCH producer, not the poll. This is the whole M0 payoff: against
    // camelot-eks (191 pods) the poll moved 3,580,862 B every second — 96 GiB
    // per 8-hour day — where a watch over the same 30 s moved 0 bytes, because
    // delta traffic is proportional to CHANGE and poll traffic to STATE SIZE.
    //
    // The app is handed a `Despensa` and never learns which producer filled it;
    // the fixture path below uses `with_feed`, which drives the SAME reader
    // through a poll. One reader type, two producers — a consumer adapts by
    // reading a declared capability, never by branching on its backend.
    let (despensa, publisher) = banken::absorb::channel();
    let _absorber = env.spawn_pod_absorber(publisher, strategy);
    let mut app = BankenApp::try_new(env, session_env(), operator, label)
        .map_err(|e| e.to_string())?
        .with_cluster(cluster)
        .with_absorber(despensa);
    egaku_term::run_async(&mut app)
        .await
        .map_err(|e| e.to_string())
}

/// The `:pods` refresh period, read from the AUTHORED config.
///
/// This function is the whole reason `banken-config` is now a dependency. The
/// interval was a hardcoded `feed::DEFAULT_POLL`, so `:refresh-interval-ms` in
/// a `(defbanken …)` form was authored, tested, and **ignored** — the config
/// crate's 16 tests were green the entire time it had no consumer.
///
/// A discovery failure falls back to the documented default rather than
/// refusing to start: banken must run on a machine that has never configured
/// it, and a navigator that will not open because a config file is malformed
/// is worse than one that opens on its prescribed defaults and says so.
///
/// `0` means "never auto-refresh" in the authored vocabulary — the
/// zero-opinion floor — and is honoured as `DEFAULT_POLL` here only because
/// the feed has no manual-only mode yet; that gap is
/// `pending-banken: manual-refresh-mode` rather than a silent reinterpretation
/// of an authored zero.
fn refresh_interval() -> std::time::Duration {
    let ms = banken_config::BankenConfig::discover_effective()
        .map(|c| c.refresh_interval_ms)
        .unwrap_or_else(|_| u64::try_from(banken::feed::DEFAULT_POLL.as_millis()).unwrap_or(1000));
    if ms == 0 {
        return banken::feed::DEFAULT_POLL;
    }
    std::time::Duration::from_millis(ms)
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
    println!(
        "  --live           read from the live cluster{}",
        live_availability()
    );
    println!("  --context <name> the kubeconfig context to read — REQUIRED with --live.");
    println!("                   banken will not read \"whatever current-context happens");
    println!("                   to be\": a merged KUBECONFIG routinely points at a");
    println!("                   different estate, and a pod table from the wrong cluster");
    println!("                   looks exactly like one from the right cluster.");
    // Derived from the enum, never a hand-typed list — the same rule the KEYS
    // block below already follows. A variant added without a help line is the
    // drift this avoids.
    print!("  --list-strategy  how the initial set is read: ");
    let mut first = true;
    for s in banken::absorb::ListStrategy::ALL {
        if !first {
            print!(" | ");
        }
        print!("{}", s.label());
        first = false;
    }
    println!();
    println!(
        "                   default: {}. There is deliberately no `auto` —",
        banken::absorb::ListStrategy::default().label()
    );
    println!("                   falling back to a different read path than the one you");
    println!("                   named is an unannounced downgrade, and a conformance-");
    println!("                   partial apiserver is exactly when you need to know which");
    println!("                   path you got. `streaming` against a server that does not");
    println!("                   negotiate it stalls in `absorbing` with no rows and no");
    println!("                   error; `list-watch` sends nothing a minimal server lacks.");
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
