import assert from "node:assert/strict";
import test from "node:test";

import {
  appendOptimisticConversationEntry,
  buildActivityRailViewState,
  buildQuickPromptSuggestions,
  buildRoutingSectionView,
  buildSidebarViewState,
  describeMemoryTimelineEvent,
  inferResponsePhaseFromFrames,
  mapFramesToTimelineEntries,
  mergeConversationFrames,
  optimisticUserMessageForPanel,
  resolvePanelResponsePhase,
  sortConversationTimelineEntries,
  stripPeerTransportScaffold,
  systemNoticeClearsBusyState,
} from "./adapters";
import { describeMemoryTimelineEvent as describeMemoryTimelineEventCore } from "@console-core";

function typedCommsNotice(args: {
  peer: string;
  body: string;
  peerId?: string;
  peerDisplayName?: string;
  kind?: string;
  direction?: "incoming" | "outgoing";
  intent?: string;
  requestId?: string;
  payload?: unknown;
}) {
  const kind = args.kind || "message";
  return {
    role: "system_notice",
    kind: "comms",
    body: args.body,
    blocks: [{
      type: "comms",
      kind,
      direction: args.direction || "incoming",
      peer: {
        id: args.peerId || args.peer,
        display_name: args.peerDisplayName || args.peer,
      },
      request_id: args.requestId || `req-${args.peer}-${kind}`,
      ...(args.intent ? { intent: args.intent } : {}),
      ...(args.payload !== undefined ? { payload: args.payload } : {}),
      content: [{ type: "text", text: args.body }],
    }],
  };
}

test("buildSidebarViewState preserves host-derived watch and degraded fields", () => {
  const viewState = buildSidebarViewState({
    selectedMemberId: "member-1",
    pinnedAgentIds: new Set(["member-1"]),
    agents: [
      {
        agent_id: "identity:luka",
        member_id: "member-1",
        label: "Luka",
        kind: "operator",
        role: "console",
        state: "running",
        watched: true,
        alertLevel: "elevated",
        degraded: true,
        degradedReason: "lease_expired",
      },
    ],
  });

  const item = viewState.blocks[1]?.sections?.[0]?.items?.[0];
  assert.equal(item?.id, "member-1");
  assert.equal(item?.pinned, true);
  assert.equal(item?.selected, true);
  assert.equal(item?.watched, true);
  assert.equal(item?.alertLevel, "elevated");
  assert.equal(item?.degraded, true);
  assert.equal(item?.degradedReason, "lease_expired");
  assert.equal(item?.meta?.[0]?.tone, "accent");
});

test("buildRoutingSectionView projects runtime routing and delivery results without host invention", () => {
  const view = buildRoutingSectionView({
    routesResponse: {
      routes: [
        {
          route_key: "vip-route",
          recipient: "vip@example.com",
          channel: "notification",
          sink: "sms",
          target_module: "delivery",
          retry_max: 0,
          backoff_ms: 5,
          rate_limit_per_minute: 9,
        },
      ],
    },
    historyResponse: {
      deliveries: [
        {
          delivery_id: "delivery-1",
          route_id: "route-000001",
          recipient: "vip@example.com",
          sink: "sms",
          target_module: "delivery",
          status: "sent",
          first_attempt_ms: 100,
          final_attempt_ms: 200,
          idempotency_key: "delivery-key-1",
          sink_adapter: "sms-mock",
          attempts: [
            { attempt: 1, status: "sent", backoff_ms: 0 },
          ],
        },
      ],
    },
  });

  assert.deepEqual(view, {
    routes: [
      {
        route_key: "vip-route",
        recipient: "vip@example.com",
        channel: "notification",
        sink: "sms",
        target_module: "delivery",
        retry_max: 0,
        backoff_ms: 5,
        rate_limit_per_minute: 9,
      },
    ],
    deliveries: [
      {
        delivery_id: "delivery-1",
        route_id: "route-000001",
        recipient: "vip@example.com",
        sink: "sms",
        target_module: "delivery",
        status: "sent",
        first_attempt_ms: 100,
        final_attempt_ms: 200,
        idempotency_key: "delivery-key-1",
        sink_adapter: "sms-mock",
        attempts: [
          { attempt: 1, status: "sent", backoff_ms: 0 },
        ],
      },
    ],
  });
});

test("mapFramesToTimelineEntries renders a partial assistant message while deltas are still streaming", () => {
  // Regression: the live-overlay path used to pass `renderTextDeltas: false`,
  // which dropped text_delta frames on the floor so the conversation pane
  // stayed visually empty until `interaction_complete` arrived (user-visible
  // as "waiting… persists while the agent is actually typing").
  const entries = mapFramesToTimelineEntries(
    {
      agent_id: "incident-commander",
      member_id: "incident-commander",
      label: "Incident Commander",
      kind: "identity",
    },
    [
      { id: "evt-1", event: "interaction_started", data: {} },
      { id: "evt-2", event: "text_delta", data: { delta: "Status " } },
      { id: "evt-3", event: "text_delta", data: { delta: "is " } },
      { id: "evt-4", event: "text_delta", data: { delta: "stable." } },
    ],
  );

  // One partial message entry with all three deltas concatenated.
  assert.equal(entries.length, 1);
  assert.equal(entries[0]?.kind, "message");
  const entry = entries[0]!;
  const text = "text" in entry
    ? entry.text
    : "blocks" in entry && Array.isArray(entry.blocks) && entry.blocks[0]?.type === "paragraph"
      ? entry.blocks[0].text
      : "";
  assert.equal(text, "Status is stable.");
});

test("mapFramesToTimelineEntries keeps timestamp-less reasoning before the answer text", () => {
  // Regression: reasoning frames frequently arrive without a timestamp. The
  // transcript sort used to send timestamp-less frames to MAX_SAFE_INTEGER, so
  // reasoning sorted AFTER the answer text and rendered "thinking" at the end of
  // the turn. Here the reasoning frame has no timestampMs and is listed after the
  // text frames; it must still render before the answer.
  const entries = mapFramesToTimelineEntries(
    { agent_id: "a", member_id: "a", label: "A", kind: "identity" },
    [
      { id: "evt-1", event: "interaction_started", interactionId: "int-1", timestampMs: 1000, data: {} },
      { id: "evt-3", event: "text_delta", interactionId: "int-1", timestampMs: 1200, data: { delta: "The answer." } },
      { id: "evt-4", event: "text_complete", interactionId: "int-1", timestampMs: 1200, data: { content: "The answer." } },
      { id: "evt-2", event: "reasoning_delta", interactionId: "int-1", data: { delta: "Thinking first." } },
      { id: "evt-5", event: "interaction_complete", interactionId: "int-1", timestampMs: 1300, data: {} },
    ],
  );
  const thinkingIdx = entries.findIndex(
    (e) => "blocks" in e && Array.isArray(e.blocks) && e.blocks.some((b) => b.type === "thinking"),
  );
  const answerIdx = entries.findIndex((e) => {
    const t = "text" in e
      ? e.text
      : "blocks" in e && Array.isArray(e.blocks) && e.blocks[0]?.type === "paragraph"
        ? e.blocks[0].text
        : "";
    return typeof t === "string" && t.includes("The answer.");
  });
  assert.ok(thinkingIdx >= 0, "thinking entry present");
  assert.ok(answerIdx >= 0, "answer entry present");
  assert.ok(thinkingIdx < answerIdx, `thinking (${thinkingIdx}) should precede answer (${answerIdx})`);
});

test("mapFramesToTimelineEntries keeps incomplete streamed markdown tails conservative", () => {
  const entries = mapFramesToTimelineEntries(
    {
      agent_id: "risk-red-team",
      member_id: "risk-red-team",
      label: "Risk Red Team",
      kind: "identity",
    },
    [
      { id: "evt-1", event: "text_delta", data: { delta: "## Risk\n\n" } },
      { id: "evt-2", event: "text_delta", data: { delta: "- first\n- seco" } },
    ],
  );

  assert.equal(entries.length, 1);
  const blocks = "blocks" in entries[0]! ? entries[0].blocks : [];
  assert.equal(blocks?.[0]?.type, "heading");
  assert.equal(blocks?.[1]?.type, "paragraph");
  assert.equal(blocks?.[1]?.type === "paragraph" ? blocks[1].text : "", "- first\n- seco");
});

test("mapFramesToTimelineEntries reparses completed streamed markdown normally", () => {
  const entries = mapFramesToTimelineEntries(
    {
      agent_id: "risk-red-team",
      member_id: "risk-red-team",
      label: "Risk Red Team",
      kind: "identity",
    },
    [
      {
        id: "evt-1",
        event: "text_delta",
        interactionId: "turn-1",
        data: { delta: "## Risk\n\n- first\n- second" },
      },
      {
        id: "evt-2",
        event: "interaction_complete",
        interactionId: "turn-1",
        data: {},
      },
    ],
  );

  const assistant = entries.find((entry) => entry.kind === "message" && entry.identity.role === "assistant");
  const blocks = assistant && "blocks" in assistant ? assistant.blocks : [];
  assert.equal(blocks?.[0]?.type, "heading");
  assert.equal(blocks?.[1]?.type, "paragraph");
  assert.equal(blocks?.[1]?.type === "paragraph" ? blocks[1].text : "", "first\nsecond");
});

test("mapFramesToTimelineEntries renders run_started parent prompts as the inbound turn", () => {
  const entries = mapFramesToTimelineEntries(
    {
      agent_id: "person-worker-alpha",
      member_id: "person-worker-alpha",
      label: "Person Worker",
      kind: "identity",
    },
    [
      {
        id: "tool-first-in-cursor-order",
        event: "tool_call_requested",
        cursor: "console:10",
        timestampMs: 200,
        data: { id: "call-1", name: "king_search", arguments: "{}" },
      },
      {
        id: "run-started-backfilled-late",
        event: "run_started",
        cursor: "console:11",
        timestampMs: 100,
        data: {
          prompt: "Peer message from parent: audit this person",
        },
      },
    ],
    { renderInteractionStartsAsUser: true },
  );

  assert.equal(entries.length, 2);
  assert.equal(entries[0]?.kind, "message");
  assert.equal(entries[0]?.identity.role, "user");
  assert.equal("text" in entries[0]! ? entries[0].text : "", "Peer message from parent: audit this person");
  assert.equal(entries[1]?.kind, "message");
  assert.equal(entries[1]?.identity.role, "assistant");
  assert.equal(
    entries[1] && "blocks" in entries[1] && Array.isArray(entries[1].blocks)
      ? entries[1].blocks[0]?.type
      : "",
    "tool-call",
  );
});

test("mapFramesToTimelineEntries preserves repeated identical run_started parent prompts", () => {
  const entries = mapFramesToTimelineEntries(
    {
      agent_id: "person-worker-alpha",
      member_id: "person-worker-alpha",
      label: "Person Worker",
      kind: "identity",
    },
    [
      {
        id: "run-started-1",
        event: "run_started",
        timestampMs: Date.parse("2026-05-23T20:29:50.000Z"),
        data: { prompt: "continue" },
      },
      {
        id: "run-started-2",
        event: "run_started",
        timestampMs: Date.parse("2026-05-23T20:30:50.000Z"),
        data: { prompt: "continue" },
      },
    ],
    { renderInteractionStartsAsUser: true },
  );

  assert.equal(entries.length, 2);
  assert.deepEqual(
    entries.map((entry) => ("text" in entry ? entry.text : "")),
    ["continue", "continue"],
  );
});

test("mapFramesToTimelineEntries suppresses duplicate terminal text after streamed deltas", () => {
  const entries = mapFramesToTimelineEntries(
    {
      agent_id: "incident-commander",
      member_id: "incident-commander",
      label: "Incident Commander",
      kind: "identity",
    },
    [
      {
        id: "evt-1",
        event: "text_delta",
        data: { delta: "Hello! How can I assist you today?" },
      },
      {
        id: "evt-2",
        event: "interaction_complete",
        data: { text: "Hello! How can I assist you today?" },
      },
    ],
  );

  assert.equal(entries.length, 1);
  assert.equal(entries[0]?.kind, "message");
  assert.equal(
    entries[0] && "blocks" in entries[0] && Array.isArray(entries[0].blocks)
      ? entries[0].blocks[0]?.type === "paragraph"
        ? entries[0].blocks[0].text
        : ""
      : "",
    "Hello! How can I assist you today?",
  );
});

test("mapFramesToTimelineEntries suppresses session-history terminal text already delivered live", () => {
  const entries = mapFramesToTimelineEntries(
    {
      agent_id: "incident-commander",
      member_id: "incident-commander",
      label: "Incident Commander",
      kind: "identity",
    },
    [
      {
        id: "evt-live-delta",
        event: "text_delta",
        timestampMs: 10,
        data: { delta: "Acknowledged. Standing by." },
      },
      {
        id: "evt-live-complete",
        event: "interaction_complete",
        timestampMs: 20,
        data: { result: "Acknowledged. Standing by." },
      },
      {
        id: "history-user",
        event: "user_input",
        sourceKind: "session_history",
        timestampMs: 30,
        data: { content: "You have been spawned as incident-commander." },
      },
      {
        id: "history-assistant",
        event: "interaction_complete",
        sourceKind: "session_history",
        timestampMs: 31,
        data: {
          text: "Acknowledged. Standing by.",
          result: "Acknowledged. Standing by.",
          source_event_type: "session_history",
        },
      },
    ],
    { renderInteractionStartsAsUser: true },
  );

  const assistantTexts = entries
    .filter((entry) => entry.kind === "message" && entry.identity.role === "assistant")
    .map((entry) => {
      if ("text" in entry) return entry.text;
      if ("blocks" in entry && Array.isArray(entry.blocks)) {
        return entry.blocks
          .map((block) => block.type === "paragraph" ? block.text : "")
          .join("");
      }
      return "";
    });

  assert.deepEqual(assistantTexts, ["Acknowledged. Standing by."]);
});

test("mapFramesToTimelineEntries ignores text_complete so the terminal event does not duplicate the same answer", () => {
  const entries = mapFramesToTimelineEntries(
    {
      agent_id: "incident-commander",
      member_id: "incident-commander",
      label: "Incident Commander",
      kind: "identity",
    },
    [
      {
        id: "evt-1",
        event: "text_delta",
        data: { delta: "Status is stable." },
      },
      {
        id: "evt-2",
        event: "text_complete",
        data: { content: "Status is stable." },
      },
      {
        id: "evt-3",
        event: "interaction_complete",
        data: { text: "Status is stable." },
      },
    ],
  );

  assert.equal(entries.length, 1);
  assert.equal(
    entries[0] && "blocks" in entries[0] && Array.isArray(entries[0].blocks)
      ? entries[0].blocks[0]?.type === "paragraph"
        ? entries[0].blocks[0].text
        : ""
      : "",
    "Status is stable.",
  );
});

test("mapFramesToTimelineEntries ignores live text_complete before matching interaction completion", () => {
  const entries = mapFramesToTimelineEntries(
    {
      agent_id: "incident-commander",
      member_id: "incident-commander",
      label: "Incident Commander",
      kind: "identity",
    },
    [
      {
        id: "evt-text-complete",
        event: "text_complete",
        interactionId: "turn-1",
        data: { content: "I’m online as incident-commander." },
      },
      {
        id: "evt-run-complete",
        event: "interaction_complete",
        interactionId: "turn-1",
        data: { result: "I’m online as incident-commander." },
      },
    ],
  );

  assert.equal(entries.length, 1);
  assert.equal(
    entries[0] && "blocks" in entries[0] && Array.isArray(entries[0].blocks)
      ? entries[0].blocks[0]?.type === "paragraph"
        ? entries[0].blocks[0].text
        : ""
      : entries[0] && "text" in entries[0]
        ? entries[0].text
        : "",
    "I’m online as incident-commander.",
  );
});

test("mapFramesToTimelineEntries ignores hidden turn markers before terminal completion", () => {
  const entries = mapFramesToTimelineEntries(
    {
      agent_id: "incident-commander",
      member_id: "incident-commander",
      label: "Incident Commander",
      kind: "identity",
    },
    [
      { id: "evt-1", event: "text_delta", data: { delta: "Status is stable." } },
      { id: "evt-2", event: "text_complete", data: { content: "Status is stable." } },
      { id: "evt-3", event: "turn_completed", data: { stop_reason: "end_turn" } },
      { id: "evt-4", event: "interaction_complete", data: { text: "Status is stable." } },
    ],
  );

  assert.equal(entries.length, 1);
  assert.equal(
    entries[0] && "blocks" in entries[0] && Array.isArray(entries[0].blocks)
      ? entries[0].blocks[0]?.type === "paragraph"
        ? entries[0].blocks[0].text
        : ""
      : "",
    "Status is stable.",
  );
});

test("inferResponsePhaseFromFrames clears working state on terminal text and terminal turn-completed frames", () => {
  assert.equal(
    inferResponsePhaseFromFrames([
      { id: "evt-1", event: "user_input", data: { content: "Hello" } },
    ]),
    "waiting",
  );

  assert.equal(
    inferResponsePhaseFromFrames([
      { id: "evt-1", event: "user_input", status: "completed", data: { content: "Hello" } },
    ]),
    null,
  );

  assert.equal(
    inferResponsePhaseFromFrames([
      { id: "evt-1", event: "text_delta", data: { delta: "Done." } },
      { id: "evt-2", event: "text_complete", data: { content: "Done." } },
      { id: "evt-3", event: "interaction_complete", data: { result: "Done." } },
      { id: "evt-4", event: "user_input", status: "completed", data: { content: "Hello" } },
    ]),
    null,
  );

  assert.equal(
    inferResponsePhaseFromFrames([
      { id: "evt-1", event: "text_delta", data: { delta: "Done." } },
      { id: "evt-2", event: "text_complete", data: { content: "Done." } },
    ]),
    null,
  );

  assert.equal(
    inferResponsePhaseFromFrames([
      { id: "evt-1", event: "run_started", data: { prompt: "Hello" } },
      { id: "evt-2", event: "text_delta", data: { delta: "Done." } },
      { id: "evt-3", event: "text_complete", data: { content: "Done." } },
    ]),
    "waiting",
  );

  assert.equal(
    inferResponsePhaseFromFrames([
      { id: "evt-1", event: "run_started", data: { prompt: "Hello" } },
      { id: "evt-2", event: "text_delta", data: { delta: "Done." } },
      { id: "evt-3", event: "text_complete", data: { content: "Done." } },
      { id: "evt-4", event: "run_completed", data: { result: "Done." } },
    ]),
    null,
  );

  assert.equal(
    inferResponsePhaseFromFrames([
      { id: "evt-1", event: "interaction_started", data: {} },
      { id: "evt-2", event: "run_started", data: { prompt: "Step one" } },
      { id: "evt-3", event: "text_complete", data: { content: "Step one done." } },
      { id: "evt-4", event: "run_completed", data: { result: "Step one done." } },
    ]),
    "waiting",
  );

  assert.equal(
    inferResponsePhaseFromFrames([
      { id: "evt-1", event: "interaction_started", data: {} },
      { id: "evt-2", event: "run_started", data: { prompt: "Step one" } },
      { id: "evt-3", event: "text_complete", data: { content: "Step one done." } },
      { id: "evt-4", event: "run_completed", data: { result: "Step one done." } },
      { id: "evt-5", event: "interaction_complete", data: { result: "All done." } },
    ]),
    null,
  );

  assert.equal(
    inferResponsePhaseFromFrames([
      { id: "evt-1", event: "text_delta", data: { delta: "Done." } },
      { id: "evt-2", event: "turn_completed", data: { stop_reason: "end_turn" } },
    ]),
    null,
  );

  assert.equal(
    inferResponsePhaseFromFrames([
      { id: "evt-1", event: "interaction_started", data: {} },
      { id: "evt-2", event: "text_delta", data: { delta: "Done." } },
      { id: "evt-3", event: "turn_completed", data: { stop_reason: "end_turn" } },
    ]),
    "waiting",
  );

  assert.equal(
    inferResponsePhaseFromFrames([
      { id: "evt-1", event: "tool_execution_started", data: {} },
      { id: "evt-2", event: "turn_completed", data: { stop_reason: "tool_use" } },
    ]),
    "tool-executing",
  );

  assert.equal(
    inferResponsePhaseFromFrames([
      { id: "evt-1", event: "text_delta", data: { delta: "Partial." } },
      { id: "evt-2", event: "turn_completed", data: { stop_reason: "max_tokens" } },
    ]),
    null,
  );

  assert.equal(
    inferResponsePhaseFromFrames([
      { id: "evt-1", event: "text_delta", data: { delta: "Stopped." } },
      { id: "evt-2", event: "turn_completed", data: { stop_reason: "stop_sequence" } },
    ]),
    null,
  );

  // Tool-completion events should not claim "tool-executing" — at that
  // instant no tool is running — but they must still keep the turn visibly
  // active until terminal text/run evidence arrives. Spawned workers can
  // lack run_started/interaction_started projections; clearing here lets
  // operator sends bypass the local queue while the worker is still active.
  assert.equal(
    inferResponsePhaseFromFrames([
      { id: "evt-1", event: "text_delta", data: { delta: "Done." } },
      { id: "evt-2", event: "text_complete", data: { content: "Done." } },
      { id: "evt-3", event: "tool_call_requested", data: { name: "save_investigation_result" } },
      { id: "evt-4", event: "tool_execution_started", data: {} },
      { id: "evt-5", event: "tool_execution_completed", data: {} },
    ]),
    "waiting",
  );

  assert.equal(
    inferResponsePhaseFromFrames([
      { id: "evt-1", event: "tool_call_requested", data: { name: "save_investigation_result" } },
      { id: "evt-2", event: "tool_result_received", data: {} },
    ]),
    "waiting",
  );

  assert.equal(
    inferResponsePhaseFromFrames([
      { id: "evt-1", event: "tool_execution_completed", data: {} },
      { id: "evt-2", event: "turn_completed", data: { stop_reason: "end_turn" } },
    ]),
    null,
  );

  assert.equal(
    inferResponsePhaseFromFrames([
      { id: "evt-1", event: "tool_execution_completed", data: {} },
      {
        id: "evt-2",
        event: "system_notice",
        data: {
          blocks: [{
            content: [{ type: "text", text: "Peer message from worker:\nDone." }],
          }],
        },
      },
    ]),
    null,
  );

  assert.equal(
    inferResponsePhaseFromFrames([
      { id: "evt-1", event: "tool_execution_completed", data: {} },
      { id: "evt-2", event: "reasoning_delta", data: { delta: "Planning next step" } },
    ]),
    "generating",
  );

  // A new tool call after a completed one should re-arm the indicator.
  assert.equal(
    inferResponsePhaseFromFrames([
      { id: "evt-1", event: "tool_execution_completed", data: {} },
      { id: "evt-2", event: "tool_call_requested", data: { name: "next_tool" } },
    ]),
    "tool-executing",
  );

  assert.equal(
    inferResponsePhaseFromFrames([
      {
        id: "evt-1",
        event: "server_tool_content",
        data: {
          content: {
            id: "ws-1",
            name: "web_search",
            type: "response.web_search_call.searching",
          },
        },
      },
    ]),
    "tool-executing",
  );

  assert.equal(
    inferResponsePhaseFromFrames([
      {
        id: "evt-1",
        event: "server_tool_content",
        data: {
          content: {
            id: "ws-1",
            name: "web_search",
            type: "response.web_search_call.in_progress",
          },
        },
      },
      {
        id: "evt-2",
        event: "server_tool_content",
        data: {
          content: {
            id: "ws-1",
            name: "web_search",
            type: "web_search_call",
            status: "completed",
          },
        },
      },
    ]),
    "waiting",
  );

  assert.equal(
    inferResponsePhaseFromFrames([
      {
        id: "evt-1",
        event: "server_tool_content",
        data: {
          content: {
            id: "msg-1",
            name: "web_search_annotations",
            type: "message_annotations",
            annotations: [{ title: "Example", url: "https://example.com" }],
          },
        },
      },
    ]),
    null,
  );

  assert.equal(
    inferResponsePhaseFromFrames([
      {
        id: "evt-1",
        event: "server_tool_content",
        data: {
          content: {
            id: "ws-1",
            name: "web_search",
            type: "response.web_search_call.in_progress",
          },
        },
      },
      { id: "evt-2", event: "interaction_complete", data: { result: "Done." } },
    ]),
    null,
  );
});

test("resolvePanelResponsePhase lets local terminal history clear stale server phase", () => {
  assert.equal(
    resolvePanelResponsePhase({
      frames: [],
      serverPhase: "generating",
    }),
    "generating",
  );

  assert.equal(
    resolvePanelResponsePhase({
      frames: [
        { id: "evt-1", event: "interaction_started", timestampMs: 1, data: { content: "Work" } },
        { id: "evt-2", event: "text_delta", timestampMs: 2, data: { delta: "Done." } },
        { id: "evt-3", event: "interaction_complete", timestampMs: 3, data: { result: "Done." } },
      ],
      serverPhase: "generating",
    }),
    null,
  );

  assert.equal(
    resolvePanelResponsePhase({
      frames: [
        { id: "evt-1", event: "run_started", timestampMs: 1, data: { prompt: "Work" } },
        { id: "evt-2", event: "tool_call_requested", timestampMs: 2, data: { name: "king_search" } },
        { id: "evt-3", event: "tool_execution_completed", timestampMs: 3, data: {} },
      ],
      serverPhase: "tool-executing",
    }),
    "waiting",
  );

  assert.equal(
    resolvePanelResponsePhase({
      frames: [
        { id: "evt-1", event: "interaction_started", timestampMs: 1, data: { content: "Work" } },
      ],
      localPhase: null,
      hasLocalPhase: true,
      serverPhase: "generating",
    }),
    null,
  );
});

