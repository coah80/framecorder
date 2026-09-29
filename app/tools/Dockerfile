# desktop builds of the framecorder app: linux (webkit2gtk) and windows (cargo-xwin)
# docker build -t framecorder-app-dev app/tools
FROM rust:1-bookworm
RUN apt-get update && apt-get install -y --no-install-recommends \
        libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev libxdo-dev libssl-dev \
        build-essential file clang lld llvm xvfb xauth imagemagick dbus-x11 fonts-dejavu-core \
    && rm -rf /var/lib/apt/lists/* \
    && ln -sf /usr/bin/clang /usr/local/bin/clang-cl \
    && ln -sf /usr/bin/lld /usr/local/bin/lld-link \
    && ln -sf /usr/bin/llvm-ar /usr/local/bin/llvm-lib
RUN rustup target add x86_64-pc-windows-msvc \
    && cargo install --locked cargo-xwin \
    && cargo install --locked tauri-cli --version "^2" \
    && rm -rf /usr/local/cargo/registry
ENV XWIN_ACCEPT_LICENSE=1
WORKDIR /src
