//! The application controller.
//!
//! `App` owns the state, the capabilities and a backend. It dispatches actions into state changes
//! or backend effects, and renders the shell that matches the layout mode. No page renderer is
//! allowed to touch a worker, a file or the clipboard directly.

use std::net::IpAddr;
use std::sync::mpsc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use eframe::egui;

use crate::android_guide;
use crate::bench::SecurityStatus;
use crate::config::{self, CustomResolver, HistoryEntry, Settings, Theme};
use crate::engine::{self, RunEvent, RunRequest, RunResult, Stage};
use crate::gui::modals;
use crate::gui::theme;
use crate::i18n;
use crate::platform::AppliedDns;
use crate::profile::Profile;
use crate::providers;
use crate::servers::{Candidate, Family, Kind, Pool, Protocol};
use crate::system::SystemInfo;
use crate::update::{self, UpdateEvent};

use super::actions::{Page, RunScope, UiAction};
use super::backend::Backend;
use super::desktop;
use super::mobile;
use super::state::{AppState, LayoutMode, ViewContext};

pub struct App {
    pub state: AppState,
    pub cx: ViewContext,
    backend: Box<dyn Backend>,
    /// Actions that need the real backend and are handled by the effect pass.
    pub effects: Vec<UiAction>,
    installed_theme: Option<Theme>,
    detect_rx: Option<mpsc::Receiver<SystemInfo>>,
    detect_notify: bool,
    run_rx: Option<mpsc::Receiver<RunEvent>>,
    apply_rx: Option<mpsc::Receiver<Result<AppliedDns, String>>>,
    revert_rx: Option<mpsc::Receiver<Result<(), String>>>,
    update_rx: Option<mpsc::Receiver<UpdateEvent>>,
    run_started: Option<Instant>,
    /// Viewport size the current Android safe area was measured at. Rotation changes the size,
    /// so the shell re-queries the activity instead of assuming portrait insets.
    safe_area_for: Option<egui::Vec2>,
}

/// Keep only the exception reason; a Java stack trace belongs in logcat, not a phone notice.
fn concise_android_error(error: &str) -> &str {
    error
        .split("Stack trace:")
        .next()
        .unwrap_or_default()
        .trim()
}

impl App {
    pub fn new(state: AppState, cx: ViewContext, backend: Box<dyn Backend>) -> Self {
        App {
            state,
            cx,
            backend,
            effects: Vec::new(),
            installed_theme: None,
            detect_rx: None,
            detect_notify: false,
            run_rx: None,
            apply_rx: None,
            revert_rx: None,
            update_rx: None,
            run_started: None,
            safe_area_for: None,
        }
    }

    /// Start the background tasks the first frame expects. Tests skip this so their backend
    /// call log contains only the action under test.
    pub fn start_initial_tasks(&mut self) {
        self.rebuild_catalog();
        self.start_detection(false);
    }

    /// Start the automatic update check in production. Tests never call this.
    pub fn start_update_check_if_enabled(&mut self) {
        if self.state.settings.check_updates {
            self.start_update_check();
        }
    }

    pub fn dispatch(&mut self, action: UiAction) {
        self.handle(action);
        self.run_effects();
        self.poll_ready();
    }

