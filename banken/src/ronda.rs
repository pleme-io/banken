//! ronda — the watchdog's rounds: which clusters are actually there.
//!
//! # The question this answers
//!
//! The picker lists eighteen contexts and every one of them looks exactly as
//! choosable as the others. Most are not: some are EKS endpoints on the public
//! internet, some are homelab addresses behind a WireGuard link that may or may
//! not be up, and one is a laptop that is probably asleep. The operator finds
//! out which is which by *picking one and waiting* — and the wait, before
//! [`crate::antessala`], was a blank screen, and after it is still a wait.
//!
//! So banken walks its rounds. Every context with a declared apiserver gets a
//! **bounded TCP connect**, all of them concurrently, repeated on an interval;
//! the picker draws the verdict beside each row. Choosing stops being a guess.
//!
//! # What a round PROVES, and what it does not
//!
//! A TCP accept proves exactly one thing: *something is listening on that
//! host and port, and the network between here and there carries packets.*
//!
//! It does **not** prove the apiserver is healthy, that the operator's
//! credentials are valid, that their SSO session is live, or that any
//! authorization will succeed. Those are the expensive questions —
//! `Client::try_from` and the exec credential helper — and they are
//! deliberately not asked here. [`Reach`] is named for what is measured
//! (`Open`, not `Ready`) because a green dot that quietly meant four things
//! would be worse than no dot: an operator would read "ready" and be
//! surprised by an auth failure, and the surprise would be banken's fault.
//!
//! `pending-banken: ronda-credentials` — a warm authenticated client pool is
//! the next rung and is NOT this. It means re-running each context's
//! credential helper on a timer, which on an EKS estate is a recurring
//! `aws eks get-token` per context: real background SSO traffic, and an
//! operator decision rather than a default.
//!
//! # Why the timeout is ours and not kube's
//!
//! kube's `Config` defaults to a 30-second connect timeout. Inheriting it
//! would make one unreachable homelab context hold a row in `Probing` for
//! half a minute — long enough that the answer arrives after the operator has
//! already chosen. A round is only useful if it finishes while the list is
//! still on screen, so [`PROBE_TIMEOUT`] is short and explicit, and a slow
//! answer is reported as [`Reach::Unreachable`] rather than waited for.
//!
//! The timeout wraps **name resolution as well as the connect**, which is not
//! a detail: a homelab hostname whose DNS server is itself behind the downed
//! link hangs in `getaddrinfo`, not in `connect`, and a timeout around only
//! the connect would never fire.
//!
//! # The name
//!
//! *ronda*, a watchman's round — the patrol you walk to find out what is still
//! standing. It composes with the navigator itself: `banken` is 番犬, the
//! watchdog, and this is the round it walks. Brazilian-Portuguese per the
//! fleet naming laws, and the literal gloss is the job.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use arc_swap::ArcSwap;

/// How long a single context's probe may take, resolution included.
///
/// Short on purpose — see the module docs. A round over eighteen contexts
/// therefore settles in at most this long, because they run concurrently.
pub const PROBE_TIMEOUT: Duration = Duration::from_millis(2500);

/// How long to wait before walking the rounds again.
///
/// A VPN comes up while the operator is reading the list, and the row should
/// notice. Cheap enough to repeat: a TCP connect to an already-open port is
/// one round trip and no authentication.
pub const ROUND_INTERVAL: Duration = Duration::from_secs(15);

