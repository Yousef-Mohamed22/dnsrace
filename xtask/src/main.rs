//! Repository maintenance tasks: icon generation and Linux AppImage packaging.
//!
//! These were shell scripts before 0.9.0. Python, shell and other helper runtimes are kept out of
//! the repository so GitHub reports a single language. External tools still do the actual image
//! conversion and AppImage assembly; this binary validates arguments, stages files and reports
//! failures with a non-zero exit status.

use std::env;
use std::fs::{self, File};
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use sha2::{Digest as _, Sha256};

const APP_ID: &str = "io.github.yousef_mohamed22.dnsrace";
const USAGE: &str = "\
dnsrace repository tasks

Usage:
  cargo run -p xtask -- icons
  cargo run -p xtask -- appimage <version> <x86_64|aarch64> [binary] [output-dir]

The appimage command stages an AppDir, validates the desktop and AppStream metadata when the
tools are installed, downloads appimagetool when necessary, and writes the AppImage plus a
SHA-256 sidecar. The icons command regenerates desktop and Android rasters from the master SVG.
";

fn main() -> ExitCode {
    match cli() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("packaging: {error}");
            ExitCode::FAILURE
        }
    }
}

fn cli() -> Result<(), String> {
    let args: Vec<String> = env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("icons") => {
            if args.len() != 1 {
                return Err("the icons command takes no arguments".to_owned());
            }
            icons()
        }
        Some("appimage") => appimage(&args[1..]),
        Some("-h" | "--help" | "help") => {
            print!("{USAGE}");
            Ok(())
        }
        Some(other) => Err(format!("unknown command: {other}\n\n{USAGE}")),
        None => Err(USAGE.to_owned()),
    }
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask must live inside the repository")
        .to_path_buf()
}

fn icons() -> Result<(), String> {
    let packaging = repo_root().join("packaging");
    let svg = packaging.join(format!("{APP_ID}.svg"));
    let foreground = packaging.join("android/ic_launcher_foreground.svg");
    for path in [&svg, &foreground] {
        if !path.is_file() {
            return Err(format!("missing source image: {}", path.display()));
        }
    }
    for tool in ["rsvg-convert", "magick", "identify"] {
        if !has_tool(tool) {
            return Err(format!("missing required tool: {tool}"));
        }
    }

    for size in [16, 32, 64, 128, 256, 512, 1024] {
        let value = size.to_string();
        run(Command::new("rsvg-convert")
            .arg("--width")
            .arg(&value)
            .arg("--height")
            .arg(&value)
            .arg(&svg)
            .arg("--output")
            .arg(packaging.join(format!("icon-{size}.png"))))?;
    }

    let icon_1024 = packaging.join("icon-1024.png");
    let ico = packaging.join("icon.ico");
    run(Command::new("magick")
        .arg(&icon_1024)
        .arg("-define")
        .arg("icon:auto-resize=256,128,64,48,32,16")
        .arg(&ico))?;

    let mut identify = Command::new("identify");
    for size in [16, 32, 64, 128, 256, 512, 1024] {
        identify.arg(packaging.join(format!("icon-{size}.png")));
    }
    identify.arg(&ico);
    run(&mut identify)?;

    let android = packaging.join("android/res");
    for (density, size) in [
        ("mdpi", 48),
        ("hdpi", 72),
        ("xhdpi", 96),
        ("xxhdpi", 144),
        ("xxxhdpi", 192),
    ] {
        let directory = android.join(format!("mipmap-{density}"));
        fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
        let value = size.to_string();
        run(Command::new("rsvg-convert")
            .arg("--width")
            .arg(&value)
            .arg("--height")
            .arg(&value)
            .arg(&svg)
            .arg("--output")
            .arg(directory.join("ic_launcher.png")))?;
    }

    for (density, size) in [
        ("mdpi", 108),
        ("hdpi", 162),
        ("xhdpi", 216),
        ("xxhdpi", 324),
        ("xxxhdpi", 432),
    ] {
        let directory = android.join(format!("mipmap-{density}"));
        fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
        let value = size.to_string();
        run(Command::new("rsvg-convert")
            .arg("--width")
            .arg(&value)
            .arg("--height")
            .arg(&value)
            .arg(&foreground)
            .arg("--output")
            .arg(directory.join("ic_launcher_foreground.png")))?;
    }

    let mut identify = Command::new("identify");
    for density in ["mdpi", "hdpi", "xhdpi", "xxhdpi", "xxxhdpi"] {
        identify
            .arg(android.join(format!("mipmap-{density}/ic_launcher.png")))
            .arg(android.join(format!("mipmap-{density}/ic_launcher_foreground.png")));
    }
    run(&mut identify)?;

    println!("wrote desktop rasters, icon.ico and Android mipmaps");
    Ok(())
}

