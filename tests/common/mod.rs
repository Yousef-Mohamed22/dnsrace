//! Helpers for the end-to-end tests: simulated DNS servers on loopback, and a way to run the
//! real `dnsrace` binary against them. Everything is Rust and works on every platform.

#![allow(dead_code)] // each test file uses a different subset

use std::net::{Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use tokio::net::UdpSocket;
use tokio::task::JoinHandle;

/// How a simulated server behaves.
#[derive(Clone, Copy)]
pub struct Behavior {
    /// Delay before answering ordinary lookups.
    pub cached: Duration,
    /// Delay before answering lookups of random names.
    pub uncached: Duration,
    /// Answer random names with a fake address instead of NXDOMAIN.
    pub hijack: bool,
    /// Only ever answer google.com (the quick-scan lookup), so the full test fails.
    pub only_google: bool,
    /// Set the DNSSEC Authenticated Data bit on every answer.
    pub dnssec: bool,
}

impl Behavior {
    pub fn new(cached_ms: u64, uncached_ms: u64) -> Self {
        Behavior {
            cached: Duration::from_millis(cached_ms),
            uncached: Duration::from_millis(uncached_ms),
            hijack: false,
            only_google: false,
            dnssec: false,
        }
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
    let mut r = pkt[..question_end].to_vec();
    r[2] = 0x81; // response, recursion desired
    r[3] = 0x80 | rcode | if ad { 0x20 } else { 0 }; // RA, response code, maybe AD
    r[6] = 0;
    r[7] = answers.len() as u8;
    for a in answers {
        r.extend_from_slice(&[0xC0, 0x0C, 0, 1, 0, 1, 0, 0, 0, 60, 0, 4]);
        r.extend_from_slice(&a.octets());
    }
    r
}

/// Starts a simulated server on `bind` (for example `127.0.0.1:0`). Returns `None` when the
/// address can't be bound, which is how a machine without IPv6 shows up for `[::1]:0`.
pub async fn spawn_mock(bind: &str, behavior: Behavior) -> Option<Mock> {
    let socket = Arc::new(UdpSocket::bind(bind).await.ok()?);
    let addr = socket.local_addr().ok()?;
    let reader = socket.clone();
    let task = tokio::spawn(async move {
        let mut buf = [0u8; 512];
        loop {
            let Ok((n, peer)) = reader.recv_from(&mut buf).await else {
                continue;
            };
            let packet = buf[..n].to_vec();
            let Some((name, qtype, end)) = parse_question(&packet) else {
                continue;
            };
            if behavior.only_google && name != "google.com" {
                continue;
            }
            let random = name.starts_with("dnsbench-");
            let (rcode, answers): (u8, Vec<Ipv4Addr>) = if qtype != 1 {
                (0, vec![]) // AAAA and the like: an empty answer
            } else if random {
                if behavior.hijack {
                    (0, vec![Ipv4Addr::new(203, 0, 113, 7)])
                } else {
                    (3, vec![]) // NXDOMAIN
                }
            } else if name == "steer.test" {
                (0, vec![Ipv4Addr::LOCALHOST])
            } else {
                (0, vec![Ipv4Addr::new(198, 51, 100, 7)])
            };
            let delay = if random {
                behavior.uncached
            } else {
                behavior.cached
            };
            let sender = socket.clone();
            // answer from a separate task so a slow server never blocks its own next lookup
            tokio::spawn(async move {
                tokio::time::sleep(delay).await;
                let _ = sender
                    .send_to(
                        &build_reply(&packet, end, rcode, &answers, behavior.dnssec),
                        peer,
                    )
                    .await;
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

/// A temporary file that is deleted when dropped.
pub struct TempFile(PathBuf);

static COUNTER: AtomicU32 = AtomicU32::new(0);

impl TempFile {
    pub fn with(contents: &str) -> Self {
        let name = format!(
            "dnsrace-test-{}-{}.txt",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        );
        let path = std::env::temp_dir().join(name);
        std::fs::write(&path, contents).expect("write temp file");
        TempFile(path)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }

    pub fn arg(&self) -> String {
        self.0.to_string_lossy().into_owned()
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// What a run of the binary printed.
pub struct Output {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

/// Runs the real binary (without blocking the simulated servers).
pub async fn run_dnsrace(args: &[&str]) -> Output {
    let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
    tokio::task::spawn_blocking(move || {
        let out = std::process::Command::new(env!("CARGO_BIN_EXE_dnsrace"))
            .args(&args)
            .output()
            .expect("failed to start dnsrace");
        Output {
            code: out.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        }
    })
    .await
    .expect("the test task panicked")
}

/// The first line containing `needle`.
pub fn line_with<'a>(text: &'a str, needle: &str) -> &'a str {
    text.lines()
        .find(|l| l.contains(needle))
        .unwrap_or_else(|| panic!("no line containing {needle:?} in:\n{text}"))
}

/// Where `needle` first appears.
pub fn position(text: &str, needle: &str) -> usize {
    text.find(needle)
        .unwrap_or_else(|| panic!("{needle:?} not found in:\n{text}"))
}
