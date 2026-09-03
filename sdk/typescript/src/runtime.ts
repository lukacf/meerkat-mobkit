/**
 * MobKit runtime — the running instance returned by the builder.
 *
 * @example
 * ```ts
 * const rt = await MobKit.builder().mob("mob.toml").gateway(bin).build();
 * const handle = rt.mobHandle();
 *
 * const status = await handle.status();
 * console.log(status.contractVersion, status.loadedModules);
 *
 * await rt.shutdown();
 * ```
 */

import { request as httpRequest } from "node:http";
import { request as httpsRequest } from "node:https";
import { readFileSync } from "node:fs";

import type { MobKitBuilderConfig } from "./builder.js";
import {
  CallbackDispatcher,
  type SessionAgentBuilder,
} from "./agent-builder.js";
import {
  MOB_EVENTS_STALE_CURSOR_CODE,
  CAPABILITY_UNAVAILABLE_CODE,
  CONSOLE_TIMELINE_REPLAY_UNAVAILABLE_CODE,
  LEASE_LOST_CODE,
  MEMORY_BACKEND_UNAVAILABLE_CODE,
  STORAGE_RESOLUTION_CODE,
  WORKGRAPH_UNAVAILABLE_CODE,
  WORKGRAPH_CONFLICT_CODE,
  CapabilityUnavailableError,
  ConsoleTimelineReplayUnavailableError,
  LeaseLostError,
  MemoryBackendUnavailableError,
  MobEventsStaleError,
  NotConnectedError,
  RpcError,
  StorageResolutionError,
  TransportError,
  WorkGraphUnavailableError,
  WorkGraphConflictError,
  isRpcError,
} from "./errors.js";
import { PersistentTransport, buildJsonRpcRequest } from "./transport.js";
import { parseSseStream, type SseEvent } from "./sse.js";
import {
  EventStream,
  parseAgentEventFromSse,
  parseMobEventFromSse,
  type AgentEventEnvelope,
  type MobEventEnvelope,
} from "./events.js";
import { discoverySpecToDict, type DiscoverySpec } from "./models.js";
import {
  LIVE_EXECUTION_IDENTITY_V1,
  activeLiveChannelHandleToWire,
  type ActiveLiveChannelConnection,
  type ActiveLiveChannelHandle,
  type ExperimentalLiveChannelStatus,
  experimentalLiveGatewayConfigToWire,
  liveOpenExecutionIdentityParams,
  type LivePlaybackOwner,
  type LivePlaybackOwnerReadiness,
  type LiveAssistantOutputAddress,
  parseExperimentalLiveChannelStatus,
  parseLiveChannelHandle,
  parseLivePlaybackOwnerReadiness,
  parsePendingLiveChannelHandle,
  parseLivePlaybackCompleteResult,
  parseLiveReplacementRequired,
  supportsLiveExecutionIdentityV1,
  supportsLiveExecutionMode,
  type LiveChannelHandle,
  type LiveExecutionIdentityV1,
  type LivePlaybackCompleteResult,
  type LiveReplacementRequired,
  type PendingLiveChannelHandle,
} from "./live.js";
import {
  parseStatusResult,
  parseStorageDoctorResult,
  parseCapabilitiesResult,
  parseReconcileResult,
  parseSpawnResult,
  parseSubscribeResult,
  parseSendMessageResult,
  parseRoutingResolution,
  parseDeliveryResult,
  parseDeliveryHistoryResult,
  parseMemoryQueryResult,
  parseAgentMemoryRecord,
  parseAgentMemoryRecallResult,
  parseAgentMemoryForgetResult,
  parseAgentMemoryUpdateResult,
  parseAgentMemoryManifestResult,
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
  parseMobStructuralEvent,
  parseMobRun,
  parseRichMemberSnapshot,
  parseIdentityResolvedToolsResult,
  parseIdentityRoutingStatusResult,
  parseHelperResult,
  parseMobRunSnapshot,
  parseCrossMobContactEntry,
  parseModelsCatalogResult,
  parseMobpackToolsCatalogResult,
  parseMobpackSkillsCatalogResult,
  parseMobpackAgentDefinitionsResult,
  parseMobpackTemplatesResult,
  parseMobpackCatalogsResult,
  parseMobpackValidationResult,
  parseMobpackSourceResult,
  parseMobpackExportResult,
  parseMobpackImportResult,
  parseMobpackDraftListResult,
  parseMobpackDraftGetResult,
  parseMobpackDraftSaveResult,
  parseMobpackDraftDeleteResult,
  parseMobpackDraftHistoryResult,
  parseMobpackApplyOperationResult,
  parseMobpackDeployCommandResult,
  parseMobpackDeployResult,
  eventQueryToDict,
  parseIdentityStatus,
  parseIdentityInspection,
  parseSendResult,
  parseDispatchResult,
  completionProgressSince,
  parseBlobGetResult,
  parseBlobUploadResult,
  dispatchInputToDict,
  contentBlockToDict,
  parseWorkGraphItem,
  parseWorkGraphEdge,
  parseWorkGraphAttentionBinding,
  parseWorkGraphSnapshotResult,
  parseWorkGraphItemsResult,
  parseWorkGraphGoalResult,
  parseWorkGraphAttentionReassignResult,
  parseWorkGraphEventEntry,
  workGraphFilterOptionsToDict,
  workGraphReadyOptionsToDict,
  workGraphEventsOptionsToDict,
  workGraphAttentionListOptionsToDict,
  workGraphCreateOptionsToDict,
  workGraphUpdateOptionsToDict,
  workGraphOwnerInputToDict,
  workGraphClaimOptionsToDict,
  workGraphCloseOptionsToDict,
  workGraphEvidenceInputToDict,
  workGraphGoalTargetToDict,
  workGraphGoalCreateOptionsToDict,
  workGraphGoalConfirmOptionsToDict,
  workGraphGoalRequestCloseOptionsToDict,
  workGraphAttentionPauseOptionsToDict,
  type StatusResult,
  type StorageDoctorResult,
  type CapabilitiesResult,
  type ReconcileResult,
  type SpawnResult,
  type SubscribeResult,
  type SendMessageResult,
  type RoutingResolution,
  type DeliveryResult,
  type DeliveryHistoryResult,
  type MemoryQueryResult,
  type AgentMemoryRecord,
  type AgentMemoryRecordMeta,
  type AgentMemoryForgetResult,
  type AgentMemoryUpdateResult,
  type MemoryStoreInfo,
  type MemoryIndexResult,
  type CallToolResult,
  type MemberSnapshot,
  type RuntimeRouteResult,
  type GatingEvaluateResult,
  type GatingDecisionResult,
  type GatingAuditEntry,
  type GatingPendingEntry,
  type RediscoverReport,
  type ReconcileEdgesReport,
  type PersistedEvent,
  type MobStructuralEvent,
  type MobRun,
  type RichMemberSnapshot,
  type IdentityResolvedToolsResult,
  type IdentityRoutingStatusResult,
  type HelperResult,
  type MobRunSnapshot,
  type CrossMobContactEntry,
  type ModelsCatalogResult,
  type MobpackToolsCatalogResult,
  type MobpackSkillsCatalogResult,
  type MobpackAgentDefinitionsResult,
  type MobpackTemplatesResult,
  type MobpackCatalogsResult,
  type MobpackValidationResult,
  type MobpackSourceResult,
  type MobpackExportResult,
  type MobpackImportResult,
  type MobpackDraftListResult,
  type MobpackDraftGetResult,
  type MobpackDraftSaveResult,
  type MobpackDraftDeleteResult,
  type MobpackDraftHistoryResult,
  type MobpackApplyOperationResult,
  type MobpackDeployCommandResult,
  type MobpackDeployResult,
  type EventQuery,
  type IdentityStatus,
  type IdentityInspection,
  type SendResult,
  type DispatchResult,
  type CompletionCursor,
  type BlobGetResult,
  type BlobUploadResult,
  type DispatchInput,
  type DispatchContentBlock,
  type WorkGraphItem,
  type WorkGraphEdge,
  type WorkGraphAttentionBinding,
  type WorkGraphSnapshotResult,
  type WorkGraphItemsResult,
  type WorkGraphGoalResult,
  type WorkGraphAttentionReassignResult,
  type WorkGraphEventEntry,
  type WorkGraphFilterOptions,
  type WorkGraphReadyOptions,
  type WorkGraphEventsOptions,
  type WorkGraphAttentionListOptions,
  type WorkGraphCreateOptions,
  type WorkGraphUpdateOptions,
  type WorkGraphOwnerInput,
  type WorkGraphClaimOptions,
  type WorkGraphCloseOptions,
  type WorkGraphEvidenceInput,
  type WorkGraphGoalTarget,
  type WorkGraphGoalCreateOptions,
  type WorkGraphGoalConfirmOptions,
  type WorkGraphGoalRequestCloseOptions,
  type WorkGraphAttentionPauseOptions,
} from "./types.js";

// -- Request ID counter ---------------------------------------------------

let requestCounter = 0;
function nextRequestId(method: string): string {
  return `${method}:${++requestCounter}`;
}

/** Narrow an RPC result to a plain record, or `{}` for non-object results. */
function asWireRecord(raw: unknown): Record<string, unknown> {
  return typeof raw === "object" && raw !== null
    ? (raw as Record<string, unknown>)
    : {};
}

function extractMobStructuralEvents(raw: unknown): MobStructuralEvent[] {
  let events: unknown = raw;
  if (typeof raw === "object" && raw !== null) {
    const record = raw as Record<string, unknown>;
    if (Array.isArray(record.events)) {
      events = record.events;
    }
  }
  if (!Array.isArray(events)) {
    return [];
  }
  return events.map(parseMobStructuralEvent);
}

/** Serialize a config value for JSON transport.
 *
 * Recursively walks dicts/arrays with cycle detection. Calls toDict()
 * on objects that have it (e.g. typed config classes). Cyclic or
 * non-serializable leaves become their constructor name.
 */
function serializeConfig(
  value: unknown,
  seen: WeakSet<object> = new WeakSet(),
): unknown {
  if (value === null || value === undefined) return value;
  if (
    typeof value === "boolean" ||
    typeof value === "number" ||
    typeof value === "string"
  )
    return value;
  if (typeof value !== "object") return String(value);
  const obj = value as object;
  if (seen.has(obj)) {
    return `[circular:${obj.constructor?.name ?? "Object"}]`;
  }
  seen.add(obj);
  if ("toDict" in (obj as Record<string, unknown>)) {
    return (obj as { toDict(): unknown }).toDict();
  }
  if (Array.isArray(obj)) {
    return obj.map((v) => serializeConfig(v, seen));
  }
  const result: Record<string, unknown> = {};
  for (const [k, v] of Object.entries(obj as Record<string, unknown>)) {
    result[k] = serializeConfig(v, seen);
  }
  return result;
}

export interface BlobUploadInput {
  readonly blob: Blob;
  readonly mediaType?: string;
  readonly filename?: string;
  readonly alt?: string;
}

export type BlobUploadSource = Blob | BlobUploadInput;

export interface SendMessageOptions {
  readonly attachments?: readonly BlobUploadSource[];
  readonly handlingMode?: "queue" | "steer";
}

export interface RememberAgentMemoryOptions {
  readonly title: string;
  readonly body: string;
  readonly tags?: readonly string[];
  readonly realm?: string;
}

export interface RecallAgentMemoryOptions {
  readonly realm?: string;
  readonly selection?: "always" | "contextual";
  readonly queryText?: string;
  readonly queryTerms?: readonly string[];
  readonly maxEntries?: number;
}

export interface ForgetAgentMemoryOptions {
  readonly realm?: string;
}

export interface UpdateAgentMemoryOptions {
  readonly title: string;
  readonly body: string;
  readonly tags?: readonly string[];
  readonly realm?: string;
}

export interface ManifestAgentMemoryOptions {
  readonly realm?: string;
  readonly tier?: "working_set" | "full";
  readonly k?: number;
}

/** Input alternatives for {@link MobHandle.mobpackImport}. */
export interface MobpackImportOptions {
  readonly mobToml?: string;
  readonly contentBase64?: string;
  readonly document?: Record<string, unknown>;
  readonly sourceName?: string;
}

/** Options for {@link MobHandle.mobpackCreate}. */
export interface MobpackCreateOptions {
  readonly template?: string;
  readonly name?: string;
  readonly trigger?: string;
}

/** Options for {@link MobHandle.mobpackSave}. */
export interface MobpackSaveOptions {
  readonly validation?: Record<string, unknown>;
  readonly stage?: string;
  readonly expectedRevision?: number;
  readonly expectedEtag?: string;
}

/** Options for {@link MobHandle.mobpackUndo} and {@link MobHandle.mobpackRedo}. */
export interface MobpackHistoryOptions {
  readonly expectedRevision?: number;
  readonly expectedEtag?: string;
}

function mobpackHistoryParams(
  draftId: string,
  options?: MobpackHistoryOptions,
): Record<string, unknown> {
  const params: Record<string, unknown> = { id: draftId };
  if (options?.expectedRevision !== undefined) {
    params.expected_revision = options.expectedRevision;
  }
  if (options?.expectedEtag !== undefined) {
    params.expected_etag = options.expectedEtag;
  }
  return params;
}

interface NormalizedBlobUpload {
  readonly uploadId: string;
  readonly blob: Blob;
  readonly mediaType: string;
  readonly filename: string;
  readonly alt?: string;
}

function normalizeBlobUpload(
  input: BlobUploadSource,
  index: number,
): NormalizedBlobUpload {
  const record = input instanceof Blob ? { blob: input } : input;
  const blob = record.blob;
  const mediaType = record.mediaType || blob.type || "application/octet-stream";
  const extension = mediaType.includes("/") ? mediaType.split("/")[1] : "bin";
  return {
    uploadId: `upload-${index + 1}`,
    blob,
    mediaType,
    filename: record.filename || `attachment-${index + 1}.${extension}`,
    alt: record.alt,
  };
}

// -- MobKitRuntime --------------------------------------------------------

export interface JobsListOptions {
  readonly sessionId: string;
  readonly limit?: number;
}

export interface JobSubscriptionOptions {
  readonly subscriptionId: string;
  readonly sessionId: string;
  readonly delivery: Record<string, unknown>;
}

