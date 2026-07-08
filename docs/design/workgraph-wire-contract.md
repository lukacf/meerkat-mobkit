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
| `mobkit/workgraph/attention/list` | `{namespace?, status?}` | `{attention: WorkAttentionBinding[]}` |
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
default true).

## Console

Headless command names (console/src/lib/headless.ts):
`workgraphSnapshot`, `workgraphGoalStatus`, `workgraphClaim`,
`workgraphRelease`, `workgraphClose`, `workgraphGoalConfirm`,
`workgraphGoalRequestClose`, `workgraphAttentionPause`,
`workgraphAttentionResume`, `workgraphAttentionReassign` → the RPC methods
above (CONSOLE_RPC_METHODS entries in console/src/lib/contract.ts).

Inline card entry kind: `"workgraph"` (`ConversationWorkGraphEntry`), one
evolving entry per goal/root work-item, aggregated from workgraph tool-call
frames. Operator actions on the card gated by
`experience.workgraph.can_manage` + `!consoleReadOnly`.
