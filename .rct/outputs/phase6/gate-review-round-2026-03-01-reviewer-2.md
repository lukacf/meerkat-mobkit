```yaml
verdict: APPROVE
gate: INTEGRATION_CORRECTNESS
phase: 6
integration_points:
  - point: JSON-RPC subscribe params -> runtime subscribe_events -> result/error
    status: WIRED
  - point: merged event bus -> scope filtering/backfill window -> SSE frame materialization
    status: WIRED
  - point: spawn_member -> merged_events append/sort -> checkpoint replay
    status: WIRED
blocking: []
non_blocking:
  - id: NB-001
    note: Spawn failure/timeout in reconnect flow not covered in Phase 6 tests.
```
