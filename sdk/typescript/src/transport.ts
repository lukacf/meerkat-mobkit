/**
 * Persistent subprocess transport for MobKit JSON-RPC.
 *
 * Keeps a long-lived gateway binary alive, communicating over stdin/stdout
 * newline-delimited JSON. Supports bidirectional callbacks from Rust.
 */

import { spawn, spawnSync } from "node:child_process";
import { createInterface } from "node:readline";
import type { ChildProcess } from "node:child_process";

// -- Types ----------------------------------------------------------------

export interface JsonRpcRequest {
  readonly jsonrpc: "2.0";
  readonly id: string;
  readonly method: string;
  readonly params: Record<string, unknown>;
}

export interface JsonRpcSuccess {
  readonly jsonrpc: "2.0";
  readonly id: string;
  readonly result: unknown;
}

export interface JsonRpcErrorBody {
  readonly code: number;
  readonly message: string;
}

export interface JsonRpcErrorResponse {
  readonly jsonrpc: "2.0";
  readonly id: string;
  readonly error: JsonRpcErrorBody;
}

export type JsonRpcResponse = JsonRpcSuccess | JsonRpcErrorResponse;

export type JsonRpcTransport = (
  request: JsonRpcRequest,
) => Promise<unknown>;

export type JsonRpcSyncTransport = (request: JsonRpcRequest) => unknown;

export type CallbackHandler = (
  method: string,
  params: Record<string, unknown>,
) => Promise<unknown>;

export type FetchLikeResponse = {
  ok: boolean;
  status: number;
  text(): Promise<string>;
};

export type FetchLike = (
  url: string,
  init: { method: "POST"; headers: Record<string, string>; body: string },
) => Promise<FetchLikeResponse>;

// -- Helpers --------------------------------------------------------------

export function buildJsonRpcRequest(
  id: string,
  method: string,
  params: Record<string, unknown>,
): JsonRpcRequest {
  return { jsonrpc: "2.0", id, method, params };
}

function sanitizeForJson(obj: unknown): unknown {
  if (obj === null || obj === undefined) return obj;
  if (typeof obj === "boolean" || typeof obj === "number" || typeof obj === "string") return obj;
  if (Array.isArray(obj)) return obj.map(sanitizeForJson);
  if (typeof obj === "object") {
    const result: Record<string, unknown> = {};
    for (const [k, v] of Object.entries(obj as Record<string, unknown>)) {
      result[k] = sanitizeForJson(v);
    }
    return result;
  }
  return String(obj);
}

// -- PersistentTransport --------------------------------------------------

/**
 * Long-lived gateway subprocess communicating over stdin/stdout JSON-RPC.
 *
 * Uses a readline reader to multiplex responses and callbacks. Unlike
 * per-call subprocess transports, this keeps the process alive so mob
 * state persists across calls.
 */
export class PersistentTransport {
  private _process: ChildProcess | null = null;
  private readonly _env: Record<string, string>;
  private readonly _timeout: number;
  private _callbackHandler: CallbackHandler | null = null;
  private readonly _pending = new Map<
    string,
    { resolve: (value: unknown) => void; reject: (error: Error) => void }
  >();

  constructor(
    readonly gatewayBin: string,
    options?: { env?: Record<string, string>; timeout?: number },
  ) {
    this._env = { ...process.env, ...(options?.env ?? {}) } as Record<string, string>;
    this._timeout = options?.timeout ?? 60_000;
  }

  setCallbackHandler(handler: CallbackHandler): void {
    this._callbackHandler = handler;
  }

  start(): void {
    if (this._process !== null && this._process.exitCode === null) {
      return;
    }

    this._process = spawn(this.gatewayBin, ["--persistent"], {
      env: this._env,
      stdio: ["pipe", "pipe", "ignore"],
    });

    const child = this._process;

    // Background reader on stdout
    if (child.stdout) {
      const rl = createInterface({ input: child.stdout });
      rl.on("line", (line: string) => {
        let msg: Record<string, unknown>;
        try {
          msg = JSON.parse(line) as Record<string, unknown>;
        } catch {
          return;
        }

        if ("method" in msg) {
          this._handleCallback(msg);
        } else if ("id" in msg) {
          const msgId = String(msg.id);
          const pending = this._pending.get(msgId);
          if (pending) {
            this._pending.delete(msgId);
            pending.resolve(msg);
          }
        }
      });

      rl.on("close", () => {
        // Process closed stdout — fail all pending requests
        for (const [id, pending] of this._pending) {
          this._pending.delete(id);
          pending.resolve({
            jsonrpc: "2.0",
            id,
            error: { code: -32099, message: "subprocess died" },
          });
        }
      });
    }

    child.on("error", () => {
      // Process spawn error — fail all pending
      for (const [id, pending] of this._pending) {
        this._pending.delete(id);
        pending.reject(new Error("gateway process failed to start"));
      }
    });
  }

