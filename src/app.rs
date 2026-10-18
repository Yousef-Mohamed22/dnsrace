//! Runs the whole thing: look around, build the candidate list, scan, test, report.

use std::collections::HashSet;
use std::io::{IsTerminal, Write};
use std::net::{IpAddr, SocketAddr};
use std::time::Instant;

use tokio::task::JoinSet;

use crate::bench::{self, Note};
use crate::cli::Config;
use crate::report;
use crate::servers::{self, Pool, CURRENT, CUSTOM, OTHER, ROUTER};
use crate::system::{self, SystemInfo};
use crate::util;

/// Where output goes. With `--json`, stdout carries only the JSON document and the usual
/// progress text goes to stderr instead.
struct Out {
    json: bool,
    color: bool,
    progress: bool,
}

impl Out {
    fn new(cfg: &Config) -> Self {
        Out {
            json: cfg.json,
            color: use_color(cfg),
            progress: std::io::stderr().is_terminal(),
        }
    }

    /// Human-readable text: stdout normally, stderr with `--json`.
    fn say(&self, text: &str) {
        if self.json {
            self.note(text);
        } else {
            write_stdout(text);
        }
    }

    /// The result document (always stdout).
    fn data(&self, text: &str) {
        write_stdout(text);
    }

    /// Warnings and errors (always stderr).
    fn note(&self, text: &str) {
        let _ = writeln!(std::io::stderr(), "{text}");
    }

    fn progress(&self, done: usize, total: usize) {
        if self.progress {
            let mut err = std::io::stderr();
            let _ = write!(err, "\r  {done}/{total} servers tested");
            let _ = err.flush();
        }
    }

    fn clear_progress(&self) {
        if self.progress {
            let mut err = std::io::stderr();
            let _ = write!(err, "\r{}\r", " ".repeat(40));
            let _ = err.flush();
        }
    }
}

/// Writes a line to stdout. A closed pipe (`dnsrace | head`) ends the program quietly instead
/// of panicking.
fn write_stdout(text: &str) {
    let mut out = std::io::stdout().lock();
    let result = out
        .write_all(text.as_bytes())
        .and_then(|()| out.write_all(b"\n"))
        .and_then(|()| out.flush());
    if let Err(e) = result {
        if e.kind() == std::io::ErrorKind::BrokenPipe {
            std::process::exit(0);
        }
    }
}

fn use_color(cfg: &Config) -> bool {
    if cfg.no_color
        || cfg.json
        || std::env::var_os("NO_COLOR").is_some()
        || !std::io::stdout().is_terminal()
    {
        return false;
    }
    if cfg!(windows) {
        // older Windows consoles print the escape codes literally; modern terminals set these
        return ["WT_SESSION", "ANSICON", "TERM"]
            .iter()
            .any(|v| std::env::var_os(v).is_some());
    }
    true
}

async fn detect_system(detect: bool) -> SystemInfo {
    if !detect {
        return SystemInfo::default();
    }
    tokio::task::spawn_blocking(system::detect)
        .await
        .unwrap_or_default()
}

type CountryList = (String, Result<Vec<IpAddr>, String>);

async fn fetch_lists(countries: &[String]) -> Vec<CountryList> {
    let mut set = JoinSet::new();
    for cc in countries {
        let cc = cc.clone();
        set.spawn_blocking(move || {
            let result = servers::fetch_country(&cc);
            (cc, result)
        });
    }
    let mut lists = Vec::new();
    while let Some(res) = set.join_next().await {
        if let Ok(item) = res {
            lists.push(item);
        }
    }
    lists.sort_by(|a, b| a.0.cmp(&b.0));
    lists
}

