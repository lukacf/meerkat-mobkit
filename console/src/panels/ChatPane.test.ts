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
