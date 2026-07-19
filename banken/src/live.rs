//! `KubeClusterEnv` — the live-cluster OBSERVE backend (BANKEN.md §VI M0
//! destination-tier / §VII "M0 read for a rio cluster").
//!
//! # pending-banken: live-read
//!
//! **Tier: DESIGN / UNTESTED-LIVE this session.** No Kubernetes cluster is
//! reachable this session (rio/camelot VPN-gated, local k3s down), so this
//! backend's read path has **not** been exercised against a real apiserver.
//! The code is real (a typed `kube` client, never a fake `Ok`) and the seam
//! is wired — it lights up the instant a cluster returns. Do NOT conflate
//! "renders from fixtures" (PROVEN) with "live cluster read" (this — DESIGN).
//!
//! **NO SHELL.** This reads pods via the typed `kube` apiserver client
//! against the current kubeconfig context — never a `kubectl` subprocess.
//! The org NO-SHELL rule holds.
//!
//! Compiled only under the `live` feature so the default (fixture) build
//! stays lean. It implements the **same** [`ClusterEnv`] seam the fixture
//! env does — the app runtime is byte-identical against either source.
//!
//! Only the OBSERVE read half (`list_resources` for pods) is implemented;
//! DECLARE opens a branch+PR (M3) and BREAK-GLASS routes through the raw
//! kubernetes MCP (the §IX named interim executor) — neither is a live
//! mutation this backend performs, so both return a typed
//! `SpecError::Interp` "not yet wired" rather than a silent wrong `Ok`.

use banken_spec::env::{
    ChangeRef, ClusterEnv, DeclareChange, DepTree, Event, GlassRecord, HealthReading, LogStream,
    ResourceRef, Row, WatchStream, WitnessedAction, Workload,
};
use banken_spec::error::SpecError;
use banken_spec::types::ResourceKind;

use k8s_openapi::api::core::v1::Pod;
use kube::api::ListParams;
use kube::{Api, Client};

/// A live cluster env reading through the typed `kube` client against the
/// current kubeconfig context. Constructed async (the client resolves the
/// kubeconfig + builds the TLS stack); reads are blocking-from-the-caller's
/// view via a small runtime handle so the sync [`ClusterEnv`] trait is
/// satisfied without changing the shipped seam.
pub struct KubeClusterEnv {
    client: Client,
    handle: tokio::runtime::Handle,
}

impl KubeClusterEnv {
    /// Build a live env from the current kubeconfig context.
    ///
    /// # Errors
    ///
    /// Returns a `SpecError::Interp { phase: "connect" }` if the kubeconfig
    /// cannot be resolved or the client cannot be constructed (the honest
    /// failure when no cluster / VPN is up — never a fake success).
    pub async fn connect() -> Result<Self, SpecError> {
        let client = Client::try_default().await.map_err(|e| SpecError::Interp {
            phase: "connect".into(),
            message: e.to_string(),
        })?;
        Ok(Self {
            client,
            handle: tokio::runtime::Handle::current(),
        })
    }

    /// Block on `fut` using the captured runtime handle. Used to satisfy the
    /// synchronous [`ClusterEnv`] read methods from an async client.
    fn block<F, T>(&self, fut: F) -> T
    where
        F: std::future::Future<Output = T>,
    {
        tokio::task::block_in_place(|| self.handle.block_on(fut))
    }

    async fn list_pods(&self, ns: Option<&str>) -> Result<Vec<Row>, SpecError> {
        let api: Api<Pod> = match ns {
            Some(ns) => Api::namespaced(self.client.clone(), ns),
            None => Api::all(self.client.clone()),
        };
        let list = api
            .list(&ListParams::default())
            .await
            .map_err(|e| SpecError::Interp {
                phase: "list-resources".into(),
                message: e.to_string(),
            })?;
        Ok(list.items.into_iter().map(pod_to_row).collect())
    }
}

