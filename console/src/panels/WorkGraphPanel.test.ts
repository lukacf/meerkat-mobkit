import assert from "node:assert/strict";
import test from "node:test";

import React from "react";
import { renderToStaticMarkup } from "react-dom/server";

import { WorkGraphPanel, __workGraphPanelTest } from "./WorkGraphPanel";
import { WorkGraphGraphView } from "./WorkGraphGraphView";
import type { WorkGraphPanelData } from "./WorkGraphPanel";
import type { WorkGraphWireBinding, WorkGraphWireEdge, WorkGraphWireItem } from "../types";

const {
  buildWorkGraphPanelTree,
  workGraphBindingStatusLabel,
  workGraphBindingTargetLabel,
  workGraphEventLine,
  workGraphOwnerLabelOf,
  workGraphGoalRevisionOf,
  workGraphEventsParams,
  workGraphEventsNewestFirst,
  createWorkGraphRefreshSequencer,
} = __workGraphPanelTest;

function item(id: string, createdAt: string, extra: Partial<WorkGraphWireItem> = {}): WorkGraphWireItem {
  return { id, title: id, created_at: createdAt, status: "open", revision: 1, ...extra };
}

test("workgraph panel tree orders roots by creation and indents children under parents", () => {
  const rows = buildWorkGraphPanelTree(
    [
      item("goal-b", "2026-07-08T09:00:00Z"),
      item("goal-a", "2026-07-08T08:00:00Z"),
      item("child-1", "2026-07-08T08:10:00Z"),
      item("grandchild-1", "2026-07-08T08:20:00Z"),
    ],
    [
      // Parent edges run child→parent.
      { kind: "parent", from_id: "child-1", to_id: "goal-a" },
      { kind: "parent", from_id: "grandchild-1", to_id: "child-1" },
      // Non-parent edges never shape the tree.
      { kind: "blocks", from_id: "goal-b", to_id: "goal-a" },
    ],
  );

  assert.deepEqual(
    rows.map((row) => [row.itemId, row.depth]),
    [
      ["goal-a", 0],
      ["child-1", 1],
      ["grandchild-1", 2],
      ["goal-b", 0],
    ],
  );
});

test("workgraph panel tree treats children of unknown parents as roots", () => {
  const rows = buildWorkGraphPanelTree(
    [item("orphan", "2026-07-08T08:00:00Z")],
    [{ kind: "parent", from_id: "orphan", to_id: "missing-parent" }],
  );
  assert.deepEqual(rows.map((row) => [row.itemId, row.depth]), [["orphan", 0]]);
});

test("workgraph panel tree ignores self-parent cycles", () => {
  const rows = buildWorkGraphPanelTree(
    [item("a", "2026-07-08T08:00:00Z"), item("b", "2026-07-08T08:05:00Z")],
    [
      { kind: "parent", from_id: "a", to_id: "a" },
      { kind: "parent", from_id: "b", to_id: "a" },
    ],
  );
  assert.deepEqual(rows.map((row) => [row.itemId, row.depth]), [["a", 0], ["b", 1]]);
});

test("workgraph binding status labels cover active, paused-with-deadline, and terminal states", () => {
  const binding = (status: WorkGraphWireBinding["status"]): WorkGraphWireBinding => ({
    binding_id: "attention-1",
    status,
  });
  assert.equal(workGraphBindingStatusLabel(binding({ state: "active" })), "active");
  assert.equal(workGraphBindingStatusLabel(binding(undefined)), "active");
  assert.equal(workGraphBindingStatusLabel(binding({ state: "paused" })), "paused");
  assert.equal(
    workGraphBindingStatusLabel(binding({ state: "paused", until: "2026-07-09T10:30:00Z" })),
    "paused until 2026-07-09 10:30",
  );
  assert.equal(workGraphBindingStatusLabel(binding({ state: "superseded" })), "superseded");
  assert.equal(workGraphBindingStatusLabel(binding({ state: "stopped" })), "stopped");
});

