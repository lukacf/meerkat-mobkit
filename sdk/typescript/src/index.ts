/**
 * MobKit TypeScript SDK — companion orchestration for the Meerkat runtime.
 *
 * @example
 * ```ts
 * import { MobKit } from "@rkat/mobkit-sdk";
 *
 * const rt = await MobKit.builder()
 *   .mob("config/mob.toml")
 *   .gateway("./target/release/phase0b_rpc_gateway")
 *   .build();
 *
 * const handle = rt.mobHandle();
 * const status = await handle.status();
 * console.log(status.contractVersion, status.loadedModules);
 *
 * for await (const event of handle.subscribeAgent("agent-1")) {
 *   if (event.event.type === "text_delta") {
 *     process.stdout.write(event.event.delta);
 *   }
 * }
 *
 * await rt.shutdown();
 * ```
 */

// -- Builder + Runtime ----------------------------------------------------

export { MobKit, MobKitBuilder } from "./builder.js";
export type { MobKitBuilderConfig } from "./builder.js";
export { MobKitRuntime, MobHandle, ToolCaller, SseBridge } from "./runtime.js";

// -- Data models ----------------------------------------------------------

export { SessionBuildOptions } from "./models.js";
export type {
  DiscoverySpec,
  PreSpawnData,
  SessionQuery,
  ToolHandler,
} from "./models.js";
export {
  discoverySpecToDict,
  preSpawnDataToDict,
  sessionQueryToDict,
} from "./models.js";

// -- Agent builder --------------------------------------------------------

export type { SessionAgentBuilder, ErrorCallback } from "./agent-builder.js";
export { CallbackDispatcher } from "./agent-builder.js";

// -- Errors ---------------------------------------------------------------

export {
  MobKitError,
  TransportError,
  RpcError,
  CapabilityUnavailableError,
  ContractMismatchError,
  NotConnectedError,
  MobkitRpcError,
} from "./errors.js";

// -- Typed return models --------------------------------------------------

export {
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
} from "./types.js";

export type {
  StatusResult,
  CapabilitiesResult,
  ReconcileResult,
  SpawnResult,
  SpawnMemberResult,
  KeepAliveConfig,
  EventEnvelope,
  SubscribeResult,
  SendMessageResult,
  RoutingResolution,
  DeliveryResult,
  DeliveryHistoryResult,
  MemoryQueryResult,
  MemoryStoreInfo,
  MemoryIndexResult,
  CallToolResult,
  MemberSnapshot,
  RuntimeRouteResult,
  GatingEvaluateResult,
  GatingDecisionResult,
  GatingAuditEntry,
  GatingPendingEntry,
  RediscoverReport,
  ReconcileEdgesReport,
  UnifiedAgentEvent,
  UnifiedModuleEvent,
  UnifiedEvent,
  PersistedEvent,
  EventQuery,
  ErrorEvent,
  ErrorCategoryValue,
} from "./types.js";

// -- Typed events ---------------------------------------------------------

export {
  parseAgentEvent,
  parseMobEventFromSse,
  parseAgentEventFromSse,
  isTextDelta,
  isTextComplete,
  isRunCompleted,
  isRunFailed,
  isTurnCompleted,
  isToolCallRequested,
  EventStream,
} from "./events.js";

export type {
  AgentEvent,
  RunStartedEvent,
  RunCompletedEvent,
  RunFailedEvent,
  TurnStartedEvent,
  TextDeltaEvent,
  TextCompleteEvent,
  ToolCallRequestedEvent,
  ToolResultReceivedEvent,
  TurnCompletedEvent,
  ToolExecutionStartedEvent,
  ToolExecutionCompletedEvent,
  UnknownEvent,
  MobEventEnvelope,
  AgentEventEnvelope,
} from "./events.js";

// -- Config modules -------------------------------------------------------

export { auth, memory, sessionStore } from "./config/index.js";

// -- Module authoring helpers ---------------------------------------------

export {
  defineModuleSpec,
  decorateModuleSpec,
  decorateModuleTool,
  defineModuleTool,
  defineModule,
  buildConsoleRoute,
  buildConsoleModulesRoute,
  buildConsoleExperienceRoute,
  buildConsoleRoutes,
} from "./helpers.js";

export type {
  RestartPolicy,
  ModuleSpec,
  ModuleSpecDecorator,
  ModuleToolContext,
  ModuleToolHandler,
  ModuleToolDecorator,
  ModuleToolDefinition,
  ModuleDefinition,
  ConsoleRoutes,
} from "./helpers.js";

// -- Transport (advanced usage) -------------------------------------------

export {
  PersistentTransport,
  buildJsonRpcRequest,
  createGatewaySyncTransport,
  createGatewayAsyncTransport,
  createJsonRpcHttpTransport,
} from "./transport.js";

export type {
  JsonRpcRequest,
  JsonRpcResponse,
  JsonRpcSuccess,
  JsonRpcErrorResponse,
  JsonRpcErrorBody,
  JsonRpcTransport,
  JsonRpcSyncTransport,
  CallbackHandler,
  FetchLike,
  FetchLikeResponse,
} from "./transport.js";

// -- SSE (advanced usage) -------------------------------------------------

export { parseSseStream, encodeSseEvent } from "./sse.js";
export type { SseEvent } from "./sse.js";

// -- Low-level clients (backward compat) ----------------------------------

export {
  MobkitTypedClient,
  MobkitAsyncClient,
} from "./client.js";

export type {
  MobkitStatusResult,
  MobkitCapabilitiesResult,
  MobkitReconcileResult,
  MobkitSpawnMemberResult,
  MobkitSubscribeScope,
  MobkitSubscribeParams,
  MobkitSubscribeKeepAlive,
  MobkitEventEnvelope,
  MobkitSubscribeResult,
} from "./client.js";
