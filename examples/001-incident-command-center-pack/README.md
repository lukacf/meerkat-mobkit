# Incident Command Center Pack

Synthetic incident-response scenario for the stock MobKit console.

This pack uses:

- the stock Rust runtime and stock `/console/*` surfaces
- stock `mobkit/interact`
- stock `mobkit/console/query_timeline`, `mobkit/console/send`, and `/console/timeline/stream`
- stock routing, delivery, inspect, lifecycle, topology, and gating methods
- a live provider-backed MobKit runtime with synthetic incident data and deterministic tool fixtures
- explicitly opt-in, single-edge topology management (`allow_bulk: false`)

## Source layout

- `scenario.yaml` — roster, links, seeded routes, gating, and smoke expectations
- `incident_command_center.rs` — scenario loader, runtime bundle, profile prompts, and tool fixtures
- `server.rs` — runnable console server entrypoint exposed as Cargo example `incident_command_center`
- `browser_smoke.cjs`, `ts_smoke.ts`, `python_smoke.py` — full provider-backed pack smoke tests
- `topology_browser_smoke.cjs`, `topology_examples.sh` — deterministic stock-console topology acceptance path

## What it proves

- identity-native chat over `mobkit/interact` + `/console/identity/stream`
- canonical console timeline replay over `mobkit/console/query_timeline`
- server-owned input acknowledgement over `mobkit/console/send`
- aggregate timeline catch-up/live streaming over `/console/timeline/stream`
- all-events updates over `/console/events/stream`
- watch/alert/degraded projection from seeded labels
- inspect, routing, gating, topology, and health panels in the stock console
- real runtime wiring reflected in topology
- pairwise connect, durable de-peer through reconciliation, and permission-gated reconnect
- no generic Connect All affordance in the stock console
- replay after `Last-Event-ID` through the TypeScript helper
- gating chain `pending -> escalate -> successor pending -> approve`

## Run

```bash
export OPENAI_API_KEY=...
./examples.sh
```

Topology control has a separate deterministic browser path. It builds and
drives the same embedded stock console against the real Rust runtime, but uses
MobKit's test client and never performs a model call:

```bash
./topology_examples.sh
```

The topology path uses `./scripts/repo-cargo` for the Rust build and server,
and writes inspectable prepare/resume browser screenshots to
`output/playwright/` by default. Set
`INCIDENT_TOPOLOGY_ARTIFACT_DIR` to choose another artifact directory.
Its offline module-readiness window is deliberately longer so a clean CI
checkout can compile generated module binaries on first boot; override it with
`INCIDENT_COMMAND_CENTER_MODULE_RECONCILE_TIMEOUT_SECS` when needed.

The scenario opts into editable topology explicitly. MobKit's default remains
disabled, so existing consumers retain the passive Graph and Roles views and
do not receive mutation controls. Editable mode always uses durable runtime
state: set `INCIDENT_COMMAND_CENTER_STATE_DIR` to choose its location, or the
standalone example defaults to the platform temporary directory under
`incident-command-center-state`.
