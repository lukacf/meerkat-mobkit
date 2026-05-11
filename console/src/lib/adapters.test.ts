import assert from "node:assert/strict";
import test from "node:test";

import {
  buildActivityRailViewState,
  buildQuickPromptSuggestions,
  buildRoutingSectionView,
  buildSidebarViewState,
  inferResponsePhaseFromFrames,
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

test("inferResponsePhaseFromFrames clears working state on terminal text and end-turn frames", () => {
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

test("mapSessionHistoryToTimelineEntries treats old comms transport text as user-authored text", () => {
  const entries = mapSessionHistoryToTimelineEntries(
    {
      session_id: "session-1",
      message_count: 7,
      offset: 0,
      has_more: false,
      messages: [
        {
          role: "system",
          content: "## Incident Comms Protocol\nIgnore lifecycle chatter.",
        },
        {
          role: "user",
          content: "You have been spawned as 'scribe' (role: scribe) in mob 'incident-command-center'.",
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
          role: "user",
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

  assert.equal(entries.length, 2);
  assert.equal(entries[0]?.identity.label, "You");
  assert.equal(
    "text" in (entries[0] || {}) ? entries[0]?.text : "",
    "[COMMS MESSAGE from incident-command-center/incident_commander/incident-commander] Please summarize the timeline.",
  );
  assert.equal(entries[1]?.identity.label, "Scribe");
  assert.equal(
    entries[1] && "blocks" in entries[1] && Array.isArray(entries[1].blocks)
      ? entries[1].blocks[0]?.type === "paragraph"
        ? entries[1].blocks[0].text
        : ""
      : "",
    "Scribe is preparing a summary.",
  );
});

test("mapSessionHistoryToTimelineEntries renders typed system-notice peer requests as comms metadata", () => {
  const entries = mapSessionHistoryToTimelineEntries(
    {
      session_id: "session-1",
      message_count: 1,
      offset: 0,
      has_more: false,
      messages: [
        {
          role: "system_notice",
          kind: "comms",
          body: "Peer request: ping",
          blocks: [{
            type: "comms",
            kind: "request",
            direction: "incoming",
            peer: { id: "11111111-2222-3333-4444-555555555555", display_name: "incident-worker-full-1" },
            intent: "ping",
            request_id: "req-1",
            payload: { body: "check readiness" },
            content: [{ type: "text", text: "check readiness" }],
          }],
        },
      ],
    },
    {
      agent_id: "incident-commander",
      member_id: "incident-commander",
      label: "Incident Commander",
      kind: "identity",
    },
  );

  assert.equal(entries.length, 1);
  assert.equal(entries[0]?.identity.id, "comms");
  assert.equal(entries[0]?.variant, "rich");
  const firstBlock = entries[0] && "blocks" in entries[0] ? entries[0].blocks?.[0] : null;
  assert.equal(firstBlock?.type, "tool-call");
  assert.equal(firstBlock?.peerTarget, "incident-worker-full-1");
  assert.equal(firstBlock?.peerIncoming, true);
});

test("mapSessionHistoryToTimelineEntries treats old bootstrap-looking user text as user-authored", () => {
  const entries = mapSessionHistoryToTimelineEntries(
    {
      session_id: "session-1",
      message_count: 5,
      offset: 0,
      has_more: false,
      messages: [
        { role: "system", content: "## Incident Comms Protocol\nIgnore lifecycle chatter." },
        { role: "user", content: "You have been spawned as 'incident-commander' (role: commander) in mob 'incident-command-center'." },
        { role: "system", content: "[SYSTEM NOTICE][TOOL_SCOPE] Tool configuration changed at turn boundary" },
        { role: "tool_results", results: [{ content: "{\"status\":\"sent\"}" }] },
        {
          role: "block_assistant",
          blocks: [{ block_type: "text", data: { text: "Real assistant reply." } }],
          stop_reason: "end_turn",
        },
      ],
    },
    {
      agent_id: "incident-commander",
      member_id: "incident-commander",
      label: "Incident Commander",
      kind: "identity",
    },
  );

  assert.deepEqual(entries.map((entry) => entry.identity.label), ["You", "Incident Commander"]);
  assert.equal(
    "text" in (entries[0] || {}) ? entries[0]?.text : "",
    "You have been spawned as 'incident-commander' (role: commander) in mob 'incident-command-center'.",
  );
});

test("mapSessionHistoryToTimelineEntries displays old peer request prefixes as user text", () => {
  const entries = mapSessionHistoryToTimelineEntries(
    {
      session_id: "session-1",
      message_count: 5,
      offset: 0,
      has_more: false,
      messages: [
        { role: "assistant", content: "Current established facts: Payments API is degraded." },
        { role: "assistant", content: "I have acknowledged the addition of the following peers: api-investigator, incident-commander." },
        { role: "user", content: "[COMMS REQUEST from incident-command-center/incident_commander/incident-commander]\nIntent: request_summary\nBody: Summarize the incident timeline." },
        {
          role: "block_assistant",
          blocks: [{ block_type: "text", data: { text: "Summary prepared and sent back to commander." } }],
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

  assert.equal(entries.length, 2);
  assert.equal(entries[0]?.identity.id, "user");
  assert.equal(
    "text" in (entries[0] || {}) ? entries[0]?.text : "",
    "[COMMS REQUEST from incident-command-center/incident_commander/incident-commander]\nIntent: request_summary\nBody: Summarize the incident timeline.",
  );
  assert.equal(entries[1]?.identity.label, "Scribe");
});

test("mapSessionHistoryToTimelineEntries keeps checksum_token old prefixes as user text", () => {
  const entries = mapSessionHistoryToTimelineEntries(
    {
      session_id: "session-1",
      message_count: 2,
      offset: 0,
      has_more: false,
      messages: [
        {
          role: "user",
          content: "[COMMS REQUEST from incident-command-center/incident_commander/incident-commander]\nIntent: checksum_token\nParams: {\n  \"subject\": \"Reply exactly: peer smoke ok\"\n}",
        },
        {
          role: "block_assistant",
          blocks: [{ block_type: "text", data: { text: "Sent the exact reply back." } }],
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

  assert.equal(entries[0]?.identity.id, "user");
  assert.equal(
    "text" in (entries[0] || {}) ? entries[0]?.text : "",
    "[COMMS REQUEST from incident-command-center/incident_commander/incident-commander]\nIntent: checksum_token\nParams: {\n  \"subject\": \"Reply exactly: peer smoke ok\"\n}",
  );
});

test("mapSessionHistoryToTimelineEntries keeps old rpc transport prefix as user text", () => {
  const entries = mapSessionHistoryToTimelineEntries(
    {
      session_id: "session-1",
      message_count: 1,
      offset: 0,
      has_more: false,
      messages: [
        { role: "user", content: "[EVENT via rpc] Ask scribe for a concise update and tell me the answer." },
      ],
    },
    {
      agent_id: "incident-commander",
      member_id: "incident-commander",
      label: "Incident Commander",
      kind: "identity",
    },
  );

  assert.equal(entries.length, 1);
  assert.equal(entries[0]?.identity.label, "You");
  assert.equal(entries[0]?.variant, "plain");
  assert.equal("text" in (entries[0] || {}) ? entries[0]?.text : "", "[EVENT via rpc] Ask scribe for a concise update and tell me the answer.");
});

test("mapSessionHistoryToTimelineEntries does not recover semantics from mixed old comms blobs", () => {
  const entries = mapSessionHistoryToTimelineEntries(
    {
      session_id: "session-1",
      message_count: 1,
      offset: 0,
      has_more: false,
      messages: [
        {
          role: "user",
          content: "[COMMS RESPONSE from incident-command-center/scribe/scribe]\nStatus: completed\n[EVENT via rpc] Ask scribe for a concise update and tell me the answer.\n[COMMS RESPONSE from incident-command-center/merchant_comms/merchant-comms]\nStatus: completed",
        },
      ],
    },
    {
      agent_id: "incident-commander",
      member_id: "incident-commander",
      label: "Incident Commander",
      kind: "identity",
    },
  );

  assert.equal(entries.length, 1);
  assert.equal(entries[0]?.identity.label, "You");
  assert.equal(
    "text" in (entries[0] || {}) ? entries[0]?.text : "",
    "[COMMS RESPONSE from incident-command-center/scribe/scribe]\nStatus: completed\n[EVENT via rpc] Ask scribe for a concise update and tell me the answer.\n[COMMS RESPONSE from incident-command-center/merchant_comms/merchant-comms]\nStatus: completed",
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

test("mapFramesToTimelineEntries does not parse inbound comms requests from run_started prompts", () => {
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

  assert.equal(entries.length, 1);
  assert.equal(entries[0]?.identity.id, "scribe");
});

test("mapFramesToTimelineEntries does not parse inbound one-line peer messages from run_started prompts", () => {
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

  assert.equal(entries.length, 1);
  assert.equal(entries[0]?.identity.id, "scribe");
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

test("mapFramesToTimelineEntries does not parse inbound peer messages from content-block run_started prompts", () => {
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

  assert.equal(entries.length, 1);
  assert.equal(entries[0]?.identity.id, "scribe");
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
