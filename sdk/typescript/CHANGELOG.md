# Changelog — @rkat/mobkit-sdk (TypeScript SDK)

## 0.6.0 — Meerkat 0.6 wire rename (BREAKING)

### Structural mob events surface

Added `MobHandle.queryMobEvents()` and `MobHandle.subscribeMobEvents()`,
backed by the new `mobkit/mob_events/query` and
`mobkit/mob_events/subscribe` JSON-RPC methods. The structural surface
preserves the full `MobEventKind` vocabulary from meerkat-mob (25
variants — `flow_started`, `step_dispatched`, `members_wired`,
`supervisor_escalation`, ...) along with the `mobId`, `runId`,
`stepId`, and `agentIdentity` fields that the legacy lossy
`UnifiedEvent::Agent` projection discards.

- New `MobStructuralEvent` interface with `parseMobStructuralEvent`
  carries `eventId`, `cursor`, `mobId`, `timestampMs`, `kind`, `runId`,
  `stepId`, `agentIdentity`, `data`.
- `EventQuery` extended with `mobId`, `runId`, `stepId`, `identity`
  filters. `afterSeq` is the cursor — pass the highest-seen `cursor`
  to paginate strictly-newer events.



Aligns the TypeScript SDK's wire-level field names with the meerkat 0.6
platform. Consumers upgrading from 0.5.x need to rename a handful of field
references — see the table below. No behavioural changes.

### Renamed fields

| Before | After | Surfaces |
|--------|-------|----------|
| `meerkatId` / `meerkat_id` | `agentIdentity` / `agent_identity` | `SpawnResult`, `MemberSnapshot`, `DiscoverySpec`, `mobkit/*` RPC params that accept a member identity |
| `profile` (as a mob-member role) | `role` | `SpawnResult`, `MemberSnapshot`, `DiscoverySpec`, `mobkit/ensure_member`, `mobkit/attach_existing_session`, `mobkit/spawn_helper` / `fork_helper` options, admin-console live snapshots |

Rename in your code:

```diff
- const spec: DiscoverySpec = { profile: "worker", meerkatId: "agent-1", labels: {...} };
+ const spec: DiscoverySpec = { role: "worker", agentIdentity: "agent-1", labels: {...} };

- await handle.ensureMember("agent-1", "worker");            // positional `profile` arg
+ await handle.ensureMember("agent-1", "worker");            // positional `role` arg (same call, renamed semantically)

- snap.meerkatId       // MemberSnapshot / SpawnResult
+ snap.agentIdentity

- snap.profile
+ snap.role
```

### Other wire shifts that ride the same rename

- `MemberSnapshot.state` is now `"Active"` / `"Retiring"` (title-cased from
  meerkat's native `MemberState` enum) where 0.5 emitted `"active"` /
  `"retiring"`. Update any string-comparison code.

- `mobkit/spawn_helper` and `mobkit/fork_helper` no longer emit
  `session_id` on the response. Meerkat 0.6 retires the helper before
  the call returns, so mobkit cannot recover the bridge session id
  post-hoc. If your code correlated helper history via this id, you need
  an alternative route (subscribe to the helper's events during the
  call, or wait for an upstream `HelperResult.bridge_session_id` field).

### Reconcile error semantics + report shape

- Reconcile responses may now include a `failures` array when meerkat's
  native `MobHandle::reconcile` records per-identity failures. On the
  Rust side, mobkit re-lifts a non-empty `failures` list into an `Err`
  so callers using `?` see pre-0.6 propagation behaviour; the TS SDK
  surfaces the full array on the JSON response.

- The `spawned` field of the reconcile report is now an array of
  `{ agent_identity: string, member_ref: string }` objects (the canonical
  `MobSpawnReceiptWire` shape from meerkat-contracts) rather than a list
  of identity strings. `member_ref` is a server-resolved opaque handle
  for subsequent member-targeted control calls.

### Lightweight roster

- `mobkit/list_members`, `mobkit/get_member`, `mobkit/find_members` no
  longer inject a `session_id` field per entry. Aligns with meerkat's
  lightweight-roster design. Use `mobkit/member_status` to bridge a
  member to a realtime session — its `MobMemberSnapshot` carries
  `current_session_id` natively.

### Removed RPCs

- `mobkit/member_current_session_id` and `mobkit/member_session_ref`
  are gone. Both were one-liners over an internal session-id lookup
  and are subsumed by `mobkit/member_status`.

### New: server-side readiness

- `MobHandle.waitReady(timeoutSeconds?)` — blocks until all current mob
  members are startup-ready for orchestration, then returns
  `{ ready: [...], timeout: false }`. On deadline expiry returns
  `{ ready: [], timeout: true }` instead of throwing. Relays meerkat
  0.6's `MobHandle::wait_for_ready`. Replaces ad-hoc client-side
  polling loops on `member_status`.

### New: flow enumeration + start

- `MobHandle.listFlows()` — returns the flow IDs declared by the mob's
  `[flows.*]` tables. Relays meerkat 0.6's `MobHandle::list_flows` via
  the new `mobkit/list_flows` RPC.
- `MobHandle.runFlow(flowId, params?)` — starts a flow run and returns
  its `run_id` string. Relays `MobHandle::run_flow` via
  `mobkit/run_flow`.

### Unchanged

- The `DurableAgentSpec` identity-first surface keeps its `profile` field (it
  mirrors meerkat's internal Rust `DurableAgentSpec`, which is a different
  surface from `MobMemberListEntry`).
- RPC method names (e.g. `mobkit/ensure_member`, `mobkit/spawn_helper`) are
  unchanged. Only their parameter and response field names shifted.
