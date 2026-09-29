#!/bin/sh
# Builds framecorder-sync on the Steam Frame and runs it as a user service,
# whenever the headset is on. Run it on the headset, from this folder.
set -eu

BIN="$HOME/.local/bin"
UNIT="$HOME/.config/systemd/user"

if ! command -v cargo >/dev/null 2>&1; then
    if [ -x "$HOME/.cargo/bin/cargo" ]; then
        PATH="$HOME/.cargo/bin:$PATH"
    else
        echo "cargo isn't installed. Get it with:"
        echo "  curl -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal"
        exit 1
    fi
fi

echo "building"
cargo build --release

echo "installing to $BIN"
install -Dm755 target/release/framecorder-sync "$BIN/framecorder-sync"
install -Dm644 ../packaging/framecorder-sync.service "$UNIT/framecorder-sync.service"
systemctl --user daemon-reload
systemctl --user enable framecorder-sync.service
systemctl --user restart framecorder-sync.service

echo "done: pair a device from the framecorder tab in the SteamVR dashboard"
