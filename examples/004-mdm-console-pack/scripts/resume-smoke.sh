#!/usr/bin/env bash
set -euo pipefail

pack_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
examples_dir="$(cd "${pack_dir}/.." && pwd)"

target_listen="${MDM_RESUME_SMOKE_LISTEN:-127.0.0.1:5817}"
agent_comms="${MDM_RESUME_SMOKE_AGENT_COMMS:-127.0.0.1:5819}"
supervisor_bridge="${MDM_RESUME_SMOKE_SUPERVISOR_BRIDGE:-127.0.0.1:5818}"
rkat_bin="${RKAT_BIN:-rkat}"
pairing_password="${MDM_TARGET_PAIRING_PASSWORD:-demo-password}"
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

rkat_binding_file="${tmp_dir}/target.rkat.json"
binding_file="${tmp_dir}/target.json"
target_context="${tmp_dir}/target-context"
target_state="${tmp_dir}/target-state"
console_state_dir="${tmp_dir}/console-state"
mkdir -p "$target_context" "$target_state"

write_target_transcripts() {
  local transcript="$1"
  local sessions
  sessions="$("$rkat_bin" \
    --realm mdm-target-resume-smoke \
    --context-root "$target_context" \
    --state-root "$target_state" \
    session list --limit 20 2>/dev/null | awk '/^[0-9a-f-]{36}/ {print $1}')"
  if [[ -z "$sessions" ]]; then
    echo "[mdm-resume-smoke] no persisted target sessions found" >&2
    return 1
  fi

  : >"$transcript"
  while IFS= read -r session_id; do
    [[ -n "$session_id" ]] || continue
    "$rkat_bin" \
      --realm mdm-target-resume-smoke \
      --context-root "$target_context" \
      --state-root "$target_state" \
      session show "$session_id" >>"$transcript" 2>&1 || true
  done <<<"$sessions"
}

assert_target_used_shell() {
  local transcript="${tmp_dir}/target-transcripts.txt"
  write_target_transcripts "$transcript"

  if grep -Eq 'Tool call: shell|Tool calls: .*shell' "$transcript"; then
    echo "[mdm-resume-smoke] target transcript contains shell tool use"
    return 0
  fi

  echo "[mdm-resume-smoke] target transcript did not contain shell tool use" >&2
  tail -200 "$transcript" >&2 || true
  return 1
}

assert_target_transcript_records_hive_response() {
  local token="$1"
  local transcript="${tmp_dir}/target-transcripts-${token}.txt"
  write_target_transcripts "$transcript"

  if grep -Fq "Peer request: checksum_token" "$transcript" && grep -Fq "Tool call: send_response" "$transcript"; then
    echo "[mdm-resume-smoke] target transcript records checksum peer request and response"
    return 0
  fi

  echo "[mdm-resume-smoke] target transcript did not record checksum peer request/response for $token" >&2
  tail -200 "$transcript" >&2 || true
  return 1
}

assert_hive_peer_request_consumed() {
  local token="$1"
  local db="${target_state}/mdm-target-resume-smoke/sessions.sqlite3"
  if ! command -v sqlite3 >/dev/null 2>&1; then
    echo "[mdm-resume-smoke] sqlite3 is required to verify target-side peer request consumption" >&2
    return 1
  fi
  if [[ ! -s "$db" ]]; then
    echo "[mdm-resume-smoke] target session database not found: $db" >&2
    return 1
  fi

  local row=""
  for _ in {1..180}; do
    row="$(sqlite3 -separator '|' "$db" "select coalesce(json_extract(state_json,'$.current_state'),''), coalesce(json_extract(state_json,'$.policy.decision.apply_mode'),''), coalesce(json_extract(state_json,'$.policy.decision.routing_disposition'),''), substr(coalesce(json_extract(state_json,'$.persisted_input.body'),''),1,180) from runtime_input_states where json_extract(state_json,'$.persisted_input.convention.convention_type')='request' and json_extract(state_json,'$.persisted_input.convention.intent')='checksum_token' and json_extract(state_json,'$.persisted_input.body') like '%${token}%' order by rowid desc limit 1;")"
    [[ "$row" == consumed'|'* ]] && break
    sleep 1
  done
  if [[ -z "$row" ]]; then
    echo "[mdm-resume-smoke] no target-side checksum peer request found for $token" >&2
    sqlite3 -header -column "$db" "select rowid,json_extract(state_json,'$.current_state') as state,json_extract(state_json,'$.persisted_input.convention.convention_type') as convention,json_extract(state_json,'$.persisted_input.convention.intent') as intent,substr(json_extract(state_json,'$.persisted_input.body'),1,140) as body from runtime_input_states order by rowid desc limit 10;" >&2 || true
    return 1
  fi
  if [[ "$row" != consumed'|'* ]]; then
    echo "[mdm-resume-smoke] target-side checksum peer request was not consumed for $token: $row" >&2
    return 1
  fi

  echo "[mdm-resume-smoke] target consumed hive peer request $token"
}

