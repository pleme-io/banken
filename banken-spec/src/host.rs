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
//! This is the typed BORDER plus a mock. It is deliberately not a data source:
//! nothing here reads `/proc` or calls `sysinfo`. A real implementation is a
//! separate impl of [`HostEnv`], and until one exists this module claims
//! nothing about live observation.

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
    fn top_processes(&self, limit: usize) -> Result<Vec<ProcessReading>, SpecError>;
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

    fn top_processes(&self, limit: usize) -> Result<Vec<ProcessReading>, SpecError> {
        let mut ps = self.processes.clone();
        ps.sort_by(|a, b| b.cpu_pct.total_cmp(&a.cpu_pct));
        ps.truncate(limit);
        Ok(ps)
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
        assert_eq!(small.load_avg.0, big.load_avg.0);
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
        assert_eq!(top.len(), 2, "the limit is respected");
        assert_eq!(top[0].name, "hot", "ranked by CPU descending");
        assert_eq!(top[1].name, "warm");
        // Above 100 is correct: cpu_pct is percent of ONE core.
        assert!(top[0].cpu_pct > 100.0);
    }

    #[test]
    fn a_limit_larger_than_the_process_list_is_not_an_error() {
        let env = MockHostEnv { reading: reading(), processes: Vec::new() };
        assert_eq!(env.top_processes(10).unwrap().len(), 0);
    }
}
