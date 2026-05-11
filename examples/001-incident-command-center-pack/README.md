# Incident Command Center Pack

Synthetic incident-response scenario for the stock MobKit console.

This pack uses:

- the stock Rust runtime and stock `/console/*` surfaces
- stock `mobkit/interact`
- stock `mobkit/console/query_timeline`, `mobkit/console/send`, and `/console/timeline/stream`
- stock routing, delivery, inspect, lifecycle, topology, and gating methods
- a live provider-backed MobKit runtime with synthetic incident data and deterministic tool fixtures

## Source layout

- `scenario.yaml` — roster, links, seeded routes, gating, and smoke expectations
- `incident_command_center.rs` — scenario loader, runtime bundle, profile prompts, and tool fixtures
- `server.rs` — runnable console server entrypoint exposed as Cargo example `incident_command_center`
- `browser_smoke.cjs`, `ts_smoke.ts`, `python_smoke.py` — pack smoke tests

## What it proves

- identity-native chat over `mobkit/interact` + `/console/identity/stream`
- canonical console timeline replay over `mobkit/console/query_timeline`
- server-owned input acknowledgement over `mobkit/console/send`
- aggregate timeline catch-up/live streaming over `/console/timeline/stream`
- all-events updates over `/console/events/stream`
- watch/alert/degraded projection from seeded labels
- inspect, routing, gating, topology, and health panels in the stock console
- real runtime wiring reflected in topology
- replay after `Last-Event-ID` through the TypeScript helper
- gating chain `pending -> escalate -> successor pending -> approve`

## Run

```bash
export OPENAI_API_KEY=...
./examples.sh
```