    fn handle(&mut self, action: UiAction) {
        match action {
            UiAction::Navigate(page) => self.state.page = page,
            UiAction::SetProtocol(protocol) => {
                self.state.settings.protocol = protocol;
                self.rebuild_catalog();
            }
            UiAction::SetFamily(family) => {
                self.state.settings.family = family;
                self.rebuild_catalog();
            }
            UiAction::SetProfile(profile) => self.state.settings.apply_profile(profile),
            UiAction::SetTheme(theme) => self.state.settings.theme = theme,
            UiAction::SetLanguage(language) => {
                self.state.settings.language = language;
                i18n::set_language(language);
            }
            UiAction::SetCheckUpdates(check) => self.state.settings.check_updates = check,
            UiAction::SetImportPath(path) => self.state.import_path = path,
            UiAction::SetSearch(query) => self.state.search = query,
            UiAction::SetCategoryFilter(category) => self.state.category_filter = category,
            UiAction::SetKindFilter(kind) => self.state.kind_filter = kind,
            UiAction::SetFamilyFilter(family) => self.state.family_filter = family,
            UiAction::SetFavoritesOnly(favorites_only) => {
                self.state.favorites_only = favorites_only;
            }
            UiAction::ToggleSelected(id) => {
                if !self.state.settings.selected.remove(&id) {
                    self.state.settings.selected.insert(id);
                }
            }
            UiAction::SelectAllVisible => {
                let ids: Vec<String> = self
                    .state
                    .visible_catalog()
                    .into_iter()
                    .map(|candidate| candidate.id.clone())
                    .collect();
                self.state.settings.selected.extend(ids);
            }
            UiAction::SelectNone => self.state.settings.selected.clear(),
            UiAction::ToggleFavorite(id) => {
                if !self.state.settings.favorites.remove(&id) {
                    self.state.settings.favorites.insert(id);
                }
            }
            UiAction::ChooseRow(id) => {
                if !self.state.details_open
                    || self.state.selected_row.as_deref() != Some(id.as_str())
                {
                    self.state.details_epoch = self.state.details_epoch.wrapping_add(1);
                }
                self.state.selected_row = Some(id);
                self.state.details_open = true;
            }
            UiAction::CloseDetails => self.state.details_open = false,
            UiAction::OpenGuide(id) => self.open_guide(id),
            UiAction::CloseGuide => self.state.guide = None,
            UiAction::CopyGuideText => {
                if let Some(text) = self
                    .state
                    .guide
                    .as_ref()
                    .and_then(android_guide::AndroidGuide::copy_text)
                {
                    self.copy(text);
                } else {
                    self.state
                        .notify("There is no hostname or address to copy yet.", true);
                }
            }
            UiAction::OpenPrivateDns => self.android_call("Private DNS settings", |host| {
                host.open_private_dns_settings()
            }),
            UiAction::OpenWifi => {
                self.android_call("Wi-Fi settings", |host| host.open_wifi_settings())
            }
            UiAction::CopyCurrentDns => {
                let addresses = self.state.current_dns();
                if addresses.is_empty() {
                    self.state.notify("Current DNS is not known yet.", true);
                } else {
                    self.copy(join_addresses(&addresses));
                }
            }
            UiAction::CopyEndpoint(id) => {
                if let Some(endpoint) = self.state.endpoint_for(&id) {
                    self.copy(endpoint);
                } else {
                    self.state.notify("That resolver has no endpoint.", true);
                }
            }
            UiAction::OpenWebsite(id) => {
                if let Some(url) = self.state.website_for(&id) {
                    self.open_link(url);
                } else {
                    self.state
                        .notify("That provider has no website in the catalogue.", true);
                }
            }
            UiAction::Apply(id) => {
                if self.cx.capabilities.can_apply_dns {
                    self.request_apply(id);
                } else {
                    self.state.notify(
                        "Android does not let dnsrace change DNS. Use the setup guide instead.",
                        true,
                    );
                }
            }
            UiAction::Revert => {
                if self.state.applied.is_none() {
                    self.state
                        .notify("There is no saved DNS change to revert.", true);
                } else {
                    self.state.confirm_revert = true;
                }
            }
            UiAction::ConfirmApply => {
                self.state.confirm_apply = false;
                self.effects.push(UiAction::ConfirmApply);
            }
            UiAction::CancelApply => {
                self.state.confirm_apply = false;
                self.state.pending_apply = None;
            }
            UiAction::ConfirmRevert => {
                self.state.confirm_revert = false;
                self.effects.push(UiAction::ConfirmRevert);
            }
            UiAction::CancelRevert => self.state.confirm_revert = false,
            UiAction::OpenHistory(index) => {
                self.state.page = Page::History;
                self.state.history_selected = Some(index);
                if let Some(entry) = self.state.history.get(index) {
                    self.state.selected_row = entry.winner().map(|row| row.id.clone());
                }
            }
            UiAction::ReopenHistory(index) => {
                let Some(entry) = self.state.history.get(index) else {
                    self.state
                        .notify("That history entry no longer exists.", true);
                    return;
                };
                let result = result_from_history(entry);
                self.state.selected_row = result.best().map(|row| row.id.clone());
                self.state.result = Some(result);
                self.state.page = Page::Overview;
                self.state.notify("History run reopened.", false);
            }
            UiAction::DeleteHistory(index) => {
                if index < self.state.history.len() {
                    self.state.history.remove(index);
                    self.state.history_selected = None;
                    match config::save_history(&self.state.history) {
                        Ok(()) => self.state.notify("History entry deleted.", false),
                        Err(error) => self
                            .state
                            .notify(format!("Could not save history: {error}"), true),
                    }
                }
            }
            UiAction::RequestClearHistory => self.state.confirm_clear_history = true,
            UiAction::CancelClearHistory => self.state.confirm_clear_history = false,
            UiAction::ClearHistory => {
                self.state.history.clear();
                self.state.history_selected = None;
                self.state.confirm_clear_history = false;
                match config::clear_history() {
                    Ok(()) => self.state.notify("History cleared.", false),
                    Err(error) => self
                        .state
                        .notify(format!("Could not clear history: {error}"), true),
                }
            }
            UiAction::OpenCustom => {
                self.state.custom_open = true;
                self.state.custom_edit = None;
                self.state.custom_draft = Default::default();
            }
            UiAction::EditCustom(index) => {
                let Some(resolver) = self.state.settings.custom.get(index) else {
                    self.state
                        .notify("That custom resolver no longer exists.", true);
                    return;
                };
                self.state.custom_open = true;
                self.state.custom_edit = Some(index);
                self.state.custom_draft = custom_draft_for(resolver);
            }
            UiAction::CancelCustom => {
                self.state.custom_open = false;
                self.state.custom_edit = None;
                self.state.custom_draft = Default::default();
            }
            UiAction::RemoveCustom(index) => {
                if index < self.state.settings.custom.len() {
                    self.state.settings.custom.remove(index);
                    self.rebuild_catalog();
                    match self.backend.save_settings(&self.state.settings) {
                        Ok(()) => self.state.notify("Custom resolver removed.", false),
                        Err(error) => self.state.notify(error, true),
                    }
                }
            }
            UiAction::OpenAbout => {
                self.state.about_open = true;
                self.state.about_scroll_pending = true;
            }
            UiAction::CloseAbout => {
                self.state.about_open = false;
                self.state.about_scroll_pending = false;
            }
            UiAction::OpenAboutLink(link) => self.open_link(link.url().to_string()),
            UiAction::ResetSettings => {
                self.state.settings = Settings::default();
                self.rebuild_catalog();
                self.state
                    .notify("Settings reset to their defaults.", false);
                self.effects.push(UiAction::SaveSettings);
            }
            UiAction::SetTimeout(timeout_ms) => {
                self.state.settings.apply_profile(Profile::Custom);
                self.state.settings.timeout_ms = timeout_ms.clamp(100, 10_000);
            }
            UiAction::SetJobs(jobs) => {
                self.state.settings.apply_profile(Profile::Custom);
                self.state.settings.jobs = jobs.clamp(1, 64);
            }
            UiAction::SetRepeat(repeat) => {
                self.state.settings.apply_profile(Profile::Custom);
                self.state.settings.repeat = repeat.clamp(1, 5);
            }
            UiAction::SetCachedRounds(rounds) => {
                self.state.settings.apply_profile(Profile::Custom);
                self.state.settings.cached_rounds = rounds.clamp(1, 10);
            }
            UiAction::SetRandomLookups(lookups) => {
                self.state.settings.apply_profile(Profile::Custom);
                self.state.settings.random_lookups = lookups.clamp(0, 50);
            }
            UiAction::SetHostnames(hostnames) => self.state.settings.hostnames = hostnames,
            UiAction::SetIncludeSystem(include) => {
                self.state.settings.include_system = include;
                self.rebuild_catalog();
            }
            UiAction::SetIncludeNearby(include) => self.state.settings.include_nearby = include,
            // Everything below crosses a worker, file or platform boundary. It is recorded now
            // and executed by the effect pass.
            UiAction::Run(_)
            | UiAction::Cancel
            | UiAction::RefreshDetection
            | UiAction::SaveSettings
            | UiAction::Export(_)
            | UiAction::ExportHistory(_, _)
            | UiAction::ShareReport(_)
            | UiAction::ShareHistory(_, _)
            | UiAction::CheckUpdates
            | UiAction::InstallUpdate
            | UiAction::CopyDiagnostics
            | UiAction::ExportResolverData
            | UiAction::ImportResolverData
            | UiAction::SaveCustom => self.effects.push(action),
        }
    }

