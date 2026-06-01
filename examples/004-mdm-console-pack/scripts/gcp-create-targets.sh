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

for i in $(seq 1 "$GCP_TARGET_COUNT"); do
  name="${GCP_TARGET_PREFIX}-${i}"
  gcloud compute instances create "$name" \
    --project "$GCP_PROJECT" \
    --zone "$GCP_ZONE" \
    --machine-type "${GCP_MACHINE_TYPE:-e2-medium}" \
    --image-family "${GCP_IMAGE_FAMILY:-debian-12}" \
    --image-project "${GCP_IMAGE_PROJECT:-debian-cloud}" \
    --metadata "mdm-kennel-url=${GCP_KENNEL_URL},mdm-target-id=${name}" \
    --tags mobkit-mdm-target
done

echo "Created $GCP_TARGET_COUNT optional MDM target VM(s). Install targetd with scripts/gcp-install-targetd.sh."
