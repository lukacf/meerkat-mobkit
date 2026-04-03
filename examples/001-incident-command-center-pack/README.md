# Incident Command Center Pack

Synthetic incident-response scenario for the stock MobKit console.

This pack uses:

- the stock Rust runtime and stock `/console/*` surfaces
- stock `mobkit/interact`
- stock routing, delivery, inspect, lifecycle, topology, and gating methods
- a deterministic scripted LLM/tool path so the scenario stays offline and repeatable

## What it proves

- identity-native chat over `mobkit/interact` + `/console/identity/stream`
- all-events updates over `/console/events/stream`
- watch/alert/degraded projection from seeded labels
- inspect, routing, gating, topology, and health panels in the stock console
- real runtime wiring reflected in topology
- replay after `Last-Event-ID` through the TypeScript helper
- gating chain `pending -> escalate -> successor pending -> approve`

## Run

```bash
./examples.sh
```
