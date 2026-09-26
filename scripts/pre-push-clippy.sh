#!/usr/bin/env bash
# Pre-push clippy gate: lint only changed crates instead of the full workspace.
# Falls back to workspace clippy when root Cargo.toml/Cargo.lock changes.
set -euo pipefail

# Incremental compilation is OFF for the push gates.
#
# rustc's incremental cache is a per-worktree, cross-invocation artifact, and a
# stale or half-written one makes the compiler abort with an internal error
# rather than a diagnostic:
#
#   error: internal compiler error: encountered incremental compilation error
#          with evaluate_obligation(...)
#   Found unstable fingerprints ... compiler/rustc_middle/src/verify_ich.rs
#
# It reproduces across runs, so it reads as a deterministic failure in the code
# being pushed, and it is not — `cargo clean` clears it. Gates are the wrong
# place to carry that risk: they run rarely, usually against a cache that has
# been idle or written by a different toolchain, so incremental buys little and
# can block a push on a compiler bug unrelated to the change. Interactive
# builds still use it; only the gates opt out.
export CARGO_INCREMENTAL=0

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
CARGO="${SCRIPT_DIR}/repo-cargo"

# Determine the remote tracking ref to diff against.
UPSTREAM=$(git rev-parse --abbrev-ref '@{upstream}' 2>/dev/null || true)
if [ -z "$UPSTREAM" ]; then
  UPSTREAM="origin/$(git rev-parse --abbrev-ref HEAD)"
fi
MERGE_BASE=$(git merge-base "$UPSTREAM" HEAD 2>/dev/null || echo "")
if [ -z "$MERGE_BASE" ]; then
  echo "No merge base with $UPSTREAM; running full workspace clippy."
  "$CARGO" clippy --workspace --all-targets -- -D warnings
  exit $?
fi

# Parse manifests without dependency resolution or an extra Cargo invocation.
exec python3 - "$MERGE_BASE" "$CARGO" <<'PY'
import os
from pathlib import Path
import subprocess
import sys
import tomllib

merge_base, cargo = sys.argv[1:]
root = Path(subprocess.check_output(
    ["git", "rev-parse", "--show-toplevel"], text=True,
).strip())
# Disable rename folding so moving a source checks both affected packages.
changed = {
    os.fsdecode(path)
    for path in subprocess.check_output([
        "git", "diff", "--name-only", "--no-renames", "-z", f"{merge_base}..HEAD",
    ]).split(b"\0") if path
}

if changed & {"Cargo.toml", "Cargo.lock"}:
    print("Workspace manifest or lockfile changed - running full workspace clippy.", flush=True)
    flags = ["--workspace"]
else:
    packages = set()
    manifests = {}
    for path in changed:
        if Path(path).suffix not in {".rs", ".toml"}:
            continue
        directory = (root / path).parent
        # The file and its parent directory may have been deleted.
        while directory.is_relative_to(root):
            manifest = directory / "Cargo.toml"
            if manifest.is_file():
                if manifest not in manifests:
                    with manifest.open("rb") as source:
                        manifests[manifest] = tomllib.load(source)
                package = manifests[manifest].get("package")
                if package is not None:
                    name = package["name"]
                    if not isinstance(name, str) or not name:
                        raise ValueError(f"Invalid package name in {manifest}")
                    packages.add(name)
                    break
            directory = directory.parent

    if not packages:
        print("No changed Rust packages, skipping clippy.")
        sys.exit(0)
    print("Clippy on changed packages: " + ", ".join(sorted(packages)), flush=True)
    flags = [flag for package in sorted(packages) for flag in ("-p", package)]

os.execv(cargo, [cargo, "clippy", *flags, "--all-targets", "--", "-D", "warnings"])
PY
