/**
 * Typed return models for MobKit SDK RPC methods.
 *
 * All interfaces use `readonly` fields with camelCase naming. Parse functions
 * convert from the wire protocol's snake_case representation.
 */

// -- Helpers (internal) ---------------------------------------------------

function asRecord(value: unknown): Record<string, unknown> {
  if (typeof value === "object" && value !== null) {
    return value as Record<string, unknown>;
  }
  return {};
}

function asStringArray(value: unknown): string[] {
  if (Array.isArray(value)) {
    return value.filter((v): v is string => typeof v === "string");
  }
  return [];
}

function asRecordArray(value: unknown): Record<string, unknown>[] {
  if (Array.isArray(value)) {
    return value.filter(
      (v): v is Record<string, unknown> => typeof v === "object" && v !== null,
    );
  }
  return [];
}

function asStringRecord(value: unknown): Record<string, string> {
  const raw = asRecord(value);
  const result: Record<string, string> = {};
  for (const [k, v] of Object.entries(raw)) {
    if (typeof v === "string") {
      result[k] = v;
    }
  }
  return result;
}

// -- Constants ------------------------------------------------------------

export const MEMBER_STATE_ACTIVE = "active" as const;
export const MEMBER_STATE_RETIRING = "retiring" as const;

// -- StatusResult ---------------------------------------------------------

export interface StatusResult {
  readonly contractVersion: string;
  readonly running: boolean;
  readonly loadedModules: readonly string[];
}

export function parseStatusResult(raw: unknown): StatusResult {
  const d = asRecord(raw);
  return {
    contractVersion: String(d.contract_version ?? ""),
    running: Boolean(d.running),
    loadedModules: asStringArray(d.loaded_modules),
  };
}

// -- RuntimeCapabilities --------------------------------------------------

export interface ProfileCapabilities {
  readonly instanceCount: number;
  readonly addressable: boolean;
  readonly hasWiring: boolean;
}

export interface RuntimeCapabilities {
  readonly canSpawnMembers: boolean;
  readonly canSendMessages: boolean;
  readonly canWireMembers: boolean;
  readonly canRetireMembers: boolean;
  readonly availableSpawnModes: readonly string[];
  readonly profileCapabilities?: Readonly<Record<string, ProfileCapabilities>>;
}

function parseProfileCapabilities(
  raw: unknown,
): Record<string, ProfileCapabilities> | undefined {
  if (raw == null || typeof raw !== "object") return undefined;
  const d = raw as Record<string, Record<string, unknown>>;
  const result: Record<string, ProfileCapabilities> = {};
  for (const [key, val] of Object.entries(d)) {
    if (val && typeof val === "object") {
      result[key] = {
        instanceCount: Number(val.instance_count ?? 0),
        addressable: Boolean(val.addressable ?? true),
        hasWiring: Boolean(val.has_wiring ?? false),
      };
    }
  }
  return Object.keys(result).length > 0 ? result : undefined;
}

function parseRuntimeCapabilities(raw: unknown): RuntimeCapabilities | undefined {
  if (raw == null || typeof raw !== "object") return undefined;
  const d = raw as Record<string, unknown>;
  return {
    canSpawnMembers: Boolean(d.can_spawn_members ?? false),
    canSendMessages: Boolean(d.can_send_messages ?? false),
    canWireMembers: Boolean(d.can_wire_members ?? false),
    canRetireMembers: Boolean(d.can_retire_members ?? false),
    availableSpawnModes: asStringArray(d.available_spawn_modes),
    profileCapabilities: parseProfileCapabilities(d.profile_capabilities),
  };
}

// -- CapabilitiesResult ---------------------------------------------------

export interface CapabilitiesResult {
  readonly contractVersion: string;
  readonly methods: readonly string[];
  readonly loadedModules: readonly string[];
  readonly runtimeCapabilities?: RuntimeCapabilities;
}

export function parseCapabilitiesResult(raw: unknown): CapabilitiesResult {
  const d = asRecord(raw);
  return {
    contractVersion: String(d.contract_version ?? ""),
    methods: asStringArray(d.methods),
    loadedModules: asStringArray(d.loaded_modules),
    runtimeCapabilities: parseRuntimeCapabilities(d.runtime_capabilities),
  };
}

