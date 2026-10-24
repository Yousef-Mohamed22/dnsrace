//! Persisted settings, benchmark history and the saved "applied DNS" state.
//!
//! Everything lives in the platform config directory (`~/.config/dnsrace` on Linux) as small
//! JSON files. Writes go through a temporary file so a crash cannot leave a half-written
//! settings file behind.

use std::collections::BTreeSet;
use std::env;
use std::fs;
use std::io;
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::bench::{Row, Steering};
use crate::platform::AppliedDns;
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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Theme {
    #[default]
    Dark,
    Light,
}

/// The settings remembered between launches.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
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
        }
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
            "{} UTC · {} · {}",
            crate::util::format_time(self.timestamp),
            self.protocol.label(),
            winner
        )
    }
}

pub fn config_dir() -> PathBuf {
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
    fs::write(&tmp, text)?;
    // Windows does not allow rename over an existing file.
    let _ = fs::remove_file(path);
    fs::rename(tmp, path)
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

/// Where an exported report goes by default.
pub fn default_export_path(extension: &str, settings: &Settings) -> PathBuf {
    let dir = settings
        .export_dir
        .clone()
        .or_else(dirs::download_dir)
        .or_else(dirs::home_dir)
        .unwrap_or_else(|| PathBuf::from("."));
    let stamp = crate::util::format_stamp(SystemTime::now());
    dir.join(format!("dnsrace-{stamp}.{extension}"))
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
        };
        assert!(entry.winner().is_some());
        assert!(entry.label().contains("Cloudflare"));
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
}
