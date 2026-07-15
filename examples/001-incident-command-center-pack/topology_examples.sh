#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
PACK_DIR="$ROOT/examples/001-incident-command-center-pack"
ARTIFACT_DIR="${INCIDENT_TOPOLOGY_ARTIFACT_DIR:-$ROOT/output/playwright}"
RUST_LANE_ID="${RUST_LANE_ID:-incident-console}"
CARGO_INCREMENTAL="${CARGO_INCREMENTAL:-0}"
STATE_DIR="$(mktemp -d "${TMPDIR:-/tmp}/incident-topology-smoke.XXXXXX")"
LISTEN_ADDR="$(python3 - <<'PY'
import socket
s = socket.socket()
s.bind(("127.0.0.1", 0))
host, port = s.getsockname()
s.close()
print(f"{host}:{port}")
PY
)"

cleanup() {
  stop_server
  rm -rf "$STATE_DIR"
}
trap cleanup EXIT
trap 'cleanup; exit 130' INT
trap 'cleanup; exit 143' TERM

wait_for_server() {
  python3 - <<'PY' "$1" "$SERVER_PID" "${INCIDENT_TOPOLOGY_READY_TIMEOUT_SECONDS:-600}"
import os, sys, time, urllib.request
base_url = sys.argv[1]
server_pid = int(sys.argv[2])
ready_timeout_seconds = float(sys.argv[3])
deadline = time.time() + ready_timeout_seconds
while time.time() < deadline:
    try:
        os.kill(server_pid, 0)
    except ProcessLookupError:
        print("offline incident topology server exited before becoming ready", file=sys.stderr)
        sys.exit(2)
    try:
        with urllib.request.urlopen(base_url + "/console/experience", timeout=2) as response:
            if response.status == 200:
                sys.exit(0)
    except Exception:
        pass
    time.sleep(0.25)
print(
    f"timed out after {ready_timeout_seconds:g}s waiting for offline incident topology server",
    file=sys.stderr,
)
sys.exit(1)
PY
}

stop_server() {
  if [[ -z "${SERVER_PID:-}" ]]; then
    return
  fi
  kill -TERM -- -"${SERVER_PID}" >/dev/null 2>&1 || kill "${SERVER_PID}" >/dev/null 2>&1 || true
  wait "${SERVER_PID}" >/dev/null 2>&1 || true
  SERVER_PID=""
}

start_server() {
  local phase="$1"
  echo "[incident-topology] starting deterministic server (${phase}) at http://${LISTEN_ADDR}"
  set -m
  (
    cd "$ROOT"
    INCIDENT_COMMAND_CENTER_OFFLINE=1 \
    INCIDENT_COMMAND_CENTER_LISTEN_ADDR="$LISTEN_ADDR" \
    INCIDENT_COMMAND_CENTER_STATE_DIR="$STATE_DIR/runtime" \
    INCIDENT_COMMAND_CENTER_MEMORY_DIR="$STATE_DIR/memory" \
    RUST_LANE_ID="$RUST_LANE_ID" \
    CARGO_INCREMENTAL="$CARGO_INCREMENTAL" \
    ./scripts/repo-cargo run -p meerkat-mobkit --example incident_command_center \
      >>"$STATE_DIR/server.log" 2>&1
  ) &
  SERVER_PID=$!
  set +m
  if ! wait_for_server "http://${LISTEN_ADDR}"; then
    sed -n '1,240p' "$STATE_DIR/server.log" >&2 || true
    return 1
  fi
}

echo "[incident-topology] building the stock embedded console"
(cd "$ROOT/console" && npm run build --silent)

echo "[incident-topology] ensuring Playwright dependencies"
(cd "$ROOT/examples" && npm ci --silent --no-fund --no-audit)

echo "[incident-topology] building the deterministic Rust example"
(cd "$ROOT" && RUST_LANE_ID="$RUST_LANE_ID" CARGO_INCREMENTAL="$CARGO_INCREMENTAL" \
  ./scripts/repo-cargo build -p meerkat-mobkit \
    --example incident_command_center --bin mcp_fixture)

mkdir -p "$ARTIFACT_DIR"
echo "[incident-topology] browser artifacts: $ARTIFACT_DIR"

start_server "prepare"
INCIDENT_TOPOLOGY_ARTIFACT_DIR="$ARTIFACT_DIR" \
  node "$PACK_DIR/topology_browser_smoke.cjs" "http://${LISTEN_ADDR}" --prepare

echo "[incident-topology] restarting against the same durable topology state"
stop_server
start_server "resume"
INCIDENT_TOPOLOGY_ARTIFACT_DIR="$ARTIFACT_DIR" \
  node "$PACK_DIR/topology_browser_smoke.cjs" "http://${LISTEN_ADDR}" --resume
