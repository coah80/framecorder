# build and test framecorder off the headset. the recorder's FFmpeg is our own,
# built by packaging/ffmpeg.sh --for-tests (x264 and nasm are for that build,
# the ffmpeg cli for looking at what the tests write)
# docker build -t framecorder-dev tools/dev
# docker run --rm -v "$PWD":/src -v framecorder-target:/src/target -v framecorder-cargo:/root/.cargo/registry framecorder-dev sh -c 'packaging/ffmpeg.sh --for-tests && cargo test'
FROM debian:trixie
RUN apt-get update && apt-get install -y --no-install-recommends \
        build-essential ca-certificates curl clang libclang-dev pkg-config nasm libx264-dev \
        libpipewire-0.3-dev libspa-0.2-dev libdrm-dev glslc libvulkan-dev ffmpeg \
    && rm -rf /var/lib/apt/lists/*
RUN curl -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal --component clippy
ENV PATH=/root/.cargo/bin:$PATH
WORKDIR /src
