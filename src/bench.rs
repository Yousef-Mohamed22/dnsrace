//! The measurements: the quick scan, the full test, and the "where does this site go" check.

use std::collections::{BTreeMap, HashSet};
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::net::TcpStream;
use tokio::sync::Semaphore;
use tokio::task::JoinSet;
use tokio::time::timeout;

use crate::dns::{
    CLASS_CH, CLASS_IN, DohProvider, TYPE_A, TYPE_AAAA, TYPE_TXT, doh_query, query, query_ext,
    query_tcp,
};
use crate::servers::{Candidate, OTHER, is_known};
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

/// Which Cloudflare data center answers you (tells you how far your ISP routes you).
pub async fn cloudflare_data_center() -> Option<String> {
    let cloudflare = SocketAddr::from(([1, 1, 1, 1], 53));
    query(
        cloudflare,
        "id.server",
        TYPE_TXT,
        CLASS_CH,
        Duration::from_secs(2),
    )
    .await?
    .txt
}

// ---------------------------------------------------------------------------------------
// stage 1: quick scan
// ---------------------------------------------------------------------------------------

/// One lookup of google.com, retried when the server is slow or flaky. Returns the round-trip
/// time in ms if the server gave a usable answer.
async fn probe(addr: SocketAddr, attempts: usize, wait: Duration, tcp: bool) -> Option<f64> {
    for _ in 0..attempts {
        let r = if tcp {
            query_tcp(addr, "google.com", TYPE_A, CLASS_IN, wait).await
        } else {
            query(addr, "google.com", TYPE_A, CLASS_IN, wait).await
        };
        if let Some(r) = r
            && r.rcode == 0
            && r.answers > 0
            && !r.addrs.is_empty()
        {
            return Some(ms(r.rtt));
        }
    }
    None
}

/// One quick-scan probe for a DNS-over-HTTPS provider.
async fn probe_doh(provider: DohProvider, wait: Duration) -> Option<f64> {
    let r = doh_query(provider, "google.com", wait).await?;
    if r.status == 0 && r.answers > 0 {
        return Some(ms(r.rtt));
    }
    None
}

async fn scan_one(
    sem: Arc<Semaphore>,
    c: Candidate,
    wait: Duration,
    tcp: bool,
) -> Option<(Candidate, f64)> {
    let _permit = sem.acquire_owned().await.ok()?;
    // open resolvers from the country lists are mostly dead, so they get a single attempt
    let attempts = if c.name == OTHER { 1 } else { 2 };
    let rtt = match c.doh_provider {
        Some(provider) => probe_doh(provider, wait).await?,
        None => probe(c.addr, attempts, wait, tcp).await?,
    };
    Some((c, rtt))
}

/// Sends one lookup to every candidate (many at a time) and returns the ones that answered,
/// fastest first, with their round-trip time in ms.
pub async fn quick_scan(pool: &[Candidate], wait: Duration, tcp: bool) -> Vec<(Candidate, f64)> {
    let sem = Arc::new(Semaphore::new(SCAN_JOBS));
    let mut set = JoinSet::new();
    for c in pool {
        set.spawn(scan_one(sem.clone(), c.clone(), wait, tcp));
    }
    let mut hits = Vec::new();
    while let Some(res) = set.join_next().await {
        if let Ok(Some(hit)) = res {
            hits.push(hit);
        }
    }
    hits.sort_by(|a, b| a.1.total_cmp(&b.1));
    hits
}

/// The servers that get the full test.
#[derive(Debug)]
pub struct Shortlist {
    pub servers: Vec<Candidate>,
    /// Your current DNS servers that did not answer the quick scan.
    pub silent_current: Vec<IpAddr>,
}

