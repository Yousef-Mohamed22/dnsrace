//! The measurements: the quick scan, the full test, DNSSEC checks and the "where does this site
//! go" steering check.
//!
//! The transport is chosen by [`Protocol`]. Plain candidates use UDP or TCP; DoT/DoH candidates
//! carry their own endpoint. All measurements return structured rows so the desktop interface
//! can filter, sort and export them without parsing text.

use std::collections::{BTreeMap, HashSet};
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tokio::net::TcpStream;
use tokio::sync::Semaphore;
use tokio::task::JoinSet;
use tokio::time::timeout;

use crate::dns::{
    CLASS_CH, CLASS_IN, Reply, TYPE_A, TYPE_AAAA, TYPE_TXT, doh_query_ext, query_dot, query_ext,
    query_tcp_ext,
};
use crate::servers::{Candidate, Protocol, Target, is_known};
use crate::util::{median, ms, percentile, rand64, stddev};

/// Popular sites used for the "cached" lookups: the big resolvers almost always have these.
pub const DOMAINS: [&str; 10] = [
    "google.com",
    "youtube.com",
    "facebook.com",
    "github.com",
    "wikipedia.org",
    "amazon.com",
    "steampowered.com",
    "archlinux.org",
    "reddit.com",
    "microsoft.com",
];

/// Quick-scan parallelism: high enough to be fast, low enough to be gentle on home routers.
const SCAN_JOBS: usize = 64;
/// How many addresses are connection-tested at once (each uses three sockets). Kept low so
/// systems with a small open-file limit, such as macOS, never run out.
const CONNECT_JOBS: usize = 16;
/// A server that fails this many of its 35 lookups in one round is declared unreliable.
const MAX_FAILS: u32 = 8;
/// How many times each popular site is looked up.
const ROUNDS: usize = 3;
/// How many random, uncacheable names are looked up.
const RANDOM_LOOKUPS: usize = 5;

/// A short verdict for a row.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Note {
    Ok,
    /// Answers names that cannot exist with a fake address.
    Hijacks,
    /// Failed too many lookups.
    Unreliable,
}

impl Note {
    pub const fn label(self) -> &'static str {
        match self {
            Note::Ok => "OK",
            Note::Hijacks => "Hijacks NXDOMAIN",
            Note::Unreliable => "Unreliable",
        }
    }
}

/// What the DNSSEC checks found. `Validated` means the resolver set the AD bit for a signed name
/// and correctly rejected a deliberately broken signature at the same time.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SecurityStatus {
    Validated,
    Insecure,
    Partial,
    BogusAccepted,
    Unknown,
}

impl SecurityStatus {
    pub const fn label(self) -> &'static str {
        match self {
            SecurityStatus::Validated => "DNSSEC validated",
            SecurityStatus::Insecure => "No DNSSEC",
            SecurityStatus::Partial => "DNSSEC partial",
            SecurityStatus::BogusAccepted => "Accepts bogus DNSSEC",
            SecurityStatus::Unknown => "DNSSEC unknown",
        }
    }

    pub const fn short(self) -> &'static str {
        match self {
            SecurityStatus::Validated => "DNSSEC ✓",
            SecurityStatus::Insecure => "No DNSSEC",
            SecurityStatus::Partial => "DNSSEC ?",
            SecurityStatus::BogusAccepted => "Bogus accepted",
            SecurityStatus::Unknown => "DNSSEC ?",
        }
    }
}

/// One measured resolver.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Row {
    /// Stable candidate id, used by the UI.
    pub id: String,
    pub provider: String,
    pub label: String,
    pub transport: Protocol,
    /// `None` for DoH endpoints (and custom DoT endpoints without a fixed address).
    pub addr: Option<SocketAddr>,
    /// Human-readable endpoint (address or encrypted URL).
    pub endpoint: String,
    /// Median of the lookups of popular sites, in ms.
    pub cached: Option<f64>,
    /// Median of lookups of random names, in ms.
    pub uncached: Option<f64>,
    pub cached_min: Option<f64>,
    pub cached_p95: Option<f64>,
    pub cached_p99: Option<f64>,
    pub cached_mean: Option<f64>,
    pub cached_max: Option<f64>,
    pub cached_jitter: Option<f64>,
    /// Whether the server set the DNSSEC Authenticated Data bit (`None` = could not check).
    pub dnssec: Option<bool>,
    /// Whether the server correctly rejected `dnssec-failed.org` (`None` = could not check).
    pub dnssec_bogus: Option<bool>,
    pub timeouts: u32,
    pub note: Note,
    /// What the table is sorted by: lower is better, flagged servers sort last.
    pub score: f64,
}

