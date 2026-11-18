//! A copyable diagnostics report for bug reports and support.
//!
//! The report contains only what the application already knows: versions, settings, the current
//! network, and the last run. It never contains hostnames, history results, or file contents.

use std::net::IpAddr;
use std::path::Path;

use crate::i18n::{fill, t};

/// The facts included in one report. Values that need formatting are formatted here so the app
/// only has to fill the struct.
pub struct Report<'a> {
    pub version: &'a str,
    pub language: &'a str,
    pub theme: &'a str,
    pub profile: &'a str,
    pub protocol: &'a str,
    pub family: &'a str,
    pub timeout_ms: u64,
    pub jobs: usize,
    pub repeat: usize,
    pub cached_rounds: usize,
    pub random_lookups: usize,
    pub current_dns: &'a [IpAddr],
    pub interface: Option<&'a str>,
    pub gateway: Option<IpAddr>,
    pub data_center: Option<&'a str>,
    pub last_run: Option<&'a str>,
    pub update: &'a str,
    pub config_dir: &'a Path,
    pub history: usize,
    pub custom: usize,
    pub favourites: usize,
}

impl Report<'_> {
    pub fn render(&self) -> String {
        let mut out = String::new();
        out.push_str("dnsrace ");
        out.push_str(self.version);
        out.push_str("\n\n");
        line(
            &mut out,
            t("Operating system"),
            &format!("{} {}", std::env::consts::OS, std::env::consts::ARCH),
        );
        line(&mut out, t("Language"), self.language);
        line(&mut out, t("Theme"), self.theme);
        line(&mut out, t("Profile"), self.profile);
        line(&mut out, t("Protocol"), self.protocol);
        line(&mut out, t("Address family"), self.family);
        line(
            &mut out,
            t("Benchmark defaults"),
            &fill(
                "{} ms · {} jobs · repeat {} · {} cached rounds · {} random lookups",
                &[
                    &self.timeout_ms,
                    &self.jobs,
                    &self.repeat,
                    &self.cached_rounds,
                    &self.random_lookups,
                ],
            ),
        );
        out.push('\n');
        line(&mut out, t("Current DNS"), &addresses(self.current_dns));
        if let Some(interface) = self.interface {
            line(&mut out, t("Interface"), interface);
        }
        if let Some(gateway) = self.gateway {
            line(&mut out, t("Gateway"), &gateway.to_string());
        }
        if let Some(data_center) = self.data_center {
            line(&mut out, t("Data center"), data_center);
        }
        out.push('\n');
        line(
            &mut out,
            t("Last run"),
            self.last_run.unwrap_or(t("No run yet")),
        );
        line(&mut out, t("Update state"), self.update);
        out.push('\n');
        line(
            &mut out,
            t("Configuration folder"),
            &self.config_dir.display().to_string(),
        );
        line(
            &mut out,
            t("Saved data"),
            &fill(
                "{} history · {} custom resolvers · {} favourites",
                &[&self.history, &self.custom, &self.favourites],
            ),
        );
        out
    }
}

fn line(out: &mut String, label: &str, value: &str) {
    out.push_str(label);
    out.push_str(": ");
    out.push_str(value);
    out.push('\n');
}

fn addresses(addresses: &[IpAddr]) -> String {
    if addresses.is_empty() {
        return t("Not detected yet").to_owned();
    }
    addresses
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn report_contains_the_version_and_no_hostnames() {
        let dns = [IpAddr::from([1, 1, 1, 1])];
        let report = Report {
            version: "0.9.0",
            language: "English",
            theme: "Dark",
            profile: "Balanced",
            protocol: "UDP",
            family: "IPv4",
            timeout_ms: 1000,
            jobs: 16,
            repeat: 1,
            cached_rounds: 3,
            random_lookups: 5,
            current_dns: &dns,
            interface: Some("eth0"),
            gateway: Some(IpAddr::from([192, 168, 1, 1])),
            data_center: Some("FRA"),
            last_run: Some("Cloudflare · 12.3 ms"),
            update: "no update is available",
            config_dir: Path::new("/tmp/dnsrace"),
            history: 2,
            custom: 1,
            favourites: 3,
        }
        .render();
        assert!(report.contains("dnsrace 0.9.0"));
        assert!(report.contains("1.1.1.1"));
        assert!(report.contains("/tmp/dnsrace"));
        assert!(!report.contains("example.com"));
    }
}
