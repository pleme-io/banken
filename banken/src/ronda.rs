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

/// How long to wait before walking the cheap rounds again.
///
/// A VPN comes up while the operator is reading the list, and the row should
/// notice. Cheap enough to repeat: a TCP connect to an already-open port is
/// one round trip and no authentication.
pub const ROUND_INTERVAL: Duration = Duration::from_secs(15);

/// How long between climbs of the EXPENSIVE half of the ladder.
///
/// Everything above [`Rung::Network`] costs a credential helper — on an EKS
/// context that is an `aws eks get-token` subprocess and an STS round-trip —
/// so it runs on its own, much slower clock, and only against contexts whose
/// port already opened.
///
/// Five minutes against the four reachable contexts on the operator's own
/// kubeconfig is ~48 helper invocations an hour, where climbing on the cheap
/// clock against all eighteen would be ~4300. That ratio is the entire reason
/// there are two cadences instead of one.
pub const DEEP_INTERVAL: Duration = Duration::from_secs(300);

/// How far banken actually got toward **using** a cluster.
///
/// **Ordered, and the order is the whole point:** each rung strictly implies
/// every rung beneath it. You cannot be authenticated without an apiserver
/// having answered, and one cannot answer without packets getting through. So
/// the ladder is a scalar, and a scalar is something a colour can be a smooth
/// function of.
///
/// This replaced a binary reachable/unreachable, which was not enough and said
/// so in its own doc comment: a TCP accept proves packets move and nothing
/// else, so the honest thing it could report was the floor. The fix is not a
/// better word for the floor — it is to climb, and to light the row by how far
/// the climb got.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub enum Rung {
    /// No round has reported yet, or there is nothing to probe (no server
    /// declared, or a URL with no host banken can extract).
    ///
    /// Outside the ramp entirely — it is the absence of a measurement, not a
    /// low one, and coldest-colour would read as a verdict.
    #[default]
    Unknown,
    /// Nothing answered at the address: a timeout, a dead route, a refused
    /// port, or a name that would not resolve.
    Down,
    /// TCP accepted — packets get through — but the climb stopped there.
    ///
    /// Typically TLS failed, the kubeconfig is broken, or the credential
    /// helper could not produce a token (an expired SSO session lands here).
    Network,
    /// An apiserver answered. It speaks Kubernetes, and it rejected or was
    /// never offered an identity.
    Serving,
    /// The apiserver accepted who you are.
    Identity,
    /// …and you may list pods here — which is the thing banken actually does.
    ///
    /// The top rung is deliberately banken's OWN verb rather than a generic
    /// "authorized": a token that can read nodes but not pods is useless to
    /// this program, and a light that went full green for it would be lying
    /// about the only question the operator is asking.
    Pods,
}

impl Rung {
    /// Every rung, lowest first.
    pub const ALL: [Self; 6] = [
        Self::Unknown,
        Self::Down,
        Self::Network,
        Self::Serving,
        Self::Identity,
        Self::Pods,
    ];

    /// Where this rung sits on the ramp, `0.0`..=`1.0`.
    ///
    /// [`Self::Unknown`] has no position — it is not a low measurement, it is
    /// the absence of one, and giving it `0.0` would paint "banken has not
    /// looked yet" in the same colour as "this cluster is down".
    #[must_use]
    pub fn position(self) -> Option<f32> {
        let i = match self {
            Self::Unknown => return None,
            Self::Down => 0.0,
            Self::Network => 0.25,
            Self::Serving => 0.5,
            Self::Identity => 0.75,
            Self::Pods => 1.0,
        };
        Some(i)
    }

    /// The one-cell marker: a circle that FILLS as the ladder is climbed.
    ///
    /// Colour is never load-bearing alone — a colour-blind operator, a
    /// monochrome terminal and a `TestBackend` frame all have to be able to
    /// read the row, so the glyph carries the same information the ramp does.
    ///
    /// `Unknown` is `◌` (a dotted outline) and not `·`: the middle dot is the
    /// separator the footer already uses between hints, so a `·` marker was
    /// indistinguishable from punctuation both to an operator scanning the
    /// screen and to a test asserting on the frame. Caught by
    /// `without_rounds_no_marker_column_is_drawn`.
    #[must_use]
    pub fn marker(self) -> &'static str {
        match self {
            Self::Unknown => "◌",
            Self::Down => "○",
            Self::Network => "◔",
            Self::Serving => "◑",
            Self::Identity => "◕",
            Self::Pods => "●",
        }
    }

    /// What the marker means, for a legend or a test.
    ///
    /// Every one of these names what was MEASURED. None of them claims more:
    /// `Identity` says the apiserver accepted you, not that you can do
    /// anything, because that is a different request and it is the next rung.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Unknown => "not probed",
            Self::Down => "nothing answered",
            Self::Network => "port open, no apiserver reached",
            Self::Serving => "apiserver answered, identity rejected",
            Self::Identity => "identity accepted",
            Self::Pods => "may list pods",
        }
    }

    /// What reaching this rung means when NOTHING ABOVE IT WAS PROBED.
    ///
    /// The waypoint twin of [`Self::label`]. Every phrase here is careful to
    /// claim only what a probe that stopped here actually established — the
    /// difference is the word "not probed" in place of a negative finding.
    /// `Pods` is the top rung, so there is nothing above it to be unsure
    /// about and the two labels agree.
    #[must_use]
    pub fn reached_label(self) -> &'static str {
        match self {
            Self::Unknown => "not probed",
            Self::Down => "nothing answered",
            Self::Network => "port open, apiserver not probed",
            Self::Serving => "apiserver answered, identity not probed",
            Self::Identity => "identity accepted, pod access not probed",
            Self::Pods => "may list pods",
        }
    }

    /// The stable machine name for this rung.
    ///
    /// # Why this is not [`Self::label`]
    ///
    /// `label` is a *phrase written for a human reading a status line* — "port
    /// open, no apiserver reached" — and it is free to be reworded for clarity
    /// at any time. A machine consumer keying off that string would break on a
    /// copy edit, so it gets a short identifier that is part of the contract
    /// and does not move.
    ///
    /// Both are emitted by `banken mcp`'s readiness tool: the token to branch
    /// on, the label to quote. Exhaustively matched, so a new rung must decide
    /// its own token rather than inherit a plausible one.
    #[must_use]
    pub fn token(self) -> &'static str {
        match self {
            Self::Unknown => "unknown",
            Self::Down => "down",
            Self::Network => "network",
            Self::Serving => "serving",
            Self::Identity => "identity",
            Self::Pods => "pods",
        }
    }

    /// Whether a round has settled on this rung.
    #[must_use]
    pub fn is_settled(self) -> bool {
        self != Self::Unknown
    }

    /// Whether it is worth spending a credential helper on this context.
    ///
    /// The deep climb is the expensive half — on an EKS estate it spawns
    /// `aws eks get-token` — so it is spent only where the cheap half already
    /// proved packets move. On the operator's own kubeconfig that is 4
    /// contexts of 18, i.e. the guard removes about three quarters of the
    /// cost for free.
    #[must_use]
    pub fn worth_climbing(self) -> bool {
        self >= Self::Network
    }
}

