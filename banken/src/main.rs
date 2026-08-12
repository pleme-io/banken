//! banken — the entry point.
//!
//! # The landing
//!
//! Bare `banken` opens the **cluster picker** ([`banken::picker`]): every
//! kubeconfig context, each with the apiserver it resolves to, fuzzy-filtered,
//! and the ambiguous ones marked unchoosable. Pick one and the `:pods`
//! navigator opens on it.
//!
//! It used to open on the fixture — five invented pods that look exactly like
//! a cluster — and `--live` alone used to print eighteen context names and
//! exit, leaving the operator to retype one of them. Neither is a defensible
//! first screen for a navigator.
//!
//! **The wrong-estate invariant is unchanged.** A live run still requires a
//! named context: riding the kubeconfig's `current-context` reads whichever
//! estate the merged `KUBECONFIG` happens to point at, and a pod table from
//! the wrong cluster is indistinguishable from one from the right cluster.
//! Picking *is* naming — [`banken::cli::Invocation::Live`] carries the same
//! non-optional `String` either way — and it names from a list that shows the
//! URL and refuses a name two files declare. See [`banken::cli`] and
//! [`banken::picker`] for the measurements.
//!
//! Usage:
//! ```text
//! banken                                  # choose a cluster, then :pods
//! banken --context camelot-eks            # :pods on that cluster directly
//! banken --fixture                        # the canned source, explicitly
//! banken --help                           # usage
//! ```

use banken::app::BankenApp;
use banken::cli::{Invocation, parse_args};
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
            Invocation::Pick { strategy } => run_pick(operator, strategy).await,
            // `.map(|_| ())` on purpose: a cancelled connect is a decision,
            // not a failure, so `--context` exits 0 in silence exactly as a
            // successful run does. Both arms of the `live` cfg satisfy this.
            Invocation::Live { context, strategy } => run_live(operator, &context, strategy)
                .await
                .map(|_landed| ()),
        }
    });

    if let Err(msg) = exit {
        eprintln!("banken: {msg}");
        std::process::exit(1);
    }
}

/// The landing: choose a cluster, then open `:pods` on it.
///
/// Two `run_async` calls in sequence rather than one app with a stage flag,
/// and that is structural rather than stylistic. [`BankenApp`] is generic over
/// its [`ClusterEnv`](banken_spec::env::ClusterEnv), so the env must exist
/// before the app does — a single app spanning both screens would have to hold
/// an `Option<E>` and answer "which cluster am I reading" with `None` for the
/// whole first screen, which is exactly the unresolved-source state the type
/// parameter exists to make impossible.
///
/// Leaving the picker without choosing is **not** an error: `esc` is a
/// decision, and exiting 0 in silence is the honest response to it.
///
/// # A failed connect returns to the LIST, not to a shell
///
/// `pending-banken: reconnect-on-failed-pick` — CLOSED. Picking a cluster the
/// VPN cannot reach used to print an error and exit, having thrown away the
/// list the operator was choosing from; the next attempt started with
/// `banken` again from a cold prompt. The loop below re-enters the picker
/// carrying the reason, so the other seventeen contexts are still on screen
/// and the retry costs one keystroke.
///
/// The loop terminates because it advances only on a *successful* connect
/// (which returns) or on the operator declining to choose (which returns) —
/// a failure re-enters exactly once per failure, driven by a keystroke.
#[cfg(feature = "live")]
async fn run_pick(
    operator: OperatorId,
    strategy: banken::absorb::ListStrategy,
) -> Result<(), String> {
    let catalog = banken_spec::load_catalog().map_err(|e| e.to_string())?;

    // ── The watchdog's rounds, started ONCE and outliving every picker entry ──
    //
    // Outside the loop deliberately. A failed connect re-enters the list, and
    // restarting the rounds there would blink all eighteen markers back to
    // `probing` at exactly the moment the operator is scanning for one that is
    // up — i.e. the retry would destroy the information the retry needs.
    //
    // The enumeration is a second filesystem read of the kubeconfig (the
    // picker does its own, per entry, so that an edit mid-session is picked
    // up). That is one extra YAML parse at startup and no network, which is a
    // fair price for rounds whose lifetime is the session rather than the
    // screen.
    let (ronda, publisher) = banken::ronda::channel();
    let _rounds = banken::live::enumerate_contexts().ok().map(|contexts| {
        banken::ronda::spawn_rounds(
            contexts
                .into_iter()
                .map(|c| (c.name, c.server))
                .collect::<Vec<_>>(),
            publisher,
        )
    });

    let mut notice: Option<String> = None;
    loop {
        let mut picker = banken::picker::ContextPicker::try_new(&catalog)
            .map_err(|e| e.to_string())?
            .with_ronda(ronda.clone());
        if let Some(n) = notice.take() {
            picker = picker.with_notice(n);
        }
        egaku_term::run_async(&mut picker)
            .await
            .map_err(|e| e.to_string())?;
        let Some(choice) = picker.chosen().cloned() else {
            return Ok(());
        };
        match run_live_from(operator.clone(), &choice.name, choice.server.clone(), strategy).await {
            Ok(Landed::Opened) => return Ok(()),
            // The operator changed their mind mid-connect. Back to the list
            // with NO notice: a cancel is a decision, and captioning it with
            // an error would read as though something had gone wrong.
            Ok(Landed::Cancelled) => notice = None,
            // Back to the list with the reason, rather than out to a prompt.
            Err(e) => notice = Some(e),
        }
    }
}

