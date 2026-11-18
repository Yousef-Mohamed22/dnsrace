# dnsrace

[English](README.md) · [العربية](README_AR.md)

<img width="1366" height="768" alt="dnsrace showdown" src="https://github.com/user-attachments/assets/c5732c9f-0beb-4d06-b08b-f6bbf7a801e6" />

> Find the fastest DNS resolver for **your** connection, then apply it in one click.

The "best" DNS server depends on where you are and how your ISP routes traffic, so a top-10
list from the internet cannot tell you which one is fastest for you. dnsrace measures public
resolvers from your own machine, ranks them by real lookup latency, checks their DNS security
behaviour, and can switch your system to the winner.

**dnsrace 0.9.0 runs on desktop and Android 8.0+.** The desktop app can apply and revert DNS.
Android is a benchmark and guided-setup platform: dnsrace measures resolvers and then helps you
enter the winner in Private DNS (Android 9+) or per-network Wi-Fi DNS (Android 8.0–8.1). There
is no iOS or other Apple phone target and no VPN or background service. A headless CLI is
available for scripts and servers.

## Features

- **Arabic interface with full right-to-left layout.** Every screen, dialog and Android guide is
  translated. Choose Arabic, English or the device language in Settings; the layout mirrors the
  navigation rail, bottom bar, cards and table alignment. Noto Sans Arabic is bundled, so no
  system font is required.
- **Built-in updates.** dnsrace checks the latest GitHub release at startup (optional) and from
  Settings. Linux, Windows and macOS downloads are verified against the release
  `SHA256SUMS.txt` and installed in place before the app restarts. Android opens the APK
  download link and leaves installation to the system.
- **One adaptive interface for desktop and phone.** The desktop uses a navigation rail, a dense
  server table and a detail pane. Android uses an app bar, bottom navigation, 48 dp touch
  controls, server cards and bottom sheets. The phone layout reflows automatically and fits
  360 dp and 320 dp screens without horizontal scrolling.
- **Android 8.0 and newer.** Minimum API 26, target API 36, with `arm64-v8a`,
  `armeabi-v7a` and `x86_64` native libraries in one APK. The app is 100% Rust; there is no
  Java, Kotlin or shell source code.
- **Guided Private DNS setup.** Android 9+ gets a measured DoT hostname (or a clearly labelled
  provider hostname when only metadata is available) and a shortcut to the Private DNS screen.
  Android 8.0–8.1 gets per-network Wi-Fi DNS steps instead.
- **Automatic DNS detection on Linux, Windows, macOS and Android.** dnsrace reads the resolvers
  your device is actually using, shows the active interface, gateway and detection source, and
  labels known addresses with their provider (for example `1.1.1.1 (Cloudflare)`).
- **Test profiles.** Quick, Balanced, Thorough or Custom. A profile sets timeout, jobs, repeat,
  cached rounds and random lookups; editing any value switches to Custom. Android defaults to
  Balanced + DoT; desktop defaults to Balanced + UDP.
- **Favourites.** Star resolvers in Servers or a result card, then filter by favourites or run
  **Test favourites**.
- **Explained recommendations.** See the fastest recommendable resolver, the fastest plain
  resolver desktop can apply, the fastest encrypted resolver measured in this run, the delta
  against your current DNS, close-call warnings, and plain-language reasons.
- **History trends and detailed metrics.** A winner-score sparkline, profile and depth in every
  history entry, and cached/uncached latency, jitter, timeouts, DNSSEC, hijack status and
  security details for every resolver.
- **Compare against what you have.** The recommendation marks a resolver that is already
  configured as **IN USE** and shows which current DNS addresses an apply would replace.
- **Test all, test selected, test favourites, or test the current DNS.**
- **UDP, TCP, DNS-over-TLS and DNS-over-HTTPS.** DoH uses RFC 8484 wire-format queries and
  supports every query type used by the benchmark, including AAAA and DNSSEC probes.
- **DNS security checks.** dnsrace checks DNSSEC validation and deliberately broken signatures,
  plus NXDOMAIN hijacking. A resolver that accepts a bogus DNSSEC answer is never recommended or
  applied.
- **Apply and revert on Linux, Windows and macOS.** The previous resolver configuration is saved
  before a change, and the app can restore it later.
- **History and reports.** The last 20 runs are kept locally and can be reopened. Desktop export
  supports JSON, CSV, HTML and a text summary; Android shares a text report through the system
  share sheet. Reports record the test profile and depth.
- **Eight themes:** System, Dark, Light, Midnight, Nord, Solarized Light, Dracula and High
  contrast. **System** follows the operating system's light/dark preference.
- **Custom resolvers, steering checks and persistent settings.**
- **Resolver backup and transfer.** Export custom resolvers and favourites as JSON, then import
  and merge them on another machine. Duplicates are skipped and invalid entries are reported.
