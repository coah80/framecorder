#!/bin/sh
# Builds the little FFmpeg the recorder links in statically: the AAC encoder,
# the mp4 and mkv muxers and file output, nothing else. Video never goes through
# FFmpeg (the hardware encoder makes it), so that's all it needs. Linking our
# own instead of SteamOS's means an OS update can't take it away: SteamOS
# 0.5.7 swapped FFmpeg 7.1 for 8.0 and a recorder linked to 7.1 stopped
# starting at all.
#
#   packaging/ffmpeg.sh              into target/ffmpeg, where .cargo/config.toml points cargo
#   packaging/ffmpeg.sh --for-tests  plus x264 and the mp4 reader the session tests use
#
# It's skipped when target/ffmpeg was already built by this exact script and flavor.
set -eu

VERSION=7.1.5
SHA256=e3963a50831c985933e1a625ed566ec4c7adb5c012c34fa9f84438e1d61bdacc
URL=https://github.com/FFmpeg/FFmpeg/archive/refs/tags/n$VERSION.tar.gz

cd "$(dirname "$0")/.."
ROOT=$PWD
PREFIX=$ROOT/target/ffmpeg
FLAVOR=release
EXTRA=""
if [ "${1:-}" = --for-tests ]; then
    FLAVOR=tests
    EXTRA="--enable-gpl --enable-libx264 --enable-encoder=libx264 --enable-demuxer=mov,mpegts --enable-decoder=h264,hevc,aac"
fi
STAMP="$VERSION $FLAVOR $(cksum <packaging/ffmpeg.sh | cut -d" " -f1)"
if [ "$(cat "$PREFIX/.built" 2>/dev/null)" = "$STAMP" ]; then
    exit 0
fi

WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT
echo "building ffmpeg $VERSION ($FLAVOR)"
curl -fsSL "$URL" -o "$WORK/ffmpeg.tar.gz"
echo "$SHA256  $WORK/ffmpeg.tar.gz" | sha256sum -c --quiet
tar -xzf "$WORK/ffmpeg.tar.gz" -C "$WORK"
cd "$WORK/FFmpeg-n$VERSION"
# shellcheck disable=SC2086
./configure --prefix="$PREFIX" --enable-static --disable-shared --enable-pic \
    --disable-autodetect --disable-programs --disable-doc --disable-network \
    --disable-everything --disable-avdevice --disable-avfilter --disable-swscale --disable-swresample --disable-postproc \
    --enable-encoder=aac --enable-muxer=mp4,mov,matroska,mpegts --enable-demuxer=matroska --enable-protocol=file --enable-parser=aac,h264,hevc \
    $EXTRA >/dev/null
make -j"$(nproc)" >/dev/null
rm -rf "$PREFIX"
make install >/dev/null
echo "$STAMP" >"$PREFIX/.built"
# ffmpeg-sys-next copies these libraries into its own rlib when it builds and
# never looks at them again, so forget it was built or binaries keep the old ones
rm -rf "$ROOT"/target/*/.fingerprint/ffmpeg-sys-next-*
echo "ffmpeg $VERSION is in $PREFIX"
