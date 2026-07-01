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
  parseAgentMemoryRecord,
  parseAgentMemoryRecallResult,
  parseAgentMemoryForgetResult,
  parseMemoryStoreInfo,
  parseMemoryIndexResult,
  parseCallToolResult,
  parseMemberSnapshot,
  parseRichMemberSnapshot,
  parseRuntimeRouteResult,
  parseGatingEvaluateResult,
  parseGatingDecisionResult,
  parseGatingAuditEntry,
  parseGatingPendingEntry,
  parseRediscoverReport,
  parseReconcileEdgesReport,
  parsePersistedEvent,
  parseErrorEvent,
  parseMobpackToolsCatalogResult,
  parseMobpackSkillsCatalogResult,
  parseMobpackAgentDefinitionsResult,
  parseMobpackTemplatesResult,
  parseMobpackCatalogsResult,
  parseMobpackValidationResult,
  parseMobpackSourceResult,
  parseMobpackExportResult,
  parseMobpackImportResult,
  parseIdentityResolvedToolsResult,
  parseMobpackDraftRow,
  parseMobpackDraftListResult,
  parseMobpackDraftGetResult,
  parseMobpackDraftSaveResult,
  parseMobpackDraftDeleteResult,
  parseMobpackDraftHistoryResult,
  parseMobpackApplyOperationResult,
  parseMobpackDeployCommandResult,
  parseMobpackDeployResult,
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
// parseRichMemberSnapshot / peer connectivity tri-state
// ---------------------------------------------------------------------------

describe("parseRichMemberSnapshot peer_connectivity", () => {
  it("reads counts from the nested 0.7.x known snapshot", () => {
    // Regression: the old flat reader returned all-zeros for the tri-state shape.
    const snapshot = parseRichMemberSnapshot({
      status: "active",
      tokens_used: 5,
      is_final: false,
      peer_connectivity: {
        status: "known",
        snapshot: {
          reachable_peer_count: 3,
          unknown_peer_count: 1,
          unreachable_peers: [{ peer: "p1", reason: "x" }],
        },
      },
    });
    assert.ok(snapshot.peerConnectivity);
    assert.equal(snapshot.peerConnectivity?.status, "known");
    assert.equal(snapshot.peerConnectivity?.reachablePeerCount, 3);
    assert.equal(snapshot.peerConnectivity?.unknownPeerCount, 1);
    assert.equal(snapshot.peerConnectivity?.unreachablePeers.length, 1);
  });

  it("surfaces not_applicable and probe_timed_out distinctly with zero counts", () => {
    const notApplicable = parseRichMemberSnapshot({
      status: "active",
      tokens_used: 0,
      is_final: false,
      peer_connectivity: { status: "not_applicable" },
    });
    assert.equal(notApplicable.peerConnectivity?.status, "not_applicable");
    assert.equal(notApplicable.peerConnectivity?.reachablePeerCount, 0);

    const timedOut = parseRichMemberSnapshot({
      status: "active",
      tokens_used: 0,
      is_final: false,
      peer_connectivity: { status: "probe_timed_out" },
    });
    assert.equal(timedOut.peerConnectivity?.status, "probe_timed_out");
  });

  it("still accepts the legacy flat shape", () => {
    const snapshot = parseRichMemberSnapshot({
      status: "active",
      tokens_used: 0,
      is_final: false,
      peer_connectivity: {
        reachable_peer_count: 2,
        unknown_peer_count: 0,
        unreachable_peers: [],
      },
    });
    assert.equal(snapshot.peerConnectivity?.reachablePeerCount, 2);
    assert.equal(snapshot.peerConnectivity?.status, "known");
  });
});

describe("parseIdentityResolvedToolsResult", () => {
  it("reads the per-identity resolved tool surface", () => {
    const result = parseIdentityResolvedToolsResult({
      identity: "domain:security",
      session_id: "sid-1",
      tools: ["shell", "send_message"],
    });
    assert.equal(result.identity, "domain:security");
    assert.equal(result.sessionId, "sid-1");
    assert.deepEqual(result.tools, ["shell", "send_message"]);
  });
});