impl Row {
    /// Measured enough to be useful: no flags, at most three failed lookups and at least one
    /// completed timing.
    pub fn healthy(&self) -> bool {
        self.note == Note::Ok
            && self.timeouts <= 3
            && (self.cached.is_some() || self.uncached.is_some())
    }

    /// Healthy and fully measured, so it can be recommended as a system resolver.
    pub fn recommendable(&self) -> bool {
        self.healthy()
            && self.cached.is_some()
            && self.uncached.is_some()
            && self.security_status() != SecurityStatus::BogusAccepted
    }

    pub fn is_doh(&self) -> bool {
        self.transport == Protocol::Doh
    }

    pub fn is_plain(&self) -> bool {
        self.transport.is_plain()
    }

    pub fn security_status(&self) -> SecurityStatus {
        match (self.dnssec, self.dnssec_bogus) {
            (_, Some(false)) => SecurityStatus::BogusAccepted,
            (Some(true), Some(true)) => SecurityStatus::Validated,
            (Some(true), _) => SecurityStatus::Partial,
            (Some(false), _) => SecurityStatus::Insecure,
            (None, _) => SecurityStatus::Unknown,
        }
    }

    /// The address that could be applied to the system resolver, if any.
    pub fn plain_ip(&self) -> Option<IpAddr> {
        self.addr.map(|a| a.ip())
    }
}

/// Which Cloudflare data center answers you (tells you how far your ISP routes you).
pub async fn cloudflare_data_center() -> Option<String> {
    let cloudflare = SocketAddr::from(([1, 1, 1, 1], 53));
    let r = query_ext(
        cloudflare,
        "id.server",
        TYPE_TXT,
        CLASS_CH,
        Duration::from_secs(2),
        false,
    )
    .await?;
    r.txt
}

/// One lookup on the transport carried by the candidate. For plain candidates `protocol`
/// selects UDP or TCP; DoT/DoH candidates ignore it.
///
/// If a UDP reply is truncated, the query is retried over TCP automatically.
pub async fn lookup(
    candidate: &Candidate,
    protocol: Protocol,
    name: &str,
    qtype: u16,
    qclass: u16,
    wait: Duration,
    dnssec: bool,
) -> Option<Reply> {
    match &candidate.target {
        Target::Plain(addr) => match protocol {
            Protocol::Udp => {
                let reply = query_ext(*addr, name, qtype, qclass, wait, dnssec).await?;
                if reply.tc {
                    query_tcp_ext(*addr, name, qtype, qclass, wait, dnssec).await
                } else {
                    Some(reply)
                }
            }
            Protocol::Tcp => query_tcp_ext(*addr, name, qtype, qclass, wait, dnssec).await,
            Protocol::Dot | Protocol::Doh => None,
        },
        Target::Dot { addr, host, port } => {
            query_dot(*addr, host, *port, name, qtype, qclass, wait, dnssec).await
        }
        Target::Doh { url } => {
            // DoH carries the same wire-format query as the other transports, so DNSSEC and
            // AAAA checks work here too instead of being reported as "unknown".
            doh_query_ext(url, name, qtype, qclass, wait, dnssec).await
        }
    }
}

/// Fills the resolver's cache before measuring. Without this, the first run against a server is
/// slower than later runs (the popular names are not cached yet), which made rankings depend on
/// who ran the benchmark before. The lookups run together and are not timed or counted.
async fn warm_up(
    candidate: &Candidate,
    protocol: Protocol,
    wait: Duration,
    cancel: Arc<AtomicBool>,
) {
    let mut set = JoinSet::new();
    for domain in DOMAINS {
        let candidate = candidate.clone();
        let cancel = cancel.clone();
        set.spawn(async move {
            if !cancel.load(Ordering::Relaxed) {
                let _ = lookup(&candidate, protocol, domain, TYPE_A, CLASS_IN, wait, false).await;
            }
        });
    }
    while set.join_next().await.is_some() {}
}

