import {
  type ConsoleWorkbenchTarget,
  type MobKitWorkbenchTarget,
} from "./targets";
import {
  CONSOLE_BLOB_PATH_PREFIX,
  CONSOLE_REST_PATHS,
  CONSOLE_RPC_METHODS,
} from "./contract";
import {
  callConsoleRpc,
  fetchJson,
  queryTimeline,
  sendConsole,
  sendConsoleMultipart,
  subscribeTimelineEvents,
  uploadConsoleBlobMultipart,
} from "./network";
import type {
  ConsoleExperience,
  ConsoleFrame,
  ConsoleModulesResponse,
  ConsoleTimelineAccepted,
  ConsoleTimelinePage,
} from "./runtime-types";

export type ConsoleFactSource =
  | "mobkit-protocol"
  | "controller-derived"
  | "optimistic"
  | "host-adapter";

export interface ConsoleFact<T> {
  value: T;
  provenance: {
    source: ConsoleFactSource;
    contractVersion?: string;
    routeOrMethod?: string;
    cursor?: string;
    capabilityVersion?: string;
    timestampMs?: number;
    correlationId?: string;
  };
}

export interface ConsoleCapabilities {
  methods: string[];
  version?: string;
  readOnly?: boolean;
  runtime_capabilities?: unknown;
  method_capabilities?: unknown;
}

export interface ConsoleTimelineQueryInput {
  identity?: string;
  conversationId?: string;
  after?: string;
  before?: string;
  mode?: "since" | "recent";
  limit?: number;
}

export interface ConsoleTimelineSubscribeInput {
  identity?: string;
  conversationId?: string;
  after?: string;
  limit?: number;
}

export interface ConsoleSendInput {
  identity: string;
  content: string | Array<Record<string, unknown>>;
  origin: string;
  idempotencyKey: string;
  handlingMode?: "queue" | "steer";
  attachments?: File[];
}

export interface ConsoleUploadInput {
  blobId?: string;
  file?: File;
  mediaType?: string;
}

export interface ConsoleUploadResult {
  blob_id: string;
  url?: string;
}

export const CONSOLE_COMMAND_NAMES = {
  inspectIdentity: "inspectIdentity",
  retireIdentity: "retireIdentity",
  respawnIdentity: "respawnIdentity",
  resetIdentity: "resetIdentity",
  listRoutingRoutes: "listRoutingRoutes",
  listDeliveryHistory: "listDeliveryHistory",
  listGatingPending: "listGatingPending",
  listGatingAudit: "listGatingAudit",
  decideGating: "decideGating",
  accessStatus: "accessStatus",
  getAccessConfig: "getAccessConfig",
  setAccessConfig: "setAccessConfig",
  enableAccess: "enableAccess",
  upsertAccessRule: "upsertAccessRule",
  deleteAccessRule: "deleteAccessRule",
  setAccessGroup: "setAccessGroup",
  deleteAccessGroup: "deleteAccessGroup",
  previewAccess: "previewAccess",
  listMemoryRecords: "listMemoryRecords",
  getMemoryRecord: "getMemoryRecord",
  listMemoryQuarantine: "listMemoryQuarantine",
  listMemoryDreams: "listMemoryDreams",
  getMemoryOverview: "getMemoryOverview",
  listMemoryProposals: "listMemoryProposals",
  listMemoryInjections: "listMemoryInjections",
  listMemoryHarvests: "listMemoryHarvests",
  listMemoryDreamRuns: "listMemoryDreamRuns",
  listMemoryAuditVerdicts: "listMemoryAuditVerdicts",
} as const;

export type ConsoleCommandName = typeof CONSOLE_COMMAND_NAMES[keyof typeof CONSOLE_COMMAND_NAMES];

type ConsoleCommandSpec = {
  method: typeof CONSOLE_RPC_METHODS[keyof typeof CONSOLE_RPC_METHODS];
  targetKinds: ReadonlySet<MobKitWorkbenchTarget["kind"]>;
};

const LEGACY_INSPECT_IDENTITY_METHOD = "mobkit/inspect_identity";
const MIN_TIMELINE_DEDUP_KEYS = 1_000;

