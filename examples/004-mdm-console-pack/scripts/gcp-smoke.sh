#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

env_file="${MDM_GCP_ENV:-.env.gcp}"
if [[ ! -f "$env_file" ]]; then
  echo "Missing $env_file. Copy deploy/gcp.env.example to $env_file and fill it locally." >&2
  exit 2
fi
set -a
source "$env_file"
set +a

: "${GCP_PROJECT:?}"
: "${GCP_ZONE:?}"
: "${GCP_TARGET_PREFIX:?}"
: "${GCP_TARGET_COUNT:?}"
: "${MDM_AUTH_TOKEN:?}"
: "${MDM_TARGET_AUTH_TOKEN:?}"

repo_root="$(cd ../.. && pwd)"
kennel_name="${GCP_TARGET_PREFIX}-kennel"
machine_type="${GCP_MACHINE_TYPE:-e2-medium}"
image_family="${GCP_IMAGE_FAMILY:-debian-12}"
image_project="${GCP_IMAGE_PROJECT:-debian-cloud}"
archive="$(mktemp -t mobkit-mdm-gcp.XXXXXX.tar.gz)"

target_names=()
for i in $(seq 1 "$GCP_TARGET_COUNT"); do
  target_names+=("${GCP_TARGET_PREFIX}-${i}")
done

cleanup_archive() {
  rm -f "$archive"
}
trap cleanup_archive EXIT

echo "[mdm-gcp-smoke] creating worktree archive"
tar -czf "$archive" \
  --exclude .git \
  --exclude target \
  --exclude node_modules \
  --exclude 'examples/node_modules' \
  --exclude 'examples/004-mdm-console-pack/.state' \
  --exclude 'examples/004-mdm-console-pack/.target-state' \
  -C "$repo_root" .

ensure_instance() {
  local name="$1"
  local tag="$2"
  if gcloud compute instances describe "$name" --project "$GCP_PROJECT" --zone "$GCP_ZONE" >/dev/null 2>&1; then
    return
  fi
  gcloud compute instances create "$name" \
    --project "$GCP_PROJECT" \
    --zone "$GCP_ZONE" \
    --machine-type "$machine_type" \
    --image-family "$image_family" \
    --image-project "$image_project" \
    --tags "$tag"
}

internal_ip() {
  gcloud compute instances describe "$1" \
    --project "$GCP_PROJECT" \
    --zone "$GCP_ZONE" \
    --format='value(networkInterfaces[0].networkIP)'
}

ensure_firewall() {
  local rule="${GCP_TARGET_PREFIX}-mdm-internal"
  if gcloud compute firewall-rules describe "$rule" --project "$GCP_PROJECT" >/dev/null 2>&1; then
    return
  fi
  gcloud compute firewall-rules create "$rule" \
    --project "$GCP_PROJECT" \
    --allow tcp:5788,tcp:5792 \
    --source-ranges "${GCP_INTERNAL_SOURCE_RANGE:-10.0.0.0/8}" \
    --target-tags mobkit-mdm-kennel,mobkit-mdm-target
}

remote_install_base() {
  local name="$1"
  gcloud compute scp "$archive" "$name:/tmp/mobkit-mdm.tgz" \
    --project "$GCP_PROJECT" \
    --zone "$GCP_ZONE" \
    --quiet
  gcloud compute ssh "$name" \
    --project "$GCP_PROJECT" \
    --zone "$GCP_ZONE" \
    --quiet \
    --command "set -euo pipefail
      sudo apt-get update
      sudo apt-get install -y curl ca-certificates
      if ! command -v node >/dev/null 2>&1; then
        curl -fsSL https://deb.nodesource.com/setup_22.x | sudo -E bash -
        sudo apt-get install -y nodejs
      fi
      sudo rm -rf /opt/meerkat-mobkit
      sudo mkdir -p /opt/meerkat-mobkit /var/lib/mdm-targetd
      sudo chown -R \$USER:\$USER /opt/meerkat-mobkit /var/lib/mdm-targetd
      tar -xzf /tmp/mobkit-mdm.tgz -C /opt/meerkat-mobkit
      cd /opt/meerkat-mobkit/examples
      npm install"
}

echo "[mdm-gcp-smoke] creating kennel and target VM(s)"
ensure_firewall
ensure_instance "$kennel_name" mobkit-mdm-kennel
for target_name in "${target_names[@]}"; do
  ensure_instance "$target_name" mobkit-mdm-target