/// Tests one server: 30 lookups of popular sites (cached) and 5 lookups of random names that
/// should not exist (uncached). `repeat` pools several rounds so a noisy connection produces a
/// steadier median. Two extra queries check DNSSEC and a deliberately broken signature.
pub async fn full_test(
    candidate: Candidate,
    protocol: Protocol,
    wait: Duration,
    repeat: usize,
    cancel: Arc<AtomicBool>,
) -> Row {
    let repeat = repeat.max(1);
    let max_fails = MAX_FAILS.saturating_mul(repeat as u32);
    let mut cached: Vec<f64> = Vec::new();
    let mut uncached: Vec<f64> = Vec::new();
    let mut fails = 0u32;
    let mut hijacks = 0u32;
    let mut unreliable = false;

    warm_up(&candidate, protocol, wait, cancel.clone()).await;

    'rounds: for _ in 0..repeat {
        if cancel.load(Ordering::Relaxed) {
            unreliable = true;
            break;
        }
        for domain in DOMAINS {
            for _ in 0..ROUNDS {
                if cancel.load(Ordering::Relaxed) {
                    unreliable = true;
                    break 'rounds;
                }
                match lookup(&candidate, protocol, domain, TYPE_A, CLASS_IN, wait, false).await {
                    Some(r) if r.rcode == 0 && r.answers > 0 && !r.addrs.is_empty() => {
                        cached.push(ms(r.rtt));
                    }
                    _ => {
                        fails += 1;
                        if fails >= max_fails {
                            unreliable = true;
                            break 'rounds;
                        }
                    }
                }
            }
        }

        for _ in 0..RANDOM_LOOKUPS {
            if cancel.load(Ordering::Relaxed) {
                unreliable = true;
                break 'rounds;
            }
            let name = format!(
                "dnsbench-{:x}-{:x}-{:x}.com",
                rand64() & 0xFFFF_FFFF,
                rand64() & 0xFFFF_FFFF,
                rand64() & 0xFFFF_FFFF
            );
            match lookup(&candidate, protocol, &name, TYPE_A, CLASS_IN, wait, false).await {
                Some(r) if r.rcode == 3 => uncached.push(ms(r.rtt)),
                Some(r) if r.rcode == 0 && r.answers > 0 && !r.addrs.is_empty() => {
                    hijacks += 1; // a fake answer for a name that should not exist
                    uncached.push(ms(r.rtt));
                }
                _ => {
                    fails += 1;
                    if fails >= max_fails {
                        unreliable = true;
                        break 'rounds;
                    }
                }
            }
        }
    }

    if fails >= max_fails {
        unreliable = true;
    }
    let cached_min = cached.iter().copied().reduce(f64::min);
    let cached_max = cached.iter().copied().reduce(f64::max);
    let cached_p95 = percentile(&mut cached.clone(), 95.0);
    let cached_p99 = percentile(&mut cached.clone(), 99.0);
    let cached_mean = if cached.is_empty() {
        None
    } else {
        Some(cached.iter().sum::<f64>() / cached.len() as f64)
    };
    let cached_jitter = stddev(&cached);
    let cached_median = median(&mut cached);
    let uncached_median = median(&mut uncached);
    let note = if unreliable {
        Note::Unreliable
    } else if hijacks > 0 {
        Note::Hijacks
    } else {
        Note::Ok
    };

    // DNSSEC checks: one signed name must have the AD bit, and one deliberately broken name
    // must be rejected. Failures here never count against the resolver's latency score.
    let (dnssec, dnssec_bogus) = if unreliable || cancel.load(Ordering::Relaxed) {
        (None, None)
    } else {
        let ad = lookup(
            &candidate,
            protocol,
            "cloudflare.com",
            TYPE_A,
            CLASS_IN,
            wait,
            true,
        )
        .await
        .filter(|r| r.rcode == 0)
        .map(|r| r.ad);
        let bogus = lookup(
            &candidate,
            protocol,
            "dnssec-failed.org",
            TYPE_A,
            CLASS_IN,
            wait,
            true,
        )
        .await
        .map(|r| r.rcode == 2);
        (ad, bogus)
    };

    let base = match (cached_median, uncached_median) {
        (Some(a), Some(b)) => (a + b) / 2.0,
        (Some(a), None) | (None, Some(a)) => 1.0e6 + a, // only half measured
        (None, None) => 1.0e9,
    };
    let penalty = match note {
        Note::Ok => 0.0,
        Note::Hijacks => 5_000.0, // flagged servers sort to the bottom
        Note::Unreliable => 10_000.0,
    };
    Row {
        id: candidate.id.clone(),
        provider: candidate.provider.clone(),
        label: candidate.label.clone(),
        transport: protocol,
        addr: candidate.plain_addr(),
        endpoint: candidate.endpoint(),
        cached: cached_median,
        uncached: uncached_median,
        cached_min,
        cached_max,
        cached_p95,
        cached_p99,
        cached_mean,
        cached_jitter,
        dnssec,
        dnssec_bogus,
        timeouts: fails,
        note,
        score: base + penalty,
    }
}

