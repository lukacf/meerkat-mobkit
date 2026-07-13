import assert from "node:assert/strict";
import test from "node:test";
import * as React from "react";
import { flushSync } from "react-dom";
import { createRoot } from "react-dom/client";
import { JSDOM } from "jsdom";

import {
  groupConversationTimelineEntries,
  type ConversationFlowRunEntry,
  type ConversationTimelineEntry,
  type ConversationWorkGraphEntry,
} from "@console-core";
import {
  ConsoleActivityRail,
  ConversationRichContent,
  ConversationTranscript,
  WorkGraphCard,
  __workGraphCardUiState,
} from "@console-components";
import { buildWorkGraphOperatorResultFrame, mapFramesToTimelineEntries } from "./adapters";
import { CONSOLE_COMMAND_NAMES } from "./headless";
import { resolveWorkGraphGoalItemRevision, type WorkGraphCommandRunner } from "./workgraph-actions";
import { Sidebar } from "../panels/Sidebar";
import { MemoryLiveStrip } from "../panels/MemoryPanel";
import { WorkGraphPanel, type WorkGraphPanelData } from "../panels/WorkGraphPanel";
import type { ConsoleAgent, ConsoleFrame } from "../types";

test("ConversationRichContent preserves its rendered nodes when streaming becomes final", () => {
  const dom = new JSDOM("<!doctype html><html><body><div id=\"root\"></div></body></html>");
  const previousWindow = globalThis.window;
  const previousDocument = globalThis.document;
  globalThis.window = dom.window as unknown as Window & typeof globalThis;
  globalThis.document = dom.window.document;

  const rootElement = dom.window.document.getElementById("root");
  assert.ok(rootElement);
  const root = createRoot(rootElement);
  const renderContent = (richStyle: "default" | "streaming", text: string) => {
    flushSync(() => {
      root.render(
        <ConversationRichContent
          blocks={[{ type: "paragraph", text }]}
          richStyle={richStyle}
        />,
      );
    });
  };

  try {
    renderContent("streaming", "A response is still arriving");

    const streamingTail = rootElement.querySelector(".cc-rich-paragraph");
    assert.ok(streamingTail);
    assert.strictEqual(rootElement.firstElementChild, streamingTail);
    assert.ok(streamingTail.classList.contains("cc-rich-streaming"));

    renderContent("default", "A response is now complete");

    assert.strictEqual(rootElement.querySelector(".cc-rich-paragraph"), streamingTail);
    assert.strictEqual(rootElement.firstElementChild, streamingTail);
    assert.equal(streamingTail.textContent, "A response is now complete");
    assert.ok(!streamingTail.classList.contains("cc-rich-streaming"));
  } finally {
    root.unmount();
    globalThis.window = previousWindow;
    globalThis.document = previousDocument;
    dom.window.close();
  }
});

test("ConsoleActivityRail wires roster panel actions to host callbacks", async () => {
  const dom = new JSDOM("<!doctype html><html><body><div id=\"root\"></div></body></html>");
  const previousWindow = globalThis.window;
  const previousDocument = globalThis.document;
  globalThis.window = dom.window as unknown as Window & typeof globalThis;
  globalThis.document = dom.window.document;

  const calls: Array<{ panelId: string; actionId: string }> = [];
  const rootElement = dom.window.document.getElementById("root");
  assert.ok(rootElement);
  const root = createRoot(rootElement);

  try {
    flushSync(() => {
      root.render(
        <ConsoleActivityRail
          Icon={({ name }) => <span aria-hidden="true" data-icon={name} />}
          viewState={{
            collapsed: false,
            panels: [
              {
                kind: "roster",
                id: "team",
                title: "Team",
                actions: [{ id: "refresh", label: "Refresh" }],
                groups: [],
              },
            ],
          }}
          onTogglePicker={() => undefined}
          onCollapse={() => undefined}
          onPanelAction={(panelId, actionId) => calls.push({ panelId, actionId })}
          renderSlotPreview={() => null}
        />,
      );
    });

    const button = dom.window.document.querySelector("[data-testid='activity-action:team:refresh']");
    assert.ok(button);
    button.dispatchEvent(new dom.window.MouseEvent("click", { bubbles: true }));

    assert.deepEqual(calls, [{ panelId: "team", actionId: "refresh" }]);
  } finally {
    root.unmount();
    globalThis.window = previousWindow;
    globalThis.document = previousDocument;
    dom.window.close();
  }
});

