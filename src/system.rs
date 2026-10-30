//! Finds your default gateway and the DNS servers your system uses right now.
//!
//! Everything here is best effort: if a command is missing or its output looks different from
//! what we expect, that piece is simply skipped. The parsers are plain functions over text so
//! they are tested on every platform; only the commands that produce the text depend on the OS.
//!
//! 0.7.0 adds richer detection: the active interface, the source that supplied the addresses,
//! NetworkManager's active connection (instead of every device), systemd-resolved's per-link
//! data, a Windows registry fallback when PowerShell is unavailable, and a macOS `networksetup`
//! fallback when `scutil` returns nothing.

use std::io::Read;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::servers::is_usable_ip;

/// What we learned about this machine's network setup.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct SystemInfo {
    /// The default gateway, which normally forwards to your ISP's DNS.
    pub gateway: Option<IpAddr>,
    /// The DNS servers in use.
    pub dns: Vec<IpAddr>,
    /// The active network interface or adapter, when it can be determined.
    pub interface: Option<String>,
    /// A short human-readable description of where the addresses came from, for example
    /// `systemd-resolved + NetworkManager` or `Windows registry`.
    pub source: Option<String>,
}

/// One PowerShell run that prints `IF <alias>`, `GW <address>` and `DNS <address>` lines for the
/// adapters that are up and have a default gateway. The output contains only addresses and
/// fixed English prefixes, so it does not depend on the Windows display language.
#[cfg(target_os = "windows")]
const WINDOWS_SCRIPT: &str = "Get-NetIPConfiguration | Where-Object { $_.NetAdapter.Status -eq 'Up' -and ($_.IPv4DefaultGateway -or $_.IPv6DefaultGateway) } | ForEach-Object { if ($_.InterfaceAlias) { 'IF ' + $_.InterfaceAlias }; foreach ($g in @($_.IPv4DefaultGateway) + @($_.IPv6DefaultGateway)) { if ($g) { 'GW ' + $g.NextHop } }; foreach ($d in $_.DNSServer.ServerAddresses) { 'DNS ' + $d } }";

/// Looks at the system. Blocking (it runs a few small commands), so call it from a blocking
/// context.
pub fn detect() -> SystemInfo {
    let mut info = SystemInfo::default();

    #[cfg(target_os = "windows")]
    detect_windows(&mut info);
    #[cfg(any(target_os = "linux", target_os = "android"))]
    detect_linux(&mut info);
    #[cfg(target_os = "macos")]
    detect_macos(&mut info);

    info.dns.retain(|ip| is_usable_ip(*ip));
    info.dns.sort();
    info.dns.dedup();
    info.gateway = info.gateway.filter(|ip| is_usable_ip(*ip));
    info.interface = info.interface.filter(|name| !name.trim().is_empty());
    info.source = info.source.filter(|source| !source.trim().is_empty());
    info
}

