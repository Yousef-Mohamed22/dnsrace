//! In-app updates from the project's GitHub releases.
//!
//! A check reads the latest release, compares its version, and picks the asset for this platform.
//! An install downloads the asset, verifies it against the release's `SHA256SUMS.txt`, and only
//! then replaces the running installation. Android cannot install a package silently, so it opens
//! the APK download link instead.

use std::env;
use std::fs;
use std::io::{Read as _, Write as _};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc::Sender;
use std::time::Duration;

use serde_json::Value;
use sha2::{Digest as _, Sha256};

const RELEASES: &str = "https://api.github.com/repos/Yousef-Mohamed22/dnsrace/releases/latest";
const RELEASE_PAGE: &str = "https://github.com/Yousef-Mohamed22/dnsrace/releases/latest";

/// One downloadable file attached to a release.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Asset {
    pub name: String,
    pub url: String,
}

/// The parts of a GitHub release the updater needs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReleaseInfo {
    pub version: String,
    pub title: String,
    pub notes: String,
    pub page: String,
    pub assets: Vec<Asset>,
}

/// Where this build can install an update.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InstallKind {
    AppImage,
    Windows,
    MacOs,
    Android,
    Unsupported,
}

pub fn install_kind() -> InstallKind {
    if cfg!(target_os = "android") {
        InstallKind::Android
    } else if cfg!(target_os = "windows") {
        InstallKind::Windows
    } else if cfg!(target_os = "macos") {
        InstallKind::MacOs
    } else if env::var_os("APPIMAGE").is_some() {
        InstallKind::AppImage
    } else {
        InstallKind::Unsupported
    }
}

/// Messages sent from the update worker to the interface.
#[derive(Clone, Debug)]
pub enum UpdateEvent {
    Checking,
    UpToDate,
    Available(ReleaseInfo),
    Downloading {
        received: u64,
        total: Option<u64>,
    },
    Verifying,
    Installing,
    /// The application should close; the platform helper starts the new build.
    Restarting,
    /// Android (or an unsupported Linux build) opens this page instead of installing.
    OpenPage(String),
    Failed(String),
}