test("mapFramesToTimelineEntries renders terminal completion without streamed deltas", () => {
  const entries = mapFramesToTimelineEntries(
    {
      agent_id: "api-investigator",
      member_id: "api-investigator",
      label: "API Investigator",
      kind: "identity",
    },
    [
      { id: "evt-1", event: "turn_completed", data: { stop_reason: "end_turn" } },
      {
        id: "evt-2",
        event: "interaction_complete",
        data: { result: "The uploaded badge says ALL CLEAR." },
      },
    ],
  );

  assert.equal(entries.length, 1);
  assert.equal(entries[0]?.identity.role, "assistant");
  assert.equal(
    entries[0] && "blocks" in entries[0] && Array.isArray(entries[0].blocks)
      ? entries[0].blocks[0]?.type === "paragraph"
        ? entries[0].blocks[0].text
        : ""
      : "",
    "The uploaded badge says ALL CLEAR.",
  );
});

test("mapFramesToTimelineEntries hides steer delivery terminal control frames", () => {
  const entries = mapFramesToTimelineEntries(
    {
      agent_id: "review",
      member_id: "review",
      label: "Review Agent",
      kind: "identity",
    },
    [
      {
        id: "evt-1",
        event: "user_input",
        interactionId: "interaction-1",
        data: { text: "Steer this", handling_mode: "steer" },
      },
      {
        id: "evt-2",
        event: "interaction_complete",
        interactionId: "interaction-1",
        data: { reason: "steer_delivered", handling_mode: "steer" },
      },
    ],
  );

  assert.equal(entries.length, 1);
  assert.notEqual(entries[0]?.identity.role, "assistant");
  assert.match("text" in entries[0] ? entries[0].text : "", /Steer this/);
  assert.doesNotMatch("text" in entries[0] ? entries[0].text : "", /steer_delivered/);
});

test("mapFramesToTimelineEntries renders image-tool turns without duplicating final text", () => {
  const entries = mapFramesToTimelineEntries(
    {
      agent_id: "incident-commander",
      member_id: "incident-commander",
      label: "Incident Commander",
      kind: "identity",
    },
    [
      {
        id: "console-evt-2",
        event: "interaction_started",
        interactionId: "turn-1",
        timestampMs: 10,
        data: { content: "Generate an ALL CLEAR badge." },
      },
      {
        id: "evt-tool-call",
        event: "tool_call_requested",
        interactionId: "turn-1",
        timestampMs: 20,
        data: { id: "tool-1", name: "generate_image", args: { prompt: "ALL CLEAR" } },
      },
      {
        id: "evt-tool-done",
        event: "tool_execution_completed",
        interactionId: "turn-1",
        timestampMs: 30,
        data: {
          id: "tool-1",
          name: "generate_image",
          result: JSON.stringify({
            images: [
              {
                blob_ref: { blob_id: "sha256:badge", media_type: "image/png" },
                height: 1024,
                image_id: "image-1",
                media_type: "image/png",
                width: 1024,
              },
            ],
          }),
        },
      },
      {
        id: "evt-image",
        event: "assistant_image",
        interactionId: "turn-1",
        timestampMs: 30,
        data: {
          blob_id: "sha256:badge",
          media_type: "image/png",
          width: 1024,
          height: 1024,
        },
      },
      {
        id: "evt-tool-result",
        event: "tool_result_received",
        interactionId: "turn-1",
        timestampMs: 30,
        data: {
          id: "tool-1",
          name: "generate_image",
          result: JSON.stringify({
            images: [
              {
                blob_ref: { blob_id: "sha256:badge", media_type: "image/png" },
                height: 1024,
                image_id: "image-1",
                media_type: "image/png",
                width: 1024,
              },
            ],
          }),
        },
      },
      {
        id: "evt-turn-started-2",
        event: "turn_started",
        interactionId: "turn-1",
        timestampMs: 30,
        data: {},
      },
      {
        id: "evt-delta-1",
        event: "text_delta",
        interactionId: "turn-1",
        timestampMs: 40,
        data: { delta: "Generated" },
      },
      {
        id: "evt-delta-2",
        event: "text_delta",
        interactionId: "turn-1",
        timestampMs: 40,
        data: { delta: " the square ALL CLEAR incident badge image." },
      },
      {
        id: "evt-text-complete",
        event: "text_complete",
        interactionId: "turn-1",
        timestampMs: 50,
        data: { content: "Generated the square ALL CLEAR incident badge image." },
      },
      {
        id: "evt-turn-complete",
        event: "turn_completed",
        interactionId: "turn-1",
        timestampMs: 50,
        data: { stop_reason: "end_turn" },
      },
      {
        id: "evt-complete",
        event: "interaction_complete",
        interactionId: "turn-1",
        timestampMs: 50,
        data: { result: "Generated the square ALL CLEAR incident badge image." },
      },
    ],
    {
      renderInteractionStartsAsUser: true,
      blobBaseUrl: "http://127.0.0.1:49551",
    },
  );

  const finalTextEntries = entries.filter((entry) => {
    if (entry.kind !== "message") return false;
    if ("text" in entry && entry.text === "Generated the square ALL CLEAR incident badge image.") return true;
    return "blocks" in entry
      && Array.isArray(entry.blocks)
      && entry.blocks.some(
        (block) => block.type === "paragraph"
          && block.text === "Generated the square ALL CLEAR incident badge image.",
      );
  });
  const imageEntries = entries.filter(
    (entry) => entry.kind === "message"
      && "blocks" in entry
      && Array.isArray(entry.blocks)
      && entry.blocks.some((block) => block.type === "image"),
  );

  assert.equal(finalTextEntries.length, 1);
  assert.equal(imageEntries.length, 1);
});

test("mapFramesToTimelineEntries renders generate_image result images immediately", () => {
  const entries = mapFramesToTimelineEntries(
    {
      agent_id: "incident-commander",
      member_id: "incident-commander",
      label: "Incident Commander",
      kind: "identity",
    },
    [
      {
        id: "evt-tool-call",
        event: "tool_call_requested",
        interactionId: "turn-1",
        timestampMs: 20,
        data: { id: "tool-1", name: "generate_image", args: { prompt: "ALL CLEAR" } },
      },
      {
        id: "evt-tool-done",
        event: "tool_execution_completed",
        interactionId: "turn-1",
        timestampMs: 30,
        data: {
          id: "tool-1",
          name: "generate_image",
          result: JSON.stringify({
            images: [
              {
                blob_ref: { blob_id: "sha256:badge", media_type: "image/png" },
                height: 1024,
                image_id: "image-1",
                media_type: "image/png",
                width: 1024,
              },
            ],
          }),
        },
      },
    ],
    {
      blobBaseUrl: "http://127.0.0.1:49551",
    },
  );

  const imageEntries = entries.filter(
    (entry) => entry.kind === "message"
      && "blocks" in entry
      && Array.isArray(entry.blocks)
      && entry.blocks.some((block) => block.type === "image"),
  );

  assert.equal(imageEntries.length, 1);
  const imageBlock = imageEntries[0]?.kind === "message"
    && "blocks" in imageEntries[0]
    && Array.isArray(imageEntries[0].blocks)
    ? imageEntries[0].blocks.find((block) => block.type === "image")
    : null;
  assert.equal(imageBlock?.type, "image");
  assert.equal(
    imageBlock?.type === "image" ? imageBlock.src : "",
    "http://127.0.0.1:49551/blobs/sha256%3Abadge",
  );
});

test("sortConversationTimelineEntries keeps optimistic user messages after older assistant replies", () => {
  const entries = sortConversationTimelineEntries([
    {
      kind: "message",
      id: "assistant-1",
      identity: { id: "agent-1", label: "Agent", role: "assistant" },
      variant: "plain",
      text: "Assistant 1",
      createdAt: "2026-04-04T10:00:01.000Z",
    },
    {
      kind: "message",
      id: "user-1",
      identity: { id: "user", label: "You", role: "user" },
      variant: "plain",
      text: "User 1",
      createdAt: "2026-04-04T10:00:00.000Z",
    },
    {
      kind: "message",
      id: "user-2",
      identity: { id: "user", label: "You", role: "user" },
      variant: "plain",
      text: "User 2",
      createdAt: "2026-04-04T10:00:02.000Z",
    },
  ]);

  assert.deepEqual(entries.map((entry) => entry.id), ["user-1", "assistant-1", "user-2"]);
});

test("optimisticUserMessageForPanel shares the latest identity prompt across split panes", () => {
  const older = {
    interactionId: "",
    sentAtMs: 10,
    entry: {
      kind: "message" as const,
      id: "older",
      identity: { id: "user", label: "You", role: "user" as const },
      variant: "plain" as const,
      text: "Older prompt",
      createdAt: "2026-05-21T14:00:00.000Z",
    },
  };
  const latest = {
    interactionId: "",
    sentAtMs: 20,
    entry: {
      kind: "message" as const,
      id: "latest",
      identity: { id: "user", label: "You", role: "user" as const },
      variant: "plain" as const,
      text: "Latest prompt",
      createdAt: "2026-05-21T14:01:00.000Z",
    },
  };
  const direct = {
    interactionId: "",
    sentAtMs: 1,
    entry: {
      kind: "message" as const,
      id: "direct",
      identity: { id: "user", label: "You", role: "user" as const },
      variant: "plain" as const,
      text: "Direct prompt",
      createdAt: "2026-05-21T14:02:00.000Z",
    },
  };
  const optimistic = {
    "panel:left:agent-chat:deep-investigator:singleton": older,
    "panel:middle:agent-chat:other-agent": direct,
    "panel:right:agent-chat:deep-investigator:singleton": latest,
  };

  assert.equal(
    optimisticUserMessageForPanel(
      optimistic,
      "panel:new:agent-chat:deep-investigator:singleton",
      "deep-investigator:singleton",
    )?.entry.id,
    "latest",
  );
  assert.equal(
    optimisticUserMessageForPanel(
      optimistic,
      "panel:middle:agent-chat:other-agent",
      "other-agent",
    )?.entry.id,
    "direct",
  );
});

test("sortConversationTimelineEntries orders accepted user frames before later assistant replies regardless of entry arrival", () => {
  const entries = sortConversationTimelineEntries([
    {
      kind: "message",
      id: "assistant-1",
      identity: { id: "agent-1", label: "Agent", role: "assistant" },
      variant: "plain",
      text: "ORDER_OK",
      createdAt: "2026-05-07T15:21:50.000Z",
    },
    {
      kind: "message",
      id: "user-1",
      identity: { id: "user", label: "You", role: "user" },
      variant: "plain",
      text: "Ordering smoke: reply with exactly ORDER_OK and no extra text.",
      createdAt: "2026-05-07T15:21:48.000Z",
    },
  ]);

  assert.deepEqual(entries.map((entry) => entry.id), ["user-1", "assistant-1"]);
});

test("appendOptimisticConversationEntry preserves timestamp transcript order", () => {
  const entries = mapFramesToTimelineEntries(
    {
      agent_id: "deep-investigator",
      member_id: "deep-investigator",
      label: "Deep Investigator",
      kind: "identity",
    },
    [
      {
        id: "tool-1",
        cursor: "console:4",
        event: "tool_call_requested",
        timestampMs: 1_779_405_464_000,
        interactionId: "turn-1",
        data: { id: "call-1", name: "get_all_initiatives", args: {} },
      },
      {
        id: "tool-1-done",
        cursor: "console:5",
        event: "tool_execution_completed",
        timestampMs: 1_779_405_464_500,
        interactionId: "turn-1",
        data: { id: "call-1", name: "get_all_initiatives", result: "[]" },
      },
      {
        id: "final",
        cursor: "console:6",
        event: "interaction_complete",
        timestampMs: 1_779_405_463_000,
        interactionId: "turn-1",
        data: { result: "I checked the initiative list." },
      },
    ],
    { renderInteractionStartsAsUser: true, renderTextDeltas: true },
  );
  const optimistic = {
    kind: "message" as const,
    id: "optimistic",
    identity: { id: "user", label: "You", role: "user" as const },
    variant: "plain" as const,
    text: "Queued follow-up",
    createdAt: "2026-05-22T00:00:00.000Z",
  };

  assert.deepEqual(
    appendOptimisticConversationEntry(entries, optimistic).map((entry) => {
      if (entry.id === "optimistic") return "optimistic";
      if (entry.kind !== "message") return entry.kind;
      if (entry.variant === "rich" && entry.blocks?.[0]?.type === "tool-call") return "tool";
      if (entry.variant === "rich" && entry.blocks?.[0]?.type === "paragraph") {
        return entry.blocks[0].text;
      }
      return "text" in entry ? entry.text : "rich";
    }),
    ["I checked the initiative list.", "tool", "optimistic"],
  );
});

test("mapFramesToTimelineEntries renders tool turns without raw tool lifecycle system noise", () => {
  const entries = mapFramesToTimelineEntries(
    {
      agent_id: "incident-commander",
      member_id: "incident-commander",
      label: "Incident Commander",
      kind: "identity",
    },
    [
      {
        id: "evt-1",
        event: "tool_call_requested",
        data: { id: "call-1", name: "send", args: { to: "payments-sre", body: "Check status" } },
      },
      {
        id: "evt-2",
        event: "tool_execution_started",
        data: { id: "call-1", name: "send" },
      },
      {
        id: "evt-3",
        event: "tool_execution_completed",
        data: { id: "call-1", name: "send", result: "{\"status\":\"sent\"}" },
      },
      {
        id: "evt-4",
        event: "text_delta",
        data: { delta: "Sent the status check." },
      },
      {
        id: "evt-5",
        event: "interaction_complete",
        data: { text: "Sent the status check." },
      },
    ],
  );

  assert.equal(entries.length, 2);
  assert.equal(entries[0]?.kind, "message");
  assert.equal(entries[0]?.identity.role, "assistant");
  assert.equal(
    entries[0] && "blocks" in entries[0] && Array.isArray(entries[0].blocks)
      ? entries[0].blocks[0]?.type
      : "",
    "tool-call",
  );
  assert.equal(entries[1]?.identity.role, "assistant");
  assert.equal(
    entries[1] && "blocks" in entries[1] && Array.isArray(entries[1].blocks)
      ? entries[1].blocks[0]?.type === "paragraph"
        ? entries[1].blocks[0].text
        : ""
      : "",
    "Sent the status check.",
  );
});

test("mapFramesToTimelineEntries renders server tool content as tool activity without raw JSON", () => {
  const entries = mapFramesToTimelineEntries(
    {
      agent_id: "incident-commander",
      member_id: "incident-commander",
      label: "Incident Commander",
      kind: "identity",
    },
    [
      {
        id: "evt-search-1",
        event: "server_tool_content",
        data: {
          content: {
            id: "ws-1",
            item_id: "ws-1",
            name: "web_search",
            type: "response.web_search_call.in_progress",
          },
        },
      },
      {
        id: "evt-search-2",
        event: "server_tool_content",
        data: {
          content: {
            id: "ws-1",
            item_id: "ws-1",
            name: "web_search",
            type: "response.web_search_call.completed",
          },
        },
      },
      {
        id: "evt-annotations",
        event: "server_tool_content",
        data: {
          content: {
            id: "msg-1",
            name: "web_search_annotations",
            type: "message_annotations",
            annotations: [
              { title: "Example", url: "https://example.com" },
            ],
          },
        },
      },
      {
        id: "evt-answer",
        event: "interaction_complete",
        data: { result: "I found one source." },
      },
    ],
  );

  assert.equal(entries.length, 2);
  const tool = entries[0] && "blocks" in entries[0] ? entries[0].blocks?.[0] : null;
  assert.equal(tool?.type, "tool-call");
  if (tool?.type === "tool-call") {
    assert.equal(tool.name, "web_search");
    assert.equal(tool.status, "success");
    assert.match(tool.result || "", /Example/);
    assert.match(tool.result || "", /https:\/\/example\.com/);
  }
  const answer = entries[1] && "blocks" in entries[1] ? entries[1].blocks?.[0] : null;
  assert.equal(answer?.type, "paragraph");
  assert.equal(answer?.text, "I found one source.");
});

test("mapFramesToTimelineEntries renders completed web_search_call status and query context", () => {
  const entries = mapFramesToTimelineEntries(
    {
      agent_id: "incident-commander",
      member_id: "incident-commander",
      label: "Incident Commander",
      kind: "identity",
    },
    [
      {
        id: "evt-search-1",
        event: "server_tool_content",
        data: {
          content: {
            id: "ws-1",
            item_id: "ws-1",
            name: "web_search_call",
            type: "web_search_call",
            status: "completed",
            action: {
              queries: ["stockholm continuity risk", "muskö naval base"],
            },
          },
        },
      },
    ],
  );

  assert.equal(entries.length, 1);
  const tool = entries[0] && "blocks" in entries[0] ? entries[0].blocks?.[0] : null;
  assert.equal(tool?.type, "tool-call");
  if (tool?.type === "tool-call") {
    assert.equal(tool.status, "success");
    assert.equal(tool.arguments, "stockholm continuity risk\nmuskö naval base");
  }
});

test("mapFramesToTimelineEntries hides orphan server tool annotations", () => {
  const entries = mapFramesToTimelineEntries(
    {
      agent_id: "incident-commander",
      member_id: "incident-commander",
      label: "Incident Commander",
      kind: "identity",
    },
    [
      {
        id: "evt-annotations",
        event: "server_tool_content",
        data: {
          content: {
            id: "msg-1",
            name: "web_search_annotations",
            type: "message_annotations",
            annotations: [
              { title: "Example", url: "https://example.com" },
            ],
          },
        },
      },
    ],
  );

  assert.equal(entries.length, 0);
});

test("mapFramesToTimelineEntries preserves text and tool interleaving inside one interaction", () => {
  const entries = mapFramesToTimelineEntries(
    {
      agent_id: "deep-investigator",
      member_id: "deep-investigator",
      label: "Deep Investigator",
      kind: "identity",
    },
    [
      {
        id: "user",
        cursor: "console:1",
        event: "user_input",
        timestampMs: 100,
        interactionId: "turn-1",
        data: { content: "Investigate Minecraft" },
      },
      {
        id: "text-1",
        cursor: "console:2",
        event: "text_delta",
        timestampMs: 200,
        interactionId: "turn-1",
        data: { delta: "I will look it up." },
      },
      {
        id: "complete-1",
        cursor: "console:3",
        event: "text_complete",
        timestampMs: 300,
        interactionId: "turn-1",
        data: { content: "I will look it up." },
      },
      {
        id: "tool-1",
        cursor: "console:4",
        event: "tool_call_requested",
        timestampMs: 400,
        interactionId: "turn-1",
        data: { id: "call-1", name: "get_all_initiatives", args: {} },
      },
      {
        id: "tool-1-done",
        cursor: "console:5",
        event: "tool_execution_completed",
        timestampMs: 500,
        interactionId: "turn-1",
        data: { id: "call-1", name: "get_all_initiatives", result: "[]" },
      },
      {
        id: "text-2",
        cursor: "console:6",
        event: "text_delta",
        timestampMs: 600,
        interactionId: "turn-1",
        data: { delta: "Matched the initiative." },
      },
      {
        id: "done",
        cursor: "console:7",
        event: "interaction_complete",
        timestampMs: 700,
        interactionId: "turn-1",
        data: { result: "I will look it up.\nMatched the initiative." },
      },
    ],
    { renderInteractionStartsAsUser: true, renderTextDeltas: true },
  );

  assert.equal(entries.length, 4);
  assert.deepEqual(
    entries.map((entry) => {
      if (entry.kind !== "message") return entry.kind;
      if (entry.variant === "rich") {
        const block = entry.blocks?.[0];
        if (block?.type === "tool-call") return "tool";
        if (block?.type === "paragraph") return block.text;
        return "rich";
      }
      return entry.text;
    }),
    ["Investigate Minecraft", "I will look it up.", "tool", "Matched the initiative."],
  );
});

test("mapFramesToTimelineEntries hides technical peer request intents and previews human params", () => {
  const entries = mapFramesToTimelineEntries(
    {
      agent_id: "incident-commander",
      member_id: "incident-commander",
      label: "Incident Commander",
      kind: "identity",
    },
    [
      {
        id: "evt-peers-done",
        event: "tool_execution_completed",
        data: {
          id: "peers-1",
          name: "peers",
          result: JSON.stringify({
            peers: [{ peer_id: "peer-scribe-1", name: "incident-command-center/scribe/scribe" }],
          }),
        },
      },
      {
        id: "evt-request",
        event: "tool_call_requested",
        data: {
          id: "call-1",
          name: "send_request",
          args: {
            peer_id: "peer-scribe-1",
            intent: "checksum_token",
            params: { subject: "Reply exactly: peer smoke ok" },
          },
        },
      },
      {
        id: "evt-request-done",
        event: "tool_execution_completed",
        data: { id: "call-1", name: "send_request", result: "{\"status\":\"sent\"}" },
      },
    ],
  );

  const block = entries[0] && "blocks" in entries[0] && Array.isArray(entries[0].blocks)
    ? entries[0].blocks[0]
    : null;
  assert.equal(block?.type, "tool-call");
  if (block?.type === "tool-call") {
    assert.equal(block.peerTarget, "scribe");
    assert.equal(block.peerIntent, undefined);
    assert.equal(block.peerBody, "Reply exactly: peer smoke ok");
  }
});

test("mapFramesToTimelineEntries previews checksum peer responses by token instead of raw JSON", () => {
  const entries = mapFramesToTimelineEntries(
    {
      agent_id: "scribe",
      member_id: "scribe",
      label: "Scribe",
      kind: "identity",
    },
    [
      {
        id: "evt-response",
        event: "tool_call_requested",
        data: {
          id: "call-1",
          name: "send_response",
          args: {
            peer_id: "peer-commander-1",
            display_name: "incident-command-center/commander/incident-commander",
            in_reply_to: "req-1",
            result: {
              request_intent: "checksum_token",
              request_subject: "peer smoke ok",
              token: "peer smoke ok",
            },
          },
        },
      },
      {
        id: "evt-response-done",
        event: "tool_execution_completed",
        data: { id: "call-1", name: "send_response", result: "{\"status\":\"sent\"}" },
      },
    ],
  );

  const block = entries[0] && "blocks" in entries[0] && Array.isArray(entries[0].blocks)
    ? entries[0].blocks[0]
    : null;
  assert.equal(block?.type, "tool-call");
  if (block?.type === "tool-call") {
    assert.equal(block.peerTarget, "incident-commander");
    assert.equal(block.peerBody, "peer smoke ok");
  }
});

test("mapFramesToTimelineEntries can render historical interaction_started frames as user messages", () => {
  const entries = mapFramesToTimelineEntries(
    null,
    [
      {
        id: "evt-1",
        event: "interaction_started",
        data: { content: "Run a status sweep." },
      },
    ],
    { renderInteractionStartsAsUser: true },
  );

  assert.equal(entries.length, 1);
  assert.equal(entries[0]?.identity.role, "user");
  assert.equal(entries[0] && "text" in entries[0] ? entries[0].text : "", "Run a status sweep.");
});

test("mapFramesToTimelineEntries renders aggregate user_input frames as user messages", () => {
  const entries = mapFramesToTimelineEntries(
    null,
    [
      {
        id: "console-frame-1",
        cursor: "console:1",
        event: "user_input",
        data: { content: "Run a status sweep." },
      },
    ],
    { renderInteractionStartsAsUser: true },
  );

  assert.equal(entries.length, 1);
  assert.equal(entries[0]?.identity.role, "user");
  assert.equal(entries[0] && "text" in entries[0] ? entries[0].text : "", "Run a status sweep.");
});

test("mapFramesToTimelineEntries renders session-history assistant text_complete frames", () => {
  const entries = mapFramesToTimelineEntries(
    {
      agent_id: "full-tool-worker",
      member_id: "full-tool-worker",
      label: "full-tool-worker",
      kind: "mob_agent",
    },
    [
      {
        id: "history-assistant",
        event: "text_complete",
        identity: "full-tool-worker",
        sourceKind: "session_history",
        data: {
          message: {
            role: "block_assistant",
            blocks: [
              {
                block_type: "text",
                data: { text: "Ready and standing by. Readiness reported." },
              },
            ],
          },
        },
      },
    ],
  );

  assert.equal(entries.length, 1);
  assert.equal(entries[0]?.identity.id, "full-tool-worker");
  const renderedText = entries[0] && "text" in entries[0]
    ? entries[0].text
    : entries[0] && "blocks" in entries[0] && Array.isArray(entries[0].blocks)
      ? entries[0].blocks.map((block) => block.type === "paragraph" ? block.text : "").join("")
      : "";
  assert.equal(
    renderedText,
    "Ready and standing by. Readiness reported.",
  );
});

