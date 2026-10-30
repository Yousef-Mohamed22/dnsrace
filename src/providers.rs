//! Provider metadata, encrypted endpoints and the built-in catalogue.
//!
//! The plain addresses live in `servers.txt`; this module adds the provider grouping, notes and
//! the DNS-over-TLS / DNS-over-HTTPS endpoints. Keeping the encrypted endpoint table here (and
//! in one place) makes it easy to audit and extend.

use std::collections::{BTreeSet, HashMap};
use std::net::{IpAddr, SocketAddr};
use std::sync::OnceLock;

use crate::servers::{
    BUILTIN, CURRENT, Candidate, Category, Family, Pool, Protocol, ROUTER, is_usable_ip,
    parse_addr, parse_servers,
};

#[derive(Clone, Copy, Debug)]
pub struct DotEndpoint {
    /// The IP address to connect to. TLS still verifies `host`.
    pub addr: &'static str,
    pub host: &'static str,
}

#[derive(Clone, Copy, Debug)]
pub struct ProviderMeta {
    /// The name used in `servers.txt`.
    pub name: &'static str,
    /// The short label shown in the interface.
    pub label: &'static str,
    /// The group used to avoid repeating the provider name in the list.
    pub group: &'static str,
    pub category: Category,
    pub note: &'static str,
    pub website: &'static str,
    /// RFC 8484 endpoint, when the provider works over HTTP/1.1 (which `ureq` speaks).
    pub doh: Option<&'static str>,
    pub dot: Option<DotEndpoint>,
}

