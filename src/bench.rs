//! The measurements: the quick scan, the full test, and the "where does this site go" check.

use std::collections::{BTreeMap, HashSet};
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::net::TcpStream;
use tokio::sync::Semaphore;
use tokio::task::JoinSet;
use tokio::time::timeout;

use crate::dns::{query, CLASS_CH, CLASS_IN, TYPE_A, TYPE_AAAA, TYPE_TXT};
use crate::servers::{Candidate, OTHER};
use crate::util::{median, ms, rand64};

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
/// A server that fails this many of its 35 lookups is declared unreliable.
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

/// One lookup of google.com, retried once. Returns the round-trip time in ms if the server
/// gave a usable answer.
async fn probe(addr: SocketAddr, attempts: usize) -> Option<f64> {
    for _ in 0..attempts {
        if let Some(r) = query(addr, "google.com", TYPE_A, CLASS_IN, Duration::from_secs(1)).await {
            return (r.rcode == 0 && r.answers > 0).then_some(ms(r.rtt));
        }
    }
    None
}

async fn scan_one(sem: Arc<Semaphore>, c: Candidate) -> Option<(Candidate, f64)> {
    let _permit = sem.acquire_owned().await.ok()?;
    // open resolvers from the country lists are mostly dead, so they get a single attempt
    let attempts = if c.name == OTHER { 1 } else { 2 };
    let rtt = probe(c.addr, attempts).await?;
    Some((c, rtt))
}

/// Sends one lookup to every candidate (many at a time) and returns the ones that answered,
/// fastest first, with their round-trip time in ms.
pub async fn quick_scan(pool: &[Candidate]) -> Vec<(Candidate, f64)> {
    let sem = Arc::new(Semaphore::new(SCAN_JOBS));
    let mut set = JoinSet::new();
    for c in pool {
        set.spawn(scan_one(sem.clone(), c.clone()));
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
        } else if n_known < known {
            n_known += 1;
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
    /// Median of the lookups of popular sites, in ms.
    pub cached: Option<f64>,
    /// Median of the lookups of random names, in ms.
    pub uncached: Option<f64>,
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

/// Tests one server: 30 lookups of popular sites (cached) and 5 lookups of random names that
/// cannot exist (uncached; the answer must be NXDOMAIN).
pub async fn full_test(c: Candidate, wait: Duration) -> Row {
    let mut cached: Vec<f64> = Vec::new();
    let mut uncached: Vec<f64> = Vec::new();
    let mut fails = 0u32;
    let mut hijacks = 0u32;
    let mut unreliable = false;

    'cached: for domain in DOMAINS {
        for _ in 0..ROUNDS {
            match query(c.addr, domain, TYPE_A, CLASS_IN, wait).await {
                Some(r) if r.rcode == 0 && r.answers > 0 => cached.push(ms(r.rtt)),
                _ => {
                    fails += 1;
                    if fails >= MAX_FAILS {
                        unreliable = true;
                        break 'cached;
                    }
                }
            }
        }
    }

    if !unreliable {
        for _ in 0..RANDOM_LOOKUPS {
            let name = format!(
                "dnsbench-{:x}-{:x}-{:x}.com",
                rand64() & 0xFFFF_FFFF,
                rand64() & 0xFFFF_FFFF,
                rand64() & 0xFFFF_FFFF
            );
            match query(c.addr, &name, TYPE_A, CLASS_IN, wait).await {
                Some(r) if r.rcode == 3 => uncached.push(ms(r.rtt)),
                Some(r) if r.rcode == 0 => {
                    hijacks += 1; // a fake answer for a name that does not exist
                    uncached.push(ms(r.rtt));
                }
                _ => fails += 1,
            }
        }
    }

    // a server that fails too many lookups is flagged, but we still show what it did manage
    if fails >= MAX_FAILS {
        unreliable = true;
    }
    let cached_median = median(&mut cached);
    let uncached_median = median(&mut uncached);
    let note = if unreliable {
        Note::Unreliable
    } else if hijacks > 0 {
        Note::Hijacks
    } else {
        Note::Ok
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
        cached: cached_median,
        uncached: uncached_median,
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
    mut on_done: impl FnMut(usize, usize),
) -> Vec<Row> {
    let total = servers.len();
    let sem = Arc::new(Semaphore::new(jobs.max(1)));
    let mut set: JoinSet<Row> = JoinSet::new();
    for c in servers {
        let sem = sem.clone();
        set.spawn(async move {
            let _permit = sem.acquire_owned().await;
            full_test(c, wait).await
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
            if want_v6 {
                if let Some(r) = query(server, &host, TYPE_AAAA, CLASS_IN, wait).await {
                    ips.extend(r.addrs);
                }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn cand(addr: &str, name: &str) -> Candidate {
        Candidate {
            addr: crate::servers::parse_addr(addr).unwrap(),
            name: name.to_string(),
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
            cached: Some(1.0),
            uncached: Some(1.0),
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
}
