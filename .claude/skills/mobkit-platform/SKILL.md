---
name: mobkit-platform
description: "MobKit platform architecture, SDK patterns, console/runtime surfaces, release workflow, and Rust core internals. Use this skill whenever working on /Users/luka/src/meerkat-mobkit, adding SDK features, writing tests, debugging console/RPC/SSE behavior, updating Meerkat dependencies, understanding MobKit's relationship to Meerkat, releasing MobKit, or authoring MobKit examples."
---

# MobKit Platform

MobKit is the companion orchestration and product layer around the Meerkat multi-agent runtime. Meerkat owns agent construction, prompt assembly, tools, sessions, comms, and the core agent loop. MobKit owns bootstrapping mobs, routing work, exposing REST/JSON-RPC/SSE surfaces, projecting console state, enforcing operational policies, packaging SDKs, and shipping examples.

Apps own policy: who exists, which identities are addressable, which peers are wired, and what operational subsystems are enabled. MobKit owns mechanism: spawn, wire, route, reconcile, persist, stream, inspect, and serve the console.

## Current Baseline

Primary repo: `/Users/luka/src/meerkat-mobkit`

Current release line in the checked-out repo family: MobKit `0.6.55`.

Current direct Meerkat dependency family in `meerkat-mobkit/Cargo.toml`: `0.6.34` for `meerkat`, `meerkat-runtime`, `meerkat-session`, and `meerkat-store`. Verify the manifest before release or dependency work; do not rely on this note if the checkout has moved.

There should be no vendored `meerkat-comms` patch in this repo. Durable image forwarding belongs upstream in Meerkat; do not restore old vendoring.

## Repo Map

| Path | Purpose |
| --- | --- |
| `meerkat-mobkit/` | Rust crate, runtime, HTTP, JSON-RPC, gateway binaries, and embedded console bundle. |
| `meerkat-mobkit/src/lib.rs` | Public exports and the fastest map of current modules. Read this before trusting notes. |
| `meerkat-mobkit/src/mob_handle_runtime.rs` | `MobRuntime` / `RealMobRuntime`, mob bootstrap, member operations, session hooks, identity bridge points. |
| `meerkat-mobkit/src/runtime/` | Runtime subsystems: bootstrap, routing/delivery, gating, scheduling, memory, metadata, event transport, cross-mob control, session stores, supervisor. |
| `meerkat-mobkit/src/unified_runtime/` | Unified runtime builder, discovery, lifecycle, mob ops, event log, console/mob event projection, edge reconciliation, cross-mob wiring. |
| `meerkat-mobkit/src/console_aggregator/` | Console timeline/log aggregation, replay, visibility policy, SQL/in-memory stores, send state machine. |
| `meerkat-mobkit/src/http_console.rs` | Embedded console, REST console JSON, JSON-RPC, multipart upload/send, timeline stream, blob serving. |
| `meerkat-mobkit/src/http_sse.rs` | Agent events, mob events, structural mob-events SSE. |
| `meerkat-mobkit/src/rpc.rs` and `src/rpc/` | JSON-RPC dispatcher and method groups for console ingress, mob ops/events, routing/delivery, gating, memory, scheduling, session store, subscriptions. |
| `meerkat-mobkit/src/identity_first/` | Identity-first continuity, local store/lease, bridge, orchestrator, gateway bridges, contracts. |
| `meerkat-mobkit/src/contact_directory.rs`, `src/auth/peer_keys.rs` | Cross-mob contacts and signed peer-key handling. |
| `console/` | React console app source, tests, Vite/build entrypoints, generated `console/dist`. |
| `packages/console-core/` | Reusable console contracts, conversation/activity/sidebar/dock logic, formatting, rich content. |
| `packages/console-components/` | Reusable console UI components and CSS tokens/themes. |
| `meerkat-mobkit/console-dist/` | Embedded console bundle copied from `console/dist` by `npm run build`. |
| `sdk/python/` | Python SDK, gateway subprocess transport, identity-first models/providers, tests. |
| `sdk/typescript/` | TypeScript SDK, fetch/SSE client, runtime builders, tests. |
| `examples/001-incident-command-center-pack/` | Self-contained Incident Commander example pack. Keep example-specific source here. |
| `examples/002-foresight-studio-pack/` | TypeScript SDK example with identity-first providers, callback tools, and a heavily customized console. |
| `scripts/repo-cargo` | Preferred Cargo wrapper for this repo. |

