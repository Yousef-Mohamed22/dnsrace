//! Every user intention the interface can express.
//!
//! Pages never mutate state or perform I/O: they collect `UiAction` values and the app dispatches
//! them. That boundary is what makes every button testable without a window.

use crate::config::Theme;
use crate::profile::Profile;
use crate::servers::{Category, Family, Kind, Protocol};

/// A link offered by the About card.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AboutLink {
    Project,
    Issues,
    Licence,
}

impl AboutLink {
    pub const fn url(self) -> &'static str {
        match self {
            AboutLink::Project => "https://github.com/Yousef-Mohamed22/dnsrace",
            AboutLink::Issues => "https://github.com/Yousef-Mohamed22/dnsrace/issues",
            AboutLink::Licence => "https://github.com/Yousef-Mohamed22/dnsrace/blob/main/LICENSE",
        }
    }
}

/// The four destinations shared by desktop and mobile.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    Overview,
    Servers,
    History,
    Settings,
}

impl Page {
    pub const ALL: [Page; 4] = [Page::Overview, Page::Servers, Page::History, Page::Settings];

    pub const fn label(self) -> &'static str {
        match self {
            Page::Overview => "Overview",
            Page::Servers => "Servers",
            Page::History => "History",
            Page::Settings => "Settings",
        }
    }

    pub const fn description(self) -> &'static str {
        match self {
            Page::Overview => "Measure your connection and see what to use.",
            Page::Servers => "Choose which public resolvers to test.",
            Page::History => "Compare past runs and reopen a result.",
            Page::Settings => "Appearance, benchmark defaults and data.",
        }
    }
}

/// Which servers a run should test.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunScope {
    Selected,
    All,
    Favorites,
    CurrentDns,
}

/// A report format offered by export and share.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReportFormat {
    Json,
    Csv,
    Html,
}

impl ReportFormat {
    pub const fn extension(self) -> &'static str {
        match self {
            ReportFormat::Json => "json",
            ReportFormat::Csv => "csv",
            ReportFormat::Html => "html",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            ReportFormat::Json => "JSON",
            ReportFormat::Csv => "CSV",
            ReportFormat::Html => "HTML",
        }
    }
}

/// One user action. Copy/share/apply actions that cross the platform boundary are handled by the
/// backend; navigation and editing actions are handled by the app state directly.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UiAction {
    // Navigation and settings.
    Navigate(Page),
    Run(RunScope),
    Cancel,
    RefreshDetection,
    CopyCurrentDns,
    SetProtocol(Protocol),
    SetFamily(Family),
    SetProfile(Profile),
    SetTheme(Theme),
    SetSearch(String),
    SetCategoryFilter(Option<Category>),
    SetKindFilter(Option<Kind>),
    SetFamilyFilter(Option<Family>),
    SetFavoritesOnly(bool),
    ToggleSelected(String),
    SelectAllVisible,
    SelectNone,
    ToggleFavorite(String),
    ChooseRow(String),
    CloseDetails,

    // Android guide.
    OpenGuide(String),
    CopyGuideText,
    OpenPrivateDns,
    OpenWifi,
    CloseGuide,

    // Apply and revert.
    Apply(String),
    Revert,
    ConfirmApply,
    CancelApply,
    ConfirmRevert,
    CancelRevert,

    // Reports and history.
    Export(ReportFormat),
    ExportHistory(usize, ReportFormat),
    ShareReport(ReportFormat),
    ShareHistory(usize, ReportFormat),
    OpenHistory(usize),
    ReopenHistory(usize),
    DeleteHistory(usize),
    RequestClearHistory,
    CancelClearHistory,
    ClearHistory,

    // Custom resolvers.
    OpenCustom,
    EditCustom(usize),
    SaveCustom,
    CancelCustom,
    RemoveCustom(usize),

    // Small actions and dialogs.
    CopyEndpoint(String),
    OpenWebsite(String),
    SaveSettings,
    ResetSettings,
    SetTimeout(u64),
    SetJobs(usize),
    SetRepeat(usize),
    SetCachedRounds(usize),
    SetRandomLookups(usize),
    SetHostnames(String),
    SetIncludeSystem(bool),
    SetIncludeNearby(bool),
    OpenAbout,
    CloseAbout,
    OpenAboutLink(AboutLink),
}
