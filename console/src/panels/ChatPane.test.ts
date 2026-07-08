import assert from "node:assert/strict";
import test from "node:test";
import * as React from "react";
import { renderToStaticMarkup } from "react-dom/server";

import type { ConversationTimelineEntry } from "@console-core";
import { ChatPane, __chatPaneTest } from "./ChatPane";

const USER = { id: "user", label: "You", role: "user" as const };
const AGENT = { id: "agent", label: "Agent", role: "assistant" as const };

function message(args: {
  id: string;
  role: "user" | "assistant";
  createdAt: string;
  text: string;
}): ConversationTimelineEntry {
  return {
    id: args.id,
    kind: "message",
    variant: "plain",
    identity: args.role === "user" ? USER : AGENT,
    createdAt: args.createdAt,
    text: args.text,
  };
}

test("chat pane does not count spawn scaffolding as user work", () => {
  const messages = __chatPaneTest.buildChatMessages([
    message({
      id: "spawn",
      role: "user",
      createdAt: "2026-05-20T04:58:01.000Z",
      text: "You have been spawned as 'review:singleton' (role: review) in mob 'ob3'.",
    }),
    message({
      id: "ready",
      role: "assistant",
      createdAt: "2026-05-20T06:43:02.000Z",
      text: "Ready.",
    }),
  ]);

  assert.equal(messages.find((entry) => entry.id === "ready")?.workedFor, undefined);
});

test("chat pane still shows duration for real user turns", () => {
  const messages = __chatPaneTest.buildChatMessages([
    message({
      id: "operator",
      role: "user",
      createdAt: "2026-05-20T06:43:02.000Z",
      text: "Please review the PR.",
    }),
    message({
      id: "done",
      role: "assistant",
      createdAt: "2026-05-20T06:45:07.000Z",
      text: "Review complete.",
    }),
  ]);

  assert.equal(messages.find((entry) => entry.id === "done")?.workedFor, "2m 5s");
});

test("chat pane groups messages into user-addressable scroll turns", () => {
  const messages = __chatPaneTest.buildChatMessages([
    message({
      id: "ask-1",
      role: "user",
      createdAt: "2026-05-20T06:43:02.000Z",
      text: "First request.",
    }),
    message({
      id: "answer-1",
      role: "assistant",
      createdAt: "2026-05-20T06:43:07.000Z",
      text: "First response.",
    }),
    message({
      id: "ask-2",
      role: "user",
      createdAt: "2026-05-20T06:44:02.000Z",
      text: "Second request.",
    }),
    message({
      id: "answer-2",
      role: "assistant",
      createdAt: "2026-05-20T06:44:07.000Z",
      text: "Second response.",
    }),
  ]);

  const turns = __chatPaneTest.buildChatTurns(messages);

  assert.equal(turns.length, 2);
  assert.deepEqual(turns[0].messages.map((entry) => entry.id), ["ask-1", "answer-1"]);
  assert.deepEqual(turns[1].messages.map((entry) => entry.id), ["ask-2", "answer-2"]);
  assert.deepEqual(__chatPaneTest.chatTurnPreview(turns[0]), {
    title: "First request.",
    body: "First response.",
  });
});

test("chat pane renders turn rail markers when multiple turns are present", () => {
  const html = renderChat({
    entries: [
      message({ id: "ask-1", role: "user", createdAt: "2026-05-20T06:43:02.000Z", text: "First request." }),
      message({ id: "answer-1", role: "assistant", createdAt: "2026-05-20T06:43:07.000Z", text: "First response." }),
      message({ id: "ask-2", role: "user", createdAt: "2026-05-20T06:44:02.000Z", text: "Second request." }),
      message({ id: "answer-2", role: "assistant", createdAt: "2026-05-20T06:44:07.000Z", text: "Second response." }),
    ],
    phase: null,
  });

  assert.match(html, /aria-label="Conversation turns"/);
  assert.match(html, /data-testid="chat-turn:agent:0"/);
  assert.match(html, /data-testid="chat-turn:agent:1"/);
  assert.match(html, /data-testid="chat-turn-rail:agent:0"/);
  assert.match(html, /data-testid="chat-turn-rail:agent:1"/);
  assert.match(html, /First request/);
  assert.match(html, /First response/);
});

