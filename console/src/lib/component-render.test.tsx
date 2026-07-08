import assert from "node:assert/strict";
import test from "node:test";
import * as React from "react";
import { renderToStaticMarkup } from "react-dom/server";

import {
  groupConversationTimelineEntries,
  normalizeConsoleNavigationModel,
  type ConversationTimelineEntry,
} from "@console-core";
import {
  ConsoleComposer,
  ConsoleWorkbench,
  ConversationTranscript,
  WorkGraphCard,
} from "@console-components";

function HorizontalNavigation() {
  const model = normalizeConsoleNavigationModel({
    orientation: "horizontal",
    activeNodeId: "thread:alpha",
    nodes: [
      {
        type: "group",
        id: "projects",
        label: "Projects",
        expanded: true,
        children: [
          { type: "item", id: "project:alpha", label: "Project Alpha" },
          { type: "item", id: "thread:alpha", label: "Planning Thread", selected: true },
        ],
      },
    ],
    order: { orderedNodeIds: [] },
  });

  return (
    <nav aria-label="Host navigation" data-orientation={model.orientation}>
      {model.nodes.flatMap((node) => node.type === "group" ? node.children : [node]).map((node) => (
        <button aria-current={node.selected ? "page" : undefined} key={node.id} type="button">
          {node.label}
        </button>
      ))}
    </nav>
  );
}

test("alternate shell renders non-sidebar navigation with MobKit transcript and composer components", () => {
  const entries: ConversationTimelineEntry[] = [
    {
      id: "user-1",
      kind: "message",
      variant: "plain",
      identity: { id: "user", label: "You", role: "user", presentation: "user" },
      text: "Use a horizontal host navigator.",
    },
    {
      id: "assistant-1",
      kind: "message",
      variant: "plain",
      identity: { id: "assistant", label: "Assistant", role: "assistant" },
      text: "MobKit transcript and composer still render.",
    },
  ];
  const html = renderToStaticMarkup(
    <ConsoleWorkbench
      launcher={<HorizontalNavigation />}
      main={(
        <ConversationTranscript
          viewState={{
            conversationId: "fixture",
            entries,
            groups: groupConversationTimelineEntries(entries),
            turnDiff: null,
            emptyState: null,
          }}
        />
      )}
      mainFooter={(
        <ConsoleComposer
          viewState={{
            value: "next prompt",
            placeholder: "Send to the selected MobKit target",
            mainRowItems: [],
            footerLeftItems: [],
            footerRightItems: [],
          }}
          onChange={() => undefined}
          onSubmit={() => undefined}
        />
      )}
    />,
  );

  assert.match(html, /data-orientation="horizontal"/);
  assert.match(html, /Project Alpha/);
  assert.match(html, /Planning Thread/);
  assert.match(html, /MobKit transcript and composer still render/);
  assert.match(html, /Send to the selected MobKit target/);
});

test("workgraph entries render as cards through the shared transcript, with action buttons gated on callbacks", () => {
  const workGraphEntry: ConversationTimelineEntry = {
    kind: "workgraph",
    id: "workgraph:goal-1",
    identity: { id: "planner", label: "Planner", role: "assistant" },
    rootId: "goal-1",
    title: "Release 0.7.30",
    objective: "Ship WorkGraph end to end",
    status: "active",
    progress: { completed: 1, total: 2 },
    items: [
      { itemId: "goal-1", title: "Release 0.7.30", status: "in_progress", revision: 4, depth: 0 },
      { itemId: "child-1", title: "Console card", status: "open", revision: 1, depth: 1, parentId: "goal-1" },
    ],
    attention: [
      { bindingId: "attention-1", mode: "pursue", statusLabel: "active", targetLabel: "sess-42", revision: 7 },
    ],
  };

  const observedHtml = renderToStaticMarkup(
    <ConversationTranscript
      viewState={{
        conversationId: "fixture",
        entries: [workGraphEntry],
        groups: groupConversationTimelineEntries([workGraphEntry]),
        turnDiff: null,
        emptyState: null,
      }}
    />,
  );

  assert.match(observedHtml, /data-work-graph-card/);
  assert.match(observedHtml, /data-root-id="goal-1"/);
  assert.match(observedHtml, /data-status="active"/);
  assert.match(observedHtml, /Release 0\.7\.30/);
  assert.match(observedHtml, /1\/2/);
  assert.match(observedHtml, /pursue/);
  // Observational transcript: no callbacks, no operator buttons.
  assert.doesNotMatch(observedHtml, /workgraph-action:/);
  assert.doesNotMatch(observedHtml, /workgraph-attention:/);

  const managedHtml = renderToStaticMarkup(
    <WorkGraphCard
      entry={workGraphEntry}
      actions={{
        onClaim: () => undefined,
        onClose: () => undefined,
        onAttentionPause: () => undefined,
      }}
    />,
  );

  assert.match(managedHtml, /data-testid="workgraph-action:child-1:claim"/);
  assert.match(managedHtml, /data-testid="workgraph-action:goal-1:close"/);
  assert.match(managedHtml, /data-testid="workgraph-attention:attention-1:pause"/);
  // Callbacks not provided render no affordance.
  assert.doesNotMatch(managedHtml, /workgraph-attention:attention-1:resume/);
  assert.doesNotMatch(managedHtml, /workgraph-attention:attention-1:confirm/);
  // No failure flag → no failed indicator.
  assert.doesNotMatch(managedHtml, /workgraph-card:goal-1:last-action-failed/);
});

test("workgraph card gates reassign to coordinate-mode bindings and surfaces the last-action-failed flag", () => {
  const entry = (mode: string, lastActionFailed?: boolean): ConversationTimelineEntry => ({
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
      { bindingId: "attention-1", mode, statusLabel: "active", revision: 7, itemId: "goal-1" },
    ],
    ...(lastActionFailed ? { lastActionFailed } : {}),
  });
  const actions = { onAttentionReassign: () => undefined };

  // Reassign authority is machine-derived from the binding mode upstream:
  // only coordinate-mode bindings render the affordance.
  const pursueHtml = renderToStaticMarkup(<WorkGraphCard entry={entry("pursue")} actions={actions} />);
  assert.doesNotMatch(pursueHtml, /workgraph-attention:attention-1:reassign/);
  const coordinateHtml = renderToStaticMarkup(<WorkGraphCard entry={entry("coordinate")} actions={actions} />);
  assert.match(coordinateHtml, /data-testid="workgraph-attention:attention-1:reassign"/);

  const failedHtml = renderToStaticMarkup(<WorkGraphCard entry={entry("pursue", true)} />);
  assert.match(failedHtml, /data-testid="workgraph-card:goal-1:last-action-failed"/);
});
