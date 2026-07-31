//! The [`SessionEnv`] implementations the **app** is generic over — the
//! `(defbancada)` analogue of [`crate::fixture`] / [`crate::live`] on the
//! [`ClusterEnv`](banken_spec::env::ClusterEnv) axis.
//!
//! [`BankenApp`](crate::app::BankenApp) carries a `SessionEnv` type parameter
//! for exactly the reason it carries a `ClusterEnv` one: the runtime is
//! byte-identical against a mock, against a live daemon, and against a build
//! that has no daemon adapter compiled in at all. The three implementations
//! are:
//!
//! | impl | where | what it does |
//! |---|---|---|
//! | [`banken_spec::testing::MockSessionEnv`] | tests | records every call, zero side effects |
//! | [`LazyTearSessionEnv`] | feature `tear` | the live handoff to a running `tear-daemon` |
//! | [`UnwiredSessionEnv`] | default build | a typed refusal naming the missing feature |
//!
//! # Why [`UnwiredSessionEnv`] refuses instead of doing nothing
//!
//! A `SessionEnv` whose methods returned `Ok(())` would make the app report a
//! session it never opened — the exact silent-wrong-answer class this repo
//! refuses everywhere else, and worse here than most, because the operator
//! would go looking for panes that do not exist. Every method returns a
//! `SpecError::Interp` naming the missing `tear` feature, so the overlay says
//! why rather than lying. This is the same shape as `main`'s
//! `#[cfg(not(feature = "live"))] run_live`.
//!
//! # Why the tear env is LAZY
//!
//! [`banken::tear_session::TearSessionEnv`](crate::tear_session::TearSessionEnv)
//! connects in its constructor. Constructing it at app startup would make
//! `banken --live` **refuse to start** whenever no `tear-daemon` happens to be
//! running — punishing the pod-table read, which does not need tear at all,
//! for the absence of a dependency only the bancada chord uses.
//! [`LazyTearSessionEnv`] connects on the first `open_session` instead, so a
//! missing daemon costs exactly one overlay saying so, at the moment the
//! operator asked for a session.

#[cfg(feature = "tear")]
use std::sync::Mutex;

use banken_spec::bancada::{
    MutatingCommand, PanePlacement, PaneProgram, PaneRef, SessionEnv, SessionLayout,
};
use banken_spec::env::WitnessedAction;
use banken_spec::error::SpecError;

/// The [`SessionEnv`] the default build ships: every call is a typed refusal.
///
/// Not a no-op and not a stub returning `Ok` — see the module docs.
#[derive(Debug, Default, Clone, Copy)]
pub struct UnwiredSessionEnv;

impl UnwiredSessionEnv {
    /// A fresh one. It has no state; the constructor exists so call sites
    /// read the same as every other env's.
    #[must_use]
    pub fn new() -> Self {
        Self
    }

    /// The one refusal, so all four methods say the same thing.
    fn refuse(phase: &'static str) -> SpecError {
        SpecError::Interp {
            phase: phase.into(),
            message: "this banken was built without the `tear` feature, so it has no \
                      adapter to open a pre-warmed session with. The plan above is real \
                      and fully resolved — rebuild with `--features tear` to open it. \
                      (Refusing rather than reporting a session that was never opened.)"
                .into(),
        }
    }
}

impl SessionEnv for UnwiredSessionEnv {
    fn open_session(
        &self,
        _name: &str,
        _layout: SessionLayout,
        _program: PaneProgram<'_>,
    ) -> Result<PaneRef, SpecError> {
        Err(Self::refuse("open-session"))
    }

    fn split(
        &self,
        _origin: PaneRef,
        _placement: PanePlacement,
        _program: PaneProgram<'_>,
    ) -> Result<PaneRef, SpecError> {
        Err(Self::refuse("split-pane"))
    }

    fn stage_witnessed(
        &self,
        _pane: PaneRef,
        _cmd: &MutatingCommand,
        _witness: &WitnessedAction,
    ) -> Result<(), SpecError> {
        Err(Self::refuse("stage-witnessed"))
    }

    fn focus(&self, _pane: PaneRef) -> Result<(), SpecError> {
        Err(Self::refuse("focus-pane"))
    }

    // *** Still no second staging method — this impl adds no arm the trait
    //     does not declare, which is what `substrate_invariant.rs` guards. ***
}