/// A rung plus why the climb stopped there.
///
/// The note is what turns a marker into a diagnosis: `Network` alone says
/// "stopped early", `Network` + "credentials: SSO session expired" says what
/// to go and do. Carried per context rather than derived from the rung,
/// because the same rung is reached for genuinely different reasons.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Standing {
    /// How far the climb got.
    pub rung: Rung,
    /// Whether the climb STOPPED at `rung` or merely REACHED it.
    ///
    /// Load-bearing, and it is why this field exists rather than being
    /// inferred from `note.is_empty()`: the two are different findings and
    /// [`Rung::label`] renders them identically. A TCP-only [`probe`] returns
    /// `at(Network)` — port open, nothing above it was asked — while the full
    /// [`crate::live::Access::climb`] returns `stopped(Network, …)` when the
    /// apiserver genuinely did not answer. Reading the second phrase over the
    /// first is a FALSE NEGATIVE that sends an operator to debug a healthy
    /// cluster.
    ///
    /// Measured 2026-08-28 against a local single-node cluster: the same MCP
    /// server reported the context as "port open, no apiserver reached" from
    /// `banken_readiness` while `banken_list --kind node` listed that
    /// cluster's node in the very next call. Two tools, one server, opposite
    /// answers — because only one of them could see this distinction.
    reach: Reach,
    /// Why it stopped, in the operator's terms. Empty when there is nothing
    /// to add — the top rung needs no excuse.
    pub note: String,
}

/// Did the climb stop at a rung, or merely reach it?
///
/// A closed two-arm enum rather than a `bool`, so a call site reads
/// `Reach::Waypoint` instead of `true` and a third case (if one is ever
/// measured) is an added arm rather than a second flag.
/// `Default` is `Waypoint`, deliberately. A default-constructed `Standing` has
/// measured nothing, and `Ceiling` asserts a negative finding — defaulting to
/// it would make every un-probed context claim its apiserver was unreachable,
/// which is the exact round-up this enum was introduced to stop.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Reach {
    /// The climb was ATTEMPTED above this rung and got no further. The rung's
    /// negative finding is real and measured.
    Ceiling,
    /// This rung was reached and nothing above it was asked. Says NOTHING
    /// about the rungs above — absence of measurement, not measured absence.
    #[default]
    Waypoint,
}

impl Standing {
    /// A standing at `rung` with no further explanation.
    ///
    /// A WAYPOINT: nothing above `rung` was probed, so nothing above it is
    /// claimed either way.
    #[must_use]
    pub fn at(rung: Rung) -> Self {
        Self {
            rung,
            note: String::new(),
            reach: Reach::Waypoint,
        }
    }

    /// How far the climb got, and whether that was a ceiling or a waypoint.
    #[must_use]
    pub const fn reach(&self) -> Reach {
        self.reach
    }

    /// What this standing MEANS — the phrase to show a human or an agent.
    ///
    /// ★ Use this, never `standing.rung.label()`. `label` is a method on
    /// `Rung` alone and therefore CANNOT distinguish "we did not ask" from
    /// "we asked and got nothing"; for every rung below the top that
    /// difference inverts the finding.
    #[must_use]
    pub fn meaning(&self) -> &'static str {
        match self.reach {
            Reach::Ceiling => self.rung.label(),
            Reach::Waypoint => self.rung.reached_label(),
        }
    }

    /// A standing at `rung`, with the reason it stopped there.
    #[must_use]
    pub fn stopped(rung: Rung, note: impl Into<String>) -> Self {
        Self {
            reach: Reach::Ceiling,
            rung,
            note: note.into(),
        }
    }
}

