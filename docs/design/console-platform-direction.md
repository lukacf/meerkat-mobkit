# MobKit Console: Unified Platform Direction (v4)

## Framing

One console type: **operator/debugging workbench for identity-based multi-agent systems**.

HomeCore and OB3 are instances of the same product category. The core console needs are shared: agent roster with health, conversation inspection, tool call visibility, activity timelines, lifecycle actions, and flow triggers. HomeCore pushes more edges (gating, routing, household state) but OB3 is a subset of the same design space.

**Architecture:** experience endpoint describes available data → host reads data and decides what panels to render → shared components render normalized view states → host provides domain-specific rendering via callbacks.

**Notation:** Items marked **[EXISTS]** are current working surface. Items marked **[NEW]** require new runtime/protocol work.

---

## Tier 1 — Identity-native interaction model [NEW]

**Current state:** The console is member/session-wired end to end. Dock targets keyed by `member_id`, send via `mobkit/send_message(member_id, ...)`, SSE correlation by `session_id` on `POST /interactions/stream` (ephemeral — open, drain, close).

**Gap:** Neither `dispatch()` (returns `(FencingToken, bool)`) nor `send()` (blocks until completion) on `IdentityRuntime` provides what the console needs: a **non-blocking identity-addressed send that returns a correlation token, with events streamed on a persistent connection**.

### New runtime API surface required

A new RPC method (e.g., `mobkit/interact`) that:

- **Request:** `{ identity: string, content: string, origin?: string }`
- **Response:** `{ interaction_id: string, identity: string }` — the `interaction_id` is the per-turn UI correlation token
- **Semantics:** Non-blocking. Enqueues the turn and returns immediately with the correlation token. Does NOT block until completion.

### Per-identity SSE connection model [NEW]

The current `POST /interactions/stream` is ephemeral (one connection per send, drained and closed). The identity model requires a **persistent SSE connection per identity** that carries all events, with concurrent interactions multiplexed and filtered by `interaction_id`.

Design decisions needed:
- Connection lifecycle: opened when a dock panel targets an identity, closed when the panel is closed or the identity changes
- Reconnection: host reconnects with `Last-Event-ID` for gap recovery
- Two panels targeting the same identity: share one SSE connection (refcounted) or separate connections?
- Buffering: events between disconnect and reconnect

### Mandatory event envelope fields

Every streamed frame must carry:

```typescript
{
  event_id: string;          // unique, monotonic for ordering
  interaction_id: string;    // correlation token from mobkit/interact response
  identity: string;          // stable identity attribution (NOT member_id)
  event_type: string;        // "text_delta", "tool_call", "tool_result", "run_completed", etc.
  timestamp_ms: number;
  data: unknown;             // event-type-specific payload
}
```

**Terminal event:** `event_type: "interaction_complete"` or `"interaction_failed"` with the matching `interaction_id`. Signals the host to stop waiting for more frames for that turn.

### Console codebase changes

- `lib/network.ts` — new `openIdentityStream(identity)` (persistent) + `interact(identity, content)` alongside existing member-addressed functions. Both may coexist during migration.
- `lib/adapters.ts` — dock targets keyed by identity, sidebar built from `IdentityStatus[]` when available
- `ConsoleApp.tsx` — send flow uses `mobkit/interact`, SSE subscription by identity with `interaction_id` filtering

---

## Tier 1 — Experience contract grows data sections

**Current state [EXISTS]:** `/console/experience` returns `agent_sidebar`, `activity_feed`, `topology`, `health_overview`, `flows`, `chat_inspector`, `session_history`. One-shot GET, no refresh model.

### What changes

The experience endpoint adds new optional sections and per-section metadata. The panel contract stays as-is: `ConsoleDockTarget.kind` + host's `renderPanelBody` switch. No panel registry.

| Section | Status | What it describes |
|---|---|---|
| `agent_sidebar` | **[EXISTS]** | Agent roster, labels, affordances, groups |
| `activity_feed` | **[EXISTS]** | SSE subscription config, event contract |
| `topology` | **[EXISTS]**, grows **[NEW]** | Currently module nodes. Grows to include identity nodes + managed edges |
| `health_overview` | **[EXISTS]** | Runtime health, loaded modules |
| `flows` | **[EXISTS]** | Scheduling, dispatch methods |
| `gating` | **[NEW]** | Pending entries, audit trail, risk tiers, action capabilities. Optional |
| `identity_status` | **[NEW]** | Per-identity continuity generation, lease health, session mapping. Optional |
| `routing` | **[NEW]** | Ingress events, dispatch outcomes, retry/dead-letter state. Optional |