const CONSOLE_COMMAND_SPECS: Record<ConsoleCommandName, ConsoleCommandSpec> = {
  [CONSOLE_COMMAND_NAMES.inspectIdentity]: {
    method: CONSOLE_RPC_METHODS.inspectIdentity,
    targetKinds: new Set<MobKitWorkbenchTarget["kind"]>([
      "mobkit/identity-chat",
      "mobkit/identity-inspect",
    ]),
  },
  [CONSOLE_COMMAND_NAMES.retireIdentity]: {
    method: CONSOLE_RPC_METHODS.retireIdentity,
    targetKinds: new Set<MobKitWorkbenchTarget["kind"]>([
      "mobkit/identity-chat",
      "mobkit/identity-inspect",
    ]),
  },
  [CONSOLE_COMMAND_NAMES.respawnIdentity]: {
    method: CONSOLE_RPC_METHODS.respawnIdentity,
    targetKinds: new Set<MobKitWorkbenchTarget["kind"]>([
      "mobkit/identity-chat",
      "mobkit/identity-inspect",
    ]),
  },
  [CONSOLE_COMMAND_NAMES.resetIdentity]: {
    method: CONSOLE_RPC_METHODS.resetIdentity,
    targetKinds: new Set<MobKitWorkbenchTarget["kind"]>([
      "mobkit/identity-chat",
      "mobkit/identity-inspect",
    ]),
  },
  [CONSOLE_COMMAND_NAMES.listRoutingRoutes]: {
    method: CONSOLE_RPC_METHODS.routingRoutesList,
    targetKinds: new Set<MobKitWorkbenchTarget["kind"]>(["mobkit/routing"]),
  },
  [CONSOLE_COMMAND_NAMES.listDeliveryHistory]: {
    method: CONSOLE_RPC_METHODS.deliveryHistory,
    targetKinds: new Set<MobKitWorkbenchTarget["kind"]>(["mobkit/routing"]),
  },
  [CONSOLE_COMMAND_NAMES.listGatingPending]: {
    method: CONSOLE_RPC_METHODS.gatingPending,
    targetKinds: new Set<MobKitWorkbenchTarget["kind"]>(["mobkit/gating"]),
  },
  [CONSOLE_COMMAND_NAMES.listGatingAudit]: {
    method: CONSOLE_RPC_METHODS.gatingAudit,
    targetKinds: new Set<MobKitWorkbenchTarget["kind"]>(["mobkit/gating"]),
  },
  [CONSOLE_COMMAND_NAMES.decideGating]: {
    method: CONSOLE_RPC_METHODS.gatingDecide,
    targetKinds: new Set<MobKitWorkbenchTarget["kind"]>(["mobkit/gating"]),
  },
  [CONSOLE_COMMAND_NAMES.accessStatus]: {
    method: CONSOLE_RPC_METHODS.accessStatus,
    targetKinds: new Set<MobKitWorkbenchTarget["kind"]>(["mobkit/access"]),
  },
  [CONSOLE_COMMAND_NAMES.getAccessConfig]: {
    method: CONSOLE_RPC_METHODS.accessGet,
    targetKinds: new Set<MobKitWorkbenchTarget["kind"]>(["mobkit/access"]),
  },
  [CONSOLE_COMMAND_NAMES.setAccessConfig]: {
    method: CONSOLE_RPC_METHODS.accessSet,
    targetKinds: new Set<MobKitWorkbenchTarget["kind"]>(["mobkit/access"]),
  },
  [CONSOLE_COMMAND_NAMES.enableAccess]: {
    method: CONSOLE_RPC_METHODS.accessEnable,
    targetKinds: new Set<MobKitWorkbenchTarget["kind"]>(["mobkit/access"]),
  },
  [CONSOLE_COMMAND_NAMES.upsertAccessRule]: {
    method: CONSOLE_RPC_METHODS.accessRuleUpsert,
    targetKinds: new Set<MobKitWorkbenchTarget["kind"]>(["mobkit/access"]),
  },
  [CONSOLE_COMMAND_NAMES.deleteAccessRule]: {
    method: CONSOLE_RPC_METHODS.accessRuleDelete,
    targetKinds: new Set<MobKitWorkbenchTarget["kind"]>(["mobkit/access"]),
  },
  [CONSOLE_COMMAND_NAMES.setAccessGroup]: {
    method: CONSOLE_RPC_METHODS.accessGroupSet,
    targetKinds: new Set<MobKitWorkbenchTarget["kind"]>(["mobkit/access"]),
  },
  [CONSOLE_COMMAND_NAMES.deleteAccessGroup]: {
    method: CONSOLE_RPC_METHODS.accessGroupDelete,
    targetKinds: new Set<MobKitWorkbenchTarget["kind"]>(["mobkit/access"]),
  },
  [CONSOLE_COMMAND_NAMES.previewAccess]: {
    method: CONSOLE_RPC_METHODS.accessPreview,
    targetKinds: new Set<MobKitWorkbenchTarget["kind"]>(["mobkit/access"]),
  },
  [CONSOLE_COMMAND_NAMES.listMemoryRecords]: {
    method: CONSOLE_RPC_METHODS.memoryPanelRecords,
    targetKinds: new Set<MobKitWorkbenchTarget["kind"]>(["mobkit/memory"]),
  },
  [CONSOLE_COMMAND_NAMES.getMemoryRecord]: {
    method: CONSOLE_RPC_METHODS.memoryPanelRecord,
    targetKinds: new Set<MobKitWorkbenchTarget["kind"]>(["mobkit/memory"]),
  },
  [CONSOLE_COMMAND_NAMES.listMemoryQuarantine]: {
    method: CONSOLE_RPC_METHODS.memoryPanelQuarantine,
    targetKinds: new Set<MobKitWorkbenchTarget["kind"]>(["mobkit/memory"]),
  },
  [CONSOLE_COMMAND_NAMES.listMemoryDreams]: {
    method: CONSOLE_RPC_METHODS.memoryPanelDreams,
    targetKinds: new Set<MobKitWorkbenchTarget["kind"]>(["mobkit/memory"]),
  },
  [CONSOLE_COMMAND_NAMES.getMemoryOverview]: {
    method: CONSOLE_RPC_METHODS.memoryPanelOverview,
    targetKinds: new Set<MobKitWorkbenchTarget["kind"]>(["mobkit/memory"]),
  },
  [CONSOLE_COMMAND_NAMES.listMemoryProposals]: {
    method: CONSOLE_RPC_METHODS.memoryPanelProposals,
    targetKinds: new Set<MobKitWorkbenchTarget["kind"]>(["mobkit/memory"]),
  },
  [CONSOLE_COMMAND_NAMES.listMemoryInjections]: {
    method: CONSOLE_RPC_METHODS.memoryPanelInjections,
    targetKinds: new Set<MobKitWorkbenchTarget["kind"]>(["mobkit/memory"]),
  },
  [CONSOLE_COMMAND_NAMES.listMemoryHarvests]: {
    method: CONSOLE_RPC_METHODS.memoryPanelHarvests,
    targetKinds: new Set<MobKitWorkbenchTarget["kind"]>(["mobkit/memory"]),
  },
  [CONSOLE_COMMAND_NAMES.listMemoryDreamRuns]: {
    method: CONSOLE_RPC_METHODS.memoryPanelDreamRuns,
    targetKinds: new Set<MobKitWorkbenchTarget["kind"]>(["mobkit/memory"]),
  },
  [CONSOLE_COMMAND_NAMES.listMemoryAuditVerdicts]: {
    method: CONSOLE_RPC_METHODS.memoryPanelAuditVerdicts,
    targetKinds: new Set<MobKitWorkbenchTarget["kind"]>(["mobkit/memory"]),
  },
};

