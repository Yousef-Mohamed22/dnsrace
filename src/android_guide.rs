//! The Android setup guide model.
//!
//! Android cannot be changed by dnsrace, so the app's job is to explain the correct system
//! setting and copy the right value. This module is pure: it turns a measured row and the
//! device's API level into modes, steps, addresses and hostnames that both the UI and the tests
//! can inspect.

use std::net::IpAddr;

use crate::bench::Row;
use crate::servers::{Candidate, Protocol, is_usable_ip};

/// Private DNS became user-configurable in Android 9 (API 28).
pub const PRIVATE_DNS_API: u32 = 28;

/// The visible limitation shown on every Android guide, regardless of version.
pub const LIMIT_NOTE: &str = "Android does not let dnsrace change DNS automatically. This guide only opens settings and copies values.";

/// Which Android setting the guide explains.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GuideMode {
    /// Android 9+: a Private DNS provider hostname.
    PrivateDns,
    /// Android 8.0–8.1: per-network Wi-Fi DNS with plain addresses.
    WifiDns,
    /// The selected resolver has nothing the guide can apply.
    NoHostname,
}

/// A complete, testable description of the Android setup guide for one row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AndroidGuide {
    pub api_level: u32,
    pub mode: GuideMode,
    /// The DoT hostname for Private DNS, when one is known.
    pub hostname: Option<String>,
    /// Whether the hostname came from a DoT measurement in this run rather than provider
    /// metadata attached to a plain or DoH result.
    pub hostname_is_measured: bool,
    /// Plain IPv4 addresses for the Android 8.0–8.1 Wi-Fi flow.
    pub plain_ips: Vec<IpAddr>,
    pub steps: Vec<String>,
    pub note: String,
    /// No hostname is available; the UI should offer to benchmark DoT servers.
    pub needs_dot_run: bool,
    /// No plain address is available; the UI should offer to benchmark plain DNS.
    pub needs_plain_run: bool,
}

fn private_dns_steps() -> Vec<String> {
    vec![
        "Copy the resolver's DoT hostname (for example cloudflare-dns.com).".into(),
        "Open Settings → Network & internet → Private DNS with a best-effort intent.".into(),
        "Choose Private DNS provider hostname and paste the hostname.".into(),
        "Save, then re-run dnsrace to confirm the result.".into(),
    ]
}

fn wifi_steps() -> Vec<String> {
    vec![
        "Open Wi-Fi settings.".into(),
        "Long-press the connected network and choose Modify network → Advanced options.".into(),
        "Change IP settings to Static.".into(),
        "Enter the resolver's IPv4 addresses in DNS 1/DNS 2.".into(),
        "Save and reconnect, then re-run dnsrace.".into(),
    ]
}

/// A plain address carried by the row itself, usable on Android 8's static-DNS screen.
fn row_plain_ip(row: &Row) -> Option<IpAddr> {
    let addr = row.addr?;
    if addr.port() == 53 && addr.is_ipv4() && is_usable_ip(addr.ip()) {
        Some(addr.ip())
    } else {
        None
    }
}

fn catalogue_plain_ips(row: &Row, catalogue: &[Candidate], exact_label: bool) -> Vec<IpAddr> {
    let mut ips = Vec::new();
    for candidate in catalogue {
        let matches = if exact_label {
            candidate.label == row.label
        } else {
            candidate.provider == row.provider
        };
        if !matches {
            continue;
        }
        let Some(addr) = candidate.plain_addr() else {
            continue;
        };
        if addr.port() != 53 || !addr.is_ipv4() || !is_usable_ip(addr.ip()) {
            continue;
        }
        if !ips.contains(&addr.ip()) {
            ips.push(addr.ip());
        }
    }
    ips
}

/// Plain addresses for an encrypted row: the provider's exact variant first, then the provider
/// group as a fallback. Deduplicated and IPv4 only, because the Wi-Fi DNS screen asks for IPv4
/// addresses in DNS 1/DNS 2.
fn catalogue_ips(row: &Row, catalogue: &[Candidate]) -> Vec<IpAddr> {
    let exact = catalogue_plain_ips(row, catalogue, true);
    if exact.is_empty() {
        catalogue_plain_ips(row, catalogue, false)
    } else {
        exact
    }
}