fn add_source(info: &mut SystemInfo, source: &str) {
    match info.source.as_mut() {
        Some(existing) if !existing.split(" + ").any(|part| part == source) => {
            existing.push_str(" + ");
            existing.push_str(source);
        }
        Some(_) => {}
        None => info.source = Some(source.to_string()),
    }
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn detect_linux(info: &mut SystemInfo) {
    info.interface = default_linux_interface();
    info.gateway = std::fs::read_to_string("/proc/net/route")
        .ok()
        .and_then(|text| parse_proc_net_route(&text))
        .or_else(|| {
            run("ip", &["route", "show", "default"]).and_then(|text| parse_default_gateway(&text))
        });

    // systemd-resolved knows which DNS servers belong to which link. Prefer the default
    // interface so Docker, VPN and virtual bridges do not pollute the answer.
    if let Some(text) = run("resolvectl", &["dns"]) {
        let links = parse_resolvectl_links(&text);
        let selected = select_resolvectl_dns(&links, info.interface.as_deref());
        if !selected.is_empty() {
            info.dns.extend(selected);
            add_source(info, "systemd-resolved");
        }
    }

    // NetworkManager: ask the active connection, not every device.
    if let Some(text) = run(
        "nmcli",
        &["-t", "-f", "NAME,DEVICE", "connection", "show", "--active"],
    ) {
        let connections = parse_nmcli_active(&text);
        let selected = info
            .interface
            .as_deref()
            .and_then(|device| {
                connections
                    .iter()
                    .find(|(_, connection_device)| connection_device == device)
            })
            .or_else(|| connections.first());
        if let Some((name, _)) = selected
            && let Some(values) = run(
                "nmcli",
                &["-g", "IP4.DNS,IP6.DNS", "connection", "show", name],
            )
        {
            let addresses = parse_ip_list(&values);
            if !addresses.is_empty() {
                info.dns.extend(addresses);
                add_source(info, "NetworkManager");
            }
        }
    }

    // Fallbacks that work even on minimal systems. The systemd-resolved stub address is
    // loopback and is removed by `detect`, but reading the upstream file gives the real servers.
    let mut resolv_sources = Vec::new();
    for (path, label) in [
        ("/etc/resolv.conf", "resolv.conf"),
        (
            "/run/systemd/resolve/resolv.conf",
            "systemd-resolved upstream",
        ),
    ] {
        if let Ok(text) = std::fs::read_to_string(path) {
            let addresses = parse_resolv_conf(&text);
            if !addresses.is_empty() {
                info.dns.extend(addresses);
                resolv_sources.push(label);
            }
        }
    }
    for source in resolv_sources {
        add_source(info, source);
    }
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn default_linux_interface() -> Option<String> {
    if let Some(text) = run("ip", &["route", "show", "default"])
        && let Some(device) = parse_default_device(&text)
    {
        return Some(device);
    }
    let text = std::fs::read_to_string("/proc/net/route").ok()?;
    text.lines().skip(1).find_map(|line| {
        let fields: Vec<&str> = line.split_whitespace().collect();
        (fields.len() >= 2 && fields[1] == "00000000").then(|| fields[0].to_string())
    })
}

#[cfg(target_os = "windows")]
fn detect_windows(info: &mut SystemInfo) {
    let script = ["-NoProfile", "-NonInteractive", "-Command", WINDOWS_SCRIPT];
    let output = run("powershell", &script).or_else(|| run("pwsh", &script));
    if let Some(text) = output {
        let (interfaces, gateways, dns) = parse_windows_netconfig(&text);
        info.interface = interfaces.first().cloned();
        info.gateway = gateways
            .iter()
            .copied()
            .find(IpAddr::is_ipv4)
            .or_else(|| gateways.first().copied());
        if !dns.is_empty() {
            info.dns = dns;
            add_source(info, "PowerShell");
        }
    }

    // PowerShell can be disabled by policy. The registry stores the same values and its value
    // names are language-independent, so use it as a fallback.
    if info.dns.is_empty() {
        let mut registry_dns = Vec::new();
        for key in [
            r"HKLM\SYSTEM\CurrentControlSet\Services\Tcpip\Parameters\Interfaces",
            r"HKLM\SYSTEM\CurrentControlSet\Services\Tcpip6\Parameters\Interfaces",
        ] {
            for value in ["NameServer", "DhcpNameServer"] {
                if let Some(text) = run("reg", &["query", key, "/s", "/v", value]) {
                    registry_dns.extend(parse_reg_nameservers(&text));
                }
            }
        }
        registry_dns.retain(|ip| is_usable_ip(*ip));
        registry_dns.sort();
        registry_dns.dedup();
        if !registry_dns.is_empty() {
            info.dns = registry_dns;
            add_source(info, "Windows registry");
        }
    }
}

#[cfg(target_os = "macos")]
fn detect_macos(info: &mut SystemInfo) {
    if let Some(text) = run("route", &["-n", "get", "default"]) {
        info.gateway = parse_route_get_default(&text);
        info.interface = parse_route_get_interface(&text);
    }
    if let Some(text) = run("scutil", &["--dns"]) {
        let addresses = parse_scutil_dns(&text);
        if !addresses.is_empty() {
            info.dns.extend(addresses);
            add_source(info, "scutil");
        }
    }
    if info.dns.is_empty()
        && let Some(text) = run("networksetup", &["-listnetworkserviceorder"])
    {
        let services = parse_macos_services(&text);
        let service = info
            .interface
            .as_deref()
            .and_then(|device| {
                services
                    .iter()
                    .find(|(_, service_device)| service_device == device)
                    .map(|(service, _)| service.clone())
            })
            .or_else(|| services.first().map(|(service, _)| service.clone()));
        if let Some(service) = service
            && let Some(text) = run("networksetup", &["-getdnsservers", &service])
        {
            let addresses = parse_networksetup_dns(&text);
            if !addresses.is_empty() {
                info.dns.extend(addresses);
                add_source(info, "networksetup");
            }
        }
    }
    if let Ok(text) = std::fs::read_to_string("/etc/resolv.conf") {
        let addresses = parse_resolv_conf(&text);
        if !addresses.is_empty() {
            info.dns.extend(addresses);
            add_source(info, "resolv.conf");
        }
    }
}

/// Runs a command and returns its output, or `None` if it is missing, fails or takes longer
/// than three seconds.
fn run(cmd: &str, args: &[&str]) -> Option<String> {
    let mut command = Command::new(cmd);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let mut child = command.spawn().ok()?;
    let mut stdout = child.stdout.take()?;
    // drain the pipe on a thread so a chatty command can never block on a full pipe
    let reader = std::thread::spawn(move || {
        // Platform tools do not always emit UTF-8 (Windows PowerShell uses the console code
        // page), and a non-UTF-8 interface name must not discard the addresses after it.
        let mut bytes = Vec::new();
        let _ = stdout.read_to_end(&mut bytes);
        String::from_utf8_lossy(&bytes).into_owned()
    });
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let text = reader.join().ok()?;
                return status.success().then_some(text);
            }
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(10)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
}

// ---------------------------------------------------------------------------------------
// parsers
// ---------------------------------------------------------------------------------------

/// One address from a word like `1.1.1.1`, `1.1.1.1#cloudflare-dns.com`, `fe80::1%eth0`,
/// `1.1.1.1:53` or `[2606::1111]:53`. Any port is dropped.
pub fn parse_ip_token(token: &str) -> Option<IpAddr> {
    let token = token.replace("\\:", ":");
    let token = token.split('#').next()?;
    let token = token.split('%').next()?;
    let token = token.trim_matches(|c: char| matches!(c, ',' | ';' | '(' | ')'));
    if let Ok(ip) = token.parse::<IpAddr>() {
        return Some(ip);
    }
    token.parse::<SocketAddr>().ok().map(|s| s.ip())
}

/// The default gateway from Linux's `/proc/net/route`.
pub fn parse_proc_net_route(text: &str) -> Option<IpAddr> {
    for line in text.lines().skip(1) {
        let f: Vec<&str> = line.split_whitespace().collect();
        if f.len() >= 4 && f[1] == "00000000" {
            let flags = u32::from_str_radix(f[3], 16).ok()?;
            if flags & 2 != 0 {
                // the kernel prints the address as a little-endian hex number
                let gw = u32::from_str_radix(f[2], 16).ok()?;
                return Some(IpAddr::V4(Ipv4Addr::from(gw.to_le_bytes())));
            }
        }
    }
    None
}

/// The `dev` field of `ip route show default`, for example `default via 192.168.1.1 dev eno1`.
pub fn parse_default_device(text: &str) -> Option<String> {
    for line in text.lines() {
        if line.split_whitespace().next() != Some("default") {
            continue;
        }
        let fields: Vec<&str> = line.split_whitespace().collect();
        if let Some(index) = fields.iter().position(|field| *field == "dev") {
            return fields.get(index + 1).map(|device| (*device).to_string());
        }
    }
    None
}

/// The gateway from `ip route show default`.
pub fn parse_default_gateway(text: &str) -> Option<IpAddr> {
    for line in text.lines() {
        if line.split_whitespace().next() != Some("default") {
            continue;
        }
        let fields: Vec<&str> = line.split_whitespace().collect();
        if let Some(index) = fields.iter().position(|field| *field == "via") {
            return fields
                .get(index + 1)
                .and_then(|value| parse_ip_token(value));
        }
    }
    None
}

/// `nameserver` lines of `/etc/resolv.conf`.
pub fn parse_resolv_conf(text: &str) -> Vec<IpAddr> {
    text.lines()
        .filter_map(|line| {
            let mut words = line.split_whitespace();
            if words.next()? != "nameserver" {
                return None;
            }
            parse_ip_token(words.next()?)
        })
        .collect()
}

/// One link from `resolvectl dns`: a name (or `None` for the global section) and its servers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvectlLink {
    pub name: Option<String>,
    pub dns: Vec<IpAddr>,
}

