//! Persisted settings, benchmark history and the saved "applied DNS" state.
//!
//! Everything lives in the platform config directory (`~/.config/dnsrace` on Linux) as small
//! JSON files. Writes go through a temporary file so a crash cannot leave a half-written
//! settings file behind.

use std::collections::BTreeSet;
use std::env;
use std::fs;
use std::io;
use std::io::Write as _;
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::bench::{Row, Steering};
use crate::platform::AppliedDns;
use crate::profile::{Profile, TestDepth};
use crate::providers;
use crate::servers::{Family, Protocol};

/// A user-added resolver definition. It is converted into a [`crate::servers::Candidate`] when
/// the catalogue is built.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CustomResolver {
    pub name: String,
    pub protocol: Protocol,
    /// Plain address, DoH URL or DoT hostname.
    pub endpoint: String,
    /// Optional fixed IP for DoT endpoints.
    #[serde(default)]
    pub address: Option<String>,
}

/// Colour presets offered in the interface. `System` follows the operating system's
/// light/dark preference; the rest are fixed palettes that work on every platform.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Theme {
    System,
    #[default]
    Dark,
    Light,
    Midnight,
    Nord,
    SolarizedLight,
    Dracula,
    HighContrast,
}

impl Theme {
    pub const ALL: [Theme; 8] = [
        Theme::System,
        Theme::Dark,
        Theme::Light,
        Theme::Midnight,
        Theme::Nord,
        Theme::SolarizedLight,
        Theme::Dracula,
        Theme::HighContrast,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Theme::System => "System",
            Theme::Dark => "Dark",
            Theme::Light => "Light",
            Theme::Midnight => "Midnight",
            Theme::Nord => "Nord",
            Theme::SolarizedLight => "Solarized Light",
            Theme::Dracula => "Dracula",
            Theme::HighContrast => "High contrast",
        }
    }

    /// Whether the preset is a dark palette. `System` has no fixed answer.
    pub const fn is_dark(self) -> bool {
        matches!(
            self,
            Theme::Dark | Theme::Midnight | Theme::Nord | Theme::Dracula | Theme::HighContrast
        )
    }

    pub const fn description(self) -> &'static str {
        match self {
            Theme::System => "Follow the operating system's light or dark setting.",
            Theme::Dark => "The default neutral dark palette.",
            Theme::Light => "A clean light palette for bright rooms.",
            Theme::Midnight => "Deep navy with cool blue highlights.",
            Theme::Nord => "Muted arctic blue-grey, easy on the eyes.",
            Theme::SolarizedLight => "Warm, low-contrast light palette.",
            Theme::Dracula => "Dark purple with vivid accents.",
            Theme::HighContrast => "Maximum contrast for accessibility.",
        }
    }
}

/// The settings remembered between launches.
#[derive(Clone, Debug, Serialize)]
pub struct Settings {
    pub protocol: Protocol,
    pub family: Family,
    pub timeout_ms: u64,
    pub jobs: usize,
    pub repeat: usize,
    pub include_system: bool,
    pub include_nearby: bool,
    /// One hostname per line for the steering check.
    pub hostnames: String,
    pub connect_port: u16,
    /// Candidate ids selected in the server list.
    pub selected: BTreeSet<String>,
    pub custom: Vec<CustomResolver>,
    pub export_dir: Option<PathBuf>,
    pub theme: Theme,
    /// The named benchmark preset. Custom keeps the user's own values.
    pub profile: Profile,
    /// Candidate ids starred by the user.
    pub favorites: BTreeSet<String>,
    /// Cached-name rounds in the full test.
    pub cached_rounds: usize,
    /// Random, uncacheable lookups in the full test.
    pub random_lookups: usize,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            protocol: Protocol::Udp,
            family: Family::Any,
            timeout_ms: 1000,
            jobs: 16,
            repeat: 1,
            include_system: true,
            include_nearby: false,
            hostnames: String::new(),
            connect_port: 443,
            selected: providers::default_selection(),
            custom: Vec::new(),
            export_dir: None,
            theme: Theme::Dark,
            profile: Profile::Balanced,
            favorites: BTreeSet::new(),
            cached_rounds: TestDepth::BALANCED.cached_rounds,
            random_lookups: TestDepth::BALANCED.random_lookups,
        }
    }
}

