#!/usr/bin/env bash
# Bump Python SDK (and TypeScript SDK if present) version to match Cargo workspace.
# Usage: ./scripts/bump-sdk-versions.sh [VERSION]
# If VERSION is omitted, reads from Cargo.toml workspace.package.version.

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"

if [ $# -ge 1 ]; then
    VERSION="$1"
else
    VERSION=$(cargo metadata --manifest-path "$ROOT/Cargo.toml" \
        --no-deps --format-version 1 \
        | jq -r '.packages[] | select(.name == "meerkat-mobkit") | .version')
fi

echo "Bumping SDK versions to $VERSION"

# Portable sed -i (macOS vs GNU)
sedi() {
    if sed --version >/dev/null 2>&1; then
        sed -i "$@"
    else
        sed -i '' "$@"
    fi
}

# Python SDK
sedi "s/^version = \".*\"/version = \"$VERSION\"/" "$ROOT/sdk/python/pyproject.toml"
echo "  Python SDK: $VERSION"

# TypeScript SDK (if present)
if [ -f "$ROOT/sdk/typescript/package.json" ]; then
    node -e "
const fs = require('fs');
const p = '$ROOT/sdk/typescript/package.json';
const pkg = JSON.parse(fs.readFileSync(p, 'utf8'));
pkg.version = '$VERSION';
fs.writeFileSync(p, JSON.stringify(pkg, null, 2) + '\n');
"
    echo "  TypeScript SDK: $VERSION"
fi

echo "Done"
