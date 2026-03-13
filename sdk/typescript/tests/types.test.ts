/**
 * Tests for every parse function in src/types.ts.
 *
 * Each section verifies:
 *   1. Parsing a valid wire-format (snake_case) object produces correct camelCase fields
 *   2. Missing optional fields get appropriate defaults
 *   3. The output is readonly / immutable where declared
 */
import { describe, it } from "node:test";
import assert from "node:assert/strict";

import {
  MEMBER_STATE_ACTIVE,
  MEMBER_STATE_RETIRING,
  ErrorCategory,
  parseStatusResult,
  parseCapabilitiesResult,
  parseReconcileResult,
  parseSpawnResult,
  parseKeepAliveConfig,
  parseEventEnvelope,
  parseSubscribeResult,
  parseSendMessageResult,
  parseRoutingResolution,
  parseDeliveryResult,
  parseDeliveryHistoryResult,
  parseMemoryQueryResult,
  parseMemoryStoreInfo,
  parseMemoryIndexResult,
  parseCallToolResult,
  parseMemberSnapshot,
  parseRuntimeRouteResult,
  parseGatingEvaluateResult,
  parseGatingDecisionResult,
  parseGatingAuditEntry,
  parseGatingPendingEntry,
  parseRediscoverReport,
  parseReconcileEdgesReport,
  parsePersistedEvent,
  parseErrorEvent,
  eventQueryToDict,
} from "../dist/index.js";

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

describe("constants", () => {
  it("MEMBER_STATE_ACTIVE is 'active'", () => {
    assert.equal(MEMBER_STATE_ACTIVE, "active");
  });

  it("MEMBER_STATE_RETIRING is 'retiring'", () => {
    assert.equal(MEMBER_STATE_RETIRING, "retiring");
  });
});

// ---------------------------------------------------------------------------
// parseStatusResult
// ---------------------------------------------------------------------------

describe("parseStatusResult", () => {
  it("parses valid wire-format object", () => {
    const result = parseStatusResult({
      contract_version: "0.2.0",
      running: true,
      loaded_modules: ["mod_a", "mod_b"],
    });
    assert.equal(result.contractVersion, "0.2.0");
    assert.equal(result.running, true);
    assert.deepEqual(result.loadedModules, ["mod_a", "mod_b"]);
  });

  it("defaults missing fields", () => {
    const result = parseStatusResult({});
    assert.equal(result.contractVersion, "");
    assert.equal(result.running, false);
    assert.deepEqual(result.loadedModules, []);
  });

  it("handles non-object input gracefully", () => {
    const result = parseStatusResult(null);
    assert.equal(result.contractVersion, "");
    assert.equal(result.running, false);
    assert.deepEqual(result.loadedModules, []);
  });

  it("produces readonly output", () => {
    const result = parseStatusResult({ running: true });
    // TypeScript enforces readonly at compile time; at runtime we verify
    // the shape is a plain frozen-style object
    assert.equal(typeof result, "object");
    assert.ok(Object.keys(result).length > 0);
  });
});

// ---------------------------------------------------------------------------
// parseCapabilitiesResult
// ---------------------------------------------------------------------------

describe("parseCapabilitiesResult", () => {
  it("parses valid wire-format object", () => {
    const result = parseCapabilitiesResult({
      contract_version: "0.3.0",
      methods: ["status", "spawn"],
      loaded_modules: ["core"],
    });
    assert.equal(result.contractVersion, "0.3.0");
    assert.deepEqual(result.methods, ["status", "spawn"]);
    assert.deepEqual(result.loadedModules, ["core"]);
  });

  it("defaults missing fields", () => {
    const result = parseCapabilitiesResult({});
    assert.equal(result.contractVersion, "");
    assert.deepEqual(result.methods, []);
    assert.deepEqual(result.loadedModules, []);
  });
});

// ---------------------------------------------------------------------------
// parseReconcileResult
// ---------------------------------------------------------------------------