"$rkat_bin" \
  --realm mdm-target-resume-smoke \
  --context-root "$target_context" \
  --state-root "$target_state" \
  run \
  --model "${MDM_TARGET_MODEL:-gpt-5.5}" \
  --provider "${MDM_TARGET_PROVIDER:-openai}" \
  --tools full \
  --comms-name target-resume-smoke \
  --comms-listen-tcp "$target_listen" \
  --comms-binding-out "$rkat_binding_file" \
  --comms-pairing-password "$pairing_password" \
  --agent-description "MDM resume smoke target" \
  --agent-label site=local-smoke \
  --agent-label platform=local-smoke \
  --agent-label target_kind=mdm \
  --keep-alive \
  "You are a remote managed target. Use local shell tools to inspect this host when asked." \
  >"${tmp_dir}/target.log" 2>&1 &
target_pid="$!"

for _ in {1..120}; do
  [[ -s "$rkat_binding_file" ]] && break
  sleep 0.5
done
if [[ ! -s "$rkat_binding_file" ]]; then
  echo "[mdm-resume-smoke] target did not write binding" >&2
  tail -100 "${tmp_dir}/target.log" >&2 || true
  exit 1
fi

for _ in {1..120}; do
  grep -q "Keep-alive: initial turn complete" "${tmp_dir}/target.log" && break
  sleep 0.5
done
if ! grep -q "Keep-alive: initial turn complete" "${tmp_dir}/target.log"; then
  echo "[mdm-resume-smoke] target wrote binding but did not finish initial keep-alive turn" >&2
  tail -100 "${tmp_dir}/target.log" >&2 || true
  exit 1
fi

node "${pack_dir}/scripts/wrap-rkat-binding.mjs" \
  "$rkat_binding_file" \
  "$binding_file" \
  "$(node -e 'const [password,address]=process.argv.slice(1); console.log(JSON.stringify({id:"target-resume-smoke",name:"target-resume-smoke",site:"local-smoke",platform:"local-smoke",address,pairing_password:password,labels:{target_runtime:"rkat_run",shell:"unrestricted"}}));' "$pairing_password" "tcp://${target_listen}")"

run_console_boot() {
  local boot="$1"
  shift
  cd "$examples_dir"
  MDM_AGENT_COMMS_ADDRESS="$agent_comms" \
  MDM_SUPERVISOR_BRIDGE_ADDRESS="$supervisor_bridge" \
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
hive_token="$(sed -n 's/.*token=\(mdm-hive-smoke-[0-9][0-9]*\).*/\1/p' "${tmp_dir}/console-B.log" | tail -1)"
if [[ -z "$hive_token" ]]; then
  echo "[mdm-resume-smoke] boot B hive smoke did not report checksum token" >&2
  tail -100 "${tmp_dir}/console-B.log" >&2 || true
  exit 1
fi
assert_hive_peer_request_consumed "$hive_token"
assert_target_transcript_records_hive_response "$hive_token"
assert_target_used_shell
echo "[mdm-resume-smoke] ok"