export interface MonitorStartOptions {
  readonly sessionId: string;
  readonly submissionKey: string;
  readonly command: string;
  readonly timeoutSecs: number;
  readonly restartClass:
    | "checkpoint_resumable"
    | "replayable"
    | "non_resumable";
  readonly delivery: Record<string, unknown>;
  readonly protocol?: "framed_jsonl" | "lines";
  readonly workingDir?: string;
  readonly maxLineBytes?: number;
  readonly maxNotificationsPerWindow?: number;
  readonly notificationWindowMs?: number;
  readonly maxRetainedDiagnosticBytes?: number;
}

/** Safe application-facing durable-job surface. */
export class JobsHandle {
  /** @internal */
  constructor(private readonly runtime: MobKitRuntime) {}

  get(jobId: string): Promise<unknown> {
    return this.runtime._rpc("jobs/get", { job_id: jobId });
  }

  list(options: JobsListOptions): Promise<unknown> {
    const params: Record<string, unknown> = { session_id: options.sessionId };
    if (options.limit !== undefined) params.limit = options.limit;
    return this.runtime._rpc("jobs/list", params);
  }

  cancel(jobId: string): Promise<unknown> {
    return this.runtime._rpc("jobs/cancel", { job_id: jobId });
  }

  progress(jobId: string): Promise<unknown> {
    return this.runtime._rpc("jobs/progress", { job_id: jobId });
  }

  result(jobId: string): Promise<unknown> {
    return this.runtime._rpc("jobs/result", { job_id: jobId });
  }

  artifacts(jobId: string): Promise<unknown> {
    return this.runtime._rpc("jobs/artifacts", { job_id: jobId });
  }

  retry(jobId: string, retryDueAtMs: number): Promise<unknown> {
    return this.runtime._rpc("jobs/retry", {
      job_id: jobId,
      retry_due_at_ms: retryDueAtMs,
    });
  }

  health(): Promise<unknown> {
    return this.runtime._rpc("jobs/health", {});
  }

  subscribe(jobId: string, options: JobSubscriptionOptions): Promise<unknown> {
    return this.runtime._rpc("jobs/subscribe", {
      job_id: jobId,
      subscription_id: options.subscriptionId,
      session_id: options.sessionId,
      delivery: options.delivery,
    });
  }

  unsubscribe(jobId: string, subscriptionId: string): Promise<unknown> {
    return this.runtime._rpc("jobs/unsubscribe", {
      job_id: jobId,
      subscription_id: subscriptionId,
    });
  }
}

/** Durable monitor convenience surface; lifecycle remains job-owned. */
export class MonitorsHandle {
  /** @internal */
  constructor(private readonly runtime: MobKitRuntime) {}

  start(options: MonitorStartOptions): Promise<unknown> {
    const params: Record<string, unknown> = {
      session_id: options.sessionId,
      submission_key: options.submissionKey,
      command: options.command,
      timeout_secs: options.timeoutSecs,
      protocol: options.protocol ?? "framed_jsonl",
      restart_class: options.restartClass,
      delivery: options.delivery,
    };
    if (options.workingDir !== undefined) params.working_dir = options.workingDir;
    if (options.maxLineBytes !== undefined) params.max_line_bytes = options.maxLineBytes;
    if (options.maxNotificationsPerWindow !== undefined) {
      params.max_notifications_per_window = options.maxNotificationsPerWindow;
    }
    if (options.notificationWindowMs !== undefined) {
      params.notification_window_ms = options.notificationWindowMs;
    }
    if (options.maxRetainedDiagnosticBytes !== undefined) {
      params.max_retained_diagnostic_bytes = options.maxRetainedDiagnosticBytes;
    }
    return this.runtime._rpc("monitors/start", params);
  }
}

/**
 * Running MobKit runtime instance.
 *
 * Supports explicit lifecycle (`connect` / `shutdown`).
 */
export class MobKitRuntime {
  private _config: MobKitBuilderConfig;
  private _transport: PersistentTransport | null;
  private _running = false;
  private _lifecycleTail: Promise<void> = Promise.resolve();
  private _lifecycleTailResult: Promise<void> | null = null;
  private _lifecycleTailIntent: "connect" | "shutdown" | null = null;
  private _lifecycleSequence = 0;
  private _dispatcher = new CallbackDispatcher();
  private _rustHttpBase: string | null = null;
  private _rustHttpPublicBase: string | null = null;
  readonly jobs = new JobsHandle(this);
  readonly monitors = new MonitorsHandle(this);

  /** @internal */
  constructor(config: MobKitBuilderConfig, transport?: PersistentTransport) {
    this._config = config;
    this._transport = transport ?? null;
  }

  /** @internal */
  static async _create(config: MobKitBuilderConfig): Promise<MobKitRuntime> {
    const runtime = new MobKitRuntime(config);
    await runtime.connect();
    return runtime;
  }

  /** Explicitly connect to the runtime. Idempotent. */
  async connect(): Promise<void> {
    if (this._running && this._lifecycleTailIntent === null) return;
    if (
      this._lifecycleTailIntent === "connect" &&
      this._lifecycleTailResult !== null
    ) {
      return this._lifecycleTailResult;
    }

    const previous = this._lifecycleTail;
    const sequence = ++this._lifecycleSequence;
    const operation = (async () => {
      await previous;
      if (this._running) return;

      try {
        await this._bootstrap();
      } catch (error) {
        // A failed bootstrap must not leave an unowned child behind for a
        // later connect attempt to overwrite.
        const transport = this._transport;
        this._transport = null;
        this._rustHttpBase = null;
        this._rustHttpPublicBase = null;
        if (transport !== null) {
          try {
            await transport.stop();
          } catch {
            // Preserve the bootstrap error, which carries the actionable
            // gateway/RPC failure.
          }
        }
        throw error;
      }

      // A later queued operation owns the final state. Do not briefly reopen
      // RPC admission when an ordered shutdown is already waiting behind us.
      if (this._lifecycleSequence === sequence) this._running = true;
    })();

    const connect = operation.finally(() => {
      if (this._lifecycleSequence === sequence) {
        this._lifecycleTailResult = null;
        this._lifecycleTailIntent = null;
      }
    });
    this._lifecycleTailResult = connect;
    this._lifecycleTailIntent = "connect";
    this._lifecycleTail = connect.then(
      () => undefined,
      () => undefined,
    );
    return connect;
  }

  private async _bootstrap(): Promise<void> {
    if (this._config.gatewayBin) {
      this._transport = new PersistentTransport(this._config.gatewayBin, {
        timeout: this._config.gatewayTimeoutMs ?? undefined,
      });

      // Register builder FIRST — init may trigger callback/build_agent
      if (this._config.sessionBuilder) {
        this._dispatcher.registerBuilder(this._config.sessionBuilder);
      }
      if (this._config.errorCallback !== null) {
        this._dispatcher.registerErrorCallback(this._config.errorCallback);
      }
      if (this._config.continuityStore !== null) {
        this._dispatcher.registerContinuityStore(this._config.continuityStore);
      }
      if (this._config.leaseProvider !== null) {
        this._dispatcher.registerLeaseProvider(this._config.leaseProvider);
      }
      if (this._config.rosterProvider !== null) {
        this._dispatcher.registerRosterProvider(this._config.rosterProvider);
      }
      if (this._config.topologyProvider !== null) {
        this._dispatcher.registerTopologyProvider(
          this._config.topologyProvider,
        );
      }
      if (this._config.agentCustomizer !== null) {
        this._dispatcher.registerAgentCustomizer(this._config.agentCustomizer);
      }
      this._dispatcher.registerJobRpc(
        (method, params) => this._rpcUnchecked(method, params),
      );
      if (this._config.jobCredentialResolver !== null) {
        this._dispatcher.registerJobCredentialResolver(
          this._config.jobCredentialResolver,
        );
      }
      this._transport.setCallbackHandler(
        this._dispatcher.handleCallback.bind(this._dispatcher),
      );
      this._transport.start();

      if (!this._transport.isRunning()) {
        throw new TransportError(
          `gateway binary failed to start: ${this._config.gatewayBin}`,
        );
      }

      try {
        const initResult = await this._rpcUnchecked(
          "mobkit/init",
          this._buildInitParams(),
        );
        if (
          typeof initResult === "object" &&
          initResult !== null &&
          "http_base_url" in initResult
        ) {
          this._rustHttpBase =
            String(
              (initResult as Record<string, unknown>).http_base_url ?? "",
            ) || null;
        }
        if (typeof initResult === "object" && initResult !== null) {
          const publicBase = (initResult as Record<string, unknown>)
            .http_public_base_url;
          this._rustHttpPublicBase =
            typeof publicBase === "string" && publicBase ? publicBase : null;
        }
      } catch (err) {
        // Pre-fix every error path here was rewritten to a generic
        // `TransportError`, destroying the original RPC code/message
        // (e.g. config errors like `bad mob.toml: line 7`). The fix:
        // re-throw a structured RpcError whenever one was received —
        // fail-closed init refusals (the typed StorageResolutionError)
        // write the error response and then exit, so the process being
        // dead does not make the structured error a transport failure.
        if (isRpcError(err)) {
          throw err;
        }
        if (this._transport !== null && !this._transport.isRunning()) {
          throw new TransportError("gateway process died during bootstrap");
        }
        throw new TransportError(
          `mobkit/init failed: ${err instanceof Error ? err.message : String(err)}`,
        );
      }
    } else if (this._config.sessionBuilder) {
      this._dispatcher.registerBuilder(this._config.sessionBuilder);
    } else {
      console.warn(
        "[mobkit] runtime started without gateway or session builder — " +
          "RPC calls will fail with NotConnectedError",
      );
    }
  }

  private _buildInitParams(): Record<string, unknown> {
    const params: Record<string, unknown> = {};
    if (this._config.mobConfigPath) {
      params.mob_config = readFileSync(this._config.mobConfigPath, "utf-8");
    }
    if (this._config.modules.length > 0) {
      params.modules = this._config.modules;
    }
    params.has_session_builder = Boolean(this._config.sessionBuilder);
    const runtimeOptions: Record<string, unknown> = {};
    if (this._config.gatingConfigPath) {
      runtimeOptions.gating_config_path = this._config.gatingConfigPath;
    }
    if (this._config.routingConfigPath) {
      runtimeOptions.routing_config_path = this._config.routingConfigPath;
    }
    if (this._config.workgraphEnabled !== null) {
      runtimeOptions.workgraph = this._config.workgraphEnabled;
    }
    if (this._config.memoryConfig) {
      runtimeOptions.memory_config = serializeConfig(this._config.memoryConfig);
    }
    if (this._config.agentMemoryConfig) {
      runtimeOptions.agent_memory = serializeConfig(this._config.agentMemoryConfig);
    }
    if (this._config.authConfig) {
      runtimeOptions.auth_config = serializeConfig(this._config.authConfig);
    }
    if (this._config.eventLog) {
      runtimeOptions.event_log = serializeConfig(this._config.eventLog);
    }
    if (this._config.runtimeStore) {
      runtimeOptions.runtime_store = serializeConfig(this._config.runtimeStore);
    }
    if (this._config.consoleConfigPath) {
      runtimeOptions.console_config_path = this._config.consoleConfigPath;
    }
    if (this._config.accessConfigPath) {
      runtimeOptions.access_config_path = this._config.accessConfigPath;
    }
    if (this._config.meerkatConfigPath) {
      runtimeOptions.meerkat_config_path = this._config.meerkatConfigPath;
    }
    if (this._config.consoleRequireAppAuth !== null) {
      runtimeOptions.console_require_app_auth =
        this._config.consoleRequireAppAuth;
    }
    if (this._config.consoleReadOnly !== null) {
      runtimeOptions.console_read_only = this._config.consoleReadOnly;
    }
    if (this._config.httpListen) {
      runtimeOptions.http_listen = this._config.httpListen;
    }
    if (this._config.httpPublicBaseUrl) {
      runtimeOptions.http_public_base_url = this._config.httpPublicBaseUrl;
    }
    if (typeof this._config.allowRemote === "boolean") {
      runtimeOptions.allow_remote = this._config.allowRemote;
    }
    if (this._config.consoleFetchTimeoutMs !== null) {
      runtimeOptions.console_fetch_timeout_ms =
        this._config.consoleFetchTimeoutMs;
    }
    if (this._config.demoLlm) {
      runtimeOptions.demo_llm = true;
    }
    if (this._config.memberCommsAddress) {
      runtimeOptions.member_comms_address = this._config.memberCommsAddress;
    }
    if (this._config.implicitDelegateIdleRetireSecs !== undefined) {
      runtimeOptions.implicit_delegate_idle_retire_secs =
        this._config.implicitDelegateIdleRetireSecs;
    }
    if (this._config.maxSessions !== null) {
      runtimeOptions.max_sessions = this._config.maxSessions;
    }
    if (this._config.experimentalLiveConfig != null) {
      runtimeOptions.experimental_live = experimentalLiveGatewayConfigToWire(
        this._config.experimentalLiveConfig,
      );
    }
    params.runtime_options = runtimeOptions;
    if (this._config.persistentState) {
      params.persistent_state = this._config.persistentState;
    }
    if (this._config.rosterProvider !== null) {
      params.has_roster_provider = true;
    }
    if (this._config.continuityStore !== null) {
      params.has_continuity_store = true;
    }
    if (this._config.leaseProvider !== null) {
      params.has_lease_provider = true;
    }
    if (this._config.scratchDir !== null) {
      params.scratch_dir = this._config.scratchDir;
    }
    if (this._config.topologyProvider !== null) {
      params.has_topology_provider = true;
    }
    if (this._config.agentCustomizer !== null) {
      params.has_agent_customizer = true;
    }
    return params;
  }

  /** @internal */
  async _rpc(
    method: string,
    params?: Record<string, unknown>,
  ): Promise<unknown> {
    if (!this._running) {
      throw new NotConnectedError(
        "runtime not started — no transport available",
      );
    }
    return this._rpcUnchecked(method, params);
  }