/// The output of `resolvectl dns`: lines like `Link 2 (eth0): 1.1.1.1 2606:4700:4700::1111`
/// and a `Global:` section.
pub fn parse_resolvectl_links(text: &str) -> Vec<ResolvectlLink> {
    text.lines()
        .filter_map(|line| {
            let (head, rest) = line.split_once(':')?;
            let head = head.trim();
            if head.is_empty() {
                return None;
            }
            let name = head
                .strip_prefix("Link ")
                .and_then(|value| value.split_once('('))
                .map(|(_, name)| name.trim_end_matches(')').trim().to_string())
                .filter(|name| !name.is_empty());
            let dns: Vec<IpAddr> = rest.split_whitespace().filter_map(parse_ip_token).collect();
            if dns.is_empty() && name.is_none() {
                return None;
            }
            Some(ResolvectlLink { name, dns })
        })
        .collect()
}

/// Chooses the DNS servers for the default interface, then the global section, then (only when
/// no interface is known) every link.
pub fn select_resolvectl_dns(links: &[ResolvectlLink], interface: Option<&str>) -> Vec<IpAddr> {
    if let Some(interface) = interface
        && let Some(link) = links
            .iter()
            .find(|link| link.name.as_deref() == Some(interface))
        && !link.dns.is_empty()
    {
        return link.dns.clone();
    }
    if let Some(global) = links.iter().find(|link| link.name.is_none())
        && !global.dns.is_empty()
    {
        return global.dns.clone();
    }
    links
        .iter()
        .flat_map(|link| link.dns.iter().copied())
        .collect()
}

