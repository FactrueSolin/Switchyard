#!/usr/bin/env bash
#
# Build and run switchyard-server with the decision_model routing algorithm.
#
# The decision model runs on DashScope (Bailian); the completion targets
# (strong/weak) run on the api.cd.actrue.cn gateway (OpenAI-compatible).
#
# Credentials come from the environment or from dev-server/decision-model.env
# (gitignored): DASHSCOPE_API_KEY and ACTRUE_API_KEY are required.
# Built in (override from the environment if needed):
#   DASHSCOPE_WORKSPACE_ID
#   HOST (127.0.0.1)  PORT (4000)
#   STRONG_MODEL / WEAK_MODEL / DECISION_MODEL / DECISION_REGION
#
# Usage:
#   ./dev-server/decision-model.sh [--dry-run]
set -euo pipefail

cd "$(dirname "$0")/.."

# Load local credentials (not committed).
ENV_FILE="$(dirname "$0")/decision-model.env"
if [ -f "$ENV_FILE" ]; then
    . "$ENV_FILE"
fi
: "${DASHSCOPE_API_KEY:?DASHSCOPE_API_KEY required; set it or create dev-server/decision-model.env}"
: "${ACTRUE_API_KEY:?ACTRUE_API_KEY required; set it or create dev-server/decision-model.env}"
export DASHSCOPE_API_KEY ACTRUE_API_KEY

# Decision model workspace (DashScope / Bailian).
export DASHSCOPE_WORKSPACE_ID="${DASHSCOPE_WORKSPACE_ID:-llm-8z032jbb8ydmpkmg}"

HOST="${HOST:-0.0.0.0}"
PORT="${PORT:-14000}"
DECISION_MODEL="${DECISION_MODEL:-decision-model-preview}"
DECISION_REGION="${DECISION_REGION:-cn-beijing}"
STRONG_MODEL="${STRONG_MODEL:-Qwen3.8-Flash-Next}"
WEAK_MODEL="${WEAK_MODEL:-qwen3.8}"

DECISION_BASE_URL="https://${DASHSCOPE_WORKSPACE_ID}.${DECISION_REGION}.maas.aliyuncs.com/compatible-mode/v1"
CONFIG="$(mktemp -d)/switchyard-decision.toml"

cat > "$CONFIG" <<EOF
schema_version = 1

# Completion targets only. The decision model client is configured inline in
# the route, so it does not need an llm_clients entry.
[llm_clients.actrue]
format = "openai_responses"
base_url = "https://api.cd.actrue.cn/v1"
api_key_env = "ACTRUE_API_KEY"

[targets.strong]
id = "${STRONG_MODEL}"
llm_client = "actrue"

[targets.weak]
id = "${WEAK_MODEL}"
llm_client = "actrue"

[routes.smart]
id = "switchyard/smart"
type = "decision_model"
strong_target = "strong"
weak_target = "weak"
default_target = "weak"
decision_base_url = "${DECISION_BASE_URL}"
decision_api_key_env = "DASHSCOPE_API_KEY"
decision_model = "${DECISION_MODEL}"
classify_trigger = "every_request"
decision_timeout_ms = 10000
EOF

cargo build --release -p switchyard-server
BIN="target/release/switchyard-server"

ROUTING_LOG="${ROUTING_LOG:-$(pwd)/routing.jsonl}"

echo "config: $CONFIG"
echo "routing log: $ROUTING_LOG"
"$BIN" --config "$CONFIG" --routing-log-file "$ROUTING_LOG" --dry-run
echo "dry-run ok"

if [[ "${1:-}" == "--dry-run" ]]; then
  exit 0
fi

exec "$BIN" --config "$CONFIG" --host "$HOST" --port "$PORT" --routing-log-file "$ROUTING_LOG"
