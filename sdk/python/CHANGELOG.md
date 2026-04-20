# Changelog — meerkat-mobkit (Python SDK)

All notable changes to the Python SDK are documented here.

## 0.6.0 — Meerkat 0.6 wire rename (BREAKING)

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

### Reconcile error semantics

- `mob_handle.reconcile(...)` may now return a report with a non-empty
  `failures` list (meerkat 0.6 collects per-identity failures rather than
  returning `Err` on the first failure). `UnifiedRuntime::reconcile()` in
  the Rust layer re-lifts this into an `Err` so callers using `?` see the
  same propagation behaviour they had pre-0.6; the Python SDK surfaces
  the full `failures` array in the response JSON when present. Watch for
  it and handle degraded-roster scenarios explicitly.

### Unchanged

- The `DurableAgentSpec` identity-first surface keeps its `profile` field (it
  mirrors meerkat's internal Rust `DurableAgentSpec`, which is a different
  surface from `MobMemberListEntry`).
- RPC method names (e.g. `mobkit/ensure_member`, `mobkit/spawn_helper`) are
  unchanged. Only their parameter and response field names shifted.
- `CatalogEntry.profile` (model-capability dict) is unrelated to mob-member
  roles and is unchanged.
