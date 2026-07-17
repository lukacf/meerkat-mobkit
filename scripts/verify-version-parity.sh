#!/usr/bin/env bash
# Verify version parity across Rust, Bazel, Python, and TypeScript packages.
# Exit 0 if everything is in sync, exit 1 with diagnostics on any mismatch.

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
CARGO="${ROOT}/scripts/repo-cargo"
FAIL=0

red()    { printf '\033[0;31m%s\033[0m\n' "$*"; }
green()  { printf '\033[0;32m%s\033[0m\n' "$*"; }

# 1. Package version parity

CARGO_VER=$("$CARGO" metadata --manifest-path "$ROOT/Cargo.toml" \
    --no-deps --format-version 1 \
    | jq -r '.packages[] | select(.name == "meerkat-mobkit") | .version')

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

MODULE_VER=""
if [ -f "$ROOT/MODULE.bazel" ]; then
    MODULE_VER=$(sed -n '/^module(/,/^)/s/.*version = "\([^"]*\)".*/\1/p' "$ROOT/MODULE.bazel")
fi

echo "Package versions:"
echo "  Cargo (meerkat-mobkit):  $CARGO_VER"
echo "  Python SDK:                   $PY_VER"
if [ -n "$TS_VER" ]; then
    echo "  TypeScript SDK:               $TS_VER"
fi
if [ -n "$MODULE_VER" ]; then
    echo "  Bazel module:                 $MODULE_VER"
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
if [ -n "$MODULE_VER" ] && [ "$CARGO_VER" != "$MODULE_VER" ]; then
    red "FAIL: Bazel module version mismatch ($MODULE_VER != $CARGO_VER)"
    PKG_OK=false
    FAIL=1
fi
if $PKG_OK; then
    green "  Package versions: OK"
fi

# 2. Bazel rustc_env parity — drives env!("CARGO_PKG_VERSION") in the
# bazel-built RELEASE binaries (and their --version). Drifted to 0.7.4 across
# 0.7.5–0.7.7 because nothing checked it; now it does.
BAZEL_FILE="$ROOT/meerkat-mobkit/BUILD.bazel"
if [ -f "$BAZEL_FILE" ]; then
    BAD_BAZEL=$(grep -oE '"CARGO_PKG_VERSION": "[^"]*"' "$BAZEL_FILE" \
        | grep -v "\"$CARGO_VER\"" | sort -u || true)
    if [ -n "$BAD_BAZEL" ]; then
        red "FAIL: BUILD.bazel CARGO_PKG_VERSION entries != $CARGO_VER:"
        printf '%s\n' "$BAD_BAZEL"
        FAIL=1
    else
        green "  BUILD.bazel CARGO_PKG_VERSION: OK"
    fi
fi

# 3. TypeScript SDK lockfile root version parity.
LOCK_FILE="$ROOT/sdk/typescript/package-lock.json"
if [ -f "$LOCK_FILE" ]; then
    LOCK_VER=$(node -p "require('$LOCK_FILE').version" 2>/dev/null || echo "")
    if [ -n "$LOCK_VER" ] && [ "$LOCK_VER" != "$CARGO_VER" ]; then
        red "FAIL: package-lock.json version mismatch ($LOCK_VER != $CARGO_VER)"
        FAIL=1
    else
        green "  package-lock.json version: OK"
    fi
fi

echo ""
if [ $FAIL -ne 0 ]; then
    red "Version parity check FAILED"
    exit 1
else
    green "All version parity checks passed"
fi
