# syntax=docker/dockerfile:1

FROM rust:1.89-slim

ENV CARGO_TERM_COLOR=always

RUN apt-get update && apt-get install -y --no-install-recommends \
    build-essential \
    pkg-config \
    libx11-dev \
    libxkbcommon-dev \
    libxkbcommon-x11-dev \
    libxcursor-dev \
    libxi-dev \
    libxrandr-dev \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /workspace

COPY . .

RUN --mount=type=cache,target=/usr/local/cargo/registry \
    cargo test -p martensite-test --lib --release --no-run

COPY docker/fuzz/fuzz-runner.sh /usr/local/bin/fuzz-runner.sh
RUN chmod +x /usr/local/bin/fuzz-runner.sh

ENV MARTENSITE_FUZZ_SEED=1 \
    MARTENSITE_FUZZ_DURATION=172800

ENTRYPOINT ["/usr/local/bin/fuzz-runner.sh"]
