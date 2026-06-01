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

gcloud compute instances delete "${GCP_TARGET_PREFIX}-kennel" \
  --project "$GCP_PROJECT" \
  --zone "$GCP_ZONE" \
  --quiet || true

for i in $(seq 1 "$GCP_TARGET_COUNT"); do
  name="${GCP_TARGET_PREFIX}-${i}"
  gcloud compute instances delete "$name" \
    --project "$GCP_PROJECT" \
    --zone "$GCP_ZONE" \
    --quiet || true
done

gcloud compute firewall-rules delete "${GCP_TARGET_PREFIX}-mdm-internal" \
  --project "$GCP_PROJECT" \
  --quiet || true