  private async _rpcUnchecked(
    method: string,
    params?: Record<string, unknown>,
  ): Promise<unknown> {
    if (this._transport === null) {
      throw new NotConnectedError(
        "runtime not started — no transport available",
      );
    }
    const rid = nextRequestId(method);
    const request = buildJsonRpcRequest(rid, method, params ?? {});
    const response = (await this._transport.sendAsync(
      request as unknown as Record<string, unknown>,
    )) as Record<string, unknown>;

    if ("error" in response) {
      const err = response.error as Record<string, unknown>;
      const code = Number(err.code ?? -1);
      const message = String(err.message ?? String(err));
      if (code === CAPABILITY_UNAVAILABLE_CODE) {
        throw new CapabilityUnavailableError(message, rid, method, err.data);
      }
      if (code === LEASE_LOST_CODE) {
        throw new LeaseLostError(message, rid, method, err.data);
      }
      if (code === MEMORY_BACKEND_UNAVAILABLE_CODE) {
        throw new MemoryBackendUnavailableError(message, rid, method, err.data);
      }
      if (code === CONSOLE_TIMELINE_REPLAY_UNAVAILABLE_CODE) {
        throw new ConsoleTimelineReplayUnavailableError(message, rid, method, err.data);
      }
      if (code === STORAGE_RESOLUTION_CODE) {
        throw new StorageResolutionError(message, rid, method, err.data);
      }
      if (code === WORKGRAPH_UNAVAILABLE_CODE) {
        throw new WorkGraphUnavailableError(message, rid, method, err.data);
      }
      if (code === WORKGRAPH_CONFLICT_CODE) {
        throw new WorkGraphConflictError(message, rid, method, err.data);
      }
      const rpcError = new RpcError(code, message, rid, method, err.data);
      if (code === MOB_EVENTS_STALE_CURSOR_CODE) {
        throw MobEventsStaleError.fromRpcError(rpcError);
      }
      throw rpcError;
    }
    return response.result;
  }

  get rustHttpBaseUrl(): string | null {
    return this._rustHttpBase;
  }

  /**
   * Base URL for clients that reach the gateway through a proxy: the
   * `http_public_base_url` the launch advertised (see
   * `MobKitBuilder.httpPublicBaseUrl`), falling back to `rustHttpBaseUrl`
   * when none was declared. The SDK's own SSE, multipart and console RPC
   * calls keep using `rustHttpBaseUrl`: this process spawned the gateway
   * and shares its network namespace, so the same-host form always answers.
   */
  get rustHttpPublicBaseUrl(): string | null {
    return this._rustHttpPublicBase ?? this._rustHttpBase;
  }

  setRustHttpBase(url: string): void {
    this._rustHttpBase = url;
  }

  /** @internal */
  _registerLiveOutputConsumer(
    channelId: string,
    consumer: (output: LiveAssistantOutputAddress) => void,
  ): () => void {
    return this._dispatcher.registerLiveOutputConsumer(channelId, consumer);
  }

  mobHandle(): MobHandle {
    return new MobHandle(this);
  }

  sseBridge(): SseBridge {
    return new SseBridge(this);
  }

  async shutdown(): Promise<void> {
    // Close public RPC admission synchronously, even when teardown must wait
    // for an already-admitted connect to finish its ordered bootstrap.
    this._running = false;
    if (
      this._lifecycleTailIntent === "shutdown" &&
      this._lifecycleTailResult !== null
    ) {
      return this._lifecycleTailResult;
    }

    const previous = this._lifecycleTail;
    const sequence = ++this._lifecycleSequence;
    const operation = (async () => {
      await previous;
      this._running = false;
      const transport = this._transport;
      this._transport = null;
      this._rustHttpBase = null;
      this._rustHttpPublicBase = null;
      if (transport !== null) await transport.stop();
    })();

    const shutdown = operation.finally(() => {
      if (this._lifecycleSequence === sequence) {
        this._lifecycleTailResult = null;
        this._lifecycleTailIntent = null;
      }
    });
    this._lifecycleTailResult = shutdown;
    this._lifecycleTailIntent = "shutdown";
    this._lifecycleTail = shutdown.then(
      () => undefined,
      () => undefined,
    );
    return shutdown;
  }

  get isRunning(): boolean {
    return this._running;
  }

  // -- Identity-first APIs (REQ-47) -----------------------------------------

  /** Get agent snapshot by identity. */
  async agent(identity: string): Promise<MemberSnapshot> {
    const status = parseIdentityStatus(
      await this._rpc("mobkit/status_identity", { identity }),
    );
    return {
      agentIdentity: status.agentRuntimeId || status.identity,
      role: status.profile,
      state: status.lifecycleState,
      wiredTo: [],
      labels: status.labels,
    };
  }

  /**
   * Send content to an identity. Content can be a string or content blocks.
   *
   * The returned `completionBaseline` is what {@link waitForCompletion} needs
   * to wait for THIS turn. Never wait by comparing `outputPreview` text —
   * consecutive turns may emit identical output.
   */
  async send(
    identity: string,
    content: string | DispatchContentBlock[],
  ): Promise<SendResult> {
    const params: Record<string, unknown> = { identity };
    if (typeof content === "string") {
      params.content = content;
    } else {
      params.content = content.map(contentBlockToDict);
    }
    return parseSendResult(await this._rpc("mobkit/send", params));
  }

  /** Dispatch structured input to an identity. */
  async dispatch(
    identity: string,
    input: DispatchInput,
  ): Promise<DispatchResult> {
    return parseDispatchResult(
      await this._rpc("mobkit/dispatch", {
        identity,
        dispatch_input: dispatchInputToDict(input),
      }),
    );
  }

  /**
   * Wait until a turn completes past `baseline`, returning the
   * `outputPreview` observed at completion.
   *
   * `baseline` is the `completionBaseline` from {@link send} or
   * {@link dispatch}. This compares cursors, so two consecutive turns emitting
   * byte-identical text are still two distinct completions.
   *
   * Throws if the wait times out, if the identity exposes no cursor, or if the
   * runtime incarnation changed — turn counts do not carry across
   * incarnations, so that is reported rather than guessed at either way.
   */
  async waitForCompletion(
    identity: string,
    baseline: CompletionCursor,
    options: { timeoutMs?: number; pollIntervalMs?: number } = {},
  ): Promise<string | null> {
    const timeoutMs = options.timeoutMs ?? 90_000;
    const pollIntervalMs = options.pollIntervalMs ?? 500;
    const deadline = Date.now() + timeoutMs;
    for (;;) {
      const inspection = await this.inspectIdentity(identity);
      const cursor = inspection.completionCursor;
      if (cursor === null) {
        throw new Error(
          `identity ${identity} reports no completion cursor; the gateway ` +
            `predates the completion contract or this is a live alias with ` +
            `no identity authority`,
        );
      }
      const progress = completionProgressSince(cursor, baseline);
      if (progress === "completed") return inspection.outputPreview;
      if (progress === "incarnation_changed") {
        throw new Error(
          `completion baseline ${baseline.epoch}:${baseline.turns} for ` +
            `identity ${identity} belongs to a superseded runtime ` +
            `incarnation (now ${cursor.epoch}:${cursor.turns}); capture a ` +
            `fresh baseline`,
        );
      }
      const remaining = deadline - Date.now();
      if (remaining <= 0) {
        throw new Error(
          `identity ${identity} did not complete a turn past ` +
            `${baseline.epoch}:${baseline.turns} within ${timeoutMs}ms`,
        );
      }
      await new Promise((resolve) =>
        setTimeout(resolve, Math.min(pollIntervalMs, remaining)),
      );
    }
  }

  /** Send, then wait for the completion of the turn that send started. */
  async sendAndWait(
    identity: string,
    content: string | DispatchContentBlock[],
    options: { timeoutMs?: number; pollIntervalMs?: number } = {},
  ): Promise<string | null> {
    const result = await this.send(identity, content);
    return this._waitForAdmission(
      identity,
      result.completionBaseline,
      "send",
      options,
    );
  }

  /** Dispatch, then wait for the completion of the turn it started. */
  async dispatchAndWait(
    identity: string,
    input: DispatchInput,
    options: { timeoutMs?: number; pollIntervalMs?: number } = {},
  ): Promise<string | null> {
    const result = await this.dispatch(identity, input);
    return this._waitForAdmission(
      identity,
      result.completionBaseline,
      "dispatch",
      options,
    );
  }

  private async _waitForAdmission(
    identity: string,
    baseline: CompletionCursor | null,
    operation: string,
    options: { timeoutMs?: number; pollIntervalMs?: number },
  ): Promise<string | null> {
    if (baseline === null) {
      throw new Error(
        `${operation} returned no completion_baseline for identity ` +
          `${identity}; the gateway predates the completion contract, so the ` +
          `turn cannot be awaited without an unsound text comparison`,
      );
    }
    return this.waitForCompletion(identity, baseline, options);
  }

  /** Subscribe to events for an identity. */
  async subscribe(identity: string): Promise<unknown> {
    return this._rpc("mobkit/subscribe", { identity });
  }

  /** Get identity status. */
  async status(identity: string): Promise<IdentityStatus> {
    return parseIdentityStatus(
      await this._rpc("mobkit/status_identity", { identity }),
    );
  }

  /** Respawn an identity (non-destructive recovery). */
  async respawn(identity: string): Promise<unknown> {
    return this._rpc("mobkit/respawn", { identity });
  }

  /** Retire an identity. */
  async retire(identity: string): Promise<unknown> {
    return this._rpc("mobkit/retire", { identity });
  }

  /** Reset an identity (destructive continuity reset). */
  async reset(identity: string): Promise<unknown> {
    return this._rpc("mobkit/reset", { identity });
  }

  /** Delete an identity permanently. */
  async deleteIdentity(identity: string): Promise<unknown> {
    return this._rpc("mobkit/delete_identity", { identity });
  }

  /** Inspect identity continuity/runtime state. */
  async inspectIdentity(identity: string): Promise<IdentityInspection> {
    return parseIdentityInspection(
      await this._rpc("mobkit/inspect_identity", { identity }),
    );
  }

  /** Re-run identity-first reconciliation. */
  async reconcileIdentity(): Promise<unknown> {
    return this._rpc("mobkit/reconcile_identity", {});
  }
}

// -- MobHandle ------------------------------------------------------------

/**
 * Proxy for the MobKit RPC API. Returns typed result objects.
 *
 * @example
 * ```ts
 * const handle = runtime.mobHandle();
 * const members = await handle.listMembers();
 * await handle.send(members[0].agentIdentity, "Hello!");
 * ```
 */
export class MobHandle {
  /** @internal */
  constructor(private readonly _runtime: MobKitRuntime) {}

  // -- Status & capabilities ----------------------------------------------

  async status(): Promise<StatusResult> {
    return parseStatusResult(await this._runtime._rpc("mobkit/status"));
  }

  async capabilities(): Promise<CapabilitiesResult> {
    return parseCapabilitiesResult(
      await this._runtime._rpc("mobkit/capabilities"),
    );
  }

  async modelsCatalog(): Promise<ModelsCatalogResult> {
    return parseModelsCatalogResult(
      await this._runtime._rpc("mobkit/models/catalog"),
    );
  }

  /**
   * Run the read-only storage doctor over a MobKit state directory.
   *
   * `stateDir` is required until the gateway can report its own persistent
   * state directory (Phase M2); omitting it rejects with
   * `CapabilityUnavailableError`. `identity` narrows the continuity
   * checkpoint census to one identity.
   */
  async storageDoctor(options?: {
    stateDir?: string;
    identity?: string;
  }): Promise<StorageDoctorResult> {
    const params: Record<string, unknown> = {};
    if (options?.stateDir !== undefined) params.state_dir = options.stateDir;
    if (options?.identity !== undefined) params.identity = options.identity;
    return parseStorageDoctorResult(
      await this._runtime._rpc("mobkit/storage/doctor", params),
    );
  }

  async toolsCatalog(): Promise<MobpackToolsCatalogResult> {
    return parseMobpackToolsCatalogResult(
      await this._runtime._rpc("mobkit/tools/catalog"),
    );
  }

  async skillsCatalog(): Promise<MobpackSkillsCatalogResult> {
    return parseMobpackSkillsCatalogResult(
      await this._runtime._rpc("mobkit/skills/catalog"),
    );
  }

  async agentDefinitions(): Promise<MobpackAgentDefinitionsResult> {
    return parseMobpackAgentDefinitionsResult(
      await this._runtime._rpc("mobkit/agent_definitions/list"),
    );
  }

  async mobpackTemplates(): Promise<MobpackTemplatesResult> {
    return parseMobpackTemplatesResult(
      await this._runtime._rpc("mobkit/mobpacks/templates"),
    );
  }

  async mobpackCatalogs(): Promise<MobpackCatalogsResult> {
    return parseMobpackCatalogsResult(
      await this._runtime._rpc("mobkit/mobpacks/catalogs"),
    );
  }

  // -- Mobpack authoring ----------------------------------------------------

  /** Validate a mobpack authoring document. */
  async mobpackValidate(
    document: Record<string, unknown>,
    rkatValidate?: boolean,
  ): Promise<MobpackValidationResult> {
    const params: Record<string, unknown> = { document };
    if (rkatValidate !== undefined) params.rkat_validate = rkatValidate;
    return parseMobpackValidationResult(
      await this._runtime._rpc("mobkit/mobpacks/validate", params),
    );
  }

  /** Render the deployable source files (mob.toml etc.) for a document. */
  async mobpackSource(
    document: Record<string, unknown>,
  ): Promise<MobpackSourceResult> {
    return parseMobpackSourceResult(
      await this._runtime._rpc("mobkit/mobpacks/source", { document }),
    );
  }

  /** Export a mobpack document as a base64-encoded archive. */
  async mobpackExport(
    document: Record<string, unknown>,
  ): Promise<MobpackExportResult> {
    return parseMobpackExportResult(
      await this._runtime._rpc("mobkit/mobpacks/export", { document }),
    );
  }

  /** Import a mob.toml, mobpack archive, or editor document. */
  async mobpackImport(
    options: MobpackImportOptions,
  ): Promise<MobpackImportResult> {
    const params: Record<string, unknown> = {};
    if (options.mobToml !== undefined) params.mob_toml = options.mobToml;
    if (options.contentBase64 !== undefined) {
      params.content_base64 = options.contentBase64;
    }
    if (options.document !== undefined) params.document = options.document;
    if (options.sourceName !== undefined) {
      params.source_name = options.sourceName;
    }
    return parseMobpackImportResult(
      await this._runtime._rpc("mobkit/mobpacks/import", params),
    );
  }

  /** List mobpack draft registry rows. */
  async mobpackList(): Promise<MobpackDraftListResult> {
    return parseMobpackDraftListResult(
      await this._runtime._rpc("mobkit/mobpacks/list", {}),
    );
  }

  /** Fetch a single mobpack draft registry row by id. */
  async mobpackGet(draftId: string): Promise<MobpackDraftGetResult> {
    return parseMobpackDraftGetResult(
      await this._runtime._rpc("mobkit/mobpacks/get", { id: draftId }),
    );
  }