/// The colour for a position on the ramp, `0.0`..=`1.0`, in the fleet theme.
///
/// A genuine gradient rather than a set of presets: the anchors are
/// interpolated, so a light part-way between two rungs — which is what an
/// eased transition draws — lands on a colour part-way between theirs. That is
/// what makes the change *smooth* rather than a jump.
///
/// The walk is red → amber → green, which is the one ramp an operator does not
/// have to be taught: the same one every signal strength and every fuel gauge
/// already uses.
///
/// **The anchors come from [`crate::palette`], not from this file.** They were
/// five hardcoded triples — five magic numbers no fleet edit could reach — and
/// are now the fleet theme's own error / warning / success, so one `ishou`
/// edit moves banken's ladder along with every other fleet surface. The two
/// intermediate rungs (`network`, `identity`) are interpolations rather than
/// separately-tuned constants, which is why three anchors replaced five.
#[must_use]
pub fn ramp(position: f32) -> (u8, u8, u8) {
    ramp_in(position, crate::palette::Palette::fleet())
}

/// [`ramp`] against an explicit palette — the seam a test uses to prove the
/// ramp follows the theme rather than merely reading from something named
/// after it.
#[must_use]
pub fn ramp_in(position: f32, palette: &crate::palette::Palette) -> (u8, u8, u8) {
    let stops = palette.ramp_stops();

    // NaN first, and not by `clamp`: `f32::clamp` PROPAGATES NaN rather than
    // pinning it, so a NaN position flowed all the way through the lerp and
    // `as u8` turned it into 0 — a BLACK cell, the one colour that is not on
    // the ramp at all and reads as a rendering fault. Caught by
    // `the_ramp_clamps_rather_than_wrapping`.
    let at = if position.is_finite() {
        position.clamp(0.0, 1.0)
    } else {
        0.0
    };
    let mut lo = stops[0];
    let mut hi = stops[stops.len() - 1];
    for pair in stops.windows(2) {
        if at >= pair[0].0 && at <= pair[1].0 {
            lo = pair[0];
            hi = pair[1];
            break;
        }
    }
    let span = hi.0 - lo.0;
    // Guard the degenerate span rather than dividing by it: two anchors at the
    // same position would otherwise produce a NaN channel and a black cell.
    let frac = if span <= f32::EPSILON {
        0.0
    } else {
        (at - lo.0) / span
    };
    let lerp = |from: u8, to: u8| {
        let from = f32::from(from);
        let to = f32::from(to);
        // `clamp` before the cast, so the value is already in `u8`'s range and
        // the conversion cannot lose a sign or truncate. `as` is the only cast
        // available from `f32` (there is no `TryFrom<f32> for u8`), which is
        // why the clamp does the work the type system cannot.
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "clamped to 0.0..=255.0 on the line above; f32 has no TryFrom<u8>"
        )]
        let channel = (from + (to - from) * frac).clamp(0.0, 255.0) as u8;
        channel
    };
    (
        lerp(lo.1.0, hi.1.0),
        lerp(lo.1.1, hi.1.1),
        lerp(lo.1.2, hi.1.2),
    )
}