### Per-section metadata [NEW]

```typescript
{
  refresh: 
    | { mode: "poll", interval_ms: number }
    | { mode: "stream", topic: string, update_semantics: "full_snapshot" | "append" | "patch" }
  capabilities?: string[]   // only for actionable sections: ["approve", "deny", "escalate"]
}
```

- `refresh` is required on all sections. Defines how the host gets updates. `update_semantics` specifies whether stream events are full replacement snapshots, append-only entries, or typed patches/deltas.
- `capabilities` is optional — only meaningful for sections with write actions (gating, routing). Omitted for read-only sections (topology, health).
- No `schema_version` (the shape is the version) or `scope` (unclear semantics) until there's concrete need.

---

## Tier 1 — Identity inspection panel [NEW]

A shared inspect view per identity. Shows what the data model can actually provide today, grows as the data model grows.

### Data sources

| Field | Source | Status |
|---|---|---|
| Lifecycle state (active, retiring, suspended) | `mobkit/status_identity` | **[EXISTS]** |
| Continuity generation + lease info | `mobkit/status_identity` | **[EXISTS]** |
| Output preview + is_final | `mobkit/inspect_identity` | **[EXISTS]** |
| Peer reachable count | `mobkit/inspect_identity` | **[EXISTS]** |
| Current session ID + member mapping | `mobkit/status_identity` | **[EXISTS]** |
| Topology peers (wired-to) | experience `topology` section | **[EXISTS]** for modules, **[NEW]** for identity graph |
| Recent tool calls | event log filtered by identity + tool_call_id | **[NEW]** — depends on identity-attributed tool events in the event model |
| Last activity timestamp | event log filtered by identity | **[NEW]** — depends on identity attribution on events |

The inspection panel renders what's available. "Recent tool calls" and "last activity" appear when the event model provides identity-attributed, tool-call-id-tagged events suitable for reconstruction. Until then, those fields are absent, not faked.

Dock panel kind: `"identity-inspect"`. Shared view state type in `@console-core`.

---

## Tier 2 — Virtualized activity log [NEW]

**Current state [EXISTS]:** `ConsoleActivityPulsePanel` — handles ~50 items, no virtualization, no search/filter.

### Shared primitive in `@console-components`

- Virtualized scrolling (1000+ items without DOM bloat)
- Text search filter
- Category/agent filter chips (by identity when identity-attributed events are available)
- Pause/resume toggle
- Click-to-navigate (focuses relevant identity in dock)
- Auto-scroll with "N new events" indicator when scrolled up

### Retention contract

- `maxBufferSize: number` — configurable by host, default 1000
- Eviction: circular buffer (drop oldest when full)
- Backward pagination: host-provided `onLoadMore?: () => Promise<items[]>` for scrolling into history beyond the buffer
- Shared component manages DOM virtualization; host manages the data buffer

**Identity attribution dependency:** Filtering/grouping by identity requires events to carry stable identity attribution in the envelope. This is a runtime requirement (see Tier 1 event envelope). Until available, filtering falls back to member_id/agent_id.

Host provides `renderItem`. Pulse/Roster/Feed stay for summary use cases.

---

## Tier 2 — Tool call/result blocks [NEW]

New block type in `@console-core`:

```typescript
interface ConversationRichToolCallBlock {
  type: "tool-call";
  toolCallId: string;
  name: string;
  arguments: string;
  result?: string;
  status: "pending" | "success" | "error";
  collapsible: boolean;
}
```

### Pairing by `tool_call_id`, not adjacency

Agents issue parallel tool calls; results return in arbitrary order.

### Shared `ToolCallAccumulator` in `@console-core`

A stateful helper that the host feeds frames into:

- Tracks open calls by `tool_call_id`
- Matches out-of-order results to their calls
- Handles never-arrived results: after a configurable timeout (default 60s), status transitions from `"pending"` to `"error"` with a timeout indicator
- Produces `ConversationRichToolCallBlock` entries that update from pending → complete/error
- Handles the common sequences: `tool_call A` → `tool_call B` → `tool_result B` → `tool_result A`

Each host should NOT reimplement this. The accumulator is shared; the host just feeds frames and reads blocks.

### Host rendering callback

On `ConversationPane`: `renderToolBlock?: (block: ToolCallBlock) => ReactNode | null`

- Return `null` → default shared renderer
- Return ReactNode → domain-specific rendering (MobKit knows nothing about what the tools do)

---

## Tier 2 — Actor topology graph [NEW for identity graph, EXISTS for module graph]

