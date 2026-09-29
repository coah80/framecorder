#!/bin/bash
# End to end on one Linux box: the real daemon against a temp HOME, the app's
# headless mode as the client. Checks pairing, catching up on an existing
# file, a finished .part showing up within a second, resuming a killed
# download with Range, and refusing a wrong fingerprint.
#
#   app/tools/e2e.sh            (from the repo root, needs cargo and gcc)
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/../.." && pwd)
WORK=$(mktemp -d)
PORT=${PORT:-38699}
trap 'kill $(jobs -p) 2>/dev/null || true; rm -rf "$WORK"' EXIT

cargo build -q --release --manifest-path "$ROOT/sync/Cargo.toml"
cargo build -q --no-default-features --manifest-path "$ROOT/app/Cargo.toml"
SYNC="$ROOT/sync/target/release/framecorder-sync"
APP="$ROOT/app/target/debug/framecorder-app"

# a stand-in for SteamVR, so the "game running" throttle can be switched on
cat > "$WORK/fakevr.c" <<'C'
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
static int scene(void) { int s = 0; FILE *f = fopen(getenv("FAKE_SCENE"), "r"); if (f) { if (fscanf(f, "%d", &s) != 1) s = 0; fclose(f); } return s; }
static void *table[32];
intptr_t VR_InitInternal2(int *err, int type, const char *a) { *err = type == 3 ? 0 : 108; return 1; }
void VR_ShutdownInternal(void) {}
void *VR_GetGenericInterface(const char *n, int *err) { if (!strcmp(n, "FnTable:IVRApplications_008")) { table[25] = (void *)scene; *err = 0; return table; } *err = 105; return 0; }
C
gcc -shared -fPIC -o "$WORK/libopenvr_api.so" "$WORK/fakevr.c"
echo 0 > "$WORK/scene"

H="$WORK/frame"
V="$H/Videos/framecorder"
S="$H/.config/framecorder/sync"
mkdir -p "$V/clips"
head -c 20000000 /dev/urandom > "$V/clips/2026-09-26_14-00-00.mp4"

HOME="$H" FAKE_SCENE="$WORK/scene" FRAMECORDER_OPENVR_LIB="$WORK/libopenvr_api.so" \
    "$SYNC" --port "$PORT" --no-mdns > "$WORK/daemon.log" 2>&1 &
for _ in $(seq 50); do [ -f "$S/info.json" ] && break; sleep 0.1; done
FP=$(sed -n 's/.*"fingerprint": "\([0-9a-f]*\)".*/\1/p' "$S/info.json")

pass() { echo "ok: $*"; }
fail() { echo "FAIL: $*"; echo "--- daemon"; cat "$WORK/daemon.log"; exit 1; }
code() { echo "{\"code\":\"$1\",\"expires\":$(( $(date +%s) + 300 ))}" > "$S/pairing.json"; }

# wrong fingerprint: refused, and the code isn't even sent
code 111111
BAD=$(printf 'ab%.0s' $(seq 32))
if "$APP" --headless-sync "$WORK/bad" --state "$WORK/bad-state" \
    --pair "framecorder://pair?host=127.0.0.1&port=$PORT&fp=$BAD&code=111111" > "$WORK/bad.log" 2>&1; then
    fail "paired with the wrong fingerprint"
fi
grep -q "isn't the frame" "$WORK/bad.log" || fail "wrong fingerprint error: $(cat "$WORK/bad.log")"
[ -f "$S/pairing.json" ] || fail "the code got used up by the wrong frame"
pass "wrong fingerprint refused"

# pair and catch up on what's already there
DL="$WORK/dl"
"$APP" --headless-sync "$DL" --state "$WORK/state" \
    --pair "framecorder://pair?host=127.0.0.1&port=$PORT&fp=$FP&code=111111&name=Frame" > "$WORK/app.log" 2>&1 &
APP_PID=$!
for _ in $(seq 100); do [ -f "$DL/clips/2026-09-26_14-00-00.mp4" ] && break; sleep 0.1; done
cmp -s "$DL/clips/2026-09-26_14-00-00.mp4" "$V/clips/2026-09-26_14-00-00.mp4" || fail "existing clip"
pass "paired, existing clip downloaded"

# a writer finishing a clip: .part, then rename
head -c 5000000 /dev/urandom > "$V/clips/x.mp4.part"
sleep 0.5
[ -e "$DL/clips/x.mp4" ] && fail ".part got synced"
START=$(date +%s%N)
mv "$V/clips/x.mp4.part" "$V/clips/x.mp4"
for _ in $(seq 200); do [ -f "$DL/clips/x.mp4" ] && break; sleep 0.01; done
TOOK=$(( ($(date +%s%N) - START) / 1000000 ))
cmp -s "$DL/clips/x.mp4" "$V/clips/x.mp4" || fail "new clip"
[ "$TOOK" -lt 1000 ] || fail "new clip took ${TOOK}ms"
pass "rename to .mp4 synced in ${TOOK}ms"

# a big one while a "game" runs (8 MB/s), killed halfway, then resumed
echo 3 > "$WORK/scene"
head -c 60000000 /dev/urandom > "$V/big.mp4.part"
mv "$V/big.mp4.part" "$V/big.mp4"
sleep 3
kill -9 "$APP_PID"; wait "$APP_PID" 2>/dev/null || true
PART=$(stat -c %s "$DL"/big.mp4.*.part)
[ "$PART" -gt 0 ] && [ "$PART" -lt 60000000 ] || fail "expected a partial download, got $PART bytes"
echo 0 > "$WORK/scene"
"$APP" --headless-sync "$DL" --state "$WORK/state" > "$WORK/app2.log" 2>&1 &
for _ in $(seq 300); do [ -f "$DL/big.mp4" ] && break; sleep 0.1; done
grep -q "resuming big.mp4 at $PART" "$WORK/app2.log" || fail "didn't resume: $(cat "$WORK/app2.log")"
cmp -s "$DL/big.mp4" "$V/big.mp4" || fail "resumed file differs"
pass "killed download resumed from byte $PART"

echo "all good"