/// One round's findings, keyed by context name.
///
/// Immutable and published whole, so a frame can never draw half of a round —
/// the same reason [`crate::absorb`]'s snapshot is immutable.
pub type Findings = BTreeMap<String, Standing>;

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

    /// This context's standing, or [`Rung::Unknown`] when no round has
    /// reported it yet.
    #[must_use]
    pub fn standing(&self, context: &str) -> Standing {
        self.findings
            .load()
            .get(context)
            .cloned()
            .unwrap_or_default()
    }

    /// How far the climb got at this context — the scalar the light is a
    /// function of.
    #[must_use]
    pub fn rung(&self, context: &str) -> Rung {
        self.findings
            .load()
            .get(context)
            .map_or(Rung::Unknown, |s| s.rung)
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
        !f.is_empty() && f.values().all(|s| s.rung.is_settled())
    }

    /// How many contexts a round is covering (for a test and for a legend).
    #[must_use]
    pub fn covered(&self) -> usize {
        self.findings.load().len()
    }

    /// Where every MEASURED context sits on the ramp.
    ///
    /// Unmeasured contexts are absent rather than present at `0.0`, which is
    /// what lets a drawer tell "banken has not looked" apart from "banken
    /// looked and there is nothing there" — the two would otherwise be the
    /// same deep red.
    #[must_use]
    pub fn positions(&self) -> Vec<(String, f32)> {
        self.findings
            .load()
            .iter()
            .filter_map(|(name, s)| s.rung.position().map(|p| (name.clone(), p)))
            .collect()
    }

    /// Every context a round covers, **including the ones not yet measured**.
    ///
    /// # Why this is not [`Self::positions`]
    ///
    /// `positions` is a *ramp* projection: it drops unmeasured contexts on
    /// purpose so a drawer cannot paint "not looked at yet" as a colour that
    /// means something. That is exactly wrong for a reader who wants a list of
    /// clusters, because it makes an unprobed context **invisible** rather
    /// than visibly unknown.
    ///
    /// Measured 2026-08-12, which is why this exists: `banken mcp` reported
    /// `covered: 18` alongside an empty context array, because all eighteen
    /// were still `Rung::Unknown` at the moment of the read. `covered` is a
    /// count and `positions` is a filtered list, so the two disagreed and
    /// neither was wrong — there was simply no method that answered "what are
    /// the eighteen".
    #[must_use]
    pub fn standings(&self) -> Vec<(String, Standing)> {
        self.findings
            .load()
            .iter()
            .map(|(name, s)| (name.clone(), s.clone()))
            .collect()
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
    pub fn report(&self, context: &str, standing: Standing) {
        let mut next = Findings::clone(&self.findings.load());
        next.insert(context.to_owned(), standing);
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
            next.insert(c.clone(), Standing::at(Rung::Unknown));
        }
        self.findings.store(Arc::new(next));
    }

    /// What is currently recorded for `context` — how the deep round decides
    /// whether a context is worth a credential helper.
    #[must_use]
    pub fn current(&self, context: &str) -> Standing {
        self.findings
            .load()
            .get(context)
            .cloned()
            .unwrap_or_default()
    }

    /// Report a rung the cheap round established as a **floor**, without
    /// lowering one the deep round already climbed past.
    ///
    /// # The clobber this exists to stop
    ///
    /// The cheap round runs every [`ROUND_INTERVAL`] and can only ever prove
    /// `Network`. The deep round runs every [`DEEP_INTERVAL`] and is the only
    /// thing that can prove anything above it. With a plain `report`, the
    /// cheap round therefore **erased the deep round's finding fifteen seconds
    /// after it landed** — a row would climb to green, sit there briefly, and
    /// silently fall back to orange with its diagnosis gone, on a loop.
    /// Measured against a live EKS context: the standing line showed the TCP result
    /// with an empty note where the climb's `SSO token expired` had been.
    ///
    /// `Down` is the exception and overwrites unconditionally: if packets have
    /// stopped getting through, every rung above it is void, and keeping a
    /// stale `Pods` would be the one lie this whole plane exists to avoid.
    pub fn report_floor(&self, context: &str, standing: Standing) {
        let existing = self.current(context);
        if standing.rung != Rung::Down && existing.rung > standing.rung {
            return;
        }
        self.report(context, standing);
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
/// omits it on an EKS endpoint (`https://EXAMPLE0.gr7.us-east-2.eks.amazonaws.com`)
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
pub async fn probe(host: &str, port: u16) -> Standing {
    let mut target = String::from(host);
    target.push(':');
    target.push_str(&port.to_string());

    match tokio::time::timeout(PROBE_TIMEOUT, tokio::net::TcpStream::connect(&target)).await {
        // The connection is dropped immediately. banken wanted the answer,
        // not the socket — holding it open would leave eighteen idle
        // connections against production apiservers every fifteen seconds.
        Ok(Ok(_stream)) => Standing::at(Rung::Network),
        // Refused is a genuinely different diagnosis from unreachable — the
        // host is up and nothing is listening (a stopped k3s, a rebooting
        // node) rather than the path being down (VPN, route, asleep). Same
        // rung, because neither got anywhere; different note, because they
        // send the operator to different things to fix.
        Ok(Err(e)) if e.kind() == std::io::ErrorKind::ConnectionRefused => Standing::stopped(
            Rung::Down,
            "connection refused — host up, nothing listening",
        ),
        Ok(Err(_)) => Standing::stopped(Rung::Down, "no route to the apiserver"),
        Err(_timeout) => Standing::stopped(Rung::Down, "timed out — VPN down, or asleep"),
    }
}

/// The expensive half of the climb: everything above [`Rung::Network`].
///
/// Injected rather than called directly so that [`spawn_rounds`] holds no
/// opinion about kube — which is what lets the orchestration be tested with a
/// mock climber and no cluster, and keeps the one place that spawns credential
/// helpers in [`crate::live`], where the rest of the kube surface lives.
pub type DeepClimb = Arc<
    dyn Fn(String) -> std::pin::Pin<Box<dyn std::future::Future<Output = Standing> + Send>>
        + Send
        + Sync,
>;

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
#[must_use = "dropping the handle cancels the rounds — bind it for the session's lifetime"]
pub fn spawn_rounds(
    targets: Vec<(String, Option<String>)>,
    publisher: RondaPublisher,
    climb: Option<DeepClimb>,
) -> tokio::task::JoinHandle<()> {
    spawn_rounds_at(targets, publisher, climb, Cadence::default())
}

/// How often each half of the ladder is walked.
///
/// Two fields rather than one because the costs differ by two orders of
/// magnitude — see [`DEEP_INTERVAL`]. Authored as `:ronda-round-ms` /
/// `:ronda-climb-ms`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cadence {
    /// Between cheap TCP rounds.
    pub round: Duration,
    /// Between credential climbs.
    pub climb: Duration,
}

impl Default for Cadence {
    fn default() -> Self {
        Self {
            round: ROUND_INTERVAL,
            climb: DEEP_INTERVAL,
        }
    }
}

