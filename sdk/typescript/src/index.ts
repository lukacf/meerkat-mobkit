declare function require(id: string): any;
declare const process: { env: Record<string, string | undefined> };

export type JsonRpcSuccess = {
  jsonrpc: "2.0";
  id: string;
  result: unknown;
};

export type JsonRpcError = {
  jsonrpc: "2.0";
  id: string;
  error: {
    code: number;
    message: string;
  };
};

export type JsonRpcResponse = JsonRpcSuccess | JsonRpcError;

export type JsonRpcRequest = {
  jsonrpc: "2.0";
  id: string;
  method: string;
  params: Record<string, unknown>;
};

export type JsonRpcTransport = (request: JsonRpcRequest) => Promise<unknown>;
export type JsonRpcSyncTransport = (request: JsonRpcRequest) => unknown;

export type FetchLikeResponse = {
  ok: boolean;
  status: number;
  text(): Promise<string>;
};

export type FetchLike = (url: string, init: {
  method: "POST";
  headers: Record<string, string>;
  body: string;
}) => Promise<FetchLikeResponse>;

export type MobkitStatusResult = {
  contract_version: string;
  running: boolean;
  loaded_modules: string[];
};

export type MobkitCapabilitiesResult = {
  contract_version: string;
  methods: string[];
  loaded_modules: string[];
};

export type MobkitReconcileResult = {
  accepted: boolean;
  reconciled_modules: string[];
  added: number;
};

export type MobkitSpawnMemberResult = {
  accepted: boolean;
  module_id: string;
};

export type MobkitSubscribeScope = "mob" | "agent" | "interaction";

export type MobkitSubscribeParams = {
  scope?: MobkitSubscribeScope;
  last_event_id?: string;
  agent_id?: string;
};

export type MobkitSubscribeKeepAlive = {
  interval_ms: number;
  event: string;
};

export type MobkitEventEnvelope = {
  event_id: string;
  source: string;
  timestamp_ms: number;
  event: unknown;
};

export type MobkitSubscribeResult = {
  scope: MobkitSubscribeScope;
  replay_from_event_id: string | null;
  keep_alive: MobkitSubscribeKeepAlive;
  keep_alive_comment: string;
  event_frames: string[];
  events: MobkitEventEnvelope[];
};

export class MobkitRpcError extends Error {
  constructor(
    readonly code: number,
    message: string,
    readonly requestId: string,
    readonly method: string,
  ) {
    super(message);
    this.name = "MobkitRpcError";
  }
}

export type ModuleSpec = {
  id: string;
  command: string;
  args: string[];
  restart_policy: "never" | "always" | "on_failure";
};

export type ModuleSpecDecorator = (spec: ModuleSpec) => ModuleSpec;

export type ModuleToolContext = {
  moduleId: string;
  requestId: string;
};

export type ModuleToolHandler<TInput = unknown, TOutput = unknown> = (
  input: TInput,
  context: ModuleToolContext,
) => Promise<TOutput> | TOutput;

export type ModuleToolDecorator<TInput = unknown, TOutput = unknown> = (
  next: ModuleToolHandler<TInput, TOutput>,
) => ModuleToolHandler<TInput, TOutput>;

export type ModuleToolDefinition<TInput = unknown, TOutput = unknown> = {
  name: string;
  description?: string;
  handler: ModuleToolHandler<TInput, TOutput>;
};

export type ModuleDefinition = {
  spec: ModuleSpec;
  description?: string;
  tools: ModuleToolDefinition[];
};

export type ConsoleRoutes = {
  modules: string;
  experience: string;
};

export function buildConsoleRoute(
  path: "/console/modules" | "/console/experience",
  authToken?: string,
): string {
  return appendAuthToken(path, authToken);
}

export function buildConsoleModulesRoute(authToken?: string): string {
  return buildConsoleRoute("/console/modules", authToken);
}

export function buildConsoleExperienceRoute(authToken?: string): string {
  return buildConsoleRoute("/console/experience", authToken);
}

export function buildConsoleRoutes(authToken?: string): ConsoleRoutes {
  return {
    modules: buildConsoleModulesRoute(authToken),
    experience: buildConsoleExperienceRoute(authToken),
  };
}

export function defineModuleSpec(input: {
  id: string;
  command: string;
  args?: string[];
  restartPolicy?: "never" | "always" | "on_failure";
}): ModuleSpec {
  return {
    id: input.id,
    command: input.command,
    args: input.args ?? [],
    restart_policy: input.restartPolicy ?? "never",
  };
}

