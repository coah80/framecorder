#!/bin/sh
# Signs a release file, so installs can tell it's ours before they take it as
# an update: the headset's updater and the desktop app both refuse one that
# isn't (src/signed.rs). Run it where the release key is, never in CI: the
# point is that getting into github isn't enough to ship an update.
#
#   packaging/sign.sh <file> <version>
#
# Next to <file> it leaves <file>.sha256 (the checksum, the file's name and
# the version; made here unless the build made it already) and <file>.sig,
# the signature over that. Both go up with the file.
set -eu

file=$1
version=$2
key=${FRAMECORDER_SIGNING_KEY:-$HOME/.config/framecorder-release/signing.pem}
[ -r "$key" ] || { echo "the release key isn't at $key" >&2; exit 1; }

name=$(basename "$file")
sum=$(sha256sum "$file" | cut -d' ' -f1)
listing="$file.sha256"
want=$(printf '%s  %s\nversion %s' "$sum" "$name" "$version")
if [ -s "$listing" ]; then
    [ "$(cat "$listing")" = "$want" ] || { echo "$listing doesn't describe $name $version" >&2; exit 1; }
else
    printf '%s\n' "$want" >"$listing"
fi

message=$(mktemp)
trap 'rm -f "$message"' EXIT
{ printf 'framecorder release v1\n'; cat "$listing"; } >"$message"
openssl pkeyutl -sign -inkey "$key" -rawin -in "$message" | od -An -tx1 | tr -d ' \n' >"$file.sig"
echo >>"$file.sig"
[ "$(wc -c <"$file.sig")" = 129 ] || { echo "signing $name failed" >&2; exit 1; }
echo "signed $name $version"