/// Quick scan: one lookup per candidate. Two attempts are made for known providers on plain
/// DNS; encrypted endpoints get one. Returns the candidates that answered and the measured RTT.
pub async fn quick_scan(
    candidates: Vec<Candidate>,
    protocol: Protocol,
    wait: Duration,
    cancel: Arc<AtomicBool>,
    mut on_done: impl FnMut(usize, usize),
) -> (Vec<(Candidate, f64)>, bool) {
    let total = candidates.len();
    let sem = Arc::new(Semaphore::new(SCAN_JOBS));
    let mut set: JoinSet<(Candidate, Option<f64>)> = JoinSet::new();
    for candidate in candidates {
        let sem = sem.clone();
        let cancel = cancel.clone();
        set.spawn(async move {
            let _permit = sem.acquire_owned().await;
            let attempts = if protocol.is_plain() && is_known(&candidate.provider) {
                2
            } else {
                1
            };
            let mut best: Option<f64> = None;
            for _ in 0..attempts {
                if cancel.load(Ordering::Relaxed) {
                    break;
                }
                if let Some(reply) = lookup(
                    &candidate,
                    protocol,
                    "google.com",
                    TYPE_A,
                    CLASS_IN,
                    wait,
                    false,
                )
                .await
                    && reply.rcode == 0
                    && !reply.addrs.is_empty()
                {
                    let value = ms(reply.rtt);
                    best = Some(best.map_or(value, |old: f64| old.min(value)));
                }
            }
            (candidate, best)
        });
    }

    let mut hits = Vec::new();
    let mut done = 0usize;
    let mut cancelled = false;
    loop {
        if cancel.load(Ordering::Relaxed) {
            set.abort_all();
            cancelled = true;
            break;
        }
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_millis(100)) => {}
            item = set.join_next() => {
                match item {
                    Some(Ok((candidate, Some(rtt)))) => {
                        hits.push((candidate, rtt));
                        done += 1;
                        on_done(done, total);
                    }
                    Some(Ok((_, None))) => {
                        done += 1;
                        on_done(done, total);
                    }
                    Some(Err(_)) => {
                        done += 1;
                        on_done(done, total);
                    }
                    None => break,
                }
            }
        }
    }
    hits.sort_by(|a, b| a.1.total_cmp(&b.1));
    (hits, cancelled)
}

/// Tests the servers, `jobs` at a time (each server's own lookups run one after another), and
/// returns the rows best first. `on_row` is called as each server finishes; `on_progress` gets
/// the number finished and the total.
#[allow(clippy::too_many_arguments)]
pub async fn full_tests(
    servers: Vec<Candidate>,
    protocol: Protocol,
    jobs: usize,
    wait: Duration,
    repeat: usize,
    cancel: Arc<AtomicBool>,
    mut on_row: impl FnMut(Row),
    mut on_progress: impl FnMut(usize, usize),
) -> (Vec<Row>, bool) {
    let total = servers.len();
    let sem = Arc::new(Semaphore::new(jobs.max(1)));
    let mut set: JoinSet<Row> = JoinSet::new();
    for candidate in servers {
        let sem = sem.clone();
        let cancel = cancel.clone();
        set.spawn(async move {
            let _permit = sem.acquire_owned().await;
            full_test(candidate, protocol, wait, repeat, cancel).await
        });
    }

    let mut rows = Vec::with_capacity(total);
    let mut done = 0usize;
    let mut cancelled = false;
    loop {
        if cancel.load(Ordering::Relaxed) {
            set.abort_all();
            cancelled = true;
            break;
        }
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_millis(100)) => {}
            item = set.join_next() => {
                match item {
                    Some(Ok(row)) => {
                        rows.push(row.clone());
                        on_row(row);
                        done += 1;
                        on_progress(done, total);
                    }
                    Some(Err(_)) => {
                        // A panicked task cannot produce a row, but it still finished: keep the
                        // progress bar moving so the interface never appears stuck.
                        done += 1;
                        on_progress(done, total);
                    }
                    None => break,
                }
            }
        }
    }
    rows.sort_by(|a, b| a.score.total_cmp(&b.score));
    (rows, cancelled)
}