export function decorateModuleSpec(
  spec: ModuleSpec,
  ...decorators: ModuleSpecDecorator[]
): ModuleSpec {
  const base: ModuleSpec = { ...spec, args: [...spec.args] };
  return decorators.reduce((current, decorate) => decorate(current), base);
}

export function decorateModuleTool<TInput = unknown, TOutput = unknown>(
  handler: ModuleToolHandler<TInput, TOutput>,
  ...decorators: ModuleToolDecorator<TInput, TOutput>[]
): ModuleToolHandler<TInput, TOutput> {
  return decorators.reduceRight((next, decorate) => decorate(next), handler);
}

export function defineModuleTool<TInput = unknown, TOutput = unknown>(input: {
  name: string;
  handler: ModuleToolHandler<TInput, TOutput>;
  description?: string;
  decorators?: ModuleToolDecorator<TInput, TOutput>[];
}): ModuleToolDefinition<TInput, TOutput> {
  return {
    name: input.name,
    description: input.description,
    handler: decorateModuleTool(input.handler, ...(input.decorators ?? [])),
  };
}

export function defineModule(input: {
  spec: ModuleSpec;
  description?: string;
  tools?: ModuleToolDefinition[];
}): ModuleDefinition {
  return {
    spec: { ...input.spec, args: [...input.spec.args] },
    description: input.description,
    tools: [...(input.tools ?? [])],
  };
}

export function createGatewaySyncTransport(gatewayBin: string): JsonRpcSyncTransport {
  return (request: JsonRpcRequest): unknown => {
    const cp = require("node:child_process");
    const requestJson = JSON.stringify(request);
    const out = cp.spawnSync(gatewayBin, {
      env: { ...process.env, MOBKIT_RPC_REQUEST: requestJson },
      encoding: "utf8",
    });

    if (out.status !== 0) {
      throw new Error(`gateway failed (status=${out.status}): ${String(out.stderr ?? "")}`);
    }

    try {
      return JSON.parse(String(out.stdout ?? "")) as unknown;
    } catch (_err) {
      throw new Error("gateway returned non-JSON response");
    }
  };
}

export function createGatewayAsyncTransport(gatewayBin: string): JsonRpcTransport {
  return async (request: JsonRpcRequest): Promise<unknown> =>
    new Promise<unknown>((resolve, reject) => {
      const cp = require("node:child_process");
      const requestJson = JSON.stringify(request);
      const child = cp.spawn(gatewayBin, [], {
        env: { ...process.env, MOBKIT_RPC_REQUEST: requestJson },
        stdio: ["ignore", "pipe", "pipe"],
      });

      let stdout = "";
      let stderr = "";

      if (child.stdout) {
        child.stdout.setEncoding("utf8");
        child.stdout.on("data", (chunk: string) => {
          stdout += chunk;
        });
      }
      if (child.stderr) {
        child.stderr.setEncoding("utf8");
        child.stderr.on("data", (chunk: string) => {
          stderr += chunk;
        });
      }

      child.on("error", (error: Error) => {
        reject(error);
      });

      child.on("close", (code: number | null) => {
        if (code !== 0) {
          reject(new Error(`gateway failed (status=${code}): ${stderr}`));
          return;
        }
        try {
          resolve(JSON.parse(stdout) as unknown);
        } catch (_err) {
          reject(new Error("gateway returned non-JSON response"));
        }
      });
    });
}

export function createJsonRpcHttpTransport(
  endpoint: string,
  options: {
    headers?: Record<string, string>;
    fetchImpl?: FetchLike;
  } = {},
): JsonRpcTransport {
  const globalFetch = (globalThis as unknown as { fetch?: FetchLike }).fetch;
  const fetchImpl = options.fetchImpl ?? globalFetch;
  if (!fetchImpl) {
    throw new Error("fetch implementation not available");
  }

  return async (request: JsonRpcRequest): Promise<unknown> => {
    const response = await fetchImpl(endpoint, {
      method: "POST",
      headers: {
        "content-type": "application/json",
        accept: "application/json",
        ...(options.headers ?? {}),
      },
      body: JSON.stringify(request),
    });

    const body = await response.text();
    if (!response.ok) {
      throw new Error(`http transport failed (status=${response.status}): ${body}`);
    }

    try {
      return JSON.parse(body) as unknown;
    } catch (_err) {
      throw new Error("http transport returned non-JSON response");
    }
  };
}

export class MobkitTypedClient {
  private readonly syncTransport: JsonRpcSyncTransport;

  constructor(private readonly gatewayBin: string) {
    this.syncTransport = createGatewaySyncTransport(gatewayBin);
  }

