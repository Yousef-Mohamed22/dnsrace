//! Helpers for the integration tests: simulated DNS servers on loopback and an in-process
//! benchmark runner. The desktop binary is never launched, so these tests work on headless
//! build machines.

#![allow(dead_code)] // each integration test uses a different subset

use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::Duration;

use dnsrace::bench::{Note, Row};
use dnsrace::engine::{self, RunEvent, RunRequest, RunResult};
use dnsrace::servers::{Candidate, Protocol};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, UdpSocket};
use tokio::task::JoinHandle;

/// How a simulated DNS server behaves.
#[derive(Clone, Copy)]
pub struct Behavior {
    /// Delay before answering ordinary lookups.
    pub cached: Duration,
    /// Delay before answering random names.
    pub uncached: Duration,
    /// Answer random names with a fake address instead of NXDOMAIN.
    pub hijack: bool,
    /// Only ever answer google.com, so a full test cannot complete.
    pub only_google: bool,
    /// Set the DNSSEC Authenticated Data bit on every answer.
    pub dnssec: bool,
    /// Return an answer for dnssec-failed.org instead of SERVFAIL.
    pub accept_bogus: bool,
}

impl Behavior {
    pub fn new(cached_ms: u64, uncached_ms: u64) -> Self {
        Behavior {
            cached: Duration::from_millis(cached_ms),
            uncached: Duration::from_millis(uncached_ms),
            hijack: false,
            only_google: false,
            dnssec: false,
            accept_bogus: false,
        }
    }

    pub fn validated(mut self) -> Self {
        self.dnssec = true;
        self
    }

    pub fn accepts_bogus(mut self) -> Self {
        self.accept_bogus = true;
        self
    }
}

/// A running simulated server. It stops when dropped.
pub struct Mock {
    pub addr: SocketAddr,
    task: JoinHandle<()>,
}

impl Drop for Mock {
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// The name in the question of a query, its type, and where the question ends.
fn parse_question(pkt: &[u8]) -> Option<(String, u16, usize)> {
    let mut pos = 12;
    let mut labels = Vec::new();
    loop {
        let len = *pkt.get(pos)? as usize;
        if len == 0 {
            break;
        }
        labels.push(String::from_utf8_lossy(pkt.get(pos + 1..pos + 1 + len)?).into_owned());
        pos += 1 + len;
    }
    let qtype = u16::from_be_bytes([*pkt.get(pos + 1)?, *pkt.get(pos + 2)?]);
    Some((labels.join("."), qtype, pos + 5))
}

fn build_reply(
    pkt: &[u8],
    question_end: usize,
    rcode: u8,
    answers: &[Ipv4Addr],
    ad: bool,
) -> Vec<u8> {
    let mut reply = pkt[..question_end].to_vec();
    reply[2] = 0x81; // response, recursion desired
    reply[3] = 0x80 | rcode | if ad { 0x20 } else { 0 }; // RA, rcode, maybe AD
    reply[6] = 0;
    reply[7] = answers.len() as u8;
    for address in answers {
        reply.extend_from_slice(&[0xC0, 0x0C, 0, 1, 0, 1, 0, 0, 0, 60, 0, 4]);
        reply.extend_from_slice(&address.octets());
    }
    reply
}

/// Creates the reply for one query, including the DNSSEC behavior.
fn answer(packet: &[u8], behavior: Behavior) -> Option<(Vec<u8>, Duration)> {
    let (name, qtype, end) = parse_question(packet)?;
    if behavior.only_google && name != "google.com" {
        return None;
    }
    let random = name.starts_with("dnsbench-");
    let (rcode, answers, delayed): (u8, Vec<Ipv4Addr>, Duration) = if qtype != 1 {
        (0, Vec::new(), behavior.cached)
    } else if name == "dnssec-failed.org" && !behavior.accept_bogus {
        (2, Vec::new(), behavior.cached)
    } else if random {
        if behavior.hijack {
            (0, vec![Ipv4Addr::new(203, 0, 113, 7)], behavior.uncached)
        } else {
            (3, Vec::new(), behavior.uncached)
        }
    } else if name == "steer.test" || name == "steer2.test" {
        (0, vec![Ipv4Addr::LOCALHOST], behavior.cached)
    } else {
        (0, vec![Ipv4Addr::new(198, 51, 100, 7)], behavior.cached)
    };
    Some((
        build_reply(packet, end, rcode, &answers, behavior.dnssec),
        delayed,
    ))
}

/// Starts a simulated UDP DNS server. Returns `None` when the address cannot be bound.
pub async fn spawn_mock(bind: &str, behavior: Behavior) -> Option<Mock> {
    let socket = Arc::new(UdpSocket::bind(bind).await.ok()?);
    let addr = socket.local_addr().ok()?;
    let task = tokio::spawn(async move {
        let mut buffer = [0u8; 512];
        loop {
            let Ok((length, peer)) = socket.recv_from(&mut buffer).await else {
                continue;
            };
            let packet = buffer[..length].to_vec();
            let Some((reply, delay)) = answer(&packet, behavior) else {
                continue;
            };
            let sender = socket.clone();
            tokio::spawn(async move {
                tokio::time::sleep(delay).await;
                let _ = sender.send_to(&reply, peer).await;
            });
        }
    });
    Some(Mock { addr, task })
}

/// Starts a simulated DNS-over-TCP server on `bind`.
pub async fn spawn_tcp_mock(bind: &str, behavior: Behavior) -> Option<Mock> {
    let listener = TcpListener::bind(bind).await.ok()?;
    let addr = listener.local_addr().ok()?;
    let task = tokio::spawn(async move {
        loop {
            let Ok((mut stream, _peer)) = listener.accept().await else {
                continue;
            };
            tokio::spawn(async move {
                loop {
                    let mut length = [0u8; 2];
                    if stream.read_exact(&mut length).await.is_err() {
                        break;
                    }
                    let size = u16::from_be_bytes(length) as usize;
                    let mut packet = vec![0u8; size];
                    if stream.read_exact(&mut packet).await.is_err() {
                        break;
                    }
                    let Some((reply, delay)) = answer(&packet, behavior) else {
                        break;
                    };
                    tokio::time::sleep(delay).await;
                    let length = (reply.len() as u16).to_be_bytes();
                    if stream.write_all(&length).await.is_err()
                        || stream.write_all(&reply).await.is_err()
                    {
                        break;
                    }
                }
            });
        }
    });
    Some(Mock { addr, task })
}

/// A UDP port on loopback that nothing listens on.
pub async fn closed_udp_port() -> u16 {
    let socket = UdpSocket::bind("127.0.0.1:0").await.expect("bind");
    let port = socket.local_addr().expect("addr").port();
    drop(socket);
    port
}

/// A TCP listener that accepts connections and drops them. Steering only needs a successful
/// three-way handshake.
pub async fn tcp_stub() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind tcp");
    let port = listener.local_addr().expect("addr").port();
    tokio::spawn(async move {
        loop {
            let _ = listener.accept().await;
        }
    });
    port
}

