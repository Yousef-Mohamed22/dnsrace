//! The candidate servers: the built-in list, user lists, and the public-dns.info country lists.

use std::collections::HashSet;
use std::net::{IpAddr, SocketAddr};
use std::time::Duration;

use crate::dns::DohProvider;

/// The default list is compiled into the binary (see servers.txt).
pub const BUILTIN: &str = include_str!("../servers.txt");

/// Label for open resolvers that come from the country lists.
pub const OTHER: &str = "other";
/// Label for DNS servers your system uses that are not in the server list.
pub const CURRENT: &str = "current";
/// Label for servers added with `--add` (or lines without a name).
pub const CUSTOM: &str = "custom";
/// Label for your default gateway, which normally forwards to your ISP's DNS.
pub const ROUTER: &str = "Router/ISP-default";

/// One server to test.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Candidate {
    pub addr: SocketAddr,
    pub name: String,
    /// When set, this candidate is a DNS-over-HTTPS provider and is queried over HTTPS instead
    /// of UDP/TCP port 53. The `addr` field is unused for DoH candidates.
    pub doh_provider: Option<DohProvider>,
}

/// The candidates, without duplicates (the first label for an address wins).
#[derive(Default)]
pub struct Pool {
    items: Vec<Candidate>,
    seen: HashSet<SocketAddr>,
}

impl Pool {
    /// Adds a server unless it is already in the pool. Returns whether it was added.
    pub fn add(&mut self, addr: SocketAddr, name: &str) -> bool {
        if !self.seen.insert(addr) {
            return false;
        }
        self.items.push(Candidate {
            addr,
            name: name.to_string(),
            doh_provider: None,
        });
        true
    }

    /// Pushes a candidate without dedup checking (used for DoH providers that share a dummy
    /// address).
    pub fn push_raw(&mut self, c: Candidate) {
        self.items.push(c);
    }

    pub fn items(&self) -> &[Candidate] {
        &self.items
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
            Some(Candidate {
                addr,
                name,
                doh_provider: None,
            })
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

/// The two-letter country code Cloudflare associates with this connection, used by `--nearby`.
/// Best effort: `None` when the network blocks the request or the answer is unexpected.
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
        assert_eq!(s[0].name, "Cloudflare");
        assert_eq!(s[1].addr.ip(), IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8)));
        assert_eq!(s[2].name, CUSTOM);
        assert_eq!(s[3].addr.port(), 5300);
        assert_eq!(s[3].name, "Local");
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
        let unique: HashSet<_> = parsed.iter().map(|c| c.addr).collect();
        assert_eq!(
            unique.len(),
            parsed.len(),
            "duplicate addresses in servers.txt"
        );
        assert!(parsed.iter().any(|c| c.addr.is_ipv4()));
        assert!(parsed.iter().any(|c| c.addr.is_ipv6()));
        for c in &parsed {
            assert!(
                is_usable_ip(c.addr.ip()),
                "{} is not a usable server",
                c.addr
            );
            assert!(
                c.name.chars().all(|ch| ch.is_ascii_graphic()),
                "bad name {:?}",
                c.name
            );
        }
    }

    #[test]
    fn the_pool_ignores_duplicates_and_keeps_the_first_label() {
        let mut pool = Pool::default();
        assert!(pool.is_empty());
        assert!(pool.add(sa("1.1.1.1"), "Cloudflare"));
        assert!(!pool.add(sa("1.1.1.1"), CURRENT));
        assert!(pool.add(sa("1.1.1.1:5353"), "Other-port"));
        assert_eq!(pool.len(), 2);
        assert_eq!(pool.items()[0].name, "Cloudflare");
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
}
