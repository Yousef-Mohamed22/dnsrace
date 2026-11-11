//! The single application state shared by the desktop and mobile shells.
//!
//! State is plain data: pages read it, pages emit actions, and the app mutates it. That makes the
//! shells interchangeable and lets tests build a complete state without a window.

use std::net::IpAddr;

use crate::android::SafeArea;
use crate::bench::Row;
use crate::config::{HistoryEntry, Settings};
use crate::engine::{RunResult, Stage};
use crate::platform::{AppliedDns, Capabilities};
use crate::recommend::{self, Recommendation};
use crate::servers::{Candidate, Category, Family, Kind, Protocol, Target};
use crate::system::SystemInfo;

use super::actions::{Page, RunScope};

/// Which chrome wraps the shared pages.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LayoutMode {
    Desktop,
    Mobile,
}

/// Capabilities and layout handed to every page renderer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ViewContext {
    pub capabilities: Capabilities,
    pub layout: LayoutMode,
    pub version: &'static str,
    /// System bar and cutout insets queried from the phone activity. The phone shell pads its
    /// app bar and bottom navigation by these; desktop always uses [`SafeArea::ZERO`].
    pub safe_area: SafeArea,
}

impl ViewContext {
    pub fn desktop_for_test() -> Self {
        ViewContext {
            capabilities: Capabilities::desktop_for_test(),
            layout: LayoutMode::Desktop,
            version: "0.8.0",
            safe_area: SafeArea::ZERO,
        }
    }

    pub fn android_for_test(api_level: u32) -> Self {
        ViewContext {
            capabilities: Capabilities::android_for_test(api_level),
            layout: LayoutMode::Mobile,
            version: "0.8.0",
            safe_area: SafeArea::ZERO,
        }
    }

    /// An Android test context with explicit window insets.
    pub fn android_for_test_with_insets(api_level: u32, safe_area: SafeArea) -> Self {
        ViewContext {
            safe_area,
            ..ViewContext::android_for_test(api_level)
        }
    }
}

/// A short message shown in the interface. `error` changes its tone, not its lifetime.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Notice {
    pub text: String,
    pub error: bool,
}

/// The in-progress custom resolver form. Validation happens only when the user saves, so a
/// half-typed URL never blocks the dialog.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CustomDraft {
    pub name: String,
    pub protocol: Protocol,
    pub endpoint: String,
    pub address: String,
    pub error: Option<String>,
}

impl Default for CustomDraft {
    fn default() -> Self {
        CustomDraft {
            name: String::new(),
            protocol: Protocol::Doh,
            endpoint: String::new(),
            address: String::new(),
            error: None,
        }
    }
}

/// An apply request waiting for confirmation. The addresses were resolved when the dialog
/// opened, so what the user confirms is exactly what the backend receives.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PendingApply {
    pub label: String,
    pub addresses: Vec<IpAddr>,
    pub warning: Option<String>,
}

/// Everything the interface knows and shows.
#[derive(Clone, Debug)]
pub struct AppState {
    pub settings: Settings,
    pub catalog: Vec<Candidate>,
    pub system: Option<SystemInfo>,
    pub history: Vec<HistoryEntry>,
    pub applied: Option<AppliedDns>,
    pub result: Option<RunResult>,
    pub live_rows: Vec<Row>,
    pub stage: Option<Stage>,
    pub progress: (usize, usize),
    pub running: bool,
    pub page: Page,
    pub selected_row: Option<String>,
    pub guide: Option<crate::android_guide::AndroidGuide>,
    pub search: String,
    pub category_filter: Option<Category>,
    pub kind_filter: Option<Kind>,
    pub family_filter: Option<Family>,
    pub favorites_only: bool,
    pub notices: Vec<Notice>,
    pub about_open: bool,
    /// Set when About opens so the page can bring the card into view once, without fighting
    /// later scrolling.
    pub about_scroll_pending: bool,
    pub confirm_apply: bool,
    pub confirm_revert: bool,
    pub confirm_clear_history: bool,
    pub history_selected: Option<usize>,
    pub custom_open: bool,
    /// Index into `settings.custom` while the editor is changing an existing resolver.
    pub custom_edit: Option<usize>,
    /// Whether the mobile row-detail sheet is showing. Desktop keeps its persistent detail card.
    pub details_open: bool,
    /// Bumped for every fresh detail-sheet opening so its scroll area starts at the top
    /// instead of inheriting the previous sheet's offset.
    pub details_epoch: u64,
    pub custom_draft: CustomDraft,
    pub pending_apply: Option<PendingApply>,
    pub detecting: bool,
    pub applying: bool,
    pub reverting: bool,
}

