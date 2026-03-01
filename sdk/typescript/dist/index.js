"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.MobkitTypedClient = void 0;
exports.buildConsoleModulesRoute = buildConsoleModulesRoute;
exports.defineModuleSpec = defineModuleSpec;
function buildConsoleModulesRoute(authToken) {
    if (!authToken) {
        return "/console/modules";
    }
    return `/console/modules?auth_token=${encodeURIComponent(authToken)}`;
}
function defineModuleSpec(input) {
    return {
        id: input.id,
        command: input.command,
        args: input.args ?? [],
        restart_policy: input.restartPolicy ?? "never",
    };
}
class MobkitTypedClient {
    constructor(gatewayBin) {
        this.gatewayBin = gatewayBin;
    }
    rpc(id, method, params) {
        const cp = require("node:child_process");
        const request = JSON.stringify({ jsonrpc: "2.0", id, method, params });
        const out = cp.spawnSync(this.gatewayBin, {
            env: { ...process.env, MOBKIT_RPC_REQUEST: request },
            encoding: "utf8",
        });
        if (out.status !== 0) {
            throw new Error(`gateway failed (status=${out.status}): ${out.stderr}`);
        }
        const payload = JSON.parse(out.stdout);
        if (typeof payload !== "object" || payload === null) {
            throw new Error("invalid JSON-RPC response envelope");
        }
        const envelope = payload;
        if (envelope.jsonrpc !== "2.0" || envelope.id !== id) {
            throw new Error("invalid JSON-RPC response envelope");
        }
        const hasResult = Object.prototype.hasOwnProperty.call(envelope, "result");
        const hasError = Object.prototype.hasOwnProperty.call(envelope, "error");
        if (hasResult === hasError) {
            throw new Error("invalid JSON-RPC response envelope");
        }
        if (hasError) {
            const rpcError = envelope.error;
            if (typeof rpcError !== "object" || rpcError === null) {
                throw new Error("invalid JSON-RPC response envelope");
            }
            const typedError = rpcError;
            if (!Number.isInteger(typedError.code) || typeof typedError.message !== "string") {
                throw new Error("invalid JSON-RPC response envelope");
            }
        }
        return envelope;
    }
}
exports.MobkitTypedClient = MobkitTypedClient;
