# MobKit

Companion orchestration platform for the [Meerkat](https://github.com/lukacf/meerkat) multi-agent runtime. Handles startup orchestration, module routing, operational subsystems, session persistence, and admin console.

## Key paths

| Area | Path |
|------|------|
| Rust crate | `meerkat-mobkit/` |
| Gateway binary | `meerkat-mobkit/src/bin/mobkit_gateway.rs` |
| Python SDK | `sdk/python/meerkat_mobkit/` |
| Python tests | `sdk/python/tests/` |
| TypeScript SDK | `sdk/typescript/` |
| Docs (Mintlify) | `docs/` |

## Python SDK (v0.4.6)

Package: `meerkat-mobkit` (import as `meerkat_mobkit`).

Public surface — `__init__.py` exports:
- **Builder/Runtime**: `MobKit`, `MobKitBuilder`, `MobKitRuntime`, `ToolCaller`
- **Models**: `DiscoverySpec`, `PreSpawnData`, `SessionBuildOptions`, `SessionQuery`
- **Protocol**: `SessionAgentBuilder`
- **Errors**: `MobKitError`, `TransportError`, `RpcError`, `NotConnectedError`, `CapabilityUnavailableError`, `ContractMismatchError`, `WorkGraphUnavailableError`, `WorkGraphConflictError`
- **Typed results**: `StatusResult`, `CapabilitiesResult`, `ReconcileResult`, `SpawnResult`, `SpawnMemberResult`, `SendMessageResult`, `SubscribeResult`, `KeepAliveConfig`, `EventEnvelope`, `RoutingResolution`, `DeliveryResult`, `DeliveryHistoryResult`, `MemoryQueryResult`, `MemoryStoreInfo`, `MemoryIndexResult`, `MemberSnapshot`, `RuntimeRouteResult`, `GatingEvaluateResult`, `GatingDecisionResult`, `GatingAuditEntry`, `GatingPendingEntry`, `CallToolResult`, `WorkGraphItem`, `WorkGraphEdge`, `WorkGraphAttentionBinding`, `WorkGraphSnapshotResult`, `WorkGraphItemsResult`, `WorkGraphGoalResult`, `WorkGraphAttentionReassignResult`, `WorkGraphEventEntry`
- **Events**: `MobEvent`, `AgentEvent`, `EventStream`
- **Config**: `auth`, `memory`, `session_store`
- **Constants**: `MEMBER_STATE_ACTIVE`, `MEMBER_STATE_RETIRING`

Module authoring helpers (`ModuleSpec`, `define_module`, etc.) live in `meerkat_mobkit.helpers` — not top-level.

Private internals (underscore-prefixed): `_client.py`, `_transport.py`, `_sse.py`.

## Build and test

All Rust commands go through `scripts/repo-cargo`, which isolates `CARGO_HOME` and `CARGO_TARGET_DIR` per repo/worktree. The Makefile uses it via `CARGO ?= ./scripts/repo-cargo`.

```bash
# Rust (via wrapper)
./scripts/repo-cargo check --workspace
./scripts/repo-cargo nextest run --workspace -E 'not test(governance_contracts)' --no-fail-fast

# Or via Makefile (preferred)
make check
make test

# Python
PYTHONPATH=sdk/python python3 -m pytest sdk/python/tests/ -v

# Full CI
make ci
```

## Branch conventions

- `main` — stable, PRs merge here
- Feature branches: `feat/<name>`, `fix/<name>`, `docs/<name>`, `refactor/<name>`