test("mapFramesToTimelineEntries renders session-history reasoning blocks outside answer text", () => {
  const entries = mapFramesToTimelineEntries(
    {
      agent_id: "incident-worker-1",
      member_id: "incident-worker-1",
      label: "incident-worker-1",
      kind: "mob_agent",
    },
    [
      {
        id: "history-reasoning-only",
        event: "interaction_complete",
        identity: "incident-worker-1",
        sourceKind: "session_history",
        data: {
          message: {
            role: "block_assistant",
            blocks: [
              {
                block_type: "reasoning",
                data: { text: "**Deciding on communication approach**\n\nI should think this through." },
              },
              {
                block_type: "tool_use",
                data: { name: "peers", args: {}, id: "call-peers" },
              },
            ],
          },
          text: "**Deciding on communication approach**\n\nI should think this through.",
          result: "**Deciding on communication approach**\n\nI should think this through.",
        },
      },
      {
        id: "history-final",
        event: "interaction_complete",
        identity: "incident-worker-1",
        sourceKind: "session_history",
        data: {
          message: {
            role: "block_assistant",
            blocks: [
              {
                block_type: "text",
                data: {
                  text: "Ready as the incident investigation worker and standing by for follow-up tasks.",
                },
              },
              {
                block_type: "reasoning",
                data: { text: "**Considering event response**\n\nI should not be rendered as an answer." },
              },
            ],
          },
          text: "**Considering event response**\n\nI should not be rendered as an answer.Ready as the incident investigation worker and standing by for follow-up tasks.",
          result: "**Considering event response**\n\nI should not be rendered as an answer.Ready as the incident investigation worker and standing by for follow-up tasks.",
        },
      },
    ],
  );

  assert.equal(entries.length, 2);
  assert.equal(entries[0]?.identity.id, "incident-worker-1");
  const reasoningOnlyBlocks = entries[0] && "blocks" in entries[0] ? entries[0].blocks : [];
  assert.equal(reasoningOnlyBlocks?.[0]?.type, "thinking");
  assert.equal(reasoningOnlyBlocks?.[0]?.text, "**Deciding on communication approach**\n\nI should think this through.");
  assert.equal(reasoningOnlyBlocks?.[0]?.label, "");
  assert.equal(reasoningOnlyBlocks?.[0]?.final, true);
  assert.equal(reasoningOnlyBlocks?.[0]?.persisted, true);
  const toolBlock = reasoningOnlyBlocks?.[1];
  assert.equal(toolBlock?.type, "tool-call");
  assert.equal(toolBlock?.name, "peers");
  assert.equal(entries[1]?.identity.id, "incident-worker-1");
  const finalBlocks = entries[1] && "blocks" in entries[1] ? entries[1].blocks : [];
  assert.equal(finalBlocks?.[0]?.type, "thinking");
  assert.equal(finalBlocks?.[0]?.text, "**Considering event response**\n\nI should not be rendered as an answer.");
  assert.equal(finalBlocks?.[0]?.label, "");
  assert.equal(finalBlocks?.[1]?.type, "paragraph");
  assert.equal(finalBlocks?.[1]?.text, "Ready as the incident investigation worker and standing by for follow-up tasks.");
  const renderedText = entries[1] && "text" in entries[1]
    ? entries[1].text
    : entries[1] && "blocks" in entries[1] && Array.isArray(entries[1].blocks)
      ? entries[1].blocks.map((block) => block.type === "paragraph" ? block.text : "").join("")
      : "";
  assert.equal(
    renderedText,
    "Ready as the incident investigation worker and standing by for follow-up tasks.",
  );
});

test("mapFramesToTimelineEntries renders live reasoning deltas as thought blocks", () => {
  const entries = mapFramesToTimelineEntries(
    {
      agent_id: "incident-worker-1",
      member_id: "incident-worker-1",
      label: "incident-worker-1",
      kind: "mob_agent",
    },
    [
      {
        id: "reasoning-1",
        event: "reasoning_delta",
        identity: "incident-worker-1",
        data: { delta: "Planning " },
      },
      {
        id: "reasoning-2",
        event: "reasoning_delta",
        identity: "incident-worker-1",
        data: { delta: "the next move." },
      },
      {
        id: "text-1",
        event: "text_delta",
        identity: "incident-worker-1",
        data: { delta: "Ready." },
      },
      {
        id: "done-1",
        event: "interaction_complete",
        identity: "incident-worker-1",
        data: { result: "Ready." },
      },
    ],
  );

  assert.equal(entries.length, 2);
  const thought = entries[0] && "blocks" in entries[0] ? entries[0].blocks?.[0] : null;
  assert.equal(thought?.type, "thinking");
  assert.equal(thought?.text, "Planning the next move.");
  assert.equal(thought?.label, "");
  assert.equal(thought?.final, true);
  const answer = entries[1] && "blocks" in entries[1] ? entries[1].blocks?.[0] : null;
  assert.equal(answer?.type, "paragraph");
  assert.equal(answer?.text, "Ready.");
});

test("mapFramesToTimelineEntries suppresses history reasoning replay after a streamed answer", () => {
  const entries = mapFramesToTimelineEntries(
    {
      agent_id: "incident-worker-1",
      member_id: "incident-worker-1",
      label: "incident-worker-1",
      kind: "mob_agent",
    },
    [
      {
        id: "reasoning-1",
        event: "reasoning_delta",
        identity: "incident-worker-1",
        data: { delta: "Planning before answering." },
      },
      {
        id: "text-1",
        event: "text_delta",
        identity: "incident-worker-1",
        data: { delta: "Ready." },
      },
      {
        id: "history-final",
        event: "interaction_complete",
        identity: "incident-worker-1",
        sourceKind: "session_history",
        data: {
          message: {
            role: "block_assistant",
            blocks: [
              {
                block_type: "text",
                data: { text: "Ready." },
              },
              {
                block_type: "reasoning",
                data: { text: "Planning before answering." },
              },
            ],
          },
          text: "Ready.",
          result: "Ready.",
        },
      },
    ],
  );

  assert.equal(entries.length, 2);
  const thought = entries[0] && "blocks" in entries[0] ? entries[0].blocks?.[0] : null;
  assert.equal(thought?.type, "thinking");
  assert.equal(thought?.text, "Planning before answering.");
  const answer = entries[1] && "blocks" in entries[1] ? entries[1].blocks?.[0] : null;
  assert.equal(answer?.type, "paragraph");
  assert.equal(answer?.text, "Ready.");
});

test("mapFramesToTimelineEntries suppresses late completed reasoning slices after an answer", () => {
  const entries = mapFramesToTimelineEntries(
    {
      agent_id: "incident-worker-1",
      member_id: "incident-worker-1",
      label: "incident-worker-1",
      kind: "mob_agent",
    },
    [
      {
        id: "reasoning-1",
        event: "reasoning_delta",
        interactionId: "turn-1",
        identity: "incident-worker-1",
        data: { delta: "Searching for context.\n\nEvaluating choices." },
      },
      {
        id: "text-1",
        event: "text_delta",
        interactionId: "turn-1",
        identity: "incident-worker-1",
        data: { delta: "Ready." },
      },
      {
        id: "done-1",
        event: "interaction_complete",
        interactionId: "turn-1",
        identity: "incident-worker-1",
        data: { result: "Ready." },
      },
      {
        id: "reasoning-complete-late",
        event: "reasoning_complete",
        interactionId: "turn-1",
        identity: "incident-worker-1",
        data: { text: "Evaluating choices." },
      },
    ],
  );

  assert.equal(entries.length, 2);
  const thought = entries[0] && "blocks" in entries[0] ? entries[0].blocks?.[0] : null;
  assert.equal(thought?.type, "thinking");
  assert.equal(thought?.text, "Searching for context.\n\nEvaluating choices.");
  const answer = entries[1] && "blocks" in entries[1] ? entries[1].blocks?.[0] : null;
  assert.equal(answer?.type, "paragraph");
  assert.equal(answer?.text, "Ready.");
});

test("mapFramesToTimelineEntries keeps pending reasoning when a complete slice arrives before an answer", () => {
  const entries = mapFramesToTimelineEntries(
    {
      agent_id: "risk-red-team",
      member_id: "risk-red-team",
      label: "risk-red-team",
      kind: "mob_agent",
    },
    [
      {
        id: "reasoning-1",
        event: "reasoning_delta",
        interactionId: "turn-1",
        identity: "risk-red-team",
        data: { delta: "Searching for context.\n\nEvaluating relocation choices." },
      },
      {
        id: "reasoning-complete-slice",
        event: "reasoning_complete",
        interactionId: "turn-1",
        identity: "risk-red-team",
        data: { text: "Evaluating relocation choices." },
      },
      {
        id: "text-1",
        event: "text_delta",
        interactionId: "turn-1",
        identity: "risk-red-team",
        data: { delta: "Answer." },
      },
    ],
  );

  assert.equal(entries.length, 2);
  const thinking = entries[0] && "blocks" in entries[0] ? entries[0].blocks?.[0] : null;
  const answer = entries[1] && "blocks" in entries[1] ? entries[1].blocks?.[0] : null;
  assert.equal(thinking?.type, "thinking");
  assert.equal(thinking?.text, "Searching for context.\n\nEvaluating relocation choices.");
  assert.equal(answer?.type, "paragraph");
  assert.equal(answer?.text, "Answer.");
});

test("mapFramesToTimelineEntries upgrades prior reasoning when a late complete is fuller", () => {
  const entries = mapFramesToTimelineEntries(
    {
      agent_id: "incident-worker-1",
      member_id: "incident-worker-1",
      label: "incident-worker-1",
      kind: "mob_agent",
    },
    [
      {
        id: "reasoning-1",
        event: "reasoning_delta",
        interactionId: "turn-1",
        identity: "incident-worker-1",
        data: { delta: "Searching for context." },
      },
      {
        id: "text-1",
        event: "text_delta",
        interactionId: "turn-1",
        identity: "incident-worker-1",
        data: { delta: "Ready." },
      },
      {
        id: "done-1",
        event: "interaction_complete",
        interactionId: "turn-1",
        identity: "incident-worker-1",
        data: { result: "Ready." },
      },
      {
        id: "reasoning-complete-late",
        event: "reasoning_complete",
        interactionId: "turn-1",
        identity: "incident-worker-1",
        data: { text: "Searching for context.\n\nEvaluating choices." },
      },
    ],
  );

  assert.equal(entries.length, 2);
  const thought = entries[0] && "blocks" in entries[0] ? entries[0].blocks?.[0] : null;
  assert.equal(thought?.type, "thinking");
  assert.equal(thought?.text, "Searching for context.\n\nEvaluating choices.");
  const answer = entries[1] && "blocks" in entries[1] ? entries[1].blocks?.[0] : null;
  assert.equal(answer?.type, "paragraph");
  assert.equal(answer?.text, "Ready.");
});

test("mapFramesToTimelineEntries flushes pending reasoning before another interaction completes", () => {
  const entries = mapFramesToTimelineEntries(
    {
      agent_id: "incident-worker-1",
      member_id: "incident-worker-1",
      label: "incident-worker-1",
      kind: "mob_agent",
    },
    [
      {
        id: "reasoning-1",
        event: "reasoning_delta",
        interactionId: "turn-1",
        identity: "incident-worker-1",
        data: { delta: "First turn reasoning." },
      },
      {
        id: "reasoning-complete-2",
        event: "reasoning_complete",
        interactionId: "turn-2",
        identity: "incident-worker-1",
        data: { text: "Second turn reasoning." },
      },
    ],
  );

  assert.equal(entries.length, 2);
  const first = entries[0] && "blocks" in entries[0] ? entries[0].blocks?.[0] : null;
  const second = entries[1] && "blocks" in entries[1] ? entries[1].blocks?.[0] : null;
  assert.equal(first?.type, "thinking");
  assert.equal(first?.text, "First turn reasoning.");
  assert.equal(second?.type, "thinking");
  assert.equal(second?.text, "Second turn reasoning.");
});

test("mapFramesToTimelineEntries suppresses late unscoped reasoning without mutating prior traces", () => {
  const entries = mapFramesToTimelineEntries(
    {
      agent_id: "incident-worker-1",
      member_id: "incident-worker-1",
      label: "incident-worker-1",
      kind: "mob_agent",
    },
    [
      {
        id: "reasoning-1",
        event: "reasoning_delta",
        identity: "incident-worker-1",
        data: { delta: "Searching for context." },
      },
      {
        id: "text-1",
        event: "text_delta",
        identity: "incident-worker-1",
        data: { delta: "Ready." },
      },
      {
        id: "reasoning-2",
        event: "reasoning_complete",
        identity: "incident-worker-1",
        data: { text: "Searching for context.\n\nEvaluating a later independent turn." },
      },
    ],
  );

  assert.equal(entries.length, 2);
  const first = entries[0] && "blocks" in entries[0] ? entries[0].blocks?.[0] : null;
  const answer = entries[1] && "blocks" in entries[1] ? entries[1].blocks?.[0] : null;
  assert.equal(first?.type, "thinking");
  assert.equal(first?.text, "Searching for context.");
  assert.equal(answer?.type, "paragraph");
  assert.equal(answer?.text, "Ready.");
});

test("mapFramesToTimelineEntries marks in-flight reasoning deltas as non-final", () => {
  const entries = mapFramesToTimelineEntries(
    {
      agent_id: "incident-worker-1",
      member_id: "incident-worker-1",
      label: "incident-worker-1",
      kind: "mob_agent",
    },
    [
      {
        id: "reasoning-1",
        event: "reasoning_delta",
        identity: "incident-worker-1",
        data: { delta: "Still thinking" },
      },
    ],
  );

  assert.equal(entries.length, 1);
  const thought = entries[0] && "blocks" in entries[0] ? entries[0].blocks?.[0] : null;
  assert.equal(thought?.type, "thinking");
  assert.equal(thought?.text, "Still thinking");
  assert.equal(thought?.final, undefined);
});

test("mapFramesToTimelineEntries renders session-history tool-use only assistant turns", () => {
  const entries = mapFramesToTimelineEntries(
    {
      agent_id: "nested-worker-coordinator",
      member_id: "nested-worker-coordinator",
      label: "nested-worker-coordinator",
      kind: "mob_agent",
    },
    [
      {
        id: "history-tool-use",
        event: "interaction_complete",
        identity: "nested-worker-coordinator",
        sourceKind: "session_history",
        data: {
          message: {
            role: "block_assistant",
            blocks: [
              {
                block_type: "tool_use",
                data: {
                  id: "call-delegate",
                  name: "delegate",
                  args: {
                    member_id: "nested-ack-worker",
                    task: "acknowledge",
                  },
                },
              },
            ],
            stop_reason: "tool_use",
          },
          text: "",
          result: "",
        },
      },
    ],
  );

  assert.equal(entries.length, 1);
  assert.equal(entries[0]?.variant, "rich");
  const firstBlock = entries[0] && "blocks" in entries[0] ? entries[0].blocks?.[0] : null;
  assert.equal(firstBlock?.type, "tool-call");
  assert.equal(firstBlock?.name, "delegate");
  assert.match(firstBlock?.arguments || "", /nested-ack-worker/);
});

test("mapFramesToTimelineEntries attaches session-history tool results to tool-use turns", () => {
  const entries = mapFramesToTimelineEntries(
    {
      agent_id: "stress-worker",
      member_id: "stress-worker",
      label: "stress-worker",
      kind: "mob_agent",
    },
    [
      {
        id: "history-tool-use",
        event: "interaction_complete",
        identity: "stress-worker",
        sourceKind: "session_history",
        timestampMs: 10,
        data: {
          message: {
            role: "block_assistant",
            blocks: [
              {
                block_type: "tool_use",
                data: { id: "call-peers", name: "peers", args: {} },
              },
            ],
            stop_reason: "tool_use",
          },
          text: "",
          result: "",
        },
      },
      {
        id: "history-tool-result",
        event: "tool_execution_completed",
        identity: "stress-worker",
        sourceKind: "session_history",
        timestampMs: 11,
        data: {
          id: "call-peers",
          tool_call_id: "call-peers",
          result: JSON.stringify("{\"peers\":[{\"peer_id\":\"peer-1\",\"name\":\"mob/worker/peer-1\"}]}"),
          is_error: false,
        },
      },
    ],
  );

  assert.equal(entries.length, 1);
  const block = entries[0] && "blocks" in entries[0] ? entries[0].blocks?.[0] : null;
  assert.equal(block?.type, "tool-call");
  assert.equal(block?.name, "peers");
  assert.equal(
    block?.type === "tool-call" ? block.result : "",
    "1 peers · worker 1\nFirst peers: peer-1",
  );
  assert.equal(block?.type === "tool-call" ? block.status : "", "success");
});

test("mapFramesToTimelineEntries drops session-history tool-use blocks already rendered live", () => {
  const entries = mapFramesToTimelineEntries(
    {
      agent_id: "browser-parent-worker",
      member_id: "browser-parent-worker",
      label: "browser-parent-worker",
      kind: "mob_agent",
    },
    [
      {
        id: "live-mob-list-requested",
        event: "tool_call_requested",
        identity: "browser-parent-worker",
        interactionId: "turn-1",
        data: { id: "call-live-mob-list", name: "mob_list", args: {} },
      },
      {
        id: "live-mob-list-completed",
        event: "tool_execution_completed",
        identity: "browser-parent-worker",
        interactionId: "turn-1",
        data: { id: "call-live-mob-list", name: "mob_list", result: "{\"mobs\":[]}" },
      },
      {
        id: "history-mob-list-tool-use",
        event: "interaction_complete",
        identity: "browser-parent-worker",
        interactionId: "turn-1",
        sourceKind: "session_history",
        data: {
          message: {
            role: "block_assistant",
            blocks: [
              {
                block_type: "tool_use",
                data: {
                  id: "call-live-mob-list",
                  name: "mob_list",
                  args: {},
                },
              },
            ],
            stop_reason: "tool_use",
          },
          text: "",
          result: "",
        },
      },
    ],
    { renderInteractionStartsAsUser: true },
  );

  const toolBlocks = entries.flatMap((entry) => entry && "blocks" in entry && Array.isArray(entry.blocks)
    ? entry.blocks.filter((block) => block.type === "tool-call")
    : []);
  assert.equal(toolBlocks.length, 1);
  assert.equal(toolBlocks[0]?.name, "mob_list");
});

test("mapFramesToTimelineEntries preserves extra history tool-use repeats beyond live count", () => {
  const entries = mapFramesToTimelineEntries(
    {
      agent_id: "browser-parent-worker",
      member_id: "browser-parent-worker",
      label: "browser-parent-worker",
      kind: "mob_agent",
    },
    [
      {
        id: "live-peer-requested",
        event: "tool_call_requested",
        identity: "browser-parent-worker",
        data: { id: "call-live-peer", name: "peers", args: {} },
      },
      {
        id: "history-peer-tool-use",
        event: "interaction_complete",
        identity: "browser-parent-worker",
        sourceKind: "session_history",
        data: {
          message: {
            role: "block_assistant",
            blocks: [
              { block_type: "tool_use", data: { id: "call-history-peer-1", name: "peers", args: {} } },
              { block_type: "tool_use", data: { id: "call-history-peer-2", name: "peers", args: {} } },
            ],
            stop_reason: "tool_use",
          },
          text: "",
          result: "",
        },
      },
    ],
  );

  const toolBlocks = entries.flatMap((entry) => entry && "blocks" in entry && Array.isArray(entry.blocks)
    ? entry.blocks.filter((block) => block.type === "tool-call")
    : []);
  assert.equal(toolBlocks.length, 2);
  assert.equal(toolBlocks.every((block) => block.name === "peers"), true);
});

test("mapFramesToTimelineEntries deduplicates paired user_input and interaction_started frames", () => {
  const entries = mapFramesToTimelineEntries(
    null,
    [
      {
        id: "console-frame-1",
        event: "user_input",
        interactionId: "turn-1",
        timestampMs: 10,
        data: { content: "Retire rollback-risk-worker please" },
      },
      {
        id: "evt-1",
        event: "interaction_started",
        interactionId: "turn-1",
        timestampMs: 10,
        data: { content: "Retire rollback-risk-worker please" },
      },
    ],
    { renderInteractionStartsAsUser: true },
  );

  assert.equal(entries.length, 1);
  assert.equal(entries[0]?.identity.role, "user");
  assert.equal(
    entries[0] && "text" in entries[0] ? entries[0].text : "",
    "Retire rollback-risk-worker please",
  );
});

test("mapFramesToTimelineEntries renders user image content blocks inline", () => {
  const entries = mapFramesToTimelineEntries(
    null,
    [
      {
        id: "evt-1",
        event: "interaction_started",
        data: {
          content: [
            { type: "text", text: "Describe this badge." },
            {
              type: "image",
              media_type: "image/jpeg",
              source: "blob",
              blob_id: "sha256:abc/def",
              alt: "incident badge",
            },
          ],
        },
      },
    ],
    {
      blobBaseUrl: "http://127.0.0.1:7000/",
      renderInteractionStartsAsUser: true,
    },
  );

  assert.equal(entries.length, 1);
  assert.equal(entries[0]?.identity.role, "user");
  assert.equal(entries[0]?.variant, "rich");
  const blocks = entries[0] && "blocks" in entries[0] ? entries[0].blocks || [] : [];
  assert.equal(blocks[0]?.type, "paragraph");
  assert.equal(blocks[0]?.type === "paragraph" ? blocks[0].text : "", "Describe this badge.");
  assert.equal(blocks[1]?.type, "image");
  assert.equal(
    blocks[1]?.type === "image" ? blocks[1].src : "",
    "http://127.0.0.1:7000/blobs/sha256%3Aabc%2Fdef",
  );
  assert.equal(blocks[1]?.type === "image" ? blocks[1].alt : "", "incident badge");
});

test("mapFramesToTimelineEntries renders user image_ref content blocks inline", () => {
  const entries = mapFramesToTimelineEntries(
    null,
    [
      {
        id: "evt-1",
        event: "interaction_started",
        data: {
          content: [
            { type: "text", text: "Please inspect the forwarded image." },
            {
              type: "image_ref",
              media_type: "image/png",
              source: "blob",
              blob_id: "sha256:forwarded/operator",
            },
          ],
        },
      },
    ],
    {
      blobBaseUrl: "http://127.0.0.1:7000",
      renderInteractionStartsAsUser: true,
    },
  );

  assert.equal(entries.length, 1);
  assert.equal(entries[0]?.identity.role, "user");
  assert.equal(entries[0]?.variant, "rich");
  const blocks = entries[0] && "blocks" in entries[0] ? entries[0].blocks || [] : [];
  assert.equal(blocks[0]?.type, "paragraph");
  assert.equal(blocks[0]?.type === "paragraph" ? blocks[0].text : "", "Please inspect the forwarded image.");
  assert.equal(blocks[1]?.type, "image");
  assert.equal(
    blocks[1]?.type === "image" ? blocks[1].src : "",
    "http://127.0.0.1:7000/blobs/sha256%3Aforwarded%2Foperator",
  );
  assert.equal(blocks[1]?.type === "image" ? blocks[1].alt : "", "referenced image");
});

test("mapFramesToTimelineEntries renders assistant_image frames with API blob URLs", () => {
  const entries = mapFramesToTimelineEntries(
    {
      agent_id: "artist",
      member_id: "artist",
      label: "Artist",
      kind: "identity",
    },
    [
      {
        id: "evt-image",
        event: "assistant_image",
        timestampMs: Date.parse("2026-04-06T23:00:00.000Z"),
        data: {
          blob_id: "sha256:abc/def",
          media_type: "image/png",
          width: 512,
          height: 512,
        },
      },
    ],
    { blobBaseUrl: "http://127.0.0.1:7000/" },
  );

  assert.equal(entries.length, 1);
  assert.equal(entries[0]?.variant, "rich");
  const block = entries[0] && "blocks" in entries[0] ? entries[0].blocks?.[0] : null;
  assert.equal(block?.type, "image");
  assert.equal(block?.src, "http://127.0.0.1:7000/blobs/sha256%3Aabc%2Fdef");
});

test("mapFramesToTimelineEntries renders assistant_image_appended frames without raw metadata", () => {
  const entries = mapFramesToTimelineEntries(
    {
      agent_id: "incident-commander",
      member_id: "incident-commander",
      label: "Incident Commander",
      kind: "identity",
    },
    [
      {
        id: "evt-image-appended",
        event: "assistant_image_appended",
        timestampMs: Date.parse("2026-04-06T23:00:00.000Z"),
        data: {
          image: {
            blob_ref: { blob_id: "sha256:generated", media_type: "image/png" },
            height: 1024,
            image_id: "image-1",
            media_type: "image/png",
            meta: { response_id: "resp_123" },
            width: 1024,
          },
          source_event_type: "assistant_image_appended",
          type: "assistant_image_appended",
        },
      },
    ],
    { blobBaseUrl: "http://127.0.0.1:7000/" },
  );

  assert.equal(entries.length, 1);
  assert.equal(entries[0]?.variant, "rich");
  const block = entries[0] && "blocks" in entries[0] ? entries[0].blocks?.[0] : null;
  assert.equal(block?.type, "image");
  assert.equal(block?.src, "http://127.0.0.1:7000/blobs/sha256%3Agenerated");
});

test("mapFramesToTimelineEntries deduplicates assistant image and appended image events for the same blob", () => {
  const entries = mapFramesToTimelineEntries(
    {
      agent_id: "incident-commander",
      member_id: "incident-commander",
      label: "Incident Commander",
      kind: "identity",
    },
    [
      {
        id: "evt-image",
        event: "assistant_image",
        timestampMs: Date.parse("2026-04-06T23:00:00.000Z"),
        data: {
          blob_id: "sha256:generated",
          media_type: "image/png",
          width: 1024,
          height: 1024,
        },
      },
      {
        id: "evt-image-appended",
        event: "assistant_image_appended",
        timestampMs: Date.parse("2026-04-06T23:00:00.000Z"),
        data: {
          image: {
            blob_ref: { blob_id: "sha256:generated", media_type: "image/png" },
            height: 1024,
            image_id: "image-1",
            media_type: "image/png",
            width: 1024,
          },
        },
      },
    ],
    { blobBaseUrl: "http://127.0.0.1:7000/" },
  );

  assert.equal(entries.length, 1);
  const block = entries[0] && "blocks" in entries[0] ? entries[0].blocks?.[0] : null;
  assert.equal(block?.type, "image");
  assert.equal(block?.src, "http://127.0.0.1:7000/blobs/sha256%3Agenerated");
});

test("mapFramesToTimelineEntries treats spawn-looking interaction prompts as user text", () => {
  const entries = mapFramesToTimelineEntries(
    {
      agent_id: "incident-commander",
      member_id: "incident-commander",
      label: "Incident Commander",
      kind: "identity",
    },
    [
      {
        id: "evt-spawn",
        event: "interaction_started",
        interactionId: "spawn-turn",
        timestampMs: 10,
        data: {
          content: "You have been spawned as 'incident-commander' (role: commander) in mob 'incident-command-center'.",
        },
      },
      {
        id: "evt-delta",
        event: "text_delta",
        interactionId: "spawn-turn",
        timestampMs: 20,
        data: { delta: "Acknowledged." },
      },
      {
        id: "evt-complete",
        event: "interaction_complete",
        interactionId: "spawn-turn",
        timestampMs: 30,
        data: { result: "Acknowledged." },
      },
    ],
    { renderInteractionStartsAsUser: true },
  );

  assert.equal(entries.length, 2);
  assert.equal(entries[0]?.identity.role, "user");
  assert.equal(
    "text" in (entries[0] || {}) ? entries[0]?.text : "",
    "You have been spawned as 'incident-commander' (role: commander) in mob 'incident-command-center'.",
  );
  assert.equal(entries[1]?.identity.role, "assistant");
  assert.equal(
    entries[1] && "blocks" in entries[1] && Array.isArray(entries[1].blocks)
      ? entries[1].blocks[0]?.type === "paragraph"
        ? entries[1].blocks[0].text
        : ""
      : "",
    "Acknowledged.",
  );
});