Do not put example-specific support code under `meerkat-mobkit/src`. Same repo is fine; the core crate source tree is not.

## Console Surfaces

Current HTTP routes to remember:

- `GET /console`, `/console/assets/console-app.js`, `/console/assets/console-app.css` serve the embedded console.
- `GET /console/experience`, `/console/modules`, `/console/identities`, `/console/timeline` serve REST JSON.
- `GET /console/timeline/stream` streams projected console timeline frames.
- `GET /console/identity/{identity}/stream` is an identity-filtered timeline stream convenience route.
- `POST /console/send` submits console sends through the aggregator.
- `POST /console/rpc` is JSON-RPC.
- `POST /console/rpc/multipart` supports `mobkit/console/send` with image uploads and `mobkit/blob/upload`.
- `GET /blobs/{blob_id}` resolves console blob/image content.
- `GET /agents/{agent_id}/events`, `/mob/events`, and `/mobkit/mob_events/stream` are SSE observation surfaces.

Console projection is contract-boundary code. Prefer changing `console_aggregator`, `http_console`, `console/src/lib/adapters.ts`, or `packages/console-core` rather than leaking raw runtime text into React components.

Agent-tool spawns (`mob_spawn_member`, `delegate`, `spawn_member`/`spawn_many_members`) project into the console through `meerkat-mobkit/src/console_spawn.rs`: the unified runtime installs a `ConsoleSpawnSink` on the mob runtime at bootstrap, and the `AutoWireParentMobToolDispatcher` (in `mob_handle_runtime.rs`) registers each spawned member's identity + labels (`spawned_by`, `via_tool`, spawn labels) and appends the `initial_message` as a deduped `spawn-kickoff:*` `user_input` console event. Live runtime events resolve to the same chat identity. No console sink → spawn behavior unchanged.

## Access Control (ABAC)

Optional attribute-based access control for console and SSE surfaces lives in `meerkat-mobkit/src/access/` (model, engine, controller). Deny-by-default with deny-overrides when enabled; `admins` bypass rules; a disabled config (or absent controller) changes nothing.

Key facts:

- Opt-in via `config/access.toml` (mobkit_gateway conventional discovery), `UnifiedRuntimeBuilder::access_control_file`, `runtime.set_access_controller(...)`, or rpc_gateway `runtime_options.access_config_path` (TS SDK: `.accessControl(path)`, Python SDK: `.access_control(path)`; both auto-discover `config/access.toml`).
- Actions vocabulary: `agent.view/send/spawn/respawn/retire/reset`, `gating.view/decide`, `mob.observe`, `runtime.admin`, `access.admin`.
- Enforcement seams: experience filtering + affordance intersection in `runtime/console_ingress.rs` (`handle_console_rest_json_route_with_snapshot_and_access`), RPC gating + result filtering + `mobkit/access/*` admin methods in `http_console.rs` (`console_rpc_access_requirement`, `handle_access_admin_rpc`), SSE gating in `http_sse.rs` (`sse_access_context`).
- `mob.observe` gates the whole-mob event surfaces (`/mob/events`, `/mobkit/mob_events/stream`, `mobkit/mob_events/query`/`subscribe`) but does NOT override per-agent `agent.view`: events are filtered per source/`agent_identity`, mob-level (unattributed) events flow on `mob.observe` alone. "Observe all" = `mob.observe` + `agent.view` on `*`.
- Spawn-lineage inheritance: agent-spawned members carry a `spawned_by` console label (recorded by `src/console_spawn.rs`); agent checks walk the member plus its spawn ancestors (`evaluate_access_lineage`, depth 8, cycle-safe), so rules matching the parent also match its spawned members, with deny-overrides preserved across the chain. Lineage is runtime-derived ONLY: `spawned_by`/`via_tool` claims in caller-controlled labels (spawn specs, roster) are stripped at every enforcement seam (`sanitize_unverified_lineage_labels`) — only the in-memory spawn registry may assert them.
- `/blobs/{id}` is a capability surface (content-addressed `sha256:` ids, deduped across agents, no per-agent ACL) — authn + hash-unguessability, not an `agent.view` boundary.
- Live config: `AccessController` (std RwLock + revision) persists TOML on every admin mutation; per-request `AccessView` snapshots; agent label/role attributes cached from roster projections so label selectors work on identity-only surfaces.
- Console UI: `console/src/panels/AccessPanel.tsx`, nav kind `access` appears only when `experience.access.can_administer`.
- Denials: JSON-RPC `-32030` with `data.kind = "access_denied"`, HTTP 403 on REST/SSE.
- Anti-lockout invariant: enabling requires non-empty `admins`; validation lives in `access/model.rs`.
- Tests: `meerkat-mobkit/tests/access_control.rs`, unit tests in `src/access/*`, console helper tests in `console/src/panels/AccessPanel.test.ts`. Docs: `docs/concepts/access-control.mdx`.