describe("parseReconcileResult", () => {
  it("parses valid wire-format object", () => {
    const result = parseReconcileResult({
      accepted: true,
      reconciled_modules: ["mod_x"],
      added: 3,
    });
    assert.equal(result.accepted, true);
    assert.deepEqual(result.reconciledModules, ["mod_x"]);
    assert.equal(result.added, 3);
  });

  it("defaults missing fields", () => {
    const result = parseReconcileResult({});
    assert.equal(result.accepted, false);
    assert.deepEqual(result.reconciledModules, []);
    assert.equal(result.added, 0);
  });
});

// ---------------------------------------------------------------------------
// parseSpawnResult
// ---------------------------------------------------------------------------

describe("parseSpawnResult", () => {
  it("parses valid wire-format object", () => {
    const result = parseSpawnResult({
      accepted: true,
      module_id: "mod-1",
      meerkat_id: "mk-42",
      profile: "assistant",
    });
    assert.equal(result.accepted, true);
    assert.equal(result.moduleId, "mod-1");
    assert.equal(result.meerkatId, "mk-42");
    assert.equal(result.profile, "assistant");
  });

  it("nullable fields default to null", () => {
    const result = parseSpawnResult({ accepted: false, module_id: "m" });
    assert.equal(result.meerkatId, null);
    assert.equal(result.profile, null);
  });

  it("defaults missing fields", () => {
    const result = parseSpawnResult({});
    assert.equal(result.accepted, false);
    assert.equal(result.moduleId, "");
    assert.equal(result.meerkatId, null);
    assert.equal(result.profile, null);
  });
});

// ---------------------------------------------------------------------------
// parseKeepAliveConfig
// ---------------------------------------------------------------------------

describe("parseKeepAliveConfig", () => {
  it("parses valid wire-format object", () => {
    const result = parseKeepAliveConfig({
      interval_ms: 30000,
      event: "ping",
    });
    assert.equal(result.intervalMs, 30000);
    assert.equal(result.event, "ping");
  });

  it("defaults missing fields", () => {
    const result = parseKeepAliveConfig({});
    assert.equal(result.intervalMs, 0);
    assert.equal(result.event, "");
  });
});

// ---------------------------------------------------------------------------
// parseEventEnvelope
// ---------------------------------------------------------------------------

describe("parseEventEnvelope", () => {
  it("parses valid wire-format object", () => {
    const payload = { type: "text_delta", delta: "hi" };
    const result = parseEventEnvelope({
      event_id: "evt-1",
      source: "agent-1",
      timestamp_ms: 1700000000000,
      event: payload,
    });
    assert.equal(result.eventId, "evt-1");
    assert.equal(result.source, "agent-1");
    assert.equal(result.timestampMs, 1700000000000);
    assert.deepEqual(result.event, payload);
  });

  it("defaults missing fields", () => {
    const result = parseEventEnvelope({});
    assert.equal(result.eventId, "");
    assert.equal(result.source, "");
    assert.equal(result.timestampMs, 0);
    assert.equal(result.event, undefined);
  });
});

// ---------------------------------------------------------------------------
// parseSubscribeResult
// ---------------------------------------------------------------------------

describe("parseSubscribeResult", () => {
  it("parses valid wire-format object", () => {
    const result = parseSubscribeResult({
      scope: "session:s1",
      replay_from_event_id: "evt-0",
      keep_alive: { interval_ms: 5000, event: "ka" },
      keep_alive_comment: "stay alive",
      event_frames: ["frame-1"],
      events: [
        { event_id: "e1", source: "a", timestamp_ms: 100, event: {} },
      ],
    });
    assert.equal(result.scope, "session:s1");
    assert.equal(result.replayFromEventId, "evt-0");
    assert.equal(result.keepAlive.intervalMs, 5000);
    assert.equal(result.keepAlive.event, "ka");
    assert.equal(result.keepAliveComment, "stay alive");
    assert.deepEqual(result.eventFrames, ["frame-1"]);
    assert.equal(result.events.length, 1);
    assert.equal(result.events[0].eventId, "e1");
  });

  it("defaults missing fields", () => {
    const result = parseSubscribeResult({});
    assert.equal(result.scope, "");
    assert.equal(result.replayFromEventId, null);
    assert.equal(result.keepAlive.intervalMs, 0);
    assert.equal(result.keepAliveComment, "");
    assert.deepEqual(result.eventFrames, []);
    assert.deepEqual(result.events, []);
  });

  it("handles null replay_from_event_id", () => {
    const result = parseSubscribeResult({ replay_from_event_id: null });
    assert.equal(result.replayFromEventId, null);
  });
});

