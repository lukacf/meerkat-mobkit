# MobKit

Companion orchestration platform for the [Meerkat](https://github.com/lukacf/meerkat) multi-agent runtime. Handles startup orchestration, module routing, operational subsystems, session persistence, and admin console.

## Key paths

| Area | Path |
|------|------|
| Rust core | `meerkat-mobkit-core/` |
| Gateway binary | `meerkat-mobkit-core/src/bin/phase0b_rpc_gateway.rs` |
| Python SDK | `sdk/python/meerkat_mobkit/` |
| Python tests | `sdk/python/tests/` |
| TypeScript SDK | `sdk/typescript/` |
| Docs (Mintlify) | `docs/` |

## Python SDK (v0.2.0)

Package: `meerkat-mobkit` (import as `meerkat_mobkit`).

Public surface — `__init__.py` exports:
- **Builder/Runtime**: `MobKit`, `MobKitBuilder`, `MobKitRuntime`
- **Models**: `DiscoverySpec`, `PreSpawnData`, `SessionBuildOptions`, `SessionQuery`
- **Protocol**: `SessionAgentBuilder`
- **Errors**: `MobKitError`, `TransportError`, `RpcError`, `NotConnectedError`, `CapabilityUnavailableError`, `ContractMismatchError`
- **Typed results**: `StatusResult`, `CapabilitiesResult`, `ReconcileResult`, `SpawnResult`, `SpawnMemberResult`, `SubscribeResult`, `KeepAliveConfig`, `EventEnvelope`, `RoutingResolution`, `DeliveryResult`, `MemoryQueryResult`
- **Events**: `MobEvent`, `AgentEvent`, `InteractionEvent`, `EventStream`
- **Config**: `auth`, `memory`, `session_store`

Module authoring helpers (`ModuleSpec`, `define_module`, etc.) live in `meerkat_mobkit.helpers` — not top-level.

Private internals (underscore-prefixed): `_client.py`, `_transport.py`, `_sse.py`.

## Build and test

```bash
# Rust
cargo check --workspace
cargo nextest run --workspace -E 'not test(phase0_governance)' --no-fail-fast

# Python
PYTHONPATH=sdk/python python3 -m pytest sdk/python/tests/ -v
```

## Branch conventions

- `main` — stable, PRs merge here
- Feature branches: `feat/<name>`, `fix/<name>`, `docs/<name>`, `refactor/<name>`