/// Runs everything and returns the process exit code.
pub async fn run(cfg: Config) -> u8 {
    let started = Instant::now();
    util::seed_rng();
    let out = Out::new(&cfg);

    // a bad server list path should fail right away
    let list_text = match &cfg.servers_file {
        Some(path) => match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(e) => {
                out.note(&format!(
                    "dnsrace: cannot read the server list {}: {}",
                    report::terminal_text(&path.display().to_string()),
                    report::terminal_text(&e.to_string())
                ));
                return 1;
            }
        },
        None => servers::BUILTIN.to_string(),
    };
    let listed = servers::parse_servers(&list_text);

    // look around: Cloudflare data center, your router and DNS, and the country lists
    let (data_center, sys, lists) = tokio::join!(
        async {
            if cfg.detect {
                bench::cloudflare_data_center().await
            } else {
                None
            }
        },
        detect_system(cfg.detect),
        fetch_lists(&cfg.countries),
    );
    if let Some(code) = &data_center {
        out.say(&report::data_center_text(code));
    }

    // the candidates: the list, your router, your current DNS, --add, then the country lists
    let mut pool = Pool::default();
    for c in &listed {
        pool.add(c.addr, &c.name);
    }
    if let Some(gateway) = sys.gateway {
        pool.add(SocketAddr::new(gateway, 53), ROUTER);
    }
    for &ip in &sys.dns {
        pool.add(SocketAddr::new(ip, 53), CURRENT);
    }
    for &addr in &cfg.add {
        pool.add(addr, CUSTOM);
    }
    let mut from_lists = 0usize;
    for (cc, result) in lists {
        match result {
            Ok(ips) => {
                for ip in ips {
                    if pool.add(SocketAddr::new(ip, 53), OTHER) {
                        from_lists += 1;
                    }
                }
            }
            Err(e) => out.note(&format!(
                "note: could not download the '{cc}' list from public-dns.info: {}",
                report::terminal_text(&e)
            )),
        }
    }
    if pool.is_empty() {
        out.note("dnsrace: no servers to test: the server list is empty.");
        return 1;
    }
    if !sys.dns.is_empty() {
        let ips: Vec<String> = sys.dns.iter().map(IpAddr::to_string).collect();
        out.say(&format!("Your current DNS: {}\n", ips.join(" ")));
    }

    // stage 1: one lookup per candidate
    let extra = if from_lists > 0 {
        format!(" ({from_lists} from public-dns.info)")
    } else {
        String::new()
    };
    out.say(&format!(
        "Stage 1: quick scan of {} candidates{extra}...",
        pool.len()
    ));
    let scan = bench::quick_scan(pool.items()).await;
    if scan.is_empty() {
        out.note("dnsrace: no server answered. Is your connection up, and is DNS (UDP port 53) allowed through any firewall or VPN?");
        return 1;
    }
    out.say(&format!("{} answered.", scan.len()));

    // stage 2: the fastest ones, tested properly
    let short = bench::shortlist(&scan, cfg.known, cfg.other, &sys.dns);
    for ip in &short.silent_current {
        out.note(&format!(
            "note: your current DNS {ip} did not answer the quick scan"
        ));
    }
    out.say(&format!(
        "\nStage 2: full test of {} servers ({} at a time)...",
        short.servers.len(),
        cfg.jobs
    ));
    let rows = bench::full_tests(short.servers, cfg.jobs, cfg.timeout, |done, total| {
        out.progress(done, total);
    })
    .await;
    out.clear_progress();

    // optional: where does the slow site get steered?
    let steering = match &cfg.host {
        Some(host) => {
            let top: Vec<SocketAddr> = rows
                .iter()
                .filter(|r| r.note == Note::Ok)
                .take(10)
                .map(|r| r.addr)
                .collect();
            let want_v6 = scan.iter().any(|(c, _)| c.addr.is_ipv6());
            Some(bench::steering(host, &top, cfg.connect_port, want_v6).await)
        }
        None => None,
    };

    let current: HashSet<IpAddr> = sys.dns.iter().copied().collect();
    if cfg.json {
        out.data(&report::json(
            env!("CARGO_PKG_VERSION"),
            data_center.as_deref(),
            &rows,
            &current,
            steering.as_ref(),
        ));
    } else {
        out.say("");
        out.say(&report::table(&rows, &current, out.color));
        out.say("");
        out.say(&report::summary(&rows, &current, out.color));
        if let Some(s) = &steering {
            out.say("");
            out.say(&report::steering_text(s));
        }
    }

    out.say("");
    if !cfg.countries.is_empty() {
        out.say("Servers named 'other' come from public-dns.info: open resolvers, often other people's routers.");
    }
    out.say("A DNS server sees every site you visit and can return fake answers, so only switch to one you trust.");
    out.say("How to switch: see 'Use the winner' in the README.");
    out.say(&format!("Done in {:.1}s.", started.elapsed().as_secs_f64()));
    0
}