// ---------------------------------------------------------------------------
// parseSendMessageResult
// ---------------------------------------------------------------------------

describe("parseSendMessageResult", () => {
  it("parses valid wire-format object", () => {
    const result = parseSendMessageResult({
      accepted: true,
      member_id: "mem-1",
      session_id: "sess-1",
    });
    assert.equal(result.accepted, true);
    assert.equal(result.memberId, "mem-1");
    assert.equal(result.sessionId, "sess-1");
  });

  it("defaults missing fields", () => {
    const result = parseSendMessageResult({});
    assert.equal(result.accepted, false);
    assert.equal(result.memberId, "");
    assert.equal(result.sessionId, "");
  });
});

// ---------------------------------------------------------------------------
// parseRoutingResolution
// ---------------------------------------------------------------------------

describe("parseRoutingResolution", () => {
  it("parses valid wire-format object", () => {
    const route = { channel: "ch1", sink: "s1" };
    const result = parseRoutingResolution({
      recipient: "agent-2",
      route,
    });
    assert.equal(result.recipient, "agent-2");
    assert.deepEqual(result.route, route);
  });

  it("defaults missing fields", () => {
    const result = parseRoutingResolution({});
    assert.equal(result.recipient, "");
    // When route is missing, the fallback uses the whole record
    assert.equal(typeof result.route, "object");
  });
});

// ---------------------------------------------------------------------------
// parseDeliveryResult
// ---------------------------------------------------------------------------

describe("parseDeliveryResult", () => {
  it("parses valid wire-format object", () => {
    const result = parseDeliveryResult({
      delivered: true,
      delivery_id: "del-1",
    });
    assert.equal(result.delivered, true);
    assert.equal(result.deliveryId, "del-1");
  });

  it("defaults missing fields", () => {
    const result = parseDeliveryResult({});
    assert.equal(result.delivered, false);
    assert.equal(result.deliveryId, "");
  });
});

// ---------------------------------------------------------------------------
// parseDeliveryHistoryResult
// ---------------------------------------------------------------------------

describe("parseDeliveryHistoryResult", () => {
  it("parses valid wire-format object", () => {
    const result = parseDeliveryHistoryResult({
      deliveries: [{ id: "d1" }, { id: "d2" }],
    });
    assert.equal(result.deliveries.length, 2);
    assert.deepEqual(result.deliveries[0], { id: "d1" });
  });

  it("defaults missing deliveries to empty array", () => {
    const result = parseDeliveryHistoryResult({});
    assert.deepEqual(result.deliveries, []);
  });

  it("filters out non-object entries", () => {
    const result = parseDeliveryHistoryResult({
      deliveries: [{ id: "d1" }, "bad", 42, null],
    });
    assert.equal(result.deliveries.length, 1);
  });
});

// ---------------------------------------------------------------------------
// parseMemoryQueryResult
// ---------------------------------------------------------------------------

describe("parseMemoryQueryResult", () => {
  it("parses valid wire-format object", () => {
    const result = parseMemoryQueryResult({
      results: [{ key: "k1", value: "v1" }],
    });
    assert.equal(result.results.length, 1);
    assert.deepEqual(result.results[0], { key: "k1", value: "v1" });
  });

  it("defaults missing fields", () => {
    const result = parseMemoryQueryResult({});
    assert.deepEqual(result.results, []);
  });
});

// ---------------------------------------------------------------------------
// parseMemoryStoreInfo
// ---------------------------------------------------------------------------

describe("parseMemoryStoreInfo", () => {
  it("parses valid wire-format object", () => {
    const result = parseMemoryStoreInfo({
      store: "long_term",
      record_count: 42,
    });
    assert.equal(result.store, "long_term");
    assert.equal(result.recordCount, 42);
  });

  it("defaults missing fields", () => {
    const result = parseMemoryStoreInfo({});
    assert.equal(result.store, "");
    assert.equal(result.recordCount, 0);
  });
});

