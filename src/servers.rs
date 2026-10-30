//! The candidate servers and the public resolver list.
//!
//! A candidate is more than an IP address now: dnsrace can measure plain DNS (UDP or TCP),
//! DNS-over-TLS and DNS-over-HTTPS. The metadata (provider, category, notes) is carried along
//! so the interface can group and explain the list without a second lookup table.

use std::collections::HashSet;
use std::net::{IpAddr, SocketAddr};
use std::time::Duration;

/// The default list is compiled into the binary (see servers.txt).
pub const BUILTIN: &str = include_str!("../servers.txt");

/// Label for open resolvers that come from the country lists.
pub const OTHER: &str = "other";
/// Label for DNS servers your system uses that are not in the server list.
pub const CURRENT: &str = "current";
/// Label for servers added by the user.
pub const CUSTOM: &str = "custom";
/// Label for your default gateway, which normally forwards to your ISP's DNS.
pub const ROUTER: &str = "Router/ISP-default";

/// A rough category used for filtering and for the badge shown in the interface.
#[derive(
    Clone,
    Copy,
    Debug,
    Default,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    serde::Serialize,
    serde::Deserialize,
)]
pub enum Category {
    #[default]
    General,
    Security,
    Family,
    Privacy,
    Uncensored,
    System,
    Custom,
}

impl Category {
    pub const fn label(self) -> &'static str {
        match self {
            Category::General => "General",
            Category::Security => "Security",
            Category::Family => "Family",
            Category::Privacy => "Privacy",
            Category::Uncensored => "Uncensored",
            Category::System => "System",
            Category::Custom => "Custom",
        }
    }
}

/// Which transport a run uses for plain IP addresses. DoT and DoH have their own endpoints.
#[derive(
    Clone,
    Copy,
    Debug,
    Default,
    PartialEq,
    Eq,
    Hash,
    PartialOrd,
    Ord,
    serde::Serialize,
    serde::Deserialize,
)]
pub enum Protocol {
    #[default]
    Udp,
    Tcp,
    Dot,
    Doh,
}

impl Protocol {
    pub const fn label(self) -> &'static str {
        match self {
            Protocol::Udp => "UDP",
            Protocol::Tcp => "TCP",
            Protocol::Dot => "DoT",
            Protocol::Doh => "DoH",
        }
    }

    pub const fn description(self) -> &'static str {
        match self {
            Protocol::Udp => "Standard DNS on port 53, the default",
            Protocol::Tcp => "DNS over TCP, for networks that block UDP port 53",
            Protocol::Dot => "Encrypted DNS over TLS on port 853",
            Protocol::Doh => "Encrypted DNS over HTTPS (RFC 8484)",
        }
    }

    pub const fn is_plain(self) -> bool {
        matches!(self, Protocol::Udp | Protocol::Tcp)
    }

    pub const fn is_encrypted(self) -> bool {
        matches!(self, Protocol::Dot | Protocol::Doh)
    }

    pub const fn all() -> [Protocol; 4] {
        [Protocol::Udp, Protocol::Tcp, Protocol::Dot, Protocol::Doh]
    }
}

/// Which address family to test.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Family {
    #[default]
    Any,
    V4,
    V6,
}

impl Family {
    pub const fn label(self) -> &'static str {
        match self {
            Family::Any => "IPv4 + IPv6",
            Family::V4 => "IPv4",
            Family::V6 => "IPv6",
        }
    }

    pub fn allows(self, ip: IpAddr) -> bool {
        match self {
            Family::Any => true,
            Family::V4 => ip.is_ipv4(),
            Family::V6 => ip.is_ipv6(),
        }
    }

    pub fn allows_addr(self, addr: SocketAddr) -> bool {
        self.allows(addr.ip())
    }
}

/// How a candidate is queried. Plain candidates are queried with the protocol selected in the
/// interface (UDP or TCP); dot/doh candidates carry their own transport.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Plain,
    Dot,
    Doh,
}