test("mapFramesToTimelineEntries renders inbound comms run_started prompts as user work", () => {
  const entries = mapFramesToTimelineEntries(
    {
      agent_id: "scribe",
      member_id: "scribe",
      label: "Scribe",
      kind: "identity",
    },
    [
      {
        id: "evt-1",
        event: "run_started",
        timestampMs: Date.parse("2026-04-06T23:00:00.000Z"),
        data: {
          prompt: "[COMMS REQUEST from incident-command-center/incident_commander/incident-commander]\nIntent: request_summary\nBody: Summarize the incident.",
        },
      },
      {
        id: "evt-2",
        event: "interaction_complete",
        timestampMs: Date.parse("2026-04-06T23:00:02.000Z"),
        data: { result: "Summary sent back to commander." },
      },
    ],
  );

  assert.equal(entries.length, 2);
  assert.equal(entries[0]?.identity.role, "user");
  assert.equal(
    entries[0] && "text" in entries[0] ? entries[0].text : "",
    "[COMMS REQUEST from incident-command-center/incident_commander/incident-commander]\nIntent: request_summary\nBody: Summarize the incident.",
  );
  assert.equal(entries[1]?.identity.id, "scribe");
});

test("mapFramesToTimelineEntries renders inbound one-line peer run_started prompts as user work", () => {
  const entries = mapFramesToTimelineEntries(
    {
      agent_id: "scribe",
      member_id: "scribe",
      label: "Scribe",
      kind: "identity",
    },
    [
      {
        id: "evt-1",
        event: "run_started",
        timestampMs: Date.parse("2026-04-06T23:00:00.000Z"),
        data: {
          prompt: "[COMMS MESSAGE from incident-command-center/commander/incident-commander] Please describe what you see in the attached image.",
        },
      },
      {
        id: "evt-2",
        event: "interaction_complete",
        timestampMs: Date.parse("2026-04-06T23:00:02.000Z"),
        data: { result: "The image shows a winged fox commander." },
      },
    ],
    { renderInteractionStartsAsUser: true },
  );

  assert.equal(entries.length, 2);
  assert.equal(entries[0]?.identity.role, "user");
  assert.equal(
    entries[0] && "text" in entries[0] ? entries[0].text : "",
    "[COMMS MESSAGE from incident-command-center/commander/incident-commander] Please describe what you see in the attached image.",
  );
  assert.equal(entries[1]?.identity.id, "scribe");
});

test("mapFramesToTimelineEntries renders inbound comms-looking user-input frames as operator chat", () => {
  const entries = mapFramesToTimelineEntries(
    {
      agent_id: "tutti-profile-worker",
      member_id: "tutti-profile-worker",
      label: "tutti-profile-worker",
      kind: "identity",
    },
    [
      {
        id: "evt-comms-user-input",
        event: "user_input",
        timestampMs: Date.parse("2026-05-11T15:48:20.000Z"),
        data: {
          content:
            "[COMMS MESSAGE from incident-command-center/worker/tutti-profile-worker]\n"
            + "Ping: please confirm you are still online and reachable for CardinalPay incident support.",
        },
      },
      {
        id: "evt-complete",
        event: "interaction_complete",
        timestampMs: Date.parse("2026-05-11T15:48:22.000Z"),
        data: { result: "Confirmed online and reachable via comms." },
      },
    ],
    { renderInteractionStartsAsUser: true },
  );

  assert.equal(entries.length, 2);
  assert.equal(entries[0]?.identity.id, "user");
  assert.equal(entries[0]?.identity.role, "user");
  assert.equal(
    "text" in (entries[0] || {}) ? entries[0]?.text : "",
    "[COMMS MESSAGE from incident-command-center/worker/tutti-profile-worker]\nPing: please confirm you are still online and reachable for CardinalPay incident support.",
  );
  assert.equal(entries[1]?.identity.id, "tutti-profile-worker");
});

test("mapFramesToTimelineEntries renders session-history typed comms frames as comms metadata", () => {
  const entries = mapFramesToTimelineEntries(
    {
      agent_id: "child-worker",
      member_id: "child-worker",
      label: "child-worker",
      kind: "identity",
    },
    [
      {
        id: "history-comms-user-input",
        event: "system_notice",
        sourceKind: "session_history",
        timestampMs: Date.parse("2026-05-11T20:45:57.000Z"),
        data: {
          message: typedCommsNotice({
            peer: "implicit-019e18c9-cd01-7ad0-8a78-ce86f780706b/delegate/grandchild-worker",
            body: "Peer message from implicit-019e18c9-cd01-7ad0-8a78-ce86f780706b/delegate/grandchild-worker:\ngrandchild-worker ping acknowledgement.",
          }),
        },
      },
    ],
    { renderInteractionStartsAsUser: true },
  );

  assert.equal(entries.length, 1);
  assert.equal(entries[0]?.identity.id, "comms");
  assert.equal(entries[0]?.variant, "rich");
  const block = entries[0] && "blocks" in entries[0] ? entries[0].blocks?.[0] : null;
  assert.equal(block?.type, "tool-call");
  assert.equal(block?.peerIncoming, true);
  assert.equal(block?.peerTarget, "grandchild-worker");
  assert.equal(block?.peerBody, "grandchild-worker ping acknowledgement.");
});

test("mapFramesToTimelineEntries preserves outgoing typed comms direction", () => {
  const entries = mapFramesToTimelineEntries(
    {
      agent_id: "planner",
      member_id: "planner",
      label: "Planner",
      kind: "identity",
    },
    [
      {
        id: "outgoing-comms",
        event: "system_notice",
        timestampMs: Date.parse("2026-05-27T08:00:00.000Z"),
        data: {
          message: typedCommsNotice({
            peer: "fugue/peer-b",
            body: "Please review this patch.",
            direction: "outgoing",
          }),
        },
      },
    ],
  );

  assert.equal(entries.length, 1);
  const block = entries[0] && "blocks" in entries[0] ? entries[0].blocks?.[0] : null;
  assert.equal(block?.type, "tool-call");
  assert.equal(block?.type === "tool-call" ? block.peerIncoming : true, false);
  assert.equal(block?.type === "tool-call" ? block.peerTarget : "", "peer-b");
});

test("mapFramesToTimelineEntries suppresses repeated assistant history after an inbound comms notice", () => {
  const entries = mapFramesToTimelineEntries(
    {
      agent_id: "qa-parent-worker",
      member_id: "qa-parent-worker",
      label: "qa-parent-worker",
      kind: "identity",
    },
    [
      {
        id: "history-final-1",
        event: "interaction_complete",
        sourceKind: "session_history",
        timestampMs: Date.parse("2026-05-13T18:57:45.401Z"),
        data: {
          message: {
            role: "block_assistant",
            blocks: [{
              block_type: "text",
              data: { text: "qa-child-worker said: “Ping acknowledged.”" },
            }],
          },
          result: "qa-child-worker said: “Ping acknowledged.”",
          source_event_type: "session_history",
          type: "session_history",
        },
      },
      {
        id: "history-comms",
        event: "system_notice",
        sourceKind: "session_history",
        timestampMs: Date.parse("2026-05-13T18:57:45.406Z"),
        data: {
          message: typedCommsNotice({
            peer: "implicit/delegate/qa-child-worker",
            body: "Ping acknowledged.",
          }),
        },
      },
      {
        id: "history-final-2",
        event: "interaction_complete",
        sourceKind: "session_history",
        timestampMs: Date.parse("2026-05-13T18:57:47.816Z"),
        data: {
          message: {
            role: "block_assistant",
            blocks: [{
              block_type: "text",
              data: { text: "qa-child-worker said: “Ping acknowledged.”" },
            }],
          },
          result: "qa-child-worker said: “Ping acknowledged.”",
          source_event_type: "session_history",
          type: "session_history",
        },
      },
    ],
  );

  const assistantMessages = entries.filter(
    (entry) => entry.kind === "message" && entry.identity.id === "qa-parent-worker",
  );
  assert.equal(assistantMessages.length, 1);
  const assistantText = "text" in assistantMessages[0]
    ? assistantMessages[0].text
    : "blocks" in assistantMessages[0] && assistantMessages[0].blocks?.[0]?.type === "paragraph"
      ? assistantMessages[0].blocks[0].text
      : "";
  assert.equal(assistantText, "qa-child-worker said: “Ping acknowledged.”");
  const commsMessages = entries.filter(
    (entry) => entry.kind === "message" && entry.identity.id === "comms",
  );
  assert.equal(commsMessages.length, 1);
});

test("mapFramesToTimelineEntries renders live typed comms system notices as comms metadata", () => {
  const entries = mapFramesToTimelineEntries(
    {
      agent_id: "child-worker",
      member_id: "child-worker",
      label: "child-worker",
      kind: "identity",
    },
    [
      {
        id: "live-comms-notice",
        event: "system_notice",
        timestampMs: Date.parse("2026-05-11T20:45:57.000Z"),
        data: {
          message: typedCommsNotice({
            peer: "implicit-019e18c9-cd01-7ad0-8a78-ce86f780706b/delegate/grandchild-worker",
            body: "grandchild-worker ping acknowledgement.",
          }),
        },
      },
    ],
    { renderInteractionStartsAsUser: true },
  );

  assert.equal(entries.length, 1);
  assert.equal(entries[0]?.identity.id, "comms");
  assert.equal(entries[0]?.variant, "rich");
  const block = entries[0] && "blocks" in entries[0] ? entries[0].blocks?.[0] : null;
  assert.equal(block?.type, "tool-call");
  assert.equal(block?.peerIncoming, true);
  assert.equal(block?.peerTarget, "grandchild-worker");
  assert.equal(block?.peerBody, "grandchild-worker ping acknowledgement.");
});

test("mapFramesToTimelineEntries suppresses raw run_started peer envelopes when structured comms notice exists", () => {
  const entries = mapFramesToTimelineEntries(
    {
      agent_id: "planner",
      member_id: "planner",
      label: "Planner",
      kind: "identity",
    },
    [
      {
        id: "raw-run-started",
        event: "run_started",
        timestampMs: Date.parse("2026-05-27T08:00:00.000Z"),
        data: {
          prompt:
            "Peer message\n"
            + "Peer message from fugue/issue_lead/LUC-642/issue_lead:\n"
            + "Peer message from fugue/issue_lead/LUC-642/issue_lead:\n"
            + "Focused RED-review replan is complete and Linear is back in Implementing...\n"
            + "Peer message",
        },
      },
      {
        id: "structured-notice",
        event: "system_notice",
        timestampMs: Date.parse("2026-05-27T08:00:01.000Z"),
        data: {
          message: typedCommsNotice({
            peer: "fugue/issue_lead/LUC-642/issue_lead",
            body: "Focused RED-review replan is complete and Linear is back in Implementing...",
          }),
        },
      },
      {
        id: "evt-complete",
        event: "interaction_complete",
        timestampMs: Date.parse("2026-05-27T08:00:02.000Z"),
        data: { result: "Acknowledged." },
      },
    ],
    { renderInteractionStartsAsUser: true },
  );

  assert.equal(entries.length, 2);
  assert.equal(entries[0]?.identity.id, "comms");
  assert.equal(entries[0]?.variant, "rich");
  assert.equal(entries.some((entry) => (
    entry.identity.id === "user"
    && "text" in entry
    && entry.text.includes("Peer message from fugue/issue_lead")
  )), false);
  assert.equal(entries[1]?.identity.id, "planner");
});

test("mapFramesToTimelineEntries keeps raw peer prompts when structured comms body is only partial", () => {
  const entries = mapFramesToTimelineEntries(
    {
      agent_id: "planner",
      member_id: "planner",
      label: "Planner",
      kind: "identity",
    },
    [
      {
        id: "raw-run-started",
        event: "run_started",
        timestampMs: Date.parse("2026-05-27T08:00:00.000Z"),
        data: {
          prompt:
            "Peer message from fugue/peer-a:\n"
            + "Done.\n"
            + "Please also validate the retry plan.",
        },
      },
      {
        id: "structured-notice",
        event: "system_notice",
        timestampMs: Date.parse("2026-05-27T08:00:01.000Z"),
        data: {
          message: typedCommsNotice({
            peer: "fugue/peer-a",
            body: "Done.",
          }),
        },
      },
    ],
    { renderInteractionStartsAsUser: true },
  );

  assert.equal(entries.length, 2);
  assert.equal(entries[0]?.identity.id, "user");
  assert.equal(
    entries[0] && "text" in entries[0] ? entries[0].text : "",
    "Peer message from fugue/peer-a:\nDone.\nPlease also validate the retry plan.",
  );
  assert.equal(entries[1]?.identity.id, "comms");
});

test("mapFramesToTimelineEntries suppresses colon-delimited peer identity envelopes", () => {
  const entries = mapFramesToTimelineEntries(
    {
      agent_id: "planner",
      member_id: "planner",
      label: "Planner",
      kind: "identity",
    },
    [
      {
        id: "raw-run-started",
        event: "run_started",
        timestampMs: Date.parse("2026-05-27T08:00:00.000Z"),
        data: {
          prompt:
            "Peer message\n"
            + "Peer message from review:singleton:\n"
            + "Done.",
        },
      },
      {
        id: "structured-notice",
        event: "system_notice",
        timestampMs: Date.parse("2026-05-27T08:00:01.000Z"),
        data: {
          message: typedCommsNotice({
            peer: "review:singleton",
            body: "Done.",
          }),
        },
      },
    ],
    { renderInteractionStartsAsUser: true },
  );

  assert.equal(entries.length, 1);
  assert.equal(entries[0]?.identity.id, "comms");
});

test("mapFramesToTimelineEntries matches raw peer envelopes against structured display names", () => {
  const entries = mapFramesToTimelineEntries(
    {
      agent_id: "planner",
      member_id: "planner",
      label: "Planner",
      kind: "identity",
    },
    [
      {
        id: "raw-run-started",
        event: "run_started",
        timestampMs: Date.parse("2026-05-27T08:00:00.000Z"),
        data: {
          prompt:
            "Peer message\n"
            + "Peer message from fugue/issue_lead/LUC-642/issue_lead:\n"
            + "Focused RED-review replan is complete and Linear is back in Implementing...",
        },
      },
      {
        id: "structured-notice",
        event: "system_notice",
        timestampMs: Date.parse("2026-05-27T08:00:01.000Z"),
        data: {
          message: typedCommsNotice({
            peer: "fugue/issue_lead/LUC-642/issue_lead",
            peerId: "peer-uuid-1",
            peerDisplayName: "fugue/issue_lead/LUC-642/issue_lead",
            body: "Focused RED-review replan is complete and Linear is back in Implementing...",
          }),
        },
      },
    ],
    { renderInteractionStartsAsUser: true },
  );

  assert.equal(entries.length, 1);
  assert.equal(entries[0]?.identity.id, "comms");
  assert.equal(entries.some((entry) => (
    entry.identity.id === "user"
    && "text" in entry
    && entry.text.includes("Peer message from fugue/issue_lead")
  )), false);
});

test("mapFramesToTimelineEntries suppresses raw peer response envelopes when structured comms notice exists", () => {
  const entries = mapFramesToTimelineEntries(
    {
      agent_id: "planner",
      member_id: "planner",
      label: "Planner",
      kind: "identity",
    },
    [
      {
        id: "raw-run-started",
        event: "run_started",
        timestampMs: Date.parse("2026-05-27T08:00:00.000Z"),
        data: {
          prompt:
            "Peer response\n"
            + "Peer response from fugue/issue_lead/LUC-642/issue_lead:\n"
            + "Done.",
        },
      },
      {
        id: "structured-notice",
        event: "system_notice",
        timestampMs: Date.parse("2026-05-27T08:00:01.000Z"),
        data: {
          message: typedCommsNotice({
            peer: "fugue/issue_lead/LUC-642/issue_lead",
            kind: "response",
            body: "Done.",
          }),
        },
      },
    ],
    { renderInteractionStartsAsUser: true },
  );

  assert.equal(entries.length, 1);
  assert.equal(entries[0]?.identity.id, "comms");
  assert.equal(entries.some((entry) => (
    entry.identity.id === "user"
    && "text" in entry
    && entry.text.includes("Peer response from fugue/issue_lead")
  )), false);
});

test("mapFramesToTimelineEntries suppresses raw peer request envelopes when structured comms notice exists", () => {
  const entries = mapFramesToTimelineEntries(
    {
      agent_id: "planner",
      member_id: "planner",
      label: "Planner",
      kind: "identity",
    },
    [
      {
        id: "raw-run-started",
        event: "run_started",
        timestampMs: Date.parse("2026-05-27T08:00:00.000Z"),
        data: {
          prompt:
            "Peer request\n"
            + "Peer request from fugue/issue_lead/LUC-642/issue_lead:\n"
            + "Can you validate the current plan?",
        },
      },
      {
        id: "structured-notice",
        event: "system_notice",
        timestampMs: Date.parse("2026-05-27T08:00:01.000Z"),
        data: {
          message: typedCommsNotice({
            peer: "fugue/issue_lead/LUC-642/issue_lead",
            kind: "request",
            body: "Can you validate the current plan?",
          }),
        },
      },
    ],
    { renderInteractionStartsAsUser: true },
  );

  assert.equal(entries.length, 1);
  assert.equal(entries[0]?.identity.id, "comms");
  assert.equal(entries.some((entry) => (
    entry.identity.id === "user"
    && "text" in entry
    && entry.text.includes("Peer request from fugue/issue_lead")
  )), false);
});

test("mapFramesToTimelineEntries suppresses plain intent/body peer envelopes when structured comms notice exists", () => {
  const entries = mapFramesToTimelineEntries(
    {
      agent_id: "planner",
      member_id: "planner",
      label: "Planner",
      kind: "identity",
    },
    [
      {
        id: "raw-run-started",
        event: "run_started",
        timestampMs: Date.parse("2026-05-27T08:00:00.000Z"),
        data: {
          prompt:
            "Peer request from fugue/peer-a:\n"
            + "Intent: request_summary\n"
            + "Body: Summarize the incident.",
        },
      },
      {
        id: "structured-notice",
        event: "system_notice",
        timestampMs: Date.parse("2026-05-27T08:00:01.000Z"),
        data: {
          message: typedCommsNotice({
            peer: "fugue/peer-a",
            kind: "request",
            body: "Summarize the incident.",
          }),
        },
      },
    ],
    { renderInteractionStartsAsUser: true },
  );

  assert.equal(entries.length, 1);
  assert.equal(entries[0]?.identity.id, "comms");
  assert.equal(entries.some((entry) => (
    entry.identity.id === "user"
    && "text" in entry
    && entry.text.includes("Intent: request_summary")
  )), false);
});

test("mapFramesToTimelineEntries suppresses duplicated intent/body peer envelopes when structured comms notice exists", () => {
  const entries = mapFramesToTimelineEntries(
    {
      agent_id: "planner",
      member_id: "planner",
      label: "Planner",
      kind: "identity",
    },
    [
      {
        id: "raw-run-started",
        event: "run_started",
        timestampMs: Date.parse("2026-05-27T08:00:00.000Z"),
        data: {
          prompt:
            "Peer request\n"
            + "Peer request from fugue/peer-a:\n"
            + "Peer request from fugue/peer-a:\n"
            + "Intent: request_summary\n"
            + "Body: Summarize the incident.\n"
            + "Peer request",
        },
      },
      {
        id: "structured-notice",
        event: "system_notice",
        timestampMs: Date.parse("2026-05-27T08:00:01.000Z"),
        data: {
          message: typedCommsNotice({
            peer: "fugue/peer-a",
            kind: "request",
            body: "Summarize the incident.",
          }),
        },
      },
    ],
    { renderInteractionStartsAsUser: true },
  );

  assert.equal(entries.length, 1);
  assert.equal(entries[0]?.identity.id, "comms");
});

test("mapFramesToTimelineEntries suppresses duplicated bracketed intent/body comms envelopes", () => {
  const entries = mapFramesToTimelineEntries(
    {
      agent_id: "planner",
      member_id: "planner",
      label: "Planner",
      kind: "identity",
    },
    [
      {
        id: "raw-run-started",
        event: "run_started",
        timestampMs: Date.parse("2026-05-27T08:00:00.000Z"),
        data: {
          prompt:
            "[COMMS REQUEST from fugue/peer-a]\n"
            + "[COMMS REQUEST from fugue/peer-a]\n"
            + "Intent: request_summary\n"
            + "Body: Summarize the incident.",
        },
      },
      {
        id: "structured-notice",
        event: "system_notice",
        timestampMs: Date.parse("2026-05-27T08:00:01.000Z"),
        data: {
          message: typedCommsNotice({
            peer: "fugue/peer-a",
            kind: "request",
            body: "Summarize the incident.",
          }),
        },
      },
    ],
    { renderInteractionStartsAsUser: true },
  );

  assert.equal(entries.length, 1);
  assert.equal(entries[0]?.identity.id, "comms");
});

test("mapFramesToTimelineEntries does not suppress peer requests with structured peer responses", () => {
  const entries = mapFramesToTimelineEntries(
    {
      agent_id: "planner",
      member_id: "planner",
      label: "Planner",
      kind: "identity",
    },
    [
      {
        id: "raw-request",
        event: "run_started",
        timestampMs: Date.parse("2026-05-27T08:00:00.000Z"),
        data: {
          prompt: "Peer request from fugue/peer-a:\nDone.",
        },
      },
      {
        id: "structured-response",
        event: "system_notice",
        timestampMs: Date.parse("2026-05-27T08:00:01.000Z"),
        data: {
          message: typedCommsNotice({
            peer: "fugue/peer-a",
            body: "Done.",
            kind: "response",
          }),
        },
      },
    ],
    { renderInteractionStartsAsUser: true },
  );

  assert.equal(entries.length, 2);
  assert.equal(entries[0]?.identity.id, "user");
  assert.equal(entries[1]?.identity.id, "comms");
});

test("mapFramesToTimelineEntries consumes one raw peer prompt per structured duplicate", () => {
  const entries = mapFramesToTimelineEntries(
    {
      agent_id: "planner",
      member_id: "planner",
      label: "Planner",
      kind: "identity",
    },
    [
      {
        id: "raw-one",
        event: "run_started",
        data: {
          prompt: "Peer message from fugue/peer-a:\nSame update.",
        },
      },
      {
        id: "raw-two",
        event: "run_started",
        data: {
          prompt: "Peer message from fugue/peer-a:\nSame update.",
        },
      },
      {
        id: "structured-one",
        event: "system_notice",
        data: {
          message: typedCommsNotice({
            peer: "fugue/peer-a",
            body: "Same update.",
          }),
        },
      },
      {
        id: "structured-two",
        event: "system_notice",
        data: {
          message: typedCommsNotice({
            peer: "fugue/peer-a",
            body: "Same update.",
            requestId: "second",
          }),
        },
      },
    ],
    { renderInteractionStartsAsUser: true },
  );

  assert.equal(entries.length, 2);
  assert.equal(entries.every((entry) => entry.identity.id === "comms"), true);
});

test("mapFramesToTimelineEntries does not overconsume raw prompts for one legacy structured notice", () => {
  const entries = mapFramesToTimelineEntries(
    {
      agent_id: "planner",
      member_id: "planner",
      label: "Planner",
      kind: "identity",
    },
    [
      {
        id: "raw-one",
        event: "run_started",
        data: {
          prompt: "Peer message from fugue/peer-a:\nSame update.",
        },
      },
      {
        id: "raw-two",
        event: "run_started",
        data: {
          prompt: "Peer message from fugue/peer-a:\nSame update.",
        },
      },
      {
        id: "structured-one",
        event: "system_notice",
        data: {
          message: {
            kind: "comms",
            body: "Peer message from fugue/peer-a:\nSame update.",
          },
        },
      },
    ],
    { renderInteractionStartsAsUser: true },
  );

  assert.equal(entries.length, 2);
  assert.equal(entries.filter((entry) => entry.identity.id === "user").length, 1);
  assert.equal(entries.filter((entry) => entry.identity.id === "comms").length, 1);
});

test("mapFramesToTimelineEntries dedupes alias-equivalent typed comms notice signatures", () => {
  const entries = mapFramesToTimelineEntries(
    {
      agent_id: "planner",
      member_id: "planner",
      label: "Planner",
      kind: "identity",
    },
    [
      {
        id: "raw-one",
        event: "run_started",
        data: {
          prompt: "Peer message from fugue/peer-display:\nSame update.",
        },
      },
      {
        id: "raw-two",
        event: "run_started",
        data: {
          prompt: "Peer message from fugue/peer-display:\nSame update.",
        },
      },
      {
        id: "structured-one",
        event: "system_notice",
        data: {
          message: typedCommsNotice({
            peer: "fugue/peer-display",
            peerId: "fugue/peer-id",
            peerDisplayName: "fugue/peer-display",
            body: "Peer message from fugue/peer-display:\nSame update.",
          }),
        },
      },
    ],
    { renderInteractionStartsAsUser: true },
  );

  assert.equal(entries.length, 2);
  assert.equal(entries.filter((entry) => entry.identity.id === "user").length, 1);
  assert.equal(entries.filter((entry) => entry.identity.id === "comms").length, 1);
});

