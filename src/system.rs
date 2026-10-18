//! Finds your default gateway and the DNS servers your system uses right now.
//!
//! Everything here is best effort: if a command is missing or its output looks different from
//! what we expect, that piece is simply skipped. The parsers are plain functions over text so
//! they are tested on every platform; only the commands that produce the text depend on the OS.

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
}

/// One PowerShell run that prints `GW <address>` and `DNS <address>` lines for the adapters
/// that are up and have a default gateway. The output contains only addresses, so it does not
/// depend on the Windows display language.
const WINDOWS_SCRIPT: &str = "Get-NetIPConfiguration | Where-Object { $_.NetAdapter.Status -eq 'Up' -and ($_.IPv4DefaultGateway -or $_.IPv6DefaultGateway) } | ForEach-Object { foreach ($g in @($_.IPv4DefaultGateway) + @($_.IPv6DefaultGateway)) { if ($g) { 'GW ' + $g.NextHop } }; foreach ($d in $_.DNSServer.ServerAddresses) { 'DNS ' + $d } }";

/// Looks at the system. Blocking (it runs a few small commands), so call it from a blocking
/// context.
pub fn detect() -> SystemInfo {
    let mut info = SystemInfo::default();

    if cfg!(windows) {
        let script = ["-NoProfile", "-NonInteractive", "-Command", WINDOWS_SCRIPT];
        if let Some(text) = run("powershell", &script) {
            let (gateways, dns) = parse_windows_netconfig(&text);
            info.gateway = gateways
                .iter()
                .copied()
                .find(IpAddr::is_ipv4)
                .or_else(|| gateways.first().copied());
            info.dns = dns;
        }
    } else {
        if cfg!(any(target_os = "linux", target_os = "android")) {
            info.gateway = std::fs::read_to_string("/proc/net/route")
                .ok()
                .and_then(|t| parse_proc_net_route(&t));
            if let Some(t) = run("resolvectl", &["dns"]) {
                info.dns.extend(parse_resolvectl_dns(&t));
            }
            if let Some(t) = run("nmcli", &["-g", "IP4.DNS,IP6.DNS", "device", "show"]) {
                info.dns.extend(parse_ip_list(&t));
            }
        } else {
            if let Some(t) = run("route", &["-n", "get", "default"]) {
                info.gateway = parse_route_get_default(&t);
            }
            if let Some(t) = run("scutil", &["--dns"]) {
                info.dns.extend(parse_scutil_dns(&t));
            }
        }
        if let Ok(t) = std::fs::read_to_string("/etc/resolv.conf") {
            info.dns.extend(parse_resolv_conf(&t));
        }
    }

    info.dns.retain(|ip| is_usable_ip(*ip));
    info.dns.sort();
    info.dns.dedup();
    info.gateway = info.gateway.filter(|ip| is_usable_ip(*ip));
    info
}

/// Runs a command and returns its output, or `None` if it is missing, fails or takes longer
/// than three seconds.
fn run(cmd: &str, args: &[&str]) -> Option<String> {
    let mut child = Command::new(cmd)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let mut stdout = child.stdout.take()?;
    // drain the pipe on a thread so a chatty command can never block on a full pipe
    let reader = std::thread::spawn(move || {
        let mut text = String::new();
        let _ = stdout.read_to_string(&mut text);
        text
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

/// The output of `resolvectl dns`: lines like `Link 2 (eth0): 1.1.1.1 2606:4700:4700::1111`.
pub fn parse_resolvectl_dns(text: &str) -> Vec<IpAddr> {
    text.lines()
        .filter_map(|line| line.split_once(": ").map(|(_, rest)| rest))
        .flat_map(|rest| rest.split_whitespace().filter_map(parse_ip_token))
        .collect()
}

/// Any list of addresses separated by spaces, newlines or `|` (what `nmcli -g` prints).
pub fn parse_ip_list(text: &str) -> Vec<IpAddr> {
    text.split(|c: char| c.is_whitespace() || c == '|')
        .filter_map(parse_ip_token)
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

/// The `GW <address>` / `DNS <address>` lines printed by [`WINDOWS_SCRIPT`].
pub fn parse_windows_netconfig(text: &str) -> (Vec<IpAddr>, Vec<IpAddr>) {
    let mut gateways = Vec::new();
    let mut dns = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("GW ") {
            gateways.extend(parse_ip_token(rest.trim()));
        } else if let Some(rest) = line.strip_prefix("DNS ") {
            dns.extend(parse_ip_token(rest.trim()));
        }
    }
    (gateways, dns)
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
        assert_eq!(parse_ip_token("(eno1):"), None);
        assert_eq!(parse_ip_token("Link"), None);
        assert_eq!(parse_ip_token("2"), None);
    }

    #[test]
    fn reads_the_linux_default_gateway() {
        let table =
            "Iface\tDestination\tGateway \tFlags\tRefCnt\tUse\tMetric\tMask\tMTU\tWindow\tIRTT\n\
                     eno1\t00000000\t0101A8C0\t0003\t0\t0\t100\t00000000\t0\t0\t0\n\
                     eno1\t0001A8C0\t00000000\t0001\t0\t0\t100\t00FFFFFF\t0\t0\t0\n";
        assert_eq!(parse_proc_net_route(table), Some(ip("192.168.1.1")));
        let no_default = "Iface\tDestination\tGateway\tFlags\n eno1\t0001A8C0\t00000000\t0001\n";
        assert_eq!(parse_proc_net_route(no_default), None);
        assert_eq!(parse_proc_net_route(""), None);
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
    fn reads_resolvectl_output() {
        let text = "Global:\nLink 2 (eno1): 95.85.95.85 2.56.220.2\nLink 3 (wlan0): 1.1.1.1#cloudflare-dns.com 2606:4700:4700::1111 fe80::1%3\nLink 4 (docker0):\n";
        assert_eq!(
            parse_resolvectl_dns(text),
            vec![
                ip("95.85.95.85"),
                ip("2.56.220.2"),
                ip("1.1.1.1"),
                ip("2606:4700:4700::1111"),
                ip("fe80::1")
            ]
        );
    }

    #[test]
    fn reads_nmcli_output() {
        assert_eq!(
            parse_ip_list("192.168.1.1 | 8.8.8.8\n2001:4860:4860::8888\n\n"),
            vec![ip("192.168.1.1"), ip("8.8.8.8"), ip("2001:4860:4860::8888")]
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
        assert_eq!(
            parse_route_get_default("route: writing to routing socket: not in table\n"),
            None
        );
    }

    #[test]
    fn reads_the_windows_script_output() {
        let text = "GW 192.168.1.1\r\nDNS 192.168.1.1\r\nDNS 2001:4860:4860::8888\r\nDNS fec0:0:0:ffff::1\r\nsomething else\r\n";
        let (gateways, dns) = parse_windows_netconfig(text);
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
}
