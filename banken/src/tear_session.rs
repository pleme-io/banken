//! `TearSessionEnv` — the LIVE `(defbancada)` handoff over a running
//! `tear-daemon` (feature `tear`).
//!
//! This is the far end of the bridge: [`banken_spec::bancada::plan`] turns an
//! authored recipe plus the selected row into a [`SessionPlan`], and this
//! module hands that plan to tear so the operator lands in a split session
//! already pointed at the broken thing.
//!
//! # It CONSUMES tear's surface; it does not invent one
//!
//! [`banken_spec::bancada::SessionEnv`] was deliberately shaped as a
//! projection of `tear_types::MultiplexerControl`, so every method here is
//! close to a rename:
//!
//! | `SessionEnv` | `MultiplexerControl` |
//! |---|---|
//! | `open_session` | `new_session_with_source_and_size` + `apply_layout` |
//! | `split` | `split_pane` (`PanePlacement` → `Direction`) |
//! | `PaneProgram::Observe` | the `shell` + `args` those two spawn |
//! | `stage_witnessed` | `send_keys` |
//! | `focus` | `select_pane` |
//!
//! Sessions are tagged `SessionSource::Named("banken-bancada")`, so
//! `tear list --source` shows at a glance which sessions banken opened.
//!
//! # `pending-banken: tear-argv-spawn` — CLOSED (2026-07-31), for READS
//!
//! The limitation this module used to document is gone. tear `5974375` threaded
//! `args: &[String]` through `MultiplexerControl::new_session_with_source_and_size`
//! / `split_pane` / `new_window` to the `PtyHandle::spawn` that had accepted one
//! since it was written. So a read pane is now **spawned as its own argv**:
//! `program_of` splits a [`PaneProgram::Observe`]'s argv into `(argv[0],
//! argv[1..])` and hands it straight to tear, which hands it to `execvp` as a
//! vector.
//!
//! **The refusal-to-quote therefore does not apply to a read pane at all** —
//! not because it was relaxed, but because there is no shell in the path to
//! quote for. An argument like `-o jsonpath={.status.phase}`, which
//! [`stageable`] rejects on sight, now reaches a pane unaltered.
//!
//! # `pending-banken: tear-argv-witnessed-arm` — the half that CANNOT convert
//!
//! A spawned program **runs immediately**, and that is exactly what a mutating
//! pane must not do. [`SessionEnv::stage_witnessed`] types its argv **without a
//! newline** on purpose: the live-effect command sits typed and ready, and the
//! operator's own Enter is the final act. banken records the witness; the human
//! still takes the step. Spawning it would delete that step silently, which is
//! a worse outcome than any quoting inconvenience.
//!
//! So the witnessed arm still types, and therefore **still refuses to quote**.
//! [`stageable`] rejects any argv word containing whitespace or a shell
//! metacharacter, with a typed error naming the word. It does not escape it,
//! does not wrap it in quotes, and does not "handle" it — a quoting function IS
//! a shell-string builder, and the whole claim of [`banken_spec::bancada`] is
//! that a staged command is a typed argv.
//!
//! This is not a limitation waiting on an upstream fix; it is a property of
//! "typed but not yet run". Closing it means a tear surface that can *place
//! text on a pane's input line without executing it as that pane's program* —
//! e.g. spawning the shell with a pre-seeded, unsubmitted line. The row records
//! the shape, not a promise.
//!
//! # A read pane now EXITS when its command does
//!
//! The direct consequence of spawning, stated rather than discovered: a pane
//! running `kubectl describe` finishes and goes `PaneState::Exited`, where
//! before it ran the command inside a shell and returned a prompt. tear keeps a
//! watched session's exited panes and their final grid (remain-on-exit), so the
//! output stays readable; the operator just cannot type in that pane. Both
//! shipped recipes hold a long-running pane (`logs --follow`, `get events
//! --watch`), so neither session can fully exit and be reaped.
//!
//! # Tier: PROVEN LIVE (measured 2026-07-30), not merely compiled
//!
//! `tests/tear_handoff.rs` — `#[ignore]`d, because a plain `cargo test` must
//! never open a session on the operator's daemon — was run against the live
//! `tear-daemon` and asserted a real **three-pane** session whose first pane's
//! *rendered grid* carries the pre-warmed
//! `kubectl --context alpha-eks … <pod>` line. Fail-once measured: stubbing
//! [`TearSessionEnv::type_into`] to send zero bytes turns it red on exactly
//! that assertion, so it is not checking a shape nothing can violate.
//!
//! The **app** now reaches this: `banken::app::BankenApp` carries a
//! `SessionEnv` type parameter, `g` resolves and previews the plan, and
//! `enter` confirms it (`pending-banken: bancada-app-open`, CLOSED
//! 2026-07-31 — mock-proven with a measured fail-once).
//!
//! # What the real-daemon run proved, and what it did NOT (2026-07-31)
//!
//! `banken --features tear` in a real PTY, `g` then `enter`, rendered
//! `BANCADA — OBSERVE — pod-triage (OPENED) / panes: 3`. Every RPC returned
//! `Ok` (an `Err` would have rendered an ERROR overlay instead), so the daemon
//! accepted the whole walk and held the session while banken walked it.
//!
//! **But `tear list` on the same socket reports no sessions immediately
//! afterwards** (`session_count: 0`), so the session is not one the operator
//! can attach to. The same disappearance shows up for sessions created through
//! mado's `tear_new_session` MCP tool, which points at a tear-side lifecycle
//! behaviour rather than at this adapter. banken must not edit tear from this
//! repo. `pending-banken: bancada-app-open-live` — open on the durability
//! claim ONLY; the call path itself is proven.