  /** Create a new mobpack draft from a starter template. */
  async mobpackCreate(
    options?: MobpackCreateOptions,
  ): Promise<MobpackDraftSaveResult> {
    const params: Record<string, unknown> = {};
    if (options?.template !== undefined) params.template = options.template;
    if (options?.name !== undefined) params.name = options.name;
    if (options?.trigger !== undefined) params.trigger = options.trigger;
    return parseMobpackDraftSaveResult(
      await this._runtime._rpc("mobkit/mobpacks/create", params),
    );
  }

  /** Save a mobpack draft, optionally guarded by revision/etag. */
  async mobpackSave(
    draftId: string,
    document: Record<string, unknown>,
    options?: MobpackSaveOptions,
  ): Promise<MobpackDraftSaveResult> {
    const params: Record<string, unknown> = { id: draftId, document };
    if (options?.validation !== undefined) {
      params.validation = options.validation;
    }
    if (options?.stage !== undefined) params.stage = options.stage;
    if (options?.expectedRevision !== undefined) {
      params.expected_revision = options.expectedRevision;
    }
    if (options?.expectedEtag !== undefined) {
      params.expected_etag = options.expectedEtag;
    }
    return parseMobpackDraftSaveResult(
      await this._runtime._rpc("mobkit/mobpacks/save", params),
    );
  }

  /** Delete a mobpack draft, optionally guarded by revision. */
  async mobpackDelete(
    draftId: string,
    expectedRevision?: number,
  ): Promise<MobpackDraftDeleteResult> {
    const params: Record<string, unknown> = { id: draftId };
    if (expectedRevision !== undefined) {
      params.expected_revision = expectedRevision;
    }
    return parseMobpackDraftDeleteResult(
      await this._runtime._rpc("mobkit/mobpacks/delete", params),
    );
  }

  /** Step a mobpack draft one entry back in its undo history. */
  async mobpackUndo(
    draftId: string,
    options?: MobpackHistoryOptions,
  ): Promise<MobpackDraftHistoryResult> {
    return parseMobpackDraftHistoryResult(
      await this._runtime._rpc(
        "mobkit/mobpacks/undo",
        mobpackHistoryParams(draftId, options),
      ),
    );
  }

  /** Step a mobpack draft one entry forward in its redo history. */
  async mobpackRedo(
    draftId: string,
    options?: MobpackHistoryOptions,
  ): Promise<MobpackDraftHistoryResult> {
    return parseMobpackDraftHistoryResult(
      await this._runtime._rpc(
        "mobkit/mobpacks/redo",
        mobpackHistoryParams(draftId, options),
      ),
    );
  }

  /** Apply a structured authoring operation to a mobpack document. */
  async mobpackApplyOperation(
    document: Record<string, unknown>,
    operation: Record<string, unknown>,
    expectedCatalogSnapshotId?: string,
  ): Promise<MobpackApplyOperationResult> {
    const params: Record<string, unknown> = { document, operation };
    if (expectedCatalogSnapshotId !== undefined) {
      params.expected_catalog_snapshot_id = expectedCatalogSnapshotId;
    }
    return parseMobpackApplyOperationResult(
      await this._runtime._rpc("mobkit/mobpacks/apply_operation", params),
    );
  }

  /** Preview the `rkat mob run` deploy command for a mobpack document. */
  async mobpackDeployCommand(
    document: Record<string, unknown>,
  ): Promise<MobpackDeployCommandResult> {
    return parseMobpackDeployCommandResult(
      await this._runtime._rpc("mobkit/mobpacks/deploy_command", { document }),
    );
  }

  /** Plan (and optionally execute) a mobpack deploy on the host. */
  async mobpackDeploy(
    document: Record<string, unknown>,
    execute?: boolean,
  ): Promise<MobpackDeployResult> {
    const params: Record<string, unknown> = { document };
    if (execute !== undefined) params.execute = execute;
    return parseMobpackDeployResult(
      await this._runtime._rpc("mobkit/mobpacks/deploy", params),
    );
  }

  // -- Spawn & reconcile --------------------------------------------------

  async spawn(spec: DiscoverySpec): Promise<SpawnResult> {
    return parseSpawnResult(
      await this._runtime._rpc(
        "mobkit/spawn_member",
        discoverySpecToDict(spec),
      ),
    );
  }

  async spawnMember(moduleId: string): Promise<SpawnResult> {
    return parseSpawnResult(
      await this._runtime._rpc("mobkit/spawn_member", { module_id: moduleId }),
    );
  }

  async reconcile(modules: string[]): Promise<ReconcileResult> {
    return parseReconcileResult(
      await this._runtime._rpc("mobkit/reconcile", { modules }),
    );
  }

  // -- Event subscription -------------------------------------------------

  async subscribeEvents(
    scope = "mob",
    lastEventId?: string,
    agentId?: string,
  ): Promise<SubscribeResult> {
    const params: Record<string, unknown> = { scope };
    if (lastEventId !== undefined) params.last_event_id = lastEventId;
    if (agentId !== undefined) params.agent_id = agentId;
    return parseSubscribeResult(
      await this._runtime._rpc("mobkit/events/subscribe", params),
    );
  }

  async *subscribeAgent(
    memberId: string,
  ): AsyncGenerator<AgentEventEnvelope, void, undefined> {
    const bridge = this._runtime.sseBridge();
    for await (const sse of bridge.agentEvents(memberId)) {
      yield parseAgentEventFromSse(sse);
    }
  }

  async *subscribeMob(): AsyncGenerator<MobEventEnvelope, void, undefined> {
    const bridge = this._runtime.sseBridge();
    for await (const sse of bridge.mobEvents()) {
      yield parseMobEventFromSse(sse);
    }
  }

  async queryEvents(query?: EventQuery): Promise<PersistedEvent[]> {
    const params = query ? eventQueryToDict(query) : {};
    const raw = await this._runtime._rpc("mobkit/query_events", params);
    let events = raw;
    if (typeof raw === "object" && raw !== null) {
      const record = raw as Record<string, unknown>;
      if (record.status === "no_event_log_configured") {
        events = Array.isArray(record.events) ? record.events : [];
      }
    }
    if (Array.isArray(events)) {
      return events.map(parsePersistedEvent);
    }
    return [];
  }

  /**
   * Query structural mob events from the meerkat ledger. Pass the
   * highest seen `cursor` as `EventQuery.afterSeq` on the next call to
   * paginate. Without `afterSeq` the call returns the latest matching
   * events (default `limit = 256`).
   *
   * Throws {@link MobEventsStaleError} when `afterSeq` is past the
   * current ledger frontier; the exception carries `afterCursor` and
   * `latestCursor` so callers can rewind.
   */
  async queryMobEvents(query?: EventQuery): Promise<MobStructuralEvent[]> {
    const params = query ? eventQueryToDict(query) : {};
    try {
      const raw = await this._runtime._rpc("mobkit/mob_events/query", params);
      return extractMobStructuralEvents(raw);
    } catch (err) {
      if (isRpcError(err) && err.code === MOB_EVENTS_STALE_CURSOR_CODE) {
        throw MobEventsStaleError.fromRpcError(err);
      }
      throw err;
    }
  }

  /**
   * Replay structural mob events as an async iterator. Yields the
   * snapshot frame returned by `mobkit/mob_events/subscribe`. Live
   * tailing for production streaming uses the SSE bridge at
   * `/mobkit/mob_events/stream`.
   *
   * Throws {@link MobEventsStaleError} when `afterSeq` is past the
   * current ledger frontier.
   */
  async *subscribeMobEvents(
    query?: EventQuery,
  ): AsyncGenerator<MobStructuralEvent, void, undefined> {
    const params = query ? eventQueryToDict(query) : {};
    let raw: unknown;
    try {
      raw = await this._runtime._rpc("mobkit/mob_events/subscribe", params);
    } catch (err) {
      if (isRpcError(err) && err.code === MOB_EVENTS_STALE_CURSOR_CODE) {
        throw MobEventsStaleError.fromRpcError(err);
      }
      throw err;
    }
    for (const event of extractMobStructuralEvents(raw)) {
      yield event;
    }
  }

  // -- Messaging ----------------------------------------------------------

  async send(
    memberId: string,
    message: string | DispatchContentBlock[],
    options?: SendMessageOptions,
  ): Promise<SendMessageResult> {
    const params: Record<string, unknown> = { member_id: memberId };
    if (options?.handlingMode) {
      params.handling_mode = options.handlingMode;
    }
    const uploads = options?.attachments?.map(normalizeBlobUpload) ?? [];
    if (typeof message === "string") {
      if (uploads.length > 0) {
        params.content = [
          ...(message.trim() ? [{ type: "text", text: message }] : []),
          ...uploads.map((upload) => ({
            type: "image_upload",
            upload_id: upload.uploadId,
            media_type: upload.mediaType,
            ...(upload.alt ? { alt: upload.alt } : {}),
          })),
        ];
      } else {
        params.message = message;
      }
    } else {
      params.content = [
        ...message.map(contentBlockToDict),
        ...uploads.map((upload) => ({
          type: "image_upload",
          upload_id: upload.uploadId,
          media_type: upload.mediaType,
          ...(upload.alt ? { alt: upload.alt } : {}),
        })),
      ];
    }
    if (uploads.length > 0) {
      return parseSendMessageResult(
        await this._multipartRpc("mobkit/send_message", params, uploads),
      );
    }
    return parseSendMessageResult(
      await this._runtime._rpc("mobkit/send_message", params),
    );
  }

  /** Alias for {@link send}. */
  sendMessage = this.send.bind(this);

  async getBlob(blobId: string): Promise<BlobGetResult> {
    return parseBlobGetResult(
      await this._runtime._rpc("mobkit/blob/get", { blob_id: blobId }),
    );
  }

  async uploadBlob(file: BlobUploadSource): Promise<BlobUploadResult> {
    const upload = normalizeBlobUpload(file, 0);
    return parseBlobUploadResult(
      await this._multipartRpc(
        "mobkit/blob/upload",
        {
          upload: {
            type: "image_upload",
            upload_id: upload.uploadId,
            media_type: upload.mediaType,
            ...(upload.alt ? { alt: upload.alt } : {}),
          },
        },
        [upload],
      ),
    );
  }

  async upload_blob(file: BlobUploadSource): Promise<BlobUploadResult> {
    return this.uploadBlob(file);
  }

  private async _multipartRpc(
    method: string,
    params: Record<string, unknown>,
    uploads: readonly NormalizedBlobUpload[],
  ): Promise<unknown> {
    const baseUrl = this._runtime.rustHttpBaseUrl;
    if (baseUrl === null) {
      throw new NotConnectedError(
        "multipart RPC requires rustHttpBaseUrl — start the gateway or call runtime.setRustHttpBase(...)",
      );
    }
    const id = nextRequestId(method);
    const form = new FormData();
    form.append(
      "payload",
      JSON.stringify(buildJsonRpcRequest(id, method, params)),
    );
    for (const upload of uploads) {
      const blob =
        upload.blob.type === upload.mediaType
          ? upload.blob
          : upload.blob.slice(0, upload.blob.size, upload.mediaType);
      form.append(`file:${upload.uploadId}`, blob, upload.filename);
    }
    const response = await fetch(
      `${baseUrl.replace(/\/$/, "")}/console/rpc/multipart`,
      {
        method: "POST",
        body: form,
      },
    );
    const responseText = await response.text();
    let body: Record<string, unknown> | null = null;
    if (responseText.trim() !== "") {
      try {
        body = JSON.parse(responseText) as Record<string, unknown>;
      } catch {
        if (!response.ok) {
          throw new TransportError(
            `multipart RPC failed (status=${response.status}): ${responseText}`,
          );
        }
        throw new TransportError("multipart RPC returned non-JSON response");
      }
    }
    if (!response.ok && body === null) {
      throw new TransportError(
        `multipart RPC failed (status=${response.status}): ${response.statusText}`,
      );
    }
    if (body === null) {
      throw new TransportError("multipart RPC returned an empty response");
    }
    if ("error" in body) {
      const err = body.error as Record<string, unknown>;
      const code = Number(err.code ?? -1);
      const message = String(err.message ?? String(err));
      if (code === CAPABILITY_UNAVAILABLE_CODE) {
        throw new CapabilityUnavailableError(message, id, method, err.data);
      }
      if (code === LEASE_LOST_CODE) {
        throw new LeaseLostError(message, id, method, err.data);
      }
      if (code === MEMORY_BACKEND_UNAVAILABLE_CODE) {
        throw new MemoryBackendUnavailableError(message, id, method, err.data);
      }
      if (code === CONSOLE_TIMELINE_REPLAY_UNAVAILABLE_CODE) {
        throw new ConsoleTimelineReplayUnavailableError(message, id, method, err.data);
      }
      if (code === STORAGE_RESOLUTION_CODE) {
        throw new StorageResolutionError(message, id, method, err.data);
      }
      const rpcError = new RpcError(code, message, id, method, err.data);
      if (code === MOB_EVENTS_STALE_CURSOR_CODE) {
        throw MobEventsStaleError.fromRpcError(rpcError);
      }
      throw rpcError;
    }
    if (!response.ok) {
      throw new TransportError(
        `multipart RPC failed (status=${response.status}): ${responseText}`,
      );
    }
    return body.result;
  }

  async ensureMember(
    memberId: string,
    role: string,
    options?: {
      labels?: Record<string, string>;
      context?: unknown;
      resumeSessionId?: string;
      additionalInstructions?: string[];
      runtimeMode?: "autonomous_host" | "turn_driven";
      backend?: "session" | "external";
      binding?: Record<string, unknown>;
    },
  ): Promise<MemberSnapshot> {
    const params: Record<string, unknown> = {
      role,
      agent_identity: memberId,
    };
    if (options?.labels) params.labels = options.labels;
    if (options?.context !== undefined) params.context = options.context;
    if (options?.resumeSessionId) {
      params.resume_session_id = options.resumeSessionId;
    }
    if (options?.additionalInstructions) {
      params.additional_instructions = options.additionalInstructions;
    }
    if (options?.runtimeMode) params.runtime_mode = options.runtimeMode;
    if (options?.backend) params.backend = options.backend;
    if (options?.binding) params.binding = options.binding;
    return parseMemberSnapshot(
      await this._runtime._rpc("mobkit/ensure_member", params),
    );
  }

  async findMembers(
    labelKey: string,
    labelValue: string,
  ): Promise<MemberSnapshot[]> {
    const raw = await this._runtime._rpc("mobkit/find_members", {
      label_key: labelKey,
      label_value: labelValue,
    });
    if (Array.isArray(raw)) {
      return raw.map(parseMemberSnapshot);
    }
    return [];
  }

  // -- Roster -------------------------------------------------------------

