# AstraFlow CLI

`astraflow` is a Rust CLI that signs a user into UCloud, selects an AstraFlow ModelVerse API key and region, and launches local coding agents with an explicit AstraFlow endpoint, credential, provider, and compatible model.

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

The installers detect the operating system and CPU architecture, download the matching GitHub Release asset, verify it against `SHA256SUMS`, and install `astraflow`. Supported release targets are:

| Operating system | Architectures |
| --- | --- |
| Linux | x86_64, aarch64 |
| macOS | Intel x86_64, Apple Silicon |
| Windows | x86_64, ARM64 |

By default, Unix installs to `/usr/local/bin` when writable and otherwise to `~/.local/bin`. Windows installs to `%LOCALAPPDATA%\Programs\astraflow\bin` and updates the user `PATH`. To select a version or directory:

```bash
curl -fsSL https://raw.githubusercontent.com/mfzzf/astraflow-cli/main/install.sh | \
  ASTRAFLOW_VERSION=0.2.8 ASTRAFLOW_INSTALL_DIR="$HOME/bin" sh
```

For a source install, Rust 1.88 or newer is required:

```bash
cargo install --git https://github.com/mfzzf/astraflow-cli --locked
```

The Rust package is named `astraflow`; the installed command is `astraflow`. `astraflow update`
checks `mfzzf/astraflow-cli` GitHub Releases and installs the checksummed release binary,
so it does not require a crates.io package.

Then choose a region, sign in, and launch an agent:

```bash
astraflow login --region singapore
astraflow codex
```

The first interactive login asks for English or Chinese and one of four ModelVerse access regions. It opens UCloud OAuth in the browser, resolves the account's default project, lists enabled UMInfer keys, and asks which key to use. If no key exists, it creates one named `AstraFlow Agent`. `--lang en|zh` overrides and saves the language.

| `--region` value | ModelVerse endpoint |
| --- | --- |
| `china` | `https://api.modelverse.cn` |
| `singapore` | `https://api-sg.umodelverse.ai` |
| `los-angeles` | `https://api-us-ca.umodelverse.ai` |
| `frankfurt` | `https://api-ge-fra.umodelverse.ai` |

Login reads the selected region's `/v1/models` endpoint to learn which model IDs the key can use. OAuth login may correlate those IDs with names and aliases from the read-only `ListUFSquareModel` catalog, but it never reads model-detail protocol metadata. `astraflow` classifies protocols locally: Claude IDs use the Anthropic Messages API, OpenAI GPT/o-series/Codex IDs can use Responses, and other conversational text models use Chat Completions. Dedicated image/video/audio generation, embedding, rerank, OCR, batch, transcription, and moderation models are excluded; vision-language chat models remain available.

For Chat Completions harnesses, AstraFlow prefers `deepseek-v4-flash-0731` whenever that exact ID is available; otherwise it falls back to the newest eligible model returned by authenticated `/v1/models`. Codex still selects a Responses-capable GPT/Codex model, and Claude Code still selects a Claude-series Anthropic Messages model because those wire protocols are not interchangeable. Wrapper-level `--model <ID>` always remains the explicit override.