test("mapFramesToTimelineEntries consumes duplicate raw prompts for duplicate typed comms blocks", () => {
  const entries = mapFramesToTimelineEntries(
    {
      agent_id: "planner",
      member_id: "planner",
      label: "Planner",
      kind: "identity",
    },
    [
      {
        id: "raw-one",
        event: "run_started",
        data: {
          prompt: "Peer message from fugue/peer-a:\nSame update.",
        },
      },
      {
        id: "raw-two",
        event: "run_started",
        data: {
          prompt: "Peer message from fugue/peer-a:\nSame update.",
        },
      },
      {
        id: "structured-one",
        event: "system_notice",
        data: {
          message: {
            role: "system_notice",
            kind: "comms",
            blocks: [
              {
                type: "comms",
                kind: "message",
                direction: "incoming",
                peer: {
                  id: "fugue/peer-a",
                  display_name: "fugue/peer-a",
                },
                content: [{ type: "text", text: "Same update." }],
              },
              {
                type: "comms",
                kind: "message",
                direction: "incoming",
                peer: {
                  id: "fugue/peer-a",
                  display_name: "fugue/peer-a",
                },
                content: [{ type: "text", text: "Same update." }],
              },
            ],
          },
        },
      },
    ],
    { renderInteractionStartsAsUser: true },
  );

  assert.equal(entries.filter((entry) => entry.identity.id === "user").length, 0);
  assert.equal(entries.filter((entry) => entry.identity.id === "comms").length, 1);
});

test("mapFramesToTimelineEntries suppresses raw prompts for body-only typed comms notices", () => {
  const entries = mapFramesToTimelineEntries(
    {
      agent_id: "planner",
      member_id: "planner",
      label: "Planner",
      kind: "identity",
    },
    [
      {
        id: "raw-one",
        event: "run_started",
        data: {
          prompt: "Peer message from fugue/peer-a:\nClean body.",
        },
      },
      {
        id: "structured-one",
        event: "system_notice",
        data: {
          message: {
            role: "system_notice",
            kind: "comms",
            body: "Clean body.",
            blocks: [{
              type: "comms",
              kind: "message",
              direction: "incoming",
              peer: {
                id: "fugue/peer-a",
                display_name: "fugue/peer-a",
              },
              request_id: "body-only",
            }],
          },
        },
      },
    ],
    { renderInteractionStartsAsUser: true },
  );

  assert.equal(entries.filter((entry) => entry.identity.id === "user").length, 0);
  assert.equal(entries.filter((entry) => entry.identity.id === "comms").length, 1);
  const block = entries[0] && "blocks" in entries[0] && Array.isArray(entries[0].blocks)
    ? entries[0].blocks[0]
    : null;
  assert.equal(block?.type === "tool-call" ? block.peerBody : "", "Clean body.");
});

test("mapFramesToTimelineEntries suppresses bracketed intent/body comms prompts", () => {
  const entries = mapFramesToTimelineEntries(
    {
      agent_id: "planner",
      member_id: "planner",
      label: "Planner",
      kind: "identity",
    },
    [
      {
        id: "raw-one",
        event: "run_started",
        data: {
          prompt: "[COMMS REQUEST from fugue/peer-a]\nIntent: request_summary\nBody: Summarize the incident.",
        },
      },
      {
        id: "structured-one",
        event: "system_notice",
        data: {
          message: typedCommsNotice({
            peer: "fugue/peer-a",
            kind: "request",
            intent: "request_summary",
            body: "Summarize the incident.",
          }),
        },
      },
    ],
    { renderInteractionStartsAsUser: true },
  );

  assert.equal(entries.filter((entry) => entry.identity.id === "user").length, 0);
  assert.equal(entries.filter((entry) => entry.identity.id === "comms").length, 1);
});

test("mapFramesToTimelineEntries preserves literal Body lines in clean comms bodies", () => {
  const body = "Here is the report\nBody: section one";
  const entries = mapFramesToTimelineEntries(
    {
      agent_id: "planner",
      member_id: "planner",
      label: "Planner",
      kind: "identity",
    },
    [
      {
        id: "raw-one",
        event: "run_started",
        data: {
          prompt: `[COMMS REQUEST from fugue/peer-a]\nIntent: request_summary\nBody: ${body}`,
        },
      },
      {
        id: "structured-one",
        event: "system_notice",
        data: {
          message: typedCommsNotice({
            peer: "fugue/peer-a",
            kind: "request",
            intent: "request_summary",
            body,
          }),
        },
      },
    ],
    { renderInteractionStartsAsUser: true },
  );

  assert.equal(entries.filter((entry) => entry.identity.id === "user").length, 0);
  assert.equal(entries.filter((entry) => entry.identity.id === "comms").length, 1);
  const block = entries[0] && "blocks" in entries[0] && Array.isArray(entries[0].blocks)
    ? entries[0].blocks[0]
    : null;
  assert.equal(
    block?.type === "tool-call" ? block.peerBody : "",
    "Here is the report Body: section one",
  );
});

test("mapFramesToTimelineEntries preserves clean comms bodies that look like intent wrappers", () => {
  const body = "Intent: preserve this line\nBody: preserve this too";
  const entries = mapFramesToTimelineEntries(
    {
      agent_id: "planner",
      member_id: "planner",
      label: "Planner",
      kind: "identity",
    },
    [
      {
        id: "raw-one",
        event: "run_started",
        data: {
          prompt: `[COMMS REQUEST from fugue/peer-a]\nIntent: request_summary\nBody: ${body}`,
        },
      },
      {
        id: "structured-one",
        event: "system_notice",
        data: {
          message: typedCommsNotice({
            peer: "fugue/peer-a",
            kind: "request",
            intent: "request_summary",
            body,
          }),
        },
      },
    ],
    { renderInteractionStartsAsUser: true },
  );

  assert.equal(entries.filter((entry) => entry.identity.id === "user").length, 0);
  assert.equal(entries.filter((entry) => entry.identity.id === "comms").length, 1);
  const block = entries[0] && "blocks" in entries[0] && Array.isArray(entries[0].blocks)
    ? entries[0].blocks[0]
    : null;
  assert.equal(
    block?.type === "tool-call" ? block.peerBody : "",
    "Intent: preserve this line Body: preserve this too",
  );
});

test("mapFramesToTimelineEntries preserves quoted peer envelope lines in clean comms bodies", () => {
  const body = "Peer message from incident-lead:\nPlease keep this quote.";
  const entries = mapFramesToTimelineEntries(
    {
      agent_id: "planner",
      member_id: "planner",
      label: "Planner",
      kind: "identity",
    },
    [
      {
        id: "structured-one",
        event: "system_notice",
        data: {
          message: typedCommsNotice({
            peer: "review:singleton",
            kind: "message",
            body,
          }),
        },
      },
    ],
    { renderInteractionStartsAsUser: true },
  );

  const block = entries[0] && "blocks" in entries[0] && Array.isArray(entries[0].blocks)
    ? entries[0].blocks[0]
    : null;
  assert.equal(
    block?.type === "tool-call" ? block.peerBody : "",
    "Peer message from incident-lead: Please keep this quote.",
  );
});

test("mapFramesToTimelineEntries preserves same-peer envelope lines quoted inside clean typed comms bodies", () => {
  const body = "Quoted wrapper:\nPeer message from review:singleton:\nPlease keep this quote.";
  const entries = mapFramesToTimelineEntries(
    {
      agent_id: "planner",
      member_id: "planner",
      label: "Planner",
      kind: "identity",
    },
    [
      {
        id: "structured-one",
        event: "system_notice",
        data: {
          message: typedCommsNotice({
            peer: "review:singleton",
            kind: "message",
            body,
          }),
        },
      },
    ],
    { renderInteractionStartsAsUser: true },
  );

  const block = entries[0] && "blocks" in entries[0] && Array.isArray(entries[0].blocks)
    ? entries[0].blocks[0]
    : null;
  assert.equal(
    block?.type === "tool-call" ? block.peerBody : "",
    "Quoted wrapper: Peer message from review:singleton: Please keep this quote.",
  );
});

test("mapFramesToTimelineEntries preserves bracketed COMMS note literals in clean comms bodies", () => {
  const body = "[COMMS NOTE]\nIntent: preserve this\nBody: preserve this too";
  const entries = mapFramesToTimelineEntries(
    {
      agent_id: "planner",
      member_id: "planner",
      label: "Planner",
      kind: "identity",
    },
    [
      {
        id: "structured-one",
        event: "system_notice",
        data: {
          message: typedCommsNotice({
            peer: "review:singleton",
            kind: "message",
            body,
          }),
        },
      },
    ],
    { renderInteractionStartsAsUser: true },
  );

  const block = entries[0] && "blocks" in entries[0] && Array.isArray(entries[0].blocks)
    ? entries[0].blocks[0]
    : null;
  assert.equal(
    block?.type === "tool-call" ? block.peerBody : "",
    "[COMMS NOTE] Intent: preserve this Body: preserve this too",
  );
});

test("mapFramesToTimelineEntries preserves quoted bracketed COMMS envelopes in clean comms bodies", () => {
  const body = "[COMMS MESSAGE from incident-lead]\nIntent: preserve this\nBody: preserve this too";
  const entries = mapFramesToTimelineEntries(
    {
      agent_id: "planner",
      member_id: "planner",
      label: "Planner",
      kind: "identity",
    },
    [
      {
        id: "structured-one",
        event: "system_notice",
        data: {
          message: typedCommsNotice({
            peer: "review:singleton",
            kind: "message",
            body,
          }),
        },
      },
    ],
    { renderInteractionStartsAsUser: true },
  );

  const block = entries[0] && "blocks" in entries[0] && Array.isArray(entries[0].blocks)
    ? entries[0].blocks[0]
    : null;
  assert.equal(
    block?.type === "tool-call" ? block.peerBody : "",
    "[COMMS MESSAGE from incident-lead] Intent: preserve this Body: preserve this too",
  );
});

test("mapFramesToTimelineEntries preserves standalone peer scaffold words in clean comms bodies", () => {
  const body = "Peer message\nPlease keep this heading.";
  const entries = mapFramesToTimelineEntries(
    {
      agent_id: "planner",
      member_id: "planner",
      label: "Planner",
      kind: "identity",
    },
    [
      {
        id: "structured-one",
        event: "system_notice",
        data: {
          message: typedCommsNotice({
            peer: "review:singleton",
            kind: "message",
            body,
          }),
        },
      },
    ],
    { renderInteractionStartsAsUser: true },
  );

  const block = entries[0] && "blocks" in entries[0] && Array.isArray(entries[0].blocks)
    ? entries[0].blocks[0]
    : null;
  assert.equal(
    block?.type === "tool-call" ? block.peerBody : "",
    "Peer message Please keep this heading.",
  );
});

// Meerkat 0.7.1 canonical model-facing peer-request projection: transport
// envelope (peer_spec address + pubkey bytes) plus protocol coaching. This is
// what the runtime stuffs into the typed comms block content for mob kickoff
// requests; none of it may surface in user-facing chat.
const MEERKAT_071_PEER_REQUEST_PROJECTION =
  "Peer request from peer_id 6f6114cd-2cf7-590f-a172-0e36feacd12c"
  + " (display_name: incident-command-center/commander/incident-commander)"
  + " (id: 964020b4-c9b6-4c31-ba6c-30598279b388)\n"
  + "Intent: mob.kickoff_started\n"
  + "Params: {\n"
  + "  \"peer\": \"incident-commander\",\n"
  + "  \"peer_spec\": {\n"
  + "    \"address\": \"inproc://incident-command-center/commander/incident-commander\",\n"
  + "    \"peer_id\": \"6f6114cd-2cf7-590f-a172-0e36feacd12c\",\n"
  + "    \"pubkey\": [20, 129, 97, 58, 74, 93, 150, 7]\n"
  + "  },\n"
  + "  \"role\": \"commander\"\n"
  + "}\n"
  + "Request ID: 964020b4-c9b6-4c31-ba6c-30598279b388\n"
  + "\n"
  + "This is a correlated peer request. Reply with send_response with arguments"
  + " {\"in_reply_to\":\"964020b4-c9b6-4c31-ba6c-30598279b388\","
  + "\"peer_id\":\"6f6114cd-2cf7-590f-a172-0e36feacd12c\",\"status\":\"completed\"}."
  + " Use status=\"failed\" instead of \"completed\" when the request cannot be"
  + " fulfilled, and include result only when the request contract provides a"
  + " typed result payload. Do not answer this request with send_message.";

function meerkat071KickoffNotice() {
  return {
    role: "system_notice",
    kind: "comms",
    body: "Peer request: mob.kickoff_started",
    blocks: [{
      type: "comms",
      kind: "request",
      direction: "incoming",
      peer: {
        id: "6f6114cd-2cf7-590f-a172-0e36feacd12c",
        display_name: "incident-command-center/commander/incident-commander",
      },
      request_id: "964020b4-c9b6-4c31-ba6c-30598279b388",
      intent: "mob.kickoff_started",
      summary: "Peer request: mob.kickoff_started",
      payload: {
        peer: "incident-commander",
        peer_spec: {
          address: "inproc://incident-command-center/commander/incident-commander",
          peer_id: "6f6114cd-2cf7-590f-a172-0e36feacd12c",
          pubkey: [20, 129, 97, 58, 74, 93, 150, 7],
        },
        role: "commander",
      },
      content: [{ type: "text", text: MEERKAT_071_PEER_REQUEST_PROJECTION }],
    }],
  };
}

test("mapFramesToTimelineEntries never renders the meerkat 0.7.1 peer transport projection as the comms body", () => {
  const entries = mapFramesToTimelineEntries(
    {
      agent_id: "scribe",
      member_id: "scribe",
      label: "Scribe",
      kind: "mob_agent",
    },
    [
      {
        id: "raw-run-started",
        event: "run_started",
        timestampMs: Date.parse("2026-06-12T18:48:01.000Z"),
        data: {
          prompt:
            "Peer request: mob.kickoff_started\n"
            + MEERKAT_071_PEER_REQUEST_PROJECTION,
        },
      },
      {
        id: "structured-notice",
        event: "system_notice",
        timestampMs: Date.parse("2026-06-12T18:48:01.519Z"),
        sourceKind: "session_history",
        data: { message: meerkat071KickoffNotice() },
      },
    ],
    { renderInteractionStartsAsUser: true },
  );

  const commsEntry = entries.find((entry) => entry.identity.id === "comms");
  assert.ok(commsEntry, "typed comms notice should render a comms entry");
  const block = commsEntry && "blocks" in commsEntry && Array.isArray(commsEntry.blocks)
    ? commsEntry.blocks[0]
    : null;
  assert.equal(block?.type, "tool-call");
  const peerBody = block?.type === "tool-call" ? block.peerBody || "" : "";
  assert.equal(peerBody, "Peer request: mob.kickoff_started");
  for (const marker of ["pubkey", "peer_spec", "send_response", "Do not answer this request"]) {
    assert.ok(
      !peerBody.includes(marker),
      `comms body must not leak transport scaffold marker "${marker}": ${peerBody}`,
    );
  }

  for (const entry of entries) {
    if (entry.identity.id !== "user" || entry.kind !== "message" || !("text" in entry)) continue;
    assert.ok(
      !/pubkey|peer_spec|Do not answer this request/.test(entry.text),
      `user entries must not leak transport scaffold: ${entry.text}`,
    );
  }
});

test("mapFramesToTimelineEntries drops pure peer transport scaffold run prompts even without a matching comms notice", () => {
  const entries = mapFramesToTimelineEntries(
    {
      agent_id: "scribe",
      member_id: "scribe",
      label: "Scribe",
      kind: "mob_agent",
    },
    [
      {
        id: "raw-run-started",
        event: "run_started",
        timestampMs: Date.parse("2026-06-12T18:48:01.000Z"),
        data: { prompt: MEERKAT_071_PEER_REQUEST_PROJECTION },
      },
    ],
    { renderInteractionStartsAsUser: true },
  );

  assert.equal(
    entries.some((entry) => entry.identity.id === "user"),
    false,
    `pure transport scaffold must not render as a user prompt: ${JSON.stringify(entries)}`,
  );
});

test("stripPeerTransportScaffold keeps human-authored remainder and drops the envelope", () => {
  assert.equal(stripPeerTransportScaffold(MEERKAT_071_PEER_REQUEST_PROJECTION), "");
  assert.equal(
    stripPeerTransportScaffold(
      `${MEERKAT_071_PEER_REQUEST_PROJECTION}\nPlease summarize the incident.`,
    ),
    "Please summarize the incident.",
  );
  // Single-line `prompt_text` form terminates with the send_message coaching.
  assert.equal(
    stripPeerTransportScaffold(
      "Peer request from peer_id 6f6114cd-2cf7-590f-a172-0e36feacd12c."
      + " Intent: review. Request ID: req-1. Params: {\"x\":1}."
      + " This is not a normal user request and not a prompt for direct"
      + " user-facing output. Reply with send_response."
      + " Do not use send_message for this reply.",
    ),
    "",
  );
  // Truncated envelope (no coaching terminator) still never leaks.
  assert.equal(
    stripPeerTransportScaffold(
      "Peer request from peer_id 6f6114cd-2cf7-590f-a172-0e36feacd12c\nParams: { \"pubkey\": [1, 2, 3",
    ),
    "",
  );
  // Ordinary peer text is untouched.
  assert.equal(
    stripPeerTransportScaffold("Peer request from fugue/peer-a:\nDone."),
    "Peer request from fugue/peer-a:\nDone.",
  );
});

test("mapFramesToTimelineEntries does not suppress explicit peer request with generic structured comms", () => {
  const entries = mapFramesToTimelineEntries(
    {
      agent_id: "planner",
      member_id: "planner",
      label: "Planner",
      kind: "identity",
    },
    [
      {
        id: "raw-request",
        event: "run_started",
        timestampMs: Date.parse("2026-05-27T08:00:00.000Z"),
        data: {
          prompt: "Peer request from fugue/peer-a:\nDone.",
        },
      },
      {
        id: "generic-structured",
        event: "system_notice",
        timestampMs: Date.parse("2026-05-27T08:00:01.000Z"),
        data: {
          message: typedCommsNotice({
            peer: "fugue/peer-a",
            body: "Done.",
          }),
        },
      },
    ],
    { renderInteractionStartsAsUser: true },
  );

  assert.equal(entries.length, 2);
  assert.equal(entries[0]?.identity.id, "user");
  assert.equal(entries[1]?.identity.id, "comms");
});

test("mapFramesToTimelineEntries suppresses raw bracketed comms response envelopes when structured comms notice exists", () => {
  const entries = mapFramesToTimelineEntries(
    {
      agent_id: "planner",
      member_id: "planner",
      label: "Planner",
      kind: "identity",
    },
    [
      {
        id: "raw-run-started",
        event: "run_started",
        timestampMs: Date.parse("2026-05-27T08:00:00.000Z"),
        data: {
          prompt:
            "[COMMS RESPONSE from fugue/issue_lead/LUC-642/issue_lead]\n"
            + "Done.",
        },
      },
      {
        id: "structured-notice",
        event: "system_notice",
        timestampMs: Date.parse("2026-05-27T08:00:01.000Z"),
        data: {
          message: typedCommsNotice({
            peer: "fugue/issue_lead/LUC-642/issue_lead",
            kind: "response",
            body: "Done.",
          }),
        },
      },
    ],
    { renderInteractionStartsAsUser: true },
  );

  assert.equal(entries.length, 1);
  assert.equal(entries[0]?.identity.id, "comms");
  assert.equal(entries.some((entry) => (
    entry.identity.id === "user"
    && "text" in entry
    && entry.text.includes("[COMMS RESPONSE from fugue/issue_lead")
  )), false);
});

test("mapFramesToTimelineEntries suppresses one-line peer envelopes whose body contains colons", () => {
  const entries = mapFramesToTimelineEntries(
    {
      agent_id: "planner",
      member_id: "planner",
      label: "Planner",
      kind: "identity",
    },
    [
      {
        id: "raw-run-started",
        event: "run_started",
        timestampMs: Date.parse("2026-05-27T08:00:00.000Z"),
        data: {
          prompt: "Peer message from review:singleton: Error: failed",
        },
      },
      {
        id: "structured-notice",
        event: "system_notice",
        timestampMs: Date.parse("2026-05-27T08:00:01.000Z"),
        data: {
          message: typedCommsNotice({
            peer: "review:singleton",
            body: "Error: failed",
          }),
        },
      },
    ],
    { renderInteractionStartsAsUser: true },
  );

  assert.equal(entries.length, 1);
  assert.equal(entries[0]?.identity.id, "comms");
});

test("mapFramesToTimelineEntries suppresses bannered peer envelopes whose body contains colons", () => {
  const entries = mapFramesToTimelineEntries(
    {
      agent_id: "planner",
      member_id: "planner",
      label: "Planner",
      kind: "identity",
    },
    [
      {
        id: "raw-run-started",
        event: "run_started",
        timestampMs: Date.parse("2026-05-27T08:00:00.000Z"),
        data: {
          prompt:
            "Peer message\n"
            + "Peer message from review:singleton: Error: failed\n"
            + "Peer message",
        },
      },
      {
        id: "structured-notice",
        event: "system_notice",
        timestampMs: Date.parse("2026-05-27T08:00:01.000Z"),
        data: {
          message: typedCommsNotice({
            peer: "review:singleton",
            body: "Error: failed",
          }),
        },
      },
    ],
    { renderInteractionStartsAsUser: true },
  );

  assert.equal(entries.length, 1);
  assert.equal(entries[0]?.identity.id, "comms");
});

test("mapFramesToTimelineEntries suppresses only the nearest duplicate raw peer prompt", () => {
  const entries = mapFramesToTimelineEntries(
    {
      agent_id: "planner",
      member_id: "planner",
      label: "Planner",
      kind: "identity",
    },
    [
      {
        id: "early-raw-run-started",
        event: "run_started",
        timestampMs: Date.parse("2026-05-27T08:00:00.000Z"),
        data: { prompt: "Peer message from review:singleton: Done." },
      },
      {
        id: "nearest-raw-run-started",
        event: "run_started",
        timestampMs: Date.parse("2026-05-27T08:00:09.000Z"),
        data: { prompt: "Peer message from review:singleton: Done." },
      },
      {
        id: "structured-notice",
        event: "system_notice",
        timestampMs: Date.parse("2026-05-27T08:00:10.000Z"),
        data: {
          message: typedCommsNotice({
            peer: "review:singleton",
            body: "Done.",
          }),
        },
      },
    ],
    { renderInteractionStartsAsUser: true },
  );

  assert.equal(entries.length, 2);
  assert.equal(entries[0]?.identity.id, "user");
  assert.equal(entries[1]?.identity.id, "comms");
});

test("mapFramesToTimelineEntries does not suppress later envelope-looking prompts", () => {
  const entries = mapFramesToTimelineEntries(
    {
      agent_id: "planner",
      member_id: "planner",
      label: "Planner",
      kind: "identity",
    },
    [
      {
        id: "actual-duplicate-before-structured",
        event: "run_started",
        timestampMs: Date.parse("2026-05-27T08:00:00.000Z"),
        data: { prompt: "Peer message from review:singleton: Done." },
      },
      {
        id: "structured-notice",
        event: "system_notice",
        timestampMs: Date.parse("2026-05-27T08:00:10.000Z"),
        data: {
          message: typedCommsNotice({
            peer: "review:singleton",
            body: "Done.",
          }),
        },
      },
      {
        id: "later-operator-looking-prompt",
        event: "run_started",
        timestampMs: Date.parse("2026-05-27T08:00:11.000Z"),
        data: { prompt: "Peer message from review:singleton: Done." },
      },
    ],
    { renderInteractionStartsAsUser: true },
  );

  assert.equal(entries.length, 2);
  assert.equal(entries[0]?.identity.id, "comms");
  assert.equal(entries[1]?.identity.id, "user");
  assert.equal(entries[1]?.id, "later-operator-looking-prompt:2");
});

test("mapFramesToTimelineEntries does not suppress quoted peer envelopes inside operator prompts", () => {
  const entries = mapFramesToTimelineEntries(
    {
      agent_id: "planner",
      member_id: "planner",
      label: "Planner",
      kind: "identity",
    },
    [
      {
        id: "operator-run-started",
        event: "run_started",
        timestampMs: Date.parse("2026-05-27T08:00:00.000Z"),
        data: {
          prompt: "Please investigate this:\nPeer message from review:singleton: Done.",
        },
      },
      {
        id: "structured-notice",
        event: "system_notice",
        timestampMs: Date.parse("2026-05-27T08:00:01.000Z"),
        data: {
          message: typedCommsNotice({
            peer: "review:singleton",
            body: "Done.",
          }),
        },
      },
    ],
    { renderInteractionStartsAsUser: true },
  );

  assert.equal(entries.length, 2);
  assert.equal(entries[0]?.identity.id, "user");
  assert.equal(entries[0] && "text" in entries[0] ? entries[0].text : "", "Please investigate this:\nPeer message from review:singleton: Done.");
  assert.equal(entries[1]?.identity.id, "comms");
});

test("mapFramesToTimelineEntries suppresses nearest duplicate by frame order without timestamps", () => {
  const entries = mapFramesToTimelineEntries(
    {
      agent_id: "planner",
      member_id: "planner",
      label: "Planner",
      kind: "identity",
    },
    [
      {
        id: "early-raw-run-started",
        event: "run_started",
        data: { prompt: "Peer message from review:singleton: Done." },
      },
      {
        id: "structured-notice",
        event: "system_notice",
        data: {
          message: typedCommsNotice({
            peer: "review:singleton",
            body: "Done.",
          }),
        },
      },
      {
        id: "later-raw-run-started",
        event: "run_started",
        data: { prompt: "Peer message from review:singleton: Done." },
      },
    ],
    { renderInteractionStartsAsUser: true },
  );

  assert.equal(entries.length, 2);
  assert.equal(entries[0]?.identity.id, "comms");
  assert.equal(entries[1]?.identity.id, "user");
  assert.equal(entries[1]?.id, "later-raw-run-started:2");
});