/// Where a candidate points.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    /// A normal DNS server on port 53 (or a custom port).
    Plain(SocketAddr),
    /// DNS-over-TLS. `addr` is optional for a custom hostname; it is resolved at test time.
    Dot {
        addr: Option<SocketAddr>,
        host: String,
        port: u16,
    },
    /// DNS-over-HTTPS (RFC 8484 wire format).
    Doh { url: String },
}

impl Target {
    pub fn kind(&self) -> Kind {
        match self {
            Target::Plain(_) => Kind::Plain,
            Target::Dot { .. } => Kind::Dot,
            Target::Doh { .. } => Kind::Doh,
        }
    }

    pub fn endpoint(&self) -> String {
        match self {
            Target::Plain(addr) => fmt_addr(*addr),
            Target::Dot { addr, host, port } => {
                let suffix = if *port == 853 {
                    String::new()
                } else {
                    format!(":{port}")
                };
                match addr {
                    Some(addr) if addr.ip().to_string() != *host => {
                        format!("{host}{suffix} ({})", addr.ip())
                    }
                    _ => format!("{host}{suffix}"),
                }
            }
            Target::Doh { url } => url.clone(),
        }
    }

    pub fn plain_addr(&self) -> Option<SocketAddr> {
        match self {
            Target::Plain(addr) => Some(*addr),
            _ => None,
        }
    }
}

/// One server or encrypted resolver to test.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Candidate {
    /// Stable identifier used to remember selections between launches.
    pub id: String,
    /// The provider group, for example `Cloudflare` or `Quad9`.
    pub provider: String,
    /// The label shown in the list, for example `Cloudflare (malware)`.
    pub label: String,
    pub target: Target,
    pub category: Category,
    /// A short explanation of what the provider filters or promises.
    pub note: String,
    pub website: String,
    /// A server dnsrace found on this machine rather than in the built-in list.
    pub is_system: bool,
    /// A server the user added.
    pub is_custom: bool,
}

impl Candidate {
    pub fn plain(addr: SocketAddr, provider: impl Into<String>, label: impl Into<String>) -> Self {
        let provider = provider.into();
        let label = label.into();
        Candidate {
            id: format!("plain:{addr}"),
            provider,
            label,
            target: Target::Plain(addr),
            category: Category::General,
            note: String::new(),
            website: String::new(),
            is_system: false,
            is_custom: false,
        }
    }

    pub fn dot(
        addr: Option<SocketAddr>,
        host: impl Into<String>,
        port: u16,
        provider: impl Into<String>,
        label: impl Into<String>,
    ) -> Self {
        let host = host.into();
        let addr_part = addr
            .map(|a| a.to_string())
            .unwrap_or_else(|| "resolve".to_string());
        Candidate {
            id: format!("dot:{host}:{port}:{addr_part}"),
            provider: provider.into(),
            label: label.into(),
            target: Target::Dot { addr, host, port },
            category: Category::General,
            note: String::new(),
            website: String::new(),
            is_system: false,
            is_custom: false,
        }
    }

    pub fn doh(
        url: impl Into<String>,
        provider: impl Into<String>,
        label: impl Into<String>,
    ) -> Self {
        let url = url.into();
        Candidate {
            id: format!("doh:{url}"),
            provider: provider.into(),
            label: label.into(),
            target: Target::Doh { url },
            category: Category::General,
            note: String::new(),
            website: String::new(),
            is_system: false,
            is_custom: false,
        }
    }

    pub fn with_category(mut self, category: Category) -> Self {
        self.category = category;
        self
    }

    pub fn with_note(mut self, note: impl Into<String>) -> Self {
        self.note = note.into();
        self
    }

    pub fn with_website(mut self, website: impl Into<String>) -> Self {
        self.website = website.into();
        self
    }

    pub fn system(addr: SocketAddr, provider: impl Into<String>, label: impl Into<String>) -> Self {
        let provider = provider.into();
        let label = label.into();
        Candidate {
            id: format!("system:{addr}:{provider}"),
            provider,
            label,
            target: Target::Plain(addr),
            category: Category::System,
            note: "Found on this machine".to_string(),
            website: String::new(),
            is_system: true,
            is_custom: false,
        }
    }