## Console Configuration

The stock console can be shaped by `config/console.toml` in the conventional workspace layout. The runtime projects the parsed config through `GET /console/experience` as `console_config`, so embedders and the bundled React console share the same contract.

Key files:

- `meerkat-mobkit/src/console_config.rs` defines the TOML-backed config schema, normalization, realm overlays, and loader helpers.
- `meerkat-mobkit/src/config_convention.rs` discovers `config/console.toml`.
- `meerkat-mobkit/src/bin/mobkit_gateway.rs` loads the conventional config and applies the TUX init `realm` overlay.
- `meerkat-mobkit/src/bin/rpc_gateway.rs` accepts `runtime_options.console_config_path`.
- `sdk/typescript/src/builder.ts` exposes `MobKit.builder().consoleConfig("config/console.toml")` for RPC-gateway embedders, plus `.consoleAuthRequired(false)` for explicit local unauthenticated console demos.
- `meerkat-mobkit/src/runtime/console_ingress.rs` projects `console_config` and uses `title` for the base panel title.
- `console/src/ConsoleApp.tsx`, `console/src/panels/Topbar.tsx`, and `console/src/panels/Sidebar.tsx` consume the config.

Current shape:

```toml
title = "OB3"

[brand]
label = "Open Brain"
logo_url = "/assets/ob3.svg"
logo_alt = "OB3"

[appearance]
default_theme = "dark"
default_variant = "graphite"

[environment]
label = "prod"

[layout]
initial_preset = "two_columns"
initial_control = "roster"
initial_agent = "identity:ops-lead"
sidebar_collapsed = false

[rail]
visible = true
collapsed = false
active_preset_id = "critical"
empty_text = "No meaningful signals yet."

[[rail.filter_presets]]
id = "critical"
label = "Critical"
alert_levels = ["critical"]

[sidebar]
visible_controls = ["topology", "roster", "logs", "health"]
hidden_controls = []

[[sidebar.buttons]]
id = "ob3-board"
label = "OB3 Board"
href = "https://example.test/ob3"
target = "_blank"
icon_name = "external-link"

[agent_list]
group_by = ["labels.console_group", "labels.group", "role"]
subgroup_by = ["labels.org", "labels.realm"]
section_order = ["Personal", "Initiatives", "Internal"]
fallback_group = "Agents"
fallback_subgroup = "Other"
default_pinned_agent_ids = ["identity:ops-lead"]
collapse_single_subgroup = true

[[agent_list.badges]]
id = "org"
label = "Org"
field = "labels.org"
tone = "info"

[[agent_list.sections]]
name = "Initiatives"
collapsed = false
empty_title = "No initiatives"
empty_text = "Create one in Linear."

[actions]
inspect_label = "Profile"
chat_label = "Open chat"
send_label = "Send to agent"
respawn_label = "Respawn"
retire_label = "Retire"
reset_label = "Reset"
show_reset = false

[realms.ob3]
title = "OB3"

[realms.ob3.agent_list]
subgroup_by = ["labels.org"]
```