  private _handleCallback(msg: Record<string, unknown>): void {
    if (!this._callbackHandler) return;

    const method = String(msg.method ?? "");
    const params = (
      typeof msg.params === "object" && msg.params !== null
        ? msg.params
        : {}
    ) as Record<string, unknown>;
    const callbackId = msg.id !== undefined ? String(msg.id) : null;

    this._callbackHandler(method, params)
      .then((result) => {
        if (callbackId === null) return; // Notification — no response
        this._writeLine({
          jsonrpc: "2.0",
          id: callbackId,
          result: sanitizeForJson(result),
        });
      })
      .catch((err: unknown) => {
        if (callbackId === null) return;
        this._writeLine({
          jsonrpc: "2.0",
          id: callbackId,
          error: { code: -32000, message: String(err instanceof Error ? err.message : err) },
        });
      });
  }

  private _writeLine(obj: Record<string, unknown>): void {
    if (this._process?.stdin?.writable) {
      this._process.stdin.write(JSON.stringify(obj) + "\n");
    }
  }

  async sendAsync(request: Record<string, unknown>): Promise<unknown> {
    this._ensureRunning();
    const msgId = String(request.id ?? "");

    return new Promise<unknown>((resolve, reject) => {
      const timer = setTimeout(() => {
        this._pending.delete(msgId);
        reject(new Error(`persistent transport: timeout after ${this._timeout}ms`));
      }, this._timeout);

      this._pending.set(msgId, {
        resolve: (value) => {
          clearTimeout(timer);
          resolve(value);
        },
        reject: (error) => {
          clearTimeout(timer);
          reject(error);
        },
      });

      this._writeLine(request as Record<string, unknown>);
    });
  }

  stop(): void {
    if (this._process === null) return;
    try {
      if (this._process.stdin) {
        this._process.stdin.end();
      }
      this._process.kill();
    } catch {
      // Ignore cleanup errors
    } finally {
      this._process = null;
    }
  }

  isRunning(): boolean {
    return this._process !== null && this._process.exitCode === null;
  }

  private _ensureRunning(): void {
    if (!this.isRunning()) {
      this.start();
    }
  }
}

// -- Per-call transport factories -----------------------------------------

/**
 * Create a synchronous transport that spawns the gateway binary per call.
 */
export function createGatewaySyncTransport(
  gatewayBin: string,
): JsonRpcSyncTransport {
  return (request: JsonRpcRequest): unknown => {
    const requestJson = JSON.stringify(request);
    const out = spawnSync(gatewayBin, [], {
      env: { ...process.env, MOBKIT_RPC_REQUEST: requestJson },
      encoding: "utf8",
    });

    if (out.status !== 0) {
      throw new Error(
        `gateway failed (status=${out.status}): ${String(out.stderr ?? "")}`,
      );
    }

    try {
      return JSON.parse(String(out.stdout ?? "")) as unknown;
    } catch {
      throw new Error("gateway returned non-JSON response");
    }
  };
}

/**
 * Create an async transport that spawns the gateway binary per call.
 */
export function createGatewayAsyncTransport(
  gatewayBin: string,
): JsonRpcTransport {
  return async (request: JsonRpcRequest): Promise<unknown> =>
    new Promise<unknown>((resolve, reject) => {
      const requestJson = JSON.stringify(request);
      const child = spawn(gatewayBin, [], {
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

      child.on("error", (error: Error) => reject(error));

      child.on("close", (code: number | null) => {
        if (code !== 0) {
          reject(new Error(`gateway failed (status=${code}): ${stderr}`));
          return;
        }
        try {
          resolve(JSON.parse(stdout) as unknown);
        } catch {
          reject(new Error("gateway returned non-JSON response"));
        }
      });
    });
}

/**
 * Create an async HTTP POST transport.
 */
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
      throw new Error(
        `http transport failed (status=${response.status}): ${body}`,
      );
    }

    try {
      return JSON.parse(body) as unknown;
    } catch {
      throw new Error("http transport returned non-JSON response");
    }
  };
}