fn appimage(args: &[String]) -> Result<(), String> {
    if !(2..=4).contains(&args.len()) {
        return Err(
            "usage: xtask appimage <version> <x86_64|aarch64> [binary] [output-dir]".to_owned(),
        );
    }
    let version = &args[0];
    let arch = &args[1];
    if !matches!(arch.as_str(), "x86_64" | "aarch64") {
        return Err(format!("unsupported architecture: {arch}"));
    }

    let root = repo_root();
    let binary = root.join(args.get(2).map_or("target/release/dnsrace", String::as_str));
    let out_dir = root.join(args.get(3).map_or("dist", String::as_str));
    if !binary.is_file() {
        return Err(format!(
            "missing binary: {} (run cargo build --release --locked first)",
            binary.display()
        ));
    }

    let build_dir = root.join("target/appimage");
    let appdir = build_dir.join("DnsRace.AppDir");
    if appdir.exists() {
        fs::remove_dir_all(&appdir).map_err(|error| error.to_string())?;
    }
    fs::create_dir_all(build_dir.join("DnsRace.AppDir/usr/bin"))
        .map_err(|error| error.to_string())?;
    for directory in [
        "usr/share/applications",
        "usr/share/icons/hicolor/scalable/apps",
        "usr/share/metainfo",
        "usr/share/licenses/dnsrace",
    ] {
        fs::create_dir_all(appdir.join(directory)).map_err(|error| error.to_string())?;
    }
    fs::create_dir_all(&out_dir).map_err(|error| error.to_string())?;

    copy_file(&binary, &appdir.join("usr/bin/dnsrace"), true)?;
    copy_file(
        &root.join(format!("packaging/{APP_ID}.desktop")),
        &appdir.join(format!("usr/share/applications/{APP_ID}.desktop")),
        false,
    )?;
    copy_file(
        &root.join(format!("packaging/{APP_ID}.desktop")),
        &appdir.join(format!("{APP_ID}.desktop")),
        false,
    )?;
    copy_file(
        &root.join(format!("packaging/{APP_ID}.svg")),
        &appdir.join(format!(
            "usr/share/icons/hicolor/scalable/apps/{APP_ID}.svg"
        )),
        false,
    )?;
    copy_file(
        &root.join(format!("packaging/{APP_ID}.svg")),
        &appdir.join(format!("{APP_ID}.svg")),
        false,
    )?;
    copy_file(
        &root.join(format!("packaging/{APP_ID}.metainfo.xml")),
        &appdir.join(format!("usr/share/metainfo/{APP_ID}.metainfo.xml")),
        false,
    )?;
    copy_file(
        &root.join("LICENSE"),
        &appdir.join("usr/share/licenses/dnsrace/LICENSE"),
        false,
    )?;

    let apprun = appdir.join("AppRun");
    fs::write(
        &apprun,
        "#!/bin/sh\nset -e\nhere=\"${APPDIR:-$(dirname \"$(readlink -f \"$0\")\")}\"\nexec \"$here/usr/bin/dnsrace\" \"$@\"\n",
    )
    .map_err(|error| error.to_string())?;
    set_mode(&apprun, 0o755)?;
    let dir_icon = appdir.join(".DirIcon");
    #[cfg(unix)]
    std::os::unix::fs::symlink(format!("{APP_ID}.svg"), &dir_icon)
        .map_err(|error| error.to_string())?;
    #[cfg(not(unix))]
    copy_file(
        &root.join(format!("packaging/{APP_ID}.svg")),
        &dir_icon,
        false,
    )?;

    let desktop = appdir.join(format!("usr/share/applications/{APP_ID}.desktop"));
    if has_tool("desktop-file-validate") {
        run(Command::new("desktop-file-validate").arg(&desktop))?;
    }
    let metainfo = appdir.join(format!("usr/share/metainfo/{APP_ID}.metainfo.xml"));
    if has_tool("appstreamcli") {
        run(Command::new("appstreamcli")
            .arg("validate")
            .arg("--no-net")
            .arg(&metainfo))?;
    }

    let tool = match env::var_os("APPIMAGETOOL") {
        Some(path) => PathBuf::from(path),
        None => {
            let path = build_dir.join(format!("appimagetool-{arch}.AppImage"));
            if !path.is_file() {
                let url = format!(
                    "https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-{arch}.AppImage"
                );
                println!("downloading appimagetool from {url}");
                download(&url, &path)?;
            }
            set_mode(&path, 0o755)?;
            path
        }
    };
    if !tool.is_file() {
        return Err(format!("missing appimagetool: {}", tool.display()));
    }

    let out = out_dir.join(format!("dnsrace-{version}-{arch}.AppImage"));
    println!("building {}", out.display());
    run(Command::new(&tool)
        .env("ARCH", arch)
        .env("VERSION", version)
        .env("APPIMAGE_EXTRACT_AND_RUN", "1")
        .arg(&appdir)
        .arg(&out))?;

    let hash = sha256_file(&out)?;
    let name = out
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or("the output file has no name")?;
    fs::write(
        out_dir.join(format!("{name}.sha256")),
        format!("{hash}  {name}\n"),
    )
    .map_err(|error| error.to_string())?;
    println!("created {}", out.display());
    Ok(())
}

