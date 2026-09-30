#!/bin/sh
# framecorder, installed from a flatpak. the recorder can't live in a sandbox:
# reading the compositor's framebuffers needs cap_sys_admin, and flatpak drops
# every capability. so this is a delivery truck: it hands the release to the
# headset, runs the same installer frame drop uses outside the sandbox, and
# asks for your password (with a normal prompt, not a terminal) for the one
# permission the recorder needs. open it again after an update to install it.
set -u

SHARE=/app/share/framecorder
STAGE="$HOME/.local/share/framecorder/installer"
RECORDER="$HOME/.local/bin/framecorder"
TITLE=framecorder

host() { flatpak-spawn --host "$@"; }

# dialogs are drawn by the desktop (kde has kdialog; zenity or a notification if not)
tell() { # tell <kind> <text>, kind: msgbox | sorry | error
    host kdialog --title "$TITLE" "--$1" "$2" 2>/dev/null && return
    case $1 in error|sorry) z=--error ;; *) z=--info ;; esac
    host zenity "$z" --title "$TITLE" --no-wrap --text "$2" 2>/dev/null && return
    host notify-send "$TITLE" "$2" 2>/dev/null || true
}
ask() { # ask <text>: yes or no
    if host sh -c 'command -v kdialog' >/dev/null 2>&1; then
        host kdialog --title "$TITLE" --yesno "$1" 2>/dev/null
    else
        host zenity --question --title "$TITLE" --no-wrap --text "$1" 2>/dev/null
    fi
}

install_release() {
    mkdir -p "$STAGE"
    cp -f "$SHARE/framecorder-setup" "$SHARE/payload.tar" "$STAGE/"
    chmod 755 "$STAGE/framecorder-setup"
    host "$STAGE/framecorder-setup" 2>&1
}

report=$(install_release)
status=$?
if [ $status -ne 0 ]; then
    tell error "framecorder couldn't be installed.

$(printf '%s\n' "$report" | tail -n 4)"
    exit 1
fi

if ! printf '%s' "$report" | grep -q "ready:"; then
    if ask "framecorder is installed. one step left: the recorder needs permission to read what's on the display.

that takes your password, once. go ahead?"; then
        host pkexec /usr/bin/setcap cap_sys_admin+ep "$RECORDER" 2>/dev/null \
            || host pkexec setcap cap_sys_admin+ep "$RECORDER" 2>/dev/null
        report=$(install_release)
    fi
fi

if printf '%s' "$report" | grep -q "ready:"; then
    tell msgbox "framecorder is ready.

put the headset on and open the steamvr dashboard: there's a framecorder tab with the record button.

open this again whenever there's an update."
else
    tell sorry "framecorder is installed, but the recorder can't read the display yet, so it can't record.

that permission needs your account's password, and steamos doesn't set one by default. if you've never set one: open konsole, type passwd, pick a password, then open framecorder again."
fi
