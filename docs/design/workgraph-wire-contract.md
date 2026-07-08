# WorkGraph wire contract (mobkit 0.7.30)

Binding contract for the `mobkit/workgraph/*` JSON-RPC group, the experience
payload section, capabilities, ABAC actions, and SDK method names. All wire
fields snake_case. Results serialize meerkat 0.7.23's typed results VERBATIM
(serde of `WorkItem`, `WorkGraphSnapshot`, `WorkAttentionBinding`,
`GoalCreateResult`, etc. — see docs/design/workgraph-integration.md for the
upstream shapes). `expected_revision` is the CAS token on every mutation.

## Methods

Read (ABAC `workgraph.view`):

| Method | Params | Result |
|---|---|---|
| `mobkit/workgraph/snapshot` | `{namespace?, all_namespaces?, statuses?: string[], labels?: string[], include_terminal?, limit?}` | `WorkGraphSnapshot` |
| `mobkit/workgraph/list` | same filter | `{items: WorkItem[]}` |
| `mobkit/workgraph/get` | `{id, namespace?}` | `{item: WorkItem}` |
| `mobkit/workgraph/ready` | `{namespace?, labels?, limit?}` | `{items: WorkItem[]}` |
| `mobkit/workgraph/events` | `{namespace?, all_namespaces?, after_seq?, limit?}` | `{events: WorkGraphEvent[]}` |
| `mobkit/workgraph/attention/list` | `{namespace?, status?}` — `status` accepts plain strings `active\|paused\|superseded\|stopped` (or upstream's tagged `{state: ...}` form) | `{attention: WorkAttentionBinding[]}` |
| `mobkit/workgraph/goal/status` | `{binding_id, namespace?}` | `{item, attention}` |

Mutate (ABAC `workgraph.manage`; all honored on unified + console surfaces,
console additionally gated by `can_mutate`):

| Method | Params | Result |
|---|---|---|
| `mobkit/workgraph/create` | `CreateWorkItemRequest` fields (`title` required; `description?, priority?, completion_policy?, labels?, due_at?, not_before?, snoozed_until?, external_refs?, evidence_refs?, status?("open"\|"blocked"), namespace?`) | `{item}` |
| `mobkit/workgraph/update` | `{id, expected_revision, title?, description?, priority?, labels?, due_at?, not_before?, snoozed_until?, namespace?}` | `{item}` |
| `mobkit/workgraph/claim` | `{id, expected_revision, owner: {kind, id, display_name?}, lease_seconds?, namespace?}` | `{item}` |
| `mobkit/workgraph/release` | `{id, expected_revision, namespace?}` | `{item}` |
| `mobkit/workgraph/close` | `{id, expected_revision, status?("completed" default\|"cancelled"\|"failed"), namespace?}` | `{item}` |
| `mobkit/workgraph/block` | `{id, expected_revision, namespace?}` (verified upstream: `block(realm_id, namespace, id, expected_revision)` — no blocker-linkage field; use `link` with kind=`blocks` for blocker edges) | `{item}` |
| `mobkit/workgraph/link` | `{kind, from_id, to_id, namespace?}` | `{edge: WorkEdge}` (upstream returns a bare `WorkEdge`; mobkit wraps it) |
| `mobkit/workgraph/evidence/add` | `{id, expected_revision, evidence: {kind, id, label?, summary?}, namespace?}` | `{item}` |
| `mobkit/workgraph/policy/escalate` | `{binding_id, id, expected_revision, completion_policy, namespace?}` — witness (`AttentionContextProjection`) fetched SERVER-SIDE from `binding_id` | `{item}` |
| `mobkit/workgraph/goal/create` | `{title, description?, target: {kind:"session", session_id} \| {kind:"identity", identity} \| {kind:"owner", owner_key:{kind,id}}, mode?("pursue" default), completion_policy?, delegated_authority?, namespace?}` — `identity` lowered via `lower_agent_identity_attention_target` with the runtime's mob id | `{item, attention}` |
| `mobkit/workgraph/goal/confirm` | `{binding_id, expected_revision, evidence?: {kind, id, label?, summary?}, namespace?}` — console surface promotes authenticated principal via `with_trusted_principal` | `{item, attention}` |
| `mobkit/workgraph/goal/request_close` | `{binding_id, expected_revision, status?, namespace?}` | `{item, attention}` |
| `mobkit/workgraph/attention/pause` | `{binding_id, expected_revision, until?, namespace?}` | `{attention}` |
| `mobkit/workgraph/attention/resume` | `{binding_id, expected_revision, namespace?}` | `{attention}` |
| `mobkit/workgraph/attention/reassign` | `{binding_id, expected_revision, target: GoalAttentionTarget-or-identity form (as goal/create), namespace?}` — witness fetched server-side | `{previous, attention}` |

## Errors

- Service not configured: JSON-RPC error code `-32041`,
  `data.kind = "workgraph_unavailable"` (memory-backend-unavailable pattern).
- CAS/revision conflict (upstream Conflict): code `-32042`,
  `data.kind = "workgraph_conflict"`, `data.detail` carries upstream message —
  SDKs and console retry by refetching revision.
- Other WorkGraphError: `-32000` internal with `data.kind = "workgraph_error"`,
  full detail (K2 disclosure posture).
- Invalid params: `-32602` (standard).
- ABAC denial (console): `-32030` `access_denied` (standard).

## Capabilities + experience

- `mobkit/capabilities` result gains `"workgraph": true|false` (service
  configured) beside `identity_first`; workgraph methods appear in the
  advertised methods list only when configured.
- `GET /console/experience` gains:
  `"workgraph": {"available": bool, "can_view": bool, "can_manage": bool}`
  (available = service configured; can_* = ABAC-intersected for the caller,
  true when access control disabled).

## ABAC

New actions in the vocabulary: `workgraph.view`, `workgraph.manage`.
Reads map to view; mutations to manage. Deny-by-default when enabled; admins
bypass as usual.

## SDK method names

Python (MobHandle): `workgraph_snapshot, workgraph_list, workgraph_get,
workgraph_ready, workgraph_events, workgraph_attention_list,
workgraph_goal_status, workgraph_create, workgraph_update, workgraph_claim,
workgraph_release, workgraph_close, workgraph_block, workgraph_link,
workgraph_add_evidence, workgraph_escalate_policy, workgraph_goal_create,
workgraph_goal_confirm, workgraph_goal_request_close,
workgraph_attention_pause, workgraph_attention_resume,
workgraph_attention_reassign`.

TypeScript: same set camelCased (`workgraphSnapshot`, ...,
`workgraphAttentionReassign`).

Typed results both SDKs: `WorkGraphItem`, `WorkGraphEdge`,
`WorkGraphAttentionBinding`, `WorkGraphSnapshotResult`, `WorkGraphGoalResult`
(= item+attention), `WorkGraphAttentionReassignResult` (= previous+attention),
`WorkGraphEventEntry` (fields: seq, realm_id, namespace, item_id?, kind, at, payload — upstream `WorkGraphEvent`), `WorkGraphItemsResult`. Tolerant parsing (`.get()` /
optional fields); retain `revision` fields verbatim.

Builder opt-out both SDKs: Python `MobKitBuilder.workgraph(enabled: bool)`,
TS `.workgraph(enabled: boolean)` → `runtime_options.workgraph` (bool,
default true; the gateway also accepts a STRING directory for an explicit
durable store location (`workgraph.sqlite3` created inside) —
identity-first launches without a state dir are otherwise memory-backed,
warned at boot).

Typed `CapabilitiesResult` in both SDKs carries the `workgraph: bool` flag.

## Console

Headless command names (console/src/lib/headless.ts):
`workgraphSnapshot`, `workgraphGet`, `workgraphGoalStatus`, `workgraphClaim`,
`workgraphRelease`, `workgraphClose`, `workgraphGoalConfirm`,
`workgraphGoalRequestClose`, `workgraphAttentionPause`,
`workgraphAttentionResume`, `workgraphAttentionReassign`,
`workgraphEvents` → the RPC methods
above (CONSOLE_RPC_METHODS entries in console/src/lib/contract.ts).
`workgraphGet`/`workgraphGoalStatus` also back the operator-action CAS
resolution path (console/src/lib/workgraph-actions.ts): when the UI never
observed a revision it fetches the live one instead of guessing 0.

Inline card entry kind: `"workgraph"` (`ConversationWorkGraphEntry`), one
evolving entry per goal/root work-item, aggregated from workgraph tool-call
frames. Operator actions on the card gated by
`experience.workgraph.can_manage` + `!consoleReadOnly`.

## Implementation notes (as-built, 0.7.30)

- `claim.owner`: the wire accepts BOTH the flat `{kind, id, display_name?}`
  form and upstream's nested `{key: {kind, id}, display_name?}`; results
  always serialize the nested upstream shape (canonical).
- `goal/confirm` with absent `evidence`: the evidence kind is derived from
  the goal's completion policy (matching the machine's confirmation
  admission). Wire-supplied evidence is restricted to
  `{kind, id, label, summary}` — `confirmation_kind` /
  `confirming_owner_key` are rejected (reserved-classification smuggling);
  the service stamps the canonical classification. PrincipalConfirmed
  policies confirm only on the console surface (authenticated principal);
  the host-trusted stdin surface has no principal.