ModelVerse documents its Claude-compatible endpoint as `POST /v1/messages` and limits that endpoint to Claude-series models, matching the [Anthropic Messages API](https://platform.claude.com/docs/en/api/messages/create). Every `claude-*` model family—including current Sonnet, Opus, Haiku, and Fable IDs—is therefore selected only for the Claude launcher and never assigned to an OpenAI-compatible harness.

See the [ModelVerse quick start](https://astraflow.ucloud.cn/docs/modelverse/quick-start) and an [example model playground/detail page](https://astraflow.ucloud.cn/modelverse/playground/umodel-1781663242).

For SSH, keep the command running and use:

```bash
astraflow login --no-open
```

Open the printed URL on any device, then paste the final `http://localhost:…/authorization?...` URL into the prompt. A supplied key can be imported non-interactively:

```bash
printf '%s' "$ASTRAFLOW_API_KEY" | astraflow login --with-key
```

## Commands

The surface follows Ori's local-harness workflow:

- `login`, `auth`, `help`, `version`, `update`, `changelog`
- `claude`, `codex`, `grok`, `opencode`, `hermes`, `pi`, `dsh`/`deepseek`, `prime-agent`/`prime`
- `harness-doctor`, `harness list|inspect|test`, `workspace`, `vault-tunnel`, `eval`
- global `--json`/`--agent`, `--human`/`--tty`, `--wizard`, `--lang`, `--log-level`, and `--completions`

In an interactive terminal every harness command opens the live model picker even when `--model` is omitted. It supports direct typing to search, Up/Down to select, Tab/Shift+Tab or Left/Right to switch model roles, `D` to save the complete AstraFlow default combination, Enter to launch, and Esc to cancel. Pi and Prime also use Space to toggle multiple models in their Ctrl+P cycle pool. Prices come from the current Key's authenticated `/v1/models` response; image/video/audio charges are hidden from this text-agent picker.

Use `--model <MODEL>` to bypass the picker and specify the primary model directly. A value-less `--model` explicitly requests the picker and therefore fails in a non-interactive script instead of guessing. Ordinary arguments after `--` pass through to the harness:

```bash
astraflow codex --model gpt-5-mini -- --full-auto
astraflow grok --model glm-5.2
astraflow opencode --model deepseek-v4-pro-0813
astraflow pi --model
astraflow claude -- --permission-mode plan
```

The role tabs match each harness's real configuration surface:

| Harness | Model tabs |
| --- | --- |
| Claude Code | Default, Fable, Opus, Sonnet, Haiku, Agents |
| Codex | Default, Review, Agents |
| Grok Build | Default, Summary, Prompts, General Agent, Explore Agent, Plan Agent |
| OpenCode | Default, Small, Build, Plan, General, Explore, Compaction, Title, Summary |
| Pi / Prime Agent | Default, Cycle Pool (multi-select) |
| DSH | Default, Title, Compaction, Spawn Agent, Fork Agent |
| Hermes | Default |

Unsupported roles are deliberately absent: Pi and Prime compaction reuse the current model, Grok web search needs a separate hosted-search capability, and image/video roles are not exposed. Saved choices live only in AstraFlow's private settings file; launches continue to generate isolated temporary harness configuration.

Provider, model, API-key, config, profile, patch, plugin, and extension flags that could replace AstraFlow routing are rejected after `--`; select the model with the outer `astraflow <harness> --model ...` option instead.

DSH supports AstraFlow-managed `headless` and `web` profiles. Headless requires a task; Web starts the browser UI while keeping AstraFlow's endpoint, key, and model patch:

```bash
astraflow dsh --model deepseek-v4-flash-0731 -- "run the tests"
astraflow dsh --model deepseek-v4-flash-0731 --profile web
```

Wrapped launches isolate every harness from conflicting local state. Codex and Grok receive temporary homes; Claude ignores user/project/local settings; OpenCode disables project configuration, default plugins, external skills, and Claude compatibility imports; Hermes uses a temporary safe-mode profile and empty managed scope; Pi and Prime use temporary agent configuration while retaining their normal session directories; DSH uses an isolated home and managed profile patch. API keys are passed through process environment only and are never written into those temporary harness files.

Organization-managed Claude, Codex, or Grok policy remains authoritative by design. If an administrator requires a conflicting provider, the wrapped launch fails instead of bypassing that policy.

The adapters track the official configuration contracts for [Claude Code](https://code.claude.com/docs/en/llm-gateway), [Codex CLI](https://developers.openai.com/codex/config-reference/), [Grok Build](https://github.com/xai-org/grok-build), [OpenCode](https://opencode.ai/docs/providers/), [Hermes Agent](https://github.com/NousResearch/hermes-agent), [Pi](https://github.com/earendil-works/pi), [DeepSeek Harness](https://github.com/deepseek-ai/DeepSeek-Harness), and [Prime Agent](https://github.com/PrimeIntellect-ai/prime-agent).

Pipes default to one JSON document on stdout. Use `--human` to force human text.

## Credential resolution and security

Resolution order is:

1. `ASTRAFLOW_API_KEY`
2. `MODELVERSE_API_KEY`
3. nearest parent `.astraflow/credentials.json`
4. the OS-specific global AstraFlow configuration directory

Credential directories are mode `0700` and files are mode `0600` on Unix. Symlinked or group/world-readable credential files are rejected; `astraflow workspace --repair` restores permissions.

The UCloud control-plane client intentionally exposes only:

- `GetProjectList`
- `ListUMInferAPIKey`
- `CreateUMInferAPIKey`
- `ListUFSquareModel` (read-only model catalog)
- `GetUMInferRequestLogDetail` (read-only, signature-authenticated verification only)

There is no delete/update/resource-management operation. Public/private UCloud keys are accepted only through process environment variables during explicit `harness test --live --verify-usage` and are never stored.

`vault-tunnel` binds only to loopback, accepts only `/v1` routes, gives the child an ephemeral local token, and injects the real ModelVerse key only into upstream requests:

```bash
astraflow vault-tunnel --exec codex
```

## Injection verification

Local executable/configuration check (no model request):

```bash
astraflow harness test codex
```

Run the real harness with one minimal prompt, then optionally perform the request-log detail check:

```bash
UCLOUD_PUBLIC_KEY=… UCLOUD_PRIVATE_KEY=… \
  astraflow harness test codex --live --verify-usage --model <text-model>
```

The live prompt asks for exactly `ASTRAFLOW_OK`. Usage verification uses one separate 12-token Chat Completions probe to obtain a request ID, then retries the read-only log lookup at most three times because ingestion may be slightly delayed.

## Development and releases

Build and test without installing dependencies on the host:

```bash
docker compose run --rm dev
docker build --target runtime -t astraflow-cli .
docker run --rm astraflow-cli --help

# Pinned image containing all eight real CLIs and hostile user configs
docker build --target harness-all -t astraflow-harness-all .
docker run --rm astraflow-harness-all
```

Pushes to `main` run formatting, tests, Clippy, and hostile-config routing checks with pinned Claude Code 2.1.233, Codex CLI 0.147.0, Grok Build 1.0.4, OpenCode 1.18.18, Hermes Agent 0.19.0, Pi 0.84.2 and 0.73.1, DeepSeek Harness 0.1.0-rc.6, and Prime Agent 0.7.2 on the repository's dedicated Linux x64 self-hosted Rust runner. Tags matching the crate version, such as `v0.2.8`, build Linux GNU binaries inside Rust 1.88 Bookworm containers for glibc 2.36 compatibility, plus native macOS Intel/Apple Silicon and Windows x64/ARM64 archives, before publishing a checksummed GitHub Release. Public pull requests do not run on the self-hosted machine.

## 中文说明

`astraflow login` 会先选择中英文和中国大陆、新加坡、洛杉矶、法兰克福四个接入地域之一，然后通过浏览器完成 UCloud OAuth 登录，自动获取默认项目，列出可用的 ModelVerse API Key，并让用户选择。若项目中没有 Key，只会创建一个名为 `AstraFlow Agent` 的 UMInfer Key，不会执行其他资源操作。

登录会从所选地域的 `/v1/models` 获取当前 Key 可用的模型，并在本地按模型 ID、模型广场名称和别名判断协议，不再调用模型详情接口。Claude 系列固定使用 Anthropic Messages API；GPT、o-series 和 Codex 系列可供 Responses 使用；其余对话文本模型使用 Chat Completions。图片、视频、音频生成、Embedding、Rerank、OCR 和 Batch 模型会被排除。每种协议默认选择认证模型列表中 `created` 时间戳最新的可用模型；重新执行 `astraflow login` 即可刷新。

启动 harness 时，`astraflow` 会隔离用户、项目和本地配置，并显式固定 endpoint、key、provider 和 model。`--` 后仍可传递普通参数，但会拒绝能够覆盖路由的内部 model/provider/config/profile/patch/plugin/extension 参数；请使用外层 `astraflow <harness> --model ...` 选择模型。DSH 例外允许由 AstraFlow 管理的 `--profile web` 和 `--profile headless`，但仍禁止自定义 profile 与 patch 覆盖。

Linux / macOS 一键安装：

```bash
curl -fsSL https://raw.githubusercontent.com/mfzzf/astraflow-cli/main/install.sh | sh
```

常用命令：

```bash
astraflow --lang zh login --region china
astraflow auth
astraflow codex
astraflow claude
astraflow pi  # 自动弹出模型/价格选择器，Tab 切换槽位，D 保存默认组合
astraflow grok --model glm-5.2
astraflow opencode --model deepseek-v4-pro-0813
astraflow dsh --model deepseek-v4-pro-0813 --profile web
astraflow harness-doctor
```

开发和测试均可在 Docker 中完成：`docker compose run --rm dev`。