export interface ConsoleCommandRequest {
  command: ConsoleCommandName;
  target: ConsoleWorkbenchTarget;
  params?: Record<string, unknown>;
}

export interface ConsoleCommandResult {
  command: ConsoleCommandName;
  accepted: boolean;
  result?: unknown;
}

export interface MobKitConsoleTransport {
  loadExperience(): Promise<ConsoleExperience>;
  loadModules?(): Promise<ConsoleModulesResponse>;
  capabilities(): Promise<ConsoleCapabilities>;
  queryTimeline(input: ConsoleTimelineQueryInput): Promise<ConsoleTimelinePage>;
  subscribeTimeline(
    input: ConsoleTimelineSubscribeInput,
    onFrame: (frame: ConsoleFrame) => void,
  ): () => void;
  send(input: ConsoleSendInput): Promise<ConsoleTimelineAccepted>;
  executeCommand?(input: ConsoleCommandRequest): Promise<ConsoleCommandResult>;
  upload?(input: ConsoleUploadInput): Promise<ConsoleUploadResult>;
  blobUrl?(blobId: string): string;
}

export interface MobKitConsoleController {
  transport: MobKitConsoleTransport;
  commands: ConsoleCommandSurface;
  timeline: ConsoleTimelineController;
  facts: {
    mobkit<T>(value: T, meta?: Partial<ConsoleFact<T>["provenance"]>): ConsoleFact<T>;
    derived<T>(value: T, meta?: Partial<ConsoleFact<T>["provenance"]>): ConsoleFact<T>;
    optimistic<T>(value: T, correlationId: string): ConsoleFact<T>;
    host<T>(value: T): ConsoleFact<T>;
  };
}