/// Check GitHub for a newer release. `None` means this build is current.
pub fn check(current: &str) -> Result<Option<ReleaseInfo>, String> {
    let body = http_get(RELEASES, 2 << 20)?;
    let json: Value = serde_json::from_slice(&body).map_err(|error| error.to_string())?;
    let tag = json
        .get("tag_name")
        .and_then(Value::as_str)
        .ok_or("the release has no tag")?;
    if !is_newer(tag, current) {
        return Ok(None);
    }
    let assets = json
        .get("assets")
        .and_then(Value::as_array)
        .map(|assets| {
            assets
                .iter()
                .filter_map(|asset| {
                    Some(Asset {
                        name: asset.get("name")?.as_str()?.to_owned(),
                        url: asset.get("browser_download_url")?.as_str()?.to_owned(),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(Some(ReleaseInfo {
        version: tag.trim_start_matches('v').to_owned(),
        title: json
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or(tag)
            .to_owned(),
        notes: json
            .get("body")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        page: json
            .get("html_url")
            .and_then(Value::as_str)
            .unwrap_or(RELEASE_PAGE)
            .to_owned(),
        assets,
    }))
}

/// Start a check on a worker thread. The receiver yields exactly one terminal event, after any
/// `Checking` event.
pub fn spawn_check(current: String, tx: Sender<UpdateEvent>) {
    std::thread::spawn(move || {
        let _ = tx.send(UpdateEvent::Checking);
        let event = match check(&current) {
            Ok(Some(release)) => UpdateEvent::Available(release),
            Ok(None) => UpdateEvent::UpToDate,
            Err(error) => UpdateEvent::Failed(error),
        };
        let _ = tx.send(event);
    });
}

/// Start an install on a worker thread. The release must be the one returned by [`check`].
pub fn spawn_install(release: ReleaseInfo, tx: Sender<UpdateEvent>) {
    std::thread::spawn(move || {
        let result = install(&release, &tx);
        let event = match result {
            Ok(InstallResult::Restarting) => UpdateEvent::Restarting,
            Ok(InstallResult::OpenPage(page)) => UpdateEvent::OpenPage(page),
            Err(error) => UpdateEvent::Failed(error),
        };
        let _ = tx.send(event);
    });
}

enum InstallResult {
    Restarting,
    OpenPage(String),
}

fn install(release: &ReleaseInfo, tx: &Sender<UpdateEvent>) -> Result<InstallResult, String> {
    if install_kind() == InstallKind::Android {
        let apk = android_asset(release).ok_or("the release has no Android APK")?;
        return Ok(InstallResult::OpenPage(apk.url.clone()));
    }
    if install_kind() == InstallKind::Unsupported {
        return Ok(InstallResult::OpenPage(release.page.clone()));
    }
    let asset = platform_asset(release).ok_or("the release has no asset for this platform")?;
    let sums = release
        .assets
        .iter()
        .find(|asset| asset.name == "SHA256SUMS.txt")
        .ok_or("the release has no checksums")?;
    let dir = crate::config::config_dir().join("updates");
    fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    let download = dir.join("download.bin");
    download_to(&asset.url, &download, tx)?;

    let _ = tx.send(UpdateEvent::Verifying);
    let checksums = http_get(&sums.url, 1 << 20)?;
    let expected = checksum_for(&checksums, &asset.name)
        .ok_or_else(|| format!("no checksum for {}", asset.name))?;
    let actual = sha256_file(&download)?;
    if !actual.eq_ignore_ascii_case(&expected) {
        let _ = fs::remove_file(&download);
        return Err("the downloaded update failed its checksum".to_owned());
    }

    let _ = tx.send(UpdateEvent::Installing);
    match install_kind() {
        InstallKind::AppImage => install_appimage(&download, tx),
        InstallKind::Windows => install_windows(&download, tx),
        InstallKind::MacOs => install_macos(&download, tx),
        InstallKind::Android => unreachable!("handled above"),
        InstallKind::Unsupported => unreachable!("handled above"),
    }
}

fn install_appimage(download: &Path, tx: &Sender<UpdateEvent>) -> Result<InstallResult, String> {
    let target = env::var_os("APPIMAGE")
        .map(PathBuf::from)
        .ok_or("this build is not an AppImage")?;
    let staged = target.with_extension("new");
    fs::copy(download, &staged).map_err(|error| error.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let mut permissions = fs::metadata(&staged)
            .map_err(|error| error.to_string())?
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&staged, permissions).map_err(|error| error.to_string())?;
    }
    fs::rename(&staged, &target).map_err(|error| error.to_string())?;
    Command::new(&target)
        .spawn()
        .map_err(|error| error.to_string())?;
    let _ = tx.send(UpdateEvent::Restarting);
    Ok(InstallResult::Restarting)
}

fn install_windows(download: &Path, tx: &Sender<UpdateEvent>) -> Result<InstallResult, String> {
    let current = env::current_exe().map_err(|error| error.to_string())?;
    let staged = current.with_file_name("dnsrace.update.exe");
    extract_member(download, "dnsrace.exe", &staged)?;
    let script = crate::config::config_dir().join("dnsrace-update.cmd");
    let script_text = format!(
        "@echo off\r\ntimeout /t 2 /nobreak >nul\r\nset tries=0\r\n:wait\r\nmove /y \"{}\" \"{}\" >nul 2>&1\r\nif errorlevel 1 (\r\nset /a tries+=1\r\nif %tries% gtr 30 exit /b 1\r\ntimeout /t 1 /nobreak >nul\r\ngoto wait\r\n)\r\nstart \"\" \"{}\"\r\ndel \"%~f0\"\r\n",
        staged.display(),
        current.display(),
        current.display()
    );
    fs::write(&script, script_text).map_err(|error| error.to_string())?;
    Command::new("cmd")
        .arg("/C")
        .arg(&script)
        .spawn()
        .map_err(|error| error.to_string())?;
    let _ = tx.send(UpdateEvent::Restarting);
    Ok(InstallResult::Restarting)
}

fn install_macos(download: &Path, tx: &Sender<UpdateEvent>) -> Result<InstallResult, String> {
    let current = env::current_exe().map_err(|error| error.to_string())?;
    let bundle = current
        .ancestors()
        .find(|path| path.extension().is_some_and(|ext| ext == "app"))
        .ok_or("dnsrace is not running from an app bundle")?
        .to_path_buf();
    let parent = bundle.parent().ok_or("the app bundle has no parent")?;
    let staging = parent.join("dnsrace-update");
    let _ = fs::remove_dir_all(&staging);
    extract_zip(download, &staging)?;
    let new_bundle = find_app(&staging).ok_or("the update has no app bundle")?;
    let replacement = staging.join("replacement.app");
    fs::rename(&new_bundle, &replacement).map_err(|error| error.to_string())?;
    let script = crate::config::config_dir().join("update-dnsrace.sh");
    let text = format!(
        "#!/bin/sh\nsleep 2\nrm -rf \"{}\"\nmv \"{}\" \"{}\"\nopen \"{}\"\nrm -f \"$0\"\n",
        bundle.display(),
        replacement.display(),
        bundle.display(),
        bundle.display()
    );
    fs::write(&script, text).map_err(|error| error.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let mut permissions = fs::metadata(&script)
            .map_err(|error| error.to_string())?
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&script, permissions).map_err(|error| error.to_string())?;
    }
    Command::new("sh")
        .arg(&script)
        .spawn()
        .map_err(|error| error.to_string())?;
    let _ = tx.send(UpdateEvent::Restarting);
    Ok(InstallResult::Restarting)
}

fn platform_asset(release: &ReleaseInfo) -> Option<&Asset> {
    let arch = match env::consts::ARCH {
        "aarch64" => "aarch64",
        _ => "x86_64",
    };
    let kind = install_kind();
    release.assets.iter().find(|asset| match kind {
        InstallKind::AppImage => {
            asset.name.ends_with(".AppImage")
                && asset.name.contains(arch)
                && !asset.name.ends_with(".sha256")
        }
        InstallKind::Windows => asset.name.ends_with("-windows-x86_64.zip"),
        InstallKind::MacOs => asset.name.ends_with(&format!("-macos-{arch}.zip")),
        InstallKind::Android => asset.name.ends_with(".apk"),
        InstallKind::Unsupported => false,
    })
}

fn android_asset(release: &ReleaseInfo) -> Option<&Asset> {
    release
        .assets
        .iter()
        .find(|asset| asset.name.ends_with(".apk"))
}

fn download_to(url: &str, path: &Path, tx: &Sender<UpdateEvent>) -> Result<(), String> {
    let response = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(15))
        .timeout_read(Duration::from_secs(60))
        .build()
        .get(url)
        .set("User-Agent", "dnsrace-updater")
        .call()
        .map_err(|error| error.to_string())?;
    let total = response
        .header("Content-Length")
        .and_then(|value| value.parse::<u64>().ok());
    let mut reader = response.into_reader();
    let mut file = fs::File::create(path).map_err(|error| error.to_string())?;
    let mut buffer = [0u8; 64 * 1024];
    let mut received = 0u64;
    loop {
        let read = reader
            .read(&mut buffer)
            .map_err(|error| error.to_string())?;
        if read == 0 {
            break;
        }
        file.write_all(&buffer[..read])
            .map_err(|error| error.to_string())?;
        received += read as u64;
        let _ = tx.send(UpdateEvent::Downloading { received, total });
    }
    file.flush().map_err(|error| error.to_string())?;
    Ok(())
}

fn http_get(url: &str, limit: u64) -> Result<Vec<u8>, String> {
    let response = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(15))
        .timeout_read(Duration::from_secs(30))
        .build()
        .get(url)
        .set("User-Agent", "dnsrace-updater")
        .set("Accept", "application/vnd.github+json")
        .call()
        .map_err(|error| error.to_string())?;
    let mut body = Vec::new();
    response
        .into_reader()
        .take(limit)
        .read_to_end(&mut body)
        .map_err(|error| error.to_string())?;
    Ok(body)
}

fn checksum_for(list: &[u8], name: &str) -> Option<String> {
    let text = std::str::from_utf8(list).ok()?;
    for line in text.lines() {
        let mut parts = line.split_whitespace();
        let Some(hash) = parts.next() else {
            continue;
        };
        let Some(file) = parts.next() else {
            continue;
        };
        let file = file.trim_start_matches('*');
        if file == name && hash.len() == 64 {
            return Some(hash.to_owned());
        }
    }
    None
}

fn sha256_file(path: &Path) -> Result<String, String> {
    let mut file = fs::File::open(path).map_err(|error| error.to_string())?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer).map_err(|error| error.to_string())?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn extract_member(archive_path: &Path, ends_with: &str, target: &Path) -> Result<(), String> {
    let file = fs::File::open(archive_path).map_err(|error| error.to_string())?;
    let mut archive = zip::ZipArchive::new(file).map_err(|error| error.to_string())?;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(|error| error.to_string())?;
        if entry.name().ends_with(ends_with) {
            let mut output = fs::File::create(target).map_err(|error| error.to_string())?;
            std::io::copy(&mut entry, &mut output).map_err(|error| error.to_string())?;
            return Ok(());
        }
    }
    Err(format!("the update has no {ends_with}"))
}

fn extract_zip(archive_path: &Path, directory: &Path) -> Result<(), String> {
    let file = fs::File::open(archive_path).map_err(|error| error.to_string())?;
    let mut archive = zip::ZipArchive::new(file).map_err(|error| error.to_string())?;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(|error| error.to_string())?;
        let Some(relative) = entry.enclosed_name() else {
            continue;
        };
        let target = directory.join(relative);
        if entry.is_dir() {
            fs::create_dir_all(&target).map_err(|error| error.to_string())?;
            continue;
        }
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        let mut output = fs::File::create(&target).map_err(|error| error.to_string())?;
        std::io::copy(&mut entry, &mut output).map_err(|error| error.to_string())?;
        #[cfg(unix)]
        if let Some(mode) = entry.unix_mode() {
            use std::os::unix::fs::PermissionsExt as _;
            let _ = fs::set_permissions(&target, fs::Permissions::from_mode(mode));
        }
    }
    Ok(())
}

