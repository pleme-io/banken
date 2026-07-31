//! The typed CLI surface — argv in, an [`Invocation`] out, or a typed
//! [`CliError`].
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
//! current-context was `us-east-2-staging-eks` (an akeyless cluster) while the
//! cluster under inspection was `camelot-eks`. banken would have rendered a
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
//! # The refusal is a one-keystroke fix, not a wall
//!
//! [`CliError::MissingContext`] renders the base message here; `main` enriches
//! it with the kubeconfig's current-context and the available context names,
//! because those live behind the `live` feature. A refusal that does not tell
//! the operator what to type instead is just friction.

/// What the operator asked banken to do.
///
/// A closed sum: every accepted argv shape is exactly one of these, so "the
/// CLI parsed but nobody decided what to run" has no value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Invocation {
    /// `--help` / `-h`.
    Help,
    /// The default: the `:pods` navigator over the fixture source.
    Fixture,
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
    },
}

/// Why an argv was refused.
///
/// Typed rather than a `String` so `main` can *enrich* the one case that has
/// useful runtime data to add (the available contexts) without string-matching
/// a message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CliError {
    /// `--live` with no `--context`. The wrong-estate hazard; see the module
    /// docs.
    MissingContext,
    /// `--context` without `--live`. Naming a cluster banken is not going to
    /// read would put a real context name in the status line and in every
    /// `(defbancada)` plan while the rows came from the fixture — a lie with
    /// a plausible shape, so it is refused rather than ignored.
    ContextWithoutLive,
    /// `--context` as the last argument, with nothing after it.
    ContextWithoutValue,
    /// `--context ""` / `--context=`. An empty context is exactly the
    /// "unknown cluster" value the whole flag exists to make impossible.
    EmptyContext,
    /// A flag banken does not know. Refused rather than ignored: a silently
    /// dropped `--contxt` would run against the fixture while the operator
    /// believed they had selected a cluster.
    UnknownFlag(String),
}

impl std::fmt::Display for CliError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CliError::MissingContext => f.write_str(
                "--live requires an explicit --context <name>. banken refuses to read \
                 \"whatever the kubeconfig's current-context happens to be\": a merged \
                 KUBECONFIG routinely points at a different estate than the one you mean \
                 to look at, and a pod table from the wrong cluster looks exactly like a \
                 pod table from the right one.",
            ),
            CliError::ContextWithoutLive => f.write_str(
                "--context <name> only applies with --live. Without --live the rows come \
                 from the fixture source, and naming a real cluster would put that name in \
                 the status line and in every (defbancada) plan while nothing had read it.",
            ),
            CliError::ContextWithoutValue => {
                f.write_str("--context requires a value: --context <name>")
            }
            CliError::EmptyContext => f.write_str(
                "--context was given an empty name — an empty context IS the unknown-cluster \
                 value this flag exists to make impossible",
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
/// banken                            → Invocation::Fixture
/// banken :pods                      → Invocation::Fixture
/// banken --live --context <name>    → Invocation::Live { context }
/// banken --live --context=<name>    → Invocation::Live { context }
/// banken --help | -h                → Invocation::Help
/// ```
///
/// `--help` wins over everything else — asking for help must never be
/// refused for an unrelated flag error.
///
/// A leading `:view` token is accepted and currently ignored (the only M0
/// view is `:pods`; the FuzzyPicker command bar is M1), matching the
/// pre-existing behaviour.
///
/// # Errors
///
/// Any [`CliError`]; in particular [`CliError::MissingContext`] for a
/// `--live` with no `--context`.
pub fn parse_args(args: &[String]) -> Result<Invocation, CliError> {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        return Ok(Invocation::Help);
    }

    let mut want_live = false;
    let mut context: Option<String> = None;
    let mut i = 0;
    while i < args.len() {
        let arg = args[i].as_str();
        match arg {
            "--live" => want_live = true,
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
            // A `:view` token — accepted, and currently routed to `:pods`.
            _ if arg.starts_with(':') => {}
            _ if arg.starts_with('-') => return Err(CliError::UnknownFlag(arg.to_owned())),
            // A bare positional. Same treatment as `:view`.
            _ => {}
        }
        i += 1;
    }

    match (want_live, context) {
        (true, Some(c)) if c.is_empty() => Err(CliError::EmptyContext),
        (true, Some(c)) => Ok(Invocation::Live { context: c }),
        (true, None) => Err(CliError::MissingContext),
        (false, Some(_)) => Err(CliError::ContextWithoutLive),
        (false, None) => Ok(Invocation::Fixture),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Invocation, CliError> {
        let owned: Vec<String> = args.iter().map(|s| (*s).to_owned()).collect();
        parse_args(&owned)
    }

    #[test]
    fn the_default_is_the_fixture_source() {
        assert_eq!(parse(&[]), Ok(Invocation::Fixture));
        assert_eq!(parse(&[":pods"]), Ok(Invocation::Fixture));
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
            parse(&["--live", "--context", "camelot-eks"]),
            Ok(Invocation::Live {
                context: "camelot-eks".into()
            }),
        );
        assert_eq!(
            parse(&["--live", "--context=camelot-eks"]),
            Ok(Invocation::Live {
                context: "camelot-eks".into()
            }),
        );
        // Order-independent, and a `:view` token does not disturb it.
        assert_eq!(
            parse(&[":pods", "--context", "rio", "--live"]),
            Ok(Invocation::Live {
                context: "rio".into()
            }),
        );
    }

    /// **THE GATE.** This is the whole reason the module exists. `--live`
    /// alone used to mean "read whatever the kubeconfig's current-context
    /// happens to be", which on this machine was an entirely different estate
    /// (`us-east-2-staging-eks`) than the one under inspection
    /// (`camelot-eks`). banken refuses rather than guesses.
    #[test]
    fn live_without_a_context_is_refused() {
        assert_eq!(parse(&["--live"]), Err(CliError::MissingContext));
        let msg = CliError::MissingContext.to_string();
        assert!(
            msg.contains("current-context"),
            "the refusal must name what it would otherwise have used: {msg}"
        );
    }

    /// The converse: naming a cluster banken is not going to read is also a
    /// lie — the status line and every bancada plan would carry a real
    /// context name over fixture rows.
    #[test]
    fn a_context_without_live_is_refused() {
        assert_eq!(
            parse(&["--context", "camelot-eks"]),
            Err(CliError::ContextWithoutLive),
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
    /// `--contxt camelot-eks` would have run against the fixture while the
    /// operator believed they had selected a cluster — the same
    /// wrong-source class one typo away.
    #[test]
    fn an_unknown_flag_is_refused_by_name() {
        match parse(&["--contxt", "camelot-eks"]) {
            Err(CliError::UnknownFlag(f)) => assert_eq!(f, "--contxt"),
            other => panic!("expected UnknownFlag, got {other:?}"),
        }
    }
}