test("mapFramesToTimelineEntries does not suppress colon-prefixed peer aliases", () => {
  const entries = mapFramesToTimelineEntries(
    {
      agent_id: "planner",
      member_id: "planner",
      label: "Planner",
      kind: "identity",
    },
    [
      {
        id: "raw-run-started",
        event: "run_started",
        timestampMs: Date.parse("2026-05-27T08:00:00.000Z"),
        data: { prompt: "Peer message from review:singleton: Error" },
      },
      {
        id: "structured-notice",
        event: "system_notice",
        timestampMs: Date.parse("2026-05-27T08:00:01.000Z"),
        data: {
          message: typedCommsNotice({
            peer: "review",
            body: "singleton: Error",
          }),
        },
      },
    ],
    { renderInteractionStartsAsUser: true },
  );

  assert.equal(entries.length, 2);
  assert.equal(entries[0]?.identity.id, "user");
  assert.equal(entries[1]?.identity.id, "comms");
});

test("mapFramesToTimelineEntries suppresses runtime-id peer envelopes without truncating colon bodies", () => {
  const entries = mapFramesToTimelineEntries(
    {
      agent_id: "planner",
      member_id: "planner",
      label: "Planner",
      kind: "identity",
    },
    [
      {
        id: "raw-run-started",
        event: "run_started",
        timestampMs: Date.parse("2026-05-27T08:00:00.000Z"),
        data: {
          prompt: "Peer message from rt:review:singleton:0: Error: failed",
        },
      },
      {
        id: "structured-notice",
        event: "system_notice",
        timestampMs: Date.parse("2026-05-27T08:00:01.000Z"),
        data: {
          message: typedCommsNotice({
            peer: "rt:review:singleton:0",
            body: "Error: failed",
          }),
        },
      },
    ],
    { renderInteractionStartsAsUser: true },
  );

  assert.equal(entries.length, 1);
  assert.equal(entries[0]?.identity.id, "comms");
});

test("mapFramesToTimelineEntries strips typed one-line comms envelopes by structured peer alias", () => {
  const entries = mapFramesToTimelineEntries(
    {
      agent_id: "planner",
      member_id: "planner",
      label: "Planner",
      kind: "identity",
    },
    [
      {
        id: "structured-notice",
        event: "system_notice",
        timestampMs: Date.parse("2026-05-27T08:00:01.000Z"),
        data: {
          message: typedCommsNotice({
            peer: "review:singleton",
            body: "Peer message from review:singleton: Error: failed",
          }),
        },
      },
    ],
    { renderInteractionStartsAsUser: true },
  );

  assert.equal(entries.length, 1);
  const block = entries[0] && "blocks" in entries[0] ? entries[0].blocks?.[0] : undefined;
  assert.equal(block?.type, "tool-call");
  assert.equal(block?.type === "tool-call" ? block.peerBody : "", "Error: failed");
});

test("mapFramesToTimelineEntries does not suppress unrelated same-body peer prompts", () => {
  const entries = mapFramesToTimelineEntries(
    {
      agent_id: "planner",
      member_id: "planner",
      label: "Planner",
      kind: "identity",
    },
    [
      {
        id: "notice-peer-a",
        event: "system_notice",
        timestampMs: Date.parse("2026-05-27T08:00:00.000Z"),
        data: {
          message: typedCommsNotice({
            peer: "fugue/peer-a",
            body: "Done.",
          }),
        },
      },
      {
        id: "run-peer-b",
        event: "run_started",
        timestampMs: Date.parse("2026-05-27T08:00:01.000Z"),
        data: {
          prompt:
            "Peer message\n"
            + "Peer message from fugue/peer-b:\n"
            + "Done.\n"
            + "Peer message",
        },
      },
    ],
    { renderInteractionStartsAsUser: true },
  );

  assert.equal(entries.length, 2);
  assert.equal(entries[0]?.identity.id, "comms");
  assert.equal(entries[1]?.identity.id, "user");
  assert.equal(
    entries[1] && "text" in entries[1] ? entries[1].text : "",
    "Peer message\nPeer message from fugue/peer-b:\nDone.\nPeer message",
  );
});

test("mapFramesToTimelineEntries deduplicates live and history copies of the same comms notice", () => {
  const entries = mapFramesToTimelineEntries(
    {
      agent_id: "planner",
      member_id: "planner",
      label: "Planner",
      kind: "identity",
    },
    [
      {
        id: "live-notice",
        event: "system_notice",
        timestampMs: Date.parse("2026-05-27T08:00:00.000Z"),
        data: {
          message: typedCommsNotice({
            peer: "fugue/issue_lead/LUC-642/issue_lead",
            body: "Focused RED-review replan is complete.",
          }),
        },
      },
      {
        id: "history-notice",
        event: "system_notice",
        sourceKind: "session_history",
        timestampMs: Date.parse("2026-05-27T08:00:02.000Z"),
        data: {
          message: typedCommsNotice({
            peer: "fugue/issue_lead/LUC-642/issue_lead",
            body: "Focused RED-review replan is complete.",
          }),
        },
      },
    ],
    { renderInteractionStartsAsUser: true },
  );

  assert.equal(entries.length, 1);
  assert.equal(entries[0]?.identity.id, "comms");
});

test("mapFramesToTimelineEntries deduplicates live comms and session-history terminal system notices", () => {
  const notice = typedCommsNotice({
    peer: "fugue/peer-a",
    body: "Done.",
  });
  const entries = mapFramesToTimelineEntries(
    {
      agent_id: "planner",
      member_id: "planner",
      label: "Planner",
      kind: "identity",
    },
    [
      {
        id: "live-notice",
        event: "system_notice",
        timestampMs: Date.parse("2026-05-27T08:00:00.000Z"),
        data: { message: notice },
      },
      {
        id: "history-text-complete",
        event: "text_complete",
        sourceKind: "session_history",
        timestampMs: Date.parse("2026-05-27T08:00:02.000Z"),
        data: { message: notice },
      },
      {
        id: "history-interaction-complete",
        event: "interaction_complete",
        sourceKind: "session_history",
        timestampMs: Date.parse("2026-05-27T08:00:03.000Z"),
        data: { message: notice },
      },
    ],
    { renderInteractionStartsAsUser: true },
  );

  assert.equal(entries.length, 1);
  assert.equal(entries[0]?.identity.id, "comms");
});

test("mapFramesToTimelineEntries deduplicates comms notices when volatile status drifts", () => {
  const liveNotice = typedCommsNotice({
    peer: "fugue/peer-a",
    body: "Done.",
  });
  liveNotice.blocks[0] = {
    ...liveNotice.blocks[0],
    status: "sent",
    state: "pending",
  };
  const historyNotice = typedCommsNotice({
    peer: "fugue/peer-a",
    body: "Done.",
  });
  historyNotice.blocks[0] = {
    ...historyNotice.blocks[0],
    status: "delivered",
    state: "complete",
  };

  const entries = mapFramesToTimelineEntries(
    {
      agent_id: "planner",
      member_id: "planner",
      label: "Planner",
      kind: "identity",
    },
    [
      {
        id: "live-notice",
        event: "system_notice",
        timestampMs: Date.parse("2026-05-27T08:00:00.000Z"),
        data: { message: liveNotice },
      },
      {
        id: "history-interaction-complete",
        event: "interaction_complete",
        sourceKind: "session_history",
        timestampMs: Date.parse("2026-05-27T08:00:03.000Z"),
        data: { message: historyNotice },
      },
    ],
    { renderInteractionStartsAsUser: true },
  );

  assert.equal(entries.length, 1);
  assert.equal(entries[0]?.identity.id, "comms");
  assert.equal(entries[0]?.variant, "rich");
  const block = entries[0] && "blocks" in entries[0] ? entries[0].blocks?.[0] : null;
  assert.equal(block?.type, "tool-call");
  assert.equal(block?.peerBody, "Done.");
});

test("mapFramesToTimelineEntries deduplicates body-only comms notices when volatile status drifts", () => {
  const liveNotice = typedCommsNotice({
    peer: "fugue/peer-a",
    body: "Done.",
  });
  liveNotice.blocks[0] = {
    ...liveNotice.blocks[0],
    content: undefined,
    status: "sent",
    state: "pending",
  };
  const historyNotice = typedCommsNotice({
    peer: "fugue/peer-a",
    body: "Done.",
  });
  historyNotice.blocks[0] = {
    ...historyNotice.blocks[0],
    content: undefined,
    status: "delivered",
    state: "complete",
  };

  const entries = mapFramesToTimelineEntries(
    {
      agent_id: "planner",
      member_id: "planner",
      label: "Planner",
      kind: "identity",
    },
    [
      {
        id: "live-notice",
        event: "system_notice",
        timestampMs: Date.parse("2026-05-27T08:00:00.000Z"),
        data: { message: liveNotice },
      },
      {
        id: "history-interaction-complete",
        event: "interaction_complete",
        sourceKind: "session_history",
        timestampMs: Date.parse("2026-05-27T08:00:03.000Z"),
        data: { message: historyNotice },
      },
    ],
    { renderInteractionStartsAsUser: true },
  );

  assert.equal(entries.length, 1);
  assert.equal(entries[0]?.identity.id, "comms");
  assert.equal(entries[0]?.variant, "rich");
  const block = entries[0] && "blocks" in entries[0] ? entries[0].blocks?.[0] : null;
  assert.equal(block?.type, "tool-call");
  assert.equal(block?.peerBody, "Done.");
});

test("mapFramesToTimelineEntries suppresses raw peer prompts using session-history terminal system notices", () => {
  const notice = typedCommsNotice({
    peer: "fugue/peer-a",
    body: "Done.",
  });
  const entries = mapFramesToTimelineEntries(
    {
      agent_id: "planner",
      member_id: "planner",
      label: "Planner",
      kind: "identity",
    },
    [
      {
        id: "raw-peer-prompt",
        event: "run_started",
        timestampMs: Date.parse("2026-05-27T08:00:00.000Z"),
        data: { prompt: "Peer message from fugue/peer-a:\nDone." },
      },
      {
        id: "history-text-complete",
        event: "text_complete",
        sourceKind: "session_history",
        timestampMs: Date.parse("2026-05-27T08:00:02.000Z"),
        data: { message: notice },
      },
    ],
    { renderInteractionStartsAsUser: true },
  );

  assert.equal(entries.length, 1);
  assert.equal(entries[0]?.identity.id, "comms");
});

test("mapFramesToTimelineEntries suppresses raw peer prompts using body-only status-drift comms notices", () => {
  const notice = typedCommsNotice({
    peer: "fugue/peer-a",
    body: "Done.",
  });
  notice.blocks[0] = {
    ...notice.blocks[0],
    content: undefined,
    status: "delivered",
    state: "complete",
  };
  const entries = mapFramesToTimelineEntries(
    {
      agent_id: "planner",
      member_id: "planner",
      label: "Planner",
      kind: "identity",
    },
    [
      {
        id: "raw-peer-prompt",
        event: "run_started",
        timestampMs: Date.parse("2026-05-27T08:00:00.000Z"),
        data: { prompt: "Peer message from fugue/peer-a:\nDone." },
      },
      {
        id: "history-text-complete",
        event: "text_complete",
        sourceKind: "session_history",
        timestampMs: Date.parse("2026-05-27T08:00:02.000Z"),
        data: { message: notice },
      },
    ],
    { renderInteractionStartsAsUser: true },
  );

  assert.equal(entries.length, 1);
  assert.equal(entries[0]?.identity.id, "comms");
  assert.equal(entries[0]?.variant, "rich");
  const block = entries[0] && "blocks" in entries[0] ? entries[0].blocks?.[0] : null;
  assert.equal(block?.type, "tool-call");
  assert.equal(block?.peerBody, "Done.");
});

test("mapFramesToTimelineEntries keeps opposite-direction comms with the same peer request and body", () => {
  const entries = mapFramesToTimelineEntries(
    {
      agent_id: "planner",
      member_id: "planner",
      label: "Planner",
      kind: "identity",
    },
    [
      {
        id: "live-outgoing",
        event: "system_notice",
        timestampMs: Date.parse("2026-05-27T08:00:00.000Z"),
        data: {
          message: typedCommsNotice({
            peer: "fugue/peer-a",
            body: "Done.",
            direction: "outgoing",
            requestId: "req-shared",
          }),
        },
      },
      {
        id: "history-incoming",
        event: "system_notice",
        sourceKind: "session_history",
        timestampMs: Date.parse("2026-05-27T08:00:02.000Z"),
        data: {
          message: typedCommsNotice({
            peer: "fugue/peer-a",
            body: "Done.",
            direction: "incoming",
            requestId: "req-shared",
          }),
        },
      },
    ],
    { renderInteractionStartsAsUser: true },
  );

  assert.equal(entries.length, 2);
  const blocks = entries.map((entry) => (
    entry && "blocks" in entry ? entry.blocks?.[0] : null
  ));
  assert.equal(blocks[0]?.type, "tool-call");
  assert.equal(blocks[0]?.peerIncoming, false);
  assert.equal(blocks[1]?.type, "tool-call");
  assert.equal(blocks[1]?.peerIncoming, true);
});

test("mapFramesToTimelineEntries preserves leading peer-envelope-looking text in structured comms content", () => {
  const entries = mapFramesToTimelineEntries(
    {
      agent_id: "planner",
      member_id: "planner",
      label: "Planner",
      kind: "identity",
    },
    [
      {
        id: "notice-quoted-envelope",
        event: "system_notice",
        timestampMs: Date.parse("2026-05-27T08:00:00.000Z"),
        data: {
          message: typedCommsNotice({
            peer: "review:singleton",
            body: "Peer message from review:singleton:\nPlease keep this quote.",
          }),
        },
      },
    ],
    { renderInteractionStartsAsUser: true },
  );

  assert.equal(entries.length, 1);
  const block = entries[0] && "blocks" in entries[0] ? entries[0].blocks?.[0] : null;
  assert.equal(block?.type, "tool-call");
  assert.equal(
    block?.peerBody,
    "Peer message from review:singleton: Please keep this quote.",
  );
});

test("mapFramesToTimelineEntries preserves leading peer-envelope-looking text in body-only structured comms", () => {
  const entries = mapFramesToTimelineEntries(
    {
      agent_id: "planner",
      member_id: "planner",
      label: "Planner",
      kind: "identity",
    },
    [
      {
        id: "notice-body-only-quote",
        event: "system_notice",
        timestampMs: Date.parse("2026-05-27T08:00:00.000Z"),
        data: {
          message: {
            role: "system_notice",
            kind: "comms",
            body: "Peer message from review:singleton:\nPlease keep this quote.",
            blocks: [{
              type: "comms",
              kind: "message",
              direction: "incoming",
              peer: { id: "review:singleton", display_name: "review:singleton" },
              request_id: "req-body-only",
            }],
          },
        },
      },
    ],
    { renderInteractionStartsAsUser: true },
  );

  assert.equal(entries.length, 1);
  const block = entries[0] && "blocks" in entries[0] ? entries[0].blocks?.[0] : null;
  assert.equal(block?.type, "tool-call");
  assert.equal(
    block?.peerBody,
    "Peer message from review:singleton: Please keep this quote.",
  );
});

test("mapFramesToTimelineEntries preserves leading slash peer envelopes in structured comms content", () => {
  const entries = mapFramesToTimelineEntries(
    {
      agent_id: "planner",
      member_id: "planner",
      label: "Planner",
      kind: "identity",
    },
    [
      {
        id: "notice-slash-envelope",
        event: "system_notice",
        timestampMs: Date.parse("2026-05-27T08:00:00.000Z"),
        data: {
          message: typedCommsNotice({
            peer: "fugue/issue_lead/LUC-642/issue_lead",
            body: "Peer message from fugue/issue_lead/LUC-642/issue_lead:\nPlease keep this quote.",
          }),
        },
      },
    ],
    { renderInteractionStartsAsUser: true },
  );

  assert.equal(entries.length, 1);
  const block = entries[0] && "blocks" in entries[0] ? entries[0].blocks?.[0] : null;
  assert.equal(block?.type, "tool-call");
  assert.equal(
    block?.peerBody,
    "Peer message from fugue/issue_lead/LUC-642/issue_lead: Please keep this quote.",
  );
});

test("mapFramesToTimelineEntries keeps mixed comms notice frames when later blocks differ", () => {
  const multiNotice = (
    id: string,
    sourceKind: "session_history" | undefined,
    secondPeer: string,
    secondBody: string,
  ) => ({
    id,
    event: "system_notice",
    sourceKind,
    timestampMs: Date.parse("2026-05-27T08:00:00.000Z"),
    data: {
      message: {
        role: "system_notice",
        kind: "comms",
        blocks: [
          {
            type: "comms",
            kind: "message",
            direction: "incoming",
            peer: { id: "peer-a", display_name: "peer-a" },
            request_id: "shared-a",
            content: [{ type: "text", text: "same" }],
          },
          {
            type: "comms",
            kind: "message",
            direction: "incoming",
            peer: { id: secondPeer, display_name: secondPeer },
            request_id: `unique-${secondPeer}`,
            content: [{ type: "text", text: secondBody }],
          },
        ],
      },
    },
  });
  const entries = mapFramesToTimelineEntries(
    {
      agent_id: "planner",
      member_id: "planner",
      label: "Planner",
      kind: "identity",
    },
    [
      multiNotice("live-notice", undefined, "peer-b", "unique live"),
      multiNotice("history-notice", "session_history", "peer-c", "unique history"),
    ],
    { renderInteractionStartsAsUser: true },
  );

  assert.equal(entries.length, 2);
  const firstBlocks = entries[0] && "blocks" in entries[0] ? entries[0].blocks ?? [] : [];
  const secondBlocks = entries[1] && "blocks" in entries[1] ? entries[1].blocks ?? [] : [];
  assert.equal(firstBlocks.length, 2);
  assert.equal(secondBlocks.length, 1);
  assert.equal(
    secondBlocks[0]?.type === "tool-call" ? secondBlocks[0].peerTarget : "",
    "peer-c",
  );
  assert.equal(
    secondBlocks[0]?.type === "tool-call" ? secondBlocks[0].peerBody : "",
    "unique history",
  );
});

test("mapFramesToTimelineEntries filters duplicate legacy comms blocks in mixed notices", () => {
  const legacyNotice = (
    id: string,
    sourceKind: "session_history" | undefined,
    secondPeer: string,
    secondBody: string,
  ) => ({
    id,
    event: "system_notice",
    sourceKind,
    timestampMs: Date.parse("2026-05-27T08:00:00.000Z"),
    data: {
      message: {
        role: "system_notice",
        kind: "generic",
        blocks: [
          {
            type: "text",
            body: "Peer message from peer-a:\nsame",
          },
          {
            type: "text",
            body: `Peer message from ${secondPeer}:\n${secondBody}`,
          },
        ],
      },
    },
  });
  const entries = mapFramesToTimelineEntries(
    {
      agent_id: "planner",
      member_id: "planner",
      label: "Planner",
      kind: "identity",
    },
    [
      legacyNotice("live-notice", undefined, "peer-b", "unique live"),
      legacyNotice("history-notice", "session_history", "peer-c", "unique history"),
    ],
    { renderInteractionStartsAsUser: true },
  );

  assert.equal(entries.length, 2);
  const secondBlocks = entries[1] && "blocks" in entries[1] ? entries[1].blocks ?? [] : [];
  assert.equal(secondBlocks.length, 1);
  assert.equal(secondBlocks[0]?.type === "divider" ? secondBlocks[0].text : "", "Peer message from peer-c:\nunique history");
});

test("mapFramesToTimelineEntries keeps same-peer same-body comms with different request ids", () => {
  const entries = mapFramesToTimelineEntries(
    {
      agent_id: "planner",
      member_id: "planner",
      label: "Planner",
      kind: "identity",
    },
    [
      {
        id: "live-notice",
        event: "system_notice",
        timestampMs: Date.parse("2026-05-27T08:00:00.000Z"),
        data: {
          message: typedCommsNotice({
            peer: "fugue/peer-a",
            body: "Done.",
            requestId: "request-a",
          }),
        },
      },
      {
        id: "history-notice",
        event: "system_notice",
        sourceKind: "session_history",
        timestampMs: Date.parse("2026-05-27T08:00:02.000Z"),
        data: {
          message: typedCommsNotice({
            peer: "fugue/peer-a",
            body: "Done.",
            requestId: "request-b",
          }),
        },
      },
    ],
    { renderInteractionStartsAsUser: true },
  );

  assert.equal(entries.length, 2);
  assert.equal(entries[0]?.identity.id, "comms");
  assert.equal(entries[1]?.identity.id, "comms");
});

test("mapFramesToTimelineEntries keeps same-body history comms from different peers", () => {
  const entries = mapFramesToTimelineEntries(
    {
      agent_id: "planner",
      member_id: "planner",
      label: "Planner",
      kind: "identity",
    },
    [
      {
        id: "live-notice",
        event: "system_notice",
        timestampMs: Date.parse("2026-05-27T08:00:00.000Z"),
        data: {
          message: typedCommsNotice({
            peer: "fugue/peer-a",
            body: "Done.",
          }),
        },
      },
      {
        id: "history-notice",
        event: "system_notice",
        sourceKind: "session_history",
        timestampMs: Date.parse("2026-05-27T08:00:02.000Z"),
        data: {
          message: typedCommsNotice({
            peer: "fugue/peer-b",
            body: "Done.",
          }),
        },
      },
    ],
    { renderInteractionStartsAsUser: true },
  );

  assert.equal(entries.length, 2);
  assert.equal(entries[0]?.identity.id, "comms");
  assert.equal(entries[1]?.identity.id, "comms");
});

test("mapFramesToTimelineEntries renders live untyped peer system notices", () => {
  const entries = mapFramesToTimelineEntries(
    {
      agent_id: "deep-investigator",
      member_id: "deep-investigator",
      label: "Deep Investigator",
      kind: "identity",
    },
    [
      {
        id: "live-untyped-peer-notice",
        event: "system_notice",
        timestampMs: Date.parse("2026-05-23T21:10:00.000Z"),
        data: {
          blocks: [{
            content: [{
              type: "text",
              text: "Peer message from ob3/investigation-worker/investigation-worker-live-proof:\nLIVE_PEER_NOTICE landed in the parent chat.",
            }],
          }],
        },
      },
    ],
    { renderInteractionStartsAsUser: true },
  );

  assert.equal(entries.length, 1);
  assert.equal(entries[0]?.identity.id, "comms");
  const text = entries[0] && "blocks" in entries[0] && entries[0].blocks?.[0]?.type === "paragraph"
    ? entries[0].blocks[0].text
    : "";
  assert.match(text, /LIVE_PEER_NOTICE landed in the parent chat/);
});

test("mapFramesToTimelineEntries renders live tool-config system notices as metadata", () => {
  const entries = mapFramesToTimelineEntries(
    {
      agent_id: "catalog-worker",
      member_id: "catalog-worker",
      label: "catalog-worker",
      kind: "identity",
    },
    [
      {
        id: "live-tool-config-notice",
        event: "system_notice",
        timestampMs: Date.parse("2026-05-15T09:00:00.000Z"),
        data: {
          role: "system_notice",
          kind: "tool_scope",
          render_class: "tool_scope_notice",
          body: "Deferred catalog changed at turn boundary: new deferred tools available: docs_search",
          blocks: [{
            type: "tool_config",
            payload: {
              operation: "reload",
              target: "deferred_catalog",
              status: "deferred_catalog_delta(added_hidden=1,removed_hidden=0,pending_sources=0)",
              status_info: {
                kind: "deferred_catalog_delta",
                added_hidden_count: 1,
                removed_hidden_count: 0,
                pending_source_count: 0,
              },
              persisted: false,
              domain: "deferred_catalog",
            },
          }],
        },
      },
    ],
    { renderInteractionStartsAsUser: true },
  );

  assert.equal(entries.length, 1);
  assert.equal(entries[0]?.identity.id, "comms");
  assert.equal(entries[0]?.variant, "rich");
  const block = entries[0] && "blocks" in entries[0] ? entries[0].blocks?.[0] : null;
  assert.equal(block?.type, "divider");
  assert.equal(
    block?.type === "divider" ? block.text : "",
    "Deferred catalog changed at turn boundary: new deferred tools available: docs_search",
  );
});

test("mapFramesToTimelineEntries renders live non-comms system notices without tool or image blocks", () => {
  const entries = mapFramesToTimelineEntries(
    {
      agent_id: "ops-worker",
      member_id: "ops-worker",
      label: "ops-worker",
      kind: "identity",
    },
    [
      {
        id: "live-mcp-notice",
        event: "system_notice",
        timestampMs: Date.parse("2026-05-15T09:01:00.000Z"),
        data: {
          message: {
            role: "system_notice",
            kind: "mcp",
            body: "MCP server docs connected",
            blocks: [{
              type: "mcp",
              server_id: "docs",
              detail: "MCP server docs connected",
              persisted: true,
            }],
          },
        },
      },
      {
        id: "live-runtime-notice",
        event: "system_notice",
        timestampMs: Date.parse("2026-05-15T09:02:00.000Z"),
        data: {
          message: {
            role: "system_notice",
            kind: "generic",
            body: "Runtime recovered from transient stream lag",
            blocks: [{
              type: "runtime_notice",
              category: "stream",
              detail: "Runtime recovered from transient stream lag",
            }],
          },
        },
      },
    ],
    { renderInteractionStartsAsUser: true },
  );

  assert.equal(entries.length, 2);
  assert.equal(entries[0]?.identity.id, "comms");
  assert.equal(entries[0]?.variant, "rich");
  const mcpBlock = entries[0] && "blocks" in entries[0] ? entries[0].blocks?.[0] : null;
  assert.equal(mcpBlock?.type, "divider");
  assert.equal(mcpBlock?.type === "divider" ? mcpBlock.text : "", "MCP server docs connected");

  const runtimeBlock = entries[1] && "blocks" in entries[1] ? entries[1].blocks?.[0] : null;
  assert.equal(runtimeBlock?.type, "paragraph");
  assert.equal(
    runtimeBlock?.type === "paragraph" ? runtimeBlock.text : "",
    "Runtime recovered from transient stream lag",
  );
});

