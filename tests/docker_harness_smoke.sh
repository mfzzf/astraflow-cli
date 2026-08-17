#!/usr/bin/env bash
set -euo pipefail

mock_log=$(mktemp)
export HARNESS_MOCK_LOG="$mock_log"
export HARNESS_MOCK_MODEL=astraflow-test-model
python3 /opt/astraflow-tests/harness_mock.py &
mock_pid=$!
trap 'kill "$mock_pid" 2>/dev/null || true' EXIT

for _ in $(seq 1 50); do
  curl -fsS http://127.0.0.1:18080/v1/models >/dev/null 2>&1 && break
  sleep 0.1
done

export ASTRAFLOW_API_KEY=offline-sentinel-key
export ASTRAFLOW_MODELVERSE_ENDPOINT=http://127.0.0.1:18080
export ASTRAFLOW_HOME=/tmp/astraflow-home
export GROK_HOME=/tmp/hostile/grok
export PI_CODING_AGENT_DIR=/tmp/hostile/pi
export PRIME_AGENT_CODING_AGENT_DIR=/tmp/hostile/prime
export PRIME_AGENT_SESSION_DIR=/tmp/hostile/prime-current-sessions
export PRIME_AGENT_CODING_AGENT_SESSION_DIR=/tmp/hostile/prime-legacy-sessions
export DSH_HOME=/tmp/hostile/dsh
export GROK_MODELS_BASE_URL=http://127.0.0.1:9/v1
export GROK_MODELS_LIST_URL=http://127.0.0.1:9/v1/models
export GROK_CLI_CHAT_PROXY_BASE_URL=http://127.0.0.1:9/v1
export GROK_DEFAULT_MODEL=hostile-model
export GROK_WEB_SEARCH_MODEL=hostile-model
export OPENCODE_CONFIG=/tmp/hostile/opencode.json
export OPENCODE_CONFIG_DIR=/tmp/hostile/opencode-dir
export OPENCODE_DISABLE_PROJECT_CONFIG=0
export OPENCODE_PURE=0
export CLAUDE_CODE_USE_BEDROCK=1
export CLAUDE_CODE_USE_VERTEX=1
export CLAUDE_CODE_USE_FOUNDRY=1
export CLAUDE_CODE_USE_ANTHROPIC_AWS=1
export CLAUDE_CODE_USE_MANTLE=1
export CLAUDE_CODE_PROVIDER_MANAGED_BY_HOST=1
export HERMES_MANAGED_DIR=/tmp/hostile/hermes-managed
export HERMES_ENABLE_PROJECT_PLUGINS=1
mkdir -p "$GROK_HOME" "$PI_CODING_AGENT_DIR" "$PRIME_AGENT_CODING_AGENT_DIR" "$DSH_HOME" \
  "$PI_CODING_AGENT_DIR/extensions" "$PRIME_AGENT_CODING_AGENT_DIR/extensions" \
  "$PI_CODING_AGENT_DIR/skills/astraflow-smoke" \
  "$PRIME_AGENT_CODING_AGENT_DIR/skills/astraflow-smoke" \
  "$GROK_HOME/skills/astraflow-grok-user" \
  "$OPENCODE_CONFIG_DIR/skills/astraflow-user-skill" \
  /root/.codex/skills/astraflow-smoke /root/.claude/skills/astraflow-smoke \
  /root/.config/opencode /root/.hermes/profiles/hostile /root/.hermes/skills/astraflow-smoke \
  "$OPENCODE_CONFIG_DIR" "$HERMES_MANAGED_DIR" \
  /tmp/astraflow-hostile-project/.claude /tmp/astraflow-hostile-project/.codex \
  /tmp/astraflow-hostile-project/.opencode/plugins \
  /tmp/astraflow-hostile-project/.opencode/skills/astraflow-project-skill \
  /tmp/astraflow-hostile-project/.grok/skills/astraflow-grok-project \
  /tmp/astraflow-hostile-project/.pi/extensions /tmp/astraflow-hostile-project/.prime/agent/extensions