done

kennel_ip="$(internal_ip "$kennel_name")"
kennel_url="http://${kennel_ip}:5788"

echo "[mdm-gcp-smoke] syncing current checkout"
remote_install_base "$kennel_name"
for target_name in "${target_names[@]}"; do
  remote_install_base "$target_name"
done

echo "[mdm-gcp-smoke] starting kennel at $kennel_url"
gcloud compute ssh "$kennel_name" \
  --project "$GCP_PROJECT" \
  --zone "$GCP_ZONE" \
  --quiet \
  --command "set -euo pipefail
    if [[ -f /tmp/mdm-kennel.pid ]]; then
      kill \"\$(cat /tmp/mdm-kennel.pid)\" 2>/dev/null || true
    fi
    cd /opt/meerkat-mobkit
    nohup env MDM_AUTH_TOKEN='${MDM_AUTH_TOKEN}' MDM_TARGET_AUTH_TOKEN='${MDM_TARGET_AUTH_TOKEN}' \
      npx --yes --package tsx@4.20.5 --package yaml@2.8.1 \
      tsx examples/004-mdm-console-pack/run.ts \
        --api-only --wait --api-listen 0.0.0.0:5788 --expect-targets ${GCP_TARGET_COUNT} --require-auth \
      > /tmp/mdm-kennel.log 2>&1 &
    echo \$! > /tmp/mdm-kennel.pid"

echo "[mdm-gcp-smoke] starting targets"
for target_name in "${target_names[@]}"; do
  target_ip="$(internal_ip "$target_name")"
  gcloud compute ssh "$target_name" \
    --project "$GCP_PROJECT" \
    --zone "$GCP_ZONE" \
    --quiet \
    --command "set -euo pipefail
      if [[ -f /tmp/mdm-targetd.pid ]]; then
        kill \"\$(cat /tmp/mdm-targetd.pid)\" 2>/dev/null || true
      fi
      cd /opt/meerkat-mobkit
      nohup npx --yes --package tsx@4.20.5 \
        tsx examples/004-mdm-console-pack/src/targetd.ts \
          --id '${target_name}' --name '${target_name}' --site gcp --platform linux-vm \
          --transport control --listen 0.0.0.0:5792 --advertise-url 'http://${target_ip}:5792' \
          --kennel '${kennel_url}' --state-dir /var/lib/mdm-targetd \
          --kennel-auth-token '${MDM_AUTH_TOKEN}' --control-auth-token '${MDM_TARGET_AUTH_TOKEN}' --allow-shell \
        > /tmp/mdm-targetd.log 2>&1 &
      echo \$! > /tmp/mdm-targetd.pid"
done

echo "[mdm-gcp-smoke] waiting for registration"
for _ in $(seq 1 120); do
  body="$(gcloud compute ssh "$kennel_name" \
    --project "$GCP_PROJECT" \
    --zone "$GCP_ZONE" \
    --quiet \
    --command "curl -fsS -H 'authorization: Bearer ${MDM_AUTH_TOKEN}' http://127.0.0.1:5788/api/targets" 2>/dev/null || true)"
  ready=1
  for target_name in "${target_names[@]}"; do
    if [[ "$body" != *"$target_name"* ]]; then
      ready=0
    fi
  done
  if [[ "$ready" == "1" ]]; then
    break
  fi
  sleep 2
done

first_target="${target_names[0]}"
turn="$(gcloud compute ssh "$kennel_name" \
  --project "$GCP_PROJECT" \
  --zone "$GCP_ZONE" \
  --quiet \
  --command "curl -fsS -X POST http://127.0.0.1:5788/api/targets/${first_target}/turn \
    -H 'authorization: Bearer ${MDM_AUTH_TOKEN}' \
    -H 'content-type: application/json' \
    -d '{\"operator\":\"gcp-smoke\",\"prompt\":\"shell: echo MOBKIT_MDM_GCP_SMOKE\"}'")"

if [[ "$turn" != *"MOBKIT_MDM_GCP_SMOKE"* ]]; then
  echo "GCP remote target turn did not return marker" >&2
  echo "$turn" >&2
  exit 1
fi

echo "[mdm-gcp-smoke] ok"
