// ../packages/console-core/src/control-plane.test.ts
import assert from "node:assert/strict";
import test from "node:test";

// ../packages/console-core/src/control-plane.ts
function trimString(value) {
  if (typeof value !== "string") {
    return void 0;
  }
  const trimmed = value.trim();
  return trimmed || void 0;
}
function stringRecord(value) {
  if (!value || typeof value !== "object") {
    return {};
  }
  return Object.fromEntries(
    Object.entries(value).map(([key, raw]) => {
      const normalizedKey = trimString(key);
      const normalizedValue = trimString(raw);
      return normalizedKey && normalizedValue ? [normalizedKey, normalizedValue] : null;
    }).filter((entry) => Boolean(entry))
  );
}
function normalizeResponsePhase(value) {
  switch (value) {
    case "waiting":
    case "tool-executing":
    case "generating":
      return value;
    case null:
    case void 0:
      return null;
    default:
      return null;
  }
}
function normalizeFiniteNumber(value) {
  return typeof value === "number" && Number.isFinite(value) ? value : void 0;
}
function normalizeStringArray(value) {
  if (!Array.isArray(value)) {
    return void 0;
  }
  const normalized = Array.from(new Set(value.map(trimString).filter((entry) => Boolean(entry))));
  return normalized.length > 0 ? normalized : void 0;
}
function normalizeSidebarWatchFields(value) {
  const record = value && typeof value === "object" ? value : {};
  const normalized = {};
  if (typeof record.watched === "boolean") {
    normalized.watched = record.watched;
  }
  if (record.alertLevel === "elevated" || record.alertLevel === "critical" || record.alertLevel === null) {
    normalized.alertLevel = record.alertLevel;
  }
  if (typeof record.degraded === "boolean") {
    normalized.degraded = record.degraded;
  }
  const degradedReason = trimString(record.degradedReason);
  if (degradedReason) {
    normalized.degradedReason = degradedReason;
  }
  return normalized;
}
function normalizeActivityFilterPreset(value) {
  const record = value && typeof value === "object" ? value : null;
  if (!record) {
    return null;
  }
  const id = trimString(record.id);
  const label = trimString(record.label);
  if (!id || !label) {
    return null;
  }
  const alertLevels = Array.isArray(record.alertLevels) ? Array.from(new Set(record.alertLevels.filter((level) => level === "elevated" || level === "critical"))) : void 0;
  const eventTypeFilter = Array.isArray(record.eventTypeFilter) ? Array.from(new Set(record.eventTypeFilter.map(trimString).filter((entry) => Boolean(entry)))) : void 0;
  return {
    id,
    label,
    ...typeof record.watchedOnly === "boolean" ? { watchedOnly: record.watchedOnly } : {},
    ...alertLevels?.length ? { alertLevels } : {},
    ...eventTypeFilter?.length ? { eventTypeFilter } : {}
  };
}
function normalizeConsoleDockTargetAddressingMode(value) {
  return value === "identity" ? "identity" : "member";
}
function normalizeConsoleInteractionRequest(value) {
  const record = value && typeof value === "object" ? value : null;
  if (!record) {
    return null;
  }
  const identity = trimString(record.identity);
  const content = trimString(record.content);
  const origin = trimString(record.origin);
  if (!identity || !content || !origin) {
    return null;
  }
  return { identity, content, origin };
}
function normalizeConsoleInteractionAccepted(value) {
  const record = value && typeof value === "object" ? value : null;
  if (!record) {
    return null;
  }
  const interactionId = trimString(record.interaction_id);
  const identity = trimString(record.identity);
  if (!interactionId || !identity) {
    return null;
  }
  return { interaction_id: interactionId, identity };
}
function normalizeIdentityStreamRequest(value) {
  const record = value && typeof value === "object" ? value : null;
  if (!record) {
    return null;
  }
  const identity = trimString(record.identity);
  return identity ? { identity } : null;
}
function normalizeConsoleIdentityEventEnvelope(value) {
  const record = value && typeof value === "object" ? value : null;
  if (!record) {
    return null;
  }
  const eventId = trimString(record.event_id);
  const identity = trimString(record.identity);
  const eventType = trimString(record.event_type);
  const timestamp = normalizeFiniteNumber(record.timestamp_ms);
  if (!eventId || !identity || !eventType || timestamp === void 0) {
    return null;
  }
  return {
    event_id: eventId,
    identity,
    event_type: eventType,
    timestamp_ms: timestamp,
    data: "data" in record ? record.data : null,
    ...trimString(record.interaction_id) ? { interaction_id: trimString(record.interaction_id) } : {}
  };
}
function normalizeExperienceSectionMeta(value) {
  const record = value && typeof value === "object" ? value : null;
  if (!record) {
    return null;
  }
  const schemaVersion = trimString(record.schema_version);
  const refresh = record.refresh && typeof record.refresh === "object" ? record.refresh : null;
  if (!schemaVersion || !refresh) {
    return null;
  }
  if (refresh.mode === "poll" && typeof refresh.interval_ms === "number" && Number.isFinite(refresh.interval_ms) && refresh.interval_ms > 0) {
    const capabilities = Array.isArray(record.capabilities) ? Array.from(new Set(record.capabilities.map(trimString).filter((entry) => Boolean(entry)))) : void 0;
    return {
      schema_version: schemaVersion,
      refresh: { mode: "poll", interval_ms: refresh.interval_ms },
      ...capabilities?.length ? { capabilities } : {}
    };
  }
  if (refresh.mode === "stream" && (refresh.update_semantics === "full_snapshot" || refresh.update_semantics === "append")) {
    const topic = trimString(refresh.topic);
    if (!topic) {
      return null;
    }
    const capabilities = Array.isArray(record.capabilities) ? Array.from(new Set(record.capabilities.map(trimString).filter((entry) => Boolean(entry)))) : void 0;
    return {
      schema_version: schemaVersion,
      refresh: {
        mode: "stream",
        topic,
        update_semantics: refresh.update_semantics
      },
      ...capabilities?.length ? { capabilities } : {}
    };
  }
  return null;
}
function normalizeIdentityStatusRow(value) {
  const record = value && typeof value === "object" ? value : null;
  if (!record) {
    return null;
  }
  const identity = trimString(record.identity);
  const state = trimString(record.state);
  if (!identity || !state) {
    return null;
  }
  const addressability = record.addressability === "internal_only" ? "internal_only" : record.addressability === "addressable" ? "addressable" : null;
  if (!addressability) {
    return null;
  }
  return {
    identity,
    state,
    addressability,
    labels: stringRecord(record.labels),
    ...trimString(record.display_name) ? { display_name: trimString(record.display_name) } : {},
    ...trimString(record.profile) ? { profile: trimString(record.profile) } : {},
    ...typeof record.generation === "number" && Number.isFinite(record.generation) ? { generation: record.generation } : {},
    ...typeof record.checkpoint_version === "number" && Number.isFinite(record.checkpoint_version) ? { checkpoint_version: record.checkpoint_version } : {},
    ...typeof record.lease_healthy === "boolean" ? { lease_healthy: record.lease_healthy } : {}
  };
}
function normalizeIdentityInspectViewState(value) {
  const record = value && typeof value === "object" ? value : null;
  const statusRow = normalizeIdentityStatusRow(value);
  if (!record || !statusRow) {
    return null;
  }
  const continuityRecord = record.continuity && typeof record.continuity === "object" ? record.continuity : {};
  const leaseRecord = record.lease && typeof record.lease === "object" ? record.lease : record.lease === null ? null : void 0;
  return {
    ...statusRow,
    continuity: {
      ...normalizeFiniteNumber(continuityRecord.generation) !== void 0 ? { generation: normalizeFiniteNumber(continuityRecord.generation) } : {},
      ...normalizeFiniteNumber(continuityRecord.checkpoint_version) !== void 0 ? { checkpoint_version: normalizeFiniteNumber(continuityRecord.checkpoint_version) } : {},
      ...trimString(continuityRecord.session_id) ? { session_id: trimString(continuityRecord.session_id) } : {},
      ...trimString(continuityRecord.agent_runtime_id) ? { agent_runtime_id: trimString(continuityRecord.agent_runtime_id) } : {}
    },
    ...leaseRecord === null ? { lease: null } : leaseRecord && normalizeFiniteNumber(leaseRecord.fencing_token) !== void 0 && normalizeFiniteNumber(leaseRecord.ttl_remaining_ms) !== void 0 && typeof leaseRecord.healthy === "boolean" ? {
      lease: {
        fencing_token: normalizeFiniteNumber(leaseRecord.fencing_token),
        ttl_remaining_ms: normalizeFiniteNumber(leaseRecord.ttl_remaining_ms),
        healthy: leaseRecord.healthy
      }
    } : {},
    ...trimString(record.output_preview) !== void 0 ? { output_preview: trimString(record.output_preview) ?? null } : {},
    ...typeof record.is_final === "boolean" || record.is_final === null ? { is_final: record.is_final } : {},
    ...normalizeFiniteNumber(record.peer_reachable_count) !== void 0 ? { peer_reachable_count: normalizeFiniteNumber(record.peer_reachable_count) } : record.peer_reachable_count === null ? { peer_reachable_count: null } : {},
    ...normalizeStringArray(record.topology_peers) ? { topology_peers: normalizeStringArray(record.topology_peers) } : {},
    ...Array.isArray(record.recent_tool_calls) ? { recent_tool_calls: record.recent_tool_calls } : {},
    ...normalizeFiniteNumber(record.last_activity_ms) !== void 0 ? { last_activity_ms: normalizeFiniteNumber(record.last_activity_ms) } : record.last_activity_ms === null ? { last_activity_ms: null } : {}
  };
}
function normalizeGatingActionRequest(value) {
  const record = value && typeof value === "object" ? value : null;
  if (!record) {
    return null;
  }
  const pendingId = trimString(record.pending_id);
  const approverId = trimString(record.approver_id);
  if (!pendingId || !approverId) {
    return null;
  }
  if (record.decision !== "approve" && record.decision !== "reject" && record.decision !== "escalate") {
    return null;
  }
  return {
    pending_id: pendingId,
    approver_id: approverId,
    decision: record.decision,
    ...trimString(record.reason) ? { reason: trimString(record.reason) } : {}
  };
}
function normalizeGatingActionResult(value) {
  const record = value && typeof value === "object" ? value : null;
  if (!record) {
    return null;
  }
  const pendingId = trimString(record.pending_id);
  const actionId = trimString(record.action_id);
  const approverId = trimString(record.approver_id);
  const decidedAt = normalizeFiniteNumber(record.decided_at_ms);
  if (!pendingId || !actionId || !approverId || decidedAt === void 0) {
    return null;
  }
  if (record.decision !== "approve" && record.decision !== "reject" && record.decision !== "escalate") {
    return null;
  }
  if (record.outcome !== "allowed" && record.outcome !== "safe_draft" && record.outcome !== "pending_approval") {
    return null;
  }
  return {
    pending_id: pendingId,
    action_id: actionId,
    approver_id: approverId,
    decision: record.decision,
    outcome: record.outcome,
    decided_at_ms: decidedAt,
    ...trimString(record.reason) ? { reason: trimString(record.reason) } : {},
    ...trimString(record.next_pending_id) ? { next_pending_id: trimString(record.next_pending_id) } : {}
  };
}
function normalizeRoutingSectionView(value) {
  const record = value && typeof value === "object" ? value : null;
  if (!record) {
    return null;
  }
  const routes = Array.isArray(record.routes) ? record.routes.map((entry) => {
    const route = entry && typeof entry === "object" ? entry : null;
    if (!route) {
      return null;
    }
    const routeKey = trimString(route.route_key);
    const recipient = trimString(route.recipient);
    const sink = trimString(route.sink);
    const targetModule = trimString(route.target_module);
    if (!routeKey || !recipient || !sink || !targetModule) {
      return null;
    }
    return {
      route_key: routeKey,
      recipient,
      sink,
      target_module: targetModule,
      ...trimString(route.channel) ? { channel: trimString(route.channel) } : {},
      ...normalizeFiniteNumber(route.retry_max) !== void 0 ? { retry_max: normalizeFiniteNumber(route.retry_max) } : {},
      ...normalizeFiniteNumber(route.backoff_ms) !== void 0 ? { backoff_ms: normalizeFiniteNumber(route.backoff_ms) } : {},
      ...normalizeFiniteNumber(route.rate_limit_per_minute) !== void 0 ? { rate_limit_per_minute: normalizeFiniteNumber(route.rate_limit_per_minute) } : {}
    };
  }).filter((entry) => Boolean(entry)) : [];
  const deliveries = Array.isArray(record.deliveries) ? record.deliveries.map((entry) => {
    const delivery = entry && typeof entry === "object" ? entry : null;
    if (!delivery) {
      return null;
    }
    const deliveryId = trimString(delivery.delivery_id);
    const routeId = trimString(delivery.route_id);
    const recipient = trimString(delivery.recipient);
    const sink = trimString(delivery.sink);
    const targetModule = trimString(delivery.target_module);
    const status = trimString(delivery.status);
    const firstAttempt = normalizeFiniteNumber(delivery.first_attempt_ms);
    const finalAttempt = normalizeFiniteNumber(delivery.final_attempt_ms);
    if (!deliveryId || !routeId || !recipient || !sink || !targetModule || !status || firstAttempt === void 0 || finalAttempt === void 0) {
      return null;
    }
    const attempts = Array.isArray(delivery.attempts) ? delivery.attempts.map((attemptRaw) => {
      const attempt = attemptRaw && typeof attemptRaw === "object" ? attemptRaw : null;
      if (!attempt) {
        return null;
      }
      const attemptNumber = normalizeFiniteNumber(attempt.attempt);
      const attemptStatus = trimString(attempt.status);
      const backoff = normalizeFiniteNumber(attempt.backoff_ms);
      if (attemptNumber === void 0 || !attemptStatus || backoff === void 0) {
        return null;
      }
      return {
        attempt: attemptNumber,
        status: attemptStatus,
        backoff_ms: backoff
      };
    }).filter((attempt) => Boolean(attempt)) : [];
    return {
      delivery_id: deliveryId,
      route_id: routeId,
      recipient,
      sink,
      target_module: targetModule,
      status,
      first_attempt_ms: firstAttempt,
      final_attempt_ms: finalAttempt,
      attempts,
      ...trimString(delivery.idempotency_key) ? { idempotency_key: trimString(delivery.idempotency_key) } : {},
      ...trimString(delivery.sink_adapter) ? { sink_adapter: trimString(delivery.sink_adapter) } : {}
    };
  }).filter((entry) => Boolean(entry)) : [];
  return { routes, deliveries };
}
function normalizeReplayUnavailableError(value) {
  const record = value && typeof value === "object" ? value : null;
  if (!record || record.error !== "replay_unavailable") {
    return null;
  }
  const stream = record.stream === "identity" || record.stream === "all_events" ? record.stream : null;
  const requested = trimString(record.requested_last_event_id);
  const latest = trimString(record.latest_event_id);
  if (!stream || !requested || !latest) {
    return null;
  }
  return {
    error: "replay_unavailable",
    stream,
    requested_last_event_id: requested,
    latest_event_id: latest
  };
}
function normalizeConsoleInteractionRejectedError(value) {
  const record = value && typeof value === "object" ? value : null;
  if (!record) {
    return null;
  }
  const code = record.code;
  const message = trimString(record.message);
  if (code !== -32001 && code !== -32002 && code !== -32003 && code !== -32004 && code !== -32602 && code !== -32603) {
    return null;
  }
  if (!message) {
    return null;
  }
  return { code, message };
}
function normalizeToolCallAccumulatorState(value) {
  const record = value && typeof value === "object" ? value : null;
  if (!record) {
    return null;
  }
  const timeoutMs = normalizeFiniteNumber(record.timeoutMs);
  if (timeoutMs === void 0 || timeoutMs <= 0) {
    return null;
  }
  const toolCalls = record.toolCalls && typeof record.toolCalls === "object" ? Object.fromEntries(
    Object.entries(record.toolCalls).map(([toolCallId, raw]) => {
      const normalizedId = trimString(toolCallId);
      const rawBlock = raw && typeof raw === "object" ? raw : null;
      if (!normalizedId || !rawBlock) {
        return null;
      }
      const name = trimString(rawBlock.name);
      const argumentsText = trimString(rawBlock.arguments);
      const status = rawBlock.status === "pending" || rawBlock.status === "success" || rawBlock.status === "error" ? rawBlock.status : null;
      if (rawBlock.type !== "tool-call" || !name || !argumentsText || !status) {
        return null;
      }
      return [
        normalizedId,
        {
          type: "tool-call",
          toolCallId: normalizedId,
          name,
          arguments: argumentsText,
          ...trimString(rawBlock.result) ? { result: trimString(rawBlock.result) } : {},
          status
        }
      ];
    }).filter((entry) => Boolean(entry))
  ) : {};
  const pendingResults = record.pendingResults && typeof record.pendingResults === "object" ? Object.fromEntries(
    Object.entries(record.pendingResults).map(([toolCallId, result]) => {
      const normalizedId = trimString(toolCallId);
      const normalizedResult = trimString(result);
      return normalizedId && normalizedResult ? [normalizedId, normalizedResult] : null;
    }).filter((entry) => Boolean(entry))
  ) : {};
  return {
    toolCalls,
    pendingResults,
    timeoutMs
  };
}

