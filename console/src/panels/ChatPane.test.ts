import assert from "node:assert/strict";
import test from "node:test";
import * as React from "react";
import { renderToStaticMarkup } from "react-dom/server";

import type { ConversationTimelineEntry } from "@console-core";
import {
  ChatPane,
  TURN_RAIL_MAX_TICKS,
  TURN_RAIL_TICK_PX,
  __chatPaneTest,
  isCanonicalVoiceRowDuringCall,
  windowTurnRail,
} from "./ChatPane";
import type { LiveSpeechItem } from "../lib/voice-session";

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

function durationNotice(id: string, createdAt: string): ConversationTimelineEntry {
  return {
    id, kind: "message", variant: "rich",
    identity: { id: "system", label: "Agent", role: "system" },
    interactionId: "review-interaction", runId: "review-run", createdAt,
    blocks: [{ type: "paragraph", text: "Background review has new evidence." }],
  };
}

const DURATION_REVIEW_ENTRIES: ConversationTimelineEntry[] = [
  { ...message({ id: "review-ask", role: "user", createdAt: "2026-05-20T06:43:02.000Z", text: "Please review the PR." }), interactionId: "review-interaction" },
  durationNotice("review-notice", "2026-05-20T06:43:02.500Z"),
  { ...message({ id: "review-answer", role: "assistant", createdAt: "2026-05-20T06:45:07.000Z", text: "Review complete." }), identity: { ...AGENT, label: "System" }, interactionId: "review-interaction", runId: "review-run" },
];

test("system notice never receives or consumes the assistant work duration", () => {
  const messages = __chatPaneTest.buildChatMessages(DURATION_REVIEW_ENTRIES);
  const notice = messages.find((entry) => entry.sourceEntryId === "review-notice");
  const answer = messages.find((entry) => entry.sourceEntryId === "review-answer");
  assert.equal(notice?.source?.kind, "system");
  assert.equal(notice?.workedFor, undefined);
  assert.equal(answer?.source?.kind, "assistant");
  assert.equal(answer?.workedFor, "2m 5s");
  assert.equal(answer?.interactionId, "review-interaction");
  assert.equal(answer?.runId, "review-run");
  const working = renderChat({ entries: DURATION_REVIEW_ENTRIES, phase: "generating" });
  assert.doesNotMatch(working, /Worked for/);
  assert.match(working, /chat-typing:agent/);
  const done = renderChat({ entries: DURATION_REVIEW_ENTRIES, phase: null });
  assert.equal((done.match(/class="msg__worked"/g) || []).length, 1);
  assert.match(done, /Worked for 2m 5s/);
});

test("system task, tool, peer and reasoning rows preserve assistant timing", () => {
  const base: ConversationTimelineEntry = {
    id: "intervening", kind: "message", variant: "rich", identity: AGENT,
    createdAt: "2026-05-20T06:43:03.000Z",
    interactionId: "review-interaction", runId: "review-run",
  };
  const intervening: ConversationTimelineEntry[] = [
    { ...base, id: "task", taskKind: "progress", taskLabel: "Assistant", blocks: [{ type: "paragraph", text: "Checking evidence." }] },
    { ...base, id: "tool", blocks: [{ type: "tool-call", toolCallId: "check", name: "workgraph_ready", arguments: "{}", status: "success", result: '{"items":[]}' }] },
    { ...base, id: "peer", blocks: [{ type: "tool-call", toolCallId: "peer-check", name: "peer_message", arguments: "{}", status: "success", peerIncoming: true, peerIdentity: "reviewer", peerTarget: "Reviewer", peerBody: "Evidence received." }] },
    { ...base, id: "reasoning", blocks: [{ type: "thinking", text: "Reviewing the evidence." }] },
  ];
  const messages = __chatPaneTest.buildChatMessages([
    DURATION_REVIEW_ENTRIES[0], ...intervening, DURATION_REVIEW_ENTRIES[2],
  ]);
  assert.ok(messages.filter((entry) => intervening.some((source) => source.id === entry.sourceEntryId)).every((entry) => entry.workedFor === undefined));
  assert.equal(messages.find((entry) => entry.sourceEntryId === "review-answer")?.workedFor, "2m 5s");
});

test("a trailing system notice cannot expose a still-working assistant duration", () => {
  const entries = [...DURATION_REVIEW_ENTRIES, durationNotice("later-notice", "2026-05-20T06:45:08.000Z")];
  const working = renderChat({ entries, phase: "generating" });
  assert.match(working, /chat-typing:agent/);
  assert.doesNotMatch(working, /Worked for/);
  const done = renderChat({ entries, phase: null });
  assert.equal((done.match(/class="msg__worked"/g) || []).length, 1);
  assert.match(done, /Worked for 2m 5s/);
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
  liveSpeech?: readonly LiveSpeechItem[];
  voiceCallStartedAt?: number | null;
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
      liveSpeech: args.liveSpeech,
      voiceCallStartedAt: args.voiceCallStartedAt ?? null,
    }),
  );
}