/// The output of `resolvectl dns`, flattened. Kept for callers that only need every address.
pub fn parse_resolvectl_dns(text: &str) -> Vec<IpAddr> {
    parse_resolvectl_links(text)
        .into_iter()
        .flat_map(|link| link.dns)
        .collect()
}

/// Any list of addresses separated by spaces, newlines or `|` (what `nmcli -g` prints).
pub fn parse_ip_list(text: &str) -> Vec<IpAddr> {
    text.replace("\\:", ":")
        .split(|c: char| c.is_whitespace() || c == '|')
        .filter_map(parse_ip_token)
        .collect()
}

/// Splits one `nmcli -t` line on unescaped colons and unescapes `\:` and `\\`.
fn split_nmcli_fields(line: &str) -> Vec<String> {
    let mut fields = Vec::new();
    let mut current = String::new();
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => current.push(chars.next().unwrap_or('\\')),
            ':' => fields.push(std::mem::take(&mut current)),
            _ => current.push(c),
        }
    }
    fields.push(current);
    fields
}

/// Active NetworkManager connections as `(name, device)` pairs.
pub fn parse_nmcli_active(text: &str) -> Vec<(String, String)> {
    text.lines()
        .filter_map(|line| {
            let fields = split_nmcli_fields(line);
            let name = fields.first()?.trim().to_string();
            let device = fields.get(1)?.trim().to_string();
            (!name.is_empty() && !device.is_empty() && device != "lo").then_some((name, device))
        })
        .collect()
}

/// The output of macOS's `scutil --dns`: lines like `nameserver[0] : 192.168.1.1`.
pub fn parse_scutil_dns(text: &str) -> Vec<IpAddr> {
    text.lines()
        .filter_map(|line| {
            let line = line.trim();
            if !line.starts_with("nameserver[") {
                return None;
            }
            parse_ip_token(line.split_once(':')?.1.trim())
        })
        .collect()
}