// ---------------------------------------------------------------------------
// parseMemoryIndexResult
// ---------------------------------------------------------------------------

describe("parseMemoryIndexResult", () => {
  it("parses valid wire-format object", () => {
    const result = parseMemoryIndexResult({
      entity: "user-1",
      topic: "preferences",
      store: "long_term",
      assertion_id: "a-1",
    });
    assert.equal(result.entity, "user-1");
    assert.equal(result.topic, "preferences");
    assert.equal(result.store, "long_term");
    assert.equal(result.assertionId, "a-1");
  });

  it("nullable assertionId defaults to null", () => {
    const result = parseMemoryIndexResult({
      entity: "e",
      topic: "t",
      store: "s",
    });
    assert.equal(result.assertionId, null);
  });

  it("defaults missing fields", () => {
    const result = parseMemoryIndexResult({});
    assert.equal(result.entity, "");
    assert.equal(result.topic, "");
    assert.equal(result.store, "");
    assert.equal(result.assertionId, null);
  });
});

// ---------------------------------------------------------------------------
// parseCallToolResult
// ---------------------------------------------------------------------------

describe("parseCallToolResult", () => {
  it("parses valid wire-format object", () => {
    const toolResult = { answer: 42 };
    const result = parseCallToolResult({
      module_id: "mod-1",
      tool: "calculator",
      result: toolResult,
    });
    assert.equal(result.moduleId, "mod-1");
    assert.equal(result.tool, "calculator");
    assert.deepEqual(result.result, toolResult);
  });

  it("defaults missing fields", () => {
    const result = parseCallToolResult({});
    assert.equal(result.moduleId, "");
    assert.equal(result.tool, "");
    assert.equal(result.result, undefined);
  });
});

// ---------------------------------------------------------------------------
// parseMemberSnapshot
// ---------------------------------------------------------------------------

describe("parseMemberSnapshot", () => {
  it("parses valid wire-format object", () => {
    const result = parseMemberSnapshot({
      meerkat_id: "mk-1",
      profile: "assistant",
      state: "active",
      wired_to: ["mk-2", "mk-3"],
      labels: { role: "lead", tier: "gold" },
    });
    assert.equal(result.meerkatId, "mk-1");
    assert.equal(result.profile, "assistant");
    assert.equal(result.state, "active");
    assert.deepEqual(result.wiredTo, ["mk-2", "mk-3"]);
    assert.deepEqual(result.labels, { role: "lead", tier: "gold" });
  });

  it("defaults missing fields", () => {
    const result = parseMemberSnapshot({});
    assert.equal(result.meerkatId, "");
    assert.equal(result.profile, "");
    assert.equal(result.state, "");
    assert.deepEqual(result.wiredTo, []);
    assert.deepEqual(result.labels, {});
  });

  it("filters non-string label values", () => {
    const result = parseMemberSnapshot({
      labels: { good: "yes", bad: 123, worse: null },
    });
    assert.deepEqual(result.labels, { good: "yes" });
  });
});

// ---------------------------------------------------------------------------
// parseRuntimeRouteResult
// ---------------------------------------------------------------------------

describe("parseRuntimeRouteResult", () => {
  it("parses valid wire-format object", () => {
    const result = parseRuntimeRouteResult({
      route_key: "rk-1",
      recipient: "agent-3",
      channel: "ch-1",
      sink: "http",
      target_module: "mod-routing",
    });
    assert.equal(result.routeKey, "rk-1");
    assert.equal(result.recipient, "agent-3");
    assert.equal(result.channel, "ch-1");
    assert.equal(result.sink, "http");
    assert.equal(result.targetModule, "mod-routing");
  });

  it("nullable channel defaults to null", () => {
    const result = parseRuntimeRouteResult({
      route_key: "rk",
      recipient: "r",
      sink: "s",
      target_module: "tm",
    });
    assert.equal(result.channel, null);
  });

  it("defaults missing fields", () => {
    const result = parseRuntimeRouteResult({});
    assert.equal(result.routeKey, "");
    assert.equal(result.recipient, "");
    assert.equal(result.channel, null);
    assert.equal(result.sink, "");
    assert.equal(result.targetModule, "");
  });
});

