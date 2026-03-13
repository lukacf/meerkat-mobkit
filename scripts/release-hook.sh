#!/usr/bin/env bash
# Pre-release hook for cargo-release.
# Called with the new version as the first argument.
# Bumps SDK versions and stages the changes for the release commit.

set -euo pipefail

VERSION="${1:?usage: release-hook.sh <version>}"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"

# Prevent duplicate execution within the same release
SENTINEL="$ROOT/.release-hook-done"
if [ -f "$SENTINEL" ] && [ "$(cat "$SENTINEL")" = "$VERSION" ]; then
  echo "release-hook: already ran for $VERSION, skipping"
  exit 0
fi

echo "release-hook: bumping SDK versions to $VERSION"
"$ROOT/scripts/bump-sdk-versions.sh" "$VERSION"

echo "release-hook: verifying version parity"
"$ROOT/scripts/verify-version-parity.sh"

echo "release-hook: staging SDK version files"
git add \
  "$ROOT/sdk/python/pyproject.toml" \
  "$ROOT/sdk/typescript/package.json"

echo "$VERSION" > "$SENTINEL"
echo "release-hook: done ($VERSION)"