fn copy_file(source: &Path, target: &Path, executable: bool) -> Result<(), String> {
    if !source.is_file() {
        return Err(format!("missing file: {}", source.display()));
    }
    fs::copy(source, target).map_err(|error| {
        format!(
            "could not copy {} to {}: {error}",
            source.display(),
            target.display()
        )
    })?;
    set_mode(target, if executable { 0o755 } else { 0o644 })
}

#[cfg(unix)]
fn set_mode(path: &Path, mode: u32) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt as _;

    let mut permissions = fs::metadata(path)
        .map_err(|error| error.to_string())?
        .permissions();
    permissions.set_mode(mode);
    fs::set_permissions(path, permissions).map_err(|error| error.to_string())
}

#[cfg(not(unix))]
fn set_mode(_path: &Path, _mode: u32) -> Result<(), String> {
    Ok(())
}

fn has_tool(tool: &str) -> bool {
    env::var_os("PATH").is_some_and(|paths| {
        env::split_paths(&paths).any(|directory| directory.join(tool).is_file())
    })
}

fn download(url: &str, target: &Path) -> Result<(), String> {
    if !has_tool("curl") {
        return Err("missing required tool: curl".to_owned());
    }
    run(Command::new("curl")
        .arg("-fL")
        .arg("--retry")
        .arg("3")
        .arg("--connect-timeout")
        .arg("20")
        .arg("-o")
        .arg(target)
        .arg(url))
}

fn run(command: &mut Command) -> Result<(), String> {
    let program = command.get_program().to_string_lossy().into_owned();
    let status = command
        .status()
        .map_err(|error| format!("could not run {program}: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{program} failed with {status}"))
    }
}

fn sha256_file(path: &Path) -> Result<String, String> {
    let mut file = File::open(path).map_err(|error| error.to_string())?;
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
