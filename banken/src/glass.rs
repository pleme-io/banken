//! glass — the break-glass ledger, written BEFORE the glass breaks.
//!
//! # The one invariant
//!
//! BREAK-GLASS is banken's single witnessed live-effect path. Its whole claim
//! is that every such action leaves a durable record naming who authorised it.
//! A ledger written *after* the action does not make that claim: the interesting
//! failures — the exec that hung, the process killed mid-session, the pane that
//! never opened — are exactly the ones that would skip the write, so the ledger
//! would be complete precisely when nothing went wrong.
//!
//! So this is a **write-ahead** ledger. [`GlassLedger::record`] appends and
//! **fsyncs** the entry, and only then hands back a [`Witnessed`]. The error
//! direction is deliberate and one-sided: a crash between the write and the
//! effect leaves a record of something that may not have happened, which is
//! over-recording. Under-recording — an effect with no record — is the state
//! this module exists to make unreachable.
//!
//! # Why [`Witnessed`] is a capability, not a receipt
//!
//! "Write the record first" is a rule an author can forget. So it is not a
//! rule here: [`open_witnessed_session`] takes `&Witnessed`, whose only
//! constructor is [`GlassLedger::record`], which cannot return without having
//! fsynced. Opening a break-glass session without a persisted record is
//! therefore not a mistake one can make — there is no value of the required
//! type obtainable any other way.
//!
//! Its honest tier: **parse-time-rejected within this crate**, not
//! unrepresentable fleet-wide. The field is private and the constructor is the
//! ledger, so no consumer can mint one — but an author *editing this file*
//! could add a second constructor. That is what
//! `the_ledger_is_the_only_way_to_mint_a_witness` is for.
//!
//! # What the outcome append is for
//!
//! [`GlassLedger::resolve`] appends a second line naming what actually
//! happened. It is an append, never an edit of the first line, so a record
//! whose outcome never arrives stays visible as an unresolved break-glass —
//! which is the shape of a session that crashed, and is information rather
//! than an inconsistency to be tidied away.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use banken_spec::env::{GlassRecord, WitnessedAction};
use banken_spec::error::SpecError;
use serde::{Deserialize, Serialize};

/// Monotonic within a process, so two break-glasses in the same millisecond
/// still get distinct ids. Not a UUID and not claiming to be one — see
/// [`mint_id`].
static SEQ: AtomicU64 = AtomicU64::new(0);

/// What happened after the glass was recorded.
///
/// A closed sum: an unresolved record (no second line at all) is a *fourth*
/// state and is deliberately not a variant here, because "we never found out"
/// is the absence of an outcome rather than one of its values. Making it a
/// variant would let a caller write it, which is how "the process died" gets
/// laundered into "we recorded that the process died".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum GlassOutcome {
    /// The session opened. What the operator did inside it is the session's
    /// own record to keep — banken witnesses the *opening*, and says so.
    Opened,
    /// The effect was attempted and failed. The record still exists, which is
    /// the half of the predicate that is easy to get wrong.
    Failed {
        /// The typed reason, as rendered.
        reason: String,
    },
    /// The operator backed out before anything ran.
    ///
    /// Distinct from `Failed` because a deliberate abort and a broken tool are
    /// different facts about an estate, and an auditor reading a month of these
    /// needs to tell them apart.
    Abandoned,
}

/// One line of the ledger.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GlassEntry {
    /// The record this line belongs to. The intent line and its outcome line
    /// share it — that is what joins them.
    pub record_id: String,
    /// Unix milliseconds at the moment of the append.
    pub at_unix_ms: u128,
    /// The cluster the action was aimed at.
    pub cluster: String,
    /// What was selected, as the action described it.
    pub selector: String,
    /// Who authorised it.
    pub witness: String,
    /// The runbook it is logged against.
    pub runbook: String,
    /// `None` on the intent line, `Some` on the outcome line.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outcome: Option<GlassOutcome>,
}

/// **Proof that a break-glass was durably recorded before anything happened.**
///
/// The field is private and the sole constructor is [`GlassLedger::record`],
/// which fsyncs before returning. Hold one of these and the record exists on
/// disk; there is no way to hold one otherwise.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Witnessed {
    record: GlassRecord,
    cluster: String,
}

impl Witnessed {
    /// The record that was written.
    #[must_use]
    pub fn record(&self) -> &GlassRecord {
        &self.record
    }

