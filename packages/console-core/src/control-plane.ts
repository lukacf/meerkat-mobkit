import type { ConversationRichToolCallBlock } from "./rich-content";

export type ExperienceSectionRefresh =
  | { mode: "poll"; interval_ms: number }
  | { mode: "stream"; topic: string; update_semantics: "full_snapshot" | "append" };

export interface ExperienceSectionMeta {
  schema_version: string;
  refresh: ExperienceSectionRefresh;
  capabilities?: string[];
}

export interface ConsoleInteractionRequest {
  identity: string;
  content: string;
  origin: string;
}

export interface ConsoleInteractionAccepted {
  interaction_id: string;
  identity: string;
}

export interface IdentityStreamRequest {
  identity: string;
}

export interface ConsoleIdentityEventEnvelope {
  event_id: string;
  interaction_id?: string;
  identity: string;
  event_type: string;
  timestamp_ms: number;
  data: unknown;
}

export interface IdentityStatusRow {
  identity: string;
  display_name?: string;
  profile?: string;
  state: string;
  addressability: "addressable" | "internal_only";
  labels: Record<string, string>;
  generation?: number;
  checkpoint_version?: number;
  lease_healthy?: boolean;
}

export type ConsoleDockTargetAddressingMode = "identity" | "member";

export interface IdentityInspectViewState extends IdentityStatusRow {
  continuity: {
    generation?: number;
    checkpoint_version?: number;
    session_id?: string;
    agent_runtime_id?: string;
  };
  lease?: {
    fencing_token: number;
    ttl_remaining_ms: number;
    healthy: boolean;
  } | null;
  output_preview?: string | null;
  is_final?: boolean | null;
  peer_reachable_count?: number | null;
  topology_peers?: string[];
  recent_tool_calls?: ConversationRichToolCallBlock[];
  last_activity_ms?: number | null;
}

export interface SidebarWatchFields {
  watched?: boolean;
  alertLevel?: "elevated" | "critical" | null;
  degraded?: boolean;
  degradedReason?: string;
}

export interface ActivityFilterPreset {
  id: string;
  label: string;
  watchedOnly?: boolean;
  alertLevels?: Array<"elevated" | "critical">;
  eventTypeFilter?: string[];
}

export type ResponsePhase = "waiting" | "tool-executing" | "generating" | null;

export interface GatingActionResult {
  pending_id: string;
  action_id: string;
  approver_id: string;
  decision: "approve" | "reject" | "escalate";
  outcome: "allowed" | "safe_draft" | "pending_approval";
  decided_at_ms: number;
  reason?: string;
  next_pending_id?: string;
}

export interface RoutingSectionView {
  routes: Array<{
    route_key: string;
    recipient: string;
    channel?: string;
    sink: string;
    target_module: string;
    retry_max?: number;
    backoff_ms?: number;
    rate_limit_per_minute?: number;
  }>;
  deliveries: Array<{
    delivery_id: string;
    route_id: string;
    recipient: string;
    sink: string;
    target_module: string;
    status: string;
    first_attempt_ms: number;
    final_attempt_ms: number;
    idempotency_key?: string;
    sink_adapter?: string;
    attempts: Array<{
      attempt: number;
      status: string;
      backoff_ms: number;
    }>;
  }>;
}

export interface GatingActionRequest {
  pending_id: string;
  approver_id: string;
  decision: "approve" | "reject" | "escalate";
  reason?: string;
}

export interface ReplayUnavailableError {
  error: "replay_unavailable";
  stream: "identity" | "all_events";
  requested_last_event_id: string;
  latest_event_id: string;
}

export interface ConsoleInteractionRejectedError {
  code: -32001 | -32002 | -32003 | -32004 | -32602 | -32603;
  message: string;
}

export interface ToolCallAccumulatorState {
  toolCalls: Record<string, ConversationRichToolCallBlock>;
  pendingResults: Record<string, string>;
  timeoutMs: number;
}

function trimString(value: unknown): string | undefined {
  if (typeof value !== "string") {
    return undefined;
  }
  const trimmed = value.trim();
  return trimmed || undefined;
}

