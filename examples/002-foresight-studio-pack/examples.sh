#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
PACK_DIR="$ROOT/examples/002-foresight-studio-pack"

echo "[foresight-pack] ensuring example JS deps"
(cd "$ROOT/examples" && npm install --silent --no-fund --no-audit)

echo "[foresight-pack] building TypeScript SDK"
(cd "$ROOT/sdk/typescript" && npm install --silent --no-fund --no-audit && npm run build --silent)

echo "[foresight-pack] running offline smoke"
(cd "$ROOT/examples" && npx tsx "$PACK_DIR/ts_smoke.ts")

if [[ "${1:-}" == "--smoke" ]]; then
  exit 0
fi

if [[ " $* " == *" --browser-smoke "* ]]; then
  echo "[foresight-pack] building console assets"
  (cd "$ROOT/console" && npm run build --silent)

  LOG_FILE="$(mktemp -t foresight-studio.XXXXXX.log)"
  cleanup() {
    if [[ -n "${SERVER_PID:-}" ]]; then
      kill "$SERVER_PID" >/dev/null 2>&1 || true
      wait "$SERVER_PID" >/dev/null 2>&1 || true
    fi
  }
  trap cleanup EXIT

  echo "[foresight-pack] starting live console for browser smoke"
  (cd "$ROOT/examples" && npx tsx "$PACK_DIR/run.ts" >"$LOG_FILE" 2>&1) &
  SERVER_PID=$!

  BASE_URL=""
  for _ in $(seq 1 180); do
    if ! kill -0 "$SERVER_PID" >/dev/null 2>&1; then
      echo "[foresight-pack] runtime exited before printing a console URL" >&2
      cat "$LOG_FILE" >&2
      exit 1
    fi
    LINE="$(grep -m1 "\\[foresight\\] console:" "$LOG_FILE" || true)"
    if [[ -n "$LINE" ]]; then
      BASE_URL="${LINE##*console: }"
      BASE_URL="${BASE_URL%/console}"
      break
    fi
    sleep 0.5
  done

  if [[ -z "$BASE_URL" ]]; then
    echo "[foresight-pack] timed out waiting for console URL" >&2
    cat "$LOG_FILE" >&2
    exit 1
  fi

  ARTIFACT_DIR="$ROOT/output/playwright/example-002"
  echo "[foresight-pack] running browser smoke at $BASE_URL"
  (cd "$ROOT/examples" && MOBKIT_BROWSER_SMOKE_ARTIFACT_DIR="$ARTIFACT_DIR" node "$PACK_DIR/browser_smoke.cjs" "$BASE_URL")
  exit $?
fi

if [[ " $* " == *" --real-llm "* && -z "${OPENAI_API_KEY:-}" ]]; then
  echo "Set OPENAI_API_KEY to run the live foresight studio pack with --real-llm" >&2
  exit 1
fi

echo "[foresight-pack] building console assets"
(cd "$ROOT/console" && npm run build --silent)

echo "[foresight-pack] starting live Foresight Studio"
(cd "$ROOT/examples" && npx tsx "$PACK_DIR/run.ts" "$@")
