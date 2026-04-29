# Changelog — meerkat-mobkit (Python SDK)

All notable changes to the Python SDK are documented here.

## 0.6.0 — Meerkat 0.6 wire rename (BREAKING)

### Structural mob events surface

Added `MobHandle.query_mob_events()` and `MobHandle.subscribe_mob_events()`,
backed by the new `mobkit/mob_events/query` and `mobkit/mob_events/subscribe`
JSON-RPC methods. The structural surface preserves the full
`MobEventKind` vocabulary from meerkat-mob (25 variants — `flow_started`,
`step_dispatched`, `members_wired`, `supervisor_escalation`, ...) along
with the `mob_id`, `run_id`, `step_id`, and `agent_identity` fields that
the legacy lossy `UnifiedEvent::Agent` projection discards.

- New typed envelope `MobStructuralEvent` (frozen dataclass with
  `from_dict`) carries `event_id`, `cursor`, `mob_id`, `timestamp_ms`,
  `kind`, `run_id`, `step_id`, `agent_identity`, `data`.
- `EventQuery` extended with `mob_id`, `run_id`, `step_id`, `identity`
  filters. `after_seq` is the cursor — pass the highest-seen `cursor` to
  paginate strictly-newer events.
- Both methods accept either an `EventQuery` instance or a plain dict.



Aligns the Python SDK's wire-level field names with the meerkat 0.6 platform.
Consumers upgrading from 0.5.x need to rename a handful of field references —
see the table below. No behavioural changes.

### Renamed fields

| Before | After | Surfaces |
|--------|-------|----------|
| `meerkat_id` | `agent_identity` | `SpawnResult`, `MemberSnapshot`, `DiscoverySpec`, `mobkit/*` RPC params that accept a member identity |
| `profile` (as a mob-member role) | `role` | `SpawnResult`, `MemberSnapshot`, `DiscoverySpec`, `mobkit/ensure_member`, `mobkit/attach_existing_session`, `mobkit/spawn_helper` / `fork_helper` options, admin-console live snapshots |

Rename in your code:

```diff
- spec = DiscoverySpec(profile="worker", meerkat_id="agent-1", labels={...})
+ spec = DiscoverySpec(role="worker", agent_identity="agent-1", labels={...})

- await handle.ensure_member("agent-1", profile="worker")
+ await handle.ensure_member("agent-1", role="worker")

- await handle.spawn_helper("h1", "task", profile="worker")
+ await handle.spawn_helper("h1", "task", role="worker")

- snap.meerkat_id       # MemberSnapshot / SpawnResult
+ snap.agent_identity

- snap.profile
+ snap.role
```

### Other wire shifts that ride the same rename

- `MemberSnapshot.state` is now `"Active"` / `"Retiring"` (title-cased from
  meerkat's native `MemberState` enum) where 0.5 emitted `"active"` /
  `"retiring"`. If you pattern-match on these strings, update the cases.

- `HelperResult.session_id` is now always `None`. Meerkat 0.6's
  `MobHandle::spawn_helper` / `fork_helper` retire the helper before the
  call returns, so mobkit cannot recover the bridge session id post-hoc;
  the field is preserved on the Python dataclass (so `data.get("session_id")`
  returns `None` cleanly) but mobkit no longer emits the key on the wire.
  If your code correlated helper history via this id, you need an
  alternative route (subscribe to the helper's events during the call, or
  wait for an upstream `HelperResult.bridge_session_id` field).

### Reconcile error semantics + report shape

- `mob_handle.reconcile(...)` may now return a report with a non-empty
  `failures` list (meerkat 0.6 collects per-identity failures rather than
  returning `Err` on the first failure). `UnifiedRuntime::reconcile()` in
  the Rust layer re-lifts this into an `Err` so callers using `?` see the
  same propagation behaviour they had pre-0.6; the Python SDK surfaces
  the full `failures` array in the response JSON when present. Watch for
  it and handle degraded-roster scenarios explicitly.

- The `spawned` field of the reconcile report is now an array of
  `{ "agent_identity": str, "member_ref": str }` objects (the canonical
  `MobSpawnReceiptWire` shape from meerkat-contracts) rather than a list
  of identity strings. `member_ref` is a server-resolved opaque handle
  for subsequent member-targeted control calls. Iterate with
  `[r["agent_identity"] for r in report["spawned"]]` to recover the
  prior projection.

### Lightweight roster

- `mobkit/list_members`, `mobkit/get_member`, `mobkit/find_members` no
  longer inject a `session_id` field per entry. Aligns with meerkat's
  lightweight-roster design. Use `mobkit/member_status` to bridge a
  member to a realtime session — its `MobMemberSnapshot` carries
  `current_session_id` natively.

### Removed RPCs

- `mobkit/member_current_session_id` and `mobkit/member_session_ref`
  are gone. Both were one-liners over an internal session-id lookup
  and are subsumed by `mobkit/member_status`. The corresponding Python
  SDK methods (`MobHandle.member_session_id`,
  `MobHandle.member_session_ref`) and the `MemberSessionRef` dataclass
  have been removed.

### New: server-side readiness

- `MobHandle.wait_ready(timeout=...)` — blocks until all current mob
  members are startup-ready for orchestration, then returns
  `{"ready": [...], "timeout": False}`. On deadline expiry returns
  `{"ready": [], "timeout": True}` instead of raising. Relays meerkat
  0.6's `MobHandle::wait_for_ready`. Replaces ad-hoc client-side
  polling loops on `member_status`.

### New: flow enumeration + start

- `MobHandle.list_flows() -> list[str]` — returns the flow IDs declared
  by the mob's `[flows.*]` tables. Relays meerkat 0.6's
  `MobHandle::list_flows` via the new `mobkit/list_flows` RPC.
- `MobHandle.run_flow(flow_id, params=None) -> str` — starts a flow run
  and returns its `run_id`. Relays `MobHandle::run_flow` via
  `mobkit/run_flow`. Pair with `flow_status`/`cancel_flow` to drive the
  run end-to-end.

### Unchanged

- The `DurableAgentSpec` identity-first surface keeps its `profile` field (it
  mirrors meerkat's internal Rust `DurableAgentSpec`, which is a different
  surface from `MobMemberListEntry`).
- RPC method names (e.g. `mobkit/ensure_member`, `mobkit/spawn_helper`) are
  unchanged. Only their parameter and response field names shifted.
- `CatalogEntry.profile` (model-capability dict) is unrelated to mob-member
  roles and is unchanged.
