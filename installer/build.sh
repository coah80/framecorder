#!/bin/sh
# builds the installer for the frame (arm64 linux, one file with bun inside)
# and puts it in the folder given, zstd'd, with its checksum:
#
#   installer/build.sh site/dl
#
# the site workflow runs this, site/install downloads what it makes.
set -eu
OUT=$(cd "${1:?where to put it}" && pwd)
cd "$(dirname "$0")"

# every platform's native opentui package, since this may not be an arm64 machine
bun install --frozen-lockfile --cpu='*' --os='*'
bun build --compile --minify --target=bun-linux-arm64 src/main.ts --outfile dist/framecorder-installer
zstd -19 -q -f dist/framecorder-installer -o "$OUT/framecorder-installer.zst"
(cd "$OUT" && sha256sum framecorder-installer.zst >framecorder-installer.zst.sha256)
ls -l "$OUT"/framecorder-installer.zst*