use std::cell::RefCell;

use banken_spec::bancada::{
    MutatingCommand, PanePlacement, PaneProgram, PaneRef, SessionEnv, SessionLayout,
};
use banken_spec::env::WitnessedAction;
use banken_spec::error::SpecError;
use tear_types::control::MultiplexerControl;
use tear_types::direction::Direction;
use tear_types::id::{PaneId, SessionId};
use tear_types::layout::LayoutKind;
use tear_types::session::SessionSource;

/// The provenance tag banken stamps on every session it opens, so
/// `tear list --source` can tell them apart from an operator's own.
pub const BANCADA_SOURCE: &str = "banken-bancada";

/// The default pane geometry for a pre-warmed session. tear's own default is
/// 80×24; a troubleshooting session is worth more room, and the child's first
/// TIOCGWINSZ returns this so a TUI in the pane renders right from t=0.
const DEFAULT_SIZE_CELLS: (u16, u16) = (200, 50);

/// Characters that would need shell quoting to survive being typed at a
/// prompt. An argv word containing any of them is REFUSED rather than
/// escaped — see the module docs.
const UNSAFE_CHARS: &[char] = &[
    ' ', '\t', '\n', '\r', '"', '\'', '\\', '`', '$', '&', ';', '|', '<', '>', '(', ')', '{', '}',
    '[', ']', '*', '?', '~', '#', '!',
];

/// Render an argv into the exact bytes to type at a prompt, or refuse by word.
///
/// **Reached only from the witnessed arm.** A read pane is spawned as its argv
/// and never passes through here — see the module docs. This exists because a
/// mutating command must sit *typed and unsubmitted*, which means it must
/// survive a shell's parser, which is where quoting lives.
///
/// # Errors
///
/// [`SpecError::Interp`] naming the first word that would need quoting. There
/// is deliberately no escaping path: quoting is shell-string construction.
pub fn stageable(argv: &[String]) -> Result<String, SpecError> {
    if argv.is_empty() {
        return Err(SpecError::Interp {
            phase: "stage".into(),
            message: "a planned pane has an empty argv — there is no program to run".into(),
        });
    }
    for word in argv {
        if word.is_empty() || word.chars().any(|c| UNSAFE_CHARS.contains(&c)) {
            let mut m = String::from("argv word `");
            m.push_str(word);
            m.push_str(
                "` would need shell quoting to be typed at a prompt, and this \
                 adapter refuses to quote — a quoting function is a shell-string \
                 builder, which is exactly what a typed argv exists to avoid. \
                 This is the WITNESSED arm: a mutating command is typed and left \
                 unsubmitted so the operator's own Enter is the final act, which \
                 is why it cannot simply be spawned the way a read pane now is \
                 (pending-banken: tear-argv-witnessed-arm). Author the \
                 (defbancada) argument without the offending character, or make \
                 the pane a read.",
            );
            return Err(SpecError::Interp {
                phase: "stage".into(),
                message: m,
            });
        }
    }
    Ok(argv.join(" "))
}

