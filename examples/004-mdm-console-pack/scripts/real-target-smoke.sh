#!/usr/bin/env bash
set -euo pipefail

pack_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
examples_dir="$(cd "${pack_dir}/.." && pwd)"
repo_root="$(cd "${examples_dir}/.." && pwd)"

target_listen="${MDM_REAL_TARGET_SMOKE_LISTEN:-127.0.0.1:5807}"
supervisor_bind="${MDM_REAL_TARGET_SMOKE_SUPERVISOR_BIND:-127.0.0.1:5808}"
agent_comms="${MDM_REAL_TARGET_SMOKE_AGENT_COMMS:-127.0.0.1:5809}"
hive_smoke="${MDM_REAL_TARGET_SMOKE_HIVE:-0}"
tmp_dir="$(mktemp -d "${TMPDIR:-/tmp}/mdm-real-target-smoke.XXXXXX")"
target_pid=""

cleanup() {
  if [[ -n "$target_pid" ]]; then
    kill "$target_pid" 2>/dev/null || true
    wait "$target_pid" 2>/dev/null || true
  fi
  if [[ "${MDM_REAL_TARGET_SMOKE_KEEP_TMP:-0}" == "1" || "${MDM_REAL_TARGET_SMOKE_KEEP_TMP:-0}" == "true" ]]; then
    echo "[mdm-real-target-smoke] kept temp dir: $tmp_dir" >&2
  else
    rm -rf "$tmp_dir"
  fi
}
trap cleanup EXIT

binding_file="${tmp_dir}/target.json"
target_data_dir="${tmp_dir}/target-state"

cd "$repo_root"
./scripts/repo-cargo build -p meerkat-mobkit --example mdm_mob_target
target_bin="$(./scripts/repo-cargo --print-env | awk -F= '$1 == "CARGO_TARGET_DIR" { print $2 }')/debug/examples/mdm_mob_target"

MDM_SUPERVISOR_ADVERTISED_ADDRESS="tcp://${supervisor_bind}" \
"$target_bin" \
  --id target-smoke \
  --name target-smoke \
  --listen "$target_listen" \
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
  echo "[mdm-real-target-smoke] target did not write binding" >&2
  tail -100 "${tmp_dir}/target.log" >&2 || true
  exit 1
fi

cd "$examples_dir"
console_args=(
  004-mdm-console-pack/run.ts
  --targets "$binding_file"
  --state-dir "${tmp_dir}/console-state"
  --real-target-smoke
  --smoke
)
if [[ "$hive_smoke" == "1" || "$hive_smoke" == "true" ]]; then
  if [[ -z "${OPENAI_API_KEY:-}" ]]; then
    echo "[mdm-real-target-smoke] hive smoke requires OPENAI_API_KEY" >&2
    exit 1
  fi
  console_args+=(--hive-target-smoke)
else
  console_args+=(--demo-llm)
fi

set +e
MDM_SUPERVISOR_BIND_ADDRESS="$supervisor_bind" \
MDM_SUPERVISOR_ADVERTISED_ADDRESS="tcp://${supervisor_bind}" \
MDM_AGENT_COMMS_ADDRESS="$agent_comms" \
npx tsx "${console_args[@]}"
status=$?
set -e

if [[ "$status" -ne 0 ]]; then
  echo "[mdm-real-target-smoke] console smoke failed; target log follows" >&2
  tail -100 "${tmp_dir}/target.log" >&2 || true
else
  if ! grep -q "\\[mdm-target\\] peer turn accepted" "${tmp_dir}/target.log"; then
    echo "[mdm-real-target-smoke] target did not observe a peer-delivered turn; target log follows" >&2
    tail -100 "${tmp_dir}/target.log" >&2 || true
    exit 1
  fi
  if [[ "$hive_smoke" == "1" || "$hive_smoke" == "true" ]]; then
    peer_turn_count="$(grep -c "\\[mdm-target\\] peer turn accepted" "${tmp_dir}/target.log" || true)"
    if [[ "$peer_turn_count" -lt 2 ]]; then
      echo "[mdm-real-target-smoke] hive smoke did not produce an additional peer-delivered target turn; target log follows" >&2
      tail -160 "${tmp_dir}/target.log" >&2 || true
      exit 1
    fi
  fi
fi

exit "$status"