test("stock Sidebar keyboard reorder announces movement and retains focus", async () => {
  const dom = new JSDOM("<!doctype html><html><body><div id=\"root\"></div></body></html>", {
    url: "http://console.test",
  });
  const previousWindow = globalThis.window;
  const previousDocument = globalThis.document;
  globalThis.window = dom.window as unknown as Window & typeof globalThis;
  globalThis.document = dom.window.document;

  const agents: ConsoleAgent[] = [
    {
      agent_id: "agent-alpha",
      member_id: "agent-alpha",
      identity: "identity:alpha",
      label: "Agent Alpha",
      kind: "mob_agent",
      role: "worker",
      group: "Alpha",
    },
    {
      agent_id: "agent-beta",
      member_id: "agent-beta",
      identity: "identity:beta",
      label: "Agent Beta",
      kind: "mob_agent",
      role: "worker",
      group: "Beta",
    },
  ];
  const rootElement = dom.window.document.getElementById("root");
  assert.ok(rootElement);
  const root = createRoot(rootElement);

  try {
    flushSync(() => {
      root.render(
        <Sidebar
          agents={agents}
          selectedMemberId="agent-alpha"
          recentActivity={[]}
          collapsed={false}
          grouping={{ group_by: ["group"] }}
          storageNamespace="component-interaction"
          onSelect={() => undefined}
          onOpenControl={() => undefined}
        />,
      );
    });

    const alpha = dom.window.document.querySelector("[data-testid='sidebar-section-toggle:Alpha']") as HTMLButtonElement | null;
    assert.ok(alpha);
    alpha.focus();
    assert.equal(dom.window.document.activeElement, alpha);

    alpha.dispatchEvent(new dom.window.KeyboardEvent("keydown", {
      key: "ArrowDown",
      altKey: true,
      bubbles: true,
    }));
    await new Promise((resolve) => setTimeout(resolve, 0));

    const live = dom.window.document.querySelector("[data-testid='sidebar-reorder-live']");
    assert.equal(live?.textContent, "Moved section Alpha after Beta.");
    const focused = dom.window.document.activeElement as HTMLElement | null;
    assert.equal(focused?.getAttribute("data-testid"), "sidebar-section-toggle:Alpha");
    assert.deepEqual(
      JSON.parse(dom.window.localStorage.getItem("mobkit-console-sidebar-section-order:component-interaction") || "[]"),
      ["Beta", "Alpha"],
    );
  } finally {
    root.unmount();
    globalThis.window = previousWindow;
    globalThis.document = previousDocument;
    dom.window.close();
  }
});

test("stock Sidebar keyboard reorder preserves virtual scroll and restores focus", async () => {
  const dom = new JSDOM("<!doctype html><html><body><div id=\"root\"></div></body></html>", {
    url: "http://console.test",
  });
  const previousWindow = globalThis.window;
  const previousDocument = globalThis.document;
  globalThis.window = dom.window as unknown as Window & typeof globalThis;
  globalThis.document = dom.window.document;
  Object.defineProperty(dom.window.HTMLElement.prototype, "clientHeight", {
    configurable: true,
    get() {
      return this.classList?.contains("sidebar__virtual-list") ? 160 : 0;
    },
  });

  const agents: ConsoleAgent[] = Array.from({ length: 50 }, (_value, index) => {
    const label = `Group ${String(index).padStart(2, "0")}`;
    return {
      agent_id: `agent-${index}`,
      member_id: `agent-${index}`,
      identity: `identity:${index}`,
      label: `Agent ${index}`,
      kind: "mob_agent",
      role: "worker",
      group: label,
    };
  });
  const rootElement = dom.window.document.getElementById("root");
  assert.ok(rootElement);
  const root = createRoot(rootElement);

  try {
    flushSync(() => {
      root.render(
        <Sidebar
          agents={agents}
          selectedMemberId="agent-12"
          recentActivity={[]}
          collapsed={false}
          grouping={{ group_by: ["group"] }}
          storageNamespace="component-interaction-virtual"
          onSelect={() => undefined}
          onOpenControl={() => undefined}
        />,
      );
    });
    await new Promise((resolve) => setTimeout(resolve, 0));

    const list = dom.window.document.querySelector("[data-testid='sidebar-agent-list']") as HTMLDivElement | null;
    assert.ok(list);
    list.scrollTop = 1200;
    list.dispatchEvent(new dom.window.Event("scroll", { bubbles: true }));
    await new Promise((resolve) => setTimeout(resolve, 0));

    const group12 = dom.window.document.querySelector("[data-testid='sidebar-section-toggle:Group 12']") as HTMLButtonElement | null;
    assert.ok(group12);
    group12.focus();
    group12.dispatchEvent(new dom.window.KeyboardEvent("keydown", {
      key: "ArrowDown",
      altKey: true,
      bubbles: true,
    }));
    await new Promise((resolve) => setTimeout(resolve, 0));
    await new Promise((resolve) => setTimeout(resolve, 0));

    assert.notEqual(list.scrollTop, 0);
    assert.equal(
      dom.window.document.activeElement?.getAttribute("data-testid"),
      "sidebar-section-toggle:Group 12",
    );
    const live = dom.window.document.querySelector("[data-testid='sidebar-reorder-live']");
    assert.equal(live?.textContent, "Moved section Group 12 after Group 13.");
  } finally {
    root.unmount();
    globalThis.window = previousWindow;
    globalThis.document = previousDocument;
    dom.window.close();
  }
});