// ../packages/console-core/src/sidebar.ts
function normalizeMeta(meta) {
  return (meta || []).filter((item) => Boolean(item?.label));
}
function normalizeActions(actions) {
  return (actions || []).filter((action) => Boolean(action?.id && action?.label));
}
function normalizeItems(items) {
  return (items || []).filter((item) => Boolean(item?.id && item?.title)).map((item) => ({
    ...item,
    ...normalizeSidebarWatchFields(item),
    meta: normalizeMeta(item.meta),
    actions: normalizeActions(item.actions)
  }));
}
function normalizeSections(sections) {
  return (sections || []).filter((section) => Boolean(section?.id && typeof section?.title === "string")).map((section) => ({
    ...section,
    meta: normalizeMeta(section.meta),
    actions: normalizeActions(section.actions),
    items: normalizeItems(section.items)
  })).filter((section) => {
    if (section.items.length > 0) {
      return true;
    }
    return Boolean(
      section.title || section.subtitle || section.iconName || section.actions.length || section.meta.length
    );
  });
}
function normalizeConsoleSidebarViewState(viewState) {
  const blocks = (viewState?.blocks || []).filter((block) => Boolean(block?.id && block?.kind)).map((block) => ({
    ...block,
    meta: normalizeMeta(block.meta),
    actions: normalizeActions(block.actions),
    sections: normalizeSections(block.sections)
  })).filter((block) => {
    if (block.kind === "action_strip") {
      return block.actions.length > 0;
    }
    if (block.sections.length > 0) {
      return true;
    }
    return Boolean(block.title || block.meta.length || block.actions.length);
  });
  return { blocks };
}

