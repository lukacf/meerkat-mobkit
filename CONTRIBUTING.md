# Contributing to MobKit

Thank you for considering contributing to MobKit.

## Development Setup

### Prerequisites

- Rust 1.85+ (edition 2024)
- Python 3.10+
- Node.js 18+ (for console and TypeScript SDK)

### Building

```bash
# Rust
cargo check --workspace
cargo build --workspace

# Python (no build step — pure Python)
PYTHONPATH=sdk/python python3 -c "import meerkat_mobkit; print('OK')"
```

### Testing

```bash
# Rust
cargo nextest run --workspace -E 'not test(phase0_governance)' --no-fail-fast

# Python
PYTHONPATH=sdk/python python3 -m pytest sdk/python/tests/ -v
```

## Branch Conventions

- `main` — stable, PRs merge here
- `feat/<name>` — new features
- `fix/<name>` — bug fixes
- `docs/<name>` — documentation
- `refactor/<name>` — non-functional refactoring

## Pull Requests

1. Create a feature branch from `main`
2. Make your changes with clear commit messages
3. Ensure all tests pass (`cargo nextest run`, `pytest`)
4. Open a PR against `main` with a description of changes

## Code Style

- **Rust**: `cargo fmt` for formatting, `cargo clippy` for lints
- **Python**: Type annotations required on all public functions

## License

By contributing, you agree that your contributions will be dual-licensed
under the MIT and Apache 2.0 licenses.
