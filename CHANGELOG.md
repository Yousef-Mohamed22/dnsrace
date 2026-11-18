# Changelog

All notable changes to dnsrace are listed here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project uses
[Semantic Versioning](https://semver.org/).

## [0.9.0] - 2026-11-17

### Added

- **Full Arabic interface and right-to-left layout.** Every desktop page, dialog, mobile shell,
  settings card, notice and Android setup step is translated. Settings offers System, English and
  Arabic; Arabic mirrors the desktop rail, phone navigation, cards, result rows and table
  alignment. Noto Sans Arabic is bundled under the SIL Open Font License, so Arabic renders
  without a system font.
- **In-app updates.** dnsrace checks the latest GitHub release at startup (optional) and on
  demand from Settings. Linux AppImage, Windows and macOS downloads are verified against the
  release `SHA256SUMS.txt` before an in-place install and restart. Android opens the APK
  download link in the browser. Failed checks, downloads or checksums leave the current version
  untouched.
- **Headless benchmark CLI.** `dnsrace --cli` reuses the GUI engine for protocol, family, profile,
  timeout, jobs, repeat, catalogue selection and system-DNS inclusion. It prints a ranked table to
  stdout, progress to stderr, and writes optional JSON or CSV reports. `--list`, `--help` and
  `--version` are included.
- **Resolver and favourite transfer.** Settings → Data exports custom resolvers and favourites to
  JSON and imports them with duplicate and protocol validation.
- **Diagnostics report.** A copyable text report includes version, OS and architecture, language,
  theme, profile and depth, current DNS, last run, update state, saved-data counts and config
  paths.
- **Rust 1.100.0** in `Cargo.toml`, `rust-toolchain.toml` and all CI, snapshot and release
  workflows.
- **Tests** for language selection and RTL helpers, update version comparison and checksum
  parsing, resolver transfer validation, diagnostics content and the new Settings controls.

### Changed

- **The Settings page gains Language, Updates, Diagnostics and resolver import/export cards.**
  The update check can be disabled for offline or metered systems.
- **The desktop and phone shells mirror for Arabic.** Card content follows the leading edge,
  wrapped rows reverse, and the mobile page header, notices and action groups align right.
- **Android guide text follows the saved language** while resolver names and technical values
  stay in Latin script.
- **JSON and CSV report keys stay English** so scripts and saved reports remain stable across
  interface languages.
- **Linux uses native Wayland first and falls back to X11 only when no compositor is
  reachable**, even when XWayland is present.
- **Packaging helpers are Rust too.** Icon generation and AppImage assembly moved from shell
  scripts to the repository-local `xtask`, so the repository contains no shell scripts.
- README, Arabic README, AppStream metadata and release notes describe 0.9.0.

### Fixed

- **`applied_age` no longer says “1 days ago”.** A single day uses the singular form.
- **Duplicate Android 8.0 clipboard bullet removed** from the 0.8.0 changelog.
- **The custom-resolver Edit and Remove buttons use translated labels** without overflowing
  narrow phone rows.
- **Long Arabic notices, checklist rows and update release notes wrap** instead of widening the
  page or painting over adjacent controls.
- **The update failure notice distinguishes a failed check from a failed install**, and the
  Settings card always offers a retry after a failure.

### Updated

- Version **0.9.0**; minimum and tested Rust **1.100.0**.
- CI and release workflows pin **1.100.0** and keep the Android API 36, build-tools 36.0.0 and
  NDK 30.0.16248370 toolchain.

### Verified

- Unit and engine integration tests pass on Rust 1.100.0.
- `cargo fmt --all -- --check` and `cargo clippy --all-targets --locked -- -D warnings` are
  clean.
- `cargo build --release --locked` succeeds for desktop.

## [0.8.0] - 2026-11-11

### Added

- **An adaptive interface for desktop and Android.** A new `Signal` visual language, a desktop
  navigation rail, a phone app bar with bottom navigation, 48 dp touch controls, bottom sheets,
  safe-area handling, and automatic reflow that fits 360 dp and 320 dp screens.
- **Android 8.0+ support (API 26), targeting API 36.** A universal APK with `arm64-v8a`,
  `armeabi-v7a` and `x86_64` libraries, built with `cargo-apk` and containing no Java or Kotlin
  application code.
- **Guided Android setup.** Android 9+ copies a measured DoT hostname and opens Private DNS;
  Android 8.0–8.1 shows per-network Wi-Fi DNS steps. Metadata-only hostnames are labelled as
  unmeasured and the guide offers a DoT benchmark.
- **Android-native detection and sharing.** `ConnectivityManager` link properties, system-bar
  insets over JNI, text reports through `ACTION_SEND`, and intent shortcuts for Private DNS and
  Wi-Fi settings.
- **Test profiles:** Quick, Balanced, Thorough and Custom, with persisted timeout, jobs, repeat,
  cached rounds and random lookups. Profile and depth are saved in history, reports and settings.
- **Favourites:** star resolvers, filter by favourites, and run **Test favourites**.
- **Recommendation insights:** the fastest plain and encrypted resolvers, current-DNS deltas,
  close-call warnings, plain-language reasons, and platform-appropriate apply/setup targets.
- **History trends and detailed metrics:** a winner-score sparkline, profile/depth per entry,
  cached and uncached latency, jitter, timeouts, DNSSEC, hijack status and security details.
- **New icon:** a deep-ink rounded tile with three ascending cyan-to-mint bars and a mint pulse
  dot, with desktop, Windows, macOS, adaptive Android, legacy Android and monochrome variants.
- **Custom resolver editing in place**, About links, guide benchmark switches, a mobile detail
  sheet with setup/favourite/website/copy actions, and **Restore previous DNS** on desktop.

### Changed

- **The application is 100% Rust on both platforms.** There is no `VpnService`, root, Java,
  Kotlin or background service.
- **Android is benchmark and guided-setup only.** Apply/revert remains desktop-only.
- **Android defaults to Balanced + DoT; desktop defaults to Balanced + UDP.**
- **The GUI is split into state, actions/effects, backend boundary, shared pages, desktop and
  mobile shells, modals, themes, icons and tests.**
- **Android reports use the system share sheet** instead of a file-export path; desktop keeps
  JSON, CSV, HTML and summary exports.
- **Mobile notices wrap, the mobile results grid is a set of 48 dp rows, and the app bar keeps
  16 dp page padding.** These changes fix the post-`Test all` layout.
- **Mobile advanced benchmark values use a two-column 44 dp grid** instead of squeezing five
  controls into one wrapping row.
- **Rust is pinned to 1.99.1**; rusqlite is not used, and the release profile strips symbols.

### Fixed

- **The phone layout no longer expands or clips after `Test all`.** Long completion notices,
  ISP/gateway resolver labels and the old three-column results grid no longer widen the page.
- **DNS replies must echo the question** (name, type and class), not just the transaction ID.
- **DNSSEC bogus classification follows the rcode matrix:** only `NOERROR` from
  `dnssec-failed.org` means accepted; `SERVFAIL` means correctly rejected; `NXDOMAIN`,
  `REFUSED`, timeouts and other errors mean unknown.
- **Config writes are crash-safe on Windows** using a temporary file, backup and atomic replace.
- **Exports reserve a unique path atomically** and never overwrite an existing report.
- **Android paths and platform name are correct**, and Linux detection commands are never run on
  a phone.
- **History entries stay score-sorted after reopening.**
- **Missing Android hostnames, denied intents and clipboard failures produce readable notices
  and manual steps.**
- **Private DNS setup falls back to Wireless settings or Settings** on phones whose firmware
  omits `android.settings.PRIVATE_DNS_SETTINGS`, with Java stack traces trimmed from notices.
- **Long resolver titles no longer overlap the detail-sheet close button**, and the disabled
  desktop-only right-hand half is omitted.
- **Custom resolver Edit/Remove actions wrap instead of painting over the name** on narrow
  phone rows.
- **History timestamps show UTC once**, not twice.
- **Opening About scrolls the card into view** instead of leaving it above the viewport.
- **Back exits the Android process cleanly**, so reopening always starts a fresh UI instead of a
  reused zombie window.
- **Run scopes count only resolvers still in the catalogue**, so stale IDs cannot enable an
  empty run.
- **Copying works on Android 8.0.** `ClipboardManager` constructs a `Handler` on the calling
  thread there, so the JNI thread now prepares a Looper before requesting the service. Without
  this, Copy produced a “Can't create handler inside thread that has not called
  Looper.prepare()” stack trace on API 26.
- **The filled favourite star is a proper five-point star.** epaint fills only convex paths, so
  the concave star is now drawn as a centre fan instead of a distorted convex polygon.
- **The row-detail sheet starts at the top every time.** It now uses a fresh scroll identity per
  opening instead of inheriting the previous sheet's offset, which could clip the title.

### Updated

- Version **0.8.0**, minimum and tested Rust **1.99.1**.
- README, CHANGELOG, AppStream metadata and 0.8.0 release notes.
- CI gains an Android debug-APK job with `aapt2` manifest/ABI assertions; the release workflow
  builds and signs the APK; the main snapshot workflow uploads a 14-day Android artifact.
- GitHub Actions use Java 17, Android SDK 36, build-tools 36.0.0 and NDK 30.0.16248370.

### Verified

- 255 unit tests and 9 engine integration tests pass on Rust 1.99.1.
- `cargo fmt --all -- --check` and `cargo clippy --all-targets --locked -- -D warnings` are
  clean.
- `cargo build --release --locked` succeeds for desktop.
- The APK passes `aapt2 dump badging` checks for package, 0.8.0, minSdk 26, targetSdk 36,
  `INTERNET` and `ACCESS_NETWORK_STATE`, and all three ABIs.
- The phone layout was exercised on a Redmi A7 Pro running Android 16 at 360 dp: every control
  on every page and dialog, all profiles and filters, runs/cancel, detail sheets, history,
  settings/reset, all eight themes, the custom-resolver editor, About, rotation,
  Back-to-exit/fresh relaunch, and a 45-second idle check with no crash.
- The Android 8.0–8.1 Wi-Fi DNS branch was runtime-verified on an Android 8.0 (API 26) x86_64
  emulator: install, layout, detection, benchmark, Wi-Fi guide, copy and intents. The physical
  Huawei Y7 2018 could not be used because its USB data pins never enumerate.
- The filled-star geometry has a unit test that checks the centre fan covers the outline exactly.

[0.7.0]: https://github.com/Yousef-Mohamed22/dnsrace/compare/v0.6.0...v0.7.0
[0.6.0]: https://github.com/Yousef-Mohamed22/dnsrace/compare/v0.5.0...v0.6.0
[0.5.0]: https://github.com/Yousef-Mohamed22/dnsrace/releases/tag/v0.5.0
[0.4.0]: https://github.com/Yousef-Mohamed22/dnsrace/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/Yousef-Mohamed22/dnsrace/releases/tag/v0.3.0
