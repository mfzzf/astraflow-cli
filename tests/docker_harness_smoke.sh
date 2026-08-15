#!/usr/bin/env bash
set -euo pipefail

mock_log=$(mktemp)
export HARNESS_MOCK_LOG="$mock_log"
export HARNESS_MOCK_MODEL=astraflow-test-model
python3 /opt/astraflow-tests/harness_mock.py &
mock_pid=$!
trap 'kill "$mock_pid" 2>/dev/null || true' EXIT

for _ in $(seq 1 50); do
  curl -fsS http://127.0.0.1:18080/v1/models >/dev/null && break
  sleep 0.1
done

export ASTRAFLOW_API_KEY=offline-sentinel-key
export ASTRAFLOW_MODELVERSE_ENDPOINT=http://127.0.0.1:18080
export ASTRAFLOW_HOME=/tmp/astraflow-home
export GROK_HOME=/tmp/hostile/grok
export PI_CODING_AGENT_DIR=/tmp/hostile/pi
export PRIME_AGENT_CODING_AGENT_DIR=/tmp/hostile/prime
export DSH_HOME=/tmp/hostile/dsh
mkdir -p "$GROK_HOME" "$PI_CODING_AGENT_DIR" "$PRIME_AGENT_CODING_AGENT_DIR" "$DSH_HOME" \
  /root/.codex /root/.claude /root/.config/opencode /root/.hermes

printf 'model = "hostile-model"\nmodel_provider = "hostile"\n[model_providers.hostile]\nname = "Hostile"\nbase_url = "http://127.0.0.1:9/v1"\nenv_key = "HOSTILE_KEY"\nwire_api = "responses"\n' > /root/.codex/config.toml
printf '{"model":"hostile/model","provider":{"hostile":{"models":{"model":{}}}}}\n' > /root/.config/opencode/opencode.json
printf '{"model":"hostile-model"}\n' > /root/.claude/settings.json
printf 'model:\n  default: hostile-model\n  base_url: http://127.0.0.1:9/v1\n' > /root/.hermes/config.yaml
printf '[model.hostile]\nmodel="hostile-model"\nbase_url="http://127.0.0.1:9/v1"\nenv_key="HOSTILE_KEY"\n' > "$GROK_HOME/config.toml"
printf '{"providers":{"hostile":{"baseUrl":"http://127.0.0.1:9/v1","api":"openai-completions","apiKey":"$HOSTILE_KEY","authHeader":true,"models":[{"id":"hostile-model"}]}}}\n' > "$PI_CODING_AGENT_DIR/models.json"
cp "$PI_CODING_AGENT_DIR/models.json" "$PRIME_AGENT_CODING_AGENT_DIR/models.json"
printf 'llm-pi-ai:\n  providers:\n    hostile:\n      baseURL: http://127.0.0.1:9/v1\n      api: openai-completions\n      models: [{id: hostile-model}]\n' > "$DSH_HOME/settings.yaml"
export HOSTILE_KEY=must-not-be-used

run_case() {
  local name=$1
  shift
  local before
  before=$(wc -l < "$mock_log")
  if ! timeout 90 astf "$name" --model astraflow-test-model -- "$@" >/tmp/"$name".out 2>/tmp/"$name".err; then
    printf '%s failed\n' "$name" >&2
    sed -n '1,200p' /tmp/"$name".err >&2
    sed -n '1,80p' /tmp/"$name".out >&2
    return 1
  fi
  if ! python3 - "$mock_log" "$before" "$name" <<'PY'
import json, sys
path, before, name = sys.argv[1], int(sys.argv[2]), sys.argv[3]
records = [json.loads(line) for line in open(path, encoding="utf-8")][before:]
assert records, f"{name}: no request reached AstraFlow mock"
for record in records:
    auth = record.get("authorization") or record.get("x_api_key")
    assert auth in ("Bearer offline-sentinel-key", "offline-sentinel-key"), (name, record)
    assert "hostile" not in json.dumps(record).lower(), (name, record)
completions = [record for record in records if record["path"].split("?", 1)[0].rstrip("/") in (
    "/v1/messages", "/v1/responses", "/v1/response", "/v1/chat/completions"
)]
assert completions, f"{name}: no inference request reached AstraFlow mock"
for record in completions:
    assert record["model"] == "astraflow-test-model", (name, record)
print(f"{name}: route/model/auth override verified ({len(records)} request(s))")
PY
  then
    sed -n '1,200p' /tmp/"$name".err >&2
    sed -n '1,120p' /tmp/"$name".out >&2
    return 1
  fi
}

run_case claude --print 'Reply with exactly ASTRAFLOW_OK' --output-format json
run_case codex exec --skip-git-repo-check --sandbox read-only 'Reply with exactly ASTRAFLOW_OK'
run_case grok --single 'Reply with exactly ASTRAFLOW_OK'
run_case opencode run 'Reply with exactly ASTRAFLOW_OK' --format json
run_case hermes --oneshot 'Reply with exactly ASTRAFLOW_OK'
run_case pi --print 'Reply with exactly ASTRAFLOW_OK'
run_case dsh --profile headless 'Reply with exactly ASTRAFLOW_OK'
run_case prime-agent --print 'Reply with exactly ASTRAFLOW_OK'

for executable in claude codex grok opencode hermes pi dsh prime-agent; do
  command -v "$executable" >/dev/null
done

echo 'all eight real harnesses passed hostile-config routing verification'