  async listMembers(): Promise<MemberSnapshot[]> {
    const raw = await this._runtime._rpc("mobkit/list_members");
    if (Array.isArray(raw)) {
      return raw.map(parseMemberSnapshot);
    }
    return [];
  }

  async getMember(memberId: string): Promise<MemberSnapshot> {
    return parseMemberSnapshot(
      await this._runtime._rpc("mobkit/get_member", { member_id: memberId }),
    );
  }

  async retireMember(memberId: string): Promise<void> {
    await this._runtime._rpc("mobkit/retire_member", {
      member_id: memberId,
    });
  }

  async respawnMember(memberId: string): Promise<void> {
    await this._runtime._rpc("mobkit/respawn_member", {
      member_id: memberId,
    });
  }

  async memberStatus(memberId: string): Promise<RichMemberSnapshot> {
    return parseRichMemberSnapshot(
      await this._runtime._rpc("mobkit/member_status", { member_id: memberId }),
    );
  }

  async identityResolvedTools(identity: string): Promise<readonly string[]> {
    const result = await this.identityResolvedToolsDetail(identity);
    return result.tools;
  }

  async identityResolvedToolsDetail(identity: string): Promise<IdentityResolvedToolsResult> {
    return parseIdentityResolvedToolsResult(
      await this._runtime._rpc("mobkit/identity/resolved_tools", { identity }),
    );
  }

  /**
   * Meerkat's typed model-routing status for an identity's live session.
   *
   * Rejects if the runtime machine does not hold the session. The rejection
   * carries a machine-readable `reason` in its error data
   * (`runtime_unsupported`, `no_current_session`, `member_lookup_failed`,
   * `session_not_held`, `upstream_read_failed`, `invalid_identity`) so a
   * fleet sweep can classify an identity rather than only fail it.
   *
   * `no_current_session` covers BOTH an identity that was materialized and
   * never addressed (the normal state after a restart, not a defect) AND an
   * identity that does not exist at all. This surface cannot distinguish them,
   * so a sweep must assert it received a status for every identity it expected
   * rather than merely that nothing threw.
   */
  async identityRoutingStatus(identity: string): Promise<IdentityRoutingStatusResult> {
    return parseIdentityRoutingStatusResult(
      await this._runtime._rpc("mobkit/identity/routing_status", { identity }),
    );
  }

  async forceCancelMember(memberId: string): Promise<void> {
    await this._runtime._rpc("mobkit/force_cancel_member", {
      member_id: memberId,
    });
  }

  /**
   * Wait until all current mob members are startup-ready for orchestration.
   *
   * Relays meerkat 0.6's `MobHandle::wait_for_ready`. Returns
   * `{ ready: [...], timeout: false }` on full convergence; on timeout
   * returns `{ ready: [], timeout: true }`. Pass `timeoutSeconds` to bound
   * the wait, or omit it to wait up to a generous server-side default ceiling
   * (~10 minutes); the wait returns as soon as members converge.
   */
  async waitReady(
    timeoutSeconds?: number,
  ): Promise<{ ready: unknown[]; timeout: boolean }> {
    const params: Record<string, unknown> = {};
    if (timeoutSeconds !== undefined) {
      params.timeout_ms = Math.round(timeoutSeconds * 1000);
    }
    const raw = await this._runtime._rpc("mobkit/wait_ready", params);
    if (typeof raw !== "object" || raw === null) {
      return { ready: [], timeout: false };
    }
    const r = raw as Record<string, unknown>;
    return {
      ready: Array.isArray(r.ready) ? r.ready : [],
      timeout: Boolean(r.timeout),
    };
  }

  // -- Flows --------------------------------------------------------------

  async cancelFlow(runId: string): Promise<void> {
    await this._runtime._rpc("mobkit/cancel_flow", { run_id: runId });
  }

  async flowStatus(runId: string): Promise<MobRunSnapshot | null> {
    const raw = await this._runtime._rpc("mobkit/flow_status", {
      run_id: runId,
    });
    if (raw === null) return null;
    if (
      typeof raw === "object" &&
      raw !== null &&
      (raw as Record<string, unknown>).status === "not_found"
    ) {
      return null;
    }
    return parseMobRunSnapshot(raw);
  }

  /**
   * List all configured flow IDs in this mob definition. Relays meerkat
   * 0.6's `MobHandle::list_flows`. Order is unspecified.
   */
  async listFlows(): Promise<string[]> {
    const raw = await this._runtime._rpc("mobkit/list_flows");
    if (Array.isArray(raw)) {
      return raw.map((id) => String(id));
    }
    if (typeof raw === "object" && raw !== null) {
      const flows = (raw as Record<string, unknown>).flows;
      if (Array.isArray(flows)) {
        return flows.map((id) => String(id));
      }
    }
    return [];
  }

  /**
   * List flow runs for this mob, optionally filtered to one `flowId`.
   * Relays `MobHandle::list_runs`. Each {@link MobRun} carries the full
   * meerkat ledger projection — `step_ledger`, `failure_ledger`,
   * `frames`, `loops`, `loop_iteration_ledger`, `flow_state`,
   * `activation_params`, etc. — verbatim from the wire JSON.
   */
  async listRuns(flowId?: string): Promise<MobRun[]> {
    const params: Record<string, unknown> = {};
    if (flowId !== undefined) params.flow_id = flowId;
    const raw = await this._runtime._rpc("mobkit/list_runs", params);
    let runs: unknown[] = [];
    if (Array.isArray(raw)) {
      runs = raw;
    } else if (typeof raw === "object" && raw !== null) {
      const maybe = (raw as Record<string, unknown>).runs;
      if (Array.isArray(maybe)) {
        runs = maybe;
      }
    }
    const result: MobRun[] = [];
    for (const entry of runs) {
      if (typeof entry === "object" && entry !== null) {
        result.push(parseMobRun(entry as Record<string, unknown>));
      }
    }
    return result;
  }

  /**
   * Start a flow run and return its run ID. Relays meerkat 0.6's
   * `MobHandle::run_flow`. `params` is forwarded verbatim as the flow's
   * activation params (any JSON value).
   */
  async runFlow(flowId: string, params: unknown = null): Promise<string> {
    const raw = await this._runtime._rpc("mobkit/run_flow", {
      flow_id: flowId,
      params,
    });
    if (typeof raw === "object" && raw !== null) {
      const runId = (raw as Record<string, unknown>).run_id;
      if (typeof runId === "string") {
        return runId;
      }
    }
    throw new Error(`unexpected run_flow response: ${JSON.stringify(raw)}`);
  }

  async collectCompleted(): Promise<Array<[string, RichMemberSnapshot]>> {
    const raw = await this._runtime._rpc("mobkit/collect_completed");
    const entries =
      typeof raw === "object" && raw !== null
        ? Array.isArray((raw as Record<string, unknown>).completed)
          ? ((raw as Record<string, unknown>).completed as unknown[])
          : []
        : Array.isArray(raw)
          ? raw
          : [];
    const result: Array<[string, RichMemberSnapshot]> = [];
    for (const entry of entries) {
      const record =
        typeof entry === "object" && entry !== null
          ? (entry as Record<string, unknown>)
          : {};
      const memberId = String(record.member_id ?? "");
      result.push([
        memberId,
        parseRichMemberSnapshot(record.snapshot ?? record),
      ]);
    }
    return result;
  }

  // -- Routing ------------------------------------------------------------

  async resolveRouting(
    recipient: string,
    options?: Record<string, unknown>,
  ): Promise<RoutingResolution> {
    return parseRoutingResolution(
      await this._runtime._rpc("mobkit/routing/resolve", {
        recipient,
        ...(options ?? {}),
      }),
    );
  }

  async listRoutes(): Promise<RuntimeRouteResult[]> {
    const raw = await this._runtime._rpc("mobkit/routing/routes/list");
    const routes =
      typeof raw === "object" && raw !== null
        ? (((raw as Record<string, unknown>).routes as unknown[]) ?? [])
        : [];
    return (routes as unknown[]).map(parseRuntimeRouteResult);
  }

  async addRoute(
    routeKey: string,
    recipient: string,
    sink: string,
    targetModule: string,
    channel?: string,
  ): Promise<RuntimeRouteResult> {
    const params: Record<string, unknown> = {
      route_key: routeKey,
      recipient,
      sink,
      target_module: targetModule,
    };
    if (channel !== undefined) params.channel = channel;
    const raw = await this._runtime._rpc("mobkit/routing/routes/add", params);
    const routeData =
      typeof raw === "object" && raw !== null
        ? ((raw as Record<string, unknown>).route ?? raw)
        : raw;
    return parseRuntimeRouteResult(routeData);
  }

  async deleteRoute(routeKey: string): Promise<RuntimeRouteResult> {
    const raw = await this._runtime._rpc("mobkit/routing/routes/delete", {
      route_key: routeKey,
    });
    const deletedData =
      typeof raw === "object" && raw !== null
        ? ((raw as Record<string, unknown>).deleted ?? raw)
        : raw;
    return parseRuntimeRouteResult(deletedData);
  }

  // -- Delivery -----------------------------------------------------------

  async sendDelivery(
    options: Record<string, unknown>,
  ): Promise<DeliveryResult> {
    return parseDeliveryResult(
      await this._runtime._rpc("mobkit/delivery/send", options),
    );
  }

  async deliveryHistory(
    recipient?: string,
    sink?: string,
    limit = 20,
  ): Promise<DeliveryHistoryResult> {
    const params: Record<string, unknown> = { limit };
    if (recipient !== undefined) params.recipient = recipient;
    if (sink !== undefined) params.sink = sink;
    return parseDeliveryHistoryResult(
      await this._runtime._rpc("mobkit/delivery/history", params),
    );
  }

  // -- Memory -------------------------------------------------------------

  /**
   * Query the operational memory assertion ledger.
   *
   * Pass `{ entity, topic, store }` for the Rust gateway's exact-filter
   * contract. The string overload is forwarded as `query`, which the gateway
   * applies as a case-insensitive substring filter across entity, topic, and
   * fact (reason for conflict signals), after the exact filters.
   */
  async memoryQuery(
    queryOrOptions?: string | {
      entity?: string;
      topic?: string;
      store?: string;
    },
    options?: Record<string, unknown>,
  ): Promise<MemoryQueryResult> {
    const params: Record<string, unknown> =
      typeof queryOrOptions === "string"
        ? { query: queryOrOptions, ...(options ?? {}) }
        : { ...(queryOrOptions ?? {}) };
    return parseMemoryQueryResult(
      await this._runtime._rpc("mobkit/memory/query", params),
    );
  }

  async memoryStores(): Promise<MemoryStoreInfo[]> {
    const raw = await this._runtime._rpc("mobkit/memory/stores");
    const stores =
      typeof raw === "object" && raw !== null
        ? (((raw as Record<string, unknown>).stores as unknown[]) ?? [])
        : [];
    return (stores as unknown[]).map(parseMemoryStoreInfo);
  }

  async memoryIndex(
    entity: string,
    topic: string,
    store: string,
    options?: Record<string, unknown>,
  ): Promise<MemoryIndexResult> {
    return parseMemoryIndexResult(
      await this._runtime._rpc("mobkit/memory/index", {
        entity,
        topic,
        store,
        ...(options ?? {}),
      }),
    );
  }

  async rememberAgentMemory(
    identity: string,
    memory: RememberAgentMemoryOptions,
  ): Promise<AgentMemoryRecord> {
    const params: Record<string, unknown> = {
      identity,
      title: memory.title,
      body: memory.body,
    };
    if (memory.realm !== undefined) params.realm = memory.realm;
    if (memory.tags !== undefined) params.tags = [...memory.tags];
    return parseAgentMemoryRecord(
      await this._runtime._rpc("mobkit/agent_memory/remember", params),
    );
  }

  async recallAgentMemory(
    identity: string,
    options: RecallAgentMemoryOptions = {},
  ): Promise<AgentMemoryRecord[]> {
    const params: Record<string, unknown> = { identity };
    if (options.realm !== undefined) params.realm = options.realm;
    if (options.selection !== undefined) params.selection = options.selection;
    if (options.queryText !== undefined) params.query_text = options.queryText;
    if (options.queryTerms !== undefined) params.query_terms = [...options.queryTerms];
    if (options.maxEntries !== undefined) params.max_entries = options.maxEntries;
    const result = parseAgentMemoryRecallResult(
      await this._runtime._rpc("mobkit/agent_memory/recall", params),
    );
    return [...result.records];
  }

  async forgetAgentMemory(
    identity: string,
    memoryId: string,
    options: ForgetAgentMemoryOptions = {},
  ): Promise<AgentMemoryForgetResult> {
    const params: Record<string, unknown> = { identity, memory_id: memoryId };
    if (options.realm !== undefined) params.realm = options.realm;
    return parseAgentMemoryForgetResult(
      await this._runtime._rpc("mobkit/agent_memory/forget", params),
    );
  }

  /**
   * Supersede a durable memory record within its lineage: the new
   * title/body/tags become the active record; the prior stays retrievable
   * with provenance and is no longer recalled.
   */
  async updateAgentMemory(
    identity: string,
    memoryId: string,
    memory: UpdateAgentMemoryOptions,
  ): Promise<AgentMemoryUpdateResult> {
    const params: Record<string, unknown> = {
      identity,
      memory_id: memoryId,
      title: memory.title,
      body: memory.body,
    };
    if (memory.realm !== undefined) params.realm = memory.realm;
    if (memory.tags !== undefined) params.tags = [...memory.tags];
    return parseAgentMemoryUpdateResult(
      await this._runtime._rpc("mobkit/agent_memory/update", params),
    );
  }

  /**
   * List durable memory record metadata (id/kind/title/description/age/rank
   * — never bodies). Tier "working_set" (default) returns the top-K ranked
   * records plus the recent/unranked slice; "full" returns everything.
   */
  async manifestAgentMemory(
    identity: string,
    options: ManifestAgentMemoryOptions = {},
  ): Promise<AgentMemoryRecordMeta[]> {
    const params: Record<string, unknown> = { identity };
    if (options.realm !== undefined) params.realm = options.realm;
    if (options.tier !== undefined) params.tier = options.tier;
    if (options.k !== undefined) params.k = options.k;
    const result = parseAgentMemoryManifestResult(
      await this._runtime._rpc("mobkit/agent_memory/manifest", params),
    );
    return [...result.records];
  }

  // -- Tools --------------------------------------------------------------

