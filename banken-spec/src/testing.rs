//! `MockClusterEnv` — the mock seam that proves the triplet with **no
//! live cluster** (BANKEN.md §III.c; the sui-spec `MockEnv` pattern).
//!
//! It records every emitted `DeclareChange` and every break-glass
//! action, and lets tests assert:
//!  (a) the *full* manifest was serialized (not a patch),
//!  (b) **zero network** was touched (a hard invariant: the mock has no
//!      socket / HTTP client — reads are canned data),
//!  (c) a DECLARE never calls a live mutate — *there is none to call*,
//!      because `ClusterEnv` has no unwitnessed-mutate method.
//!
//! Exposed as a `testing` module (not `#[cfg(test)]`) so downstream
//! crates (the banken app, golden-frame tests) can reuse the same seam.

use std::cell::RefCell;

use crate::{
    bancada::{MutatingCommand, PanePlacement, PaneProgram, PaneRef, SessionEnv, SessionLayout},
    env::{
        ChangeRef, ClusterEnv, DeclareChange, DepTree, Event, GlassRecord, HealthReading,
        LogStream, ResourceRef, Row, WatchStream, WitnessedAction, Workload,
    },
    error::SpecError,
    types::ResourceKind,
};

/// A mock cluster env with canned reads and recorded writes. No
/// network, no cluster — every read returns pre-seeded data, every
/// write is appended to a log a test inspects.
#[derive(Default)]
pub struct MockClusterEnv {
    /// Canned rows returned by `list_resources` (per kind).
    rows: Vec<(ResourceKind, Row)>,
    /// Recorded DECLARE changes (the git writes that would happen).
    pub declared: RefCell<Vec<DeclareChange>>,
    /// Recorded break-glass actions.
    pub glass: RefCell<Vec<WitnessedAction>>,
    /// A monotonic counter for synthetic change-ref ids.
    next_ref: RefCell<u64>,
}

impl MockClusterEnv {
    /// A fresh empty mock.
    pub fn new() -> Self {
        Self::default()
    }

    /// Seed a row for a kind (builder-style, like sui-spec's `MockEnv`).
    #[must_use]
    pub fn with_row(mut self, kind: ResourceKind, row: Row) -> Self {
        self.rows.push((kind, row));
        self
    }

    /// How many DECLARE changes have been recorded.
    pub fn declared_count(&self) -> usize {
        self.declared.borrow().len()
    }

    /// How many break-glass actions have been recorded.
    pub fn glass_count(&self) -> usize {
        self.glass.borrow().len()
    }
}

impl ClusterEnv for MockClusterEnv {
    // ── OBSERVE — canned reads, zero network ──

    fn list_resources(&self, kind: ResourceKind, _ns: Option<&str>) -> Result<Vec<Row>, SpecError> {
        Ok(self
            .rows
            .iter()
            .filter(|(k, _)| *k == kind)
            .map(|(_, r)| r.clone())
            .collect())
    }

    fn get_resource(
        &self,
        kind: ResourceKind,
        name: &str,
        _ns: Option<&str>,
    ) -> Result<Row, SpecError> {
        self.rows
            .iter()
            .find(|(k, r)| *k == kind && r.name == name)
            .map(|(_, r)| r.clone())
            .ok_or_else(|| SpecError::Interp {
                phase: "get-resource".into(),
                message: format!("{}/{name} not seeded in mock", kind.label()),
            })
    }

    fn logs(&self, pod: &str, _ns: &str, follow: bool) -> Result<LogStream, SpecError> {
        Ok(LogStream {
            pod: pod.to_string(),
            lines: vec!["<canned log line>".to_string()],
            follow,
        })
    }

    fn events(&self, _ns: Option<&str>) -> Result<Vec<Event>, SpecError> {
        Ok(Vec::new())
    }

    fn topology(&self, root: &ResourceRef) -> Result<DepTree, SpecError> {
        Ok(DepTree {
            root: root.name.clone(),
            edges: Vec::new(),
        })
    }

    fn health_signals(&self, _w: &Workload) -> Result<HealthReading, SpecError> {
        Ok(HealthReading {
            band_phases: vec![("MemoryBand".into(), "Holding".into())],
            metric_present: true,
            detections: Vec::new(),
        })
    }

    fn watch(&self, kind: ResourceKind, ns: Option<&str>) -> Result<WatchStream, SpecError> {
        Ok(WatchStream {
            kind_label: kind.label().to_string(),
            rows: self.list_resources(kind, ns)?,
        })
    }

    // ── DECLARE — records the git write, mutates nothing live ──