impl AppState {
    pub fn new(
        settings: Settings,
        history: Vec<HistoryEntry>,
        applied: Option<AppliedDns>,
        catalog: Vec<Candidate>,
        _capabilities: Capabilities,
    ) -> Self {
        AppState {
            settings,
            catalog,
            system: None,
            history,
            applied,
            result: None,
            live_rows: Vec::new(),
            stage: None,
            progress: (0, 0),
            running: false,
            page: Page::Overview,
            selected_row: None,
            guide: None,
            search: String::new(),
            category_filter: None,
            kind_filter: None,
            family_filter: None,
            favorites_only: false,
            notices: Vec::new(),
            about_open: false,
            about_scroll_pending: false,
            confirm_apply: false,
            confirm_revert: false,
            confirm_clear_history: false,
            history_selected: None,
            custom_open: false,
            custom_edit: None,
            details_open: false,
            details_epoch: 0,
            custom_draft: CustomDraft::default(),
            pending_apply: None,
            detecting: false,
            applying: false,
            reverting: false,
        }
    }

    pub fn current_dns(&self) -> Vec<IpAddr> {
        self.system
            .as_ref()
            .map(|system| system.dns.clone())
            .unwrap_or_default()
    }

    pub fn recommendation(&self) -> Recommendation {
        self.result
            .as_ref()
            .map(|result| {
                recommend::build(result, self.selected_row.as_deref(), &self.current_dns())
            })
            .unwrap_or_default()
    }

    pub fn find_row(&self, id: &str) -> Option<&Row> {
        self.result
            .as_ref()
            .and_then(|result| result.rows.iter().find(|row| row.id == id))
            .or_else(|| self.live_rows.iter().find(|row| row.id == id))
    }

    pub fn find_candidate(&self, id: &str) -> Option<&Candidate> {
        self.catalog.iter().find(|candidate| candidate.id == id)
    }

    pub fn endpoint_for(&self, id: &str) -> Option<String> {
        self.find_row(id)
            .map(|row| row.endpoint.clone())
            .or_else(|| self.find_candidate(id).map(Candidate::endpoint))
    }

    pub fn website_for(&self, id: &str) -> Option<String> {
        self.find_row(id)
            .map(|row| row.website.clone())
            .filter(|website| !website.is_empty())
            .or_else(|| {
                self.find_candidate(id)
                    .map(|candidate| candidate.website.clone())
                    .filter(|website| !website.is_empty())
            })
    }

    /// The catalogue after the current search and filter chips.
    pub fn visible_catalog(&self) -> Vec<&Candidate> {
        let needle = self.search.trim().to_lowercase();
        self.catalog
            .iter()
            .filter(|candidate| {
                let search_matches = needle.is_empty() || {
                    let text = format!(
                        "{} {} {} {}",
                        candidate.provider,
                        candidate.label,
                        candidate.endpoint(),
                        candidate.category.label()
                    )
                    .to_lowercase();
                    text.contains(&needle)
                };
                search_matches
                    && self
                        .category_filter
                        .is_none_or(|category| candidate.category == category)
                    && self.kind_filter.is_none_or(|kind| candidate.kind() == kind)
                    && self
                        .family_filter
                        .is_none_or(|family| family_matches(candidate, family))
                    && (!self.favorites_only || self.settings.favorites.contains(&candidate.id))
            })
            .collect()
    }

    /// Selected resolvers that still exist in the current protocol catalogue. Selection ids
    /// change with the protocol, so stale ids must not enable a run with no targets.
    pub fn selected_in_catalog(&self) -> usize {
        self.catalog
            .iter()
            .filter(|candidate| self.settings.selected.contains(&candidate.id))
            .count()
    }

    /// Favourites that still exist in the current protocol catalogue.
    pub fn favourites_in_catalog(&self) -> usize {
        self.catalog
            .iter()
            .filter(|candidate| self.settings.favorites.contains(&candidate.id))
            .count()
    }