test("MemoryLiveStrip pause-on-scroll freezes, jump-to-live resumes, top auto-unfreezes", async () => {
  const dom = new JSDOM("<!doctype html><html><body><div id=\"root\"></div></body></html>");
  const previousWindow = globalThis.window;
  const previousDocument = globalThis.document;
  globalThis.window = dom.window as unknown as Window & typeof globalThis;
  globalThis.document = dom.window.document;

  const frame = (id: string): ConsoleFrame =>
    ({
      id,
      event: "memory.dream.completed",
      timestampMs: Date.now(),
      data: { realm: "default", run_id: id },
    }) as ConsoleFrame;

  const rootElement = dom.window.document.getElementById("root");
  assert.ok(rootElement);
  const root = createRoot(rootElement);
  const render = (frames: ConsoleFrame[]) => {
    flushSync(() => {
      root.render(<MemoryLiveStrip frames={frames} onPivot={() => undefined} />);
    });
  };
  const query = (selector: string) => dom.window.document.querySelector(selector);
  // Scroll-driven state updates run at continuous priority; a follow-up
  // flushSync render commits them so assertions observe the new state.
  const scrollTo = (list: HTMLElement, top: number, frames: ConsoleFrame[]) => {
    list.scrollTop = top;
    list.dispatchEvent(new dom.window.Event("scroll", { bubbles: true }));
    render(frames);
  };

  try {
    const initial = [frame("f-2"), frame("f-1")]; // newest first
    render(initial);
    assert.ok(query("[data-testid='memory-live-row:f-2']"));
    assert.ok(query("[data-testid='memory-live-seam']"), "ring seam marker renders");

    // Scrolling away from the top freezes the visible list.
    const list = query(".memory-live__list") as HTMLElement;
    assert.ok(list);
    scrollTo(list, 100, initial);

    // New frames arrive while paused: rows stay frozen, badge counts them.
    const second = [frame("f-3"), ...initial];
    render(second);
    assert.equal(query("[data-testid='memory-live-row:f-3']"), null, "frozen list holds");
    const jump = query("[data-testid='memory-live-jump']");
    assert.ok(jump, "N-behind badge renders while paused");
    assert.match(jump?.textContent || "", /1 behind/);

    // Jump-to-live clears the freeze and renders the new frames (click is a
    // discrete event — the flushSync wrapper commits it synchronously).
    flushSync(() => {
      jump?.dispatchEvent(new dom.window.MouseEvent("click", { bubbles: true }));
    });
    assert.ok(query("[data-testid='memory-live-row:f-3']"), "resumed list shows new frames");
    assert.equal(query("[data-testid='memory-live-jump']"), null);

    // Scrolling back to the top with nothing behind auto-unfreezes: the next
    // frame renders immediately, with no badge.
    scrollTo(list, 100, second); // freeze again
    scrollTo(list, 0, second); // back to top, behind === 0 → unfreeze
    render([frame("f-4"), ...second]);
    assert.ok(query("[data-testid='memory-live-row:f-4']"), "auto-unfrozen at top");
    assert.equal(query("[data-testid='memory-live-jump']"), null);
  } finally {
    root.unmount();
    globalThis.window = previousWindow;
    globalThis.document = previousDocument;
    dom.window.close();
  }
});