/// Split a [`PaneProgram`] into the `(program, args)` pair tear spawns.
///
/// A [`PaneProgram::Observe`] becomes its own argv — `argv[0]` is the program,
/// the rest is the argument vector, handed to tear as a vector and reaching
/// `execvp` with no shell in between. [`PaneProgram::Shell`] becomes the
/// operator's shell with no arguments; its command is typed in afterwards by
/// [`SessionEnv::stage_witnessed`].
///
/// # Errors
///
/// [`SpecError::Interp`] when an observed pane names no program — an empty argv
/// *or* an empty `argv[0]`. Both are reachable: `plan` on a `(defbancada)` whose
/// `:program` is `""` yields the argv `[""]`, which `split_first` accepts, so
/// checking only for an empty vector would hand tear a nameless program and let
/// the daemon report it. Measured, not assumed — the first cut of this function
/// checked only the vector and
/// `an_empty_read_argv_is_refused_rather_than_spawned` went red with
/// `("", [])`.
fn program_of<'a>(
    program: PaneProgram<'a>,
    shell: &'a str,
) -> Result<(&'a str, &'a [String]), SpecError> {
    match program {
        PaneProgram::Observe(cmd) => {
            let (head, tail) = cmd
                .argv()
                .split_first()
                .filter(|(head, _)| !head.is_empty())
                .ok_or_else(|| SpecError::Interp {
                    phase: "spawn".into(),
                    message: "a planned read pane names no program to spawn — its argv \
                              is empty or begins with an empty word"
                        .into(),
                })?;
            Ok((head.as_str(), tail))
        }
        PaneProgram::Shell => Ok((shell, &[])),
    }
}

/// Project a `(defbancada)` placement onto tear's split direction.
///
/// [`PanePlacement::Root`] has no direction — it is the session itself — so
/// this returns `None` for it rather than defaulting to a split.
fn direction_of(placement: PanePlacement) -> Option<Direction> {
    match placement {
        PanePlacement::Root => None,
        PanePlacement::Right => Some(Direction::Right),
        PanePlacement::Below => Some(Direction::Below),
        PanePlacement::Left => Some(Direction::Left),
        PanePlacement::Above => Some(Direction::Above),
    }
}

/// Project a `(defbancada)` layout onto tear's. Total by construction:
/// [`SessionLayout`] is exactly `LayoutKind` minus `Custom`.
fn layout_of(layout: SessionLayout) -> LayoutKind {
    match layout {
        SessionLayout::EvenHorizontal => LayoutKind::EvenHorizontal,
        SessionLayout::EvenVertical => LayoutKind::EvenVertical,
        SessionLayout::MainHorizontal => LayoutKind::MainHorizontal,
        SessionLayout::MainVertical => LayoutKind::MainVertical,
        SessionLayout::Tiled => LayoutKind::Tiled,
    }
}

/// A live [`SessionEnv`] over a connected `tear-daemon`.
pub struct TearSessionEnv {
    client: tear_client::Client,
    /// The program a [`PaneProgram::Shell`] pane spawns — the operator's
    /// shell, into which a WITNESSED argv is typed and left unsubmitted. A read
    /// pane does not use it: it spawns its own argv. See the module docs.
    shell: String,
    /// The session this env opened, once it has opened one. Kept so a caller
    /// (or a test) can address or tear down what it created.
    session: RefCell<Option<SessionId>>,
}

