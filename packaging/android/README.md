# dnsrace Android packaging

dnsrace 0.8.0 ships as one universal debug-signed APK. It is a 100% Rust
application built with `cargo-apk` 0.10 and an `android-native-activity`
entry point. There is no Java/Kotlin code, no `VpnService`, no root, and no
background service: Android is a benchmark and guided-setup app only.

## Toolchain

- JDK 17
- Android SDK platform 36, build-tools 36.0.0, platform-tools
- Android NDK 30.0.16248370 (`r30`), required for compile SDK 36
- Rust targets `armv7-linux-androideabi`, `aarch64-linux-android`,
  `x86_64-linux-android`
- `cargo-apk` 0.10.0

Point the build at the SDK and NDK:

```sh
export ANDROID_HOME="$HOME/Android/Sdk"
export NDK_HOME="$HOME/Android/android-ndk-r30"
```

NDK r27c tops out at platform 35; cargo-apk rejects API 36 with it. Use r30 or
newer.

## Build

`[package.metadata.android]` in `Cargo.toml` pins package
`io.github.yousef_mohamed22.dnsrace`, minSdk 26 (Android 8.0), target and
compile SDK 36, and the three ABIs in one APK. `target_sdk_version` also
selects the compiled-against `platforms/android-36/android.jar`; cargo-apk
0.10 has no separate `compile_sdk` key.

```sh
./packaging/build-icons.sh
cargo apk build --lib
mkdir -p dist
cp target/debug/apk/dnsrace.apk dist/dnsrace-0.8.0-android-debug.apk
aapt2 dump badging dist/dnsrace-0.8.0-android-debug.apk
```

`--lib` is required because the crate also has a desktop binary; without it
cargo-apk would try to package `src/main.rs` as well.

The debug-signed dev APK is large and slow. For a phone, build the optimized
release profile and sign it with the debug keystore:

```sh
export CARGO_APK_RELEASE_KEYSTORE="$HOME/.android/debug.keystore"
export CARGO_APK_RELEASE_KEYSTORE_PASSWORD=android
cargo apk build --lib --release
cp target/release/apk/dnsrace.apk \
  dist/dnsrace-0.8.0-android-release-debugkey.apk
```

`[profile.release]` uses `lto`, `codegen-units = 1`, and `strip = "strip"`; the
result is about 14 MB instead of the debug APK's 146 MB. Release APKs compress
the `.so` files, so `[package.metadata.android.application]` sets
`extract_native_libs = true`. Put it under `application`, not at the top level
of `[package.metadata.android]`, or cargo-apk silently ignores it and install
fails with `Failed to extract native libraries, res=-2`.

Both APKs are debug-key signed (`~/.android/debug.keystore`, generated on first
use). They install normally on Android 8.0+ and in Waydroid (the x86_64 ABI is
included for that reason), but they are not suitable for Play Store
distribution.

## Release signing (later)

The release workflow reads a real keystore from CI secrets when one is
configured:

- `CARGO_APK_RELEASE_KEYSTORE`
- `CARGO_APK_RELEASE_KEYSTORE_PASSWORD`

The key alias and key password are supplied to the signing step alongside
those variables. If a signed release is requested without a keystore, the
release job must fail clearly instead of silently publishing a debug APK.
For 0.8.0 the phone artifact is
`dnsrace-0.8.0-android-release-debugkey.apk` and the larger dev artifact is
`dnsrace-0.8.0-android-debug.apk`; both are debug-key signed.

## Phone testing

```sh
adb install -r dist/dnsrace-0.8.0-android-release-debugkey.apk
adb shell am start -W -n \
  io.github.yousef_mohamed22.dnsrace/android.app.NativeActivity
```

On HyperOS, enable **Install via USB** first. Use plain `adb install -r`;
`adb install -g` fails on this device.

## Resources

`packaging/android/res` contains:

- legacy `mipmap-*/ic_launcher.png` icons at 48/72/96/144/192 px;
- `mipmap-*/ic_launcher_foreground.png` adaptive foregrounds inside the
  centred 66 dp safe zone;
- `mipmap-anydpi-v26/ic_launcher.xml` with the deep-ink background colour;
- `drawable/ic_launcher_monochrome.xml` for Android 13 themed icons.

Everything is regenerated from the two SVG sources by
`./packaging/build-icons.sh`; do not edit the PNGs by hand.

## Android behaviour

The app can:

- benchmark resolvers over UDP, TCP, DoT, and DoH using `INTERNET`;
- read the active network's DNS servers through
  `ConnectivityManager`/`LinkProperties` using `ACCESS_NETWORK_STATE`;
- copy a DoT hostname, open Private DNS settings (Android 9+), open Wi-Fi
  settings (Android 8.0/8.1), and share a text report.

It never changes DNS itself and never requests storage or VPN permissions.
