#!/bin/sh
# Gathers the sync app for every platform it's been built for into dist/,
# under the names the website links to. Build them first, app/README.md says
# how. Run from the repo's root, then upload dist/ to the release. macos (and
# windows and linux too) also come from .github/workflows/app.yml on a tag.
set -eu
OUT=dist
mkdir -p "$OUT"

take() {
    if [ -f "$1" ]; then
        cp "$1" "$OUT/$2"
        echo "  $2"
    else
        echo "  $2 is missing: $1 hasn't been built" >&2
    fi
}

echo "gathering"
take app/target/docker/x86_64-pc-windows-msvc/release/framecorder-app.exe framecorder-windows.exe
take app/target/docker/release/framecorder-app framecorder-linux
take android/app/build/outputs/apk/release/app-release.apk framecorder-android.apk
ls -l "$OUT"