/// [`spawn_rounds`] at an explicit cadence.
///
/// # Errors
///
/// None — a context with no dialable address is reported as
/// [`Rung::Down`] rather than failing the round, because one unreachable
/// context must not stop the other seventeen from being measured.
#[must_use = "dropping the handle cancels the rounds — bind it for the session's lifetime"]
pub fn spawn_rounds_at(
    targets: Vec<(String, Option<String>)>,
    publisher: RondaPublisher,
    climb: Option<DeepClimb>,
    cadence: Cadence,
) -> tokio::task::JoinHandle<()> {
    // A zero cheap-round would spin the loop as fast as the executor allows —
    // a busy loop dressed as a config value. Clamped to the prescribed
    // interval rather than refused, because a watchdog is not worth failing a
    // session over, and the floor is the value the operator would have got.
    let cadence = Cadence {
        round: if cadence.round.is_zero() {
            ROUND_INTERVAL
        } else {
            cadence.round
        },
        climb: if cadence.climb.is_zero() {
            DEEP_INTERVAL
        } else {
            cadence.climb
        },
    };
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
                publisher.report(
                    name,
                    Standing::stopped(Rung::Down, "no apiserver declared for this context"),
                );
            }
        }

        // The two cadences are the whole cost story. The cheap half runs
        // often; the expensive half runs rarely, and only where the cheap half
        // already proved packets move.
        let mut since_deep = cadence.climb; // …so the first pass climbs.
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
                    let standing = probe(&host, port).await;
                    // A FLOOR, not a verdict — see `report_floor`. The cheap
                    // round can only ever prove `Network`, so reporting it
                    // plainly would erase the deep round's finding on every
                    // fifteen-second tick.
                    publisher.report_floor(&name, standing);
                }));
            }
            for handle in round {
                let _finished = handle.await;
            }

            if let Some(climb) = climb.clone()
                && since_deep >= cadence.climb
            {
                since_deep = Duration::ZERO;
                let mut deep = Vec::new();
                for (name, _) in &dials {
                    // The guard that makes this affordable. Climbing a context
                    // whose port did not even open would spend a credential
                    // helper to discover what the TCP probe already said.
                    if !publisher.current(name).rung.worth_climbing() {
                        continue;
                    }
                    let name = name.clone();
                    let publisher = publisher.clone();
                    let climb = climb.clone();
                    deep.push(tokio::spawn(async move {
                        let standing = climb(name.clone()).await;
                        publisher.report(&name, standing);
                    }));
                }
                for handle in deep {
                    let _finished = handle.await;
                }
            }

            tokio::time::sleep(cadence.round).await;
            since_deep = since_deep.saturating_add(cadence.round);
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plain_host_and_port_is_dialled_as_written() {
        assert_eq!(
            dial_target("https://192.0.2.3:6443"),
            Some(("192.0.2.3".into(), 6443)),
        );
    }

    /// **The EKS case, and the reason the default comes from the SCHEME.** A
    /// kubeconfig routinely omits the port on an EKS endpoint; defaulting to
    /// 6443 would dial a port nothing listens on and report a healthy estate
    /// as down.
    #[test]
    fn a_missing_port_defaults_from_the_scheme() {
        assert_eq!(
            dial_target("https://EXAMPLE0.gr7.us-east-2.eks.amazonaws.com"),
            Some(("EXAMPLE0.gr7.us-east-2.eks.amazonaws.com".into(), 443)),
        );
        assert_eq!(
            dial_target("http://localhost"),
            Some(("localhost".into(), 80)),
        );
    }

    #[test]
    fn a_path_is_not_part_of_the_address() {
        assert_eq!(
            dial_target("https://k8s.example.invalid/some/path"),
            Some(("k8s.example.invalid".into(), 443)),
        );
    }

    /// An IPv6 literal's colons are not port separators. Splitting on the last
    /// colon without handling the brackets would dial a host of `[::1` on a
    /// port of `1]`, which parses as nothing and reports the cluster
    /// unprobeable — a real cluster hidden by a parser bug.
    #[test]
    fn an_ipv6_literal_keeps_its_colons() {
        assert_eq!(
            dial_target("https://[::1]:6443"),
            Some(("::1".into(), 6443))
        );
        assert_eq!(
            dial_target("https://[fd00::5]"),
            Some(("fd00::5".into(), 443))
        );
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

    // ── the ladder ───────────────────────────────────────────────────────

    /// **`Unknown` is the absence of a measurement, not a low one.** This is
    /// the distinction the whole light rests on: painting "banken has not
    /// looked yet" at the bottom of the ramp would say "this cluster is down"
    /// about a cluster nobody has asked about.
    #[test]
    fn unknown_is_not_the_bottom_of_the_ramp() {
        assert_eq!(Rung::Unknown.position(), None);
        assert_eq!(Rung::Down.position(), Some(0.0));
        assert!(!Rung::Unknown.is_settled());
        assert!(Rung::Down.is_settled(), "down IS an answer");
    }

    /// The ladder is ordered, and the order is what makes it a scalar a colour
    /// can be a function of.
    /// A WAYPOINT must never read as a measured negative.
    ///
    /// The defect this pins: `Rung::label` renders `Network` as "port open, no
    /// apiserver reached" — a negative finding — and the TCP-only `probe`
    /// returns exactly that rung without ever asking about an apiserver. Read
    /// through `label`, a healthy cluster whose port was merely reached is
    /// reported as broken, which is what happened to a live single-node
    /// cluster on 2026-08-28 while a sibling tool on the same server listed
    /// its node.
    #[test]
    fn a_waypoint_does_not_claim_the_rungs_above_it() {
        let waypoint = Standing::at(Rung::Network);
        assert_eq!(waypoint.reach(), Reach::Waypoint);
        assert_eq!(waypoint.meaning(), "port open, apiserver not probed");
        assert!(
            !waypoint.meaning().contains("no apiserver"),
            "a waypoint must not assert a negative it never measured; got {:?}",
            waypoint.meaning()
        );

        // The same rung reached as a CEILING keeps the negative, because there
        // it was actually measured.
        let ceiling = Standing::stopped(Rung::Network, "no apiserver answer: timed out");
        assert_eq!(ceiling.reach(), Reach::Ceiling);
        assert_eq!(ceiling.meaning(), "port open, no apiserver reached");

        // Anti-vacuity: the two must differ, or `meaning` is a constant and
        // proves nothing.
        assert_ne!(waypoint.meaning(), ceiling.meaning());
    }

    /// An unmeasured standing defaults to claiming nothing.
    #[test]
    fn default_reach_is_waypoint_not_ceiling() {
        assert_eq!(Reach::default(), Reach::Waypoint);
        assert!(
            !Standing::default().meaning().contains("no apiserver"),
            "a default-constructed Standing has measured nothing and must not              report a negative finding"
        );
    }

    #[test]
    fn the_ladder_is_ordered_lowest_first() {
        let mut sorted = Rung::ALL;
        sorted.sort_unstable();
        assert_eq!(sorted, Rung::ALL);
        assert!(Rung::Down < Rung::Network);
        assert!(Rung::Network < Rung::Serving);
        assert!(Rung::Serving < Rung::Identity);
        assert!(Rung::Identity < Rung::Pods);
    }

    /// Positions rise with the rung and stay inside the ramp.
    #[test]
    fn positions_rise_with_the_rung() {
        let mut last = -1.0_f32;
        for rung in Rung::ALL {
            let Some(p) = rung.position() else { continue };
            assert!((0.0..=1.0).contains(&p), "{rung:?} at {p}");
            assert!(p > last, "{rung:?} must be above the rung below it");
            last = p;
        }
    }

    /// **The ramp is CONTINUOUS** — the property that makes the light change
    /// smoothly rather than snap between five presets. A small step along the
    /// ramp must never produce a large jump in colour.
    #[test]
    fn the_ramp_is_continuous() {
        let mut prev = ramp(0.0);
        let mut p = 0.0_f32;
        while p <= 1.0 {
            let c = ramp(p);
            // Straight `u8::abs_diff` — the previous form round-tripped through
            // `i32`/`unsigned_abs`/`as i32` for no reason, and the casts were
            // the only thing clippy had to complain about.
            let jump = u32::from(c.0.abs_diff(prev.0))
                + u32::from(c.1.abs_diff(prev.1))
                + u32::from(c.2.abs_diff(prev.2));
            assert!(
                jump <= 12,
                "a 1% step at {p} jumped {jump} — the ramp is not smooth",
            );
            prev = c;
            p += 0.01;
        }
    }

    /// The ends of the ramp are the ends of the ladder, and they are the
    /// colours an operator already knows: red is nothing, green is arrived.
    #[test]
    fn the_ramp_runs_red_to_green() {
        let down = ramp(0.0);
        let pods = ramp(1.0);
        assert!(down.0 > down.1, "the bottom is red-dominant: {down:?}");
        assert!(pods.1 > pods.0, "the top is green-dominant: {pods:?}");
    }

    /// **The convergence guard.** The ramp must be a projection of the fleet
    /// theme, not a copy of it that happens to agree today. Two different
    /// themes must produce two different ramps — if they did not, `palette`
    /// would be an elaborate way to keep the five magic numbers it replaced.
    ///
    /// Asserted at the RAMP rather than only at the palette, because that is
    /// where the wiring can break: `ramp` could read its anchors from anywhere
    /// and every palette test would still pass.
    #[test]
    fn the_ramp_follows_the_fleet_theme_rather_than_a_frozen_copy() {
        use crate::palette::Palette;
        let bare = Palette::for_theme(ishou_tokens::FleetTheme::Bare);
        let vellum = Palette::for_theme(ishou_tokens::FleetTheme::Vellum);

        let differs = [0.0_f32, 0.25, 0.5, 0.75, 1.0]
            .iter()
            .any(|&p| ramp_in(p, &bare) != ramp_in(p, &vellum));
        assert!(
            differs,
            "every position drew the same colour under two different themes — \
             the ramp is not reading the theme",
        );
        // And the free function IS the fleet-theme one, so the default path is
        // the converged path rather than a third behaviour.
        let fleet = Palette::fleet();
        for p in [0.0_f32, 0.25, 0.5, 0.75, 1.0] {
            assert_eq!(ramp(p), ramp_in(p, fleet), "at {p}");
        }
    }

    /// The intermediate rungs are INTERPOLATIONS now, not authored constants —
    /// which is the claim that let five anchors become three. Each must sit
    /// strictly between its neighbours on at least one channel, or the ramp
    /// has a flat segment where an operator expects a step.
    #[test]
    fn the_intermediate_rungs_are_genuine_interpolations() {
        for pair in Rung::ALL.windows(2) {
            let (Some(a), Some(b)) = (pair[0].position(), pair[1].position()) else {
                continue; // Unknown has no position — it is off the ramp.
            };
            assert_ne!(
                ramp(a),
                ramp(b),
                "{:?} and {:?} draw the same colour — a flat segment",
                pair[0],
                pair[1],
            );
        }
    }

    /// Out-of-range positions clamp rather than wrap or produce a black cell.
    #[test]
    fn the_ramp_clamps_rather_than_wrapping() {
        assert_eq!(ramp(-5.0), ramp(0.0));
        assert_eq!(ramp(5.0), ramp(1.0));
        assert_eq!(ramp(f32::NAN), ramp(0.0), "NaN must not paint a black cell");
    }

    /// The machine token is a CONTRACT: distinct per rung, and stable in a way
    /// the human label deliberately is not. A consumer branches on it, so two
    /// rungs sharing one would silently collapse two different answers — the
    /// worst of which is `unknown` reading as `down`.
    #[test]
    fn every_rung_has_its_own_stable_token() {
        for (i, a) in Rung::ALL.iter().enumerate() {
            for b in &Rung::ALL[i + 1..] {
                assert_ne!(
                    a.token(),
                    b.token(),
                    "{a:?} and {b:?} share the token `{}` — a consumer cannot \
                     tell them apart",
                    a.token(),
                );
            }
        }
        // Pinned, not merely unique. These strings are published on banken's
        // MCP surface; renaming one is a breaking change to a consumer, and
        // this is what makes that visible in a diff rather than at their end.
        assert_eq!(Rung::Unknown.token(), "unknown");
        assert_eq!(Rung::Down.token(), "down");
        assert_eq!(Rung::Network.token(), "network");
        assert_eq!(Rung::Serving.token(), "serving");
        assert_eq!(Rung::Identity.token(), "identity");
        assert_eq!(Rung::Pods.token(), "pods");
    }

    /// `standings()` reports EVERY covered context; `positions()` reports only
    /// the measured ones. Both are correct for their own reader, and the two
    /// disagreeing is the point — a drawer must not paint "not looked at yet",
    /// and a list must not omit it.
    #[test]
    fn standings_lists_the_unmeasured_contexts_that_positions_drops() {
        let (ronda, publisher) = channel();
        publisher.seed(&["alpha".to_owned(), "bravo".to_owned()]);

        assert_eq!(ronda.covered(), 2);
        assert!(
            ronda.positions().is_empty(),
            "the ramp projection drops unmeasured contexts, by design",
        );
        let all = ronda.standings();
        assert_eq!(all.len(), 2, "the list must not: {all:?}");
        assert!(all.iter().all(|(_, s)| s.rung == Rung::Unknown));

        // Once one is measured, it appears in BOTH — the split is about
        // measurement, not about which method is authoritative.
        publisher.report("alpha", Standing::at(Rung::Pods));
        assert_eq!(ronda.positions().len(), 1);
        assert_eq!(ronda.standings().len(), 2);
    }

    /// Every rung is distinguishable WITHOUT colour — a colour-blind operator,
    /// a monochrome terminal and a `TestBackend` frame all have to read it.
    #[test]
    fn every_rung_has_its_own_one_cell_marker() {
        for (i, a) in Rung::ALL.iter().enumerate() {
            for b in &Rung::ALL[i + 1..] {
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

    /// **No label may claim more than its rung measured.** `Identity` means
    /// the apiserver accepted who you are — NOT that you can do anything, which
    /// is a different request and is the rung above.
    #[test]
    fn no_label_claims_more_than_its_rung_measured() {
        for (rung, forbidden) in [
            (Rung::Down, &["ready", "reachable"][..]),
            (Rung::Network, &["authenticated", "ready", "healthy"][..]),
            (Rung::Serving, &["authenticated", "authorized"][..]),
            (Rung::Identity, &["authorized", "may list"][..]),
        ] {
            let l = rung.label();
            for claim in forbidden {
                assert!(!l.contains(claim), "{rung:?} label `{l}` claims `{claim}`");
            }
        }
    }

    /// The guard that makes the expensive half affordable: a context whose
    /// port never opened is not worth a credential helper.
    #[test]
    fn only_network_up_contexts_are_worth_climbing() {
        assert!(!Rung::Unknown.worth_climbing());
        assert!(!Rung::Down.worth_climbing());
        assert!(Rung::Network.worth_climbing());
        assert!(Rung::Pods.worth_climbing());
    }

    // ── the findings store ───────────────────────────────────────────────

    #[test]
    fn an_unreported_context_is_unknown() {
        let (ronda, _pub) = channel();
        assert_eq!(ronda.rung("alpha-eks"), Rung::Unknown);
        assert_eq!(ronda.standing("alpha-eks"), Standing::at(Rung::Unknown));
    }

    #[test]
    fn a_reported_context_reads_back_with_its_note() {
        let (ronda, publisher) = channel();
        publisher.report("bravo", Standing::at(Rung::Pods));
        publisher.report(
            "charlie",
            Standing::stopped(Rung::Network, "credentials: SSO session expired"),
        );
        assert_eq!(ronda.rung("bravo"), Rung::Pods);
        assert_eq!(ronda.rung("charlie"), Rung::Network);
        assert_eq!(
            ronda.standing("charlie").note,
            "credentials: SSO session expired"
        );
        assert_eq!(ronda.rung("never-mentioned"), Rung::Unknown);
    }

    /// A report must not disturb the rest of the round — the copy-on-write
    /// swap rebuilds the whole map, and dropping the other entries would make
    /// every light fall back to grey as each answer arrived.
    #[test]
    fn a_report_leaves_the_rest_of_the_round_alone() {
        let (ronda, publisher) = channel();
        publisher.seed(&["a".into(), "b".into(), "c".into()]);
        publisher.report("b", Standing::at(Rung::Pods));
        assert_eq!(ronda.rung("a"), Rung::Unknown);
        assert_eq!(ronda.rung("b"), Rung::Pods);
        assert_eq!(ronda.rung("c"), Rung::Unknown);
        assert_eq!(ronda.covered(), 3);
    }

    /// Only MEASURED contexts have a place on the ramp — the drawer needs the
    /// unmeasured ones absent, not present at zero.
    #[test]
    fn positions_omit_the_unmeasured() {
        let (ronda, publisher) = channel();
        publisher.seed(&["a".into(), "b".into()]);
        publisher.report("b", Standing::at(Rung::Serving));
        let positions = ronda.positions();
        assert_eq!(positions.len(), 1, "only `b` has been measured");
        assert_eq!(positions[0].0, "b");
        assert!((positions[0].1 - 0.5).abs() < f32::EPSILON);
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

        publisher.report("a", Standing::at(Rung::Pods));
        assert!(!ronda.all_settled(), "one still outstanding");

        publisher.report("b", Standing::at(Rung::Down));
        assert!(ronda.all_settled(), "and now the round is done");
    }

    #[test]
    fn an_inert_ronda_finds_nothing() {
        let r = Ronda::inert();
        assert_eq!(r.rung("anything"), Rung::Unknown);
        assert_eq!(r.covered(), 0);
        assert!(r.positions().is_empty());
        assert!(!r.all_settled());
    }

    // ── the cheap probe ──────────────────────────────────────────────────

    /// A refused port is a real, distinct diagnosis — same rung as a timeout
    /// (neither got anywhere) but a different note, because they send the
    /// operator to different things to fix.
    #[tokio::test]
    async fn a_closed_local_port_is_down_and_says_refused() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("a loopback bind must succeed");
        let port = listener.local_addr().expect("bound").port();
        drop(listener);
        let s = probe("127.0.0.1", port).await;
        assert_eq!(s.rung, Rung::Down);
        assert!(s.note.contains("refused"), "{}", s.note);
    }

    /// And an open one reaches `Network` — the other half of the same gate:
    /// without it a probe that reported `Down` unconditionally would pass the
    /// test above.
    #[tokio::test]
    async fn an_open_local_port_reaches_network() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("a loopback bind must succeed");
        let port = listener.local_addr().expect("bound").port();
        assert_eq!(probe("127.0.0.1", port).await.rung, Rung::Network);
        drop(listener);
    }

    /// A name that cannot resolve is `Down`, not a hang and not a panic — the
    /// DNS half the timeout has to cover.
    #[tokio::test]
    async fn an_unresolvable_name_is_down_not_a_hang() {
        let started = std::time::Instant::now();
        assert_eq!(
            probe("this-host-does-not-exist.invalid", 6443).await.rung,
            Rung::Down,
        );
        assert!(
            started.elapsed() < PROBE_TIMEOUT + Duration::from_secs(1),
            "and it came back inside our own timeout, not kube's 30s",
        );
    }

    // ── the rounds ───────────────────────────────────────────────────────

    /// A context with no probeable address settles at `Down` rather than
    /// sitting in `Unknown` forever — which would leave the picker on its fast
    /// redraw cadence permanently.
    #[tokio::test]
    async fn a_context_with_no_server_settles_immediately() {
        let (ronda, publisher) = channel();
        let rounds = spawn_rounds(
            vec![
                ("no-server".into(), None),
                ("bad-url".into(), Some("not-a-url".into())),
            ],
            publisher,
            None,
        );
        for _ in 0..100 {
            if ronda.all_settled() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        rounds.abort();
        assert_eq!(ronda.rung("no-server"), Rung::Down);
        assert!(ronda.standing("no-server").note.contains("no apiserver"));
    }

    /// **THE COST GATE.** The deep climb spawns a credential helper, so it must
    /// run ONLY against contexts whose port already opened. Without this the
    /// eighteen-context kubeconfig would spend fourteen `aws eks get-token`
    /// invocations per round to rediscover what the TCP probe already said.
    #[tokio::test]
    async fn the_deep_climb_skips_contexts_that_never_opened() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("a loopback bind must succeed");
        let port = listener.local_addr().expect("bound").port();
        let mut up = String::from("http://127.0.0.1:");
        up.push_str(&port.to_string());

        let climbed: Arc<std::sync::Mutex<Vec<String>>> =
            Arc::new(std::sync::Mutex::new(Vec::new()));
        let seen = Arc::clone(&climbed);
        let climb: DeepClimb = Arc::new(move |context: String| {
            let seen = Arc::clone(&seen);
            Box::pin(async move {
                seen.lock().expect("not poisoned").push(context);
                Standing::at(Rung::Pods)
            })
        });

        let (ronda, publisher) = channel();
        let rounds = spawn_rounds(
            vec![
                ("up".into(), Some(up)),
                ("down".into(), Some("https://127.0.0.1:1".into())),
                ("no-server".into(), None),
            ],
            publisher,
            Some(climb),
        );
        for _ in 0..300 {
            if ronda.rung("up") == Rung::Pods {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        rounds.abort();
        drop(listener);

        let climbed = climbed.lock().expect("not poisoned").clone();
        assert_eq!(
            climbed,
            vec!["up".to_owned()],
            "only the context whose port opened may cost a credential helper",
        );
        assert_eq!(
            ronda.rung("up"),
            Rung::Pods,
            "and it climbed the full ladder"
        );
        assert_eq!(ronda.rung("down"), Rung::Down);
    }

    /// Without a climber the rounds still run — the cheap half stands alone,
    /// which is what `--no-default-features` and a test both need.
    #[tokio::test]
    async fn the_rounds_run_without_a_climber() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("a loopback bind must succeed");
        let port = listener.local_addr().expect("bound").port();
        let mut up = String::from("http://127.0.0.1:");
        up.push_str(&port.to_string());

        let (ronda, publisher) = channel();
        let rounds = spawn_rounds(vec![("up".into(), Some(up))], publisher, None);
        for _ in 0..300 {
            if ronda.all_settled() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        rounds.abort();
        drop(listener);
        assert_eq!(ronda.rung("up"), Rung::Network, "the floor, and no further");
    }
}