  async callTool(
    moduleId: string,
    tool: string,
    args?: Record<string, unknown>,
  ): Promise<CallToolResult> {
    const params: Record<string, unknown> = { module_id: moduleId, tool };
    if (args) params.arguments = args;
    return parseCallToolResult(
      await this._runtime._rpc("mobkit/call_tool", params),
    );
  }

  toolCaller(moduleId: string): ToolCaller {
    return new ToolCaller(this, moduleId);
  }

  async sessionStoreBigQuery(
    options: Record<string, unknown>,
  ): Promise<unknown> {
    return this._runtime._rpc("mobkit/session_store/bigquery", options);
  }

  // -- Gating -------------------------------------------------------------

  async gatingEvaluate(
    action: string,
    actorId: string,
    options?: Record<string, unknown>,
  ): Promise<GatingEvaluateResult> {
    return parseGatingEvaluateResult(
      await this._runtime._rpc("mobkit/gating/evaluate", {
        action,
        actor_id: actorId,
        ...(options ?? {}),
      }),
    );
  }

  async gatingPending(): Promise<GatingPendingEntry[]> {
    const raw = await this._runtime._rpc("mobkit/gating/pending");
    const entries =
      typeof raw === "object" && raw !== null
        ? (((raw as Record<string, unknown>).pending as unknown[]) ?? [])
        : [];
    return (entries as unknown[]).map(parseGatingPendingEntry);
  }

  async gatingDecide(
    pendingId: string,
    decision: string,
    approverId: string,
    options?: Record<string, unknown>,
  ): Promise<GatingDecisionResult> {
    return parseGatingDecisionResult(
      await this._runtime._rpc("mobkit/gating/decide", {
        pending_id: pendingId,
        decision,
        approver_id: approverId,
        ...(options ?? {}),
      }),
    );
  }

  async gatingAudit(limit = 100): Promise<GatingAuditEntry[]> {
    const raw = await this._runtime._rpc("mobkit/gating/audit", { limit });
    const entries =
      typeof raw === "object" && raw !== null
        ? (((raw as Record<string, unknown>).entries as unknown[]) ?? [])
        : [];
    return (entries as unknown[]).map(parseGatingAuditEntry);
  }

  // -- WorkGraph ------------------------------------------------------------
  //
  // Goals, work items, and attention bindings (`mobkit/workgraph/*`). Wire
  // params are snake_case; option objects here are typed camelCase and
  // converted at the call site. See docs/design/workgraph-wire-contract.md.

  async workgraphSnapshot(
    options?: WorkGraphFilterOptions,
  ): Promise<WorkGraphSnapshotResult> {
    return parseWorkGraphSnapshotResult(
      await this._runtime._rpc(
        "mobkit/workgraph/snapshot",
        workGraphFilterOptionsToDict(options),
      ),
    );
  }

  async workgraphList(
    options?: WorkGraphFilterOptions,
  ): Promise<WorkGraphItem[]> {
    const result = parseWorkGraphItemsResult(
      await this._runtime._rpc(
        "mobkit/workgraph/list",
        workGraphFilterOptionsToDict(options),
      ),
    );
    return [...result.items];
  }

  async workgraphGet(
    id: string,
    options?: { namespace?: string },
  ): Promise<WorkGraphItem> {
    const params: Record<string, unknown> = { id };
    if (options?.namespace !== undefined) params.namespace = options.namespace;
    const raw = await this._runtime._rpc("mobkit/workgraph/get", params);
    return parseWorkGraphItem(asWireRecord(raw).item);
  }

  async workgraphReady(
    options?: WorkGraphReadyOptions,
  ): Promise<WorkGraphItem[]> {
    const result = parseWorkGraphItemsResult(
      await this._runtime._rpc(
        "mobkit/workgraph/ready",
        workGraphReadyOptionsToDict(options),
      ),
    );
    return [...result.items];
  }

  async workgraphEvents(
    options?: WorkGraphEventsOptions,
  ): Promise<WorkGraphEventEntry[]> {
    const raw = await this._runtime._rpc(
      "mobkit/workgraph/events",
      workGraphEventsOptionsToDict(options),
    );
    const events = asWireRecord(raw).events;
    return (Array.isArray(events) ? events : []).map(parseWorkGraphEventEntry);
  }

  async workgraphAttentionList(
    options?: WorkGraphAttentionListOptions,
  ): Promise<WorkGraphAttentionBinding[]> {
    const raw = await this._runtime._rpc(
      "mobkit/workgraph/attention/list",
      workGraphAttentionListOptionsToDict(options),
    );
    const attention = asWireRecord(raw).attention;
    return (Array.isArray(attention) ? attention : []).map(
      parseWorkGraphAttentionBinding,
    );
  }

  async workgraphGoalStatus(
    bindingId: string,
    options?: { namespace?: string },
  ): Promise<WorkGraphGoalResult> {
    const params: Record<string, unknown> = { binding_id: bindingId };
    if (options?.namespace !== undefined) params.namespace = options.namespace;
    return parseWorkGraphGoalResult(
      await this._runtime._rpc("mobkit/workgraph/goal/status", params),
    );
  }

  async workgraphCreate(
    title: string,
    options?: WorkGraphCreateOptions,
  ): Promise<WorkGraphItem> {
    const params = workGraphCreateOptionsToDict(options);
    params.title = title;
    const raw = await this._runtime._rpc("mobkit/workgraph/create", params);
    return parseWorkGraphItem(asWireRecord(raw).item);
  }

  async workgraphUpdate(
    id: string,
    expectedRevision: number,
    options?: WorkGraphUpdateOptions,
  ): Promise<WorkGraphItem> {
    const params = workGraphUpdateOptionsToDict(options);
    params.id = id;
    params.expected_revision = expectedRevision;
    const raw = await this._runtime._rpc("mobkit/workgraph/update", params);
    return parseWorkGraphItem(asWireRecord(raw).item);
  }

  async workgraphClaim(
    id: string,
    expectedRevision: number,
    owner: WorkGraphOwnerInput,
    options?: WorkGraphClaimOptions,
  ): Promise<WorkGraphItem> {
    const params = workGraphClaimOptionsToDict(options);
    params.id = id;
    params.expected_revision = expectedRevision;
    params.owner = workGraphOwnerInputToDict(owner);
    const raw = await this._runtime._rpc("mobkit/workgraph/claim", params);
    return parseWorkGraphItem(asWireRecord(raw).item);
  }

  async workgraphRelease(
    id: string,
    expectedRevision: number,
    options?: { namespace?: string },
  ): Promise<WorkGraphItem> {
    const params: Record<string, unknown> = {
      id,
      expected_revision: expectedRevision,
    };
    if (options?.namespace !== undefined) params.namespace = options.namespace;
    const raw = await this._runtime._rpc("mobkit/workgraph/release", params);
    return parseWorkGraphItem(asWireRecord(raw).item);
  }

  async workgraphClose(
    id: string,
    expectedRevision: number,
    options?: WorkGraphCloseOptions,
  ): Promise<WorkGraphItem> {
    const params = workGraphCloseOptionsToDict(options);
    params.id = id;
    params.expected_revision = expectedRevision;
    const raw = await this._runtime._rpc("mobkit/workgraph/close", params);
    return parseWorkGraphItem(asWireRecord(raw).item);
  }

  async workgraphBlock(
    id: string,
    expectedRevision: number,
    options?: { namespace?: string },
  ): Promise<WorkGraphItem> {
    const params: Record<string, unknown> = {
      id,
      expected_revision: expectedRevision,
    };
    if (options?.namespace !== undefined) params.namespace = options.namespace;
    const raw = await this._runtime._rpc("mobkit/workgraph/block", params);
    return parseWorkGraphItem(asWireRecord(raw).item);
  }

  async workgraphLink(
    kind: string,
    fromId: string,
    toId: string,
    options?: { namespace?: string },
  ): Promise<WorkGraphEdge> {
    const params: Record<string, unknown> = {
      kind,
      from_id: fromId,
      to_id: toId,
    };
    if (options?.namespace !== undefined) params.namespace = options.namespace;
    const raw = await this._runtime._rpc("mobkit/workgraph/link", params);
    return parseWorkGraphEdge(asWireRecord(raw).edge);
  }

  async workgraphAddEvidence(
    id: string,
    expectedRevision: number,
    evidence: WorkGraphEvidenceInput,
    options?: { namespace?: string },
  ): Promise<WorkGraphItem> {
    const params: Record<string, unknown> = {
      id,
      expected_revision: expectedRevision,
      evidence: workGraphEvidenceInputToDict(evidence),
    };
    if (options?.namespace !== undefined) params.namespace = options.namespace;
    const raw = await this._runtime._rpc(
      "mobkit/workgraph/evidence/add",
      params,
    );
    return parseWorkGraphItem(asWireRecord(raw).item);
  }

  async workgraphEscalatePolicy(
    bindingId: string,
    id: string,
    expectedRevision: number,
    completionPolicy: unknown,
    options?: { namespace?: string },
  ): Promise<WorkGraphItem> {
    const params: Record<string, unknown> = {
      binding_id: bindingId,
      id,
      expected_revision: expectedRevision,
      completion_policy: completionPolicy,
    };
    if (options?.namespace !== undefined) params.namespace = options.namespace;
    const raw = await this._runtime._rpc(
      "mobkit/workgraph/policy/escalate",
      params,
    );
    return parseWorkGraphItem(asWireRecord(raw).item);
  }

  async workgraphGoalCreate(
    title: string,
    target: WorkGraphGoalTarget,
    options?: WorkGraphGoalCreateOptions,
  ): Promise<WorkGraphGoalResult> {
    const params = workGraphGoalCreateOptionsToDict(options);
    params.title = title;
    params.target = workGraphGoalTargetToDict(target);
    return parseWorkGraphGoalResult(
      await this._runtime._rpc("mobkit/workgraph/goal/create", params),
    );
  }

  async workgraphGoalConfirm(
    bindingId: string,
    expectedRevision: number,
    options?: WorkGraphGoalConfirmOptions,
  ): Promise<WorkGraphGoalResult> {
    const params = workGraphGoalConfirmOptionsToDict(options);
    params.binding_id = bindingId;
    params.expected_revision = expectedRevision;
    return parseWorkGraphGoalResult(
      await this._runtime._rpc("mobkit/workgraph/goal/confirm", params),
    );
  }

  async workgraphGoalRequestClose(
    bindingId: string,
    expectedRevision: number,
    options?: WorkGraphGoalRequestCloseOptions,
  ): Promise<WorkGraphGoalResult> {
    const params = workGraphGoalRequestCloseOptionsToDict(options);
    params.binding_id = bindingId;
    params.expected_revision = expectedRevision;
    return parseWorkGraphGoalResult(
      await this._runtime._rpc("mobkit/workgraph/goal/request_close", params),
    );
  }

  async workgraphAttentionPause(
    bindingId: string,
    expectedRevision: number,
    options?: WorkGraphAttentionPauseOptions,
  ): Promise<WorkGraphAttentionBinding> {
    const params = workGraphAttentionPauseOptionsToDict(options);
    params.binding_id = bindingId;
    params.expected_revision = expectedRevision;
    const raw = await this._runtime._rpc(
      "mobkit/workgraph/attention/pause",
      params,
    );
    return parseWorkGraphAttentionBinding(asWireRecord(raw).attention);
  }

  async workgraphAttentionResume(
    bindingId: string,
    expectedRevision: number,
    options?: { namespace?: string },
  ): Promise<WorkGraphAttentionBinding> {
    const params: Record<string, unknown> = {
      binding_id: bindingId,
      expected_revision: expectedRevision,
    };
    if (options?.namespace !== undefined) params.namespace = options.namespace;
    const raw = await this._runtime._rpc(
      "mobkit/workgraph/attention/resume",
      params,
    );
    return parseWorkGraphAttentionBinding(asWireRecord(raw).attention);
  }

  async workgraphAttentionReassign(
    bindingId: string,
    expectedRevision: number,
    target: WorkGraphGoalTarget,
    options?: { namespace?: string },
  ): Promise<WorkGraphAttentionReassignResult> {
    const params: Record<string, unknown> = {
      binding_id: bindingId,
      expected_revision: expectedRevision,
      target: workGraphGoalTargetToDict(target),
    };
    if (options?.namespace !== undefined) params.namespace = options.namespace;
    return parseWorkGraphAttentionReassignResult(
      await this._runtime._rpc("mobkit/workgraph/attention/reassign", params),
    );
  }

  /**
   * Prune TERMINAL (superseded/stopped) attention binding rows. The
   * workgraph event stream keeps the audit history; binding rows otherwise
   * grow monotonically with reassignment churn. Pass an RFC3339
   * `updatedBefore` to prune only rows last updated strictly before that
   * instant. Returns the number of rows pruned.
   */
  async workgraphAttentionPrune(options?: {
    updatedBefore?: string;
    namespace?: string;
  }): Promise<number> {
    const params: Record<string, unknown> = {};
    if (options?.updatedBefore !== undefined) {
      params.updated_before = options.updatedBefore;
    }
    if (options?.namespace !== undefined) params.namespace = options.namespace;
    const raw = await this._runtime._rpc(
      "mobkit/workgraph/attention/prune",
      params,
    );
    const pruned = asWireRecord(raw).pruned;
    return typeof pruned === "number" ? pruned : 0;
  }

  // -- Live (realtime) sessions — mobkit/live/* (mobkit 0.7.31) ------------

  /**
   * Open a realtime (live) channel on a member's session. Returns the
   * transport bootstrap `{channel_id, transport: {type: "websocket", url,
   * token}, capabilities, continuity}`. The token is single-use with a
   * short TTL — hand the URL to the client immediately.
   */
  async liveOpen(
    identity: string,
    options?: Record<string, unknown>,
  ): Promise<Record<string, unknown>> {
    const raw = await this._runtime._rpc("mobkit/live/open", {
      identity,
      ...(options ?? {}),
    });
    return asWireRecord(raw);
  }

