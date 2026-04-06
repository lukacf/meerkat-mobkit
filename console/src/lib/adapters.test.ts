import assert from "node:assert/strict";
import test from "node:test";

import {
  buildActivityRailViewState,
  buildQuickPromptSuggestions,
  buildRoutingSectionView,
  buildSidebarViewState,
  mapSessionHistoryToTimelineEntries,
  mapFramesToTimelineEntries,
  mergeConversationFrames,
  sortConversationTimelineEntries,
} from "./adapters";

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
        profile: "console",
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

test("mapSessionHistoryToTimelineEntries preserves real session ordering and inbound comms notices", () => {
  const entries = mapSessionHistoryToTimelineEntries(
    {
      session_id: "session-1",
      message_count: 5,
      offset: 0,
      has_more: false,
      messages: [
        {
          role: "system",
          content: "You are the incident commander.",
        },
        {
          role: "user",
          content: "Talk to scribe.",
        },
        {
          role: "user",
          content: "[SYSTEM NOTICE][TOOL_SCOPE] Tool configuration changed at turn boundary",
        },
        {
          role: "system",
          content: "[COMMS MESSAGE from incident-command-center/incident_commander/incident-commander] Please summarize the timeline.",
        },
        {
          role: "block_assistant",
          blocks: [
            { block_type: "text", data: { text: "Scribe is preparing a summary." } },
          ],
          stop_reason: "end_turn",
        },
      ],
    },
    {
      agent_id: "scribe",
      member_id: "scribe",
      label: "Scribe",
      kind: "identity",
    },
  );

  assert.equal(entries.length, 3);
  assert.equal(entries[0]?.identity.label, "You");
  assert.equal(entries[1]?.identity.label, "System");
  assert.equal(entries[2]?.identity.label, "Scribe");
  assert.equal(
    entries[2] && "blocks" in entries[2] && Array.isArray(entries[2].blocks)
      ? entries[2].blocks[0]?.type === "paragraph"
        ? entries[2].blocks[0].text
        : ""
      : "",
    "Scribe is preparing a summary.",
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
