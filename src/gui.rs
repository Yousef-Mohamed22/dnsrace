//! The native desktop interface.
//!
//! The layout is intentionally simple: a server picker on the left, a toolbar on top, and the
//! results plus apply/export actions on the right. All long operations run on background
//! threads so the window keeps painting while DNS queries time out or an administrator prompt
//! is open.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};
use std::net::IpAddr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering as AtomicOrdering};
use std::sync::{Arc, mpsc};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use eframe::egui::{self, Align, Color32, Layout, RichText, Stroke, Vec2};
use egui_extras::{Column, TableBuilder};

use crate::bench::{Note, Row, SecurityStatus, Steering};
use crate::config::{self, CustomResolver, HistoryEntry, Settings, Theme};
use crate::engine::{self, RunEvent, RunRequest, RunResult, Stage};
use crate::platform::{self, AppliedDns};
use crate::providers;
use crate::report;
use crate::servers::{self, Candidate, Category, Family, Kind, Pool, Protocol};
use crate::system::SystemInfo;

const VERSION: &str = env!("CARGO_PKG_VERSION");
const ACCENT: Color32 = Color32::from_rgb(105, 167, 255);
const GREEN: Color32 = Color32::from_rgb(78, 214, 161);
const YELLOW: Color32 = Color32::from_rgb(241, 197, 107);
const RED: Color32 = Color32::from_rgb(255, 123, 123);