  rpc(id: string, method: string, params: Record<string, unknown>): JsonRpcResponse {
    const payload = this.syncTransport(buildJsonRpcRequest(id, method, params));
    return parseJsonRpcResponse(payload, id);
  }

  status(requestId = "status"): MobkitStatusResult {
    return unwrapTypedResult(
      this.rpc(requestId, "mobkit/status", {}),
      requestId,
      "mobkit/status",
      isMobkitStatusResult,
    );
  }

  capabilities(requestId = "capabilities"): MobkitCapabilitiesResult {
    return unwrapTypedResult(
      this.rpc(requestId, "mobkit/capabilities", {}),
      requestId,
      "mobkit/capabilities",
      isMobkitCapabilitiesResult,
    );
  }

  reconcile(modules: string[], requestId = "reconcile"): MobkitReconcileResult {
    return unwrapTypedResult(
      this.rpc(requestId, "mobkit/reconcile", { modules }),
      requestId,
      "mobkit/reconcile",
      isMobkitReconcileResult,
    );
  }

  spawnMember(moduleId: string, requestId = "spawn_member"): MobkitSpawnMemberResult {
    return unwrapTypedResult(
      this.rpc(requestId, "mobkit/spawn_member", { module_id: moduleId }),
      requestId,
      "mobkit/spawn_member",
      isMobkitSpawnMemberResult,
    );
  }

  subscribeEvents(
    params: MobkitSubscribeParams = {},
    requestId = "events_subscribe",
  ): MobkitSubscribeResult {
    return unwrapTypedResult(
      this.rpc(requestId, "mobkit/events/subscribe", buildSubscribeParams(params)),
      requestId,
      "mobkit/events/subscribe",
      isMobkitSubscribeResult,
    );
  }
}

export class MobkitAsyncClient {
  constructor(private readonly transport: JsonRpcTransport) {}

  static fromGatewayBin(gatewayBin: string): MobkitAsyncClient {
    return new MobkitAsyncClient(createGatewayAsyncTransport(gatewayBin));
  }

  static fromHttp(
    endpoint: string,
    options: {
      headers?: Record<string, string>;
      fetchImpl?: FetchLike;
    } = {},
  ): MobkitAsyncClient {
    return new MobkitAsyncClient(createJsonRpcHttpTransport(endpoint, options));
  }

  async rpc(id: string, method: string, params: Record<string, unknown>): Promise<JsonRpcResponse> {
    const payload = await this.transport(buildJsonRpcRequest(id, method, params));
    return parseJsonRpcResponse(payload, id);
  }

  async status(requestId = "status"): Promise<MobkitStatusResult> {
    return this.request(
      requestId,
      "mobkit/status",
      {},
      isMobkitStatusResult,
    );
  }

  async capabilities(requestId = "capabilities"): Promise<MobkitCapabilitiesResult> {
    return this.request(
      requestId,
      "mobkit/capabilities",
      {},
      isMobkitCapabilitiesResult,
    );
  }

  async reconcile(modules: string[], requestId = "reconcile"): Promise<MobkitReconcileResult> {
    return this.request(
      requestId,
      "mobkit/reconcile",
      { modules },
      isMobkitReconcileResult,
    );
  }

  async spawnMember(moduleId: string, requestId = "spawn_member"): Promise<MobkitSpawnMemberResult> {
    return this.request(
      requestId,
      "mobkit/spawn_member",
      { module_id: moduleId },
      isMobkitSpawnMemberResult,
    );
  }

  async subscribeEvents(
    params: MobkitSubscribeParams = {},
    requestId = "events_subscribe",
  ): Promise<MobkitSubscribeResult> {
    return this.request(
      requestId,
      "mobkit/events/subscribe",
      buildSubscribeParams(params),
      isMobkitSubscribeResult,
    );
  }

  async request<TResult>(
    id: string,
    method: string,
    params: Record<string, unknown>,
    isExpected: (value: unknown) => value is TResult,
  ): Promise<TResult> {
    const response = await this.rpc(id, method, params);
    return unwrapTypedResult(response, id, method, isExpected);
  }
}

function buildJsonRpcRequest(
  id: string,
  method: string,
  params: Record<string, unknown>,
): JsonRpcRequest {
  return {
    jsonrpc: "2.0",
    id,
    method,
    params,
  };
}

