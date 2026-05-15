/**
 * Typed error hierarchy for the MobKit SDK.
 *
 * @example
 * ```ts
 * import { RpcError, MobKitError } from "@rkat/mobkit-sdk";
 *
 * try {
 *   await handle.status();
 * } catch (err) {
 *   if (err instanceof RpcError) {
 *     console.error(`RPC ${err.method} failed: code=${err.code}`);
 *   }
 * }
 * ```
 */

// -- Base error -----------------------------------------------------------

/** Base exception for all MobKit SDK errors. */
export class MobKitError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "MobKitError";
  }
}

// -- Transport errors -----------------------------------------------------

/** Raised when the transport layer fails (subprocess died, connection refused, etc.). */
export class TransportError extends MobKitError {
  constructor(message: string) {
    super(message);
    this.name = "TransportError";
  }
}

// -- RPC errors -----------------------------------------------------------

/** Raised when a JSON-RPC call returns an error response. */
export class RpcError extends MobKitError {
  constructor(
    readonly code: number,
    message: string,
    readonly requestId: string,
    readonly method: string,
    /** Optional structured payload from the JSON-RPC `error.data` field. */
    readonly data?: unknown,
  ) {
    super(message);
    this.name = "RpcError";
  }
}

/**
 * JSON-RPC error code returned when the caller's `after_seq` is past the
 * current ledger frontier. Returned by `mobkit/mob_events/{query,subscribe}`
 * and the `/mobkit/mob_events/stream` SSE route (HTTP 410 Gone).
 */
export const MOB_EVENTS_STALE_CURSOR_CODE = -32010 as const;
export const CAPABILITY_UNAVAILABLE_CODE = -32004 as const;
export const MEMORY_BACKEND_UNAVAILABLE_CODE = -32012 as const;

/**
 * Raised when the caller passes an `after_seq` past the current ledger
 * frontier. The server's `error.data` payload carries `after_cursor` and
 * `latest_cursor` — use the latter to rewind and resume.
 */
export class MobEventsStaleError extends RpcError {
  constructor(
    message: string,
    readonly afterCursor: number,
    readonly latestCursor: number,
    requestId: string,
    method: string,
    data?: unknown,
  ) {
    super(MOB_EVENTS_STALE_CURSOR_CODE, message, requestId, method, data);
    this.name = "MobEventsStaleError";
  }

  /**
   * Reify a generic {@link RpcError} with code `-32010` into the typed
   * form. Reads `after_cursor` / `latest_cursor` from the JSON-RPC
   * `error.data` payload; missing fields fall back to `0`.
   */
  static fromRpcError(err: RpcError): MobEventsStaleError {
    const payload =
      typeof err.data === "object" && err.data !== null
        ? (err.data as Record<string, unknown>)
        : {};
    const afterCursor = Number(payload.after_cursor ?? 0);
    const latestCursor = Number(payload.latest_cursor ?? 0);
    return new MobEventsStaleError(
      err.message,
      Number.isFinite(afterCursor) ? afterCursor : 0,
      Number.isFinite(latestCursor) ? latestCursor : 0,
      err.requestId,
      err.method,
      err.data,
    );
  }
}

// -- Capability errors ----------------------------------------------------

/** Raised when a requested capability is not available on the runtime. */
export class CapabilityUnavailableError extends RpcError {
  constructor(
    message: string,
    requestId = "",
    method = "",
    data?: unknown,
  ) {
    super(CAPABILITY_UNAVAILABLE_CODE, message, requestId, method, data);
    this.name = "CapabilityUnavailableError";
  }
}

export class MemoryBackendUnavailableError extends RpcError {
  constructor(
    message: string,
    requestId: string,
    method: string,
    data?: unknown,
  ) {
    super(MEMORY_BACKEND_UNAVAILABLE_CODE, message, requestId, method, data);
    this.name = "MemoryBackendUnavailableError";
  }
}

// -- Contract errors ------------------------------------------------------

/** Raised when the SDK and runtime contract versions are incompatible. */
export class ContractMismatchError extends MobKitError {
  constructor(message: string) {
    super(message);
    this.name = "ContractMismatchError";
  }
}

// -- Connection errors ----------------------------------------------------

/** Raised when an operation requires a connected runtime but none is available. */
export class NotConnectedError extends MobKitError {
  constructor(message: string) {
    super(message);
    this.name = "NotConnectedError";
  }
}

// -- Cross-module identity helpers ---------------------------------------

/**
 * Structural test for an `RpcError`. Pre-fix every site used
 * `err instanceof RpcError`, but JS class identity is module-scoped:
 * dual CJS+ESM packaging, vitest module isolation, and hoisted-vs-
 * nested workspace deps can produce two `RpcError` constructors that
 * fail `instanceof` for each other's instances. The structural check
 * survives those splits.
 */
export function isRpcError(err: unknown): err is RpcError {
  if (err instanceof RpcError) {
    return true;
  }
  if (err === null || typeof err !== "object") {
    return false;
  }
  const candidate = err as { name?: unknown; code?: unknown };
  return (
    candidate.name === "RpcError" ||
    candidate.name === "MobEventsStaleError" ||
    candidate.name === "CapabilityUnavailableError" ||
    candidate.name === "MemoryBackendUnavailableError"
  ) && typeof candidate.code === "number";
}

/** Structural test for a `MobEventsStaleError`. See `isRpcError`. */
export function isMobEventsStaleError(err: unknown): err is MobEventsStaleError {
  if (err instanceof MobEventsStaleError) {
    return true;
  }
  return (
    isRpcError(err) &&
    err.code === MOB_EVENTS_STALE_CURSOR_CODE &&
    (err as { name: string }).name === "MobEventsStaleError"
  );
}

// -- Backward compatibility -----------------------------------------------

/** @deprecated Use {@link RpcError} instead. */
export const MobkitRpcError = RpcError;
/** @deprecated Use {@link RpcError} instead. */
export type MobkitRpcError = RpcError;