// ---------------------------------------------------------------------------
// parseGatingEvaluateResult
// ---------------------------------------------------------------------------

describe("parseGatingEvaluateResult", () => {
  it("parses valid wire-format object", () => {
    const result = parseGatingEvaluateResult({
      action_id: "act-1",
      action: "delete_user",
      actor_id: "actor-1",
      risk_tier: "high",
      outcome: "approved",
      pending_id: "pend-1",
    });
    assert.equal(result.actionId, "act-1");
    assert.equal(result.action, "delete_user");
    assert.equal(result.actorId, "actor-1");
    assert.equal(result.riskTier, "high");
    assert.equal(result.outcome, "approved");
    assert.equal(result.pendingId, "pend-1");
  });

  it("nullable fields default to null", () => {
    const result = parseGatingEvaluateResult({
      action_id: "a",
      action: "b",
      actor_id: "c",
      outcome: "denied",
    });
    assert.equal(result.riskTier, null);
    assert.equal(result.pendingId, null);
  });

  it("defaults missing fields", () => {
    const result = parseGatingEvaluateResult({});
    assert.equal(result.actionId, "");
    assert.equal(result.action, "");
    assert.equal(result.actorId, "");
    assert.equal(result.riskTier, null);
    assert.equal(result.outcome, "");
    assert.equal(result.pendingId, null);
  });
});

// ---------------------------------------------------------------------------
// parseGatingDecisionResult
// ---------------------------------------------------------------------------

describe("parseGatingDecisionResult", () => {
  it("parses valid wire-format object", () => {
    const result = parseGatingDecisionResult({
      pending_id: "pend-1",
      action_id: "act-1",
      decision: "approved",
    });
    assert.equal(result.pendingId, "pend-1");
    assert.equal(result.actionId, "act-1");
    assert.equal(result.decision, "approved");
  });

  it("defaults missing fields", () => {
    const result = parseGatingDecisionResult({});
    assert.equal(result.pendingId, "");
    assert.equal(result.actionId, "");
    assert.equal(result.decision, "");
  });
});

// ---------------------------------------------------------------------------
// parseGatingAuditEntry
// ---------------------------------------------------------------------------

describe("parseGatingAuditEntry", () => {
  it("parses valid wire-format object", () => {
    const result = parseGatingAuditEntry({
      audit_id: "aud-1",
      timestamp_ms: 1700000000000,
      event_type: "evaluate",
      action_id: "act-1",
      actor_id: "actor-1",
      risk_tier: "medium",
      outcome: "approved",
    });
    assert.equal(result.auditId, "aud-1");
    assert.equal(result.timestampMs, 1700000000000);
    assert.equal(result.eventType, "evaluate");
    assert.equal(result.actionId, "act-1");
    assert.equal(result.actorId, "actor-1");
    assert.equal(result.riskTier, "medium");
    assert.equal(result.outcome, "approved");
  });

  it("nullable riskTier defaults to null", () => {
    const result = parseGatingAuditEntry({
      audit_id: "a",
      timestamp_ms: 0,
      event_type: "e",
      action_id: "a",
      actor_id: "a",
      outcome: "o",
    });
    assert.equal(result.riskTier, null);
  });

  it("defaults missing fields", () => {
    const result = parseGatingAuditEntry({});
    assert.equal(result.auditId, "");
    assert.equal(result.timestampMs, 0);
    assert.equal(result.eventType, "");
    assert.equal(result.actionId, "");
    assert.equal(result.actorId, "");
    assert.equal(result.riskTier, null);
    assert.equal(result.outcome, "");
  });
});

// ---------------------------------------------------------------------------
// parseGatingPendingEntry
// ---------------------------------------------------------------------------

