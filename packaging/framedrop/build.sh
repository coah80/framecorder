#!/bin/sh
# builds framecorder.exe (launcher.c) into the folder given, with whichever
# windows c compiler is around: mingw, or zig (pip install ziglang).
set -eu
OUT=${1:?where to put framecorder.exe}
SRC=$(dirname "$0")/launcher.c

if command -v x86_64-w64-mingw32-gcc >/dev/null 2>&1; then
    x86_64-w64-mingw32-gcc -O2 -s -o "$OUT/framecorder.exe" "$SRC"
elif command -v zig >/dev/null 2>&1; then
    zig cc -target x86_64-windows-gnu -O2 -s -o "$OUT/framecorder.exe" "$SRC"
elif python3 -m ziglang version >/dev/null 2>&1; then
    python3 -m ziglang cc -target x86_64-windows-gnu -O2 -s -o "$OUT/framecorder.exe" "$SRC"
else
    echo "need a windows c compiler for the frame drop launcher: pip install ziglang" >&2
    exit 1
fi
