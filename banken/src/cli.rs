//! The typed CLI surface — argv in, an [`Invocation`] out, or a typed
//! [`CliError`].
//!
//! # The landing: banken opens on a chooser, not on data
//!
//! With no arguments banken lands on [`Invocation::Pick`] — the cluster picker
//! ([`crate::picker`]). It used to land on the fixture, i.e. on five invented
//! pods that look exactly like a cluster, which is not a defensible default
//! for a navigator. The fixture is still reachable, and now says so out loud:
//! `banken --fixture` (★★ MODULARIZE, DON'T DELETE — the capability is
//! retired from the *default*, not removed).
//!
//! `--live` with no `--context` is [`Invocation::Pick`] as well. That was
//! previously a refusal that printed every context name and exited, which is
//! the shape of a tool that knows the answer and declines to act on it. **The
//! invariant below is unchanged by that**: picking produces the same
//! non-optional `String` typing it does, and does so from a list that shows
//! each name's apiserver and refuses the ambiguous ones — see
//! [`crate::picker`].
//!
//! # Why this is a module and not an inline scan in `main`
//!
//! The flag it exists for is [`Invocation::Live`]'s `context`, and that field
//! is **required** for a measured reason rather than a stylistic one.
//!
//! `banken --live` used to ride `kube::Client::try_default()`, which resolves
//! the kubeconfig's `current-context`. On an operator workstation with a
//! merged `KUBECONFIG` that is routinely a *different estate* than the one
//! they mean to look at — measured 2026-07-31 on this machine: the
//! current-context was `beta-eks` (a different estate) while the
//! cluster under inspection was `alpha-eks`. banken would have rendered a
//! pod table from the wrong estate and reported nothing unusual, because from
//! the inside there IS nothing unusual: the read succeeds, the rows are real,
//! only the *cluster* is wrong.
//!
//! That is the same failure class [`banken_spec::bancada`] already refuses one
//! layer down — a pre-warmed pane opened without an explicit `--context` lands
//! on "whatever the shell happens to be on", so an unknown cluster is an
//! [`banken_spec::error::SpecError::UnresolvedContextField`] refusal rather
//! than a `--context ""`. Reading an *unnamed* estate is the same lie one
//! layer up, so it gets the same answer: **refuse and name what you would have
//! got**, never guess.
//!
//! Honest tier: this is **parse-time-rejected**, not truly-unrepresentable.
//! `Invocation::Live` carries a non-optional `String`, so no code path past
//! [`parse_args`] can run live against an unnamed context — but
//! [`crate::live::KubeClusterEnv::connect`] still exists (it is the honest
//! in-cluster / explicit-caller constructor), so the *library* can still do
//! it. The CLI cannot.
//!
//! # A refusal is a one-keystroke fix, not a wall
//!
//! Every [`CliError`] below names what to type instead — the legal strategy
//! values, the flag that was misspelled. The one refusal that could not do
//! that usefully (a `--live` with no context, which needed a list of eighteen
//! names) is no longer a refusal at all: it is the picker.

/// What the operator asked banken to do.
///
/// A closed sum: every accepted argv shape is exactly one of these, so "the
/// CLI parsed but nobody decided what to run" has no value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Invocation {
    /// `--help` / `-h`.
    Help,
    /// `--fixture`: the `:pods` navigator over the canned fixture source.
    ///
    /// Explicit since it stopped being the default. The rows are invented, and
    /// a flag the operator typed is the honest way to ask for that.
    Fixture,
    /// The default, and `--live` with no `--context`: open the cluster picker,
    /// then run live against whatever the operator chooses.
    ///
    /// Carries the strategy so `banken --list-strategy list-watch` selects the
    /// read path for the run the picker is about to start — the flag describes
    /// the session, not the argv shape.
    Pick {
        /// How the initial object set will be obtained once a context is
        /// chosen. See [`Invocation::Live::strategy`].
        strategy: crate::absorb::ListStrategy,
    },
    /// `--live --context <name>`: the `:pods` navigator over the named
    /// kubeconfig context.
    ///
    /// The context is a plain `String` and **not** an `Option` on purpose —
    /// see the module docs. An unnamed live run is rejected at the parse
    /// boundary and therefore has no representation here.
    Live {
        /// The kubeconfig context to read. Never empty:
        /// [`parse_args`] rejects `--context ""` as
        /// [`CliError::EmptyContext`].
        context: String,
        /// How the initial object set is obtained.
        ///
        /// Defaults to [`ListStrategy::Streaming`] when unnamed. It is carried
        /// here rather than resolved later so the value that selected the read
        /// path and the value banken reports are the same one — the same
        /// reasoning that makes `context` a non-optional `String`.
        strategy: crate::absorb::ListStrategy,
    },
    /// `banken mcp --context <name>` / `banken mcp --fixture`: serve the
    /// OBSERVE half over MCP on stdio instead of drawing a screen.
    ///
    /// # Why the source is not an `Option`
    ///
    /// Same wrong-estate invariant as [`Invocation::Live`], with one rung
    /// added: the escape hatch that makes an unnamed live run acceptable —
    /// *ask the operator* — **does not exist here**. stdout carries the
    /// JSON-RPC framing, so a picker cannot be drawn, and there is nobody at a
    /// keyboard to answer it. An MCP server that rode `current-context` would
    /// silently serve whichever estate the merged `KUBECONFIG` happens to
    /// point at, to a *reader who cannot see the mistake* — an agent has no
    /// peripheral vision for a title bar naming the wrong cluster. So the
    /// unnamed case is refused at the parse boundary and has no value here.
    Mcp {
        /// Where the served reads come from.
        source: McpSource,
    },
}

