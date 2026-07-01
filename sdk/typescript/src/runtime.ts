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
  CapabilityUnavailableError,
  ConsoleTimelineReplayUnavailableError,
  LeaseLostError,
  MemoryBackendUnavailableError,
  MobEventsStaleError,
  NotConnectedError,
  RpcError,
  TransportError,
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
  parseStatusResult,
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
  parseBlobGetResult,
  parseBlobUploadResult,
  dispatchInputToDict,
  contentBlockToDict,
  type StatusResult,
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
  type AgentMemoryForgetResult,
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
  type BlobGetResult,
  type BlobUploadResult,
  type DispatchInput,
  type DispatchContentBlock,
} from "./types.js";

// -- Request ID counter ---------------------------------------------------

let requestCounter = 0;
function nextRequestId(method: string): string {
  return `${method}:${++requestCounter}`;
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

/**
 * Running MobKit runtime instance.
 *
 * Supports explicit lifecycle (`connect` / `shutdown`).
 */
export class MobKitRuntime {
  private _config: MobKitBuilderConfig;
  private _transport: PersistentTransport | null;
  private _running = false;
  private _dispatcher = new CallbackDispatcher();
  private _rustHttpBase: string | null = null;

  /** @internal */
  constructor(config: MobKitBuilderConfig, transport?: PersistentTransport) {
    this._config = config;
    this._transport = transport ?? null;
  }

  /** @internal */
  static async _create(config: MobKitBuilderConfig): Promise<MobKitRuntime> {
    const runtime = new MobKitRuntime(config);
    await runtime._bootstrap();
    return runtime;
  }

  /** Explicitly connect to the runtime. Idempotent. */
  async connect(): Promise<void> {
    if (this._running) return;
    await this._bootstrap();
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
        const initResult = await this._rpc(
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
      } catch (err) {
        // Pre-fix every error path here was rewritten to a generic
        // `TransportError`, destroying the original RPC code/message
        // (e.g. config errors like `bad mob.toml: line 7`). The fix:
        // only synthesize TransportError when the subprocess actually
        // died; otherwise re-throw the structured RpcError so operators
        // can diagnose config errors without spelunking gateway logs.
        if (this._transport !== null && !this._transport.isRunning()) {
          throw new TransportError("gateway process died during bootstrap");
        }
        if (isRpcError(err)) {
          throw err;
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
    this._running = true;
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
    if (this._config.schedulingFiles.length > 0) {
      runtimeOptions.scheduling_files = this._config.schedulingFiles;
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
    if (this._config.consoleConfigPath) {
      runtimeOptions.console_config_path = this._config.consoleConfigPath;
    }
    if (this._config.accessConfigPath) {
      runtimeOptions.access_config_path = this._config.accessConfigPath;
    }
    if (this._config.consoleRequireAppAuth !== null) {
      runtimeOptions.console_require_app_auth =
        this._config.consoleRequireAppAuth;
    }
    if (this._config.consoleReadOnly !== null) {
      runtimeOptions.console_read_only = this._config.consoleReadOnly;
    }
    if (this._config.consoleFetchTimeoutMs !== null) {
      runtimeOptions.console_fetch_timeout_ms =
        this._config.consoleFetchTimeoutMs;
    }
    if (this._config.demoLlm) {
      runtimeOptions.demo_llm = true;
    }
    if (this._config.implicitDelegateIdleRetireSecs !== undefined) {
      runtimeOptions.implicit_delegate_idle_retire_secs =
        this._config.implicitDelegateIdleRetireSecs;
    }
    if (this._config.maxSessions !== null) {
      runtimeOptions.max_sessions = this._config.maxSessions;
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

  setRustHttpBase(url: string): void {
    this._rustHttpBase = url;
  }

  mobHandle(): MobHandle {
    return new MobHandle(this);
  }

  sseBridge(): SseBridge {
    return new SseBridge(this);
  }

  async shutdown(): Promise<void> {
    this._running = false;
    if (this._transport !== null) {
      this._transport.stop();
      this._transport = null;
    }
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

  /** Send content to an identity. Content can be a string or content blocks. */
  async send(
    identity: string,
    content: string | DispatchContentBlock[],
  ): Promise<unknown> {
    const params: Record<string, unknown> = { identity };
    if (typeof content === "string") {
      params.content = content;
    } else {
      params.content = content.map(contentBlockToDict);
    }
    return this._rpc("mobkit/send", params);
  }

  /** Dispatch structured input to an identity. */
  async dispatch(identity: string, input: DispatchInput): Promise<unknown> {
    return this._rpc("mobkit/dispatch", {
      identity,
      dispatch_input: dispatchInputToDict(input),
    });
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
  async inspectIdentity(identity: string): Promise<unknown> {
    return this._rpc("mobkit/inspect_identity", { identity });
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

  // -- Scheduling ---------------------------------------------------------

  async schedulingEvaluate(
    schedules: readonly Record<string, unknown>[],
    tickMs: number,
  ): Promise<unknown> {
    return this._runtime._rpc("mobkit/scheduling/evaluate", {
      schedules: [...schedules],
      tick_ms: tickMs,
    });
  }

  async schedulingDispatch(
    schedules: readonly Record<string, unknown>[],
    tickMs: number,
  ): Promise<unknown> {
    return this._runtime._rpc("mobkit/scheduling/dispatch", {
      schedules: [...schedules],
      tick_ms: tickMs,
    });
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
   * contract. The string overload is retained only for older callers and is
   * forwarded as `query`; current Rust gateways do not perform semantic search
   * on that field.
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
