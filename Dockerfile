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

FROM node:22-bookworm AS harness-all
ARG CLAUDE_VERSION=2.1.233
ARG CODEX_VERSION=0.147.0
ARG OPENCODE_VERSION=1.18.18
ARG PI_VERSION=0.84.2
ARG DSH_VERSION=0.1.0-rc.6
ARG HERMES_COMMIT=4b0c1031dba37cd6d3dba402ab91d20b720e48ab
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates git curl bash python3 python3-venv python3-pip \
    && rm -rf /var/lib/apt/lists/*
RUN npm install --global \
    "@anthropic-ai/claude-code@${CLAUDE_VERSION}" \
    "@openai/codex@${CODEX_VERSION}" \
    "opencode-ai@${OPENCODE_VERSION}" \
    "@earendil-works/pi-coding-agent@${PI_VERSION}" \
    "@deepseek-ai/dsh@${DSH_VERSION}" \
    --silent
RUN GROK_BIN_DIR=/usr/local/bin bash -c 'curl -fsSL https://x.ai/cli/install.sh | bash'
RUN curl -fsSL https://app.primeintellect.ai/prime-agent/install.sh | sh
RUN mkdir -p /usr/local/lib/hermes-agent \
    && curl -fsSL "https://github.com/NousResearch/hermes-agent/archive/${HERMES_COMMIT}.tar.gz" \
    | tar -xz --strip-components=1 -C /usr/local/lib/hermes-agent \
    && curl -LsSf https://astral.sh/uv/install.sh | env UV_INSTALL_DIR=/usr/local/bin sh \
    && cd /usr/local/lib/hermes-agent \
    && uv venv --python /usr/bin/python3.11 venv \
    && VIRTUAL_ENV=/usr/local/lib/hermes-agent/venv uv pip install -e . \
    && ln -sf /usr/local/lib/hermes-agent/venv/bin/hermes /usr/local/bin/hermes
COPY --from=build /workspace/target/release/astraflow /usr/local/bin/astraflow
COPY tests/harness_mock.py tests/docker_harness_smoke.sh /opt/astraflow-tests/
RUN chmod +x /opt/astraflow-tests/docker_harness_smoke.sh
CMD ["/opt/astraflow-tests/docker_harness_smoke.sh"]

FROM debian:bookworm-slim AS runtime
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*
COPY --from=build /workspace/target/release/astraflow /usr/local/bin/astraflow
ENTRYPOINT ["astraflow"]
CMD ["--help"]
