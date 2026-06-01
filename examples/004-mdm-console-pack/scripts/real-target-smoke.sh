#!/usr/bin/env bash
set -euo pipefail

pack_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
examples_dir="$(cd "${pack_dir}/.." && pwd)"
repo_root="$(cd "${examples_dir}/.." && pwd)"

target_listen="${MDM_REAL_TARGET_SMOKE_LISTEN:-127.0.0.1:5807}"
supervisor_bind="${MDM_REAL_TARGET_SMOKE_SUPERVISOR_BIND:-127.0.0.1:5808}"
tmp_dir="$(mktemp -d "${TMPDIR:-/tmp}/mdm-real-target-smoke.XXXXXX")"
target_pid=""

cleanup() {
  if [[ -n "$target_pid" ]]; then
    kill "$target_pid" 2>/dev/null || true
    wait "$target_pid" 2>/dev/null || true
  fi
  rm -rf "$tmp_dir"
}
trap cleanup EXIT

binding_file="${tmp_dir}/target.json"
target_data_dir="${tmp_dir}/target-state"

cd "$repo_root"
./scripts/repo-cargo build -p meerkat-mobkit --example mdm_mob_target
target_bin="$(./scripts/repo-cargo --print-env | awk -F= '$1 == "CARGO_TARGET_DIR" { print $2 }')/debug/examples/mdm_mob_target"

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
set +e
MDM_SUPERVISOR_BIND_ADDRESS="$supervisor_bind" \
MDM_SUPERVISOR_ADVERTISED_ADDRESS="tcp://${supervisor_bind}" \
npx tsx 004-mdm-console-pack/run.ts \
  --targets "$binding_file" \
  --real-target-smoke \
  --smoke \
  --demo-llm
status=$?
set -e

if [[ "$status" -ne 0 ]]; then
  echo "[mdm-real-target-smoke] console bind failed; target log follows" >&2
  tail -100 "${tmp_dir}/target.log" >&2 || true
fi

exit "$status"