printf 'model = "hostile-model"\nmodel_provider = "modelverse"\n[model_providers.modelverse]\nname = "Hostile"\nbase_url = "http://127.0.0.1:9/v1"\nenv_key = "HOSTILE_KEY"\nwire_api = "responses"\nhttp_headers = { "x-hostile" = "present" }\nquery_params = { hostile = "yes" }\n' > /root/.codex/config.toml
printf 'model = "hostile-project-model"\nmodel_provider = "modelverse"\n' > /tmp/astraflow-hostile-project/.codex/config.toml
printf '%s\n' '---' 'name: astraflow-smoke' 'description: Benign user skill used by the AstraFlow smoke test.' '---' '' 'Preserve this user skill.' > /root/.codex/skills/astraflow-smoke/SKILL.md
cp /root/.codex/skills/astraflow-smoke/SKILL.md /root/.claude/skills/astraflow-smoke/SKILL.md
cp /root/.codex/skills/astraflow-smoke/SKILL.md /root/.hermes/skills/astraflow-smoke/SKILL.md
cp /root/.codex/skills/astraflow-smoke/SKILL.md "$PI_CODING_AGENT_DIR/skills/astraflow-smoke/SKILL.md"
cp /root/.codex/skills/astraflow-smoke/SKILL.md "$PRIME_AGENT_CODING_AGENT_DIR/skills/astraflow-smoke/SKILL.md"
printf '%s\n' '---' 'name: astraflow-user-skill' 'description: AstraFlow OpenCode user skill smoke.' '---' '' 'user skill' > "$OPENCODE_CONFIG_DIR/skills/astraflow-user-skill/SKILL.md"
printf '%s\n' '---' 'name: astraflow-project-skill' 'description: AstraFlow OpenCode project skill smoke.' '---' '' 'project skill' > /tmp/astraflow-hostile-project/.opencode/skills/astraflow-project-skill/SKILL.md
printf '%s\n' '---' 'name: astraflow-grok-user' 'description: AstraFlow Grok user skill smoke.' '---' '' 'user skill' > "$GROK_HOME/skills/astraflow-grok-user/SKILL.md"
printf '%s\n' '---' 'name: astraflow-grok-project' 'description: AstraFlow Grok project skill smoke.' '---' '' 'project skill' > /tmp/astraflow-hostile-project/.grok/skills/astraflow-grok-project/SKILL.md
printf '{"model":"hostile/model","provider":{"hostile":{"models":{"model":{}}}}}\n' > /root/.config/opencode/opencode.json
printf '{"model":"hostile/model","provider":{"astraflow":{"options":{"baseURL":"http://127.0.0.1:9/v1","apiKey":"hostile"}}}}\n' > "$OPENCODE_CONFIG"
printf '{"model":"hostile/model","provider":{"astraflow":{"options":{"baseURL":"http://127.0.0.1:9/v1","apiKey":"hostile"}}}}\n' > "$OPENCODE_CONFIG_DIR/opencode.json"
printf '{"model":"hostile/model","provider":{"astraflow":{"options":{"baseURL":"http://127.0.0.1:9/v1","apiKey":"hostile"}}}}\n' > /tmp/astraflow-hostile-project/opencode.json
printf 'import { writeFileSync } from "node:fs"; writeFileSync("/tmp/opencode-plugin-ran", "yes"); export const AstraFlowSmoke = async () => ({});\n' > /tmp/astraflow-hostile-project/.opencode/plugins/astraflow-smoke.ts
printf '{"model":"hostile-model","env":{"ANTHROPIC_AUTH_TOKEN":"hostile-token","ANTHROPIC_BASE_URL":"http://127.0.0.1:9"}}\n' > /root/.claude/settings.json
printf '{"model":"hostile-model","env":{"ANTHROPIC_AUTH_TOKEN":"hostile-token","ANTHROPIC_BASE_URL":"http://127.0.0.1:9"},"hooks":{"SessionStart":[{"hooks":[{"type":"command","command":"touch /tmp/claude-hook-ran"}]}]}}\n' > /tmp/astraflow-hostile-project/.claude/settings.json
cp /tmp/astraflow-hostile-project/.claude/settings.json /tmp/astraflow-hostile-project/.claude/settings.local.json
printf 'model:\n  default: hostile-model\n  base_url: http://127.0.0.1:9/v1\n' > /root/.hermes/config.yaml
printf 'hostile\n' > /root/.hermes/active_profile
printf 'model:\n  default: hostile-model\n  provider: hostile\n' > /root/.hermes/profiles/hostile/config.yaml
printf 'providers:\n  astraflow:\n    base_url: http://127.0.0.1:9/v1\n    key_env: HOSTILE_KEY\n' > "$HERMES_MANAGED_DIR/config.yaml"
printf 'theme="user-theme"\n[model.user]\nmodel="user-model"\nbase_url="http://127.0.0.1:9/v1"\nenv_key="HOSTILE_KEY"\napi_backend="chat_completions"\n[model.astraflow]\nmodel="hostile-model"\nbase_url="http://127.0.0.1:9/v1"\nenv_key="HOSTILE_KEY"\napi_key="hostile-token"\napi_backend="chat_completions"\n[model.astraflow.extra_headers]\nAuthorization="Bearer hostile-header-token"\n' > "$GROK_HOME/config.toml"
printf '{"providers":{"hostile":{"baseUrl":"http://127.0.0.1:9/v1","api":"openai-completions","apiKey":"$HOSTILE_KEY","authHeader":true,"models":[{"id":"hostile-model"}]}}}\n' > "$PI_CODING_AGENT_DIR/models.json"
cp "$PI_CODING_AGENT_DIR/models.json" "$PRIME_AGENT_CODING_AGENT_DIR/models.json"
printf '{"astraflow":{"type":"api_key","key":"hostile-token"}}\n' > "$PI_CODING_AGENT_DIR/auth.json"
cp "$PI_CODING_AGENT_DIR/auth.json" "$PRIME_AGENT_CODING_AGENT_DIR/auth.json"
printf 'import { writeFileSync } from "node:fs"; writeFileSync("/tmp/pi-extension-ran", "yes"); export default function () {}\n' > "$PI_CODING_AGENT_DIR/extensions/astraflow-smoke.ts"
printf 'import { writeFileSync } from "node:fs"; writeFileSync("/tmp/prime-extension-ran", "yes"); export default function () {}\n' > "$PRIME_AGENT_CODING_AGENT_DIR/extensions/astraflow-smoke.ts"
printf 'agent-default-model:\n  provider: astraflow\n  model: hostile-model\nllm-pi-ai:\n  providers:\n    astraflow:\n      apiKeyEnv: HOSTILE_KEY\n      baseURL: http://127.0.0.1:9/v1\n      api: openai-completions\n      models: [{id: hostile-model}]\n' > "$DSH_HOME/settings.yaml"
export HOSTILE_KEY=must-not-be-used
cd /tmp/astraflow-hostile-project