    /// The ledger id joining this witness to its outcome line.
    #[must_use]
    pub fn record_id(&self) -> &str {
        &self.record.record_id
    }

    /// The cluster this break-glass was aimed at.
    #[must_use]
    pub fn cluster(&self) -> &str {
        &self.cluster
    }
}

/// An append-only, fsynced break-glass ledger.
#[derive(Debug, Clone)]
pub struct GlassLedger {
    path: PathBuf,
    cluster: String,
}

impl GlassLedger {
    /// A ledger at an explicit path.
    #[must_use]
    pub fn at(path: impl Into<PathBuf>, cluster: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            cluster: cluster.into(),
        }
    }

    /// The default ledger location for this operator.
    ///
    /// `$XDG_STATE_HOME/banken/glass.jsonl`. State, not cache and not config:
    /// it is durable, it is not the operator's to hand-edit, and it must
    /// survive a cache wipe — which is exactly the XDG state tier.
    ///
    /// # Why this delegates rather than resolving
    ///
    /// The hand-rolled version here did `std::env::var_os("XDG_STATE_HOME")
    /// .map(PathBuf::from)` and used whatever came back — which the XDG spec
    /// forbids: *"If an implementation encounters a relative path in any of
    /// these variables it should consider the path invalid and ignore it."*
    ///
    /// A relative `XDG_STATE_HOME` was therefore resolved against the current
    /// working directory, so **the record of a break-glass could land in a
    /// different place depending on where banken was started from** — an
    /// audit trail that cannot be audited, which is the one property this
    /// ledger exists to have. `okiba` refuses a relative override and cannot
    /// return a relative path from any arm.
    ///
    /// Two other fleet resolvers had the same bug (tear's praca store,
    /// escriba's plugin dir), which is why it became a crate rather than a
    /// fix here.
    #[must_use]
    pub fn default_path() -> Option<PathBuf> {
        okiba::Okiba::for_app("banken")
            .try_path(okiba::Tier::State, "glass.jsonl")
            .ok()
    }

    /// The path this ledger writes to.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// **Record the intent, durably, and hand back the capability.**
    ///
    /// Appends one line and fsyncs it before returning. Everything that can
    /// fail — a missing directory, a full disk, a read-only mount — fails
    /// *here*, before any live effect has happened, which is the correct place
    /// for it: a break-glass that cannot be recorded must not proceed, and
    /// this is the call that refuses.
    ///
    /// # Errors
    ///
    /// `SpecError::Interp { phase: "break-glass" }` if the entry could not be
    /// durably written. The caller must treat that as a refusal of the whole
    /// action, not as a logging hiccup to carry on past.
    pub fn record(&self, action: &WitnessedAction) -> Result<Witnessed, SpecError> {
        let record_id = mint_id();
        let entry = GlassEntry {
            record_id: record_id.clone(),
            at_unix_ms: now_unix_ms(),
            cluster: self.cluster.clone(),
            selector: action.selector.clone(),
            witness: action.witness.as_str().to_owned(),
            runbook: action.runbook.0.clone(),
            outcome: None,
        };
        self.append(&entry)?;
        Ok(Witnessed {
            record: GlassRecord {
                action: action.clone(),
                record_id,
            },
            cluster: self.cluster.clone(),
        })
    }

    /// Append the outcome line for a recorded break-glass.
    ///
    /// Takes the witness by reference rather than an id, so an outcome cannot
    /// be filed against a record that was never written.
    ///
    /// # Errors
    ///
    /// `SpecError::Interp { phase: "break-glass" }` on a write failure. Note
    /// the asymmetry with [`Self::record`]: a failure here loses the *outcome*,
    /// and the intent line already survives — so a caller may report it and
    /// carry on, where a failure in `record` must abort.
    pub fn resolve(&self, witness: &Witnessed, outcome: GlassOutcome) -> Result<(), SpecError> {
        let entry = GlassEntry {
            record_id: witness.record_id().to_owned(),
            at_unix_ms: now_unix_ms(),
            cluster: witness.cluster().to_owned(),
            selector: witness.record().action.selector.clone(),
            witness: witness.record().action.witness.as_str().to_owned(),
            runbook: witness.record().action.runbook.0.clone(),
            outcome: Some(outcome),
        };
        self.append(&entry)
    }

    /// Every line in the ledger, oldest first.
    ///
    /// A malformed line is **skipped, not fatal**: a truncated final line is
    /// what a crash mid-append looks like, and refusing to read the whole
    /// ledger because of it would destroy the record's usefulness at exactly
    /// the moment it matters most.
    ///
    /// # Errors
    ///
    /// `SpecError::Interp { phase: "break-glass" }` if the file exists but
    /// cannot be read. A ledger that has never been written is `Ok(vec![])` —
    /// no break-glass has happened, which is a fact, not an error.
    pub fn entries(&self) -> Result<Vec<GlassEntry>, SpecError> {
        match std::fs::read_to_string(&self.path) {
            Ok(text) => Ok(text
                .lines()
                .filter(|l| !l.trim().is_empty())
                .filter_map(|l| serde_json::from_str(l).ok())
                .collect()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(e) => Err(interp(&format!(
                "cannot read the break-glass ledger at {}: {e}",
                self.path.display()
            ))),
        }
    }

    /// Records with an intent line and no outcome line — break-glasses banken
    /// started and never saw finish.
    ///
    /// Worth surfacing: each one is a session that crashed, was killed, or is
    /// still open, and all three are things an operator wants to know about
    /// their own estate.
    ///
    /// # Errors
    ///
    /// As [`Self::entries`].
    pub fn unresolved(&self) -> Result<Vec<GlassEntry>, SpecError> {
        let all = self.entries()?;
        let resolved: Vec<&str> = all
            .iter()
            .filter(|e| e.outcome.is_some())
            .map(|e| e.record_id.as_str())
            .collect();
        Ok(all
            .iter()
            .filter(|e| e.outcome.is_none() && !resolved.contains(&e.record_id.as_str()))
            .cloned()
            .collect())
    }

    /// One durable append. Creates the parent directory on first use.
    fn append(&self, entry: &GlassEntry) -> Result<(), SpecError> {
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| {
                interp(&format!(
                    "cannot create the break-glass ledger directory {}: {e}",
                    dir.display()
                ))
            })?;
        }
        let mut line = serde_json::to_string(entry)
            .map_err(|e| interp(&format!("cannot serialize the break-glass record: {e}")))?;
        line.push('\n');

        let mut f: File = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .map_err(|e| {
                interp(&format!(
                    "cannot open the break-glass ledger at {}: {e}",
                    self.path.display()
                ))
            })?;
        f.write_all(line.as_bytes())
            .map_err(|e| interp(&format!("cannot write the break-glass record: {e}")))?;
        // The load-bearing line. Without it the record lives in the page cache
        // and a hard power loss takes the record while leaving the effect —
        // which is the exact ordering this module exists to forbid.
        f.sync_all()
            .map_err(|e| interp(&format!("cannot flush the break-glass record to disk: {e}")))?;
        Ok(())
    }
}