/// The fastest `known` servers from the list, the fastest `other` country resolvers, and always
/// your current DNS servers.
pub fn shortlist(
    scan: &[(Candidate, f64)],
    known: usize,
    other: usize,
    current: &[IpAddr],
) -> Shortlist {
    let mut servers: Vec<Candidate> = Vec::new();
    let (mut n_known, mut n_other) = (0, 0);
    for (c, _) in scan {
        if c.name == OTHER {
            if n_other < other {
                n_other += 1;
                servers.push(c.clone());
            }
        } else if is_known(&c.name) {
            if n_known < known {
                n_known += 1;
                servers.push(c.clone());
            }
        } else {
            // your router, your current DNS and --add servers are always fully tested
            servers.push(c.clone());
        }
    }
    let mut silent_current = Vec::new();
    for &ip in current {
        match scan
            .iter()
            .find(|(c, _)| c.addr.ip() == ip && c.addr.port() == 53)
        {
            Some((c, _)) => {
                if !servers.iter().any(|s| s.addr == c.addr) {
                    servers.push(c.clone());
                }
            }
            None => silent_current.push(ip),
        }
    }
    Shortlist {
        servers,
        silent_current,
    }
}

// ---------------------------------------------------------------------------------------
// stage 2: full test
// ---------------------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Note {
    Ok,
    /// Answers names that cannot exist with a fake address.
    Hijacks,
    /// Failed too many lookups.
    Unreliable,
}

#[derive(Clone, Debug)]
pub struct Row {
    pub addr: SocketAddr,
    pub name: String,
    /// When set, this result came from a DNS-over-HTTPS provider rather than a port-53 server.
    pub is_doh: bool,
    /// Median of the lookups of popular sites, in ms.
    pub cached: Option<f64>,
    /// Median of the lookups of random names, in ms.
    pub uncached: Option<f64>,
    /// Fastest cached lookup, in ms.
    pub cached_min: Option<f64>,
    /// 95th percentile of the cached lookups, in ms.
    pub cached_p95: Option<f64>,
    /// 99th percentile of the cached lookups, in ms.
    pub cached_p99: Option<f64>,
    /// Mean of the cached lookups, in ms.
    pub cached_mean: Option<f64>,
    /// Slowest cached lookup, in ms.
    pub cached_max: Option<f64>,
    /// Standard deviation of the cached lookups, in ms: how much the times jump around.
    pub cached_jitter: Option<f64>,
    /// Whether the server set the DNSSEC Authenticated Data bit (`None` = could not check).
    pub dnssec: Option<bool>,
    pub timeouts: u32,
    pub note: Note,
    /// What the table is sorted by: lower is better, flagged servers sort last.
    pub score: f64,
}

impl Row {
    /// Fast enough to recommend: no flags and at most three failed lookups.
    pub fn healthy(&self) -> bool {
        self.note == Note::Ok && self.timeouts <= 3
    }
}

/// Fills the resolver's cache before measuring. Without this, the first run against a server is
/// slower than later runs (the popular names are not cached yet), which made rankings depend on
/// who ran the benchmark before. The lookups run together and are not timed or counted.
async fn warm_up(addr: SocketAddr, wait: Duration, tcp: bool) {
    let mut set = JoinSet::new();
    for domain in DOMAINS {
        set.spawn(async move {
            if tcp {
                let _ = query_tcp(addr, domain, TYPE_A, CLASS_IN, wait).await;
            } else {
                let _ = query(addr, domain, TYPE_A, CLASS_IN, wait).await;
            }
        });
    }
    while set.join_next().await.is_some() {}
}

