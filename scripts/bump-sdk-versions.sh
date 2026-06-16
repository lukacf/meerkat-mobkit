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

# Bazel build rules embed the crate version via rustc_env, which feeds
# env!("CARGO_PKG_VERSION") in the bazel-built RELEASE binaries (e.g. their
# `--version` output). Keep it in lockstep so released gateways don't report a
# stale version (this drifted to 0.7.4 across 0.7.5–0.7.7 before being caught).
if [ -f "$ROOT/meerkat-mobkit/BUILD.bazel" ]; then
    sedi -E "s/\"CARGO_PKG_VERSION\": \"[^\"]*\"/\"CARGO_PKG_VERSION\": \"$VERSION\"/g" "$ROOT/meerkat-mobkit/BUILD.bazel"
    echo "  Bazel BUILD.bazel CARGO_PKG_VERSION: $VERSION"
fi

echo "Done"