impl TearSessionEnv {
    /// Connect to the default tear socket.
    ///
    /// # Errors
    ///
    /// [`SpecError::Interp`] when no daemon is reachable — the honest failure,
    /// never a fake success.
    pub fn connect_default() -> Result<Self, SpecError> {
        let client = tear_client::Client::connect_default().map_err(|e| SpecError::Interp {
            phase: "tear-connect".into(),
            message: e.to_string(),
        })?;
        Ok(Self {
            client,
            shell: std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_owned()),
            session: RefCell::new(None),
        })
    }

    /// Override the per-pane shell (tests, and an operator who wants a
    /// specific one).
    #[must_use]
    pub fn with_shell(mut self, shell: impl Into<String>) -> Self {
        self.shell = shell.into();
        self
    }

    /// The session this env opened, if any.
    #[must_use]
    pub fn session_id(&self) -> Option<SessionId> {
        *self.session.borrow()
    }

    /// Kill the session this env opened. Used by tests to clean up after
    /// themselves; an operator's session is theirs to close.
    ///
    /// # Errors
    ///
    /// [`SpecError::Interp`] when the daemon rejects the kill.
    pub fn kill_opened_session(&self) -> Result<(), SpecError> {
        if let Some(id) = self.session_id() {
            self.client
                .kill_session(id)
                .map_err(|e| SpecError::Interp {
                    phase: "tear-kill-session".into(),
                    message: e.to_string(),
                })?;
        }
        Ok(())
    }

    /// The first pane of a session — the root the recipe splits from.
    fn first_pane(&self, id: SessionId) -> Result<PaneId, SpecError> {
        let s = self.client.get_session(id).map_err(|e| SpecError::Interp {
            phase: "tear-get-session".into(),
            message: e.to_string(),
        })?;
        s.panes
            .keys()
            .next()
            .copied()
            .ok_or_else(|| SpecError::Interp {
                phase: "tear-get-session".into(),
                message: "the new session has no panes".into(),
            })
    }

    /// Type `text` into a pane, optionally submitting it.
    fn type_into(&self, pane: PaneRef, text: &str, submit: bool) -> Result<(), SpecError> {
        let mut bytes = text.as_bytes().to_vec();
        if submit {
            bytes.push(b'\r');
        }
        self.client
            .send_keys(PaneId(pane.0), &bytes)
            .map_err(|e| SpecError::Interp {
                phase: "tear-send-keys".into(),
                message: e.to_string(),
            })
    }
}

impl SessionEnv for TearSessionEnv {
    fn open_session(
        &self,
        name: &str,
        layout: SessionLayout,
        program: PaneProgram<'_>,
    ) -> Result<PaneRef, SpecError> {
        let (prog, args) = program_of(program, &self.shell)?;
        let id = self
            .client
            .new_session_with_source_and_size(
                name,
                prog,
                args,
                SessionSource::Named(BANCADA_SOURCE.to_owned()),
                DEFAULT_SIZE_CELLS,
            )
            .map_err(|e| SpecError::Interp {
                phase: "tear-new-session".into(),
                message: e.to_string(),
            })?;
        *self.session.borrow_mut() = Some(id);

        // Apply the recipe's layout to the session's active window. A
        // single-pane window has no arrangement to impose, so this is a no-op
        // until the first split — applying it here anyway keeps the call at
        // the place the layout is DECLARED rather than at some later pane.
        let s = self.client.get_session(id).map_err(|e| SpecError::Interp {
            phase: "tear-get-session".into(),
            message: e.to_string(),
        })?;
        self.client
            .apply_layout(s.active_window, layout_of(layout))
            .map_err(|e| SpecError::Interp {
                phase: "tear-apply-layout".into(),
                message: e.to_string(),
            })?;

        Ok(PaneRef(self.first_pane(id)?.0))
    }

