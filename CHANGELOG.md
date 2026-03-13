# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/),
and this project adheres to [Semantic Versioning](https://semver.org/).

## [Unreleased]

## [0.4.8] - 2026-03-13

First public release. Version aligned with Meerkat v0.4.8.

### Added

- Rust crate `meerkat-mobkit` published to crates.io
- Python SDK `meerkat-mobkit` published to PyPI
- TypeScript SDK `@rkat/mobkit-sdk` published to npm (full Python SDK parity)
- Gateway binary builds for 5 platforms (linux x86/arm, macOS x86/arm, Windows)
- CI/CD pipeline (GitHub Actions: fmt, clippy, test, audit, release)
- Release workflow with automated registry publishing
- Comprehensive clippy lint config (pedantic + deny unwrap/expect/panic)
- Pre-commit hooks (fmt on commit, gitleaks + clippy + tests on push)
- cargo-deny security auditing
- Version parity scripts across Rust, Python, and TypeScript
- Documentation site with architecture overview, quickstart, API reference
- MobKit logo and architecture diagram

### Changed

- Crate renamed from `meerkat-mobkit-core` to `meerkat-mobkit`
- Crate layout flattened (`crates/meerkat-mobkit-core/` → `meerkat-mobkit/`)
- TypeScript SDK renamed from `@meerkat/mobkit-sdk` to `@rkat/mobkit-sdk`
- Edition upgraded to 2024, rust-version to 1.94.0
- Meerkat dependencies bumped to 0.4.8 (resolved from crates.io)
- `spawn_many` now runs concurrently via `futures::try_join_all`
- RPC mob handlers extracted to `rpc/mob_methods.rs`
- Event log type aliases (`EventLogError`, `EventFilter`)
- Python SDK: `ensure_member()` and `find_members()` return typed `MemberSnapshot`
- Python SDK: `send()` returns `SendMessageResult` with `session_id`

## [0.4.6] - 2026-03-11

Initial internal release. Version aligned with Meerkat v0.4.6.

### Added

- Rust core orchestration engine
  - Unified runtime with module loading, mob lifecycle, and RPC gateway
  - Roster API: list, get, retire, and respawn mob members
  - Routing engine with wildcard matching and retry policies
  - Delivery subsystem with history tracking
  - Gating framework for risk-tiered action approval
  - Memory stores (knowledge graph, vector, timeline, todo, top-of-mind)
  - Session persistence with BigQuery adapter
  - Scheduling engine with cron and interval evaluation
  - Persistent operational event log
  - SSE event streaming for agent and mob observation
  - JWT/JWKS authentication with OIDC discovery
  - Admin console REST API
- Python SDK
  - Builder pattern for runtime configuration
  - Typed `MobHandle` with 30+ methods covering all RPC operations
  - Typed result models for all API responses
  - SSE bridge for real-time event streaming
  - ASGI app for serving the runtime over HTTP
  - Session agent builder protocol for callback-driven agents
  - Error event hooks for operational alerting
- Admin console (React)
- Mintlify documentation site
