<p align="center">
  <img src="docs/logo.png" alt="MobKit" width="160" />
</p>

<h1 align="center">MobKit</h1>

<p align="center">
  Identity-first control, durable agent continuity, and an operator console for Meerkat mobs.
</p>

<p align="center">
  <a href="LICENSE-MIT"><img src="https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg" alt="License" /></a>
  <a href="https://crates.io/crates/meerkat-mobkit"><img src="https://img.shields.io/crates/v/meerkat-mobkit.svg" alt="crates.io" /></a>
  <a href="https://pypi.org/project/meerkat-mobkit/"><img src="https://img.shields.io/pypi/v/meerkat-mobkit.svg" alt="PyPI" /></a>
  <a href="https://www.npmjs.com/package/@rkat/mobkit-sdk"><img src="https://img.shields.io/npm/v/@rkat/mobkit-sdk.svg" alt="npm" /></a>
  <a href="https://www.rust-lang.org"><img src="https://img.shields.io/badge/rust-1.94%2B-orange.svg?logo=rust" alt="Rust 1.94+" /></a>
</p>

---

MobKit is the application-facing kit around Meerkat mobs. It gives host apps a stable way to boot a mob, project it into a console, route user and system work by durable identity, expose JSON-RPC/REST/SSE control surfaces, and keep long-lived agents recoverable across restarts.

MobKit does not replace Meerkat or `meerkat-mob`. Meerkat runs sessions and the agent loop. `meerkat-mob` owns mob membership, topology, lifecycle, and mob-level event/comms behavior. `meerkat-mobkit` packages the surrounding control plane: identity-first continuity, runtime bootstrap, operational modules, SDKs, and the console projection.

<p align="center">
  <img src="docs/images/mobkit-console.png" alt="HarborOps MobKit console with route planning, topology, roster, and signals panels" width="900" />
</p>

## What MobKit Is For

MobKit is useful when a system needs agents that keep meaning over time:

- **Personal agents** such as `identity:luka` that represent a person and receive direct conversational work
- **Domain agents** such as `domain:calendar` or `domain:school` that own a durable capability
- **Coordination agents** such as `triage:main`, `family-group:main`, or `gate:main` that route, summarize, or approve work
- **Task agents** that can still be spawned for short-lived execution while the durable identities stay stable

The important idea is that the app talks to identities, not throwaway runtime members. A runtime member can be created, resumed, retired, respawned, or rematerialized, but `identity:luka` remains the app-facing handle.

## Identity-First Continuity

Identity-first mode is MobKit's contract for long-lived agents.

Each durable identity has a continuity record containing the app-facing `AgentIdentity`, the current runtime member id, the Meerkat session id, a continuity generation, and a checkpoint version. MobKit uses a continuity store plus leases and fencing tokens so a process can prove it is the current owner before it sends, dispatches, checkpoints, or retires an identity.

That gives host apps a few concrete guarantees:

- **Stable addressing.** Send to `identity:luka` or dispatch to `triage:main` even if the concrete runtime member has changed.
- **Session continuity.** Non-destructive recovery keeps the same durable identity and session history when possible.
- **Safe ownership.** Lease renewal and fencing protect long-lived identities from split-brain writes.
- **Controlled lifecycle.** `respawn()` and `rematerialize()` recover an identity without intentionally wiping continuity; `reset()` starts a destructive new generation.
- **Lazy or eager materialization.** Runtimes can materialize every identity at boot, register identities lazily, or warm them in the background.

For local and embedded deployments, `.persistent_state(...)` creates SQLite-backed MobKit metadata, console logs, runtime state, session state, and blob storage under one directory. For externally authoritative deployments, provide a continuity store, lease provider, roster provider, and scratch directory instead.

## Quick Start

```python
from meerkat_mobkit import MobKit
from meerkat_mobkit.identity_first_models import DurableAgentSpec, ManagedPeerEdge


class Roster:
    async def roster(self, ctx):
        return [
            DurableAgentSpec(
                identity="identity:luka",
                profile="personal",
                addressability="addressable",
            ),
            DurableAgentSpec(
                identity="triage:main",
                profile="triage",
                addressability="internal_only",
            ),
            DurableAgentSpec(
                identity="domain:calendar",
                profile="calendar",
                addressability="internal_only",
            ),
        ]


class Topology:
    async def compute_edges(self, target_identities, ctx):
        return [
            ManagedPeerEdge(a="identity:luka", b="triage:main"),
            ManagedPeerEdge(a="triage:main", b="domain:calendar"),
        ]


rt = await (
    MobKit.builder()
    .mob("config/mob.toml")
    .persistent_state(".mobkit/state")
    .roster(Roster())
    .topology_provider(Topology())
    .gateway("./target/debug/mobkit_gateway")
    .build()
)

luka = rt.agent("identity:luka")
triage = rt.agent("triage:main")

await triage.dispatch_text("New calendar event needs triage", origin="connector")
await luka.send("What needs my attention before lunch?")

print(await luka.wait_for_output(timeout=60))
```