/// Project a `k8s_openapi` Pod into a banken [`Row`] with the canonical
/// `:pods` columns (READY / STATUS / RESTARTS / AGE). Read-only projection.
fn pod_to_row(pod: Pod) -> Row {
    let name = pod.metadata.name.clone().unwrap_or_default();
    let namespace = pod.metadata.namespace.clone();

    let status = pod.status.as_ref();
    let phase = status
        .and_then(|s| s.phase.clone())
        .unwrap_or_else(|| "Unknown".into());

    // READY = ready_containers / total_containers.
    let (ready, total, restarts) = status
        .and_then(|s| s.container_statuses.as_ref())
        .map(|cs| {
            let total = cs.len();
            let ready = cs.iter().filter(|c| c.ready).count();
            let restarts: i32 = cs.iter().map(|c| c.restart_count).sum();
            (ready, total, restarts)
        })
        .unwrap_or((0, 0, 0));

    // Surface a waiting-reason (CrashLoopBackOff, ImagePullBackOff, …) as
    // the STATUS when present — matches k9s's column semantics.
    let display_status = status
        .and_then(|s| s.container_statuses.as_ref())
        .and_then(|cs| {
            cs.iter().find_map(|c| {
                c.state
                    .as_ref()
                    .and_then(|st| st.waiting.as_ref())
                    .and_then(|w| w.reason.clone())
            })
        })
        .unwrap_or(phase);

    let ready_cell = {
        let mut s = String::new();
        s.push_str(&ready.to_string());
        s.push('/');
        s.push_str(&total.to_string());
        s
    };

    Row {
        name,
        namespace,
        cells: vec![
            ("READY".into(), ready_cell),
            ("STATUS".into(), display_status),
            ("RESTARTS".into(), restarts.to_string()),
            // AGE derivation from creation_timestamp is a follow-up
            // (needs a clock); shown as "-" rather than a wrong value.
            ("AGE".into(), "-".into()),
        ],
    }
}

impl ClusterEnv for KubeClusterEnv {
    fn list_resources(&self, kind: ResourceKind, ns: Option<&str>) -> Result<Vec<Row>, SpecError> {
        match kind {
            ResourceKind::Pod => self.block(self.list_pods(ns)),
            // Other kinds land as views are added (BANKEN.md §VI M1). A
            // typed error, never a fake empty `Ok` that hides the gap.
            other => Err(SpecError::Interp {
                phase: "list-resources".into(),
                message: {
                    let mut m = String::from(other.label());
                    m.push_str(" live read not yet wired (pending-banken: live-read M1)");
                    m
                },
            }),
        }
    }

    fn get_resource(
        &self,
        _kind: ResourceKind,
        _name: &str,
        _ns: Option<&str>,
    ) -> Result<Row, SpecError> {
        Err(SpecError::Interp {
            phase: "get-resource".into(),
            message: "live get not yet wired (pending-banken: live-read M1)".into(),
        })
    }

    fn logs(&self, _pod: &str, _ns: &str, _follow: bool) -> Result<LogStream, SpecError> {
        // KubeClient has no `log` method (BANKEN.md §II); the interim log
        // source is the kubernetes MCP. Surface the gap, never a fake tail.
        Err(SpecError::Interp {
            phase: "logs".into(),
            message: "live logs via the kubernetes MCP (pending-banken: native-logs)".into(),
        })
    }

    fn events(&self, _ns: Option<&str>) -> Result<Vec<Event>, SpecError> {
        Err(SpecError::Interp {
            phase: "events".into(),
            message: "live events not yet wired (pending-banken: live-read M1)".into(),
        })
    }

    fn topology(&self, _root: &ResourceRef) -> Result<DepTree, SpecError> {
        Err(SpecError::Interp {
            phase: "topology".into(),
            message: "XRay tree is M2 (pending-banken: xray)".into(),
        })
    }

