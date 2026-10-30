//! Applying and reverting the system DNS settings.
//!
//! This is deliberately explicit and conservative: the interface asks for confirmation first,
//! the previous settings are saved in `applied.json`, and every command is visible in the code.
//! Linux uses NetworkManager when available and falls back to systemd-resolved; Windows uses
//! PowerShell with a UAC prompt; macOS uses `networksetup` through the standard administrator
//! prompt.

use std::collections::BTreeMap;
use std::net::IpAddr;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::servers::is_usable_ip;

/// A saved system change. It is written to the config directory after a successful apply.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AppliedDns {
    pub platform: String,
    /// `networkmanager`, `resolvectl`, `powershell` or `networksetup`.
    pub method: String,
    /// Connection name, interface name, adapter alias or network service.
    pub interface: String,
    pub applied: Vec<IpAddr>,
    /// Method-specific previous values, kept so Revert can restore them.
    pub previous: BTreeMap<String, String>,
    pub timestamp: u64,
}

impl AppliedDns {
    pub fn summary(&self) -> String {
        format!(
            "{} on {} ({})",
            self.applied
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(", "),
            self.interface,
            self.method
        )
    }
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub const fn platform_name() -> &'static str {
    if cfg!(target_os = "linux") {
        "Linux"
    } else if cfg!(target_os = "windows") {
        "Windows"
    } else if cfg!(target_os = "macos") {
        "macOS"
    } else {
        "this platform"
    }
}

pub fn validation_error(servers: &[IpAddr]) -> Option<String> {
    if servers.is_empty() {
        return Some("There is no DNS address to apply.".into());
    }
    if let Some(ip) = servers.iter().find(|ip| !is_usable_ip(**ip)) {
        return Some(format!("{ip} is not a usable public DNS address."));
    }
    None
}

fn clean_servers(servers: &[IpAddr]) -> Result<Vec<IpAddr>, String> {
    if let Some(error) = validation_error(servers) {
        return Err(error);
    }
    let mut out = Vec::new();
    for ip in servers {
        if !out.contains(ip) {
            out.push(*ip);
        }
    }
    out.truncate(4);
    Ok(out)
}

/// Applies `servers` to the active network connection. Requires administrator permission on
/// most systems; the platform-specific implementation raises the normal prompt.
pub fn apply(servers: &[IpAddr]) -> Result<AppliedDns, String> {
    let servers = clean_servers(servers)?;

    #[cfg(target_os = "linux")]
    {
        apply_linux(&servers)
    }
    #[cfg(target_os = "windows")]
    {
        apply_windows(&servers)
    }
    #[cfg(target_os = "macos")]
    {
        apply_macos(&servers)
    }
    #[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
    {
        let _ = servers;
        Err("Changing DNS automatically is not implemented on this platform.".into())
    }
}

/// Restores the settings saved in `applied`.
pub fn revert(applied: &AppliedDns) -> Result<(), String> {
    match applied.method.as_str() {
        #[cfg(target_os = "linux")]
        "networkmanager" => revert_linux_nm(applied),
        #[cfg(target_os = "linux")]
        "resolvectl" => revert_linux_resolvectl(applied),
        #[cfg(target_os = "windows")]
        "powershell" => revert_windows(applied),
        #[cfg(target_os = "macos")]
        "networksetup" => revert_macos(applied),
        other => Err(format!(
            "This saved change ({other}) was made by a different platform and cannot be reverted here."
        )),
    }
}