/// Which cluster an [`Invocation::Mcp`] serves.
///
/// A closed sum rather than an `Option<String>`, so "serving the fixture" is a
/// thing the operator *said* rather than the absence of a thing they didn't.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum McpSource {
    /// `--context <name>`: a real kubeconfig context.
    Live(String),
    /// `--fixture`: the canned source, for wiring an agent up without a
    /// cluster.
    Fixture,
}

/// Why an argv was refused.
///
/// Typed rather than a `String` so `main` can *enrich* the one case that has
/// useful runtime data to add (the available contexts) without string-matching
/// a message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CliError {
    /// `--list-strategy <x>` naming something that is not a strategy.
    ///
    /// A refusal rather than a fall back to the default: reading via a
    /// different strategy than the operator named is an unannounced downgrade,
    /// which is exactly what `ListStrategy` has no `Auto` variant to prevent.
    UnknownListStrategy(String),
    /// `--list-strategy` with no value after it.
    ListStrategyWithoutValue,
    /// `--fixture` alongside `--live` or `--context`.
    ///
    /// Two sources named at once. Refused rather than ranked: silently
    /// preferring one would mean the operator who typed both cannot tell from
    /// the argv which rows they are about to look at, and "are these rows
    /// real" is the one question banken must never leave ambiguous.
    ConflictingSources,
    /// `--context` as the last argument, with nothing after it.
    ContextWithoutValue,
    /// `--context ""` / `--context=`. An empty context is exactly the
    /// "unknown cluster" value the whole flag exists to make impossible.
    EmptyContext,
    /// A flag banken does not know. Refused rather than ignored: a silently
    /// dropped `--contxt` would run against the fixture while the operator
    /// believed they had selected a cluster.
    UnknownFlag(String),
    /// `banken mcp` with neither `--context` nor `--fixture`.
    ///
    /// The TUI can answer an unnamed run by asking. An MCP server cannot: its
    /// stdout is the protocol, and its reader is an agent that would take the
    /// wrong estate's rows at face value.
    McpWithoutSource,
}