    fn run_effects(&mut self) {
        let effects = std::mem::take(&mut self.effects);
        for effect in effects {
            match effect {
                UiAction::Run(scope) => self.start_run(scope),
                UiAction::Cancel => self.cancel_run(),
                UiAction::RefreshDetection => self.start_detection(true),
                UiAction::SaveSettings => self.save_settings(),
                UiAction::Export(format) => self.export_result(format),
                UiAction::ExportHistory(index, format) => self.export_history(index, format),
                UiAction::ShareReport(format) => self.share_report(format),
                UiAction::ShareHistory(index, format) => self.share_history(index, format),
                UiAction::ConfirmApply => self.confirm_apply(),
                UiAction::ConfirmRevert => self.confirm_revert(),
                UiAction::SaveCustom => self.save_custom(),
                UiAction::CheckUpdates => self.start_update_check(),
                UiAction::InstallUpdate => self.start_update_install(),
                UiAction::CopyDiagnostics => self.copy_diagnostics(),
                UiAction::ExportResolverData => self.export_resolvers(),
                UiAction::ImportResolverData => self.import_resolvers(),
                _ => unreachable!("only backend actions are queued as effects"),
            }
        }
    }

    /// Poll every channel once without a UI context. Dispatch calls this so a one-shot test
    /// backend is visible immediately; the real app polls with a context from `logic`.
    fn poll_ready(&mut self) {
        self.poll_detection(None);
        self.poll_run(None);
        self.poll_apply(None);
        self.poll_update(None);
    }

    fn poll(&mut self, ctx: &egui::Context) {
        self.poll_detection(Some(ctx));
        self.poll_run(Some(ctx));
        self.poll_apply(Some(ctx));
        self.poll_update(Some(ctx));
    }

    fn start_detection(&mut self, notify: bool) {
        if self.detect_rx.is_some() {
            self.detect_notify |= notify;
            return;
        }
        self.state.detecting = true;
        self.detect_notify = notify;
        self.detect_rx = Some(self.backend.start_detection());
    }

    fn poll_detection(&mut self, ctx: Option<&egui::Context>) {
        let Some(rx) = self.detect_rx.take() else {
            return;
        };
        match rx.try_recv() {
            Ok(info) => {
                self.state.detecting = false;
                self.apply_system_info(info);
                if self.detect_notify {
                    self.detect_notify = false;
                }
            }
            Err(mpsc::TryRecvError::Empty) => {
                self.detect_rx = Some(rx);
                if let Some(ctx) = ctx {
                    ctx.request_repaint_after(Duration::from_millis(120));
                }
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                self.state.detecting = false;
                self.state.notify("DNS detection ended unexpectedly.", true);
            }
        }
    }

    fn apply_system_info(&mut self, info: SystemInfo) {
        let count = info.dns.len();
        self.state.system = Some(info);
        if self.state.settings.include_system && self.state.settings.protocol.is_plain() {
            self.rebuild_catalog();
        }
        if self.detect_notify {
            let message = if count == 0 {
                "Refreshed: no system DNS servers were detected.".to_string()
            } else {
                crate::i18n::fill("Refreshed: {} DNS server(s) detected.", &[&count])
            };
            self.state.notify(message, false);
        }
    }

