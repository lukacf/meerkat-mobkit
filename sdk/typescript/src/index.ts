/**
 * MobKit TypeScript SDK — companion orchestration for the Meerkat runtime.
 *
 * @example
 * ```ts
 * import { MobKit } from "@rkat/mobkit-sdk";
 *
 * const rt = await MobKit.builder()
 *   .mob("config/mob.toml")
 *   .gateway("./target/release/mobkit_gateway")
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
export type {
  BlobUploadInput,
  BlobUploadSource,
  SendMessageOptions,
  ForgetAgentMemoryOptions,
  ManifestAgentMemoryOptions,
  RecallAgentMemoryOptions,
  RememberAgentMemoryOptions,
  UpdateAgentMemoryOptions,
  MobpackImportOptions,
  MobpackCreateOptions,
  MobpackSaveOptions,
  MobpackHistoryOptions,
} from "./runtime.js";

// -- Data models ----------------------------------------------------------

// Rich tool-result content blocks for callback tool handlers.
export { textBlock, imageBlock, imageBlobBlock, toolContent, ToolResultContent } from "./tool-content.js";
export type { ContentBlock } from "./tool-content.js";

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
  MobEventsStaleError,
  MOB_EVENTS_STALE_CURSOR_CODE,
  CAPABILITY_UNAVAILABLE_CODE,
  CONSOLE_TIMELINE_REPLAY_UNAVAILABLE_CODE,
  LEASE_LOST_CODE,
  MEMORY_BACKEND_UNAVAILABLE_CODE,
  CapabilityUnavailableError,
  ConsoleTimelineReplayUnavailableError,
  LeaseLostError,
  MemoryBackendUnavailableError,
  ContractMismatchError,
  NotConnectedError,
  MobkitRpcError,
  isRpcError,
  isMobEventsStaleError,
} from "./errors.js";

// -- Typed return models --------------------------------------------------

export {
  MEMBER_STATE_ACTIVE,
  MEMBER_STATE_RETIRING,
  MEMBER_STATE_BROKEN,
  MEMBER_STATE_COMPLETED,
  MEMBER_STATE_UNKNOWN,
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
  parseAgentMemoryRecordMeta,
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
  parseHelperResult,
  parseMobRunSnapshot,
  parseCrossMobContactEntry,
  parseModelsCatalogResult,
  parseMobpackToolsCatalogResult,
  parseMobpackSkillsCatalogResult,
  parseMobpackAgentDefinitionsResult,
  parseMobpackTemplatesResult,
  parseMobpackCatalogsResult,
  parseMobpackDiagnostic,
  parseMobpackDisplayRow,
  parseMobpackValidationResult,
  parseMobpackSourceFile,
  parseMobpackSourceResult,
  parseMobpackExportResult,
  parseMobpackImportResult,
  parseMobpackDraftRow,
  parseMobpackDraftListResult,
  parseMobpackDraftGetResult,
  parseMobpackDraftSaveResult,
  parseMobpackDraftDeleteResult,
  parseMobpackDraftHistoryResult,
  parseMobpackApplyOperationResult,
  parseMobpackDeployCommandResult,
  parseMobpackDeployResult,
  parseStepRecord,
  parseFailureRecord,
  parseFrameRecord,
  parseLoopRecord,
  parseLoopIterationRecord,
  parseErrorEvent,
  eventQueryToDict,
  // Identity-first parsers
  parseDurableAgentSpec,
  durableAgentSpecToDict,
  parseDispatchInput,
  dispatchInputToDict,
  parseManagedPeerEdge,
  managedPeerEdgeToDict,
  parseExternalToolDef,
  externalToolDefToDict,
  parseAgentBuildContext,
  parseAgentBuildDraft,
  agentBuildDraftToDict,
  parseIdentityStatus,
  parseContinuityRecord,
  continuityRecordToDict,
  parseContinuityFailure,
  parseContinuityResolveState,
  parseSessionSnapshot,
  sessionSnapshotToDict,
  parseLeaseGrant,
  leaseGrantToDict,
  parseLeaseAcquireResult,
  leaseAcquireResultToDict,
  parseLeaseRenewResult,
  leaseRenewResultToDict,
  parseBlobUploadResult,
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
  MemoryAssertion,
  MemoryConflictSignal,
  MemoryQueryResult,
  AgentMemoryRecord,
  AgentMemoryRecordMeta,
  AgentMemoryRecallResult,
  AgentMemoryForgetResult,
  AgentMemoryUpdateResult,
  AgentMemoryManifestResult,
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
  MobStructuralEvent,
  MobRun,
  MobRunStatus,
  MobMemberStatus,
  RichMemberSnapshot,
  IdentityResolvedToolsResult,
  HelperResult,
  MobRunSnapshot,
  MobUnreachablePeer,
  PeerConnectivitySnapshot,
  CrossMobContactEntry,
  CatalogEntry,
  ProviderDefaults,
  ModelsCatalogResult,
  MobpackToolsCatalogResult,
  MobpackSkillsCatalogResult,
  MobpackAgentDefinitionsResult,
  MobpackTemplatesResult,
  MobpackCatalogsResult,
  MobpackDiagnostic,
  MobpackDisplayRow,
  MobpackValidationResult,
  MobpackSourceFile,
  MobpackSourceResult,
  MobpackExportResult,
  MobpackImportResult,
  MobpackDraftRow,
  MobpackDraftListResult,
  MobpackDraftGetResult,
  MobpackDraftSaveResult,
  MobpackDraftDeleteResult,
  MobpackDraftHistoryResult,
  MobpackApplyOperationResult,
  MobpackDeployCommandResult,
  MobpackDeployResult,
  StepRecord,
  FailureRecord,
  FrameRecord,
  LoopRecord,
  LoopIterationRecord,
  EventQuery,
  ErrorEvent,
  ErrorCategoryValue,
  SessionCreatedContext,
  // Identity-first types
  DurableAgentSpec,
  DispatchInput,
  DispatchContentBlock,
  TextContentBlock,
  ImageContentBlock,
  InlineImageContentBlock,
  BlobImageContentBlock,
  BlobGetResult,
  BlobUploadResult,
  DispatchOrigin,
  ManagedPeerEdge,
  ExternalToolDef,
  AgentBuildContext,
  AgentBuildDraft,
  IdentityStatus,
  LeaseInfo,
  DurabilityPolicy,
  ContinuityHealth,
  ContinuityRecord,
  ContinuityFailure,
  ContinuityResolveState,
  SessionSnapshot,
  LeaseGrant,
  LeaseAcquireResult,
  LeaseRenewResult,
  // Provider interfaces
  ContinuityStore,
  LeaseProvider,
  RosterProvider,
  AgentCustomizer,
  TopologyProvider,
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
  ModuleBoundary,
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
