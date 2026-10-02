/**
 * Persistent subprocess transport for MobKit JSON-RPC.
 *
 * Keeps a long-lived gateway binary alive, communicating over stdin/stdout
 * newline-delimited JSON. Supports bidirectional callbacks from Rust.
 */

import { spawn, spawnSync } from "node:child_process";
import { randomUUID } from "node:crypto";
import { closeSync, openSync } from "node:fs";
import { performance } from "node:perf_hooks";
import { createInterface } from "node:readline";
import type { ChildProcess } from "node:child_process";
import type { ProviderCallbackContext } from "./types.js";
import { TransportReaderFailedError } from "./errors.js";

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
  context: ProviderCallbackContext,
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
export const PERSISTENT_TRANSPORT_SHUTDOWN_GRACE_MS = 337_000;
export const PROVIDER_CALLBACK_COMPLETION_MS = 125_000;

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

/** mobkit/init: accepted, then settled (#550). */
export const INIT_PROTOCOL_ACCEPTED_THEN_SETTLED = "accepted_then_settled";
const INIT_PROGRESS_METHOD = "mobkit/init_progress";
const INIT_SETTLED_METHOD = "mobkit/init_settled";

/**
 * Correlates one accepted-then-settled `mobkit/init`. Registered before the
 * init request is written, so no progress or settlement line arrives
 * unclaimed.
 */
export class InitWatch {
  /** The last `mobkit/init_progress` phase seen, for diagnostics. */
  lastPhase: string | null = null;
  private _settle!: (params: Record<string, unknown>) => void;
  private _fail!: (error: Error) => void;
  private _done = false;
  private readonly _settlement: Promise<Record<string, unknown>>;

  constructor(readonly initId: string) {
    this._settlement = new Promise((resolve, reject) => {
      this._settle = resolve;
      this._fail = reject;
    });
    // A failure nobody awaits must not become an unhandled rejection.
    this._settlement.catch(() => undefined);
  }

  /** @internal */
  deliver(method: string, params: Record<string, unknown>): void {
    if (method === INIT_PROGRESS_METHOD) {
      if (typeof params.phase === "string") this.lastPhase = params.phase;
    } else if (method === INIT_SETTLED_METHOD && !this._done) {
      this._done = true;
      this._settle(params);
    }
  }

  /** @internal */
  fail(error: Error): void {
    if (this._done) return;
    this._done = true;
    this._fail(error);
  }

  /**
   * Wait for `mobkit/init_settled`. `deadlineMs` is the caller's own cap
   * (`null`: wait until the gateway settles, exits, or the reader fails);
   * exceeding it rejects with `InitDeadlineExceeded`.
   */
  settlement(deadlineMs: number | null): Promise<Record<string, unknown>> {
    if (deadlineMs === null) return this._settlement;
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => reject(new InitDeadlineExceeded(deadlineMs)), deadlineMs);
      this._settlement.then(
        (value) => {
          clearTimeout(timer);
          resolve(value);
        },
        (error: unknown) => {
          clearTimeout(timer);
          reject(error);
        },
      );
    });
  }
}

/** The caller's init deadline ran out before the init settled. */
export class InitDeadlineExceeded extends Error {
  constructor(readonly deadlineMs: number) {
    super(`the init deadline of ${deadlineMs}ms ran out before init settled`);
    this.name = "InitDeadlineExceeded";
  }
}