impl std::fmt::Display for CliError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CliError::ConflictingSources => f.write_str(
                "--fixture names the canned source and --live/--context names a real \
                 cluster; banken will not pick one for you. \"Are these rows real\" is the \
                 one question the argv must answer unambiguously.",
            ),
            CliError::ContextWithoutValue => {
                f.write_str("--context requires a value: --context <name>")
            }
            CliError::ListStrategyWithoutValue => f.write_str(
                "--list-strategy requires a value: --list-strategy <streaming|list-watch>",
            ),
            CliError::UnknownListStrategy(got) => {
                // The refusal NAMES the legal values, because the cost of this
                // refusal must be one keystroke rather than a detour — the same
                // reasoning as the --context refusal listing every context.
                f.write_str("unknown --list-strategy `")?;
                f.write_str(got)?;
                f.write_str("`. Legal values: ")?;
                let mut first = true;
                for s in crate::absorb::ListStrategy::ALL {
                    if !first {
                        f.write_str(", ")?;
                    }
                    f.write_str(s.label())?;
                    first = false;
                }
                f.write_str(
                    ". There is deliberately no `auto`: falling back to a different read \
                     path than the one you named is an unannounced downgrade, and a \
                     conformance-partial apiserver is exactly when you need to know which \
                     path you got.",
                )
            }
            CliError::EmptyContext => f.write_str(
                "--context was given an empty name — an empty context IS the unknown-cluster \
                 value this flag exists to make impossible",
            ),
            CliError::McpWithoutSource => f.write_str(
                "`banken mcp` needs a source: `mcp --context <name>` for a cluster, or \
                 `mcp --fixture` for the canned one. Unlike the TUI, this cannot fall back \
                 to asking — stdout carries the MCP protocol, so there is no screen to draw \
                 a picker on, and the reader is an agent that cannot notice it is looking at \
                 the wrong estate.",
            ),
            CliError::UnknownFlag(flag) => {
                f.write_str("unknown flag `")?;
                f.write_str(flag)?;
                f.write_str(
                    "` (see --help). banken refuses unknown flags rather than \
                             ignoring them: a dropped flag runs something other than what \
                             you asked for.",
                )
            }
        }
    }
}

impl std::error::Error for CliError {}

/// Parse banken's argv (already stripped of `argv[0]`).
///
/// Accepted shapes:
///
/// ```text
/// banken                            → Invocation::Pick     (the cluster picker)
/// banken :pods                      → Invocation::Pick
/// banken --live                     → Invocation::Pick
/// banken --context <name>           → Invocation::Live { context }
/// banken --live --context <name>    → Invocation::Live { context }
/// banken --live --context=<name>    → Invocation::Live { context }
/// banken --fixture                  → Invocation::Fixture
/// banken --help | -h                → Invocation::Help
/// ```
///
/// `--live` is now optional in front of `--context`: naming a cluster **is**
/// asking to read it, and there is no longer a fixture default for the name
/// to be misattached to. It is still accepted, because operators have it in
/// their shell history.
///
/// `--help` wins over everything else — asking for help must never be
/// refused for an unrelated flag error.
///
/// A leading `:view` token is accepted and currently ignored (the only M0
/// view is `:pods`), matching the pre-existing behaviour.
///
/// # Errors
///
/// Any [`CliError`].
pub fn parse_args(args: &[String]) -> Result<Invocation, CliError> {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        return Ok(Invocation::Help);
    }

    let mut want_fixture = false;
    let mut want_live = false;
    let mut want_mcp = false;
    let mut context: Option<String> = None;
    let mut strategy = crate::absorb::ListStrategy::default();
    let mut i = 0;
    while i < args.len() {
        let arg = args[i].as_str();
        match arg {
            "--live" => want_live = true,
            "--fixture" => want_fixture = true,
            "--context" => {
                let value = args.get(i + 1).ok_or(CliError::ContextWithoutValue)?;
                // A value that is itself a flag means the name was forgotten:
                // `banken --live --context --foo` must not read a cluster
                // called "--foo".
                if value.starts_with('-') {
                    return Err(CliError::ContextWithoutValue);
                }
                context = Some(value.clone());
                i += 1;
            }
            _ if arg.starts_with("--context=") => {
                context = Some(arg["--context=".len()..].to_owned());
            }
            "--list-strategy" => {
                let value = args.get(i + 1).ok_or(CliError::ListStrategyWithoutValue)?;
                if value.starts_with('-') {
                    return Err(CliError::ListStrategyWithoutValue);
                }
                strategy = crate::absorb::ListStrategy::parse(value)
                    .ok_or_else(|| CliError::UnknownListStrategy(value.clone()))?;
                i += 1;
            }
            _ if arg.starts_with("--list-strategy=") => {
                let value = &arg["--list-strategy=".len()..];
                strategy = crate::absorb::ListStrategy::parse(value)
                    .ok_or_else(|| CliError::UnknownListStrategy(value.to_owned()))?;
            }
            // A `:view` token — accepted, and currently routed to `:pods`.
            _ if arg.starts_with(':') => {}
            _ if arg.starts_with('-') => return Err(CliError::UnknownFlag(arg.to_owned())),
            // The one meaningful positional: serve MCP instead of a screen.
            "mcp" => want_mcp = true,
            // Any other bare positional. Same treatment as `:view`.
            _ => {}
        }
        i += 1;
    }

    if want_fixture && (want_live || context.is_some()) {
        return Err(CliError::ConflictingSources);
    }
    // Resolved BEFORE the screen-landing arms, because `mcp` decides what kind
    // of process this is, not merely which rows it shows. Note it reuses the
    // same `ConflictingSources` check above — naming two sources is exactly as
    // ambiguous for an agent as it is for a human.
    if want_mcp {
        return match context {
            Some(c) if c.is_empty() => Err(CliError::EmptyContext),
            Some(c) => Ok(Invocation::Mcp {
                source: McpSource::Live(c),
            }),
            None if want_fixture => Ok(Invocation::Mcp {
                source: McpSource::Fixture,
            }),
            // `mcp --live` lands here too, and that is correct: `--live` means
            // "a real cluster, you pick which", and there is nothing to pick
            // with.
            None => Err(CliError::McpWithoutSource),
        };
    }
    if want_fixture {
        return Ok(Invocation::Fixture);
    }
    match context {
        Some(c) if c.is_empty() => Err(CliError::EmptyContext),
        Some(context) => Ok(Invocation::Live { context, strategy }),
        // No name given — including for a bare `--live`. The picker asks,
        // rather than guessing at a `current-context` or refusing with a list
        // the operator would then have to retype from.
        None if want_live => Ok(Invocation::Pick { strategy }),
        None => Ok(default_landing(strategy)),
    }
}

