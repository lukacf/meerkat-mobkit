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
: "${GCP_KENNEL_URL:?}"
: "${MDM_AUTH_TOKEN:?}"
: "${MDM_TARGET_AUTH_TOKEN:?}"

for i in $(seq 1 "$GCP_TARGET_COUNT"); do
  name="${GCP_TARGET_PREFIX}-${i}"
  gcloud compute ssh "$name" \
    --project "$GCP_PROJECT" \
    --zone "$GCP_ZONE" \
    --command "set -euo pipefail
      sudo apt-get update
      sudo apt-get install -y git curl ca-certificates
      if ! command -v node >/dev/null 2>&1; then
        curl -fsSL https://deb.nodesource.com/setup_22.x | sudo -E bash -
        sudo apt-get install -y nodejs
      fi
      sudo mkdir -p /opt/meerkat-mobkit /var/lib/mdm-targetd
      sudo chown -R \$USER:\$USER /opt/meerkat-mobkit /var/lib/mdm-targetd
      if [[ ! -d /opt/meerkat-mobkit/.git ]]; then
        git clone https://github.com/lukacf/meerkat-mobkit /opt/meerkat-mobkit
      else
        git -C /opt/meerkat-mobkit pull --ff-only
      fi
      cd /opt/meerkat-mobkit/examples
      npm install
      advertise_host=\$(hostname -I | awk '{print \$1}')
      nohup npx tsx 004-mdm-console-pack/src/targetd.ts \
        --id ${name} --name ${name} --site gcp --platform linux-vm \
        --transport control --listen 0.0.0.0:5792 \
        --advertise-url http://\${advertise_host}:5792 \
        --kennel ${GCP_KENNEL_URL} --state-dir /var/lib/mdm-targetd \
        --kennel-auth-token ${MDM_AUTH_TOKEN} \
        --control-auth-token ${MDM_TARGET_AUTH_TOKEN} \
        --allow-shell > /tmp/mdm-targetd.log 2>&1 &"
done

echo "Installed optional GCP target daemons."