  /** Open a live channel and parse the strict typed channel handle. */
  async liveOpenTyped(
    identity: string,
    executionIdentity?: LiveExecutionIdentityV1,
    options?: Record<string, unknown>,
  ): Promise<LiveChannelHandle | PendingLiveChannelHandle> {
    let advertisedFeatureCapabilities: readonly string[] = [];
    if (executionIdentity !== undefined) {
      const forbidden = [
        "mode",
        "execution_mode",
        "profile",
        "profile_id",
        "execution_profile",
        "execution_profile_id",
        "delegation",
        "delegation_type",
        "delegation_model",
        "responses",
        "responses_model",
        "responses_tools",
        "responses_instructions",
        "bridge_model",
        "bridge_tools",
        "bridge_instructions",
        "auth_binding",
        "self_hosted_server_id",
        "provider_params",
        "tools",
        "instructions",
        "member_id",
        "session_id",
      ].find((field) => Object.prototype.hasOwnProperty.call(options ?? {}, field));
      if (forbidden !== undefined) {
        throw new TypeError(`experimental live/open does not accept ${forbidden}`);
      }
    }
    if (
      executionIdentity !== undefined &&
      (Object.prototype.hasOwnProperty.call(options ?? {}, "model") ||
        Object.prototype.hasOwnProperty.call(options ?? {}, "provider"))
    ) {
      throw new TypeError(
        "executionIdentity conflicts with legacy top-level model/provider",
      );
    }
    if (executionIdentity !== undefined) {
      const capabilities = await this.capabilities();
      advertisedFeatureCapabilities = capabilities.featureCapabilities;
      if (!supportsLiveExecutionIdentityV1(capabilities)) {
        throw new CapabilityUnavailableError(
          `capability ${LIVE_EXECUTION_IDENTITY_V1} is not available`,
          "",
          "mobkit/live/open",
          { capability: LIVE_EXECUTION_IDENTITY_V1 },
        );
      }
    }
    const params: Record<string, unknown> = {
      identity,
      ...(options ?? {}),
    };
    if (executionIdentity !== undefined) {
      Object.assign(
        params,
        liveOpenExecutionIdentityParams(executionIdentity),
      );
    }
    const raw = asWireRecord(
      await this._runtime._rpc("mobkit/live/open", params),
    );
    if (executionIdentity !== undefined) {
      const pending = parsePendingLiveChannelHandle(raw);
      if (pending.targetIdentity !== identity) {
        throw new TypeError(
          "strict mobkit/live/open returned a different target identity",
        );
      }
      if (!supportsLiveExecutionMode(
        { featureCapabilities: advertisedFeatureCapabilities },
        pending.executionMode,
      )) {
        throw new CapabilityUnavailableError(
          "resolved live execution mode is not advertised",
          "",
          "mobkit/live/open",
          { execution_mode: pending.executionMode },
        );
      }
      return pending;
    }
    return parseLiveChannelHandle({
      ...raw,
      target_identity: raw.target_identity ?? identity,
    });
  }

  async livePlaybackOwnerRegister(
    pending: PendingLiveChannelHandle,
  ): Promise<LivePlaybackOwnerReadiness> {
    const readiness = parseLivePlaybackOwnerReadiness(
      await this._runtime._rpc("mobkit/live/playback_owner/register", {
        identity: pending.targetIdentity,
        channel_id: pending.channelId,
        pending_receipt: pending.pendingReceipt,
      }),
    );
    if (readiness.channelId !== pending.channelId) {
      throw new TypeError("playback owner readiness channel mismatch");
    }
    return readiness;
  }

  async livePlaybackOwnerRevoke(
    pending: PendingLiveChannelHandle,
    readiness: LivePlaybackOwnerReadiness,
    active?: ActiveLiveChannelHandle,
  ): Promise<ExperimentalLiveChannelStatus> {
    if (readiness.channelId !== pending.channelId) {
      throw new TypeError("playback owner readiness channel mismatch");
    }
    const params: Record<string, unknown> = {
      identity: pending.targetIdentity,
      channel_id: pending.channelId,
      pending_receipt: pending.pendingReceipt,
      readiness_receipt: readiness.readinessReceipt,
    };
    if (active !== undefined) {
      activeLiveChannelHandleToWire(active);
      if (
        active.channelId !== pending.channelId ||
        active.targetIdentity !== pending.targetIdentity ||
        active.executionMode !== pending.executionMode
      ) {
        throw new TypeError("active playback owner does not match pending custody");
      }
      params.activation_receipt = active.activationReceipt;
    }
    const status = parseExperimentalLiveChannelStatus(
      await this._runtime._rpc("mobkit/live/playback_owner/revoke", params),
    );
    if (status.phase !== "revoked") {
      throw new TypeError("playback owner revoke did not return revoked custody");
    }
    return status;
  }

  async liveExperimentalStatus(
    handle: PendingLiveChannelHandle | ActiveLiveChannelHandle,
  ): Promise<ExperimentalLiveChannelStatus> {
    const params: Record<string, unknown> = {
      identity: handle.targetIdentity,
      channel_id: handle.channelId,
    };
    if ("pendingReceipt" in handle) {
      params.pending_receipt = handle.pendingReceipt;
    } else {
      params.activation_receipt = handle.activationReceipt;
    }
    const status = parseExperimentalLiveChannelStatus(
      await this._runtime._rpc("mobkit/live/status", params),
    );
    if (
      status.phase === "active" &&
      (status.handle.channelId !== handle.channelId ||
        status.handle.targetIdentity !== handle.targetIdentity ||
        status.handle.executionMode !== handle.executionMode)
    ) {
      throw new TypeError("active live handle does not match pending custody");
    }
    return status;
  }

  async liveConnect(
    identity: string,
    executionIdentity: LiveExecutionIdentityV1,
    playbackOwner: LivePlaybackOwner,
    options?: Record<string, unknown> & {
      readonly activationPollIntervalMs?: number;
      readonly activationAttempts?: number;
    },
  ): Promise<ActiveLiveChannelConnection> {
    const activationPollIntervalMs = options?.activationPollIntervalMs ?? 50;
    const activationAttempts = options?.activationAttempts ?? 200;
    if (!Number.isFinite(activationPollIntervalMs) || activationPollIntervalMs < 0) {
      throw new TypeError("activationPollIntervalMs must be non-negative");
    }
    if (!Number.isSafeInteger(activationAttempts) || activationAttempts <= 0) {
      throw new TypeError("activationAttempts must be a positive integer");
    }
    const openOptions: Record<string, unknown> = { ...(options ?? {}) };
    delete openOptions.activationPollIntervalMs;
    delete openOptions.activationAttempts;
    let pending: PendingLiveChannelHandle | undefined;
    try {
      const opened = await this.liveOpenTyped(
        identity,
        executionIdentity,
        openOptions,
      );
      if (!("pendingReceipt" in opened)) {
        throw new TypeError("experimental live/open did not return a pending handle");
      }
      pending = opened;
      if (pending.transport.transport !== "webrtc") {
        throw new TypeError("experimental live/open did not return a WebRTC bootstrap");
      }
      const offerSdp = await playbackOwner.prepare(pending);
      if (typeof offerSdp !== "string" || offerSdp.trim().length === 0) {
        throw new TypeError("playback owner returned an invalid SDP offer");
      }
      const readiness = await this.livePlaybackOwnerRegister(pending);
      const answerSdp = await this.liveWebrtcAnswerPending(
        pending,
        readiness,
        offerSdp,
      );
      await playbackOwner.acceptAnswer(answerSdp);
      for (let attempt = 0; attempt < activationAttempts; attempt += 1) {
        const status = await this.liveExperimentalStatus(pending);
        if (status.phase === "active") {
          await playbackOwner.activate(status.handle);
          const active = status.handle;
          const ownerPending = pending;
          let revocation: Promise<ExperimentalLiveChannelStatus> | undefined;
          const ownerLost = (): Promise<ExperimentalLiveChannelStatus> => {
            revocation ??= (async () => {
              const revoked = await this.livePlaybackOwnerRevoke(
                ownerPending,
                readiness,
                active,
              );
              try {
                await playbackOwner.abort();
              } catch {
                // Machine revocation has already removed active authority.
              }
              return revoked;
            })();
            return revocation;
          };
          const connection: ActiveLiveChannelConnection = {
            ...active,
            pendingReceipt: ownerPending.pendingReceipt,
            readinessReceipt: readiness.readinessReceipt,
            ownerLost,
            dispose: ownerLost,
          };
          if (playbackOwner.waitForLoss !== undefined) {
            void playbackOwner.waitForLoss().then(ownerLost, ownerLost).catch(() => {
              // The explicit connection lifecycle remains available for retry.
            });
          }
          return connection;
        }
        if (status.phase === "revoked" || status.phase === "closed") {
          throw new Error(
            `experimental live channel ${status.phase} before activation`,
          );
        }
        if (activationPollIntervalMs > 0) {
          await new Promise<void>((resolve) =>
            setTimeout(resolve, activationPollIntervalMs),
          );
        }
      }
      throw new Error("experimental live channel did not activate");
    } catch (error) {
      try {
        await playbackOwner.abort();
      } finally {
        if (pending !== undefined) {
          try {
            await this.liveClose(pending);
          } catch {
            // Preserve the activation error.
          }
        }
      }
      throw error;
    }
  }

  /** Complete the one-use WebRTC signaling bootstrap for a live channel. */
  async liveWebrtcAnswer(
    channelId: string,
    token: string,
    offerSdp: string,
  ): Promise<string> {
    const raw = asWireRecord(
      await this._runtime._rpc("live/webrtc/answer", {
        channel_id: channelId,
        token,
        offer_sdp: offerSdp,
      }),
    );
    if (typeof raw.answer_sdp !== "string") {
      throw new TypeError("live/webrtc/answer returned an invalid answer");
    }
    return raw.answer_sdp;
  }

  async liveWebrtcAnswerPending(
    pending: PendingLiveChannelHandle,
    readiness: LivePlaybackOwnerReadiness,
    offerSdp: string,
  ): Promise<string> {
    if (readiness.channelId !== pending.channelId) {
      throw new TypeError("playback owner readiness channel mismatch");
    }
    if (pending.transport.transport !== "webrtc") {
      throw new TypeError("pending handle is not a WebRTC bootstrap");
    }
    const raw = asWireRecord(
      await this._runtime._rpc("live/webrtc/answer", {
        identity: pending.targetIdentity,
        channel_id: pending.channelId,
        pending_receipt: pending.pendingReceipt,
        readiness_receipt: readiness.readinessReceipt,
        token: pending.transport.token,
        offer_sdp: offerSdp,
      }),
    );
    if (typeof raw.answer_sdp !== "string") {
      throw new TypeError("live/webrtc/answer returned an invalid answer");
    }
    return raw.answer_sdp;
  }

  /** Read retryably pending signaling without auto-renegotiating. */
  async liveReplacementRequired(
    active: ActiveLiveChannelHandle,
  ): Promise<LiveReplacementRequired> {
    activeLiveChannelHandleToWire(active);
    return parseLiveReplacementRequired(
      await this._runtime._rpc("mobkit/live/replacement_required", {
        identity: active.targetIdentity,
        channel_id: active.channelId,
        activation_receipt: active.activationReceipt,
      }),
    );
  }

  /**
   * Stream opaque assistant outputs with bounded, loss-intolerant delivery.
   * Closing the iterator closes the exact live channel.
   */
  async *liveOutputs(
    active: ActiveLiveChannelHandle,
    options?: { readonly capacity?: number },
  ): AsyncGenerator<LiveAssistantOutputAddress, void, void> {
    activeLiveChannelHandleToWire(active);
    const capacity = options?.capacity ?? 16;
    if (!Number.isSafeInteger(capacity) || capacity <= 0) {
      throw new TypeError("live output queue capacity must be a positive integer");
    }
    const items: LiveAssistantOutputAddress[] = [];
    const waiters: Array<(output: LiveAssistantOutputAddress) => void> = [];
    const unregister = this._runtime._registerLiveOutputConsumer(
      active.channelId,
      (output) => {
        const waiter = waiters.shift();
        if (waiter !== undefined) {
          waiter(output);
          return;
        }
        if (items.length >= capacity) {
          throw new Error(`live output consumer queue is full for ${active.channelId}`);
        }
        items.push(output);
      },
    );
    try {
      while (true) {
        const output = items.shift() ?? await new Promise<LiveAssistantOutputAddress>(
          (resolve) => waiters.push(resolve),
        );
        yield output;
      }
    } finally {
      unregister();
      await this.liveClose(active);
    }
  }

  /** Report measured playback completion for an exact opaque output. */
  async livePlaybackComplete(
    active: ActiveLiveChannelHandle,
    output: LiveAssistantOutputAddress,
  ): Promise<LivePlaybackCompleteResult> {
    activeLiveChannelHandleToWire(active);
    if (output.channelId !== active.channelId) {
      throw new TypeError("assistant output does not belong to active channel");
    }
    return parseLivePlaybackCompleteResult(
      await this._runtime._rpc("mobkit/live/playback_complete", {
        identity: active.targetIdentity,
        channel_id: active.channelId,
        activation_receipt: active.activationReceipt,
        output_id: output.outputId,
      }),
    );
  }

  async liveStatus(
    identity: string,
    options?: Record<string, unknown>,
  ): Promise<Record<string, unknown>> {
    const raw = await this._runtime._rpc("mobkit/live/status", {
      identity,
      ...(options ?? {}),
    });
    return asWireRecord(raw);
  }

  async liveClose(
    handle: string | PendingLiveChannelHandle | ActiveLiveChannelHandle,
    options?: Record<string, unknown>,
  ): Promise<Record<string, unknown>> {
    const params: Record<string, unknown> = {
      channel_id: typeof handle === "string" ? handle : handle.channelId,
      ...(options ?? {}),
    };
    if (typeof handle !== "string") {
      params.identity = handle.targetIdentity;
      if ("pendingReceipt" in handle) {
        params.pending_receipt = handle.pendingReceipt;
      } else {
        params.activation_receipt = handle.activationReceipt;
      }
    }
    const raw = await this._runtime._rpc("mobkit/live/close", params);
    return asWireRecord(raw);
  }

  /**
   * Send a still image into the member's open live channel (meerkat
   * 0.7.27+). `idempotencyKey` must be caller-stable within the session —
   * retries with the same key are exact-retry deduplicated.
   */
  async liveSendInputImage(
    identity: string,
    idempotencyKey: string,
    mime: string,
    dataBase64: string,
    options?: Record<string, unknown>,
  ): Promise<Record<string, unknown>> {
    const raw = await this._runtime._rpc("mobkit/live/send_input", {
      identity,
      chunk: {
        kind: "image",
        idempotency_key: idempotencyKey,
        mime,
        data: dataBase64,
      },
      ...(options ?? {}),
    });
    return asWireRecord(raw);
  }

  /**
   * Push refreshed mutable config (instructions/tools/audio) into an open
   * live channel without rebuilding the transport. Model/provider swaps
   * require close + reopen.
   */
  async liveRefresh(
    identity: string,
    options?: Record<string, unknown>,
  ): Promise<Record<string, unknown>> {
    const raw = await this._runtime._rpc("mobkit/live/refresh", {
      identity,
      ...(options ?? {}),
    });
    return asWireRecord(raw);
  }