// ../packages/console-core/src/control-plane.test.ts
test("normalizeExperienceSectionMeta keeps valid poll and stream metadata", () => {
  assert.deepEqual(
    normalizeExperienceSectionMeta({
      schema_version: "1",
      refresh: { mode: "stream", topic: "identity:luka", update_semantics: "append" },
      capabilities: ["inspect", " inspect ", "", null]
    }),
    {
      schema_version: "1",
      refresh: { mode: "stream", topic: "identity:luka", update_semantics: "append" },
      capabilities: ["inspect"]
    }
  );
  assert.equal(
    normalizeExperienceSectionMeta({
      schema_version: "1",
      refresh: { mode: "poll", interval_ms: 0 }
    }),
    null
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
      lease_healthy: true
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
      lease_healthy: true
    }
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
      degradedReason: " lease_expired "
    }),
    {
      watched: true,
      alertLevel: "elevated",
      degraded: true,
      degradedReason: "lease_expired"
    }
  );
  assert.deepEqual(
    normalizeActivityFilterPreset({
      id: " watched-critical ",
      label: " Critical ",
      watchedOnly: true,
      alertLevels: ["critical", "invalid", "critical"],
      eventTypeFilter: ["tool_call", "", "tool_call", "interaction_failed"]
    }),
    {
      id: "watched-critical",
      label: "Critical",
      watchedOnly: true,
      alertLevels: ["critical"],
      eventTypeFilter: ["tool_call", "interaction_failed"]
    }
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
                meta: [{ id: "status", label: "elevated", tone: "warning" }]
              }
            ]
          }
        ]
      }
    ]
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
        agent_runtime_id: " runtime-1 "
      },
      lease: {
        fencing_token: 12,
        ttl_remaining_ms: 9e3,
        healthy: true
      },
      topology_peers: [" peer-a ", "", "peer-b"],
      is_final: false
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
        agent_runtime_id: "runtime-1"
      },
      lease: {
        fencing_token: 12,
        ttl_remaining_ms: 9e3,
        healthy: true
      },
      topology_peers: ["peer-a", "peer-b"],
      is_final: false
    }
  );
  assert.deepEqual(
    normalizeReplayUnavailableError({
      error: "replay_unavailable",
      stream: "identity",
      requested_last_event_id: "evt-1",
      latest_event_id: "evt-99"
    }),
    {
      error: "replay_unavailable",
      stream: "identity",
      requested_last_event_id: "evt-1",
      latest_event_id: "evt-99"
    }
  );
  assert.deepEqual(
    normalizeConsoleInteractionRejectedError({
      code: -32002,
      message: " identity not addressable "
    }),
    {
      code: -32002,
      message: "identity not addressable"
    }
  );
});
test("normalize gating and routing payloads for the shared control-plane contract", () => {
  assert.deepEqual(
    normalizeGatingActionRequest({
      pending_id: " pending-1 ",
      approver_id: " luka ",
      decision: "escalate",
      reason: " needs higher approval "
    }),
    {
      pending_id: "pending-1",
      approver_id: "luka",
      decision: "escalate",
      reason: "needs higher approval"
    }
  );
  assert.deepEqual(
    normalizeGatingActionResult({
      pending_id: "pending-1",
      action_id: "action-1",
      approver_id: "luka",
      decision: "escalate",
      outcome: "pending_approval",
      decided_at_ms: 1717171717,
      next_pending_id: " pending-2 "
    }),
    {
      pending_id: "pending-1",
      action_id: "action-1",
      approver_id: "luka",
      decision: "escalate",
      outcome: "pending_approval",
      decided_at_ms: 1717171717,
      next_pending_id: "pending-2"
    }
  );
  assert.deepEqual(
    normalizeRoutingSectionView({
      routes: [
        {
          route_key: " route:1 ",
          recipient: " ops ",
          sink: "slack",
          target_module: "routing",
          channel: " approvals "
        }
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
            { attempt: 1, status: "sent", backoff_ms: 0 }
          ]
        }
      ]
    }),
    {
      routes: [
        {
          route_key: "route:1",
          recipient: "ops",
          sink: "slack",
          target_module: "routing",
          channel: "approvals"
        }
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
            { attempt: 1, status: "sent", backoff_ms: 0 }
          ]
        }
      ]
    }
  );
});
test("normalize interaction request/acceptance, stream request, and event envelope", () => {
  assert.deepEqual(
    normalizeConsoleInteractionRequest({
      identity: " identity:luka ",
      content: " hello ",
      origin: " console:panel-1 "
    }),
    {
      identity: "identity:luka",
      content: "hello",
      origin: "console:panel-1"
    }
  );
  assert.deepEqual(
    normalizeConsoleInteractionAccepted({
      interaction_id: " turn-1 ",
      identity: " identity:luka "
    }),
    {
      interaction_id: "turn-1",
      identity: "identity:luka"
    }
  );
  assert.deepEqual(
    normalizeIdentityStreamRequest({ identity: " identity:luka " }),
    { identity: "identity:luka" }
  );
  assert.deepEqual(
    normalizeConsoleIdentityEventEnvelope({
      event_id: " evt-1 ",
      interaction_id: " turn-1 ",
      identity: " identity:luka ",
      event_type: " tool_call ",
      timestamp_ms: 12,
      data: { tool_call_id: "tool-1" }
    }),
    {
      event_id: "evt-1",
      interaction_id: "turn-1",
      identity: "identity:luka",
      event_type: "tool_call",
      timestamp_ms: 12,
      data: { tool_call_id: "tool-1" }
    }
  );
});
test("normalize tool-call accumulator state for pending and out-of-order results", () => {
  assert.deepEqual(
    normalizeToolCallAccumulatorState({
      timeoutMs: 6e4,
      toolCalls: {
        " tool-1 ": {
          type: "tool-call",
          name: "search",
          arguments: ' {"q":"test"} ',
          status: "pending"
        }
      },
      pendingResults: {
        " tool-2 ": " result before call "
      }
    }),
    {
      timeoutMs: 6e4,
      toolCalls: {
        "tool-1": {
          type: "tool-call",
          toolCallId: "tool-1",
          name: "search",
          arguments: '{"q":"test"}',
          status: "pending"
        }
      },
      pendingResults: {
        "tool-2": "result before call"
      }
    }
  );
});
