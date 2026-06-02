import {
  type ConsoleWorkbenchTarget,
  type MobKitWorkbenchTarget,
} from "@console-core";
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
} from "./network";
import type {
  ConsoleExperience,
  ConsoleFrame,
  ConsoleModulesResponse,
  ConsoleTimelineAccepted,
  ConsoleTimelinePage,
} from "../types";

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
} as const;

export type ConsoleCommandName = typeof CONSOLE_COMMAND_NAMES[keyof typeof CONSOLE_COMMAND_NAMES];

type ConsoleCommandSpec = {
  method: typeof CONSOLE_RPC_METHODS[keyof typeof CONSOLE_RPC_METHODS];
  targetKinds: ReadonlySet<MobKitWorkbenchTarget["kind"]>;
};

const CONSOLE_COMMAND_SPECS: Record<ConsoleCommandName, ConsoleCommandSpec> = {
  [CONSOLE_COMMAND_NAMES.inspectIdentity]: {
    method: CONSOLE_RPC_METHODS.inspectIdentity,
    targetKinds: new Set<MobKitWorkbenchTarget["kind"]>([
      "mobkit/identity-chat",
      "mobkit/identity-inspect",
    ]),
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
  execute(input: ConsoleCommandRequest): Promise<ConsoleCommandResult>;
}

export interface ConsoleTimelineController {
  query(input: ConsoleTimelineQueryInput): Promise<ConsoleFact<ConsoleTimelinePage>>;
  subscribeWithBackfill(
    input: ConsoleTimelineSubscribeInput,
    onFrame: (frame: ConsoleFact<ConsoleFrame>) => void,
  ): Promise<() => void>;
}

export function createHttpConsoleTransport({ baseUrl }: { baseUrl: string }): MobKitConsoleTransport {
  return {
    loadExperience: () => fetchJson<ConsoleExperience>(baseUrl, CONSOLE_REST_PATHS.experience),
    loadModules: () => fetchJson<ConsoleModulesResponse>(baseUrl, CONSOLE_REST_PATHS.modules),
    capabilities: async () => normalizeCapabilities(
      await callConsoleRpc<unknown>(baseUrl, CONSOLE_RPC_METHODS.capabilities),
    ),
    queryTimeline: (input) => queryTimeline(baseUrl, input, input.limit),
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
        );
      }
      return sendConsole(
        baseUrl,
        input.identity,
        input.content,
        input.origin,
        input.idempotencyKey,
        handlingMode,
      );
    },
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

function createConsoleCommandSurface(
  transport: MobKitConsoleTransport,
  facts: MobKitConsoleController["facts"],
): ConsoleCommandSurface {
  return {
    async sendMessage(target, input) {
      const identity = identityForSendTarget(target);
      if (!identity) {
        throw new Error(`target ${target.kind} cannot send MobKit console messages`);
      }
      const capabilities = await transport.capabilities();
      requireCapability(capabilities, CONSOLE_RPC_METHODS.send);
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
          capabilityVersion: capabilities.version,
          correlationId: input.idempotencyKey,
          cursor: accepted.cursor,
        }),
      };
    },
    async execute(input) {
      if (!isMobKitTarget(input.target)) {
        throw new Error(`host target ${input.target.kind} cannot execute MobKit commands`);
      }
      const spec = commandSpec(input.command);
      if (!spec.targetKinds.has(input.target.kind)) {
        throw new Error(`target ${input.target.kind} cannot execute command ${input.command}`);
      }
      const capabilities = await transport.capabilities();
      requireCapability(capabilities, spec.method);
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
      const delivered = new Set<string>();
      const deliver = (frame: ConsoleFrame) => {
        const key = frame.id || `${frame.event}:${frame.cursor || frame.timestampMs || delivered.size}`;
        if (delivered.has(key)) return;
        delivered.add(key);
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
    runtime_capabilities: record.runtime_capabilities,
    method_capabilities: record.method_capabilities,
  };
}

function requireCapability(capabilities: ConsoleCapabilities, method: string) {
  if (!capabilities.methods.includes(method)) {
    throw new Error(`MobKit capability missing for ${method}`);
  }
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
  return target.kind === "mobkit/identity-chat" ? target.identity : null;
}

function isMobKitTarget(target: ConsoleWorkbenchTarget): target is MobKitWorkbenchTarget {
  return target.kind.startsWith("mobkit/");
}
