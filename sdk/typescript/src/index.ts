/**
 * MobKit TypeScript SDK - companion gateway and operator layer for Meerkat
 * agents and mobs.
 *
 * @example
 * ```ts
 * import { MobKit } from "@rkat/mobkit-sdk";
 *
 * const rt = await MobKit.builder()
 *   .mob("config/mob.toml")
 *   .gateway("./target/release/rpc_gateway")
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
export {
  LIVE_EXECUTION_CLIENT_CONTEXT_V1,
  LIVE_EXECUTION_FUNCTION_BRIDGE_V1,
  LIVE_EXECUTION_IDENTITY_V1,
  activeLiveChannelHandleToWire,
  experimentalLiveGatewayConfigToWire,
  liveExecutionIdentityV1ToWire,
  liveChannelHandleToWire,
  liveOpenExecutionIdentityParams,
  liveExecutionModeCapability,
  pendingLiveChannelHandleToWire,
  parseActiveLiveChannelHandle,
  parseExperimentalLiveChannelStatus,
  parseLivePlaybackOwnerReadiness,
  parsePendingLiveChannelHandle,
  parseLiveChannelHandle,
  parseLiveAssistantOutputAddress,
  parseLivePlaybackCompleteResult,
  parseLiveReplacementRequired,
  supportsLiveExecutionIdentityV1,
  supportsLiveExecutionMode,
} from "./live.js";
export type {
  ActiveLiveChannelHandle,
  ExperimentalLiveChannelStatus,
  ExperimentalLiveGatewayConfig,
  LiveAuthBindingOverride,
  LiveAuthBindingRef,
  LiveAssistantOutputAddress,
  LiveChannelCapabilities,
  LiveChannelHandle,
  LiveContinuityMode,
  LiveExecutionIdentityV1,
  LiveExecutionIdentityWireV1,
  LiveExecutionMode,
  LivePlaybackOwnerReadiness,
  LivePlaybackOwner,
  PendingLiveChannelHandle,
  LiveProvider,
  LivePlaybackCompleteResult,
  LiveReplacementRequired,
  LiveTransportBootstrap,
} from "./live.js";
export {
  MobKitRuntime,
  MobHandle,
  ToolCaller,
  SseBridge,
  JobsHandle,
  MonitorsHandle,
} from "./runtime.js";
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
  JobsListOptions,
  JobSubscriptionOptions,
  MonitorStartOptions,
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

// Detached callback-job host contracts. The runtime's generated machine
// remains authoritative; these types only bind host execution/reporting.
export {
  DetachedJobExecution,
  DetachedJobResult,
} from "./jobs.js";
export type {
  DetachedJobAuthority,
  DetachedJobContext,
  DetachedJobExecutionOptions,
  DetachedJobHandler,
  DetachedJobRunner,
  JobCredentialResolver,
  JobIdempotencyScope,
  JobRestartClass,
} from "./jobs.js";
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
  STORAGE_RESOLUTION_CODE,
  WORKGRAPH_UNAVAILABLE_CODE,
  WORKGRAPH_CONFLICT_CODE,
  CapabilityUnavailableError,
  ConsoleTimelineReplayUnavailableError,
  LeaseLostError,
  MemoryBackendUnavailableError,
  StorageResolutionError,
  WorkGraphUnavailableError,
  WorkGraphConflictError,
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
  RESOLUTION_ERROR_CATEGORIES,
  isResolutionErrorEvent,
  parseStatusResult,
  parseStorageDoctorResult,
  parseStorageSummary,
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
  parseMemberProgressSnapshot,
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
  parseIdentityInspection,
  identityInspectionToDict,
  parseSendResult,
  sendResultToDict,
  parseDispatchResult,
  dispatchResultToDict,
  parseCompletionCursor,
  completionCursorToDict,
  completionProgressSince,
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
  // WorkGraph parsers + option toDict helpers
  parseWorkGraphOwnerKey,
  workGraphOwnerKeyInputToDict,
  parseWorkGraphOwner,
  workGraphOwnerInputToDict,
  parseWorkGraphClaim,
  parseWorkGraphExternalRef,
  parseWorkGraphEvidenceRef,
  workGraphEvidenceInputToDict,
  parseWorkGraphItem,
  parseWorkGraphEdge,
  parseWorkGraphWorkRef,
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
  workGraphClaimOptionsToDict,
  workGraphCloseOptionsToDict,
  workGraphGoalTargetToDict,
  workGraphGoalCreateOptionsToDict,
  workGraphGoalConfirmOptionsToDict,
  workGraphGoalRequestCloseOptionsToDict,
  workGraphAttentionPauseOptionsToDict,
} from "./types.js";

export type {
  StatusResult,
  StorageDoctorResult,
  StorageDoctorFinding,
  StorageSummary,
  StorageSlotSummary,
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
  MemberProgressSnapshot,
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
  IdentityInspection,
  SendResult,
  DispatchResult,
  CompletionCursor,
  CompletionProgress,
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
  // WorkGraph types
  WorkGraphOwnerKey,
  WorkGraphOwnerKeyInput,
  WorkGraphOwner,
  WorkGraphOwnerInput,
  WorkGraphClaim,
  WorkGraphExternalRef,
  WorkGraphEvidenceRef,
  WorkGraphEvidenceInput,
  WorkGraphItem,
  WorkGraphEdge,
  WorkGraphWorkRef,
  WorkGraphAttentionBinding,
  WorkGraphSnapshotResult,
  WorkGraphItemsResult,
  WorkGraphGoalResult,
  WorkGraphAttentionReassignResult,
  WorkGraphEventEntry,
  WorkGraphFilterOptions,
  WorkGraphReadyOptions,
  WorkGraphEventsOptions,
  WorkGraphAttentionListOptions,
  WorkGraphCreateOptions,
  WorkGraphUpdateOptions,
  WorkGraphClaimOptions,
  WorkGraphCloseOptions,
  WorkGraphGoalTarget,
  WorkGraphGoalCreateOptions,
  WorkGraphGoalConfirmOptions,
  WorkGraphGoalRequestCloseOptions,
  WorkGraphAttentionPauseOptions,
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

export { auth, eventLog, memory, runtimeStore, sessionStore } from "./config/index.js";

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
