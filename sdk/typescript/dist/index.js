"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.MobkitAsyncClient = exports.MobkitTypedClient = exports.MobkitRpcError = void 0;
exports.buildConsoleRoute = buildConsoleRoute;
exports.buildConsoleModulesRoute = buildConsoleModulesRoute;
exports.buildConsoleExperienceRoute = buildConsoleExperienceRoute;
exports.buildConsoleRoutes = buildConsoleRoutes;
exports.defineModuleSpec = defineModuleSpec;
exports.decorateModuleSpec = decorateModuleSpec;
exports.decorateModuleTool = decorateModuleTool;
exports.defineModuleTool = defineModuleTool;
exports.defineModule = defineModule;
exports.createGatewaySyncTransport = createGatewaySyncTransport;
exports.createGatewayAsyncTransport = createGatewayAsyncTransport;
exports.createJsonRpcHttpTransport = createJsonRpcHttpTransport;
class MobkitRpcError extends Error {
    constructor(code, message, requestId, method) {
        super(message);
        this.code = code;
        this.requestId = requestId;
        this.method = method;
        this.name = "MobkitRpcError";
    }
}
exports.MobkitRpcError = MobkitRpcError;
function buildConsoleRoute(path, authToken) {
    return appendAuthToken(path, authToken);
}
function buildConsoleModulesRoute(authToken) {
    return buildConsoleRoute("/console/modules", authToken);
}
function buildConsoleExperienceRoute(authToken) {
    return buildConsoleRoute("/console/experience", authToken);
}
function buildConsoleRoutes(authToken) {
    return {
        modules: buildConsoleModulesRoute(authToken),
        experience: buildConsoleExperienceRoute(authToken),
    };
}
function defineModuleSpec(input) {
    return {
        id: input.id,
        command: input.command,
        args: input.args ?? [],
        restart_policy: input.restartPolicy ?? "never",
    };
}
function decorateModuleSpec(spec, ...decorators) {
    const base = { ...spec, args: [...spec.args] };
    return decorators.reduce((current, decorate) => decorate(current), base);
}
function decorateModuleTool(handler, ...decorators) {
    return decorators.reduceRight((next, decorate) => decorate(next), handler);
}
function defineModuleTool(input) {
    return {
        name: input.name,
        description: input.description,
        handler: decorateModuleTool(input.handler, ...(input.decorators ?? [])),
    };
}
function defineModule(input) {
    return {
        spec: { ...input.spec, args: [...input.spec.args] },
        description: input.description,
        tools: [...(input.tools ?? [])],
    };
}
function createGatewaySyncTransport(gatewayBin) {
    return (request) => {
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
            return JSON.parse(String(out.stdout ?? ""));
        }
        catch (_err) {
            throw new Error("gateway returned non-JSON response");
        }
    };
}
function createGatewayAsyncTransport(gatewayBin) {
    return async (request) => new Promise((resolve, reject) => {
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
            child.stdout.on("data", (chunk) => {
                stdout += chunk;
            });
        }
        if (child.stderr) {
            child.stderr.setEncoding("utf8");
            child.stderr.on("data", (chunk) => {
                stderr += chunk;
            });
        }
        child.on("error", (error) => {
            reject(error);
        });
        child.on("close", (code) => {
            if (code !== 0) {
                reject(new Error(`gateway failed (status=${code}): ${stderr}`));
                return;
            }
            try {
                resolve(JSON.parse(stdout));
            }
            catch (_err) {
                reject(new Error("gateway returned non-JSON response"));
            }
        });
    });
}
function createJsonRpcHttpTransport(endpoint, options = {}) {
    const globalFetch = globalThis.fetch;
    const fetchImpl = options.fetchImpl ?? globalFetch;
    if (!fetchImpl) {
        throw new Error("fetch implementation not available");
    }
    return async (request) => {
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
            return JSON.parse(body);
        }
        catch (_err) {
            throw new Error("http transport returned non-JSON response");
        }
    };
}
class MobkitTypedClient {
    constructor(gatewayBin) {
        this.gatewayBin = gatewayBin;
        this.syncTransport = createGatewaySyncTransport(gatewayBin);
    }
    rpc(id, method, params) {
        const payload = this.syncTransport(buildJsonRpcRequest(id, method, params));
        return parseJsonRpcResponse(payload, id);
    }
    status(requestId = "status") {
        return unwrapTypedResult(this.rpc(requestId, "mobkit/status", {}), requestId, "mobkit/status", isMobkitStatusResult);
    }
    capabilities(requestId = "capabilities") {
        return unwrapTypedResult(this.rpc(requestId, "mobkit/capabilities", {}), requestId, "mobkit/capabilities", isMobkitCapabilitiesResult);
    }
    reconcile(modules, requestId = "reconcile") {
        return unwrapTypedResult(this.rpc(requestId, "mobkit/reconcile", { modules }), requestId, "mobkit/reconcile", isMobkitReconcileResult);
    }
    spawnMember(moduleId, requestId = "spawn_member") {
        return unwrapTypedResult(this.rpc(requestId, "mobkit/spawn_member", { module_id: moduleId }), requestId, "mobkit/spawn_member", isMobkitSpawnMemberResult);
    }
    subscribeEvents(params = {}, requestId = "events_subscribe") {
        return unwrapTypedResult(this.rpc(requestId, "mobkit/events/subscribe", buildSubscribeParams(params)), requestId, "mobkit/events/subscribe", isMobkitSubscribeResult);
    }
}
exports.MobkitTypedClient = MobkitTypedClient;
class MobkitAsyncClient {
    constructor(transport) {
        this.transport = transport;
    }
    static fromGatewayBin(gatewayBin) {
        return new MobkitAsyncClient(createGatewayAsyncTransport(gatewayBin));
    }
    static fromHttp(endpoint, options = {}) {
        return new MobkitAsyncClient(createJsonRpcHttpTransport(endpoint, options));
    }
    async rpc(id, method, params) {
        const payload = await this.transport(buildJsonRpcRequest(id, method, params));
        return parseJsonRpcResponse(payload, id);
    }
    async status(requestId = "status") {
        return this.request(requestId, "mobkit/status", {}, isMobkitStatusResult);
    }
    async capabilities(requestId = "capabilities") {
        return this.request(requestId, "mobkit/capabilities", {}, isMobkitCapabilitiesResult);
    }
    async reconcile(modules, requestId = "reconcile") {
        return this.request(requestId, "mobkit/reconcile", { modules }, isMobkitReconcileResult);
    }
    async spawnMember(moduleId, requestId = "spawn_member") {
        return this.request(requestId, "mobkit/spawn_member", { module_id: moduleId }, isMobkitSpawnMemberResult);
    }
    async subscribeEvents(params = {}, requestId = "events_subscribe") {
        return this.request(requestId, "mobkit/events/subscribe", buildSubscribeParams(params), isMobkitSubscribeResult);
    }
    async request(id, method, params, isExpected) {
        const response = await this.rpc(id, method, params);
        return unwrapTypedResult(response, id, method, isExpected);
    }
}
exports.MobkitAsyncClient = MobkitAsyncClient;
function buildJsonRpcRequest(id, method, params) {
    return {
        jsonrpc: "2.0",
        id,
        method,
        params,
    };
}
function buildSubscribeParams(params) {
    const next = {};
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
function parseJsonRpcResponse(payload, expectedId) {
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
    return envelope;
}
function unwrapTypedResult(response, requestId, method, isExpected) {
    if (isJsonRpcError(response)) {
        throw new MobkitRpcError(response.error.code, response.error.message, requestId, method);
    }
    if (!isExpected(response.result)) {
        throw new Error(`invalid result payload for ${method}`);
    }
    return response.result;
}
function isJsonRpcError(response) {
    return Object.prototype.hasOwnProperty.call(response, "error");
}
function appendAuthToken(path, authToken) {
    if (!authToken) {
        return path;
    }
    const joiner = path.includes("?") ? "&" : "?";
    return `${path}${joiner}auth_token=${encodeURIComponent(authToken)}`;
}
function asObject(value) {
    if (typeof value !== "object" || value === null) {
        throw new Error("invalid JSON-RPC response envelope");
    }
    return value;
}
function isStringArray(value) {
    return Array.isArray(value) && value.every((item) => typeof item === "string");
}
function isMobkitStatusResult(value) {
    const object = asValueObject(value);
    return (typeof object.contract_version === "string" &&
        typeof object.running === "boolean" &&
        isStringArray(object.loaded_modules));
}
function isMobkitCapabilitiesResult(value) {
    const object = asValueObject(value);
    return (typeof object.contract_version === "string" &&
        isStringArray(object.methods) &&
        isStringArray(object.loaded_modules));
}
function isMobkitReconcileResult(value) {
    const object = asValueObject(value);
    return (typeof object.accepted === "boolean" &&
        isStringArray(object.reconciled_modules) &&
        Number.isInteger(object.added));
}
function isMobkitSpawnMemberResult(value) {
    const object = asValueObject(value);
    return (typeof object.accepted === "boolean" &&
        typeof object.module_id === "string");
}
function isMobkitSubscribeResult(value) {
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
        return (typeof eventObject.event_id === "string" &&
            typeof eventObject.source === "string" &&
            Number.isInteger(eventObject.timestamp_ms) &&
            Object.prototype.hasOwnProperty.call(eventObject, "event"));
    });
}
function asValueObject(value) {
    if (typeof value !== "object" || value === null) {
        return {};
    }
    return value;
}
