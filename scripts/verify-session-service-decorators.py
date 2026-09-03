#!/usr/bin/env python3
"""Every production `MobSessionService` decorator must name every trait method.

meerkat-mob's `MobSessionService` carries dozens of DEFAULTED methods, and most
defaults are typed refusals (`Unsupported`, `Unavailable`) or downgrades (read
cost `Unsupported`, fork source `None`). A decorator that omits one compiles
cleanly and silently answers the default instead of forwarding. On 2026-09-03
that shape failed a destructive reset in production
("session service cannot run a pre-retire archive hook") and an audit found
nine more such methods in both decorators - see PR #390 / #391.

This gate reads the trait from the pinned meerkat-mob source (via `cargo
metadata`, so it is the exact version Cargo.lock resolves), reads the features
Cargo enables on meerkat-mob (via `cargo tree`), and fails when any PRODUCTION
`impl MobSessionService for X` block in meerkat-mobkit/src lacks a method that
exists under those features. Test doubles inside `#[cfg(test)]` modules are
skipped with the same trap-aware scan as verify-decorator-authority.py.

Required methods are checked too: a missing one is a compile error anyway, but
listing it here keeps the report complete.
"""

from __future__ import annotations

import json
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SRC = ROOT / "meerkat-mobkit" / "src"
TRAIT = "MobSessionService"
IMPL = re.compile(r"^(\s*)impl\s+(?:[\w:]+::)?" + TRAIT + r"\s+for\s+(\$?\w+)")
CFG_TEST = re.compile(r"^\s*#\[cfg\(test\)\]")
MOD_LINE = re.compile(r"^(\s*)mod\s+\w+\s*\{")
FN = re.compile(r"^\s*(?:pub(?:\([^)]*\))?\s+)?(?:async\s+)?fn\s+([a-z_0-9]+)\s*[<(]")
CFG_FEATURE = re.compile(r'#\[cfg\(feature\s*=\s*"([^"]+)"\)\]')


def trait_methods(trait_source: str) -> dict[str, str | None]:
    """Method name -> feature it is gated on (None when unconditional)."""
    m = re.search(r"^pub trait " + TRAIT + r"\b.*?\n\}\n", trait_source, re.S | re.M)
    if not m:
        raise SystemExit(f"error: `pub trait {TRAIT}` not found in the meerkat-mob source")
    methods: dict[str, str | None] = {}
    pending_feature: str | None = None
    depth = 0
    # The header may span several lines (`pub trait X:\n    A + B\n{`); the
    # body starts after the first brace, whatever line it is on.
    body = m.group(0)[m.group(0).index("{") + 1 :]
    for line in body.splitlines():
        stripped = line.strip()
        if depth == 0:
            cfg = CFG_FEATURE.search(stripped)
            if cfg:
                pending_feature = cfg.group(1)
            fn = FN.match(line)
            if fn:
                methods[fn.group(1)] = pending_feature
                pending_feature = None
            elif stripped and not stripped.startswith(("#[", "//", "///")):
                # any other top-level statement resets a dangling cfg
                if not stripped.startswith(("async", "fn", "pub")):
                    pending_feature = None
        depth += line.count("{") - line.count("}")
    return methods


def test_regions(lines: list[str]) -> list[tuple[int, int]]:
    regions: list[tuple[int, int]] = []
    for i, line in enumerate(lines):
        if not CFG_TEST.match(line):
            continue
        j = i + 1
        while j < len(lines) and (lines[j].strip().startswith("#[") or not lines[j].strip()):
            j += 1
        if j >= len(lines) or not MOD_LINE.match(lines[j]):
            continue
        depth = 0
        for k in range(j, len(lines)):
            depth += lines[k].count("{") - lines[k].count("}")
            if depth <= 0 and k > j:
                regions.append((i, k + 1))
                break
        else:
            regions.append((i, len(lines)))
    return regions


def impl_block_end(lines: list[str], start: int) -> int:
    depth = 0
    for k in range(start, len(lines)):
        depth += lines[k].count("{") - lines[k].count("}")
        if depth <= 0 and k > start:
            return k + 1
    return len(lines)


def production_impls(source: str) -> list[tuple[str, set[str]]]:
    """(impl target, method names) for every impl outside `#[cfg(test)]` modules."""
    lines = source.splitlines()
    regions = test_regions(lines)
    found: list[tuple[str, set[str]]] = []
    for i, line in enumerate(lines):
        m = IMPL.match(line)
        if not m:
            continue
        if any(a <= i < b for a, b in regions):
            continue
        end = impl_block_end(lines, i)
        names = {fm.group(1) for l in lines[i:end] if (fm := FN.match(l))}
        found.append((m.group(2), names))
    return found


def missing_methods(
    methods: dict[str, str | None], enabled_features: set[str], impl_names: set[str]
) -> list[str]:
    return sorted(
        name
        for name, feature in methods.items()
        if (feature is None or feature in enabled_features) and name not in impl_names
    )


def meerkat_mob_trait_source() -> str:
    meta = json.loads(
        subprocess.run(
            ["cargo", "metadata", "--format-version", "1", "--locked"],
            check=True,
            capture_output=True,
            cwd=ROOT,
        ).stdout
    )
    pkgs = [p for p in meta["packages"] if p["name"] == "meerkat-mob"]
    if len(pkgs) != 1:
        raise SystemExit(f"error: expected exactly one meerkat-mob package, found {len(pkgs)}")
    manifest = Path(pkgs[0]["manifest_path"])
    path = manifest.parent / "src" / "runtime" / "session_service.rs"
    if not path.is_file():
        raise SystemExit(f"error: meerkat-mob source not found at {path}")
    return path.read_text(encoding="utf-8")


def enabled_meerkat_mob_features() -> set[str]:
    out = subprocess.run(
        ["cargo", "tree", "-p", "meerkat-mobkit", "-e", "features", "-i", "meerkat-mob", "--depth", "1"],
        check=True,
        capture_output=True,
        text=True,
        cwd=ROOT,
    ).stdout
    return set(re.findall(r'meerkat-mob feature "([^"]+)"', out))


MIN_EXPECTED_METHODS = 40  # the trait has ~57 today; a parser that finds fewer is stale, not lucky


def main() -> int:
    methods = trait_methods(meerkat_mob_trait_source())
    if len(methods) < MIN_EXPECTED_METHODS:
        print(
            f"error: parsed only {len(methods)} {TRAIT} methods (expected >= {MIN_EXPECTED_METHODS}); "
            "the trait parser is stale and this gate would pass vacuously",
            file=sys.stderr,
        )
        return 1
    features = enabled_meerkat_mob_features()
    failed = False
    checked = 0
    for path in sorted(SRC.rglob("*.rs")):
        for target, names in production_impls(path.read_text(encoding="utf-8")):
            checked += 1
            missing = missing_methods(methods, features, names)
            if missing:
                failed = True
                print(f"{path.relative_to(ROOT)}: `impl {TRAIT} for {target}` lacks {len(missing)} method(s):")
                for name in missing:
                    print(f"    - {name}")
    if checked == 0:
        print(f"error: found NO production `impl {TRAIT}` blocks; the pattern is stale", file=sys.stderr)
        return 1
    if failed:
        print(
            "a decorator that omits a defaulted method answers the trait default (a refusal or a downgrade) "
            "instead of forwarding; add the forward",
            file=sys.stderr,
        )
        return 1
    print(f"ok: {checked} production {TRAIT} impl(s) name all {len([m for m,f in methods.items() if f is None or f in features])} reachable trait methods")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