/// Build the guide for a device API level and a row from the finished run.
pub fn build(api_level: u32, row: &Row, catalogue: &[Candidate]) -> AndroidGuide {
    let note = LIMIT_NOTE.to_string();
    if api_level >= PRIVATE_DNS_API {
        let hostname = row.dot_host.clone();
        let mode = if hostname.is_some() {
            GuideMode::PrivateDns
        } else {
            GuideMode::NoHostname
        };
        return AndroidGuide {
            api_level,
            mode,
            hostname_is_measured: mode == GuideMode::PrivateDns && row.transport == Protocol::Dot,
            hostname,
            plain_ips: row_plain_ip(row).into_iter().collect(),
            steps: private_dns_steps(),
            note,
            needs_dot_run: mode == GuideMode::NoHostname,
            needs_plain_run: false,
        };
    }

    let mut plain_ips: Vec<IpAddr> = row_plain_ip(row).into_iter().collect();
    if plain_ips.is_empty() {
        plain_ips = catalogue_ips(row, catalogue);
    }
    let mode = if plain_ips.is_empty() {
        GuideMode::NoHostname
    } else {
        GuideMode::WifiDns
    };
    AndroidGuide {
        api_level,
        mode,
        hostname: None,
        hostname_is_measured: false,
        plain_ips,
        steps: wifi_steps(),
        note,
        needs_dot_run: false,
        needs_plain_run: mode == GuideMode::NoHostname,
    }
}