    fn poll_run(&mut self, ctx: Option<&egui::Context>) {
        let Some(rx) = self.run_rx.take() else {
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
                    self.handle_run_event(event);
                    if terminal {
                        keep = false;
                        break;
                    }
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    keep = false;
                    if self.state.running {
                        self.state.running = false;
                        self.state
                            .notify("The benchmark ended unexpectedly. Try again.", true);
                    }
                    break;
                }
            }
        }
        if keep {
            self.run_rx = Some(rx);
        }
        if self.state.running
            && let Some(ctx) = ctx
        {
            ctx.request_repaint_after(Duration::from_millis(80));
        }
    }

    fn handle_run_event(&mut self, event: RunEvent) {
        match event {
            RunEvent::Detected {
                current_dns,
                gateway,
                data_center: _,
            } => {
                let mut system = self.state.system.clone().unwrap_or_default();
                system.dns = current_dns;
                system.gateway = gateway;
                self.state.system = Some(system);
                if self.state.settings.include_system && self.state.settings.protocol.is_plain() {
                    self.rebuild_catalog();
                }
            }
            RunEvent::Stage { stage, done, total } => {
                self.state.stage = Some(stage);
                self.state.progress = (done, total);
            }
            RunEvent::Row(row) => {
                if !self
                    .state
                    .live_rows
                    .iter()
                    .any(|existing| existing.id == row.id)
                {
                    self.state.live_rows.push(row);
                }
            }
            RunEvent::Finished(result) => self.finish_run(result),
            RunEvent::Cancelled(result) => {
                self.state.running = false;
                self.state.stage = Some(Stage::Done);
                self.state.live_rows = result.rows.clone();
                if result.rows.is_empty() {
                    self.state.result = None;
                } else {
                    self.state.selected_row = result.best().map(|row| row.id.clone());
                    self.state.result = Some(result);
                }
                self.state
                    .notify("Benchmark cancelled. Partial results are shown.", false);
            }
            RunEvent::Error(error) => {
                self.state.running = false;
                self.state.stage = Some(Stage::Done);
                self.state.notify(error, true);
            }
        }
    }

    fn finish_run(&mut self, result: RunResult) {
        self.state.running = false;
        self.state.stage = Some(Stage::Done);
        self.state.progress = (result.tested, result.tested);
        self.state.selected_row = result.best().map(|row| row.id.clone());
        self.state.live_rows = result.rows.clone();

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
            profile: result.profile,
            depth: result.depth,
        };
        match config::push_history(self.state.history.clone(), entry) {
            Ok(history) => self.state.history = history,
            Err(error) => self
                .state
                .notify(format!("Could not save history: {error}"), true),
        }

        let message = match result.best() {
            Some(row) => {
                let score = format!("{:.1}", row.score);
                crate::i18n::fill("Finished: {} is fastest at {} ms.", &[&row.label, &score])
            }
            None => "Finished, but no resolver completed enough measurements.".to_string(),
        };
        self.state.notify(message, false);
        self.state.result = Some(result);
        self.run_started = None;
    }

    fn start_run(&mut self, scope: RunScope) {
        if self.state.running {
            return;
        }

        let (candidates, protocol, family, include_system, include_nearby) = match scope {
            RunScope::CurrentDns => {
                let mut info = self.state.system.clone().unwrap_or_default();
                info.gateway = None;
                let mut pool = Pool::default();
                for candidate in providers::system_candidates(&info, &[]) {
                    pool.add(candidate);
                }
                (pool.into_items(), Protocol::Udp, Family::Any, false, false)
            }
            RunScope::All | RunScope::Selected | RunScope::Favorites => (
                self.state.candidates_for_scope(scope),
                self.state.settings.protocol,
                self.state.settings.family,
                self.state.settings.include_system,
                self.state.settings.include_nearby && self.state.settings.protocol.is_plain(),
            ),
        };

        if candidates.is_empty() {
            let message = match scope {
                RunScope::CurrentDns => {
                    "No current DNS servers are known yet. Use Refresh detection and try again."
                }
                RunScope::Favorites => {
                    "No favourites are available for the current protocol. Star a resolver in \
                     Servers, or test all servers."
                }
                RunScope::Selected => {
                    "Select at least one server for this protocol, or use Test all."
                }
                RunScope::All => {
                    "There are no servers in this category. Add a custom resolver or choose \
                     another protocol."
                }
            };
            self.state.notify(message, true);
            return;
        }

        let request = RunRequest {
            candidates,
            protocol,
            family,
            wait: self.state.settings.timeout(),
            jobs: self.state.settings.jobs(),
            repeat: self.state.settings.repeat(),
            hostnames: self.state.settings.hostnames(),
            connect_port: self.state.settings.connect_port.max(1),
            include_system,
            include_nearby,
            environment: None,
            profile: self.state.settings.profile,
            depth: self.state.settings.depth(),
        };

        self.state.result = None;
        self.state.live_rows.clear();
        self.state.selected_row = None;
        self.state.guide = None;
        self.state.stage = Some(Stage::Scan);
        self.state.progress = (0, 0);
        self.state.running = true;
        self.run_started = Some(Instant::now());
        self.run_rx = Some(self.backend.start_run(request));
    }

    fn cancel_run(&mut self) {
        if self.state.running {
            self.backend.cancel_run();
            self.state.notify("Cancelling…", false);
        }
    }

    fn request_apply(&mut self, id: String) {
        let Some(row) = self.state.find_row(&id).cloned() else {
            self.state
                .notify("Finish a benchmark before applying a resolver.", true);
            return;
        };
        if !row.is_plain() {
            self.state.notify(
                "Encrypted DNS (DoT/DoH) cannot be applied system-wide here. Run UDP or TCP and \
                 choose a plain resolver.",
                true,
            );
            return;
        }
        if row.security_status() == SecurityStatus::BogusAccepted {
            self.state.notify(
                "That resolver accepted a deliberately broken DNSSEC signature, so it will not \
                 be applied.",
                true,
            );
            return;
        }
        let addresses = engine::apply_addresses(&row, &self.apply_candidates());
        if addresses.is_empty() {
            self.state
                .notify("No usable address was found for that resolver.", true);
            return;
        }
        let warning = if row.healthy() {
            None
        } else {
            Some(format!(
                "This resolver had {} failed lookups and is marked \"{}\". It can still be \
                 applied, but it did not rank as healthy.",
                row.timeouts,
                row.note.label()
            ))
        };
        self.state.selected_row = Some(id);
        self.state.pending_apply = Some(super::state::PendingApply {
            label: format!("{} ({})", row.label, row.endpoint),
            addresses,
            warning,
        });
        self.state.confirm_apply = true;
    }

    fn confirm_apply(&mut self) {
        let Some(pending) = self.state.pending_apply.take() else {
            self.state
                .notify("The apply request is no longer available.", true);
            return;
        };
        self.state.applying = true;
        self.state.notify(
            "Waiting for permission to change the system DNS settings…",
            false,
        );
        self.apply_rx = Some(self.backend.apply(pending.addresses));
    }

    fn confirm_revert(&mut self) {
        let Some(applied) = self.state.applied.clone() else {
            self.state
                .notify("There is no saved DNS change to revert.", true);
            return;
        };
        self.state.reverting = true;
        self.state
            .notify("Restoring the previous DNS settings…", false);
        self.revert_rx = Some(self.backend.revert(applied));
    }

    fn poll_apply(&mut self, ctx: Option<&egui::Context>) {
        if let Some(rx) = self.apply_rx.take() {
            match rx.try_recv() {
                Ok(Ok(applied)) => {
                    self.state.applying = false;
                    match config::save_applied(&applied) {
                        Ok(()) => self
                            .state
                            .notify(format!("Applied {}.", applied.summary()), false),
                        Err(error) => self.state.notify(
                            format!(
                                "DNS was applied, but the revert information could not be saved: \
                                 {error}"
                            ),
                            true,
                        ),
                    }
                    let mut system = self.state.system.clone().unwrap_or_default();
                    system.dns = applied.applied.clone();
                    system.interface = Some(applied.interface.clone());
                    system.source = Some(crate::i18n::fill("Applied via {}", &[&applied.method]));
                    self.state.system = Some(system);
                    self.state.applied = Some(applied);
                    if self.state.settings.include_system && self.state.settings.protocol.is_plain()
                    {
                        self.rebuild_catalog();
                    }
                    self.start_detection(false);
                }
                Ok(Err(error)) => {
                    self.state.applying = false;
                    self.state
                        .notify(format!("Could not apply DNS: {error}"), true);
                }
                Err(mpsc::TryRecvError::Empty) => {
                    self.apply_rx = Some(rx);
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.state.applying = false;
                    self.state
                        .notify("The apply operation ended unexpectedly.", true);
                }
            }
        }

        if let Some(rx) = self.revert_rx.take() {
            match rx.try_recv() {
                Ok(Ok(())) => {
                    self.state.reverting = false;
                    match config::clear_applied() {
                        Ok(()) => self.state.notify("Previous DNS settings restored.", false),
                        Err(error) => self.state.notify(
                            format!("DNS was restored, but state cleanup failed: {error}"),
                            true,
                        ),
                    }
                    self.state.applied = None;
                    self.start_detection(false);
                }
                Ok(Err(error)) => {
                    self.state.reverting = false;
                    self.state
                        .notify(format!("Could not restore DNS: {error}"), true);
                }
                Err(mpsc::TryRecvError::Empty) => {
                    self.revert_rx = Some(rx);
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.state.reverting = false;
                    self.state
                        .notify("The revert operation ended unexpectedly.", true);
                }
            }
        }

        if (self.state.applying || self.state.reverting)
            && let Some(ctx) = ctx
        {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
    }

    fn start_update_check(&mut self) {
        if self.update_rx.is_some()
            || matches!(
                self.state.update.phase,
                super::state::UpdatePhase::Downloading
                    | super::state::UpdatePhase::Verifying
                    | super::state::UpdatePhase::Installing
            )
        {
            return;
        }
        let (sender, receiver) = mpsc::channel();
        self.update_rx = Some(receiver);
        self.state.update.phase = super::state::UpdatePhase::Checking;
        self.state.update.error = None;
        update::spawn_check(self.cx.version.to_owned(), sender);
    }

    fn start_update_install(&mut self) {
        let Some(release) = self.state.update.release.clone() else {
            self.state.notify("Check for updates first.", true);
            return;
        };
        if self.update_rx.is_some() {
            return;
        }
        let (sender, receiver) = mpsc::channel();
        self.update_rx = Some(receiver);
        self.state.update.phase = super::state::UpdatePhase::Downloading;
        self.state.update.progress = Some((0, None));
        self.state.update.error = None;
        update::spawn_install(release, sender);
    }

    fn poll_update(&mut self, ctx: Option<&egui::Context>) {
        let Some(rx) = self.update_rx.take() else {
            return;
        };
        let mut keep = true;
        loop {
            match rx.try_recv() {
                Ok(UpdateEvent::Checking) => {
                    self.state.update.phase = super::state::UpdatePhase::Checking;
                }
                Ok(UpdateEvent::UpToDate) => {
                    self.state.update.phase = super::state::UpdatePhase::UpToDate;
                    self.state.update.release = None;
                    self.state.update.progress = None;
                    keep = false;
                }
                Ok(UpdateEvent::Available(release)) => {
                    self.state.update.phase = super::state::UpdatePhase::Available;
                    self.state.update.release = Some(release);
                    self.state.update.progress = None;
                    keep = false;
                }
                Ok(UpdateEvent::Downloading { received, total }) => {
                    self.state.update.phase = super::state::UpdatePhase::Downloading;
                    self.state.update.progress = Some((received, total));
                }
                Ok(UpdateEvent::Verifying) => {
                    self.state.update.phase = super::state::UpdatePhase::Verifying;
                }
                Ok(UpdateEvent::Installing) => {
                    self.state.update.phase = super::state::UpdatePhase::Installing;
                }
                Ok(UpdateEvent::Restarting) => {
                    self.state.update.phase = super::state::UpdatePhase::Restarting;
                    keep = false;
                }
                Ok(UpdateEvent::OpenPage(url)) => {
                    self.state.update.phase = super::state::UpdatePhase::Idle;
                    self.open_link(url);
                    keep = false;
                }
                Ok(UpdateEvent::Failed(error)) => {
                    self.state.update.phase = super::state::UpdatePhase::Failed;
                    self.state.update.error = Some(error.clone());
                    keep = false;
                    let message = if self.state.update.release.is_some()
                        && self.state.update.progress.is_some()
                    {
                        format!("Could not update: {error}")
                    } else {
                        format!("Could not check for updates: {error}")
                    };
                    self.state.notify(message, true);
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    keep = false;
                    if self.state.update.phase == super::state::UpdatePhase::Checking {
                        self.state.update.phase = super::state::UpdatePhase::Failed;
                        self.state.notify("Could not check for updates.", true);
                    }
                    break;
                }
            }
        }
        if keep {
            self.update_rx = Some(rx);
        }
        if self.state.update.phase == super::state::UpdatePhase::Restarting
            && let Some(ctx) = ctx
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        } else if let Some(ctx) = ctx
            && matches!(
                self.state.update.phase,
                super::state::UpdatePhase::Checking
                    | super::state::UpdatePhase::Downloading
                    | super::state::UpdatePhase::Verifying
                    | super::state::UpdatePhase::Installing
            )
        {
            ctx.request_repaint_after(Duration::from_millis(120));
        }
    }

    fn copy_diagnostics(&mut self) {
        let settings = &self.state.settings;
        let update = match self.state.update.phase {
            super::state::UpdatePhase::Idle => i18n::t("not checked").to_owned(),
            super::state::UpdatePhase::Checking => i18n::t("checking").to_owned(),
            super::state::UpdatePhase::UpToDate => i18n::t("up to date").to_owned(),
            super::state::UpdatePhase::Available => self
                .state
                .update
                .release
                .as_ref()
                .map(|release| i18n::fill("version {} available", &[&release.version]))
                .unwrap_or_default(),
            super::state::UpdatePhase::Downloading => i18n::t("downloading").to_owned(),
            super::state::UpdatePhase::Verifying => i18n::t("verifying").to_owned(),
            super::state::UpdatePhase::Installing => i18n::t("installing").to_owned(),
            super::state::UpdatePhase::Restarting => i18n::t("restarting").to_owned(),
            super::state::UpdatePhase::Failed => i18n::t("failed").to_owned(),
        };
        let last_run = self
            .state
            .result
            .as_ref()
            .and_then(|result| result.best())
            .map(|row| format!("{} · {:.1} ms", row.label, row.score));
        let system = self.state.system.as_ref();
        let config_dir = config::config_dir();
        let report = crate::diagnostics::Report {
            version: self.cx.version,
            language: settings.language.label(),
            theme: settings.theme.label(),
            profile: settings.profile.label(),
            protocol: settings.protocol.label(),
            family: settings.family.label(),
            timeout_ms: settings.timeout_ms,
            jobs: settings.jobs,
            repeat: settings.repeat,
            cached_rounds: settings.cached_rounds,
            random_lookups: settings.random_lookups,
            current_dns: system
                .map(|system| system.dns.as_slice())
                .unwrap_or_default(),
            interface: system.and_then(|system| system.interface.as_deref()),
            gateway: system.and_then(|system| system.gateway),
            data_center: self
                .state
                .result
                .as_ref()
                .and_then(|result| result.data_center.as_deref()),
            last_run: last_run.as_deref(),
            update: &update,
            config_dir: &config_dir,
            history: self.state.history.len(),
            custom: settings.custom.len(),
            favourites: settings.favorites.len(),
        };
        match self.backend.copy(&report.render()) {
            Ok(()) => self
                .state
                .notify("Diagnostics copied to the clipboard.", false),
            Err(error) => self.state.notify(format!("Could not copy: {error}"), true),
        }
    }

    fn export_resolvers(&mut self) {
        if !self.cx.capabilities.can_export_files {
            self.state.notify(
                "Exporting files is not available on Android. Share the report instead.",
                true,
            );
            return;
        }
        let text = match crate::transfer::export(&self.state.settings) {
            Ok(text) => text,
            Err(error) => {
                self.state
                    .notify(format!("Could not export: {error}"), true);
                return;
            }
        };
        let path = config::resolver_export_path(&self.state.settings);
        if let Some(parent) = path.parent()
            && let Err(error) = std::fs::create_dir_all(parent)
        {
            self.state
                .notify(format!("Could not export: {error}"), true);
            return;
        }
        match std::fs::write(&path, text) {
            Ok(()) => self
                .state
                .notify("Exported resolvers and favourites.", false),
            Err(error) => self
                .state
                .notify(format!("Could not export: {error}"), true),
        }
    }

    fn import_resolvers(&mut self) {
        let path = if self.state.import_path.trim().is_empty() {
            config::resolver_export_path(&self.state.settings)
        } else {
            std::path::PathBuf::from(self.state.import_path.trim())
        };
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(error) => {
                self.state
                    .notify(format!("Could not import that file: {error}"), true);
                return;
            }
        };
        let mut settings = self.state.settings.clone();
        match crate::transfer::import(&text, &mut settings) {
            Ok(summary) => {
                self.state.settings = settings;
                self.rebuild_catalog();
                let message = i18n::fill(
                    "Imported {} resolvers and {} favourites.",
                    &[&summary.added_resolvers, &summary.added_favourites],
                );
                self.state.notify(message, false);
                if let Err(error) = self.backend.save_settings(&self.state.settings) {
                    self.state.notify(error, true);
                }
            }
            Err(error) if error == "not a dnsrace resolver export" => {
                self.state
                    .notify("That file is not a dnsrace resolver export.", true);
            }
            Err(error) => {
                self.state
                    .notify(format!("Could not import that file: {error}"), true);
            }
        }
    }

    fn save_settings(&mut self) {
        match self.backend.save_settings(&self.state.settings) {
            Ok(()) => self.state.notify("Settings saved.", false),
            Err(error) => self.state.notify(error, true),
        }
    }

    fn save_custom(&mut self) {
        let draft = self.state.custom_draft.clone();
        let name = draft.name.trim().to_string();
        let endpoint = draft.endpoint.trim().to_string();
        let candidate = match draft.protocol {
            Protocol::Udp | Protocol::Tcp => providers::custom_plain(&name, &endpoint),
            Protocol::Dot => providers::custom_dot(&name, &endpoint, Some(draft.address.as_str())),
            Protocol::Doh => providers::custom_doh(&name, &endpoint),
        };

        match candidate {
            Ok(candidate) => {
                let resolver = CustomResolver {
                    name: if name.is_empty() {
                        endpoint.clone()
                    } else {
                        name
                    },
                    protocol: draft.protocol,
                    endpoint,
                    address: if draft.address.trim().is_empty() {
                        None
                    } else {
                        Some(draft.address.trim().to_string())
                    },
                };
                let editing = self.state.custom_edit;
                let old_id = editing
                    .and_then(|index| self.state.settings.custom.get(index))
                    .and_then(|old| custom_candidate(old).ok())
                    .map(|old| old.id);
                let message = match editing {
                    Some(index) if index < self.state.settings.custom.len() => {
                        self.state.settings.custom[index] = resolver;
                        // An edit can change the endpoint and therefore the candidate ID.
                        // Remove the stale selection so the old resolver is not tested.
                        if let Some(old_id) = old_id
                            && old_id != candidate.id
                        {
                            self.state.settings.selected.remove(&old_id);
                        }
                        format!("Updated {}.", candidate.display())
                    }
                    _ => {
                        self.state.settings.custom.push(resolver);
                        format!("Added {}.", candidate.display())
                    }
                };
                self.state.settings.selected.insert(candidate.id.clone());
                self.state.custom_open = false;
                self.state.custom_edit = None;
                self.state.custom_draft = Default::default();
                self.rebuild_catalog();
                match self.backend.save_settings(&self.state.settings) {
                    Ok(()) => self.state.notify(message, false),
                    Err(error) => self.state.notify(error, true),
                }
            }
            Err(error) => {
                self.state.custom_draft.error = Some(error);
            }
        }
    }

    fn export_result(&mut self, format: super::actions::ReportFormat) {
        if !self.cx.capabilities.can_export_files {
            self.state.notify(
                "Exporting files is not available on Android. Share the report instead.",
                true,
            );
            return;
        }
        let Some(result) = self.state.result.clone() else {
            self.state.notify("Run a benchmark before exporting.", true);
            return;
        };
        match self.backend.export(format, &result) {
            Ok(path) => self
                .state
                .notify(format!("Saved {}", path.display()), false),
            Err(error) => self
                .state
                .notify(format!("Could not export: {error}"), true),
        }
    }

    fn export_history(&mut self, index: usize, format: super::actions::ReportFormat) {
        let Some(entry) = self.state.history.get(index) else {
            self.state
                .notify("That history entry no longer exists.", true);
            return;
        };
        if !self.cx.capabilities.can_export_files {
            self.state.notify(
                "Exporting files is not available on Android. Share the report instead.",
                true,
            );
            return;
        }
        let result = result_from_history(entry);
        match self.backend.export(format, &result) {
            Ok(path) => self
                .state
                .notify(format!("Saved {}", path.display()), false),
            Err(error) => self
                .state
                .notify(format!("Could not export: {error}"), true),
        }
    }

    fn share_report(&mut self, format: super::actions::ReportFormat) {
        let Some(result) = self.state.result.clone() else {
            self.state
                .notify("Run a benchmark before sharing a report.", true);
            return;
        };
        let text = crate::report::summary_text(&result);
        let subject = format!("dnsrace report ({})", format.label());
        match self.backend.share_text(&subject, &text) {
            Ok(()) => {
                let message = if self.cx.capabilities.is_android() {
                    "Report shared."
                } else {
                    "Report copied to the clipboard."
                };
                self.state.notify(message, false);
            }
            Err(error) => self
                .state
                .notify(format!("Could not share the report: {error}"), true),
        }
    }

    fn share_history(&mut self, index: usize, format: super::actions::ReportFormat) {
        let Some(entry) = self.state.history.get(index) else {
            self.state
                .notify("That history entry no longer exists.", true);
            return;
        };
        let result = result_from_history(entry);
        let text = crate::report::summary_text(&result);
        let subject = format!("dnsrace history report ({})", format.label());
        match self.backend.share_text(&subject, &text) {
            Ok(()) => {
                let message = if self.cx.capabilities.is_android() {
                    "Report shared."
                } else {
                    "Report copied to the clipboard."
                };
                self.state.notify(message, false);
            }
            Err(error) => self
                .state
                .notify(format!("Could not share the report: {error}"), true),
        }
    }

    fn rebuild_catalog(&mut self) {
        let mut pool = Pool::default();
        let catalog = providers::catalog(self.state.settings.protocol, self.state.settings.family);
        if self.state.settings.include_system && self.state.settings.protocol.is_plain() {
            let system = self.state.system.clone().unwrap_or_default();
            for candidate in providers::system_candidates(&system, &catalog) {
                if family_allows(&candidate, self.state.settings.family) {
                    pool.add(candidate);
                }
            }
        }
        for candidate in catalog {
            pool.add(candidate);
        }
        for custom in &self.state.settings.custom {
            let Ok(candidate) = custom_candidate(custom) else {
                continue;
            };
            let kind_ok = match self.state.settings.protocol {
                Protocol::Udp | Protocol::Tcp => candidate.kind() == Kind::Plain,
                Protocol::Dot => candidate.kind() == Kind::Dot,
                Protocol::Doh => candidate.kind() == Kind::Doh,
            };
            if kind_ok && family_allows(&candidate, self.state.settings.family) {
                pool.add(candidate);
            }
        }
        self.state.catalog = pool.into_items();
    }

    fn apply_candidates(&self) -> Vec<Candidate> {
        let mut list = providers::catalog(Protocol::Udp, Family::Any);
        for custom in &self.state.settings.custom {
            if let Ok(candidate) = custom_candidate(custom)
                && candidate.kind() == Kind::Plain
            {
                list.push(candidate);
            }
        }
        let system = self.state.system.clone().unwrap_or_default();
        list.extend(providers::system_candidates(&system, &list));
        list
    }

    fn open_guide(&mut self, id: String) {
        let api_level = self
            .cx
            .capabilities
            .android_sdk
            .unwrap_or(android_guide::PRIVATE_DNS_API);
        let Some(row) = self.state.find_row(&id).cloned() else {
            self.state
                .notify("Finish a benchmark before opening the setup guide.", true);
            return;
        };
        self.state.guide = Some(android_guide::build(api_level, &row, &self.state.catalog));
        self.state.selected_row = Some(id);
    }

    fn copy(&mut self, text: String) {
        match self.backend.copy(&text) {
            Ok(()) => self.state.notify(format!("Copied {text}"), false),
            Err(error) => self.state.notify(format!("Could not copy: {error}"), true),
        }
    }

    fn open_link(&mut self, url: String) {
        match self.backend.open_link(&url) {
            Ok(()) => self.state.notify(format!("Opened {url}"), false),
            Err(error) => self
                .state
                .notify(format!("Could not open the link: {error}"), true),
        }
    }

    fn android_call(
        &mut self,
        name: &str,
        call: impl FnOnce(&dyn crate::android::AndroidHost) -> Result<(), String>,
    ) {
        let outcome = match self.backend.android() {
            Some(host) => call(host),
            None => Err("This is only available in the Android app.".to_string()),
        };
        match outcome {
            Ok(()) => self.state.notify(format!("Opened {name}."), false),
            Err(error) => {
                let summary = concise_android_error(&error);
                self.state
                    .notify(format!("Could not open {name}: {summary}"), true);
            }
        }
    }

    /// Read the phone's system-bar insets once per viewport size. A zero result means the
    /// activity has not attached its window insets yet, so ask again next frame.
    fn refresh_safe_area(&mut self, ctx: &egui::Context) {
        let size = ctx.viewport_rect().size();
        if self.safe_area_for == Some(size) {
            return;
        }
        match self.backend.android() {
            Some(host) => {
                let safe_area = host.safe_area();
                self.cx.safe_area = safe_area;
                if !safe_area.is_zero() {
                    self.safe_area_for = Some(size);
                }
            }
            None => {
                self.cx.safe_area = crate::android::SafeArea::ZERO;
                self.safe_area_for = Some(size);
            }
        }
    }

    pub fn render(&mut self, ui: &mut egui::Ui) {
        if self.installed_theme != Some(self.state.settings.theme) {
            theme::install(ui.ctx(), self.state.settings.theme);
            self.installed_theme = Some(self.state.settings.theme);
        }
        self.refresh_safe_area(ui.ctx());
        self.poll(ui.ctx());

        let layout = if self.cx.capabilities.platform == crate::platform::Platform::Android
            || ui.available_width() < 720.0
        {
            LayoutMode::Mobile
        } else {
            LayoutMode::Desktop
        };
        self.cx.layout = layout;

        let mut actions = Vec::new();
        match layout {
            LayoutMode::Mobile => {
                mobile::render_mobile(&mut self.state, &self.cx, ui, &mut actions)
            }
            LayoutMode::Desktop => {
                desktop::render_desktop(&mut self.state, &self.cx, ui, &mut actions)
            }
        }
        modals::render(&mut self.state, &self.cx, ui.ctx(), &mut actions);

        for action in actions {
            self.dispatch(action);
        }
    }
}

