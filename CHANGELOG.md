# Changelog

All notable changes to dnsrace are listed here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project uses
[Semantic Versioning](https://semver.org/).

## [0.7.0] - 2026-10-30

### Added

- **Cross-platform DNS auto-detection.** Linux reads systemd-resolved per-link data, the active
  NetworkManager connection, the default route and resolver files; Windows uses PowerShell with a
  `pwsh` fallback and a registry fallback; macOS uses `scutil`, `networksetup` and the default
  route. The active interface, gateway and detection source are shown in the interface.
- **Provider-aware detected addresses**, for example `1.1.1.1 (Cloudflare)`, in the toolbar,
  sidebar, recommendation, clipboard summary, JSON details and HTML report.
- **Refresh detection**, **Test current DNS** and **Copy** actions, including background detection
  so the window opens immediately.
- **Eight themes:** System, Dark, Light, Midnight, Nord, Solarized Light, Dracula and High
  contrast. `System` follows the operating system preference.
- **Sortable column headings** for Provider, Server, Cached, Uncached, Score, DNSSEC and Health,
  with ascending/descending order.
- **Health and category filters** for results, in addition to text search.
- **Current-DNS comparison** in the recommendation, with an `IN USE` badge and the addresses an
  apply will replace.
- **Keyboard shortcuts:** Ctrl/Cmd+R or F5 refreshes detection, Ctrl/Cmd+Enter tests selected,
  Ctrl/Cmd+Shift+Enter tests all, Ctrl/Cmd+E exports HTML and Esc cancels.
- **History scanned/tested counts**, with backward-compatible defaults for old files.
- **A `main` snapshot workflow** that builds Linux, Windows and macOS artifacts on every push to
  `main`, and **release-tag validation** that checks the version and release notes before building.

### Changed

- **Detection is asynchronous** and no longer blocks the first frame.
- **Themes apply a full palette** (background, panels, borders, hover, selection and text) instead
  of only switching the egui base style.
- **The HTML report only includes Steering when steering was checked**, and the JSON report adds
  `current_dns_details` while keeping the stable `current_dns` list.
- **System candidates are only added to plain-DNS runs**, so encrypted runs do not test plain IP
  candidates that cannot answer.
- **Exports are collision-safe:** a second export in the same second gets a numeric suffix.
- **Rust is pinned to 1.99.1** in `rust-toolchain.toml`, CI and release workflows.
- **GitHub Actions were refreshed** (`checkout@v5`, current artifact actions, release concurrency).

### Fixed

- **0.6.0 did not compile with current `rustls-native-certs` 0.8**; the `CertificateResult` API is
  now handled correctly.
- **DNS replies must echo the question** (name, type and class), not just the transaction ID.
- **DoH supports all query types and DNSSEC/AAAA checks.**
- **A panicked full-test task no longer freezes progress.**
- **NetworkManager connection names with escaped colons** are parsed correctly.
- **The current-DNS display and `IN USE` badge update after Apply or Revert** instead of
  continuing to describe the old resolver configuration.
- **Non-UTF-8 platform command output** is decoded lossily, so a localized Windows adapter name
  cannot discard the detected DNS addresses.
- **The GUI no longer runs blocking detection in its constructor.**

### Updated

- Version **0.7.0**, minimum and tested Rust **1.99.1**.
- README, AppStream metadata and release notes for 0.7.0.
- CI, release and new `main` snapshot workflows.

### Verified

- 98 unit tests and 9 engine integration tests pass on Rust 1.99.1.
- `cargo fmt --all -- --check` and `cargo clippy --all-targets --locked -- -D warnings` are clean.
- `cargo build --release --locked` succeeds.
- The Linux x86_64 AppImage builds, extracts, and passes `desktop-file-validate` and
  `appstreamcli validate`.

## [0.6.0] - 2026-10-24

### Added

- **A GUI-only desktop application.** The window is now the product: server picker, protocol and
  address-family filters, search, categories, default selection, live progress, sortable results,
  recommendation, apply/revert, history, settings and themes. There is no terminal or CLI mode.
- **Test all and Test selected.** Run the whole current catalogue or only the checked resolvers.
  The quick scan finds responders first; every responder is then fully tested.
- **DNS-over-TLS (DoT)** on port 853, including endpoints that only publish HTTP/2 DoH services.
- **Expanded DNS-over-HTTPS (DoH) catalogue**, using RFC 8484 wire-format queries. Providers include
  Cloudflare, Google, Quad9, AdGuard, CleanBrowsing, OpenDNS, DNS.SB, Control D and others.
- **DNS security checks** for DNSSEC validation and deliberately broken signatures. A resolver
  that accepts a bogus DNSSEC answer is shown as **Bogus accepted**, is never recommended, and is
  blocked from being applied.
- **System DNS apply/revert** on Linux (NetworkManager or systemd-resolved), Windows (PowerShell
  with UAC) and macOS (`networksetup` with the administrator prompt). The previous configuration
  is saved in `applied.json` before the change and can be restored from the app.
- **Run history** of the last 20 runs, with reopen and export as JSON, CSV or HTML.
- **Custom resolvers** for plain DNS, DoT and DoH, including an optional fixed IP for DoT.
- **Steering checks** for one or more hostnames, with returned addresses and TCP connect attempts.
- **Persistent settings** for protocol, family, timeout, jobs, rounds, selected servers, custom
  resolvers, steering hostnames, export folder and theme.
- **AppImage packaging** for Linux x86_64 and aarch64, with desktop entry, scalable icon and
  AppStream metadata.
- **GUI-only CI and release workflows** that build AppImages, Windows archives and macOS app
  bundles and publish combined SHA-256 checksums.

### Changed

- **The binary always opens the GUI.** `src/main.rs` only starts `dnsrace::gui::run()`.
- **The benchmark engine is now library code** shared by the GUI and the integration tests, with
  progress events, incremental rows and cancellation.
- **Apply is restricted to plain port-53 resolvers.** DoT/DoH results remain benchmark-only, and
  custom non-53 ports are never applied.
- **Windows release builds use the GUI subsystem**, so no console window appears behind the app.
- **rustls now uses the `ring` provider only**, avoiding the ambiguous dual-provider configuration
  that would panic when opening the first DoT connection.
- **The app ID is `io.github.yousef_mohamed22.dnsrace`** for the desktop entry, AppStream metadata
  and AppImage.
- **Rust 1.99 / edition 2024** remain the minimum and edition.

### Removed

- The CLI argument parser, terminal tables, ANSI colours and every CLI-only flag.
- The `gui` Cargo feature, `--gui`, `--json`, `--csv`, `--quiet`, `--verbose`, `--export-*`,
  `--doh`, `--tcp`, `--nearby`, `--countries`, `--servers`, `--add`, `--top`, `--repeat`,
  `--no-detect`, `--no-color` and the old `--features gui` build mode.
- The old `src/app.rs` and `src/cli.rs` entry points.

### Fixed

- **A custom resolver on a non-53 port could be applied** by falling back to its IP address even
  though the port was not 53. It is now rejected.
- **Windows revert did not clear an applied IPv6 resolver** when the adapter originally had no
  static IPv6 DNS servers. Revert now resets both families and restores the saved values.
- **Windows builds referenced a Unix-only helper** when choosing PowerShell; the command lookup is
  now implemented for Windows as well.
- **The history window reopened after loading a run** because its close state was overwritten.
- **The results table overflowed the right edge** at smaller window widths. Columns adapt to the
  available width, and the Use button remains visible.
- **DNSSEC labels in reports and the GUI** now consistently use the same security vocabulary.

### Verified

- 83 unit tests and 9 engine integration tests pass on Rust 1.99.1.
- `cargo fmt --check` and `cargo clippy --all-targets --locked -- -D warnings` are clean.
- DoH and DoT were exercised against Cloudflare from the development machine.
- The x86_64 AppImage was built locally, extracted, validated with `desktop-file-validate` and
  `appstreamcli`, and launched successfully in a graphical session.

## [0.5.0] - 2026-10-22

### Added

- **Graphical user interface**: `--gui` opens a native desktop window (egui/eframe) with
  settings, live progress, results table, and copy-to-clipboard. Built with `cargo build
  --features gui`.
- **DNS-over-HTTPS (DoH)**: `--doh` adds Cloudflare DoH and Google DoH as testable servers,
  queried over HTTPS instead of port 53.
- **TCP DNS**: `--tcp` sends DNS queries over TCP instead of UDP, useful for networks that
  block or intercept UDP port 53.
- **Multiple hostnames**: pass several `HOSTNAME` arguments for steering checks against
  multiple sites at once.
- **Enhanced statistics**: `--verbose` now also shows mean, max, and p99 latency alongside
  min, p95, and jitter.
- **File export**: `--export-json <FILE>`, `--export-csv <FILE>`, and `--export-html <FILE>`
  write results to disk in addition to stdout.
- **HTML report**: `--export-html` produces a self-contained, styled HTML report with the
  full results table, recommendation, and verdict.

### Changed

- **Version 0.5.0.** `rust-version` stays at 1.99, edition 2024.
- **`HOSTNAME` is now repeatable:** you can pass multiple hostnames for steering checks.
- **`Row` gains `is_doh`, `cached_mean`, `cached_max`, `cached_p99` fields** for DoH support
  and richer statistics.

### Fixed

- **Multiple hostnames now produce multiple steering reports.** Every requested site is
  displayed in order instead of silently showing only the first one. JSON includes all of them
  in a new `steerings` array (while keeping `steering` for compatibility), and HTML exports
  include a steering section for each hostname.
- **DNS-over-HTTPS rows are excluded from steering checks.** Their placeholder address is not a
  real port-53 server, so they can no longer receive hostname queries.
- **The GUI shows the full benchmark report**, including steering results, instead of only
  `exit code: 0`. Hostname entries are honored, failures display a useful message, and JSON/CSV
  output is no longer polluted by a trailing "Done" line.
- **HTML reports keep the results table aligned** and HTML-escape server names, data-center
  names and hostnames.
- **GUI builds cleanly with `--features gui`** on Linux (glow), Windows (wgpu), and macOS (wgpu).
  The GUI uses eframe's re-exported egui so its versions cannot drift apart, and Windows resolves
  a single `windows` crate version so `wgpu-hal` can compile.
- **The GUI no longer hangs forever** if its benchmark thread dies: it reports that the worker
  stopped instead of leaving a spinner running.
- **Release workflow fixed:** Linux ARM64 builds natively on an ARM runner, CLI and GUI binaries
  are built separately for every platform, Windows packaging produces the expected `.exe` files,
  macOS uses standard checksums, and the release job publishes one consolidated
  `SHA256SUMS.txt`.

## [0.4.0] - 2026-10-20

### Added

- **`--nearby`**: detects your country through Cloudflare and also tests the open resolvers
  public-dns.info lists there. One flag instead of looking up your country code first.
- **`-q, --quiet`**: prints exactly one line, the recommended address, for shell scripts. It
  exits 1 when no healthy server can be recommended.
- **`--csv`**: one row per server, ready for a spreadsheet. `--json`, `--csv` and `--quiet` are
  mutually exclusive; with `--csv`, the `HOSTNAME` steering check is skipped with a note.
- **`-v, --verbose`**: a second table with the fastest cached lookup (`min`), the 95th percentile
  (`p95`), jitter (standard deviation) and the DNSSEC result.
- **`--repeat <N>`**: repeats the full test up to five times and pools the measurements. Useful
  when the top servers are within a millisecond of each other.
- **`--top <N>`**: limits the tables to the N fastest rows without changing the summary, JSON or
  CSV.
- **`-4, --ipv4` and `-6, --ipv6`**: restrict the whole run to one address family, for networks
  with broken IPv6.
- **DNSSEC check**: one extra query per server asks for DNSSEC records (DO bit) and reads the
  Authenticated Data (AD) bit. The result appears in `-v`, JSON and CSV. It is reported as
  "could not check" when the server does not answer.
- **Latency spread in JSON**: `min_ms`, `p95_ms` and `jitter_ms` for every server.
- **`recommended` in JSON**, plus the existing `fastest` and `fastest_known` fields.
- **A real verdict**: the summary now says whether switching would save time, how much, and
  whether the result is too close to call.
- **Progress with percentage and elapsed time** during the full test.

### Changed

- **Rust 1.99 / edition 2024.** `rust-version` in `Cargo.toml`, the lint toolchain and the MSRV
  CI job are all on 1.99. Release binaries are still built with stable Rust.
- **Dependencies refreshed**: tokio 1.54 and ureq 2.12 (latest 2.x, with native certificates).
- **The summary is easier to read.** "Fastest known provider" is now "Recommended"; the fastest
  overall server is shown separately when it differs; your current DNS shows the gap in ms; a
  close-call hint appears when the top two are within 1.5 ms.
- **`--timeout` now affects the quick scan too**, capped at 3 seconds so a large server list
  cannot make stage 1 take minutes.
- **README rewritten** around the common cases: one command to start, one to get local
  resolvers, one to script it.
- **Release archives now include `CHANGELOG.md`.**

### Fixed

- **Cached results depended on who had run the benchmark before.** The first run against a
  resolver paid for cache misses, so the same server could be 35 ms on one run and 10 ms on the
  next. Each server now gets an untimed warm-up pass (ten lookups at once) before measurement, so
  "cached" means the same thing on every run.
- **NOERROR replies with no A/AAAA records were counted as successes.** A server that returned
  only a CNAME (or an empty answer) could look healthy. A usable cached answer now needs both an
  answer record and at least one address.
- **The quick scan stopped retrying after a SERVFAIL or an empty answer.** It returned "failed"
  immediately instead of trying the second attempt.
- **`--connect-port 0` was accepted** and produced meaningless TCP checks. It is now rejected
  with a clear message.
- **`--timeout` was ignored by the quick scan**, so `--timeout 3000` did not help a slow link in
  stage 1.
- **A current DNS server of the other address family was reported as "did not answer"** when
  using `--ipv4` or `--ipv6`, even though it had simply been filtered out. Current DNS is now
  filtered consistently before the scan and the shortlist.
- **Your router, current DNS and `--add` servers consumed `--known` slots.** With `--known 2`,
  two of the three full-test slots could go to servers you had already selected, leaving fewer
  public resolvers tested. These servers are now always fully tested in addition to the `--known`
  and `--other` picks.
- **A run with no servers selected** (for example `--known 0 --other 0`) now exits 1 with a
  message instead of printing an empty result.

### Verified

- 66 unit tests and 12 end-to-end tests pass with the `gui` feature on Rust 1.99.
- `cargo fmt --check` and `cargo clippy --all-targets --features gui -- -D warnings` are clean.
- End-to-end tests cover quiet, CSV, JSON, verbose/DNSSEC, repeat, IPv4/IPv6 filtering, ranking,
  hijacking, unreliable servers, multiple steering hostnames, file exports and HTML reports.

## [0.3.0] - 2026-10-17

### Added

- First Rust-only release: no shell or Python required.
- Two-stage benchmark: a quick scan of every candidate, then a full test of the fastest.
- Cached and uncached lookups, medians, timeouts, `HIJACKS-NXDOMAIN` and `UNRELIABLE` flags.
- Built-in list of about 85 public resolvers, IPv4 and IPv6.
- `--countries`, `--known`, `--other`, `--servers`, `--add`, `--jobs`, `--timeout`,
  `--connect-port`, `--no-detect`, `--json` and `--no-color`.
- Router and current-DNS detection on Linux, macOS and Windows.
- Hostname steering check with TCP connect times.
- Cross-platform CI and release workflows.

[0.7.0]: https://github.com/Yousef-Mohamed22/dnsrace/compare/v0.6.0...v0.7.0
[0.6.0]: https://github.com/Yousef-Mohamed22/dnsrace/compare/v0.5.0...v0.6.0
[0.5.0]: https://github.com/Yousef-Mohamed22/dnsrace/releases/tag/v0.5.0
[0.4.0]: https://github.com/Yousef-Mohamed22/dnsrace/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/Yousef-Mohamed22/dnsrace/releases/tag/v0.3.0
