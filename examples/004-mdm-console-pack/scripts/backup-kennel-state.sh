#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

state_dir="${MDM_STATE_DIR:-.state}"
backup_dir="${MDM_BACKUP_DIR:-.backups}"
stamp="$(date -u +%Y%m%dT%H%M%SZ)"
archive="${backup_dir}/mdm-kennel-state-${stamp}.tar.gz"

mkdir -p "$backup_dir"

if [[ ! -d "$state_dir" ]]; then
  echo "Missing state directory: $state_dir" >&2
  exit 2
fi

tar -czf "$archive" \
  -C "$state_dir" \
  kennel-state.json \
  contacts.generated.toml \
  mobkit_console.sqlite \
  mobkit_metadata.sqlite \
  runtime.sqlite 2>/dev/null || {
    echo "Backup failed. Ensure the kennel has run and state files exist in $state_dir." >&2
    exit 1
  }

echo "$archive"