test("workgraph binding target labels cover session and lowered-owner targets", () => {
  assert.equal(
    workGraphBindingTargetLabel({ target: { kind: "session", session_id: "sess-42" } }),
    "sess-42",
  );
  assert.equal(
    workGraphBindingTargetLabel({
      target: { kind: "lowered_owner", owner_key: { kind: "agent", id: "planner" } },
    }),
    "agent:planner",
  );
  assert.equal(workGraphBindingTargetLabel({}), "");
});

test("workgraph event lines render timestamp, kind, and item id compactly", () => {
  assert.equal(
    workGraphEventLine({ kind: "item_claimed", at: "2026-07-08T09:15:30Z", item_id: "item-1" }),
    "2026-07-08 09:15 · item claimed · item-1",
  );
  assert.equal(workGraphEventLine({}), "event");
});

test("workgraph goal revision resolves the binding's bound work item, not the binding machine", () => {
  const items: WorkGraphWireItem[] = [
    item("goal-1", "2026-07-08T08:00:00Z", { revision: 4 }),
    item("child-1", "2026-07-08T08:10:00Z", { revision: 2 }),
  ];
  const binding: WorkGraphWireBinding = {
    binding_id: "attention-1",
    work_ref: { item_id: "goal-1" },
    machine_state: { revision: 7 },
  };
  assert.equal(workGraphGoalRevisionOf(binding, items), 4);
  // Unknown bound item / missing work_ref: no token (the action falls back
  // to 0 and surfaces the CAS conflict rather than silently guessing).
  assert.equal(
    workGraphGoalRevisionOf({ binding_id: "b", work_ref: { item_id: "gone" } }, items),
    undefined,
  );
  assert.equal(workGraphGoalRevisionOf({ binding_id: "b" }, items), undefined);
});

test("workgraph events params page from the snapshot high-water mark so the tail never freezes", () => {
  // Upstream returns ASCENDING truncated to limit: a bare {limit} query pins
  // the oldest window once the ledger outgrows it.
  assert.deepEqual(workGraphEventsParams(137, 50), { limit: 50, after_seq: 87 });
  assert.deepEqual(workGraphEventsParams(50, 50), { limit: 50, after_seq: 0 });
  assert.deepEqual(workGraphEventsParams(12, 50), { limit: 50, after_seq: 0 });
  // Fresh store (null mark) and older runtimes (absent mark) fall back to
  // the bare query.
  assert.deepEqual(workGraphEventsParams(null, 50), { limit: 50 });
  assert.deepEqual(workGraphEventsParams(undefined, 50), { limit: 50 });
});

test("workgraph events render newest-first without mutating the wire order", () => {
  const ascending = [{ seq: 1, kind: "a" }, { seq: 2, kind: "b" }, { seq: 3, kind: "c" }];
  const rendered = workGraphEventsNewestFirst(ascending);
  assert.deepEqual(rendered.map((event) => event.seq), [3, 2, 1]);
  assert.deepEqual(ascending.map((event) => event.seq), [1, 2, 3]);
  assert.deepEqual(workGraphEventsNewestFirst([]), []);
});

test("workgraph refresh sequencer invalidates stale refreshes the moment a newer one begins", () => {
  const sequencer = createWorkGraphRefreshSequencer();
  const first = sequencer.begin();
  assert.equal(first(), true);
  const second = sequencer.begin();
  assert.equal(first(), false, "an older refresh must not overwrite a newer one");
  assert.equal(second(), true);
  const third = sequencer.begin();
  assert.equal(second(), false);
  assert.equal(third(), true);
});

// ── Graph view rendering ─────────────────────────────────────────────────

function graphFixture(): { items: WorkGraphWireItem[]; edges: WorkGraphWireEdge[] } {
  return {
    items: [
      item("root", "2026-07-08T08:00:00Z", { title: "Ship it" }),
      item("child-run", "2026-07-08T08:10:00Z", {
        status: "in_progress",
        claim: { owner: { key: { kind: "agent", id: "helper" } } },
      }),
      item("child-done", "2026-07-08T08:20:00Z", { status: "completed" }),
      item("child-stuck", "2026-07-08T08:30:00Z", { status: "blocked" }),
    ],
    edges: [
      { kind: "parent", from_id: "child-run", to_id: "root" },
      { kind: "parent", from_id: "child-done", to_id: "root" },
      { kind: "parent", from_id: "child-stuck", to_id: "root" },
      { kind: "blocks", from_id: "child-run", to_id: "child-stuck" },
    ],
  };
}