test("live speech renders as distinct provisional rows, never as transcript messages", () => {
  const html = renderChat({
    entries: WORK_ENTRIES,
    phase: null,
    voiceCallStartedAt: Date.parse("2026-05-20T07:00:00.000Z"),
    liveSpeech: [
      { itemId: "item-1", speaker: "user", text: "What is the vault phrase", startedAt: 1, final: true },
      { itemId: "item-2", speaker: "assistant", text: "The vault phrase is", startedAt: 2, final: false },
    ],
  });
  assert.match(html, /chat-live-speech:agent/);
  assert.match(html, /chat-live-row:agent:item-1/);
  assert.match(html, /msg--live msg--live-user/);
  assert.match(html, /msg--live msg--live-assistant/);
  assert.match(html, /msg__live-label[^>]*>live</);
  assert.match(html, /data-live-final="false"/);
  // Provisional rows carry no copy affordance and are not counted as turns.
  const liveBlock = html.slice(html.indexOf("chat-live-speech:agent"));
  assert.doesNotMatch(liveBlock, /Copy (message|turn)/);
  // The two canonical entries still render as ordinary message rows; the live
  // rows are outside every turn container.
  assert.equal((html.match(/class="msg msg--(user|agent)"/g) || []).length, 2);
});

test("canonical rows created during the active call stay hidden until the call ends", () => {
  const callStart = Date.parse("2026-05-20T07:00:00.000Z");
  const entries = [
    ...WORK_ENTRIES,
    message({ id: "spoken-q", role: "user", createdAt: "2026-05-20T07:00:05.000Z", text: "Spoken question" }),
    message({ id: "spoken-a", role: "assistant", createdAt: "2026-05-20T07:00:09.000Z", text: "Spoken answer" }),
  ];
  const during = renderChat({ entries, phase: null, voiceCallStartedAt: callStart });
  assert.match(during, /Review complete\./);
  assert.doesNotMatch(during, /Spoken question/);
  assert.doesNotMatch(during, /Spoken answer/);
  const after = renderChat({ entries, phase: null, voiceCallStartedAt: null });
  assert.match(after, /Spoken question/);
  assert.match(after, /Spoken answer/);
  assert.doesNotMatch(after, /chat-live-speech:agent/);
  // Rows without a timestamp are never hidden.
  const undated = { ...message({ id: "u", role: "user", createdAt: "x", text: "Undated" }), createdAt: undefined };
  assert.equal(isCanonicalVoiceRowDuringCall(undated, callStart), false);
  assert.equal(isCanonicalVoiceRowDuringCall(entries[2], callStart), true);
  assert.equal(isCanonicalVoiceRowDuringCall(entries[0], callStart), false);
});

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

test("windowTurnRail keeps short conversations un-windowed", () => {
  assert.deepEqual(windowTurnRail(1, 560), { start: 0, overflow: 0 });
  assert.deepEqual(windowTurnRail(20, 560), { start: 0, overflow: 0 });
  // Exactly at the height budget: still no windowing.
  const budget = Math.floor(560 / TURN_RAIL_TICK_PX);
  assert.deepEqual(windowTurnRail(Math.min(budget, TURN_RAIL_MAX_TICKS), 560), {
    start: 0,
    overflow: 0,
  });
});

test("windowTurnRail collapses long-running agents to the measured band", () => {
  // 200 turns in a 560px band: newest turns keep ticks, the rest collapse.
  const windowed = windowTurnRail(200, 560);
  assert.ok(windowed.overflow > 0, "long history must window");
  const visible = 200 - windowed.start;
  const budget = Math.min(TURN_RAIL_MAX_TICKS, Math.floor(560 / TURN_RAIL_TICK_PX));
  assert.ok(
    visible + 1 <= budget,
    `visible ticks (${visible}) + overflow slot must fit the budget (${budget})`,
  );
  assert.equal(windowed.start, windowed.overflow);
});

test("windowTurnRail respects tiny panes but never drops below the floor", () => {
  const tiny = windowTurnRail(200, 40); // 4 slots by height -> floor of 6 applies
  assert.ok(200 - tiny.start >= 5, "at least five recent turns stay railed");
  assert.ok(200 - tiny.start <= 6, "tiny panes stay tightly bounded");
});

test("windowTurnRail treats an unmeasured band as the hard ceiling", () => {
  const unmeasured = windowTurnRail(500, null);
  const visible = 500 - unmeasured.start;
  assert.ok(
    visible + 1 <= TURN_RAIL_MAX_TICKS,
    "without a measurement the hard ceiling caps the rail",
  );
});