wrong_status=$(curl -sS -o /tmp/wrong-key.json -w '%{http_code}' \
  -H 'Authorization: Bearer deliberately-wrong-key' \
  -H 'Content-Type: application/json' \
  -d '{"model":"astraflow-test-model","messages":[{"role":"user","content":"test"}]}' \
  http://127.0.0.1:18080/v1/chat/completions)
if [ "$wrong_status" != 401 ]; then
  printf 'wrong-key preflight returned HTTP %s instead of 401\n' "$wrong_status" >&2
  exit 1
fi
grep -Fq 'Validate Certification failed' /tmp/wrong-key.json
echo 'wrong-key preflight: HTTP 401 verified'

run_case() {
  local name=$1
  shift
  local before
  local binary_args=()
  local customization_marker=
  case "$name" in
    claude) customization_marker=/tmp/claude-hook-ran ;;
    opencode) customization_marker=/tmp/opencode-plugin-ran ;;
    pi) customization_marker=/tmp/pi-extension-ran ;;
    prime-agent) customization_marker=/tmp/prime-extension-ran ;;
  esac
  if [ -n "$customization_marker" ]; then
    rm -f "$customization_marker"
  fi
  if [ -n "${HARNESS_BINARY:-}" ]; then
    binary_args=(--binary "$HARNESS_BINARY")
  fi
  before=$(wc -l < "$mock_log")
  if ! timeout 90 astraflow "$name" "${binary_args[@]}" --model astraflow-test-model -- "$@" >/tmp/"$name".out 2>/tmp/"$name".err; then
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
expected_path = {
    "claude": "/v1/messages",
    "codex": "/v1/responses",
}.get(name, "/v1/chat/completions")
for record in completions:
    assert record["path"].split("?", 1)[0].rstrip("/") == expected_path, (name, record)
    assert record.get("authorization") == "Bearer offline-sentinel-key", (name, record)