/// Deserialization helper: an old settings file has no profile or depth fields. When the profile
/// is absent, infer the closest preset from the stored benchmark values; non-default values become
/// Custom instead of being mislabelled Balanced.
#[derive(Deserialize)]
#[serde(default)]
struct SettingsData {
    protocol: Protocol,
    family: Family,
    timeout_ms: u64,
    jobs: usize,
    repeat: usize,
    include_system: bool,
    include_nearby: bool,
    hostnames: String,
    connect_port: u16,
    selected: BTreeSet<String>,
    custom: Vec<CustomResolver>,
    export_dir: Option<PathBuf>,
    theme: Theme,
    profile: Option<Profile>,
    favorites: BTreeSet<String>,
    cached_rounds: Option<usize>,
    random_lookups: Option<usize>,
}

impl Default for SettingsData {
    fn default() -> Self {
        let settings = Settings::default();
        Self {
            protocol: settings.protocol,
            family: settings.family,
            timeout_ms: settings.timeout_ms,
            jobs: settings.jobs,
            repeat: settings.repeat,
            include_system: settings.include_system,
            include_nearby: settings.include_nearby,
            hostnames: settings.hostnames,
            connect_port: settings.connect_port,
            selected: settings.selected,
            custom: settings.custom,
            export_dir: settings.export_dir,
            theme: settings.theme,
            profile: None,
            favorites: settings.favorites,
            cached_rounds: None,
            random_lookups: None,
        }
    }
}

impl<'de> Deserialize<'de> for Settings {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let data = SettingsData::deserialize(deserializer)?;
        let depth = TestDepth {
            cached_rounds: data
                .cached_rounds
                .unwrap_or(TestDepth::BALANCED.cached_rounds),
            random_lookups: data
                .random_lookups
                .unwrap_or(TestDepth::BALANCED.random_lookups),
        }
        .clamped();
        let profile = data
            .profile
            .unwrap_or_else(|| Profile::infer(data.timeout_ms, data.jobs, data.repeat, depth));
        Ok(Settings {
            protocol: data.protocol,
            family: data.family,
            timeout_ms: data.timeout_ms,
            jobs: data.jobs,
            repeat: data.repeat,
            include_system: data.include_system,
            include_nearby: data.include_nearby,
            hostnames: data.hostnames,
            connect_port: data.connect_port,
            selected: data.selected,
            custom: data.custom,
            export_dir: data.export_dir,
            theme: data.theme,
            profile,
            favorites: data.favorites,
            cached_rounds: depth.cached_rounds,
            random_lookups: depth.random_lookups,
        })
    }
}

impl Settings {
    pub fn timeout(&self) -> std::time::Duration {
        std::time::Duration::from_millis(self.timeout_ms.clamp(100, 10_000))
    }

    pub fn jobs(&self) -> usize {
        self.jobs.clamp(1, 64)
    }

    pub fn repeat(&self) -> usize {
        self.repeat.clamp(1, 5)
    }

    pub fn hostnames(&self) -> Vec<String> {
        self.hostnames
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(ToOwned::to_owned)
            .collect()
    }

    /// The depth values this settings file would send to the engine.
    pub fn depth(&self) -> TestDepth {
        TestDepth {
            cached_rounds: self.cached_rounds,
            random_lookups: self.random_lookups,
        }
        .clamped()
    }

    /// Apply a named preset. `Custom` only changes the label and keeps the current values.
    pub fn apply_profile(&mut self, profile: Profile) {
        self.profile = profile;
        if let Some(values) = profile.values() {
            self.timeout_ms = values.timeout_ms;
            self.jobs = values.jobs;
            self.repeat = values.repeat;
            self.cached_rounds = values.cached_rounds;
            self.random_lookups = values.random_lookups;
        }
    }
}

/// One completed benchmark, kept in the local history.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HistoryEntry {
    /// Unix seconds.
    pub timestamp: u64,
    pub protocol: Protocol,
    pub family: Family,
    pub duration_secs: f64,
    pub data_center: Option<String>,
    pub current_dns: Vec<IpAddr>,
    pub rows: Vec<Row>,
    pub steerings: Vec<Steering>,
    /// Candidates that were quick-scanned. Older history files default to the row count.
    #[serde(default)]
    pub scanned: usize,
    /// Candidates that completed the full test. Older history files default to the row count.
    #[serde(default)]
    pub tested: usize,
    /// The profile this run used, when the history entry was created by 0.8 or newer.
    #[serde(default)]
    pub profile: Option<Profile>,
    /// The inner depth this run used, when recorded.
    #[serde(default)]
    pub depth: Option<TestDepth>,
}