function containsNonFinite(obj: unknown): boolean {
  if (typeof obj === "number") return !Number.isFinite(obj);
  if (Array.isArray(obj)) return obj.some(containsNonFinite);
  if (typeof obj === "object" && obj !== null) {
    return Object.values(obj as Record<string, unknown>).some(containsNonFinite);
  }
  return false;
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
  private _stderrFd: number | null = null;
  private readonly _env: Record<string, string>;
  private readonly _timeout: number;
  private _callbackHandler: CallbackHandler | null = null;
  // Mutable only so deterministic tests can scale the 125-second contract.
  private _providerCallbackCompletionMs = PROVIDER_CALLBACK_COMPLETION_MS;
  private _supportsShutdownHandshake = false;
  private _shutdownHorizonMs = PERSISTENT_TRANSPORT_SHUTDOWN_GRACE_MS;
  private readonly _pending = new Map<
    string,
    { resolve: (value: unknown) => void; reject: (error: Error) => void }
  >();
  // Set once the reader for the current gateway process has stopped; every
  // waiter and every later request fails with it.
  private _readerFailure: TransportReaderFailedError | null = null;
  // The accepted-then-settled init in flight on this process, if any.
  private _initWatch: InitWatch | null = null;

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

  /** Register the watch for `initId` before writing `mobkit/init`. */
  openInitWatch(initId: string): InitWatch {
    const watch = new InitWatch(initId);
    this._initWatch = watch;
    if (this._readerFailure !== null) watch.fail(this._readerFailure);
    return watch;
  }

  closeInitWatch(watch: InitWatch): void {
    if (this._initWatch === watch) this._initWatch = null;
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
    // A new process gets a new reader.
    this._readerFailure = null;
    this._process = spawn(this.gatewayBin, ["--persistent"], {
      env: this._env,
      stdio: ["pipe", "pipe", this._stderrDisposition()],
    });

    const child = this._process;

    // Background reader on stdout
    if (child.stdout) {
      const rl = createInterface({ input: child.stdout });
      rl.on("line", (line: string) => this._handleLine(line));
      rl.on("close", () => this._onReaderClosed("the gateway closed its stdout"));
    }

    child.on("error", () => {
      // Process spawn error — fail all pending
      for (const [id, pending] of this._pending) {
        this._pending.delete(id);
        pending.reject(new Error("gateway process failed to start"));
      }
    });
  }

  /** Handle one stdout line. A line that is not a JSON object is skipped. */
  private _handleLine(line: string): void {
    let parsed: unknown;
    try {
      parsed = JSON.parse(line);
    } catch {
      return;
    }
    if (typeof parsed !== "object" || parsed === null || Array.isArray(parsed)) return;
    const msg = parsed as Record<string, unknown>;

    if ("method" in msg) {
      const method = String(msg.method);
      if (!("id" in msg) && (method === INIT_PROGRESS_METHOD || method === INIT_SETTLED_METHOD)) {
        const params = msg.params;
        const watch = this._initWatch;
        // Only the init this process is running; a late or foreign line is ignored.
        if (
          watch !== null &&
          typeof params === "object" &&
          params !== null &&
          (params as Record<string, unknown>).init_id === watch.initId
        ) {
          watch.deliver(method, params as Record<string, unknown>);
        }
        return;
      }
      this._handleCallback(msg);
    } else if ("id" in msg) {
      const msgId = String(msg.id);
      const pending = this._pending.get(msgId);
      if (pending) {
        this._pending.delete(msgId);
        pending.resolve(msg);
      }
    }
  }

  /** No response can arrive any more: fail every waiter, typed. */
  private _onReaderClosed(reason: string): void {
    const failure = new TransportReaderFailedError(reason);
    this._readerFailure = failure;
    for (const [id, pending] of this._pending) {
      this._pending.delete(id);
      pending.reject(failure);
    }
    this._initWatch?.fail(failure);
  }

  private _handleCallback(msg: Record<string, unknown>): void {
    const handler = this._callbackHandler;
    const method = String(msg.method ?? "");
    if (!handler) {
      // Answer now: the gateway would otherwise wait out its full callback
      // deadline for a response that never comes.
      if (msg.id !== undefined) {
        this._writeLine({
          jsonrpc: "2.0",
          id: String(msg.id),
          error: {
            code: -32000,
            message: `no callback handler is registered for ${method}`,
            data: { kind: "callback_handler_unavailable", method },
          },
        });
      }
      return;
    }

    const params = (
      typeof msg.params === "object" && msg.params !== null
        ? msg.params
        : {}
    ) as Record<string, unknown>;
    const callbackId = msg.id !== undefined ? String(msg.id) : null;
    const controller = new AbortController();
    const deadlineMs = Date.now() + this._providerCallbackCompletionMs;
    let completed = false;
    const timer = setTimeout(() => {
      if (completed) return;
      completed = true;
      const error = new Error(
        "provider callback exceeded its 125s host completion deadline",
      );
      controller.abort(error);
      if (callbackId !== null) {
        this._writeLine({
          jsonrpc: "2.0",
          id: callbackId,
          error: { code: -32000, message: error.message },
        });
      }
    }, this._providerCallbackCompletionMs);
    timer.unref?.();

    Promise.resolve()
      .then(() => handler(method, params, {
        signal: controller.signal,
        deadlineMs,
      }))
      .then((result) => {
        if (completed) return;
        completed = true;
        clearTimeout(timer);
        if (callbackId === null) return; // Notification — no response
        const sanitized = sanitizeForJson(result);
        if (containsNonFinite(sanitized)) {
          // JSON has no NaN or Infinity; JSON.stringify would silently turn
          // them into null and change the provider's answer. Refuse it.
          this._writeLine({
            jsonrpc: "2.0",
            id: callbackId,
            error: {
              code: -32000,
              message: `${method} returned a non-finite number (NaN or Infinity), which JSON cannot carry`,
              data: { kind: "non_finite_result" },
            },
          });
          return;
        }
        this._writeLine({
          jsonrpc: "2.0",
          id: callbackId,
          result: sanitized,
        });
      })
      .catch((err: unknown) => {
        if (completed) return;
        completed = true;
        clearTimeout(timer);
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

  /**
   * Send one request and await its response. `options.timeoutMs` overrides
   * the transport's request timeout for this request only (a server-side
   * wait whose own deadline is longer than the default).
   */
  async sendAsync(
    request: Record<string, unknown>,
    options: { timeoutMs?: number } = {},
  ): Promise<unknown> {
    const response = await this._sendAsyncWithTimeout(
      request,
      options.timeoutMs ?? this._timeout,
    );
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
    if (this._readerFailure !== null) throw this._readerFailure;

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
        if (this._stderrFd !== null) {
          closeSync(this._stderrFd);
          this._stderrFd = null;
        }
      }
      if (this._stopping === stopping) this._stopping = null;
    });
    this._stopping = stopping;
    return stopping;
  }

  isRunning(): boolean {
    return this._process !== null && this._process.exitCode === null;
  }

  /**
   * Gateway stderr disposition (tracing lines, panic hooks, migration
   * progress). Default: `"inherit"` — the child's stderr flows to the host
   * process's stderr; the old `"ignore"` default silently discarded a week
   * of panic-hook lines in one production fleet. Opt-outs, mirroring the
   * Python SDK:
   *
   * - `MOBKIT_GATEWAY_STDERR_FILE=<path>`: append gateway stderr to a file.
   * - `MOBKIT_GATEWAY_STDERR=devnull`: discard (the pre-0.8.9 default).
   */
  private _stderrDisposition(): "inherit" | "ignore" | number {
    if (this._stderrFd !== null) {
      closeSync(this._stderrFd);
      this._stderrFd = null;
    }
    const stderrPath = (this._env.MOBKIT_GATEWAY_STDERR_FILE ?? "").trim();
    if (stderrPath) {
      this._stderrFd = openSync(stderrPath, "a");
      return this._stderrFd;
    }
    const optOut = (this._env.MOBKIT_GATEWAY_STDERR ?? "").trim().toLowerCase();
    if (optOut === "devnull") {
      return "ignore";
    }
    return "inherit";
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
