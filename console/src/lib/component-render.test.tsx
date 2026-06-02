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