impl HistoryEntry {
    pub fn winner(&self) -> Option<&Row> {
        self.rows.iter().find(|r| r.recommendable())
    }

    pub fn label(&self) -> String {
        let winner = self
            .winner()
            .map(|r| format!("{} ({:.1} ms)", r.label, r.score))
            .unwrap_or_else(|| "no recommendation".to_string());
        format!(
            "{} · {} · {} · {} tested",
            crate::util::format_time(self.timestamp),
            self.protocol.label(),
            winner,
            self.tested.max(self.rows.len())
        )
    }
}

/// Application paths supplied by a platform entry point (Android) or tests. The override is
/// initialised once, before the first frame, so no code below it has to know whether it is on a
/// phone or a desktop.
#[derive(Clone, Debug)]
struct AppPaths {
    config_dir: PathBuf,
    export_dir: Option<PathBuf>,
}

static APP_PATHS: Mutex<Option<AppPaths>> = Mutex::new(None);

/// Point every config/export path at an application-owned directory. On Android this is called
/// with `AndroidApp::internal_data_path()` before the GUI starts.
pub fn init_paths(config_dir: PathBuf, export_dir: Option<PathBuf>) {
    let _ = fs::create_dir_all(&config_dir);
    let mut paths = APP_PATHS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    *paths = Some(AppPaths {
        config_dir,
        export_dir,
    });
}

#[cfg(test)]
pub fn reset_paths_for_test() {
    let mut paths = APP_PATHS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    *paths = None;
}

/// Tests that change the application-wide paths must hold this lock so they cannot observe or
/// reset each other's override.
#[cfg(test)]
pub fn path_test_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn app_paths() -> Option<AppPaths> {
    APP_PATHS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
}

pub fn config_dir() -> PathBuf {
    if let Some(paths) = app_paths() {
        return paths.config_dir;
    }
    if let Some(dir) = env::var_os("DNSRACE_CONFIG_DIR") {
        return PathBuf::from(dir);
    }
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("dnsrace")
}

pub fn settings_path() -> PathBuf {
    config_dir().join("settings.json")
}

pub fn history_path() -> PathBuf {
    config_dir().join("history.json")
}

pub fn applied_path() -> PathBuf {
    config_dir().join("applied.json")
}

fn read_json<T: DeserializeOwned>(path: &Path) -> Option<T> {
    let text = fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

fn write_json<T: Serialize>(path: &Path, value: &T) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("json.tmp");
    let text = serde_json::to_vec_pretty(value)?;
    let mut file = fs::File::create(&tmp)?;
    file.write_all(&text)?;
    file.flush()?;
    file.sync_all()?;
    match replace_file(&tmp, path) {
        Ok(()) => Ok(()),
        Err(error) => {
            let _ = fs::remove_file(&tmp);
            Err(error)
        }
    }
}

/// Replace `path` with `tmp`. On Unix `rename` is already atomic over an existing file. On
/// Windows it is not, so keep the old file as a backup until the new one is in place and restore
/// it if the final rename fails.
fn replace_file(tmp: &Path, path: &Path) -> io::Result<()> {
    match fs::rename(tmp, path) {
        Ok(()) => return Ok(()),
        Err(error) if !path.exists() => return Err(error),
        Err(_) => {}
    }
    let backup = path.with_extension("json.bak");
    let _ = fs::remove_file(&backup);
    fs::rename(path, &backup)?;
    match fs::rename(tmp, path) {
        Ok(()) => {
            let _ = fs::remove_file(&backup);
            Ok(())
        }
        Err(error) => {
            let _ = fs::rename(&backup, path);
            Err(error)
        }
    }
}

pub fn load_settings() -> Settings {
    read_json(&settings_path()).unwrap_or_default()
}

pub fn save_settings(settings: &Settings) -> io::Result<()> {
    write_json(&settings_path(), settings)
}

pub fn load_history() -> Vec<HistoryEntry> {
    let mut history: Vec<HistoryEntry> = read_json(&history_path()).unwrap_or_default();
    history.sort_by_key(|entry| std::cmp::Reverse(entry.timestamp));
    history.truncate(20);
    history
}

pub fn save_history(history: &[HistoryEntry]) -> io::Result<()> {
    write_json(&history_path(), &history)
}