function buildSubscribeParams(params: MobkitSubscribeParams): Record<string, unknown> {
  const next: Record<string, unknown> = {};
  if (params.scope !== undefined) {
    next.scope = params.scope;
  }
  if (params.last_event_id !== undefined) {
    next.last_event_id = params.last_event_id;
  }
  if (params.agent_id !== undefined) {
    next.agent_id = params.agent_id;
  }
  return next;
}

function parseJsonRpcResponse(payload: unknown, expectedId: string): JsonRpcResponse {
  const envelope = asObject(payload);
  if (envelope.jsonrpc !== "2.0" || envelope.id !== expectedId) {
    throw new Error("invalid JSON-RPC response envelope");
  }

  const hasResult = Object.prototype.hasOwnProperty.call(envelope, "result");
  const hasError = Object.prototype.hasOwnProperty.call(envelope, "error");
  if (hasResult === hasError) {
    throw new Error("invalid JSON-RPC response envelope");
  }

  if (hasError) {
    const rpcError = asObject(envelope.error);
    if (!Number.isInteger(rpcError.code) || typeof rpcError.message !== "string") {
      throw new Error("invalid JSON-RPC response envelope");
    }
  }

  return envelope as JsonRpcResponse;
}

function unwrapTypedResult<TResult>(
  response: JsonRpcResponse,
  requestId: string,
  method: string,
  isExpected: (value: unknown) => value is TResult,
): TResult {
  if (isJsonRpcError(response)) {
    throw new MobkitRpcError(response.error.code, response.error.message, requestId, method);
  }
  if (!isExpected(response.result)) {
    throw new Error(`invalid result payload for ${method}`);
  }
  return response.result;
}

function isJsonRpcError(response: JsonRpcResponse): response is JsonRpcError {
  return Object.prototype.hasOwnProperty.call(response, "error");
}

function appendAuthToken(path: string, authToken?: string): string {
  if (!authToken) {
    return path;
  }
  const joiner = path.includes("?") ? "&" : "?";
  return `${path}${joiner}auth_token=${encodeURIComponent(authToken)}`;
}

function asObject(value: unknown): Record<string, unknown> {
  if (typeof value !== "object" || value === null) {
    throw new Error("invalid JSON-RPC response envelope");
  }
  return value as Record<string, unknown>;
}

function isStringArray(value: unknown): value is string[] {
  return Array.isArray(value) && value.every((item) => typeof item === "string");
}

function isMobkitStatusResult(value: unknown): value is MobkitStatusResult {
  const object = asValueObject(value);
  return (
    typeof object.contract_version === "string" &&
    typeof object.running === "boolean" &&
    isStringArray(object.loaded_modules)
  );
}

function isMobkitCapabilitiesResult(value: unknown): value is MobkitCapabilitiesResult {
  const object = asValueObject(value);
  return (
    typeof object.contract_version === "string" &&
    isStringArray(object.methods) &&
    isStringArray(object.loaded_modules)
  );
}

function isMobkitReconcileResult(value: unknown): value is MobkitReconcileResult {
  const object = asValueObject(value);
  return (
    typeof object.accepted === "boolean" &&
    isStringArray(object.reconciled_modules) &&
    Number.isInteger(object.added)
  );
}

function isMobkitSpawnMemberResult(value: unknown): value is MobkitSpawnMemberResult {
  const object = asValueObject(value);
  return (
    typeof object.accepted === "boolean" &&
    typeof object.module_id === "string"
  );
}

function isMobkitSubscribeResult(value: unknown): value is MobkitSubscribeResult {
  const object = asValueObject(value);
  const scope = object.scope;
  if (scope !== "mob" && scope !== "agent" && scope !== "interaction") {
    return false;
  }

  if (!(object.replay_from_event_id === null || typeof object.replay_from_event_id === "string")) {
    return false;
  }

  const keepAlive = asValueObject(object.keep_alive);
  if (!Number.isInteger(keepAlive.interval_ms) || typeof keepAlive.event !== "string") {
    return false;
  }

  if (typeof object.keep_alive_comment !== "string") {
    return false;
  }

  if (!isStringArray(object.event_frames)) {
    return false;
  }

  if (!Array.isArray(object.events)) {
    return false;
  }

  return object.events.every((event) => {
    const eventObject = asValueObject(event);
    return (
      typeof eventObject.event_id === "string" &&
      typeof eventObject.source === "string" &&
      Number.isInteger(eventObject.timestamp_ms) &&
      Object.prototype.hasOwnProperty.call(eventObject, "event")
    );
  });
}

function asValueObject(value: unknown): Record<string, unknown> {
  if (typeof value !== "object" || value === null) {
    return {};
  }
  return value as Record<string, unknown>;
}