- **Diagnostics report.** Copy version, OS, language, theme, profile, current DNS, last run,
  update state and config paths in one text block for support requests.
- **Headless CLI.** `dnsrace --cli` runs the same engine without a window, prints a ranked table
  and can export JSON or CSV. It is available on desktop builds.
- **No telemetry, no account, no root and no background service.**

## Download

| Platform | File |
| --- | --- |
| Linux x86_64 | `dnsrace-0.9.0-x86_64.AppImage` |
| Linux aarch64 | `dnsrace-0.9.0-aarch64.AppImage` |
| Windows x86_64 | `dnsrace-0.9.0-windows-x86_64.zip` |
| macOS Intel | `dnsrace-0.9.0-macos-x86_64.zip` |
| macOS Apple silicon | `dnsrace-0.9.0-macos-aarch64.zip` |
| Android 8.0+ | `dnsrace-0.9.0-android-release-debugkey.apk` |

Every release includes `SHA256SUMS.txt`; AppImages and the Android APK also have individual
`.sha256` files. Pushes to `main` produce separate snapshot builds from the **Main snapshot**
workflow; those artifacts are development snapshots, not tagged releases.

The 0.9.0 APK is signed with the Android debug keystore, not a Play Store release key. It is
fine for sideloading and testing.

### Linux

```sh
chmod +x dnsrace-0.9.0-x86_64.AppImage
./dnsrace-0.9.0-x86_64.AppImage
```

If your system does not have FUSE, use the AppImage's extract-and-run mode:

```sh
./dnsrace-0.9.0-x86_64.AppImage --appimage-extract-and-run
```

The x86_64 AppImage is built on Ubuntu 22.04 and the aarch64 AppImage on Ubuntu 24.04; both
need a reasonably recent glibc and desktop stack. The app prefers native Wayland when the
compositor is reachable and falls back to X11 when it is not, even if XWayland is available.
Installing DNS settings uses NetworkManager (`nmcli`) when available, otherwise
systemd-resolved (`resolvectl`). A desktop polkit prompt may ask for permission.

### Windows

Unzip the archive and run `dnsrace.exe`. Release builds use the Windows GUI subsystem, so no
console window opens. Applying DNS changes the active adapter through PowerShell and asks for
administrator permission with a UAC prompt.

### macOS

Unzip the archive, move `dnsrace.app` to Applications, and open it. The app is not signed with an
Apple Developer ID, so the first launch may require right-clicking the app and choosing **Open**,
or running:

```sh
xattr -dr com.apple.quarantine dnsrace.app
```

Applying DNS uses `networksetup` and asks for administrator approval. macOS support is provided
on a best-effort basis.

### Android

1. Download `dnsrace-0.9.0-android-release-debugkey.apk`.
2. Allow installs from your browser or file manager when Android asks.
3. Run a benchmark, open a resolver, and use **Set up this resolver** to follow the Private DNS
   (Android 9+) or Wi-Fi DNS (Android 8.0–8.1) steps.

Or install over USB:

```sh
adb install -r dnsrace-0.9.0-android-release-debugkey.apk
```

Android does not let a normal app change Private DNS silently. dnsrace does not request a VPN,
root, or a background service; it measures resolvers and hands you the value to enter. Reports
are shared as text, so the app needs no storage permission or export folder.

## Updates

dnsrace checks the latest GitHub release when it starts, unless you turn
**Check automatically on startup** off in Settings. **Check for updates** in Settings runs the
same check on demand. A newer version shows its release notes and a download action.

- **Linux AppImage, Windows and macOS:** the archive is downloaded, its SHA-256 is checked
  against the release `SHA256SUMS.txt`, and the app installs the new version in place and
  restarts. A failed check or checksum leaves the current version untouched.
- **Android:** Android does not allow an app to replace itself silently. dnsrace opens the new
  APK download link in your browser, and Android installs it after you confirm.

No update channel or version skipping is used: there is one stable release stream.

## Language and right-to-left layout

Settings → Appearance → Language offers **System**, **English** and **Arabic**. **System**
follows the device locale. Arabic switches every screen to right-to-left: the desktop rail moves
to the right, the phone navigation mirrors, card content aligns to the right, and tables and
result rows follow the reading direction. The bundled Noto Sans Arabic font covers Arabic text
without a system font, and the interface stays usable in English on the same installation.

Resolver names, endpoints and other technical values stay in Latin script, as do JSON and CSV
keys, so scripts and saved reports keep working across languages.

## Command line

The same benchmark engine is available without a window:

```sh
dnsrace --cli                          # default catalogue, ranked table
dnsrace --cli --profile quick --top 10
dnsrace --cli --protocol dot --json result.json --csv result.csv
dnsrace --cli --list                   # show what would be tested
dnsrace --cli --help
```

