# Changelog

## 0.3.6 — 2026-08-18

- Add a sourced model context-window catalog shared by Claude Code, Codex, Pi, and Prime Agent;
  known models use their maintained maximum while unknown and custom IDs default to 1M tokens.

## 0.3.5 — 2026-08-17

- Replace remote-script self-updates with versioned raw release binaries, bounded downloads,
  SHA-256 verification, executable version validation, and atomic replacement.
- Verify pinned third-party Docker installers before execution and run all deliverable container
  targets as an unprivileged `astraflow` user with a runtime health check.
- Pin GitHub Actions to immutable commit SHAs, restrict release write permission to the publish job,
  and let Dependabot maintain Actions and Cargo dependencies.
- Generate invalid test credentials at runtime and document the narrowly scoped SHA-1 compatibility
  boundary mandated by the UCloud OpenAPI V1 signing protocol.

## 0.3.4 — 2026-08-17

- Stop Claude Code's interactive custom-API-key confirmation by authenticating the AstraFlow Anthropic gateway exclusively with `ANTHROPIC_AUTH_TOKEN`.
- Declare the conservative 128K context window used by AstraFlow's custom model catalogs so recent Claude Code versions retain proactive compaction without showing an unknown-model window warning.

## 0.3.3 — 2026-08-17

- Preserve each harness's user configuration and customization discovery while keeping AstraFlow endpoint, credentials, protocol, provider, and model slots pinned.
- Restore Claude Code user/project/local skills, plugins, hooks, agents, and settings through a high-priority ephemeral routing overlay.
- Restore Codex skills/plugins/apps, Grok Web search and user home, OpenCode project/default/external skills and plugins, Hermes skills/plugins/MCP/hooks/memory, and Pi/Prime extensions and agent resources.
- Add positive Docker smoke coverage proving user hooks/plugins/extensions execute while all eight real harnesses still send the selected model and AstraFlow credential to the managed endpoint.

## 0.3.2 — 2026-08-16

- Restored DeepSeek Harness settings, plugins, profiles, sessions, and Web onboarding persistence; the enabled settings service now uses a persistent AstraFlow-scoped document so existing DSH provider settings cannot override the final managed routing patch.
- Expanded the real DSH Web smoke test from process startup to an HTTP-served browser UI check under hostile user routing configuration.

## 0.3.1 — 2026-08-16

- Split text pricing into Input, Cache Read, Cache Create, and Output starting-price columns, with distinct 5-minute and 1-hour cache creation rates.
- Replaced the selected model's raw price sentence with a context-tier table that preserves long-context pricing boundaries.
- Added a best-effort daily release check with Update now, Remind me later, and Skip this version choices.

## 0.3.0 — 2026-08-15

- Split model pricing into aligned Input, Cache, and Output columns on normal-width terminals, with a compact responsive fallback for narrow terminals.
- Added named provider configurations with a selectable default, explicit `--config <name>` routing, and secure config CRUD commands.
- Added first-run onboarding for UCloud OAuth, direct AstraFlow API keys, and custom Base URL/API key providers using Chat Completions, Responses, or Anthropic Messages.

## 0.2.9 — 2026-08-15

- Rebuilt the interactive model picker with Ratatui and Crossterm, adding a responsive bordered layout, role tabs, a search panel, a scrollable highlighted model table, compact token prices, selected-model details, and automatic terminal restoration.
- Removed harness-specific model-family filtering: Claude Code, Codex, and every other agent now share the complete conversational text model inventory returned by authenticated `/v1/models`.
- Treat every listed conversational text model as compatible with Chat Completions, Responses, and Anthropic Messages until maintained protocol capability metadata is available.
- Prefer `deepseek-v4-flash-0731` as the default for every protocol and fall back to the shared chat default when an older saved credential has no protocol-specific selection.

## 0.2.8 — 2026-08-15