function stringRecord(value: unknown): Record<string, string> {
  if (!value || typeof value !== "object") {
    return {};
  }

  return Object.fromEntries(
    Object.entries(value as Record<string, unknown>)
      .map(([key, raw]) => {
        const normalizedKey = trimString(key);
        const normalizedValue = trimString(raw);
        return normalizedKey && normalizedValue ? [normalizedKey, normalizedValue] : null;
      })
      .filter((entry): entry is [string, string] => Boolean(entry)),
  );
}

export function normalizeResponsePhase(value: unknown): ResponsePhase {
  switch (value) {
    case "waiting":
    case "tool-executing":
    case "generating":
      return value;
    case null:
    case undefined:
      return null;
    default:
      return null;
  }
}

function normalizeFiniteNumber(value: unknown): number | undefined {
  return typeof value === "number" && Number.isFinite(value) ? value : undefined;
}

function normalizeStringArray(value: unknown): string[] | undefined {
  if (!Array.isArray(value)) {
    return undefined;
  }
  const normalized = Array.from(new Set(value.map(trimString).filter((entry): entry is string => Boolean(entry))));
  return normalized.length > 0 ? normalized : undefined;
}

export function normalizeSidebarWatchFields(value: unknown): SidebarWatchFields {
  const record = value && typeof value === "object" ? value as Record<string, unknown> : {};
  const normalized: SidebarWatchFields = {};

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

export function normalizeActivityFilterPreset(value: unknown): ActivityFilterPreset | null {
  const record = value && typeof value === "object" ? value as Record<string, unknown> : null;
  if (!record) {
    return null;
  }

  const id = trimString(record.id);
  const label = trimString(record.label);
  if (!id || !label) {
    return null;
  }

  const alertLevels = Array.isArray(record.alertLevels)
    ? Array.from(new Set(record.alertLevels.filter((level): level is "elevated" | "critical" =>
      level === "elevated" || level === "critical")))
    : undefined;
  const eventTypeFilter = Array.isArray(record.eventTypeFilter)
    ? Array.from(new Set(record.eventTypeFilter.map(trimString).filter((entry): entry is string => Boolean(entry))))
    : undefined;

  return {
    id,
    label,
    ...(typeof record.watchedOnly === "boolean" ? { watchedOnly: record.watchedOnly } : {}),
    ...(alertLevels?.length ? { alertLevels } : {}),
    ...(eventTypeFilter?.length ? { eventTypeFilter } : {}),
  };
}

export function normalizeConsoleDockTargetAddressingMode(value: unknown): ConsoleDockTargetAddressingMode {
  return value === "identity" ? "identity" : "member";
}

export function normalizeConsoleInteractionRequest(value: unknown): ConsoleInteractionRequest | null {
  const record = value && typeof value === "object" ? value as Record<string, unknown> : null;
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

export function normalizeConsoleInteractionAccepted(value: unknown): ConsoleInteractionAccepted | null {
  const record = value && typeof value === "object" ? value as Record<string, unknown> : null;
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

export function normalizeIdentityStreamRequest(value: unknown): IdentityStreamRequest | null {
  const record = value && typeof value === "object" ? value as Record<string, unknown> : null;
  if (!record) {
    return null;
  }
  const identity = trimString(record.identity);
  return identity ? { identity } : null;
}

export function normalizeConsoleIdentityEventEnvelope(value: unknown): ConsoleIdentityEventEnvelope | null {
  const record = value && typeof value === "object" ? value as Record<string, unknown> : null;
  if (!record) {
    return null;
  }
  const eventId = trimString(record.event_id);
  const identity = trimString(record.identity);
  const eventType = trimString(record.event_type);
  const timestamp = normalizeFiniteNumber(record.timestamp_ms);
  if (!eventId || !identity || !eventType || timestamp === undefined) {
    return null;
  }
  return {
    event_id: eventId,
    identity,
    event_type: eventType,
    timestamp_ms: timestamp,
    data: "data" in record ? record.data : null,
    ...(trimString(record.interaction_id) ? { interaction_id: trimString(record.interaction_id) } : {}),
  };
}

export function normalizeExperienceSectionMeta(value: unknown): ExperienceSectionMeta | null {
  const record = value && typeof value === "object" ? value as Record<string, unknown> : null;
  if (!record) {
    return null;
  }

  const schemaVersion = trimString(record.schema_version);
  const refresh = record.refresh && typeof record.refresh === "object"
    ? record.refresh as Record<string, unknown>
    : null;
  if (!schemaVersion || !refresh) {
    return null;
  }

  if (refresh.mode === "poll" && typeof refresh.interval_ms === "number" && Number.isFinite(refresh.interval_ms) && refresh.interval_ms > 0) {
    const capabilities = Array.isArray(record.capabilities)
      ? Array.from(new Set(record.capabilities.map(trimString).filter((entry): entry is string => Boolean(entry))))
      : undefined;
    return {
      schema_version: schemaVersion,
      refresh: { mode: "poll", interval_ms: refresh.interval_ms },
      ...(capabilities?.length ? { capabilities } : {}),
    };
  }

  if (
    refresh.mode === "stream"
    && (refresh.update_semantics === "full_snapshot" || refresh.update_semantics === "append")
  ) {
    const topic = trimString(refresh.topic);
    if (!topic) {
      return null;
    }
    const capabilities = Array.isArray(record.capabilities)
      ? Array.from(new Set(record.capabilities.map(trimString).filter((entry): entry is string => Boolean(entry))))
      : undefined;
    return {
      schema_version: schemaVersion,
      refresh: {
        mode: "stream",
        topic,
        update_semantics: refresh.update_semantics,
      },
      ...(capabilities?.length ? { capabilities } : {}),
    };
  }

  return null;
}

export function normalizeIdentityStatusRow(value: unknown): IdentityStatusRow | null {
  const record = value && typeof value === "object" ? value as Record<string, unknown> : null;
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
    ...(trimString(record.display_name) ? { display_name: trimString(record.display_name) } : {}),
    ...(trimString(record.profile) ? { profile: trimString(record.profile) } : {}),
    ...(typeof record.generation === "number" && Number.isFinite(record.generation) ? { generation: record.generation } : {}),
    ...(typeof record.checkpoint_version === "number" && Number.isFinite(record.checkpoint_version)
      ? { checkpoint_version: record.checkpoint_version }
      : {}),
    ...(typeof record.lease_healthy === "boolean" ? { lease_healthy: record.lease_healthy } : {}),
  };
}

export function normalizeIdentityInspectViewState(value: unknown): IdentityInspectViewState | null {
  const record = value && typeof value === "object" ? value as Record<string, unknown> : null;
  const statusRow = normalizeIdentityStatusRow(value);
  if (!record || !statusRow) {
    return null;
  }

  const continuityRecord = record.continuity && typeof record.continuity === "object"
    ? record.continuity as Record<string, unknown>
    : {};
  const leaseRecord = record.lease && typeof record.lease === "object"
    ? record.lease as Record<string, unknown>
    : record.lease === null
      ? null
      : undefined;

  return {
    ...statusRow,
    continuity: {
      ...(normalizeFiniteNumber(continuityRecord.generation) !== undefined
        ? { generation: normalizeFiniteNumber(continuityRecord.generation) }
        : {}),
      ...(normalizeFiniteNumber(continuityRecord.checkpoint_version) !== undefined
        ? { checkpoint_version: normalizeFiniteNumber(continuityRecord.checkpoint_version) }
        : {}),
      ...(trimString(continuityRecord.session_id) ? { session_id: trimString(continuityRecord.session_id) } : {}),
      ...(trimString(continuityRecord.agent_runtime_id) ? { agent_runtime_id: trimString(continuityRecord.agent_runtime_id) } : {}),
    },
    ...(leaseRecord === null
      ? { lease: null }
      : leaseRecord
        && normalizeFiniteNumber(leaseRecord.fencing_token) !== undefined
        && normalizeFiniteNumber(leaseRecord.ttl_remaining_ms) !== undefined
        && typeof leaseRecord.healthy === "boolean"
          ? {
              lease: {
                fencing_token: normalizeFiniteNumber(leaseRecord.fencing_token)!,
                ttl_remaining_ms: normalizeFiniteNumber(leaseRecord.ttl_remaining_ms)!,
                healthy: leaseRecord.healthy,
              },
            }
          : {}),
    ...(trimString(record.output_preview) !== undefined ? { output_preview: trimString(record.output_preview) ?? null } : {}),
    ...(typeof record.is_final === "boolean" || record.is_final === null ? { is_final: record.is_final as boolean | null } : {}),
    ...(normalizeFiniteNumber(record.peer_reachable_count) !== undefined
      ? { peer_reachable_count: normalizeFiniteNumber(record.peer_reachable_count) }
      : record.peer_reachable_count === null
        ? { peer_reachable_count: null }
        : {}),
    ...(normalizeStringArray(record.topology_peers) ? { topology_peers: normalizeStringArray(record.topology_peers) } : {}),
    ...(Array.isArray(record.recent_tool_calls) ? { recent_tool_calls: record.recent_tool_calls as ConversationRichToolCallBlock[] } : {}),
    ...(normalizeFiniteNumber(record.last_activity_ms) !== undefined
      ? { last_activity_ms: normalizeFiniteNumber(record.last_activity_ms) }
      : record.last_activity_ms === null
        ? { last_activity_ms: null }
        : {}),
  };
}

export function normalizeGatingActionRequest(value: unknown): GatingActionRequest | null {
  const record = value && typeof value === "object" ? value as Record<string, unknown> : null;
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
    ...(trimString(record.reason) ? { reason: trimString(record.reason) } : {}),
  };
}

export function normalizeGatingActionResult(value: unknown): GatingActionResult | null {
  const record = value && typeof value === "object" ? value as Record<string, unknown> : null;
  if (!record) {
    return null;
  }
  const pendingId = trimString(record.pending_id);
  const actionId = trimString(record.action_id);
  const approverId = trimString(record.approver_id);
  const decidedAt = normalizeFiniteNumber(record.decided_at_ms);
  if (!pendingId || !actionId || !approverId || decidedAt === undefined) {
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
    ...(trimString(record.reason) ? { reason: trimString(record.reason) } : {}),
    ...(trimString(record.next_pending_id) ? { next_pending_id: trimString(record.next_pending_id) } : {}),
  };
}

export function normalizeRoutingSectionView(value: unknown): RoutingSectionView | null {
  const record = value && typeof value === "object" ? value as Record<string, unknown> : null;
  if (!record) {
    return null;
  }
  const routes = Array.isArray(record.routes)
    ? record.routes
      .map((entry) => {
        const route = entry && typeof entry === "object" ? entry as Record<string, unknown> : null;
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
          ...(trimString(route.channel) ? { channel: trimString(route.channel) } : {}),
          ...(normalizeFiniteNumber(route.retry_max) !== undefined ? { retry_max: normalizeFiniteNumber(route.retry_max) } : {}),
          ...(normalizeFiniteNumber(route.backoff_ms) !== undefined ? { backoff_ms: normalizeFiniteNumber(route.backoff_ms) } : {}),
          ...(normalizeFiniteNumber(route.rate_limit_per_minute) !== undefined
            ? { rate_limit_per_minute: normalizeFiniteNumber(route.rate_limit_per_minute) }
            : {}),
        };
      })
      .filter((entry): entry is RoutingSectionView["routes"][number] => Boolean(entry))
    : [];
  const deliveries = Array.isArray(record.deliveries)
    ? record.deliveries
      .map((entry) => {
        const delivery = entry && typeof entry === "object" ? entry as Record<string, unknown> : null;
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
        if (!deliveryId || !routeId || !recipient || !sink || !targetModule || !status || firstAttempt === undefined || finalAttempt === undefined) {
          return null;
        }
        const attempts = Array.isArray(delivery.attempts)
          ? delivery.attempts
            .map((attemptRaw) => {
              const attempt = attemptRaw && typeof attemptRaw === "object" ? attemptRaw as Record<string, unknown> : null;
              if (!attempt) {
                return null;
              }
              const attemptNumber = normalizeFiniteNumber(attempt.attempt);
              const attemptStatus = trimString(attempt.status);
              const backoff = normalizeFiniteNumber(attempt.backoff_ms);
              if (attemptNumber === undefined || !attemptStatus || backoff === undefined) {
                return null;
              }
              return {
                attempt: attemptNumber,
                status: attemptStatus,
                backoff_ms: backoff,
              };
            })
            .filter((attempt): attempt is RoutingSectionView["deliveries"][number]["attempts"][number] => Boolean(attempt))
          : [];
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
          ...(trimString(delivery.idempotency_key) ? { idempotency_key: trimString(delivery.idempotency_key) } : {}),
          ...(trimString(delivery.sink_adapter) ? { sink_adapter: trimString(delivery.sink_adapter) } : {}),
        };
      })
      .filter((entry): entry is RoutingSectionView["deliveries"][number] => Boolean(entry))
    : [];
  return { routes, deliveries };
}

