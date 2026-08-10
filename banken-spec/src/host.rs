//! Host observation — the NODE plane, sibling to the cluster plane.
//!
//! banken observes clusters. A fleet needs the machine underneath observed the
//! same way: what is this node doing, and which processes are doing it. That is
//! htop's question, asked through banken's typed surface instead of a curses
//! program nobody can query.
//!
//! # Why this is a sibling trait, not a `ClusterEnv` method
//!
//! `ClusterEnv` is the cluster's afferent surface — every method takes a
//! `ResourceKind` or a namespace and reads through a kubeconfig. A host read
//! shares none of that: no namespace, no kind, no apiserver, and it is
//! meaningful on a machine that belongs to no cluster at all. Hanging
//! `host_reading()` off `ClusterEnv` would make every cluster implementation
//! answer a question it has no business answering, and would make a
//! host-only observer impossible to write.
//!
//! # Why the readings are NUMBERS
//!
//! The existing [`crate::env::HealthReading`] carries band PHASES —
//! `("MemoryBand", "Holding")` — which is the right shape for "is the
//! controller happy" and the wrong shape for "how loaded is this box". A phase
//! cannot be sorted, thresholded, or diffed across nodes. Every field here is a
//! number or a count, so a table can rank by it and an agent can compare two
//! machines without parsing prose.
//!
//! # Tier
//!
//! The typed border, a mock, and ONE live implementation
//! ([`SysinfoHostEnv`]). What does NOT exist yet is a `ViewSource::Host` arm,
//! so nothing in banken's view/table machinery consumes this — the readings are
//! real, and nothing displays them. That is the honest state, and the next
//! step: adding the arm will fail every exhaustive match until each is wired,
//! which is the point of adding it rather than special-casing a host view.

use crate::error::SpecError;
use serde::{Deserialize, Serialize};

/// A point-in-time reading of one host.
///
/// Every field is numeric so it can be sorted and compared. Counts that a
/// platform cannot supply are `None` rather than `0` — zero threads is a claim,
/// absence is not.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct HostReading {
    /// The node's name, as the fleet knows it.
    pub host: String,
    /// Seconds since boot.
    pub uptime_secs: u64,
    /// Physical cores. The denominator for `load_avg` — a load of 8 is idle on
    /// 32 cores and drowning on 2, and reporting the load without it invites
    /// exactly that misreading.
    pub cpu_cores: u32,
    /// 1/5/15-minute load averages.
    pub load_avg: (f32, f32, f32),
    /// Total physical memory in bytes.
    pub mem_total_bytes: u64,
    /// Used physical memory in bytes.
    pub mem_used_bytes: u64,
    /// Swap in use, in bytes. `None` on a machine with no swap configured —
    /// distinct from `Some(0)`, which means swap exists and is unused.
    pub swap_used_bytes: Option<u64>,
    /// Total process count.
    pub processes: u32,
}

impl HostReading {
    /// Memory used as a fraction of total, `None` when total is 0.
    ///
    /// Computed rather than stored: a stored percentage and a stored total can
    /// disagree, and then a reader has to guess which is stale.
    #[must_use]
    pub fn mem_fraction(&self) -> Option<f32> {
        if self.mem_total_bytes == 0 {
            return None;
        }
        #[allow(clippy::cast_precision_loss)]
        Some(self.mem_used_bytes as f32 / self.mem_total_bytes as f32)
    }

    /// 1-minute load per core — the comparable form across differently-sized
    /// machines, which raw load is not.
    #[must_use]
    pub fn load_per_core(&self) -> Option<f32> {
        if self.cpu_cores == 0 {
            return None;
        }
        #[allow(clippy::cast_precision_loss)]
        Some(self.load_avg.0 / self.cpu_cores as f32)
    }
}

/// One process on a host.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct ProcessReading {
    pub pid: u32,
    /// Command name, not the full argv — the full line is unbounded and this
    /// is a table column.
    pub name: String,
    /// CPU percent of ONE core, so a value above 100 is possible and correct
    /// on a multi-threaded process. Stated because the alternative convention
    /// (percent of the whole machine) makes the same number mean something
    /// four times smaller, and nothing in a bare `f32` says which is meant.
    pub cpu_pct: f32,
    /// Resident set size in bytes.
    pub rss_bytes: u64,
}