/// Starts the graphical application. There is no terminal/CLI mode in 0.7.0.
pub fn run() -> eframe::Result {
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("dnsrace")
        .with_app_id("io.github.yousef_mohamed22.dnsrace")
        .with_inner_size([1240.0, 800.0])
        .with_min_inner_size([1000.0, 620.0]);
    if let Ok(icon) = eframe::icon_data::from_png_bytes(include_bytes!("../packaging/icon-256.png"))
    {
        viewport = viewport.with_icon(icon);
    }
    let options = eframe::NativeOptions {
        viewport,
        centered: true,
        ..Default::default()
    };
    eframe::run_native(
        "dnsrace",
        options,
        Box::new(|_cc| Ok(Box::new(DnsRaceGui::new()))),
    )
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SortKey {
    Score,
    Cached,
    Uncached,
    Provider,
    Endpoint,
    Security,
    Health,
}

impl SortKey {
    const ALL: [SortKey; 7] = [
        SortKey::Score,
        SortKey::Cached,
        SortKey::Uncached,
        SortKey::Provider,
        SortKey::Endpoint,
        SortKey::Security,
        SortKey::Health,
    ];

    const fn label(self) -> &'static str {
        match self {
            SortKey::Score => "Score",
            SortKey::Cached => "Cached latency",
            SortKey::Uncached => "Uncached latency",
            SortKey::Provider => "Provider",
            SortKey::Endpoint => "Server",
            SortKey::Security => "DNSSEC",
            SortKey::Health => "Health",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum HealthFilter {
    #[default]
    All,
    Healthy,
    Issues,
}

impl HealthFilter {
    const ALL: [HealthFilter; 3] = [
        HealthFilter::All,
        HealthFilter::Healthy,
        HealthFilter::Issues,
    ];

    const fn label(self) -> &'static str {
        match self {
            HealthFilter::All => "All health states",
            HealthFilter::Healthy => "Healthy only",
            HealthFilter::Issues => "With issues",
        }
    }

    fn allows(self, row: &Row) -> bool {
        match self {
            HealthFilter::All => true,
            HealthFilter::Healthy => row.healthy(),
            HealthFilter::Issues => !row.healthy() || row.note != Note::Ok,
        }
    }
}

const CATEGORIES: [Category; 7] = [
    Category::General,
    Category::Security,
    Category::Family,
    Category::Privacy,
    Category::Uncensored,
    Category::System,
    Category::Custom,
];

/// The colours for one theme preset. `dark` picks the egui base style; the rest replace the
/// parts of `Visuals` that decide how panels and cards look.
#[derive(Clone, Copy)]
struct Palette {
    dark: bool,
    bg: Color32,
    panel: Color32,
    border: Color32,
    hover: Color32,
    selection: Color32,
    text: Color32,
}

fn palette_for(theme: Theme, active: egui::Theme) -> Palette {
    let theme = if theme == Theme::System {
        if active == egui::Theme::Dark {
            Theme::Dark
        } else {
            Theme::Light
        }
    } else {
        theme
    };
    match theme {
        Theme::Light => Palette {
            dark: false,
            bg: Color32::from_rgb(241, 244, 249),
            panel: Color32::from_rgb(255, 255, 255),
            border: Color32::from_rgb(215, 221, 230),
            hover: Color32::from_rgb(236, 241, 248),
            selection: Color32::from_rgb(184, 208, 240),
            text: Color32::from_rgb(25, 32, 44),
        },
        Theme::Midnight => Palette {
            dark: true,
            bg: Color32::from_rgb(8, 12, 24),
            panel: Color32::from_rgb(15, 21, 38),
            border: Color32::from_rgb(33, 44, 68),
            hover: Color32::from_rgb(25, 34, 56),
            selection: Color32::from_rgb(38, 74, 140),
            text: Color32::from_rgb(220, 230, 250),
        },
        Theme::Nord => Palette {
            dark: true,
            bg: Color32::from_rgb(46, 52, 64),
            panel: Color32::from_rgb(59, 66, 82),
            border: Color32::from_rgb(76, 86, 106),
            hover: Color32::from_rgb(67, 76, 94),
            selection: Color32::from_rgb(94, 129, 172),
            text: Color32::from_rgb(236, 239, 244),
        },
        Theme::SolarizedLight => Palette {
            dark: false,
            bg: Color32::from_rgb(253, 246, 227),
            panel: Color32::from_rgb(238, 232, 213),
            border: Color32::from_rgb(211, 203, 184),
            hover: Color32::from_rgb(232, 225, 205),
            selection: Color32::from_rgb(181, 203, 224),
            text: Color32::from_rgb(88, 110, 117),
        },
        Theme::Dracula => Palette {
            dark: true,
            bg: Color32::from_rgb(40, 42, 54),
            panel: Color32::from_rgb(45, 47, 61),
            border: Color32::from_rgb(68, 71, 90),
            hover: Color32::from_rgb(56, 58, 75),
            selection: Color32::from_rgb(98, 70, 160),
            text: Color32::from_rgb(248, 248, 242),
        },
        Theme::HighContrast => Palette {
            dark: true,
            bg: Color32::BLACK,
            panel: Color32::from_rgb(8, 8, 8),
            border: Color32::from_rgb(150, 150, 150),
            hover: Color32::from_rgb(28, 28, 28),
            selection: Color32::from_rgb(0, 90, 200),
            text: Color32::WHITE,
        },
        Theme::System | Theme::Dark => Palette {
            dark: true,
            bg: Color32::from_rgb(14, 17, 22),
            panel: Color32::from_rgb(22, 27, 35),
            border: Color32::from_rgb(43, 51, 64),
            hover: Color32::from_rgb(34, 42, 54),
            selection: Color32::from_rgb(40, 91, 158),
            text: Color32::from_rgb(232, 237, 245),
        },
    }
}

#[derive(Clone)]
struct CustomDraft {
    name: String,
    protocol: Protocol,
    endpoint: String,
    address: String,
    error: Option<String>,
}

impl Default for CustomDraft {
    fn default() -> Self {
        Self {
            name: String::new(),
            protocol: Protocol::Doh,
            endpoint: String::new(),
            address: String::new(),
            error: None,
        }
    }
}

struct Notice {
    text: String,
    error: bool,
}

#[derive(Clone)]
struct PendingApply {
    label: String,
    addresses: Vec<IpAddr>,
    warning: Option<String>,
}

/// The whole application state. Results are kept as structured rows; the UI never parses text
/// to decide what to show.
pub struct DnsRaceGui {
    settings: Settings,
    catalog: Vec<Candidate>,
    system: SystemInfo,
    current_dns: Vec<IpAddr>,
    gateway: Option<IpAddr>,
    data_center: Option<String>,
    history: Vec<HistoryEntry>,
    applied: Option<AppliedDns>,

    result: Option<RunResult>,
    live_rows: Vec<Row>,
    chosen_id: Option<String>,
    running: bool,
    stage: Stage,
    progress: (usize, usize),
    started: Option<Instant>,
    cancel: Arc<AtomicBool>,
    events: Option<mpsc::Receiver<RunEvent>>,

    detect_rx: Option<mpsc::Receiver<SystemInfo>>,
    detecting: bool,
    detected_once: bool,
    detect_notify: bool,

    apply_rx: Option<mpsc::Receiver<Result<AppliedDns, String>>>,
    apply_running: bool,
    revert_rx: Option<mpsc::Receiver<Result<(), String>>>,
    revert_running: bool,
    pending_apply: Option<PendingApply>,
    confirm_revert: bool,

    search: String,
    result_filter: String,
    server_category: Option<Category>,
    result_category: Option<Category>,
    health_filter: HealthFilter,
    sort: SortKey,
    sort_descending: bool,
    notice: Option<Notice>,
    theme_applied: Option<(Theme, egui::Theme)>,

    show_settings: bool,
    show_custom: bool,
    show_history: bool,
    show_about: bool,
    custom: CustomDraft,
    export_dir_text: String,
}

impl DnsRaceGui {
    fn new() -> Self {
        let settings = config::load_settings();
        let history = config::load_history();
        let applied = config::load_applied();
        let export_dir_text = settings
            .export_dir
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_default();
        let mut app = Self {
            settings,
            catalog: Vec::new(),
            system: SystemInfo::default(),
            current_dns: Vec::new(),
            gateway: None,
            data_center: None,
            history,
            applied,
            result: None,
            live_rows: Vec::new(),
            chosen_id: None,
            running: false,
            stage: Stage::Scan,
            progress: (0, 0),
            started: None,
            cancel: Arc::new(AtomicBool::new(false)),
            events: None,
            detect_rx: None,
            detecting: false,
            detected_once: false,
            detect_notify: true,
            apply_rx: None,
            apply_running: false,
            revert_rx: None,
            revert_running: false,
            pending_apply: None,
            confirm_revert: false,
            search: String::new(),
            result_filter: String::new(),
            server_category: None,
            result_category: None,
            health_filter: HealthFilter::All,
            sort: SortKey::Score,
            sort_descending: false,
            notice: None,
            theme_applied: None,
            show_settings: false,
            show_custom: false,
            show_history: false,
            show_about: false,
            custom: CustomDraft::default(),
            export_dir_text,
        };
        app.rebuild_catalog();
        // Detection runs a few commands and can take a moment; never block the first frame.
        app.start_detection();
        app
    }

    fn rebuild_catalog(&mut self) {
        let mut pool = Pool::default();
        let catalog = providers::catalog(self.settings.protocol, self.settings.family);
        if self.settings.include_system && self.settings.protocol.is_plain() {
            for candidate in providers::system_candidates(&self.system, &catalog) {
                if family_allows(&candidate, self.settings.family) {
                    pool.add(candidate);
                }
            }
        }
        for candidate in catalog {
            pool.add(candidate);
        }
        for custom in &self.settings.custom {
            let Ok(candidate) = custom_candidate(custom) else {
                continue;
            };
            let kind_ok = match self.settings.protocol {
                Protocol::Udp | Protocol::Tcp => candidate.kind() == Kind::Plain,
                Protocol::Dot => candidate.kind() == Kind::Dot,
                Protocol::Doh => candidate.kind() == Kind::Doh,
            };
            if kind_ok && family_allows(&candidate, self.settings.family) {
                pool.add(candidate);
            }
        }
        self.catalog = pool.into_items();
    }

    fn start_detection(&mut self) {
        self.start_detection_with_notice(true);
    }

    /// Refreshes detection without replacing the current notice, so an apply/revert message
    /// stays visible.
    fn start_detection_quietly(&mut self) {
        self.start_detection_with_notice(false);
    }

    fn start_detection_with_notice(&mut self, notify: bool) {
        if self.detecting {
            self.detect_notify |= notify;
            return;
        }
        let (tx, rx) = mpsc::channel();
        self.detect_rx = Some(rx);
        self.detecting = true;
        self.detect_notify = notify;
        let spawned = thread::Builder::new()
            .name("dnsrace-detect".into())
            .spawn(move || {
                let _ = tx.send(engine::detect_system());
            });
        if let Err(error) = spawned {
            self.detecting = false;
            self.detect_rx = None;
            self.notify(format!("Could not start DNS detection: {error}"), true);
        }
    }

    fn poll_detection(&mut self, ctx: &egui::Context) {
        let Some(rx) = self.detect_rx.take() else {
            return;
        };
        match rx.try_recv() {
            Ok(info) => {
                self.detecting = false;
                self.apply_system_info(info);
            }
            Err(mpsc::TryRecvError::Empty) => {
                self.detect_rx = Some(rx);
                ctx.request_repaint_after(Duration::from_millis(120));
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                self.detecting = false;
                self.notify("DNS detection ended unexpectedly.", true);
            }
        }
    }

    fn apply_system_info(&mut self, info: SystemInfo) {
        let count = info.dns.len();
        self.current_dns = info.dns.clone();
        self.gateway = info.gateway;
        self.system = info;
        if self.settings.include_system {
            self.rebuild_catalog();
        }
        if self.detected_once && self.detect_notify {
            let message = if count == 0 {
                "Refreshed: no system DNS servers were detected.".to_string()
            } else {
                format!(
                    "Refreshed: {} DNS server{} detected.",
                    count,
                    if count == 1 { "" } else { "s" }
                )
            };
            self.notify(message, false);
        }
        self.detected_once = true;
    }

    fn save_settings(&mut self) {
        if let Err(error) = config::save_settings(&self.settings) {
            self.notify(format!("Could not save settings: {error}"), true);
        }
    }

    fn notify(&mut self, text: impl Into<String>, error: bool) {
        self.notice = Some(Notice {
            text: text.into(),
            error,
        });
    }

    fn selected_in_catalog(&self) -> usize {
        self.catalog
            .iter()
            .filter(|candidate| self.settings.selected.contains(&candidate.id))
            .count()
    }

    fn approve_theme(&mut self, ctx: &egui::Context) {
        let preference = match self.settings.theme {
            Theme::System => egui::ThemePreference::System,
            theme if theme.is_dark() => egui::ThemePreference::Dark,
            _ => egui::ThemePreference::Light,
        };
        ctx.set_theme(preference);
        let active = ctx.theme();
        if self.theme_applied == Some((self.settings.theme, active)) {
            return;
        }
        let palette = palette_for(self.settings.theme, active);
        let mut visuals = if palette.dark {
            egui::Visuals::dark()
        } else {
            egui::Visuals::light()
        };
        visuals.panel_fill = palette.bg;
        visuals.window_fill = palette.panel;
        visuals.faint_bg_color = palette.panel;
        visuals.extreme_bg_color = palette.bg;
        visuals.code_bg_color = palette.bg;
        visuals.override_text_color = Some(palette.text);
        visuals.hyperlink_color = ACCENT;
        visuals.selection.bg_fill = palette.selection;
        visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, palette.border);
        visuals.widgets.inactive.weak_bg_fill = palette.panel;
        visuals.widgets.inactive.bg_fill = palette.panel;
        visuals.widgets.hovered.weak_bg_fill = palette.hover;
        visuals.widgets.active.weak_bg_fill = palette.hover;
        ctx.set_visuals_of(active, visuals);
        ctx.style_mut_of(active, |style| {
            style.spacing.item_spacing = Vec2::new(8.0, 8.0);
            style.spacing.button_padding = Vec2::new(10.0, 6.0);
            style.spacing.interact_size.y = 24.0;
        });
        self.theme_applied = Some((self.settings.theme, active));
    }

    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        let (refresh, run_selected, run_all, cancel, export) = ctx.input(|input| {
            let command = input.modifiers.command;
            (
                command && input.key_pressed(egui::Key::R) || input.key_pressed(egui::Key::F5),
                command && !input.modifiers.shift && input.key_pressed(egui::Key::Enter),
                command && input.modifiers.shift && input.key_pressed(egui::Key::Enter),
                input.key_pressed(egui::Key::Escape),
                command && input.key_pressed(egui::Key::E),
            )
        });
        if refresh {
            self.start_detection();
        }
        if run_all {
            self.start_run(true);
        } else if run_selected {
            self.start_run(false);
        }
        if cancel {
            self.cancel_run();
        }
        if export && !self.running {
            self.export("html");
        }
    }

    fn poll_events(&mut self, ctx: &egui::Context) {
        let Some(rx) = self.events.take() else {
            return;
        };
        let mut keep = true;
        loop {
            match rx.try_recv() {
                Ok(event) => {
                    let terminal = matches!(
                        event,
                        RunEvent::Finished(_) | RunEvent::Cancelled(_) | RunEvent::Error(_)
                    );
                    self.handle_event(event);
                    if terminal {
                        keep = false;
                        break;
                    }
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    keep = false;
                    if self.running {
                        self.running = false;
                        self.notify("The benchmark ended unexpectedly. Try again.", true);
                    }
                    break;
                }
            }
        }
        if keep {
            self.events = Some(rx);
        }
        if self.running {
            ctx.request_repaint_after(Duration::from_millis(80));
        }
    }

    fn handle_event(&mut self, event: RunEvent) {
        match event {
            RunEvent::Detected {
                current_dns,
                gateway,
                data_center,
            } => {
                self.current_dns = current_dns.clone();
                self.system.dns = current_dns;
                self.gateway = gateway;
                self.system.gateway = gateway;
                if self.settings.include_system {
                    self.rebuild_catalog();
                }
                if data_center.is_some() {
                    self.data_center = data_center;
                }
            }
            RunEvent::Stage { stage, done, total } => {
                self.stage = stage;
                self.progress = (done, total);
            }
            RunEvent::Row(row) => {
                if !self.live_rows.iter().any(|existing| existing.id == row.id) {
                    self.live_rows.push(row);
                }
            }
            RunEvent::Finished(result) => {
                self.running = false;
                self.stage = Stage::Done;
                self.progress = (result.tested, result.tested);
                self.chosen_id = result.best().map(|row| row.id.clone());
                self.live_rows = result.rows.clone();
                let entry = HistoryEntry {
                    timestamp: now(),
                    protocol: result.protocol,
                    family: result.family,
                    duration_secs: result.duration_secs,
                    data_center: result.data_center.clone(),
                    current_dns: result.current_dns.clone(),
                    rows: result.rows.clone(),
                    steerings: result.steerings.clone(),
                    scanned: result.scanned,
                    tested: result.tested,
                };
                match config::push_history(self.history.clone(), entry) {
                    Ok(history) => self.history = history,
                    Err(error) => self.notify(format!("Could not save history: {error}"), true),
                }
                let message = match result.best() {
                    Some(row) => {
                        format!("Finished: {} is fastest at {:.1} ms.", row.label, row.score)
                    }
                    None => "Finished, but no resolver completed enough measurements.".to_string(),
                };
                self.notify(message, false);
                self.result = Some(result);
            }
            RunEvent::Cancelled(result) => {
                self.running = false;
                self.stage = Stage::Done;
                self.live_rows = result.rows.clone();
                if result.rows.is_empty() {
                    self.result = None;
                } else {
                    self.chosen_id = result.best().map(|row| row.id.clone());
                    self.result = Some(result);
                }
                self.notify("Benchmark cancelled. Partial results are shown.", false);
            }
            RunEvent::Error(error) => {
                self.running = false;
                self.stage = Stage::Done;
                self.notify(error, true);
            }
        }
    }

    fn poll_apply(&mut self, ctx: &egui::Context) {
        if let Some(rx) = self.apply_rx.take() {
            match rx.try_recv() {
                Ok(Ok(applied)) => {
                    self.apply_running = false;
                    if let Err(error) = config::save_applied(&applied) {
                        self.notify(
                            format!(
                                "DNS was applied, but the revert information could not be saved: {error}"
                            ),
                            true,
                        );
                    } else {
                        self.notify(format!("Applied {}.", applied.summary()), false);
                    }
                    // Reflect the change immediately, then confirm it with a quiet detection so
                    // the IN USE badge and current-DNS text never describe the old settings.
                    self.current_dns = applied.applied.clone();
                    self.system.dns = applied.applied.clone();
                    self.system.interface = Some(applied.interface.clone());
                    self.system.source = Some(format!("Applied via {}", applied.method));
                    if self.settings.include_system {
                        self.rebuild_catalog();
                    }
                    self.applied = Some(applied);
                    self.start_detection_quietly();
                }
                Ok(Err(error)) => {
                    self.apply_running = false;
                    self.notify(format!("Could not apply DNS: {error}"), true);
                }
                Err(mpsc::TryRecvError::Empty) => {
                    self.apply_rx = Some(rx);
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.apply_running = false;
                    self.notify("The apply operation ended unexpectedly.", true);
                }
            }
        }
        if let Some(rx) = self.revert_rx.take() {
            match rx.try_recv() {
                Ok(Ok(())) => {
                    self.revert_running = false;
                    if let Err(error) = config::clear_applied() {
                        self.notify(
                            format!("DNS was restored, but state cleanup failed: {error}"),
                            true,
                        );
                    } else {
                        self.notify("Previous DNS settings restored.", false);
                    }
                    self.applied = None;
                    self.start_detection_quietly();
                }
                Ok(Err(error)) => {
                    self.revert_running = false;
                    self.notify(format!("Could not restore DNS: {error}"), true);
                }
                Err(mpsc::TryRecvError::Empty) => {
                    self.revert_rx = Some(rx);
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.revert_running = false;
                    self.notify("The revert operation ended unexpectedly.", true);
                }
            }
        }
        if self.apply_running || self.revert_running {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
    }

    fn start_run(&mut self, all: bool) {
        if self.running {
            return;
        }
        let pool: Vec<Candidate> = if all {
            self.catalog.clone()
        } else {
            self.catalog
                .iter()
                .filter(|candidate| self.settings.selected.contains(&candidate.id))
                .cloned()
                .collect()
        };
        if pool.is_empty() {
            self.notify(
                if all {
                    "There are no servers in this category. Add a custom resolver or choose another protocol."
                } else {
                    "Select at least one server, or use Test all."
                },
                true,
            );
            return;
        }
        self.launch_run(pool);
    }

    fn start_current_dns_run(&mut self) {
        if self.running {
            return;
        }
        let mut info = self.system.clone();
        info.gateway = None;
        let mut pool = Pool::default();
        for candidate in providers::system_candidates(&info, &[]) {
            pool.add(candidate);
        }
        let pool = pool.into_items();
        if pool.is_empty() {
            self.notify(
                "No current DNS servers are known yet. Use Refresh detection and try again.",
                true,
            );
            return;
        }
        // Detected addresses are plain resolvers, so use UDP regardless of the selected
        // encrypted mode; otherwise every candidate would be skipped.
        self.launch_run_with(pool, Protocol::Udp, Family::Any, false, false);
    }

    fn launch_run(&mut self, pool: Vec<Candidate>) {
        self.launch_run_with(
            pool,
            self.settings.protocol,
            self.settings.family,
            self.settings.include_system,
            self.settings.include_nearby && self.settings.protocol.is_plain(),
        );
    }

    fn launch_run_with(
        &mut self,
        pool: Vec<Candidate>,
        protocol: Protocol,
        family: Family,
        include_system: bool,
        include_nearby: bool,
    ) {
        if self.running {
            return;
        }
        let request = RunRequest {
            candidates: pool,
            protocol,
            family,
            wait: self.settings.timeout(),
            jobs: self.settings.jobs(),
            repeat: self.settings.repeat(),
            hostnames: self.settings.hostnames(),
            connect_port: self.settings.connect_port.max(1),
            include_system,
            include_nearby,
        };
        let (tx, rx) = mpsc::channel();
        self.events = Some(rx);
        self.cancel = Arc::new(AtomicBool::new(false));
        let cancel = self.cancel.clone();
        self.result = None;
        self.live_rows.clear();
        self.chosen_id = None;
        self.notice = None;
        self.stage = Stage::Scan;
        self.progress = (0, 0);
        self.started = Some(Instant::now());
        self.running = true;
        let spawned = thread::Builder::new()
            .name("dnsrace-benchmark".into())
            .spawn(move || {
                let runtime = tokio::runtime::Builder::new_multi_thread()
                    .enable_all()
                    .build();
                match runtime {
                    Ok(runtime) => runtime.block_on(engine::run(request, tx, cancel)),
                    Err(error) => {
                        let _ = tx.send(RunEvent::Error(format!(
                            "Could not start the benchmark runtime: {error}"
                        )));
                    }
                }
            });
        if let Err(error) = spawned {
            self.running = false;
            self.events = None;
            self.notify(
                format!("Could not start the benchmark thread: {error}"),
                true,
            );
        }
    }

    fn cancel_run(&mut self) {
        if self.running {
            self.cancel.store(true, AtomicOrdering::Relaxed);
            self.notify("Cancelling…", false);
        }
    }

    fn apply_candidates(&self) -> Vec<Candidate> {
        let mut list = providers::catalog(Protocol::Udp, Family::Any);
        for custom in &self.settings.custom {
            if let Ok(candidate) = custom_candidate(custom)
                && candidate.kind() == Kind::Plain
            {
                list.push(candidate);
            }
        }
        let extra = providers::system_candidates(&self.system, &list);
        list.extend(extra);
        list
    }

    fn request_apply(&mut self, row: Row) {
        if !row.is_plain() {
            self.notify(
                "Encrypted DNS (DoT/DoH) cannot be applied system-wide here. Run UDP or TCP and choose a plain resolver.",
                true,
            );
            return;
        }
        if row.security_status() == SecurityStatus::BogusAccepted {
            self.notify(
                "That resolver accepted a deliberately broken DNSSEC signature, so it will not be applied.",
                true,
            );
            return;
        }
        let addresses = engine::apply_addresses(&row, &self.apply_candidates());
        if addresses.is_empty() {
            self.notify("No usable address was found for that resolver.", true);
            return;
        }
        let warning = if row.healthy() {
            None
        } else {
            Some(format!(
                "This resolver had {} failed lookups and is marked “{}”. It can still be applied, but it did not rank as healthy.",
                row.timeouts,
                row.note.label()
            ))
        };
        self.pending_apply = Some(PendingApply {
            label: format!("{} ({})", row.label, row.endpoint),
            addresses,
            warning,
        });
    }

    fn start_apply(&mut self, addresses: Vec<IpAddr>) {
        if self.apply_running {
            return;
        }
        let (tx, rx) = mpsc::channel();
        self.apply_rx = Some(rx);
        self.apply_running = true;
        self.notice = Some(Notice {
            text: "Waiting for permission to change the system DNS settings…".into(),
            error: false,
        });
        let spawned = thread::Builder::new()
            .name("dnsrace-apply".into())
            .spawn(move || {
                let _ = tx.send(platform::apply(&addresses));
            });
        if let Err(error) = spawned {
            self.apply_running = false;
            self.apply_rx = None;
            self.notify(
                format!("Could not start the apply operation: {error}"),
                true,
            );
        }
    }

    fn start_revert(&mut self) {
        if self.revert_running {
            return;
        }
        let Some(applied) = self.applied.clone() else {
            self.notify("There is no saved DNS change to revert.", true);
            return;
        };
        let (tx, rx) = mpsc::channel();
        self.revert_rx = Some(rx);
        self.revert_running = true;
        self.notice = Some(Notice {
            text: "Restoring the previous DNS settings…".into(),
            error: false,
        });
        let spawned = thread::Builder::new()
            .name("dnsrace-revert".into())
            .spawn(move || {
                let _ = tx.send(platform::revert(&applied));
            });
        if let Err(error) = spawned {
            self.revert_running = false;
            self.revert_rx = None;
            self.notify(
                format!("Could not start the revert operation: {error}"),
                true,
            );
        }
    }

    fn export(&mut self, extension: &str) {
        let Some(result) = self.result.clone() else {
            self.notify("Run a benchmark before exporting.", true);
            return;
        };
        let path = config::default_export_path(extension, &self.settings);
        let text = match extension {
            "json" => report::json(&result),
            "csv" => report::csv(&result),
            "html" => report::html(&result),
            _ => return,
        };
        if let Some(parent) = path.parent()
            && let Err(error) = std::fs::create_dir_all(parent)
        {
            self.notify(
                format!("Could not create {}: {error}", parent.display()),
                true,
            );
            return;
        }
        match std::fs::write(&path, text) {
            Ok(()) => self.notify(format!("Saved {}", path.display()), false),
            Err(error) => self.notify(format!("Could not save {}: {error}", path.display()), true),
        }
    }

    fn top_bar(&mut self, ui: &mut egui::Ui) {
        let mut protocol_change = None;
        let mut family_change = None;
        let mut run_all = false;
        let mut run_selected = false;
        let mut cancel = false;
        let mut open_settings = false;
        let mut open_history = false;
        let mut open_about = false;
        let mut refresh_detection = false;
        let mut theme_choice = self.settings.theme;
        let mut theme_change = None;
        let running = self.running;
        let selected = self.selected_in_catalog();

        ui.horizontal(|ui| {
            ui.label(RichText::new("dnsrace").size(25.0).strong().color(ACCENT));
            ui.label(RichText::new(format!("v{VERSION}")).small().weak());
            ui.add_space(14.0);
            for protocol in Protocol::all() {
                let response = ui.add_enabled(
                    !running,
                    egui::Button::selectable(self.settings.protocol == protocol, protocol.label())
                        .min_size(Vec2::new(54.0, 28.0)),
                );
                if response.on_hover_text(protocol.description()).clicked()
                    && !running
                    && self.settings.protocol != protocol
                {
                    protocol_change = Some(protocol);
                }
            }
            ui.separator();
            ui.label(RichText::new("Family").small().weak());
            let mut family = self.settings.family;
            egui::ComboBox::from_id_salt("family")
                .selected_text(family.label())
                .width(118.0)
                .show_ui(ui, |ui| {
                    for option in [Family::Any, Family::V4, Family::V6] {
                        ui.selectable_value(&mut family, option, option.label());
                    }
                });
            if family != self.settings.family {
                family_change = Some(family);
            }

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.button("About").clicked() {
                    open_about = true;
                }
                if ui.button("History").clicked() {
                    open_history = true;
                }
                if ui.button("Settings").clicked() {
                    open_settings = true;
                }
                egui::ComboBox::from_id_salt("theme_quick")
                    .selected_text(theme_choice.label())
                    .width(128.0)
                    .show_ui(ui, |ui| {
                        for theme in Theme::ALL {
                            if ui
                                .selectable_value(&mut theme_choice, theme, theme.label())
                                .on_hover_text(theme.description())
                                .clicked()
                                && theme != self.settings.theme
                            {
                                theme_change = Some(theme);
                            }
                        }
                    });
                ui.add_space(4.0);
                if running {
                    if ui
                        .add(
                            egui::Button::new(RichText::new("Cancel").strong())
                                .fill(RED)
                                .min_size(Vec2::new(84.0, 30.0)),
                        )
                        .clicked()
                    {
                        cancel = true;
                    }
                } else {
                    if ui
                        .add(
                            egui::Button::new(RichText::new("Test all").strong())
                                .fill(ACCENT)
                                .min_size(Vec2::new(92.0, 30.0)),
                        )
                        .clicked()
                    {
                        run_all = true;
                    }
                    if ui
                        .add(
                            egui::Button::new(format!("Test selected ({selected})"))
                                .min_size(Vec2::new(126.0, 30.0)),
                        )
                        .clicked()
                    {
                        run_selected = true;
                    }
                }
            });
        });

        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("Current DNS:").small().weak());
            if self.detecting {
                ui.spinner();
                ui.label(RichText::new("detecting…").small().weak());
            } else if self.current_dns.is_empty() {
                ui.label(RichText::new("not detected").small().color(YELLOW));
            } else {
                ui.label(RichText::new(format_current_dns(&self.current_dns)).small());
            }
            if ui
                .small_button(if self.detecting {
                    "Detecting"
                } else {
                    "Refresh"
                })
                .on_hover_text("Detect the DNS servers this machine uses now (Ctrl+R)")
                .clicked()
            {
                refresh_detection = true;
            }
            if !self.current_dns.is_empty() && !self.detecting {
                if ui
                    .small_button("Test current")
                    .on_hover_text("Benchmark the DNS servers detected on this machine")
                    .clicked()
                {
                    self.start_current_dns_run();
                }
                if ui
                    .small_button("Copy")
                    .on_hover_text("Copy the current DNS addresses")
                    .clicked()
                {
                    ui.ctx().copy_text(format_ip_list(&self.current_dns));
                    self.notify("Current DNS addresses copied.", false);
                }
            }
            if let Some(gateway) = self.gateway {
                ui.add_space(6.0);
                ui.label(RichText::new("Gateway:").small().weak());
                ui.label(RichText::new(gateway.to_string()).small().monospace());
            }
            if let Some(interface) = &self.system.interface {
                ui.add_space(6.0);
                ui.label(RichText::new("Interface:").small().weak());
                ui.label(RichText::new(interface).small().monospace());
            }
            if let Some(dc) = &self.data_center {
                ui.add_space(6.0);
                ui.label(RichText::new("Cloudflare edge:").small().weak());
                ui.label(RichText::new(dc).small().monospace().color(GREEN));
            }
            ui.add_space(6.0);
            ui.label(
                RichText::new(format!("{} of {} selected", selected, self.catalog.len()))
                    .small()
                    .weak(),
            );
            if let Some(applied) = &self.applied {
                ui.add_space(6.0);
                ui.label(
                    RichText::new(format!("DNS applied · {}", applied.interface))
                        .small()
                        .color(GREEN),
                );
            }
        });

        if let Some(protocol) = protocol_change {
            self.settings.protocol = protocol;
            self.rebuild_catalog();
            self.save_settings();
        }
        if let Some(family) = family_change {
            self.settings.family = family;
            self.rebuild_catalog();
            self.save_settings();
        }
        if let Some(theme) = theme_change {
            self.settings.theme = theme;
            self.save_settings();
        }
        if refresh_detection {
            self.start_detection();
        }
        if run_all {
            self.start_run(true);
        }
        if run_selected {
            self.start_run(false);
        }
        if cancel {
            self.cancel_run();
        }
        if open_settings {
            self.show_settings = true;
        }
        if open_history {
            self.show_history = true;
        }
        if open_about {
            self.show_about = true;
        }
    }

    fn sidebar(&mut self, ui: &mut egui::Ui) {
        let catalog = self.catalog.clone();
        let selected = self.settings.selected.clone();
        let current = self.current_dns.clone();
        let query = self.search.trim().to_lowercase();
        let mut refresh = false;
        let mut test_current = false;
        let mut copy_current = false;

        ui.horizontal(|ui| {
            ui.label(RichText::new("Servers").size(17.0).strong());
            ui.label(
                RichText::new(format!(
                    "{} of {} selected",
                    catalog
                        .iter()
                        .filter(|candidate| selected.contains(&candidate.id))
                        .count(),
                    catalog.len()
                ))
                .small()
                .weak(),
            );
        });
        ui.label(
            RichText::new(match self.settings.protocol {
                Protocol::Udp => "Plain DNS on port 53. Fastest and most compatible.",
                Protocol::Tcp => "Plain DNS over TCP. Useful when UDP port 53 is blocked.",
                Protocol::Dot => "Encrypted DNS over TLS on port 853.",
                Protocol::Doh => "Encrypted DNS over HTTPS (RFC 8484).",
            })
            .small()
            .weak(),
        );

        card_frame(ui).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("THIS MACHINE")
                        .size(10.0)
                        .strong()
                        .color(ACCENT),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui
                        .small_button(if self.detecting {
                            "Detecting…"
                        } else {
                            "Refresh"
                        })
                        .on_hover_text("Detect the DNS servers this machine uses now (Ctrl+R)")
                        .clicked()
                    {
                        refresh = true;
                    }
                });
            });
            if self.detecting {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(RichText::new("Detecting system DNS…").small().weak());
                });
            } else if current.is_empty() {
                ui.label(
                    RichText::new("No current DNS detected.")
                        .small()
                        .color(YELLOW),
                );
            } else {
                for ip in &current {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(ip.to_string()).monospace());
                        if let Some(meta) = providers::identify_ip(*ip) {
                            tag(ui, meta.label, category_color(meta.category));
                        }
                    });
                }
            }
            if let Some(gateway) = self.gateway {
                ui.label(RichText::new(format!("Gateway {gateway}")).small().weak());
            }
            if let Some(interface) = &self.system.interface {
                ui.label(
                    RichText::new(format!("Interface {interface}"))
                        .small()
                        .weak(),
                );
            }
            if let Some(source) = &self.system.source {
                ui.label(
                    RichText::new(format!("Detected via {source}"))
                        .small()
                        .weak(),
                )
                .on_hover_text("Where the DNS addresses above were read from.");
            }
            if let Some(dc) = &self.data_center {
                ui.label(
                    RichText::new(format!("Cloudflare edge {dc}"))
                        .small()
                        .weak(),
                );
            }
            if !current.is_empty() && !self.detecting {
                ui.horizontal(|ui| {
                    if ui
                        .small_button("Test current DNS")
                        .on_hover_text("Benchmark only the resolvers detected on this machine")
                        .clicked()
                    {
                        test_current = true;
                    }
                    if ui
                        .small_button("Copy")
                        .on_hover_text("Copy the current DNS addresses")
                        .clicked()
                    {
                        copy_current = true;
                    }
                });
            }
        });
        if refresh {
            self.start_detection();
        }
        if test_current {
            self.start_current_dns_run();
        }
        if copy_current {
            ui.ctx().copy_text(format_ip_list(&current));
            self.notify("Current DNS addresses copied.", false);
        }

        ui.horizontal(|ui| {
            if ui
                .add(
                    egui::Button::new(RichText::new("+ Add resolver").strong())
                        .fill(ACCENT)
                        .min_size(Vec2::new(126.0, 28.0)),
                )
                .clicked()
            {
                self.custom = CustomDraft::default();
                self.show_custom = true;
            }
            if ui.button("Select all").clicked() {
                for candidate in &catalog {
                    self.settings.selected.insert(candidate.id.clone());
                }
                self.save_settings();
            }
            if ui.button("None").clicked() {
                for candidate in &catalog {
                    self.settings.selected.remove(&candidate.id);
                }
                self.save_settings();
            }
        });

        ui.add(
            egui::TextEdit::singleline(&mut self.search)
                .hint_text("Search provider, label or address")
                .desired_width(f32::INFINITY),
        );
        ui.horizontal(|ui| {
            ui.label(RichText::new("Category").small().weak());
            let mut category = self.server_category;
            egui::ComboBox::from_id_salt("server_category")
                .selected_text(category.map_or("All", Category::label))
                .width(150.0)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut category, None, "All categories");
                    for option in CATEGORIES {
                        ui.selectable_value(&mut category, Some(option), option.label());
                    }
                });
            if category != self.server_category {
                self.server_category = category;
            }
        });
        ui.add_space(2.0);

        let mut groups: BTreeMap<String, Vec<usize>> = BTreeMap::new();
        for (index, candidate) in catalog.iter().enumerate() {
            let category_ok = self
                .server_category
                .is_none_or(|category| candidate.category == category);
            if category_ok && (query.is_empty() || matches_filter(candidate, &query)) {
                groups
                    .entry(candidate.provider.clone())
                    .or_default()
                    .push(index);
            }
        }
        let visible: Vec<usize> = groups.values().flatten().copied().collect();
        ui.horizontal(|ui| {
            if ui.small_button("Select visible").clicked() {
                for index in &visible {
                    self.settings.selected.insert(catalog[*index].id.clone());
                }
                self.save_settings();
            }
            if ui.small_button("Clear visible").clicked() {
                for index in &visible {
                    self.settings.selected.remove(&catalog[*index].id);
                }
                self.save_settings();
            }
            if ui.small_button("Default selection").clicked() {
                self.settings.selected = providers::default_selection();
                for candidate in &catalog {
                    if candidate.is_custom {
                        self.settings.selected.insert(candidate.id.clone());
                    }
                }
                self.save_settings();
            }
        });

        let mut toggles: Vec<(String, bool)> = Vec::new();
        egui::ScrollArea::vertical()
            .id_salt("server_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                if groups.is_empty() {
                    ui.add_space(18.0);
                    ui.label(
                        RichText::new("No servers match that search.")
                            .weak()
                            .italics(),
                    );
                    return;
                }
                for (provider, indices) in &groups {
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(provider).strong().size(13.0));
                        ui.label(RichText::new(format!("{}", indices.len())).small().weak());
                    });
                    for index in indices {
                        let candidate = &catalog[*index];
                        let checked = selected.contains(&candidate.id);
                        ui.horizontal(|ui| {
                            let mut on = checked;
                            if ui.checkbox(&mut on, "").changed() {
                                toggles.push((candidate.id.clone(), on));
                            }
                            ui.vertical(|ui| {
                                ui.horizontal_wrapped(|ui| {
                                    let label = if candidate.label == candidate.provider {
                                        candidate.endpoint()
                                    } else {
                                        candidate.label.clone()
                                    };
                                    ui.label(RichText::new(label).strong())
                                        .on_hover_text(format!(
                                            "{}\n{}",
                                            candidate.endpoint(),
                                            if candidate.note.is_empty() {
                                                "No notes for this provider."
                                            } else {
                                                &candidate.note
                                            }
                                        ));
                                    if candidate
                                        .plain_addr()
                                        .is_some_and(|addr| current.contains(&addr.ip()))
                                    {
                                        tag(ui, "CURRENT", GREEN);
                                    }
                                    if candidate.is_custom {
                                        tag(ui, "CUSTOM", Color32::from_rgb(185, 140, 255));
                                    } else if candidate.is_system {
                                        tag(ui, "SYSTEM", YELLOW);
                                    } else {
                                        tag(
                                            ui,
                                            candidate.category.label(),
                                            category_color(candidate.category),
                                        );
                                    }
                                });
                                ui.label(
                                    RichText::new(candidate.endpoint())
                                        .small()
                                        .weak()
                                        .monospace(),
                                );
                            });
                        });
                    }
                    ui.separator();
                }
            });

        if !toggles.is_empty() {
            for (id, on) in toggles {
                if on {
                    self.settings.selected.insert(id);
                } else {
                    self.settings.selected.remove(&id);
                }
            }
            self.save_settings();
        }
    }

    fn main_panel(&mut self, ui: &mut egui::Ui) {
        let result = self.result.clone();
        let live = self.live_rows.clone();
        egui::ScrollArea::vertical()
            .id_salt("main_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                if let Some(applied) = self.applied.clone() {
                    self.applied_banner(ui, &applied);
                }
                if self.running {
                    self.progress_card(ui);
                    if !live.is_empty() {
                        ui.add_space(10.0);
                        self.results_section(ui, &live, true);
                    }
                } else if let Some(result) = result.clone() {
                    self.recommendation_card(ui, &result);
                    ui.add_space(12.0);
                    self.results_section(ui, &result.rows, false);
                    self.steering_section(ui, &result);
                } else {
                    self.hero(ui);
                }
                ui.add_space(24.0);
            });
    }

    fn applied_banner(&mut self, ui: &mut egui::Ui, applied: &AppliedDns) {
        let dark = ui.visuals().dark_mode;
        let fill = if dark {
            Color32::from_rgb(21, 39, 32)
        } else {
            Color32::from_rgb(230, 247, 239)
        };
        egui::Frame::new()
            .fill(fill)
            .stroke(Stroke::new(1.0, Color32::from_rgb(56, 130, 96)))
            .corner_radius(10)
            .inner_margin(egui::Margin::symmetric(14, 10))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("DNS is applied").strong().color(GREEN));
                    ui.label(RichText::new(applied.summary()).small().weak());
                    ui.label(
                        RichText::new(format!("· {}", config::applied_age(applied)))
                            .small()
                            .weak(),
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui
                            .add_enabled(!self.revert_running, egui::Button::new("Revert"))
                            .clicked()
                        {
                            self.confirm_revert = true;
                        }
                    });
                });
            });
        ui.add_space(10.0);
    }

    fn hero(&mut self, ui: &mut egui::Ui) {
        card_frame(ui).show(ui, |ui| {
            ui.add_space(8.0);
            ui.label(
                RichText::new("Find the fastest DNS for this connection.")
                    .size(28.0)
                    .strong(),
            );
            ui.label(
                RichText::new(
                    "Test public resolvers, compare cached and uncached lookups, check DNSSEC, then apply the winner in one click.",
                )
                .size(14.0)
                .weak(),
            );
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                if ui
                    .add(
                        egui::Button::new(RichText::new("Test all servers").strong())
                            .fill(ACCENT)
                            .min_size(Vec2::new(150.0, 34.0)),
                    )
                    .clicked()
                {
                    self.start_run(true);
                }
                if ui
                    .add(
                        egui::Button::new(format!(
                            "Test selected ({})",
                            self.selected_in_catalog()
                        ))
                        .min_size(Vec2::new(150.0, 34.0)),
                    )
                    .clicked()
                {
                    self.start_run(false);
                }
                if ui.button("Choose in the server list").clicked() {
                    self.notify("Use the checkboxes on the left to choose servers.", false);
                }
            });
            ui.add_space(10.0);
            ui.label(
                RichText::new(
                    "Test all measures every server in the current protocol. Test selected measures only the checked servers. Encrypted modes never apply automatically.",
                )
                .small()
                .weak(),
            );
        });

        ui.add_space(14.0);
        ui.columns(2, |columns| {
            card_frame(&columns[0]).show(&mut columns[0], |ui| {
                ui.label(RichText::new("What you get").strong());
                ui.label("· Median cached and uncached latency");
                ui.label("· Jitter, p95 and p99 timings");
                ui.label("· DNSSEC validation and bogus-answer checks");
                ui.label("· NXDOMAIN hijacking detection");
                ui.label("· JSON, CSV and HTML exports");
            });
            card_frame(&columns[1]).show(&mut columns[1], |ui| {
                ui.label(RichText::new("Safe apply").strong());
                ui.label("· The previous settings are saved");
                ui.label("· The system asks for permission");
                ui.label("· Revert restores the saved settings");
                ui.label("· Nothing is uploaded or telemetered");
                if !self.history.is_empty() {
                    ui.add_space(6.0);
                    if ui
                        .link(format!("Open history ({})", self.history.len()))
                        .clicked()
                    {
                        self.show_history = true;
                    }
                }
            });
        });
    }

    fn progress_card(&mut self, ui: &mut egui::Ui) {
        card_frame(ui).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(self.stage.label()).size(18.0).strong());
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(
                        RichText::new(format_elapsed(self.started))
                            .small()
                            .weak()
                            .monospace(),
                    );
                });
            });
            let (done, total) = self.progress;
            let fraction = if total == 0 {
                0.0
            } else {
                (done as f32 / total as f32).clamp(0.0, 1.0)
            };
            let bar = if total == 0 {
                egui::ProgressBar::new(0.0).animate(true).desired_height(9.0)
            } else {
                egui::ProgressBar::new(fraction)
                    .text(format!("{done} / {total}"))
                    .desired_height(18.0)
                    .fill(ACCENT)
            };
            ui.add(bar);
            ui.label(
                RichText::new(format!(
                    "{} server{} measured so far",
                    self.live_rows.len(),
                    if self.live_rows.len() == 1 { "" } else { "s" }
                ))
                .small()
                .weak(),
            );
            if self.stage == Stage::Scan {
                ui.label(
                    RichText::new(
                        "The quick scan finds responders first; the full test then measures cached and uncached latency.",
                    )
                    .small()
                    .weak(),
                );
            }
        });
    }

    fn recommendation_card(&mut self, ui: &mut egui::Ui, result: &RunResult) {
        let best = result.best();
        let chosen = self
            .chosen_id
            .as_ref()
            .and_then(|id| result.rows.iter().find(|row| &row.id == id))
            .or(best);
        let chosen_is_best = match (chosen, best) {
            (Some(chosen), Some(best)) => chosen.id == best.id,
            _ => false,
        };
        let already_current = chosen
            .and_then(Row::plain_ip)
            .is_some_and(|ip| result.current_dns.contains(&ip));
        let dark = ui.visuals().dark_mode;
        let fill = if dark {
            Color32::from_rgb(24, 37, 53)
        } else {
            Color32::from_rgb(232, 241, 253)
        };
        let stroke_col = if dark {
            Color32::from_rgb(52, 82, 122)
        } else {
            Color32::from_rgb(170, 198, 235)
        };
        egui::Frame::new()
            .fill(fill)
            .stroke(Stroke::new(1.0, stroke_col))
            .corner_radius(14)
            .inner_margin(egui::Margin::same(18))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(if chosen_is_best {
                            "RECOMMENDED"
                        } else {
                            "SELECTED RESOLVER"
                        })
                        .size(10.0)
                        .strong()
                        .color(ACCENT),
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(
                            RichText::new(format!(
                                "{} · {} · {} tested in {:.1} s",
                                result.protocol.label(),
                                result.family.label(),
                                result.tested,
                                result.duration_secs
                            ))
                            .small()
                            .weak(),
                        );
                    });
                });
                let Some(row) = chosen else {
                    ui.add_space(6.0);
                    ui.label(
                        RichText::new("No resolver completed enough measurements.")
                            .size(19.0)
                            .strong(),
                    );
                    ui.label(
                        RichText::new(
                            "The run finished, but every server failed or was flagged. Check the protocol, timeout and network connection.",
                        )
                        .small()
                        .weak(),
                    );
                    return;
                };
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.label(RichText::new(&row.label).size(27.0).strong());
                        ui.horizontal_wrapped(|ui| {
                            ui.label(
                                RichText::new(&row.endpoint)
                                    .monospace()
                                    .color(ACCENT),
                            );
                            ui.label(RichText::new(format!("· {}", row.transport.label())).small().weak());
                            tag(ui, row.security_status().short(), security_color(row.security_status()));
                            if already_current {
                                tag(ui, "IN USE", GREEN);
                            }
                            if row.note != Note::Ok {
                                tag(ui, row.note.label(), RED);
                            }
                            if !row.healthy() {
                                tag(ui, "UNHEALTHY", YELLOW);
                            }
                        });
                        ui.label(
                            RichText::new(format!(
                                "Cached {} · Uncached {} · Jitter {} · {} timeouts",
                                report::fmt_ms(row.cached),
                                report::fmt_ms(row.uncached),
                                report::fmt_ms(row.cached_jitter),
                                row.timeouts
                            ))
                            .small()
                            .weak(),
                        );
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.vertical(|ui| {
                            ui.label(
                                RichText::new(format!("{:.1}", row.score))
                                    .size(32.0)
                                    .strong()
                                    .color(ACCENT),
                            );
                            ui.label(RichText::new("combined ms").small().weak());
                        });
                    });
                });
                ui.add_space(8.0);
                if let Some(warning) = apply_warning(row) {
                    ui.label(RichText::new(warning).small().color(YELLOW));
                }
                ui.horizontal_wrapped(|ui| {
                    let can_apply = row.is_plain()
                        && !engine::apply_addresses(row, &self.apply_candidates()).is_empty();
                    if can_apply {
                        if ui
                            .add(
                                egui::Button::new(RichText::new("Apply to system").strong())
                                    .fill(ACCENT)
                                    .min_size(Vec2::new(136.0, 32.0)),
                            )
                            .clicked()
                        {
                            self.request_apply(row.clone());
                        }
                    } else {
                        ui.label(
                            RichText::new(
                                "Only plain resolvers on port 53 can be applied. DoT, DoH and custom ports are benchmark-only.",
                            )
                            .small()
                            .weak(),
                        );
                    }
                    if !chosen_is_best
                        && let Some(best) = best
                        && ui.button("Use best").clicked()
                    {
                        self.chosen_id = Some(best.id.clone());
                    }
                    if let Some(ip) = row.plain_ip()
                        && ui.button("Copy address").clicked()
                    {
                        ui.ctx().copy_text(ip.to_string());
                        self.notify(format!("Copied {ip}."), false);
                    }
                    if ui.button("Copy summary").clicked() {
                        ui.ctx().copy_text(report::summary_text(result));
                        self.notify("Summary copied to the clipboard.", false);
                    }
                    if ui.button("JSON").clicked() {
                        self.export("json");
                    }
                    if ui.button("CSV").clicked() {
                        self.export("csv");
                    }
                    if ui.button("HTML").clicked() {
                        self.export("html");
                    }
                    if let Some(applied) = &self.applied
                        && ui.button("Revert applied DNS").clicked()
                    {
                        self.confirm_revert = true;
                        let _ = applied;
                    }
                });
                ui.add_space(4.0);
                ui.label(
                    RichText::new(format!(
                        "Current DNS at run time: {} · Cloudflare edge: {}",
                        if result.current_dns.is_empty() {
                            "not detected".to_string()
                        } else {
                            format_current_dns(&result.current_dns)
                        },
                        result.data_center.as_deref().unwrap_or("not detected")
                    ))
                    .small()
                    .weak(),
                );
                if already_current {
                    ui.label(
                        RichText::new(
                            "This resolver is already configured on this machine; no change is needed.",
                        )
                        .small()
                        .color(GREEN),
                    );
                } else if !result.current_dns.is_empty() && row.is_plain() {
                    ui.label(
                        RichText::new(format!(
                            "Applying it will replace the current DNS: {}.",
                            format_current_dns(&result.current_dns)
                        ))
                        .small()
                        .weak(),
                    );
                }
            });
    }

    fn results_section(&mut self, ui: &mut egui::Ui, rows: &[Row], live: bool) {
        let mut sorted = rows.to_vec();
        sort_rows(&mut sorted, self.sort, self.sort_descending);
        let query = self.result_filter.trim().to_lowercase();
        if !query.is_empty() {
            sorted.retain(|row| row_matches(row, &query));
        }
        sorted.retain(|row| self.health_filter.allows(row));
        if let Some(category) = self.result_category {
            sorted.retain(|row| row_category(row) == category);
        }
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(if live { "Live results" } else { "All results" })
                    .size(18.0)
                    .strong(),
            );
            ui.label(
                RichText::new(format!("{} shown", sorted.len()))
                    .small()
                    .weak(),
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                egui::ComboBox::from_id_salt("sort_results")
                    .selected_text(format!(
                        "{}{}",
                        self.sort.label(),
                        if self.sort_descending { " ↓" } else { " ↑" }
                    ))
                    .width(154.0)
                    .show_ui(ui, |ui| {
                        for key in SortKey::ALL {
                            ui.selectable_value(&mut self.sort, key, key.label());
                        }
                    });
                if ui
                    .small_button(if self.sort_descending {
                        "Descending"
                    } else {
                        "Ascending"
                    })
                    .on_hover_text("Reverse the sort order")
                    .clicked()
                {
                    self.sort_descending = !self.sort_descending;
                }
            });
        });
        ui.horizontal_wrapped(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.result_filter)
                    .hint_text("Filter provider, server, status or security")
                    .desired_width(240.0),
            );
            ui.label(RichText::new("Category").small().weak());
            let mut category = self.result_category;
            egui::ComboBox::from_id_salt("result_category")
                .selected_text(category.map_or("All", Category::label))
                .width(132.0)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut category, None, "All categories");
                    for option in CATEGORIES {
                        ui.selectable_value(&mut category, Some(option), option.label());
                    }
                });
            if category != self.result_category {
                self.result_category = category;
            }
            ui.label(RichText::new("Health").small().weak());
            egui::ComboBox::from_id_salt("result_health")
                .selected_text(self.health_filter.label())
                .width(138.0)
                .show_ui(ui, |ui| {
                    for option in HealthFilter::ALL {
                        ui.selectable_value(&mut self.health_filter, option, option.label());
                    }
                });
        });
        if sorted.is_empty() {
            ui.label(
                RichText::new("No results match these filters yet.")
                    .small()
                    .weak()
                    .italics(),
            );
            return;
        }
        let chosen = self.chosen_id.clone();
        if let Some(id) = results_table(
            ui,
            &sorted,
            chosen.as_deref(),
            &mut self.sort,
            &mut self.sort_descending,
        ) {
            self.chosen_id = Some(id);
        }
        if !live {
            ui.label(
                RichText::new(
                    "Click a result or Use to choose it. Click a column heading to sort; the recommendation above follows your choice.",
                )
                .small()
                .weak(),
            );
        }
    }

    fn steering_section(&mut self, ui: &mut egui::Ui, result: &RunResult) {
        if result.steerings.is_empty() {
            return;
        }
        ui.add_space(14.0);
        ui.label(RichText::new("Steering").size(18.0).strong());
        ui.label(
            RichText::new(
                "Where hostnames resolve from the fastest plain-DNS servers, and whether the returned addresses are reachable.",
            )
            .small()
            .weak(),
        );
        for steering in &result.steerings {
            egui::CollapsingHeader::new(format!(
                "{} — {} servers",
                steering.host, steering.servers
            ))
            .id_salt(format!("steering:{}", steering.host))
            .default_open(true)
            .show(ui, |ui| {
                steering_table(ui, steering);
            });
        }
    }

    fn status_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if let Some(notice) = &self.notice {
                ui.label(RichText::new(&notice.text).small().color(if notice.error {
                    RED
                } else {
                    GREEN
                }));
            } else if self.apply_running {
                ui.spinner();
                ui.label(
                    RichText::new("Waiting for administrator permission…")
                        .small()
                        .weak(),
                );
            } else if self.revert_running {
                ui.spinner();
                ui.label(
                    RichText::new("Restoring the previous DNS settings…")
                        .small()
                        .weak(),
                );
            } else if self.running {
                ui.spinner();
                ui.label(
                    RichText::new(format!(
                        "{} · {} / {}",
                        self.stage.label(),
                        self.progress.0,
                        self.progress.1
                    ))
                    .small()
                    .weak(),
                );
            } else if let Some(result) = &self.result {
                ui.label(
                    RichText::new(format!(
                        "Finished · {} tested · {:.1} s",
                        result.tested, result.duration_secs
                    ))
                    .small()
                    .weak(),
                );
            } else {
                ui.label(
                    RichText::new(format!(
                        "Ready · {} servers available · {} selected",
                        self.catalog.len(),
                        self.selected_in_catalog()
                    ))
                    .small()
                    .weak(),
                );
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(
                    RichText::new(format!("{} · dnsrace {VERSION}", platform::platform_name()))
                        .small()
                        .weak(),
                );
            });
        });
    }

    fn windows(&mut self, ctx: &egui::Context) {
        self.settings_window(ctx);
        self.custom_window(ctx);
        self.history_window(ctx);
        self.about_window(ctx);
        self.confirm_apply_window(ctx);
        self.confirm_revert_window(ctx);
    }

    fn settings_window(&mut self, ctx: &egui::Context) {
        if !self.show_settings {
            return;
        }
        let mut open = true;
        let mut settings = self.settings.clone();
        let mut export_dir = self.export_dir_text.clone();
        let mut save = false;
        let mut reset = false;
        let mut add_custom = false;
        egui::Window::new("Settings")
            .open(&mut open)
            .default_width(520.0)
            .resizable(false)
            .show(ctx, |ui| {
                ui.label(RichText::new("Benchmark").strong());
                egui::Grid::new("settings_grid")
                    .num_columns(2)
                    .spacing([18.0, 8.0])
                    .show(ui, |ui| {
                        ui.label("Per-query timeout");
                        ui.add(
                            egui::DragValue::new(&mut settings.timeout_ms)
                                .range(100..=5000)
                                .suffix(" ms"),
                        );
                        ui.end_row();
                        ui.label("Parallel servers");
                        ui.add(egui::DragValue::new(&mut settings.jobs).range(1..=32));
                        ui.end_row();
                        ui.label("Rounds");
                        ui.add(egui::DragValue::new(&mut settings.repeat).range(1..=3));
                        ui.end_row();
                        ui.label("Steering connect port");
                        ui.add(egui::DragValue::new(&mut settings.connect_port).range(1..=65535));
                        ui.end_row();
                    });
                ui.add_space(6.0);
                ui.checkbox(
                    &mut settings.include_system,
                    "Include this machine's current DNS and router in plain tests",
                );
                ui.checkbox(
                    &mut settings.include_nearby,
                    "Add nearby-country open resolvers (downloads a country list when you start)",
                );
                ui.add_space(8.0);
                ui.label(RichText::new("Steering hostnames").strong());
                ui.label(
                    RichText::new("One hostname per line, for example example.com.")
                        .small()
                        .weak(),
                );
                ui.add(
                    egui::TextEdit::multiline(&mut settings.hostnames)
                        .hint_text("example.com\ncloudflare.com")
                        .desired_rows(3)
                        .desired_width(f32::INFINITY),
                );
                ui.add_space(8.0);
                ui.label(RichText::new("Exports").strong());
                ui.horizontal(|ui| {
                    ui.label("Folder");
                    ui.add(
                        egui::TextEdit::singleline(&mut export_dir)
                            .hint_text("Leave empty for Downloads")
                            .desired_width(340.0),
                    );
                });
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.label("Appearance");
                    egui::ComboBox::from_id_salt("theme")
                        .selected_text(settings.theme.label())
                        .width(150.0)
                        .show_ui(ui, |ui| {
                            for theme in Theme::ALL {
                                ui.selectable_value(&mut settings.theme, theme, theme.label())
                                    .on_hover_text(theme.description());
                            }
                        });
                });
                ui.label(RichText::new(settings.theme.description()).small().weak());
                ui.add_space(10.0);
                ui.separator();
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Custom resolvers").strong());
                    if ui.small_button("+ Add").clicked() {
                        add_custom = true;
                    }
                });
                if settings.custom.is_empty() {
                    ui.label(RichText::new("No custom resolvers yet.").small().weak());
                } else {
                    let mut remove = None;
                    for (index, custom) in settings.custom.iter().enumerate() {
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(format!(
                                    "{} · {}",
                                    custom.name,
                                    custom.protocol.label()
                                ))
                                .small(),
                            );
                            ui.label(RichText::new(&custom.endpoint).small().weak().monospace());
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if ui.small_button("Remove").clicked() {
                                    remove = Some(index);
                                }
                            });
                        });
                    }
                    if let Some(index) = remove {
                        settings.custom.remove(index);
                    }
                }
                ui.add_space(10.0);
                ui.separator();
                ui.horizontal(|ui| {
                    if ui
                        .add(
                            egui::Button::new(RichText::new("Save").strong())
                                .fill(ACCENT)
                                .min_size(Vec2::new(90.0, 30.0)),
                        )
                        .clicked()
                    {
                        save = true;
                    }
                    if ui.button("Reset benchmark defaults").clicked() {
                        reset = true;
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(
                            RichText::new(format!(
                                "Applying uses {} system settings.",
                                platform::platform_name()
                            ))
                            .small()
                            .weak(),
                        );
                    });
                });
            });
        if reset {
            let defaults = Settings::default();
            settings.timeout_ms = defaults.timeout_ms;
            settings.jobs = defaults.jobs;
            settings.repeat = defaults.repeat;
            settings.connect_port = defaults.connect_port;
            settings.include_system = defaults.include_system;
            settings.include_nearby = defaults.include_nearby;
            settings.hostnames = defaults.hostnames.clone();
            settings.export_dir = defaults.export_dir;
            settings.theme = defaults.theme;
            export_dir.clear();
        }
        if save {
            self.settings.timeout_ms = settings.timeout_ms;
            self.settings.jobs = settings.jobs;
            self.settings.repeat = settings.repeat;
            self.settings.connect_port = settings.connect_port;
            self.settings.include_system = settings.include_system;
            self.settings.include_nearby = settings.include_nearby;
            self.settings.hostnames = settings.hostnames.clone();
            self.settings.theme = settings.theme;
            self.settings.custom = settings.custom.clone();
            self.settings.export_dir = if export_dir.trim().is_empty() {
                None
            } else {
                Some(PathBuf::from(export_dir.trim()))
            };
            self.export_dir_text = export_dir;
            self.save_settings();
            self.rebuild_catalog();
            self.notify("Settings saved.", false);
            open = false;
        }
        if add_custom {
            self.custom = CustomDraft::default();
            self.show_custom = true;
        }
        self.show_settings = open;
    }

    fn custom_window(&mut self, ctx: &egui::Context) {
        if !self.show_custom {
            return;
        }
        let mut open = true;
        let mut draft = self.custom.clone();
        let mut add = false;
        egui::Window::new("Add custom resolver")
            .open(&mut open)
            .default_width(500.0)
            .resizable(false)
            .show(ctx, |ui| {
                ui.label(
                    RichText::new(
                        "Custom resolvers are tested exactly like built-in servers. Plain, DoT and DoH endpoints are supported.",
                    )
                    .small()
                    .weak(),
                );
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.label("Name");
                    ui.add(
                        egui::TextEdit::singleline(&mut draft.name)
                            .hint_text("Optional label")
                            .desired_width(300.0),
                    );
                });
                ui.horizontal(|ui| {
                    ui.label("Type");
                    egui::ComboBox::from_id_salt("custom_protocol")
                        .selected_text(draft.protocol.label())
                        .show_ui(ui, |ui| {
                            for protocol in Protocol::all() {
                                ui.selectable_value(&mut draft.protocol, protocol, protocol.label());
                            }
                        });
                });
                match draft.protocol {
                    Protocol::Udp | Protocol::Tcp => {
                        ui.horizontal(|ui| {
                            ui.label("Address");
                            ui.add(
                                egui::TextEdit::singleline(&mut draft.endpoint)
                                    .hint_text("1.1.1.1 or 1.1.1.1:5353")
                                    .desired_width(330.0),
                            );
                        });
                    }
                    Protocol::Dot => {
                        ui.horizontal(|ui| {
                            ui.label("Hostname");
                            ui.add(
                                egui::TextEdit::singleline(&mut draft.endpoint)
                                    .hint_text("dns.example.com")
                                    .desired_width(330.0),
                            );
                        });
                        ui.horizontal(|ui| {
                            ui.label("IP (optional)");
                            ui.add(
                                egui::TextEdit::singleline(&mut draft.address)
                                    .hint_text("1.2.3.4")
                                    .desired_width(330.0),
                            );
                        });
                    }
                    Protocol::Doh => {
                        ui.horizontal(|ui| {
                            ui.label("DoH URL");
                            ui.add(
                                egui::TextEdit::singleline(&mut draft.endpoint)
                                    .hint_text("https://dns.example.com/dns-query")
                                    .desired_width(330.0),
                            );
                        });
                    }
                }
                if draft.protocol.is_plain() {
                    ui.label(
                        RichText::new(
                            "Custom ports are useful for testing; the Apply button only accepts port 53.",
                        )
                        .small()
                        .weak(),
                    );
                }
                if let Some(error) = &draft.error {
                    ui.label(RichText::new(error).small().color(RED));
                }
                ui.add_space(8.0);
                if ui
                    .add(
                        egui::Button::new(RichText::new("Add resolver").strong())
                            .fill(ACCENT)
                            .min_size(Vec2::new(126.0, 30.0)),
                    )
                    .clicked()
                {
                    add = true;
                }
            });
        self.custom = draft;
        if add {
            let name = self.custom.name.trim().to_string();
            let endpoint = self.custom.endpoint.trim().to_string();
            let candidate = match self.custom.protocol {
                Protocol::Udp | Protocol::Tcp => providers::custom_plain(&name, &endpoint),
                Protocol::Dot => {
                    providers::custom_dot(&name, &endpoint, Some(self.custom.address.as_str()))
                }
                Protocol::Doh => providers::custom_doh(&name, &endpoint),
            };
            match candidate {
                Ok(candidate) => {
                    self.settings.custom.push(CustomResolver {
                        name: if name.is_empty() {
                            endpoint.clone()
                        } else {
                            name
                        },
                        protocol: self.custom.protocol,
                        endpoint,
                        address: if self.custom.address.trim().is_empty() {
                            None
                        } else {
                            Some(self.custom.address.trim().to_string())
                        },
                    });
                    self.settings.selected.insert(candidate.id.clone());
                    self.save_settings();
                    self.rebuild_catalog();
                    self.notify(format!("Added {}.", candidate.display()), false);
                    self.custom = CustomDraft::default();
                    self.show_custom = false;
                    return;
                }
                Err(error) => {
                    self.custom.error = Some(error);
                    open = true;
                }
            }
        }
        self.show_custom = open;
    }

    fn history_window(&mut self, ctx: &egui::Context) {
        if !self.show_history {
            return;
        }
        let mut open = true;
        let entries = self.history.clone();
        let mut load = None;
        let mut clear = false;
        let mut close = false;
        egui::Window::new("History")
            .open(&mut open)
            .default_width(560.0)
            .resizable(false)
            .show(ctx, |ui| {
                ui.label(
                    RichText::new(
                        "The last 20 runs are stored locally as JSON in the config folder.",
                    )
                    .small()
                    .weak(),
                );
                ui.add_space(6.0);
                if entries.is_empty() {
                    ui.label(RichText::new("No runs saved yet.").weak().italics());
                }
                for (index, entry) in entries.iter().enumerate() {
                    egui::Frame::new()
                        .fill(ui.visuals().faint_bg_color)
                        .corner_radius(8)
                        .inner_margin(egui::Margin::same(10))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.vertical(|ui| {
                                    ui.label(RichText::new(entry.label()).strong());
                                    ui.label(
                                        RichText::new(format!(
                                            "{} results · {} tested · {:.1} s · {}",
                                            entry.rows.len(),
                                            entry.tested.max(entry.rows.len()),
                                            entry.duration_secs,
                                            entry.family.label()
                                        ))
                                        .small()
                                        .weak(),
                                    );
                                });
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    if ui.button("Open").clicked() {
                                        load = Some(index);
                                    }
                                });
                            });
                        });
                    ui.add_space(4.0);
                }
                ui.separator();
                if ui.button("Clear history").clicked() {
                    clear = true;
                }
            });
        if clear {
            match config::clear_history() {
                Ok(()) => {
                    self.history.clear();
                    self.notify("History cleared.", false);
                }
                Err(error) => self.notify(format!("Could not clear history: {error}"), true),
            }
        }
        if let Some(index) = load
            && let Some(entry) = entries.get(index).cloned()
        {
            let count = entry.rows.len();
            let result = RunResult {
                protocol: entry.protocol,
                family: entry.family,
                duration_secs: entry.duration_secs,
                data_center: entry.data_center,
                current_dns: entry.current_dns,
                rows: entry.rows,
                steerings: entry.steerings,
                scanned: entry.scanned.max(count),
                tested: entry.tested.max(count),
            };
            self.settings.protocol = result.protocol;
            self.settings.family = result.family;
            self.rebuild_catalog();
            self.save_settings();
            self.chosen_id = result.best().map(|row| row.id.clone());
            self.live_rows = result.rows.clone();
            self.stage = Stage::Done;
            self.progress = (result.tested, result.tested);
            self.result = Some(result);
            close = true;
            self.notify("Loaded a saved run.", false);
        }
        self.show_history = open && !close;
    }

    fn about_window(&mut self, ctx: &egui::Context) {
        if !self.show_about {
            return;
        }
        let mut open = true;
        let mut close = false;
        egui::Window::new("About dnsrace")
            .open(&mut open)
            .default_width(480.0)
            .resizable(false)
            .show(ctx, |ui| {
                ui.label(RichText::new(format!("dnsrace {VERSION}")).size(22.0).strong());
                ui.label(
                    RichText::new(
                        "A desktop tool for finding and applying the fastest DNS resolver for your connection.",
                    )
                    .weak(),
                );
                ui.add_space(8.0);
                ui.label(RichText::new("What's new in 0.7.0").strong());
                ui.label("· Automatic detection of the DNS servers this machine is using");
                ui.label("· Provider names next to detected addresses (for example 1.1.1.1 · Cloudflare)");
                ui.label("· Eight themes, including System, Midnight, Nord, Solarized Light, Dracula and High contrast");
                ui.label("· Refresh detection, test the current DNS, and compare it with the winner");
                ui.label("· Sortable columns, health/category filters and a safer, collision-free export");
                ui.add_space(8.0);
                ui.label(RichText::new("Privacy").strong());
                ui.label(
                    "No telemetry. The only network requests are DNS probes, TLS/HTTPS connections to the resolvers being tested, and the optional nearby-country list.",
                );
                ui.add_space(8.0);
                ui.label(RichText::new("Auto-detection").strong());
                ui.label(
                    "Linux: systemd-resolved, NetworkManager, /etc/resolv.conf and the default route. Windows: Get-NetIPConfiguration with a registry fallback. macOS: scutil, networksetup and the default route.",
                );
                ui.add_space(8.0);
                ui.label(RichText::new("Apply support").strong());
                ui.label(
                    "Linux: NetworkManager or systemd-resolved. Windows: PowerShell with UAC. macOS: networksetup with the administrator prompt.",
                );
                ui.add_space(8.0);
                ui.label(RichText::new("Shortcuts").strong());
                ui.label("Ctrl/Cmd+R or F5: refresh detection · Ctrl/Cmd+Enter: test selected");
                ui.label("Ctrl/Cmd+Shift+Enter: test all · Ctrl/Cmd+E: export HTML · Esc: cancel");
                ui.add_space(8.0);
                ui.label(RichText::new("Built with").strong());
                ui.label("Rust 1.99.1, egui/eframe, rustls, tokio and ureq.");
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.hyperlink_to(
                        "Project page",
                        "https://github.com/Yousef-Mohamed22/dnsrace",
                    );
                    ui.label("·");
                    ui.label("MIT licensed");
                });
                ui.add_space(8.0);
                if ui.button("Close").clicked() {
                    close = true;
                }
            });
        self.show_about = open && !close;
    }

    fn confirm_apply_window(&mut self, ctx: &egui::Context) {
        let Some(pending) = self.pending_apply.clone() else {
            return;
        };
        let mut open = true;
        let mut apply = false;
        let mut cancel = false;
        egui::Window::new("Apply DNS settings?")
            .open(&mut open)
            .default_width(470.0)
            .resizable(false)
            .collapsible(false)
            .show(ctx, |ui| {
                ui.label(RichText::new(&pending.label).strong());
                ui.add_space(4.0);
                ui.label("These DNS servers will be set on the active connection:");
                for ip in &pending.addresses {
                    ui.label(RichText::new(ip.to_string()).monospace().color(ACCENT));
                }
                if let Some(warning) = &pending.warning {
                    ui.label(RichText::new(warning).small().color(YELLOW));
                }
                ui.add_space(6.0);
                ui.label(
                    RichText::new(
                        "The previous settings are saved first, so you can use Revert. Your system may ask for administrator permission.",
                    )
                    .small()
                    .weak(),
                );
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui
                        .add(
                            egui::Button::new(RichText::new("Apply").strong())
                                .fill(ACCENT)
                                .min_size(Vec2::new(90.0, 30.0)),
                        )
                        .clicked()
                    {
                        apply = true;
                    }
                    if ui.button("Cancel").clicked() {
                        cancel = true;
                    }
                });
            });
        if apply {
            self.pending_apply = None;
            self.start_apply(pending.addresses.clone());
        } else if cancel || !open {
            self.pending_apply = None;
        }
    }

    fn confirm_revert_window(&mut self, ctx: &egui::Context) {
        if !self.confirm_revert {
            return;
        }
        let applied = self.applied.clone();
        let mut open = true;
        let mut restore = false;
        let mut cancel = false;
        egui::Window::new("Restore previous DNS?")
            .open(&mut open)
            .default_width(460.0)
            .resizable(false)
            .collapsible(false)
            .show(ctx, |ui| {
                if let Some(applied) = &applied {
                    ui.label("dnsrace will restore the DNS settings saved when it applied:");
                    ui.label(RichText::new(applied.summary()).small().weak());
                } else {
                    ui.label("No saved DNS change was found.");
                }
                ui.add_space(6.0);
                ui.label(
                    RichText::new(
                        "Your system may ask for administrator permission. If the saved settings are unavailable, the connection's automatic DNS is restored.",
                    )
                    .small()
                    .weak(),
                );
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui
                        .add(
                            egui::Button::new(RichText::new("Restore").strong())
                                .fill(ACCENT)
                                .min_size(Vec2::new(90.0, 30.0)),
                        )
                        .clicked()
                    {
                        restore = true;
                    }
                    if ui.button("Cancel").clicked() {
                        cancel = true;
                    }
                });
            });
        if restore {
            self.confirm_revert = false;
            self.start_revert();
        } else {
            self.confirm_revert = !(cancel || !open);
        }
    }
}

