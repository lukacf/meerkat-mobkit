import assert from "node:assert/strict";
import test from "node:test";

import {
  normalizeConsoleIdentityEventEnvelope,
  normalizeConsoleInteractionAccepted,
  normalizeConsoleInteractionRequest,
  normalizeConsoleDockTargetAddressingMode,
  normalizeConsoleInteractionRejectedError,
  normalizeActivityFilterPreset,
  normalizeExperienceSectionMeta,
  normalizeGatingActionRequest,
  normalizeGatingActionResult,
  normalizeIdentityStreamRequest,
  normalizeIdentityInspectViewState,
  normalizeIdentityStatusRow,
  normalizeReplayUnavailableError,
  normalizeResponsePhase,
  normalizeRoutingSectionView,
  normalizeSidebarWatchFields,
  normalizeToolCallAccumulatorState,
} from "./control-plane";
import { normalizeConsoleSidebarViewState } from "./sidebar";

test("normalizeExperienceSectionMeta keeps valid poll and stream metadata", () => {
  assert.deepEqual(
    normalizeExperienceSectionMeta({
      schema_version: "1",
      refresh: { mode: "stream", topic: "identity:luka", update_semantics: "append" },
      capabilities: ["inspect", " inspect ", "", null],
    }),
    {
      schema_version: "1",
      refresh: { mode: "stream", topic: "identity:luka", update_semantics: "append" },
      capabilities: ["inspect"],
    },
  );

  assert.equal(
    normalizeExperienceSectionMeta({
      schema_version: "1",
      refresh: { mode: "poll", interval_ms: 0 },
    }),
    null,
  );
});

test("normalizeIdentityStatusRow and response phase trim data without inventing fields", () => {
  assert.deepEqual(
    normalizeIdentityStatusRow({
      identity: " identity:luka ",
      display_name: " Luka ",
      profile: " operator ",
      state: " active ",
      addressability: "addressable",
      labels: { team: " console ", empty: "   ", " ": "ignored" },
      generation: 4,
      checkpoint_version: 8,
      lease_healthy: true,
    }),
    {
      identity: "identity:luka",
      display_name: "Luka",
      profile: "operator",
      state: "active",
      addressability: "addressable",
      labels: { team: "console" },
      generation: 4,
      checkpoint_version: 8,
      lease_healthy: true,
    },
  );

  assert.equal(normalizeResponsePhase("tool-executing"), "tool-executing");
  assert.equal(normalizeResponsePhase("thinking"), null);
});

test("normalize sidebar watch fields and filter presets preserves the phase-0 contract", () => {
  assert.deepEqual(
    normalizeSidebarWatchFields({
      watched: true,
      alertLevel: "elevated",
      degraded: true,
      degradedReason: " lease_expired ",
    }),
    {
      watched: true,
      alertLevel: "elevated",
      degraded: true,
      degradedReason: "lease_expired",
    },
  );

  assert.deepEqual(
    normalizeActivityFilterPreset({
      id: " watched-critical ",
      label: " Critical ",
      watchedOnly: true,
      alertLevels: ["critical", "invalid", "critical"],
      eventTypeFilter: ["tool_call", "", "tool_call", "interaction_failed"],
    }),
    {
      id: "watched-critical",
      label: "Critical",
      watchedOnly: true,
      alertLevels: ["critical"],
      eventTypeFilter: ["tool_call", "interaction_failed"],
    },
  );
});

test("sidebar normalization preserves warning tone and watch fields", () => {
  const viewState = normalizeConsoleSidebarViewState({
    blocks: [
      {
        id: "agents",
        kind: "list",
        sections: [
          {
            id: "operators",
            title: "Operators",
            items: [
              {
                id: "identity:luka",
                title: "Luka",
                watched: true,
                alertLevel: "elevated",
                degraded: true,
                degradedReason: "lease_expired",
                meta: [{ id: "status", label: "elevated", tone: "warning" }],
              },
            ],
          },
        ],
      },
    ],
  });

  const item = viewState.blocks[0]?.sections?.[0]?.items?.[0];
  assert.equal(item?.watched, true);
  assert.equal(item?.alertLevel, "elevated");
  assert.equal(item?.degraded, true);
  assert.equal(item?.degradedReason, "lease_expired");
  assert.equal(item?.meta?.[0]?.tone, "warning");
});