test("long histories render a windowed rail with the overflow jump tick", () => {
  const entries: ConversationTimelineEntry[] = [];
  for (let index = 0; index < 120; index += 1) {
    entries.push(
      message({
        id: `u-${index}`,
        role: "user",
        createdAt: new Date(1720000000000 + index * 60000).toISOString(),
        text: `question ${index}`,
      }),
      message({
        id: `a-${index}`,
        role: "assistant",
        createdAt: new Date(1720000000000 + index * 60000 + 1000).toISOString(),
        text: `answer ${index}`,
      }),
    );
  }
  const html = renderToStaticMarkup(
    React.createElement(ChatPane, {
      agent: {
        agent_id: "long-agent",
        member_id: "long-agent",
        identity: "long-agent",
        label: "Long Agent",
        kind: "mob_agent",
        role: "worker",
        state: "active",
        model_capabilities: { image_input: false },
      },
      agentLabel: "Long Agent",
      identity: "long-agent",
      entries,
      phase: null,
      draft: "",
      sending: false,
      readOnly: true,
      staged: [],
      onDraftChange: () => undefined,
      onStagedChange: () => undefined,
      onSend: () => true,
    }),
  );
  const tickCount = (html.match(/chat-turn-rail:long-agent:\d+/g) ?? []).length;
  assert.ok(
    tickCount + 1 <= TURN_RAIL_MAX_TICKS,
    `rendered ticks (${tickCount}) must fit the ceiling`,
  );
  assert.ok(
    html.includes("chat-turn-rail:long-agent:overflow"),
    "the collapsed history exposes the overflow jump tick",
  );
  assert.ok(
    html.includes("earlier turns"),
    "the overflow preview names the collapsed count",
  );
});

