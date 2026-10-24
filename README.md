# dnsrace

> Find the fastest DNS resolver for **your** connection, then apply it in one click.

The "best" DNS server depends on where you are and how your ISP routes traffic, so a top-10
list from the internet cannot tell you which one is fastest for you. dnsrace measures public
resolvers from your own machine, ranks them by real lookup latency, checks their DNS security
behaviour, and can switch your system to the winner.

**dnsrace 0.6.0 is a desktop application.** There is no terminal or CLI mode: launch the app,
choose the servers you want, and use the window. Linux x86_64 and aarch64 AppImages are
provided, along with Windows and macOS builds.

## Features

- **A native desktop interface** with a searchable server picker, protocol and address-family
  filters, live progress, a sortable results table, and a plain-language recommendation.
- **Test all or test selected.** Scan the whole catalogue for the current protocol, or compare
  only the resolvers you have checked.
- **UDP, TCP, DNS-over-TLS and DNS-over-HTTPS.** DoH uses RFC 8484 wire-format queries. Some
  providers that only publish HTTP/2 DoH endpoints are included through their DoT endpoint.
- **Cached and uncached measurements.** Every server gets an untimed warm-up, then popular-domain
  and random-name lookups. The table shows medians, jitter, p95/p99, timeouts and a combined
  score.
- **DNS security checks.** dnsrace checks DNSSEC validation and deliberately broken signatures,
  plus NXDOMAIN hijacking. A resolver that accepts a bogus DNSSEC answer is never recommended or
  applied.
- **Apply and revert on Linux, Windows and macOS.** The previous resolver configuration is saved
  before a change, and the app can restore it later.
- **History and reports.** The last 20 runs are kept locally and can be reopened or exported as
  JSON, CSV or a self-contained HTML report.
- **Custom resolvers, steering checks, themes and persistent settings.**
- **No telemetry, no account and no background service.**

## Download

| Platform | File |
| --- | --- |
| Linux x86_64 | `dnsrace-0.6.0-x86_64.AppImage` |
| Linux aarch64 | `dnsrace-0.6.0-aarch64.AppImage` |
| Windows x86_64 | `dnsrace-0.6.0-windows-x86_64.zip` |
| macOS Intel | `dnsrace-0.6.0-macos-x86_64.zip` |
| macOS Apple silicon | `dnsrace-0.6.0-macos-aarch64.zip` |

Every release includes `SHA256SUMS.txt`. AppImages also have individual `.sha256` files.

### Linux

```sh
chmod +x dnsrace-0.6.0-x86_64.AppImage
./dnsrace-0.6.0-x86_64.AppImage
```

If your system does not have FUSE, use the AppImage's extract-and-run mode:

```sh
./dnsrace-0.6.0-x86_64.AppImage --appimage-extract-and-run
```

The x86_64 AppImage is built on Ubuntu 22.04 and the aarch64 AppImage on Ubuntu 24.04; both need a reasonably recent glibc and desktop stack.
Installing DNS settings uses NetworkManager (`nmcli`) when available, otherwise systemd-resolved
(`resolvectl`). A desktop polkit prompt may ask for permission.

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

## Using dnsrace

1. **Pick a protocol and address family.** UDP is the normal choice. TCP helps on networks that
   block or mangle UDP port 53. DoT and DoH test encrypted resolvers but cannot be applied as the
   system resolver.
2. **Choose servers.** Use the search and category filters, tick individual rows, use
   **Select visible**, or keep the **Default selection**. **Test selected** benchmarks only the
   checked servers; **Test all** benchmarks the entire catalogue for the current protocol.
3. **Start the run.** The app quick-scans every candidate, then fully tests the responders. Rows
   appear as servers finish, so you can watch progress instead of waiting for a blank window.
4. **Review the recommendation.** The top card shows the fastest recommendable resolver, its
   cached/uncached latency and DNSSEC status. Click any row or **Use** to choose a different
   resolver; the recommendation card follows your choice.
5. **Apply or revert.** **Apply to system** asks for confirmation and then changes the active
   connection. The previous settings are written to `applied.json`, and the banner or the
   **Revert** button restores them.

Only plain DNS resolvers on port 53 can be applied. DoT and DoH entries are benchmark-only.
A resolver with a bogus DNSSEC result is blocked from applying; other unhealthy results can be
applied with a warning if you explicitly choose them.

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

Files are `settings.json`, `history.json` and `applied.json`. History is capped at 20 runs.
Set `DNSRACE_CONFIG_DIR` to use a different directory, which is useful for portable or test
setups.

dnsrace does not send telemetry. During normal use it contacts the resolver endpoints being
tested and asks Cloudflare's 1.1.1.1 for the DNS CHAOS `id.server` record used by the edge
display. Optional features make additional requests: Cloudflare's HTTPS trace endpoint and
public-dns.info when **Add nearby-country open resolvers** is enabled. Exports are written to
Downloads by default, or to the folder chosen in Settings.

## Building from source

You need Rust 1.99 or newer and a working desktop build environment. On Debian/Ubuntu:

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
./packaging/build-appimage.sh 0.6.0 x86_64 target/release/dnsrace dist
```

The script stages an AppDir, validates the desktop entry and AppStream metadata when the tools
are installed, downloads `appimagetool` when necessary, and writes
`dist/dnsrace-0.6.0-x86_64.AppImage` plus its `.sha256`.

Run the test suite and lints:

```sh
cargo fmt --all -- --check
cargo test --locked
cargo clippy --all-targets --locked -- -D warnings
```

The engine and platform parsers are covered by unit and integration tests; the integration tests
use simulated DNS servers on loopback and do not open a window.

## Project layout

- `src/gui.rs` — the desktop interface and application state.
- `src/engine.rs` — the benchmark pipeline shared by the GUI and tests.
- `src/bench.rs` — quick scan, full tests, DNSSEC, hijacking and steering.
- `src/providers.rs` — the built-in resolver catalogue.
- `src/platform.rs` — apply/revert implementations for Linux, Windows and macOS.
- `src/report.rs` — JSON, CSV and HTML exports.
- `src/config.rs` — settings, history and applied-state persistence.
- `packaging/` — AppImage assets and build script.

## License

MIT. See [LICENSE](LICENSE).