    fn health_signals(&self, _w: &Workload) -> Result<HealthReading, SpecError> {
        Err(SpecError::Interp {
            phase: "health-signals".into(),
            message: "health ward is M2 (pending-banken: health-panel)".into(),
        })
    }

    fn watch(&self, kind: ResourceKind, ns: Option<&str>) -> Result<WatchStream, SpecError> {
        // A poll-materialized batch (the true informer is M1 unbuilt
        // substrate — pending-banken: live-watch).
        Ok(WatchStream {
            kind_label: kind.label().to_string(),
            rows: self.list_resources(kind, ns)?,
        })
    }

    fn declare(&self, _change: DeclareChange) -> Result<ChangeRef, SpecError> {
        // DECLARE opens a branch+PR against the release.yaml owner (M3,
        // never direct-to-main). Not wired here; a typed error, never a
        // silent live effect.
        Err(SpecError::Interp {
            phase: "declare".into(),
            message: "DECLARE branch+PR flow is M3 (pending-banken: declare-rail)".into(),
        })
    }

    fn break_glass(&self, _action: WitnessedAction) -> Result<GlassRecord, SpecError> {
        // BREAK-GLASS exec routes through the kubernetes MCP `k8s-pod-exec`
        // (the §IX named interim executor), not this backend. A typed error
        // until that arm is wired.
        Err(SpecError::Interp {
            phase: "break-glass".into(),
            message: "break-glass exec via the kubernetes MCP (pending-banken: break-glass)".into(),
        })
    }

    // *** No unwitnessed-mutate method — ClusterEnv has none. ***
}

#[cfg(test)]
mod tests {
    use super::*;
    use k8s_openapi::api::core::v1::{
        ContainerState, ContainerStateWaiting, ContainerStatus, PodStatus,
    };

    fn pod_with(name: &str, phase: &str, waiting_reason: Option<&str>, ready: bool) -> Pod {
        let mut pod = Pod::default();
        pod.metadata.name = Some(name.into());
        pod.metadata.namespace = Some("catch".into());
        let mut cs = ContainerStatus {
            name: "main".into(),
            ready,
            restart_count: 3,
            ..Default::default()
        };
        if let Some(reason) = waiting_reason {
            cs.state = Some(ContainerState {
                waiting: Some(ContainerStateWaiting {
                    reason: Some(reason.into()),
                    ..Default::default()
                }),
                ..Default::default()
            });
        }
        pod.status = Some(PodStatus {
            phase: Some(phase.into()),
            container_statuses: Some(vec![cs]),
            ..Default::default()
        });
        pod
    }

    // Pure projection test — no cluster needed. Proves pod_to_row maps the
    // typed k8s Pod into the canonical banken columns correctly. (The LIVE
    // read itself is untested this session; this exercises the projection.)
    #[test]
    fn pod_to_row_projects_canonical_columns() {
        let row = pod_to_row(pod_with("catch-api", "Running", None, true));
        assert_eq!(row.name, "catch-api");
        let get = |k: &str| {
            row.cells
                .iter()
                .find(|(kk, _)| kk == k)
                .map(|(_, v)| v.as_str())
        };
        assert_eq!(get("READY"), Some("1/1"));
        assert_eq!(get("STATUS"), Some("Running"));
        assert_eq!(get("RESTARTS"), Some("3"));
    }

    #[test]
    fn pod_to_row_surfaces_waiting_reason_as_status() {
        let row = pod_to_row(pod_with(
            "catch-worker",
            "Pending",
            Some("CrashLoopBackOff"),
            false,
        ));
        let status = row
            .cells
            .iter()
            .find(|(k, _)| k == "STATUS")
            .map(|(_, v)| v.as_str());
        assert_eq!(
            status,
            Some("CrashLoopBackOff"),
            "a waiting reason takes precedence over the phase (k9s semantics)"
        );
        let ready = row
            .cells
            .iter()
            .find(|(k, _)| k == "READY")
            .map(|(_, v)| v.as_str());
        assert_eq!(ready, Some("0/1"));
    }
}