impl eframe::App for DnsRaceGui {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.approve_theme(ctx);
        self.poll_detection(ctx);
        self.poll_events(ctx);
        self.poll_apply(ctx);
        self.handle_shortcuts(ctx);
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.approve_theme(ui.ctx());
        let ctx = ui.ctx().clone();
        let top_frame = egui::Frame::new()
            .fill(ui.visuals().panel_fill)
            .inner_margin(egui::Margin::symmetric(16, 10))
            .stroke(Stroke::new(
                1.0,
                ui.visuals().widgets.noninteractive.bg_stroke.color,
            ));
        egui::Panel::top("toolbar")
            .resizable(false)
            .frame(top_frame)
            .show(ui, |ui| self.top_bar(ui));
        let bottom_frame = egui::Frame::new()
            .fill(ui.visuals().panel_fill)
            .inner_margin(egui::Margin::symmetric(16, 4));
        egui::Panel::bottom("status")
            .resizable(false)
            .exact_size(28.0)
            .frame(bottom_frame)
            .show(ui, |ui| self.status_bar(ui));
        let side_frame = egui::Frame::new()
            .fill(ui.visuals().panel_fill)
            .inner_margin(egui::Margin::symmetric(14, 12));
        egui::Panel::left("servers")
            .resizable(true)
            .default_size(356.0)
            .size_range(300.0..=480.0)
            .frame(side_frame)
            .show(ui, |ui| self.sidebar(ui));
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(ui.visuals().panel_fill)
                    .inner_margin(egui::Margin::symmetric(22, 12)),
            )
            .show(ui, |ui| self.main_panel(ui));
        self.windows(&ctx);
    }
}

