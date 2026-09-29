# build and test framecorder off the headset. debian trixie has ffmpeg 7.1 like steamos
# docker build -t framecorder-dev tools/dev
# docker run --rm -v "$PWD":/src -v framecorder-target:/src/target -v framecorder-cargo:/root/.cargo/registry framecorder-dev cargo test
FROM debian:trixie
RUN apt-get update && apt-get install -y --no-install-recommends \
        build-essential ca-certificates curl clang libclang-dev pkg-config \
        libavcodec-dev libavformat-dev libavutil-dev libswresample-dev libswscale-dev libavfilter-dev libavdevice-dev \
        libpipewire-0.3-dev libspa-0.2-dev libdrm-dev glslc libvulkan-dev ffmpeg \
    && rm -rf /var/lib/apt/lists/*
RUN curl -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal --component clippy
ENV PATH=/root/.cargo/bin:$PATH
WORKDIR /src
