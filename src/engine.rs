//! The benchmark pipeline shared by the desktop interface and the integration tests.
//!
//! The engine is deliberately independent of egui: it receives a list of candidates and emits
//! progress events, incremental rows and a final result. That makes the whole run testable
//! without opening a window.

use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::bench::{self, Row, Steering};
use crate::providers;
use crate::servers::{Candidate, Family, OTHER, Pool, Protocol, fmt_addr, is_usable_ip};
use crate::system::{self, SystemInfo};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    Scan,
    Test,
    Steering,
    Done,
}

impl Stage {
    pub const fn label(self) -> &'static str {
        match self {
            Stage::Scan => "Checking which servers answer",
            Stage::Test => "Measuring latency and DNSSEC",
            Stage::Steering => "Checking where hostnames are steered",
            Stage::Done => "Finished",
        }
    }
}

/// Progress and results sent to the interface (or a test).
#[derive(Clone, Debug)]
pub enum RunEvent {
    /// Information discovered on this machine before the measurements started.
    Detected {
        current_dns: Vec<IpAddr>,
        gateway: Option<IpAddr>,
        data_center: Option<String>,
    },
    Stage {
        stage: Stage,
        done: usize,
        total: usize,
    },
    /// A server finished its full test. The interface can add it to the table immediately.
    Row(Row),
    Finished(RunResult),
    Cancelled(RunResult),
    Error(String),
}

/// Everything one run needs. The candidate list is already filtered by the interface (all
/// servers or only the selected ones); the engine adds system and nearby candidates when asked.
#[derive(Clone, Debug)]
pub struct RunRequest {
    pub candidates: Vec<Candidate>,
    pub protocol: Protocol,
    pub family: Family,
    pub wait: Duration,
    pub jobs: usize,
    pub repeat: usize,
    pub hostnames: Vec<String>,
    pub connect_port: u16,
    pub include_system: bool,
    pub include_nearby: bool,
}

