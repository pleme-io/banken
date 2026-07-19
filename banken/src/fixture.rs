//! `FixtureClusterEnv` — the default OBSERVE source this session.
//!
//! No Kubernetes cluster is reachable this session (rio/camelot VPN-gated,
//! local k3s down — verified), so banken renders + dispatches against this
//! fixture source, exactly the sui-spec `MockEnv` discipline: a real,
//! verifiable increment whose read path is canned data. It implements the
//! **same** [`banken_spec::env::ClusterEnv`] seam the live backend does
//! ([`crate::live::KubeClusterEnv`] under the `live` feature), so the app
//! is identical against either — the source is a swap, not a rewrite.
//!
//! The fixture pods are deliberately varied (a `CrashLoopBackOff`, a
//! `Running`, a `Pending`, a multi-container `2/3 Running`) so the render
//! exercises the pathology-color axis and the sort surfaces the unhealthy
//! rows.

use banken_spec::env::{
    ChangeRef, ClusterEnv, DeclareChange, DepTree, Event, GlassRecord, HealthReading, LogStream,
    ResourceRef, Row, WatchStream, WitnessedAction, Workload,
};
use banken_spec::error::SpecError;
use banken_spec::types::ResourceKind;

/// A cluster env backed by canned fixture data — no network, no cluster.
/// Reads return the seeded fixture; the DECLARE path records a git change
/// (never a live mutation); break-glass records a witnessed action. There
/// is no unwitnessed-mutate method to implement — [`ClusterEnv`] has none.
pub struct FixtureClusterEnv {
    pods: Vec<Row>,
}

impl FixtureClusterEnv {
    /// A fixture env pre-seeded with a realistic mixed-health pod set.
    #[must_use]
    pub fn new() -> Self {
        Self {
            pods: fixture_pods(),
        }
    }
}

impl Default for FixtureClusterEnv {
    fn default() -> Self {
        Self::new()
    }
}

/// A realistic mixed set of fixture pods — one of every health shape the
/// render + sort must handle.
fn fixture_pods() -> Vec<Row> {
    let mk = |name: &str, ready: &str, status: &str, restarts: &str, age: &str| Row {
        name: name.into(),
        namespace: Some("catch".into()),
        cells: vec![
            ("READY".into(), ready.into()),
            ("STATUS".into(), status.into()),
            ("RESTARTS".into(), restarts.into()),
            ("AGE".into(), age.into()),
        ],
    };
    vec![
        mk("catch-api-7d9f", "1/1", "Running", "0", "3d"),
        mk("catch-worker-2a1c", "0/1", "CrashLoopBackOff", "14", "22m"),
        mk("catch-migrate-xk4", "0/1", "Pending", "0", "45s"),
        mk("catch-cache-9b3e", "2/3", "Running", "1", "3d"),
        mk("catch-web-5f2d", "1/1", "Running", "0", "3d"),
    ]
}

impl ClusterEnv for FixtureClusterEnv {
    // ── OBSERVE — canned reads, zero network ──

    fn list_resources(&self, kind: ResourceKind, _ns: Option<&str>) -> Result<Vec<Row>, SpecError> {
        match kind {
            ResourceKind::Pod => Ok(self.pods.clone()),
            // Other kinds are not fixtured for M0 — an empty read, never a
            // fake `Ok` claiming rows that do not exist.
            _ => Ok(Vec::new()),
        }
    }

    fn get_resource(
        &self,
        kind: ResourceKind,
        name: &str,
        _ns: Option<&str>,
    ) -> Result<Row, SpecError> {
        self.pods
            .iter()
            .find(|r| kind == ResourceKind::Pod && r.name == name)
            .cloned()
            .ok_or_else(|| SpecError::Interp {
                phase: "get-resource".into(),
                message: {
                    let mut m = String::from(kind.label());
                    m.push('/');
                    m.push_str(name);
                    m.push_str(" not in fixture");
                    m
                },
            })
    }

    fn logs(&self, pod: &str, _ns: &str, follow: bool) -> Result<LogStream, SpecError> {
        // A short canned log tail — enough to render the OBSERVE result.
        Ok(LogStream {
            pod: pod.to_string(),
            lines: vec![
                "[info] starting catch-worker".into(),
                "[error] connect ECONNREFUSED postgres:5432".into(),
                "[fatal] exiting after 14 retries".into(),
            ],
            follow,
        })
    }

    fn events(&self, _ns: Option<&str>) -> Result<Vec<Event>, SpecError> {
        Ok(vec![Event {
            kind: "Warning".into(),
            reason: "BackOff".into(),
            message: "Back-off restarting failed container".into(),
            involved: "pod/catch-worker-2a1c".into(),
        }])
    }

    fn topology(&self, root: &ResourceRef) -> Result<DepTree, SpecError> {
        Ok(DepTree {
            root: root.name.clone(),
            edges: Vec::new(),
        })
    }

    fn health_signals(&self, _w: &Workload) -> Result<HealthReading, SpecError> {
        Ok(HealthReading {
            band_phases: vec![
                ("MemoryBand".into(), "Holding".into()),
                ("ReplicaBand".into(), "AtFloor".into()),
            ],
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

    fn declare(&self, _change: DeclareChange) -> Result<ChangeRef, SpecError> {
        // The fixture env is read-only for demonstration; a real DECLARE
        // opens a branch+PR (BANKEN.md §VI M3, never direct-to-main). Here
        // we return a synthetic ref so the dispatch is exercisable end to
        // end without a git remote. This is honest: the fixture never
        // *commits* — it proves the postigo dispatch reaches `declare`.
        Ok(ChangeRef("fixture-declare-preview".into()))
    }

    // ── BREAK-GLASS — records the witnessed action ──

    fn break_glass(&self, action: WitnessedAction) -> Result<GlassRecord, SpecError> {
        Ok(GlassRecord {
            action,
            // A placeholder AnomalyChain handle (labelled, never rounded up
            // — the real chain ships later, BANKEN.md §VII).
            record_id: "fixture-anomaly-chain-record".into(),
        })
    }

    // *** No mutate method to implement — ClusterEnv has none. ***
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixture_lists_a_mixed_health_pod_set() {
        let env = FixtureClusterEnv::new();
        let pods = env.list_resources(ResourceKind::Pod, None).unwrap();
        assert_eq!(pods.len(), 5);
        let statuses: Vec<&str> = pods
            .iter()
            .filter_map(|p| {
                p.cells
                    .iter()
                    .find(|(k, _)| k == "STATUS")
                    .map(|(_, v)| v.as_str())
            })
            .collect();
        assert!(statuses.contains(&"CrashLoopBackOff"));
        assert!(statuses.contains(&"Running"));
        assert!(statuses.contains(&"Pending"));
    }

    #[test]
    fn non_pod_kinds_read_empty_not_fake() {
        let env = FixtureClusterEnv::new();
        let svcs = env.list_resources(ResourceKind::Service, None).unwrap();
        assert!(svcs.is_empty());
    }

    #[test]
    fn logs_return_a_canned_tail() {
        let env = FixtureClusterEnv::new();
        let logs = env.logs("catch-worker-2a1c", "catch", false).unwrap();
        assert_eq!(logs.pod, "catch-worker-2a1c");
        assert!(logs.lines.iter().any(|l| l.contains("ECONNREFUSED")));
    }
}
