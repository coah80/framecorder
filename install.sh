#!/bin/sh
# Builds framecorder on the Steam Frame and sets it up to run from the
# SteamVR dashboard. Run it on the headset, from this folder.
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
install -Dm755 target/release/framecorder "$BIN/framecorder"
install -Dm755 target/release/framecorder-ui "$BIN/framecorder-ui"

# Reading the compositor's framebuffers needs CAP_SYS_ADMIN. Only the
# recorder gets it, the dashboard tab runs without.
echo "giving the recorder permission to read the display (needs your password)"
sudo setcap cap_sys_admin+ep "$BIN/framecorder"

install -Dm644 packaging/framecorder-ui.service "$UNIT/framecorder-ui.service"
systemctl --user daemon-reload
systemctl --user enable framecorder-ui.service
systemctl --user restart framecorder-ui.service

echo "setting up Wi-Fi sync"
(cd sync && ./install.sh)

echo "done: open the SteamVR dashboard and look for the framecorder tab"
