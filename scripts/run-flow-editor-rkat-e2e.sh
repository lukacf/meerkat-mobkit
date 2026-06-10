#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PORT="${MOBKIT_FLOW_EDITOR_E2E_PORT:-4192}"
ADDR="127.0.0.1:${PORT}"
RPC_URL="http://${ADDR}/flow-editor/rpc"
CARGO_CMD="${CARGO:-"$ROOT/scripts/repo-cargo"}"
NPM_SCRIPT="test:rkat-e2e"
SERVER_ARGS=(--listen "$ADDR")
NPM_ENV=(MOBKIT_FLOW_EDITOR_RPC_URL="$RPC_URL")

if [[ "${1:-}" == "--deploy" ]]; then
  NPM_SCRIPT="test:rkat-deploy-e2e"
  SERVER_ARGS+=(--allow-host-deploy)
  NPM_ENV+=(MOBKIT_FLOW_EDITOR_EXPECT_HOST_DEPLOY=1)
fi

if ! command -v rkat >/dev/null 2>&1; then
  echo "rkat is required for Flow Editor live mobpack validation" >&2
  exit 127
fi

CARGO_INCREMENTAL="${CARGO_INCREMENTAL:-0}" "$CARGO_CMD" build -p meerkat-mobkit --bin mobkit_flow_editor

LOG_FILE="$(mktemp -t mobkit-flow-editor-rkat-e2e.XXXXXX.log)"
"$ROOT/target/debug/mobkit_flow_editor" "${SERVER_ARGS[@]}" >"$LOG_FILE" 2>&1 &
SERVER_PID=$!

cleanup() {
  kill "$SERVER_PID" >/dev/null 2>&1 || true
  wait "$SERVER_PID" >/dev/null 2>&1 || true
  rm -f "$LOG_FILE"
}
trap cleanup EXIT

for _ in $(seq 1 100); do
  if ! kill -0 "$SERVER_PID" >/dev/null 2>&1; then
    echo "mobkit_flow_editor exited before becoming ready" >&2
    cat "$LOG_FILE" >&2 || true
    exit 1
  fi
  if curl -fsS -X POST "$RPC_URL" \
    -H "Content-Type: application/json" \
    -d '{"jsonrpc":"2.0","id":"ready","method":"mobkit/capabilities","params":{}}' \
    >/dev/null 2>&1; then
    env "${NPM_ENV[@]}" npm --prefix "$ROOT/flow-editor" run "$NPM_SCRIPT" --silent
    exit 0
  fi
  sleep 0.2
done

echo "timed out waiting for mobkit_flow_editor at $RPC_URL" >&2
cat "$LOG_FILE" >&2 || true
exit 1