/// What a round found at one context's apiserver address.
///
/// Named for what was MEASURED, never for what an operator wants to know.
/// See the module docs: `Open` is not `Ready`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Reach {
    /// No round has finished for this context yet.
    #[default]
    Probing,
    /// The address accepted a TCP connection. Nothing more is claimed.
    Open,
    /// The host answered and **refused** the port.
    ///
    /// A distinct arm from [`Self::Unreachable`] because it points somewhere
    /// different: something is up at that address and the apiserver is not
    /// listening — a stopped k3s, a rebooting node — where a timeout usually
    /// means the path itself is down (VPN, route, asleep). Collapsing the two
    /// would send the operator to the wrong thing to fix.
    Refused,
    /// The address could not be reached inside [`PROBE_TIMEOUT`] — a timeout,
    /// a dead route, or a name that would not resolve.
    Unreachable,
    /// There is nothing to probe: the kubeconfig declares no server for this
    /// context, or its URL has no host banken can extract.
    ///
    /// Reported rather than silently skipped, because "banken did not check"
    /// and "banken checked and it was down" are different facts and only one
    /// of them is about the cluster.
    Unprobeable,
}

impl Reach {
    /// The one-glyph marker for a picker row.
    #[must_use]
    pub fn marker(self) -> &'static str {
        match self {
            Self::Probing => "◌",
            Self::Open => "●",
            Self::Refused => "◍",
            Self::Unreachable => "○",
            Self::Unprobeable => "·",
        }
    }

    /// What the marker means, for a legend or a test.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Probing => "probing",
            Self::Open => "accepting connections",
            Self::Refused => "refused — host up, apiserver not listening",
            Self::Unreachable => "unreachable — no route, or asleep",
            Self::Unprobeable => "no apiserver declared",
        }
    }

    /// Whether a round has settled on this context.
    #[must_use]
    pub fn is_settled(self) -> bool {
        self != Self::Probing
    }
}

/// One round's findings, keyed by context name.
///
/// Immutable and published whole, so a frame can never draw half of a round —
/// the same reason [`crate::absorb`]'s snapshot is immutable.
pub type Findings = BTreeMap<String, Reach>;

/// The read half — what the picker holds.
#[derive(Debug, Clone)]
pub struct Ronda {
    findings: Arc<ArcSwap<Findings>>,
}

impl Ronda {
    /// A ronda that has found nothing and never will — the seam for the
    /// fixture path and for a test that wants no markers at all.
    #[must_use]
    pub fn inert() -> Self {
        Self {
            findings: Arc::new(ArcSwap::from_pointee(Findings::new())),
        }
    }

    /// This context's standing, or [`Reach::Probing`] when no round has
    /// reported it yet.
    #[must_use]
    pub fn reach(&self, context: &str) -> Reach {
        self.findings
            .load()
            .get(context)
            .copied()
            .unwrap_or_default()
    }

    /// Whether every context a round covers has settled.
    ///
    /// Drives the redraw cadence: fast while answers are still arriving, slow
    /// once they have. A picker that repaints five times a second forever
    /// because it *might* learn something is a busy loop with a nice
    /// justification.
    #[must_use]
    pub fn all_settled(&self) -> bool {
        let f = self.findings.load();
        !f.is_empty() && f.values().all(|r| r.is_settled())
    }

    /// How many contexts a round is covering (for a test and for a legend).
    #[must_use]
    pub fn covered(&self) -> usize {
        self.findings.load().len()
    }
}

/// The write half — what a round holds.
#[derive(Debug, Clone)]
pub struct RondaPublisher {
    findings: Arc<ArcSwap<Findings>>,
}

impl RondaPublisher {
    /// Publish one context's verdict, leaving the rest of the round alone.
    ///
    /// Copy-on-write: the map is rebuilt and swapped whole, so a reader
    /// either sees the update or does not, never a torn map. The maps are
    /// tens of entries, so the copy is cheaper than the lock it replaces.
    pub fn report(&self, context: &str, reach: Reach) {
        let mut next = Findings::clone(&self.findings.load());
        next.insert(context.to_owned(), reach);
        self.findings.store(Arc::new(next));
    }