    fn split(
        &self,
        origin: PaneRef,
        placement: PanePlacement,
        program: PaneProgram<'_>,
    ) -> Result<PaneRef, SpecError> {
        let direction = direction_of(placement).ok_or_else(|| SpecError::Interp {
            phase: "tear-split-pane".into(),
            message: "`root` is not a split direction — the root pane is the \
                      session itself"
                .into(),
        })?;
        let (prog, args) = program_of(program, &self.shell)?;
        let pane = self
            .client
            .split_pane(PaneId(origin.0), direction, prog, args)
            .map_err(|e| SpecError::Interp {
                phase: "tear-split-pane".into(),
                message: e.to_string(),
            })?;
        Ok(PaneRef(pane.0))
    }

    fn stage_witnessed(
        &self,
        pane: PaneRef,
        cmd: &MutatingCommand,
        _witness: &WitnessedAction,
    ) -> Result<(), SpecError> {
        let line = stageable(cmd.argv())?;
        // *** Deliberately NOT submitted. *** The live-effect command is typed
        // and waiting; the operator's own Enter is the final act. banken has
        // recorded the witness, and the human still takes the step.
        self.type_into(pane, &line, /* submit */ false)
    }

    fn focus(&self, pane: PaneRef) -> Result<(), SpecError> {
        self.client
            .select_pane(PaneId(pane.0))
            .map_err(|e| SpecError::Interp {
                phase: "tear-select-pane".into(),
                message: e.to_string(),
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use banken_spec::bancada::{
        BancadaContext, CommandArg, CommandEffect, PaneRole, StagedCommand, plan,
    };

    /// An [`ObservedCommand`] carrying `argv` — built the only way one can be,
    /// through a real `PlannedPane`, so the test exercises the same
    /// construction seal production does.
    fn observed(argv: &[&str]) -> banken_spec::bancada::ObservedCommand {
        use banken_spec::bancada::{BancadaPane, BancadaSpec, PanePlacement, SessionLayout};
        use banken_spec::chord::ActionChord;
        use banken_spec::interp::Selection;
        use banken_spec::types::ResourceKind;

        let (program, rest) = argv.split_first().map_or(("", &[][..]), |(h, t)| (*h, t));
        let spec = BancadaSpec {
            name: "t".into(),
            keys: ActionChord::parse("g").expect("a valid chord"),
            from: "pods".into(),
            layout: SessionLayout::MainVertical,
            session_prefix: "t".into(),
            witness: None,
            runbook: None,
            panes: vec![BancadaPane {
                role: PaneRole::Logs,
                placement: PanePlacement::Root,
                command: StagedCommand {
                    program: program.to_owned(),
                    args: rest
                        .iter()
                        .map(|a| CommandArg::Literal((*a).to_owned()))
                        .collect(),
                    effect: CommandEffect::Observes,
                },
            }],
        };
        let ctx = BancadaContext {
            cluster: "c".into(),
            selection: Selection {
                kind: ResourceKind::Pod,
                name: "p".into(),
                namespace: Some("n".into()),
                current: Vec::new(),
            },
            container: None,
        };
        plan(&spec, &ctx).expect("plans").panes()[0]
            .as_observed()
            .expect("an observing pane projects to an ObservedCommand")
    }

    /// **THE GATE, and it is now about the WITNESSED arm only.** A mutating
    /// command is typed at a prompt and left unsubmitted, so it must survive a
    /// shell's parser — and this adapter refuses to quote rather than build one.
    /// A word that would need quoting has no path into a *witnessed* pane: it
    /// is not escaped, not wrapped, not "handled".
    #[test]
    fn an_argv_word_needing_quoting_is_refused_by_name() {
        let ok = stageable(&["kubectl".into(), "logs".into(), "-f".into(), "pod-1".into()])
            .expect("a plain argv types safely");
        assert_eq!(ok, "kubectl logs -f pod-1");

        for bad in ["a b", "a;b", "$HOME", "a|b", "a`b`", "a\nb", ""] {
            let err = stageable(&["kubectl".into(), bad.into()])
                .expect_err("an unsafe word must be refused");
            let msg = err.to_string();
            assert!(
                msg.contains("refuses to quote"),
                "the refusal must say it will not quote: {msg}"
            );
        }

        assert!(
            stageable(&[]).is_err(),
            "an empty argv has no program to run"
        );
    }

    /// **THE CONVERSION GATE.** A read pane is spawned as its own argv — the
    /// program is `argv[0]` and the rest is the argument vector tear hands to
    /// `execvp`. This is what `pending-banken: tear-argv-spawn` bought.
    #[test]
    fn a_read_pane_is_spawned_as_its_own_argv_not_typed_into_a_shell() {
        let cmd = observed(&["kubectl", "--context", "alpha-eks", "logs", "-f", "catch-0"]);
        let (prog, args) = program_of(PaneProgram::Observe(&cmd), "/bin/zsh")
            .expect("a resolved read pane spawns");
        assert_eq!(prog, "kubectl", "argv[0] IS the program tear spawns");
        assert_eq!(
            args,
            ["--context", "alpha-eks", "logs", "-f", "catch-0"],
            "argv[1..] reaches execvp as a vector — no shell, so nothing is \
             joined into a command string",
        );
    }

    /// **THE POINT OF THE WHOLE CHANGE.** An argv word `stageable` refuses on
    /// sight now reaches a read pane untouched, because there is no shell in
    /// the path to quote for. Before the tear argv change this argument had no
    /// way into a pane at all.
    #[test]
    fn an_argument_the_typed_path_refuses_reaches_a_spawned_read_pane_intact() {
        let jsonpath = "-o=jsonpath={.status.phase}";
        assert!(
            stageable(&["kubectl".into(), jsonpath.into()]).is_err(),
            "positive control: the typed path must still refuse this word",
        );

        let cmd = observed(&["kubectl", "get", "pod", jsonpath]);
        let (prog, args) = program_of(PaneProgram::Observe(&cmd), "/bin/zsh").expect("spawns");
        assert_eq!(prog, "kubectl");
        assert_eq!(
            args.last().map(String::as_str),
            Some(jsonpath),
            "the braces survive verbatim — a vector has nothing to quote",
        );
    }

    /// A mutating pane is a SHELL. It must not resolve to a spawn, because a
    /// spawned program runs immediately and the operator's Enter is the whole
    /// point of the witnessed arm.
    #[test]
    fn a_shell_pane_spawns_the_operators_shell_with_no_arguments() {
        let (prog, args) = program_of(PaneProgram::Shell, "/bin/zsh").expect("shell spawns");
        assert_eq!(prog, "/bin/zsh");
        assert!(
            args.is_empty(),
            "a shell pane carries no argv — its command is TYPED in afterwards, \
             unsubmitted",
        );
    }

    /// An empty read argv is refused, never spawned as an empty program name.
    /// `plan` cannot construct one; this is the typed floor, not an `unwrap`.
    #[test]
    fn an_empty_read_argv_is_refused_rather_than_spawned() {
        let cmd = observed(&[]);
        let err = program_of(PaneProgram::Observe(&cmd), "/bin/zsh")
            .expect_err("an empty argv has no program to spawn");
        assert!(
            err.to_string().contains("no program"),
            "the refusal says what is missing: {err}",
        );
    }

    /// The two projections are total and agree with tear's own vocabulary —
    /// which is what makes this adapter a rename rather than a translation.
    #[test]
    fn the_placement_and_layout_projections_are_total() {
        assert_eq!(direction_of(PanePlacement::Root), None);
        assert_eq!(direction_of(PanePlacement::Right), Some(Direction::Right));
        assert_eq!(direction_of(PanePlacement::Below), Some(Direction::Below));
        assert_eq!(direction_of(PanePlacement::Left), Some(Direction::Left));
        assert_eq!(direction_of(PanePlacement::Above), Some(Direction::Above));
        // Every SessionLayout maps, and none maps to `Custom` — "custom"
        // means whatever the operator arranged by hand, which a recipe
        // cannot declare.
        for l in SessionLayout::ALL {
            assert_ne!(layout_of(*l), LayoutKind::Custom);
        }
        assert_eq!(SessionLayout::ALL.len(), 5);
    }
}