test("WorkGraphCard actions carry the right CAS token per class: goal actions the item revision, attention actions the binding revision", async () => {
  const dom = new JSDOM("<!doctype html><html><body><div id=\"root\"></div></body></html>");
  const previousWindow = globalThis.window;
  const previousDocument = globalThis.document;
  globalThis.window = dom.window as unknown as Window & typeof globalThis;
  globalThis.document = dom.window.document;

  const calls: Array<{ action: string; input: Record<string, unknown> }> = [];
  const record = (action: string) => (input: Record<string, unknown>) => calls.push({ action, input });
  const entry: ConversationWorkGraphEntry = {
    kind: "workgraph",
    id: "workgraph:goal-1",
    identity: { id: "planner", label: "Planner", role: "assistant" },
    rootId: "goal-1",
    title: "Release 0.7.30",
    status: "active",
    progress: { completed: 0, total: 1 },
    items: [
      { itemId: "goal-1", title: "Release 0.7.30", status: "in_progress", revision: 4, depth: 0 },
    ],
    attention: [
      // Binding machine revision 7 vs bound goal item revision 4 — the two
      // CAS classes must not leak into each other.
      { bindingId: "b-coord", mode: "coordinate", statusLabel: "active", revision: 7, itemId: "goal-1" },
      { bindingId: "b-pursue", mode: "pursue", statusLabel: "active", revision: 9, itemId: "goal-1" },
    ],
  };

  const rootElement = dom.window.document.getElementById("root");
  assert.ok(rootElement);
  const root = createRoot(rootElement);
  try {
    flushSync(() => {
      root.render(
        <WorkGraphCard
          entry={entry}
          actions={{
            onGoalConfirm: record("confirm"),
            onGoalRequestClose: record("request-close"),
            onAttentionPause: record("pause"),
            onAttentionResume: record("resume"),
            onAttentionReassign: record("reassign"),
          }}
        />,
      );
    });

    const click = (testId: string) => {
      const button = dom.window.document.querySelector(`[data-testid='${testId}']`);
      assert.ok(button, `expected button ${testId}`);
      button.dispatchEvent(new dom.window.MouseEvent("click", { bubbles: true }));
    };
    click("workgraph-attention:b-coord:confirm");
    click("workgraph-attention:b-coord:request-close");
    click("workgraph-attention:b-coord:pause");
    click("workgraph-attention:b-coord:reassign");
    assert.deepEqual(calls, [
      { action: "confirm", input: { bindingId: "b-coord", revision: 4 } },
      { action: "request-close", input: { bindingId: "b-coord", revision: 4 } },
      { action: "pause", input: { bindingId: "b-coord", revision: 7 } },
      { action: "reassign", input: { bindingId: "b-coord", revision: 7 } },
    ]);

    // Reassign is coordinate-only (upstream derives the authority from the
    // binding mode); pursue bindings render no reassign affordance.
    assert.equal(
      dom.window.document.querySelector("[data-testid='workgraph-attention:b-pursue:reassign']"),
      null,
    );
    assert.ok(dom.window.document.querySelector("[data-testid='workgraph-attention:b-pursue:pause']"));
  } finally {
    root.unmount();
    globalThis.window = previousWindow;
    globalThis.document = previousDocument;
    dom.window.close();
  }
});