Agent grouping selectors and badge fields support `labels.<key>`, `label:<key>`, raw label keys, and direct fields such as `group`, `subgroup`, `role`, `kind`, `identity`, `member_id`, and `agent_id`. Spawned/delegate rows inherit configured group/subgroup metadata from their detected host if they do not carry matching metadata themselves. Prefer mob member labels such as `console_group = "Initiatives"` and `org = "Payments"` for domain-specific grouping.

When extending console customization, keep it view-level. The config should decide presentation, ordering, visibility, labels, defaults, and links; it should not decide runtime authorization, routing, or whether an agent exists. Be cautious with arbitrary CSS/theme color injection until there is a token contract.

Current console UX decisions to preserve:

- The main sidebar is navigation. Clicking an agent opens chat; no hover-driven chat/info toggle.
- Roster/details belong in roster-style surfaces, not in a redundant Inspect mode.
- Approvals owns pending, auto, audit, and policies. Do not reintroduce a separate `Gates` nav item.
- Signals should show meaningful user requests, peer-to-peer communication, assistant replies, image events, and relevant tool activity. Group repeated events from one interaction when useful.
- Roster should list actual wired peers, not only counts.
- Suppress scaffold/noise prompts, raw `checksum_token`, raw tool metadata, and transport envelopes from user-facing chat.
- Preserve user messages and timeline order. Ordering bugs usually come from merging history, live frames, and optimistic entries with partial timestamps.
- Render `assistant_image` and `assistant_image_appended` as images and dedupe by blob/reference.

## Cross-Mob And Comms

Comms delivers work; observation chooses scope. Do not design APIs that mix send and observe into one operation.

Cross-mob support includes in-process wiring and signed TCP/UDS contacts. Real transports require real Ed25519 peer pubkeys. In-process descriptors can stay unsigned because the in-process router authorizes through the identity map.

Useful files:

- `meerkat-mobkit/src/unified_runtime/cross_mob.rs`
- `meerkat-mobkit/src/runtime/cross_mob_control.rs`
- `meerkat-mobkit/src/runtime/cross_mob_remote.rs`
- `meerkat-mobkit/src/contact_directory.rs`
- `meerkat-mobkit/src/auth/peer_keys.rs`
- `meerkat-mobkit/tests/cross_mob_signed.rs`
- `meerkat-mobkit/tests/cross_mob_tcp.rs`
- `meerkat-mobkit/tests/cross_mob_uds.rs`

Image forwarding gotcha: the old failure was `image_ref_unavailable: current_turn image 0 did not resolve to a current-turn image`. Same-turn generate-and-send worked because the image was still in current-turn scope. Durable forwarding should use current upstream Meerkat behavior, not a vendored patch.

## Identity-First Continuity

Identity-first makes stable `AgentIdentity` strings the control-plane key and treats runtime member IDs as generated bindings. Prefer identity-scoped APIs for durable agents and member-scoped APIs only when working at the mob layer.

Key files:

- `meerkat-mobkit/src/identity_first/types.rs`
- `meerkat-mobkit/src/identity_first/contracts.rs`
- `meerkat-mobkit/src/identity_first/runtime.rs`
- `meerkat-mobkit/src/identity_first/orchestrator.rs`
- `meerkat-mobkit/src/identity_first/bridge.rs`
- `meerkat-mobkit/src/identity_first/gateway_bridges.rs`
- `sdk/python/meerkat_mobkit/identity_first_models.py`
- `sdk/python/meerkat_mobkit/identity_first_providers.py`
- `sdk/typescript/src/runtime.ts`
- `sdk/typescript/src/models.ts`

Avoid manual runtime ID extraction for ordinary identity-scoped work. Do not assume a continuity record means an active mob member; retired/stale records can exist.

## Incident Commander Example

The Incident Commander example is self-contained:

- `examples/001-incident-command-center-pack/incident_command_center.rs`
- `examples/001-incident-command-center-pack/server.rs`
- `examples/001-incident-command-center-pack/scenario.yaml`
- `examples/001-incident-command-center-pack/README.md`
- `examples/001-incident-command-center-pack/ts_smoke.ts`
- `examples/001-incident-command-center-pack/python_smoke.py`

