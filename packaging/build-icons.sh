#!/usr/bin/env bash
# Regenerate every desktop icon raster from the master SVG.
# Idempotent: running it again overwrites the same files with identical output.
set -euo pipefail
cd "$(dirname "$0")"

for tool in rsvg-convert magick identify; do
    if ! command -v "$tool" >/dev/null 2>&1; then
        echo "build-icons.sh: missing required tool: $tool" >&2
        exit 1
    fi
done

svg="io.github.yousef_mohamed22.dnsrace.svg"

for size in 16 32 64 128 256 512 1024; do
    rsvg-convert --width "$size" --height "$size" "$svg" --output "icon-$size.png"
done

# Windows embeds 16/32/48/64/128/256 in one ICO. ImageMagick derives the 48 px layer.
magick icon-1024.png -define icon:auto-resize=256,128,64,48,32,16 icon.ico

identify icon-16.png icon-256.png icon-1024.png icon.ico >/dev/null

# Android legacy launcher icons: 48 dp at mdpi through 192 dp at xxxhdpi.
android_res="android/res"
for entry in "mdpi 48" "hdpi 72" "xhdpi 96" "xxhdpi 144" "xxxhdpi 192"; do
    set -- $entry
    mkdir -p "$android_res/mipmap-$1"
    rsvg-convert --width "$2" --height "$2" "$svg" --output "$android_res/mipmap-$1/ic_launcher.png"
done

# Adaptive foreground: 108 dp canvas, mark inside the centred 66 dp safe zone.
foreground="android/ic_launcher_foreground.svg"
for entry in "mdpi 108" "hdpi 162" "xhdpi 216" "xxhdpi 324" "xxxhdpi 432"; do
    set -- $entry
    mkdir -p "$android_res/mipmap-$1"
    rsvg-convert --width "$2" --height "$2" "$foreground" \
        --output "$android_res/mipmap-$1/ic_launcher_foreground.png"
done

identify "$android_res"/mipmap-*/ic_launcher*.png >/dev/null
echo "build-icons.sh: wrote desktop rasters, icon.ico and Android mipmaps"