impl eframe::App for App {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if self.installed_theme != Some(self.state.settings.theme) {
            theme::install(ctx, self.state.settings.theme);
            self.installed_theme = Some(self.state.settings.theme);
        }
        self.poll(ctx);
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ui, |ui| self.render(ui));
    }
}

fn join_addresses(addresses: &[IpAddr]) -> String {
    addresses
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}

/// Turn a saved history entry into the same result shape a fresh run produces.
fn result_from_history(entry: &HistoryEntry) -> RunResult {
    RunResult {
        protocol: entry.protocol,
        family: entry.family,
        duration_secs: entry.duration_secs,
        data_center: entry.data_center.clone(),
        current_dns: entry.current_dns.clone(),
        rows: entry.rows.clone(),
        steerings: entry.steerings.clone(),
        scanned: entry.scanned.max(entry.rows.len()),
        tested: entry.tested.max(entry.rows.len()),
        profile: entry.profile,
        depth: entry.depth,
    }
}

fn custom_draft_for(resolver: &CustomResolver) -> crate::gui::state::CustomDraft {
    crate::gui::state::CustomDraft {
        name: resolver.name.clone(),
        protocol: resolver.protocol,
        endpoint: resolver.endpoint.clone(),
        address: resolver.address.clone().unwrap_or_default(),
        error: None,
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

/// Whether a candidate carries an address in the selected family. Encrypted candidates with no
/// fixed address are kept because their family is decided at query time.
fn family_allows(candidate: &Candidate, family: Family) -> bool {
    match &candidate.target {
        crate::servers::Target::Plain(addr) => family.allows(addr.ip()),
        crate::servers::Target::Dot { addr, .. } => match addr {
            Some(addr) => family.allows(addr.ip()),
            None => family == Family::Any,
        },
        crate::servers::Target::Doh { .. } => true,
    }
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Settings;
    use crate::gui::actions::UiAction;
    use crate::gui::backend::{BackendCall, BackendReply, TestBackend};
    use crate::gui::state::{AppState, ViewContext};
    use crate::profile::{Profile, TestDepth};
    use std::rc::Rc;

    fn test_app() -> (App, Rc<TestBackend>) {
        let backend = Rc::new(TestBackend::new());
        let app = App::new(
            AppState::demo(),
            ViewContext::desktop_for_test(),
            Box::new(backend.clone()),
        );
        (app, backend)
    }

    #[test]
    fn navigate_changes_page() {
        let (mut app, _backend) = test_app();
        assert_eq!(app.state.page, Page::Overview);
        app.dispatch(UiAction::Navigate(Page::Servers));
        assert_eq!(app.state.page, Page::Servers);
    }

    #[test]
    fn set_profile_updates_settings_and_depth() {
        let (mut app, _backend) = test_app();
        app.dispatch(UiAction::SetProfile(Profile::Thorough));
        assert_eq!(app.state.settings.profile, Profile::Thorough);
        assert_eq!(app.state.settings.timeout_ms, 1500);
        assert_eq!(app.state.settings.jobs, 16);
        assert_eq!(app.state.settings.repeat, 2);
        assert_eq!(
            app.state.settings.depth(),
            TestDepth {
                cached_rounds: 4,
                random_lookups: 8
            }
        );
    }

    #[test]
    fn toggle_favorite_persists_in_memory() {
        let (mut app, _backend) = test_app();
        let id = "demo:Cloudflare".to_string();
        app.dispatch(UiAction::ToggleFavorite(id.clone()));
        assert!(app.state.settings.favorites.contains(&id));
        app.dispatch(UiAction::ToggleFavorite(id.clone()));
        assert!(!app.state.settings.favorites.contains(&id));
    }

    #[test]
    fn select_all_uses_visible_rows() {
        let (mut app, _backend) = test_app();
        app.state.settings.selected.clear();
        app.state.search = "Cloudflare".into();
        app.dispatch(UiAction::SelectAllVisible);
        let visible: Vec<String> = app
            .state
            .visible_catalog()
            .into_iter()
            .map(|candidate| candidate.id.clone())
            .collect();
        assert!(!visible.is_empty());
        for id in &visible {
            assert!(
                app.state.settings.selected.contains(id),
                "{id} not selected"
            );
        }
        assert_eq!(app.state.settings.selected.len(), visible.len());
    }

    #[test]
    fn test_backend_records_copy_and_open() {
        let (mut app, backend) = test_app();
        app.dispatch(UiAction::CopyCurrentDns);
        app.dispatch(UiAction::OpenWebsite("demo:Cloudflare".into()));
        let calls = backend.take_calls();
        assert!(calls.contains(&BackendCall::Copy("9.9.9.9".into())));
        assert!(calls.contains(&BackendCall::OpenLink(
            "https://example.com/Cloudflare".into()
        )));
    }

    #[test]
    fn test_backend_can_fail_copy() {
        let (mut app, backend) = test_app();
        backend.push_reply(BackendReply::Copy(Err("clipboard denied".into())));
        app.dispatch(UiAction::CopyCurrentDns);
        let notice = app.state.notices.last().expect("a failure notice");
        assert!(notice.error);
        assert!(notice.text.contains("clipboard denied"));
    }

    #[test]
    fn default_settings_exist_for_a_fresh_app() {
        let state = AppState::new(
            Settings::default(),
            Vec::new(),
            None,
            Vec::new(),
            crate::platform::Capabilities::desktop_for_test(),
        );
        assert!(!state.settings.selected.is_empty());
    }

    #[test]
    fn android_error_notices_drop_the_java_stack_trace() {
        let error = "Caught Java exception: No Activity found to handle Intent { \
                     act=android.settings.PRIVATE_DNS_SETTINGS }\nStack trace:\n0: \
                     android.app.Instrumentation.checkStartActivityResult";
        assert_eq!(
            concise_android_error(error),
            "Caught Java exception: No Activity found to handle Intent { \
             act=android.settings.PRIVATE_DNS_SETTINGS }"
        );
    }
}
