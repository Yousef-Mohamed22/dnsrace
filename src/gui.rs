//! Graphical interface for dnsrace using egui/eframe.
//!
//! Compiled only with the `gui` feature. Launched with `dnsrace --gui`.

use std::net::SocketAddr;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use eframe::egui;

use crate::cli::{self, Config};
use crate::report;

/// Message sent from the benchmark thread to the GUI.
enum GuiMsg {
    /// Progress: (done, total, elapsed_seconds)
    Progress(usize, usize, f64),
    /// The benchmark finished with the result text and exit code.
    Finished(String, u8),
    /// An error message.
    Error(String),
}

/// The GUI application state.
pub struct DnsRaceGui {
    /// Hostnames to test (one per line).
    hosts: String,
    /// Whether to test DNS-over-HTTPS providers.
    doh: bool,
    /// Query over TCP instead of UDP.
    tcp: bool,
    /// Per-lookup timeout in ms.
    timeout_ms: String,
    /// Number of concurrent servers to test.
    jobs: String,
    /// Repeat count (1-5).
    repeat: String,
    /// Address family filter.
    family: Family,
    /// Output format.
    format: OutputFormat,
    /// Verbose output.
    verbose: bool,
    /// Whether to auto-detect router/current DNS/country.
    detect: bool,
    /// Test nearby country resolvers.
    nearby: bool,
    /// Whether a benchmark is currently running.
    running: bool,
    /// Result text from the last run.
    result: Option<String>,
    /// Exit code from the last run.
    exit_code: u8,
    /// Progress (done, total, elapsed).
    progress: Option<(usize, usize, f64)>,
    /// Error message.
    error: Option<String>,
    /// Receiver for benchmark messages.
    rx: Option<mpsc::Receiver<GuiMsg>>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Family {
    Any,
    V4,
    V6,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum OutputFormat {
    Table,
    Json,
    Csv,
    Quiet,
}

impl Default for DnsRaceGui {
    fn default() -> Self {
        Self {
            hosts: String::new(),
            doh: false,
            tcp: false,
            timeout_ms: "1000".to_string(),
            jobs: "16".to_string(),
            repeat: "1".to_string(),
            family: Family::Any,
            format: OutputFormat::Table,
            verbose: false,
            detect: true,
            nearby: false,
            running: false,
            result: None,
            exit_code: 0,
            progress: None,
            error: None,
            rx: None,
        }
    }
}

impl DnsRaceGui {
    pub fn new() -> Self {
        Self::default()
    }

    /// Check for messages from the benchmark thread (non-blocking).
    fn poll_messages(&mut self) {
        use std::sync::mpsc::TryRecvError;

        let rx = match self.rx.take() {
            Some(rx) => rx,
            None => return,
        };
        let mut finished = false;
        loop {
            match rx.try_recv() {
                Ok(GuiMsg::Progress(done, total, elapsed)) => {
                    self.progress = Some((done, total, elapsed));
                }
                Ok(GuiMsg::Finished(text, code)) => {
                    self.running = false;
                    self.result = Some(text);
                    self.exit_code = code;
                    self.progress = None;
                    finished = true;
                }
                Ok(GuiMsg::Error(err)) => {
                    self.running = false;
                    self.error = Some(err);
                    self.progress = None;
                    finished = true;
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    if !finished {
                        self.running = false;
                        self.error = Some("The benchmark thread stopped unexpectedly.".to_string());
                        self.progress = None;
                    }
                    return;
                }
            }
        }
        if !finished {
            self.rx = Some(rx);
        }
    }

    /// Build a Config from the current UI state.
    fn build_config(&self) -> Config {
        Config {
            doh: self.doh,
            tcp: self.tcp,
            timeout: Duration::from_millis(
                self.timeout_ms
                    .trim()
                    .parse::<u64>()
                    .unwrap_or(1000)
                    .clamp(100, 60_000),
            ),
            jobs: self.jobs.trim().parse::<usize>().unwrap_or(16).clamp(1, 64),
            repeat: self.repeat.trim().parse::<usize>().unwrap_or(1).clamp(1, 5),
            family: match self.family {
                Family::Any => cli::Family::Any,
                Family::V4 => cli::Family::V4,
                Family::V6 => cli::Family::V6,
            },
            format: match self.format {
                OutputFormat::Table => cli::OutputFormat::Table,
                OutputFormat::Json => cli::OutputFormat::Json,
                OutputFormat::Csv => cli::OutputFormat::Csv,
                OutputFormat::Quiet => cli::OutputFormat::Quiet,
            },
            verbose: self.verbose,
            detect: self.detect,
            nearby: self.nearby,
            host: self
                .hosts
                .lines()
                .map(|l| l.trim().to_string())
                .filter(|l| !l.is_empty())
                .collect(),
            ..Default::default()
        }
    }

    /// Start the benchmark in a background thread.
    fn start_benchmark(&mut self) {
        let cfg = self.build_config();
        let (tx, rx) = mpsc::channel();
        self.rx = Some(rx);
        self.running = true;
        self.result = None;
        self.error = None;
        self.progress = None;

        let tx_progress = tx.clone();
        thread::spawn(move || {
            let runtime = match tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
            {
                Ok(rt) => rt,
                Err(e) => {
                    let _ = tx.send(GuiMsg::Error(format!("cannot start async runtime: {e}")));
                    return;
                }
            };

            let (text, exit_code) =
                runtime.block_on(run_gui_benchmark(cfg, move |done, total, elapsed| {
                    let _ = tx_progress.send(GuiMsg::Progress(done, total, elapsed));
                }));

            let _ = tx.send(GuiMsg::Finished(text, exit_code));
        });
    }
}

impl eframe::App for DnsRaceGui {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_messages();
        if self.running {
            ctx.request_repaint_after(Duration::from_millis(200));
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();

        egui::CentralPanel::default().show(ui, |ui| {
            ui.heading("dnsrace");
            ui.label("Race DNS servers from your connection and find the fastest one.");
            ui.separator();

            egui::Grid::new("settings").show(ui, |ui| {
                ui.label("Hostnames (one per line, optional):");
                ui.text_edit_multiline(&mut self.hosts);
                ui.end_row();

                ui.label("Timeout (ms):");
                ui.text_edit_singleline(&mut self.timeout_ms);
                ui.end_row();

                ui.label("Concurrent servers (jobs):");
                ui.text_edit_singleline(&mut self.jobs);
                ui.end_row();

                ui.label("Repeat (1-5):");
                ui.text_edit_singleline(&mut self.repeat);
                ui.end_row();

                ui.label("Address family:");
                ui.radio_value(&mut self.family, Family::Any, "IPv4 + IPv6");
                ui.radio_value(&mut self.family, Family::V4, "IPv4 only");
                ui.radio_value(&mut self.family, Family::V6, "IPv6 only");
                ui.end_row();

                ui.label("Query over TCP:");
                ui.checkbox(&mut self.tcp, "Use TCP instead of UDP");
                ui.end_row();

                ui.label("DNS-over-HTTPS:");
                ui.checkbox(&mut self.doh, "Include Cloudflare and Google DoH");
                ui.end_row();

                ui.label("Detect system DNS/router:");
                ui.checkbox(&mut self.detect, "Auto-detect router and current DNS");
                ui.end_row();

                ui.label("Test nearby resolvers:");
                ui.checkbox(&mut self.nearby, "Also test resolvers for your country");
                ui.end_row();

                ui.label("Verbose:");
                ui.checkbox(&mut self.verbose, "Show spread and DNSSEC details");
                ui.end_row();

                ui.label("Output format:");
                ui.radio_value(&mut self.format, OutputFormat::Table, "Table");
                ui.radio_value(&mut self.format, OutputFormat::Json, "JSON");
                ui.radio_value(&mut self.format, OutputFormat::Csv, "CSV");
                ui.radio_value(
                    &mut self.format,
                    OutputFormat::Quiet,
                    "Quiet (address only)",
                );
                ui.end_row();
            });

            ui.separator();

            let run_enabled = !self.running;
            if ui
                .add_enabled(run_enabled, egui::Button::new("Run Benchmark"))
                .clicked()
            {
                self.start_benchmark();
            }

            if self.running {
                if let Some((done, total, elapsed)) = self.progress {
                    let pct = done.saturating_mul(100) / total.max(1);
                    let fraction = pct as f32 / 100.0_f32;
                    ui.add(egui::ProgressBar::new(fraction).text(format!(
                        "Testing {done}/{total} servers ({pct}%) - {elapsed:.1}s"
                    )));
                } else {
                    ui.spinner();
                    ui.label("Starting benchmark...");
                }
            }

            if self.error.is_some() {
                let err = self.error.clone().unwrap();
                ui.separator();
                egui::Window::new("Error")
                    .collapsible(false)
                    .resizable(false)
                    .show(&ctx, |ui| {
                        ui.colored_label(egui::Color32::RED, &err);
                        if ui.button("Dismiss").clicked() {
                            self.error = None;
                        }
                    });
            }

            if let Some(text) = &self.result {
                ui.separator();
                if self.exit_code == 0 {
                    ui.label("Results:");
                } else {
                    ui.colored_label(
                        egui::Color32::RED,
                        format!(
                            "Benchmark did not finish successfully (exit code {}).",
                            self.exit_code
                        ),
                    );
                }
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.monospace(text);
                });
                ui.separator();
                if ui.button("Copy to clipboard").clicked() {
                    ctx.copy_text(text.clone());
                }
            }
        });
    }
}