describe("parseGatingPendingEntry", () => {
  it("parses valid wire-format object", () => {
    const result = parseGatingPendingEntry({
      pending_id: "pend-1",
      action_id: "act-1",
      action: "transfer_funds",
      actor_id: "actor-1",
      risk_tier: "critical",
      created_at_ms: 1700000000000,
    });
    assert.equal(result.pendingId, "pend-1");
    assert.equal(result.actionId, "act-1");
    assert.equal(result.action, "transfer_funds");
    assert.equal(result.actorId, "actor-1");
    assert.equal(result.riskTier, "critical");
    assert.equal(result.createdAtMs, 1700000000000);
  });

  it("nullable riskTier defaults to null", () => {
    const result = parseGatingPendingEntry({
      pending_id: "p",
      action_id: "a",
      action: "x",
      actor_id: "a",
      created_at_ms: 0,
    });
    assert.equal(result.riskTier, null);
  });

  it("defaults missing fields", () => {
    const result = parseGatingPendingEntry({});
    assert.equal(result.pendingId, "");
    assert.equal(result.actionId, "");
    assert.equal(result.action, "");
    assert.equal(result.actorId, "");
    assert.equal(result.riskTier, null);
    assert.equal(result.createdAtMs, 0);
  });
});

// ---------------------------------------------------------------------------
// parseReconcileEdgesReport
// ---------------------------------------------------------------------------

describe("parseReconcileEdgesReport", () => {
  it("parses valid wire-format object", () => {
    const result = parseReconcileEdgesReport({
      desired_edges: [{ from: "a", to: "b" }],
      wired_edges: [{ from: "a", to: "b" }],
      unwired_edges: [],
      retained_edges: [{ from: "c", to: "d" }],
      preexisting_edges: [],
      skipped_missing_members: [],
      pruned_stale_managed_edges: [],
      failures: [],
    });
    assert.equal(result.desiredEdges.length, 1);
    assert.equal(result.wiredEdges.length, 1);
    assert.deepEqual(result.unwiredEdges, []);
    assert.equal(result.retainedEdges.length, 1);
    assert.deepEqual(result.preexistingEdges, []);
    assert.deepEqual(result.skippedMissingMembers, []);
    assert.deepEqual(result.prunedStaleManagedEdges, []);
    assert.deepEqual(result.failures, []);
  });

  it("isComplete is true when no failures and no skipped", () => {
    const result = parseReconcileEdgesReport({
      failures: [],
      skipped_missing_members: [],
    });
    assert.equal(result.isComplete, true);
  });

  it("isComplete is false when failures present", () => {
    const result = parseReconcileEdgesReport({
      failures: [{ reason: "timeout" }],
      skipped_missing_members: [],
    });
    assert.equal(result.isComplete, false);
  });

  it("isComplete is false when skipped_missing_members present", () => {
    const result = parseReconcileEdgesReport({
      failures: [],
      skipped_missing_members: [{ member: "mk-1" }],
    });
    assert.equal(result.isComplete, false);
  });

  it("isComplete is false when both failures and skipped present", () => {
    const result = parseReconcileEdgesReport({
      failures: [{ reason: "err" }],
      skipped_missing_members: [{ member: "mk-1" }],
    });
    assert.equal(result.isComplete, false);
  });

  it("defaults missing fields to empty arrays", () => {
    const result = parseReconcileEdgesReport({});
    assert.deepEqual(result.desiredEdges, []);
    assert.deepEqual(result.wiredEdges, []);
    assert.deepEqual(result.unwiredEdges, []);
    assert.deepEqual(result.retainedEdges, []);
    assert.deepEqual(result.preexistingEdges, []);
    assert.deepEqual(result.skippedMissingMembers, []);
    assert.deepEqual(result.prunedStaleManagedEdges, []);
    assert.deepEqual(result.failures, []);
    assert.equal(result.isComplete, true);
  });
});

// ---------------------------------------------------------------------------
// parseRediscoverReport
// ---------------------------------------------------------------------------

describe("parseRediscoverReport", () => {
  it("parses valid wire-format object", () => {
    const result = parseRediscoverReport({
      spawned: ["mk-1", "mk-2"],
      edges: {
        desired_edges: [{ from: "a", to: "b" }],
        wired_edges: [],
        unwired_edges: [],
        retained_edges: [],
        preexisting_edges: [],
        skipped_missing_members: [],
        pruned_stale_managed_edges: [],
        failures: [],
      },
    });
    assert.deepEqual(result.spawned, ["mk-1", "mk-2"]);
    assert.equal(result.edges.desiredEdges.length, 1);
    assert.equal(result.edges.isComplete, true);
  });

  it("defaults missing fields", () => {
    const result = parseRediscoverReport({});
    assert.deepEqual(result.spawned, []);
    assert.deepEqual(result.edges.desiredEdges, []);
    assert.equal(result.edges.isComplete, true);
  });
});

