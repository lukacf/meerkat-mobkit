#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

docker compose up -d

cleanup() {
  docker compose down
}
trap cleanup EXIT

for _ in $(seq 1 120); do
  body="$(curl -fsS http://127.0.0.1:5788/api/targets || true)"
  if [[ "$body" == *"target-a"* && "$body" == *"target-b"* ]]; then
    break
  fi
  sleep 1
done

body="$(curl -fsS http://127.0.0.1:5788/api/targets)"
if [[ "$body" != *"target-a"* || "$body" != *"target-b"* ]]; then
  echo "targets did not register through Docker compose" >&2
  echo "$body" >&2
  exit 1
fi

curl -fsS -X POST http://127.0.0.1:5788/api/targets/target-b/claim \
  -H 'content-type: application/json' \
  -d '{"operator":"docker-smoke"}' >/dev/null

turn="$(curl -fsS -X POST http://127.0.0.1:5788/api/targets/target-b/turn \
  -H 'content-type: application/json' \
  -d '{"operator":"docker-smoke","prompt":"shell: echo MOBKIT_MDM_DOCKER_SMOKE"}')"

if [[ "$turn" != *"MOBKIT_MDM_DOCKER_SMOKE"* ]]; then
  echo "remote Docker target turn did not return marker" >&2
  echo "$turn" >&2
  exit 1
fi

echo "[mdm-docker-smoke] ok"
