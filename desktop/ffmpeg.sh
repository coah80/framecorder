#!/bin/sh
# builds the small ffmpeg the desktop app ships next to itself, for the grid's
# thumbnails and the live page's picture and sound. most people don't have
# ffmpeg, and a mac app couldn't use brew's anyway: apps don't get the shell's
# PATH. lgpl, and only what those need: reading mp4, mkv and the live stream,
# decoding h.264, hevc and aac, scaling and resampling, and writing a jpeg,
# raw frames or raw samples.
# same pinned source as packaging/ffmpeg.sh.
#
#   desktop/ffmpeg.sh OUT          for this machine, ffmpeg (or ffmpeg.exe) lands in OUT
#   desktop/ffmpeg.sh OUT x86_64   an intel one, built on an apple silicon mac
set -eu

mkdir -p "$1"
OUT=$(cd "$1" && pwd)
ARCH=${2:-}
cd "$(dirname "$0")/.."
eval "$(grep -E '^(VERSION|SHA256|URL)=' packaging/ffmpeg.sh)"

WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT
echo "building ffmpeg $VERSION for the desktop app ${ARCH:-}"
curl -fsSL "$URL" -o "$WORK/ffmpeg.tar.gz"
got=$( (sha256sum "$WORK/ffmpeg.tar.gz" 2>/dev/null || shasum -a 256 "$WORK/ffmpeg.tar.gz") | cut -d" " -f1)
if [ "$got" != "$SHA256" ]; then
    echo "the ffmpeg source doesn't match its pinned sha256 ($got)" >&2
    exit 1
fi
tar -xzf "$WORK/ffmpeg.tar.gz" -C "$WORK"
cd "$WORK/FFmpeg-n$VERSION"

set --
case "$(uname -s)" in
    # one exe that needs no mingw dlls next to it
    MINGW* | MSYS*) set -- --extra-ldflags=-static ;;
esac
if [ -n "$ARCH" ]; then
    set -- "$@" --enable-cross-compile --arch="$ARCH" --target-os=darwin --cc="clang -arch $ARCH" --extra-ldflags="-arch $ARCH"
fi
./configure "$@" --disable-autodetect --disable-doc --disable-debug --enable-small \
    --disable-everything --disable-ffplay --disable-ffprobe --disable-avdevice --disable-postproc \
    --enable-protocol=file,pipe,http,tcp --enable-demuxer=mov,matroska,mpegts --enable-parser=h264,hevc,aac \
    --enable-decoder=h264,hevc,aac --enable-filter=scale,fps,format,pad,null,aresample,aformat,anull \
    --enable-encoder=mjpeg,rawvideo,pcm_f32le --enable-muxer=image2,rawvideo,pcm_f32le >/dev/null ||
    { tail -n 20 ffbuild/config.log; exit 1; }
make -j"$(getconf _NPROCESSORS_ONLN 2>/dev/null || nproc)" >/dev/null
for bin in ffmpeg ffmpeg.exe; do
    if [ -f "$bin" ]; then
        strip "$bin" 2>/dev/null || true
        cp "$bin" "$OUT/"
        ls -l "$OUT/$bin"
    fi
done