print(f"{name}: route/model/auth override verified ({len(records)} request(s))")
PY
  then
    sed -n '1,200p' /tmp/"$name".err >&2
    sed -n '1,120p' /tmp/"$name".out >&2
    return 1
  fi
  if ! grep -Fq ASTRAFLOW_OK /tmp/"$name".out /tmp/"$name".err; then
    printf '%s returned no ASTRAFLOW_OK marker\n' "$name" >&2
    return 1
  fi
  if [ -n "$customization_marker" ] && [ ! -e "$customization_marker" ]; then
    printf '%s did not load the user customization marker: %s\n' "$name" "$customization_marker" >&2
    return 1
  fi
  if [ -n "$customization_marker" ]; then
    echo "$name: user customization loaded"
  fi
}

run_case claude --print 'Reply with exactly ASTRAFLOW_OK' --output-format json
run_case codex exec --skip-git-repo-check --sandbox read-only 'Reply with exactly ASTRAFLOW_OK'
run_case grok --single 'Reply with exactly ASTRAFLOW_OK'
run_case opencode run 'Reply with exactly ASTRAFLOW_OK' --format json
run_case hermes --oneshot 'Reply with exactly ASTRAFLOW_OK'
run_case pi --print 'Reply with exactly ASTRAFLOW_OK'
HARNESS_BINARY=/opt/pi-legacy/node_modules/.bin/pi run_case pi --print 'Reply with exactly ASTRAFLOW_OK'
unset HARNESS_BINARY
run_case dsh 'Reply with exactly ASTRAFLOW_OK'
run_case prime-agent --print 'Reply with exactly ASTRAFLOW_OK'

(
  printf '%s\n' \
    '{"method":"initialize","id":0,"params":{"clientInfo":{"name":"astraflow-smoke","title":"AstraFlow Smoke","version":"1"}}}' \
    '{"method":"initialized","params":{}}' \
    '{"method":"skills/list","id":25,"params":{"cwds":["/tmp/astraflow-hostile-project"],"forceReload":true}}'
  sleep 2
) | timeout 20 astraflow codex --model astraflow-test-model -- app-server \
  >/tmp/codex-skills.jsonl 2>/tmp/codex-skills.err
python3 - /tmp/codex-skills.jsonl <<'PY'
import json, sys
messages = [json.loads(line) for line in open(sys.argv[1], encoding="utf-8") if line.strip()]
response = next(message for message in messages if message.get("id") == 25)
assert "error" not in response, response
def walk(value):
    if isinstance(value, dict):
        if isinstance(value.get("name"), str):
            yield value["name"], value.get("path") or value.get("location")
        for child in value.values():
            yield from walk(child)
    elif isinstance(value, list):
        for child in value:
            yield from walk(child)
found = list(walk(response["result"]))
assert any(name == "astraflow-smoke" and "/root/.codex/skills/astraflow-smoke/" in str(path) for name, path in found), found
print("codex: user skill visible via app-server skills/list")
PY

timeout 20 astraflow opencode --model astraflow-test-model -- debug skill \
  >/tmp/opencode-skills.json 2>/tmp/opencode-skills.err
python3 - /tmp/opencode-skills.json <<'PY'
import json, sys
skills = json.load(open(sys.argv[1], encoding="utf-8"))
by_name = {skill["name"]: skill for skill in skills}
assert {"astraflow-user-skill", "astraflow-project-skill"} <= by_name.keys(), by_name.keys()
assert str(by_name["astraflow-user-skill"].get("location", "")).startswith("/tmp/hostile/opencode-dir/skills/"), by_name
assert str(by_name["astraflow-project-skill"].get("location", "")).startswith("/tmp/astraflow-hostile-project/.opencode/skills/"), by_name
print("opencode: user and project skills discovered")
PY

timeout 20 astraflow grok --model astraflow-test-model -- inspect --json \
  >/tmp/grok-inspect.json 2>/tmp/grok-inspect.err
python3 - /tmp/grok-inspect.json "$GROK_HOME/config.toml" <<'PY'
import json, sys
result = json.load(open(sys.argv[1], encoding="utf-8"))
by_name = {skill["name"]: skill for skill in result["skills"]}
assert {"astraflow-grok-user", "astraflow-grok-project"} <= by_name.keys(), by_name.keys()
def source_type(skill):
    source = skill.get("source")
    return source.get("type") if isinstance(source, dict) else source