/// A candidate backed by a mock server.
pub fn mock_candidate(mock: &Mock, provider: &str, label: &str) -> Candidate {
    Candidate::plain(mock.addr, provider, label)
}

/// Runs the engine in-process. All emitted events are collected for assertions.
pub fn run_engine(request: RunRequest) -> (Result<RunResult, String>, Vec<RunEvent>) {
    run_engine_cancelled(request, Arc::new(AtomicBool::new(false)))
}

/// Runs the engine with a caller-supplied cancellation flag.
pub fn run_engine_cancelled(
    request: RunRequest,
    cancel: Arc<AtomicBool>,
) -> (Result<RunResult, String>, Vec<RunEvent>) {
    // `run_blocking` creates and shuts down a Tokio runtime. Do that on a plain thread: dropping
    // a runtime inside an async test's runtime is not allowed.
    let (result_sender, result_receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let (sender, receiver) = mpsc::channel();
        let result = engine::run_blocking(request, sender, cancel);
        let events = receiver.try_iter().collect();
        let _ = result_sender.send((result, events));
    });
    result_receiver
        .recv()
        .expect("the benchmark helper thread stopped without a result")
}

/// A sample row for tests that do not need to run the network.
pub fn sample_row(id: &str, label: &str, ip: &str, score: f64) -> Row {
    Row {
        id: id.to_string(),
        provider: label.to_string(),
        label: label.to_string(),
        transport: Protocol::Udp,
        addr: Some(format!("{ip}:53").parse().expect("socket address")),
        endpoint: ip.to_string(),
        cached: Some(score),
        uncached: Some(score + 1.0),
        cached_min: Some(score),
        cached_p95: Some(score + 2.0),
        cached_p99: Some(score + 3.0),
        cached_mean: Some(score + 1.0),
        cached_max: Some(score + 4.0),
        cached_jitter: Some(1.0),
        dnssec: Some(true),
        dnssec_bogus: Some(true),
        timeouts: 0,
        note: Note::Ok,
        score,
    }
}

/// Waits briefly for background tasks to observe a cancellation flag.
pub async fn cancel_soon(cancel: Arc<AtomicBool>) {
    tokio::time::sleep(Duration::from_millis(30)).await;
    cancel.store(true, Ordering::Relaxed);
}
