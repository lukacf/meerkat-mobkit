#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
PACK_DIR="$ROOT/examples/001-incident-command-center-pack"
SCENARIO="$PACK_DIR/scenario.yaml"
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
  if [[ -n "${SERVER_PID:-}" ]]; then
    kill "${SERVER_PID}" >/dev/null 2>&1 || true
    wait "${SERVER_PID}" >/dev/null 2>&1 || true
    SERVER_PID=""
  fi
}
trap cleanup EXIT

wait_for_server() {
  python3 - <<'PY' "$1"
import sys, time, urllib.request
base_url = sys.argv[1]
deadline = time.time() + 120
while time.time() < deadline:
    try:
        with urllib.request.urlopen(base_url + "/console/experience", timeout=2) as resp:
            if resp.status == 200:
                sys.exit(0)
    except Exception:
        pass
    time.sleep(0.5)
print("timed out waiting for incident console server", file=sys.stderr)
sys.exit(1)
PY
}

start_server() {
  local listen_addr="$1"
  echo "[incident-pack] starting incident example server at http://${listen_addr}"
  (cd "$ROOT" && INCIDENT_COMMAND_CENTER_LISTEN_ADDR="$listen_addr" cargo run -p meerkat-mobkit --example incident_command_center > /tmp/incident-command-center.log 2>&1) &
  SERVER_PID=$!
  wait_for_server "http://${listen_addr}"
}

run_leg() {
  local label="$1"
  shift
  local listen_addr
  listen_addr="$(python3 - <<'PY'
import socket
s = socket.socket()
s.bind(("127.0.0.1", 0))
host, port = s.getsockname()
s.close()
print(f"{host}:{port}")
PY
)"
  start_server "$listen_addr"
  echo "[incident-pack] running ${label}"
  if ! "$@" "http://${listen_addr}"; then
    cleanup
    return 1
  fi
  cleanup
}

echo "[incident-pack] building console assets"
(cd "$ROOT/console" && npm run build --silent)

echo "[incident-pack] ensuring example JS deps"
(cd "$ROOT/examples" && npm install --silent --no-fund --no-audit)

status=0

run_leg "browser smoke" node "$PACK_DIR/browser_smoke.cjs" || status=1
run_leg "TypeScript smoke" bash -lc "cd \"$ROOT/examples\" && npx tsx \"$PACK_DIR/ts_smoke.ts\" \"\$1\"" _ || status=1
run_leg "Python smoke" python3 "$PACK_DIR/python_smoke.py" || status=1

exit "$status"