test("systemNoticeClearsBusyState only treats peer/comms notices as terminal", () => {
  const activeToolFrames = [
    {
      id: "tool-start",
      event: "tool_execution_started" as const,
      timestampMs: 1,
      data: { name: "king_search" },
    },
    {
      id: "tool-done",
      event: "tool_execution_completed" as const,
      timestampMs: 2,
      data: { name: "king_search", result: "ok" },
    },
  ];
  const runtimeNotice = {
    id: "runtime-notice",
    event: "system_notice" as const,
    timestampMs: 3,
    data: {
      message: {
        role: "system_notice",
        kind: "generic",
        body: "Received from runtime control plane: recovered from transient stream lag",
        blocks: [{
          type: "runtime_notice",
          category: "stream",
          detail: "Received from runtime control plane: recovered from transient stream lag",
        }],
      },
    },
  };
  const toolConfigNotice = {
    id: "tool-config-notice",
    event: "system_notice" as const,
    timestampMs: 4,
    data: {
      role: "system_notice",
      kind: "tool_scope",
      body: "Peer message from tool catalog is not real comms; deferred tools available: docs_search",
      blocks: [{ type: "tool_config", payload: { operation: "reload" } }],
    },
  };
  const nestedRuntimePeerPhraseNotice = {
    id: "nested-runtime-peer-phrase-notice",
    event: "system_notice" as const,
    timestampMs: 4.5,
    data: {
      message: {
        role: "system_notice",
        kind: "generic",
        body: "Runtime metadata update",
        blocks: [{
          type: "runtime_notice",
          category: "stream",
          detail: "runtime payload mentioned Peer message from docs-worker but it is not a comms notice",
          payload: {
            text: "Peer message from docs-worker should not count when nested in runtime metadata",
          },
        }],
      },
    },
  };
  const commsNotice = {
    id: "comms-notice",
    event: "system_notice" as const,
    timestampMs: 5,
    data: {
      message: typedCommsNotice({
        peer: "ob3/delegate/worker",
        body: "Peer result landed.",
      }),
    },
  };
  const untypedPeerNotice = {
    id: "untyped-peer-notice",
    event: "system_notice" as const,
    timestampMs: 6,
    data: {
      blocks: [{
        content: [{
          type: "text",
          text: "Peer message from ob3/worker:\nFinished.",
        }],
      }],
    },
  };
  const runtimeReceivedNotice = {
    id: "runtime-received-notice",
    event: "system_notice" as const,
    timestampMs: 7,
    data: {
      message: {
        role: "system_notice",
        kind: "generic",
        body: "Received from runtime metadata channel: lease renewed",
        blocks: [{
          type: "runtime_notice",
          category: "lease",
          detail: "Received from runtime metadata channel: lease renewed",
        }],
      },
    },
  };

  assert.equal(systemNoticeClearsBusyState(runtimeNotice), false);
  assert.equal(systemNoticeClearsBusyState(toolConfigNotice), false);
  assert.equal(systemNoticeClearsBusyState(runtimeReceivedNotice), false);
  assert.equal(systemNoticeClearsBusyState(nestedRuntimePeerPhraseNotice), false);
  assert.equal(systemNoticeClearsBusyState(commsNotice), true);
  assert.equal(systemNoticeClearsBusyState(untypedPeerNotice), true);
  assert.equal(
    inferResponsePhaseFromFrames([...activeToolFrames, runtimeNotice], null),
    "waiting",
    "runtime notices must not make an active tool turn look idle",
  );
  assert.equal(
    inferResponsePhaseFromFrames([...activeToolFrames, toolConfigNotice], null),
    "waiting",
    "tool-config notices must not make an active tool turn look idle",
  );
  assert.equal(
    inferResponsePhaseFromFrames([...activeToolFrames, commsNotice], "waiting"),
    null,
    "peer/comms notices still clear completed peer-send busy state",
  );
});

test("mapFramesToTimelineEntries hides external-event system notices that duplicate user input", () => {
  const entries = mapFramesToTimelineEntries(
    {
      agent_id: "incident-commander",
      member_id: "incident-commander",
      label: "Incident Commander",
      kind: "identity",
    },
    [
      {
        id: "user-input",
        event: "user_input",
        timestampMs: 1,
        data: { content: "Create a worker" },
      },
      {
        id: "external-event-history",
        event: "system_notice",
        sourceKind: "session_history",
        timestampMs: 2,
        data: {
          message: {
            role: "system_notice",
            kind: "external_event",
            body: "Create a worker",
            blocks: [{
              type: "external_event",
              source: "rpc",
              event_type: "rpc",
              summary: "Create a worker",
              body: "Create a worker",
              payload: { body: "Create a worker" },
            }],
          },
        },
      },
    ],
    { renderInteractionStartsAsUser: true },
  );

  assert.equal(entries.length, 1);
  assert.equal(entries[0]?.identity.id, "user");
  assert.equal("text" in (entries[0] || {}) ? entries[0]?.text : "", "Create a worker");
});

test("mapFramesToTimelineEntries hides image external-event notices after rich user input", () => {
  const entries = mapFramesToTimelineEntries(
    {
      agent_id: "api-investigator",
      member_id: "api-investigator",
      label: "API Investigator",
      kind: "identity",
    },
    [
      {
        id: "user-input",
        event: "user_input",
        timestampMs: 1,
        data: {
          content: [
            { type: "text", text: "Describe the attached image." },
            {
              type: "image",
              source: "blob",
              blob_id: "sha256:badge",
              media_type: "image/png",
            },
          ],
        },
      },
      {
        id: "external-event-history",
        event: "system_notice",
        sourceKind: "session_history",
        timestampMs: 2,
        data: {
          message: {
            role: "system_notice",
            kind: "external_event",
            body: "Describe the attached image.\n[image: image/png]",
            blocks: [{
              type: "external_event",
              source: "rpc",
              event_type: "rpc",
              summary: "Describe the attached image.\n[image: image/png]",
              body: "Describe the attached image.\n[image: image/png]",
              content: [
                { type: "text", text: "Describe the attached image." },
                {
                  type: "image",
                  source: "blob",
                  blob_id: "sha256:badge",
                  media_type: "image/png",
                },
              ],
            }],
          },
        },
      },
    ],
    { renderInteractionStartsAsUser: true, blobBaseUrl: "http://localhost:63212" },
  );

  assert.equal(entries.length, 1);
  assert.equal(entries[0]?.identity.id, "user");
  assert.equal(entries[0]?.variant, "rich");
  const blocks = entries[0] && "blocks" in entries[0] ? entries[0].blocks : [];
  assert.equal(blocks?.filter((block) => block.type === "paragraph").length, 1);
  assert.equal(blocks?.filter((block) => block.type === "image").length, 1);
});

test("mapFramesToTimelineEntries renders session-history typed peer system notices as comms metadata", () => {
  const entries = mapFramesToTimelineEntries(
    {
      agent_id: "final-grandchild-worker",
      member_id: "final-grandchild-worker",
      label: "final-grandchild-worker",
      kind: "identity",
    },
    [
      {
        id: "history-peer-system-notice",
        event: "system_notice",
        sourceKind: "session_history",
        timestampMs: Date.parse("2026-05-11T21:10:23.000Z"),
        data: {
          message: typedCommsNotice({
            peer: "incident-command-center/commander/final-child-worker",
            body: "ping acknowledgement to final-child-worker",
            kind: "request",
            intent: "checksum_token",
            requestId: "8d04e806-3316-40c4-816f-345ded237fbc",
            payload: { subject: "ping acknowledgement to final-child-worker" },
          }),
        },
      },
    ],
    { renderInteractionStartsAsUser: true },
  );

  assert.equal(entries.length, 1);
  assert.equal(entries[0]?.identity.id, "comms");
  assert.equal(entries[0]?.variant, "rich");
  const block = entries[0] && "blocks" in entries[0] ? entries[0].blocks?.[0] : null;
  assert.equal(block?.type, "tool-call");
  assert.equal(block?.peerIncoming, true);
  assert.equal(block?.peerTarget, "final-child-worker");
  assert.equal(block?.peerIntent, "checksum_token");
  assert.equal(block?.peerBody, "ping acknowledgement to final-child-worker");
});

test("mapFramesToTimelineEntries renders session-history peer image notices as comms plus image", () => {
  const entries = mapFramesToTimelineEntries(
    {
      agent_id: "incident-commander",
      member_id: "incident-commander",
      label: "Incident Commander",
      kind: "identity",
    },
    [
      {
        id: "history-peer-image",
        event: "system_notice",
        sourceKind: "session_history",
        timestampMs: Date.parse("2026-05-11T21:12:00.000Z"),
        data: {
          message: {
            role: "system_notice",
            blocks: [{
              type: "comms",
              kind: "message",
              peer: { display_name: "incident-command-center/scribe/scribe" },
              request_id: "peer-image-1",
              content: [
                { type: "text", text: "Generated incident badge forwarded." },
                {
                  type: "image_ref",
                  blob_ref: {
                    blob_id: "sha256:badge/1",
                    media_type: "image/png",
                  },
                  media_type: "image/png",
                  alt: "incident badge",
                },
              ],
            }],
          },
        },
      },
    ],
    {
      blobBaseUrl: "http://127.0.0.1:7000",
      renderInteractionStartsAsUser: true,
    },
  );

  assert.equal(entries.length, 1);
  assert.equal(entries[0]?.identity.id, "comms");
  assert.equal(entries[0]?.variant, "rich");
  const blocks = entries[0] && "blocks" in entries[0] ? entries[0].blocks || [] : [];
  assert.equal(blocks.length, 1);
  assert.equal(blocks[0]?.type, "tool-call");
  assert.equal(blocks[0]?.type === "tool-call" ? blocks[0].peerIncoming : false, true);
  assert.equal(blocks[0]?.type === "tool-call" ? blocks[0].peerTarget : "", "scribe");
  assert.equal(blocks[0]?.type === "tool-call" ? blocks[0].peerBody : "", "Generated incident badge forwarded.");
  const peerImages = blocks[0]?.type === "tool-call" ? blocks[0].peerImages || [] : [];
  assert.equal(peerImages.length, 1);
  assert.equal(
    peerImages[0]?.src || "",
    "http://127.0.0.1:7000/blobs/sha256%3Abadge%2F1",
  );
  assert.equal(peerImages[0]?.alt || "", "incident badge");
});

test("mapFramesToTimelineEntries renders live typed peer blob-ref image notices as comms plus image", () => {
  const entries = mapFramesToTimelineEntries(
    {
      agent_id: "image-recipient",
      member_id: "image-recipient",
      label: "Image Recipient",
      kind: "identity",
    },
    [
      {
        id: "live-peer-image",
        event: "system_notice",
        timestampMs: Date.parse("2026-06-03T21:12:00.000Z"),
        data: {
          message: {
            role: "system_notice",
            blocks: [{
              type: "comms",
              kind: "message",
              peer: { display_name: "ob3/delegate/image-artist-2" },
              request_id: "peer-image-live-1",
              content: [
                { type: "text", text: "Saved to generated_images/ob3_admin_role_v2.png." },
                {
                  type: "image",
                  image: {
                    blob_ref: {
                      blob_id: "sha256:ob3-admin-role",
                      media_type: "image/png",
                    },
                    width: 1024,
                    height: 1024,
                    image_id: "image-ob3-admin-role",
                  },
                },
              ],
            }],
          },
        },
      },
    ],
    {
      blobBaseUrl: "http://127.0.0.1:7000",
      renderInteractionStartsAsUser: true,
    },
  );

  assert.equal(entries.length, 1);
  assert.equal(entries[0]?.identity.id, "comms");
  const blocks = entries[0] && "blocks" in entries[0] ? entries[0].blocks || [] : [];
  assert.equal(blocks.length, 1);
  assert.equal(blocks[0]?.type, "tool-call");
  assert.equal(blocks[0]?.type === "tool-call" ? blocks[0].peerTarget : "", "image-artist-2");
  assert.equal(blocks[0]?.type === "tool-call" ? blocks[0].peerBody : "", "Saved to generated_images/ob3_admin_role_v2.png.");
  const peerImages = blocks[0]?.type === "tool-call" ? blocks[0].peerImages || [] : [];
  assert.equal(peerImages.length, 1);
  assert.equal(
    peerImages[0]?.src || "",
    "http://127.0.0.1:7000/blobs/sha256%3Aob3-admin-role",
  );
  assert.equal(peerImages[0]?.width || 0, 1024);
  assert.equal(peerImages[0]?.imageId || "", "image-ob3-admin-role");
});

test("mapFramesToTimelineEntries suppresses raw peer image prompts when structured comms image exists", () => {
  const entries = mapFramesToTimelineEntries(
    {
      agent_id: "scribe",
      member_id: "scribe",
      label: "Scribe",
      kind: "identity",
    },
    [
      {
        id: "raw-run-started",
        event: "run_started",
        timestampMs: Date.parse("2026-06-03T22:06:43.000Z"),
        data: {
          prompt:
            "Peer message\n"
            + "Peer message from incident-command-center/commander/incident-commander:\n"
            + "Peer message from incident-command-center/commander/incident-commander:\n"
            + "Generated fictional CardinalPay payments-api outage dashboard image with required labels and rollback 64%.\n"
            + "Generated fictional CardinalPay payments-api outage dashboard image with required labels and rollback 64%.\n"
            + "[image: image/png]\n"
            + "Peer message",
        },
      },
      {
        id: "structured-comms-image",
        event: "system_notice",
        timestampMs: Date.parse("2026-06-03T22:06:42.000Z"),
        data: {
          message: {
            role: "system_notice",
            blocks: [{
              type: "comms",
              kind: "message",
              peer: { display_name: "incident-command-center/commander/incident-commander" },
              request_id: "peer-image-duplicate",
              content: [
                {
                  type: "text",
                  text:
                    "Generated fictional CardinalPay payments-api outage dashboard image with required labels and rollback 64%.\n"
                    + "Generated fictional CardinalPay payments-api outage dashboard image with required labels and rollback 64%.",
                },
                {
                  type: "image_ref",
                  blob_ref: {
                    blob_id: "sha256:cardinalpay-dashboard",
                    media_type: "image/png",
                  },
                  media_type: "image/png",
                },
              ],
            }],
          },
        },
      },
    ],
    {
      blobBaseUrl: "http://127.0.0.1:7000",
      renderInteractionStartsAsUser: true,
    },
  );

  assert.equal(entries.length, 1);
  assert.equal(entries[0]?.identity.id, "comms");
  const blocks = entries[0] && "blocks" in entries[0] ? entries[0].blocks || [] : [];
  assert.equal(blocks.length, 1);
  const block = blocks[0];
  assert.equal(block?.type, "tool-call");
  assert.equal(block?.type === "tool-call" ? block.peerTarget : "", "incident-commander");
  assert.equal(block?.type === "tool-call" ? (block.peerImages || []).length : 0, 1);
});

test("mapFramesToTimelineEntries does not render raw peer image run-started prompts", () => {
  const entries = mapFramesToTimelineEntries(
    {
      agent_id: "scribe",
      member_id: "scribe",
      label: "Scribe",
      kind: "identity",
    },
    [
      {
        id: "raw-peer-image-run-started",
        event: "run_started",
        timestampMs: Date.parse("2026-06-03T22:30:01.655Z"),
        data: {
          prompt: [
            {
              type: "text",
              text:
                "Peer message\n"
                + "Peer message from incident-command-center/commander/incident-commander:\n"
                + "Peer message from incident-command-center/commander/incident-commander:\n"
                + "codex-comms-image-mpyn1bac Generated fictional CardinalPay payments-api outage dashboard image with required labels and rollback 64%.\n"
                + "codex-comms-image-mpyn1bac Generated fictional CardinalPay payments-api outage dashboard image with required labels and rollback 64%.\n"
                + "[image: image/png]\n"
                + "Peer message",
            },
            {
              type: "image",
              source: "inline",
              media_type: "image/png",
              data: "iVBORw0KGgo=",
            },
          ],
        },
      },
    ],
    {
      blobBaseUrl: "http://127.0.0.1:7000",
      renderInteractionStartsAsUser: true,
    },
  );

  assert.equal(entries.length, 0);
});

test("mapFramesToTimelineEntries resolves session-history peer tool targets from live peers results", () => {
  const entries = mapFramesToTimelineEntries(
    {
      agent_id: "incident-commander",
      member_id: "incident-commander",
      label: "Incident Commander",
      kind: "identity",
    },
    [
      {
        id: "peers-result",
        event: "tool_execution_completed",
        timestampMs: Date.parse("2026-05-11T21:29:47.000Z"),
        data: {
          id: "call-peers",
          name: "peers",
          result: JSON.stringify({
            peers: [{
              peer_id: "16e23049-513c-5ec1-94f6-892a5daf2f89",
              name: "incident-command-center/scribe/scribe",
            }],
          }),
        },
      },
      {
        id: "history-send-message",
        event: "interaction_complete",
        sourceKind: "session_history",
        timestampMs: Date.parse("2026-05-11T21:29:50.000Z"),
        data: {
          message: {
            role: "block_assistant",
            stop_reason: "tool_use",
            blocks: [{
              block_type: "tool_use",
              data: {
                id: "call-send",
                name: "send_message",
                args: {
                  peer_id: "16e23049-513c-5ec1-94f6-892a5daf2f89",
                  body: "peer label check",
                },
              },
            }],
          },
        },
      },
    ],
    { renderInteractionStartsAsUser: true },
  );

  const toolEntry = entries.find((entry) =>
    entry.kind === "message"
    && "blocks" in entry
    && entry.blocks?.[0]?.type === "tool-call"
  );
  const block = toolEntry && "blocks" in toolEntry ? toolEntry.blocks?.[0] : null;
  assert.equal(block?.type, "tool-call");
  assert.equal(block?.peerTarget, "scribe");
});

test("mapFramesToTimelineEntries summarizes image refs in peer send_message tool bodies", () => {
  const entries = mapFramesToTimelineEntries(
    {
      agent_id: "image-worker",
      member_id: "image-worker",
      label: "image-worker",
      kind: "identity",
    },
    [
      {
        id: "history-send-image",
        event: "interaction_complete",
        sourceKind: "session_history",
        timestampMs: Date.parse("2026-05-11T21:13:00.000Z"),
        data: {
          message: {
            role: "block_assistant",
            stop_reason: "tool_use",
            blocks: [{
              block_type: "tool_use",
              data: {
                id: "call-send-image",
                name: "send_message",
                args: {
                  peer_id: "peer-image-target",
                  body: [
                    { type: "text", text: "Forwarding generated result." },
                    {
                      type: "image_ref",
                      source: "blob",
                      blob_id: "sha256:generated/1",
                      media_type: "image/png",
                      alt: "generated incident badge",
                    },
                  ],
                },
              },
            }],
          },
        },
      },
    ],
    { renderInteractionStartsAsUser: true },
  );

  assert.equal(entries.length, 1);
  const block = entries[0] && "blocks" in entries[0] ? entries[0].blocks?.[0] : null;
  assert.equal(block?.type, "tool-call");
  assert.equal(block?.type === "tool-call" ? block.peerBody : "", "Forwarding generated result. generated incident badge");
});

test("mapFramesToTimelineEntries deduplicates repeated session-history kickoff prompts", () => {
  const prompt = "You are the child worker in a worker-chain test. Create a worker named grandchild-worker.";
  const entries = mapFramesToTimelineEntries(
    {
      agent_id: "child-worker",
      member_id: "child-worker",
      label: "child-worker",
      kind: "identity",
    },
    [
      {
        id: "history-kickoff-1",
        event: "user_input",
        sourceKind: "session_history",
        timestampMs: 1,
        data: { content: prompt },
      },
      {
        id: "history-kickoff-2",
        event: "user_input",
        sourceKind: "session_history",
        timestampMs: 2,
        data: { content: prompt },
      },
    ],
    { renderInteractionStartsAsUser: true },
  );

  assert.equal(entries.length, 1);
  assert.equal(entries[0] && "text" in entries[0] ? entries[0].text : "", prompt);
});

test("mapFramesToTimelineEntries renders inbound content-block run_started prompts as user work", () => {
  const entries = mapFramesToTimelineEntries(
    {
      agent_id: "scribe",
      member_id: "scribe",
      label: "Scribe",
      kind: "identity",
    },
    [
      {
        id: "evt-1",
        event: "run_started",
        timestampMs: Date.parse("2026-04-06T23:00:00.000Z"),
        data: {
          prompt: [
            { type: "text", text: "[COMMS MESSAGE from incident-command-center/commander/incident-commander]" },
            { type: "text", text: "Please describe this generated self-portrait image." },
            { type: "image", data: "base64-image-data", media_type: "image/png" },
          ],
        },
      },
      {
        id: "evt-2",
        event: "interaction_complete",
        timestampMs: Date.parse("2026-04-06T23:00:02.000Z"),
        data: { result: "The image shows a winged fox commander." },
      },
    ],
    { renderInteractionStartsAsUser: true },
  );

  assert.equal(entries.length, 2);
  assert.equal(entries[0]?.identity.role, "user");
  assert.equal(entries[0]?.variant, "rich");
  const blocks = entries[0] && "blocks" in entries[0] ? entries[0].blocks || [] : [];
  assert.equal(blocks.length, 3);
  assert.equal(
    blocks
      .filter((block) => block.type === "paragraph")
      .map((block) => block.type === "paragraph" ? block.text : "")
      .join("\n"),
    "[COMMS MESSAGE from incident-command-center/commander/incident-commander]\nPlease describe this generated self-portrait image.",
  );
  assert.equal(blocks[2]?.type, "image");
  assert.equal(blocks[2]?.type === "image" ? blocks[2].src : "", "data:image/png;base64,base64-image-data");
  assert.equal(entries[1]?.identity.id, "scribe");
});

test("mapFramesToTimelineEntries orders persisted interaction history by interaction semantics, not raw arrival order", () => {
  const entries = mapFramesToTimelineEntries(
    {
      agent_id: "incident-commander",
      member_id: "incident-commander",
      label: "Incident Commander",
      kind: "identity",
    },
    [
      {
        id: "evt-2",
        event: "text_delta",
        interactionId: "turn-1",
        timestampMs: 10,
        data: { delta: "Working on it." },
      },
      {
        id: "evt-1",
        event: "interaction_started",
        interactionId: "turn-1",
        timestampMs: 11,
        data: { content: "Run a status sweep." },
      },
      {
        id: "evt-3",
        event: "interaction_complete",
        interactionId: "turn-1",
        timestampMs: 12,
        data: { text: "Working on it." },
      },
    ],
    { renderInteractionStartsAsUser: true },
  );

  assert.equal(entries.length, 2);
  assert.equal(entries[0]?.identity.role, "user");
  assert.equal(entries[0] && "text" in entries[0] ? entries[0].text : "", "Run a status sweep.");
  assert.equal(entries[1]?.identity.role, "assistant");
  assert.equal(
    entries[1] && "blocks" in entries[1] && Array.isArray(entries[1].blocks)
      ? entries[1].blocks[0]?.type === "paragraph"
        ? entries[1].blocks[0].text
        : ""
      : "",
    "Working on it.",
  );
});

test("mapFramesToTimelineEntries keeps accepted user input before the later assistant response when frames arrive reversed", () => {
  const entries = mapFramesToTimelineEntries(
    {
      agent_id: "incident-commander",
      member_id: "incident-commander",
      label: "Incident Commander",
      kind: "identity",
    },
    [
      {
        id: "assistant-frame",
        cursor: "console:10",
        event: "interaction_complete",
        interactionId: "runtime-turn-1",
        timestampMs: Date.parse("2026-05-07T15:16:28.000Z"),
        data: { text: "OK" },
      },
      {
        id: "user-frame",
        cursor: "console:11",
        event: "user_input",
        interactionId: "console-interaction-1",
        timestampMs: Date.parse("2026-05-07T15:16:26.000Z"),
        data: { content: "Reply with exactly OK." },
      },
    ],
    { renderInteractionStartsAsUser: true },
  );

  assert.equal(entries.length, 2);
  assert.equal(entries[0]?.identity.role, "user");
  assert.equal(entries[0] && "text" in entries[0] ? entries[0].text : "", "Reply with exactly OK.");
  assert.equal(entries[1]?.identity.role, "assistant");
  assert.equal(
    entries[1] && "blocks" in entries[1] && Array.isArray(entries[1].blocks)
      ? entries[1].blocks[0]?.type === "paragraph"
        ? entries[1].blocks[0].text
        : ""
      : "",
    "OK",
  );
});

test("mapFramesToTimelineEntries keeps no-interaction peer turns after the originating user turn", () => {
  const entries = mapFramesToTimelineEntries(
    {
      agent_id: "review",
      member_id: "review:singleton",
      label: "Review Agent",
      kind: "identity",
    },
    [
      {
        id: "peer-result-started",
        cursor: "console:1141",
        event: "run_started",
        timestampMs: Date.parse("2026-05-25T10:43:26.000Z"),
        data: {
          prompt:
            "Peer message from ob3/review-worker/review-worker-vibe-forward-chat-review-fix: { result: true }",
        },
      },
      {
        id: "peer-result-complete",
        cursor: "console:3106",
        event: "interaction_complete",
        timestampMs: Date.parse("2026-05-25T10:43:49.000Z"),
        data: {
          result:
            "Tool progress: Full review result forwarded to the Vibe Forward initiative agent.",
        },
      },
      {
        id: "operator-prompt",
        cursor: "console:425",
        event: "user_input",
        interactionId: "console-interaction-review",
        timestampMs: Date.parse("2026-05-25T10:42:04.000Z"),
        data: {
          content:
            "Console chat smoke chat-review-fix: run a fresh OSIR review for initiative Vibe Forward.",
        },
      },
      {
        id: "operator-handoff",
        cursor: "console:923",
        event: "interaction_complete",
        interactionId: "console-interaction-review",
        timestampMs: Date.parse("2026-05-25T10:42:23.000Z"),
        data: {
          result:
            "Tool progress: Spawned exactly one review-worker. Worker handoff.",
        },
      },
    ],
    { renderInteractionStartsAsUser: true },
  );

  const texts = entries.map((entry) =>
    "text" in entry
      ? entry.text
      : "blocks" in entry
        ? JSON.stringify(entry.blocks)
        : "",
  );
  const promptIndex = texts.findIndex((text) => text.includes("Console chat smoke"));
  const handoffIndex = texts.findIndex((text) => text.includes("Worker handoff"));
  const peerIndex = texts.findIndex((text) => text.includes("Peer message from ob3/review-worker"));
  const finalIndex = texts.findIndex((text) => text.includes("Full review result forwarded"));

  assert(promptIndex >= 0, `missing operator prompt: ${texts.join("\n---\n")}`);
  assert(handoffIndex > promptIndex, `handoff must follow prompt: ${texts.join("\n---\n")}`);
  assert(peerIndex > handoffIndex, `peer turn must follow originating turn: ${texts.join("\n---\n")}`);
  assert(finalIndex > peerIndex, `final result must follow peer turn: ${texts.join("\n---\n")}`);
});