    /// Seed every context as [`Reach::Probing`] before the round starts.
    ///
    /// Load-bearing rather than cosmetic: [`Ronda::all_settled`] is false only
    /// while something is outstanding, and a round that published nothing up
    /// front would read as "settled with no findings" for its whole first
    /// pass, so the picker would drop to the slow cadence and the markers
    /// would appear to arrive late.
    pub fn seed(&self, contexts: &[String]) {
        let mut next = Findings::new();
        for c in contexts {
            next.insert(c.clone(), Reach::Probing);
        }
        self.findings.store(Arc::new(next));
    }
}

/// A fresh ronda and its publisher — the shape [`crate::absorb::channel`] uses.
#[must_use]
pub fn channel() -> (Ronda, RondaPublisher) {
    let findings = Arc::new(ArcSwap::from_pointee(Findings::new()));
    (
        Ronda {
            findings: Arc::clone(&findings),
        },
        RondaPublisher { findings },
    )
}

/// The `host:port` a probe should dial, extracted from an apiserver URL.
///
/// Hand-parsed rather than pulled through a URL crate, and that is a scope
/// decision: banken needs an authority and a port from a string it already
/// trusts (kube parsed it first — an unparseable server never reaches here),
/// so the whole of URL semantics is not on the table. Anything this cannot
/// read becomes [`Reach::Unprobeable`], which is reported, not guessed at.
///
/// The default port comes from the SCHEME, because a kubeconfig routinely
/// omits it on an EKS endpoint (`https://ABC.gr7.us-east-2.eks.amazonaws.com`)
/// and dialling 6443 there would probe a port nothing has ever listened on and
/// report the estate down.
#[must_use]
pub fn dial_target(server: &str) -> Option<(String, u16)> {
    let (scheme, rest) = server.split_once("://")?;
    let default_port = match scheme {
        "https" => 443,
        "http" => 80,
        _ => return None,
    };
    // Strip any path, query or fragment — the authority is all that is dialled.
    let authority = rest
        .split(['/', '?', '#'])
        .next()
        .filter(|a| !a.is_empty())?;
    // Userinfo is legal in a URL and is not part of the address.
    let authority = authority.rsplit_once('@').map_or(authority, |(_, a)| a);

    // IPv6 literals are bracketed, and their colons are not port separators.
    if let Some(rest) = authority.strip_prefix('[') {
        let (host, tail) = rest.split_once(']')?;
        if host.is_empty() {
            return None;
        }
        let port = match tail.strip_prefix(':') {
            Some(p) => p.parse().ok()?,
            None => default_port,
        };
        return Some((host.to_owned(), port));
    }

    match authority.rsplit_once(':') {
        Some((host, port)) if !host.is_empty() => Some((host.to_owned(), port.parse().ok()?)),
        Some(_) => None,
        None => Some((authority.to_owned(), default_port)),
    }
}

/// Probe one address: does anything accept a TCP connection there?
///
/// The timeout wraps resolution AND the connect — see the module docs for why
/// that distinction is load-bearing on a homelab name.
pub async fn probe(host: &str, port: u16) -> Reach {
    let mut target = String::from(host);
    target.push(':');
    target.push_str(&port.to_string());

    match tokio::time::timeout(PROBE_TIMEOUT, tokio::net::TcpStream::connect(&target)).await {
        // The connection is dropped immediately. banken wanted the answer,
        // not the socket — holding it open would leave eighteen idle
        // connections against production apiservers every fifteen seconds.
        Ok(Ok(_stream)) => Reach::Open,
        Ok(Err(e)) if e.kind() == std::io::ErrorKind::ConnectionRefused => Reach::Refused,
        // Every other io error, and the timeout, mean the same thing to an
        // operator: banken could not get there. The distinction that matters
        // is refused-vs-not, and that one is above.
        Ok(Err(_)) | Err(_) => Reach::Unreachable,
    }
}

