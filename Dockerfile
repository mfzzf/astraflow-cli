FROM rust:1.88-bookworm AS development

RUN rustup component add rustfmt clippy
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates git curl bash zsh \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /workspace
COPY Cargo.toml Cargo.lock ./
RUN mkdir src && printf 'fn main() {}\n' > src/main.rs && cargo fetch --locked && rm -rf src
COPY . .

FROM development AS test
RUN cargo test --all-targets --locked
RUN cargo clippy --all-targets --locked -- -D warnings

FROM test AS harness-smoke
ARG CODEX_VERSION=0.147.0
RUN apt-get update \
    && apt-get install -y --no-install-recommends nodejs npm \
    && npm install --global "@openai/codex@${CODEX_VERSION}" --silent \
    && rm -rf /var/lib/apt/lists/*
CMD ["codex", "--version"]

FROM development AS build
RUN cargo build --release --locked

FROM debian:bookworm-slim AS runtime
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*
COPY --from=build /workspace/target/release/astf /usr/local/bin/astf
ENTRYPOINT ["astf"]
CMD ["--help"]