  /** Truncate an exact opaque output at measured playback progress. */
  async liveTruncate(
    active: ActiveLiveChannelHandle,
    output: LiveAssistantOutputAddress,
    audioPlayedMs: number,
    options?: Record<string, unknown>,
  ): Promise<Record<string, unknown>> {
    activeLiveChannelHandleToWire(active);
    if (output.channelId !== active.channelId) {
      throw new TypeError("assistant output does not belong to active channel");
    }
    const raw = await this._runtime._rpc("mobkit/live/truncate", {
      identity: active.targetIdentity,
      channel_id: active.channelId,
      activation_receipt: active.activationReceipt,
      output_id: output.outputId,
      audio_played_ms: audioPlayedMs,
      ...(options ?? {}),
    });
    return asWireRecord(raw);
  }

  async liveRefreshActive(
    active: ActiveLiveChannelHandle,
    options?: Record<string, unknown>,
  ): Promise<Record<string, unknown>> {
    activeLiveChannelHandleToWire(active);
    return asWireRecord(
      await this._runtime._rpc("mobkit/live/refresh", {
        identity: active.targetIdentity,
        channel_id: active.channelId,
        activation_receipt: active.activationReceipt,
        ...(options ?? {}),
      }),
    );
  }

  async liveSendInputImageActive(
    active: ActiveLiveChannelHandle,
    idempotencyKey: string,
    mime: string,
    dataBase64: string,
  ): Promise<Record<string, unknown>> {
    activeLiveChannelHandleToWire(active);
    return asWireRecord(
      await this._runtime._rpc("mobkit/live/send_input", {
        identity: active.targetIdentity,
        channel_id: active.channelId,
        activation_receipt: active.activationReceipt,
        chunk: {
          kind: "image",
          idempotency_key: idempotencyKey,
          mime,
          data: dataBase64,
        },
      }),
    );
  }

  async liveCommitInputActive(
    active: ActiveLiveChannelHandle,
    options?: Record<string, unknown>,
  ): Promise<Record<string, unknown>> {
    activeLiveChannelHandleToWire(active);
    return asWireRecord(
      await this._runtime._rpc("mobkit/live/commit_input", {
        identity: active.targetIdentity,
        channel_id: active.channelId,
        activation_receipt: active.activationReceipt,
        ...(options ?? {}),
      }),
    );
  }

  async liveInterruptActive(
    active: ActiveLiveChannelHandle,
  ): Promise<Record<string, unknown>> {
    activeLiveChannelHandleToWire(active);
    return asWireRecord(
      await this._runtime._rpc("mobkit/live/interrupt", {
        identity: active.targetIdentity,
        channel_id: active.channelId,
        activation_receipt: active.activationReceipt,
      }),
    );
  }

  // -- Topology -----------------------------------------------------------

  async rediscover(): Promise<RediscoverReport | null> {
    const raw = await this._runtime._rpc("mobkit/rediscover");
    if (
      typeof raw === "object" &&
      raw !== null &&
      "status" in (raw as Record<string, unknown>)
    ) {
      return null;
    }
    return parseRediscoverReport(raw);
  }

  async reconcileEdges(): Promise<ReconcileEdgesReport> {
    return parseReconcileEdgesReport(
      await this._runtime._rpc("mobkit/reconcile_edges"),
    );
  }

  // -- Cross-mob ----------------------------------------------------------

  async listExternalMobs(): Promise<CrossMobContactEntry[]> {
    const raw = await this._runtime._rpc("mobkit/cross_mob/directory");
    const mobs =
      typeof raw === "object" && raw !== null
        ? Array.isArray((raw as Record<string, unknown>).mobs)
          ? ((raw as Record<string, unknown>).mobs as unknown[])
          : []
        : [];
    return mobs.map(parseCrossMobContactEntry);
  }

  async peerInfo(memberId: string): Promise<Record<string, string>> {
    const raw = await this._runtime._rpc("mobkit/cross_mob/peer_info", {
      member_id: memberId,
    });
    const out: Record<string, string> = {};
    if (typeof raw === "object" && raw !== null) {
      for (const [k, v] of Object.entries(raw as Record<string, unknown>)) {
        out[k] = String(v);
      }
    }
    return out;
  }

  async peerPubkey(): Promise<string> {
    const raw = await this._runtime._rpc("mobkit/peer_pubkey");
    return typeof raw === "object" && raw !== null
      ? String((raw as Record<string, unknown>).pubkey_b64 ?? "")
      : "";
  }

  async wireLocal(
    localMemberId: string,
    remoteCommsName: string,
    remotePeerId: string,
    remoteAddress: string,
    options?: { remotePubkeyB64?: string },
  ): Promise<void> {
    const params: Record<string, unknown> = {
      local_member_id: localMemberId,
      remote_comms_name: remoteCommsName,
      remote_peer_id: remotePeerId,
      remote_address: remoteAddress,
    };
    if (options?.remotePubkeyB64) {
      params.remote_pubkey_b64 = options.remotePubkeyB64;
    }
    await this._runtime._rpc("mobkit/cross_mob/wire_local", params);
  }

  async unwireLocal(
    localMemberId: string,
    remoteCommsName: string,
    remotePeerId: string,
    remoteAddress: string,
    options?: { remotePubkeyB64?: string },
  ): Promise<void> {
    const params: Record<string, unknown> = {
      local_member_id: localMemberId,
      remote_comms_name: remoteCommsName,
      remote_peer_id: remotePeerId,
      remote_address: remoteAddress,
    };
    if (options?.remotePubkeyB64) {
      params.remote_pubkey_b64 = options.remotePubkeyB64;
    }
    await this._runtime._rpc("mobkit/cross_mob/unwire_local", params);
  }

  async wireCrossMob(
    localMemberId: string,
    remoteMemberId: string,
    remoteHandle: MobHandle,
  ): Promise<void> {
    const localInfo = await this.peerInfo(localMemberId);
    const remoteInfo = await remoteHandle.peerInfo(remoteMemberId);
    await this.wireLocal(
      localMemberId,
      remoteInfo.comms_name,
      remoteInfo.peer_id,
      remoteInfo.address,
    );
    try {
      await remoteHandle.wireLocal(
        remoteMemberId,
        localInfo.comms_name,
        localInfo.peer_id,
        localInfo.address,
      );
    } catch (err) {
      try {
        await this.unwireLocal(
          localMemberId,
          remoteInfo.comms_name,
          remoteInfo.peer_id,
          remoteInfo.address,
        );
      } catch {
        // Best-effort rollback; preserve the original remote error.
      }
      throw err;
    }
  }

  async sendCrossMob(
    remoteMemberId: string,
    remoteHandle: MobHandle,
    message: string | DispatchContentBlock[],
  ): Promise<SendMessageResult> {
    return remoteHandle.send(remoteMemberId, message);
  }

  // -- Helper members -----------------------------------------------------

  async spawnHelper(
    agentIdentity: string,
    task: string,
    resultLabel: string,
    maxTextBytes: number,
    options?: {
      role?: string;
      runtimeMode?: string;
      backend?: string;
    },
  ): Promise<HelperResult> {
    const helperOptions: Record<string, unknown> = {};
    if (options?.role) helperOptions.role = options.role;
    if (options?.runtimeMode) helperOptions.runtime_mode = options.runtimeMode;
    if (options?.backend) helperOptions.backend = options.backend;
    const params: Record<string, unknown> = {
      agent_identity: agentIdentity,
      task,
      result_label: resultLabel,
      max_text_bytes: maxTextBytes,
    };
    if (Object.keys(helperOptions).length > 0) params.options = helperOptions;
    return parseHelperResult(
      await this._runtime._rpc("mobkit/spawn_helper", params),
    );
  }

  async forkHelper(
    sourceMemberId: string,
    agentIdentity: string,
    task: string,
    resultLabel: string,
    maxTextBytes: number,
    options?: {
      forkContext?: Record<string, unknown>;
      role?: string;
      runtimeMode?: string;
      backend?: string;
    },
  ): Promise<HelperResult> {
    const helperOptions: Record<string, unknown> = {};
    if (options?.role) helperOptions.role = options.role;
    if (options?.runtimeMode) helperOptions.runtime_mode = options.runtimeMode;
    if (options?.backend) helperOptions.backend = options.backend;
    const params: Record<string, unknown> = {
      source_member_id: sourceMemberId,
      agent_identity: agentIdentity,
      task,
      result_label: resultLabel,
      max_text_bytes: maxTextBytes,
    };
    if (options?.forkContext) params.fork_context = options.forkContext;
    if (Object.keys(helperOptions).length > 0) params.options = helperOptions;
    return parseHelperResult(
      await this._runtime._rpc("mobkit/fork_helper", params),
    );
  }

  async attachSession(
    role: string,
    agentIdentity: string,
    sessionId: string,
  ): Promise<RichMemberSnapshot> {
    return parseRichMemberSnapshot(
      await this._runtime._rpc("mobkit/attach_existing_session", {
        role,
        agent_identity: agentIdentity,
        session_id: sessionId,
      }),
    );
  }

  // -- Mob/run labels — mobkit-side sidecar metadata ----------------------

  /**
   * Replace the label set associated with this mob.
   *
   * Mobkit owns these labels — they are separate from meerkat-mob's
   * member-level labels. Replacement is wholesale; pass `{}` to clear.
   */
  async setMobLabels(labels: Record<string, string>): Promise<void> {
    await this._runtime._rpc("mobkit/mob_labels/set", { labels });
  }

  /** Return the label set associated with this mob (or `{}`). */
  async getMobLabels(): Promise<Record<string, string>> {
    const raw = await this._runtime._rpc("mobkit/mob_labels/get");
    if (typeof raw !== "object" || raw === null) return {};
    const labels = (raw as Record<string, unknown>).labels;
    if (typeof labels !== "object" || labels === null) return {};
    const out: Record<string, string> = {};
    for (const [k, v] of Object.entries(labels as Record<string, unknown>)) {
      out[k] = String(v);
    }
    return out;
  }

  /** Remove the label set associated with this mob. */
  async deleteMobLabels(): Promise<void> {
    await this._runtime._rpc("mobkit/mob_labels/delete");
  }

  /**
   * Replace the label set associated with `runId` under this mob.
   * Replacement is wholesale (see {@link setMobLabels}).
   */
  async setRunLabels(
    runId: string,
    labels: Record<string, string>,
  ): Promise<void> {
    await this._runtime._rpc("mobkit/run_labels/set", {
      run_id: runId,
      labels,
    });
  }

  /** Return the label set for `runId` (or `{}`). */
  async getRunLabels(runId: string): Promise<Record<string, string>> {
    const raw = await this._runtime._rpc("mobkit/run_labels/get", {
      run_id: runId,
    });
    if (typeof raw !== "object" || raw === null) return {};
    const labels = (raw as Record<string, unknown>).labels;
    if (typeof labels !== "object" || labels === null) return {};
    const out: Record<string, string> = {};
    for (const [k, v] of Object.entries(labels as Record<string, unknown>)) {
      out[k] = String(v);
    }
    return out;
  }

  /** Remove the label set for `runId`. */
  async deleteRunLabels(runId: string): Promise<void> {
    await this._runtime._rpc("mobkit/run_labels/delete", { run_id: runId });
  }
}

// -- ToolCaller -----------------------------------------------------------

/**
 * Bound callable scoped to one MCP module.
 *
 * @example
 * ```ts
 * const gmail = handle.toolCaller("google-workspace");
 * const messages = await gmail.call("gmail_search", { query: "is:unread" });
 * ```
 */
export class ToolCaller {
  constructor(
    private readonly _mobHandle: MobHandle,
    private readonly _moduleId: string,
  ) {}

  async call(tool: string, args?: Record<string, unknown>): Promise<unknown> {
    const result = await this._mobHandle.callTool(this._moduleId, tool, args);
    return result.result;
  }
}

// -- SseBridge ------------------------------------------------------------

/**
 * Bridge for streaming SSE from the Rust backend's HTTP server.
 */
export class SseBridge {
  constructor(private readonly _runtime: MobKitRuntime) {}

  private _baseUrl(): string {
    const base = this._runtime.rustHttpBaseUrl;
    if (base === null) {
      throw new NotConnectedError(
        "SSE bridge requires rustHttpBaseUrl — set it via " +
          "runtime.setRustHttpBase('http://127.0.0.1:8081') or " +
          "ensure the Rust binary reports it during bootstrap",
      );
    }
    return base;
  }

  async *agentEvents(
    agentId: string,
  ): AsyncGenerator<SseEvent, void, undefined> {
    const url = `${this._baseUrl()}/agents/${agentId}/events`;
    yield* this._streamSse(url);
  }

  async *mobEvents(): AsyncGenerator<SseEvent, void, undefined> {
    const url = `${this._baseUrl()}/mob/events`;
    yield* this._streamSse(url);
  }

  private async *_streamSse(
    url: string,
  ): AsyncGenerator<SseEvent, void, undefined> {
    // Pre-fix, breaking out of the consumer iterator left the
    // underlying http.ClientRequest open — sockets and Node refs
    // accumulated. Now: tie the request lifetime to the generator via
    // try/finally so consumer `break`/`throw`/`return` always destroys
    // the request.
    const { body, destroy } = await this._fetchSseStream(url);
    try {
      yield* parseSseStream(body);
    } finally {
      destroy();
    }
  }

  private _fetchSseStream(
    url: string,
  ): Promise<{ body: AsyncIterable<Uint8Array>; destroy: () => void }> {
    const parsed = new URL(url);
    const requester = parsed.protocol === "https:" ? httpsRequest : httpRequest;

    return new Promise((resolve, reject) => {
      const req = requester(
        url,
        { method: "GET", headers: { accept: "text/event-stream" } },
        (res) => {
          if (!res.statusCode || res.statusCode >= 400) {
            reject(
              new Error(
                `SSE request failed: ${res.statusCode} ${res.statusMessage}`,
              ),
            );
            res.resume();
            return;
          }

          // Convert Node readable stream to AsyncIterable<Uint8Array>
          const stream = (async function* () {
            for await (const chunk of res) {
              yield chunk instanceof Uint8Array
                ? chunk
                : new TextEncoder().encode(String(chunk));
            }
          })();

          resolve({
            body: stream,
            destroy: () => {
              req.destroy();
              res.destroy();
            },
          });
        },
      );

      req.on("error", reject);
      req.end();
    });
  }
}
