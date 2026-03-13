#!/usr/bin/env bash
# Verify version parity across Rust workspace, Python SDK, and TypeScript SDK.
# Exit 0 if everything is in sync, exit 1 with diagnostics on any mismatch.

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
FAIL=0

red()    { printf '\033[0;31m%s\033[0m\n' "$*"; }
green()  { printf '\033[0;32m%s\033[0m\n' "$*"; }

# 1. Package version parity

CARGO_VER=$(cargo metadata --manifest-path "$ROOT/Cargo.toml" \
    --no-deps --format-version 1 \
    | jq -r '.packages[] | select(.name == "meerkat-mobkit-core") | .version')

PY_VER=$(python3 -c "
import pathlib
try:
    import tomllib
except ModuleNotFoundError:
    import tomli as tomllib
d = tomllib.loads(pathlib.Path('$ROOT/sdk/python/pyproject.toml').read_text())
print(d['project']['version'])
")

TS_VER=""
if [ -f "$ROOT/sdk/typescript/package.json" ]; then
    TS_VER=$(node -p "require('$ROOT/sdk/typescript/package.json').version")
fi

echo "Package versions:"
echo "  Cargo (meerkat-mobkit-core):  $CARGO_VER"
echo "  Python SDK:                   $PY_VER"
if [ -n "$TS_VER" ]; then
    echo "  TypeScript SDK:               $TS_VER"
fi

PKG_OK=true
if [ "$CARGO_VER" != "$PY_VER" ]; then
    red "FAIL: Python SDK version mismatch ($PY_VER != $CARGO_VER)"
    PKG_OK=false
    FAIL=1
fi
if [ -n "$TS_VER" ] && [ "$CARGO_VER" != "$TS_VER" ]; then
    red "FAIL: TypeScript SDK version mismatch ($TS_VER != $CARGO_VER)"
    PKG_OK=false
    FAIL=1
fi
if $PKG_OK; then
    green "  Package versions: OK"
fi

echo ""
if [ $FAIL -ne 0 ]; then
    red "Version parity check FAILED"
    exit 1
else
    green "All version parity checks passed"
fi
