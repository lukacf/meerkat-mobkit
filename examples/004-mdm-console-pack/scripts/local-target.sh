#!/usr/bin/env bash
set -euo pipefail

pack_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
state_dir="${pack_dir}/.state"
bindings_dir="${state_dir}/bindings"
targets_file="${MDM_TARGET_BINDINGS_FILE:-${state_dir}/target-bindings.json}"

usage() {
  cat >&2 <<'USAGE'
Usage:
  local-target.sh start [--id target-a] [--name this-mac] [--listen 127.0.0.1:5791] [--advertise tcp://127.0.0.1:5791] [--reset-state]
  local-target.sh foreground [same options]
  local-target.sh stop [--id target-a]
  local-target.sh status [--id target-a]

Environment:
  RKAT_BIN                      rkat binary, default rkat.
  MDM_TARGET_BINDINGS_FILE      Aggregated console binding file.
  MDM_TARGET_MODEL              Target model, default gpt-5.5.
  MDM_TARGET_PROVIDER           Target provider, default openai.
  MDM_TARGET_PAIRING_PASSWORD   Pairing password, default demo-password.
USAGE
}

command="${1:-start}"
if [[ $# -gt 0 ]]; then shift; fi

id="target-a"
name="$(hostname -s 2>/dev/null || hostname)"
listen="127.0.0.1:5791"
advertise=""
site="local"
platform="$(uname -s | tr '[:upper:]' '[:lower:]')-local"
model="${MDM_TARGET_MODEL:-gpt-5.5}"
provider="${MDM_TARGET_PROVIDER:-openai}"
pairing_password="${MDM_TARGET_PAIRING_PASSWORD:-demo-password}"
rkat_bin="${RKAT_BIN:-rkat}"
reset_state=0

while [[ $# -gt 0 ]]; do
  case "$1" in
    --id) id="$2"; shift 2 ;;
    --name) name="$2"; shift 2 ;;
    --listen) listen="$2"; shift 2 ;;
    --advertise) advertise="$2"; shift 2 ;;
    --site) site="$2"; shift 2 ;;
    --platform) platform="$2"; shift 2 ;;
    --model) model="$2"; shift 2 ;;
    --provider) provider="$2"; shift 2 ;;
    --pairing-password) pairing_password="$2"; shift 2 ;;
    --reset-state) reset_state=1; shift ;;
    -h|--help) usage; exit 0 ;;
    *) echo "unknown argument: $1" >&2; usage; exit 2 ;;
  esac
done

advertise="${advertise:-tcp://${listen}}"
mkdir -p "$bindings_dir" "${state_dir}/targets/${id}/context" "${state_dir}/targets/${id}/state"
pid_file="${state_dir}/targets/${id}.pid"
log_file="${state_dir}/targets/${id}.log"
rkat_binding_file="${bindings_dir}/${id}.rkat.json"
binding_file="${bindings_dir}/${id}.json"

rkat_args=(
  --realm "mdm-${id}"
  --context-root "${state_dir}/targets/${id}/context"
  --state-root "${state_dir}/targets/${id}/state"
  run
  --model "$model"
  --provider "$provider"
  --tools full
  --comms-name "$name"
  --comms-listen-tcp "$listen"
  --comms-binding-out "$rkat_binding_file"
  --comms-pairing-password "$pairing_password"
  --agent-description "${name} managed target"
  --agent-label "site=${site}"
  --agent-label "platform=${platform}"
  --agent-label "target_kind=mdm"
  --keep-alive
  "You are a remote managed target. Use local shell tools to inspect this host when asked."
)

metadata_json() {
  node -e 'const [id,name,site,platform,address,password]=process.argv.slice(1); console.log(JSON.stringify({id,name,site,platform,address,pairing_password:password,labels:{target_runtime:"rkat_run",shell:"unrestricted"}}));' \
    "$id" "$name" "$site" "$platform" "$advertise" "$pairing_password"
}

case "$command" in
  start)
    if [[ -f "$pid_file" ]] && kill -0 "$(cat "$pid_file")" 2>/dev/null; then
      if [[ "$reset_state" -eq 1 ]]; then
        kill "$(cat "$pid_file")"
        rm -f "$pid_file"
      else
        echo "[mdm-local-target] already running: $id pid=$(cat "$pid_file")"
        exit 0
      fi
    fi
    if [[ "$reset_state" -eq 1 ]]; then
      rm -rf "${state_dir}/targets/${id}"
      mkdir -p "${state_dir}/targets/${id}/context" "${state_dir}/targets/${id}/state"
    fi
    rm -f "$rkat_binding_file" "$binding_file"
    nohup "$rkat_bin" "${rkat_args[@]}" >"$log_file" 2>&1 </dev/null &
    echo $! >"$pid_file"
    for _ in {1..120}; do
      [[ -s "$rkat_binding_file" ]] && break
      sleep 0.5
    done
    if [[ ! -s "$rkat_binding_file" ]]; then
      echo "[mdm-local-target] target did not write binding; log: $log_file" >&2
      tail -100 "$log_file" >&2 || true
      exit 1
    fi
    for _ in {1..120}; do
      grep -q "Keep-alive: initial turn complete" "$log_file" && break
      sleep 0.5
    done
    if ! grep -q "Keep-alive: initial turn complete" "$log_file"; then
      echo "[mdm-local-target] target wrote binding but did not finish initial keep-alive turn; log: $log_file" >&2
      tail -100 "$log_file" >&2 || true
      exit 1
    fi
    node "${pack_dir}/scripts/wrap-rkat-binding.mjs" "$rkat_binding_file" "$binding_file" "$(metadata_json)"
    node "${pack_dir}/scripts/merge-bindings.mjs" "$targets_file" "$binding_file" >/dev/null
    echo "[mdm-local-target] started $id pid=$(cat "$pid_file")"
    echo "[mdm-local-target] binding: $binding_file"
    echo "[mdm-local-target] targets: $targets_file"
    ;;
  foreground)
    exec "$rkat_bin" "${rkat_args[@]}"
    ;;
  stop)
    if [[ -f "$pid_file" ]] && kill -0 "$(cat "$pid_file")" 2>/dev/null; then
      kill "$(cat "$pid_file")"
      rm -f "$pid_file"
      echo "[mdm-local-target] stopped $id"
    else
      echo "[mdm-local-target] not running: $id"
    fi
    ;;
  status)
    if [[ -f "$pid_file" ]] && kill -0 "$(cat "$pid_file")" 2>/dev/null; then
      echo "[mdm-local-target] running $id pid=$(cat "$pid_file")"
      echo "[mdm-local-target] binding: $binding_file"
    else
      echo "[mdm-local-target] stopped $id"
    fi
    ;;
  *)
    usage
    exit 2
    ;;
esac
