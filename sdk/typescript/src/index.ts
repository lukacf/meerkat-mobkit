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

export type ModuleSpec = {
  id: string;
  command: string;
  args: string[];
  restart_policy: "never" | "always" | "on_failure";
};

export function buildConsoleModulesRoute(authToken?: string): string {
  if (!authToken) {
    return "/console/modules";
  }
  return `/console/modules?auth_token=${encodeURIComponent(authToken)}`;
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

export class MobkitTypedClient {
  constructor(private readonly gatewayBin: string) {}

  rpc(id: string, method: string, params: Record<string, unknown>): JsonRpcResponse {
    const cp = require("node:child_process");
    const request = JSON.stringify({ jsonrpc: "2.0", id, method, params });
    const out = cp.spawnSync(this.gatewayBin, {
      env: { ...process.env, MOBKIT_RPC_REQUEST: request },
      encoding: "utf8",
    });

    if (out.status !== 0) {
      throw new Error(`gateway failed (status=${out.status}): ${out.stderr}`);
    }

    const payload: unknown = JSON.parse(out.stdout);
    if (typeof payload !== "object" || payload === null) {
      throw new Error("invalid JSON-RPC response envelope");
    }

    const envelope = payload as Record<string, unknown>;
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

      const typedError = rpcError as Record<string, unknown>;
      if (!Number.isInteger(typedError.code) || typeof typedError.message !== "string") {
        throw new Error("invalid JSON-RPC response envelope");
      }
    }

    return envelope as JsonRpcResponse;
  }
}
