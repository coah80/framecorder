#!/bin/sh
# Builds a release: framecorder-arm64.tar.gz, the installer and everything it
# installs. site/install downloads it. Run it on the headset (or any arm64
# SteamOS), from the repo's root:
#
#   packaging/release.sh
#
# then put dist/framecorder-arm64.tar.gz on a github release. the site
# workflow copies it to the site's /dl, which is where site/install gets it.
set -eu

OUT=dist
WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT

if [ "$(uname -m)" != aarch64 ]; then
    echo "this machine is $(uname -m), the headset is aarch64. build the release there." >&2
    exit 1
fi
command -v cargo >/dev/null 2>&1 || PATH="$HOME/.cargo/bin:$PATH"

echo "building"
cargo build --release
(cd sync && cargo build --release)

mkdir -p "$WORK/payload/bin" "$WORK/payload/services" "$WORK/release" "$OUT"
cp target/release/framecorder target/release/framecorder-ui sync/target/release/framecorder-sync "$WORK/payload/bin/"
cp packaging/framecorder-ui.service packaging/framecorder-sync.service "$WORK/payload/services/"
tar -cf "$WORK/release/payload.tar" -C "$WORK/payload" bin services
cp target/release/framecorder-setup "$WORK/release/"

TARBALL="$OUT/framecorder-arm64.tar.gz"
tar -czf "$TARBALL" --owner=0 --group=0 -C "$WORK/release" framecorder-setup payload.tar
ls -l "$TARBALL"
echo "put it on a github release"
