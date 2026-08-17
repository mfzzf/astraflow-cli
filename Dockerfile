FROM rust:1.88-bookworm@sha256:af306cfa71d987911a781c37b59d7d67d934f49684058f96cf72079c3626bfe0 AS development

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
    && useradd --create-home --uid 10001 --shell /bin/bash astraflow \
    && rm -rf /var/lib/apt/lists/*
USER astraflow
WORKDIR /home/astraflow
CMD ["codex", "--version"]

FROM development AS build
RUN cargo build --release --locked

FROM node:26-bookworm@sha256:0353e48e0e8a993db87b720c242f54b207059d1bcc0106534896e8a11054c837 AS harness-all
ARG CLAUDE_VERSION=2.1.233
ARG CODEX_VERSION=0.147.0
ARG OPENCODE_VERSION=1.18.18
ARG PI_VERSION=0.84.2
ARG LEGACY_PI_VERSION=0.73.1
ARG DSH_VERSION=0.1.0-rc.6
ARG HERMES_VERSION=0.19.0
ARG GROK_VERSION=1.0.4
ARG PRIME_AGENT_VERSION=0.7.2
ARG UV_VERSION=0.12.5
ARG GROK_INSTALL_SHA256=43d0943123edade1383a476a4f778674877acee7c1f98a00f094c4a0f7349321
ARG PRIME_AGENT_INSTALL_SHA256=38d14a1be73b325652c7ce8342e3bf19335721837192855a7907732caf8e6d04
ARG UV_INSTALL_SHA256=504511fbbbd811aeaba6738abc79408956b6c7da0ca35437b3dcc24a41efc111
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
RUN curl -fsSL https://x.ai/cli/install.sh -o /tmp/grok-install.sh \
    && echo "${GROK_INSTALL_SHA256}  /tmp/grok-install.sh" | sha256sum --check --strict \
    && env GROK_BIN_DIR=/usr/local/bin bash /tmp/grok-install.sh "${GROK_VERSION}" \
    && rm -f /tmp/grok-install.sh \
    && grok --version | grep -F "grok ${GROK_VERSION} " \
    && install -D -m 0755 "$(readlink -f /usr/local/bin/grok)" /usr/local/libexec/astraflow/grok \
    && ln -sfn /usr/local/libexec/astraflow/grok /usr/local/bin/grok \
    && ln -sfn /usr/local/libexec/astraflow/grok /usr/local/bin/agent \
    && rm -rf /root/.grok \
    && grok --version | grep -F "grok ${GROK_VERSION} "
RUN curl -fsSL https://app.primeintellect.ai/prime-agent/install.sh -o /tmp/prime-agent-install.sh \
    && echo "${PRIME_AGENT_INSTALL_SHA256}  /tmp/prime-agent-install.sh" | sha256sum --check --strict \
    && sh /tmp/prime-agent-install.sh "${PRIME_AGENT_VERSION}" \
    && rm -f /tmp/prime-agent-install.sh \
    && prime-agent --version 2>&1 | grep -Fx "${PRIME_AGENT_VERSION}"
RUN curl -fsSL "https://astral.sh/uv/${UV_VERSION}/install.sh" -o /tmp/uv-install.sh \
    && echo "${UV_INSTALL_SHA256}  /tmp/uv-install.sh" | sha256sum --check --strict \
    && env UV_INSTALL_DIR=/usr/local/bin sh /tmp/uv-install.sh \
    && rm -f /tmp/uv-install.sh \
    && uv venv --python /usr/bin/python3.11 /usr/local/lib/hermes-agent/venv \
    && VIRTUAL_ENV=/usr/local/lib/hermes-agent/venv uv pip install "hermes-agent==${HERMES_VERSION}" \
    && ln -sf /usr/local/lib/hermes-agent/venv/bin/hermes /usr/local/bin/hermes \
    && hermes --version | grep -F "Hermes Agent v${HERMES_VERSION}"
COPY --from=build /workspace/target/release/astraflow /usr/local/bin/astraflow
COPY tests/harness_mock.py tests/docker_harness_smoke.sh /opt/astraflow-tests/
RUN chmod 0555 /opt/astraflow-tests/docker_harness_smoke.sh \
    && chmod 0444 /opt/astraflow-tests/harness_mock.py \
    && useradd --create-home --uid 10001 --shell /bin/bash astraflow
USER astraflow
WORKDIR /home/astraflow
CMD ["/opt/astraflow-tests/docker_harness_smoke.sh"]

FROM debian:bookworm-slim@sha256:abd67ffcfa541b485a3dff59865ab629aa048a6c613e639d36e7456b0b229241 AS runtime
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && useradd --create-home --uid 10001 --shell /usr/sbin/nologin astraflow \
    && rm -rf /var/lib/apt/lists/*
COPY --from=build /workspace/target/release/astraflow /usr/local/bin/astraflow
USER astraflow
WORKDIR /home/astraflow
HEALTHCHECK --interval=30s --timeout=5s --start-period=5s --retries=3 CMD ["astraflow", "--json", "version"]
ENTRYPOINT ["astraflow"]
CMD ["--help"]
