#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

env_file="${1:-deploy/mdm-targetd.env.example}"
plist="/Library/LaunchDaemons/com.meerkat.mdm-targetd.plist"

if [[ ! -f "$env_file" ]]; then
  echo "Missing target env file: $env_file" >&2
  exit 2
fi

sudo install -d -m 0755 /etc/mdm-targetd /var/lib/mdm-targetd
sudo install -m 0600 "$env_file" /etc/mdm-targetd/mdm-targetd.env
sudo install -m 0644 deploy/com.meerkat.mdm-targetd.plist "$plist"
sudo launchctl bootstrap system "$plist" 2>/dev/null || true
sudo launchctl enable system/com.meerkat.mdm-targetd

echo "Installed mdm-targetd launchd daemon."
echo "Start with: sudo launchctl kickstart -k system/com.meerkat.mdm-targetd"
