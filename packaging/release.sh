#!/bin/sh
# Builds a release for Frame Drop: one zip with the installer and everything
# it installs, and the manifest that points at it. Run it on the headset (or
# any arm64 SteamOS), from the repo's root. it also needs a windows c compiler
# for the launcher frame drop starts (see packaging/framedrop), zig is easiest:
# pip install ziglang
#
#   packaging/release.sh https://you.github.io/framecorder/dl
#
# then put both files from dist/ on a github release. the site workflow copies
# the zip to github pages, which is where the manifest points: frame drop
# can't use github's release download urls directly (they redirect to a url
# with no .zip on the end).
set -eu

BASE=${1:?where the zip will be downloaded from, like https://you.github.io/framecorder/dl}
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

mkdir -p "$WORK/payload/bin" "$WORK/payload/services" "$WORK/zip" "$OUT"
cp target/release/framecorder target/release/framecorder-ui sync/target/release/framecorder-sync "$WORK/payload/bin/"
cp packaging/framecorder-ui.service packaging/framecorder-sync.service "$WORK/payload/services/"
tar -cf "$WORK/zip/payload.tar" -C "$WORK/payload" bin services
# frame drop uploads without the executable bit, so the installer rides in a
# tar and a windows launcher (the one program in the zip, so there's no doubt
# what to launch) gets it running. see packaging/framedrop/launcher.c
mkdir -p "$WORK/setup"
cp target/release/framecorder-setup "$WORK/setup/"
tar -cf "$WORK/zip/setup.tar" -C "$WORK/setup" framecorder-setup
cp packaging/framedrop/install.sh "$WORK/zip/"
packaging/framedrop/build.sh "$WORK/zip"

ZIP="$OUT/framecorder-arm64.zip"
rm -f "$ZIP"
python3 - "$WORK/zip" "$ZIP" <<'PY'
import os, sys, zipfile
src, out = sys.argv[1], sys.argv[2]
with zipfile.ZipFile(out, "w", zipfile.ZIP_DEFLATED, compresslevel=9) as z:
    for name in sorted(os.listdir(src)):
        path = os.path.join(src, name)
        info = zipfile.ZipInfo.from_file(path, name)
        info.compress_type = zipfile.ZIP_DEFLATED
        info.external_attr = 0o644 << 16
        with open(path, "rb") as f:
            z.writestr(info, f.read(), compresslevel=9)
PY

SUM=$(sha256sum "$ZIP" | cut -d' ' -f1)
cat > "$OUT/framecorder.framedrop.json" <<JSON
{
  "schema": "framedrop.install/v1",
  "name": "framecorder",
  "files": [
    {
      "url": "$BASE/framecorder-arm64.zip",
      "sha256": "$SUM"
    }
  ]
}
JSON
ls -l "$OUT"
echo "upload both to $BASE"
