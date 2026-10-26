#!/bin/sh
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

echo "unlocking the panels (needs your password)"
sudo -v
"$WORK/framecorder-setup"
