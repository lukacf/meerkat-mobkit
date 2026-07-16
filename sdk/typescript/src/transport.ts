/**
 * Persistent subprocess transport for MobKit JSON-RPC.
 *
 * Keeps a long-lived gateway binary alive, communicating over stdin/stdout
 * newline-delimited JSON. Supports bidirectional callbacks from Rust.
 */

import { spawn, spawnSync } from "node:child_process";
import { randomUUID } from "node:crypto";
import { performance } from "node:perf_hooks";
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
  init: {
    method: "POST";
    headers: Record<string, string>;
    body: string;
    /** Optional AbortSignal — surfaced so the http transport can cancel
     * a hung server request after `timeoutMs`. Implementations that
     * don't support it can ignore the field. */
    signal?: AbortSignal;
  },
) => Promise<FetchLikeResponse>;

/**
 * Grace period for the persistent gateway to finish its own shutdown.
 *
 * Provider operations are publicly required to finish within 120 seconds;
 * the stock Rust gateway gives each callback a hard 130-second wire deadline.
 * Its advertised 335-second horizon covers two such callback windows,
 * runtime event/mob drains, bounded RPC/HTTP/stdout phases, and
 * response-delivery/process-reap margin. The same value is the safe fallback
 * for handshake-capable gateways without the newer explicit capability.
 */
export const PERSISTENT_TRANSPORT_SHUTDOWN_GRACE_MS = 335_000;

const PERSISTENT_TRANSPORT_SIGTERM_GRACE_MS = 5_000;
const PERSISTENT_TRANSPORT_SIGKILL_GRACE_MS = 5_000;
const MAX_GATEWAY_SHUTDOWN_HORIZON_MS = 2_147_483_647;
const GATEWAY_SHUTDOWN_METHOD = "mobkit/shutdown";

type ChildExitWaiter = (
  child: ChildProcess,
  timeoutMs: number,
) => Promise<boolean>;

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

function childHasExited(child: ChildProcess): boolean {
  return child.exitCode !== null || child.signalCode !== null;
}

function validateGatewayShutdownResponse(response: unknown): void {
  if (typeof response !== "object" || response === null) {
    throw new Error("gateway shutdown returned a malformed response");
  }
  const envelope = response as Record<string, unknown>;
  if (typeof envelope.error === "object" && envelope.error !== null) {
    const message = String(
      (envelope.error as Record<string, unknown>).message ?? "unknown gateway error",
    );
    throw new Error(`gateway shutdown failed: ${message}`);
  }
  const result = envelope.result;
  if (
    typeof result !== "object" ||
    result === null ||
    (result as Record<string, unknown>).shutdown !== true ||
    (result as Record<string, unknown>).runtime_cleanup_completed !== true
  ) {
    throw new Error("gateway shutdown did not complete runtime-owned cleanup");
  }
}

async function waitForChildExit(
  child: ChildProcess,
  timeoutMs: number,
): Promise<boolean> {
  if (childHasExited(child)) return true;

  return new Promise<boolean>((resolve) => {
    let settled = false;
    let timer: NodeJS.Timeout | null = null;

    const finish = (exited: boolean): void => {
      if (settled) return;
      settled = true;
      child.removeListener("exit", onExit);
      child.removeListener("close", onExit);
      if (timer !== null) clearTimeout(timer);
      resolve(exited);
    };
    const onExit = (): void => finish(true);

    child.once("exit", onExit);
    child.once("close", onExit);

    // Close the check/listener race: the process may have exited between the
    // initial fast path and listener registration.
    if (childHasExited(child)) {
      finish(true);
      return;
    }

    timer = setTimeout(() => finish(childHasExited(child)), timeoutMs);
    timer.unref?.();
    if (settled) clearTimeout(timer);
  });
}