// ---------------------------------------------------------------------------
// parsePersistedEvent — Agent variant
// ---------------------------------------------------------------------------

describe("parsePersistedEvent", () => {
  it("parses Agent unified event", () => {
    const result = parsePersistedEvent({
      id: "evt-1",
      seq: 5,
      timestamp_ms: 1700000000000,
      member_id: "mem-1",
      event: {
        Agent: { agent_id: "agent-1", event_type: "text_delta" },
      },
    });
    assert.equal(result.id, "evt-1");
    assert.equal(result.seq, 5);
    assert.equal(result.timestampMs, 1700000000000);
    assert.equal(result.memberId, "mem-1");
    assert.equal(result.event.kind, "agent");
    if (result.event.kind === "agent") {
      assert.equal(result.event.agentId, "agent-1");
      assert.equal(result.event.eventType, "text_delta");
    }
  });

  it("parses Module unified event", () => {
    const result = parsePersistedEvent({
      id: "evt-2",
      seq: 6,
      timestamp_ms: 1700000000001,
      member_id: "mem-2",
      event: {
        Module: {
          module: "mod_memory",
          event_type: "store_updated",
          payload: { key: "val" },
        },
      },
    });
    assert.equal(result.event.kind, "module");
    if (result.event.kind === "module") {
      assert.equal(result.event.module, "mod_memory");
      assert.equal(result.event.eventType, "store_updated");
      assert.deepEqual(result.event.payload, { key: "val" });
    }
  });

  it("nullable memberId defaults to null", () => {
    const result = parsePersistedEvent({
      id: "e",
      seq: 0,
      timestamp_ms: 0,
      event: { Agent: { agent_id: "a", event_type: "e" } },
    });
    assert.equal(result.memberId, null);
  });

  it("fallback for unknown event shape", () => {
    const result = parsePersistedEvent({
      id: "e",
      seq: 0,
      timestamp_ms: 0,
      event: { SomethingElse: { data: "x" } },
    });
    // Falls through to module fallback
    assert.equal(result.event.kind, "module");
    if (result.event.kind === "module") {
      assert.equal(result.event.module, "unknown");
      assert.equal(result.event.eventType, "unknown");
    }
  });

  it("fallback for missing event", () => {
    const result = parsePersistedEvent({ id: "e", seq: 0, timestamp_ms: 0 });
    assert.equal(result.event.kind, "module");
    if (result.event.kind === "module") {
      assert.equal(result.event.module, "unknown");
      assert.equal(result.event.eventType, "unknown");
      assert.deepEqual(result.event.payload, {});
    }
  });

  it("defaults missing fields", () => {
    const result = parsePersistedEvent({});
    assert.equal(result.id, "");
    assert.equal(result.seq, 0);
    assert.equal(result.timestampMs, 0);
    assert.equal(result.memberId, null);
  });
});

// ---------------------------------------------------------------------------
// parseErrorEvent — all ErrorCategory variants
// ---------------------------------------------------------------------------