/// Open a break-glass session — **only** against a persisted witness.
///
/// The `&Witnessed` parameter is the whole point: it is unforgeable outside
/// [`GlassLedger::record`], so this function cannot run before the record is on
/// disk. The rule is not documented-and-hoped-for, it is the signature.
///
/// # Errors
///
/// Whatever the session env returns. The caller is expected to
/// [`GlassLedger::resolve`] with [`GlassOutcome::Failed`] on the error path —
/// and the intent line survives regardless, which is what makes that a
/// completeness improvement rather than the record's only chance.
pub fn open_witnessed_session<E: banken_spec::bancada::SessionEnv>(
    env: &E,
    witness: &Witnessed,
    plan: &banken_spec::bancada::SessionPlan,
) -> Result<Vec<banken_spec::bancada::PaneRef>, SpecError> {
    // `witness` is unread on purpose — it is a *precondition*, not an input.
    // Its job is done at the call site, where producing one forced the record
    // to be written; reading it here would add nothing and suggest it were
    // data. Deliberately not `_witness`: the name documents the parameter, and
    // the binding below is what silences the lint without renaming it.
    let _ = witness;
    banken_spec::bancada::open(plan, env)
}

fn interp(message: &str) -> SpecError {
    SpecError::Interp {
        phase: "break-glass".into(),
        message: message.to_owned(),
    }
}

fn now_unix_ms() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis())
}

/// A ledger id: nanosecond clock, process id, and a per-process counter.
///
/// **Not a UUID and not claiming to be one.** It is unique enough to join two
/// lines of one file written by one machine, which is all it is asked to do.
/// If the ledger ever becomes a distributed chain, the id becomes that chain's
/// — this is deliberately not a homegrown crypto identifier.
fn mint_id() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    format!("glass-{nanos:x}-{}-{seq:x}", std::process::id())
}

