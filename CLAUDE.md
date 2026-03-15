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
- **Errors**: `MobKitError`, `TransportError`, `RpcError`, `NotConnectedError`, `CapabilityUnavailableError`, `ContractMismatchError`
- **Typed results**: `StatusResult`, `CapabilitiesResult`, `ReconcileResult`, `SpawnResult`, `SpawnMemberResult`, `SendMessageResult`, `SubscribeResult`, `KeepAliveConfig`, `EventEnvelope`, `RoutingResolution`, `DeliveryResult`, `DeliveryHistoryResult`, `MemoryQueryResult`, `MemoryStoreInfo`, `MemoryIndexResult`, `MemberSnapshot`, `RuntimeRouteResult`, `GatingEvaluateResult`, `GatingDecisionResult`, `GatingAuditEntry`, `GatingPendingEntry`, `CallToolResult`
- **Events**: `MobEvent`, `AgentEvent`, `EventStream`
- **Config**: `auth`, `memory`, `session_store`
- **Constants**: `MEMBER_STATE_ACTIVE`, `MEMBER_STATE_RETIRING`

Module authoring helpers (`ModuleSpec`, `define_module`, etc.) live in `meerkat_mobkit.helpers` — not top-level.

Private internals (underscore-prefixed): `_client.py`, `_transport.py`, `_sse.py`.

## Build and test

```bash
# Rust
cargo check --workspace
cargo nextest run --workspace -E 'not test(governance_contracts)' --no-fail-fast

# Python
PYTHONPATH=sdk/python python3 -m pytest sdk/python/tests/ -v

# Full CI
make ci
```

## Branch conventions

- `main` — stable, PRs merge here
- Feature branches: `feat/<name>`, `fix/<name>`, `docs/<name>`, `refactor/<name>`