/// Whether a live run reached the navigator.
///
/// A cancelled connect is NOT an error and must not be reported as one — the
/// distinction is why this is a typed outcome rather than `Result<(), String>`
/// with a magic message. `--context` exits 0 in silence; the picker loop
/// re-enters the list without a red footer.
#[cfg(feature = "live")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Landed {
    Opened,
    Cancelled,
}

/// Without the `live` feature there are no clusters to choose between, so the
/// picker is a typed error rather than a silent fall-through to the fixture —
/// which would answer "show me my clusters" with invented rows.
#[cfg(not(feature = "live"))]
async fn run_pick(
    _operator: OperatorId,
    _strategy: banken::absorb::ListStrategy,
) -> Result<(), String> {
    Err(
        "choosing a cluster requires building with `--features live` (kube backend); \
         this binary ships the fixture source only — run `banken --fixture`"
            .into(),
    )
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
    "  (unavailable — rebuild with `--features live`)"
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
) -> Result<Landed, String> {
    run_live_from(operator, context, None, strategy).await
}

/// Run the navigator over the live kube source, against the **named**
/// kubeconfig context, waiting in [`banken::antessala`].
///
/// `server` is the apiserver URL when the caller already knows it — the picker
/// does, from the row that was chosen. It is passed in rather than resolved
/// again so the wait screen can show the URL from its first frame.
///
/// # The connect runs as a TASK, and that is the whole change
///
/// It used to be awaited inline behind one `eprintln!`, which made the wait
/// structurally un-drawable and un-cancellable: there was no loop to paint a
/// frame from and no other future to select against. Now the connect owns a
/// task publishing [`banken::antessala::Stage`]s, and the antechamber is an
/// ordinary `AsyncApp` watching them — so the wait gets a screen, a timer, and
/// an `esc`.
///
/// The context banken connected with is the context banken reports: it is
/// carried into the status line and into every `(defbancada)`'s
/// `(:context cluster)` from the same `String` that selected the apiserver, so
/// the two cannot drift.
#[cfg(feature = "live")]
async fn run_live_from(
    operator: OperatorId,
    context: &str,
    server: Option<String>,
    strategy: banken::absorb::ListStrategy,
) -> Result<Landed, String> {
    use banken::antessala::{ConnectingScreen, SettleOnDrop, Stage, Waited, channel};
    use banken::live::KubeClusterEnv;

    let (reporter, watch) = channel();
    let owned = context.to_owned();

    // EVERYTHING up to the first drawable frame happens in here, not just the
    // connect. Ending the task at the connect is what the first version did,
    // and it left the operator staring at a blank terminal through
    // `try_new`'s synchronous pod listing — the wait relocated rather than
    // removed. The rule this encodes: the antechamber closes when there is
    // something to draw, never when one named step finishes.
    let build = tokio::spawn(async move {
        // Armed for the whole body. Whatever happens from here — an error, a
        // panic, an `abort()` dropping this future mid-poll — `Settled` is
        // published and the screen cannot hang.
        let settle = SettleOnDrop::new(reporter);
        let env = KubeClusterEnv::connect_with_context_staged(&owned, settle.reporter())
            .await
            .map_err(|e| {
                let mut m = String::from("live connect failed (VPN/kubeconfig?): ");
                m.push_str(&e.to_string());
                m
            })?;
        // The env reports the context it was CONSTRUCTED with, so this is the
        // same value throughout. An empty one is impossible here (the CLI
        // rejects it), which is what makes the bancada `(:context cluster)`
        // resolution trustworthy rather than merely populated.
        let cluster = env.context_name().unwrap_or_default();
        let label = banken::absorb::live_source_label(&cluster, env.server(), strategy);
        let receipt = env.server().map(str::to_owned);
        // The WATCH producer, not the poll. This is the whole M0 payoff:
        // against camelot-eks (191 pods) the poll moved 3,580,862 B every
        // second — 96 GiB per 8-hour day — where a watch over the same 30 s
        // moved 0 bytes, because delta traffic is proportional to CHANGE and
        // poll traffic to STATE SIZE.
        //
        // The app is handed a `Despensa` and never learns which producer
        // filled it; the fixture path uses `with_feed`, which drives the SAME
        // reader through a poll. One reader type, two producers — a consumer
        // adapts by reading a declared capability, never by branching on its
        // backend.
        let (despensa, publisher) = banken::absorb::channel();
        let absorber = env.spawn_pod_absorber(publisher, strategy);
        // `try_new` lists pods synchronously (`app.rs:309`). Against an
        // unreachable cluster that is the longest wait in the whole startup,
        // and it used to happen with the terminal already handed back.
        settle.reporter().reached(Stage::FirstRead);
        let app = BankenApp::try_new(env, session_env(), operator, label)
            .map_err(|e| e.to_string())?
            .with_cluster(cluster.clone())
            .with_absorber(despensa);
        Ok::<_, String>((app, absorber, cluster, receipt))
    });

    let mut waiting = ConnectingScreen::new(context, server, watch);
    egaku_term::run_async(&mut waiting)
        .await
        .map_err(|e| e.to_string())?;

    if waiting.outcome() == Waited::Cancelled {
        // Abort rather than detach. A credential helper left running would
        // keep a subprocess and an SSO round-trip alive behind a screen the
        // operator has already left, and its result would have nowhere to go.
        build.abort();
        return Ok(Landed::Cancelled);
    }

    let (mut app, _absorber, cluster, receipt) = build
        .await
        // A panic inside the build task must surface as an error, never as a
        // silent fall-through to a navigator with no cluster behind it.
        .map_err(|e| {
            let mut m = String::from("the connect task did not finish: ");
            m.push_str(&e.to_string());
            m
        })??;

    egaku_term::run_async(&mut app)
        .await
        .map_err(|e| e.to_string())?;

    // ── The receipt, on the way OUT ──
    //
    // It used to be printed on the way IN, before the alt-screen, so that it
    // would survive on the primary screen after banken exited. That instinct
    // was right and the placement was not: written before the connect, it
    // recorded the estate banken was ABOUT TO attempt — so a run that failed
    // to connect, or one the operator cancelled, still left a line claiming
    // it. Printed here it records the estate that was actually read, which is
    // the thing a receipt is for. It also stops the wait screen having to
    // fight a stray line above it.
    eprint!("banken: read `");
    eprint!("{cluster}");
    match receipt {
        Some(server) => eprintln!("` ({server})"),
        None => eprintln!("`"),
    }
    Ok(Landed::Opened)
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
///
/// The signature tracks the live one exactly (2026-08-09): it had drifted to
/// two parameters against a three-argument call, so `--no-default-features`
/// did not compile at all. A `#[cfg]`-gated arm nothing builds is a claim
/// nothing checks.
#[cfg(not(feature = "live"))]
async fn run_live(
    _operator: OperatorId,
    _context: &str,
    _strategy: banken::absorb::ListStrategy,
) -> Result<(), String> {
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
    println!("Every action is typed into one of three classes and there is no unwitnessed");
    println!("live-mutate path: OBSERVE reads, DECLARE lowers to a full-manifest GitOps");
    println!("change a reconciler applies, BREAK-GLASS is witnessed and logged.");
    println!();
    println!("USAGE:");
    println!(
        "  banken                        choose a cluster, then open :pods on it{}",
        live_availability()
    );
    println!("  banken --context <name>       open :pods on that cluster directly");
    println!("  banken --fixture              explore the interface on canned rows");
    println!("  banken --help");
    println!();
    println!("VIEWS:");
    println!("  :pods            the pod table (default; the only M0 view)");
    println!();
    println!("FLAGS:");
    println!("  --context <name> the kubeconfig context to read. banken never reads");
    println!("                   \"whatever current-context happens to be\": a merged");
    println!("                   KUBECONFIG routinely points at a different estate, and a");
    println!("                   pod table from the wrong cluster looks exactly like one");
    println!("                   from the right cluster. With no name, banken asks.");
    println!("  --fixture        the canned source — invented rows, no cluster read.");
    println!("                   Explicit since it stopped being the default: a navigator");
    println!("                   whose first screen is fabricated data is not a navigator.");
    println!("  --live           accepted for compatibility. Implied by --context; on its");
    println!("                   own it opens the chooser.");
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
    // ── The vocabulary — the SAME `HelpPage` the in-app `h` overlay draws ──
    //
    // This block used to build its own text: it walked the catalog here, and
    // `banken::app` walked it again for the status-line legend. Two faces
    // that build their own text are two things that can disagree, and they
    // did — `--help` advertised `S` for a chord the runtime bound as
    // `shift+s`. There is now one derivation, in `banken_spec::help`, and
    // this is one of its two renderers.
    match banken_spec::load_catalog() {
        Ok(catalog) => {
            let page = banken_spec::help::HelpPage::build(
                &catalog,
                banken_spec::help::Wiring {
                    unbound_actions: &banken::app::unbound_action_names(&catalog),
                    unbound_bancadas: &banken::app::unbound_bancada_names(&catalog),
                },
            );
            for line in page.plain_lines() {
                println!("{line}");
            }
        }
        // Honest: if the vocabulary does not load, say so rather than print a
        // hand-written key list that might not match what would have run.
        Err(e) => println!("  <the authored vocabulary failed to load: {e}>"),
    }
}