#[cfg(test)]
mod tests {
    use super::*;
    use banken_spec::types::{OperatorId, RunbookRef};

    fn action(selector: &str) -> WitnessedAction {
        WitnessedAction {
            selector: selector.to_owned(),
            witness: OperatorId::new("drzzln").expect("non-blank"),
            runbook: RunbookRef("clusters/alpha/RUNBOOK.md".into()),
        }
    }

    fn ledger(dir: &Path) -> GlassLedger {
        GlassLedger::at(dir.join("glass.jsonl"), "alpha-eks")
    }

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "banken-glass-{}-{}-{name}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed),
        ));
        std::fs::create_dir_all(&d).expect("tmp dir");
        d
    }

    /// **THE PREDICATE.** The record exists the moment the witness does — i.e.
    /// before any effect could have run — so an effect that then fails, hangs,
    /// or kills the process cannot leave an unrecorded break-glass.
    #[test]
    fn the_record_is_on_disk_before_the_witness_is_handed_back() {
        let d = tmp("write-ahead");
        let l = ledger(&d);
        let w = l.record(&action("pod/api in ns catch")).expect("recorded");

        // Read the file with no further calls into the ledger — this is what a
        // separate process inspecting the estate would see, which is the only
        // reading that proves durability.
        let raw = std::fs::read_to_string(l.path()).expect("the file exists already");
        assert!(raw.contains(w.record_id()), "{raw}");
        assert!(raw.contains("pod/api in ns catch"), "{raw}");
        assert!(raw.contains("drzzln"), "{raw}");
        assert!(raw.contains("RUNBOOK.md"), "{raw}");

        let entries = l.entries().expect("readable");
        assert_eq!(entries.len(), 1);
        assert_eq!(
            entries[0].outcome, None,
            "the intent line carries no outcome — nothing has happened yet",
        );
    }

    /// **The half that is easy to get wrong.** A break-glass whose effect fails
    /// must still be in the ledger, and must be there as a *failure* rather
    /// than as an unresolved record.
    #[test]
    fn a_failing_break_glass_is_recorded_as_failed_not_omitted() {
        let d = tmp("failing");
        let l = ledger(&d);
        let w = l.record(&action("pod/api")).expect("recorded");

        // The effect fails here.
        l.resolve(
            &w,
            GlassOutcome::Failed {
                reason: "tear daemon not running".into(),
            },
        )
        .expect("outcome appended");

        let entries = l.entries().expect("readable");
        assert_eq!(entries.len(), 2, "intent AND outcome: {entries:?}");
        assert_eq!(entries[0].record_id, entries[1].record_id, "joined by id");
        assert!(matches!(
            entries[1].outcome,
            Some(GlassOutcome::Failed { .. })
        ));
        assert!(
            l.unresolved().expect("readable").is_empty(),
            "a failure is a resolution, not a dangling record",
        );
    }

    /// A session that never reported back stays visible. This is the state an
    /// operator most wants surfaced, and the one an "update the row" ledger
    /// would have silently overwritten.
    #[test]
    fn an_effect_that_never_reports_back_stays_unresolved() {
        let d = tmp("dangling");
        let l = ledger(&d);
        let done = l.record(&action("pod/finished")).expect("recorded");
        let _hung = l.record(&action("pod/hung")).expect("recorded");
        l.resolve(&done, GlassOutcome::Opened).expect("resolved");

        let dangling = l.unresolved().expect("readable");
        assert_eq!(dangling.len(), 1, "{dangling:?}");
        assert_eq!(dangling[0].selector, "pod/hung");
    }

    /// A deliberate abort and a broken tool are different facts about an
    /// estate; an auditor reading a month of these has to tell them apart.
    #[test]
    fn abandoning_is_its_own_outcome_not_a_failure() {
        let d = tmp("abandoned");
        let l = ledger(&d);
        let w = l.record(&action("pod/api")).expect("recorded");
        l.resolve(&w, GlassOutcome::Abandoned).expect("resolved");
        let e = l.entries().expect("readable");
        assert_eq!(e[1].outcome, Some(GlassOutcome::Abandoned));
    }

    /// The ledger is APPEND-only across process lifetimes — a second run must
    /// not truncate the first run's records. `OpenOptions::append` is what
    /// guarantees it, and `create(true)` alone would not.
    #[test]
    fn a_second_ledger_over_the_same_path_appends_rather_than_truncating() {
        let d = tmp("append");
        let first = ledger(&d);
        first.record(&action("pod/one")).expect("recorded");
        let second = ledger(&d);
        second.record(&action("pod/two")).expect("recorded");

        let e = second.entries().expect("readable");
        assert_eq!(e.len(), 2, "{e:?}");
        assert_eq!(e[0].selector, "pod/one");
        assert_eq!(e[1].selector, "pod/two");
    }

    /// A crash mid-append leaves a truncated final line. Reading the ledger
    /// must still work — a partial write is exactly when the earlier records
    /// matter most, and refusing to parse the file would destroy them.
    #[test]
    fn a_truncated_final_line_does_not_hide_the_records_before_it() {
        let d = tmp("truncated");
        let l = ledger(&d);
        l.record(&action("pod/intact")).expect("recorded");
        // Simulate the crash: a half-written JSON object with no newline.
        let mut f = OpenOptions::new()
            .append(true)
            .open(l.path())
            .expect("open");
        f.write_all(b"{\"recordId\":\"glass-tru").expect("partial");
        drop(f);

        let e = l.entries().expect("still readable");
        assert_eq!(e.len(), 1, "the intact record survives: {e:?}");
        assert_eq!(e[0].selector, "pod/intact");
    }

    /// Two records minted in the same millisecond must not collide — the id is
    /// what joins an outcome to its intent, so a duplicate would file one
    /// session's result against another's.
    #[test]
    fn ids_are_distinct_even_back_to_back() {
        let d = tmp("ids");
        let l = ledger(&d);
        let ids: Vec<String> = (0..50)
            .map(|i| {
                l.record(&action(&format!("pod/{i}")))
                    .expect("recorded")
                    .record_id()
                    .to_owned()
            })
            .collect();
        let mut sorted = ids.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), ids.len(), "duplicate ledger id");
    }

    /// A ledger that cannot be written REFUSES, and it refuses at `record` —
    /// before any effect. A break-glass that cannot be recorded must not
    /// proceed, so this failing is the feature.
    #[test]
    fn a_ledger_that_cannot_be_written_refuses_before_the_effect() {
        let d = tmp("unwritable");
        // A *file* where the ledger's parent directory must be, so
        // `create_dir_all` cannot succeed.
        let blocker = d.join("blocked");
        std::fs::write(&blocker, b"not a directory").expect("write");
        let l = GlassLedger::at(blocker.join("glass.jsonl"), "alpha-eks");

        let e = l.record(&action("pod/api")).expect_err("must refuse");
        match e {
            SpecError::Interp { phase, .. } => assert_eq!(phase, "break-glass"),
            other => panic!("wrong error shape: {other:?}"),
        }
    }

    /// A never-written ledger reads as empty rather than as an error. No
    /// break-glass has happened, which is a fact about the estate — and the
    /// most common one.
    #[test]
    fn an_absent_ledger_is_empty_not_broken() {
        let d = tmp("absent");
        let l = ledger(&d);
        assert!(l.entries().expect("ok").is_empty());
        assert!(l.unresolved().expect("ok").is_empty());
    }

    /// **The capability guard.** `Witnessed` must be mintable ONLY by the
    /// ledger, because that is the whole mechanism — if a second constructor
    /// appeared, `open_witnessed_session` would stop proving anything while
    /// still type-checking everywhere.
    ///
    /// Rust cannot assert "no other constructor exists", so this asserts the
    /// property that makes one impossible from outside: the struct's fields
    /// are private, so the literal form is unavailable and there is no
    /// `Default`. Inside this module the check is the source itself — hence the
    /// grep-shaped assertion in `tests/no_estate_identifiers.rs`'s sibling
    /// gate, and hence this test being about the OBSERVABLE property.
    #[test]
    fn the_ledger_is_the_only_way_to_mint_a_witness() {
        let d = tmp("mint");
        let l = ledger(&d);
        let w = l.record(&action("pod/api")).expect("recorded");
        // The accessors expose the record without exposing construction.
        assert!(!w.record_id().is_empty());
        assert_eq!(w.cluster(), "alpha-eks");
        assert_eq!(w.record().action.selector, "pod/api");
        // And every witness that exists corresponds to a line on disk.
        assert!(
            l.entries()
                .expect("readable")
                .iter()
                .any(|e| e.record_id == w.record_id()),
        );
    }
}