pub fn push_history(
    mut history: Vec<HistoryEntry>,
    entry: HistoryEntry,
) -> io::Result<Vec<HistoryEntry>> {
    history.insert(0, entry);
    history.truncate(20);
    save_history(&history)?;
    Ok(history)
}

pub fn clear_history() -> io::Result<()> {
    match fs::remove_file(history_path()) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

pub fn load_applied() -> Option<AppliedDns> {
    read_json(&applied_path())
}

pub fn save_applied(applied: &AppliedDns) -> io::Result<()> {
    write_json(&applied_path(), applied)
}

pub fn clear_applied() -> io::Result<()> {
    match fs::remove_file(applied_path()) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

fn export_dir(settings: &Settings) -> PathBuf {
    app_paths()
        .and_then(|paths| paths.export_dir)
        .or_else(|| settings.export_dir.clone())
        .or_else(dirs::download_dir)
        .or_else(dirs::home_dir)
        .unwrap_or_else(|| PathBuf::from("."))
}

fn export_candidate(dir: &Path, stamp: &str, extension: &str, suffix: u64) -> PathBuf {
    if suffix == 0 {
        dir.join(format!("dnsrace-{stamp}.{extension}"))
    } else {
        dir.join(format!("dnsrace-{stamp}-{suffix}.{extension}"))
    }
}

/// Where an exported report is suggested. If a file with the same second-resolution timestamp
/// already exists, a numeric suffix is added. The actual write reserves the path with
/// `create_new`, so this is only a suggestion.
pub fn default_export_path(extension: &str, settings: &Settings) -> PathBuf {
    let dir = export_dir(settings);
    let stamp = crate::util::format_stamp(SystemTime::now());
    let mut suffix = 0u64;
    loop {
        let candidate = export_candidate(&dir, &stamp, extension, suffix);
        if !candidate.exists() {
            return candidate;
        }
        suffix = suffix.saturating_add(1);
    }
}

/// Write a report to a fresh file. `create_new` closes the race between choosing a name and
/// writing it, and the suffix retry never caps out, so an existing export is never overwritten.
pub fn write_export(extension: &str, settings: &Settings, contents: &[u8]) -> io::Result<PathBuf> {
    let dir = export_dir(settings);
    fs::create_dir_all(&dir)?;
    let stamp = crate::util::format_stamp(SystemTime::now());
    let mut suffix = 0u64;
    loop {
        let candidate = export_candidate(&dir, &stamp, extension, suffix);
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(mut file) => {
                file.write_all(contents)?;
                file.flush()?;
                return Ok(candidate);
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                suffix = suffix.saturating_add(1);
            }
            Err(error) => return Err(error),
        }
    }
}

/// A short relative description of the last applied change, for the banner in the interface.
pub fn applied_age(applied: &AppliedDns) -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let age = now.saturating_sub(applied.timestamp);
    match age {
        0..=59 => "just now".to_string(),
        60..=3599 => format!("{} min ago", age / 60),
        3600..=86_399 => format!("{} h ago", age / 3600),
        _ => format!("{} days ago", age / 86_400),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bench::{Note, SecurityStatus};

    fn sample_row() -> Row {
        Row {
            id: "plain:1.1.1.1:53".into(),
            provider: "Cloudflare".into(),
            label: "Cloudflare".into(),
            transport: Protocol::Udp,
            addr: Some("1.1.1.1:53".parse().unwrap()),
            endpoint: "1.1.1.1".into(),
            category: crate::servers::Category::General,
            description: String::new(),
            website: String::new(),
            dot_host: None,
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
    fn defaults_are_sane_and_include_a_selection() {
        let s = Settings::default();
        assert_eq!(s.protocol, Protocol::Udp);
        assert_eq!(s.family, Family::Any);
        assert!(!s.selected.is_empty());
        assert_eq!(s.timeout_ms, 1000);
        assert_eq!(s.jobs(), 16);
        assert_eq!(s.repeat(), 1);
        assert!(s.include_system);
        assert!(!s.include_nearby);
    }

    #[test]
    fn settings_round_trip_through_json() {
        let mut s = Settings {
            protocol: Protocol::Doh,
            family: Family::V6,
            timeout_ms: 2500,
            jobs: 7,
            repeat: 3,
            hostnames: "example.com\n\ncloudflare.com\n".into(),
            ..Settings::default()
        };
        s.custom.push(CustomResolver {
            name: "Mine".into(),
            protocol: Protocol::Dot,
            endpoint: "dns.example.com".into(),
            address: Some("1.2.3.4".into()),
        });
        s.selected
            .insert("doh:https://dns.example.com/dns-query".into());
        let text = serde_json::to_string(&s).unwrap();
        let back: Settings = serde_json::from_str(&text).unwrap();
        assert_eq!(back.protocol, Protocol::Doh);
        assert_eq!(back.family, Family::V6);
        assert_eq!(back.hostnames(), vec!["example.com", "cloudflare.com"]);
        assert_eq!(back.custom.len(), 1);
        assert!(
            back.selected
                .contains("doh:https://dns.example.com/dns-query")
        );
    }

    #[test]
    fn missing_fields_get_defaults_when_loading_old_settings() {
        let s: Settings = serde_json::from_str(r#"{"protocol":"Doh"}"#).unwrap();
        assert_eq!(s.protocol, Protocol::Doh);
        assert_eq!(s.family, Family::Any);
        assert_eq!(s.jobs(), 16);
        assert!(s.include_system);
    }

    #[test]
    fn history_winner_and_label_are_stable() {
        let mut row = sample_row();
        row.dnssec_bogus = Some(false);
        assert_eq!(row.security_status(), SecurityStatus::BogusAccepted);
        let entry = HistoryEntry {
            timestamp: 1_700_000_000,
            protocol: Protocol::Udp,
            family: Family::Any,
            duration_secs: 12.5,
            data_center: Some("MRS".into()),
            current_dns: vec!["1.1.1.1".parse().unwrap()],
            rows: vec![sample_row()],
            steerings: Vec::new(),
            scanned: 1,
            tested: 1,
            profile: Some(Profile::Balanced),
            depth: Some(TestDepth::BALANCED),
        };
        assert!(entry.winner().is_some());
        assert!(entry.label().contains("Cloudflare"));
        assert!(
            entry.label().contains("22:13 UTC ·"),
            "the label keeps one UTC marker: {}",
            entry.label()
        );
        assert!(
            !entry.label().contains("UTC UTC"),
            "the label must not repeat UTC: {}",
            entry.label()
        );
    }

    #[test]
    fn history_cap_and_order() {
        let entry = |timestamp| HistoryEntry {
            timestamp,
            protocol: Protocol::Udp,
            family: Family::Any,
            duration_secs: 1.0,
            data_center: None,
            current_dns: Vec::new(),
            rows: Vec::new(),
            steerings: Vec::new(),
            scanned: 0,
            tested: 0,
            profile: None,
            depth: None,
        };
        let mut list = vec![entry(1), entry(3), entry(2)];
        list.sort_by_key(|entry| std::cmp::Reverse(entry.timestamp));
        list.truncate(20);
        assert_eq!(list[0].timestamp, 3);
    }

    #[test]
    fn settings_clamp_bad_values() {
        let mut s = Settings {
            timeout_ms: 1,
            ..Settings::default()
        };
        assert_eq!(s.timeout().as_millis(), 100);
        s.timeout_ms = 99_999;
        assert_eq!(s.timeout().as_millis(), 10_000);
        s.jobs = 0;
        assert_eq!(s.jobs(), 1);
        s.jobs = 500;
        assert_eq!(s.jobs(), 64);
        s.repeat = 0;
        assert_eq!(s.repeat(), 1);
        s.repeat = 99;
        assert_eq!(s.repeat(), 5);
    }

    #[test]
    fn every_theme_has_a_label_and_a_light_or_dark_answer() {
        for theme in Theme::ALL {
            assert!(!theme.label().is_empty());
            assert!(!theme.description().is_empty());
            if theme == Theme::System {
                assert!(!theme.is_dark(), "System follows the OS");
            }
        }
        assert!(Theme::Midnight.is_dark());
        assert!(!Theme::SolarizedLight.is_dark());
    }

    #[test]
    fn old_settings_and_history_still_load() {
        let settings: Settings = serde_json::from_str(r#"{"theme":"Light"}"#).unwrap();
        assert_eq!(settings.theme, Theme::Light);
        let entry: HistoryEntry =
            serde_json::from_str(r#"{"timestamp":1,"protocol":"Udp","family":"Any","duration_secs":1.0,"current_dns":[],"rows":[],"steerings":[]}"#)
                .unwrap();
        assert_eq!(entry.scanned, 0);
        assert_eq!(entry.tested, 0);
    }

    #[test]
    fn export_paths_do_not_overwrite_an_existing_file() {
        let dir = temp_dir("export-paths");
        let settings = Settings {
            export_dir: Some(dir.clone()),
            ..Settings::default()
        };
        let first = default_export_path("json", &settings);
        fs::write(&first, "{}").unwrap();
        let second = default_export_path("json", &settings);
        assert_ne!(first, second);
        assert!(!second.exists());
        let _ = fs::remove_dir_all(&dir);
    }

    fn temp_dir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "dnsrace-config-{label}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn settings_roundtrip_keeps_profile_favorites_and_depth() {
        let mut settings = Settings {
            profile: Profile::Thorough,
            cached_rounds: 4,
            random_lookups: 8,
            ..Settings::default()
        };
        settings.favorites.insert("plain:1.1.1.1:53".into());
        let text = serde_json::to_string(&settings).unwrap();
        let back: Settings = serde_json::from_str(&text).unwrap();
        assert_eq!(back.profile, Profile::Thorough);
        assert_eq!(
            back.depth(),
            TestDepth {
                cached_rounds: 4,
                random_lookups: 8
            }
        );
        assert!(back.favorites.contains("plain:1.1.1.1:53"));
    }

    #[test]
    fn old_settings_json_loads_balanced_defaults() {
        let settings: Settings = serde_json::from_str(r#"{"protocol":"Doh"}"#).unwrap();
        assert_eq!(settings.profile, Profile::Balanced);
        assert_eq!(settings.depth(), TestDepth::BALANCED);
        assert!(settings.favorites.is_empty());
    }

    #[test]
    fn old_settings_with_non_default_benchmark_values_become_custom() {
        let settings: Settings =
            serde_json::from_str(r#"{"timeout_ms":2500,"jobs":7,"repeat":3,"protocol":"Doh"}"#)
                .unwrap();
        assert_eq!(settings.profile, Profile::Custom);
        assert_eq!(settings.timeout_ms, 2500);
        assert_eq!(settings.jobs, 7);
        assert_eq!(settings.repeat, 3);
    }

    #[test]
    fn explicit_old_profile_is_preserved() {
        let settings: Settings =
            serde_json::from_str(r#"{"profile":"Quick","timeout_ms":2500}"#).unwrap();
        assert_eq!(settings.profile, Profile::Quick);
    }

    #[test]
    fn custom_depth_survives_roundtrip() {
        let settings = Settings {
            profile: Profile::Custom,
            timeout_ms: 900,
            jobs: 9,
            repeat: 2,
            cached_rounds: 6,
            random_lookups: 11,
            ..Settings::default()
        };
        let back: Settings =
            serde_json::from_str(&serde_json::to_string(&settings).unwrap()).unwrap();
        assert_eq!(back.profile, Profile::Custom);
        assert_eq!(
            back.depth(),
            TestDepth {
                cached_rounds: 6,
                random_lookups: 11
            }
        );
    }

    #[test]
    fn atomic_write_replaces_existing_file() {
        let dir = temp_dir("atomic");
        let path = dir.join("settings.json");
        write_json(&path, &"first").unwrap();
        write_json(&path, &"second").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "\"second\"");
        assert!(!path.with_extension("json.tmp").exists());
        assert!(!path.with_extension("json.bak").exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn export_writer_never_overwrites() {
        let dir = temp_dir("export-writer");
        let settings = Settings {
            export_dir: Some(dir.clone()),
            ..Settings::default()
        };
        let first = write_export("json", &settings, b"first").unwrap();
        let second = write_export("json", &settings, b"second").unwrap();
        assert_ne!(first, second);
        assert_eq!(fs::read(&first).unwrap(), b"first");
        assert_eq!(fs::read(&second).unwrap(), b"second");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn path_override_is_used_by_config_dir() {
        let _guard = path_test_lock();
        let dir = temp_dir("override");
        let config = dir.join("config");
        let export = dir.join("export");
        init_paths(config.clone(), Some(export.clone()));
        assert_eq!(config_dir(), config);
        assert_eq!(settings_path(), config.join("settings.json"));
        let settings = Settings::default();
        assert_eq!(
            default_export_path("json", &settings).parent(),
            Some(export.as_path())
        );
        reset_paths_for_test();
        let _ = fs::remove_dir_all(&dir);
    }
}
