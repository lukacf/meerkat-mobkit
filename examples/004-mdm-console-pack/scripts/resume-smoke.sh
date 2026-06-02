#!/usr/bin/env bash
set -euo pipefail

pack_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
examples_dir="$(cd "${pack_dir}/.." && pwd)"
repo_root="$(cd "${examples_dir}/.." && pwd)"

target_listen="${MDM_RESUME_SMOKE_LISTEN:-127.0.0.1:5817}"
supervisor_bind="${MDM_RESUME_SMOKE_SUPERVISOR_BIND:-127.0.0.1:5818}"
agent_comms="${MDM_RESUME_SMOKE_AGENT_COMMS:-127.0.0.1:5819}"
tmp_dir="$(mktemp -d "${TMPDIR:-/tmp}/mdm-resume-smoke.XXXXXX")"
target_pid=""

cleanup() {
  if [[ -n "$target_pid" ]]; then
    kill "$target_pid" 2>/dev/null || true
    wait "$target_pid" 2>/dev/null || true
  fi
  if [[ "${MDM_RESUME_SMOKE_KEEP_TMP:-0}" == "1" || "${MDM_RESUME_SMOKE_KEEP_TMP:-0}" == "true" ]]; then
    echo "[mdm-resume-smoke] kept temp dir: $tmp_dir" >&2
  else
    rm -rf "$tmp_dir"
  fi
}
trap cleanup EXIT

if [[ -z "${OPENAI_API_KEY:-}" ]]; then
  echo "[mdm-resume-smoke] OPENAI_API_KEY is required for the boot-B hive smoke" >&2
  exit 1
fi

binding_file="${tmp_dir}/target.json"
target_data_dir="${tmp_dir}/target-state"
console_state_dir="${tmp_dir}/console-state"

cd "$repo_root"
./scripts/repo-cargo build -p meerkat-mobkit --example mdm_mob_target --bin rpc_gateway
target_bin="$(./scripts/repo-cargo --print-env | awk -F= '$1 == "CARGO_TARGET_DIR" { print $2 }')/debug/examples/mdm_mob_target"

"$target_bin" \
  --id target-resume-smoke \
  --name target-resume-smoke \
  --listen "$target_listen" \
  --control-listen "$(python3 - "$target_listen" <<'PY'
import sys
host, port = sys.argv[1].rsplit(":", 1)
print(f"{host}:{int(port)+1000}")
PY
)" \
  --advertise "tcp://${target_listen}" \
  --data-dir "$target_data_dir" \
  --binding-out "$binding_file" \
  --model "${MDM_TARGET_MODEL:-gpt-5.5}" \
  --provider "${MDM_TARGET_PROVIDER:-openai}" \
  >"${tmp_dir}/target.log" 2>&1 &
target_pid="$!"

for _ in {1..120}; do
  [[ -s "$binding_file" ]] && break
  sleep 0.5
done
if [[ ! -s "$binding_file" ]]; then
  echo "[mdm-resume-smoke] target did not write binding" >&2
  tail -100 "${tmp_dir}/target.log" >&2 || true
  exit 1
fi

run_console_boot() {
  local boot="$1"
  shift
  cd "$examples_dir"
  MDM_SUPERVISOR_BIND_ADDRESS="$supervisor_bind" \
  MDM_SUPERVISOR_ADVERTISED_ADDRESS="tcp://${supervisor_bind}" \
  MDM_AGENT_COMMS_ADDRESS="$agent_comms" \
  npx tsx 004-mdm-console-pack/run.ts \
    --targets "$binding_file" \
    --state-dir "$console_state_dir" \
    --real-target-smoke \
    --smoke \
    --skip-build \
    "$@" | tee "${tmp_dir}/console-${boot}.log"
}

run_console_boot A --demo-llm
[[ -f "${console_state_dir}/mob.sqlite" ]] || {
  echo "[mdm-resume-smoke] boot A did not create persistent mob.sqlite" >&2
  exit 1
}

run_console_boot B --hive-target-smoke

peer_turns="$(grep -c "\\[mdm-target\\] peer turn accepted" "${tmp_dir}/target.log" || true)"
peer_responses="$(grep -c "\\[mdm-target\\] peer response sent" "${tmp_dir}/target.log" || true)"
if [[ "$peer_turns" -lt 3 || "$peer_responses" -lt 1 ]]; then
  echo "[mdm-resume-smoke] expected boot A direct turn, boot B direct turn, and boot B hive peer request" >&2
  tail -160 "${tmp_dir}/target.log" >&2 || true
  exit 1
fi

echo "[mdm-resume-smoke] ok peer_turns=${peer_turns} peer_responses=${peer_responses}"