/// The built-in provider table. Every name in `servers.txt` has an entry so the list has useful
/// labels even for providers without an encrypted endpoint.
pub const PROVIDERS: &[ProviderMeta] = &[
    ProviderMeta {
        name: "AdGuard",
        label: "AdGuard DNS",
        group: "AdGuard",
        category: Category::Security,
        note: "Blocks ads, trackers and malicious domains.",
        website: "https://adguard-dns.io",
        doh: Some("https://dns.adguard-dns.com/dns-query"),
        dot: Some(DotEndpoint {
            addr: "94.140.14.14",
            host: "dns.adguard-dns.com",
        }),
    },
    ProviderMeta {
        name: "AdGuard-unfiltered",
        label: "AdGuard DNS (unfiltered)",
        group: "AdGuard",
        category: Category::Privacy,
        note: "No filtering; encrypted DNS from AdGuard.",
        website: "https://adguard-dns.io",
        doh: Some("https://unfiltered.adguard-dns.com/dns-query"),
        dot: Some(DotEndpoint {
            addr: "94.140.14.140",
            host: "unfiltered.adguard-dns.com",
        }),
    },
    ProviderMeta {
        name: "CIRA-Shield",
        label: "CIRA Canadian Shield",
        group: "CIRA",
        category: Category::Security,
        note: "Canadian privacy and malware protection.",
        website: "https://www.cira.ca/cybersecurity-services/canadian-shield",
        doh: None, // CIRA's DoH endpoint requires HTTP/2.
        dot: Some(DotEndpoint {
            addr: "149.112.121.10",
            host: "protected.canadianshield.cira.ca",
        }),
    },
    ProviderMeta {
        name: "CleanBrowsing",
        label: "CleanBrowsing (security)",
        group: "CleanBrowsing",
        category: Category::Security,
        note: "Blocks malware, phishing and adult content.",
        website: "https://cleanbrowsing.org",
        doh: Some("https://doh.cleanbrowsing.org/doh/security-filter/"),
        dot: Some(DotEndpoint {
            addr: "185.228.168.9",
            host: "security-filter-dns.cleanbrowsing.org",
        }),
    },
    ProviderMeta {
        name: "CleanBrowsing-family",
        label: "CleanBrowsing (family)",
        group: "CleanBrowsing",
        category: Category::Family,
        note: "Family filter: blocks adult content and malware.",
        website: "https://cleanbrowsing.org",
        doh: Some("https://doh.cleanbrowsing.org/doh/family-filter/"),
        dot: Some(DotEndpoint {
            addr: "185.228.168.168",
            host: "family-filter-dns.cleanbrowsing.org",
        }),
    },
    ProviderMeta {
        name: "Cloudflare",
        label: "Cloudflare",
        group: "Cloudflare",
        category: Category::General,
        note: "Fast, privacy-focused resolver with no filtering.",
        website: "https://1.1.1.1",
        doh: Some("https://cloudflare-dns.com/dns-query"),
        dot: Some(DotEndpoint {
            addr: "1.1.1.1",
            host: "cloudflare-dns.com",
        }),
    },
    ProviderMeta {
        name: "Cloudflare-malware",
        label: "Cloudflare (malware)",
        group: "Cloudflare",
        category: Category::Security,
        note: "Blocks malware and phishing domains.",
        website: "https://1.1.1.1",
        doh: Some("https://security.cloudflare-dns.com/dns-query"),
        dot: Some(DotEndpoint {
            addr: "1.1.1.2",
            host: "cloudflare-dns.com",
        }),
    },
    ProviderMeta {
        name: "Cloudflare-family",
        label: "Cloudflare (family)",
        group: "Cloudflare",
        category: Category::Family,
        note: "Blocks malware and adult content.",
        website: "https://1.1.1.1",
        doh: Some("https://family.cloudflare-dns.com/dns-query"),
        dot: Some(DotEndpoint {
            addr: "1.1.1.3",
            host: "cloudflare-dns.com",
        }),
    },
    ProviderMeta {
        name: "Comodo",
        label: "Comodo Secure DNS",
        group: "Comodo",
        category: Category::Security,
        note: "Malware and phishing filtering.",
        website: "https://www.comodo.com",
        doh: None,
        dot: None,
    },
    ProviderMeta {
        name: "ControlD",
        label: "Control D (free)",
        group: "Control D",
        category: Category::General,
        note: "Free unfiltered resolver with a DoH endpoint.",
        website: "https://controld.com",
        doh: Some("https://freedns.controld.com/p0"),
        dot: None, // Control D's free DoT endpoint refuses port 853.
    },
    ProviderMeta {
        name: "DNS.SB",
        label: "DNS.SB",
        group: "DNS.SB",
        category: Category::Privacy,
        note: "Privacy-focused resolver; no filtering.",
        website: "https://dns.sb",
        doh: Some("https://doh.sb/dns-query"),
        dot: Some(DotEndpoint {
            addr: "185.222.222.222",
            host: "dot.sb",
        }),
    },
    ProviderMeta {
        name: "DNS.WATCH",
        label: "DNS.WATCH",
        group: "DNS.WATCH",
        category: Category::Privacy,
        note: "Privacy-focused resolver; no filtering.",
        website: "https://dns.watch",
        doh: None,
        dot: None,
    },
    ProviderMeta {
        name: "Gcore",
        label: "Gcore",
        group: "Gcore",
        category: Category::General,
        note: "Public global resolver.",
        website: "https://gcore.com",
        doh: None,
        dot: None,
    },
    ProviderMeta {
        name: "Google",
        label: "Google Public DNS",
        group: "Google",
        category: Category::General,
        note: "Fast global resolver; no filtering.",
        website: "https://dns.google",
        doh: Some("https://dns.google/dns-query"),
        dot: Some(DotEndpoint {
            addr: "8.8.8.8",
            host: "dns.google",
        }),
    },
    ProviderMeta {
        name: "HurricaneElectric",
        label: "Hurricane Electric",
        group: "Hurricane Electric",
        category: Category::General,
        note: "Public resolver from Hurricane Electric.",
        website: "https://he.net",
        doh: None,
        dot: None,
    },
    ProviderMeta {
        name: "Level3",
        label: "Level3 / CenturyLink",
        group: "Level3",
        category: Category::General,
        note: "Public resolver from Lumen.",
        website: "https://www.lumen.com",
        doh: None,
        dot: None,
    },
    ProviderMeta {
        name: "Mullvad",
        label: "Mullvad",
        group: "Mullvad",
        category: Category::Privacy,
        note: "Privacy-focused resolver; no logging.",
        website: "https://mullvad.net",
        doh: None, // Mullvad's DoH endpoint requires HTTP/2.
        dot: Some(DotEndpoint {
            addr: "194.242.2.2",
            host: "dns.mullvad.net",
        }),
    },
    ProviderMeta {
        name: "Neustar",
        label: "Neustar UltraDNS",
        group: "Neustar",
        category: Category::Security,
        note: "Public resolver with threat filtering options.",
        website: "https://www.home.neustar",
        doh: None,
        dot: None,
    },
    ProviderMeta {
        name: "OpenDNS",
        label: "OpenDNS",
        group: "OpenDNS",
        category: Category::Security,
        note: "Cisco resolver; blocks phishing by default.",
        website: "https://www.opendns.com",
        doh: Some("https://doh.opendns.com/dns-query"),
        dot: Some(DotEndpoint {
            addr: "208.67.222.222",
            host: "doh.opendns.com",
        }),
    },
    ProviderMeta {
        name: "Quad101",
        label: "Quad101 (TWNIC)",
        group: "Quad101",
        category: Category::Privacy,
        note: "Taiwanese public resolver; no logging.",
        website: "https://101.101.101.101",
        doh: None,
        dot: None,
    },
    ProviderMeta {
        name: "Quad9",
        label: "Quad9 (filtered)",
        group: "Quad9",
        category: Category::Security,
        note: "Blocks malware and validates DNSSEC.",
        website: "https://quad9.net",
        doh: None, // Quad9's DoH endpoint requires HTTP/2.
        dot: Some(DotEndpoint {
            addr: "9.9.9.9",
            host: "dns.quad9.net",
        }),
    },
    ProviderMeta {
        name: "Quad9-ECS",
        label: "Quad9 (ECS)",
        group: "Quad9",
        category: Category::General,
        note: "Quad9 with EDNS Client Subnet for closer answers.",
        website: "https://quad9.net",
        doh: None,
        dot: None,
    },
    ProviderMeta {
        name: "Quad9-unfiltered",
        label: "Quad9 (unfiltered)",
        group: "Quad9",
        category: Category::Privacy,
        note: "No filtering; still validates DNSSEC.",
        website: "https://quad9.net",
        doh: None,
        dot: Some(DotEndpoint {
            addr: "9.9.9.10",
            host: "dns.quad9.net",
        }),
    },
    ProviderMeta {
        name: "SafeDNS",
        label: "SafeDNS",
        group: "SafeDNS",
        category: Category::Security,
        note: "Content and malware filtering.",
        website: "https://www.safedns.com",
        doh: None,
        dot: None,
    },
    ProviderMeta {
        name: "UncensoredDNS",
        label: "UncensoredDNS",
        group: "UncensoredDNS",
        category: Category::Uncensored,
        note: "No filtering and no logging.",
        website: "https://blog.uncensoreddns.org",
        doh: None,
        dot: None,
    },
    ProviderMeta {
        name: "Verisign",
        label: "Verisign Public DNS",
        group: "Verisign",
        category: Category::General,
        note: "Public resolver from Verisign.",
        website: "https://publicdns.verisign.com",
        doh: None,
        dot: None,
    },
    ProviderMeta {
        name: "Yandex",
        label: "Yandex DNS",
        group: "Yandex",
        category: Category::General,
        note: "Public resolver from Yandex.",
        website: "https://dns.yandex.com",
        doh: None,
        dot: None,
    },
];