/// The live [`SessionEnv`], connecting to the `tear-daemon` on first use.
///
/// A thin lazily-connecting wrapper over
/// [`TearSessionEnv`](crate::tear_session::TearSessionEnv), which owns all the
/// actual protocol work. See the module docs for why the laziness matters.
///
/// # Why a `Mutex` and not a `RefCell`
///
/// Not a style choice — a hard requirement, and one that also explains why
/// `TearSessionEnv` could never have been handed to the app directly.
/// [`AsyncApp`](egaku_term::AsyncApp)'s `draw` takes `&self` and returns an
/// `impl Future + Send`, so `&BankenApp<E, S>` must be `Send`, so `S` must be
/// `Sync`. `TearSessionEnv` holds a `RefCell<Option<SessionId>>` and is
/// therefore **not** `Sync`. A `RefCell` here would have inherited exactly the
/// same problem — this wrapper is what makes the live adapter reachable from
/// the app at all, not merely lazy.
#[cfg(feature = "tear")]
pub struct LazyTearSessionEnv {
    inner: Mutex<Option<crate::tear_session::TearSessionEnv>>,
}

#[cfg(feature = "tear")]
impl Default for LazyTearSessionEnv {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(feature = "tear")]
impl LazyTearSessionEnv {
    /// An unconnected env. No daemon is contacted until the first
    /// [`SessionEnv::open_session`].
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(None),
        }
    }

    /// Run `f` against the adapter, connecting first if this is the first
    /// call.
    ///
    /// A connect failure is propagated, never swallowed: no daemon means no
    /// session, and the operator is told so at the moment they asked.
    fn with_connected<R>(
        &self,
        f: impl FnOnce(&crate::tear_session::TearSessionEnv) -> Result<R, SpecError>,
    ) -> Result<R, SpecError> {
        let mut guard = self.inner.lock().map_err(|_| Self::poisoned())?;
        if guard.is_none() {
            *guard = Some(crate::tear_session::TearSessionEnv::connect_default()?);
        }
        f(guard
            .as_ref()
            .expect("connected directly above, and nothing clears it"))
    }

    /// Run `f` against an adapter that must ALREADY be connected.
    ///
    /// The three non-opening methods deliberately do not connect on their own:
    /// `bancada::open` walks the plan's root pane first, so reaching a
    /// split/stage/focus with no session is a malformed plan, and connecting
    /// here would paper over it by opening a session nobody asked for.
    fn with_established<R>(
        &self,
        f: impl FnOnce(&crate::tear_session::TearSessionEnv) -> Result<R, SpecError>,
    ) -> Result<R, SpecError> {
        let guard = self.inner.lock().map_err(|_| Self::poisoned())?;
        let env = guard.as_ref().ok_or_else(|| SpecError::Interp {
            phase: "tear-session".into(),
            message: "no tear session has been opened yet — `open` walks the plan's \
                      root pane first, so reaching a split/stage/focus before it is a \
                      malformed plan"
                .into(),
        })?;
        f(env)
    }

    /// A poisoned lock means a previous call panicked mid-session. Reported
    /// rather than recovered with `into_inner`: the daemon-side state is of
    /// unknown shape, and silently continuing against it is how a half-opened
    /// session becomes a confusing one.
    fn poisoned() -> SpecError {
        SpecError::Interp {
            phase: "tear-session".into(),
            message: "the tear session lock is poisoned — a previous call panicked \
                      mid-session, so the daemon-side state is of unknown shape. \
                      Restart banken rather than opening more panes onto it."
                .into(),
        }
    }
}

#[cfg(feature = "tear")]
impl SessionEnv for LazyTearSessionEnv {
    fn open_session(
        &self,
        name: &str,
        layout: SessionLayout,
        program: PaneProgram<'_>,
    ) -> Result<PaneRef, SpecError> {
        self.with_connected(|e| e.open_session(name, layout, program))
    }

    fn split(
        &self,
        origin: PaneRef,
        placement: PanePlacement,
        program: PaneProgram<'_>,
    ) -> Result<PaneRef, SpecError> {
        self.with_established(|e| e.split(origin, placement, program))
    }

    fn stage_witnessed(
        &self,
        pane: PaneRef,
        cmd: &MutatingCommand,
        witness: &WitnessedAction,
    ) -> Result<(), SpecError> {
        self.with_established(|e| e.stage_witnessed(pane, cmd, witness))
    }

    fn focus(&self, pane: PaneRef) -> Result<(), SpecError> {
        self.with_established(|e| e.focus(pane))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **THE GATE.** A build with no tear adapter REFUSES to open a session.
    /// It must never report one it did not open — the operator would go
    /// looking for panes that do not exist.
    #[test]
    fn the_unwired_env_refuses_rather_than_faking_success() {
        let env = UnwiredSessionEnv::new();
        let err = env
            .open_session("triage-x", SessionLayout::MainVertical, PaneProgram::Shell)
            .expect_err("an unwired env must not report a session it never opened");
        let msg = err.to_string();
        assert!(
            msg.contains("tear"),
            "the refusal names the missing feature: {msg}"
        );
        // And every other arm refuses too — a partial refusal would leave the
        // plan walk half-applied.
        assert!(
            env.split(PaneRef(1), PanePlacement::Right, PaneProgram::Shell)
                .is_err()
        );
        assert!(env.focus(PaneRef(1)).is_err());
    }
}
