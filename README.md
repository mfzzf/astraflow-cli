# AstraFlow CLI

`astf` is a Rust CLI that signs a user into UCloud, selects an AstraFlow ModelVerse API key, and launches local coding-agent harnesses with the correct provider environment.

```text
login → default UCloud project → choose/create ModelVerse API key → ready
```

## Install

Linux and macOS:

```bash
curl -fsSL https://raw.githubusercontent.com/mfzzf/astraflow-cli/main/install.sh | sh
```

Windows PowerShell:

```powershell
irm https://raw.githubusercontent.com/mfzzf/astraflow-cli/main/install.ps1 | iex
```

The installers detect the operating system and CPU architecture, download the matching GitHub Release asset, verify it against `SHA256SUMS`, and install `astf`. Supported release targets are:

| Operating system | Architectures |
| --- | --- |
| Linux | x86_64, aarch64 |
| macOS | Intel x86_64, Apple Silicon |
| Windows | x86_64, ARM64 |

By default, Unix installs to `/usr/local/bin` when writable and otherwise to `~/.local/bin`. Windows installs to `%LOCALAPPDATA%\Programs\astf\bin` and updates the user `PATH`. To select a version or directory:

```bash
curl -fsSL https://raw.githubusercontent.com/mfzzf/astraflow-cli/main/install.sh | \
  ASTF_VERSION=0.1.0 ASTF_INSTALL_DIR="$HOME/bin" sh
```

For a source install, Rust 1.88 or newer is required:

```bash
cargo install --git https://github.com/mfzzf/astraflow-cli --locked
```

Then sign in and launch an agent:

```bash
astf login
astf codex
```

The first interactive login asks for English or Chinese. It opens UCloud OAuth in the browser, resolves the account's default project, lists enabled UMInfer keys, and asks which key to use. If no key exists, it creates one named `AstraFlow Agent`. `--lang en|zh` overrides and saves the language.

For SSH, keep the command running and use:

```bash
astf login --no-open
```

Open the printed URL on any device, then paste the final `http://localhost:…/authorization?...` URL into the prompt. A supplied key can be imported non-interactively:

```bash
printf '%s' "$ASTRAFLOW_API_KEY" | astf login --with-key
```

## Commands

The surface follows Ori's local-harness workflow:

- `login`, `auth`, `help`, `version`, `update`, `changelog`
- `claude`, `codex`, `grok`, `opencode`, `hermes`, `pi`, `dsh`/`deepseek`, `prime-agent`/`prime`
- `harness-doctor`, `harness list|inspect|test`, `workspace`, `vault-tunnel`, `eval`
- global `--json`/`--agent`, `--human`/`--tty`, `--wizard`, `--lang`, `--log-level`, and `--completions`

Arguments after a harness command pass through unchanged:

```bash
astf codex -- --model gpt-5
astf claude -- --permission-mode plan
```

Pipes default to one JSON document on stdout. Use `--human` to force human text.

## Credential resolution and security

Resolution order is:

1. `ASTRAFLOW_API_KEY`
2. `MODELVERSE_API_KEY`
3. nearest parent `.astraflow/credentials.json`
4. the OS-specific global AstraFlow configuration directory

Credential directories are mode `0700` and files are mode `0600` on Unix. Symlinked or group/world-readable credential files are rejected; `astf workspace --repair` restores permissions.

The UCloud control-plane client intentionally exposes only:

- `GetProjectList`
- `ListUMInferAPIKey`
- `CreateUMInferAPIKey`
- `GetUMInferRequestLogDetail` (read-only, signature-authenticated verification only)

There is no delete/update/resource-management operation. Public/private UCloud keys are accepted only through process environment variables during explicit `harness test --live --verify-usage` and are never stored.

`vault-tunnel` binds only to loopback, accepts only `/v1` routes, gives the child an ephemeral local token, and injects the real ModelVerse key only into upstream requests:

```bash
astf vault-tunnel --exec codex
```

## Injection verification

Local (no model request):

```bash
astf harness test codex
```

One minimal model request plus the request-log detail check:

```bash
UCLOUD_PUBLIC_KEY=… UCLOUD_PRIVATE_KEY=… \
  astf harness test codex --live --verify-usage --model <text-model>
```

The live probe asks for exactly `ASTRAFLOW_OK` with a 12-token cap. Usage verification retries at most three times because log ingestion may be slightly delayed.

## Development and releases

Build and test without installing dependencies on the host:

```bash
docker compose run --rm dev
docker build --target runtime -t astraflow-cli .
docker run --rm astraflow-cli --help

# Optional image with a pinned real Codex CLI for wrapper smoke tests
docker build --target harness-smoke -t astraflow-harness-smoke .
```

Pushes to `main` run formatting, tests, and Clippy on the repository's dedicated Linux x64 self-hosted Rust runner. Tags matching the crate version, such as `v0.1.0`, build native release archives on Linux x64/ARM64, macOS Intel/Apple Silicon, and Windows x64/ARM64 before publishing a checksummed GitHub Release. Public pull requests do not run on the self-hosted machine.

## 中文说明

`astf login` 会先选择中英文，然后通过浏览器完成 UCloud OAuth 登录，自动获取默认项目，列出可用的 ModelVerse API Key，并让用户选择。若项目中没有 Key，只会创建一个名为 `AstraFlow Agent` 的 UMInfer Key，不会执行其他资源操作。

Linux / macOS 一键安装：

```bash
curl -fsSL https://raw.githubusercontent.com/mfzzf/astraflow-cli/main/install.sh | sh
```

常用命令：

```bash
astf --lang zh login
astf auth
astf codex
astf claude
astf harness-doctor
```

开发和测试均可在 Docker 中完成：`docker compose run --rm dev`。