/// Metadata for a provider name from `servers.txt`.
pub fn meta(name: &str) -> Option<&'static ProviderMeta> {
    PROVIDERS.iter().find(|p| p.name == name)
}

/// Looks up the provider that owns a plain DNS address. This is what makes auto-detection
/// useful: a system resolver of `1.1.1.1` is shown as Cloudflare, not as an anonymous address.
pub fn identify_ip(ip: IpAddr) -> Option<&'static ProviderMeta> {
    static INDEX: OnceLock<HashMap<IpAddr, &'static ProviderMeta>> = OnceLock::new();
    INDEX
        .get_or_init(|| {
            let mut index = HashMap::new();
            for candidate in parse_servers(BUILTIN) {
                if let Some(addr) = candidate.plain_addr()
                    && let Some(meta) = meta(&candidate.provider)
                {
                    index.entry(addr.ip()).or_insert(meta);
                }
            }
            index
        })
        .get(&ip)
        .copied()
}

/// A human-readable provider name for an address, with a neutral fallback.
pub fn describe_ip(ip: IpAddr) -> String {
    identify_ip(ip)
        .map(|meta| meta.label.to_string())
        .unwrap_or_else(|| "Unknown resolver".to_string())
}

/// A short description such as `1.1.1.1 (Cloudflare)`, used by the interface and reports.
pub fn describe_address(ip: IpAddr) -> String {
    match identify_ip(ip) {
        Some(meta) => format!("{ip} ({})", meta.label),
        None => ip.to_string(),
    }
}

/// A catalogue entry with provider metadata applied.
fn enrich(mut candidate: Candidate) -> Candidate {
    let Some(meta) = meta(&candidate.provider) else {
        candidate.label = candidate.provider.clone();
        return candidate;
    };
    candidate.provider = meta.group.to_string();
    candidate.label = meta.label.to_string();
    candidate.category = meta.category;
    candidate.note = meta.note.to_string();
    candidate.website = meta.website.to_string();
    candidate
}