- Added an AstraFlow-managed DSH Web profile: `astraflow dsh --model <model> --profile web` now starts the browser UI while preserving the injected ModelVerse endpoint, key, model, and protected patch layer.
- Kept `headless` as the default DSH profile and continued rejecting custom profiles and user-supplied patches that could replace AstraFlow routing.
- Restored the persistent self-hosted runner's workspace ownership after containerized Linux release builds.
- Made a value-less `--model` open a searchable interactive model selector with live ModelVerse pricing; explicit IDs such as `glm-5.2` and `deepseek-v4-pro-0813` remain supported.
- Open the model picker for every interactive harness launch when `--model` is omitted, with direct search, live text-token pricing, Tab/Shift+Tab and Left/Right role switching, Up/Down selection, `D` to save defaults, and Enter to launch.
- Added verified multi-model role routing for Claude Code, Codex, Grok Build, OpenCode, DSH, plus a multi-select Ctrl+P cycle pool for Pi and Prime Agent; unsupported or media-only roles remain hidden.
- Prefer `deepseek-v4-flash-0731` for every Chat Completions harness when available, while retaining protocol-compatible Responses models for Codex and Anthropic Messages models for Claude Code.
- Reject inner Claude/OpenCode model and Claude settings/fallback overrides that could bypass AstraFlow routing.

## 0.2.7 — 2026-08-15

- Built both Linux GNU release targets inside Rust 1.88 Bookworm containers so published binaries run on glibc 2.36 systems such as Debian 12 instead of requiring the build runner's glibc 2.39.
- Executed each Linux release binary inside its Debian 12 build container before packaging it.

## 0.2.6 — 2026-08-15

- Isolated Codex, Grok, Hermes, Pi, Prime Agent, and DSH from conflicting user, project, profile, and managed configuration during wrapped launches.
- Disabled OpenCode project configuration, default plugins, external skills, and Claude compatibility imports; retained Claude's explicit empty setting sources.
- Rejected passthrough provider, model, API-key, config, profile, patch, plugin, and extension flags that could override AstraFlow routing.
- Added generated Codex catalog entries for authenticated Responses models not bundled with Codex CLI 0.147.0.
- Forced Grok main and auxiliary requests to the selected AstraFlow model and removed stale same-name credentials and authorization headers.
- Expanded the pinned real-binary hostile-config suite to verify exact protocol path, model, bearer authentication, and non-execution of hooks/plugins/extensions across every harness.

## 0.2.5 — 2026-08-15

- Prevented Claude Code user/project/local settings from overriding the AstraFlow endpoint, bearer token, or model for wrapped launches.
- Added a real Claude Code regression with hostile `settings.json` endpoint and authentication values.

## 0.2.4 — 2026-08-15

- Fixed Pi 0.73.x authentication by supporting its legacy environment-variable resolution alongside current Pi releases.
- Added real routing/authentication coverage for both Pi 0.73.1 and Pi 0.84.2.

## 0.2.3 — 2026-08-15

- Renamed both the Rust package and executable from `astraflow-cli`/`astf` to `astraflow`.
- Replaced the nonexistent crates.io update endpoint with the `mfzzf/astraflow-cli` GitHub Releases API.
- Made self-update install checksummed GitHub Release binaries through the cross-platform installers.

## 0.2.2 — 2026-08-15

- Select the newest eligible Chat Completions, Responses, and Anthropic model from the authenticated `/v1/models` `created` timestamps.
- Removed hard-coded default model preferences while retaining explicit `--model` overrides.

## 0.2.1 — 2026-08-15

- Removed all `GetUFSquareModelDetail` requests and protocol-metadata dependencies.
- Added local protocol classification from `/v1/models` IDs plus optional catalog names and aliases.
- Routed Claude model families exclusively through the Anthropic Messages API.
- Expanded filtering for image/video/audio generation, embeddings, rerank, OCR, batch, transcription, and moderation models while retaining vision-language chat models.

## 0.2.0 — 2026-08-15

- Added China, Singapore, Los Angeles, and Frankfurt ModelVerse region selection.
- Added `/v1/models` discovery and model-square catalog correlation.
- Made all eight harness launchers override conflicting endpoint, credential, provider, and model configuration deterministically.
- Added a pinned Docker image that runs Claude Code, Codex CLI, Grok Build, OpenCode, Hermes Agent, Pi, DeepSeek Harness, and Prime Agent against a hostile-config routing test.
- Changed `harness test --live` to invoke the real installed harness instead of an internal probe.

## 0.1.0 — 2026-08-15

- Added UCloud OAuth login with a loopback callback and English/Chinese onboarding.
- Added default-project discovery plus guarded ModelVerse API-key listing, creation, and selection.
- Added Claude Code, Codex, Grok, OpenCode, Hermes, Pi, DeepSeek Harness, and Prime Agent launchers.
- Added machine JSON output, shell completions, workspace diagnostics, harness diagnostics, evals, and self-update checks.
- Added a loopback vault tunnel and child-process injection/live-usage verification.