Cargo example name: `incident_command_center`, pointing at `../examples/001-incident-command-center-pack/server.rs`.

Useful commands:

```bash
./scripts/repo-cargo check -p meerkat-mobkit --example incident_command_center
./scripts/repo-cargo test -p meerkat-mobkit --test incident_command_center_pack
./scripts/repo-cargo run -p meerkat-mobkit --example incident_command_center
```

Default listen address: `127.0.0.1:63210`. Override with `INCIDENT_COMMAND_CENTER_LISTEN_ADDR`.

## Development Workflow

For frontend/console changes:

```bash
cd console
npm run phase0:types --silent
npm run phase1:targets --silent
npm run build --silent
```

`npm run build` updates `console/dist` and `meerkat-mobkit/console-dist`. Include generated bundle changes when the Rust server should serve the new console.

For Rust changes:

```bash
./scripts/repo-cargo fmt --check
./scripts/repo-cargo check -p meerkat-mobkit
./scripts/repo-cargo test -p meerkat-mobkit
```

For SDK changes:

```bash
cd sdk/python && pytest
cd sdk/typescript && npm test
```

For UI behavior, use the Browser plugin against a live console after code changes.

## Release Workflow

Version files normally move together:

- `Cargo.toml`
- `Cargo.lock`
- `sdk/python/pyproject.toml`
- `sdk/typescript/package.json`
- `sdk/typescript/package-lock.json`

Use:

```bash
./scripts/bump-sdk-versions.sh <version>
./scripts/verify-version-parity.sh
```

When updating Meerkat dependencies, edit `meerkat-mobkit/Cargo.toml`, then run `./scripts/repo-cargo update -p ...` for the Meerkat family and `meerkat-mobkit`.

Release tags are `vX.Y.Z`. The GitHub release workflow validates version parity, builds binaries, and publishes registry packages. The job named `Publish crate + SDK packages` publishes crates.io, PyPI, and npm. When the user only cares about crates/packages, watch that job and verify registries directly:

```bash
cargo search meerkat-mobkit --limit 1
npm view @rkat/mobkit-sdk version
python -m pip index versions meerkat-mobkit
```

Registry versions are immutable. If a version already published from the wrong commit, roll forward to the next patch version.

## Tests To Remember

Rust:

- `meerkat-mobkit/tests/unified_console.rs`
- `meerkat-mobkit/tests/console_experience.rs`
- `meerkat-mobkit/tests/console_route_auth.rs`
- `meerkat-mobkit/tests/cross_mob_signed.rs`
- `meerkat-mobkit/tests/cross_mob_tcp.rs`
- `meerkat-mobkit/tests/cross_mob_uds.rs`
- `meerkat-mobkit/tests/e2e_target_contracts.rs`
- `meerkat-mobkit/tests/e2e_sdk_wire.rs`
- `meerkat-mobkit/tests/identity_first_*.rs`
- `meerkat-mobkit/tests/incident_command_center_pack.rs`
- `meerkat-mobkit/tests/mob_events_*.rs`
- `meerkat-mobkit/tests/routing_delivery.rs`
- `meerkat-mobkit/tests/sse_auth_gate.rs`

Console:

- `console/src/lib/adapters.test.ts`
- `console/src/lib/network.test.ts`
- `console/src/panels/Sidebar.test.ts`
- `console/src/panels/LogsPanel.test.ts`
- `console/src/panels/SignalsRail.test.ts`
- `packages/console-core/src/*.test.ts`
- `packages/console-components/src/**/*.test.tsx`
- `console/browser-e2e.cjs`

SDK:

- `sdk/python/tests/`
- `sdk/typescript/tests/`

## Practical Rules

- Follow the code that exists now. Start with `rg` and `meerkat-mobkit/src/lib.rs`.
- Keep examples out of core source.
- Keep signed cross-mob transport fail-closed. Missing pubkeys on real transports are errors.
- Keep raw transport, checksum, scaffold, reasoning, and tool-envelope details out of user-facing console rendering.
- Use structured parsers/contract adapters rather than ad hoc string slicing where the repo already has a boundary helper.
- After frontend changes, rebuild the embedded console bundle.