fn plain_catalog(family: Family) -> Vec<Candidate> {
    let mut out: Vec<_> = parse_servers(BUILTIN)
        .into_iter()
        .filter(|c| c.plain_addr().is_some_and(|a| family.allows_addr(a)))
        .map(enrich)
        .collect();
    sort_catalog(&mut out);
    out
}

fn dot_catalog(family: Family) -> Vec<Candidate> {
    let mut pool = Pool::default();
    for meta in PROVIDERS {
        let Some(dot) = meta.dot else {
            continue;
        };
        let Ok(ip) = dot.addr.parse::<IpAddr>() else {
            continue;
        };
        if !family.allows(ip) || !is_usable_ip(ip) {
            continue;
        }
        let addr = SocketAddr::new(ip, 853);
        let candidate = Candidate::dot(Some(addr), dot.host, 853, meta.group, meta.label)
            .with_category(meta.category)
            .with_note(meta.note)
            .with_website(meta.website);
        pool.add(candidate);
    }
    let mut out = pool.into_items();
    sort_catalog(&mut out);
    out
}

fn doh_catalog() -> Vec<Candidate> {
    let mut pool = Pool::default();
    for meta in PROVIDERS {
        let Some(url) = meta.doh else {
            continue;
        };
        let candidate = Candidate::doh(url, meta.group, meta.label)
            .with_category(meta.category)
            .with_note(meta.note)
            .with_website(meta.website);
        pool.add(candidate);
    }
    let mut out = pool.into_items();
    sort_catalog(&mut out);
    out
}

fn sort_catalog(items: &mut [Candidate]) {
    items.sort_by(|a, b| {
        a.provider
            .to_ascii_lowercase()
            .cmp(&b.provider.to_ascii_lowercase())
            .then_with(|| a.label.cmp(&b.label))
            .then_with(|| a.endpoint().cmp(&b.endpoint()))
    });
}

/// All built-in candidates for a protocol and address family.
pub fn catalog(protocol: Protocol, family: Family) -> Vec<Candidate> {
    match protocol {
        Protocol::Udp | Protocol::Tcp => plain_catalog(family),
        Protocol::Dot => dot_catalog(family),
        Protocol::Doh => doh_catalog(),
    }
}

/// The recommended starting selection: one IPv4 address from the major providers, plus every
/// encrypted endpoint. Users can change it at any time.
pub fn default_selection() -> BTreeSet<String> {
    let mut selected = BTreeSet::new();
    let groups = [
        "Cloudflare",
        "Google",
        "Quad9",
        "AdGuard",
        "OpenDNS",
        "DNS.SB",
        "Control D",
        "CleanBrowsing",
        "Mullvad",
        "CIRA",
    ];
    let plain = plain_catalog(Family::Any);
    for group in groups {
        if let Some(candidate) = plain.iter().find(|c| {
            c.provider == group
                && c.plain_addr()
                    .is_some_and(|a| a.is_ipv4() && a.port() == 53)
        }) {
            selected.insert(candidate.id.clone());
        }
    }
    for candidate in dot_catalog(Family::Any).into_iter().chain(doh_catalog()) {
        selected.insert(candidate.id);
    }
    selected
}

/// All system-found servers that are not already represented by an address in `existing`.
pub fn system_candidates(
    info: &crate::system::SystemInfo,
    existing: &[Candidate],
) -> Vec<Candidate> {
    let mut out = Vec::new();
    let known: Vec<IpAddr> = existing
        .iter()
        .filter_map(Candidate::plain_addr)
        .map(|a| a.ip())
        .collect();
    if let Some(gateway) = info.gateway
        && is_usable_ip(gateway)
        && !known.contains(&gateway)
    {
        out.push(
            Candidate::system(
                SocketAddr::new(gateway, 53),
                ROUTER,
                format!("Router / ISP default ({gateway})"),
            )
            .with_note("Your default gateway, which normally forwards to your ISP's DNS."),
        );
    }
    for ip in &info.dns {
        if known.contains(ip) || Some(*ip) == info.gateway || !is_usable_ip(*ip) {
            continue;
        }
        let candidate = match identify_ip(*ip) {
            Some(meta) => Candidate::system(
                SocketAddr::new(*ip, 53),
                meta.group,
                format!("{} — current DNS", meta.label),
            )
            .with_note(format!(
                "{} Detected as a DNS server your system uses right now.",
                meta.note
            )),
            None => Candidate::system(
                SocketAddr::new(*ip, 53),
                CURRENT,
                format!("Current DNS ({ip})"),
            )
            .with_note("A DNS server your system is using right now."),
        };
        out.push(candidate);
    }
    out
}