test("transcript rows carry a typed source header, compact event rows, and day separators", () => {
  const peerEvent: ConversationTimelineEntry = {
    id: "peer-ingested",
    kind: "message",
    variant: "meta",
    identity: { id: "system", label: "System", role: "system" },
    createdAt: "2026-05-19T21:04:24.000Z",
    text: "Received a message from triage:main (homecore mob).",
    runtimeEvent: {
      eventType: "peer_content_ingested",
      kind: "message",
      peer: { id: "978419a8-69f6-5103-8d31-4482e1f76b52", displayName: "homecore/triage/mk--triage_cmain" },
      senderTaint: "tainted",
      payload: { kind: "message", sender_taint: "tainted", type: "peer_content_ingested" },
    },
  };
  const probe: ConversationTimelineEntry = {
    ...message({
      id: "probe",
      role: "user",
      createdAt: "2026-05-20T01:00:29.000Z",
      text: "Operator gate probe. Reply with exactly the token gate-turn-proof-1 and nothing else.",
    }),
  };
  const reply = message({ id: "reply", role: "assistant", createdAt: "2026-05-20T01:00:31.000Z", text: "gate-turn-proof-1" });
  const html = renderToStaticMarkup(
    React.createElement(ChatPane, {
      agent: null,
      agentLabel: "Triage",
      identity: "agent",
      entries: [peerEvent, probe, reply],
      phase: null,
      draft: "",
      sending: false,
      staged: [],
      onDraftChange: () => undefined,
      onStagedChange: () => undefined,
      onSend: () => true,
      peerLabels: new Map([["triage:main", "Triage"]]),
    }),
  );
  // The runtime event reads as a sentence with the roster label, a taint
  // badge, and its raw payload only inside the details disclosure.
  assert.match(html, /Received a message from Triage \(homecore mob\)\./);
  assert.match(html, /untrusted source/);
  assert.doesNotMatch(html, /peer_content_ingested: \{/);
  const beforeDetails = html.slice(0, html.indexOf("<details"));
  assert.doesNotMatch(beforeDetails, /sender_taint/);
  assert.match(html, /<summary>Event details<\/summary>/);
  // The probe has no typed origin, so it is a plain user message: never
  // classified by its text.
  assert.match(html, /class="msg__source">User message</);
  assert.match(html, /class="msg__source">Assistant</);
  // Icon copy buttons, no bare glyph in the text flow.
  assert.doesNotMatch(html, /⎘/);
  assert.match(html, /aria-label="Copy message"/);
  assert.match(html, /data-icon="copy"/);
  // Day separators at the first row and at the local day change.
  const days = html.match(/data-testid="chat-day:agent:[0-9-]+"/g) || [];
  const expectedDays = new Set([
    "2026-05-19T21:04:24.000Z",
    "2026-05-20T01:00:29.000Z",
  ].map((iso) => {
    const d = new Date(iso);
    return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
  }));
  assert.equal(days.length, expectedDays.size);
});

test("consecutive rows from the same assistant share one header", () => {
  const messages = __chatPaneTest.buildChatMessages([
    message({ id: "q", role: "user", createdAt: "2026-05-20T06:43:02.000Z", text: "Question" }),
    message({ id: "a1", role: "assistant", createdAt: "2026-05-20T06:43:05.000Z", text: "Part one" }),
    message({ id: "a2", role: "assistant", createdAt: "2026-05-20T06:43:09.000Z", text: "Part two" }),
  ]);
  assert.deepEqual(
    messages.map((m) => [m.id, m.showHeader]),
    [["q", true], ["a1", true], ["a2", false]],
  );
});

test("distinct assistant interactions retain equal replies and their own headers", () => {
  const base = message({ id: "first", role: "assistant", createdAt: "2026-05-20T06:43:05.000Z", text: "Ready." });
  for (const secondText of ["Ready.", "Acknowledged."]) {
    const messages = __chatPaneTest.buildChatMessages([
      { ...base, interactionId: "11111111-1111-4111-8111-111111111111", runId: "run-first" },
      { ...base, id: "second", text: secondText, interactionId: "22222222-2222-4222-8222-222222222222", runId: "run-second" },
    ]);
    assert.deepEqual(messages.map(row => [row.id, row.showHeader]), [["first", true], ["second", true]]);
  }
});

test("distinct assistant runs in one interaction retain equal replies and headers", () => {
  const base = { ...message({ id: "first", role: "assistant", createdAt: "2026-05-20T06:43:05.000Z", text: "Ready." }), interactionId: "11111111-1111-4111-8111-111111111111" };
  const messages = __chatPaneTest.buildChatMessages([
    { ...base, runId: "run-first" },
    { ...base, id: "second", runId: "run-second" },
  ]);
  assert.deepEqual(messages.map(row => [row.id, row.showHeader]), [["first", true], ["second", true]]);
});

test("Markdown copy and transcript export retain exact source whitespace", () => {
  const source = "  # A heading\n\nA line with a hard break  \nNext line\n\n";
  const entries: ConversationTimelineEntry[] = [{
    id: "raw-markdown", kind: "message", variant: "rich", identity: AGENT,
    blocks: [{ type: "markdown", id: "document", source, streaming: true }],
  }];
  const rows = __chatPaneTest.buildChatMessages(entries);
  assert.equal(__chatPaneTest.msgCopyText(rows[0]), source);
  assert.equal(__chatPaneTest.transcriptCopyText(rows), `Assistant - Agent: ${source}`);
  assert.equal(rows[0].scrollRowId, "raw-markdown");
});

test("Markdown rows remain distinct when whitespace carries source meaning", () => {
  const entries: ConversationTimelineEntry[] = ["one  \ntwo", "one two"].map((source, i) => ({
    id: `raw-${i}`, kind: "message", variant: "rich", identity: AGENT,
    blocks: [{ type: "markdown", id: `document-${i}`, source, streaming: false }],
  }));
  assert.equal(__chatPaneTest.buildChatMessages(entries).length, 2);
});

test("compact stock header retains target actions and destination", () => {
  const html = renderToStaticMarkup(React.createElement(ChatPane, { agent: null, agentLabel: "Agent", identity: "canonical-agent", entries: [], phase: null, draft: "", sending: false, staged: [], onDraftChange: () => {}, onStagedChange: () => {}, onSend: () => true, onInspect: () => {}, headerVariant: "compact" }));
  assert.match(html, /conv__head--compact/);
  assert.match(html, /title="canonical-agent"/);
  assert.match(html, /conv-action:details/);
  assert.match(html, /To:/);
  assert.doesNotMatch(html, /class="conv__identity"/);
});

test("stock folds consecutive proven generic completion but keeps unknown visible", () => {
  const makeTool = (id: string, name: string, known: boolean): ConversationTimelineEntry => ({ id, kind: "message", variant: "rich", identity: AGENT, blocks: [{ type: "tool-call", toolCallId: id, name, arguments: "{}", status: known ? "success" : "pending", completionEvidence: { outcome: known ? "success" : "unknown", source: known ? "session-history" : "unknown", toolCallId: id } }] });
  const html = renderToStaticMarkup(React.createElement(ChatPane, { agent: null, agentLabel: "Agent", identity: "agent", entries: [makeTool("one", "read_file", true), makeTool("two", "list_files", true), makeTool("three", "read_file", false)], phase: null, draft: "", sending: false, staged: [], onDraftChange: () => {}, onStagedChange: () => {}, onSend: () => true }));
  assert.match(html, /2 completed tool calls/);
  assert.match(html, /Completion unknown/);
  assert.match(html, /data-conversation-row-id="one"/);
  assert.match(html, /data-conversation-row-id="two"/);
});