export interface ConsoleCommandSurface {
  sendMessage(
    target: ConsoleWorkbenchTarget,
    input: Omit<ConsoleSendInput, "identity">,
  ): Promise<{
    optimistic: ConsoleFact<{ idempotencyKey: string; targetId: string }>;
    accepted: ConsoleFact<ConsoleTimelineAccepted>;
  }>;
  uploadBlob(input: ConsoleUploadInput): Promise<ConsoleFact<ConsoleUploadResult>>;
  execute(input: ConsoleCommandRequest): Promise<ConsoleCommandResult>;
}

export interface ConsoleTimelineController {
  query(input: ConsoleTimelineQueryInput): Promise<ConsoleFact<ConsoleTimelinePage>>;
  subscribeWithBackfill(
    input: ConsoleTimelineSubscribeInput,
    onFrame: (frame: ConsoleFact<ConsoleFrame>) => void,
  ): Promise<() => void>;
}

export function createHttpConsoleTransport({
  baseUrl,
  fetchTimeoutMs,
}: {
  baseUrl: string;
  fetchTimeoutMs?: number | (() => number);
}): MobKitConsoleTransport {
  const timeout = () => typeof fetchTimeoutMs === "function" ? fetchTimeoutMs() : fetchTimeoutMs;
  return {
    loadExperience: () => fetchJson<ConsoleExperience>(baseUrl, CONSOLE_REST_PATHS.experience, timeout()),
    loadModules: () => fetchJson<ConsoleModulesResponse>(baseUrl, CONSOLE_REST_PATHS.modules, timeout()),
    capabilities: async () => normalizeCapabilities(
      await callConsoleRpc<unknown>(baseUrl, CONSOLE_RPC_METHODS.capabilities, {}, timeout()),
    ),
    queryTimeline: (input) => queryTimeline(baseUrl, input, input.limit, timeout()),
    subscribeTimeline: (input, onFrame) => subscribeTimelineEvents(baseUrl, input, onFrame),
    send: (input) => {
      const handlingMode = input.handlingMode ?? "queue";
      if (input.attachments?.length) {
        return sendConsoleMultipart(
          baseUrl,
          input.identity,
          input.content,
          input.attachments,
          input.origin,
          input.idempotencyKey,
          handlingMode,
          timeout(),
        );
      }
      return sendConsole(
        baseUrl,
        input.identity,
        input.content,
        input.origin,
        input.idempotencyKey,
        handlingMode,
        timeout(),
      );
    },
    executeCommand: async (input) => {
      const spec = commandSpec(input.command);
      const params = { ...(input.params || {}) };
      if (identityCommandMethods.has(spec.method)) {
        const identity = stringValue(params.identity) || identityForCommandTarget(input.target);
        if (!identity) {
          throw new Error(`${input.command} requires an identity-addressed target`);
        }
        params.identity = identity;
      }
      let result: unknown;
      try {
        result = await callConsoleRpc<unknown>(baseUrl, spec.method, params, timeout());
      } catch (error) {
        if (
          spec.method !== CONSOLE_RPC_METHODS.inspectIdentity ||
          !isJsonRpcMethodNotFoundError(error)
        ) {
          throw error;
        }
        result = await callConsoleRpc<unknown>(baseUrl, LEGACY_INSPECT_IDENTITY_METHOD, params, timeout());
      }
      return {
        command: input.command,
        accepted: true,
        result,
      };
    },
    upload: (input) => uploadConsoleBlobMultipart(baseUrl, input, timeout()),
    blobUrl: (blobId) => `${baseUrl}${CONSOLE_BLOB_PATH_PREFIX}${encodeURIComponent(blobId)}`,
  };
}

export function createMobKitConsoleController({
  transport,
}: {
  transport: MobKitConsoleTransport;
}): MobKitConsoleController {
  const facts = createFactFactory();
  return {
    transport,
    facts,
    timeline: createTimelineController(transport, facts),
    commands: createConsoleCommandSurface(transport, facts),
  };
}

