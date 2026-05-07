# Changelog — @rkat/mobkit-sdk (TypeScript SDK)

## Unreleased

### Multimodal attachments

- Added `MobHandle.uploadBlob(...)` / `upload_blob(...)` for efficient
  binary image upload through the console multipart endpoint.
- `MobHandle.send(...)` accepts image attachments and sends multipart
  placeholders instead of inlining large base64 payloads in JSON.

### Streaming structural events + durable cursors

Parity with the Python SDK: structural events are now durable
end-to-end. Cursors come from the meerkat ledger and survive mobkit
restarts on SQLite-backed deployments.

- `MobStructuralEvent.cursor` is the meerkat ledger cursor (not a
  per-process counter). The runtime auto-installs a SQLite metadata
  store next to `.persistent_state(path)` builds.
- New `MobEventsStaleError extends RpcError` raised by
  `queryMobEvents()` / `subscribeMobEvents()` on JSON-RPC code
  `-32010`. Carries `afterCursor` and `latestCursor`. The shared
  `MOB_EVENTS_STALE_CURSOR_CODE` constant is exported.
- `mobkit/mob_events/subscribe` returns a `subscribe_url` pointing
  to the new `/mobkit/mob_events/stream?after_seq=...&...` SSE
  route; original filters echo back so the SSE handler picks up
  gaplessly with the same predicate.
- `mobkit/mob_events/query` always returns a numeric
  `next_after_seq` even on empty match (was `null`).

### `MobHandle.listRuns(flowId?)`

New SDK method wrapping `mobkit/list_runs`. Returns `MobRun[]` with
the full meerkat ledger projection: `stepLedger`, `failureLedger`,
`frames` (map keyed by frame id), `loops` (map keyed by loop id),
`loopIterationLedger`, `flowState`, `activationParams`,
`schemaVersion`, `rootStepOutputs`, `loopIterationOutputs`.
Meerkat-internal sub-shapes pass through opaquely as `unknown`.

New interfaces and parsers exported from `@rkat/mobkit-sdk`:
`MobRun`, `MobRunStatus`, `StepRecord`, `FailureRecord`,
`FrameRecord`, `LoopRecord`, `LoopIterationRecord` plus
`parseMobRun` / `parseStepRecord` / etc.

### Bug-hunt fixes (PR #69)

- `RpcError.data` propagates the JSON-RPC `error.data` payload.
- New `isRpcError(err)` / `isMobEventsStaleError(err)` structural
  type guards. **Use these instead of `instanceof RpcError`** —
  dual CJS+ESM packaging, vitest module isolation, and
  hoisted-vs-nested monorepos all produce two `RpcError`
  constructors that fail `instanceof` for each other's instances.
  The structural checks survive the split.
- `MobKitRuntime.start()` no longer rewrites `mobkit/init`
  `RpcError` as a generic `TransportError`. Original code/message/
  data flow through; only synthesizes `TransportError` when the
  subprocess actually died.
- `SseBridge` ties the underlying HTTP request to the generator
  lifetime. Breaking out of `for await (const e of
  handle.subscribeAgent(...))` now destroys the request — pre-fix
  it leaked an `http.ClientRequest` per iteration.
- `createJsonRpcHttpTransport` accepts a `timeoutMs` option
  (default 60s) and aborts via `AbortController`. Pre-fix a server
  that accepted but never replied hung the caller's `await`
  forever.
- SSE parser handles `id:N` / `event:foo` lines without the
  optional space (SSE-spec-legal). Pre-fix only the
  `"id: "` / `"event: "` prefixes matched and resume cursors were
  lost.
- `parseSubscribeResult` filters non-object entries from `events[]`
  before mapping. Pre-fix null/string/number entries were coerced
  into silently-empty envelopes — data loss.
- `parseDispatchInput` validates `origin` against the closed
  `DispatchOrigin` union and falls back to `"system"` for unknown
  values. Pre-fix it was an unchecked cast.

### Cross-mob signed-peer surface

- New `mobkit/peer_pubkey` RPC returns the local gateway's Ed25519
  signing pubkey as a base64 string. Use this to bootstrap trust before
  populating a peer mobkit's contact directory with this gateway's
  pubkey. (Rust + Python wrappers ship in parallel; TS wrapper to
  follow once the cross-process transport lands.)
- `mobkit/cross_mob/wire_local` and `mobkit/cross_mob/unwire_local` now
  accept an optional `remote_pubkey_b64` (base64 of the peer gateway's
  Ed25519 verifying key). Non-inproc transports (`tcp://`, `uds://`)
  require it — the gateway rejects unsigned descriptors on real
  transports so meerkat-comms can verify envelope signatures at ingress.
- `mobkit/cross_mob/directory` entries gain an optional `pubkey` field
  (base64) when the contact-directory TOML uses the new table form
  (`{ transport = "...", pubkey = "ed25519:..." }`). Bare-string entries
  remain backward compatible.

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