function panelData(overrides: Partial<WorkGraphPanelData> = {}): WorkGraphPanelData {
  const fixture = graphFixture();
  return {
    items: fixture.items,
    edges: fixture.edges,
    attention: [],
    events: [],
    capturedAt: "2026-07-08T09:00:00Z",
    unavailable: false,
    denied: false,
    error: null,
    ...overrides,
  };
}

function count(html: string, needle: string): number {
  return html.split(needle).length - 1;
}

test("workgraph panel head renders the tree/graph toggle and defaults to the tree", () => {
  const html = renderToStaticMarkup(
    React.createElement(WorkGraphPanel, {
      data: panelData(),
      canManage: false,
      onRefresh: () => {},
    }),
  );
  assert.ok(html.includes('data-testid="workgraph-view-toggle:tree"'));
  assert.ok(html.includes('data-testid="workgraph-view-toggle:graph"'));
  // Default mode is the tree: item rows render, the graph svg does not.
  assert.ok(html.includes('data-testid="workgraph-panel-item:root"'));
  assert.ok(!html.includes('data-testid="workgraph-graph"'));
  // The existing head affordances stay untouched.
  assert.ok(html.includes('data-testid="workgraph-panel-refresh"'));
});

test("workgraph graph view draws nodes with status classes and typed edges", () => {
  const fixture = graphFixture();
  const html = renderToStaticMarkup(
    React.createElement(WorkGraphGraphView, {
      items: fixture.items,
      edges: fixture.edges,
      attention: [{ binding_id: "b-1", work_ref: { item_id: "root" } }],
      selectedId: "child-run",
    }),
  );
  assert.equal(count(html, 'data-testid="workgraph-graph-node"'), 4);
  assert.equal(count(html, 'data-testid="workgraph-graph-edge"'), 4);
  assert.ok(html.includes("is-in_progress"));
  assert.ok(html.includes("is-completed"));
  assert.ok(html.includes("is-blocked"));
  assert.ok(html.includes('data-kind="blocks"'));
  assert.ok(html.includes("is-selected"));
  assert.ok(html.includes('data-testid="workgraph-graph-viewport"'));
  assert.ok(html.includes('data-testid="workgraph-graph-fit"'));
  // The attention-bound root carries the goal ring.
  assert.ok(html.includes("workgraph-graph__node-goal-ring"));
  // Selection detail footer names the selected item.
  assert.ok(html.includes('data-testid="workgraph-graph-detail"'));
  assert.ok(html.includes("child-run"));
});

test("workgraph graph view reports overflow past the node cap", () => {
  const items = Array.from({ length: 205 }, (_, index) =>
    item(`item-${String(index).padStart(3, "0")}`, "2026-07-08T08:00:00Z"));
  const html = renderToStaticMarkup(
    React.createElement(WorkGraphGraphView, { items, edges: [], attention: [] }),
  );
  assert.equal(count(html, 'data-testid="workgraph-graph-node"'), 200);
  assert.ok(html.includes('data-testid="workgraph-graph-overflow"'));
  assert.ok(html.includes("+5 more items not drawn"));
});

test("workgraph owner labels prefer display names, then key ids, then claim owners", () => {
  assert.equal(
    workGraphOwnerLabelOf({ owner: { key: { kind: "agent", id: "planner" }, display_name: "Planner" } }),
    "Planner",
  );
  assert.equal(
    workGraphOwnerLabelOf({ owner: { key: { kind: "agent", id: "planner" } } }),
    "planner",
  );
  assert.equal(
    workGraphOwnerLabelOf({ claim: { owner: { key: { kind: "session", id: "sess-42" } } } }),
    "sess-42",
  );
  assert.equal(workGraphOwnerLabelOf({}), "");
});
