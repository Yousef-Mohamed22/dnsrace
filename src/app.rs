//! Runs the whole thing: look around, build the candidate list, scan, test, report.

use std::collections::HashSet;
use std::io::{IsTerminal, Write};
use std::net::{IpAddr, SocketAddr};
use std::time::{Duration, Instant};

use tokio::task::JoinSet;

use crate::bench;
use crate::cli::{Config, Family, OutputFormat};
use crate::dns::DohProvider;
use crate::report;
use crate::servers::{self, CURRENT, CUSTOM, Candidate, OTHER, Pool, ROUTER, fmt_addr};
use crate::system::{self, SystemInfo};
use crate::util;

/// Where output goes. With a machine-readable format, stdout carries only the data and the
/// usual progress text goes to stderr instead.
struct Out {
    machine: bool,
    quiet: bool,
    color: bool,
    progress: bool,
}

impl Out {
    fn new(cfg: &Config) -> Self {
        let machine = cfg.format != OutputFormat::Table;
        Out {
            machine,
            quiet: cfg.format == OutputFormat::Quiet,
            color: use_color(cfg),
            progress: !machine && std::io::stderr().is_terminal(),
        }
    }

    /// Human-readable text: stdout normally, stderr with a machine-readable format.
    fn say(&self, text: &str) {
        if self.quiet {
            return;
        }
        if self.machine {
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

    /// Informational notes: stderr normally, fully silent with `--quiet`.
    fn info(&self, text: &str) {
        if !self.quiet {
            self.note(text);
        }
    }

    fn progress(&self, done: usize, total: usize, started: Instant) {
        if self.progress {
            let percent = done.saturating_mul(100) / total.max(1);
            let mut err = std::io::stderr();
            let _ = write!(
                err,
                "\r  {done}/{total} servers tested ({percent}%, {:.1}s)",
                started.elapsed().as_secs_f64()
            );
            let _ = err.flush();
        }
    }

    fn clear_progress(&self) {
        if self.progress {
            let mut err = std::io::stderr();
            let _ = write!(err, "\r{}\r", " ".repeat(48));
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
    if let Err(e) = result
        && e.kind() == std::io::ErrorKind::BrokenPipe
    {
        std::process::exit(0);
    }
}

fn use_color(cfg: &Config) -> bool {
    if cfg.no_color
        || cfg.format != OutputFormat::Table
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

async fn detect_nearby_country(enabled: bool) -> Option<String> {
    if !enabled {
        return None;
    }
    tokio::task::spawn_blocking(servers::fetch_country_code)
        .await
        .unwrap_or(None)
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

fn family_allows(family: Family, addr: SocketAddr) -> bool {
    match family {
        Family::Any => true,
        Family::V4 => addr.is_ipv4(),
        Family::V6 => addr.is_ipv6(),
    }
}

/// Runs everything and returns the process exit code.
pub async fn run(cfg: Config) -> u8 {
    let started = Instant::now();
    util::seed_rng();
    let out = Out::new(&cfg);

    #[cfg(feature = "gui")]
    {
        if cfg.gui {
            return crate::gui::run_gui(cfg).await as u8;
        }
    }
    #[cfg(not(feature = "gui"))]
    {
        if cfg.gui {
            out.note("--gui requires the 'gui' feature: build with `cargo build --features gui`");
            return 1;
        }
    }

    // a bad server list path should fail right away
    let list_text = match &cfg.servers_file {
        Some(path) => match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(e) => {
                out.note(&format!(
                    "dnsrace: cannot read the server list {}: {e}",
                    path.display()
                ));
                return 1;
            }
        },
        None => servers::BUILTIN.to_string(),
    };
    let listed = servers::parse_servers(&list_text);

    // look around: Cloudflare data center, your router and DNS, and your country if --nearby
    let (data_center, sys, nearby_cc) = tokio::join!(
        async {
            if cfg.detect {
                bench::cloudflare_data_center().await
            } else {
                None
            }
        },
        detect_system(cfg.detect),
        detect_nearby_country(cfg.nearby),
    );
    if let Some(code) = &data_center {
        out.say(&format!(
            "== Cloudflare data center you reach (first 3 letters = nearest city's airport code, e.g. cai = Cairo, mrs = Marseille, fra = Frankfurt) ==\n{code}\n"
        ));
    }

    // the country lists to download: --countries, plus the detected country with --nearby
    let mut countries = cfg.countries.clone();
    match &nearby_cc {
        Some(cc) => {
            if !countries.contains(cc) {
                countries.push(cc.clone());
            }
            out.info(&format!("note: --nearby detected country '{cc}'"));
        }
        None if cfg.nearby => {
            out.info("note: --nearby could not detect your country; use --countries <cc> instead")
        }
        None => {}
    }
    countries.sort();
    countries.dedup();
    let lists = fetch_lists(&countries).await;

    // the candidates: the list, your router, your current DNS, --add, then the country lists
    let mut pool = Pool::default();
    for c in &listed {
        if family_allows(cfg.family, c.addr) {
            pool.add(c.addr, &c.name);
        }
    }
    let current_ips: Vec<IpAddr> = sys
        .dns
        .iter()
        .copied()
        .filter(|ip| family_allows(cfg.family, SocketAddr::new(*ip, 53)))
        .collect();
    if let Some(gateway) = sys.gateway
        && family_allows(cfg.family, SocketAddr::new(gateway, 53))
    {
        pool.add(SocketAddr::new(gateway, 53), ROUTER);
    }
    for &ip in &current_ips {
        pool.add(SocketAddr::new(ip, 53), CURRENT);
    }
    for &addr in &cfg.add {
        if family_allows(cfg.family, addr) {
            pool.add(addr, CUSTOM);
        }
    }
    let mut from_lists = 0usize;
    for (cc, result) in lists {
        match result {
            Ok(ips) => {
                for ip in ips {
                    let addr = SocketAddr::new(ip, 53);
                    if family_allows(cfg.family, addr) && pool.add(addr, OTHER) {
                        from_lists += 1;
                    }
                }
            }
            Err(e) => out.note(&format!(
                "note: could not download the '{cc}' list from public-dns.info: {e}"
            )),
        }
    }

    // DNS-over-HTTPS providers are always tested when --doh is set
    if cfg.doh {
        for provider in DohProvider::all() {
            let addr = SocketAddr::from(([0, 0, 0, 0], 53));
            pool.push_raw(Candidate {
                addr,
                name: format!("{} (DoH)", provider.name()),
                doh_provider: Some(provider),
            });
        }
    }
    if pool.is_empty() {
        if cfg.family == Family::Any {
            out.note("dnsrace: no servers to test: the server list is empty.");
        } else {
            out.note(
                "dnsrace: no servers to test after the address-family filter (--ipv4/--ipv6).",
            );
        }
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
    let scan_wait = cfg.timeout.min(Duration::from_secs(3));
    let candidate_word = if pool.len() == 1 {
        "candidate"
    } else {
        "candidates"
    };
    out.say(&format!(
        "Stage 1: quick scan of {} {candidate_word}{extra}...",
        pool.len()
    ));
    let scan = bench::quick_scan(pool.items(), scan_wait, cfg.tcp).await;
    if scan.is_empty() {
        out.note("dnsrace: no server answered. Is your connection up, and is DNS (UDP port 53) allowed through any firewall or VPN?");
        return 1;
    }
    out.say(&format!("{} answered.", scan.len()));

    // stage 2: the fastest ones, tested properly
    let short = bench::shortlist(&scan, cfg.known, cfg.other, &current_ips);
    for ip in &short.silent_current {
        out.note(&format!(
            "note: your current DNS {ip} did not answer the quick scan"
        ));
    }
    if short.servers.is_empty() {
        out.note(
            "dnsrace: no servers were selected for the full test (check --known and --other).",
        );
        return 1;
    }
    let repeat_note = if cfg.repeat > 1 {
        format!(", {} rounds each", cfg.repeat)
    } else {
        String::new()
    };
    let server_word = if short.servers.len() == 1 {
        "server"
    } else {
        "servers"
    };
    out.say(&format!(
        "\nStage 2: full test of {} {server_word} ({} at a time{repeat_note})...",
        short.servers.len(),
        cfg.jobs
    ));
    let stage2_started = Instant::now();
    let rows = bench::full_tests(
        short.servers,
        cfg.jobs,
        cfg.timeout,
        cfg.repeat,
        cfg.tcp,
        |done, total| out.progress(done, total, stage2_started),
    )
    .await;
    out.clear_progress();

    // optional: where do the requested sites get steered? (not part of the CSV shape)
    let wants_steering = !cfg.host.is_empty()
        && (matches!(cfg.format, OutputFormat::Table | OutputFormat::Json)
            || cfg.export_json.is_some()
            || cfg.export_html.is_some());
    if !cfg.host.is_empty() && !wants_steering {
        out.info(
            "note: steering checks are not included in CSV or quiet output; use --json, --export-json or --export-html to include them.",
        );
    }
    let steerings = if wants_steering {
        let want_v6 = matches!(cfg.family, Family::Any | Family::V6);
        bench::steerings(&cfg.host, &rows, cfg.connect_port, want_v6).await
    } else {
        Vec::new()
    };

    let current: HashSet<IpAddr> = current_ips.iter().copied().collect();
    let json_output = report::json(
        env!("CARGO_PKG_VERSION"),
        data_center.as_deref(),
        &rows,
        &current,
        &steerings,
    );
    let csv_output = report::csv(&rows, &current);

    match cfg.format {
        OutputFormat::Json => {
            out.data(&json_output);
        }
        OutputFormat::Csv => {
            out.data(&csv_output);
        }
        OutputFormat::Quiet => match report::recommended(&rows) {
            Some(r) => out.data(&fmt_addr(r.addr)),
            None => {
                out.note("dnsrace: no healthy server to recommend; try again or raise --timeout.");
                return 1;
            }
        },
        OutputFormat::Table => {
            let shown = if cfg.top == 0 {
                rows.len()
            } else {
                cfg.top.min(rows.len())
            };
            out.say("");
            out.say(&report::table(&rows[..shown], &current, out.color));
            out.say("");
            out.say(&report::summary(&rows, &current, out.color, cfg.repeat));
            if cfg.verbose {
                out.say("");
                out.say(&report::details(&rows[..shown]));
            }
            for s in &steerings {
                out.say("");
                out.say(&report::steering_text(s));
            }
        }
    }

    // File exports (in addition to stdout output)
    if let Some(path) = &cfg.export_json {
        match std::fs::write(path, &json_output) {
            Ok(()) => out.info(&format!("wrote JSON results to {}", path.display())),
            Err(e) => out.note(&format!("could not write JSON to {}: {e}", path.display())),
        }
    }
    if let Some(path) = &cfg.export_csv {
        match std::fs::write(path, &csv_output) {
            Ok(()) => out.info(&format!("wrote CSV results to {}", path.display())),
            Err(e) => out.note(&format!("could not write CSV to {}: {e}", path.display())),
        }
    }
    if let Some(path) = &cfg.export_html {
        let html = report::html(
            env!("CARGO_PKG_VERSION"),
            data_center.as_deref(),
            &rows,
            &current,
            &steerings,
        );
        match std::fs::write(path, &html) {
            Ok(()) => out.info(&format!("wrote HTML report to {}", path.display())),
            Err(e) => out.note(&format!("could not write HTML to {}: {e}", path.display())),
        }
    }

    out.say("");
    if !countries.is_empty() {
        out.say("Servers named 'other' come from public-dns.info: open resolvers, often other people's routers.");
    }
    out.say("A DNS server sees every site you visit and can return fake answers, so only switch to one you trust.");
    out.say("How to switch: see 'Use the winner' in the README.");
    out.say(&format!("Done in {:.1}s.", started.elapsed().as_secs_f64()));
    0
}
