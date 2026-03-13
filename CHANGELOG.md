# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/),
and this project adheres to [Semantic Versioning](https://semver.org/).

## [0.4.6] - 2026-03-11

Version aligned with Meerkat v0.4.6.

### Added

- Rust core orchestration engine (`meerkat-mobkit-core`)
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
- Python SDK (`meerkat-mobkit`)
  - Builder pattern for runtime configuration
  - Typed `MobHandle` with 30+ methods covering all RPC operations
  - Typed result models for all API responses
  - SSE bridge for real-time event streaming
  - ASGI app for serving the runtime over HTTP
  - Session agent builder protocol for callback-driven agents
  - Error event hooks for operational alerting
- TypeScript SDK (minimal, parity test artifacts)
- Admin console (React)
- Mintlify documentation site
