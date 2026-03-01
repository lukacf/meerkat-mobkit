# Meerkat MobKit Original Plan (User-Provided)

## Purpose
MobKit is a companion to Meerkat for multi-agent applications. Meerkat remains runtime owner (agent lifecycle, comms, tools, skills, sessions, flows). MobKit adds startup orchestration, module system, admin tooling, and multi-language SDK support.

## Core Principles
1. Meerkat is runtime owner; MobKit does not mediate agent construction/tool dispatch/prompt assembly.
2. Data over callbacks.
3. Hot paths in-process; operational subsystems out-of-process as MCP modules.
4. Modules are MCP servers.
5. Two modes (library + RPC), one module system.
6. Working implementations, not abstract placeholders.

## Meerkat Prerequisites (must exist in baseline)
- MobEventRouter
- inject_and_subscribe(id, msg)
- subscribe_agent_events(id)
- subscribe_all_agent_events()
- SpawnPolicy trait
- respawn(id, msg)
- AttributedEvent
- Roster::session_id(id)
- Roster::find_by_label(k, v)
- SessionBuildOptions.app_context
- SessionBuildOptions.additional_instructions
- CreateSessionRequest.labels
- RosterEntry.labels
- SpawnMemberSpec.resume_session_id

## Boundary with Meerkat
- Meerkat+App: agent construction/prompt/tool dispatch.
- MobKit: bootstrap/reconcile/runtime ops, stores, auth, SSE bridge, event bus, supervisor, module router.

## Two Modes
- Library mode (Rust direct calls)
- RPC mode (JSON-RPC stdio to mobkit-rpc)

## In-Process Infrastructure (v0.1 foundation)
- Bootstrap lifecycle
- Session stores (in-process)
- Auth
- SSE bridge
- Event bus
- Task supervisor

## MCP Modules
- scheduling, routing, delivery, gating, memory (+ third-party)

## Packages / Priority
Priority starts with core crate (bootstrap/event bus/supervisor/module router), then session stores, then mobkit-rpc, then SSE/auth/console/modules/SDKs.

## Practical Answers (User Decisions)
These are fixed defaults for v0.1 and should be reflected in spec/plan/checklist:
1. BQ: configurable dataset/table names, no hardcoded project.
2. Auth: Google OAuth + email allowlist.
3. Module security: `mobkit.toml` is trusted.
4. RPC versioning: `contract_version` in capabilities response; no method-level versioning.
5. Console: REST JSON endpoints behind app auth middleware.
6. SLOs: not v0.1 targets; only document measurable metrics.
7. Multi-replica: single-replica v0.1 documented constraint.
8. Release: crates.io, npm, PyPI, GitHub Releases; same support matrix as Meerkat.
9. Real blocker: Meerkat prerequisite APIs baseline/merge status.

## Immediate Build Target
Start with `meerkat-mobkit` core crate and prove mob startup with module process spawning and merged event bus streams.