    pub fn as_system(mut self) -> Self {
        self.is_system = true;
        self
    }

    pub fn as_custom(mut self) -> Self {
        self.is_custom = true;
        self
    }

    pub fn kind(&self) -> Kind {
        self.target.kind()
    }

    pub fn endpoint(&self) -> String {
        self.target.endpoint()
    }

    pub fn plain_addr(&self) -> Option<SocketAddr> {
        self.target.plain_addr()
    }

    pub fn display(&self) -> String {
        if self.label.is_empty() || self.label == self.provider {
            self.endpoint()
        } else {
            format!("{} — {}", self.label, self.endpoint())
        }
    }
}

/// Candidates without duplicates (the first label for an id wins).
#[derive(Default)]
pub struct Pool {
    items: Vec<Candidate>,
    seen: HashSet<String>,
}

impl Pool {
    /// Adds a candidate unless one with the same id is already present.
    pub fn add(&mut self, candidate: Candidate) -> bool {
        if !self.seen.insert(candidate.id.clone()) {
            return false;
        }
        self.items.push(candidate);
        true
    }

    pub fn items(&self) -> &[Candidate] {
        &self.items
    }

    pub fn into_items(self) -> Vec<Candidate> {
        self.items
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

/// Parses `1.1.1.1`, `1.1.1.1:5353`, `2606:4700:4700::1111` or `[2606:4700:4700::1111]:5353`.
/// The port defaults to 53.
pub fn parse_addr(token: &str) -> Option<SocketAddr> {
    if let Ok(ip) = token.parse::<IpAddr>() {
        return Some(SocketAddr::new(ip, 53));
    }
    token.parse::<SocketAddr>().ok()
}

/// `<address> <name>` per line; blank lines and lines starting with # are ignored.
pub fn parse_servers(text: &str) -> Vec<Candidate> {
    text.lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                return None;
            }
            let mut parts = line.split_whitespace();
            let addr = parse_addr(parts.next()?)?;
            let name = parts.next().unwrap_or(CUSTOM).to_string();
            Some(Candidate::plain(addr, name.clone(), name))
        })
        .collect()
}

/// "Known" means a named provider from the server list: not a random open resolver, an
/// address your system happens to use, or your router.
pub fn is_known(name: &str) -> bool {
    !matches!(name, OTHER | CURRENT | CUSTOM) && !name.starts_with("Router")
}

/// How an address is shown: the port only when it is not 53.
pub fn fmt_addr(addr: SocketAddr) -> String {
    if addr.port() == 53 {
        addr.ip().to_string()
    } else {
        addr.to_string()
    }
}

/// Whether an address can sensibly be tested as a DNS server. Rules out loopback and other
/// special addresses, link-local IPv6 (it needs an interface scope we don't know) and the
/// deprecated site-local IPv6 range that Windows lists as a placeholder.
pub fn is_usable_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            !(v4.is_unspecified()
                || v4.is_loopback()
                || v4.is_link_local()
                || v4.is_multicast()
                || v4.is_broadcast())
        }
        IpAddr::V6(v6) => {
            let first = v6.segments()[0];
            !(v6.is_unspecified()
                || v6.is_loopback()
                || v6.is_multicast()
                || (first & 0xffc0) == 0xfe80
                || (first & 0xffc0) == 0xfec0)
        }
    }
}

/// The two-letter country code Cloudflare associates with this connection, used by the
/// "nearby resolvers" option. Best effort: `None` when the network blocks the request or the
/// answer is unexpected.
pub fn fetch_country_code() -> Option<String> {
    let body = ureq::get("https://1.1.1.1/cdn-cgi/trace")
        .set("User-Agent", concat!("dnsrace/", env!("CARGO_PKG_VERSION")))
        .timeout(Duration::from_secs(5))
        .call()
        .ok()?
        .into_string()
        .ok()?;
    parse_country_code(&body)
}

