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
//! | `stage_observed` / `stage_witnessed` | `send_keys` |
//! | `focus` | `select_pane` |
//!
//! Sessions are tagged `SessionSource::Named("banken-bancada")`, so
//! `tear list --source` shows at a glance which sessions banken opened.
//!
//! # THE UPSTREAM LIMITATION, stated rather than worked around
//!
//! `MultiplexerControl` spawns a pane's program with **no arguments**:
//! `tear-core/src/inproc.rs:667` calls `PtyHandle::spawn(shell, &[], …)`, and
//! `new_session*` / `split_pane` take only `shell: &str`. `PtyHandle::spawn`
//! itself takes a real `args: &[String]` (`tear-core/src/pty.rs:68`) — the
//! argv is dropped one layer above it.
//!
//! So a pre-warmed pane cannot be *spawned* as `kubectl logs -f <pod>` today.
//! It spawns the operator's shell and the argv is **typed into it** via
//! `send_keys`. That is the honest mechanism, and it has a sharp edge: typing
//! an argv at a shell prompt is where argv-vs-shell-string safety lives.
//!
//! **This adapter refuses to quote.** [`stageable`] rejects any argv word
//! containing whitespace or a shell metacharacter, with a typed error naming
//! the word. It does not escape it, does not wrap it in quotes, and does not
//! "handle" it — because a quoting function IS a shell-string builder, and the
//! whole claim of [`banken_spec::bancada`] is that a staged command is a typed
//! argv. An argv that cannot be typed safely has no path to a pane.
//!
//! The load-bearing fix is upstream: thread `args: &[String]` through
//! `MultiplexerControl::new_session*` / `split_pane` to the `PtyHandle::spawn`
//! that already accepts it, and this module collapses to a direct spawn with
//! no typing and no refusal. `pending-banken: tear-argv-spawn` — the row lives
//! here because banken must not edit tear from this repo.
//!
//! # The witnessed arm does NOT press Enter
//!
//! [`SessionEnv::stage_observed`] sends the argv **and a newline** — the read
//! starts immediately, which is the whole point of pre-warming. [`SessionEnv::stage_witnessed`]
//! sends the argv **without** one: the live-effect command sits typed and
//! ready, and the operator's own Enter is the final act. banken records the
//! witness; the human still takes the step. That asymmetry is a property of
//! the `CommandEffect` split, not a policy an author can forget to apply.
//!
//! # Tier: PROVEN LIVE (measured 2026-07-30), not merely compiled
//!
//! `tests/tear_handoff.rs` — `#[ignore]`d, because a plain `cargo test` must
//! never open a session on the operator's daemon — was run against the live
//! `tear-daemon` and asserted a real **three-pane** session whose first pane's
//! *rendered grid* carries the pre-warmed
//! `kubectl --context camelot-eks … <pod>` line. Fail-once measured: stubbing
//! [`TearSessionEnv::type_into`] to send zero bytes turns it red on exactly
//! that assertion, so it is not checking a shape nothing can violate.
//!
//! What is NOT yet true: the **app** does not call this. Pressing `g` in
//! banken previews the plan; a confirm-then-open path from the overlay is
//! `pending-banken: bancada-app-open`. Do not read "the handoff works" as
//! "the keystroke opens a session".

use std::cell::RefCell;

use banken_spec::env::WitnessedAction;
use banken_spec::error::SpecError;
use banken_spec::bancada::{
    MutatingCommand, ObservedCommand, PanePlacement, PaneRef, SessionEnv, SessionLayout,
};
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

/// Render an argv into the exact bytes to type, or refuse by word.
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
                 Fix upstream (pending-banken: tear-argv-spawn: thread \
                 `args: &[String]` through MultiplexerControl to the \
                 PtyHandle::spawn that already takes it), or author the \
                 (defbancada) argument without the offending character.",
            );
            return Err(SpecError::Interp {
                phase: "stage".into(),
                message: m,
            });
        }
    }
    Ok(argv.join(" "))
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
    /// The program each pane spawns — the operator's shell, into which the
    /// staged argv is typed. See the module docs' upstream-limitation note.
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
    fn open_session(&self, name: &str, layout: SessionLayout) -> Result<PaneRef, SpecError> {
        let id = self
            .client
            .new_session_with_source_and_size(
                name,
                &self.shell,
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

    fn split(&self, origin: PaneRef, placement: PanePlacement) -> Result<PaneRef, SpecError> {
        let direction = direction_of(placement).ok_or_else(|| SpecError::Interp {
            phase: "tear-split-pane".into(),
            message: "`root` is not a split direction — the root pane is the \
                      session itself"
                .into(),
        })?;
        let pane = self
            .client
            .split_pane(PaneId(origin.0), direction, &self.shell)
            .map_err(|e| SpecError::Interp {
                phase: "tear-split-pane".into(),
                message: e.to_string(),
            })?;
        Ok(PaneRef(pane.0))
    }

    fn stage_observed(&self, pane: PaneRef, cmd: &ObservedCommand) -> Result<(), SpecError> {
        let line = stageable(cmd.argv())?;
        // A read starts immediately — that IS the pre-warming.
        self.type_into(pane, &line, /* submit */ true)
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

    /// **THE GATE.** The adapter refuses to quote. An argv word that would
    /// need shell quoting has no path into a pane — it is not escaped, not
    /// wrapped, not "handled".
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