    fn declare(&self, change: DeclareChange) -> Result<ChangeRef, SpecError> {
        self.declared.borrow_mut().push(change);
        let mut n = self.next_ref.borrow_mut();
        *n += 1;
        Ok(ChangeRef(format!("mock-change-{n}")))
    }

    // ── BREAK-GLASS — records the witnessed action ──

    fn break_glass(&self, action: WitnessedAction) -> Result<GlassRecord, SpecError> {
        self.glass.borrow_mut().push(action.clone());
        Ok(GlassRecord {
            action,
            // A placeholder handle — the real AnomalyChain record id
            // lands when the chain ships (labelled, never rounded up).
            record_id: "mock-anomaly-chain-record".into(),
        })
    }

    // *** No mutate method to implement — because ClusterEnv has none. ***
}

/// A mock terminal-multiplexer env for the `(defbancada)` triplet — the
/// [`SessionEnv`] analogue of [`MockClusterEnv`].
///
/// It records every session opened, every split, what program each pane was
/// **born running**, and every witnessed staging — **keeping the spawn and the
/// witnessed-staging paths in separate logs** so a test can assert that a
/// mutating command went through the witnessed arm rather than merely that
/// "something happened". No tear daemon, no subprocess, no PTY — every call
/// appends to a `RefCell` a test inspects.
///
/// `spawned` and `shells` together are what make "a live-effect command was
/// never auto-run" *checkable*: a mutating pane must appear in `shells` and
/// must not appear in `spawned`.
#[derive(Default)]
pub struct MockSessionEnv {
    /// `(session name, layout)` per [`SessionEnv::open_session`] call.
    pub sessions: RefCell<Vec<(String, SessionLayout)>>,
    /// `(origin, placement)` per [`SessionEnv::split`] call.
    pub splits: RefCell<Vec<(PaneRef, PanePlacement)>>,
    /// `(pane, argv)` per pane born as a read-only command
    /// ([`PaneProgram::Observe`]) — spawned, never typed.
    pub spawned: RefCell<Vec<(PaneRef, Vec<String>)>>,
    /// Every pane born as a bare shell ([`PaneProgram::Shell`]), in order.
    pub shells: RefCell<Vec<PaneRef>>,
    /// `(pane, argv, witness)` per **witnessed** staging.
    pub witnessed: RefCell<Vec<(PaneRef, Vec<String>, WitnessedAction)>>,
    /// Every pane focused, in order.
    pub focused: RefCell<Vec<PaneRef>>,
    /// Monotonic pane-handle counter.
    next_pane: RefCell<u64>,
}

impl MockSessionEnv {
    /// A fresh empty mock.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// How many commands were staged through the **witnessed** arm.
    #[must_use]
    pub fn witnessed_count(&self) -> usize {
        self.witnessed.borrow().len()
    }

    fn mint(&self) -> PaneRef {
        let mut n = self.next_pane.borrow_mut();
        *n += 1;
        PaneRef(*n)
    }
}

impl MockSessionEnv {
    /// Record what a freshly-minted pane was born running.
    fn record_program(&self, pane: PaneRef, program: PaneProgram<'_>) {
        match program {
            PaneProgram::Observe(cmd) => {
                self.spawned.borrow_mut().push((pane, cmd.argv().to_vec()));
            }
            PaneProgram::Shell => self.shells.borrow_mut().push(pane),
        }
    }
}

impl SessionEnv for MockSessionEnv {
    fn open_session(
        &self,
        name: &str,
        layout: SessionLayout,
        program: PaneProgram<'_>,
    ) -> Result<PaneRef, SpecError> {
        self.sessions.borrow_mut().push((name.to_owned(), layout));
        let pane = self.mint();
        self.record_program(pane, program);
        Ok(pane)
    }

    fn split(
        &self,
        origin: PaneRef,
        placement: PanePlacement,
        program: PaneProgram<'_>,
    ) -> Result<PaneRef, SpecError> {
        self.splits.borrow_mut().push((origin, placement));
        let pane = self.mint();
        self.record_program(pane, program);
        Ok(pane)
    }

    fn stage_witnessed(
        &self,
        pane: PaneRef,
        cmd: &MutatingCommand,
        witness: &WitnessedAction,
    ) -> Result<(), SpecError> {
        self.witnessed
            .borrow_mut()
            .push((pane, cmd.argv().to_vec(), witness.clone()));
        Ok(())
    }

    fn focus(&self, pane: PaneRef) -> Result<(), SpecError> {
        self.focused.borrow_mut().push(pane);
        Ok(())
    }

    // *** No second staging method. There is ONE, and it demands a witness.
    //     A read pane is not staged at all — it is spawned as its own command
    //     via `PaneProgram::Observe`. See `bancada`'s module docs. ***
}