/// The `gateway:` line of macOS/BSD `route -n get default`.
pub fn parse_route_get_default(text: &str) -> Option<IpAddr> {
    text.lines().find_map(|line| {
        let rest = line.trim().strip_prefix("gateway:")?;
        parse_ip_token(rest.trim())
    })
}

/// The `interface:` line of macOS/BSD `route -n get default`.
pub fn parse_route_get_interface(text: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let rest = line.trim().strip_prefix("interface:")?;
        let name = rest.trim();
        (!name.is_empty()).then(|| name.to_string())
    })
}

/// The output of `networksetup -getdnsservers <service>`. The "There aren't any DNS Servers
/// set..." message is not an address and is ignored.
pub fn parse_networksetup_dns(text: &str) -> Vec<IpAddr> {
    text.lines().filter_map(parse_ip_token).collect()
}

/// The service order printed by `networksetup -listnetworkserviceorder`.
pub fn parse_macos_services(text: &str) -> Vec<(String, String)> {
    let mut services = Vec::new();
    let mut current: Option<String> = None;
    for line in text.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix('(')
            && let Some((_, name)) = rest.split_once(") ")
        {
            current = Some(name.trim().to_string());
            continue;
        }
        if let Some(device) = line
            .split_once("Device:")
            .map(|(_, device)| device.trim().trim_end_matches(')').to_string())
            && let Some(service) = current.take()
        {
            services.push((service, device));
        }
    }
    services
}

/// The `IF <alias>` / `GW <address>` / `DNS <address>` lines printed by [`WINDOWS_SCRIPT`].
pub fn parse_windows_netconfig(text: &str) -> (Vec<String>, Vec<IpAddr>, Vec<IpAddr>) {
    let mut interfaces = Vec::new();
    let mut gateways = Vec::new();
    let mut dns = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("IF ") {
            let name = rest.trim();
            if !name.is_empty() && !interfaces.iter().any(|known| known == name) {
                interfaces.push(name.to_string());
            }
        } else if let Some(rest) = line.strip_prefix("GW ") {
            gateways.extend(parse_ip_token(rest.trim()));
        } else if let Some(rest) = line.strip_prefix("DNS ") {
            dns.extend(parse_ip_token(rest.trim()));
        }
    }
    (interfaces, gateways, dns)
}