function isJsonRpcMethodNotFoundError(error: unknown): boolean {
  const rpcError = (error as { rpcError?: { code?: unknown } } | null)?.rpcError;
  return rpcError?.code === -32601;
}

function createConsoleCommandSurface(
  transport: MobKitConsoleTransport,
  facts: MobKitConsoleController["facts"],
): ConsoleCommandSurface {
  let cachedCapabilities: ConsoleCapabilities | null = null;
  let capabilitiesRequest: Promise<ConsoleCapabilities> | null = null;
  const capabilities = async (force = false) => {
    if (force || !cachedCapabilities) {
      if (!capabilitiesRequest) {
        capabilitiesRequest = transport.capabilities().finally(() => {
          capabilitiesRequest = null;
        });
      }
      cachedCapabilities = await capabilitiesRequest;
    }
    return cachedCapabilities;
  };
  const requireFreshCapability = async (method: string) => {
    let currentCapabilities = await capabilities(true);
    if (!hasCapability(currentCapabilities, method)) {
      currentCapabilities = await capabilities(true);
    }
    requireCapability(currentCapabilities, method);
    return currentCapabilities;
  };
  return {
    async sendMessage(target, input) {
      const identity = identityForSendTarget(target);
      if (!identity) {
        throw new Error(`target ${target.kind} cannot send MobKit console messages`);
      }
      const currentCapabilities = await requireFreshCapability(CONSOLE_RPC_METHODS.send);
      const optimistic = facts.optimistic({
        idempotencyKey: input.idempotencyKey,
        targetId: target.id,
      }, input.idempotencyKey);
      const accepted = await transport.send({
        ...input,
        identity,
      });
      return {
        optimistic,
        accepted: facts.mobkit(accepted, {
          routeOrMethod: CONSOLE_RPC_METHODS.send,
          capabilityVersion: currentCapabilities.version,
          correlationId: input.idempotencyKey,
          cursor: accepted.cursor,
        }),
      };
    },
    async uploadBlob(input) {
      const currentCapabilities = await requireFreshCapability(CONSOLE_RPC_METHODS.blobUpload);
      if (!transport.upload) {
        throw new Error(`transport does not implement ${CONSOLE_RPC_METHODS.blobUpload}`);
      }
      const uploaded = await transport.upload(input);
      return facts.mobkit(uploaded, {
        routeOrMethod: CONSOLE_RPC_METHODS.blobUpload,
        capabilityVersion: currentCapabilities.version,
      });
    },
    async execute(input) {
      if (!isMobKitTarget(input.target)) {
        throw new Error(`host target ${input.target.kind} cannot execute MobKit commands`);
      }
      const spec = commandSpec(input.command);
      if (!spec.targetKinds.has(input.target.kind)) {
        throw new Error(`target ${input.target.kind} cannot execute command ${input.command}`);
      }
      await requireFreshCapability(spec.method);
      if (!transport.executeCommand) {
        throw new Error(`transport does not implement command ${input.command}`);
      }
      return transport.executeCommand(input);
    },
  };
}

function createTimelineController(
  transport: MobKitConsoleTransport,
  facts: MobKitConsoleController["facts"],
): ConsoleTimelineController {
  return {
    async query(input) {
      const page = await transport.queryTimeline(input);
      return facts.mobkit(page, {
        routeOrMethod: CONSOLE_RPC_METHODS.queryTimeline,
        cursor: page.latestCursor || page.nextCursor,
      });
    },
    async subscribeWithBackfill(input, onFrame) {
      const delivered = createBoundedTimelineDedupSet(input.limit);
      const deliver = (frame: ConsoleFrame) => {
        const key = timelineDedupKey(frame);
        if (key && !delivered.add(key)) return;
        onFrame(facts.mobkit(frame, {
          routeOrMethod: CONSOLE_REST_PATHS.timelineStream,
          cursor: frame.cursor,
        }));
      };
      const seed = await transport.queryTimeline({
        ...input,
        mode: "recent",
      });
      seed.frames.forEach(deliver);
      const after = seed.latestCursor || seed.nextCursor || input.after;
      const unsubscribe = transport.subscribeTimeline({ ...input, after }, (frame) => {
        if (frame.event === "replay_unavailable") {
          void transport.queryTimeline({ ...input, mode: "recent" }).then((page) => {
            page.frames.forEach(deliver);
          });
          return;
        }
        deliver(frame);
      });
      return unsubscribe;
    },
  };
}

