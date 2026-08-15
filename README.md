# AstraFlow CLI

`astraflow` is a Rust CLI that signs a user into UCloud, selects an AstraFlow ModelVerse API key and region, and launches local coding agents with an explicit AstraFlow endpoint, credential, provider, and compatible model.

```text
first run → choose OAuth / AstraFlow Key / custom provider → save named config → ready
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
  ASTRAFLOW_VERSION=0.3.1 ASTRAFLOW_INSTALL_DIR="$HOME/bin" sh
```

For a source install, Rust 1.88 or newer is required:

```bash
cargo install --git https://github.com/mfzzf/astraflow-cli --locked
```

The Rust package is named `astraflow`; the installed command is `astraflow`. In an interactive
terminal AstraFlow checks GitHub Releases at most once every 24 hours. When an update is available,
it presents **Update now**, **Remind me later**, and **Skip this version**. Skipping suppresses that
exact release but allows the next release to appear. `astraflow update` remains available for an
explicit checksummed upgrade, and `ASTRAFLOW_NO_UPDATE_CHECK=1` disables automatic checks. No
crates.io package is required.

Run `astraflow` or `astraflow login` for first-time setup, then launch an agent:

```bash
astraflow login --region singapore
astraflow codex
```

The first interactive setup asks for English or Chinese, a config name, and one of three provider methods:

1. **UCloud OAuth (Recommended)** opens UCloud OAuth, selects a region, resolves the default project, and lets the user choose or create an AstraFlow ModelVerse key.
2. **AstraFlow API Key** selects a region and securely prompts for an existing key.
3. **Custom Provider** asks for a Base URL, API key, exactly one wire protocol (Chat Completions, Responses, or Anthropic Messages), and a default model. It accepts Base URLs with or without a trailing `/v1`; if `/v1/models` is unavailable, `--default-model` or the interactive manual-model prompt can be used.

Each result is a named config. The first config becomes the default automatically; later configs become the default only when requested. Existing global `credentials.json` files are copied once into a private config named `default`, so upgrades do not lose credentials.

```bash
# Interactive onboarding
astraflow config add

# Direct AstraFlow key without putting it in shell history
printf '%s' "$ASTRAFLOW_API_KEY" | \
  astraflow config add --name work --method astraflow-key --with-key - --region singapore

# Custom OpenAI Responses-compatible provider
printf '%s' "$CUSTOM_API_KEY" | \
  astraflow config add --name responses-lab --method custom --with-key - \
    --base-url https://gateway.example.com/v1 --protocol responses --default-model gpt-5-mini
```

Manage and select configs with:

```bash
astraflow config list
astraflow config show work
astraflow config edit work
astraflow config default work
astraflow config remove responses-lab

astraflow --config work claude
astraflow codex --config responses-lab
ASTRAFLOW_CONFIG=work astraflow pi
```

`--config <name>` overrides the default config for one command and takes precedence over workspace or environment credentials. Config-specific model-role defaults are kept separately. `config list`, `config show`, `auth`, and JSON output never reveal API keys or OAuth tokens. A custom single-protocol config is accepted only by matching harnesses: Claude Code uses Anthropic Messages, Codex uses Responses, and the remaining harnesses use Chat Completions.

| `--region` value | ModelVerse endpoint |
| --- | --- |
| `china` | `https://api.modelverse.cn` |
| `singapore` | `https://api-sg.umodelverse.ai` |
| `los-angeles` | `https://api-us-ca.umodelverse.ai` |
| `frankfurt` | `https://api-ge-fra.umodelverse.ai` |

Login reads the selected region's `/v1/models` endpoint to learn which model IDs the key can use. OAuth login may correlate those IDs with names and aliases from the read-only `ListUFSquareModel` catalog, but it never reads model-detail protocol metadata. Until maintained protocol capability data is available, `astraflow` treats every returned conversational text model as selectable in every harness, including Claude Code and Codex. Dedicated image/video/audio generation, embedding, rerank, OCR, batch, transcription, and moderation models are excluded; vision-language chat models remain available.

For every harness, AstraFlow prefers `deepseek-v4-flash-0731` whenever that exact ID is available; otherwise it falls back to the newest eligible model returned by authenticated `/v1/models`. The harness still determines the wire endpoint it uses—Claude Code uses Anthropic Messages, Codex uses Responses, and the remaining adapters use their configured OpenAI-compatible endpoint—but AstraFlow does not hide models based on names or inferred protocol support. Wrapper-level `--model <ID>` always remains the explicit override.

Claude Code model roles such as Default, Fable, Opus, Sonnet, Haiku, and Agents all draw from that same shared text inventory. These are routing slots, not model-family filters.

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

- `login`, `config add|list|show|edit|remove|default`, `auth`, `help`, `version`, `update`, `changelog`
- `claude`, `codex`, `grok`, `opencode`, `hermes`, `pi`, `dsh`/`deepseek`, `prime-agent`/`prime`
- `harness-doctor`, `harness list|inspect|test`, `workspace`, `vault-tunnel`, `eval`
- global `--config`, `--json`/`--agent`, `--human`/`--tty`, `--wizard`, `--lang`, `--log-level`, and `--completions`

