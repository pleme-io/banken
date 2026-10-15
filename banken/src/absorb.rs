//! How a live stream reads its initial object set, and how the status line names
//! the source it reads. The streams themselves live in [`crate::armazem`] and
//! [`crate::kube_source`].

/// How the initial object set is obtained before the watch takes over.
///
/// # There is deliberately NO `Auto` variant
///
/// An `Auto` would probe for streaming-list support and fall back when it is
/// absent — and that fallback is an **unannounced downgrade to a weaker read
/// path**, which is the silent-degradation class banken exists to refuse. The
/// two strategies do not differ only in speed: they differ in what the
/// apiserver must implement, so "it quietly worked anyway" is precisely the
/// answer an operator must not receive. The strategy is named, or it is the
/// documented default.
///
/// # Why this is not merely a tuning knob
///
/// Measured against `alpha-eks` (EKS v1.33) 2026-08-08: `Streaming` is what
/// makes the initial set arrive as one `Init`/`InitApply`*/`InitDone` series
/// and publish ONCE (generation 1 for 83 pods) rather than buffering a whole
/// list body server-side. But `kube-runtime`'s `ListWatch` path sends **zero**
/// parameters that a minimal apiserver need not implement
/// (`watcher.rs:412-413` — `ListSemantic::MostRecent`, the default, emits
/// neither `resourceVersion` nor `resourceVersionMatch`), which is what lets
/// banken read an apiserver that has not yet negotiated `sendInitialEvents`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ListStrategy {
    /// `sendInitialEvents=true` + `resourceVersionMatch=NotOlderThan`,
    /// terminated by an `InitialEventsEnd` bookmark. One call, no whole-list
    /// buffer server-side. Requires an apiserver that negotiates it
    /// (upstream: beta 1.30, GA 1.32).
    ///
    /// **Failure mode when the server does NOT implement it, and it is silent:**
    /// the parameter is dropped, the read degrades to a live tail with no
    /// replay, the terminating bookmark never arrives, `InitDone` never fires
    /// and the replica never publishes. banken then sits in
    /// [`izumi_armazem::Phase::Absorbing`] with no rows and no error — which is the honest
    /// rendering, and the reason that phase exists rather than an empty table.
    #[default]
    Streaming,
    /// Classic LIST then WATCH. Sends no parameters a minimal apiserver need
    /// not implement, so it is the strategy that reads a conformance-partial
    /// backend.
    ListWatch,
}

/// The live source line the operator reads at the top of the table.
///
/// # Why this is a function and not two `push_str` runs
///
/// It used to be built inline in `main.rs` **and again** in
/// `tests/live_read.rs`. That is not merely duplication — it made the test
/// **structurally incapable of catching a drift in what the binary renders**,
/// because it asserted against a label it had constructed itself. Measured
/// 2026-08-08: adding the apiserver URL to `main.rs` changed nothing the test
/// could see, and the test still passed.
///
/// One function, both callers (Prime Directive: duplication is a bug).
///
/// `server` is `Some` only on the named-context path. A context NAME is a
/// label and is not unique — `KUBECONFIG` is a merge list — so the URL is the
/// part an operator can actually check "the right cluster" against. It is
/// **omitted rather than guessed** when unknown.
#[must_use]
pub fn live_source_label(context: &str, server: Option<&str>, strategy: ListStrategy) -> String {
    let mut label = String::from("source: LIVE ");
    label.push_str(context);
    if let Some(server) = server {
        label.push_str(" (");
        label.push_str(server);
        label.push(')');
    }
    // The strategy is part of the receipt, not a hidden default: which read
    // path was used decides what a missing table MEANS (a `Streaming` read
    // against a server that does not negotiate `sendInitialEvents` stalls in
    // `Absorbing` with no rows and no error), so the operator must be able to
    // see it without guessing.
    label.push_str(" [");
    label.push_str(strategy.label());
    label.push(']');
    label
}

impl ListStrategy {
    /// Every strategy — the catalog-reflection surface.
    pub const ALL: &'static [ListStrategy] = &[ListStrategy::Streaming, ListStrategy::ListWatch];

    /// Stable label for the CLI, the status line and telemetry.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            ListStrategy::Streaming => "streaming",
            ListStrategy::ListWatch => "list-watch",
        }
    }

    /// Parse an operator-supplied strategy name.
    ///
    /// Returns `None` for anything unrecognised — never a silent fall back to
    /// the default, which would make a typo an unannounced downgrade.
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|v| v.label() == s)
    }
}

#[cfg(test)]
mod strategy_tests {
    use super::*;

    /// The refusal that gives the type its meaning.
    ///
    /// A typo must not resolve to the default: `--list-strategy streming` has
    /// to be a refusal, because silently reading via a different strategy than
    /// the operator named is the same unannounced-downgrade class the missing
    /// `Auto` variant exists to forbid.
    #[test]
    fn an_unrecognised_strategy_is_refused_not_defaulted() {
        assert_eq!(
            ListStrategy::parse("streaming"),
            Some(ListStrategy::Streaming)
        );
        assert_eq!(
            ListStrategy::parse("list-watch"),
            Some(ListStrategy::ListWatch)
        );
        assert_eq!(ListStrategy::parse("streming"), None, "a typo must refuse");
        assert_eq!(ListStrategy::parse(""), None);
        assert_eq!(
            ListStrategy::parse("auto"),
            None,
            "there is no Auto, by design"
        );
    }

    /// Every variant round-trips through its label, so the CLI surface and the
    /// enum cannot drift (★★ CATALOG REFLECTION, gated both directions).
    #[test]
    fn every_strategy_round_trips_through_its_label() {
        for s in ListStrategy::ALL {
            assert_eq!(
                ListStrategy::parse(s.label()),
                Some(*s),
                "`{}` must round-trip",
                s.label()
            );
        }
        assert_eq!(
            ListStrategy::ALL.len(),
            2,
            "a new variant needs a label + a row here"
        );
    }
}
