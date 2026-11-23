#!/bin/sh
# Builds framecorder on the Steam Frame and installs it the way a release
# does, through framecorder-setup. Run it on the headset, from this folder.
set -eu

cd "$(dirname "$0")"

if ! command -v cargo >/dev/null 2>&1; then
    if [ -x "$HOME/.cargo/bin/cargo" ]; then
        PATH="$HOME/.cargo/bin:$PATH"
    else
        echo "cargo isn't installed. Get it with:"
        echo "  curl -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal"
        exit 1
    fi
fi

packaging/release.sh

WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT
tar -xzf dist/framecorder-arm64.tar.gz -C "$WORK"

has_password() {
    state=$(passwd -S 2>/dev/null | awk '{print $2}')
    case "$state" in
        P) return 0 ;;
        NP|L) return 1 ;;
    esac
    if command -v steamos-passwd >/dev/null 2>&1; then
        steamos-passwd --has-password
        return $?
    fi
    return 1
}

if ! has_password; then
    echo "steamos doesn't have a password yet, and framecorder won't work without one." >&2
    echo "set one in steam (settings -> system -> developer mode, then settings -> developer -> user password) or run 'passwd', then run this again." >&2
    exit 1
fi

echo "unlocking the panels (needs your password)"
sudo -v
"$WORK/framecorder-setup"