/// Walk the rounds forever: probe every target concurrently, publish, wait,
/// repeat.
///
/// `targets` is `(context name, declared server)`. A context with no server,
/// or one whose URL yields no address, settles once as
/// [`Reach::Unprobeable`] and is not dialled again — there is nothing there to
/// change.
///
/// **Seeded once, before the first round, and never re-seeded.** Re-seeding
/// each pass would flip every marker back to `probing` on a fifteen-second
/// cycle, so a list an operator was reading would strobe. A re-probe updates
/// in place; a row only ever changes when its answer does.
///
/// The handle is the round's lifetime: drop it and the rounds stop.
///
/// **`main` holds it for the whole picker session, which includes the time the
/// navigator is open** — so an operator who opens a cluster, works, and comes
/// back to the list lands on findings that are current rather than on a screen
/// of `probing`. The cost of that choice is stated rather than hidden: banken
/// keeps making one TCP connect per context every [`ROUND_INTERVAL`] against
/// every declared apiserver, including production ones, for as long as it is
/// running. No authentication, no request, no bytes past the handshake — but
/// it is not zero, and an estate with connection logging will see it.
///
/// The `--context` path starts no rounds at all: there is no list to mark.
pub fn spawn_rounds(
    targets: Vec<(String, Option<String>)>,
    publisher: RondaPublisher,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let names: Vec<String> = targets.iter().map(|(n, _)| n.clone()).collect();
        publisher.seed(&names);

        // Resolved once: the addresses come from a kubeconfig that was already
        // parsed, and re-deriving them every fifteen seconds would be work
        // that cannot produce a different answer.
        let dials: Vec<(String, Option<(String, u16)>)> = targets
            .into_iter()
            .map(|(name, server)| {
                let target = server.as_deref().and_then(dial_target);
                (name, target)
            })
            .collect();

        for (name, target) in &dials {
            if target.is_none() {
                publisher.report(name, Reach::Unprobeable);
            }
        }

        loop {
            let mut round = Vec::new();
            for (name, target) in &dials {
                let Some((host, port)) = target.clone() else {
                    continue;
                };
                let name = name.clone();
                let publisher = publisher.clone();
                // One task per context, so eighteen probes cost one timeout of
                // wall-clock rather than eighteen. Each publishes as it lands,
                // so the fast answers light up while the slow ones are still
                // outstanding.
                round.push(tokio::spawn(async move {
                    let reach = probe(&host, port).await;
                    publisher.report(&name, reach);
                }));
            }
            for handle in round {
                let _finished = handle.await;
            }
            tokio::time::sleep(ROUND_INTERVAL).await;
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plain_host_and_port_is_dialled_as_written() {
        assert_eq!(
            dial_target("https://192.168.50.3:6443"),
            Some(("192.168.50.3".into(), 6443)),
        );
    }

    /// **The EKS case, and the reason the default comes from the SCHEME.** A
    /// kubeconfig routinely omits the port on an EKS endpoint; defaulting to
    /// 6443 would dial a port nothing listens on and report a healthy estate
    /// as down.
    #[test]
    fn a_missing_port_defaults_from_the_scheme() {
        assert_eq!(
            dial_target("https://ABC.gr7.us-east-2.eks.amazonaws.com"),
            Some(("ABC.gr7.us-east-2.eks.amazonaws.com".into(), 443)),
        );
        assert_eq!(
            dial_target("http://localhost"),
            Some(("localhost".into(), 80)),
        );
    }

    #[test]
    fn a_path_is_not_part_of_the_address() {
        assert_eq!(
            dial_target("https://k8s.novaskyn.com/some/path"),
            Some(("k8s.novaskyn.com".into(), 443)),
        );
    }

    /// An IPv6 literal's colons are not port separators. Splitting on the last
    /// colon without handling the brackets would dial a host of `[::1` on a
    /// port of `1]`, which parses as nothing and reports the cluster
    /// unprobeable — a real cluster hidden by a parser bug.
    #[test]
    fn an_ipv6_literal_keeps_its_colons() {
        assert_eq!(dial_target("https://[::1]:6443"), Some(("::1".into(), 6443)));
        assert_eq!(dial_target("https://[fd00::5]"), Some(("fd00::5".into(), 443)));
    }

    #[test]
    fn userinfo_is_not_part_of_the_address() {
        assert_eq!(
            dial_target("https://user:pw@host.example:6443"),
            Some(("host.example".into(), 6443)),
        );
    }

    /// What cannot be read is REPORTED as unreadable, never guessed at.
    #[test]
    fn an_unreadable_server_yields_no_target() {
        for server in [
            "",
            "host-with-no-scheme:6443",
            "ftp://host:21",
            "https://",
            "https://host:not-a-port",
            "https://:6443",
        ] {
            assert_eq!(dial_target(server), None, "`{server}` must not be dialled");
        }
    }

    #[test]
    fn an_unreported_context_reads_as_probing() {
        let (ronda, _pub) = channel();
        assert_eq!(ronda.reach("camelot-eks"), Reach::Probing);
        assert!(!Reach::Probing.is_settled());
    }

    #[test]
    fn a_reported_context_reads_back() {
        let (ronda, publisher) = channel();
        publisher.report("rio", Reach::Unreachable);
        publisher.report("camelot-eks", Reach::Open);
        assert_eq!(ronda.reach("rio"), Reach::Unreachable);
        assert_eq!(ronda.reach("camelot-eks"), Reach::Open);
        assert_eq!(ronda.reach("never-mentioned"), Reach::Probing);
    }

    /// A report must not disturb the rest of the round — the copy-on-write
    /// swap rebuilds the whole map, and dropping the other entries would make
    /// every marker flicker back to `probing` as each answer arrived.
    #[test]
    fn a_report_leaves_the_rest_of_the_round_alone() {
        let (ronda, publisher) = channel();
        publisher.seed(&["a".into(), "b".into(), "c".into()]);
        publisher.report("b", Reach::Open);
        assert_eq!(ronda.reach("a"), Reach::Probing);
        assert_eq!(ronda.reach("b"), Reach::Open);
        assert_eq!(ronda.reach("c"), Reach::Probing);
        assert_eq!(ronda.covered(), 3);
    }

    /// **Seeding is load-bearing.** Without it an unstarted round reads as
    /// settled-with-no-findings, and the picker drops to the slow redraw
    /// cadence for its whole first pass.
    #[test]
    fn an_unseeded_round_is_not_mistaken_for_a_finished_one() {
        let (ronda, publisher) = channel();
        assert!(!ronda.all_settled(), "empty is not settled");

        publisher.seed(&["a".into(), "b".into()]);
        assert!(!ronda.all_settled(), "seeded and outstanding");

        publisher.report("a", Reach::Open);
        assert!(!ronda.all_settled(), "one still outstanding");

        publisher.report("b", Reach::Unreachable);
        assert!(ronda.all_settled(), "and now the round is done");
    }

    /// An inert ronda reports `Probing` for everything and settles never — the
    /// fixture path draws no markers rather than inventing optimistic ones.
    #[test]
    fn an_inert_ronda_finds_nothing() {
        let r = Ronda::inert();
        assert_eq!(r.reach("anything"), Reach::Probing);
        assert_eq!(r.covered(), 0);
        assert!(!r.all_settled());
    }

    /// Every variant has a distinct marker. Two states sharing a glyph is a
    /// row that cannot be read.
    #[test]
    fn every_reach_has_its_own_marker() {
        let all = [
            Reach::Probing,
            Reach::Open,
            Reach::Refused,
            Reach::Unreachable,
            Reach::Unprobeable,
        ];
        for (i, a) in all.iter().enumerate() {
            for b in &all[i + 1..] {
                assert_ne!(a.marker(), b.marker(), "{a:?} vs {b:?}");
                assert_ne!(a.label(), b.label(), "{a:?} vs {b:?}");
            }
            assert_eq!(
                a.marker().chars().count(),
                1,
                "{a:?} must be ONE cell — a two-cell marker shifts the column",
            );
        }
    }

    /// **`Open` must not be spelled `Ready` anywhere an operator reads.** A
    /// TCP accept says nothing about credentials, and a label that implied
    /// otherwise would make banken responsible for the surprise.
    #[test]
    fn no_label_claims_more_than_a_tcp_accept_proves() {
        for r in [Reach::Open, Reach::Probing, Reach::Refused] {
            let l = r.label();
            for overclaim in ["ready", "authenticated", "healthy", "connected"] {
                assert!(
                    !l.contains(overclaim),
                    "{r:?} label `{l}` claims `{overclaim}`",
                );
            }
        }
    }

    /// A refused port is a real, distinct answer — the local case that proves
    /// the arm is reachable rather than merely declared.
    #[tokio::test]
    async fn a_closed_local_port_reads_as_refused() {
        // Bind, learn the port, then drop the listener so nothing is behind it.
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("a loopback bind must succeed");
        let port = listener.local_addr().expect("bound").port();
        drop(listener);
        assert_eq!(probe("127.0.0.1", port).await, Reach::Refused);
    }

    /// And an open one reads as open, which is the other half of the same
    /// gate: without it, a probe that reported `Refused` unconditionally would
    /// pass the test above.
    #[tokio::test]
    async fn an_open_local_port_reads_as_open() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("a loopback bind must succeed");
        let port = listener.local_addr().expect("bound").port();
        assert_eq!(probe("127.0.0.1", port).await, Reach::Open);
        drop(listener);
    }

    /// **A context with no probeable address settles as `Unprobeable` rather
    /// than sitting in `probing` forever** — which would leave the picker on
    /// its fast redraw cadence permanently, and leave a row looking like it
    /// was still being worked on.
    #[tokio::test]
    async fn a_context_with_no_server_settles_immediately() {
        let (ronda, publisher) = channel();
        let rounds = spawn_rounds(
            vec![
                ("no-server".into(), None),
                ("bad-url".into(), Some("not-a-url".into())),
            ],
            publisher,
        );
        // Both are answered without dialling anything, so a short yield is
        // enough — no network is involved on either path.
        for _ in 0..50 {
            if ronda.all_settled() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        rounds.abort();
        assert_eq!(ronda.reach("no-server"), Reach::Unprobeable);
        assert_eq!(ronda.reach("bad-url"), Reach::Unprobeable);
        assert!(ronda.all_settled());
    }

    /// The rounds actually reach the network and publish a real verdict —
    /// end-to-end over a loopback listener, so the whole path (spawn, seed,
    /// dial, report) is exercised rather than its pieces.
    #[tokio::test]
    async fn a_round_reports_a_live_address_as_open() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("a loopback bind must succeed");
        let port = listener.local_addr().expect("bound").port();
        let mut server = String::from("http://127.0.0.1:");
        server.push_str(&port.to_string());

        let (ronda, publisher) = channel();
        assert_eq!(ronda.reach("local"), Reach::Probing, "before the round");

        let rounds = spawn_rounds(vec![("local".into(), Some(server))], publisher);
        for _ in 0..200 {
            if ronda.all_settled() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        rounds.abort();
        drop(listener);
        assert_eq!(ronda.reach("local"), Reach::Open);
    }

    /// A name that cannot resolve is `Unreachable`, not a hang and not a
    /// panic. This is the DNS half the timeout has to cover.
    #[tokio::test]
    async fn an_unresolvable_name_is_unreachable_not_a_hang() {
        let started = std::time::Instant::now();
        assert_eq!(
            probe("this-host-does-not-exist.invalid", 6443).await,
            Reach::Unreachable,
        );
        assert!(
            started.elapsed() < PROBE_TIMEOUT + Duration::from_secs(1),
            "and it came back inside our own timeout, not kube's 30s",
        );
    }
}