impl Drop for DnsRaceGui {
    fn drop(&mut self) {
        self.cancel.store(true, AtomicOrdering::Relaxed);
    }
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

fn format_elapsed(started: Option<Instant>) -> String {
    match started {
        Some(started) => format!("{:.1} s", started.elapsed().as_secs_f32()),
        None => String::new(),
    }
}

fn format_ip_list(ips: &[IpAddr]) -> String {
    ips.iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}

/// Current DNS addresses with provider names, e.g. `1.1.1.1 (Cloudflare), 8.8.8.8 (Google Public DNS)`.
fn format_current_dns(ips: &[IpAddr]) -> String {
    ips.iter()
        .map(|ip| providers::describe_address(*ip))
        .collect::<Vec<_>>()
        .join(", ")
}

fn family_allows(candidate: &Candidate, family: Family) -> bool {
    match &candidate.target {
        servers::Target::Plain(addr) => family.allows(addr.ip()),
        servers::Target::Dot { addr, .. } => match addr {
            Some(addr) => family.allows(addr.ip()),
            None => family == Family::Any,
        },
        servers::Target::Doh { .. } => true,
    }
}

fn custom_candidate(custom: &CustomResolver) -> Result<Candidate, String> {
    match custom.protocol {
        Protocol::Udp | Protocol::Tcp => providers::custom_plain(&custom.name, &custom.endpoint),
        Protocol::Dot => {
            providers::custom_dot(&custom.name, &custom.endpoint, custom.address.as_deref())
        }
        Protocol::Doh => providers::custom_doh(&custom.name, &custom.endpoint),
    }
}

fn matches_filter(candidate: &Candidate, query: &str) -> bool {
    candidate.provider.to_lowercase().contains(query)
        || candidate.label.to_lowercase().contains(query)
        || candidate.endpoint().to_lowercase().contains(query)
        || candidate.category.label().to_lowercase().contains(query)
}

fn row_matches(row: &Row, query: &str) -> bool {
    row.provider.to_lowercase().contains(query)
        || row.label.to_lowercase().contains(query)
        || row.endpoint.to_lowercase().contains(query)
        || row.security_status().label().to_lowercase().contains(query)
        || row.note.label().to_lowercase().contains(query)
}

fn row_category(row: &Row) -> Category {
    providers::PROVIDERS
        .iter()
        .find(|meta| meta.group == row.provider || meta.label == row.label)
        .map(|meta| meta.category)
        .unwrap_or(Category::General)
}

fn sort_rows(rows: &mut [Row], sort: SortKey, descending: bool) {
    let compare = |a: &Row, b: &Row| -> Ordering {
        match sort {
            SortKey::Score => a.score.total_cmp(&b.score),
            SortKey::Cached => a
                .cached
                .unwrap_or(f64::INFINITY)
                .total_cmp(&b.cached.unwrap_or(f64::INFINITY)),
            SortKey::Uncached => a
                .uncached
                .unwrap_or(f64::INFINITY)
                .total_cmp(&b.uncached.unwrap_or(f64::INFINITY)),
            SortKey::Provider => a
                .provider
                .to_lowercase()
                .cmp(&b.provider.to_lowercase())
                .then_with(|| a.label.cmp(&b.label)),
            SortKey::Endpoint => a.endpoint.to_lowercase().cmp(&b.endpoint.to_lowercase()),
            SortKey::Security => security_rank(a)
                .cmp(&security_rank(b))
                .then_with(|| a.score.total_cmp(&b.score)),
            SortKey::Health => health_rank(a)
                .cmp(&health_rank(b))
                .then_with(|| a.score.total_cmp(&b.score)),
        }
    };
    if descending {
        rows.sort_by(|a, b| compare(b, a));
    } else {
        rows.sort_by(compare);
    }
}

fn security_rank(row: &Row) -> u8 {
    match row.security_status() {
        SecurityStatus::Validated => 0,
        SecurityStatus::Partial => 1,
        SecurityStatus::Insecure => 2,
        SecurityStatus::Unknown => 3,
        SecurityStatus::BogusAccepted => 4,
    }
}

fn health_rank(row: &Row) -> u8 {
    if row.healthy() { 0 } else { 1 }
}

fn security_color(status: SecurityStatus) -> Color32 {
    match status {
        SecurityStatus::Validated => GREEN,
        SecurityStatus::Insecure | SecurityStatus::Partial | SecurityStatus::Unknown => YELLOW,
        SecurityStatus::BogusAccepted => RED,
    }
}

fn category_color(category: Category) -> Color32 {
    match category {
        Category::General => Color32::from_rgb(130, 170, 220),
        Category::Security => GREEN,
        Category::Family => YELLOW,
        Category::Privacy => Color32::from_rgb(185, 140, 255),
        Category::Uncensored => Color32::from_rgb(110, 205, 220),
        Category::System => YELLOW,
        Category::Custom => Color32::from_rgb(185, 140, 255),
    }
}

fn tag(ui: &mut egui::Ui, text: &str, color: Color32) {
    egui::Frame::new()
        .fill(color.gamma_multiply(0.16))
        .corner_radius(4)
        .inner_margin(egui::Margin::symmetric(5, 1))
        .show(ui, |ui| {
            ui.label(RichText::new(text).size(9.0).strong().color(color));
        });
}

fn card_frame(ui: &egui::Ui) -> egui::Frame {
    let visuals = ui.visuals();
    egui::Frame::new()
        .fill(visuals.widgets.inactive.weak_bg_fill)
        .stroke(Stroke::new(
            1.0,
            visuals.widgets.noninteractive.bg_stroke.color,
        ))
        .corner_radius(12)
        .inner_margin(egui::Margin::same(14))
}

fn apply_warning(row: &Row) -> Option<String> {
    if row.security_status() == SecurityStatus::BogusAccepted {
        return Some(
            "This resolver accepted a deliberately bogus DNSSEC signature. Do not apply it as a system resolver."
                .to_string(),
        );
    }
    if !row.healthy() {
        return Some(
            "This resolver did not pass all health checks. Applying it is possible, but not recommended."
                .to_string(),
        );
    }
    None
}

fn sort_header(
    ui: &mut egui::Ui,
    sort: &mut SortKey,
    descending: &mut bool,
    label: &str,
    key: SortKey,
) {
    let active = *sort == key;
    let arrow = if active {
        if *descending { " ↓" } else { " ↑" }
    } else {
        ""
    };
    let response = ui.add(
        egui::Button::new(
            RichText::new(format!("{label}{arrow}"))
                .size(10.0)
                .strong()
                .weak(),
        )
        .frame(false),
    );
    if response.clicked() {
        if active {
            *descending = !*descending;
        } else {
            *sort = key;
            *descending = false;
        }
    }
}

fn results_table(
    ui: &mut egui::Ui,
    rows: &[Row],
    chosen: Option<&str>,
    sort: &mut SortKey,
    descending: &mut bool,
) -> Option<String> {
    let mut choice = None;
    ui.push_id("results_table", |ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        let full = ui.available_width() >= 760.0;
        let mut table = TableBuilder::new(ui)
            .striped(true)
            .resizable(true)
            .sense(egui::Sense::click())
            .cell_layout(Layout::left_to_right(Align::Center))
            .column(Column::exact(30.0))
            .column(Column::initial(120.0).at_least(100.0).clip(true))
            .column(Column::remainder().at_least(120.0).clip(true));
        if full {
            table = table
                .column(Column::exact(38.0))
                .column(Column::exact(58.0))
                .column(Column::exact(66.0));
        }
        table
            .column(Column::exact(54.0))
            .column(Column::exact(90.0))
            .column(Column::exact(82.0))
            .column(Column::exact(48.0))
            .max_scroll_height(420.0)
            .header(24.0, |mut header| {
                header.col(|ui| {
                    ui.label(RichText::new("#").size(10.0).strong().weak());
                });
                header.col(|ui| {
                    sort_header(ui, sort, descending, "Provider", SortKey::Provider);
                });
                header.col(|ui| {
                    sort_header(ui, sort, descending, "Server", SortKey::Endpoint);
                });
                if full {
                    header.col(|ui| {
                        ui.label(RichText::new("Type").size(10.0).strong().weak());
                    });
                    header.col(|ui| {
                        sort_header(ui, sort, descending, "Cached", SortKey::Cached);
                    });
                    header.col(|ui| {
                        sort_header(ui, sort, descending, "Uncached", SortKey::Uncached);
                    });
                }
                header.col(|ui| {
                    sort_header(ui, sort, descending, "Score", SortKey::Score);
                });
                header.col(|ui| {
                    sort_header(ui, sort, descending, "DNSSEC", SortKey::Security);
                });
                header.col(|ui| {
                    sort_header(ui, sort, descending, "Status", SortKey::Health);
                });
                header.col(|ui| {
                    ui.label(RichText::new("Use").size(10.0).strong().weak());
                });
            })
            .body(|mut body| {
                for (index, row) in rows.iter().enumerate() {
                    body.row(28.0, |mut table_row| {
                        let selected = chosen == Some(row.id.as_str());
                        table_row.set_selected(selected);
                        table_row.col(|ui| {
                            if ui
                                .selectable_label(selected, format!("{}", index + 1))
                                .clicked()
                            {
                                choice = Some(row.id.clone());
                            }
                        });
                        table_row.col(|ui| {
                            ui.label(RichText::new(&row.provider).strong())
                                .on_hover_text(format!(
                                    "{}\n{}\nCached {} · Uncached {}",
                                    row.label,
                                    row.endpoint,
                                    report::fmt_ms(row.cached),
                                    report::fmt_ms(row.uncached)
                                ));
                        });
                        table_row.col(|ui| {
                            ui.label(RichText::new(&row.endpoint).monospace().size(12.0))
                                .on_hover_text(format!(
                                    "{}\n{}\nCached {} · Uncached {}",
                                    row.label,
                                    if row.note == Note::Ok {
                                        "No health flags."
                                    } else {
                                        row.note.label()
                                    },
                                    report::fmt_ms(row.cached),
                                    report::fmt_ms(row.uncached)
                                ));
                        });
                        if full {
                            table_row.col(|ui| {
                                ui.label(RichText::new(row.transport.label()).small());
                            });
                            table_row.col(|ui| {
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    ui.label(
                                        RichText::new(report::fmt_ms(row.cached))
                                            .small()
                                            .monospace(),
                                    );
                                });
                            });
                            table_row.col(|ui| {
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    ui.label(
                                        RichText::new(report::fmt_ms(row.uncached))
                                            .small()
                                            .monospace(),
                                    );
                                });
                            });
                        }
                        table_row.col(|ui| {
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                ui.label(
                                    RichText::new(format!("{:.1}", row.score))
                                        .small()
                                        .strong()
                                        .color(ACCENT)
                                        .monospace(),
                                );
                            });
                        });
                        table_row.col(|ui| {
                            let status = row.security_status();
                            ui.label(
                                RichText::new(status.short())
                                    .small()
                                    .color(security_color(status)),
                            );
                        });
                        table_row.col(|ui| {
                            if row.note != Note::Ok {
                                ui.label(RichText::new(row.note.label()).small().color(RED));
                            } else if row.healthy() {
                                ui.label(RichText::new("OK").small().color(GREEN));
                            } else {
                                ui.label(RichText::new("Check").small().color(YELLOW));
                            }
                        });
                        table_row.col(|ui| {
                            if ui.small_button("Use").clicked() {
                                choice = Some(row.id.clone());
                            }
                        });
                        if table_row.response().clicked() {
                            choice = Some(row.id.clone());
                        }
                    });
                }
            });
    });
    choice
}