// ---------------------------------------------------------------------------------------
// optional: where does a particular site get steered?
// ---------------------------------------------------------------------------------------

/// One address a site resolved to, and how reachable it is.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SteerEntry {
    pub ip: IpAddr,
    /// How many of the servers returned this address.
    pub returned_by: usize,
    /// Three TCP connect attempts, in ms (`None` = failed or timed out).
    pub connects: Vec<Option<f64>>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Steering {
    pub host: String,
    /// How many servers were asked.
    pub servers: usize,
    pub entries: Vec<SteerEntry>,
}

async fn connect_ms(addr: SocketAddr, wait: Duration) -> Option<f64> {
    let start = Instant::now();
    match timeout(wait, TcpStream::connect(addr)).await {
        Ok(Ok(_stream)) => Some(ms(start.elapsed())),
        _ => None,
    }
}

/// Asks each server for `host` (A records, and AAAA records when `want_v6`), then times TCP
/// connections to every address that came back.
pub async fn steering(
    host: &str,
    servers: &[SocketAddr],
    connect_port: u16,
    want_v6: bool,
) -> Steering {
    let mut set = JoinSet::new();
    for &server in servers {
        let host = host.to_string();
        set.spawn(async move {
            let wait = Duration::from_secs(2);
            let mut ips = Vec::new();
            if let Some(r) = query_ext(server, &host, TYPE_A, CLASS_IN, wait, false).await {
                ips.extend(r.addrs);
            }
            if want_v6
                && let Some(r) = query_ext(server, &host, TYPE_AAAA, CLASS_IN, wait, false).await
            {
                ips.extend(r.addrs);
            }
            ips
        });
    }
    let mut counts: BTreeMap<IpAddr, usize> = BTreeMap::new();
    while let Some(res) = set.join_next().await {
        if let Ok(ips) = res {
            let unique: HashSet<IpAddr> = ips.into_iter().collect();
            for ip in unique {
                *counts.entry(ip).or_insert(0) += 1;
            }
        }
    }

    let sem = Arc::new(Semaphore::new(CONNECT_JOBS));
    let mut set = JoinSet::new();
    for &ip in counts.keys() {
        let sem = sem.clone();
        set.spawn(async move {
            let _permit = sem.acquire_owned().await;
            let addr = SocketAddr::new(ip, connect_port);
            let wait = Duration::from_secs(3);
            let (a, b, c) = tokio::join!(
                connect_ms(addr, wait),
                connect_ms(addr, wait),
                connect_ms(addr, wait)
            );
            (ip, vec![a, b, c])
        });
    }
    let mut connects: BTreeMap<IpAddr, Vec<Option<f64>>> = BTreeMap::new();
    while let Some(res) = set.join_next().await {
        if let Ok((ip, tries)) = res {
            connects.insert(ip, tries);
        }
    }

    let entries = counts
        .into_iter()
        .map(|(ip, returned_by)| SteerEntry {
            ip,
            returned_by,
            connects: connects.remove(&ip).unwrap_or_default(),
        })
        .collect();
    Steering {
        host: host.to_string(),
        servers: servers.len(),
        entries,
    }
}

/// The port-53 servers used for steering checks: healthy, fastest first, encrypted rows excluded.
fn steering_targets(rows: &[Row]) -> Vec<SocketAddr> {
    rows.iter()
        .filter(|r| r.healthy() && r.is_plain())
        .take(10)
        .filter_map(|r| r.addr)
        .collect()
}