test("chat pane does not count peer update scaffolding as user work", () => {
  const messages = __chatPaneTest.buildChatMessages([
    message({
      id: "peer-update",
      role: "user",
      createdAt: "2026-05-20T06:43:02.000Z",
      text: "[PEER UPDATE] review:singleton is now idle.",
    }),
    message({
      id: "reply",
      role: "assistant",
      createdAt: "2026-05-20T06:45:07.000Z",
      text: "Ready.",
    }),
  ]);

  assert.equal(messages.find((entry) => entry.id === "reply")?.workedFor, undefined);
});

test("chat pane disables composer in read-only mode", () => {
  const html = renderToStaticMarkup(
    React.createElement(ChatPane, {
      agent: {
        agent_id: "agent",
        member_id: "agent",
        identity: "agent",
        label: "Agent",
        kind: "mob_agent",
        role: "worker",
        state: "active",
        model_capabilities: { image_input: true },
      },
      agentLabel: "Agent",
      identity: "agent",
      entries: [],
      phase: null,
      draft: "hello",
      sending: false,
      readOnly: true,
      staged: [],
      onDraftChange: () => undefined,
      onStagedChange: () => undefined,
      onSend: () => true,
    }),
  );

  assert.match(html, /disabled=""/);
  assert.match(html, /View-only console/);
  assert.match(html, /view only/);
});

function renderChat(args: {
  entries: ConversationTimelineEntry[];
  phase: "waiting" | "tool-executing" | "generating" | null;
  isLoadingHistory?: boolean;
}): string {
  return renderToStaticMarkup(
    React.createElement(ChatPane, {
      agent: {
        agent_id: "agent",
        member_id: "agent",
        identity: "agent",
        label: "Agent",
        kind: "mob_agent",
        role: "worker",
        state: "active",
        model_capabilities: { image_input: true },
      },
      agentLabel: "Agent",
      identity: "agent",
      entries: args.entries,
      phase: args.phase,
      isLoadingHistory: args.isLoadingHistory ?? false,
      draft: "",
      sending: false,
      readOnly: false,
      staged: [],
      onDraftChange: () => undefined,
      onStagedChange: () => undefined,
      onSend: () => true,
    }),
  );
}

const WORK_ENTRIES: ConversationTimelineEntry[] = [
  message({ id: "ask", role: "user", createdAt: "2026-05-20T06:43:02.000Z", text: "Please review the PR." }),
  message({ id: "answer", role: "assistant", createdAt: "2026-05-20T06:45:07.000Z", text: "Review complete." }),
];

test("chat pane shows the working indicator XOR the worked-for summary, never both", () => {
  // While the latest turn is still working, its "Worked for" summary must be
  // suppressed (otherwise it renders alongside the working indicator).
  const working = renderChat({ entries: WORK_ENTRIES, phase: "waiting" });
  assert.match(working, /chat-typing:agent/);
  assert.doesNotMatch(working, /Worked for/);

  // Once the turn is done (phase null) the summary shows and the indicator is gone.
  const done = renderChat({ entries: WORK_ENTRIES, phase: null });
  assert.doesNotMatch(done, /chat-typing:agent/);
  assert.match(done, /Worked for 2m 5s/);
});

test("chat pane shows a loading indicator while an empty session history is fetched", () => {
  const loading = renderChat({ entries: [], phase: null, isLoadingHistory: true });
  assert.match(loading, /Loading conversation/);
  assert.doesNotMatch(loading, /No messages yet/);

  const empty = renderChat({ entries: [], phase: null, isLoadingHistory: false });
  assert.match(empty, /No messages yet/);
  assert.doesNotMatch(empty, /Loading conversation/);
});

// ── WorkGraph inline card ───────────────────────────────────────────────────