test("mapFramesToTimelineEntries decodes stringified delta payloads from persisted history", () => {
  const entries = mapFramesToTimelineEntries(
    {
      agent_id: "merchant-success",
      member_id: "merchant-success",
      label: "Merchant Success",
      kind: "identity",
    },
    [
      {
        id: "evt-1",
        event: "text_delta",
        timestampMs: 1,
        data: "{\"delta\":\"Enterprise merchants are experiencing significant payment failures.\",\"source_event_type\":\"text_delta\",\"type\":\"text_delta\"}",
      },
      {
        id: "evt-2",
        event: "interaction_complete",
        timestampMs: 2,
        data: { text: "Enterprise merchants are experiencing significant payment failures." },
      },
    ],
  );

  assert.equal(entries.length, 1);
  assert.equal(
    entries[0] && "blocks" in entries[0] && Array.isArray(entries[0].blocks)
      ? entries[0].blocks[0]?.type === "paragraph"
        ? entries[0].blocks[0].text
        : ""
      : "",
    "Enterprise merchants are experiencing significant payment failures.",
  );
});

test("mapFramesToTimelineEntries preserves whitespace-only text deltas instead of stringifying the payload", () => {
  const entries = mapFramesToTimelineEntries(
    {
      agent_id: "payments-sre",
      member_id: "payments-sre",
      label: "Payments SRE",
      kind: "identity",
    },
    [
      { id: "evt-1", event: "text_delta", data: { delta: "Payments-API remains degraded at" } },
      { id: "evt-2", event: "text_delta", data: { delta: " " } },
      { id: "evt-3", event: "text_delta", data: { delta: "38%" } },
      { id: "evt-4", event: "interaction_complete", data: { text: "Payments-API remains degraded at 38%" } },
    ],
  );

  assert.equal(entries.length, 1);
  assert.equal(
    entries[0] && "blocks" in entries[0] && Array.isArray(entries[0].blocks)
      ? entries[0].blocks[0]?.type === "paragraph"
        ? entries[0].blocks[0].text
        : ""
      : "",
    "Payments-API remains degraded at 38%",
  );
});

test("mergeConversationFrames deduplicates history and live copies of the same event", () => {
  const merged = mergeConversationFrames(
    [
      {
        id: "evt-1",
        event: "interaction_started",
        interactionId: "turn-1",
        data: { content: "Run a status sweep." },
      },
      {
        id: "evt-2",
        event: "text_delta",
        interactionId: "turn-1",
        data: { delta: "Working" },
      },
    ],
    [
      {
        id: "evt-2",
        event: "text_delta",
        interactionId: "turn-1",
        data: { delta: "Working" },
      },
      {
        id: "evt-3",
        event: "interaction_complete",
        interactionId: "turn-1",
        data: { text: "Working" },
      },
    ],
  );

  assert.deepEqual(merged.map((frame) => frame.id), ["evt-1", "evt-2", "evt-3"]);
});

test("buildActivityRailViewState hides text deltas and internal config churn", () => {
  const view = buildActivityRailViewState({
    agents: [
      {
        agent_id: "incident-commander",
        member_id: "incident-commander",
        identity: "incident-commander",
        label: "Incident Commander",
        kind: "identity",
      },
    ],
    eventFrames: [
      {
        id: "evt-1",
        event: "text_delta",
        identity: "incident-commander",
        data: { delta: "hello" },
      },
      {
        id: "evt-2",
        event: "tool_config_changed",
        identity: "incident-commander",
        data: { target: "tool_scope" },
      },
      {
        id: "evt-2a",
        event: "snapshot_started",
        identity: "_system",
        data: { type: "snapshot_started", after: "console:1" },
      },
      {
        id: "evt-2b",
        event: "snapshot_complete",
        identity: "_system",
        data: { type: "snapshot_complete", cursor: "console:2" },
      },
      {
        id: "evt-3",
        event: "tool_call_requested",
        identity: "incident-commander",
        data: { id: "call-1", name: "send" },
      },
      {
        id: "evt-4",
        event: "tool_execution_started",
        identity: "incident-commander",
        data: { id: "call-1", name: "send" },
      },
      {
        id: "evt-5",
        event: "interaction_complete",
        identity: "incident-commander",
        data: { text: "done" },
      },
      {
        id: "evt-6",
        event: "interaction_complete",
        identity: "incident-commander",
        sourceKind: "session_history",
        data: {
          message: {
            role: "block_assistant",
            blocks: [
              {
                block_type: "reasoning",
                data: { text: "**Thinking**\n\nDo not show this in the rail." },
              },
            ],
          },
          text: "**Thinking**\n\nDo not show this in the rail.",
        },
      },
    ],
  });

  assert.deepEqual(view.panels[0]?.items.map((item) => item.id), ["event:evt-5"]);
});

test("buildQuickPromptSuggestions projects stock prompt labels into runnable suggestions", () => {
  const suggestions = buildQuickPromptSuggestions({
    agent_id: "incident-commander",
    member_id: "incident-commander",
    label: "Incident Commander",
    kind: "identity",
    labels: {
      console_prompt_1_label: "Status sweep",
      console_prompt_1_value: "Run a status sweep.",
      console_prompt_2_label: "Merchant impact",
      console_prompt_2_value: "Summarize merchant impact.",
    },
  });

  assert.deepEqual(
    suggestions.map((suggestion) => ({ label: suggestion.label, value: suggestion.value })),
    [
      { label: "Status sweep", value: "Run a status sweep." },
      { label: "Merchant impact", value: "Summarize merchant impact." },
    ],
  );
});

test("memory timeline events render as clean meta entries, never raw JSON", () => {
  const frames = [
    {
      id: "m1",
      event: "memory.dream.completed",
      timestampMs: 1000,
      data: { realm: "default", run_id: "run-9", ops_committed: 3, detail: "distilled prefs" },
    },
    {
      id: "m2",
      event: "memory.write.quarantined",
      timestampMs: 2000,
      data: { realm: "default", author: "agent", reason: "low trust" },
    },
    {
      id: "m3",
      event: "memory.taint.transition",
      timestampMs: 3000,
      data: { session_key: "sess-1", kind: "tainted", source: "reset" },
    },
    {
      id: "m4",
      event: "memory.future.event",
      timestampMs: 4000,
      data: { reason: "something new" },
    },
  ];

  const entries = mapFramesToTimelineEntries(null, frames);
  assert.equal(entries.length, 4);
  for (const entry of entries) {
    assert.equal(entry.variant, "meta");
    const text = entry.kind === "message" && "text" in entry ? entry.text || "" : "";
    assert.ok(text.length > 0, "memory entry must have text");
    assert.ok(!text.includes("{"), `memory entry must not leak JSON: ${text}`);
    assert.ok(!text.includes("}"), `memory entry must not leak JSON: ${text}`);
  }

  const dream = entries[0];
  assert.ok(
    dream.kind === "message" && "text" in dream && dream.text?.includes("3 ops committed"),
    "dream.completed summarizes committed ops",
  );

  const unknown = entries[3];
  assert.ok(
    unknown.kind === "message" && "text" in unknown && unknown.text?.startsWith("Memory future event"),
    "unknown memory.* subtypes humanize the event name",
  );
});

test("describeMemoryTimelineEvent produces clean lines for every documented subtype", () => {
  const cases: Array<[string, Record<string, unknown>, RegExp]> = [
    ["memory.dream.started", { run_id: "run-1" }, /Dream started/],
    ["memory.dream.skipped", { reason: "no changes" }, /Dream skipped — no changes/],
    ["memory.record.promoted", { scope_kind: "identity", scope_key: "luka", gated: true }, /promoted to identity:luka \(gated\)/],
    ["memory.quarantine.verdict", { verdict: "reject", rationale: "duplicate" }, /Quarantine verdict: reject — duplicate/],
    ["memory.quarantine.release_blocked", { record_id: "rec-9", verdict: "release", class: "private-key" }, /Quarantine release blocked for rec-9 — matches secret pattern private-key/],
    ["memory.conflict.signal", { entity: "user", topic: "tz", reason: "mismatch" }, /Conflict signal on user \/ tz — mismatch/],
    ["memory.budget.denied", { stage: "harvest", reason: "over cap" }, /Budget denied at harvest — over cap/],
    ["memory.promotion.pending_gate", { scope_kind: "mob", scope_key: "main" }, /awaiting gate for mob:main/],
    ["memory.harvest.completed", { promoted: 2, tombstoned: 1 }, /Harvest completed — 2 promoted, 1 tombstoned/],
    ["memory.distill.timed_out", { cause: "slow" }, /Distill timed out — slow/],
    ["memory.hygiene.applied", { cause: "reset", ops: 4 }, /Hygiene applied — 4 ops/],
    ["memory.hygiene.blocked", { reason: "revision drift" }, /Hygiene blocked — revision drift/],
  ];
  for (const [event, data, expected] of cases) {
    const line = describeMemoryTimelineEvent(event, data);
    assert.match(line, expected, `${event}: ${line}`);
    assert.ok(!line.includes("{") && !line.includes("}"), `${event} leaked JSON: ${line}`);
  }
});

test("describeMemoryTimelineEvent renders exact copy for quarantine release_blocked", () => {
  assert.equal(
    describeMemoryTimelineEvent("memory.quarantine.release_blocked", {
      realm: "main",
      record_id: "rec-9",
      verdict: "release",
      class: "private-key",
    }),
    "Quarantine release blocked for rec-9 — matches secret pattern private-key",
  );
  assert.equal(
    describeMemoryTimelineEvent("memory.quarantine.release_blocked", {
      realm: "main",
      record_id: "rec-12",
      verdict: "promote_pending_gate",
      class: "credential-assignment",
    }),
    "Quarantine promotion blocked for rec-12 — matches secret pattern credential-assignment",
  );
  // Degraded payload still yields a clean line, never the humanized fallback.
  assert.equal(
    describeMemoryTimelineEvent("memory.quarantine.release_blocked", {}),
    "Quarantine release blocked",
  );
});

test("describeMemoryTimelineEvent stays in sync with the console-core mirror", () => {
  // Every payload-dependent branch of the formatter gets its own variant so a
  // divergence in EITHER copy trips the comparison, not just the happy path.
  const cases: Record<string, Array<Record<string, unknown>>> = {
    "memory.dream.started": [{ run_id: "run-1" }, {}],
    "memory.dream.completed": [
      { run_id: "run-1", ops_committed: 3, detail: "2 promoted" },
      { run_id: "run-1", ops_committed: 1 },
      { detail: "nothing to do" },
      {},
    ],
    "memory.dream.skipped": [{ reason: "no changes" }, {}],
    "memory.record.promoted": [
      { record_id: "rec-1", scope_kind: "identity", scope_key: "luka", gated: true },
      { record_id: "rec-1", scope_kind: "identity", gated: false },
      { record_id: "rec-1", scope_key: "main" },
      {},
    ],
    "memory.quarantine.verdict": [
      { record_id: "rec-2", verdict: "reject", rationale: "duplicate" },
      { record_id: "rec-2", verdict: "release" },
      {},
    ],
    "memory.quarantine.release_blocked": [
      { record_id: "rec-9", verdict: "release", class: "private-key" },
      { record_id: "rec-12", verdict: "promote_pending_gate", class: "credential-assignment" },
      { record_id: "rec-13", verdict: "future_verdict", class: "github-token" },
      { verdict: "release", class: "aws-access-key-id" },
      { record_id: "rec-14", verdict: "release" },
      {},
    ],
    "memory.conflict.signal": [
      { entity: "user", topic: "tz", reason: "mismatch" },
      { entity: "user" },
      { topic: "tz" },
      {},
    ],
    "memory.write.quarantined": [
      { author: "distiller", reason: "tainted session" },
      { reason: "tainted session" },
      { author: "distiller" },
      {},
    ],
    "memory.taint.transition": [
      { session_key: "s-1", kind: "tainted", source: "web_fetch" },
      { session_key: "s-1", kind: "reset_boundary" },
      { kind: "rotated_clean", source: "steward" },
      { kind: "future_kind" },
      {},
    ],
    "memory.budget.denied": [
      { stage: "harvest", reason: "over cap" },
      { stage: "harvest" },
      { reason: "over cap" },
      {},
    ],
    "memory.promotion.pending_gate": [
      { pending_id: "p-1", record_id: "rec-3", scope_kind: "mob", scope_key: "main" },
      { scope_kind: "mob" },
      {},
    ],
    "memory.harvest.completed": [
      { identity: "scout", promoted: 2, tombstoned: 1 },
      { promoted: 0 },
      { tombstoned: 3 },
      {},
    ],
    "memory.distill.timed_out": [
      { session_key: "s-2", cause: "slow" },
      { session_key: "s-2" },
      { cause: "slow" },
      {},
    ],
    "memory.hygiene.proposed": [
      { session_key: "s-3", cause: "reset", ops: 2 },
      { cause: "reset" },
      {},
    ],
    "memory.hygiene.applied": [
      { session_key: "s-3", cause: "reset", ops: 4 },
      { ops: 1 },
    ],
    "memory.hygiene.blocked": [
      { session_key: "s-3", cause: "reset", reason: "revision drift" },
      { cause: "reset", ops: 2 },
    ],
    "memory.hygiene.skipped": [
      { session_key: "s-3", cause: "reset", reason: "mid-turn" },
      {},
    ],
    // Unknown-subtype fallback: each best-effort reason source in turn.
    "memory.future.event": [
      { reason: "unknown subtype" },
      { detail: "detail fallback" },
      { cause: "cause fallback" },
      { verdict: "verdict fallback" },
      {},
    ],
    // Non-memory-prefixed event exercises the humanizer's other branch.
    "console.mystery": [{}],
  };
  for (const [event, variants] of Object.entries(cases)) {
    for (const data of variants) {
      assert.equal(
        describeMemoryTimelineEvent(event, data),
        describeMemoryTimelineEventCore(event, data),
        `copies diverge for ${event} with ${JSON.stringify(data)}`,
      );
    }
  }
});

// ── WorkGraph inline card aggregation ───────────────────────────────────────

const WORKGRAPH_AGENT = {
  agent_id: "planner",
  member_id: "planner",
  label: "Planner",
  kind: "identity",
};

function workGraphItem(args: {
  id: string;
  title?: string;
  status?: string;
  revision?: number;
  priority?: string;
  description?: string;
  owner?: { kind: string; id: string; display_name?: string };
  createdAt?: string;
}): Record<string, unknown> {
  return {
    id: args.id,
    realm_id: "realm-1",
    namespace: "default",
    title: args.title || args.id,
    ...(args.description ? { description: args.description } : {}),
    status: args.status || "open",
    priority: args.priority || "medium",
    revision: args.revision ?? 1,
    ...(args.owner
      ? {
          owner: {
            key: { kind: args.owner.kind, id: args.owner.id },
            ...(args.owner.display_name ? { display_name: args.owner.display_name } : {}),
          },
        }
      : {}),
    created_at: args.createdAt || "2026-07-08T09:00:00Z",
    updated_at: "2026-07-08T09:00:00Z",
  };
}

function workGraphToolFrames(args: {
  idPrefix: string;
  name: string;
  callArgs: Record<string, unknown>;
  result: Record<string, unknown>;
  interactionId?: string;
  timestampMs?: number;
}) {
  const interactionId = args.interactionId || "turn-wg";
  const timestampMs = args.timestampMs ?? 1_779_405_464_000;
  return [
    {
      id: `${args.idPrefix}-call`,
      event: "tool_call_requested",
      interactionId,
      timestampMs,
      data: { id: `${args.idPrefix}-tc`, name: args.name, args: args.callArgs },
    },
    {
      id: `${args.idPrefix}-done`,
      event: "tool_execution_completed",
      interactionId,
      timestampMs: timestampMs + 200,
      data: { id: `${args.idPrefix}-tc`, name: args.name, result: JSON.stringify(args.result) },
    },
  ];
}

test("workgraph create→claim→close folds into one evolving card with correct progress and revisions", () => {
  const frames = [
    ...workGraphToolFrames({
      idPrefix: "wg-1",
      name: "workgraph_create",
      callArgs: { title: "Ship the fix" },
      result: { item: workGraphItem({ id: "item-1", title: "Ship the fix", status: "open", revision: 1 }) },
    }),
    ...workGraphToolFrames({
      idPrefix: "wg-2",
      name: "workgraph_claim",
      callArgs: { id: "item-1", expected_revision: 1, owner: { kind: "agent", id: "planner" } },
      result: {
        item: workGraphItem({
          id: "item-1",
          title: "Ship the fix",
          status: "in_progress",
          revision: 2,
          owner: { kind: "agent", id: "planner", display_name: "Planner" },
        }),
      },
      timestampMs: 1_779_405_465_000,
    }),
    ...workGraphToolFrames({
      idPrefix: "wg-3",
      name: "workgraph_close",
      callArgs: { id: "item-1", expected_revision: 2 },
      result: {
        item: workGraphItem({
          id: "item-1",
          title: "Ship the fix",
          status: "completed",
          revision: 3,
          owner: { kind: "agent", id: "planner", display_name: "Planner" },
        }),
      },
      timestampMs: 1_779_405_466_000,
    }),
  ];

  const entries = mapFramesToTimelineEntries(WORKGRAPH_AGENT, frames);
  const cards = entries.filter((entry) => entry.kind === "workgraph");
  assert.equal(cards.length, 1);
  const card = cards[0];
  assert.equal(card.kind === "workgraph" && card.id, "workgraph:interaction:turn-wg");
  if (card.kind !== "workgraph") return;
  assert.equal(card.title, "Ship the fix");
  assert.equal(card.status, "completed");
  assert.deepEqual(card.progress, { completed: 1, total: 1 });
  assert.equal(card.items.length, 1);
  assert.equal(card.items[0].revision, 3);
  assert.equal(card.items[0].status, "completed");
  assert.equal(card.items[0].ownerLabel, "Planner");

  // No generic tool rows for workgraph calls.
  const toolBlockNames = entries.flatMap((entry) => (
    entry.kind === "message" && Array.isArray(entry.blocks)
      ? entry.blocks.filter((block) => block.type === "tool-call").map((block) => block.name)
      : []
  ));
  assert.deepEqual(toolBlockNames.filter((name) => String(name).startsWith("workgraph_")), []);
});

test("workgraph card id and identity stay stable across live passes so updates land in place", () => {
  const createFrames = workGraphToolFrames({
    idPrefix: "wg-1",
    name: "workgraph_create",
    callArgs: { title: "Ship the fix" },
    result: { item: workGraphItem({ id: "item-1", title: "Ship the fix", status: "open", revision: 1 }) },
  });
  const claimFrames = workGraphToolFrames({
    idPrefix: "wg-2",
    name: "workgraph_claim",
    callArgs: { id: "item-1", expected_revision: 1 },
    result: { item: workGraphItem({ id: "item-1", title: "Ship the fix", status: "in_progress", revision: 2 }) },
    timestampMs: 1_779_405_465_000,
  });

  const firstPass = mapFramesToTimelineEntries(WORKGRAPH_AGENT, createFrames)
    .filter((entry) => entry.kind === "workgraph");
  const secondPass = mapFramesToTimelineEntries(WORKGRAPH_AGENT, [...createFrames, ...claimFrames])
    .filter((entry) => entry.kind === "workgraph");

  assert.equal(firstPass.length, 1);
  assert.equal(secondPass.length, 1);
  assert.equal(firstPass[0].id, secondPass[0].id);
  assert.equal(firstPass[0].kind === "workgraph" ? firstPass[0].status : "", "active");
  assert.equal(secondPass[0].kind === "workgraph" ? secondPass[0].items[0].revision : 0, 2);
  assert.equal(secondPass[0].identity.role, "assistant");
});

test("workgraph snapshot results hydrate the goal tree, parent depths, and attention bindings", () => {
  const frames = workGraphToolFrames({
    idPrefix: "wg-snap",
    name: "workgraph_snapshot",
    callArgs: {},
    result: {
      snapshot: {
        realm_id: "realm-1",
        all_namespaces: false,
        captured_at: "2026-07-08T09:10:00Z",
        items: [
          workGraphItem({
            id: "goal-1",
            title: "Release 0.7.30",
            description: "Ship WorkGraph end to end",
            status: "in_progress",
            revision: 4,
            createdAt: "2026-07-08T08:00:00Z",
          }),
          workGraphItem({
            id: "child-1",
            title: "Console card",
            status: "completed",
            revision: 2,
            createdAt: "2026-07-08T08:10:00Z",
          }),
          workGraphItem({
            id: "child-2",
            title: "SDK parity",
            status: "open",
            revision: 1,
            priority: "high",
            createdAt: "2026-07-08T08:20:00Z",
          }),
        ],
        edges: [
          { realm_id: "realm-1", namespace: "default", kind: "parent", from_id: "child-1", to_id: "goal-1", created_at: "2026-07-08T08:10:00Z" },
          { realm_id: "realm-1", namespace: "default", kind: "parent", from_id: "child-2", to_id: "goal-1", created_at: "2026-07-08T08:20:00Z" },
        ],
        attention: [
          {
            binding_id: "attention-1",
            work_ref: { realm_id: "realm-1", namespace: "default", item_id: "goal-1" },
            target: { kind: "session", session_id: "sess-42" },
            mode: "pursue",
            status: { state: "active" },
            machine_state: { lifecycle_phase: "active", revision: 7 },
            created_at: "2026-07-08T08:00:00Z",
            updated_at: "2026-07-08T09:00:00Z",
          },
        ],
        ready_item_ids: ["child-2"],
      },
    },
  });

  const entries = mapFramesToTimelineEntries(WORKGRAPH_AGENT, frames);
  const cards = entries.filter((entry) => entry.kind === "workgraph");
  assert.equal(cards.length, 1);
  const card = cards[0];
  if (card.kind !== "workgraph") return;
  assert.equal(card.id, "workgraph:goal-1");
  assert.equal(card.rootId, "goal-1");
  assert.equal(card.title, "Release 0.7.30");
  assert.equal(card.objective, "Ship WorkGraph end to end");
  assert.equal(card.status, "active");
  assert.deepEqual(card.progress, { completed: 1, total: 3 });
  assert.deepEqual(
    card.items.map((item) => [item.itemId, item.depth]),
    [["goal-1", 0], ["child-1", 1], ["child-2", 1]],
  );
  assert.equal(card.items[2].priority, "high");
  assert.equal(card.attention.length, 1);
  assert.equal(card.attention[0].bindingId, "attention-1");
  assert.equal(card.attention[0].mode, "pursue");
  assert.equal(card.attention[0].statusLabel, "active");
  assert.equal(card.attention[0].revision, 7);
  assert.equal(card.attention[0].targetLabel, "sess-42");
});

test("workgraph unrooted items in one interaction group into a single catch-all card", () => {
  const frames = [
    ...workGraphToolFrames({
      idPrefix: "wg-a",
      name: "workgraph_create",
      callArgs: { title: "Item A" },
      result: { item: workGraphItem({ id: "item-a", title: "Item A", status: "open", revision: 1 }) },
    }),
    ...workGraphToolFrames({
      idPrefix: "wg-b",
      name: "workgraph_create",
      callArgs: { title: "Item B" },
      result: { item: workGraphItem({ id: "item-b", title: "Item B", status: "blocked", revision: 1 }) },
      timestampMs: 1_779_405_465_000,
    }),
  ];

  const entries = mapFramesToTimelineEntries(WORKGRAPH_AGENT, frames);
  const cards = entries.filter((entry) => entry.kind === "workgraph");
  assert.equal(cards.length, 1);
  const card = cards[0];
  if (card.kind !== "workgraph") return;
  assert.equal(card.title, "Work items");
  assert.equal(card.items.length, 2);
  assert.equal(card.status, "active");
});

test("workgraph failed tool results do not poison the card fold", () => {
  const frames = [
    ...workGraphToolFrames({
      idPrefix: "wg-ok",
      name: "workgraph_create",
      callArgs: { title: "Item A" },
      result: { item: workGraphItem({ id: "item-a", title: "Item A", status: "open", revision: 1 }) },
    }),
    {
      id: "wg-err-call",
      event: "tool_call_requested",
      interactionId: "turn-wg",
      timestampMs: 1_779_405_465_000,
      data: { id: "wg-err-tc", name: "workgraph_claim", args: { id: "item-a", expected_revision: 9 } },
    },
    {
      id: "wg-err-done",
      event: "tool_execution_completed",
      interactionId: "turn-wg",
      timestampMs: 1_779_405_465_200,
      data: { id: "wg-err-tc", name: "workgraph_claim", is_error: true, result: "revision conflict" },
    },
  ];

  const entries = mapFramesToTimelineEntries(WORKGRAPH_AGENT, frames);
  const cards = entries.filter((entry) => entry.kind === "workgraph");
  assert.equal(cards.length, 1);
  const card = cards[0];
  if (card.kind !== "workgraph") return;
  assert.equal(card.items[0].status, "open");
  assert.equal(card.items[0].revision, 1);
});
