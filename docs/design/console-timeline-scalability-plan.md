# Console Timeline Scalability Plan

## Context

Fugue exposed a MobKit console failure mode on a moderate real deployment:

- `/console/identities` was healthy and showed the expected agents, including Kapellmeister.
- Identity-specific `mobkit/console/query_timeline` calls returned frames for the selected agent.
- The browser received those frames, but chat panes could remain on "No messages yet" while the frontend waited for a full paged identity backfill loop to finish.
- The global timeline/activity seed started at aggregate cursor 0, so it showed old Kapellmeister startup frames instead of recent issue-agent activity.
- Empty or invisible cursor windows could advance only a small raw range without surfacing useful visible frames.

This is a MobKit console projection/query/UX problem, not a Meerkat RPC problem. Meerkat already supplies the underlying frames; MobKit owns console log storage, visibility filtering, REST/RPC/SSE console surfaces, and the React console behavior.

The fix must scale beyond Fugue. Real deployments may have 10x-100x more frames and more sparse per-identity activity, so the console cannot depend on walking from the beginning of the aggregate log or loading a whole identity history before first render.

## Goals

- Opening an agent chat shows recent usable history after the first bounded request.
- Global activity/timeline shows recent meaningful events, not the oldest log page.
- Cursor paging skips invisible gaps within a bounded scan budget.
- Initial console load remains store-local and does not synchronously refresh broad session history.
- The design works for sparse identities in large aggregate logs.

## Non-Goals

- Do not add a Fugue-specific history path.
- Do not move console timeline semantics into Meerkat core.
- Do not require full transcript loading before a panel is usable.
- Do not reintroduce live mob actor/member inspection into timeline reads.

## Ownership Boundary

MobKit should own the full fix:

- Console log store query modes and indexes.
- Visibility-aware timeline paging.
- Recent/tail snapshots for REST/RPC/SSE surfaces.
- Incremental identity timeline rendering in the browser.
- Browser and backend stress coverage.

Meerkat changes are only needed if MobKit lacks stable event metadata for turn anchors. Current evidence says MobKit already has enough frame identity, cursor, timestamp, and event-kind data to solve this locally.

## Proposed Contract

Add explicit timeline query semantics instead of overloading `after + limit`.

Suggested shape:

```json
{
  "identity": "LUC-631/planner",
  "limit": 200,
  "mode": "recent"
}
```

and:

```json
{
  "identity": "LUC-631/planner",
  "limit": 200,
  "mode": "since",
  "after": "console:219000"
}
```

Semantics:

- `mode = "recent"` returns the latest visible frames for the target, in display order, plus a continuation cursor suitable for live SSE continuation.
- `mode = "since"` returns visible frames after `after`, scanning through invisible frames up to a bounded raw-frame budget.
- Existing calls without `mode` can keep backward-compatible oldest-first behavior temporarily, but the bundled console should switch to explicit modes.
- Responses should expose enough paging state to distinguish "no visible frames in this window" from "target exhausted".

Potential response additions:

```json
{
  "frames": [],
  "next_cursor": "console:86020",
  "latest_cursor": "console:240000",
  "exhausted": false
}
```

## Backend Plan

1. Add store-level recent queries.

   - SQLite global recent: `ORDER BY cursor_seq DESC LIMIT ?`, reverse in memory before returning.
   - SQLite identity recent: query by `(identity, cursor_seq)`; add an index if missing.
   - SQLite conversation recent: query by `(conversation_id, cursor_seq)` if conversation queries remain supported.
   - In-memory store: use reverse `BTreeMap` iteration for tail queries.

2. Add visibility-aware paging.

   - `MobKitConsoleAggregator::query_timeline` should page raw store frames until it collects `limit` visible frames, reaches store exhaustion, or hits a strict raw scan budget.
   - The returned cursor should track the raw scan cursor, not only the last visible frame, so callers can continue past invisible gaps.
   - The loop must stay store-local and use cached identity visibility/read-model data.

3. Add recent/tail behavior to RPC and REST.

   - `mobkit/console/query_timeline` should accept the new query mode.
   - `GET /console/timeline` should support the same mode.
   - The existing SSE fresh snapshot helper is useful, but it should not be the only tail-aware path because the React console currently seeds through RPC.

4. Revisit identity anchors.

   - For identity chat, a recent slice should include a useful turn-start anchor before noisy/tool-heavy tail frames.
   - Anchor candidates should include `user_input`, `run_started`, and prompt-bearing `interaction_started`.
   - Avoid full-history scans to find anchors. Prefer bounded reverse scans or future turn-summary metadata.

## Frontend Plan

1. Change global activity seed.

   - Replace `queryTimeline(baseUrl, {}, 200)` oldest-first seeding with `mode: "recent"`.
   - Subscribe SSE from the returned latest cursor.
   - Keep activity and topology buffers bounded.

2. Change identity panel initial load.

   - On panel open, request `mode: "recent"` for that identity.
   - Reconcile and render the first returned page immediately.
   - Do not wait for the full identity transcript before leaving the empty state.

3. Make older history demand-driven.

   - Fetch older pages when the user scrolls near the top.
   - Keep background prefetch optional, bounded, and cancellable on panel close.
   - Preserve the current single identity log and dedupe behavior.

4. Make periodic refresh incremental.

   - The open-panel refresh loop should reconcile each page as it arrives or prefer `since latest known cursor`.
   - It should not restart a full 100-page identity scan every 2 seconds.

## Test Plan

Backend unit/integration tests:

- Global recent query over at least 250k frames returns recent frames, not cursor 0.
- Sparse identity recent query returns latest identity frames without scanning the full aggregate log.
- Query after an invisible gap returns visible frames within a bounded raw scan budget.
- Recent identity query includes a useful turn-start anchor before a noisy recent tail.
- Existing store-local timeline tests continue to prove no broad synchronous session-history refresh.

Frontend unit tests:

- Identity timeline fetch reconciles and renders page 1 before page 2 resolves.
- Opening a chat panel with multi-page history does not remain on "No messages yet" after the first page returns.
- Global activity seed calls `queryTimeline` with recent mode.
- Periodic refresh uses a continuation strategy instead of full oldest-first reload.

Browser E2E:

- Mock a large global log with recent worker frames late in the cursor range; activity shows recent worker activity.
- Mock an identity history split across delayed pages; chat renders after the first page.
- Mock invisible cursor gaps; timeline advances to visible frames without stopping on an empty visible page.
- Verify SSE continuation starts from the recent snapshot cursor, not cursor 0.

Stress target:

- Tests should cover at least Fugue scale, 10x Fugue scale, and a 100x-scale synthetic log with sparse identity frames.

## Rollout Plan

1. Add backend query-mode support and tests.
2. Switch the React console to recent-mode initial loads.
3. Add incremental identity rendering and demand-driven older-history fetch.
4. Run browser E2E against synthetic large logs.
5. Validate against Fugue before release.
6. Release MobKit and update Fugue back to a published crate version rather than a git/dev pin.

## Acceptance Criteria

- Kapellmeister and issue agents appear immediately in the console.
- Opening any Fugue agent chat shows recent transcript within one bounded request.
- Global timeline/activity shows recent issue-agent activity, not ancient startup frames.
- Empty visible pages do not trap pagination behind invisible raw frames.
- Initial load cost is bounded and does not grow linearly with total aggregate log size.
- Behavior remains correct when the aggregate log is 10x-100x larger than the Fugue case.