// -- ReconcileResult ------------------------------------------------------

export interface ReconcileResult {
  readonly accepted: boolean;
  readonly reconciledModules: readonly string[];
  readonly added: number;
}

export function parseReconcileResult(raw: unknown): ReconcileResult {
  const d = asRecord(raw);
  return {
    accepted: Boolean(d.accepted),
    reconciledModules: asStringArray(d.reconciled_modules),
    added: Number(d.added ?? 0),
  };
}

// -- SpawnResult ----------------------------------------------------------

export interface SpawnResult {
  readonly accepted: boolean;
  readonly moduleId: string;
  readonly meerkatId: string | null;
  readonly profile: string | null;
}

/** Alias for backward compatibility. */
export type SpawnMemberResult = SpawnResult;

export function parseSpawnResult(raw: unknown): SpawnResult {
  const d = asRecord(raw);
  return {
    accepted: Boolean(d.accepted),
    moduleId: String(d.module_id ?? ""),
    meerkatId: typeof d.meerkat_id === "string" ? d.meerkat_id : null,
    profile: typeof d.profile === "string" ? d.profile : null,
  };
}

// -- KeepAliveConfig ------------------------------------------------------

export interface KeepAliveConfig {
  readonly intervalMs: number;
  readonly event: string;
}

export function parseKeepAliveConfig(raw: unknown): KeepAliveConfig {
  const d = asRecord(raw);
  return {
    intervalMs: Number(d.interval_ms ?? 0),
    event: String(d.event ?? ""),
  };
}

// -- EventEnvelope --------------------------------------------------------

export interface EventEnvelope {
  readonly eventId: string;
  readonly source: string;
  readonly timestampMs: number;
  readonly event: unknown;
}

export function parseEventEnvelope(raw: unknown): EventEnvelope {
  const d = asRecord(raw);
  return {
    eventId: String(d.event_id ?? ""),
    source: String(d.source ?? ""),
    timestampMs: Number(d.timestamp_ms ?? 0),
    event: d.event,
  };
}

// -- SubscribeResult ------------------------------------------------------

export interface SubscribeResult {
  readonly scope: string;
  readonly replayFromEventId: string | null;
  readonly keepAlive: KeepAliveConfig;
  readonly keepAliveComment: string;
  readonly eventFrames: readonly string[];
  readonly events: readonly EventEnvelope[];
}

export function parseSubscribeResult(raw: unknown): SubscribeResult {
  const d = asRecord(raw);
  const eventsRaw = Array.isArray(d.events) ? d.events : [];
  return {
    scope: String(d.scope ?? ""),
    replayFromEventId:
      typeof d.replay_from_event_id === "string"
        ? d.replay_from_event_id
        : null,
    keepAlive: parseKeepAliveConfig(d.keep_alive),
    keepAliveComment: String(d.keep_alive_comment ?? ""),
    eventFrames: asStringArray(d.event_frames),
    events: eventsRaw.map(parseEventEnvelope),
  };
}

// -- SendMessageResult ----------------------------------------------------

export interface SendMessageResult {
  readonly accepted: boolean;
  readonly memberId: string;
  readonly sessionId: string;
}

export function parseSendMessageResult(raw: unknown): SendMessageResult {
  const d = asRecord(raw);
  return {
    accepted: Boolean(d.accepted),
    memberId: String(d.member_id ?? ""),
    sessionId: String(d.session_id ?? ""),
  };
}

// -- RoutingResolution ----------------------------------------------------

export interface RoutingResolution {
  readonly recipient: string;
  readonly route: Record<string, unknown>;
}

export function parseRoutingResolution(raw: unknown): RoutingResolution {
  const d = asRecord(raw);
  return {
    recipient: String(d.recipient ?? ""),
    route: asRecord(d.route ?? d),
  };
}

// -- DeliveryResult -------------------------------------------------------

export interface DeliveryResult {
  readonly delivered: boolean;
  readonly deliveryId: string;
}

export function parseDeliveryResult(raw: unknown): DeliveryResult {
  const d = asRecord(raw);
  return {
    delivered: Boolean(d.delivered),
    deliveryId: String(d.delivery_id ?? ""),
  };
}

// -- DeliveryHistoryResult ------------------------------------------------

export interface DeliveryHistoryResult {
  readonly deliveries: readonly Record<string, unknown>[];
}

