#!/bin/sh
# screenshots of every screen into a folder.
# usage: tools/preview/shots.sh [out dir]
set -eu
OUT=${1:-/tmp/fcapp}
HERE=$(cd "$(dirname "$0")" && pwd)
CHROME=$(command -v google-chrome || command -v chromium || command -v chromium-browser)
mkdir -p "$OUT"
python3 "$HERE/serve.py" 8765 >/dev/null &
SERVER=$!
trap 'kill $SERVER' EXIT
sleep 1
shot() {
    "$CHROME" --headless=new --disable-gpu --hide-scrollbars --force-device-scale-factor=1 \
        --virtual-time-budget=4000 --window-size="$3" --screenshot="$OUT/$1.png" \
        "http://localhost:8765/$2&still" >/dev/null 2>&1
}
for s in connected syncing unreachable full forgot empty pair pair-found settings; do
    shot "desktop-$s" "?s=$s" 920,860
done
ls "$OUT"
