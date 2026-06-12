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

function validateAgentIdentity(identity: string): string {
  if (
    identity.length === 0 ||
    identity.trim() !== identity ||
    /\s/.test(identity) ||
    identity.includes("/")
  ) {
    throw new Error(`invalid agent identity: ${identity}`);
  }
  return identity;
}

// -- Constants ------------------------------------------------------------

export const MEMBER_STATE_ACTIVE = "active" as const;
export const MEMBER_STATE_RETIRING = "retiring" as const;

// -- SessionCreatedContext ------------------------------------------------

/** Context delivered to SessionAgentBuilder.afterCreate after a session
 *  is successfully created. */
export interface SessionCreatedContext {
  readonly model: string;
  readonly labels: Record<string, string>;
  readonly systemPrompt: string | null;
}

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
  readonly agentIdentity: string | null;
  readonly role: string | null;
}

/** Alias for backward compatibility. */
export type SpawnMemberResult = SpawnResult;

export function parseSpawnResult(raw: unknown): SpawnResult {
  const d = asRecord(raw);
  return {
    accepted: Boolean(d.accepted),
    moduleId: String(d.module_id ?? ""),
    agentIdentity:
      typeof d.agent_identity === "string" ? d.agent_identity : null,
    role: typeof d.role === "string" ? d.role : null,
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
  // Pre-fix this mapped every entry — including null/string/number —
  // through `parseEventEnvelope`, where `asRecord` quietly returned
  // `{}` and produced silently-empty envelopes. Filter to objects so
  // non-object entries are dropped instead of becoming data-loss.
  const eventsRaw = Array.isArray(d.events)
    ? d.events.filter(
        (entry): entry is Record<string, unknown> =>
          typeof entry === "object" && entry !== null && !Array.isArray(entry),
      )
    : [];
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
  readonly agentIdentity: string;
  readonly role: string;
  readonly state: string;
  readonly wiredTo: readonly string[];
  readonly labels: Readonly<Record<string, string>>;
}

export function parseMemberSnapshot(raw: unknown): MemberSnapshot {
  const d = asRecord(raw);
  return {
    agentIdentity: String(d.agent_identity ?? ""),
    role: String(d.role ?? ""),
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
  readonly payload?: Record<string, unknown> | null;
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
  if (d.kind === "agent") {
    return {
      kind: "agent",
      agentId: String(d.agent_id ?? d.agentId ?? ""),
      eventType: String(d.event_type ?? d.eventType ?? ""),
      payload:
        typeof d.payload === "object" && d.payload !== null
          ? asRecord(d.payload)
          : null,
    };
  }
  if (d.kind === "module") {
    return {
      kind: "module",
      module: String(d.module ?? ""),
      eventType: String(d.event_type ?? d.eventType ?? ""),
      payload: asRecord(d.payload),
    };
  }
  if ("Agent" in d) {
    const agent = asRecord(d.Agent);
    return {
      kind: "agent",
      agentId: String(agent.agent_id ?? ""),
      eventType: String(agent.event_type ?? ""),
      payload:
        typeof agent.payload === "object" && agent.payload !== null
          ? asRecord(agent.payload)
          : null,
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

/**
 * Query parameters for historical event retrieval. `afterSeq` is the
 * pagination cursor — pass the highest seen `seq`/`cursor` to receive
 * only strictly-newer events. `mobId`, `runId`, `stepId`, `identity`
 * filter the structural mob-events surface (`mobkit/mob_events/query`).
 */
export interface EventQuery {
  readonly sinceMs?: number;
  readonly untilMs?: number;
  readonly memberId?: string;
  readonly identity?: string;
  readonly mobId?: string;
  readonly runId?: string;
  readonly stepId?: string;
  readonly eventTypes?: readonly string[];
  readonly limit?: number;
  readonly afterSeq?: number;
}

export function eventQueryToDict(query: EventQuery): Record<string, unknown> {
  const d: Record<string, unknown> = {};
  if (query.sinceMs !== undefined) d.since_ms = query.sinceMs;
  if (query.untilMs !== undefined) d.until_ms = query.untilMs;
  if (query.memberId !== undefined) d.member_id = query.memberId;
  if (query.identity !== undefined) d.identity = query.identity;
  if (query.mobId !== undefined) d.mob_id = query.mobId;
  if (query.runId !== undefined) d.run_id = query.runId;
  if (query.stepId !== undefined) d.step_id = query.stepId;
  if (query.eventTypes !== undefined && query.eventTypes.length > 0) {
    d.event_types = [...query.eventTypes];
  }
  if (query.limit !== undefined) d.limit = query.limit;
  if (query.afterSeq !== undefined) d.after_seq = query.afterSeq;
  return d;
}

// -- MobStructuralEvent ---------------------------------------------------

/**
 * Structural mob event projected from `MobEventKind`. Preserves
 * `mobId`/`runId`/`stepId`/`agentIdentity` that the legacy lossy
 * `UnifiedEvent::Agent` projection discards. Use `cursor` as the
 * `EventQuery.afterSeq` pagination token on the next request.
 */
export interface MobStructuralEvent {
  readonly eventId: string;
  readonly cursor: number;
  readonly mobId: string;
  readonly timestampMs: number;
  readonly kind: string;
  readonly runId: string | null;
  readonly stepId: string | null;
  readonly agentIdentity: string | null;
  readonly mobLabels: Readonly<Record<string, string>>;
  readonly runLabels: Readonly<Record<string, string>>;
  readonly data: Record<string, unknown>;
}

export function parseMobStructuralEvent(raw: unknown): MobStructuralEvent {
  const d = asRecord(raw);
  const data = d.data;
  return {
    eventId: String(d.event_id ?? ""),
    cursor: Number(d.cursor ?? 0),
    mobId: String(d.mob_id ?? ""),
    timestampMs: Number(d.timestamp_ms ?? 0),
    kind: String(d.kind ?? ""),
    runId: typeof d.run_id === "string" ? d.run_id : null,
    stepId: typeof d.step_id === "string" ? d.step_id : null,
    agentIdentity:
      typeof d.agent_identity === "string" ? d.agent_identity : null,
    mobLabels: asStringRecord(d.mob_labels),
    runLabels: asStringRecord(d.run_labels),
    data:
      typeof data === "object" && data !== null
        ? (data as Record<string, unknown>)
        : {},
  };
}

// -- MobRun (full ledger projection) --------------------------------------

/** Lifecycle states for a flow run; mirrors meerkat's `MobRunStatus`. */
export type MobRunStatus =
  | "pending"
  | "running"
  | "completed"
  | "failed"
  | "canceled";

/** Per-target step execution ledger entry; mirrors `StepLedgerEntry`. */
export interface StepRecord {
  readonly stepId: string;
  readonly agentIdentity: string;
  readonly status: string;
  readonly output: unknown;
  readonly timestamp: string;
}

export function parseStepRecord(raw: unknown): StepRecord {
  const d = asRecord(raw);
  return {
    stepId: String(d.step_id ?? ""),
    agentIdentity: String(d.agent_identity ?? ""),
    status: String(d.status ?? ""),
    output: d.output,
    timestamp: String(d.timestamp ?? ""),
  };
}

/** Flow-level failure log entry; mirrors `FailureLedgerEntry`. */
export interface FailureRecord {
  readonly stepId: string;
  readonly reason: string;
  readonly timestamp: string;
}

export function parseFailureRecord(raw: unknown): FailureRecord {
  const d = asRecord(raw);
  return {
    stepId: String(d.step_id ?? ""),
    reason: String(d.reason ?? ""),
    timestamp: String(d.timestamp ?? ""),
  };
}

/** Per-frame kernel snapshot; mirrors `FrameSnapshot`.
 *
 * `kernelState` is meerkat-internal `flow_frame::State` and passes
 * through as `unknown`. */
export interface FrameRecord {
  readonly kernelState: unknown;
}

export function parseFrameRecord(raw: unknown): FrameRecord {
  const d = asRecord(raw);
  return { kernelState: d.kernel_state };
}

/** Per-loop kernel snapshot; mirrors `LoopSnapshot`.
 *
 * `kernelState` is meerkat-internal `loop_iteration::State` and passes
 * through as `unknown`. */
export interface LoopRecord {
  readonly kernelState: unknown;
}

export function parseLoopRecord(raw: unknown): LoopRecord {
  const d = asRecord(raw);
  return { kernelState: d.kernel_state };
}

/** Loop-iteration → body-frame ledger entry; mirrors `LoopIterationLedgerEntry`. */
export interface LoopIterationRecord {
  readonly loopInstanceId: string;
  readonly iteration: number;
  readonly frameId: string;
}

export function parseLoopIterationRecord(raw: unknown): LoopIterationRecord {
  const d = asRecord(raw);
  return {
    loopInstanceId: String(d.loop_instance_id ?? ""),
    iteration: Number(d.iteration ?? 0),
    frameId: String(d.frame_id ?? ""),
  };
}

/**
 * Persisted flow run aggregate returned by `MobHandle.listRuns`.
 *
 * Carries the full meerkat ledger projection. Meerkat-internal
 * sub-shapes (`flowState`, `activationParams`, `StepRecord.output`,
 * `rootStepOutputs` / `loopIterationOutputs` value blobs, frame /
 * loop `kernelState`) pass through as `unknown` rather than being
 * re-typed in the SDK. `frames` and `loops` are **maps**, not arrays.
 */
export interface MobRun {
  readonly runId: string;
  readonly mobId: string;
  readonly flowId: string;
  readonly status: MobRunStatus;
  readonly flowState: unknown;
  readonly activationParams: unknown;
  readonly createdAt: string;
  readonly completedAt: string | null;
  readonly stepLedger: StepRecord[];
  readonly failureLedger: FailureRecord[];
  readonly frames: Readonly<Record<string, FrameRecord>>;
  readonly loops: Readonly<Record<string, LoopRecord>>;
  readonly loopIterationLedger: LoopIterationRecord[];
  readonly schemaVersion: number;
  readonly rootStepOutputs: Readonly<Record<string, unknown>>;
  readonly loopIterationOutputs: Readonly<Record<string, unknown>>;
}

function parseMobRunStatus(raw: unknown): MobRunStatus {
  const known: readonly MobRunStatus[] = [
    "pending",
    "running",
    "completed",
    "failed",
    "canceled",
  ];
  if (typeof raw === "string" && (known as readonly string[]).includes(raw)) {
    return raw as MobRunStatus;
  }
  return "pending";
}

export function parseMobRun(raw: unknown): MobRun {
  const d = asRecord(raw);
  const stepLedger = Array.isArray(d.step_ledger)
    ? d.step_ledger.map(parseStepRecord)
    : [];
  const failureLedger = Array.isArray(d.failure_ledger)
    ? d.failure_ledger.map(parseFailureRecord)
    : [];
  const iterations = Array.isArray(d.loop_iteration_ledger)
    ? d.loop_iteration_ledger.map(parseLoopIterationRecord)
    : [];
  const framesIn =
    typeof d.frames === "object" && d.frames !== null
      ? (d.frames as Record<string, unknown>)
      : {};
  const frames: Record<string, FrameRecord> = {};
  for (const [k, v] of Object.entries(framesIn)) {
    if (typeof v === "object" && v !== null) {
      frames[k] = parseFrameRecord(v);
    }
  }
  const loopsIn =
    typeof d.loops === "object" && d.loops !== null
      ? (d.loops as Record<string, unknown>)
      : {};
  const loops: Record<string, LoopRecord> = {};
  for (const [k, v] of Object.entries(loopsIn)) {
    if (typeof v === "object" && v !== null) {
      loops[k] = parseLoopRecord(v);
    }
  }
  const rootOutputs =
    typeof d.root_step_outputs === "object" && d.root_step_outputs !== null
      ? (d.root_step_outputs as Record<string, unknown>)
      : {};
  const iterOutputs =
    typeof d.loop_iteration_outputs === "object" &&
    d.loop_iteration_outputs !== null
      ? (d.loop_iteration_outputs as Record<string, unknown>)
      : {};
  return {
    runId: String(d.run_id ?? ""),
    mobId: String(d.mob_id ?? ""),
    flowId: String(d.flow_id ?? ""),
    status: parseMobRunStatus(d.status),
    flowState: d.flow_state,
    activationParams: d.activation_params,
    createdAt: String(d.created_at ?? ""),
    completedAt:
      d.completed_at === null || d.completed_at === undefined
        ? null
        : String(d.completed_at),
    stepLedger,
    failureLedger,
    frames,
    loops,
    loopIterationLedger: iterations,
    schemaVersion: Number(d.schema_version ?? 0),
    rootStepOutputs: rootOutputs,
    loopIterationOutputs: iterOutputs,
  };
}

// -- Rich member / helper / catalog / cross-mob results --------------------

export type MobMemberStatus =
  | "active"
  | "retiring"
  | "broken"
  | "completed"
  | "unknown";

export interface MobUnreachablePeer {
  readonly peer: string;
  readonly reason: string | null;
}

export function parseMobUnreachablePeer(raw: unknown): MobUnreachablePeer {
  const d = asRecord(raw);
  return {
    peer: String(d.peer ?? ""),
    reason: typeof d.reason === "string" ? d.reason : null,
  };
}

export interface PeerConnectivitySnapshot {
  readonly reachablePeerCount: number;
  readonly unknownPeerCount: number;
  readonly unreachablePeers: readonly MobUnreachablePeer[];
}

export function parsePeerConnectivitySnapshot(raw: unknown): PeerConnectivitySnapshot {
  const d = asRecord(raw);
  return {
    reachablePeerCount: Number(d.reachable_peer_count ?? 0),
    unknownPeerCount: Number(d.unknown_peer_count ?? 0),
    unreachablePeers: asRecordArray(d.unreachable_peers).map(parseMobUnreachablePeer),
  };
}

export interface RichMemberSnapshot {
  readonly status: string;
  readonly outputPreview: string | null;
  readonly error: string | null;
  readonly tokensUsed: number;
  readonly isFinal: boolean;
  readonly currentSessionId: string | null;
  readonly peerConnectivity: PeerConnectivitySnapshot | null;
}

export function parseRichMemberSnapshot(raw: unknown): RichMemberSnapshot {
  const d = asRecord(raw);
  return {
    status: String(d.status ?? "unknown"),
    outputPreview: typeof d.output_preview === "string" ? d.output_preview : null,
    error: typeof d.error === "string" ? d.error : null,
    tokensUsed: Number(d.tokens_used ?? 0),
    isFinal: Boolean(d.is_final),
    currentSessionId:
      typeof d.current_session_id === "string" ? d.current_session_id : null,
    peerConnectivity:
      typeof d.peer_connectivity === "object" && d.peer_connectivity !== null
        ? parsePeerConnectivitySnapshot(d.peer_connectivity)
        : null,
  };
}

export interface HelperResult {
  readonly output: string | null;
  readonly tokensUsed: number;
  readonly sessionId: string | null;
}

export function parseHelperResult(raw: unknown): HelperResult {
  const d = asRecord(raw);
  return {
    output: typeof d.output === "string" ? d.output : null,
    tokensUsed: Number(d.tokens_used ?? 0),
    sessionId: typeof d.session_id === "string" ? d.session_id : null,
  };
}

export interface MobRunSnapshot {
  readonly runId: string;
  readonly mobId: string;
  readonly flowId: string;
  readonly status: string;
  readonly stepLedger: readonly Record<string, unknown>[];
  readonly failureLedger: readonly Record<string, unknown>[];
}

export function parseMobRunSnapshot(raw: unknown): MobRunSnapshot {
  const d = asRecord(raw);
  return {
    runId: String(d.run_id ?? ""),
    mobId: String(d.mob_id ?? ""),
    flowId: String(d.flow_id ?? ""),
    status: String(d.status ?? "unknown"),
    stepLedger: asRecordArray(d.step_ledger),
    failureLedger: asRecordArray(d.failure_ledger),
  };
}

export interface CrossMobContactEntry {
  readonly mobId: string;
  readonly transport: string;
}

export function parseCrossMobContactEntry(raw: unknown): CrossMobContactEntry {
  const d = asRecord(raw);
  let transport = d.transport;
  if (typeof transport === "object" && transport !== null) {
    const t = asRecord(transport);
    if (typeof t.Tcp === "string") transport = `tcp://${t.Tcp}`;
    else if (typeof t.Uds === "string") transport = `uds://${t.Uds}`;
    else transport = "inproc";
  } else if (transport === "Inproc") {
    transport = "inproc";
  }
  return {
    mobId: String(d.mob_id ?? ""),
    transport: String(transport ?? ""),
  };
}

export interface CatalogEntry {
  readonly id: string;
  readonly displayName: string;
  readonly provider: string;
  readonly tier: string;
  readonly contextWindow: number | null;
  readonly maxOutputTokens: number | null;
  readonly vision: boolean;
  readonly imageToolResults: boolean;
}

export function parseCatalogEntry(raw: unknown): CatalogEntry {
  const d = asRecord(raw);
  const profile = asRecord(d.profile);
  return {
    id: String(d.id ?? ""),
    displayName: String(d.display_name ?? ""),
    provider: String(d.provider ?? ""),
    tier: String(d.tier ?? ""),
    contextWindow: d.context_window == null ? null : Number(d.context_window),
    maxOutputTokens: d.max_output_tokens == null ? null : Number(d.max_output_tokens),
    vision: Boolean(profile.vision),
    imageToolResults: Boolean(profile.image_tool_results),
  };
}

export interface ProviderDefaults {
  readonly provider: string;
  readonly defaultModelId: string;
  readonly models: readonly CatalogEntry[];
}

export function parseProviderDefaults(raw: unknown): ProviderDefaults {
  const d = asRecord(raw);
  return {
    provider: String(d.provider ?? ""),
    defaultModelId: String(d.default_model_id ?? ""),
    models: asRecordArray(d.models).map(parseCatalogEntry),
  };
}

export interface ModelsCatalogResult {
  readonly models: readonly CatalogEntry[];
  readonly providerDefaults: readonly ProviderDefaults[];
}

export function parseModelsCatalogResult(raw: unknown): ModelsCatalogResult {
  const d = asRecord(raw);
  return {
    models: asRecordArray(d.models).map(parseCatalogEntry),
    providerDefaults: asRecordArray(d.provider_defaults).map(parseProviderDefaults),
  };
}

// -- Mobpack editor catalogs ----------------------------------------------

export interface MobpackToolsCatalogResult {
  readonly schemaVersion: string;
  readonly runtimeBacked: boolean;
  readonly source: string;
  readonly authoringProvider: Record<string, unknown>;
  readonly runtimeUnavailableReason: string | null;
  readonly toolCatalog: readonly Record<string, unknown>[];
}

export function parseMobpackToolsCatalogResult(
  raw: unknown,
): MobpackToolsCatalogResult {
  const d = asRecord(raw);
  return {
    schemaVersion: String(d.schema_version ?? ""),
    runtimeBacked: Boolean(d.runtime_backed),
    source: String(d.source ?? ""),
    authoringProvider: asRecord(d.authoring_provider),
    runtimeUnavailableReason:
      d.runtime_unavailable_reason == null
        ? null
        : String(d.runtime_unavailable_reason),
    toolCatalog: asRecordArray(d.tool_catalog),
  };
}

export interface MobpackSkillsCatalogResult {
  readonly schemaVersion: string;
  readonly runtimeBacked: boolean;
  readonly source: string;
  readonly authoringProvider: Record<string, unknown>;
  readonly runtimeUnavailableReason: string | null;
  readonly skillRealms: readonly Record<string, unknown>[];
}

export function parseMobpackSkillsCatalogResult(
  raw: unknown,
): MobpackSkillsCatalogResult {
  const d = asRecord(raw);
  return {
    schemaVersion: String(d.schema_version ?? ""),
    runtimeBacked: Boolean(d.runtime_backed),
    source: String(d.source ?? ""),
    authoringProvider: asRecord(d.authoring_provider),
    runtimeUnavailableReason:
      d.runtime_unavailable_reason == null
        ? null
        : String(d.runtime_unavailable_reason),
    skillRealms: asRecordArray(d.skill_realms),
  };
}

export interface MobpackAgentDefinitionsResult {
  readonly schemaVersion: string;
  readonly runtimeBacked: boolean;
  readonly source: string;
  readonly authoringProvider: Record<string, unknown>;
  readonly runtimeUnavailableReason: string | null;
  readonly agentDefinitions: readonly Record<string, unknown>[];
}

export function parseMobpackAgentDefinitionsResult(
  raw: unknown,
): MobpackAgentDefinitionsResult {
  const d = asRecord(raw);
  return {
    schemaVersion: String(d.schema_version ?? ""),
    runtimeBacked: Boolean(d.runtime_backed),
    source: String(d.source ?? ""),
    authoringProvider: asRecord(d.authoring_provider),
    runtimeUnavailableReason:
      d.runtime_unavailable_reason == null
        ? null
        : String(d.runtime_unavailable_reason),
    agentDefinitions: asRecordArray(d.agent_definitions),
  };
}

export interface MobpackTemplatesResult {
  readonly schemaVersion: string;
  readonly source: string;
  readonly authoringProvider: Record<string, unknown>;
  readonly runtimeUnavailableReason: string | null;
  readonly blankMobpack: Record<string, unknown> | null;
  readonly sampleMobpacks: readonly Record<string, unknown>[];
  readonly sampleAgentDefinitions: readonly Record<string, unknown>[];
  readonly templates: Record<string, unknown>;
}

export function parseMobpackTemplatesResult(raw: unknown): MobpackTemplatesResult {
  const d = asRecord(raw);
  return {
    schemaVersion: String(d.schema_version ?? ""),
    source: String(d.source ?? ""),
    authoringProvider: asRecord(d.authoring_provider),
    runtimeUnavailableReason:
      d.runtime_unavailable_reason == null
        ? null
        : String(d.runtime_unavailable_reason),
    blankMobpack: d.blank_mobpack == null ? null : asRecord(d.blank_mobpack),
    sampleMobpacks: asRecordArray(d.sample_mobpacks),
    sampleAgentDefinitions: asRecordArray(d.sample_agent_definitions),
    templates: asRecord(d.templates),
  };
}

export interface MobpackCatalogsResult {
  readonly schemaVersion: string;
  readonly runtimeBacked: boolean;
  readonly authoringProvider: Record<string, unknown>;
  readonly runtimeUnavailableReason: string | null;
  readonly sources: Record<string, unknown>;
  readonly templates: Record<string, unknown>;
  readonly toolCatalog: readonly Record<string, unknown>[];
  readonly skillRealms: readonly Record<string, unknown>[];
  readonly blankMobpack: Record<string, unknown> | null;
  readonly sampleMobpacks: readonly Record<string, unknown>[];
  readonly agentDefinitions: readonly Record<string, unknown>[];
  readonly sampleAgentDefinitions: readonly Record<string, unknown>[];
  readonly models: readonly CatalogEntry[];
  readonly providerDefaults: readonly ProviderDefaults[];
}

export function parseMobpackCatalogsResult(raw: unknown): MobpackCatalogsResult {
  const d = asRecord(raw);
  return {
    schemaVersion: String(d.schema_version ?? ""),
    runtimeBacked: Boolean(d.runtime_backed),
    authoringProvider: asRecord(d.authoring_provider),
    runtimeUnavailableReason:
      d.runtime_unavailable_reason == null
        ? null
        : String(d.runtime_unavailable_reason),
    sources: asRecord(d.sources),
    templates: asRecord(d.templates),
    toolCatalog: asRecordArray(d.tool_catalog),
    skillRealms: asRecordArray(d.skill_realms),
    blankMobpack: d.blank_mobpack == null ? null : asRecord(d.blank_mobpack),
    sampleMobpacks: asRecordArray(d.sample_mobpacks),
    agentDefinitions: asRecordArray(d.agent_definitions),
    sampleAgentDefinitions: asRecordArray(d.sample_agent_definitions),
    models: asRecordArray(d.models).map(parseCatalogEntry),
    providerDefaults: asRecordArray(d.provider_defaults).map(parseProviderDefaults),
  };
}

// -- Mobpack authoring ------------------------------------------------------

export interface MobpackDiagnostic {
  readonly severity: string;
  readonly code: string;
  readonly message: string;
  readonly path: string | null;
}

export function parseMobpackDiagnostic(raw: unknown): MobpackDiagnostic {
  const d = asRecord(raw);
  return {
    severity: String(d.severity ?? ""),
    code: String(d.code ?? ""),
    message: String(d.message ?? ""),
    path: d.path == null ? null : String(d.path),
  };
}

export interface MobpackDisplayRow {
  readonly kind: string;
  readonly glyph: string;
  readonly head: string;
  readonly sub: string;
  readonly meta: string;
}

export function parseMobpackDisplayRow(raw: unknown): MobpackDisplayRow {
  const d = asRecord(raw);
  return {
    kind: String(d.kind ?? ""),
    glyph: String(d.glyph ?? ""),
    head: String(d.head ?? ""),
    sub: String(d.sub ?? ""),
    meta: String(d.meta ?? ""),
  };
}

export interface MobpackValidationResult {
  readonly ok: boolean;
  readonly diagnostics: readonly MobpackDiagnostic[];
  readonly displayRows: readonly MobpackDisplayRow[];
  readonly mobId: string | null;
  readonly flowIds: readonly string[];
  readonly validationSource: string;
  readonly deployCommand: string;
}

export function parseMobpackValidationResult(
  raw: unknown,
): MobpackValidationResult {
  const d = asRecord(raw);
  return {
    ok: Boolean(d.ok),
    diagnostics: asRecordArray(d.diagnostics).map(parseMobpackDiagnostic),
    displayRows: asRecordArray(d.display_rows).map(parseMobpackDisplayRow),
    mobId: d.mob_id == null ? null : String(d.mob_id),
    flowIds: asStringArray(d.flow_ids),
    validationSource: String(d.validation_source ?? ""),
    deployCommand: String(d.deploy_command ?? ""),
  };
}

export interface MobpackSourceFile {
  readonly path: string;
  readonly mediaType: string;
  readonly sizeBytes: number;
  readonly contentBase64: string;
  readonly sha256: string;
  readonly text: string | null;
}

export function parseMobpackSourceFile(raw: unknown): MobpackSourceFile {
  const d = asRecord(raw);
  return {
    path: String(d.path ?? ""),
    mediaType: String(d.media_type ?? ""),
    sizeBytes: Number(d.size_bytes ?? 0),
    contentBase64: String(d.content_base64 ?? ""),
    sha256: String(d.sha256 ?? ""),
    text: d.text == null ? null : String(d.text),
  };
}

export interface MobpackSourceResult {
  readonly filename: string;
  readonly mediaType: string;
  readonly mobToml: string;
  readonly sourceFiles: readonly MobpackSourceFile[];
  readonly validation: MobpackValidationResult;
  readonly source: string;
}

export function parseMobpackSourceResult(raw: unknown): MobpackSourceResult {
  const d = asRecord(raw);
  return {
    filename: String(d.filename ?? ""),
    mediaType: String(d.media_type ?? ""),
    mobToml: String(d.mob_toml ?? ""),
    sourceFiles: asRecordArray(d.source_files).map(parseMobpackSourceFile),
    validation: parseMobpackValidationResult(d.validation),
    source: String(d.source ?? ""),
  };
}

export interface MobpackExportResult {
  readonly filename: string;
  readonly mediaType: string;
  readonly contentBase64: string;
  readonly mobToml: string;
  readonly sourceFiles: readonly MobpackSourceFile[];
  readonly validation: MobpackValidationResult;
}

export function parseMobpackExportResult(raw: unknown): MobpackExportResult {
  const d = asRecord(raw);
  return {
    filename: String(d.filename ?? ""),
    mediaType: String(d.media_type ?? ""),
    contentBase64: String(d.content_base64 ?? ""),
    mobToml: String(d.mob_toml ?? ""),
    sourceFiles: asRecordArray(d.source_files).map(parseMobpackSourceFile),
    validation: parseMobpackValidationResult(d.validation),
  };
}

export interface MobpackImportResult {
  readonly document: Record<string, unknown>;
  readonly validation: MobpackValidationResult;
  readonly source: string;
  readonly sourceLabel: string;
  readonly sourceMediaType: string;
}

export function parseMobpackImportResult(raw: unknown): MobpackImportResult {
  const d = asRecord(raw);
  return {
    document: asRecord(d.document),
    validation: parseMobpackValidationResult(d.validation),
    source: String(d.source ?? ""),
    sourceLabel: String(d.source_label ?? ""),
    sourceMediaType: String(d.source_media_type ?? ""),
  };
}

/**
 * A row from the mobpack draft registry. The `document` and `validation`
 * payloads are passed through as permissive records — the mobpack document
 * schema is opaque to the SDK.
 */
export interface MobpackDraftRow {
  readonly id: string;
  readonly name: string;
  readonly version: string;
  readonly stage: string;
  readonly trigger: string;
  readonly source: string;
  readonly revision: number;
  readonly etag: string;
  readonly updatedAtUnixMs: number;
  readonly document: Record<string, unknown>;
  readonly validation: Record<string, unknown>;
  readonly canUndo: boolean | null;
  readonly canRedo: boolean | null;
}

export function parseMobpackDraftRow(raw: unknown): MobpackDraftRow {
  const d = asRecord(raw);
  return {
    id: String(d.id ?? ""),
    name: String(d.name ?? ""),
    version: String(d.version ?? ""),
    stage: String(d.stage ?? ""),
    trigger: String(d.trigger ?? ""),
    source: String(d.source ?? ""),
    revision: Number(d.revision ?? 0),
    etag: String(d.etag ?? ""),
    updatedAtUnixMs: Number(d.updated_at_unix_ms ?? 0),
    document: asRecord(d.document),
    validation: asRecord(d.validation),
    canUndo: d.can_undo == null ? null : Boolean(d.can_undo),
    canRedo: d.can_redo == null ? null : Boolean(d.can_redo),
  };
}

export interface MobpackDraftListResult {
  readonly source: string;
  readonly storePath: string | null;
  readonly runtimeBacked: boolean;
  readonly rows: readonly MobpackDraftRow[];
}

export function parseMobpackDraftListResult(
  raw: unknown,
): MobpackDraftListResult {
  const d = asRecord(raw);
  return {
    source: String(d.source ?? ""),
    storePath: d.store_path == null ? null : String(d.store_path),
    runtimeBacked: Boolean(d.runtime_backed),
    rows: asRecordArray(d.rows).map(parseMobpackDraftRow),
  };
}

export interface MobpackDraftGetResult {
  readonly source: string;
  readonly storePath: string | null;
  readonly runtimeBacked: boolean;
  readonly row: MobpackDraftRow;
}

export function parseMobpackDraftGetResult(
  raw: unknown,
): MobpackDraftGetResult {
  const d = asRecord(raw);
  return {
    source: String(d.source ?? ""),
    storePath: d.store_path == null ? null : String(d.store_path),
    runtimeBacked: Boolean(d.runtime_backed),
    row: parseMobpackDraftRow(d.row),
  };
}

/** Result shape shared by mobkit/mobpacks/create and mobkit/mobpacks/save. */
export interface MobpackDraftSaveResult {
  readonly source: string;
  readonly storePath: string | null;
  readonly row: MobpackDraftRow;
  readonly rows: readonly MobpackDraftRow[];
}

export function parseMobpackDraftSaveResult(
  raw: unknown,
): MobpackDraftSaveResult {
  const d = asRecord(raw);
  return {
    source: String(d.source ?? ""),
    storePath: d.store_path == null ? null : String(d.store_path),
    row: parseMobpackDraftRow(d.row),
    rows: asRecordArray(d.rows).map(parseMobpackDraftRow),
  };
}

/**
 * Result shape shared by mobkit/mobpacks/undo and mobkit/mobpacks/redo.
 * `stepped` is false (with a `reason`) when there is no history or future
 * entry to step to; the draft is left untouched in that case.
 */
export interface MobpackDraftHistoryResult {
  readonly source: string;
  readonly storePath: string | null;
  readonly stepped: boolean;
  readonly reason: string | null;
  readonly row: MobpackDraftRow;
  readonly rows: readonly MobpackDraftRow[];
}

export function parseMobpackDraftHistoryResult(
  raw: unknown,
): MobpackDraftHistoryResult {
  const d = asRecord(raw);
  return {
    source: String(d.source ?? ""),
    storePath: d.store_path == null ? null : String(d.store_path),
    stepped: Boolean(d.stepped),
    reason: d.reason == null ? null : String(d.reason),
    row: parseMobpackDraftRow(d.row),
    rows: asRecordArray(d.rows).map(parseMobpackDraftRow),
  };
}

export interface MobpackDraftDeleteResult {
  readonly source: string;
  readonly storePath: string | null;
  readonly id: string;
  readonly deleted: boolean;
  readonly rows: readonly MobpackDraftRow[];
}

export function parseMobpackDraftDeleteResult(
  raw: unknown,
): MobpackDraftDeleteResult {
  const d = asRecord(raw);
  return {
    source: String(d.source ?? ""),
    storePath: d.store_path == null ? null : String(d.store_path),
    id: String(d.id ?? ""),
    deleted: Boolean(d.deleted),
    rows: asRecordArray(d.rows).map(parseMobpackDraftRow),
  };
}

export interface MobpackApplyOperationResult {
  readonly source: string;
  readonly operation: string;
  readonly ok: boolean;
  readonly document: Record<string, unknown>;
  readonly selection: Record<string, unknown> | null;
  readonly validation: MobpackValidationResult;
}

export function parseMobpackApplyOperationResult(
  raw: unknown,
): MobpackApplyOperationResult {
  const d = asRecord(raw);
  return {
    source: String(d.source ?? ""),
    operation: String(d.operation ?? ""),
    ok: Boolean(d.ok),
    document: asRecord(d.document),
    selection: d.selection == null ? null : asRecord(d.selection),
    validation: parseMobpackValidationResult(d.validation),
  };
}

export interface MobpackDeployCommandResult {
  readonly command: string;
  readonly argv: readonly string[];
  readonly deployCommand: string;
  readonly filename: string;
  readonly validation: MobpackValidationResult;
  readonly source: string;
}

export function parseMobpackDeployCommandResult(
  raw: unknown,
): MobpackDeployCommandResult {
  const d = asRecord(raw);
  return {
    command: String(d.command ?? ""),
    argv: asStringArray(d.argv),
    deployCommand: String(d.deploy_command ?? ""),
    filename: String(d.filename ?? ""),
    validation: parseMobpackValidationResult(d.validation),
    source: String(d.source ?? ""),
  };
}

export interface MobpackDeployResult {
  readonly filename: string;
  readonly packPath: string;
  readonly packSha256: string;
  readonly command: string;
  readonly argv: readonly string[];
  readonly planTrace: readonly Record<string, unknown>[];
  readonly executed: boolean;
  readonly success: boolean;
  readonly statusCode: number | null;
  readonly stdout: string | null;
  readonly stderr: string | null;
  readonly validation: MobpackValidationResult;
  readonly displayRows: readonly MobpackDisplayRow[];
}

export function parseMobpackDeployResult(raw: unknown): MobpackDeployResult {
  const d = asRecord(raw);
  return {
    filename: String(d.filename ?? ""),
    packPath: String(d.pack_path ?? ""),
    packSha256: String(d.pack_sha256 ?? ""),
    command: String(d.command ?? ""),
    argv: asStringArray(d.argv),
    planTrace: asRecordArray(d.plan_trace),
    executed: Boolean(d.executed),
    success: Boolean(d.success),
    statusCode: d.status_code == null ? null : Number(d.status_code),
    stdout: d.stdout == null ? null : String(d.stdout),
    stderr: d.stderr == null ? null : String(d.stderr),
    validation: parseMobpackValidationResult(d.validation),
    displayRows: asRecordArray(d.display_rows).map(parseMobpackDisplayRow),
  };
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
    case "identity_materialization_failure": {
      const identity = String(context.identity ?? "");
      const initiator = String(context.initiator ?? "");
      const operation = String(context.operation ?? "");
      const target = initiator ? `${identity} for ${initiator}` : identity;
      message = [target, operation, error].filter(Boolean).join(": ");
      break;
    }
    default:
      message = JSON.stringify(d);
      break;
  }

  return { category, message, context };
}

// =========================================================================
// Identity-First Continuity Types
// =========================================================================

// -- DurableAgentSpec (REQ-46) --------------------------------------------

export interface DurableAgentSpec {
  readonly identity: string;
  readonly profile: string;
  readonly addressability: string;
  readonly displayName: string | null;
  readonly labels: Readonly<Record<string, string>>;
  readonly context: unknown | null;
  readonly additionalInstructions: readonly string[];
  readonly runtimeModeOverride?: "autonomous_host" | "turn_driven" | null;
  readonly backend?: "session" | "external" | null;
  readonly binding?: Readonly<Record<string, unknown>> | null;
}

export function parseDurableAgentSpec(raw: unknown): DurableAgentSpec {
  const d = asRecord(raw);
  return {
    identity: validateAgentIdentity(String(d.identity ?? "")),
    profile: String(d.profile ?? ""),
    addressability: String(d.addressability ?? "addressable"),
    displayName: typeof d.display_name === "string" ? d.display_name : null,
    labels: asStringRecord(d.labels),
    context: d.context !== undefined && d.context !== null ? d.context : null,
    additionalInstructions: asStringArray(d.additional_instructions),
    runtimeModeOverride:
      d.runtime_mode_override === "autonomous_host" || d.runtime_mode_override === "turn_driven"
        ? d.runtime_mode_override
        : null,
    backend:
      d.backend === "session" || d.backend === "external" ? d.backend : null,
    binding:
      d.binding != null && typeof d.binding === "object"
        ? { ...(d.binding as Record<string, unknown>) }
        : null,
  };
}

export function durableAgentSpecToDict(
  spec: DurableAgentSpec,
): Record<string, unknown> {
  const result: Record<string, unknown> = {
    identity: spec.identity,
    profile: spec.profile,
    addressability: spec.addressability,
  };
  if (spec.displayName !== null) result.display_name = spec.displayName;
  if (Object.keys(spec.labels).length > 0) result.labels = { ...spec.labels };
  if (spec.context !== null) result.context = spec.context;
  if (spec.additionalInstructions.length > 0) {
    result.additional_instructions = [...spec.additionalInstructions];
  }
  if (spec.runtimeModeOverride) {
    result.runtime_mode_override = spec.runtimeModeOverride;
  }
  if (spec.backend) result.backend = spec.backend;
  if (spec.binding) result.binding = { ...spec.binding };
  return result;
}

// -- DispatchContentBlock + DispatchInput (REQ-49) ------------------------

export interface TextContentBlock {
  readonly type: "text";
  readonly text: string;
}

export interface InlineImageContentBlock {
  readonly type: "image";
  readonly mediaType: string;
  readonly source?: "inline";
  readonly data: string;
}

export interface BlobImageContentBlock {
  readonly type: "image";
  readonly mediaType: string;
  readonly source: "blob";
  readonly blobId: string;
}

export type ImageContentBlock = InlineImageContentBlock | BlobImageContentBlock;

export interface BlobGetResult {
  readonly blobId: string;
  readonly mediaType: string;
  readonly size: number;
  readonly data: string;
}

export function parseBlobGetResult(raw: unknown): BlobGetResult {
  const d = asRecord(raw);
  return {
    blobId: String(d.blob_id ?? d.blobId ?? ""),
    mediaType: String(d.media_type ?? d.mediaType ?? ""),
    size: Number(d.size ?? 0) || 0,
    data: String(d.data ?? ""),
  };
}

export interface BlobUploadResult {
  readonly blobId: string;
  readonly mediaType: string;
  readonly size: number;
}

export function parseBlobUploadResult(raw: unknown): BlobUploadResult {
  const d = asRecord(raw);
  return {
    blobId: String(d.blob_id ?? d.blobId ?? ""),
    mediaType: String(d.media_type ?? d.mediaType ?? ""),
    size: Number(d.size ?? 0) || 0,
  };
}

export type DispatchContentBlock = TextContentBlock | ImageContentBlock;

export type DispatchOrigin =
  | "connector"
  | "scheduler"
  | "policy"
  | "flow"
  | "system";

export interface DispatchInput {
  readonly content: string | DispatchContentBlock[];
  readonly origin: DispatchOrigin;
  readonly correlationId?: string;
  readonly idempotencyKey?: string;
}

function parseContentBlock(raw: unknown): DispatchContentBlock {
  const d = asRecord(raw);
  if (d.type === "image") {
    const mediaType = String(d.media_type ?? d.mediaType ?? "");
    if (d.source === "blob") {
      return {
        type: "image",
        mediaType,
        source: "blob",
        blobId: String(d.blob_id ?? d.blobId ?? ""),
      };
    }
    return {
      type: "image",
      mediaType,
      source: "inline",
      data: String(d.data ?? ""),
    };
  }
  return { type: "text", text: String(d.text ?? "") };
}

export function contentBlockToDict(
  block: DispatchContentBlock,
): Record<string, unknown> {
  if (block.type === "image") {
    if (block.source === "blob") {
      return {
        type: "image",
        media_type: block.mediaType,
        source: "blob",
        blob_id: block.blobId,
      };
    }
    return {
      type: "image",
      media_type: block.mediaType,
      source: "inline",
      data: block.data,
    };
  }
  return { type: "text", text: block.text };
}

const DISPATCH_ORIGIN_VALUES: readonly DispatchOrigin[] = [
  "connector",
  "scheduler",
  "policy",
  "flow",
  "system",
];

function parseDispatchOrigin(raw: unknown): DispatchOrigin {
  // Pre-fix this was `String(d.origin ?? "system") as DispatchOrigin`
  // — an unchecked cast that admitted arbitrary strings, so a
  // consumer's `switch (input.origin)` over the closed set hit no
  // branch silently. Validate against the union and fall back to
  // "system" for anything else.
  if (typeof raw === "string" && (DISPATCH_ORIGIN_VALUES as readonly string[]).includes(raw)) {
    return raw as DispatchOrigin;
  }
  return "system";
}

export function parseDispatchInput(raw: unknown): DispatchInput {
  const d = asRecord(raw);
  let content: string | DispatchContentBlock[];
  if (typeof d.content === "string") {
    content = d.content;
  } else if (Array.isArray(d.content)) {
    content = d.content.map(parseContentBlock);
  } else {
    content = "";
  }
  return {
    content,
    origin: parseDispatchOrigin(d.origin),
    correlationId: typeof d.correlation_id === "string" ? d.correlation_id : undefined,
    idempotencyKey: typeof d.idempotency_key === "string" ? d.idempotency_key : undefined,
  };
}

export function dispatchInputToDict(
  input: DispatchInput,
): Record<string, unknown> {
  const result: Record<string, unknown> = {
    origin: input.origin,
  };
  if (typeof input.content === "string") {
    result.content = input.content;
  } else {
    result.content = input.content.map(contentBlockToDict);
  }
  if (input.correlationId !== undefined) {
    result.correlation_id = input.correlationId;
  }
  if (input.idempotencyKey !== undefined) {
    result.idempotency_key = input.idempotencyKey;
  }
  return result;
}

// -- ManagedPeerEdge (REQ-49a) --------------------------------------------

export interface ManagedPeerEdge {
  readonly a: string;
  readonly b: string;
}

export function parseManagedPeerEdge(raw: unknown): ManagedPeerEdge {
  const d = asRecord(raw);
  const a = validateAgentIdentity(String(d.a ?? ""));
  const b = validateAgentIdentity(String(d.b ?? ""));
  if (a === b) {
    throw new Error(`managed peer edge cannot connect an identity to itself: ${a}`);
  }
  return a < b ? { a, b } : { a: b, b: a };
}

export function managedPeerEdgeToDict(edge: ManagedPeerEdge): Record<string, string> {
  const parsed = parseManagedPeerEdge(edge);
  return { a: parsed.a, b: parsed.b };
}

// -- ExternalToolDef (REQ-49a) --------------------------------------------

export interface ExternalToolDef {
  readonly name: string;
  readonly description: string;
  readonly inputSchema: Record<string, unknown>;
}

export function parseExternalToolDef(raw: unknown): ExternalToolDef {
  const d = asRecord(raw);
  return {
    name: String(d.name ?? ""),
    description: String(d.description ?? ""),
    inputSchema: asRecord(d.input_schema),
  };
}

export function externalToolDefToDict(
  tool: ExternalToolDef,
): Record<string, unknown> {
  return {
    name: tool.name,
    description: tool.description,
    input_schema: tool.inputSchema,
  };
}

// -- AgentBuildContext (REQ-49a) -------------------------------------------

export interface AgentBuildContext {
  readonly identity: string;
  readonly activePeers: readonly string[];
  readonly managedEdges: readonly ManagedPeerEdge[];
}

export function parseAgentBuildContext(raw: unknown): AgentBuildContext {
  const d = asRecord(raw);
  const rawEdges = Array.isArray(d.managed_edges) ? d.managed_edges : [];
  return {
    identity: String(d.identity ?? ""),
    activePeers: asStringArray(d.active_peers),
    managedEdges: rawEdges.map(parseManagedPeerEdge),
  };
}

// -- AgentBuildDraft (REQ-49a) --------------------------------------------

export interface AgentBuildDraft {
  model: string | null;
  systemPrompt: string | null;
  additionalInstructions: string[];
  labels: Record<string, string>;
  appContext: unknown | null;
  externalTools: ExternalToolDef[];
}

export function parseAgentBuildDraft(raw: unknown): AgentBuildDraft {
  const d = asRecord(raw);
  const rawTools = Array.isArray(d.external_tools) ? d.external_tools : [];
  return {
    model: typeof d.model === "string" ? d.model : null,
    systemPrompt: typeof d.system_prompt === "string" ? d.system_prompt : null,
    additionalInstructions: asStringArray(d.additional_instructions),
    labels: asStringRecord(d.labels),
    appContext: d.app_context !== undefined && d.app_context !== null ? d.app_context : null,
    externalTools: rawTools.map(parseExternalToolDef),
  };
}

export function agentBuildDraftToDict(
  draft: AgentBuildDraft,
): Record<string, unknown> {
  const result: Record<string, unknown> = {};
  if (draft.model !== null) result.model = draft.model;
  if (draft.systemPrompt !== null) result.system_prompt = draft.systemPrompt;
  if (draft.additionalInstructions.length > 0) {
    result.additional_instructions = [...draft.additionalInstructions];
  }
  if (Object.keys(draft.labels).length > 0) result.labels = { ...draft.labels };
  if (draft.appContext !== null) result.app_context = draft.appContext;
  if (draft.externalTools.length > 0) {
    result.external_tools = draft.externalTools.map(externalToolDefToDict);
  }
  return result;
}

// -- LeaseInfo (REQ-49b) --------------------------------------------------

export interface LeaseInfo {
  readonly fencingToken: number;
  readonly ttlRemainingMs: number;
  readonly healthy: boolean;
}

function parseLeaseInfo(raw: unknown): LeaseInfo | null {
  if (raw == null || typeof raw !== "object") return null;
  const d = raw as Record<string, unknown>;
  return {
    fencingToken: Number(d.fencing_token ?? 0),
    ttlRemainingMs: Number(d.ttl_remaining_ms ?? 0),
    healthy: Boolean(d.healthy ?? false),
  };
}

// -- DurabilityPolicy (REQ-49b) -------------------------------------------

export interface DurabilityPolicy {
  readonly kind: "syncWriteThrough" | "asyncReplicated" | "bufferedExport";
  readonly maxLossWindowMs?: number;
}

function parseDurabilityPolicy(raw: unknown): DurabilityPolicy {
  const d = asRecord(raw);
  const wireKind = String(d.kind ?? "sync_write_through");
  let kind: DurabilityPolicy["kind"];
  switch (wireKind) {
    case "async_replicated":
      kind = "asyncReplicated";
      break;
    case "buffered_export":
      kind = "bufferedExport";
      break;
    default:
      kind = "syncWriteThrough";
      break;
  }
  const result: DurabilityPolicy = { kind };
  if (kind === "bufferedExport" && d.max_loss_window_ms !== undefined) {
    return { kind, maxLossWindowMs: Number(d.max_loss_window_ms) };
  }
  return result;
}

// -- ContinuityHealth (REQ-49b) -------------------------------------------

export interface ContinuityHealth {
  readonly storeReachable: boolean;
  readonly durabilityPolicy: DurabilityPolicy;
  readonly lastCheckpointVersion: number | null;
}

function parseContinuityHealth(raw: unknown): ContinuityHealth | null {
  if (raw == null || typeof raw !== "object") return null;
  const d = raw as Record<string, unknown>;
  return {
    storeReachable: Boolean(d.store_reachable ?? false),
    durabilityPolicy: parseDurabilityPolicy(d.durability_policy),
    lastCheckpointVersion:
      typeof d.last_checkpoint_version === "number"
        ? d.last_checkpoint_version
        : null,
  };
}

// -- IdentityStatus (REQ-49b) ---------------------------------------------

export interface IdentityStatus {
  readonly identity: string;
  readonly lifecycleState: string;
  readonly agentRuntimeId: string;
  readonly sessionId: string;
  readonly profile: string;
  readonly addressability: string;
  readonly displayName: string | null;
  readonly labels: Readonly<Record<string, string>>;
  readonly generation: number;
  readonly checkpointVersion: number;
  readonly lease: LeaseInfo | null;
  readonly continuityHealth: ContinuityHealth | null;
}

export function parseIdentityStatus(raw: unknown): IdentityStatus {
  const d = asRecord(raw);
  return {
    identity: String(d.identity ?? ""),
    lifecycleState: String(d.state ?? ""),
    agentRuntimeId: String(d.agent_runtime_id ?? ""),
    sessionId: String(d.session_id ?? ""),
    profile: String(d.profile ?? ""),
    addressability: String(d.addressability ?? "addressable"),
    displayName: typeof d.display_name === "string" ? d.display_name : null,
    labels: asStringRecord(d.labels),
    generation: Number(d.generation ?? 0),
    checkpointVersion: Number(d.checkpoint_version ?? 0),
    lease: parseLeaseInfo(d.lease),
    continuityHealth: parseContinuityHealth(d.continuity_health),
  };
}

// -- ContinuityRecord (REQ-49c) -------------------------------------------

export interface ContinuityRecord {
  readonly identity: string;
  readonly agentRuntimeId: string;
  readonly sessionId: string;
  readonly generation: number;
  /**
   * Monotonic for (identity, generation). It advances across session rotations
   * and resets only after destructive reset advances generation.
   */
  readonly checkpointVersion: number;
}

export function parseContinuityRecord(raw: unknown): ContinuityRecord {
  const d = asRecord(raw);
  return {
    identity: String(d.identity ?? ""),
    agentRuntimeId: String(d.agent_runtime_id ?? ""),
    sessionId: String(d.session_id ?? ""),
    generation: Number(d.generation ?? 0),
    checkpointVersion: Number(d.checkpoint_version ?? 0),
  };
}

export function continuityRecordToDict(
  record: ContinuityRecord,
): Record<string, unknown> {
  return {
    identity: record.identity,
    agent_runtime_id: record.agentRuntimeId,
    session_id: record.sessionId,
    generation: record.generation,
    checkpoint_version: record.checkpointVersion,
  };
}

// -- ContinuityFailure (REQ-49c) ------------------------------------------

export interface ContinuityFailure {
  readonly identity: string;
  readonly kind: string;
  readonly record?: ContinuityRecord;
  readonly detail: string;
}

export function parseContinuityFailure(raw: unknown): ContinuityFailure {
  const d = asRecord(raw);
  return {
    identity: String(d.identity ?? ""),
    kind: String(d.kind ?? ""),
    record: d.record != null ? parseContinuityRecord(d.record) : undefined,
    detail: String(d.detail ?? ""),
  };
}

// -- ContinuityResolveState (REQ-49c) -------------------------------------

export interface ContinuityResolveState {
  readonly state: "uninitialized" | "ready" | "broken";
  readonly record?: ContinuityRecord;
  readonly failure?: ContinuityFailure;
}

export function parseContinuityResolveState(
  raw: unknown,
): ContinuityResolveState {
  const d = asRecord(raw);
  const state = String(d.state ?? "uninitialized") as ContinuityResolveState["state"];
  return {
    state,
    record: d.record != null ? parseContinuityRecord(d.record) : undefined,
    failure: d.failure != null ? parseContinuityFailure(d.failure) : undefined,
  };
}

// -- SessionSnapshot (REQ-49c) --------------------------------------------

export interface SessionSnapshot {
  readonly data: Uint8Array;
}

export function parseSessionSnapshot(raw: unknown): SessionSnapshot {
  const d = asRecord(raw);
  const b64 = String(d.data ?? "");
  const binary = atob(b64);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i++) {
    bytes[i] = binary.charCodeAt(i);
  }
  return { data: bytes };
}

export function sessionSnapshotToDict(
  snap: SessionSnapshot,
): Record<string, string> {
  let binary = "";
  for (let i = 0; i < snap.data.length; i++) {
    binary += String.fromCharCode(snap.data[i]);
  }
  return { data: btoa(binary) };
}

// -- LeaseGrant (REQ-49c) -------------------------------------------------

export interface LeaseGrant {
  readonly identity: string;
  readonly fencingToken: number;
  readonly ttlMs: number;
}

export function parseLeaseGrant(raw: unknown): LeaseGrant {
  const d = asRecord(raw);
  return {
    identity: String(d.identity ?? ""),
    fencingToken: Number(d.fencing_token ?? 0),
    ttlMs: Number(d.ttl ?? d.ttl_ms ?? 0),
  };
}

export function leaseGrantToDict(grant: LeaseGrant): Record<string, unknown> {
  return {
    identity: grant.identity,
    fencing_token: grant.fencingToken,
    ttl: grant.ttlMs,
  };
}

// -- LeaseAcquireResult (REQ-49c) -----------------------------------------

export interface LeaseAcquireResult {
  readonly status: "acquired" | "alreadyHeld";
  readonly identity?: string;
  readonly grant?: LeaseGrant;
  readonly holder?: string;
}

export function parseLeaseAcquireResult(raw: unknown): LeaseAcquireResult {
  const d = asRecord(raw);
  const wireStatus = String(d.result ?? d.status ?? "acquired");
  const status = wireStatus === "already_held" ? "alreadyHeld" as const : "acquired" as const;
  return {
    status,
    identity: typeof d.identity === "string" ? d.identity : undefined,
    grant: d.grant != null
      ? parseLeaseGrant(d.grant)
      : status === "acquired"
        ? parseLeaseGrant(d)
        : undefined,
    holder: typeof d.holder === "string" ? d.holder : undefined,
  };
}

export function leaseAcquireResultToDict(
  result: LeaseAcquireResult,
): Record<string, unknown> {
  if (result.status === "alreadyHeld") {
    return {
      result: "already_held",
      identity: result.identity ?? result.grant?.identity ?? "",
      holder: result.holder ?? "",
    };
  }
  const grant = result.grant;
  return {
    result: "acquired",
    ...(grant ? leaseGrantToDict(grant) : {}),
  };
}

// -- LeaseRenewResult (REQ-49c) -------------------------------------------

export interface LeaseRenewResult {
  readonly status: "renewed" | "lost";
  readonly identity?: string;
  readonly grant?: LeaseGrant;
}

export function parseLeaseRenewResult(raw: unknown): LeaseRenewResult {
  const d = asRecord(raw);
  const status = String(d.result ?? d.status ?? "renewed") as LeaseRenewResult["status"];
  return {
    status,
    identity: typeof d.identity === "string" ? d.identity : undefined,
    grant: d.grant != null
      ? parseLeaseGrant(d.grant)
      : status === "renewed"
        ? parseLeaseGrant(d)
        : undefined,
  };
}

export function leaseRenewResultToDict(
  result: LeaseRenewResult,
): Record<string, unknown> {
  if (result.status === "lost") {
    return {
      result: "lost",
      identity: result.identity ?? result.grant?.identity ?? "",
    };
  }
  const grant = result.grant;
  return {
    result: "renewed",
    ...(grant ? leaseGrantToDict(grant) : {}),
  };
}

// -- Provider interfaces (REQ-48) -----------------------------------------

export interface ContinuityStore {
  resolveMany(identities: string[]): Promise<Record<string, ContinuityResolveState>>;
  loadSessionSnapshot(sessionId: string): Promise<SessionSnapshot | null>;
  /** Save only if fencingToken is current and version advances the identity/generation head. */
  saveSessionSnapshot(
    identity: string,
    sessionId: string,
    generation: number,
    version: number,
    fencingToken: number,
    snapshot: SessionSnapshot,
  ): Promise<void>;
  /** Persist the binding without rewinding checkpointVersion on session rebind. */
  upsertContinuityRecord(record: ContinuityRecord, fencingToken: number): Promise<void>;
  deleteContinuityRecord(identity: string, fencingToken: number): Promise<void>;
  deleteSessionSnapshotIfCurrentRevision?(
    sessionId: string,
    expectedCurrentRevision: string,
  ): Promise<boolean>;
}

export interface LeaseProvider {
  acquireLeases(
    identities: string[],
    runtimeInstance: string,
  ): Promise<Record<string, LeaseAcquireResult>>;
  renewLeases(grants: LeaseGrant[]): Promise<Record<string, LeaseRenewResult>>;
  releaseLeases(grants: LeaseGrant[]): Promise<void>;
}

export interface RosterProvider {
  roster(context: unknown): Promise<DurableAgentSpec[]>;
}

export interface AgentCustomizer {
  customizeBuild(
    context: AgentBuildContext,
    spec: DurableAgentSpec,
    draft: AgentBuildDraft,
  ): Promise<void>;
  afterCreate?(
    identity: string,
    sessionId: string,
    context: SessionCreatedContext,
  ): Promise<void>;
}

export interface TopologyProvider {
  computeEdges(
    targetIdentities: string[],
    context: unknown,
  ): Promise<ManagedPeerEdge[]>;
}