## Console

The bundled React console is served by the runtime and talks to the same contracts SDKs use:

- `GET /console/experience` for console configuration, roster projection, identity status, and runtime capabilities
- `POST /console/rpc` for console commands such as send, inspect identity, query timeline, routing, delivery, and gating
- `GET /console/timeline` and `GET /console/timeline/stream` for replayable console frames and live continuation
- `GET /console/identity/{identity}/stream` for identity-scoped event streams
- `/blobs/{id}` and multipart console RPC for image and attachment flows

Host apps can shape the stock console with `config/console.toml`: branding, theme, environment labels, layout presets, sidebar grouping, default pins, rail filters, action labels, and host-provided links. The console stays a projection of runtime truth; host apps own policy and domain-specific decisions.

## Core Capabilities

- **Unified runtime bootstrap.** Build a Meerkat mob and MobKit operational surface in one runtime.
- **Identity-first runtime.** Durable rosters, topology providers, continuity stores, leases, fencing, lifecycle recovery, and identity-scoped handles.
- **Operational modules.** Routing, delivery, scheduling, gating, memory, session-store adapters, metadata, and event transport.
- **Console projection.** Roster, topology, conversations, activity signals, logs, health, approvals, timeline replay, and live SSE updates.
- **SDKs.** Rust exports plus Python and TypeScript clients with typed identity-first, mob, event, memory, routing, delivery, and gating APIs.
- **Auth and deployment controls.** JWT/OIDC validation, console route protection, peer keys, release metadata checks, and conventional config discovery.

## Meerkat vs `meerkat-mob` vs `meerkat-mobkit`

| | Meerkat | `meerkat-mob` | `meerkat-mobkit` |
|---|---|---|---|
| **Primary job** | Runs agent sessions | Runs mobs of agents | Makes mobs usable by apps and operators |
| **Owns** | Agent loop, prompt assembly, tool execution, session runtime, session persistence primitives | Membership, topology, mob lifecycle, mob event ledger, mob handles, mob-level comms behavior | Identity-first control plane, continuity adapters, runtime bootstrap, operational modules, console projection, SDK transport |
| **App-facing handles** | Session ids and agent runtime APIs | Member ids, mob handles, mob events | Durable identities such as `identity:luka` plus console and SDK handles |
| **Typical surface** | Rust crates, CLI, REST/RPC/MCP surfaces in the Meerkat repo | Rust mob APIs and event streams | Rust crate, `mobkit_gateway`, JSON-RPC, REST/SSE console routes, Python SDK, TypeScript SDK |
| **Boundary rule** | Executes individual agent work | Decides what the mob is and how members relate | Projects and controls the mob without inventing ownership that belongs in Meerkat or `meerkat-mob` |

## Install

```bash
# Python
pip install meerkat-mobkit

# TypeScript
npm install @rkat/mobkit-sdk

# Rust
cargo add meerkat-mobkit
```

## Repository Layout

| Path | Description |
|------|-------------|
| `meerkat-mobkit/` | Rust crate, gateway binaries, runtime, identity-first, RPC, HTTP, console contracts |
| `sdk/python/` | Python SDK (`meerkat-mobkit` on PyPI) |
| `sdk/typescript/` | TypeScript SDK (`@rkat/mobkit-sdk` on npm) |
| `console/` | React console source and browser smoke harness |
| `packages/console-core/` | Console data adapters and headless core logic |
| `packages/console-components/` | Reusable React console components |
| `docs/` | Mintlify documentation site |
| `examples/` | Example packs and runtime demos |

## Development

```bash
make ci                         # Full CI pipeline
make test                       # Rust tests
make test-python                # Python SDK tests
npm --prefix console run build  # Rebuild embedded console assets
```

Useful focused checks:

```bash
npm --prefix console run phase0:types --silent
npm --prefix console run embedded:freshness --silent
./scripts/repo-cargo check -p meerkat-mobkit
```

## Documentation

Full documentation is available at [docs.rkat.ai](https://docs.rkat.ai).

## License

Dual-licensed under [MIT](LICENSE-MIT) and [Apache 2.0](LICENSE-APACHE).
