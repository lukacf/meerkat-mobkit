#!/usr/bin/env bash
# Access Control Pack runner.
#
#   ./examples.sh           build console, start the example server, run the
#                           deterministic HTTP smoke (no API key needed)
#   ./examples.sh --serve   build console, start the server + persona proxies,
#                           and leave them running for browser exploration
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
PACK_DIR="$ROOT/examples/005-access-control-pack"
MODE="${1:-smoke}"

pick_port() {
  python3 - <<'PY'
import socket
s = socket.socket()
s.bind(("127.0.0.1", 0))
print("%s:%d" % s.getsockname())
s.close()
PY
}

wait_for_server() {
  python3 - "$1" <<'PY'
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
print("timed out waiting for access control server", file=sys.stderr)
sys.exit(1)
PY
}

cleanup() {
  for pid in "${SERVER_PID:-}" "${PROXY_PID:-}"; do
    [[ -n "$pid" ]] && { kill "$pid" >/dev/null 2>&1 || true; wait "$pid" >/dev/null 2>&1 || true; }
  done
}
trap cleanup EXIT

echo "[access-control-pack] building console assets"
(cd "$ROOT/console" && npm run build --silent)

# Fresh work dir so the seeded access.toml is deterministic each run.
WORK_DIR="$(mktemp -d -t mobkit-access-control-pack.XXXXXX)"
export ACCESS_CONTROL_WORK_DIR="$WORK_DIR"

if [[ "$MODE" == "--serve" ]]; then
  LISTEN_ADDR="127.0.0.1:7300"
  echo "[access-control-pack] starting example server at http://${LISTEN_ADDR}"
  (cd "$ROOT" && ACCESS_CONTROL_LISTEN_ADDR="$LISTEN_ADDR" \
    cargo run -p meerkat-mobkit --example access_control_console) &
  SERVER_PID=$!
  wait_for_server "http://${LISTEN_ADDR}"
  echo "[access-control-pack] starting persona proxies"
  ACCESS_CONTROL_TARGET="$LISTEN_ADDR" node "$PACK_DIR/persona-proxy.mjs" &
  PROXY_PID=$!
  echo "[access-control-pack] ready — open the persona tabs printed above. Ctrl-C to stop."
  wait "$SERVER_PID"
  exit 0
fi

LISTEN_ADDR="$(pick_port)"
echo "[access-control-pack] starting example server at http://${LISTEN_ADDR}"
(cd "$ROOT" && ACCESS_CONTROL_LISTEN_ADDR="$LISTEN_ADDR" \
  cargo run -p meerkat-mobkit --example access_control_console > "$WORK_DIR/server.log" 2>&1) &
SERVER_PID=$!
wait_for_server "http://${LISTEN_ADDR}"
echo "[access-control-pack] running HTTP smoke"
node "$PACK_DIR/smoke.mjs" "http://${LISTEN_ADDR}"
