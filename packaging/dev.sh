#!/bin/sh
# Puts what's on the dev branch up as the dev build: a prerelease called
# "dev-build" on github, which the dev install command (see AGENTS.md) and dev
# installs' updates download from. Nothing released changes, the site and
# everyone else stay on main's releases.
#
# Run it from a computer with gh and bun, on the dev branch, with the headset
# reachable over ssh (FRAME, "frame" by default). It builds the headset
# release on the headset, the installer here, and uploads both.
#
#   packaging/dev.sh
set -eu

FRAME=${FRAME:-frame}
TAG=dev-build
OUT=dist/dev
cd "$(dirname "$0")/.."

if [ "$(git branch --show-current)" != dev ]; then
    echo "this is for the dev branch, you're on $(git branch --show-current)" >&2
    exit 1
fi
mkdir -p "$OUT"

echo "building the headset release on $FRAME"
rsync -a --delete --exclude target --exclude .git --exclude app/target --exclude app/gen --exclude sync/target \
    --exclude dist --exclude installer/node_modules --exclude installer/dist ./ "$FRAME:framecorder-dev/"
ssh "$FRAME" 'cd framecorder-dev && PATH=$HOME/.cargo/bin:$PATH packaging/release.sh >/dev/null'
scp -q "$FRAME:framecorder-dev/dist/framecorder-arm64.tar.gz" "$FRAME:framecorder-dev/dist/framecorder-arm64.tar.gz.sha256" "$OUT/"

echo "building the installer"
installer/build.sh "$OUT" >/dev/null

commit=$(git rev-parse --short HEAD)
notes="the dev build, from dev at $commit. not a release: see AGENTS.md for installing it."
if gh release view "$TAG" </dev/null >/dev/null 2>&1; then
    gh release edit "$TAG" --prerelease --target dev --notes "$notes" </dev/null >/dev/null
else
    gh release create "$TAG" --prerelease --target dev --title "dev build" --notes "$notes" </dev/null >/dev/null
fi
gh release upload "$TAG" --clobber </dev/null "$OUT"/framecorder-arm64.tar.gz "$OUT"/framecorder-arm64.tar.gz.sha256 \
    "$OUT"/framecorder-installer.zst "$OUT"/framecorder-installer.zst.sha256
echo "dev build is up, from $commit"