/// Which metric a process table was actually ranked by.
///
/// Exists because CPU is not always readable — but NOT for the reason first
/// written here. That comment claimed every process reports 0.0% "regardless of
/// refresh kind or sampling interval, MEASURED". That was FALSE, and it was
/// committed. The probe behind it took two samples and stopped.
///
/// Re-measured on the same host and pin, counting nonzero of ~590 processes
/// after each successive refresh:
///
/// ```text
///   after refresh #1:  0     <- where the original probe stopped
///   after refresh #2: 19
///   after refresh #3: 19
///   after refresh #4: 27
/// ```
///
/// sysinfo needs the process table to be seen at least twice AFTER the initial
/// population before a delta exists, so two samples is one short. The lesson is
/// the shape of the mistake: "I measured and saw nothing" is not the same claim
/// as "there is nothing to see", and only the second one justifies a permanent
/// comment.
///
/// The enum stays, because CPU genuinely can be unreadable — a short-lived
/// process, a restricted platform — and a table that says "top by CPU" while
/// returning an arbitrary order is worse than one that says what it could
/// actually sort by.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub enum RankedBy {
    /// CPU percent — available and non-degenerate.
    Cpu,
    /// Resident memory, because every CPU reading was zero.
    RssFallback,
}

/// A ranked process table that reports its own ranking metric.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct ProcessTable {
    pub ranked_by: RankedBy,
    pub rows: Vec<ProcessReading>,
}

/// The host afferent surface. Read-only by construction: there is no method
/// here that changes anything, so a host observer cannot become a host
/// controller by accident — the same posture `ClusterEnv` holds for OBSERVE.
pub trait HostEnv {
    /// Read the host's current state.
    ///
    /// # Errors
    /// A `SpecError::Interp { phase: "host-reading" }` when the platform read
    /// fails.
    fn host_reading(&self) -> Result<HostReading, SpecError>;

    /// The `limit` processes ranked by CPU, descending.
    ///
    /// Ranking belongs to the implementation because the platform already
    /// sorts while sampling; asking every caller to sort invites two callers
    /// ranking differently and calling both "top".
    ///
    /// # Errors
    /// A `SpecError::Interp { phase: "host-processes" }` when the read fails.
    fn top_processes(&self, limit: usize) -> Result<ProcessTable, SpecError>;
}


/// A live [`HostEnv`] backed by `sysinfo`.
///
/// The counters here are the same ones `pleme-io/tear` reads in its
/// `system_resources` MCP tool; the version is pinned to match so the two
/// cannot drift into reporting different numbers for one machine. See the
/// dependency comment in Cargo.toml for why this is a second copy rather than
/// an extracted crate, and what would trigger the extraction.
#[derive(Debug, Default)]
pub struct SysinfoHostEnv;

impl HostEnv for SysinfoHostEnv {
    fn host_reading(&self) -> Result<HostReading, SpecError> {
        use sysinfo::System;
        let mut sys = System::new();
        sys.refresh_memory();
        sys.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
        let load = System::load_average();

        // `physical_core_count` is an Option because a platform may not expose
        // it; 0 would be a claim, and `load_per_core` already returns None on
        // a zero denominator rather than dividing.
        let cores = sys.physical_core_count().unwrap_or(0);
        let swap_total = sys.total_swap();

        Ok(HostReading {
            host: System::host_name().unwrap_or_else(|| "unknown".to_string()),
            uptime_secs: System::uptime(),
            cpu_cores: u32::try_from(cores).unwrap_or(u32::MAX),
            #[allow(clippy::cast_possible_truncation)]
            load_avg: (load.one as f32, load.five as f32, load.fifteen as f32),
            mem_total_bytes: sys.total_memory(),
            mem_used_bytes: sys.used_memory(),
            // None when no swap is CONFIGURED, Some(n) when it exists — the
            // distinction the type exists to preserve, resolved here from the
            // total rather than from the used value, since used==0 is a real
            // and different state.
            swap_used_bytes: (swap_total > 0).then(|| sys.used_swap()),
            processes: u32::try_from(sys.processes().len()).unwrap_or(u32::MAX),
        })
    }