test("WorkGraphCard goal actions on an unfolded goal item carry NO revision so the live-resolution path runs", async () => {
  const dom = new JSDOM("<!doctype html><html><body><div id=\"root\"></div></body></html>");
  const previousWindow = globalThis.window;
  const previousDocument = globalThis.document;
  globalThis.window = dom.window as unknown as Window & typeof globalThis;
  globalThis.document = dom.window.document;

  const calls: Array<{ action: string; input: { bindingId: string; revision?: number } }> = [];
  const record = (action: string) => (input: { bindingId: string; revision?: number }) =>
    calls.push({ action, input });
  // The binding names a goal item the fold never observed; the card root has
  // a revision of its own which must NOT be substituted for it.
  const entry: ConversationWorkGraphEntry = {
    kind: "workgraph",
    id: "workgraph:root-1",
    identity: { id: "planner", label: "Planner", role: "assistant" },
    rootId: "root-1",
    title: "Root card",
    status: "active",
    progress: { completed: 0, total: 1 },
    items: [
      { itemId: "root-1", title: "Root card", status: "in_progress", revision: 4, depth: 0 },
    ],
    attention: [
      { bindingId: "b-unfolded", mode: "pursue", statusLabel: "active", revision: 7, itemId: "goal-unfolded" },
    ],
  };

  const rootElement = dom.window.document.getElementById("root");
  assert.ok(rootElement);
  const root = createRoot(rootElement);
  try {
    flushSync(() => {
      root.render(
        <WorkGraphCard
          entry={entry}
          actions={{ onGoalConfirm: record("confirm"), onGoalRequestClose: record("request-close") }}
        />,
      );
    });
    const click = (testId: string) => {
      const button = dom.window.document.querySelector(`[data-testid='${testId}']`);
      assert.ok(button, `expected button ${testId}`);
      button.dispatchEvent(new dom.window.MouseEvent("click", { bubbles: true }));
    };
    click("workgraph-attention:b-unfolded:confirm");
    click("workgraph-attention:b-unfolded:request-close");
    assert.deepEqual(calls, [
      { action: "confirm", input: { bindingId: "b-unfolded", revision: undefined } },
      { action: "request-close", input: { bindingId: "b-unfolded", revision: undefined } },
    ], "neither the root's revision 4 nor the binding's machine revision 7 leaks into the goal payload");

    // The revision-less payload is exactly what drives the host's live
    // resolution (revisionOr → resolveWorkGraphGoalItemRevision): the same
    // dispatch the app performs resolves the goal item via goal/status.
    const queries: Array<{ command: string; params: Record<string, unknown> }> = [];
    const runner: WorkGraphCommandRunner = async (command, params) => {
      queries.push({ command, params });
      return { item: { id: "goal-unfolded", revision: 12 } };
    };
    const payload = calls[0].input;
    const expectedRevision = payload.revision !== undefined
      ? payload.revision
      : await resolveWorkGraphGoalItemRevision(runner, payload.bindingId);
    assert.equal(expectedRevision, 12, "the CAS token comes from the live read, never another item");
    assert.deepEqual(queries, [
      { command: CONSOLE_COMMAND_NAMES.workgraphGoalStatus, params: { binding_id: "b-unfolded" } },
    ]);
  } finally {
    root.unmount();
    globalThis.window = previousWindow;
    globalThis.document = previousDocument;
    dom.window.close();
  }
});

test("WorkGraphCard expansion and collapse survive the catch-all→rooted entry rekey via the stable uiStateKey anchor", async () => {
  const dom = new JSDOM("<!doctype html><html><body><div id=\"root\"></div></body></html>");
  const previousWindow = globalThis.window;
  const previousDocument = globalThis.document;
  globalThis.window = dom.window as unknown as Window & typeof globalThis;
  globalThis.document = dom.window.document;
  __workGraphCardUiState.reset();

  const uiStateKey = "workgraph:interaction:turn-1";
  const item = {
    itemId: "item-mig",
    title: "Migrating goal",
    status: "open",
    revision: 1,
    depth: 0,
    description: "Expandable detail",
  };
  const catchAllEntry: ConversationWorkGraphEntry = {
    kind: "workgraph",
    id: uiStateKey,
    uiStateKey,
    identity: { id: "planner", label: "Planner", role: "assistant" },
    rootId: "interaction:turn-1",
    title: "Migrating goal",
    status: "active",
    progress: { completed: 0, total: 1 },
    items: [item],
    attention: [],
  };
  // The same graph after hierarchy formed: new entry id (→ React remounts
  // the card), same uiStateKey and item ids.
  const rootedEntry: ConversationWorkGraphEntry = {
    ...catchAllEntry,
    id: "workgraph:item-mig",
    rootId: "item-mig",
    items: [
      item,
      { itemId: "item-mig-child", title: "Child task", status: "open", revision: 1, depth: 1 },
    ],
    progress: { completed: 0, total: 2 },
  };

  const rootElement = dom.window.document.getElementById("root");
  assert.ok(rootElement);
  const root = createRoot(rootElement);
  const render = (entry: ConversationWorkGraphEntry) => {
    flushSync(() => {
      // Keyed by entry id exactly like the transcript renderer, so the id
      // migration really remounts the subtree.
      root.render(<WorkGraphCard key={entry.id} entry={entry} />);
    });
  };
  const click = (testId: string) => {
    const button = dom.window.document.querySelector(`[data-testid='${testId}']`);
    assert.ok(button, `expected button ${testId}`);
    // flushSync so the expansion state commits before the assertions read
    // the DOM.
    flushSync(() => {
      button.dispatchEvent(new dom.window.MouseEvent("click", { bubbles: true }));
    });
  };
  const detailVisible = () =>
    dom.window.document.querySelector(".cc-work-graph__item-detail") !== null;

  try {
    render(catchAllEntry);
    assert.equal(detailVisible(), false);
    click("workgraph-item:item-mig");
    assert.equal(detailVisible(), true, "clicking the row expands the item detail");

    render(rootedEntry);
    assert.equal(
      detailVisible(),
      true,
      "item expansion carries across the catch-all→rooted remount",
    );

    // Collapse the migrated card, rekey back (e.g. a live refold), and the
    // collapse must stick too — it is keyed on the uiStateKey anchor.
    click("workgraph-card:item-mig:toggle");
    assert.equal(
      dom.window.document.querySelector(".cc-work-graph__items"),
      null,
      "collapsing hides the item list",
    );
    render({ ...rootedEntry, id: "workgraph:item-mig-rekeyed" });
    assert.equal(
      dom.window.document.querySelector(".cc-work-graph__items"),
      null,
      "collapse state carries across a further rekey",
    );
  } finally {
    root.unmount();
    __workGraphCardUiState.reset();
    globalThis.window = previousWindow;
    globalThis.document = previousDocument;
    dom.window.close();
  }
});