function createBoundedTimelineDedupSet(limit: number | undefined): { add(key: string): boolean } {
  const max = Math.max(MIN_TIMELINE_DEDUP_KEYS, (limit || 400) * 4);
  const keys = new Set<string>();
  const order: string[] = [];
  return {
    add(key) {
      if (keys.has(key)) {
        return false;
      }
      keys.add(key);
      order.push(key);
      while (order.length > max) {
        const oldest = order.shift();
        if (oldest) {
          keys.delete(oldest);
        }
      }
      return true;
    },
  };
}

function timelineDedupKey(frame: ConsoleFrame): string | null {
  const id = frame.id?.trim();
  if (id) return `id:${id}`;
  const cursor = frame.cursor?.trim();
  if (cursor) return `cursor:${cursor}`;
  const timestamp = frame.timestampMs;
  if (typeof timestamp === "number") {
    return `timestamp:${frame.event || ""}:${frame.identity || ""}:${timestamp}:${stableDedupText(frame.data)}`;
  }
  return null;
}

function stableDedupText(value: unknown): string {
  try {
    return JSON.stringify(value, (_key, nested) => {
      if (!nested || typeof nested !== "object" || Array.isArray(nested)) {
        return nested;
      }
      return Object.fromEntries(
        Object.entries(nested as Record<string, unknown>).sort(([left], [right]) => (
          left.localeCompare(right)
        )),
      );
    });
  } catch {
    return String(value);
  }
}

function createFactFactory(): MobKitConsoleController["facts"] {
  const wrap = <T>(
    source: ConsoleFactSource,
    value: T,
    meta: Partial<ConsoleFact<T>["provenance"]> = {},
  ): ConsoleFact<T> => ({
    value,
    provenance: {
      source,
      timestampMs: Date.now(),
      ...meta,
    },
  });
  return {
    mobkit: (value, meta) => wrap("mobkit-protocol", value, meta),
    derived: (value, meta) => wrap("controller-derived", value, meta),
    optimistic: (value, correlationId) => wrap("optimistic", value, { correlationId }),
    host: (value) => wrap("host-adapter", value),
  };
}

function normalizeCapabilities(value: unknown): ConsoleCapabilities {
  const record = value && typeof value === "object" ? value as Record<string, unknown> : {};
  const methods = Array.isArray(record.methods)
    ? Array.from(new Set(record.methods.filter((method): method is string => typeof method === "string" && method.trim().length > 0)))
    : [];
  return {
    methods,
    version: typeof record.version === "string" ? record.version : undefined,
    ...(typeof record.read_only === "boolean" ? { readOnly: record.read_only } : {}),
    runtime_capabilities: record.runtime_capabilities,
    method_capabilities: record.method_capabilities,
  };
}

function requireCapability(capabilities: ConsoleCapabilities, method: string) {
  if (!hasCapability(capabilities, method)) {
    throw new Error(`MobKit capability missing for ${method}`);
  }
}

function hasCapability(capabilities: ConsoleCapabilities, method: string): boolean {
  return capabilities.methods.includes(method);
}

function commandSpec(command: ConsoleCommandName): ConsoleCommandSpec {
  if (!isConsoleCommandName(command)) {
    throw new Error(`unknown MobKit console command ${String(command)}`);
  }
  return CONSOLE_COMMAND_SPECS[command];
}

function isConsoleCommandName(command: unknown): command is ConsoleCommandName {
  return typeof command === "string" && command in CONSOLE_COMMAND_SPECS;
}

function identityForSendTarget(target: ConsoleWorkbenchTarget): string | null {
  return target.kind === "mobkit/identity-chat" && "identity" in target ? target.identity : null;
}

const identityCommandMethods = new Set<string>([
  CONSOLE_RPC_METHODS.inspectIdentity,
  CONSOLE_RPC_METHODS.retireIdentity,
  CONSOLE_RPC_METHODS.respawnIdentity,
  CONSOLE_RPC_METHODS.resetIdentity,
]);

function identityForCommandTarget(target: ConsoleWorkbenchTarget): string | null {
  if (
    (target.kind === "mobkit/identity-chat" || target.kind === "mobkit/identity-inspect")
    && "identity" in target
  ) {
    return target.identity;
  }
  return null;
}

function stringValue(value: unknown): string | null {
  return typeof value === "string" && value.trim() ? value.trim() : null;
}

function isMobKitTarget(target: ConsoleWorkbenchTarget): target is MobKitWorkbenchTarget {
  return target.kind.startsWith("mobkit/");
}
