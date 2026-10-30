#!/usr/bin/env bash
# Build a relocatable Linux AppImage for dnsrace.
#
# Usage:
#   packaging/build-appimage.sh <version> <arch> [binary] [output-dir]
#
# <arch> is x86_64 or aarch64. The script downloads appimagetool if it is not already
# available, stages an AppDir, validates the desktop/AppStream metadata when the tools are
# installed, and writes dist/dnsrace-<version>-<arch>.AppImage.
set -euo pipefail

if [[ $# -lt 2 ]]; then
    echo "usage: $0 <version> <x86_64|aarch64> [binary] [output-dir]" >&2
    exit 2
fi

version="$1"
arch="$2"
binary="${3:-target/release/dnsrace}"
out_dir="${4:-dist}"
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
build_dir="$root/target/appimage"
appdir="$build_dir/DnsRace.AppDir"
app_id="io.github.yousef_mohamed22.dnsrace"
out="$out_dir/dnsrace-${version}-${arch}.AppImage"

case "$arch" in
    x86_64|aarch64) ;;
    *) echo "unsupported architecture: $arch" >&2; exit 2 ;;
esac

if [[ ! -f "$binary" ]]; then
    echo "missing binary: $binary (run cargo build --release --locked first)" >&2
    exit 1
fi

mkdir -p "$build_dir" "$out_dir"
rm -rf "$appdir"
mkdir -p \
    "$appdir/usr/bin" \
    "$appdir/usr/share/applications" \
    "$appdir/usr/share/icons/hicolor/scalable/apps" \
    "$appdir/usr/share/metainfo" \
    "$appdir/usr/share/licenses/dnsrace"

install -m 0755 "$binary" "$appdir/usr/bin/dnsrace"
install -m 0644 "$root/packaging/${app_id}.desktop" \
    "$appdir/usr/share/applications/${app_id}.desktop"
# appimagetool also looks for the desktop entry and icon at the AppDir root.
install -m 0644 "$root/packaging/${app_id}.desktop" "$appdir/${app_id}.desktop"
install -m 0644 "$root/packaging/${app_id}.svg" \
    "$appdir/usr/share/icons/hicolor/scalable/apps/${app_id}.svg"
install -m 0644 "$root/packaging/${app_id}.svg" "$appdir/${app_id}.svg"
install -m 0644 "$root/packaging/${app_id}.metainfo.xml" \
    "$appdir/usr/share/metainfo/${app_id}.metainfo.xml"
install -m 0644 "$root/LICENSE" "$appdir/usr/share/licenses/dnsrace/LICENSE"
ln -sf "${app_id}.svg" "$appdir/.DirIcon"

cat > "$appdir/AppRun" <<'APPRUN'
#!/bin/sh
set -e
here="${APPDIR:-$(dirname "$(readlink -f "$0")")}"
exec "$here/usr/bin/dnsrace" "$@"
APPRUN
chmod 0755 "$appdir/AppRun"

if command -v desktop-file-validate >/dev/null 2>&1; then
    desktop-file-validate "$appdir/usr/share/applications/${app_id}.desktop"
fi
if command -v appstreamcli >/dev/null 2>&1; then
    appstreamcli validate --no-net "$appdir/usr/share/metainfo/${app_id}.metainfo.xml"
fi

tool="${APPIMAGETOOL:-}"
if [[ -z "$tool" ]]; then
    tool="$build_dir/appimagetool-${arch}.AppImage"
    if [[ ! -x "$tool" ]]; then
        url="https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-${arch}.AppImage"
        echo "downloading appimagetool from $url"
        curl -fL --retry 3 --connect-timeout 20 -o "$tool" "$url"
        chmod +x "$tool"
    fi
fi

echo "building $out"
ARCH="$arch" VERSION="$version" APPIMAGE_EXTRACT_AND_RUN=1 \
    "$tool" "$appdir" "$out"

if command -v sha256sum >/dev/null 2>&1; then
    (cd "$out_dir" && sha256sum "$(basename "$out")" > "$(basename "$out").sha256")
fi
echo "created $out"