### Shared base panel

- Identity nodes positioned in a graph layout (from `topology` section of experience)
- Managed edges between them
- Node state indicators (active, idle, error)
- Click to inspect (opens identity inspection panel)

Host overlays via callback. Base graph and layout is shared; hosts annotate nodes with domain-specific decorations.

---

## Tier 2 — Generic gating/approvals surface [NEW]

MobKit owns gating primitives **[EXISTS]**: `GatingEvaluateResult`, `GatingDecisionResult`, `GatingAuditEntry`, `GatingPendingEntry`.

Console surface for these **[NEW]**.

### Read side

- Pending entries with identity, action, risk tier, timestamp
- Approved/denied/escalated history
- Audit trail per decision

### Write/action side — entry-addressed operations

Each pending entry has a `pending_entry_id`. Actions target a specific entry:

```typescript
// Request shape for all gating actions
{
  pending_entry_id: string;
  rationale?: string;          // optional comment/reason
}

// Methods declared in experience gating.capabilities:
"mobkit/gating/approve"    → { pending_entry_id, rationale? }
"mobkit/gating/deny"       → { pending_entry_id, rationale? }
"mobkit/gating/escalate"   → { pending_entry_id, rationale? }
```

- Actions are **idempotent**: approving an already-approved entry is a no-op, not an error
- Response: `{ entry_id, outcome: "approved" | "denied" | "escalated", resolved_at }`
- `rationale` support declared in capabilities: `capabilities: ["approve", "deny", "escalate", "rationale"]`

The shared panel renders action buttons based on declared capabilities. Hosts specialize the **entry context rendering** (what policy information is shown alongside the action), not the action mechanics.

Optional — only rendered when `gating` is present in the experience contract.

---

## Tier 2 — Routing/delivery surface [NEW]

Separate from `flows` (scheduling). The shared platform layer covers:

- Ingress event received (source, timestamp, payload summary)
- Target identity (who it was routed to)
- Dispatch outcome (delivered, queued, failed, dead-lettered)
- Retry/dead-letter state

Host-specific overlays (NOT shared): household notification semantics, channel-specific rendering, business routing reasons. The shared component provides the timeline/list chrome; hosts provide domain-specific entry content rendering.

**Note:** `list_routes`, `retry_delivery`, `inspect_connector` are NOT current RPCs. This section should be added to the experience contract **when a real shared abstraction emerges from actual HomeCore/OB3 routing implementations**, not designed ahead of the data model. Until then, hosts handle routing visibility in their own custom dock panels.

---

## Tier 3 — UX polish

### Response phase indicator [NEW]

```typescript
type ResponsePhase = "waiting" | "tool-executing" | "generating" | null;
```

- `"waiting"` — sent, no events received yet (the most common and longest phase)
- `"tool-executing"` — `tool_call` event received, waiting for result
- `"generating"` — `text_delta` events arriving
- `null` — idle

Host derives the phase from the event stream. Shared component renders the appropriate indicator. `"thinking"` omitted until the event model supports a thinking event from providers.

### Watch/monitoring semantics [EXISTS]

`pinned` + `unread` on `ConsoleSidebarItem` is sufficient for initial integration. May need: watchlist with alert thresholds, degraded-state badges, sticky activity filters in future iterations. Not a blocker.

### Per-agent lifecycle actions [EXISTS for RPCs, NEW for console wiring]

Context menu: retire, respawn, reset, inspect. Action strip for global flow triggers (host-configurable). Wired to existing identity-first RPCs.

---

## Host-specific surfaces

| Surface | Who | Extension point |
|---|---|---|
| Household state projections | HomeCore | Custom dock panel `kind` + `renderPanelBody` |
| Connector/routing dashboard | HomeCore | Custom dock panel (until shared routing section matures) |
| Domain-specific tool renderer | Both | `renderToolBlock` callback |
| Flow trigger actions | Both | Action strip items + `onBlockAction` |
| Domain-specific context menu | Both | `onItemContextMenu` callback |
| Domain-specific gating entry context | Both | Gating entry render callback |

---

## Architectural principle

The experience endpoint describes what data is available, with protocol metadata per section (refresh behavior, capabilities for actionable sections). The host decides what panels to show. Shared components render normalized view states via callbacks and slots. MobKit owns generic operator primitives (identity, gating, topology, lifecycle, activity). Hosts own domain-specific rendering and policy semantics. MobKit knows nothing about Slack, reviews, households, or domain-specific gate policies — but it provides typed extension points for hosts that do.