impl Default for RunRequest {
    fn default() -> Self {
        RunRequest {
            candidates: Vec::new(),
            protocol: Protocol::Udp,
            family: Family::Any,
            wait: Duration::from_millis(1000),
            jobs: 16,
            repeat: 1,
            hostnames: Vec::new(),
            connect_port: 443,
            include_system: true,
            include_nearby: false,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RunResult {
    pub protocol: Protocol,
    pub family: Family,
    pub duration_secs: f64,
    pub data_center: Option<String>,
    pub current_dns: Vec<IpAddr>,
    pub rows: Vec<Row>,
    pub steerings: Vec<Steering>,
    /// How many candidates were quick-scanned.
    pub scanned: usize,
    /// How many candidates made it into the full test.
    pub tested: usize,
}

impl RunResult {
    /// The fastest recommendable resolver, regardless of transport.
    pub fn best(&self) -> Option<&Row> {
        self.rows.iter().find(|r| r.recommendable())
    }

    /// The fastest recommendable plain resolver: the one that can be applied to the OS.
    pub fn best_plain(&self) -> Option<&Row> {
        self.rows
            .iter()
            .find(|r| r.recommendable() && r.is_plain() && r.plain_ip().is_some())
    }
}

/// Runs the benchmark. Events are sent as the run progresses; a closed receiver stops the run.
pub async fn run(request: RunRequest, events: Sender<RunEvent>, cancel: Arc<AtomicBool>) {
    crate::util::seed_rng();
    let started = Instant::now();

    // Find the system's current DNS and the Cloudflare edge before testing, so the interface can
    // explain what is being compared.
    let info = if request.include_system {
        let info = tokio::task::spawn_blocking(system::detect)
            .await
            .unwrap_or_default();
        let data_center = bench::cloudflare_data_center().await;
        let _ = events.send(RunEvent::Detected {
            current_dns: info.dns.clone(),
            gateway: info.gateway,
            data_center: data_center.clone(),
        });
        Some((info, data_center))
    } else {
        None
    };
    let current_dns = info
        .as_ref()
        .map(|(info, _)| info.dns.clone())
        .unwrap_or_default();
    let data_center = info.as_ref().and_then(|(_, dc)| dc.clone());

    let mut pool = Pool::default();
    for candidate in request.candidates {
        pool.add(candidate);
    }
    if let Some((info, _)) = &info {
        for candidate in providers::system_candidates(info, pool.items()) {
            pool.add(candidate);
        }
    }

    if request.include_nearby && request.protocol.is_plain() {
        let _ = events.send(RunEvent::Stage {
            stage: Stage::Scan,
            done: 0,
            total: 0,
        });
        add_nearby(&mut pool, request.protocol, request.wait).await;
    }

    let candidates = pool.into_items();
    if candidates.is_empty() {
        let _ = events.send(RunEvent::Error(
            "There are no servers to test. Select at least one server or add a custom resolver."
                .into(),
        ));
        return;
    }
    if cancel.load(Ordering::Relaxed) {
        let _ = events.send(RunEvent::Cancelled(RunResult {
            protocol: request.protocol,
            family: request.family,
            duration_secs: started.elapsed().as_secs_f64(),
            data_center,
            current_dns,
            rows: Vec::new(),
            steerings: Vec::new(),
            scanned: 0,
            tested: 0,
        }));
        return;
    }

    let total_scan = candidates.len();
    let _ = events.send(RunEvent::Stage {
        stage: Stage::Scan,
        done: 0,
        total: total_scan,
    });
    let scan_events = events.clone();
    let scan_cancel = cancel.clone();
    let (hits, scan_cancelled) = bench::quick_scan(
        candidates,
        request.protocol,
        request.wait,
        scan_cancel,
        move |done, total| {
            let _ = scan_events.send(RunEvent::Stage {
                stage: Stage::Scan,
                done,
                total,
            });
        },
    )
    .await;

    if scan_cancelled || cancel.load(Ordering::Relaxed) {
        let _ = events.send(RunEvent::Cancelled(RunResult {
            protocol: request.protocol,
            family: request.family,
            duration_secs: started.elapsed().as_secs_f64(),
            data_center,
            current_dns,
            rows: Vec::new(),
            steerings: Vec::new(),
            scanned: total_scan,
            tested: 0,
        }));
        return;
    }
    if hits.is_empty() {
        let _ = events.send(RunEvent::Error(
            "No server answered the quick scan. Check the connection, the protocol (UDP/TCP/DoT/DoH) or the timeout."
                .into(),
        ));
        return;
    }

    let tested = hits.len();
    let _ = events.send(RunEvent::Stage {
        stage: Stage::Test,
        done: 0,
        total: tested,
    });
    let row_events = events.clone();
    let test_events = events.clone();
    let (rows, test_cancelled) = bench::full_tests(
        hits.into_iter().map(|(candidate, _)| candidate).collect(),
        request.protocol,
        request.jobs,
        request.wait,
        request.repeat,
        cancel.clone(),
        move |row| {
            let _ = row_events.send(RunEvent::Row(row));
        },
        move |done, total| {
            let _ = test_events.send(RunEvent::Stage {
                stage: Stage::Test,
                done,
                total,
            });
        },
    )
    .await;

    if test_cancelled || cancel.load(Ordering::Relaxed) {
        let _ = events.send(RunEvent::Cancelled(RunResult {
            protocol: request.protocol,
            family: request.family,
            duration_secs: started.elapsed().as_secs_f64(),
            data_center,
            current_dns,
            rows,
            steerings: Vec::new(),
            scanned: total_scan,
            tested,
        }));
        return;
    }

    let mut steerings = Vec::new();
    if !request.hostnames.is_empty() && rows.iter().any(|r| r.is_plain() && r.healthy()) {
        let _ = events.send(RunEvent::Stage {
            stage: Stage::Steering,
            done: 0,
            total: request.hostnames.len(),
        });
        steerings = bench::steerings(
            &request.hostnames,
            &rows,
            request.connect_port,
            request.family != Family::V4,
        )
        .await;
    }
    let _ = events.send(RunEvent::Stage {
        stage: Stage::Done,
        done: 1,
        total: 1,
    });
    let _ = events.send(RunEvent::Finished(RunResult {
        protocol: request.protocol,
        family: request.family,
        duration_secs: started.elapsed().as_secs_f64(),
        data_center,
        current_dns,
        rows,
        steerings,
        scanned: total_scan,
        tested,
    }));
}

/// Adds the public resolvers for the current country as extra candidates. Best effort: a
/// network failure simply leaves the built-in list untouched.
async fn add_nearby(pool: &mut Pool, protocol: Protocol, wait: Duration) {
    if !protocol.is_plain() {
        return;
    }
    let cc = tokio::task::spawn_blocking(crate::servers::fetch_country_code)
        .await
        .ok()
        .flatten();
    let Some(cc) = cc else {
        return;
    };
    let country = cc.clone();
    let listed = tokio::task::spawn_blocking(move || crate::servers::fetch_country(&country))
        .await
        .ok()
        .and_then(Result::ok)
        .unwrap_or_default();
    let _ = wait;
    for ip in listed.into_iter().take(12) {
        if !is_usable_ip(ip) {
            continue;
        }
        let addr = SocketAddr::new(ip, 53);
        let candidate = Candidate::plain(addr, OTHER, format!("Open resolver ({cc})"))
            .with_note("Public open resolver from the nearby-country list.")
            .with_category(crate::servers::Category::General);
        pool.add(candidate);
    }
}

/// Addresses to apply for a winning row. For a normal provider this includes the other
/// addresses with the same variant (for example both Cloudflare malware addresses), but never a
/// different variant that happens to share a provider group.
pub fn apply_addresses(row: &Row, candidates: &[Candidate]) -> Vec<IpAddr> {
    let mut out = Vec::new();
    for candidate in candidates {
        if candidate.label == row.label
            && let Some(addr) = candidate.plain_addr()
            && addr.port() == 53
            && is_usable_ip(addr.ip())
            && !out.contains(&addr.ip())
        {
            out.push(addr.ip());
        }
    }
    if out.is_empty()
        && let Some(addr) = row.addr
        && addr.port() == 53
        && is_usable_ip(addr.ip())
        && !out.contains(&addr.ip())
    {
        out.push(addr.ip());
    }
    out.truncate(4);
    out
}

/// The label shown for an applied address list.
pub fn describe_addresses(addresses: &[IpAddr]) -> String {
    addresses
        .iter()
        .map(|ip| fmt_addr(SocketAddr::new(*ip, 53)))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Runs a benchmark synchronously for tests and simple callers.
pub fn run_blocking(
    request: RunRequest,
    events: Sender<RunEvent>,
    cancel: Arc<AtomicBool>,
) -> Result<RunResult, String> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|e| e.to_string())?;
    let (tx, rx) = std::sync::mpsc::channel();
    runtime.spawn(async move {
        run(request, tx, cancel).await;
    });
    let mut result = None;
    while let Ok(event) = rx.recv() {
        match &event {
            RunEvent::Finished(value) | RunEvent::Cancelled(value) => {
                result = Some(value.clone());
            }
            RunEvent::Error(error) => return Err(error.clone()),
            _ => {}
        }
        let _ = events.send(event);
    }
    runtime.shutdown_timeout(Duration::from_secs(1));
    result.ok_or_else(|| "the benchmark stopped without a result".to_string())
}

/// System information for the interface's "current DNS" display. Kept here so callers do not
/// have to touch the platform-specific module directly.
pub fn detect_system() -> SystemInfo {
    system::detect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bench::{Note, SecurityStatus};
    use crate::servers::{Candidate, Protocol};

    fn row(label: &str, ip: &str, protocol: Protocol) -> Row {
        Row {
            id: format!("{label}:{ip}"),
            provider: label.split(' ').next().unwrap_or(label).to_string(),
            label: label.to_string(),
            transport: protocol,
            addr: Some(SocketAddr::new(ip.parse().unwrap(), 53)),
            endpoint: ip.to_string(),
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
            timeouts: 0,
            note: Note::Ok,
            score: 15.0,
        }
    }

    #[test]
    fn best_prefers_plain_for_system_application() {
        let mut doh = row("Encrypted", "1.1.1.1", Protocol::Doh);
        doh.transport = Protocol::Doh;
        let plain = row("Plain", "8.8.8.8", Protocol::Udp);
        let result = RunResult {
            protocol: Protocol::Udp,
            family: Family::Any,
            duration_secs: 1.0,
            data_center: None,
            current_dns: Vec::new(),
            rows: vec![doh, plain],
            steerings: Vec::new(),
            scanned: 2,
            tested: 2,
        };
        assert_eq!(result.best().unwrap().label, "Encrypted");
        assert_eq!(result.best_plain().unwrap().label, "Plain");
    }

    #[test]
    fn apply_addresses_respect_provider_variants() {
        let base = Candidate::plain("1.1.1.1:53".parse().unwrap(), "Cloudflare", "Cloudflare");
        let second = Candidate::plain("1.0.0.1:53".parse().unwrap(), "Cloudflare", "Cloudflare");
        let malware = Candidate::plain(
            "1.1.1.2:53".parse().unwrap(),
            "Cloudflare",
            "Cloudflare (malware)",
        );
        let candidates = vec![base, second, malware];
        let mut winner = row("Cloudflare", "1.1.1.1", Protocol::Udp);
        winner.addr = Some("1.1.1.1:53".parse().unwrap());
        let addresses = apply_addresses(&winner, &candidates);
        assert_eq!(
            addresses,
            vec![
                "1.1.1.1".parse::<IpAddr>().unwrap(),
                "1.0.0.1".parse::<IpAddr>().unwrap()
            ]
        );
        assert_eq!(describe_addresses(&addresses), "1.1.1.1, 1.0.0.1");
    }

    #[test]
    fn describe_addresses_keeps_nonstandard_ports() {
        let list = vec!["1.1.1.1".parse().unwrap()];
        assert_eq!(describe_addresses(&list), "1.1.1.1");
    }

    #[test]
    fn result_ignores_bogus_dnssec_for_recommendation() {
        let mut r = row("Bad", "1.1.1.1", Protocol::Udp);
        r.dnssec_bogus = Some(false);
        assert_eq!(r.security_status(), SecurityStatus::BogusAccepted);
        assert!(!r.recommendable());
    }
}
