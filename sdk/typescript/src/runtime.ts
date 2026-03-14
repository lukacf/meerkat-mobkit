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
import { CallbackDispatcher, type SessionAgentBuilder } from "./agent-builder.js";
import { NotConnectedError, RpcError, TransportError } from "./errors.js";
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
  eventQueryToDict,
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
  type EventQuery,
} from "./types.js";

// -- Request ID counter ---------------------------------------------------

let requestCounter = 0;
function nextRequestId(method: string): string {
  return `${method}:${++requestCounter}`;
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
  constructor(
    config: MobKitBuilderConfig,
    transport?: PersistentTransport,
  ) {
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
      this._transport = new PersistentTransport(this._config.gatewayBin);

      // Register builder FIRST — init may trigger callback/build_agent
      if (this._config.sessionBuilder) {
        this._dispatcher.registerBuilder(this._config.sessionBuilder);
      }
      if (this._config.errorCallback !== null) {
        this._dispatcher.registerErrorCallback(this._config.errorCallback);
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
          this._rustHttpBase = String(
            (initResult as Record<string, unknown>).http_base_url ?? "",
          ) || null;
        }
      } catch {
        if (this._transport !== null && !this._transport.isRunning()) {
          throw new TransportError("gateway process died during bootstrap");
        }
        throw new TransportError(
          "mobkit/init failed — runtime could not be initialized",
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
      runtimeOptions.memory_config = this._config.memoryConfig;
    }
    if (this._config.authConfig) {
      runtimeOptions.auth_config = this._config.authConfig;
    }
    if (this._config.eventLog) {
      runtimeOptions.event_log = this._config.eventLog;
    }
    params.runtime_options = runtimeOptions;
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
      throw new RpcError(
        Number(err.code ?? -1),
        String(err.message ?? String(err)),
        rid,
        method,
      );
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
    }
  }

  get isRunning(): boolean {
    return this._running;
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
 * await handle.send(members[0].meerkatId, "Hello!");
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

  // -- Spawn & reconcile --------------------------------------------------

  async spawn(spec: DiscoverySpec): Promise<SpawnResult> {
    return parseSpawnResult(
      await this._runtime._rpc("mobkit/spawn_member", discoverySpecToDict(spec)),
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
    if (
      typeof raw === "object" &&
      raw !== null &&
      (raw as Record<string, unknown>).status === "no_event_log_configured"
    ) {
      return [];
    }
    if (Array.isArray(raw)) {
      return raw.map(parsePersistedEvent);
    }
    return [];
  }

  // -- Messaging ----------------------------------------------------------

  async send(memberId: string, message: string): Promise<SendMessageResult> {
    return parseSendMessageResult(
      await this._runtime._rpc("mobkit/send_message", {
        member_id: memberId,
        message,
      }),
    );
  }

  /** Alias for {@link send}. */
  sendMessage = this.send.bind(this);

  async ensureMember(
    memberId: string,
    profile: string,
    options?: {
      labels?: Record<string, string>;
      context?: unknown;
      resumeSessionId?: string;
      additionalInstructions?: string[];
    },
  ): Promise<MemberSnapshot> {
    const params: Record<string, unknown> = {
      profile,
      meerkat_id: memberId,
    };
    if (options?.labels) params.labels = options.labels;
    if (options?.context !== undefined) params.context = options.context;
    if (options?.resumeSessionId) {
      params.resume_session_id = options.resumeSessionId;
    }
    if (options?.additionalInstructions) {
      params.additional_instructions = options.additionalInstructions;
    }
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
        ? ((raw as Record<string, unknown>).routes as unknown[]) ?? []
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
        ? (raw as Record<string, unknown>).route ?? raw
        : raw;
    return parseRuntimeRouteResult(routeData);
  }

  async deleteRoute(routeKey: string): Promise<RuntimeRouteResult> {
    const raw = await this._runtime._rpc("mobkit/routing/routes/delete", {
      route_key: routeKey,
    });
    const deletedData =
      typeof raw === "object" && raw !== null
        ? (raw as Record<string, unknown>).deleted ?? raw
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

  async memoryQuery(
    query: string,
    options?: Record<string, unknown>,
  ): Promise<MemoryQueryResult> {
    return parseMemoryQueryResult(
      await this._runtime._rpc("mobkit/memory/query", {
        query,
        ...(options ?? {}),
      }),
    );
  }

  async memoryStores(): Promise<MemoryStoreInfo[]> {
    const raw = await this._runtime._rpc("mobkit/memory/stores");
    const stores =
      typeof raw === "object" && raw !== null
        ? ((raw as Record<string, unknown>).stores as unknown[]) ?? []
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
        ? ((raw as Record<string, unknown>).pending as unknown[]) ?? []
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
        ? ((raw as Record<string, unknown>).entries as unknown[]) ?? []
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

  async call(
    tool: string,
    args?: Record<string, unknown>,
  ): Promise<unknown> {
    const result = await this._mobHandle.callTool(
      this._moduleId,
      tool,
      args,
    );
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
    const body = await this._fetchSseStream(url);
    yield* parseSseStream(body);
  }

  private _fetchSseStream(url: string): Promise<AsyncIterable<Uint8Array>> {
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

          resolve(stream);
        },
      );

      req.on("error", reject);
      req.end();
    });
  }
}
