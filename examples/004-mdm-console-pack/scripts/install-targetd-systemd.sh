#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

env_file="${1:-deploy/mdm-targetd.env.example}"
install_root="${MDM_INSTALL_ROOT:-/opt/meerkat-mobkit}"

if [[ ! -f "$env_file" ]]; then
  echo "Missing target env file: $env_file" >&2
  exit 2
fi

sudo install -d -m 0755 /etc/mdm-targetd /var/lib/mdm-targetd
sudo install -m 0600 "$env_file" /etc/mdm-targetd/mdm-targetd.env
sudo install -m 0644 deploy/mdm-targetd.systemd.service /etc/systemd/system/mdm-targetd.service
sudo systemctl daemon-reload

echo "Installed mdm-targetd systemd unit."
echo "Repo/runtime expected at $install_root/examples."
echo "Start with: sudo systemctl enable --now mdm-targetd"
