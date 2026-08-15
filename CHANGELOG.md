# Changelog

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