test("normalize inspect, dock addressing, and typed console errors", () => {
  assert.equal(normalizeConsoleDockTargetAddressingMode("identity"), "identity");
  assert.equal(normalizeConsoleDockTargetAddressingMode("anything-else"), "member");

  assert.deepEqual(
    normalizeIdentityInspectViewState({
      identity: " identity:luka ",
      display_name: " Luka ",
      profile: "operator",
      state: "running",
      addressability: "addressable",
      labels: { role: " primary " },
      continuity: {
        generation: 4,
        checkpoint_version: 8,
        session_id: " session-1 ",
        agent_runtime_id: " runtime-1 ",
      },
      lease: {
        fencing_token: 12,
        ttl_remaining_ms: 9000,
        healthy: true,
      },
      topology_peers: [" peer-a ", "", "peer-b"],
      is_final: false,
    }),
    {
      identity: "identity:luka",
      display_name: "Luka",
      profile: "operator",
      state: "running",
      addressability: "addressable",
      labels: { role: "primary" },
      continuity: {
        generation: 4,
        checkpoint_version: 8,
        session_id: "session-1",
        agent_runtime_id: "runtime-1",
      },
      lease: {
        fencing_token: 12,
        ttl_remaining_ms: 9000,
        healthy: true,
      },
      topology_peers: ["peer-a", "peer-b"],
      is_final: false,
    },
  );

  assert.deepEqual(
    normalizeReplayUnavailableError({
      error: "replay_unavailable",
      stream: "identity",
      requested_last_event_id: "evt-1",
      latest_event_id: "evt-99",
    }),
    {
      error: "replay_unavailable",
      stream: "identity",
      requested_last_event_id: "evt-1",
      latest_event_id: "evt-99",
    },
  );

  assert.deepEqual(
    normalizeConsoleInteractionRejectedError({
      code: -32002,
      message: " identity not addressable ",
    }),
    {
      code: -32002,
      message: "identity not addressable",
    },
  );
});

test("normalize gating and routing payloads for the shared control-plane contract", () => {
  assert.deepEqual(
    normalizeGatingActionRequest({
      pending_id: " pending-1 ",
      approver_id: " luka ",
      decision: "escalate",
      reason: " needs higher approval ",
    }),
    {
      pending_id: "pending-1",
      approver_id: "luka",
      decision: "escalate",
      reason: "needs higher approval",
    },
  );

  assert.deepEqual(
    normalizeGatingActionResult({
      pending_id: "pending-1",
      action_id: "action-1",
      approver_id: "luka",
      decision: "escalate",
      outcome: "pending_approval",
      decided_at_ms: 1717171717,
      next_pending_id: " pending-2 ",
    }),
    {
      pending_id: "pending-1",
      action_id: "action-1",
      approver_id: "luka",
      decision: "escalate",
      outcome: "pending_approval",
      decided_at_ms: 1717171717,
      next_pending_id: "pending-2",
    },
  );

  assert.deepEqual(
    normalizeRoutingSectionView({
      routes: [
        {
          route_key: " route:1 ",
          recipient: " ops ",
          sink: "slack",
          target_module: "routing",
          channel: " approvals ",
        },
      ],
      deliveries: [
        {
          delivery_id: " delivery-1 ",
          route_id: " route-1 ",
          recipient: "ops",
          sink: "slack",
          target_module: "routing",
          status: "delivered",
          first_attempt_ms: 1,
          final_attempt_ms: 2,
          attempts: [
            { attempt: 1, status: "sent", backoff_ms: 0 },
          ],
        },
      ],
    }),
    {
      routes: [
        {
          route_key: "route:1",
          recipient: "ops",
          sink: "slack",
          target_module: "routing",
          channel: "approvals",
        },
      ],
      deliveries: [
        {
          delivery_id: "delivery-1",
          route_id: "route-1",
          recipient: "ops",
          sink: "slack",
          target_module: "routing",
          status: "delivered",
          first_attempt_ms: 1,
          final_attempt_ms: 2,
          attempts: [
            { attempt: 1, status: "sent", backoff_ms: 0 },
          ],
        },
      ],
    },
  );
});

test("normalize interaction request/acceptance, stream request, and event envelope", () => {
  assert.deepEqual(
    normalizeConsoleInteractionRequest({
      identity: " identity:luka ",
      content: " hello ",
      origin: " console:panel-1 ",
    }),
    {
      identity: "identity:luka",
      content: "hello",
      origin: "console:panel-1",
    },
  );

  assert.deepEqual(
    normalizeConsoleInteractionAccepted({
      interaction_id: " turn-1 ",
      identity: " identity:luka ",
    }),
    {
      interaction_id: "turn-1",
      identity: "identity:luka",
    },
  );

  assert.deepEqual(
    normalizeIdentityStreamRequest({ identity: " identity:luka " }),
    { identity: "identity:luka" },
  );

  assert.deepEqual(
    normalizeConsoleIdentityEventEnvelope({
      event_id: " evt-1 ",
      interaction_id: " turn-1 ",
      identity: " identity:luka ",
      event_type: " tool_call ",
      timestamp_ms: 12,
      data: { tool_call_id: "tool-1" },
    }),
    {
      event_id: "evt-1",
      interaction_id: "turn-1",
      identity: "identity:luka",
      event_type: "tool_call",
      timestamp_ms: 12,
      data: { tool_call_id: "tool-1" },
    },
  );
});

test("normalize tool-call accumulator state for pending and out-of-order results", () => {
  assert.deepEqual(
    normalizeToolCallAccumulatorState({
      timeoutMs: 60000,
      toolCalls: {
        " tool-1 ": {
          type: "tool-call",
          name: "search",
          arguments: " {\"q\":\"test\"} ",
          status: "pending",
        },
      },
      pendingResults: {
        " tool-2 ": " result before call ",
      },
    }),
    {
      timeoutMs: 60000,
      toolCalls: {
        "tool-1": {
          type: "tool-call",
          toolCallId: "tool-1",
          name: "search",
          arguments: "{\"q\":\"test\"}",
          status: "pending",
        },
      },
      pendingResults: {
        "tool-2": "result before call",
      },
    },
  );
});
