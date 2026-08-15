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
ARG LEGACY_PI_VERSION=0.73.1
ARG DSH_VERSION=0.1.0-rc.6
ARG HERMES_VERSION=0.19.0
ARG GROK_VERSION=1.0.4
ARG PRIME_AGENT_VERSION=0.7.2
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
RUN npm install --prefix /opt/pi-legacy \
    "@mariozechner/pi-coding-agent@${LEGACY_PI_VERSION}" \
    --silent
RUN curl -fsSL https://x.ai/cli/install.sh | env GROK_BIN_DIR=/usr/local/bin bash -s -- "${GROK_VERSION}" \
    && grok --version | grep -F "grok ${GROK_VERSION} "
RUN curl -fsSL https://app.primeintellect.ai/prime-agent/install.sh | sh -s -- "${PRIME_AGENT_VERSION}" \
    && prime-agent --version 2>&1 | grep -Fx "${PRIME_AGENT_VERSION}"
RUN curl -LsSf https://astral.sh/uv/install.sh | env UV_INSTALL_DIR=/usr/local/bin sh \
    && uv venv --python /usr/bin/python3.11 /usr/local/lib/hermes-agent/venv \
    && VIRTUAL_ENV=/usr/local/lib/hermes-agent/venv uv pip install "hermes-agent==${HERMES_VERSION}" \
    && ln -sf /usr/local/lib/hermes-agent/venv/bin/hermes /usr/local/bin/hermes \
    && hermes --version | grep -F "Hermes Agent v${HERMES_VERSION}"
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