/// Runs the benchmark logic for the GUI, reporting progress via a callback.
async fn run_gui_benchmark<F>(cfg: Config, mut on_progress: F) -> (String, u8)
where
    F: FnMut(usize, usize, f64) + Send + 'static,
{
    use std::collections::HashSet;
    use std::time::Instant;

    use tokio::task::JoinSet;

    use crate::bench;
    use crate::cli::OutputFormat;
    use crate::servers::{self, CURRENT, CUSTOM, OTHER, Pool, ROUTER};
    use crate::system;
    use crate::util;

    fn failed(message: impl Into<String>) -> (String, u8) {
        (message.into(), 1)
    }

    let started = Instant::now();
    util::seed_rng();

    let list_text = match &cfg.servers_file {
        Some(path) => match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(e) => {
                return failed(format!(
                    "Could not read the server list {}: {e}",
                    path.display()
                ));
            }
        },
        None => servers::BUILTIN.to_string(),
    };
    let listed = servers::parse_servers(&list_text);

    let data_center = if cfg.detect {
        bench::cloudflare_data_center().await
    } else {
        None
    };
    let sys = tokio::task::spawn_blocking(system::detect)
        .await
        .unwrap_or_default();
    let nearby_cc = if cfg.nearby {
        tokio::task::spawn_blocking(servers::fetch_country_code)
            .await
            .unwrap_or(None)
    } else {
        None
    };

    let mut countries = cfg.countries.clone();
    if let Some(cc) = &nearby_cc
        && !countries.contains(cc)
    {
        countries.push(cc.clone());
    }
    countries.sort();
    countries.dedup();

    let mut lists = Vec::new();
    if !countries.is_empty() {
        let mut set = JoinSet::new();
        for cc in countries {
            let cc = cc.clone();
            set.spawn_blocking(move || {
                let result = servers::fetch_country(&cc);
                (cc, result)
            });
        }
        while let Some(res) = set.join_next().await {
            if let Ok(item) = res {
                lists.push(item);
            }
        }
        lists.sort_by(|a, b| a.0.cmp(&b.0));
    }

    let family_allows = |addr: SocketAddr| match cfg.family {
        cli::Family::Any => true,
        cli::Family::V4 => addr.is_ipv4(),
        cli::Family::V6 => addr.is_ipv6(),
    };

    let mut pool = Pool::default();
    for c in &listed {
        if family_allows(c.addr) {
            pool.add(c.addr, &c.name);
        }
    }
    let current_ips: Vec<std::net::IpAddr> = sys
        .dns
        .iter()
        .copied()
        .filter(|ip| family_allows(SocketAddr::new(*ip, 53)))
        .collect();
    if let Some(gateway) = sys.gateway
        && family_allows(SocketAddr::new(gateway, 53))
    {
        pool.add(SocketAddr::new(gateway, 53), ROUTER);
    }
    for &ip in &current_ips {
        pool.add(SocketAddr::new(ip, 53), CURRENT);
    }
    for &addr in &cfg.add {
        if family_allows(addr) {
            pool.add(addr, CUSTOM);
        }
    }

    for (_cc, result) in lists {
        if let Ok(ips) = result {
            for ip in ips {
                let addr = SocketAddr::new(ip, 53);
                if family_allows(addr) {
                    pool.add(addr, OTHER);
                }
            }
        }
    }

    if cfg.doh {
        for provider in crate::dns::DohProvider::all() {
            pool.push_raw(crate::servers::Candidate {
                addr: SocketAddr::from(([0, 0, 0, 0], 53)),
                name: format!("{} (DoH)", provider.name()),
                doh_provider: Some(provider),
            });
        }
    }

    if pool.is_empty() {
        return failed(
            "No servers to test. The server list is empty or every server was filtered out by the address-family setting.",
        );
    }

    let scan_wait = cfg.timeout.min(Duration::from_secs(3));
    let scan = bench::quick_scan(pool.items(), scan_wait, cfg.tcp).await;
    if scan.is_empty() {
        return failed(
            "No server answered the quick scan. Check your connection and whether UDP/TCP port 53 is blocked by a firewall or VPN.",
        );
    }
    on_progress(scan.len(), scan.len(), started.elapsed().as_secs_f64());

    let short = bench::shortlist(&scan, cfg.known, cfg.other, &current_ips);
    if short.servers.is_empty() {
        return failed(
            "No servers were selected for the full test. Check --known/--other or your server list.",
        );
    }

    let stage2_started = Instant::now();
    let rows = bench::full_tests(
        short.servers,
        cfg.jobs,
        cfg.timeout,
        cfg.repeat,
        cfg.tcp,
        |done, total| {
            on_progress(done, total, stage2_started.elapsed().as_secs_f64());
        },
    )
    .await;

    let steerings =
        if cfg.host.is_empty() || !matches!(cfg.format, OutputFormat::Table | OutputFormat::Json) {
            Vec::new()
        } else {
            let want_v6 = matches!(cfg.family, cli::Family::Any | cli::Family::V6);
            bench::steerings(&cfg.host, &rows, cfg.connect_port, want_v6).await
        };

    let current: HashSet<std::net::IpAddr> = current_ips.iter().copied().collect();
    let json_output = report::json(
        env!("CARGO_PKG_VERSION"),
        data_center.as_deref(),
        &rows,
        &current,
        &steerings,
    );
    let csv_output = report::csv(&rows, &current);

    let mut out = String::new();
    match cfg.format {
        OutputFormat::Json => out.push_str(&json_output),
        OutputFormat::Csv => out.push_str(&csv_output),
        OutputFormat::Quiet => match report::recommended(&rows) {
            Some(r) => out.push_str(&crate::servers::fmt_addr(r.addr)),
            None => {
                return failed(
                    "No healthy server to recommend. Try again, raise the timeout, or use --repeat.",
                );
            }
        },
        OutputFormat::Table => {
            out.push_str(&report::table(&rows, &current, false));
            out.push('\n');
            out.push_str(&report::summary(&rows, &current, false, cfg.repeat));
            if cfg.verbose {
                out.push('\n');
                out.push_str(&report::details(&rows));
            }
            for steering in &steerings {
                out.push('\n');
                out.push_str(&report::steering_text(steering));
            }
            out.push_str(&format!(
                "\n\nDone in {:.1}s.",
                started.elapsed().as_secs_f64()
            ));
        }
    }

    (out, 0)
}

/// Entry point for the GUI binary.
pub async fn run_gui(_cfg: Config) -> u8 {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1000.0, 760.0])
            .with_min_inner_size([700.0, 480.0]),
        ..Default::default()
    };
    match eframe::run_native(
        "dnsrace",
        options,
        Box::new(|_cc| Ok(Box::new(DnsRaceGui::new()))),
    ) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("dnsrace: could not start the graphical interface: {e}");
            1
        }
    }
}