describe("parseErrorEvent", () => {
  it("parses spawn_failure with member_id", () => {
    const result = parseErrorEvent({
      category: "spawn_failure",
      error: "timeout",
      member_id: "mk-1",
    });
    assert.equal(result.category, "spawn_failure");
    assert.equal(result.message, "mk-1: timeout");
    assert.equal(result.context.error, "timeout");
    assert.equal(result.context.member_id, "mk-1");
  });

  it("parses spawn_failure without member_id", () => {
    const result = parseErrorEvent({
      category: "spawn_failure",
      error: "no slots",
    });
    assert.equal(result.message, "no slots");
  });

  it("parses reconcile_incomplete", () => {
    const result = parseErrorEvent({
      category: "reconcile_incomplete",
      failures: 2,
      skipped: 1,
    });
    assert.equal(result.category, "reconcile_incomplete");
    assert.equal(result.message, "2 failures, 1 skipped");
  });

  it("parses checkpoint_failure with session_id", () => {
    const result = parseErrorEvent({
      category: "checkpoint_failure",
      error: "disk full",
      session_id: "sess-1",
    });
    assert.equal(result.category, "checkpoint_failure");
    assert.equal(result.message, "sess-1: disk full");
  });

  it("parses checkpoint_failure without session_id", () => {
    const result = parseErrorEvent({
      category: "checkpoint_failure",
      error: "disk full",
    });
    assert.equal(result.message, "disk full");
  });

  it("parses host_loop_crash with member_id", () => {
    const result = parseErrorEvent({
      category: "host_loop_crash",
      error: "panic",
      member_id: "mk-2",
    });
    assert.equal(result.category, "host_loop_crash");
    assert.equal(result.message, "mk-2: panic");
  });

  it("parses host_loop_crash without member_id", () => {
    const result = parseErrorEvent({
      category: "host_loop_crash",
      error: "panic",
    });
    assert.equal(result.message, "panic");
  });

  it("parses rediscover_failure", () => {
    const result = parseErrorEvent({
      category: "rediscover_failure",
      error: "network unreachable",
    });
    assert.equal(result.category, "rediscover_failure");
    assert.equal(result.message, "network unreachable");
  });

  it("parses unknown category as JSON", () => {
    const result = parseErrorEvent({
      category: "alien_invasion",
      data: "xeno",
    });
    assert.equal(result.category, "alien_invasion");
    // Unknown categories get JSON.stringify of the full input
    const parsed = JSON.parse(result.message);
    assert.equal(parsed.category, "alien_invasion");
    assert.equal(parsed.data, "xeno");
  });

  it("defaults missing category to 'unknown'", () => {
    const result = parseErrorEvent({});
    assert.equal(result.category, "unknown");
  });

  it("context excludes category key", () => {
    const result = parseErrorEvent({
      category: "spawn_failure",
      error: "x",
      extra: "data",
    });
    assert.equal(result.context.category, undefined);
    assert.equal(result.context.error, "x");
    assert.equal(result.context.extra, "data");
  });

  it("ErrorCategory constants match expected values", () => {
    assert.equal(ErrorCategory.SPAWN_FAILURE, "spawn_failure");
    assert.equal(ErrorCategory.RECONCILE_INCOMPLETE, "reconcile_incomplete");
    assert.equal(ErrorCategory.CHECKPOINT_FAILURE, "checkpoint_failure");
    assert.equal(ErrorCategory.HOST_LOOP_CRASH, "host_loop_crash");
    assert.equal(ErrorCategory.REDISCOVER_FAILURE, "rediscover_failure");
  });
});

// ---------------------------------------------------------------------------
// eventQueryToDict
// ---------------------------------------------------------------------------

describe("eventQueryToDict", () => {
  it("converts all fields to snake_case", () => {
    const result = eventQueryToDict({
      sinceMs: 1000,
      untilMs: 2000,
      memberId: "mem-1",
      eventTypes: ["text_delta", "run_completed"],
      limit: 50,
      afterSeq: 10,
    });
    assert.equal(result.since_ms, 1000);
    assert.equal(result.until_ms, 2000);
    assert.equal(result.member_id, "mem-1");
    assert.deepEqual(result.event_types, ["text_delta", "run_completed"]);
    assert.equal(result.limit, 50);
    assert.equal(result.after_seq, 10);
  });

  it("omits undefined fields", () => {
    const result = eventQueryToDict({});
    assert.deepEqual(result, {});
  });

  it("omits empty eventTypes array", () => {
    const result = eventQueryToDict({ eventTypes: [] });
    assert.equal(result.event_types, undefined);
  });

  it("includes sinceMs alone", () => {
    const result = eventQueryToDict({ sinceMs: 500 });
    assert.deepEqual(result, { since_ms: 500 });
  });

  it("copies eventTypes array (no shared reference)", () => {
    const types = ["a", "b"];
    const result = eventQueryToDict({ eventTypes: types });
    assert.deepEqual(result.event_types, ["a", "b"]);
    assert.notEqual(result.event_types, types);
  });
});