- `realm_id` is rejected (-32602) on every method — the service is
  realm-scoped at construction (mob definition id), and agent tool calls are
  scope-pinned (`ScopePinnedWorkGraphTools`) for the same reason.
- Upstream request fields the tables omit (e.g. `goal/create`
  `projection_policy`, `update` `external_refs`, `claim`
  `lease_expires_at`) are accepted verbatim.
- Library-mode spec constructors and `UnifiedRuntimeBuilder` also wire
  workgraph (not just the gateways) — a profile with `tools.workgraph=true`
  and no dispatcher is a fail-closed member-build error upstream.
- ONE BINDING PER TARGET, enforced across `goal/create`,
  `attention/reassign` (the new target), and `attention/resume`: a second
  ACTIVE — or PAUSED, since pauses expire — binding whose target aliases the
  same member (session and identity forms are unified through the roster)
  returns `-32042` naming the existing binding. Upstream
  `MultipleActiveBindings` would otherwise hard-fail every subsequent turn
  of that member. The same admission (`WorkGraphAdmission`) guards the agent
  tool plane's `workgraph_attention_reassign` (conflict surfaces as a
  `workgraph_conflict` tool error naming the occupant); the check-then-act
  is serialized per runtime across the RPC surfaces and the tool plane, and
  — for SQLite-backed stores, which two processes may share — cross-process
  via a `workgraph.admission.sqlite3` sidecar lock beside the store.
- `goal/create`, `attention/*`, and `policy/escalate` reject non-default
  `namespace` (`-32602`) — upstream turn-overlay resolution only reads the
  service default namespace, so goals elsewhere would be silently inert.
- `attention/reassign` is restricted by upstream's authority model to
  COORDINATE-mode bindings at meerkat 0.7.23 (the witness's
  `can_link_derived_from` derives from mode); other modes get a precise
  error, and the console hides the affordance. Upstream ask filed for a
  host-plane reassign.
- Upstream semantics consumers should know: `due_at` is an ELIGIBILITY time
  (claims guard-reject until `due_at <= now`), not a deadline; an
  attention-bound member turn's provider-visible tools are hard-filtered to
  the binding mode's workgraph allow-set for that turn; there are no push
  events at meerkat 0.7.23 — poll `snapshot`
  (`event_high_water_mark`) + `events(after_seq)`.