Progress goes to stderr and the table to stdout, so `dnsrace --cli > result.txt` works. Use
`--all` for the whole catalogue, `--no-system` to skip the current resolvers, and
`--timeout`, `--jobs`, `--repeat` and `--family` for custom limits. JSON and CSV keep stable
English keys for scripts. The release Windows binary uses the GUI subsystem, so the CLI is
intended for Linux and macOS release builds (or a Windows debug build from a terminal).

## Import, export and diagnostics

Settings → Data can export custom resolvers and favourites to a JSON file and import them on
another machine. Import merges entries, skips duplicates and invalid protocols, and reports how
many entries were added. The default export path is shown in Settings and can be overridden with
the path field.

**Copy diagnostics** in Settings copies a plain-text support report: version, operating system
and architecture, language, theme, profile and depth, current DNS, last run, update state,
history/custom/favourite counts and config paths. It contains no telemetry and no account data.

## Using dnsrace

1. **Check what you are using.** dnsrace detects the system's DNS servers in the background at
   startup. The current-DNS card shows the active interface, detection source and provider-aware
   addresses. Use **Refresh detection** after changing networks, **Test current** to benchmark
   exactly those servers, or **Copy current DNS** on desktop to copy the addresses.
2. **Pick a protocol and address family.** UDP is the normal desktop choice. TCP helps on
   networks that block or mangle UDP port 53. DoT is the default on a fresh Android install. DoT
   and DoH test encrypted resolvers; desktop cannot apply them as the system resolver.
3. **Choose servers.** Search, filter by category, transport, address family or favourites, tick
   individual rows, or use the default selection. **Test selected** benchmarks only the checked
   servers; **Test all** benchmarks the entire catalogue for the current protocol.
4. **Start the run.** The app quick-scans every candidate, then fully tests the responders. Rows
   appear as servers finish, so you can watch progress instead of waiting for a blank window.
   On desktop, sort by any column heading and click it again to reverse the order.
5. **Review the recommendation.** The card shows the fastest recommendable resolver, its
   cached/uncached latency, DNSSEC status, reasons, and the delta against your current DNS. It
   marks a resolver that is already configured as **IN USE**. Choose another row with **Use this
   resolver** on desktop or **Set up this resolver** on Android; the card follows your choice.
6. **Act on the result.** On desktop, **Apply to system** asks for confirmation and changes the
   active connection. The previous settings are written to `applied.json`, and **Restore
   previous DNS** brings them back. On Android, follow the setup guide and enter the hostname or
   addresses in system settings.

Only plain DNS resolvers on port 53 can be applied. DoT and DoH entries are benchmark-only on
desktop. A resolver with a bogus DNSSEC result is blocked from applying; other unhealthy results
can be applied with a warning if you explicitly choose them. On Android, a selection that only
has a provider's DoT hostname is labelled **“provider's DoT hostname — not measured in this
run”** and the guide offers a DoT benchmark.

### Automatic detection

| Platform | Sources |
| --- | --- |
| Linux | systemd-resolved per-link data, the active NetworkManager connection, the default route, `/etc/resolv.conf` and systemd-resolved's upstream file. |
| Windows | `Get-NetIPConfiguration` through PowerShell, with a `pwsh` fallback and a language-independent registry fallback. |
| macOS | `scutil --dns`, `networksetup` service order and DNS values, plus the default route. |
| Android | `ConnectivityManager` link properties and the active network. |

Detection runs asynchronously, so a slow or missing platform command never delays the first frame.
When no source yields an address, the interface says so and the rest of the app still works.

### Themes and shortcuts

Choose a theme in Settings: **System**, **Dark**, **Light**, **Midnight**, **Nord**,
**Solarized Light**, **Dracula** or **High contrast**. **System** follows the operating system's
light/dark preference. All eight themes remain on both platforms.

Desktop shortcuts:

| Shortcut | Action |
| --- | --- |
| Ctrl/Cmd+R or F5 | Refresh DNS detection |
| Ctrl/Cmd+Enter | Test selected |
| Ctrl/Cmd+Shift+Enter | Test all |
| Ctrl/Cmd+E | Export HTML |
| Esc | Close the topmost dialog, or cancel the current run |

### What the columns mean

| Column | Meaning |
| --- | --- |
| Cached | Median latency for popular domains the resolver is likely to have cached. |
| Uncached | Median latency for random names that force a recursive lookup. |
| Score | Combined ranking value. Health and hijack penalties push bad resolvers down. |
| DNSSEC | Validated, Insecure, Partial, Unknown, or Bogus accepted. |
| Status | OK, Check, Unreliable or Hijacks, based on the full test. |

## Configuration and privacy

Settings, history and the saved applied state live in the platform config directory:

| Platform | Directory |
| --- | --- |
| Linux | `~/.config/dnsrace/` |
| Windows | `%APPDATA%\dnsrace\` |
| macOS | `~/Library/Application Support/dnsrace/` |
| Android | app-private internal storage |

Files are `settings.json`, `history.json` and `applied.json`. History is capped at 20 runs.
Set `DNSRACE_CONFIG_DIR` on desktop to use a different directory, which is useful for portable or
test setups. Android uses its internal data path directly; reports leave the app through the
share sheet rather than a file path.

dnsrace does not send telemetry. During normal use it contacts the resolver endpoints being
tested and asks Cloudflare's 1.1.1.1 for the DNS CHAOS `id.server` record used by the edge
display. Optional features make additional requests: Cloudflare's HTTPS trace endpoint and
public-dns.info when **Include nearby open resolvers** is enabled. Desktop exports are written to
Downloads by default, or to the folder chosen in Settings.

## Building from source

You need Rust **1.100.0** (the repository pins it in `rust-toolchain.toml`) and a working desktop
build environment. On Debian/Ubuntu:

```sh
sudo apt-get install build-essential pkg-config \
  libgl1-mesa-dev libegl1-mesa-dev libfontconfig1-dev \
  libwayland-dev libxkbcommon-dev libxkbcommon-x11-dev \
  libx11-dev libxcursor-dev libxi-dev libxrandr-dev

cargo build --release --locked
./target/release/dnsrace
```

Build a Linux AppImage locally:

```sh
cargo run -p xtask -- appimage 0.9.0 x86_64 target/release/dnsrace dist
```

The task stages an AppDir, validates the desktop entry and AppStream metadata when the tools
are installed, downloads `appimagetool` when necessary, and writes
`dist/dnsrace-0.9.0-x86_64.AppImage` plus its `.sha256`.

### Android APK

You need Java 17, Android SDK platform 36, build-tools 36.0.0, NDK **30.0.16248370**, and
`cargo-apk` 0.10.0. NDK r27 is too old for compileSdk 36.

```sh
rustup target add armv7-linux-androideabi aarch64-linux-android x86_64-linux-android
cargo install cargo-apk --version 0.10.0 --locked

export ANDROID_HOME="$HOME/Android/Sdk"
export ANDROID_NDK_HOME="$HOME/Android/android-ndk-r30"

cargo apk build --lib              # debug APK
cargo apk build --lib --release    # release APK
```

`--lib` is required because the crate also has a desktop binary. The release build is signed
with `CARGO_APK_RELEASE_KEYSTORE`, `CARGO_APK_RELEASE_KEYSTORE_PASSWORD`,
`CARGO_APK_RELEASE_KEY_ALIAS` and `CARGO_APK_RELEASE_KEY_PASSWORD` when those are set; the 0.9.0
release artifact uses the debug keystore unless CI secrets provide a private key.

Run the test suite and lints:

```sh
cargo fmt --all -- --check
cargo test --locked
cargo clippy --all-targets --locked -- -D warnings
```

The engine and platform parsers are covered by unit and integration tests; the integration tests
use simulated DNS servers on loopback and do not open a window. The GUI tests use
`egui_kittest` to render every page at desktop, phone and landscape sizes, click the labelled
controls, and check accessibility, touch sizes, large text and safe areas.

## Project layout

- `src/gui/` — the adaptive interface: `app.rs` (state and effects), `state.rs`, `actions.rs`,
  `backend.rs`, `desktop.rs`, `mobile.rs`, `pages/`, `modals.rs`, `components.rs`, `theme.rs`,
  `icons.rs` and `tests/`.
- `src/engine.rs` — the benchmark pipeline shared by the GUI and tests.
- `src/bench.rs` — quick scan, full tests, DNSSEC, hijacking and steering.
- `src/providers.rs` — the built-in resolver catalogue and provider identification.
- `src/recommend.rs` — the recommendation model, deltas, close calls and reasons.
- `src/android_guide.rs` — the Private DNS and Wi-Fi DNS setup guide.
- `src/android.rs` — the Android JNI host, safe-area insets, sharing and intents.
- `src/system.rs` — cross-platform current-DNS detection and parsers.
- `src/platform.rs` — apply/revert implementations for Linux, Windows and macOS.
- `src/report.rs` — JSON, CSV, HTML and summary reports.
- `src/i18n.rs` — English/Arabic strings, system-language selection and RTL helpers.
- `src/update.rs` — GitHub release checks, SHA-256 verification and platform installers.
- `src/cli.rs` — the `--cli` headless benchmark.
- `src/diagnostics.rs` — the copyable support report.
- `src/transfer.rs` — custom-resolver and favourite import/export.
- `src/config.rs` — settings, history and applied-state persistence.
- `packaging/` — AppImage assets, Android resources and the icon build script.

## License

MIT. See [LICENSE](LICENSE).