    /// The candidates a run scope covers. `All` respects the visible filters so "Test all" means
    /// the set the user is looking at.
    pub fn candidates_for_scope(&self, scope: RunScope) -> Vec<Candidate> {
        match scope {
            RunScope::All => self.visible_catalog().into_iter().cloned().collect(),
            RunScope::Selected => self
                .catalog
                .iter()
                .filter(|candidate| self.settings.selected.contains(&candidate.id))
                .cloned()
                .collect(),
            RunScope::Favorites => self
                .catalog
                .iter()
                .filter(|candidate| self.settings.favorites.contains(&candidate.id))
                .cloned()
                .collect(),
            RunScope::CurrentDns => {
                let current = self.current_dns();
                self.catalog
                    .iter()
                    .filter(|candidate| {
                        candidate
                            .plain_addr()
                            .is_some_and(|addr| current.contains(&addr.ip()))
                    })
                    .cloned()
                    .collect()
            }
        }
    }

    pub fn notify(&mut self, text: impl Into<String>, error: bool) {
        self.notices.push(Notice {
            text: text.into(),
            error,
        });
        if self.notices.len() > 6 {
            self.notices.remove(0);
        }
    }

    /// A complete state for GUI tests: two recommendable plain rows, a current DNS match and one
    /// history entry.
    #[cfg(test)]
    pub fn demo() -> AppState {
        use crate::bench::Note;
        use crate::profile::{Profile, TestDepth};
        use crate::servers::{Family, Protocol};
        use std::net::SocketAddr;

        fn demo_row(label: &str, ip: &str, score: f64) -> Row {
            Row {
                id: format!("demo:{label}"),
                provider: label.into(),
                label: label.into(),
                transport: Protocol::Udp,
                addr: Some(SocketAddr::new(ip.parse().unwrap(), 53)),
                endpoint: ip.into(),
                category: Category::General,
                description: format!("{label} demo resolver."),
                website: format!("https://example.com/{label}"),
                dot_host: Some(format!("{}.example.com", label.to_lowercase())),
                cached: Some(score),
                uncached: Some(score + 2.0),
                cached_min: Some(score - 1.0),
                cached_p95: Some(score + 3.0),
                cached_p99: Some(score + 4.0),
                cached_mean: Some(score),
                cached_max: Some(score + 5.0),
                cached_jitter: Some(0.4),
                dnssec: Some(true),
                dnssec_bogus: Some(true),
                timeouts: 0,
                note: Note::Ok,
                score,
            }
        }

        let mut settings = Settings {
            protocol: Protocol::Udp,
            ..Settings::default()
        };
        settings.selected.clear();
        let catalog = crate::providers::catalog(Protocol::Udp, Family::Any);
        let mut state = AppState::new(
            settings,
            Vec::new(),
            None,
            catalog,
            Capabilities::desktop_for_test(),
        );
        state.system = Some(SystemInfo {
            gateway: Some("192.168.1.1".parse().unwrap()),
            dns: vec!["9.9.9.9".parse().unwrap()],
            interface: Some("wlan0".into()),
            source: Some("demo".into()),
        });
        let rows = vec![
            demo_row("Cloudflare", "1.1.1.1", 10.0),
            demo_row("Quad9", "9.9.9.9", 25.0),
            demo_row("Google", "8.8.8.8", 30.0),
        ];
        state.selected_row = Some(rows[0].id.clone());
        let result = RunResult {
            protocol: Protocol::Udp,
            family: Family::Any,
            duration_secs: 1.2,
            data_center: Some("FRA".into()),
            current_dns: vec!["9.9.9.9".parse().unwrap()],
            rows: rows.clone(),
            steerings: Vec::new(),
            scanned: 3,
            tested: 3,
            profile: Some(Profile::Balanced),
            depth: Some(TestDepth::BALANCED),
        };
        state.history = vec![HistoryEntry {
            timestamp: 1,
            protocol: result.protocol,
            family: result.family,
            duration_secs: result.duration_secs,
            data_center: result.data_center.clone(),
            current_dns: result.current_dns.clone(),
            rows: result.rows.clone(),
            steerings: Vec::new(),
            scanned: result.scanned,
            tested: result.tested,
            profile: result.profile,
            depth: result.depth,
        }];
        state.live_rows = rows;
        state.result = Some(result);
        state
    }
}

/// Whether a candidate carries an address in the selected family. Encrypted candidates with no
/// fixed address (DoH, custom DoT hostnames) are kept because their family is decided at query
/// time.
fn family_matches(candidate: &Candidate, family: Family) -> bool {
    match family {
        Family::Any => true,
        Family::V4 | Family::V6 => match &candidate.target {
            Target::Plain(addr) => family.allows_addr(*addr),
            Target::Dot {
                addr: Some(addr), ..
            } => family.allows_addr(*addr),
            Target::Dot { addr: None, .. } | Target::Doh { .. } => true,
        },
    }
}