/// Addresses from `reg query ... /v NameServer` and `DhcpNameServer` output. Values can be a
/// comma- or space-separated list, and may be empty.
pub fn parse_reg_nameservers(text: &str) -> Vec<IpAddr> {
    text.lines()
        .filter_map(|line| {
            let line = line.trim();
            let (_, value) = line.split_once("REG_SZ")?;
            Some(
                value
                    .split(|c: char| c == ',' || c.is_whitespace())
                    .filter_map(parse_ip_token)
                    .collect::<Vec<_>>(),
            )
        })
        .flatten()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap()
    }

    #[test]
    fn tokens_are_cleaned_up() {
        assert_eq!(parse_ip_token("1.1.1.1"), Some(ip("1.1.1.1")));
        assert_eq!(
            parse_ip_token("1.1.1.1#cloudflare-dns.com"),
            Some(ip("1.1.1.1"))
        );
        assert_eq!(parse_ip_token("fe80::1%eth0"), Some(ip("fe80::1")));
        assert_eq!(parse_ip_token("1.1.1.1:5353"), Some(ip("1.1.1.1")));
        assert_eq!(
            parse_ip_token("[2606:4700:4700::1111]:53"),
            Some(ip("2606:4700:4700::1111"))
        );
        assert_eq!(
            parse_ip_token("2606\\:4700\\:4700\\:\\:1111"),
            Some(ip("2606:4700:4700::1111"))
        );
        assert_eq!(parse_ip_token("(eno1):"), None);
        assert_eq!(parse_ip_token("Link"), None);
        assert_eq!(parse_ip_token("2"), None);
    }

    #[test]
    fn reads_the_linux_default_gateway() {
        let table = "Iface\tDestination\tGateway \tFlags\tRefCnt\tUse\tMetric\tMask\tMTU\tWindow\tIRTT\n\
                     eno1\t00000000\t0101A8C0\t0003\t0\t0\t100\t00000000\t0\t0\t0\n\
                     eno1\t0001A8C0\t00000000\t0001\t0\t0\t100\t00FFFFFF\t0\t0\t0\n";
        assert_eq!(parse_proc_net_route(table), Some(ip("192.168.1.1")));
        let no_default = "Iface\tDestination\tGateway\tFlags\n eno1\t0001A8C0\t00000000\t0001\n";
        assert_eq!(parse_proc_net_route(no_default), None);
        assert_eq!(parse_proc_net_route(""), None);
    }

    #[test]
    fn reads_default_routes() {
        let route = "default via 192.168.1.1 dev wlan0 proto dhcp metric 600\n192.168.1.0/24 dev wlan0 proto kernel scope link\n";
        assert_eq!(parse_default_device(route).as_deref(), Some("wlan0"));
        assert_eq!(parse_default_gateway(route), Some(ip("192.168.1.1")));
        assert_eq!(parse_default_device("10.0.0.0/8 dev eth0\n"), None);
        assert_eq!(parse_default_gateway("10.0.0.0/8 dev eth0\n"), None);
    }

    #[test]
    fn reads_resolv_conf() {
        let conf = "# generated\nsearch lan\nnameserver 192.168.1.1\nnameserver 2606:4700:4700::1111\nnameserver 127.0.0.53\noptions edns0\n";
        assert_eq!(
            parse_resolv_conf(conf),
            vec![
                ip("192.168.1.1"),
                ip("2606:4700:4700::1111"),
                ip("127.0.0.53")
            ]
        );
    }

    #[test]
    fn reads_resolvectl_output_and_prefers_the_default_link() {
        let text = "Global: 9.9.9.9\nLink 2 (eno1): 95.85.95.85 2.56.220.2\nLink 3 (wlan0): 1.1.1.1#cloudflare-dns.com 2606:4700:4700::1111 fe80::1%3\nLink 4 (docker0): 172.17.0.1\n";
        assert_eq!(
            parse_resolvectl_dns(text),
            vec![
                ip("9.9.9.9"),
                ip("95.85.95.85"),
                ip("2.56.220.2"),
                ip("1.1.1.1"),
                ip("2606:4700:4700::1111"),
                ip("fe80::1"),
                ip("172.17.0.1")
            ]
        );
        let links = parse_resolvectl_links(text);
        assert_eq!(
            select_resolvectl_dns(&links, Some("eno1")),
            vec![ip("95.85.95.85"), ip("2.56.220.2")]
        );
        assert_eq!(
            select_resolvectl_dns(&links, Some("missing")),
            vec![ip("9.9.9.9")]
        );
        assert_eq!(select_resolvectl_dns(&links, None), vec![ip("9.9.9.9")]);
    }

    #[test]
    fn reads_nmcli_output() {
        assert_eq!(
            parse_ip_list("192.168.1.1 | 8.8.8.8\n2001:4860:4860::8888\n\n"),
            vec![ip("192.168.1.1"), ip("8.8.8.8"), ip("2001:4860:4860::8888")]
        );
        assert_eq!(
            parse_ip_list("1.1.1.1 | 2606\\:4700\\:4700\\:\\:1111\n"),
            vec![ip("1.1.1.1"), ip("2606:4700:4700::1111")]
        );
        assert_eq!(
            parse_nmcli_active("Wired connection 1:eno1\nlo:lo\nVPN\\: work:tun0\n"),
            vec![
                ("Wired connection 1".into(), "eno1".into()),
                ("VPN: work".into(), "tun0".into())
            ]
        );
    }

    #[test]
    fn reads_macos_scutil_output() {
        let text = "DNS configuration\n\nresolver #1\n  search domain[0] : lan\n  nameserver[0] : 192.168.1.1\n  nameserver[1] : 2001:db8::1\n  if_index : 15 (en0)\n  flags    : Request A records\n  reach    : 0x00020002 (Reachable,Directly Reachable Address)\n\nresolver #2\n  domain   : local\n  options  : mdns\n";
        assert_eq!(
            parse_scutil_dns(text),
            vec![ip("192.168.1.1"), ip("2001:db8::1")]
        );
    }

    #[test]
    fn reads_the_bsd_and_macos_gateway() {
        let text = "   route to: default\ndestination: default\n       mask: default\n    gateway: 192.168.0.1\n  interface: en0\n      flags: <UP,GATEWAY,DONE,STATIC,PRCLONING,GLOBAL>\n";
        assert_eq!(parse_route_get_default(text), Some(ip("192.168.0.1")));
        assert_eq!(parse_route_get_interface(text).as_deref(), Some("en0"));
        assert_eq!(
            parse_route_get_default("route: writing to routing socket: not in table\n"),
            None
        );
    }

    #[test]
    fn reads_networksetup_dns_and_services() {
        let order = "An asterisk (*) denotes that a network service is disabled.\n(1) Wi-Fi\n(Hardware Port: Wi-Fi, Device: en0)\n(2) USB 10/100/1000 LAN\n(Hardware Port: Ethernet, Device: en7)\n";
        assert_eq!(
            parse_macos_services(order),
            vec![
                ("Wi-Fi".into(), "en0".into()),
                ("USB 10/100/1000 LAN".into(), "en7".into())
            ]
        );
        assert_eq!(
            parse_networksetup_dns("1.1.1.1\n2606:4700:4700::1111\n"),
            vec![ip("1.1.1.1"), ip("2606:4700:4700::1111")]
        );
        assert!(parse_networksetup_dns("There aren't any DNS Servers set on Wi-Fi.\n").is_empty());
    }

    #[test]
    fn reads_the_windows_script_output() {
        let text = "IF Wi-Fi\r\nGW 192.168.1.1\r\nDNS 192.168.1.1\r\nDNS 2001:4860:4860::8888\r\nDNS fec0:0:0:ffff::1\r\nsomething else\r\n";
        let (interfaces, gateways, dns) = parse_windows_netconfig(text);
        assert_eq!(interfaces, vec!["Wi-Fi"]);
        assert_eq!(gateways, vec![ip("192.168.1.1")]);
        assert_eq!(
            dns,
            vec![
                ip("192.168.1.1"),
                ip("2001:4860:4860::8888"),
                ip("fec0:0:0:ffff::1")
            ]
        );
    }

    #[test]
    fn reads_windows_registry_nameservers() {
        let text = "\r\nHKEY_LOCAL_MACHINE\\SYSTEM\\CurrentControlSet\\Services\\Tcpip\\Parameters\\Interfaces\\{abc}\r\n    DhcpNameServer    REG_SZ    192.168.1.1,8.8.8.8\r\n    NameServer    REG_SZ    \r\n\r\nHKEY_LOCAL_MACHINE\\SYSTEM\\CurrentControlSet\\Services\\Tcpip6\\Parameters\\Interfaces\\{def}\r\n    NameServer    REG_SZ    2606:4700:4700::1111\r\n";
        assert_eq!(
            parse_reg_nameservers(text),
            vec![ip("192.168.1.1"), ip("8.8.8.8"), ip("2606:4700:4700::1111")]
        );
    }

    #[test]
    fn a_missing_command_is_not_an_error() {
        assert_eq!(run("dnsrace-no-such-command-exists", &[]), None);
    }

    #[test]
    fn detection_never_panics_and_only_reports_usable_servers() {
        let info = detect();
        assert!(info.dns.iter().all(|ip| is_usable_ip(*ip)));
        if let Some(gw) = info.gateway {
            assert!(is_usable_ip(gw));
        }
    }

    #[test]
    fn sources_are_joined_without_duplicates() {
        let mut info = SystemInfo::default();
        add_source(&mut info, "systemd-resolved");
        add_source(&mut info, "NetworkManager");
        add_source(&mut info, "systemd-resolved");
        assert_eq!(
            info.source.as_deref(),
            Some("systemd-resolved + NetworkManager")
        );
    }
}