const WORKGRAPH_ENTRY: ConversationTimelineEntry = {
  kind: "workgraph",
  id: "workgraph:goal-1",
  identity: AGENT,
  createdAt: "2026-05-20T06:44:00.000Z",
  rootId: "goal-1",
  title: "Release 0.7.30",
  objective: "Ship WorkGraph end to end",
  status: "active",
  progress: { completed: 1, total: 3 },
  items: [
    {
      itemId: "goal-1",
      title: "Release 0.7.30",
      status: "in_progress",
      priority: null,
      ownerLabel: null,
      revision: 4,
      depth: 0,
      parentId: null,
      description: "Ship WorkGraph end to end",
    },
    {
      itemId: "child-1",
      title: "Console card",
      status: "completed",
      priority: null,
      ownerLabel: "Planner",
      revision: 2,
      depth: 1,
      parentId: "goal-1",
    },
    {
      itemId: "child-2",
      title: "SDK parity",
      status: "open",
      priority: "high",
      ownerLabel: null,
      revision: 1,
      depth: 1,
      parentId: "goal-1",
    },
  ],
  attention: [
    {
      bindingId: "attention-1",
      mode: "pursue",
      statusLabel: "active",
      targetLabel: "sess-42",
      revision: 7,
    },
  ],
};

test("chat pane flattens workgraph entries into a dedicated card message", () => {
  const messages = __chatPaneTest.buildChatMessages([
    message({ id: "ask", role: "user", createdAt: "2026-05-20T06:43:02.000Z", text: "Plan the release." }),
    WORKGRAPH_ENTRY,
    message({ id: "answer", role: "assistant", createdAt: "2026-05-20T06:45:07.000Z", text: "On it." }),
  ]);

  const card = messages.find((entry) => entry.kind === "workgraph");
  assert.ok(card);
  assert.equal(card?.workGraphEntry?.rootId, "goal-1");
  // Copy/transcript surfaces get the textual projection.
  assert.match(card?.text || "", /Release 0\.7\.30 \(1\/3\)/);
});

test("chat pane renders the workgraph card inline without action buttons when no callbacks are provided", () => {
  const html = renderChat({
    entries: [
      message({ id: "ask", role: "user", createdAt: "2026-05-20T06:43:02.000Z", text: "Plan the release." }),
      WORKGRAPH_ENTRY,
    ],
    phase: null,
  });

  assert.match(html, /data-work-graph-card/);
  assert.match(html, /data-root-id="goal-1"/);
  assert.match(html, /data-status="active"/);
  assert.match(html, /data-testid="workgraph-card:goal-1"/);
  assert.match(html, /Release 0\.7\.30/);
  assert.match(html, /1\/3/);
  assert.match(html, /Console card/);
  assert.match(html, /pursue/);
  // Undefined-handler convention: no callbacks, no operator buttons.
  assert.doesNotMatch(html, /workgraph-action:/);
  assert.doesNotMatch(html, /workgraph-attention:/);
});

test("chat pane renders workgraph operator buttons only for provided callbacks", () => {
  const html = renderToStaticMarkup(
    React.createElement(ChatPane, {
      agent: {
        agent_id: "agent",
        member_id: "agent",
        identity: "agent",
        label: "Agent",
        kind: "mob_agent",
        role: "worker",
        state: "active",
        model_capabilities: { image_input: true },
      },
      agentLabel: "Agent",
      identity: "agent",
      entries: [WORKGRAPH_ENTRY],
      phase: null,
      draft: "",
      sending: false,
      readOnly: false,
      staged: [],
      onDraftChange: () => undefined,
      onStagedChange: () => undefined,
      onSend: () => true,
      workGraphActions: {
        onClaim: () => undefined,
        onAttentionPause: () => undefined,
      },
    }),
  );

  // Claim renders only on the open, unowned item.
  assert.match(html, /data-testid="workgraph-action:child-2:claim"/);
  assert.doesNotMatch(html, /workgraph-action:child-1:claim/);
  // Close callback was not provided — no Done buttons anywhere.
  assert.doesNotMatch(html, /:close"/);
  // Pause renders on the active binding.
  assert.match(html, /data-testid="workgraph-attention:attention-1:pause"/);
  assert.doesNotMatch(html, /workgraph-attention:attention-1:resume/);
});
