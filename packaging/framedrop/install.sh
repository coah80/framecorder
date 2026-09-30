#!/bin/sh
# started by framecorder.exe (see launcher.c), inside the container steam runs
# proton in. the installer needs the headset itself (systemctl, setcap), so
# this hops out with flatpak-spawn and runs again out there, where it unpacks
# the installer (a tar keeps it executable, the zip can't) and runs it.
set -u
# steam's overlay, preloaded into everything proton starts, only adds noise here
unset LD_PRELOAD
DIR=$(cd "$(dirname "$0")" && pwd)
cd "$DIR"

if [ -e /run/pressure-vessel ]; then
    if command -v flatpak-spawn >/dev/null 2>&1; then
        flatpak-spawn --host --directory="$DIR" /bin/sh "$DIR/install.sh" >"$DIR/install.log" 2>&1
    else
        steam-runtime-launch-client --host --directory="$DIR" -- /bin/sh "$DIR/install.sh" >"$DIR/install.log" 2>&1
    fi
    echo $? >"$DIR/install.done"
    exit
fi

tar -xf setup.tar && ./framecorder-setup