fn find_app(directory: &Path) -> Option<PathBuf> {
    let mut stack = vec![directory.to_path_buf()];
    while let Some(path) = stack.pop() {
        for entry in fs::read_dir(&path).ok()? {
            let entry = entry.ok()?;
            let path = entry.path();
            if path.extension().is_some_and(|ext| ext == "app") {
                return Some(path);
            }
            if path.is_dir() {
                stack.push(path);
            }
        }
    }
    None
}

/// Compare dotted versions, ignoring a leading `v` and any pre-release suffix.
pub fn is_newer(latest: &str, current: &str) -> bool {
    fn parts(version: &str) -> Vec<u64> {
        version
            .trim_start_matches('v')
            .split(['-', '+'])
            .next()
            .unwrap_or_default()
            .split('.')
            .map(|part| part.parse().unwrap_or(0))
            .collect()
    }
    let latest = parts(latest);
    let current = parts(current);
    for index in 0..latest.len().max(current.len()) {
        let left = latest.get(index).copied().unwrap_or(0);
        let right = current.get(index).copied().unwrap_or(0);
        if left != right {
            return left > right;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newer_versions_are_detected() {
        assert!(is_newer("v0.9.1", "0.9.0"));
        assert!(is_newer("1.0", "0.9.9"));
        assert!(!is_newer("v0.9.0", "0.9.0"));
        assert!(!is_newer("0.8.9", "0.9.0"));
    }

    #[test]
    fn checksum_lines_are_parsed() {
        let list = b"not-a-checksum-line\nabc  dnsrace.AppImage\n0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef  dnsrace.zip\n";
        assert_eq!(
            checksum_for(list, "dnsrace.zip").as_deref(),
            Some("0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef")
        );
        assert!(checksum_for(list, "other.zip").is_none());
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "android")))]
    #[test]
    fn download_page_is_the_fallback_for_unsupported_builds() {
        assert_eq!(install_kind(), InstallKind::Unsupported);
    }
}