test("WorkGraphCard row detail lists additional parents as an also-under note", async () => {
  const dom = new JSDOM("<!doctype html><html><body><div id=\"root\"></div></body></html>");
  const previousWindow = globalThis.window;
  const previousDocument = globalThis.document;
  globalThis.window = dom.window as unknown as Window & typeof globalThis;
  globalThis.document = dom.window.document;
  __workGraphCardUiState.reset();

  const entry: ConversationWorkGraphEntry = {
    kind: "workgraph",
    id: "workgraph:goal-a",
    identity: { id: "planner", label: "Planner", role: "assistant" },
    rootId: "goal-a",
    title: "Goal A",
    status: "active",
    progress: { completed: 0, total: 2 },
    items: [
      { itemId: "goal-a", title: "Goal A", status: "open", revision: 1, depth: 0 },
      // Placed under its first parent; the second parent surfaces as detail.
      { itemId: "child-shared", title: "Shared child", status: "open", revision: 1, depth: 1, parentId: "goal-a", alsoUnder: ["Goal B"] },
    ],
    attention: [],
  };

  const rootElement = dom.window.document.getElementById("root");
  assert.ok(rootElement);
  const root = createRoot(rootElement);
  try {
    flushSync(() => {
      root.render(<WorkGraphCard entry={entry} />);
    });
    const row = dom.window.document.querySelector("[data-testid='workgraph-item:child-shared']");
    assert.ok(row, "expected the shared child row");
    assert.equal(row.getAttribute("aria-expanded"), "false", "the also-under note alone makes the row expandable");
    flushSync(() => {
      row.dispatchEvent(new dom.window.MouseEvent("click", { bubbles: true }));
    });
    const note = dom.window.document.querySelector("[data-testid='workgraph-item:child-shared:also-under']");
    assert.ok(note, "expected the also-under note in the expanded detail");
    assert.equal(note.textContent, "also under Goal B");
  } finally {
    root.unmount();
    __workGraphCardUiState.reset();
    globalThis.window = previousWindow;
    globalThis.document = previousDocument;
    dom.window.close();
  }
});

