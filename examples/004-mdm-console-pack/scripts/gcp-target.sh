#!/usr/bin/env bash
set -euo pipefail

pack_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
env_file="${MDM_GCP_ENV_FILE:-${pack_dir}/.env.gcp}"
state_dir="${pack_dir}/.state"
bindings_dir="${state_dir}/bindings"
targets_file="${MDM_TARGET_BINDINGS_FILE:-${state_dir}/target-bindings.json}"
meerkat_repo="${MEERKAT_REPO:-/Users/luka/.codex/worktrees/8ebe/meerkat}"

if [[ -f "$env_file" ]]; then
  set -a
  # shellcheck disable=SC1090
  . "$env_file"
  set +a
fi

usage() {
  cat >&2 <<'USAGE'
Usage:
  gcp-target.sh start --id target-b [--name gcp-target-b] [--listen HOST:PORT] [--advertise tcp://HOST:PORT] [--reset-state]
  gcp-target.sh fetch --id target-b
  gcp-target.sh stop --id target-b

Required environment or .env.gcp:
  GCP_PROJECT
  GCP_ZONE

Optional:
  MEERKAT_REPO                  local Meerkat repo/worktree to sync to the VM.
  GCP_TARGET_PREFIX             default mobkit-mdm-target
  GCP_MACHINE_TYPE              default e2-medium
  GCP_IMAGE_FAMILY              default debian-12
  GCP_IMAGE_PROJECT             default debian-cloud
  GCP_SSH_USER
  MDM_TARGET_MODEL              default gpt-5.5
  MDM_TARGET_PROVIDER           default openai
  MDM_TARGET_PAIRING_PASSWORD   default demo-password
  MDM_GCP_FORWARD_ENV           space-separated env names to copy to the VM
USAGE
}

command="${1:-start}"
if [[ $# -gt 0 ]]; then shift; fi

id=""
name=""
listen=""
advertise=""
site="gcp-live"
platform="linux-gcp-vm"
model="${MDM_TARGET_MODEL:-gpt-5.5}"
provider="${MDM_TARGET_PROVIDER:-openai}"
pairing_password="${MDM_TARGET_PAIRING_PASSWORD:-demo-password}"
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

[[ -n "$id" ]] || { echo "--id is required" >&2; usage; exit 2; }
[[ -d "$meerkat_repo" ]] || { echo "MEERKAT_REPO does not exist: $meerkat_repo" >&2; exit 2; }
name="${name:-$id}"
: "${GCP_PROJECT:?GCP_PROJECT is required}"
: "${GCP_ZONE:?GCP_ZONE is required}"

prefix="${GCP_TARGET_PREFIX:-mobkit-mdm-target}"
machine_type="${GCP_MACHINE_TYPE:-e2-medium}"
image_family="${GCP_IMAGE_FAMILY:-debian-12}"
image_project="${GCP_IMAGE_PROJECT:-debian-cloud}"
instance="${prefix}-${id}"
ssh_target="$instance"
if [[ -n "${GCP_SSH_USER:-}" ]]; then
  ssh_target="${GCP_SSH_USER}@${instance}"
fi

gcloud_base=(gcloud --project "$GCP_PROJECT" compute)
ssh_base=("${gcloud_base[@]}" ssh "$ssh_target" --zone "$GCP_ZONE" --quiet)
scp_base=("${gcloud_base[@]}" scp --zone "$GCP_ZONE" --quiet)

shell_quote() {
  printf "'"
  printf "%s" "$1" | sed "s/'/'\\\\''/g"
  printf "'"
}

listen_from_tcp_address() {
  local address="$1"
  if [[ "$address" != tcp://* ]]; then
    echo "only tcp:// advertise addresses can infer --listen: $address" >&2
    return 1
  fi
  local authority="${address#tcp://}"
  authority="${authority%%/*}"
  if [[ -z "$authority" || "$authority" != *:* ]]; then
    echo "cannot infer --listen from advertise address: $address" >&2
    return 1
  fi
  printf "%s" "$authority"
}

ensure_instance() {
  if "${gcloud_base[@]}" instances describe "$instance" --zone "$GCP_ZONE" >/dev/null 2>&1; then
    return
  fi
  "${gcloud_base[@]}" instances create "$instance" \
    --zone "$GCP_ZONE" \
    --machine-type "$machine_type" \
    --image-family "$image_family" \
    --image-project "$image_project" \
    --tags mobkit-mdm-target
}

internal_ip() {
  "${gcloud_base[@]}" instances describe "$instance" \
    --zone "$GCP_ZONE" \
    --format='get(networkInterfaces[0].networkIP)'
}

sync_meerkat() {
  tar -C "$meerkat_repo" \
    --exclude ./.git \
    --exclude ./target \
    -czf - . | "${ssh_base[@]}" --command "
      set -euo pipefail
      rm -rf ~/meerkat.next
      mkdir -p ~/meerkat.next
      tar -xzf - -C ~/meerkat.next
      rm -rf ~/meerkat
      mv ~/meerkat.next ~/meerkat
    "
}

sync_remote_env() {
  local env_tmp
  env_tmp="$(mktemp)"
  chmod 600 "$env_tmp"
  local names="${MDM_GCP_FORWARD_ENV:-OPENAI_API_KEY ANTHROPIC_API_KEY GOOGLE_API_KEY GEMINI_API_KEY OPENAI_BASE_URL OPENAI_ORG_ID}"
  for env_name in $names; do
    if [[ -n "${!env_name:-}" ]]; then
      printf "%s=" "$env_name" >>"$env_tmp"
      shell_quote "${!env_name}" >>"$env_tmp"
      printf "\n" >>"$env_tmp"
    fi
  done
  "${ssh_base[@]}" --command "mkdir -p ~/.cache/mdm-rkat-target && chmod 700 ~/.cache/mdm-rkat-target"
  "${scp_base[@]}" "$env_tmp" "${ssh_target}:~/.cache/mdm-rkat-target/${id}.env"
  rm -f "$env_tmp"
}

remote_start() {
  local target_listen="$1"
  "${ssh_base[@]}" --command "
    set -euo pipefail
    set -a
    . ~/.cache/mdm-rkat-target/${id}.env
    set +a
    . \"\$HOME/.cargo/env\" 2>/dev/null || true
    if ! command -v cargo >/dev/null 2>&1; then
      curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal
      . \"\$HOME/.cargo/env\"
    fi
    if ! dpkg -s build-essential pkg-config libssl-dev git >/dev/null 2>&1; then
      sudo apt-get update
      sudo apt-get install -y build-essential pkg-config libssl-dev git
    fi
    mkdir -p ~/.cache/mdm-rkat-target/${id}/context ~/.cache/mdm-rkat-target/${id}/state /tmp/mdm-rkat-target
    if [[ -f /tmp/mdm-${id}.pid ]]; then kill \"\$(cat /tmp/mdm-${id}.pid)\" 2>/dev/null || true; fi
    if [[ '${reset_state}' == '1' ]]; then rm -rf ~/.cache/mdm-rkat-target/${id}; mkdir -p ~/.cache/mdm-rkat-target/${id}/context ~/.cache/mdm-rkat-target/${id}/state; fi
    rm -f /tmp/mdm-rkat-target/${id}.rkat.json
    cd ~/meerkat
    CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 cargo build -p rkat
    rkat_bin=\"\$PWD/target/debug/rkat\"
    nohup \"\$rkat_bin\" \
      --realm mdm-${id} \
      --context-root ~/.cache/mdm-rkat-target/${id}/context \
      --state-root ~/.cache/mdm-rkat-target/${id}/state \
      run \
      --model '${model}' \
      --provider '${provider}' \
      --tools full \
      --comms-name '${name}' \
      --comms-listen-tcp '${target_listen}' \
      --comms-binding-out /tmp/mdm-rkat-target/${id}.rkat.json \
      --comms-pairing-password '${pairing_password}' \
      --agent-description '${name} managed target' \
      --agent-label 'site=${site}' \
      --agent-label 'platform=${platform}' \
      --agent-label 'target_kind=mdm' \
      --keep-alive \
      'You are a remote managed target. Use local shell tools to inspect this host when asked.' \
      > /tmp/mdm-${id}.log 2>&1 &
    echo \$! > /tmp/mdm-${id}.pid
    for i in \$(seq 1 180); do
      if test -s /tmp/mdm-rkat-target/${id}.rkat.json && grep -q 'Keep-alive: initial turn complete' /tmp/mdm-${id}.log; then
        exit 0
      fi
      sleep 1
    done
    tail -100 /tmp/mdm-${id}.log >&2
    exit 1
  "
}

fetch_binding() {
  mkdir -p "$bindings_dir"
  local rkat_binding_file="${bindings_dir}/${id}.rkat.json"
  local binding_file="${bindings_dir}/${id}.json"
  "${scp_base[@]}" "${ssh_target}:/tmp/mdm-rkat-target/${id}.rkat.json" "$rkat_binding_file"
  node "${pack_dir}/scripts/wrap-rkat-binding.mjs" \
    "$rkat_binding_file" \
    "$binding_file" \
    "$(node -e 'const [id,name,site,platform,address,password]=process.argv.slice(1); console.log(JSON.stringify({id,name,site,platform,address,pairing_password:password,labels:{target_runtime:"rkat_run",shell:"unrestricted"}}));' "$id" "$name" "$site" "$platform" "$advertise" "$pairing_password")"
  node "${pack_dir}/scripts/merge-bindings.mjs" "$targets_file" "$binding_file" >/dev/null
  echo "[mdm-gcp-target] binding: $binding_file"
  echo "[mdm-gcp-target] targets: $targets_file"
}

case "$command" in
  start)
    ensure_instance
    sync_meerkat
    sync_remote_env
    if [[ -z "$listen" && -z "$advertise" ]]; then
      listen="0.0.0.0:5791"
      advertise="tcp://$(internal_ip):5791"
    elif [[ -n "$listen" && -z "$advertise" ]]; then
      advertise="tcp://${listen}"
    elif [[ -z "$listen" && -n "$advertise" ]]; then
      listen="$(listen_from_tcp_address "$advertise")"
    fi
    remote_start "$listen"
    fetch_binding
    ;;
  fetch)
    [[ -n "$advertise" ]] || advertise="tcp://$(internal_ip):5791"
    fetch_binding
    ;;
  stop)
    "${ssh_base[@]}" --command "if [[ -f /tmp/mdm-${id}.pid ]]; then kill \$(cat /tmp/mdm-${id}.pid) 2>/dev/null || true; rm -f /tmp/mdm-${id}.pid; fi"
    ;;
  *)
    usage
    exit 2
    ;;
esac