fn clean_name(name: &str, fallback: &str) -> String {
    let name = name.trim();
    if name.is_empty() {
        fallback.trim().to_string()
    } else {
        name.chars().take(64).collect()
    }
}

/// Validates and builds a custom plain DNS candidate.
pub fn custom_plain(name: &str, endpoint: &str) -> Result<Candidate, String> {
    let addr = parse_addr(endpoint.trim()).ok_or_else(|| {
        "Enter an IP address, optionally with a port (for example 1.1.1.1).".to_string()
    })?;
    if !is_usable_ip(addr.ip()) {
        return Err(
            "That address is loopback, link-local or otherwise not a public DNS server.".into(),
        );
    }
    let name = clean_name(name, endpoint);
    Ok(Candidate::plain(addr, name.clone(), name)
        .with_category(Category::Custom)
        .with_note("Custom resolver added by you.")
        .as_custom())
}

/// Validates and builds a custom DoT candidate. `ip` is optional; when omitted, the hostname is
/// resolved with the system resolver at test time.
pub fn custom_dot(name: &str, host: &str, ip: Option<&str>) -> Result<Candidate, String> {
    let host = host.trim();
    if host.is_empty() || host.len() > 253 || host.chars().any(|c| c.is_whitespace() || c == '/') {
        return Err("Enter a DoT hostname (for example dns.example.com).".into());
    }
    let addr = match ip.map(str::trim).filter(|s| !s.is_empty()) {
        None => None,
        Some(raw) => {
            let ip: IpAddr = raw
                .parse()
                .map_err(|_| "The optional DoT address must be a valid IP address.".to_string())?;
            if !is_usable_ip(ip) {
                return Err("The optional DoT address must be a public IP address.".into());
            }
            Some(SocketAddr::new(ip, 853))
        }
    };
    let name = clean_name(name, host);
    Ok(Candidate::dot(addr, host, 853, name.clone(), name)
        .with_category(Category::Custom)
        .with_note("Custom DNS-over-TLS endpoint added by you.")
        .as_custom())
}

