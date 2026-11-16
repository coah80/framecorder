#!/bin/sh
set -eu

file=${1:?usage: packaging/sign.sh <file> <version>}
version=${2:?usage: packaging/sign.sh <file> <version>}
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