fn median_connect(entry: &crate::bench::SteerEntry) -> Option<f64> {
    let mut values: Vec<f64> = entry.connects.iter().flatten().copied().collect();
    crate::util::median(&mut values)
}

fn steering_table(ui: &mut egui::Ui, steering: &Steering) {
    if steering.entries.is_empty() {
        ui.label(
            RichText::new("No addresses were returned by the tested servers.")
                .small()
                .weak(),
        );
        return;
    }
    egui::Grid::new(format!("steering_grid:{}", steering.host))
        .num_columns(4)
        .spacing([18.0, 4.0])
        .striped(true)
        .show(ui, |ui| {
            ui.label(RichText::new("Address").small().strong().weak());
            ui.label(RichText::new("Returned by").small().strong().weak());
            ui.label(RichText::new("Median connect").small().strong().weak());
            ui.label(RichText::new("Attempts").small().strong().weak());
            ui.end_row();
            for entry in &steering.entries {
                ui.label(RichText::new(entry.ip.to_string()).monospace().small());
                ui.label(
                    RichText::new(format!("{} / {}", entry.returned_by, steering.servers)).small(),
                );
                ui.label(
                    RichText::new(report::fmt_ms(median_connect(entry)))
                        .small()
                        .monospace(),
                );
                ui.label(
                    RichText::new(
                        entry
                            .connects
                            .iter()
                            .map(|value| {
                                value
                                    .map(|ms| format!("{ms:.1}"))
                                    .unwrap_or_else(|| "fail".into())
                            })
                            .collect::<Vec<_>>()
                            .join(" · "),
                    )
                    .small()
                    .weak()
                    .monospace(),
                );
                ui.end_row();
            }
        });
}