In an interactive terminal every harness command opens a cross-platform Ratatui model picker even when `--model` is omitted. Its bordered, responsive interface includes role tabs, direct search, and a scrollable highlighted model table. Wide terminals show four professional starting-price columns—`Input`, `Cache Read`, `Cache Create`, and `Output`—alongside the model name. The selected model is rendered below as a context-tier table with `Input`, `Cache Read`, `Create 5 min`, `Create 1 hour`, optional hourly cache storage, and `Output`, so long-context and prompt-cache rates are not collapsed into one sentence. Narrow terminals use compact starting-price tables while retaining clear price categories. Up/Down selects, Tab/Shift+Tab or Left/Right switches model roles, `D` saves the complete AstraFlow default combination, Enter launches, and Esc cancels. Pi and Prime also use Space to toggle multiple models in their Ctrl+P cycle pool. Prices come from the current Key's authenticated `/v1/models` response; image/video/audio charges are hidden from this text-agent picker.

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

1. explicit `--config <name>` or `ASTRAFLOW_CONFIG`
2. `ASTRAFLOW_API_KEY`
3. `MODELVERSE_API_KEY`
4. nearest parent `.astraflow/credentials.json`
5. the named default config in the OS-specific AstraFlow configuration directory
6. a legacy global `credentials.json` during its one-time migration

Credential and named-config directories are mode `0700` and files are mode `0600` on Unix. Symlinked or group/world-readable credential files are rejected; `astraflow workspace --repair` restores permissions for every named config.

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

Pushes to `main` run formatting, tests, Clippy, and hostile-config routing checks with pinned Claude Code 2.1.233, Codex CLI 0.147.0, Grok Build 1.0.4, OpenCode 1.18.18, Hermes Agent 0.19.0, Pi 0.84.2 and 0.73.1, DeepSeek Harness 0.1.0-rc.6, and Prime Agent 0.7.2 on the repository's dedicated Linux x64 self-hosted Rust runner. Tags matching the crate version, such as `v0.3.1`, build Linux GNU binaries inside Rust 1.88 Bookworm containers for glibc 2.36 compatibility, plus native macOS Intel/Apple Silicon and Windows x64/ARM64 archives, before publishing a checksummed GitHub Release. Public pull requests do not run on the self-hosted machine.

## 中文说明

首次直接运行 `astraflow`、`astraflow login` 或 `astraflow config add` 会进入配置向导，可选择 UCloud OAuth（推荐）、直接填写 AstraFlow API Key，或自定义 Base URL + API Key。自定义服务必须选择 Chat Completions、Responses、Anthropic Messages 三种协议之一；若服务不支持 `/v1/models`，可手动填写默认模型。OAuth 会选择中国大陆、新加坡、洛杉矶、法兰克福四个接入地域之一，自动获取默认项目并选择或创建名为 `AstraFlow Agent` 的 Key。

每次完成向导都会保存为一个具名 Config。第一个自动成为默认配置，也可用 `astraflow config default <名称>` 修改默认项；启动时用 `astraflow --config <名称> claude` 或 `astraflow claude --config <名称>` 临时切换。`config add/list/show/edit/remove/default` 提供完整增删改查，列表和 JSON 输出不会显示 API Key 或 OAuth Token。旧版全局凭据会一次性迁移为名为 `default` 的配置。

登录会从所选地域的 `/v1/models` 获取当前 Key 可用的模型，不再调用模型详情接口。在后续接入可维护的协议能力数据之前，所有对话文本模型都会同时出现在 Claude Code、Codex 和其他 agent 的模型选择器中，不再按 Claude、GPT、o-series 或 Codex 名称过滤。图片、视频、音频生成、Embedding、Rerank、OCR 和 Batch 模型仍会被排除，视觉语言对话模型保留。所有协议都优先使用 `deepseek-v4-flash-0731`，否则选择认证模型列表中最新的可用文本模型；重新执行 `astraflow login` 即可刷新。

交互式模型选择器使用 Ratatui 和 Crossterm 构建。宽终端的模型列表分别展示 Input、Cache Read、Cache Create、Output 四项起价；下方再按上下文档位用表格展示 Input、Cache Read、Create 5 分钟、Create 1 小时、可选的按小时缓存存储和 Output 完整价格，不再把不同上下文和缓存类型拼成一行。窄终端会切换为紧凑起价表格。

交互式启动时每天最多检查一次 GitHub Release。发现新版本会显示 Update now、Remind me later、Skip this version 三个选项；跳过仅针对当前版本，下一个版本仍会提示。可通过 `ASTRAFLOW_NO_UPDATE_CHECK=1` 关闭自动检查，也可以随时运行 `astraflow update` 主动更新。

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