test("WorkGraphCard actions after a CAS conflict send the refetched revision, not the wedged one", async () => {
  // The full heal loop the console runs on a -32042 conflict: the failed
  // mutation echoes its error, the refetch echoes the live entity (marked
  // `refresh`), and the refolded card's NEXT action carries the fresh token.
  const dom = new JSDOM("<!doctype html><html><body><div id=\"root\"></div></body></html>");
  const previousWindow = globalThis.window;
  const previousDocument = globalThis.document;
  globalThis.window = dom.window as unknown as Window & typeof globalThis;
  globalThis.document = dom.window.document;
  __workGraphCardUiState.reset();

  const agent = { agent_id: "planner", member_id: "planner", label: "Planner", kind: "identity" };
  const item = (revision: number, status: string) => ({
    id: "item-cas",
    realm_id: "realm-1",
    namespace: "default",
    title: "Wedged target",
    status,
    priority: "medium",
    revision,
    created_at: "2026-07-08T09:00:00Z",
    updated_at: "2026-07-08T09:00:00Z",
  });
  const frames = [
    {
      id: "wg-cas-call",
      event: "tool_call_requested",
      interactionId: "turn-cas",
      timestampMs: 1_779_405_464_000,
      data: { id: "wg-cas-tc", name: "workgraph_create", args: { title: "Wedged target" } },
    },
    {
      id: "wg-cas-done",
      event: "tool_execution_completed",
      interactionId: "turn-cas",
      timestampMs: 1_779_405_464_200,
      data: { id: "wg-cas-tc", name: "workgraph_create", result: JSON.stringify({ item: item(1, "open") }) },
    },
    // The claim sent revision 1 while the item had moved to 5: conflict…
    buildWorkGraphOperatorResultFrame({
      method: "mobkit/workgraph/claim",
      params: { id: "item-cas", expected_revision: 1, owner: { kind: "principal", id: "ops@example.com" } },
      errorMessage: "workgraph conflict: stale revision 1, item is at 5",
      identity: "planner",
      timestampMs: 1_779_405_465_000,
      frameId: "local-wg-cas-fail",
    }),
    // …and the automatic post-conflict re-read folds the live state.
    buildWorkGraphOperatorResultFrame({
      method: "mobkit/workgraph/get",
      params: { id: "item-cas" },
      result: { item: item(5, "in_progress") },
      identity: "planner",
      timestampMs: 1_779_405_465_100,
      frameId: "local-wg-cas-refresh",
      refresh: true,
    }),
  ];
  const cards = mapFramesToTimelineEntries(agent, frames)
    .filter((entry): entry is ConversationWorkGraphEntry => entry.kind === "workgraph");
  assert.equal(cards.length, 1);
  assert.equal(cards[0].lastActionFailed, true, "the conflict stays visible on the card");

  const calls: Array<{ action: string; input: Record<string, unknown> }> = [];
  const record = (action: string) => (input: Record<string, unknown>) => calls.push({ action, input });
  const rootElement = dom.window.document.getElementById("root");
  assert.ok(rootElement);
  const root = createRoot(rootElement);
  try {
    flushSync(() => {
      root.render(<WorkGraphCard entry={cards[0]} actions={{ onClose: record("close") }} />);
    });
    const button = dom.window.document.querySelector("[data-testid='workgraph-action:item-cas:close']");
    assert.ok(button, "expected the Done affordance");
    button.dispatchEvent(new dom.window.MouseEvent("click", { bubbles: true }));
    assert.deepEqual(calls, [
      { action: "close", input: { itemId: "item-cas", revision: 5 } },
    ], "the subsequent action payload carries the refetched revision");
  } finally {
    root.unmount();
    __workGraphCardUiState.reset();
    globalThis.window = previousWindow;
    globalThis.document = previousDocument;
    dom.window.close();
  }
});

test("flow-run member rows expose status and a labelled bounded transcript when expanded", () => {
  const dom = new JSDOM("<!doctype html><html><body><div id=\"root\"></div></body></html>");
  const previousWindow = globalThis.window;
  const previousDocument = globalThis.document;
  globalThis.window = dom.window as unknown as Window & typeof globalThis;
  globalThis.document = dom.window.document;

  const memberEntries: ConversationTimelineEntry[] = [
    {
      id: "builder-message",
      kind: "message",
      variant: "plain",
      identity: { id: "builder", label: "Builder", role: "other" },
      text: "Implemented the shared flow-run presentation.",
    },
  ];
  const flowRunEntry: ConversationFlowRunEntry = {
    id: "flow-run:release-crew",
    kind: "flow_run",
    identity: { id: "coordinator", label: "Coordinator", role: "assistant" },
    helperId: "helper-1",
    flowName: "Release crew",
    status: "running",
    rows: [
      {
        memberKey: "builder",
        label: "Builder",
        caption: "Implementing the shared component",
        status: "running",
        subView: {
          conversationId: "builder-transcript",
          entries: memberEntries,
          groups: groupConversationTimelineEntries(memberEntries),
          turnDiff: null,
          emptyState: null,
        },
      },
    ],
  };

  const rootElement = dom.window.document.getElementById("root");
  assert.ok(rootElement);
  const root = createRoot(rootElement);
  try {
    flushSync(() => {
      root.render(
        <ConversationTranscript
          viewState={{
            conversationId: "fixture",
            entries: [flowRunEntry],
            groups: groupConversationTimelineEntries([flowRunEntry]),
            turnDiff: null,
            emptyState: null,
          }}
        />,
      );
    });

    const row = dom.window.document.querySelector("button.cc-flow-run__member-row");
    assert.ok(row);
    assert.match(row.textContent || "", /Builder.*Working/);
    assert.equal(row.getAttribute("aria-expanded"), "false");
    const detailId = row.getAttribute("aria-controls");
    assert.ok(detailId);

    flushSync(() => {
      row.dispatchEvent(new dom.window.MouseEvent("click", { bubbles: true }));
    });

    assert.equal(row.getAttribute("aria-expanded"), "true");
    const region = dom.window.document.querySelector(
      "[role='region'][aria-label='Builder transcript']",
    ) as HTMLElement | null;
    assert.ok(region);
    assert.equal(region.id, detailId);
    assert.equal(region.tabIndex, 0);
    assert.match(region.textContent || "", /Implemented the shared flow-run presentation/);
  } finally {
    root.unmount();
    globalThis.window = previousWindow;
    globalThis.document = previousDocument;
    dom.window.close();
  }
});