impl AndroidGuide {
    /// The value the Copy button puts on the clipboard: the hostname when one exists, otherwise
    /// the comma-separated plain addresses.
    pub fn copy_text(&self) -> Option<String> {
        if let Some(hostname) = &self.hostname {
            return Some(hostname.clone());
        }
        if self.plain_ips.is_empty() {
            return None;
        }
        Some(
            self.plain_ips
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(", "),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bench::Note;
    use crate::servers::{Candidate, Category, Protocol};
    use std::net::SocketAddr;

    const NOTE: &str = "Android does not let dnsrace change DNS automatically. This guide only opens settings and copies values.";

    fn row(label: &str, transport: Protocol, addr: Option<SocketAddr>) -> Row {
        Row {
            id: format!("{label}:{}", transport.label()),
            provider: label.into(),
            label: label.into(),
            transport,
            addr,
            endpoint: label.into(),
            category: Category::General,
            description: String::new(),
            website: String::new(),
            dot_host: None,
            cached: Some(10.0),
            uncached: Some(11.0),
            cached_min: Some(9.0),
            cached_p95: Some(12.0),
            cached_p99: Some(13.0),
            cached_mean: Some(10.0),
            cached_max: Some(14.0),
            cached_jitter: Some(0.5),
            dnssec: Some(true),
            dnssec_bogus: Some(true),
            timeouts: 0,
            note: Note::Ok,
            score: 10.0,
        }
    }

    fn dot_row(label: &str, host: &str) -> Row {
        let mut dot = row(label, Protocol::Dot, None);
        dot.dot_host = Some(host.into());
        dot
    }

    #[test]
    fn api_33_dot_row_gets_private_dns_steps() {
        let guide = build(33, &dot_row("Cloudflare", "cloudflare-dns.com"), &[]);
        assert_eq!(guide.api_level, 33);
        assert_eq!(guide.mode, GuideMode::PrivateDns);
        assert_eq!(guide.hostname.as_deref(), Some("cloudflare-dns.com"));
        assert!(guide.hostname_is_measured);
        assert!(!guide.needs_dot_run);
        assert!(!guide.needs_plain_run);
        assert_eq!(
            guide.steps,
            vec![
                "Copy the resolver's DoT hostname (for example cloudflare-dns.com).",
                "Open Settings → Network & internet → Private DNS with a best-effort intent.",
                "Choose Private DNS provider hostname and paste the hostname.",
                "Save, then re-run dnsrace to confirm the result.",
            ]
        );
        assert_eq!(guide.note, NOTE);
    }

    #[test]
    fn api_33_plain_row_uses_provider_dot_hostname_unmeasured() {
        let mut plain = row(
            "Cloudflare",
            Protocol::Udp,
            Some("1.1.1.1:53".parse().unwrap()),
        );
        plain.dot_host = Some("cloudflare-dns.com".into());
        let guide = build(33, &plain, &[]);
        assert_eq!(guide.mode, GuideMode::PrivateDns);
        assert_eq!(guide.hostname.as_deref(), Some("cloudflare-dns.com"));
        assert!(
            !guide.hostname_is_measured,
            "metadata hostname from a plain run is not measured"
        );
        assert_eq!(guide.plain_ips, vec!["1.1.1.1".parse::<IpAddr>().unwrap()]);
        assert!(!guide.needs_dot_run);
    }

    #[test]
    fn api_33_custom_row_without_hostname_needs_dot_run() {
        let mut custom = row("Mine", Protocol::Udp, Some("1.2.3.4:53".parse().unwrap()));
        custom.dot_host = None;
        let guide = build(33, &custom, &[]);
        assert_eq!(guide.mode, GuideMode::NoHostname);
        assert_eq!(guide.hostname, None);
        assert!(!guide.hostname_is_measured);
        assert!(guide.needs_dot_run);
        assert!(!guide.needs_plain_run);
        assert_eq!(guide.steps.len(), 4);
        assert_eq!(guide.note, NOTE);
    }

    #[test]
    fn api_26_plain_row_gets_wifi_steps() {
        let guide = build(
            26,
            &row(
                "Cloudflare",
                Protocol::Udp,
                Some("1.1.1.1:53".parse().unwrap()),
            ),
            &[],
        );
        assert_eq!(guide.mode, GuideMode::WifiDns);
        assert_eq!(guide.plain_ips, vec!["1.1.1.1".parse::<IpAddr>().unwrap()]);
        assert!(!guide.needs_plain_run);
        assert!(!guide.needs_dot_run);
        assert_eq!(
            guide.steps,
            vec![
                "Open Wi-Fi settings.",
                "Long-press the connected network and choose Modify network → Advanced options.",
                "Change IP settings to Static.",
                "Enter the resolver's IPv4 addresses in DNS 1/DNS 2.",
                "Save and reconnect, then re-run dnsrace.",
            ]
        );
        assert_eq!(guide.note, NOTE);
    }

    #[test]
    fn api_26_dot_row_uses_provider_plain_addresses() {
        let catalog = vec![
            Candidate::plain("1.1.1.1:53".parse().unwrap(), "Cloudflare", "Cloudflare"),
            Candidate::plain("1.0.0.1:53".parse().unwrap(), "Cloudflare", "Cloudflare"),
            Candidate::plain("1.1.1.1:53".parse().unwrap(), "Cloudflare", "Cloudflare"),
            Candidate::plain(
                "1.1.1.2:53".parse().unwrap(),
                "Cloudflare",
                "Cloudflare (malware)",
            ),
        ];
        let guide = build(26, &dot_row("Cloudflare", "cloudflare-dns.com"), &catalog);
        assert_eq!(guide.mode, GuideMode::WifiDns);
        assert_eq!(
            guide.plain_ips,
            vec![
                "1.1.1.1".parse::<IpAddr>().unwrap(),
                "1.0.0.1".parse::<IpAddr>().unwrap(),
            ],
            "exact label match, deduplicated, IPv4 only"
        );
        assert!(!guide.needs_plain_run);
        assert_eq!(guide.hostname, None);
    }

    #[test]
    fn api_26_row_without_plain_address_needs_plain_run() {
        let mut doh = row("Custom", Protocol::Doh, None);
        doh.dot_host = None;
        let guide = build(26, &doh, &[]);
        assert_eq!(guide.mode, GuideMode::NoHostname);
        assert!(guide.plain_ips.is_empty());
        assert!(guide.needs_plain_run);
        assert!(!guide.needs_dot_run);
        assert_eq!(guide.steps.len(), 5);
    }

    #[test]
    fn api_26_never_offers_private_dns() {
        let guide = build(26, &dot_row("Cloudflare", "cloudflare-dns.com"), &[]);
        assert_ne!(guide.mode, GuideMode::PrivateDns);
        assert_eq!(guide.hostname, None);
        assert!(!guide.needs_dot_run, "API 26 must not suggest a DoT run");
        assert!(guide.needs_plain_run, "there is no plain address to show");
    }

    #[test]
    fn api_28_is_the_private_dns_boundary() {
        let at_28 = build(28, &dot_row("Cloudflare", "cloudflare-dns.com"), &[]);
        assert_eq!(at_28.mode, GuideMode::PrivateDns);
        let at_27 = build(
            27,
            &dot_row("Cloudflare", "cloudflare-dns.com"),
            &[Candidate::plain(
                "1.1.1.1:53".parse().unwrap(),
                "Cloudflare",
                "Cloudflare",
            )],
        );
        assert_eq!(at_27.mode, GuideMode::WifiDns);
    }

    #[test]
    fn copy_text_prefers_measured_hostname() {
        let measured = dot_row("Cloudflare", "cloudflare-dns.com");
        assert_eq!(
            build(33, &measured, &[]).copy_text().as_deref(),
            Some("cloudflare-dns.com")
        );

        let mut plain = row(
            "Cloudflare",
            Protocol::Udp,
            Some("1.1.1.1:53".parse().unwrap()),
        );
        plain.dot_host = Some("cloudflare-dns.com".into());
        assert_eq!(
            build(33, &plain, &[]).copy_text().as_deref(),
            Some("cloudflare-dns.com"),
            "an inherited hostname is copied with the unmeasured label in the UI"
        );

        let mut wifi = dot_row("Cloudflare", "cloudflare-dns.com");
        wifi.addr = Some("1.1.1.1:53".parse().unwrap());
        let guide = build(26, &wifi, &[]);
        assert_eq!(guide.copy_text().as_deref(), Some("1.1.1.1"));

        let empty = build(26, &row("Mine", Protocol::Doh, None), &[]);
        assert_eq!(empty.copy_text(), None);
    }
}