export function parseDeliveryHistoryResult(
  raw: unknown,
): DeliveryHistoryResult {
  const d = asRecord(raw);
  return {
    deliveries: asRecordArray(d.deliveries),
  };
}

// -- MemoryQueryResult ----------------------------------------------------

export interface MemoryQueryResult {
  readonly results: readonly Record<string, unknown>[];
}

export function parseMemoryQueryResult(raw: unknown): MemoryQueryResult {
  const d = asRecord(raw);
  return {
    results: asRecordArray(d.results),
  };
}

// -- MemoryStoreInfo ------------------------------------------------------

export interface MemoryStoreInfo {
  readonly store: string;
  readonly recordCount: number;
}

export function parseMemoryStoreInfo(raw: unknown): MemoryStoreInfo {
  const d = asRecord(raw);
  return {
    store: String(d.store ?? ""),
    recordCount: Number(d.record_count ?? 0),
  };
}

// -- MemoryIndexResult ----------------------------------------------------

export interface MemoryIndexResult {
  readonly entity: string;
  readonly topic: string;
  readonly store: string;
  readonly assertionId: string | null;
}

export function parseMemoryIndexResult(raw: unknown): MemoryIndexResult {
  const d = asRecord(raw);
  return {
    entity: String(d.entity ?? ""),
    topic: String(d.topic ?? ""),
    store: String(d.store ?? ""),
    assertionId: typeof d.assertion_id === "string" ? d.assertion_id : null,
  };
}

// -- CallToolResult -------------------------------------------------------

export interface CallToolResult {
  readonly moduleId: string;
  readonly tool: string;
  readonly result: unknown;
}

export function parseCallToolResult(raw: unknown): CallToolResult {
  const d = asRecord(raw);
  return {
    moduleId: String(d.module_id ?? ""),
    tool: String(d.tool ?? ""),
    result: d.result,
  };
}

// -- MemberSnapshot -------------------------------------------------------

export interface MemberSnapshot {
  readonly meerkatId: string;
  readonly profile: string;
  readonly state: string;
  readonly wiredTo: readonly string[];
  readonly labels: Readonly<Record<string, string>>;
}

export function parseMemberSnapshot(raw: unknown): MemberSnapshot {
  const d = asRecord(raw);
  return {
    meerkatId: String(d.meerkat_id ?? ""),
    profile: String(d.profile ?? ""),
    state: String(d.state ?? ""),
    wiredTo: asStringArray(d.wired_to),
    labels: asStringRecord(d.labels),
  };
}

// -- RuntimeRouteResult ---------------------------------------------------

export interface RuntimeRouteResult {
  readonly routeKey: string;
  readonly recipient: string;
  readonly channel: string | null;
  readonly sink: string;
  readonly targetModule: string;
}

export function parseRuntimeRouteResult(raw: unknown): RuntimeRouteResult {
  const d = asRecord(raw);
  return {
    routeKey: String(d.route_key ?? ""),
    recipient: String(d.recipient ?? ""),
    channel: typeof d.channel === "string" ? d.channel : null,
    sink: String(d.sink ?? ""),
    targetModule: String(d.target_module ?? ""),
  };
}

// -- GatingEvaluateResult -------------------------------------------------

export interface GatingEvaluateResult {
  readonly actionId: string;
  readonly action: string;
  readonly actorId: string;
  readonly riskTier: string | null;
  readonly outcome: string;
  readonly pendingId: string | null;
}

export function parseGatingEvaluateResult(
  raw: unknown,
): GatingEvaluateResult {
  const d = asRecord(raw);
  return {
    actionId: String(d.action_id ?? ""),
    action: String(d.action ?? ""),
    actorId: String(d.actor_id ?? ""),
    riskTier: typeof d.risk_tier === "string" ? d.risk_tier : null,
    outcome: String(d.outcome ?? ""),
    pendingId: typeof d.pending_id === "string" ? d.pending_id : null,
  };
}

// -- GatingDecisionResult -------------------------------------------------

export interface GatingDecisionResult {
  readonly pendingId: string;
  readonly actionId: string;
  readonly decision: string;
}

