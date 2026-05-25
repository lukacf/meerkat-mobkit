import assert from "node:assert/strict";
import test from "node:test";

import {
  appendOptimisticConversationEntry,
  buildActivityRailViewState,
  buildQuickPromptSuggestions,
  buildRoutingSectionView,
  buildSidebarViewState,
  inferResponsePhaseFromFrames,
  mapFramesToTimelineEntries,
  mergeConversationFrames,
  optimisticUserMessageForPanel,
  resolvePanelResponsePhase,
  sortConversationTimelineEntries,
} from "./adapters";

function typedCommsNotice(args: {
  peer: string;
  body: string;
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
      peer: { id: args.peer, display_name: args.peer },
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
      { id: "evt-1", event: "text_delta", data: { delta: "Done." } },
      { id: "evt-2", event: "turn_completed", data: { stop_reason: "end_turn" } },
    ]),
    null,
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
  // instant no tool is running. Without this, a stream that ends on
  // tool_execution_completed (e.g., agent finishes by saving a result and
  // no subsequent run_completed event fires) leaves the indicator stuck
  // at "...working" indefinitely.
  assert.equal(
    inferResponsePhaseFromFrames([
      { id: "evt-1", event: "text_delta", data: { delta: "Done." } },
      { id: "evt-2", event: "text_complete", data: { content: "Done." } },
      { id: "evt-3", event: "tool_call_requested", data: { name: "save_investigation_result" } },
      { id: "evt-4", event: "tool_execution_started", data: {} },
      { id: "evt-5", event: "tool_execution_completed", data: {} },
    ]),
    null,
  );

  assert.equal(
    inferResponsePhaseFromFrames([
      { id: "evt-1", event: "tool_call_requested", data: { name: "save_investigation_result" } },
      { id: "evt-2", event: "tool_result_received", data: {} },
    ]),
    null,
  );

  // A new tool call after a completed one should re-arm the indicator.
  assert.equal(
    inferResponsePhaseFromFrames([
      { id: "evt-1", event: "tool_execution_completed", data: {} },
      { id: "evt-2", event: "tool_call_requested", data: { name: "next_tool" } },
    ]),
    "tool-executing",
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
    "tool-executing",
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

test("mapFramesToTimelineEntries renders session-history interaction completions from text blocks only", () => {
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
                block_type: "reasoning",
                data: { text: "**Considering event response**\n\nI should not be rendered as an answer." },
              },
              {
                block_type: "text",
                data: {
                  text: "Ready as the incident investigation worker and standing by for follow-up tasks.",
                },
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
  const toolBlock = entries[0] && "blocks" in entries[0] ? entries[0].blocks?.[0] : null;
  assert.equal(toolBlock?.type, "tool-call");
  assert.equal(toolBlock?.name, "peers");
  assert.equal(entries[1]?.identity.id, "incident-worker-1");
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
                  source: "blob",
                  blob_id: "sha256:badge/1",
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
  assert.equal(blocks.length, 2);
  assert.equal(blocks[0]?.type, "tool-call");
  assert.equal(blocks[0]?.type === "tool-call" ? blocks[0].peerIncoming : false, true);
  assert.equal(blocks[0]?.type === "tool-call" ? blocks[0].peerTarget : "", "scribe");
  assert.equal(blocks[0]?.type === "tool-call" ? blocks[0].peerBody : "", "Generated incident badge forwarded.");
  assert.equal(blocks[1]?.type, "image");
  assert.equal(
    blocks[1]?.type === "image" ? blocks[1].src : "",
    "http://127.0.0.1:7000/blobs/sha256%3Abadge%2F1",
  );
  assert.equal(blocks[1]?.type === "image" ? blocks[1].alt : "", "incident badge");
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
  assert.equal(
    entries[0] && "text" in entries[0] ? entries[0].text : "",
    "[COMMS MESSAGE from incident-command-center/commander/incident-commander]\nPlease describe this generated self-portrait image.",
  );
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