    fn top_processes(&self, limit: usize) -> Result<ProcessTable, SpecError> {
        use sysinfo::System;
        let mut sys = System::new();
        // Two refreshes with a gap: sysinfo computes CPU as a DELTA between
        // samples, so a single refresh reports 0.0 for everything. A "top by
        // CPU" that always returns zeros would sort arbitrarily while looking
        // like it worked.
        // CPU is a DELTA and needs the table seen more than twice: measured on
        // this host, sample 2 still reports 0 nonzero and sample 3 reports 19.
        // Two refreshes — the obvious reading of "a delta needs two samples" —
        // is one short, and produces an all-zero ranking that looks sorted.
        // `.with_memory()` too, because RSS is the fallback metric and a
        // CPU-only refresh leaves it at 0, which would make the fallback rank
        // over zeros exactly the way the CPU path did.
        //
        // `pleme-io/tear` calls `refresh_all()` ONCE and reads `cpu_usage()`,
        // which is sample 1 — always 0.
        let kind = sysinfo::ProcessRefreshKind::new().with_cpu().with_memory();
        for _ in 0..3 {
            sys.refresh_processes_specifics(sysinfo::ProcessesToUpdate::All, true, kind);
            std::thread::sleep(sysinfo::MINIMUM_CPU_UPDATE_INTERVAL);
        }
        sys.refresh_processes_specifics(sysinfo::ProcessesToUpdate::All, true, kind);

        let rows: Vec<ProcessReading> = sys
            .processes()
            .iter()
            .map(|(pid, p)| ProcessReading {
                pid: pid.as_u32(),
                name: p.name().to_string_lossy().into_owned(),
                cpu_pct: p.cpu_usage(),
                rss_bytes: p.memory(),
            })
            .collect();
        Ok(rank(rows, limit))
    }
}

/// Rank a process list, falling back to RSS when CPU is uniformly zero.
///
/// Shared by every implementation so the fallback rule cannot differ between
/// the mock and the live reader — a mock that ranks differently from production
/// is a test that proves the wrong thing.
#[must_use]
pub fn rank(mut rows: Vec<ProcessReading>, limit: usize) -> ProcessTable {
    let cpu_usable = rows.iter().any(|p| p.cpu_pct > 0.0);
    if cpu_usable {
        rows.sort_by(|a, b| b.cpu_pct.total_cmp(&a.cpu_pct));
    } else {
        rows.sort_by(|a, b| b.rss_bytes.cmp(&a.rss_bytes));
    }
    rows.truncate(limit);
    ProcessTable {
        ranked_by: if cpu_usable { RankedBy::Cpu } else { RankedBy::RssFallback },
        rows,
    }
}

/// A fixed reading, for tests and for exercising a view without a machine.
#[derive(Debug, Clone)]
pub struct MockHostEnv {
    pub reading: HostReading,
    pub processes: Vec<ProcessReading>,
}

impl HostEnv for MockHostEnv {
    fn host_reading(&self) -> Result<HostReading, SpecError> {
        Ok(self.reading.clone())
    }

