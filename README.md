# MobKit

[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](LICENSE-MIT)
[![Rust: 1.85+](https://img.shields.io/badge/rust-1.85%2B-orange.svg)](https://www.rust-lang.org)

Companion orchestration platform for the
[Meerkat](https://github.com/lukacf/meerkat) multi-agent runtime.
Handles startup orchestration, module routing, operational subsystems,
session persistence, and admin console.

## Key Paths

| Area | Path |
|------|------|
| Rust core | `crates/meerkat-mobkit-core/` |
| Python SDK | `sdk/python/meerkat_mobkit/` |
| TypeScript SDK | `sdk/typescript/` |
| Docs (Mintlify) | `docs/` |

## Quick Start (Python SDK)

Install the package, then connect to a running MobKit instance:

```python
from meerkat_mobkit import MobKit

async with await MobKit.builder().mob("mob.toml").build() as rt:
    handle = rt.mob_handle()
    status = await handle.status()
    print(f"Running: {status.running}, modules: {status.loaded_modules}")
```

## Build and Test

### Rust

```bash
cargo check --workspace
cargo nextest run --workspace -E 'not test(phase0_governance)' --no-fail-fast
```

### Python

```bash
PYTHONPATH=sdk/python python3 -m pytest sdk/python/tests/ -v
```

## Branch Conventions

| Branch pattern | Purpose |
|----------------|---------|
| `main` | Stable -- PRs merge here |
| `feat/<name>` | New features |
| `fix/<name>` | Bug fixes |
| `docs/<name>` | Documentation changes |
| `refactor/<name>` | Refactoring |

## License

Dual-licensed under MIT and Apache 2.0.
See [LICENSE-MIT](LICENSE-MIT) and [LICENSE-APACHE](LICENSE-APACHE).
