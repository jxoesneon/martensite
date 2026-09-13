# Dockerfile for Linux verification of the v0.16.0 media pipeline.
#
# Covers what macOS cannot:
#   - `decoder-vaapi` compile + the self-contained H.264 SPS/PPS/slice
#     parser unit tests (compiled only on Linux)
#   - `decoder-ffmpeg` real-decode test on the checked-in Annex-B fixture
#   - the noop/dispatch conformance suite on Linux
#
# VAAPI *runtime* decode still needs a real GPU — containers on macOS get
# no DRI device, so `VaapiDecoder::create` fails fast there; the compile
# gate and parser tests are what this image verifies.
#
# Usage:
#   docker build -t martensite-media-test -f docker/media-test.Dockerfile .
#   docker run --rm martensite-media-test
#
# With a real GPU (Linux host):
#   docker run --rm --device /dev/dri martensite-media-test

FROM rust:1.95-slim-bookworm

ENV CARGO_TERM_COLOR=always

# The slim image ships a minimal profile; add clippy/rustfmt so the
# `-D warnings` gate can run against real Linux libva/ffmpeg too.
RUN rustup component add clippy rustfmt

# libva-dev: cros-libva build (pkg-config).
# ffmpeg -dev packages: ffmpeg-next build (pkg-config).
# mesa vulkan drivers: wgpu Vulkan device creation on llvmpipe/lavapipe
# for the noop-wgpu interop tests.
RUN apt-get update && apt-get install -y --no-install-recommends \
    build-essential \
    pkg-config \
    clang \
    libva-dev \
    libva-drm2 \
    libavcodec-dev \
    libavformat-dev \
    libavutil-dev \
    libswscale-dev \
    libavfilter-dev \
    libavdevice-dev \
    libvulkan1 \
    mesa-vulkan-drivers \
    vulkan-tools \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /workspace

# Copy the workspace. `.dockerignore` already excludes target/ and .git/.
COPY . .

# Compile all media test targets at build time (verifies VAAPI links
# against real libva). Tests run at container start so failures can be
# debugged interactively: docker run -it martensite-media-test bash
RUN cargo test --no-run -p martensite-media-platform -p martensite-media-test \
    --features martensite-media-platform/decoder-vaapi,martensite-media-platform/decoder-ffmpeg,martensite-media-test/decoder-ffmpeg,martensite-media-test/decoder-vaapi,martensite-media-test/test-noop

CMD ["sh", "-c", "cargo test -p martensite-media-platform --features decoder-vaapi,decoder-ffmpeg && cargo test -p martensite-media-test --features decoder-ffmpeg,decoder-vaapi,test-noop"]
