# Changelog — meerkat-mobkit (Python SDK)

All notable changes to the Python SDK are documented here.

## Unreleased

### Runtime state builders

- `.persistent_state(path)` no longer creates a local redb mob store.
  Gateway-backed SDK runtimes keep mob storage in-memory while sessions,
  runtime state, metadata, console logs, and binary blobs remain under
  the configured state directory.

### Multimodal attachments

- Added `MobHandle.upload_blob(...)` for efficient binary image upload
  through the console multipart endpoint.
- `MobHandle.send(...)` accepts image attachments and sends multipart
  placeholders instead of inlining large base64 payloads in JSON.

### Streaming structural events + durable cursors

Mobkit's structural-events surface is now durable end-to-end. Consumers
can checkpoint a `cursor` from any `MobStructuralEvent` and resume
across mobkit gateway restarts on SQLite-backed deployments.

- `MobStructuralEvent.cursor` is now the **meerkat ledger cursor** —
  not a per-process `AtomicU64`. The 500ms poller is gone; mobkit's
  subscription task streams from `MobEventsView::subscribe_after`
  directly. Restart-resume is automatic when the runtime is built
  with `.persistent_state(path)` (a `SqliteMetadataStore` is
  auto-installed at `<path>/mobkit_metadata.sqlite`).
- New `MobEventsStaleError(RpcError)` raised by `query_mob_events()`
  / `subscribe_mob_events()` when `after_seq` is past the current
  ledger frontier. Carries `after_cursor` and `latest_cursor` so
  callers can rewind. JSON-RPC code is `-32010`
  (`MOB_EVENTS_STALE_CURSOR_CODE`).
- `mobkit/mob_events/subscribe` now returns a `subscribe_url`
  pointing to `/mobkit/mob_events/stream?after_seq=...&...` — a
  per-client SSE route that opens its own
  `MobEventsView::subscribe_after`, eliminating the gap between
  snapshot and live tail. Original filters are echoed back.
- `mobkit/mob_events/query` always returns a numeric
  `next_after_seq` even when the filter matches nothing (was
  `null`); a polling SDK keeps a valid resume anchor.

### `MobHandle.list_runs(flow_id?)`

New SDK method wrapping `mobkit/list_runs` over `MobHandle::list_runs`.
Returns `list[MobRun]` carrying the **full meerkat ledger projection**
— `step_ledger`, `failure_ledger`, `frames` (map keyed by frame id),
`loops` (map keyed by loop id), `loop_iteration_ledger`, `flow_state`,
`activation_params`, `schema_version`, `root_step_outputs`,
`loop_iteration_outputs`. Meerkat-internal sub-shapes (kernel state,
output blobs) pass through opaquely as `Any`.

New types in `meerkat_mobkit.types`:
`MobRun`, `MobRunStatus`, `StepRecord`, `FailureRecord`,
`FrameRecord`, `LoopRecord`, `LoopIterationRecord`. All exported
from `meerkat_mobkit`.

### Bug-hunt fixes (PR #69)

- `RpcError.data` propagates the JSON-RPC `error.data` payload —
  used by `MobEventsStaleError` to surface typed cursor info.
- `AsgiApp.__call__("/rpc", ...)` no longer masks `RpcError` as
  `-32603 / AttributeError`: the handler used a non-existent
  `exc.message` attribute and fell through to the generic
  `except Exception`. Real RPC code/message/data are now returned.
- `_transport.send_sync` rejects empty / duplicate request ids
  (`ValueError`) instead of deadlocking concurrent callers on the
  shared `_pending[""]` slot for the full 60s timeout.
- `EventEnvelope.from_dict` / `KeepAliveConfig.from_dict` coerce
  numeric fields (`timestamp_ms`, `interval_ms`) via `_coerce_int`.
  Pre-fix `None`, `"15"`, or floats survived into `int`-annotated
  fields and broke arithmetic far from the parse site.
- `MobEvent.from_sse` / `AgentEvent.from_sse` preserve non-dict
  payloads as `UnknownEvent(type="non_dict_payload", data={"raw":
  ...})`. Pre-fix the raw value was silently dropped.
- `subscribe_mob_events` accepts bare-list responses (not just dict
  envelopes), matching `query_mob_events`.
- Memory backend failures (`mobkit/memory/index`,
  `mobkit/memory/query`) now use distinct JSON-RPC code `-32012`
  (`MEMORY_BACKEND_UNAVAILABLE_CODE`). Pre-fix they shared `-32010`
  with the mob_events stale-cursor contract; SDKs branching on the
  code misclassified.

### Cross-mob signed-peer surface

- New `MobHandle.peer_pubkey()` → returns the local gateway's Ed25519
  signing pubkey as a base64 string. Wraps the new `mobkit/peer_pubkey`
  RPC. Use this to bootstrap trust before populating a peer mobkit's
  contact directory with this gateway's pubkey.
- `MobHandle.wire_local()` and `MobHandle.unwire_local()` accept an
  optional keyword `remote_pubkey_b64`. Non-inproc transports (`tcp://`,
  `uds://`) require it: the gateway rejects unsigned descriptors on real
  transports so meerkat-comms can verify envelope signatures at ingress.
  Inproc-only callers keep the existing four-positional-arg shape.
- `mobkit/cross_mob/directory` entries gain an optional `pubkey` field
  (base64) when the contact-directory TOML uses the new table form
  (`{ transport = "...", pubkey = "ed25519:..." }`). Bare-string entries
  remain backward compatible and report `pubkey = None`.

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

- await handle.ensure_member("agent-1", role="worker")
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
