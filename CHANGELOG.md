# Changelog

## 0.2.0 — 2026-08-15

- Added China, Singapore, Los Angeles, and Frankfurt ModelVerse region selection.
- Added `/v1/models` discovery and protocol-aware model selection from `ListUFSquareModel` and `GetUFSquareModelDetail`.
- Made all eight harness launchers override conflicting endpoint, credential, provider, and model configuration deterministically.
- Added a pinned Docker image that runs Claude Code, Codex CLI, Grok Build, OpenCode, Hermes Agent, Pi, DeepSeek Harness, and Prime Agent against a hostile-config routing test.
- Changed `harness test --live` to invoke the real installed harness instead of an internal probe.

## 0.1.0 — 2026-08-15

- Added UCloud OAuth login with a loopback callback and English/Chinese onboarding.
- Added default-project discovery plus guarded ModelVerse API-key listing, creation, and selection.
- Added Claude Code, Codex, Grok, OpenCode, Hermes, Pi, DeepSeek Harness, and Prime Agent launchers.
- Added machine JSON output, shell completions, workspace diagnostics, harness diagnostics, evals, and self-update checks.
- Added a loopback vault tunnel and child-process injection/live-usage verification.