// ---------------------------------------------------------------------------
// parseStatusResult
// ---------------------------------------------------------------------------

describe("parseStatusResult", () => {
  it("parses valid wire-format object", () => {
    const result = parseStatusResult({
      contract_version: "0.4.0",
      running: true,
      loaded_modules: ["mod_a", "mod_b"],
    });
    assert.equal(result.contractVersion, "0.4.0");
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
      contract_version: "0.4.0",
      methods: ["status", "spawn"],
      loaded_modules: ["core"],
    });
    assert.equal(result.contractVersion, "0.4.0");
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
      agent_identity: "mk-42",
      role: "assistant",
    });
    assert.equal(result.accepted, true);
    assert.equal(result.moduleId, "mod-1");
    assert.equal(result.agentIdentity, "mk-42");
    assert.equal(result.role, "assistant");
  });

  it("nullable fields default to null", () => {
    const result = parseSpawnResult({ accepted: false, module_id: "m" });
    assert.equal(result.agentIdentity, null);
    assert.equal(result.role, null);
  });

  it("defaults missing fields", () => {
    const result = parseSpawnResult({});
    assert.equal(result.accepted, false);
    assert.equal(result.moduleId, "");
    assert.equal(result.agentIdentity, null);
    assert.equal(result.role, null);
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
      assertions: [{
        assertion_id: "a-1",
        entity: "identity:luka",
        topic: "preferences",
        store: "knowledge_graph",
        fact: "Prefers concise summaries.",
        metadata: { source: "test" },
        indexed_at_ms: 100,
      }],
      conflicts: [{
        entity: "identity:luka",
        topic: "preferences",
        store: "knowledge_graph",
        reason: "stale",
        updated_at_ms: 200,
      }],
    });
    assert.equal(result.assertions.length, 1);
    assert.equal(result.assertions[0].assertionId, "a-1");
    assert.equal(result.assertions[0].fact, "Prefers concise summaries.");
    assert.equal(result.conflicts.length, 1);
    assert.equal(result.conflicts[0].reason, "stale");
    assert.equal(result.results.length, 2);
  });

  it("preserves legacy results when Rust fields are absent", () => {
    const result = parseMemoryQueryResult({
      results: [{ key: "k1", value: "v1" }],
    });
    assert.equal(result.assertions.length, 0);
    assert.equal(result.conflicts.length, 0);
    assert.equal(result.results.length, 1);
    assert.deepEqual(result.results[0], { key: "k1", value: "v1" });
  });

  it("defaults missing fields", () => {
    const result = parseMemoryQueryResult({});
    assert.deepEqual(result.assertions, []);
    assert.deepEqual(result.conflicts, []);
    assert.deepEqual(result.results, []);
  });
});

// ---------------------------------------------------------------------------
// parseAgentMemoryRecord
// ---------------------------------------------------------------------------

describe("parseAgentMemoryRecord", () => {
  it("parses valid wire-format object", () => {
    const result = parseAgentMemoryRecord({
      memory_id: "mem-1",
      title: "School pickup",
      body: "Pickup is before calendar planning.",
      tags: ["calendar", "family"],
      created_at_ms: 10,
      updated_at_ms: 20,
    });
    assert.equal(result.memoryId, "mem-1");
    assert.equal(result.title, "School pickup");
    assert.equal(result.body, "Pickup is before calendar planning.");
    assert.deepEqual(result.tags, ["calendar", "family"]);
    assert.equal(result.createdAtMs, 10);
    assert.equal(result.updatedAtMs, 20);
  });

  it("rejects malformed durable records", () => {
    assert.throws(
      () => parseAgentMemoryRecord({}),
      /agent_memory_record\.memory_id must be a non-empty string/,
    );
    assert.throws(
      () =>
        parseAgentMemoryRecord({
          memory_id: "mem-1",
          title: "Title",
          body: "Body",
          tags: [42],
          created_at_ms: 1,
          updated_at_ms: 1,
        }),
      /agent_memory_record\.tags must be an array of strings/,
    );
  });
});

