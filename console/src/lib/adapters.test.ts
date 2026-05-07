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

test("mapSessionHistoryToTimelineEntries preserves session ordering while turning comms transport into meta messages", () => {
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

  assert.equal(entries.length, 3);
  assert.equal(entries[0]?.identity.label, "You");
  assert.equal(entries[1]?.identity.label, "System");
  assert.equal("text" in (entries[1] || {}) ? entries[1]?.text : "", "Peer message: Please summarize the timeline.");
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

test("mapSessionHistoryToTimelineEntries drops bootstrap scaffolding and tool-scope chatter", () => {
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

  assert.deepEqual(entries.map((entry) => entry.identity.label), ["Incident Commander"]);
});

test("mapSessionHistoryToTimelineEntries anchors receiver panes on recent peer activity instead of bootstrap chatter", () => {
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
  assert.equal(entries[0]?.identity.label, "System");
  assert.equal("text" in (entries[0] || {}) ? entries[0]?.text : "", "Peer request: request_summary");
  assert.equal(entries[1]?.identity.label, "Scribe");
});

test("mapSessionHistoryToTimelineEntries strips rpc transport prefix from operator prompts", () => {
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
  assert.equal("text" in (entries[0] || {}) ? entries[0]?.text : "", "Ask scribe for a concise update and tell me the answer.");
});

test("mapSessionHistoryToTimelineEntries extracts embedded rpc prompts from mixed comms blobs", () => {
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

  assert.equal(entries.length, 2);
  assert.equal(entries[0]?.identity.label, "You");
  assert.equal("text" in (entries[0] || {}) ? entries[0]?.text : "", "Ask scribe for a concise update and tell me the answer.");
  assert.equal(entries[1]?.identity.label, "System");
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

test("mapFramesToTimelineEntries surfaces inbound comms requests from run_started prompts", () => {
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
  assert.equal(entries[0]?.kind, "message");
  assert.equal(entries[0]?.identity.id, "system");
  assert.match(entries[0]?.text || "", /Peer request:/);
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