    fn top_processes(&self, limit: usize) -> Result<ProcessTable, SpecError> {
        Ok(rank(self.processes.clone(), limit))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reading() -> HostReading {
        HostReading {
            host: "cid".into(),
            uptime_secs: 3_600,
            cpu_cores: 8,
            load_avg: (4.0, 3.0, 2.0),
            mem_total_bytes: 32 * 1024 * 1024 * 1024,
            mem_used_bytes: 8 * 1024 * 1024 * 1024,
            swap_used_bytes: None,
            processes: 412,
        }
    }

    /// The reason this type exists: numbers that can be compared, where
    /// `HealthReading` carries phase strings that cannot.
    #[test]
    fn derived_values_are_computed_not_stored() {
        let r = reading();
        assert_eq!(r.mem_fraction(), Some(0.25));
        assert_eq!(r.load_per_core(), Some(0.5));
    }

    /// A load of 4 is busy on 2 cores and idle on 32. Without the denominator
    /// the number is unreadable, which is why `cpu_cores` is not optional.
    #[test]
    fn load_is_only_meaningful_against_the_core_count() {
        let small = HostReading { cpu_cores: 2, ..reading() };
        let big = HostReading { cpu_cores: 32, ..reading() };
        assert_eq!(small.load_per_core(), Some(2.0), "saturated");
        assert_eq!(big.load_per_core(), Some(0.125), "idle");
        // Same raw load, opposite conclusions.
        assert!((small.load_avg.0 - big.load_avg.0).abs() < f32::EPSILON);
    }

    #[test]
    fn a_zero_denominator_yields_none_rather_than_a_divide() {
        let r = HostReading { cpu_cores: 0, mem_total_bytes: 0, ..reading() };
        assert_eq!(r.load_per_core(), None);
        assert_eq!(r.mem_fraction(), None);
    }

    /// Absence and zero are different claims, and swap is where that bites:
    /// `None` means no swap is configured, `Some(0)` means swap exists and is
    /// unused. Collapsing them would report a swapless machine as healthy-with-
    /// swap-free, which is a different fact.
    #[test]
    fn absent_swap_is_not_zero_swap() {
        assert_eq!(reading().swap_used_bytes, None);
        let swapped = HostReading { swap_used_bytes: Some(0), ..reading() };
        assert_ne!(swapped.swap_used_bytes, reading().swap_used_bytes);
    }

    #[test]
    fn top_processes_ranks_by_cpu_and_respects_the_limit() {
        let p = |pid, name: &str, cpu| ProcessReading {
            pid,
            name: name.into(),
            cpu_pct: cpu,
            rss_bytes: 1024,
        };
        let env = MockHostEnv {
            reading: reading(),
            processes: vec![p(1, "idle", 0.5), p(2, "hot", 190.0), p(3, "warm", 40.0)],
        };
        let top = env.top_processes(2).unwrap();
        assert_eq!(top.ranked_by, RankedBy::Cpu, "CPU was readable, so it must be used");
        assert_eq!(top.rows.len(), 2, "the limit is respected");
        assert_eq!(top.rows[0].name, "hot", "ranked by CPU descending");
        assert_eq!(top.rows[1].name, "warm");
        // Above 100 is correct: cpu_pct is percent of ONE core.
        assert!(top.rows[0].cpu_pct > 100.0);
    }

    /// **The LIVE implementation, against this machine.**
    ///
    /// Every test above uses the mock, so they prove the TYPE and nothing about
    /// whether sysinfo is being read correctly. Ignored by default because it
    /// depends on the machine it runs on and sleeps for a CPU sample.
    ///
    ///   cargo test -p banken-spec `reads_this_machine` -- --ignored --nocapture
    #[test]
    #[ignore = "reads the real host; run explicitly"]
    fn reads_this_machine() {
        let env = SysinfoHostEnv;
        let r = env.host_reading().expect("host reading");
        println!(
            "\n{} | {} cores | load {:.2} ({:.2}/core) | mem {:.1}% | swap {:?} | {} procs",
            r.host,
            r.cpu_cores,
            r.load_avg.0,
            r.load_per_core().unwrap_or(0.0),
            r.mem_fraction().unwrap_or(0.0) * 100.0,
            r.swap_used_bytes,
            r.processes
        );
        assert!(r.cpu_cores > 0, "0 cores means the read failed, not a coreless machine");
        assert!(r.mem_total_bytes > 0, "0 total memory is not a real reading");
        assert!(r.processes > 0, "a machine running this test has processes");

        let top = env.top_processes(5).expect("processes");
        println!("   ranked_by: {:?}", top.ranked_by);
        for p in &top.rows {
            println!("   {:>7} {:<24} cpu {:>6.1}%  rss {} MB", p.pid, p.name, p.cpu_pct, p.rss_bytes / 1_048_576);
        }
        assert!(!top.rows.is_empty(), "no processes returned");
        // The table must NAME what it ranked by. On macOS every cpu_usage() is
        // 0.0 (measured), so the honest outcome here is RssFallback with a
        // descending RSS order — not a CPU claim over arbitrary data.
        match top.ranked_by {
            RankedBy::Cpu => assert!(
                top.rows.iter().any(|p| p.cpu_pct > 0.0),
                "claimed a CPU ranking while every reading is 0"
            ),
            RankedBy::RssFallback => {
                let rss: Vec<u64> = top.rows.iter().map(|p| p.rss_bytes).collect();
                let mut sorted = rss.clone();
                sorted.sort_unstable_by(|a, b| b.cmp(a));
                assert_eq!(rss, sorted, "fell back to RSS but did not sort by it");
            }
        }
    }

    /// The fallback is a RULE, not a platform accident — pinned here so the
    /// mock and the live reader cannot diverge on it.
    #[test]
    fn all_zero_cpu_falls_back_to_rss_and_says_so() {
        let p = |pid, name: &str, cpu, rss| ProcessReading {
            pid,
            name: name.into(),
            cpu_pct: cpu,
            rss_bytes: rss,
        };
        let t = rank(vec![p(1, "small", 0.0, 10), p(2, "big", 0.0, 999)], 5);
        assert_eq!(t.ranked_by, RankedBy::RssFallback, "no usable CPU, so say RSS");
        assert_eq!(t.rows[0].name, "big", "and actually sort by it");

        // One non-zero reading is enough to make CPU the honest metric.
        let t = rank(vec![p(1, "small", 0.0, 999), p(2, "busy", 3.0, 10)], 5);
        assert_eq!(t.ranked_by, RankedBy::Cpu);
        assert_eq!(t.rows[0].name, "busy");
    }

    #[test]
    fn a_limit_larger_than_the_process_list_is_not_an_error() {
        let env = MockHostEnv { reading: reading(), processes: Vec::new() };
        assert_eq!(env.top_processes(10).unwrap().rows.len(), 0);
    }
}