export function normalizeReplayUnavailableError(value: unknown): ReplayUnavailableError | null {
  const record = value && typeof value === "object" ? value as Record<string, unknown> : null;
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
    latest_event_id: latest,
  };
}

export function normalizeConsoleInteractionRejectedError(value: unknown): ConsoleInteractionRejectedError | null {
  const record = value && typeof value === "object" ? value as Record<string, unknown> : null;
  if (!record) {
    return null;
  }
  const code = record.code;
  const message = trimString(record.message);
  if (
    code !== -32001
    && code !== -32002
    && code !== -32003
    && code !== -32004
    && code !== -32602
    && code !== -32603
  ) {
    return null;
  }
  if (!message) {
    return null;
  }
  return { code, message };
}

export function normalizeToolCallAccumulatorState(value: unknown): ToolCallAccumulatorState | null {
  const record = value && typeof value === "object" ? value as Record<string, unknown> : null;
  if (!record) {
    return null;
  }
  const timeoutMs = normalizeFiniteNumber(record.timeoutMs);
  if (timeoutMs === undefined || timeoutMs <= 0) {
    return null;
  }

  const toolCalls = record.toolCalls && typeof record.toolCalls === "object"
    ? Object.fromEntries(
        Object.entries(record.toolCalls as Record<string, unknown>)
          .map(([toolCallId, raw]) => {
            const normalizedId = trimString(toolCallId);
            const rawBlock = raw && typeof raw === "object" ? raw as Record<string, unknown> : null;
            if (!normalizedId || !rawBlock) {
              return null;
            }
            const name = trimString(rawBlock.name);
            const argumentsText = trimString(rawBlock.arguments);
            const status = rawBlock.status === "pending" || rawBlock.status === "success" || rawBlock.status === "error"
              ? rawBlock.status
              : null;
            if (rawBlock.type !== "tool-call" || !name || !argumentsText || !status) {
              return null;
            }
            return [
              normalizedId,
              {
                type: "tool-call" as const,
                toolCallId: normalizedId,
                name,
                arguments: argumentsText,
                ...(trimString(rawBlock.result) ? { result: trimString(rawBlock.result) } : {}),
                status,
              },
            ];
          })
          .filter((entry): entry is [string, ConversationRichToolCallBlock] => Boolean(entry)),
      )
    : {};

  const pendingResults = record.pendingResults && typeof record.pendingResults === "object"
    ? Object.fromEntries(
        Object.entries(record.pendingResults as Record<string, unknown>)
          .map(([toolCallId, result]) => {
            const normalizedId = trimString(toolCallId);
            const normalizedResult = trimString(result);
            return normalizedId && normalizedResult ? [normalizedId, normalizedResult] : null;
          })
          .filter((entry): entry is [string, string] => Boolean(entry)),
      )
    : {};

  return {
    toolCalls,
    pendingResults,
    timeoutMs,
  };
}