export function parseGatingDecisionResult(
  raw: unknown,
): GatingDecisionResult {
  const d = asRecord(raw);
  return {
    pendingId: String(d.pending_id ?? ""),
    actionId: String(d.action_id ?? ""),
    decision: String(d.decision ?? ""),
  };
}

// -- GatingAuditEntry -----------------------------------------------------

export interface GatingAuditEntry {
  readonly auditId: string;
  readonly timestampMs: number;
  readonly eventType: string;
  readonly actionId: string;
  readonly actorId: string;
  readonly riskTier: string | null;
  readonly outcome: string;
}

export function parseGatingAuditEntry(raw: unknown): GatingAuditEntry {
  const d = asRecord(raw);
  return {
    auditId: String(d.audit_id ?? ""),
    timestampMs: Number(d.timestamp_ms ?? 0),
    eventType: String(d.event_type ?? ""),
    actionId: String(d.action_id ?? ""),
    actorId: String(d.actor_id ?? ""),
    riskTier: typeof d.risk_tier === "string" ? d.risk_tier : null,
    outcome: String(d.outcome ?? ""),
  };
}

// -- GatingPendingEntry ---------------------------------------------------

export interface GatingPendingEntry {
  readonly pendingId: string;
  readonly actionId: string;
  readonly action: string;
  readonly actorId: string;
  readonly riskTier: string | null;
  readonly createdAtMs: number;
}

export function parseGatingPendingEntry(raw: unknown): GatingPendingEntry {
  const d = asRecord(raw);
  return {
    pendingId: String(d.pending_id ?? ""),
    actionId: String(d.action_id ?? ""),
    action: String(d.action ?? ""),
    actorId: String(d.actor_id ?? ""),
    riskTier: typeof d.risk_tier === "string" ? d.risk_tier : null,
    createdAtMs: Number(d.created_at_ms ?? 0),
  };
}

// -- ReconcileEdgesReport -------------------------------------------------

export interface ReconcileEdgesReport {
  readonly desiredEdges: readonly Record<string, unknown>[];
  readonly wiredEdges: readonly Record<string, unknown>[];
  readonly unwiredEdges: readonly Record<string, unknown>[];
  readonly retainedEdges: readonly Record<string, unknown>[];
  readonly preexistingEdges: readonly Record<string, unknown>[];
  readonly skippedMissingMembers: readonly Record<string, unknown>[];
  readonly prunedStaleManagedEdges: readonly Record<string, unknown>[];
  readonly failures: readonly Record<string, unknown>[];
  readonly isComplete: boolean;
}

export function parseReconcileEdgesReport(
  raw: unknown,
): ReconcileEdgesReport {
  const d = asRecord(raw);
  const failures = asRecordArray(d.failures);
  const skipped = asRecordArray(d.skipped_missing_members);
  return {
    desiredEdges: asRecordArray(d.desired_edges),
    wiredEdges: asRecordArray(d.wired_edges),
    unwiredEdges: asRecordArray(d.unwired_edges),
    retainedEdges: asRecordArray(d.retained_edges),
    preexistingEdges: asRecordArray(d.preexisting_edges),
    skippedMissingMembers: skipped,
    prunedStaleManagedEdges: asRecordArray(d.pruned_stale_managed_edges),
    failures,
    isComplete: failures.length === 0 && skipped.length === 0,
  };
}

// -- RediscoverReport -----------------------------------------------------

export interface RediscoverReport {
  readonly spawned: readonly string[];
  readonly edges: ReconcileEdgesReport;
}

export function parseRediscoverReport(raw: unknown): RediscoverReport {
  const d = asRecord(raw);
  return {
    spawned: asStringArray(d.spawned),
    edges: parseReconcileEdgesReport(d.edges),
  };
}

// -- Unified events (persisted event log) ---------------------------------

export interface UnifiedAgentEvent {
  readonly kind: "agent";
  readonly agentId: string;
  readonly eventType: string;
}

export interface UnifiedModuleEvent {
  readonly kind: "module";
  readonly module: string;
  readonly eventType: string;
  readonly payload: Record<string, unknown>;
}

export type UnifiedEvent = UnifiedAgentEvent | UnifiedModuleEvent;

