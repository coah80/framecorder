#!/bin/sh
# Builds a release: framecorder-arm64.tar.gz, the installer and everything it
# installs. site/install downloads it. Run it on the headset (or any arm64
# SteamOS), from the repo's root:
#
#   packaging/release.sh
#
# then put both files from dist/ on a github release. the site workflow
# copies them to the site's /dl, where site/install gets the release and
# installed headsets look for updates (see setup::update).
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
cp target/release/framecorder target/release/framecorder-ui target/release/framecorder-setup target/release/framecorder-grab \
    sync/target/release/framecorder-sync "$WORK/payload/bin/"
cp packaging/framecorder-ui.service packaging/framecorder-sync.service \
    packaging/framecorder-update.service packaging/framecorder-update.timer "$WORK/payload/services/"
tar -cf "$WORK/release/payload.tar" -C "$WORK/payload" bin services
cp target/release/framecorder-setup "$WORK/release/"

TARBALL="$OUT/framecorder-arm64.tar.gz"
tar -czf "$TARBALL" --owner=0 --group=0 -C "$WORK/release" framecorder-setup payload.tar
# what installed headsets check every few hours for an update
(cd "$OUT" && sha256sum framecorder-arm64.tar.gz >framecorder-arm64.tar.gz.sha256)
# and its version under it, which the apps show (updaters only read the first word)
echo "version $(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n1)" >>"$OUT/framecorder-arm64.tar.gz.sha256"
ls -l "$OUT"
echo "put both on a github release"
