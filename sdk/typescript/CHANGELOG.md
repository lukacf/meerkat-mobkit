# Changelog — @rkat/mobkit-sdk (TypeScript SDK)

## 0.6.0 — Meerkat 0.6 wire rename (BREAKING)

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

### Unchanged

- The `DurableAgentSpec` identity-first surface keeps its `profile` field (it
  mirrors meerkat's internal Rust `DurableAgentSpec`, which is a different
  surface from `MobMemberListEntry`).
- RPC method names (e.g. `mobkit/ensure_member`, `mobkit/spawn_helper`) are
  unchanged. Only their parameter and response field names shifted.