/// Validates and builds a custom DoH candidate.
pub fn custom_doh(name: &str, url: &str) -> Result<Candidate, String> {
    let url = url.trim();
    if !(url.starts_with("https://") || url.starts_with("http://")) || url.len() < 9 {
        return Err("Enter a full DoH URL (for example https://dns.example.com/dns-query).".into());
    }
    if url.chars().any(char::is_whitespace) {
        return Err("The DoH URL cannot contain spaces.".into());
    }
    let host = url
        .split_once("://")
        .and_then(|(_, rest)| rest.split('/').next())
        .unwrap_or_default();
    if host.is_empty() {
        return Err("The DoH URL is missing a hostname.".into());
    }
    let name = clean_name(name, host);
    Ok(Candidate::doh(url, name.clone(), name)
        .with_category(Category::Custom)
        .with_note("Custom DNS-over-HTTPS endpoint added by you.")
        .as_custom())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_plain_server_has_metadata() {
        for candidate in parse_servers(BUILTIN) {
            let name = candidate.provider.clone();
            assert!(
                meta(&name).is_some(),
                "missing provider metadata for {name}"
            );
        }
    }

    #[test]
    fn catalogues_are_non_empty_and_unique() {
        for protocol in Protocol::all() {
            let list = catalog(protocol, Family::Any);
            assert!(!list.is_empty(), "{protocol:?}");
            let mut ids = BTreeSet::new();
            for c in &list {
                assert!(ids.insert(c.id.clone()), "duplicate id {}", c.id);
                assert!(!c.provider.is_empty());
                assert!(!c.label.is_empty());
                assert!(!c.endpoint().is_empty());
            }
        }
    }

    #[test]
    fn family_filter_applies_to_plain_and_dot() {
        let v4 = catalog(Protocol::Udp, Family::V4);
        let v6 = catalog(Protocol::Udp, Family::V6);
        assert!(v4.iter().all(|c| c.plain_addr().unwrap().is_ipv4()));
        assert!(v6.iter().all(|c| c.plain_addr().unwrap().is_ipv6()));
        assert!(v4.len() > 10);
        assert!(v6.len() > 5);
        let dot4 = catalog(Protocol::Dot, Family::V4);
        let dot6 = catalog(Protocol::Dot, Family::V6);
        assert!(
            dot4.iter()
                .all(|c| c.target.endpoint().contains('(') || c.endpoint().contains('.'))
        );
        assert!(dot6.is_empty() || dot6.iter().all(|c| c.endpoint().contains(':')));
        // DoH endpoints are hostnames and are not filtered by address family.
        assert_eq!(
            catalog(Protocol::Doh, Family::V4).len(),
            catalog(Protocol::Doh, Family::V6).len()
        );
    }

    #[test]
    fn default_selection_includes_major_plain_and_encrypted_resolvers() {
        let selected = default_selection();
        assert!(selected.len() >= 10);
        let plain = catalog(Protocol::Udp, Family::Any);
        assert!(selected.iter().any(|id| {
            plain
                .iter()
                .any(|c| &c.id == id && c.provider == "Cloudflare")
        }));
        assert!(!catalog(Protocol::Dot, Family::Any).is_empty());
        assert!(!catalog(Protocol::Doh, Family::Any).is_empty());
        assert!(
            catalog(Protocol::Doh, Family::Any)
                .iter()
                .all(|c| selected.contains(&c.id))
        );
    }

    #[test]
    fn custom_plain_rejects_private_and_accepts_public() {
        assert!(custom_plain("Mine", "127.0.0.1").is_err());
        assert!(custom_plain("Mine", "not an ip").is_err());
        let c = custom_plain("Mine", "1.2.3.4:5353").unwrap();
        assert!(c.is_custom);
        assert_eq!(c.plain_addr().unwrap().port(), 5353);
        assert_eq!(c.category, Category::Custom);
    }

    #[test]
    fn custom_dot_and_doh_validate_input() {
        let c = custom_dot("Mine", "dns.example.com", Some("1.2.3.4")).unwrap();
        assert_eq!(c.kind(), crate::servers::Kind::Dot);
        assert!(custom_dot("Mine", "", None).is_err());
        assert!(custom_dot("Mine", "dns.example.com", Some("127.0.0.1")).is_err());
        let d = custom_doh("Mine", "https://dns.example.com/dns-query").unwrap();
        assert_eq!(d.kind(), crate::servers::Kind::Doh);
        assert!(custom_doh("Mine", "dns.example.com").is_err());
        assert!(custom_doh("Mine", "https://").is_err());
    }

    #[test]
    fn system_candidates_skip_addresses_already_in_the_catalogue() {
        let info = crate::system::SystemInfo {
            gateway: Some("192.168.1.1".parse().unwrap()),
            dns: vec![
                "1.1.1.1".parse().unwrap(),
                "192.168.1.1".parse().unwrap(),
                "9.9.9.9".parse().unwrap(),
            ],
            ..Default::default()
        };
        let existing = catalog(Protocol::Udp, Family::Any);
        let found = system_candidates(&info, &existing);
        assert_eq!(found.len(), 1, "only the router is new");
        assert!(found[0].label.contains("192.168.1.1"));
    }

    #[test]
    fn detected_addresses_are_named_after_their_provider() {
        assert_eq!(
            identify_ip("1.1.1.1".parse().unwrap()).unwrap().label,
            "Cloudflare"
        );
        assert_eq!(
            identify_ip("8.8.8.8".parse().unwrap()).unwrap().label,
            "Google Public DNS"
        );
        assert!(identify_ip("203.0.113.7".parse().unwrap()).is_none());
        assert_eq!(
            describe_address("1.1.1.1".parse().unwrap()),
            "1.1.1.1 (Cloudflare)"
        );
    }

    #[test]
    fn unknown_current_dns_is_labelled_with_its_provider_when_known() {
        let info = crate::system::SystemInfo {
            gateway: Some("192.168.1.1".parse().unwrap()),
            dns: vec![
                "94.140.14.14".parse().unwrap(),
                "203.0.113.7".parse().unwrap(),
            ],
            ..Default::default()
        };
        let found = system_candidates(&info, &[]);
        assert_eq!(found.len(), 3, "router plus two current servers");
        assert!(found.iter().any(|candidate| {
            candidate.label == "AdGuard DNS — current DNS" && candidate.provider == "AdGuard"
        }));
        assert!(found.iter().any(|candidate| {
            candidate.label == "Current DNS (203.0.113.7)" && candidate.provider == CURRENT
        }));
    }
}