fn run_command(program: &str, args: &[String]) -> Result<String, String> {
    let mut command = Command::new(program);
    command.args(args);
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let output = command
        .output()
        .map_err(|error| format!("Could not run {program}: {error}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let detail = if !stderr.is_empty() { stderr } else { stdout };
        return Err(format!("{program} exited with {}: {detail}", output.status));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

#[cfg(target_os = "linux")]
fn command_exists(program: &str) -> bool {
    Command::new(program)
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[cfg(target_os = "windows")]
fn command_exists(program: &str) -> bool {
    let mut command = Command::new(program);
    command.args(["-NoProfile", "-Command", "exit 0"]);
    // This check runs before the window is shown; do not flash a console window.
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    command.creation_flags(CREATE_NO_WINDOW);
    command
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[cfg(target_os = "linux")]
fn is_root() -> bool {
    Command::new("id")
        .arg("-u")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .is_some_and(|s| s.trim() == "0")
}

/// Runs as the current user first; if that fails and `elevate` is requested, tries `pkexec`.
/// NetworkManager and resolvectl both use polkit, so the desktop normally shows a password
/// dialog for this.
#[cfg(target_os = "linux")]
fn run_linux(program: &str, args: &[String], elevate: bool) -> Result<String, String> {
    match run_command(program, args) {
        Ok(out) => Ok(out),
        Err(direct_error) if elevate && !is_root() && command_exists("pkexec") => {
            let mut elevated = vec![program.to_string()];
            elevated.extend_from_slice(args);
            run_command("pkexec", &elevated).map_err(|pk_error| {
                format!("{direct_error}\nAdministrator command also failed: {pk_error}")
            })
        }
        Err(error) => Err(error),
    }
}

#[cfg(target_os = "linux")]
fn apply_linux(servers: &[IpAddr]) -> Result<AppliedDns, String> {
    let v4: Vec<String> = servers
        .iter()
        .filter(|ip| ip.is_ipv4())
        .map(ToString::to_string)
        .collect();
    let v6: Vec<String> = servers
        .iter()
        .filter(|ip| ip.is_ipv6())
        .map(ToString::to_string)
        .collect();

    if command_exists("nmcli") {
        let (name, _device) = active_nmcli_connection()?;
        let mut previous = BTreeMap::new();
        for field in [
            "ipv4.dns",
            "ipv4.ignore-auto-dns",
            "ipv6.dns",
            "ipv6.ignore-auto-dns",
        ] {
            let value = run_command(
                "nmcli",
                &[
                    "-g".to_string(),
                    field.to_string(),
                    "connection".to_string(),
                    "show".to_string(),
                    name.clone(),
                ],
            )
            .unwrap_or_default();
            previous.insert(field.to_string(), value.trim().to_string());
        }

        if !v4.is_empty() {
            run_linux(
                "nmcli",
                &[
                    "connection".into(),
                    "modify".into(),
                    name.clone(),
                    "ipv4.dns".into(),
                    v4.join(" "),
                    "ipv4.ignore-auto-dns".into(),
                    "yes".into(),
                ],
                true,
            )?;
        }
        if !v6.is_empty() {
            run_linux(
                "nmcli",
                &[
                    "connection".into(),
                    "modify".into(),
                    name.clone(),
                    "ipv6.dns".into(),
                    v6.join(" "),
                    "ipv6.ignore-auto-dns".into(),
                    "yes".into(),
                ],
                true,
            )?;
        }
        run_linux(
            "nmcli",
            &["connection".into(), "up".into(), name.clone()],
            true,
        )?;
        flush_linux_caches();
        return Ok(AppliedDns {
            platform: platform_name().into(),
            method: "networkmanager".into(),
            interface: name,
            applied: servers.to_vec(),
            previous,
            timestamp: now(),
        });
    }

    if command_exists("resolvectl") {
        let interface = default_interface().ok_or_else(|| {
            "Could not find the default network interface. Use NetworkManager or set DNS manually."
                .to_string()
        })?;
        let previous_dns =
            run_command("resolvectl", &["dns".into(), interface.clone()]).unwrap_or_default();
        let mut args = vec!["dns".to_string(), interface.clone()];
        args.extend(servers.iter().map(ToString::to_string));
        run_linux("resolvectl", &args, true)?;
        flush_linux_caches();
        let mut previous = BTreeMap::new();
        previous.insert("dns".into(), previous_dns.trim().to_string());
        return Ok(AppliedDns {
            platform: platform_name().into(),
            method: "resolvectl".into(),
            interface,
            applied: servers.to_vec(),
            previous,
            timestamp: now(),
        });
    }

    Err(
        "Neither NetworkManager (nmcli) nor systemd-resolved (resolvectl) is available. Set the DNS addresses in your network settings, or install one of them."
            .into(),
    )
}

#[cfg(target_os = "linux")]
fn flush_linux_caches() {
    if command_exists("resolvectl") {
        let _ = run_command("resolvectl", &["flush-caches".into()]);
    }
}

#[cfg(target_os = "linux")]
fn active_nmcli_connection() -> Result<(String, String), String> {
    let default_device = default_interface();
    let output = run_command(
        "nmcli",
        &[
            "-t".into(),
            "-f".into(),
            "DEVICE,NAME".into(),
            "connection".into(),
            "show".into(),
            "--active".into(),
        ],
    )?;
    let connections = parse_nmcli_connections(&output);
    let selected = connections
        .iter()
        .find(|(device, _)| default_device.as_deref() == Some(device))
        .or_else(|| connections.first())
        .ok_or_else(|| "NetworkManager has no active connection to change.".to_string())?;
    Ok((selected.1.clone(), selected.0.clone()))
}

#[cfg(target_os = "linux")]
fn default_interface() -> Option<String> {
    if let Ok(output) = run_command("ip", &["route".into(), "show".into(), "default".into()])
        && let Some(device) = parse_default_device(&output)
    {
        return Some(device);
    }
    // Fallback for systems without the ip command: /proc/net/route.
    let text = std::fs::read_to_string("/proc/net/route").ok()?;
    for line in text.lines().skip(1) {
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() >= 2 && fields[1] == "00000000" {
            return Some(fields[0].to_string());
        }
    }
    None
}

#[cfg(any(target_os = "linux", test))]
fn parse_default_device(output: &str) -> Option<String> {
    for line in output.lines() {
        if line.split_whitespace().next() == Some("default") {
            let fields: Vec<&str> = line.split_whitespace().collect();
            if let Some(index) = fields.iter().position(|field| *field == "dev") {
                return fields.get(index + 1).map(|s| (*s).to_string());
            }
        }
    }
    None
}

#[cfg(any(target_os = "linux", test))]
fn unescape_nmcli(value: &str) -> String {
    value.replace("\\:", ":").replace("\\\\", "\\")
}

#[cfg(any(target_os = "linux", test))]
fn parse_nmcli_connections(output: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for line in output.lines() {
        let Some((device, name)) = line.split_once(':') else {
            continue;
        };
        if device == "lo" || device.is_empty() || name.is_empty() {
            continue;
        }
        out.push((device.to_string(), unescape_nmcli(name)));
    }
    out
}

#[cfg(target_os = "linux")]
fn revert_linux_nm(applied: &AppliedDns) -> Result<(), String> {
    let mut args = vec![
        "connection".to_string(),
        "modify".to_string(),
        applied.interface.clone(),
    ];
    for field in [
        "ipv4.dns",
        "ipv4.ignore-auto-dns",
        "ipv6.dns",
        "ipv6.ignore-auto-dns",
    ] {
        let mut value = applied.previous.get(field).cloned().unwrap_or_else(|| {
            if field.ends_with("dns") {
                String::new()
            } else {
                "no".to_string()
            }
        });
        if field.ends_with("ignore-auto-dns") && value.is_empty() {
            value = "no".to_string();
        }
        args.push(field.to_string());
        args.push(value);
    }
    run_linux("nmcli", &args, true)?;
    run_linux(
        "nmcli",
        &["connection".into(), "up".into(), applied.interface.clone()],
        true,
    )?;
    flush_linux_caches();
    Ok(())
}

#[cfg(target_os = "linux")]
fn revert_linux_resolvectl(applied: &AppliedDns) -> Result<(), String> {
    run_linux(
        "resolvectl",
        &["revert".into(), applied.interface.clone()],
        true,
    )?;
    flush_linux_caches();
    Ok(())
}

// ---------------------------------------------------------------------------------------
// Windows
// ---------------------------------------------------------------------------------------

#[cfg(target_os = "windows")]
fn powershell() -> &'static str {
    if command_exists("powershell") {
        "powershell"
    } else {
        "pwsh"
    }
}

#[cfg(target_os = "windows")]
fn run_powershell(script: &str) -> Result<String, String> {
    run_command(
        powershell(),
        &[
            "-NoProfile".into(),
            "-NonInteractive".into(),
            "-ExecutionPolicy".into(),
            "Bypass".into(),
            "-Command".into(),
            script.to_string(),
        ],
    )
}

#[cfg(target_os = "windows")]
fn powershell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

#[cfg(target_os = "windows")]
fn active_adapter() -> Result<String, String> {
    let script = "Get-NetAdapter | Where-Object { $_.Status -eq 'Up' } | Sort-Object InterfaceMetric | Select-Object -First 1 -ExpandProperty Name";
    let output = run_powershell(script)?;
    let name = output.trim();
    if name.is_empty() {
        Err("No active network adapter was found.".into())
    } else {
        Ok(name.to_string())
    }
}

#[cfg(target_os = "windows")]
fn adapter_dns(alias: &str, family: &str) -> Vec<String> {
    let script = format!(
        "(Get-DnsClientServerAddress -InterfaceAlias {} -AddressFamily {}).ServerAddresses",
        powershell_quote(alias),
        family
    );
    run_powershell(&script)
        .unwrap_or_default()
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

#[cfg(target_os = "windows")]
fn elevated_script(script: &str, name: &str) -> Result<(), String> {
    let path = std::env::temp_dir().join(format!("dnsrace-{name}-{}.ps1", std::process::id()));
    std::fs::write(&path, script)
        .map_err(|e| format!("Could not write the PowerShell script: {e}"))?;
    let path_text = path.display().to_string();
    let command = format!(
        "Start-Process -FilePath 'powershell.exe' -Verb RunAs -Wait -ArgumentList @('-NoProfile','-ExecutionPolicy','Bypass','-File',{})",
        powershell_quote(&path_text)
    );
    let result = run_powershell(&command);
    let _ = std::fs::remove_file(path);
    match result {
        Ok(output) if output.to_ascii_lowercase().contains("cancel") => Err(
            "The administrator prompt was cancelled, so the DNS settings were not changed.".into(),
        ),
        Ok(_) => Ok(()),
        Err(error) => {
            if error.to_ascii_lowercase().contains("cancel") {
                Err(
                    "The administrator prompt was cancelled, so the DNS settings were not changed."
                        .into(),
                )
            } else {
                Err(error)
            }
        }
    }
}

#[cfg(target_os = "windows")]
fn apply_windows(servers: &[IpAddr]) -> Result<AppliedDns, String> {
    let alias = active_adapter()?;
    let previous4 = adapter_dns(&alias, "IPv4");
    let previous6 = adapter_dns(&alias, "IPv6");
    let v4: Vec<String> = servers
        .iter()
        .filter(|ip| ip.is_ipv4())
        .map(ToString::to_string)
        .collect();
    let v6: Vec<String> = servers
        .iter()
        .filter(|ip| ip.is_ipv6())
        .map(ToString::to_string)
        .collect();
    let mut script = String::from("$ErrorActionPreference = 'Stop'\n");
    if !v4.is_empty() {
        script.push_str(&format!(
            "Set-DnsClientServerAddress -InterfaceAlias {} -ServerAddresses ({})\n",
            powershell_quote(&alias),
            v4.iter()
                .map(|ip| powershell_quote(ip))
                .collect::<Vec<_>>()
                .join(",")
        ));
    }
    if !v6.is_empty() {
        script.push_str(&format!(
            "Set-DnsClientServerAddress -InterfaceAlias {} -ServerAddresses ({})\n",
            powershell_quote(&alias),
            v6.iter()
                .map(|ip| powershell_quote(ip))
                .collect::<Vec<_>>()
                .join(",")
        ));
    }
    elevated_script(&script, "apply")?;

    let check = adapter_dns(&alias, "IPv4");
    let check6 = adapter_dns(&alias, "IPv6");
    if (!v4.is_empty() && !v4.iter().all(|ip| check.contains(ip)))
        || (!v6.is_empty() && !v6.iter().all(|ip| check6.contains(ip)))
    {
        return Err(
            "Windows accepted the command, but the adapter still reports different DNS servers."
                .into(),
        );
    }

    let mut previous = BTreeMap::new();
    previous.insert("ipv4".into(), previous4.join(" "));
    previous.insert("ipv6".into(), previous6.join(" "));
    Ok(AppliedDns {
        platform: platform_name().into(),
        method: "powershell".into(),
        interface: alias,
        applied: servers.to_vec(),
        previous,
        timestamp: now(),
    })
}

#[cfg(target_os = "windows")]
fn revert_windows(applied: &AppliedDns) -> Result<(), String> {
    let previous4: Vec<&str> = applied
        .previous
        .get("ipv4")
        .map(|s| s.split_whitespace().collect())
        .unwrap_or_default();
    let previous6: Vec<&str> = applied
        .previous
        .get("ipv6")
        .map(|s| s.split_whitespace().collect())
        .unwrap_or_default();
    let mut script = String::from("$ErrorActionPreference = 'Stop'\n");
    // Reset both families first, then restore whichever values were saved. This also clears an
    // applied IPv6 address when the adapter originally had no static IPv6 DNS servers.
    script.push_str(&format!(
        "Set-DnsClientServerAddress -InterfaceAlias {} -ResetServerAddresses\n",
        powershell_quote(&applied.interface)
    ));
    if !previous4.is_empty() {
        script.push_str(&format!(
            "Set-DnsClientServerAddress -InterfaceAlias {} -ServerAddresses ({})\n",
            powershell_quote(&applied.interface),
            previous4
                .iter()
                .map(|ip| powershell_quote(ip))
                .collect::<Vec<_>>()
                .join(",")
        ));
    }
    if !previous6.is_empty() {
        script.push_str(&format!(
            "Set-DnsClientServerAddress -InterfaceAlias {} -ServerAddresses ({})\n",
            powershell_quote(&applied.interface),
            previous6
                .iter()
                .map(|ip| powershell_quote(ip))
                .collect::<Vec<_>>()
                .join(",")
        ));
    }
    elevated_script(&script, "revert")
}

// ---------------------------------------------------------------------------------------
// macOS
// ---------------------------------------------------------------------------------------

#[cfg(target_os = "macos")]
fn run_macos(args: &[String]) -> Result<String, String> {
    run_command("networksetup", args)
}

#[cfg(target_os = "macos")]
fn macos_service() -> Result<String, String> {
    let order = run_macos(&["-listnetworkserviceorder".into()])?;
    let default_device = default_macos_interface();
    let services = parse_macos_services(&order);
    if let Some(device) = default_device
        && let Some((service, _)) = services.iter().find(|(_, dev)| *dev == device)
    {
        return Ok(service.clone());
    }
    services
        .into_iter()
        .next()
        .map(|(service, _)| service)
        .ok_or_else(|| "No macOS network service was found.".to_string())
}

#[cfg(target_os = "macos")]
fn default_macos_interface() -> Option<String> {
    let output = run_command("route", &["-n".into(), "get".into(), "default".into()]).ok()?;
    output.lines().find_map(|line| {
        line.trim()
            .strip_prefix("interface:")
            .map(|value| value.trim().to_string())
    })
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn parse_macos_services(output: &str) -> Vec<(String, String)> {
    let mut services = Vec::new();
    let mut current: Option<String> = None;
    for line in output.lines() {
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

#[cfg(target_os = "macos")]
fn macos_dns(service: &str) -> Vec<String> {
    let output = run_macos(&["-getdnsservers".into(), service.to_string()]).unwrap_or_default();
    if output.to_ascii_lowercase().contains("aren't any") {
        return Vec::new();
    }
    output
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && line.parse::<IpAddr>().is_ok())
        .map(ToOwned::to_owned)
        .collect()
}

#[cfg(target_os = "macos")]
fn run_macos_admin(script: &str, name: &str) -> Result<(), String> {
    let path = std::env::temp_dir().join(format!("dnsrace-{name}-{}.sh", std::process::id()));
    std::fs::write(&path, script).map_err(|e| format!("Could not write the helper script: {e}"))?;
    let command = format!(
        "do shell script \"/bin/sh {}\" with administrator privileges",
        path.display()
    );
    let output = run_command("osascript", &["-e".into(), command]);
    let _ = std::fs::remove_file(path);
    output.map(|_| ()).map_err(|error| {
        if error.to_ascii_lowercase().contains("cancel") {
            "The administrator prompt was cancelled, so the DNS settings were not changed."
                .to_string()
        } else {
            error
        }
    })
}

#[cfg(target_os = "macos")]
fn macos_shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[cfg(target_os = "macos")]
fn apply_macos(servers: &[IpAddr]) -> Result<AppliedDns, String> {
    let service = macos_service()?;
    let previous = macos_dns(&service);
    let script = format!(
        "#!/bin/sh\n/usr/sbin/networksetup -setdnsservers {} {}\n/usr/bin/dscacheutil -flushcache\n/usr/bin/killall -HUP mDNSResponder\n",
        macos_shell_quote(&service),
        servers
            .iter()
            .map(|ip| macos_shell_quote(&ip.to_string()))
            .collect::<Vec<_>>()
            .join(" ")
    );
    run_macos_admin(&script, "apply")?;
    let mut map = BTreeMap::new();
    map.insert("dns".into(), previous.join(" "));
    Ok(AppliedDns {
        platform: platform_name().into(),
        method: "networksetup".into(),
        interface: service,
        applied: servers.to_vec(),
        previous: map,
        timestamp: now(),
    })
}

#[cfg(target_os = "macos")]
fn revert_macos(applied: &AppliedDns) -> Result<(), String> {
    let previous = applied
        .previous
        .get("dns")
        .map(|s| {
            s.split_whitespace()
                .map(ToOwned::to_owned)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let values = if previous.is_empty() {
        "Empty".to_string()
    } else {
        previous
            .iter()
            .map(|ip| macos_shell_quote(ip))
            .collect::<Vec<_>>()
            .join(" ")
    };
    let script = format!(
        "#!/bin/sh\n/usr/sbin/networksetup -setdnsservers {} {}\n/usr/bin/dscacheutil -flushcache\n/usr/bin/killall -HUP mDNSResponder\n",
        macos_shell_quote(&applied.interface),
        values
    );
    run_macos_admin(&script, "revert")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validation_rejects_empty_and_special_addresses() {
        assert!(validation_error(&[]).is_some());
        assert!(validation_error(&["127.0.0.1".parse().unwrap()]).is_some());
        assert!(validation_error(&["1.1.1.1".parse().unwrap()]).is_none());
        assert!(validation_error(&["2606:4700:4700::1111".parse().unwrap()]).is_none());
    }

    #[test]
    fn clean_servers_deduplicates_and_limits() {
        let input: Vec<IpAddr> = [
            "1.1.1.1", "1.0.0.1", "1.1.1.1", "8.8.8.8", "9.9.9.9", "8.8.4.4",
        ]
        .iter()
        .map(|ip| ip.parse().unwrap())
        .collect();
        let cleaned = clean_servers(&input).unwrap();
        assert_eq!(
            cleaned,
            vec![
                "1.1.1.1".parse::<IpAddr>().unwrap(),
                "1.0.0.1".parse().unwrap(),
                "8.8.8.8".parse().unwrap(),
                "9.9.9.9".parse().unwrap(),
            ]
        );
    }

    #[test]
    fn parses_default_routes() {
        let output = "default via 192.168.1.1 dev wlan0 proto dhcp metric 600\n192.168.1.0/24 dev wlan0 proto kernel scope link\n";
        assert_eq!(parse_default_device(output).as_deref(), Some("wlan0"));
        assert!(parse_default_device("10.0.0.0/8 dev eth0\n").is_none());
    }

    #[test]
    fn parses_nmcli_connection_names_with_escaped_colons() {
        let output = "wlan0:My Wi-Fi\neth0:Wired\\: work\nlo:loopback\nbadline\n";
        assert_eq!(
            parse_nmcli_connections(output),
            vec![
                ("wlan0".into(), "My Wi-Fi".into()),
                ("eth0".into(), "Wired: work".into())
            ]
        );
    }

    #[test]
    fn parses_networksetup_service_order() {
        let output = "An asterisk (*) denotes that a network service is disabled.\n(1) Wi-Fi\n(Hardware Port: Wi-Fi, Device: en0)\n(2) USB 10/100/1000 LAN\n(Hardware Port: Ethernet, Device: en7)\n";
        assert_eq!(
            parse_macos_services(output),
            vec![
                ("Wi-Fi".into(), "en0".into()),
                ("USB 10/100/1000 LAN".into(), "en7".into())
            ]
        );
    }

    #[test]
    fn platform_name_is_not_empty() {
        assert!(!platform_name().is_empty());
    }

    #[test]
    fn applied_summary_lists_addresses_and_method() {
        let applied = AppliedDns {
            platform: "Linux".into(),
            method: "networkmanager".into(),
            interface: "Wired connection 1".into(),
            applied: vec!["1.1.1.1".parse().unwrap()],
            previous: BTreeMap::new(),
            timestamp: 0,
        };
        assert!(applied.summary().contains("1.1.1.1"));
        assert!(applied.summary().contains("networkmanager"));
    }
}