fn parse_country_code(body: &str) -> Option<String> {
    let code = body
        .lines()
        .find_map(|line| line.trim().strip_prefix("loc="))?
        .trim()
        .to_ascii_lowercase();
    (code.len() == 2 && code.chars().all(|c| c.is_ascii_lowercase())).then_some(code)
}

/// Downloads public-dns.info's list of working open resolvers for one country.
pub fn fetch_country(cc: &str) -> Result<Vec<IpAddr>, String> {
    let url = format!("https://public-dns.info/nameserver/{cc}.txt");
    let body = ureq::get(&url)
        .set("User-Agent", concat!("dnsrace/", env!("CARGO_PKG_VERSION")))
        .timeout(Duration::from_secs(20))
        .call()
        .map_err(|e| e.to_string())?
        .into_string()
        .map_err(|e| e.to_string())?;
    Ok(parse_country_list(&body))
}

fn parse_country_list(body: &str) -> Vec<IpAddr> {
    body.lines()
        .filter_map(|l| l.trim().parse::<IpAddr>().ok())
        .filter(|ip| is_usable_ip(*ip))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{Ipv4Addr, Ipv6Addr};

    fn sa(s: &str) -> SocketAddr {
        parse_addr(s).expect("valid address")
    }

    #[test]
    fn parses_every_address_form() {
        assert_eq!(
            sa("1.1.1.1"),
            SocketAddr::new(Ipv4Addr::new(1, 1, 1, 1).into(), 53)
        );
        assert_eq!(sa("1.1.1.1:5353").port(), 5353);
        assert_eq!(sa("2606:4700:4700::1111").port(), 53);
        assert!(sa("2606:4700:4700::1111").is_ipv6());
        assert_eq!(sa("[2606:4700:4700::1111]:5353").port(), 5353);
        assert_eq!(
            sa("2a09::").ip(),
            IpAddr::V6(Ipv6Addr::new(0x2a09, 0, 0, 0, 0, 0, 0, 0))
        );
        assert!(parse_addr("not-an-ip").is_none());
        assert!(parse_addr("1.1.1").is_none());
        assert!(parse_addr("1.1.1.1:99999").is_none());
    }

    #[test]
    fn parses_server_lists() {
        let list = "# comment\n\n1.1.1.1 Cloudflare\n 8.8.8.8   Google  \nnot-an-ip Foo\n9.9.9.9\n[::1]:5300 Local\r\n";
        let s = parse_servers(list);
        assert_eq!(s.len(), 4);
        assert_eq!(s[0].provider, "Cloudflare");
        assert_eq!(
            s[1].target.plain_addr().unwrap().ip(),
            IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8))
        );
        assert_eq!(s[2].provider, CUSTOM);
        assert_eq!(s[3].target.plain_addr().unwrap().port(), 5300);
        assert_eq!(s[3].provider, "Local");
    }

    #[test]
    fn the_builtin_list_is_valid() {
        let text = BUILTIN;
        let parsed = parse_servers(text);
        let lines = text
            .lines()
            .filter(|l| !l.trim().is_empty() && !l.trim().starts_with('#'))
            .count();
        assert_eq!(parsed.len(), lines, "every line of servers.txt must parse");
        let unique: HashSet<_> = parsed.iter().filter_map(Candidate::plain_addr).collect();
        assert_eq!(
            unique.len(),
            parsed.len(),
            "duplicate addresses in servers.txt"
        );
        assert!(
            parsed
                .iter()
                .any(|c| c.plain_addr().is_some_and(|a| a.is_ipv4()))
        );
        assert!(
            parsed
                .iter()
                .any(|c| c.plain_addr().is_some_and(|a| a.is_ipv6()))
        );
        for c in &parsed {
            let addr = c.plain_addr().unwrap();
            assert!(is_usable_ip(addr.ip()), "{} is not a usable server", addr);
            assert!(
                c.provider.chars().all(|ch| ch.is_ascii_graphic()),
                "bad name {:?}",
                c.provider
            );
        }
    }

    #[test]
    fn the_pool_ignores_duplicates_and_keeps_the_first_label() {
        let mut pool = Pool::default();
        assert!(pool.is_empty());
        assert!(pool.add(Candidate::plain(sa("1.1.1.1"), "Cloudflare", "Cloudflare")));
        assert!(!pool.add(Candidate::plain(sa("1.1.1.1"), CURRENT, CURRENT)));
        assert!(pool.add(Candidate::plain(
            sa("1.1.1.1:5353"),
            "Other-port",
            "Other-port"
        )));
        assert_eq!(pool.len(), 2);
        assert_eq!(pool.items()[0].provider, "Cloudflare");
    }

    #[test]
    fn known_names() {
        assert!(is_known("Cloudflare"));
        assert!(!is_known(OTHER));
        assert!(!is_known(CURRENT));
        assert!(!is_known(CUSTOM));
        assert!(!is_known(ROUTER));
    }

    #[test]
    fn addresses_are_shown_without_the_default_port() {
        assert_eq!(fmt_addr(sa("1.1.1.1")), "1.1.1.1");
        assert_eq!(fmt_addr(sa("1.1.1.1:5353")), "1.1.1.1:5353");
        assert_eq!(fmt_addr(sa("2a09::")), "2a09::");
        assert_eq!(fmt_addr(sa("[2a09::]:5353")), "[2a09::]:5353");
    }

    #[test]
    fn unusable_addresses_are_rejected() {
        for bad in [
            "0.0.0.0",
            "127.0.0.1",
            "169.254.1.1",
            "224.0.0.1",
            "255.255.255.255",
            "::",
            "::1",
            "fe80::1",
            "fec0:0:0:ffff::1",
            "ff02::1",
        ] {
            assert!(
                !is_usable_ip(bad.parse().unwrap()),
                "{bad} should be rejected"
            );
        }
        for good in [
            "1.1.1.1",
            "192.168.1.1",
            "10.0.0.1",
            "2606:4700:4700::1111",
            "fd00::1",
        ] {
            assert!(
                is_usable_ip(good.parse().unwrap()),
                "{good} should be accepted"
            );
        }
    }

    #[test]
    fn reads_the_country_code_from_a_cloudflare_trace() {
        let trace = "fl=123abc\nh=1.1.1.1\nip=203.0.113.9\nts=1700000000.0\nvisit_scheme=https\ncolo=MRS\nloc=EG\ntls=TLSv1.3\nwarp=off\n";
        assert_eq!(parse_country_code(trace).as_deref(), Some("eg"));
        assert_eq!(parse_country_code("loc=USA\n").as_deref(), None);
        assert_eq!(parse_country_code("loc=\n").as_deref(), None);
        assert_eq!(parse_country_code("nothing here\n"), None);
    }

    #[test]
    fn country_lists_keep_only_usable_addresses() {
        let body =
            "8.8.8.8\r\n127.0.0.1\n\nnot an ip\n2001:4860:4860::8888\nfe80::1\n  9.9.9.9  \n";
        assert_eq!(
            parse_country_list(body),
            vec![
                "8.8.8.8".parse::<IpAddr>().unwrap(),
                "2001:4860:4860::8888".parse().unwrap(),
                "9.9.9.9".parse().unwrap()
            ]
        );
    }

    #[test]
    fn candidate_ids_are_stable_and_distinct() {
        let plain = Candidate::plain(sa("1.1.1.1"), "Cloudflare", "Cloudflare");
        assert_eq!(plain.id, "plain:1.1.1.1:53");
        let dot = Candidate::dot(
            Some(sa("1.1.1.1")),
            "cloudflare-dns.com",
            853,
            "Cloudflare",
            "Cloudflare",
        );
        assert!(dot.id.starts_with("dot:cloudflare-dns.com:853:"));
        let doh = Candidate::doh(
            "https://cloudflare-dns.com/dns-query",
            "Cloudflare",
            "Cloudflare",
        );
        assert_eq!(doh.id, "doh:https://cloudflare-dns.com/dns-query");
        assert_ne!(plain.id, dot.id);
        assert_eq!(doh.kind(), Kind::Doh);
    }
}