/// Runs a steering check for every hostname, in the order the user supplied them.
pub async fn steerings(
    hosts: &[String],
    rows: &[Row],
    connect_port: u16,
    want_v6: bool,
) -> Vec<Steering> {
    if hosts.is_empty() {
        return Vec::new();
    }
    let top = steering_targets(rows);
    let mut set = JoinSet::new();
    for (index, host) in hosts.iter().enumerate() {
        let host = host.clone();
        let top = top.clone();
        set.spawn(async move { (index, steering(&host, &top, connect_port, want_v6).await) });
    }
    let mut results: Vec<(usize, Steering)> = Vec::new();
    while let Some(res) = set.join_next().await {
        if let Ok(item) = res {
            results.push(item);
        }
    }
    results.sort_by_key(|(index, _)| *index);
    results.into_iter().map(|(_, result)| result).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::servers::{Candidate, parse_addr};

    fn candidate(addr: &str, name: &str) -> Candidate {
        Candidate::plain(parse_addr(addr).unwrap(), name, name)
    }

    fn row(name: &str, note: Note, timeouts: u32) -> Row {
        Row {
            id: name.to_string(),
            provider: name.to_string(),
            label: name.to_string(),
            transport: Protocol::Udp,
            addr: Some("1.1.1.1:53".parse().unwrap()),
            endpoint: "1.1.1.1".into(),
            cached: Some(10.0),
            uncached: Some(20.0),
            cached_min: Some(9.0),
            cached_p95: Some(15.0),
            cached_p99: Some(18.0),
            cached_mean: Some(12.0),
            cached_max: Some(20.0),
            cached_jitter: Some(2.0),
            dnssec: Some(true),
            dnssec_bogus: Some(true),
            timeouts,
            note,
            score: 15.0,
        }
    }

    #[test]
    fn a_row_needs_a_measurement_to_be_healthy() {
        let mut r = row("ok", Note::Ok, 0);
        assert!(r.healthy());
        r.cached = None;
        r.uncached = None;
        assert!(!r.healthy());
        let mut r = row("hijack", Note::Hijacks, 0);
        assert!(!r.healthy());
        r.note = Note::Unreliable;
        assert!(!r.healthy());
    }

    #[test]
    fn security_status_combines_ad_and_bogus_checks() {
        let mut r = row("validated", Note::Ok, 0);
        assert_eq!(r.security_status(), SecurityStatus::Validated);
        r.dnssec_bogus = Some(false);
        assert_eq!(r.security_status(), SecurityStatus::BogusAccepted);
        r.dnssec = Some(false);
        assert_eq!(r.security_status(), SecurityStatus::BogusAccepted);
        r.dnssec_bogus = Some(true);
        assert_eq!(r.security_status(), SecurityStatus::Insecure);
        r.dnssec = Some(true);
        r.dnssec_bogus = None;
        assert_eq!(r.security_status(), SecurityStatus::Partial);
        r.dnssec = None;
        assert_eq!(r.security_status(), SecurityStatus::Unknown);
    }

    #[test]
    fn recommendable_requires_both_medians() {
        let mut r = row("half", Note::Ok, 0);
        r.uncached = None;
        assert!(!r.recommendable());
        r.uncached = Some(20.0);
        assert!(r.recommendable());
        r.dnssec_bogus = Some(false);
        assert!(!r.recommendable());
    }

    #[tokio::test]
    async fn quick_scan_skips_silent_servers_and_ranks_the_rest() {
        let silent = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let silent_addr = silent.local_addr().unwrap();
        let mut list = vec![candidate(&silent_addr.to_string(), "silent")];
        assert!(!list.is_empty());
        let (hits, cancelled) = quick_scan(
            std::mem::take(&mut list),
            Protocol::Udp,
            Duration::from_millis(150),
            Arc::new(AtomicBool::new(false)),
            |_, _| {},
        )
        .await;
        assert!(hits.is_empty());
        assert!(!cancelled);
    }

    #[tokio::test]
    async fn a_cancelled_scan_returns_partial_results() {
        let silent = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let addr = silent.local_addr().unwrap();
        let cancel = Arc::new(AtomicBool::new(true));
        let (hits, cancelled) = quick_scan(
            vec![candidate(&addr.to_string(), "silent")],
            Protocol::Udp,
            Duration::from_millis(50),
            cancel,
            |_, _| {},
        )
        .await;
        assert!(hits.is_empty());
        assert!(cancelled);
    }

    #[test]
    fn steering_targets_are_plain_healthy_and_best_first() {
        let mut bad = row("bad", Note::Hijacks, 0);
        bad.transport = Protocol::Udp;
        let mut doh = row("doh", Note::Ok, 0);
        doh.transport = Protocol::Doh;
        doh.addr = None;
        let rows = vec![row("ok", Note::Ok, 0), bad, doh];
        let targets = steering_targets(&rows);
        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].ip(), "1.1.1.1".parse::<IpAddr>().unwrap());
    }
}