test("WorkGraphPanel attention actions split CAS tokens the same way and gate reassign to coordinate mode", async () => {
  const dom = new JSDOM("<!doctype html><html><body><div id=\"root\"></div></body></html>");
  const previousWindow = globalThis.window;
  const previousDocument = globalThis.document;
  globalThis.window = dom.window as unknown as Window & typeof globalThis;
  globalThis.document = dom.window.document;

  const calls: Array<{ action: string; input: Record<string, unknown> }> = [];
  const record = (action: string) => (input: Record<string, unknown>) => calls.push({ action, input });
  const data: WorkGraphPanelData = {
    items: [
      { id: "goal-1", title: "Release 0.7.30", status: "in_progress", revision: 4, created_at: "2026-07-08T08:00:00Z" },
    ],
    edges: [],
    attention: [
      {
        binding_id: "b-coord",
        work_ref: { item_id: "goal-1" },
        mode: "coordinate",
        status: { state: "active" },
        machine_state: { revision: 7 },
      },
      {
        binding_id: "b-pursue",
        work_ref: { item_id: "goal-1" },
        mode: "pursue",
        status: { state: "active" },
        machine_state: { revision: 9 },
      },
    ],
    events: [],
    capturedAt: "2026-07-08T09:00:00Z",
    unavailable: false,
    denied: false,
    error: null,
  };

  const rootElement = dom.window.document.getElementById("root");
  assert.ok(rootElement);
  const root = createRoot(rootElement);
  try {
    flushSync(() => {
      root.render(
        <WorkGraphPanel
          data={data}
          canManage
          onRefresh={() => undefined}
          onGoalConfirm={record("confirm")}
          onGoalRequestClose={record("request-close")}
          onAttentionPause={record("pause")}
          onAttentionResume={record("resume")}
          onAttentionReassign={record("reassign")}
        />,
      );
    });

    const bindingRow = (bindingId: string) => {
      const row = dom.window.document.querySelector(`[data-testid='workgraph-panel-binding:${bindingId}']`);
      assert.ok(row, `expected binding row ${bindingId}`);
      return row;
    };
    const clickByLabel = (row: Element, label: string) => {
      const button = [...row.querySelectorAll("button")].find((candidate) => candidate.textContent === label);
      assert.ok(button, `expected "${label}" button`);
      // flushSync so state updates (the reassign popover) commit before the
      // next query.
      flushSync(() => {
        button.dispatchEvent(new dom.window.MouseEvent("click", { bubbles: true }));
      });
    };

    const coordRow = bindingRow("b-coord");
    clickByLabel(coordRow, "Confirm");
    clickByLabel(coordRow, "Request close");
    clickByLabel(coordRow, "Pause");
    assert.deepEqual(calls, [
      { action: "confirm", input: { bindingId: "b-coord", revision: 4 } },
      { action: "request-close", input: { bindingId: "b-coord", revision: 4 } },
      { action: "pause", input: { bindingId: "b-coord", revision: 7 } },
    ]);

    // Reassign renders only on the coordinate binding (upstream derives the
    // authority from the binding mode).
    const pursueRow = bindingRow("b-pursue");
    assert.equal(
      [...pursueRow.querySelectorAll("button")].some((candidate) => candidate.textContent === "Reassign"),
      false,
      "pursue bindings expose no reassign affordance",
    );
    clickByLabel(coordRow, "Reassign");
    const input = dom.window.document.querySelector("[data-testid='workgraph-panel-reassign-input:b-coord']");
    assert.ok(input, "reassign popover opens for coordinate bindings");
    const submit = dom.window.document.querySelector(
      "[data-testid='workgraph-panel-reassign-submit:b-coord']",
    ) as HTMLButtonElement | null;
    assert.ok(submit);
    assert.equal(submit.disabled, true, "submit stays disabled until an identity is typed");
    // A disabled submit never fires the callback.
    flushSync(() => {
      submit.dispatchEvent(new dom.window.MouseEvent("click", { bubbles: true }));
    });
    assert.equal(calls.some((call) => call.action === "reassign"), false);
  } finally {
    root.unmount();
    globalThis.window = previousWindow;
    globalThis.document = previousDocument;
    dom.window.close();
  }
});