/// The selected set is kept in `Settings`; these helpers are used by tests and the header.
pub fn selected_ids(settings: &Settings) -> &BTreeSet<String> {
    &settings.selected
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bench::Note;

    fn row(provider: &str, score: f64, security: Option<bool>) -> Row {
        Row {
            id: format!("{provider}:{score}"),
            provider: provider.to_string(),
            label: provider.to_string(),
            transport: Protocol::Udp,
            addr: Some("1.1.1.1:53".parse().unwrap()),
            endpoint: "1.1.1.1".into(),
            cached: Some(score),
            uncached: Some(score + 1.0),
            cached_min: Some(score),
            cached_p95: Some(score + 2.0),
            cached_p99: Some(score + 3.0),
            cached_mean: Some(score + 1.0),
            cached_max: Some(score + 4.0),
            cached_jitter: Some(1.0),
            dnssec: security,
            dnssec_bogus: Some(true),
            timeouts: 0,
            note: Note::Ok,
            score,
        }
    }

    #[test]
    fn sort_orders_fastest_first() {
        let mut rows = vec![row("slow", 30.0, Some(true)), row("fast", 10.0, Some(true))];
        sort_rows(&mut rows, SortKey::Score, false);
        assert_eq!(rows[0].provider, "fast");
        sort_rows(&mut rows, SortKey::Score, true);
        assert_eq!(rows[0].provider, "slow");
    }

    #[test]
    fn sortable_columns_cover_provider_endpoint_security_and_health() {
        let mut rows = vec![
            row("Zulu", 30.0, None),
            row("Alpha", 10.0, Some(true)),
            row("Mike", 20.0, Some(false)),
        ];
        sort_rows(&mut rows, SortKey::Provider, false);
        assert_eq!(rows[0].provider, "Alpha");
        sort_rows(&mut rows, SortKey::Endpoint, false);
        assert_eq!(rows.len(), 3);
        sort_rows(&mut rows, SortKey::Security, false);
        assert_eq!(rows[0].security_status(), SecurityStatus::Validated);
        sort_rows(&mut rows, SortKey::Health, false);
        assert!(rows[0].healthy());
    }

    #[test]
    fn health_and_category_filters_narrow_the_table() {
        let healthy = row("Cloudflare", 10.0, Some(true));
        let mut bad = row("Quad9", 20.0, Some(true));
        bad.note = Note::Unreliable;
        assert!(HealthFilter::All.allows(&bad));
        assert!(HealthFilter::Healthy.allows(&healthy));
        assert!(!HealthFilter::Healthy.allows(&bad));
        assert!(HealthFilter::Issues.allows(&bad));
        assert_eq!(row_category(&healthy), Category::General);
        assert_eq!(row_category(&bad), Category::Security);
    }

    #[test]
    fn current_dns_text_names_known_providers() {
        let text =
            format_current_dns(&["1.1.1.1".parse().unwrap(), "203.0.113.7".parse().unwrap()]);
        assert_eq!(text, "1.1.1.1 (Cloudflare), 203.0.113.7");
    }

    #[test]
    fn filters_match_provider_and_security() {
        let row = row("Cloudflare", 10.0, Some(true));
        assert!(row_matches(&row, "cloud"));
        assert!(row_matches(&row, "dnssec"));
        assert!(!row_matches(&row, "quad9"));
    }

    #[test]
    fn category_and_security_colors_are_distinct() {
        assert_ne!(security_color(SecurityStatus::Validated), RED);
        assert_eq!(security_color(SecurityStatus::BogusAccepted), RED);
        assert_ne!(
            category_color(Category::Security),
            category_color(Category::Family)
        );
    }
}