/// Tests one server over UDP or TCP: 30 lookups of popular sites (cached) and 5 lookups of
/// random names that cannot exist (uncached; the answer must be NXDOMAIN). `repeat` pools
/// several rounds so a noisy connection produces a steadier median.
pub async fn full_test(c: Candidate, wait: Duration, repeat: usize, tcp: bool) -> Row {
    let repeat = repeat.max(1);
    let max_fails = MAX_FAILS.saturating_mul(repeat as u32);
    let mut cached: Vec<f64> = Vec::new();
    let mut uncached: Vec<f64> = Vec::new();
    let mut fails = 0u32;
    let mut hijacks = 0u32;
    let mut unreliable = false;

    warm_up(c.addr, wait, tcp).await;

    'rounds: for _ in 0..repeat {
        for domain in DOMAINS {
            for _ in 0..ROUNDS {
                let r = if tcp {
                    query_tcp(c.addr, domain, TYPE_A, CLASS_IN, wait).await
                } else {
                    query(c.addr, domain, TYPE_A, CLASS_IN, wait).await
                };
                match r {
                    Some(r) if r.rcode == 0 && r.answers > 0 && !r.addrs.is_empty() => {
                        cached.push(ms(r.rtt))
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
            let name = format!(
                "dnsbench-{:x}-{:x}-{:x}.com",
                rand64() & 0xFFFF_FFFF,
                rand64() & 0xFFFF_FFFF,
                rand64() & 0xFFFF_FFFF
            );
            let r = if tcp {
                query_tcp(c.addr, &name, TYPE_A, CLASS_IN, wait).await
            } else {
                query(c.addr, &name, TYPE_A, CLASS_IN, wait).await
            };
            match r {
                Some(r) if r.rcode == 3 => uncached.push(ms(r.rtt)),
                Some(r) if r.rcode == 0 && r.answers > 0 && !r.addrs.is_empty() => {
                    hijacks += 1; // a fake answer for a name that does not exist
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

    // a server that fails too many lookups is flagged, but we still show what it did manage
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
    // Ask a signed name with DNSSEC enabled: the AD bit tells us whether the server validates.
    // One extra lookup, and a failure here never counts against the server.
    let dnssec = if unreliable {
        None
    } else {
        let q = if tcp {
            query_tcp(c.addr, "cloudflare.com", TYPE_A, CLASS_IN, wait).await
        } else {
            query_ext(c.addr, "cloudflare.com", TYPE_A, CLASS_IN, wait, true).await
        };
        q.filter(|r| r.rcode == 0).map(|r| r.ad)
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
        addr: c.addr,
        name: c.name,
        is_doh: false,
        cached: cached_median,
        uncached: uncached_median,
        cached_min,
        cached_max,
        cached_p95,
        cached_p99,
        cached_mean,
        cached_jitter,
        dnssec,
        timeouts: fails,
        note,
        score: base + penalty,
    }
}

/// Tests one DNS-over-HTTPS provider. Instead of port-53 lookups, we send one HTTPS GET per
/// round and measure the response time.
pub async fn full_test_doh(provider: DohProvider, wait: Duration, repeat: usize) -> Row {
    let repeat = repeat.max(1);
    let max_fails = MAX_FAILS.saturating_mul(repeat as u32);
    let mut cached: Vec<f64> = Vec::new();
    let mut uncached: Vec<f64> = Vec::new();
    let mut fails = 0u32;
    let mut hijacks = 0u32;
    let mut unreliable = false;

    'rounds: for _ in 0..repeat {
        // cached: popular name
        for domain in DOMAINS {
            for _ in 0..ROUNDS {
                match doh_query(provider, domain, wait).await {
                    Some(r) if r.status == 0 && r.answers > 0 => cached.push(ms(r.rtt)),
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
        // uncached: random name that should not exist
        for _ in 0..RANDOM_LOOKUPS {
            let name = format!(
                "dnsbench-{:x}-{:x}-{:x}.com",
                rand64() & 0xFFFF_FFFF,
                rand64() & 0xFFFF_FFFF,
                rand64() & 0xFFFF_FFFF
            );
            match doh_query(provider, &name, wait).await {
                Some(r) if r.status == 3 => uncached.push(ms(r.rtt)),
                Some(r) if r.status == 0 && r.answers > 0 => {
                    hijacks += 1;
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
    // DoH servers always validate DNSSEC (they set AD), but we can't verify independently.
    let dnssec = if unreliable { None } else { Some(true) };
    let base = match (cached_median, uncached_median) {
        (Some(a), Some(b)) => (a + b) / 2.0,
        (Some(a), None) | (None, Some(a)) => 1.0e6 + a,
        (None, None) => 1.0e9,
    };
    let penalty = match note {
        Note::Ok => 0.0,
        Note::Hijacks => 5_000.0,
        Note::Unreliable => 10_000.0,
    };
    Row {
        addr: SocketAddr::from(([0, 0, 0, 0], 53)),
        name: provider.name().to_string(),
        is_doh: true,
        cached: cached_median,
        uncached: uncached_median,
        cached_min,
        cached_max,
        cached_p95,
        cached_p99,
        cached_mean,
        cached_jitter,
        dnssec,
        timeouts: fails,
        note,
        score: base + penalty,
    }
}

/// Tests the servers, `jobs` at a time (each server's own lookups run one after another), and
/// returns the rows best first. `on_done(finished, total)` is called as each server finishes.
pub async fn full_tests(
    servers: Vec<Candidate>,
    jobs: usize,
    wait: Duration,
    repeat: usize,
    tcp: bool,
    mut on_done: impl FnMut(usize, usize),
) -> Vec<Row> {
    let total = servers.len();
    let sem = Arc::new(Semaphore::new(jobs.max(1)));
    let mut set: JoinSet<Row> = JoinSet::new();
    for c in servers {
        let sem = sem.clone();
        set.spawn(async move {
            let _permit = sem.acquire_owned().await;
            match c.doh_provider {
                Some(provider) => full_test_doh(provider, wait, repeat).await,
                None => full_test(c, wait, repeat, tcp).await,
            }
        });
    }
    let mut rows = Vec::with_capacity(total);
    while let Some(res) = set.join_next().await {
        if let Ok(row) = res {
            rows.push(row);
            on_done(rows.len(), total);
        }
    }
    rows.sort_by(|a, b| a.score.total_cmp(&b.score));
    rows
}

// ---------------------------------------------------------------------------------------
// optional: where does a particular site get steered?
// ---------------------------------------------------------------------------------------

/// One address a site resolved to, and how reachable it is.
#[derive(Clone, Debug)]
pub struct SteerEntry {
    pub ip: IpAddr,
    /// How many of the servers returned this address.
    pub returned_by: usize,
    /// Three TCP connect attempts, in ms (`None` = failed or timed out).
    pub connects: Vec<Option<f64>>,
}

#[derive(Clone, Debug)]
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
    let mut set: JoinSet<Vec<IpAddr>> = JoinSet::new();
    for &server in servers {
        let host = host.to_string();
        set.spawn(async move {
            let wait = Duration::from_secs(2);
            let mut ips = Vec::new();
            if let Some(r) = query(server, &host, TYPE_A, CLASS_IN, wait).await {
                ips.extend(r.addrs);
            }
            if want_v6 && let Some(r) = query(server, &host, TYPE_AAAA, CLASS_IN, wait).await {
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
    let mut set: JoinSet<(IpAddr, Vec<Option<f64>>)> = JoinSet::new();
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

/// The port-53 servers used for steering checks: healthy, fastest first, DoH rows excluded.
fn steering_targets(rows: &[Row]) -> Vec<SocketAddr> {
    rows.iter()
        .filter(|r| r.healthy() && !r.is_doh)
        .take(10)
        .map(|r| r.addr)
        .collect()
}

/// Runs a steering check for every hostname, in the order the user supplied them.
///
/// DNS-over-HTTPS rows are skipped: their placeholder address is not a real DNS server that can
/// be asked about a hostname. The hosts are checked against the ten fastest healthy port-53
/// servers.
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

    fn cand(addr: &str, name: &str) -> Candidate {
        Candidate {
            addr: crate::servers::parse_addr(addr).unwrap(),
            name: name.to_string(),
            doh_provider: None,
        }
    }

    #[test]
    fn shortlist_takes_the_fastest_of_each_kind_and_always_your_current_dns() {
        let scan = vec![
            (cand("1.1.1.1", "Cloudflare"), 10.0),
            (cand("5.5.5.5", OTHER), 11.0),
            (cand("8.8.8.8", "Google"), 12.0),
            (cand("6.6.6.6", OTHER), 13.0),
            (cand("9.9.9.9", "Quad9"), 14.0),
            (cand("7.7.7.7", "Slow"), 99.0),
        ];
        let s = shortlist(
            &scan,
            2,
            1,
            &["7.7.7.7".parse().unwrap(), "4.4.4.4".parse().unwrap()],
        );
        let got: Vec<String> = s.servers.iter().map(|c| c.addr.ip().to_string()).collect();
        assert_eq!(got, ["1.1.1.1", "5.5.5.5", "8.8.8.8", "7.7.7.7"]);
        assert_eq!(s.silent_current, vec!["4.4.4.4".parse::<IpAddr>().unwrap()]);
    }

    #[test]
    fn router_current_and_added_servers_do_not_consume_known_slots() {
        let scan = vec![
            (cand("192.168.1.1", crate::servers::ROUTER), 1.0),
            (cand("5.5.5.5", crate::servers::CUSTOM), 2.0),
            (cand("1.1.1.1", "Cloudflare"), 10.0),
            (cand("8.8.8.8", "Google"), 12.0),
        ];
        let s = shortlist(&scan, 1, 0, &[]);
        let got: Vec<String> = s.servers.iter().map(|c| c.addr.ip().to_string()).collect();
        assert_eq!(got, ["192.168.1.1", "5.5.5.5", "1.1.1.1"]);
    }

    #[test]
    fn a_current_dns_on_another_port_is_not_the_same_server() {
        let scan = vec![(cand("1.1.1.1:5353", "Local"), 1.0)];
        let s = shortlist(&scan, 0, 0, &["1.1.1.1".parse().unwrap()]);
        assert!(s.servers.is_empty());
        assert_eq!(s.silent_current.len(), 1);
    }

    #[test]
    fn healthy_means_no_flag_and_few_timeouts() {
        let mut row = Row {
            addr: "1.1.1.1:53".parse().unwrap(),
            name: "x".into(),
            is_doh: false,
            cached: Some(1.0),
            uncached: Some(1.0),
            cached_min: Some(1.0),
            cached_max: Some(2.0),
            cached_p95: Some(1.0),
            cached_p99: Some(1.5),
            cached_mean: Some(1.0),
            cached_jitter: Some(0.0),
            dnssec: Some(true),
            timeouts: 3,
            note: Note::Ok,
            score: 1.0,
        };
        assert!(row.healthy());
        row.timeouts = 4;
        assert!(!row.healthy());
        row.timeouts = 0;
        row.note = Note::Hijacks;
        assert!(!row.healthy());
    }

    fn result_row(addr: &str, is_doh: bool, note: Note, timeouts: u32) -> Row {
        Row {
            addr: addr.parse().unwrap(),
            name: "x".into(),
            is_doh,
            cached: Some(1.0),
            uncached: Some(1.0),
            cached_min: Some(1.0),
            cached_max: Some(1.0),
            cached_p95: Some(1.0),
            cached_p99: Some(1.0),
            cached_mean: Some(1.0),
            cached_jitter: Some(0.0),
            dnssec: None,
            timeouts,
            note,
            score: 1.0,
        }
    }

    #[test]
    fn steering_targets_skip_doh_flagged_rows_and_cap_at_ten() {
        let rows = vec![
            result_row("0.0.0.0:53", true, Note::Ok, 0),
            result_row("1.1.1.1:53", false, Note::Hijacks, 0),
            result_row("8.8.8.8:53", false, Note::Ok, 4),
            result_row("9.9.9.9:53", false, Note::Ok, 0),
        ];
        let targets = steering_targets(&rows);
        assert_eq!(targets, vec!["9.9.9.9:53".parse().unwrap()]);

        let many: Vec<Row> = (1..=12)
            .map(|i| result_row(&format!("192.0.2.{i}:53"), false, Note::Ok, 0))
            .collect();
        assert_eq!(steering_targets(&many).len(), 10);
    }
}