function parseUnifiedEvent(raw: unknown): UnifiedEvent {
  const d = asRecord(raw);
  if ("Agent" in d) {
    const agent = asRecord(d.Agent);
    return {
      kind: "agent",
      agentId: String(agent.agent_id ?? ""),
      eventType: String(agent.event_type ?? ""),
    };
  }
  if ("Module" in d) {
    const mod = asRecord(d.Module);
    return {
      kind: "module",
      module: String(mod.module ?? ""),
      eventType: String(mod.event_type ?? ""),
      payload: asRecord(mod.payload),
    };
  }
  return {
    kind: "module",
    module: "unknown",
    eventType: "unknown",
    payload: asRecord(raw),
  };
}

// -- PersistedEvent -------------------------------------------------------

export interface PersistedEvent {
  readonly id: string;
  readonly seq: number;
  readonly timestampMs: number;
  readonly memberId: string | null;
  readonly event: UnifiedEvent;
}

export function parsePersistedEvent(raw: unknown): PersistedEvent {
  const d = asRecord(raw);
  const rawEvent = d.event;
  const event =
    typeof rawEvent === "object" && rawEvent !== null
      ? parseUnifiedEvent(rawEvent)
      : ({ kind: "module", module: "unknown", eventType: "unknown", payload: {} } as UnifiedModuleEvent);
  return {
    id: String(d.id ?? ""),
    seq: Number(d.seq ?? 0),
    timestampMs: Number(d.timestamp_ms ?? 0),
    memberId: typeof d.member_id === "string" ? d.member_id : null,
    event,
  };
}

// -- EventQuery -----------------------------------------------------------

export interface EventQuery {
  readonly sinceMs?: number;
  readonly untilMs?: number;
  readonly memberId?: string;
  readonly eventTypes?: readonly string[];
  readonly limit?: number;
  readonly afterSeq?: number;
}

export function eventQueryToDict(query: EventQuery): Record<string, unknown> {
  const d: Record<string, unknown> = {};
  if (query.sinceMs !== undefined) d.since_ms = query.sinceMs;
  if (query.untilMs !== undefined) d.until_ms = query.untilMs;
  if (query.memberId !== undefined) d.member_id = query.memberId;
  if (query.eventTypes !== undefined && query.eventTypes.length > 0) {
    d.event_types = [...query.eventTypes];
  }
  if (query.limit !== undefined) d.limit = query.limit;
  if (query.afterSeq !== undefined) d.after_seq = query.afterSeq;
  return d;
}

// -- ErrorCategory / ErrorEvent -------------------------------------------

export const ErrorCategory = {
  SPAWN_FAILURE: "spawn_failure",
  RECONCILE_INCOMPLETE: "reconcile_incomplete",
  CHECKPOINT_FAILURE: "checkpoint_failure",
  HOST_LOOP_CRASH: "host_loop_crash",
  REDISCOVER_FAILURE: "rediscover_failure",
} as const;

export type ErrorCategoryValue =
  (typeof ErrorCategory)[keyof typeof ErrorCategory];

export interface ErrorEvent {
  readonly category: string;
  readonly message: string;
  readonly context: Record<string, unknown>;
}

export function parseErrorEvent(raw: unknown): ErrorEvent {
  const d = asRecord(raw);
  const category = String(d.category ?? "unknown");
  const context: Record<string, unknown> = {};
  for (const [k, v] of Object.entries(d)) {
    if (k !== "category") context[k] = v;
  }

  const error = String(context.error ?? "");
  const memberId = String(context.member_id ?? "");
  let message: string;

  switch (category) {
    case ErrorCategory.SPAWN_FAILURE:
      message = memberId ? `${memberId}: ${error}` : error;
      break;
    case ErrorCategory.RECONCILE_INCOMPLETE: {
      const failures = Number(context.failures ?? 0);
      const skipped = Number(context.skipped ?? 0);
      message = `${failures} failures, ${skipped} skipped`;
      break;
    }
    case ErrorCategory.CHECKPOINT_FAILURE: {
      const sessionId = String(context.session_id ?? "");
      message = sessionId ? `${sessionId}: ${error}` : error;
      break;
    }
    case ErrorCategory.HOST_LOOP_CRASH:
      message = memberId ? `${memberId}: ${error}` : error;
      break;
    case ErrorCategory.REDISCOVER_FAILURE:
      message = error;
      break;
    default:
      message = JSON.stringify(d);
      break;
  }

  return { category, message, context };
}