/// Where a bare `banken` lands, DERIVED from whether a live backend is
/// compiled in rather than asserted.
///
/// The two `#[cfg]` arms below **are** the derivation, the same shape
/// `main`'s `live_availability()` uses: a build with no kube client cannot
/// offer a chooser over clusters it could not then read, and must land on the
/// only source it has. Neither arm can go stale, because there is no third
/// place stating what the default is.
#[cfg(feature = "live")]
fn default_landing(strategy: crate::absorb::ListStrategy) -> Invocation {
    Invocation::Pick { strategy }
}

#[cfg(not(feature = "live"))]
fn default_landing(_strategy: crate::absorb::ListStrategy) -> Invocation {
    Invocation::Fixture
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Invocation, CliError> {
        let owned: Vec<String> = args.iter().map(|s| (*s).to_owned()).collect();
        parse_args(&owned)
    }

    /// **THE LANDING GATE.** Bare `banken` must not open on invented rows.
    /// It did — five fixture pods, labelled in the status line and otherwise
    /// indistinguishable from a cluster — and a navigator whose default screen
    /// is fabricated data is the defect the picker exists to fix.
    #[cfg(feature = "live")]
    #[test]
    fn the_default_landing_is_the_cluster_picker() {
        let pick = Invocation::Pick {
            strategy: crate::absorb::ListStrategy::default(),
        };
        assert_eq!(parse(&[]), Ok(pick.clone()));
        assert_eq!(parse(&[":pods"]), Ok(pick));
    }

    /// **The agent surface's own wrong-estate gate.** `banken mcp` with no
    /// source is refused, and this is a *stricter* rule than the TUI's on
    /// purpose: the TUI answers an unnamed run by opening the picker, and an
    /// MCP server has neither a screen to draw one on nor a human to answer
    /// it. Falling back to `current-context` would serve some other estate's
    /// rows to a reader with no way to notice.
    #[test]
    fn mcp_refuses_to_guess_which_estate_it_serves() {
        assert_eq!(parse(&["mcp"]), Err(CliError::McpWithoutSource));
        // `--live` names "a real cluster, you pick which" — and there is
        // nothing here to pick with, so it is the same refusal.
        assert_eq!(parse(&["mcp", "--live"]), Err(CliError::McpWithoutSource));
        // The refusal must name both roads out, or it costs a detour.
        let msg = CliError::McpWithoutSource.to_string();
        assert!(msg.contains("--context"), "{msg}");
        assert!(msg.contains("--fixture"), "{msg}");
    }

    #[test]
    fn mcp_takes_either_source_by_name() {
        assert_eq!(
            parse(&["mcp", "--context", "alpha-eks"]),
            Ok(Invocation::Mcp {
                source: McpSource::Live("alpha-eks".to_owned()),
            }),
        );
        assert_eq!(
            parse(&["mcp", "--context=alpha-eks"]),
            Ok(Invocation::Mcp {
                source: McpSource::Live("alpha-eks".to_owned()),
            }),
        );
        assert_eq!(
            parse(&["mcp", "--fixture"]),
            Ok(Invocation::Mcp {
                source: McpSource::Fixture,
            }),
        );
    }

    /// Naming two sources is exactly as ambiguous for an agent as for a human,
    /// and an empty context is the unknown-cluster value either way — so `mcp`
    /// inherits both refusals rather than re-deciding them.
    #[test]
    fn mcp_inherits_the_source_refusals() {
        assert_eq!(
            parse(&["mcp", "--fixture", "--context", "alpha-eks"]),
            Err(CliError::ConflictingSources),
        );
        assert_eq!(parse(&["mcp", "--context="]), Err(CliError::EmptyContext));
    }

    /// The fixture is retired from the default, not removed — it is now one
    /// explicit flag away (★★ MODULARIZE, DON'T DELETE).
    #[test]
    fn the_fixture_is_still_reachable_by_name() {
        assert_eq!(parse(&["--fixture"]), Ok(Invocation::Fixture));
    }

    /// Naming both sources is refused, never ranked: an operator who typed
    /// both cannot otherwise tell which rows they are about to look at.
    #[test]
    fn naming_two_sources_is_refused() {
        assert_eq!(
            parse(&["--fixture", "--live"]),
            Err(CliError::ConflictingSources),
        );
        assert_eq!(
            parse(&["--fixture", "--context", "alpha-eks"]),
            Err(CliError::ConflictingSources),
        );
    }

    /// A build with no kube client has no chooser to offer, so it lands on the
    /// only source it has — stated once, in `default_landing`'s two `#[cfg]`
    /// arms, and asserted here rather than left to the reader.
    #[cfg(not(feature = "live"))]
    #[test]
    fn without_the_live_backend_the_default_landing_is_the_fixture() {
        assert_eq!(parse(&[]), Ok(Invocation::Fixture));
    }

    #[test]
    fn help_wins_over_everything() {
        assert_eq!(parse(&["--help"]), Ok(Invocation::Help));
        assert_eq!(parse(&["-h"]), Ok(Invocation::Help));
        // Even alongside an otherwise-refused argv: asking how to use the
        // tool must not be refused for using it wrong.
        assert_eq!(parse(&["--live", "--help"]), Ok(Invocation::Help));
    }

    #[test]
    fn a_named_context_selects_the_live_source() {
        assert_eq!(
            parse(&["--live", "--context", "alpha-eks"]),
            Ok(Invocation::Live {
                context: "alpha-eks".into(),
                strategy: crate::absorb::ListStrategy::Streaming,
            }),
        );
        assert_eq!(
            parse(&["--live", "--context=alpha-eks"]),
            Ok(Invocation::Live {
                context: "alpha-eks".into(),
                strategy: crate::absorb::ListStrategy::Streaming,
            }),
        );
        // Order-independent, and a `:view` token does not disturb it.
        assert_eq!(
            parse(&[":pods", "--context", "bravo", "--live"]),
            Ok(Invocation::Live {
                context: "bravo".into(),
                strategy: crate::absorb::ListStrategy::Streaming,
            }),
        );
    }

    /// **THE GATE, and it is unchanged in substance.** `--live` alone used to
    /// mean "read whatever the kubeconfig's current-context happens to be",
    /// which on this machine was an entirely different estate
    /// (`us-east-2-staging-eks`) than the one under inspection
    /// (`alpha-eks`). banken still never guesses — it now *asks*, which is
    /// strictly more than the old refusal did and still produces a name the
    /// operator chose.
    ///
    /// What must remain true is the type-level half: no accepted argv yields a
    /// live run whose context is absent. `Invocation::Live` carries a
    /// non-optional `String`, so that is enforced by construction; this
    /// asserts the parser never reaches it without one.
    #[test]
    fn live_without_a_context_asks_instead_of_guessing() {
        assert_eq!(
            parse(&["--live"]),
            Ok(Invocation::Pick {
                strategy: crate::absorb::ListStrategy::default()
            }),
        );
        // The gate itself: every parse that DOES yield a live run names a
        // non-empty context.
        for args in [
            vec!["--live"],
            vec![],
            vec!["--live", "--context", "alpha-eks"],
            vec!["--context", "bravo"],
            vec!["--fixture"],
        ] {
            if let Ok(Invocation::Live { context, .. }) = parse(&args) {
                assert!(!context.is_empty(), "{args:?} produced an unnamed live run");
            }
        }
    }

    /// Naming a context no longer needs `--live` in front of it: with no
    /// fixture default left for the name to be misattached to, naming a
    /// cluster IS asking to read it.
    #[test]
    fn a_context_alone_selects_the_live_source() {
        assert_eq!(
            parse(&["--context", "alpha-eks"]),
            Ok(Invocation::Live {
                context: "alpha-eks".into(),
                strategy: crate::absorb::ListStrategy::Streaming,
            }),
        );
    }

    #[test]
    fn a_context_with_no_value_is_refused() {
        assert_eq!(
            parse(&["--live", "--context"]),
            Err(CliError::ContextWithoutValue),
        );
        // The next token being a flag means the NAME was forgotten — banken
        // must not read a cluster called "--live".
        assert_eq!(
            parse(&["--context", "--live"]),
            Err(CliError::ContextWithoutValue),
        );
    }

    #[test]
    fn an_empty_context_is_refused() {
        assert_eq!(
            parse(&["--live", "--context", ""]),
            Err(CliError::EmptyContext),
        );
        assert_eq!(
            parse(&["--live", "--context="]),
            Err(CliError::EmptyContext)
        );
    }

    /// An unknown flag is refused, never ignored. A silently-dropped
    /// `--contxt alpha-eks` would have run against the fixture while the
    /// operator believed they had selected a cluster — the same
    /// wrong-source class one typo away.
    #[test]
    fn an_unknown_flag_is_refused_by_name() {
        match parse(&["--contxt", "alpha-eks"]) {
            Err(CliError::UnknownFlag(f)) => assert_eq!(f, "--contxt"),
            other => panic!("expected UnknownFlag, got {other:?}"),
        }
    }
}

