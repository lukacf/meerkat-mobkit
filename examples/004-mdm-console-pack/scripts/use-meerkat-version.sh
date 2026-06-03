#!/usr/bin/env bash
set -euo pipefail

pack_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
repo_root="$(cd "${pack_dir}/../.." && pwd)"
version="${1:-}"

if [[ -z "$version" ]]; then
  echo "usage: use-meerkat-version.sh <version>" >&2
  echo "example: use-meerkat-version.sh 0.6.33" >&2
  exit 2
fi

crate_manifest="${repo_root}/meerkat-mobkit/Cargo.toml"
crates=(
  meerkat
  meerkat-core
  meerkat-client
  meerkat-contracts
  meerkat-mob
  meerkat-mob-mcp
  meerkat-mcp
  meerkat-models
  meerkat-runtime
  meerkat-session
  meerkat-store
  meerkat-tools
  meerkat-comms
)

if ! cargo search meerkat --limit 1 | grep -q "meerkat = \"${version}\""; then
  echo "meerkat ${version} is not visible on crates.io yet" >&2
  exit 1
fi

for crate in "${crates[@]}"; do
  perl -0pi -e "s/(${crate}\\s*=\\s*\\{\\s*version\\s*=\\s*)\"[^\"]+\"/\${1}\"${version}\"/g; s/(${crate}\\s*=\\s*)\"[^\"]+\"/\${1}\"${version}\"/g" "$crate_manifest"
done

cd "$repo_root"
for crate in "${crates[@]}"; do
  ./scripts/repo-cargo update -p "$crate" --precise "$version"
done

echo "[mdm] Meerkat dependencies pinned to ${version}"