/** @internal Exported for deterministic lifecycle policy tests. */
export async function stopChildProcess(
  child: ChildProcess,
  waitForExit: ChildExitWaiter = waitForChildExit,
  shutdownGraceMs = PERSISTENT_TRANSPORT_SHUTDOWN_GRACE_MS,
): Promise<void> {
  try {
    child.stdin?.end();
  } catch {
    // Continue with signal-based cleanup if stdin is already unavailable.
  }

  if (
    await waitForExit(child, shutdownGraceMs)
  ) {
    return;
  }

  try {
    child.kill("SIGTERM");
  } catch {
    // A concurrent exit is observed by the bounded wait below.
  }

  if (await waitForExit(child, PERSISTENT_TRANSPORT_SIGTERM_GRACE_MS)) {
    return;
  }

  try {
    child.kill("SIGKILL");
  } catch {
    // Best effort: never make SDK shutdown unbounded on a failed kill call.
  }

  if (!(await waitForExit(child, PERSISTENT_TRANSPORT_SIGKILL_GRACE_MS))) {
    throw new Error(
      "persistent transport: gateway process did not terminate after bounded cleanup",
    );
  }
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
  private _stopping: Promise<void> | null = null;
  private readonly _env: Record<string, string>;
  private readonly _timeout: number;
  private _callbackHandler: CallbackHandler | null = null;
  private _supportsShutdownHandshake = false;
  private _shutdownHorizonMs = PERSISTENT_TRANSPORT_SHUTDOWN_GRACE_MS;
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
    if (this._stopping !== null) {
      throw new Error("persistent transport is stopping");
    }
    if (this._process !== null && this._process.exitCode === null) {
      return;
    }

    // Capabilities are process-scoped and must be renegotiated on init after
    // a child restart.
    this._supportsShutdownHandshake = false;
    this._shutdownHorizonMs = PERSISTENT_TRANSPORT_SHUTDOWN_GRACE_MS;
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
    const response = await this._sendAsyncWithTimeout(request, this._timeout);
    if (request.method === "mobkit/init") {
      const result =
        typeof response === "object" && response !== null
          ? (response as Record<string, unknown>).result
          : null;
      this._supportsShutdownHandshake =
        typeof result === "object" &&
        result !== null &&
        (result as Record<string, unknown>).stdio_shutdown_handshake === true;
      this._shutdownHorizonMs = PERSISTENT_TRANSPORT_SHUTDOWN_GRACE_MS;
      if (this._supportsShutdownHandshake) {
        const horizonMs = (result as Record<string, unknown>).stdio_shutdown_horizon_ms;
        if (
          typeof horizonMs === "number" &&
          Number.isSafeInteger(horizonMs) &&
          horizonMs > 0 &&
          horizonMs <= MAX_GATEWAY_SHUTDOWN_HORIZON_MS
        ) {
          this._shutdownHorizonMs = horizonMs;
        }
      }
    }
    return response;
  }

  private async _sendAsyncWithTimeout(
    request: Record<string, unknown>,
    timeoutMs: number,
    expectedChild?: ChildProcess,
  ): Promise<unknown> {
    if (expectedChild === undefined) {
      this._ensureRunning();
    } else if (this._process !== expectedChild || childHasExited(expectedChild)) {
      throw new Error("persistent transport: subprocess is not running");
    }
    const msgId = String(request.id ?? "");

    return new Promise<unknown>((resolve, reject) => {
      const timer = setTimeout(() => {
        this._pending.delete(msgId);
        reject(new Error(`persistent transport: timeout after ${timeoutMs}ms`));
      }, timeoutMs);

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

      try {
        this._writeLine(request as Record<string, unknown>);
      } catch (error) {
        clearTimeout(timer);
        this._pending.delete(msgId);
        reject(error instanceof Error ? error : new Error(String(error)));
      }
    });
  }

  async stop(): Promise<void> {
    if (this._stopping !== null) return this._stopping;

    const child = this._process;
    if (child === null) return;

    const shutdownStarted = performance.now();
    let childTerminated = false;
    let stopping: Promise<void>;
    stopping = (async () => {
      let shutdownError: Error | null = null;
      try {
        // Keep stdin open while the gateway shuts its runtime down: external
        // lease/continuity providers may still need callback round-trips.
        // Capability negotiation keeps older/custom gateways on their EOF
        // protocol instead of assuming method-not-found behavior.
        if (this._supportsShutdownHandshake) {
          const response = await this._sendAsyncWithTimeout(
            {
              jsonrpc: "2.0",
              id: `mobkit-shutdown-${randomUUID()}`,
              method: GATEWAY_SHUTDOWN_METHOD,
              params: {},
            },
            this._shutdownHorizonMs,
            child,
          );
          validateGatewayShutdownResponse(response);
        }
      } catch (error) {
        shutdownError = error instanceof Error ? error : new Error(String(error));
      }
      const elapsedMs = performance.now() - shutdownStarted;
      const remainingGraceMs = Math.max(
        0,
        this._shutdownHorizonMs - elapsedMs,
      );
      await stopChildProcess(child, waitForChildExit, remainingGraceMs);
      childTerminated = true;
      if (shutdownError !== null) {
        throw new Error(
          `persistent transport: gateway shutdown failed after bounded cleanup: ${shutdownError.message}`,
        );
      }
    })().finally(() => {
      if (
        this._process === child &&
        (childTerminated || childHasExited(child))
      ) {
        this._process = null;
      }
      if (this._stopping === stopping) this._stopping = null;
    });
    this._stopping = stopping;
    return stopping;
  }

  isRunning(): boolean {
    return this._process !== null && this._process.exitCode === null;
  }

  private _ensureRunning(): void {
    if (this._stopping !== null) {
      throw new Error("persistent transport is stopping");
    }
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
 * Default request timeout for {@link createJsonRpcHttpTransport}. A
 * server that accepts the connection but never replies would otherwise
 * leak the fetch task forever; pre-fix there was no timeout at all.
 */
export const DEFAULT_HTTP_TRANSPORT_TIMEOUT_MS = 60_000;

/**
 * Create an async HTTP POST transport.
 */
export function createJsonRpcHttpTransport(
  endpoint: string,
  options: {
    headers?: Record<string, string>;
    fetchImpl?: FetchLike;
    /** Maximum time (ms) a single request may stay pending before being
     * aborted. Defaults to 60s. Set to 0 to disable (not recommended). */
    timeoutMs?: number;
  } = {},
): JsonRpcTransport {
  const globalFetch = (globalThis as unknown as { fetch?: FetchLike }).fetch;
  const fetchImpl = options.fetchImpl ?? globalFetch;
  if (!fetchImpl) {
    throw new Error("fetch implementation not available");
  }
  const timeoutMs =
    options.timeoutMs === undefined
      ? DEFAULT_HTTP_TRANSPORT_TIMEOUT_MS
      : options.timeoutMs;

  return async (request: JsonRpcRequest): Promise<unknown> => {
    const controller =
      timeoutMs > 0 ? new AbortController() : undefined;
    const timer =
      controller !== undefined && timeoutMs > 0
        ? setTimeout(() => controller.abort(), timeoutMs)
        : undefined;

    try {
      const response = await fetchImpl(endpoint, {
        method: "POST",
        headers: {
          "content-type": "application/json",
          accept: "application/json",
          ...(options.headers ?? {}),
        },
        body: JSON.stringify(request),
        signal: controller?.signal,
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
    } catch (err) {
      if (
        err !== null &&
        typeof err === "object" &&
        "name" in err &&
        (err as { name: unknown }).name === "AbortError"
      ) {
        throw new Error(
          `http transport timed out after ${timeoutMs}ms; server unresponsive`,
        );
      }
      throw err;
    } finally {
      if (timer !== undefined) {
        clearTimeout(timer);
      }
    }
  };
}