assert source_type(by_name["astraflow-grok-user"]) == "user", by_name["astraflow-grok-user"]
assert source_type(by_name["astraflow-grok-project"]) == "project", by_name["astraflow-grok-project"]
assert sys.argv[2] in json.dumps(result["configSources"]), result["configSources"]
print("grok: user and project skills plus GROK_HOME config visible")
PY

printf '%s\n' '{"type":"get_commands"}' | timeout 20 \
  astraflow pi --model astraflow-test-model -- --mode rpc --no-session \
  >/tmp/pi-commands.jsonl 2>/tmp/pi-commands.err
printf '%s\n' '{"type":"get_commands"}' | timeout 20 \
  astraflow prime-agent --model astraflow-test-model -- --mode rpc --no-session \
  >/tmp/prime-commands.jsonl 2>/tmp/prime-commands.err
python3 - /tmp/pi-commands.jsonl /tmp/prime-commands.jsonl <<'PY'
import json, sys
for path in sys.argv[1:]:
    messages = [json.loads(line) for line in open(path, encoding="utf-8") if line.strip()]
    response = next(message for message in messages if message.get("type") == "response" and message.get("command") == "get_commands")
    assert response.get("success") is True, response
    commands = {command["name"]: command for command in response["data"]["commands"]}
    assert "skill:astraflow-smoke" in commands, commands.keys()
    assert commands["skill:astraflow-smoke"].get("source") == "skill", commands["skill:astraflow-smoke"]
    print(path + ": user skill visible via RPC")
PY

grep -Fq 'user-theme' "$GROK_HOME/config.toml"
grep -Fq '[model.user]' "$GROK_HOME/config.toml"
python3 - "$PI_CODING_AGENT_DIR/models.json" "$PRIME_AGENT_CODING_AGENT_DIR/models.json" <<'PY'
import json, sys
for path in sys.argv[1:]:
    config = json.load(open(path, encoding="utf-8"))
    assert "hostile" in config["providers"], (path, config)
    assert "astraflow-managed" in config["providers"], (path, config)
PY
echo 'Grok, Pi, and Prime user config entries were preserved alongside AstraFlow routing'

astraflow dsh --model astraflow-test-model --profile web \
  >/tmp/dsh-web.out 2>/tmp/dsh-web.err &
dsh_web_pid=$!
dsh_web_url=
for _ in $(seq 1 100); do
  dsh_web_url=$(sed -n 's/^dsh web: \(http:\/\/127\.0\.0\.1:[0-9][0-9]*\).*$/\1/p' /tmp/dsh-web.out | tail -n 1)
  if [ -n "$dsh_web_url" ] && curl -fsS "$dsh_web_url" >/tmp/dsh-web.html; then
    break
  fi
  if ! kill -0 "$dsh_web_pid" 2>/dev/null; then
    printf 'dsh web exited before serving the UI\n' >&2
    sed -n '1,120p' /tmp/dsh-web.out >&2
    sed -n '1,120p' /tmp/dsh-web.err >&2
    exit 1
  fi
  sleep 0.1
done
if [ -z "$dsh_web_url" ] || [ ! -s /tmp/dsh-web.html ]; then
  printf 'dsh web did not serve its browser UI in time\n' >&2
  sed -n '1,120p' /tmp/dsh-web.out >&2
  sed -n '1,120p' /tmp/dsh-web.err >&2
  exit 1
fi
curl -fsS \
  -H 'Content-Type: application/json' \
  -d '{"type":"client-request","rpcId":"00000000-0000-4000-8000-000000000001","method":"settings.mutate","payload":{"ns":"ui-onboarding","ops":[{"op":"set","path":["welcomeNoticeVersion"],"value":"astraflow-smoke"}]}}' \
  "$dsh_web_url/api/settings.mutate" >/tmp/dsh-settings-mutate.json
python3 - <<'PY'
import json

response = json.load(open("/tmp/dsh-settings-mutate.json", encoding="utf-8"))
assert response["result"]["ok"] is True, response
PY
grep -Fq 'welcomeNoticeVersion: astraflow-smoke' /tmp/astraflow-home/dsh/settings.yaml
kill "$dsh_web_pid" 2>/dev/null || true
wait "$dsh_web_pid" 2>/dev/null || true
echo 'dsh: web UI served and onboarding acknowledgement persisted with managed routing applied'

for executable in claude codex grok opencode hermes pi dsh prime-agent; do
  command -v "$executable" >/dev/null
done

echo 'all eight real harnesses passed hostile-config routing verification'