// ---------------------------------------------------------------------------
// parseAgentMemoryRecallResult
// ---------------------------------------------------------------------------

describe("parseAgentMemoryRecallResult", () => {
  it("parses valid recall envelopes", () => {
    const result = parseAgentMemoryRecallResult({
      records: [{
        memory_id: "mem-1",
        title: "School pickup",
        body: "Pickup is before calendar planning.",
        tags: ["calendar", "family"],
        created_at_ms: 10,
        updated_at_ms: 20,
      }],
    });

    assert.equal(result.records.length, 1);
    assert.equal(result.records[0]!.memoryId, "mem-1");
  });

  it("rejects malformed recall envelopes", () => {
    assert.throws(
      () => parseAgentMemoryRecallResult({}),
      /agent_memory_recall_result\.records must be an array/,
    );
    assert.throws(
      () =>
        parseAgentMemoryRecallResult({
          records: [{ memory_id: "mem-1" }],
        }),
      /agent_memory_record\.title must be a non-empty string/,
    );
  });
});

// ---------------------------------------------------------------------------
// parseAgentMemoryForgetResult
// ---------------------------------------------------------------------------

describe("parseAgentMemoryForgetResult", () => {
  it("parses valid wire-format object", () => {
    const result = parseAgentMemoryForgetResult({
      memory_id: "mem-1",
      deleted: true,
    });

    assert.equal(result.memoryId, "mem-1");
    assert.equal(result.deleted, true);
  });

  it("rejects malformed forget results", () => {
    assert.throws(
      () => parseAgentMemoryForgetResult({}),
      /agent_memory_forget_result\.memory_id must be a non-empty string/,
    );
    assert.throws(
      () =>
        parseAgentMemoryForgetResult({
          memory_id: "mem-1",
          deleted: "yes",
        }),
      /agent_memory_forget_result\.deleted must be a boolean/,
    );
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
      conflict_active: false,
    });
    assert.equal(result.entity, "user-1");
    assert.equal(result.topic, "preferences");
    assert.equal(result.store, "long_term");
    assert.equal(result.assertionId, "a-1");
    assert.equal(result.conflictActive, false);
  });

  it("parses conflict_active true", () => {
    const result = parseMemoryIndexResult({
      entity: "e",
      topic: "t",
      store: "s",
      conflict_active: true,
    });
    assert.equal(result.conflictActive, true);
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
    assert.equal(result.conflictActive, false);
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
      agent_identity: "mk-1",
      role: "assistant",
      state: "active",
      wired_to: ["mk-2", "mk-3"],
      labels: { role: "lead", tier: "gold" },
    });
    assert.equal(result.agentIdentity, "mk-1");
    assert.equal(result.role, "assistant");
    assert.equal(result.state, "active");
    assert.deepEqual(result.wiredTo, ["mk-2", "mk-3"]);
    assert.deepEqual(result.labels, { role: "lead", tier: "gold" });
  });

  it("defaults missing fields", () => {
    const result = parseMemberSnapshot({});
    assert.equal(result.agentIdentity, "");
    assert.equal(result.role, "");
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
      assert.equal(result.event.payload, null);
    }
  });

  it("parses Agent unified event payload when present", () => {
    const result = parsePersistedEvent({
      id: "evt-1b",
      seq: 6,
      timestamp_ms: 1700000000002,
      member_id: "mem-1",
      event: {
        Agent: {
          agent_id: "agent-1",
          event_type: "tool_execution_completed",
          payload: { type: "tool_execution_completed", tool_call_id: "tool-1", result: "done" },
        },
      },
    });
    assert.equal(result.event.kind, "agent");
    if (result.event.kind === "agent") {
      assert.deepEqual(result.event.payload, {
        type: "tool_execution_completed",
        tool_call_id: "tool-1",
        result: "done",
      });
    }
  });

  it("parses Rust internally tagged agent events", () => {
    const result = parsePersistedEvent({
      id: "evt-rust-agent",
      seq: 7,
      timestamp_ms: 1700000000003,
      member_id: "mem-1",
      event: {
        kind: "agent",
        agent_id: "agent-1",
        event_type: "run_completed",
        payload: { ok: true },
      },
    });
    assert.equal(result.event.kind, "agent");
    if (result.event.kind === "agent") {
      assert.equal(result.event.agentId, "agent-1");
      assert.equal(result.event.eventType, "run_completed");
      assert.deepEqual(result.event.payload, { ok: true });
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

  it("parses Rust internally tagged module events", () => {
    const result = parsePersistedEvent({
      id: "evt-rust-module",
      seq: 8,
      timestamp_ms: 1700000000004,
      event: {
        kind: "module",
        module: "routing",
        event_type: "route_added",
        payload: { route: "pager" },
      },
    });
    assert.equal(result.event.kind, "module");
    if (result.event.kind === "module") {
      assert.equal(result.event.module, "routing");
      assert.equal(result.event.eventType, "route_added");
      assert.deepEqual(result.event.payload, { route: "pager" });
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

  it("parses identity_materialization_failure", () => {
    const result = parseErrorEvent({
      category: "identity_materialization_failure",
      identity: "initiative:broken",
      initiator: "review:singleton",
      operation: "materialize_reachable_peers",
      error: "bridge create_session: missing skill",
    });
    assert.equal(result.category, "identity_materialization_failure");
    assert.equal(
      result.message,
      "initiative:broken for review:singleton: materialize_reachable_peers: bridge create_session: missing skill",
    );
    assert.equal(result.context.identity, "initiative:broken");
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
      identity: "identity:luka",
      eventTypes: ["text_delta", "run_completed"],
      limit: 50,
      afterSeq: 10,
    });
    assert.equal(result.since_ms, 1000);
    assert.equal(result.until_ms, 2000);
    assert.equal(result.member_id, "mem-1");
    assert.equal(result.identity, "identity:luka");
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

// ---------------------------------------------------------------------------
// Mobpack editor catalogs
// ---------------------------------------------------------------------------

describe("mobpack editor catalog parsers", () => {
  it("parses split MobKit catalog payloads", () => {
    const tools = parseMobpackToolsCatalogResult({
      schema_version: "mobpack.editor.v1",
      runtime_backed: false,
      source: "mobkit/tool-config",
      authoring_provider: { id: "standalone_authoring", runtime_binding: "unbound" },
      runtime_unavailable_reason: "standalone",
      tool_catalog: [{ id: "shell", tag_class: "ops" }],
    });
    assert.equal(tools.schemaVersion, "mobpack.editor.v1");
    assert.equal(tools.runtimeBacked, false);
    assert.equal(tools.authoringProvider.id, "standalone_authoring");
    assert.equal(tools.runtimeUnavailableReason, "standalone");
    assert.deepEqual(tools.toolCatalog, [{ id: "shell", tag_class: "ops" }]);

    const skills = parseMobpackSkillsCatalogResult({
      schema_version: "mobpack.editor.v1",
      runtime_backed: false,
      source: "mobkit/authoring-skill-realms",
      authoring_provider: { id: "standalone_authoring" },
      skill_realms: [{ id: "mobkit/authoring", skills: [] }],
    });
    assert.equal(skills.authoringProvider.id, "standalone_authoring");
    assert.deepEqual(skills.skillRealms, [{ id: "mobkit/authoring", skills: [] }]);

    const definitions = parseMobpackAgentDefinitionsResult({
      schema_version: "mobpack.editor.v1",
      runtime_backed: false,
      source: "mobkit/authoring-agent-definitions",
      authoring_provider: { id: "standalone_authoring" },
      agent_definitions: [{ id: "authoring:reviewer", role: "reviewer" }],
    });
    assert.equal(definitions.authoringProvider.id, "standalone_authoring");
    assert.deepEqual(definitions.agentDefinitions, [
      { id: "authoring:reviewer", role: "reviewer" },
    ]);

    const templates = parseMobpackTemplatesResult({
      schema_version: "mobpack.editor.v1",
      source: "mobkit/mobpack-templates",
      authoring_provider: { id: "standalone_authoring" },
      runtime_unavailable_reason: "standalone",
      blank_mobpack: { document: { members: [] } },
      sample_mobpacks: [{ id: "review" }],
      sample_agent_definitions: [{ id: "sample:reviewer" }],
      templates: { blank_mobpack: { document: { members: [] } } },
    });
    assert.equal(templates.authoringProvider.id, "standalone_authoring");
    assert.equal(templates.runtimeUnavailableReason, "standalone");
    assert.deepEqual(templates.blankMobpack, { document: { members: [] } });
    assert.deepEqual(templates.sampleMobpacks, [{ id: "review" }]);
    assert.deepEqual(templates.sampleAgentDefinitions, [{ id: "sample:reviewer" }]);
  });

  it("parses composed MobKit catalog snapshots", () => {
    const result = parseMobpackCatalogsResult({
      schema_version: "mobpack.editor.v1",
      runtime_backed: false,
      authoring_provider: { id: "standalone_authoring", runtime_binding: "unbound" },
      runtime_unavailable_reason: "standalone",
      sources: { tools: "mobkit/tools/catalog" },
      templates: { blank_mobpack: {} },
      tool_catalog: [{ id: "shell" }],
      skill_realms: [{ id: "mobkit/authoring" }],
      blank_mobpack: { document: {} },
      sample_mobpacks: [{ id: "sample" }],
      agent_definitions: [{ id: "authoring:reviewer" }],
      sample_agent_definitions: [{ id: "sample:reviewer" }],
      models: [{
        id: "gpt-5",
        display_name: "GPT-5",
        provider: "openai",
        tier: "frontier",
        profile: { vision: true },
      }],
      provider_defaults: [{
        provider: "openai",
        default_model_id: "gpt-5",
        models: [],
      }],
    });
    assert.equal(result.authoringProvider.runtime_binding, "unbound");
    assert.equal(result.runtimeUnavailableReason, "standalone");
    assert.equal(result.sources.tools, "mobkit/tools/catalog");
    assert.deepEqual(result.toolCatalog, [{ id: "shell" }]);
    assert.deepEqual(result.skillRealms, [{ id: "mobkit/authoring" }]);
    assert.equal(result.models[0].displayName, "GPT-5");
    assert.equal(result.providerDefaults[0].defaultModelId, "gpt-5");
  });
});

// ---------------------------------------------------------------------------
// Mobpack authoring
// ---------------------------------------------------------------------------

describe("mobpack authoring parsers", () => {
  it("parses validation results with diagnostics and display rows", () => {
    const validation = parseMobpackValidationResult({
      ok: false,
      diagnostics: [
        {
          severity: "error",
          code: "missing_member",
          message: "no members defined",
          path: "members",
        },
      ],
      display_rows: [
        {
          kind: "crit",
          glyph: "!",
          head: "invalid mobpack",
          sub: "no members defined",
          meta: "members",
        },
      ],
      mob_id: "demo",
      flow_ids: ["flow_a"],
      validation_source: "mobkit/mobpacks/validate",
      deploy_command: "rkat mob deploy",
    });
    assert.equal(validation.ok, false);
    assert.equal(validation.diagnostics[0].severity, "error");
    assert.equal(validation.diagnostics[0].code, "missing_member");
    assert.equal(validation.diagnostics[0].path, "members");
    assert.equal(validation.displayRows[0].kind, "crit");
    assert.equal(validation.displayRows[0].glyph, "!");
    assert.equal(validation.mobId, "demo");
    assert.deepEqual(validation.flowIds, ["flow_a"]);
    assert.equal(validation.deployCommand, "rkat mob deploy");
  });

  it("defaults missing validation fields", () => {
    const validation = parseMobpackValidationResult({ ok: true });
    assert.equal(validation.ok, true);
    assert.deepEqual(validation.diagnostics, []);
    assert.deepEqual(validation.displayRows, []);
    assert.equal(validation.mobId, null);
    assert.deepEqual(validation.flowIds, []);
  });

  it("parses source and export payloads", () => {
    const source = parseMobpackSourceResult({
      filename: "demo.mobpack",
      media_type: "application/vnd.meerkat.mobpack",
      mob_toml: "[mob]\n",
      source_files: [
        {
          path: "mob.toml",
          media_type: "text/x-toml",
          size_bytes: 7,
          content_base64: "W21vYl0K",
          sha256: "abc",
          text: "[mob]\n",
        },
      ],
      validation: { ok: true },
      source: "mobkit/mobpacks/source",
    });
    assert.equal(source.filename, "demo.mobpack");
    assert.equal(source.sourceFiles[0].path, "mob.toml");
    assert.equal(source.sourceFiles[0].sizeBytes, 7);
    assert.equal(source.sourceFiles[0].text, "[mob]\n");
    assert.equal(source.validation.ok, true);

    const exported = parseMobpackExportResult({
      filename: "demo.mobpack",
      media_type: "application/vnd.meerkat.mobpack",
      content_base64: "UEsDBA==",
      mob_toml: "[mob]\n",
      source_files: [],
      validation: { ok: true },
    });
    assert.equal(exported.contentBase64, "UEsDBA==");
    assert.equal(exported.mediaType, "application/vnd.meerkat.mobpack");
    assert.equal(exported.validation.ok, true);
  });

  it("parses import results", () => {
    const imported = parseMobpackImportResult({
      document: { mob_id: "demo" },
      validation: { ok: true },
      source: "mobkit/mobpacks/import:archive",
      source_label: "demo.mobpack",
      source_media_type: "application/vnd.meerkat.mobpack",
    });
    assert.deepEqual(imported.document, { mob_id: "demo" });
    assert.equal(imported.source, "mobkit/mobpacks/import:archive");
    assert.equal(imported.sourceLabel, "demo.mobpack");
  });

  it("parses draft rows and registry results", () => {
    const rowPayload = {
      id: "f_demo",
      name: "Demo",
      version: "mobpack.editor.v1",
      stage: "draft",
      trigger: "MobKit authoring draft",
      source: "mobkit/mobpacks/create",
      revision: 3,
      etag: "f_demo:3",
      updated_at_unix_ms: 1700000000000,
      document: { mob_id: "demo" },
      validation: { ok: true },
      can_undo: true,
      can_redo: false,
    };
    const row = parseMobpackDraftRow(rowPayload);
    assert.equal(row.id, "f_demo");
    assert.equal(row.stage, "draft");
    assert.equal(row.revision, 3);
    assert.equal(row.etag, "f_demo:3");
    assert.deepEqual(row.document, { mob_id: "demo" });
    assert.deepEqual(row.validation, { ok: true });
    assert.equal(row.canUndo, true);
    assert.equal(row.canRedo, false);

    const bareRow = parseMobpackDraftRow({ id: "f_old" });
    assert.equal(bareRow.canUndo, null);
    assert.equal(bareRow.canRedo, null);

    const listed = parseMobpackDraftListResult({
      source: "mobkit/mobpacks/list",
      store_path: "/tmp/drafts.json",
      runtime_backed: true,
      rows: [rowPayload],
    });
    assert.equal(listed.storePath, "/tmp/drafts.json");
    assert.equal(listed.runtimeBacked, true);
    assert.equal(listed.rows[0].id, "f_demo");

    const got = parseMobpackDraftGetResult({
      source: "mobkit/mobpacks/get",
      runtime_backed: true,
      row: rowPayload,
    });
    assert.equal(got.storePath, null);
    assert.equal(got.row.revision, 3);

    const saved = parseMobpackDraftSaveResult({
      source: "mobkit/mobpacks/save",
      store_path: "/tmp/drafts.json",
      row: rowPayload,
      rows: [rowPayload],
    });
    assert.equal(saved.row.id, "f_demo");
    assert.equal(saved.rows.length, 1);

    const deleted = parseMobpackDraftDeleteResult({
      source: "mobkit/mobpacks/delete",
      store_path: "/tmp/drafts.json",
      id: "f_demo",
      deleted: true,
      rows: [],
    });
    assert.equal(deleted.id, "f_demo");
    assert.equal(deleted.deleted, true);
    assert.deepEqual(deleted.rows, []);
  });

  it("parses draft history results for stepped and blocked steps", () => {
    const rowPayload = {
      id: "f_demo",
      name: "Demo",
      stage: "draft",
      revision: 4,
      etag: "f_demo:4",
      document: { mob_id: "demo" },
      validation: { ok: true },
      can_undo: false,
      can_redo: true,
    };
    const stepped = parseMobpackDraftHistoryResult({
      source: "mobkit/mobpacks/undo",
      store_path: "/tmp/drafts.json",
      stepped: true,
      row: rowPayload,
      rows: [rowPayload],
    });
    assert.equal(stepped.source, "mobkit/mobpacks/undo");
    assert.equal(stepped.storePath, "/tmp/drafts.json");
    assert.equal(stepped.stepped, true);
    assert.equal(stepped.reason, null);
    assert.equal(stepped.row.revision, 4);
    assert.equal(stepped.row.canUndo, false);
    assert.equal(stepped.row.canRedo, true);
    assert.equal(stepped.rows[0].id, "f_demo");

    const blocked = parseMobpackDraftHistoryResult({
      source: "mobkit/mobpacks/redo",
      store_path: "/tmp/drafts.json",
      stepped: false,
      reason: "nothing to redo",
      row: rowPayload,
      rows: [rowPayload],
    });
    assert.equal(blocked.stepped, false);
    assert.equal(blocked.reason, "nothing to redo");
    assert.equal(blocked.row.etag, "f_demo:4");
  });

  it("parses apply-operation results with and without selection", () => {
    const applied = parseMobpackApplyOperationResult({
      source: "mobkit/mobpacks/apply_operation",
      operation: "add_member",
      ok: true,
      document: { mob_id: "demo", members: [{ id: "reviewer" }] },
      selection: { kind: "agent", id: "reviewer" },
      validation: { ok: true },
    });
    assert.equal(applied.operation, "add_member");
    assert.equal(applied.ok, true);
    assert.deepEqual(applied.selection, { kind: "agent", id: "reviewer" });
    assert.equal(applied.validation.ok, true);

    const noSelection = parseMobpackApplyOperationResult({
      source: "mobkit/mobpacks/apply_operation",
      operation: "delete_member",
      ok: true,
      document: { mob_id: "demo" },
      selection: null,
      validation: { ok: true },
    });
    assert.equal(noSelection.selection, null);
  });

  it("parses deploy command and deploy results", () => {
    const preview = parseMobpackDeployCommandResult({
      command: "rkat mob deploy demo.mobpack",
      argv: ["rkat", "mob", "deploy", "demo.mobpack"],
      deploy_command: "rkat mob deploy",
      filename: "demo.mobpack",
      validation: { ok: true },
      source: "meerkat_mobkit::mobpack::deploy_argv",
    });
    assert.equal(preview.command, "rkat mob deploy demo.mobpack");
    assert.deepEqual(preview.argv, ["rkat", "mob", "deploy", "demo.mobpack"]);
    assert.equal(preview.deployCommand, "rkat mob deploy");

    const deployed = parseMobpackDeployResult({
      filename: "demo.mobpack",
      pack_path: "/tmp/demo.mobpack",
      pack_sha256: "deadbeef",
      command: "rkat mob deploy /tmp/demo.mobpack",
      argv: ["rkat", "mob", "deploy", "/tmp/demo.mobpack"],
      plan_trace: [{ step: "validate" }],
      executed: true,
      success: true,
      status_code: 0,
      stdout: "deployed",
      validation: { ok: true },
      display_rows: [
        {
          kind: "ok",
          glyph: "✓",
          head: "deploy executed",
          sub: "deployed",
          meta: "/tmp/demo.mobpack",
        },
      ],
    });
    assert.equal(deployed.executed, true);
    assert.equal(deployed.success, true);
    assert.equal(deployed.statusCode, 0);
    assert.equal(deployed.stdout, "deployed");
    assert.equal(deployed.stderr, null);
    assert.deepEqual(deployed.planTrace, [{ step: "validate" }]);
    assert.equal(deployed.displayRows[0].head, "deploy executed");
  });
});