#[cfg(test)]
mod list_strategy_cli_tests {
    use super::*;
    use crate::absorb::ListStrategy;

    fn parse(args: &[&str]) -> Result<Invocation, CliError> {
        parse_args(&args.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>())
    }

    #[test]
    fn the_strategy_defaults_to_streaming_when_unnamed() {
        assert_eq!(
            parse(&["--live", "--context", "alpha-eks"]),
            Ok(Invocation::Live {
                context: "alpha-eks".into(),
                strategy: ListStrategy::Streaming,
            })
        );
    }

    #[test]
    fn both_spellings_select_the_strategy() {
        for args in [
            vec!["--live", "--context", "c", "--list-strategy", "list-watch"],
            vec!["--live", "--context", "c", "--list-strategy=list-watch"],
        ] {
            assert_eq!(
                parse(&args),
                Ok(Invocation::Live {
                    context: "c".into(),
                    strategy: ListStrategy::ListWatch,
                }),
                "{args:?}"
            );
        }
    }

    /// The refusal that carries the design.
    ///
    /// A typo must NOT resolve to the default. Reading via a different strategy
    /// than the operator named is an unannounced downgrade — and a
    /// conformance-partial apiserver is exactly the situation where knowing
    /// which read path you got is the whole question. `auto` is refused for the
    /// same reason: there is no such variant, by design.
    #[test]
    fn a_misspelled_strategy_is_refused_and_the_refusal_names_the_legal_values() {
        let err = parse(&["--live", "--context", "c", "--list-strategy", "streming"])
            .expect_err("a typo must be refused, never defaulted");
        assert_eq!(err, CliError::UnknownListStrategy("streming".into()));

        let msg = err.to_string();
        for legal in ListStrategy::ALL {
            assert!(
                msg.contains(legal.label()),
                "the refusal must name `{}` so the fix is one keystroke; got: {msg}",
                legal.label()
            );
        }
        assert!(
            msg.contains("auto"),
            "the refusal must say why there is no auto"
        );

        assert_eq!(
            parse(&["--live", "--context", "c", "--list-strategy", "auto"]),
            Err(CliError::UnknownListStrategy("auto".into())),
            "`auto` is not a strategy — the absence is the design"
        );
    }

    #[test]
    fn a_valueless_strategy_flag_is_refused() {
        assert_eq!(
            parse(&["--live", "--context", "c", "--list-strategy"]),
            Err(CliError::ListStrategyWithoutValue)
        );
        // A following flag means the value was forgotten — the same shape as
        // `--context --foo` refusing to read a cluster called "--foo".
        assert_eq!(
            parse(&["--live", "--list-strategy", "--context", "c"]),
            Err(CliError::ListStrategyWithoutValue)
        );
    }
}
