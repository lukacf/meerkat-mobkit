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
  constructor(message: string, options?: ErrorOptions) {
    super(message, options);
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
/**
 * Transient/recoverable identity-plane lease loss on a send/dispatch. Distinct
 * from {@link CAPABILITY_UNAVAILABLE_CODE} (-32004) so a lease that merely needs
 * re-acquisition is not mis-typed as a permanent capability gap.
 */
export const LEASE_LOST_CODE = -32005 as const;
export const MEMORY_BACKEND_UNAVAILABLE_CODE = -32012 as const;
export const CONSOLE_TIMELINE_REPLAY_UNAVAILABLE_CODE = -32013 as const;
/**
 * Fail-closed storage refusal at gateway startup (`mobkit/init`): file-name
 * twins the storage layout refuses to pick between, a store that failed to
 * open where the silent fallback used to be, or a state-root creation
 * failure. The message carries the remediation (the storage doctor, or the
 * explicit ephemeral declaration).
 */
export const STORAGE_RESOLUTION_CODE = -32014 as const;
/** WorkGraph service not configured on this runtime (memory-backend-unavailable pattern). */
export const WORKGRAPH_UNAVAILABLE_CODE = -32041 as const;
/** WorkGraph CAS/revision conflict — refetch the item/binding's current revision and retry. */
export const WORKGRAPH_CONFLICT_CODE = -32042 as const;

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

/**
 * Raised when an identity's lease was lost mid send/dispatch. Transient and
 * recoverable — the identity simply needs to re-acquire its lease. Distinct
 * from {@link CapabilityUnavailableError} so callers do not treat a recoverable
 * lease loss as a permanent capability gap.
 */
export class LeaseLostError extends RpcError {
  constructor(
    message: string,
    requestId = "",
    method = "",
    data?: unknown,
  ) {
    super(LEASE_LOST_CODE, message, requestId, method, data);
    this.name = "LeaseLostError";
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

export class ConsoleTimelineReplayUnavailableError extends RpcError {
  constructor(
    message: string,
    requestId: string,
    method: string,
    data?: unknown,
  ) {
    super(CONSOLE_TIMELINE_REPLAY_UNAVAILABLE_CODE, message, requestId, method, data);
    this.name = "ConsoleTimelineReplayUnavailableError";
  }
}

/**
 * Raised when the gateway refuses to start over a storage resolution gap.
 *
 * The refusals are deliberate (storage-unification fail-closed posture):
 * file-name twins (e.g. `sessions.sqlite3` beside `sessions.db`) the layout
 * will not pick between, a session/runtime/blob/metadata/console store that
 * failed to open where older gateways silently fell back to in-memory, or
 * an uncreatable state root. The message names the remediation — run the
 * storage doctor (`mobkit/storage/doctor`) for twins, fix the database
 * file, or declare the ephemeral choice explicitly (e.g.
 * `runtimeStore.memory()`).
 */
export class StorageResolutionError extends RpcError {
  constructor(
    message: string,
    requestId = "",
    method = "",
    data?: unknown,
  ) {
    super(STORAGE_RESOLUTION_CODE, message, requestId, method, data);
    this.name = "StorageResolutionError";
  }
}

/** Raised when a `mobkit/workgraph/*` call is made but no WorkGraph service is configured. */
export class WorkGraphUnavailableError extends RpcError {
  constructor(
    message: string,
    requestId: string,
    method: string,
    data?: unknown,
  ) {
    super(WORKGRAPH_UNAVAILABLE_CODE, message, requestId, method, data);
    this.name = "WorkGraphUnavailableError";
  }
}

/**
 * Raised on a WorkGraph CAS/revision conflict (the caller's `expected_revision`
 * is stale). `data.detail` carries the upstream message. Refetch the item or
 * attention binding's current revision and retry.
 */
export class WorkGraphConflictError extends RpcError {
  constructor(
    message: string,
    requestId: string,
    method: string,
    data?: unknown,
  ) {
    super(WORKGRAPH_CONFLICT_CODE, message, requestId, method, data);
    this.name = "WorkGraphConflictError";
  }
}

/** Why a gating decision named no request pending in the owner. */
export type StalePendingDecisionReason = "malformed" | "unknown" | "other_incarnation" | "resolved";

/**
 * Raised by `gatingDecide` when the decision named no request that is pending
 * in the gating owner: the ref is malformed or absent (a legacy `pending_id`
 * alone is refused), was never issued, was issued by a replaced owner, or is
 * already resolved. Nothing was decided and nothing was notified. Refresh the
 * pending list rather than retrying.
 */
export class StalePendingDecisionError extends RpcError {
  constructor(
    message: string,
    requestId: string,
    method: string,
    data?: unknown,
  ) {
    super(-32602, message, requestId, method, data);
    this.name = "StalePendingDecisionError";
  }

  /** The owner's reason. Open set: tolerate values added later. */
  get reason(): StalePendingDecisionReason | string {
    const data = typeof this.data === "object" && this.data !== null
      ? (this.data as Record<string, unknown>)
      : {};
    return typeof data.reason === "string" ? data.reason : "unknown";
  }
}

/** True when a JSON-RPC error payload is a typed stale gating decision. */
export function isStalePendingDecisionData(data: unknown): boolean {
  return typeof data === "object" && data !== null
    && (data as Record<string, unknown>).kind === "stale_pending_decision";
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

// -- Ticketed turns -------------------------------------------------------

/**
 * The turn a ticket names ran and failed (`mobkit/turn_result` state
 * `failed`). `reason` carries the runtime's typed detail.
 */
export class TurnFailedError extends MobKitError {
  readonly identity: string;
  readonly ticket: string;
  readonly reason: string;
  /** The send/dispatch result when an `*AndWait` call threw this, else `null`. */
  admission: unknown = null;

  constructor(identity: string, ticket: string, reason: string) {
    super(`turn ${ticket} of identity ${identity} failed: ${reason}`);
    this.name = "TurnFailedError";
    this.identity = identity;
    this.ticket = ticket;
    this.reason = reason;
  }
}

/**
 * The gateway knows no turn with this ticket for this identity: never
 * admitted there, another identity's, aged out, or the gateway restarted
 * since. Never guessed in either direction.
 */
/**
 * A server-side wait ended, typed, before what it waited for happened.
 * `outcome` is the gateway's token: `run_failed` (a run failed or was
 * cancelled during the wait), `broken`, `retiring` (retiring, retired or
 * being deleted), `identity_gone` or `shutting_down`. Tolerate future values.
 */
export class WaitEndedError extends MobKitError {
  readonly identity: string;
  readonly outcome: string;
  /** The send/dispatch result when an `*AndWait` call threw this, else `null`. */
  admission: unknown = null;
  /**
   * The ticket of the tracked turn the `*AndWait` call was waiting on, else
   * `null` (an untracked wait, or not thrown by an `*AndWait` call).
   */
  ticket: string | null = null;

  constructor(identity: string, outcome: string, detail = "") {
    const message = `wait on identity ${identity} ended: ${outcome}`;
    super(detail ? `${message} (${detail})` : message);
    this.name = "WaitEndedError";
    this.identity = identity;
    this.outcome = outcome;
  }
}

export class TurnUnknownError extends MobKitError {
  readonly identity: string;
  readonly ticket: string;
  /**
   * The send/dispatch result when an `*AndWait` call threw this after its
   * delivery was admitted, else `null`: do not redispatch the delivery
   * because its ticket is no longer known.
   */
  admission: unknown = null;

  constructor(identity: string, ticket: string) {
    super(
      `no turn ${ticket} is known for identity ${identity} (never admitted ` +
        `there, aged out, or the gateway restarted)`,
    );
    this.name = "TurnUnknownError";
    this.identity = identity;
    this.ticket = ticket;
  }
}

/**
 * A `trackTurn` delivery was NOT delivered at all (`turnUnavailable.code ===
 * "not_delivered"`: the gateway has no session bridge, or the identity has no
 * bound runtime). Nothing ran and there is no turn to wait for, so retrying
 * is safe, unlike a failed or unknown turn.
 */
export class TurnNotDeliveredError extends MobKitError {
  readonly identity: string;
  readonly operation: string;
  readonly code: string;
  readonly reason: string;
  /** The send/dispatch result that reported it, when an `*AndWait` call threw this. */
  admission: unknown = null;

  constructor(identity: string, operation: string, code: string, reason: string) {
    super(
      `${operation} for identity ${identity} was not delivered ` +
        `(${code}: ${reason}); there is no turn to wait for`,
    );
    this.name = "TurnNotDeliveredError";
    this.identity = identity;
    this.operation = operation;
    this.code = code;
    this.reason = reason;
  }
}

/**
 * An `*AndWait` call made with `requireAttribution: true` could not track its
 * delivery's own turn, so it refused to wait identity-wide.
 *
 * The delivery was ADMITTED: the work was handed to the identity and will
 * run, or already ran. Keep `admission` (the full send/dispatch result) and
 * do not redispatch it. `code` and `reason` carry the typed `turnUnavailable`
 * reason (`autonomous_host`, `runtime_refused`, ...), or are `null` when the
 * gateway predates turn tickets. Without `requireAttribution` the call waits
 * identity-wide instead and resolves a non-attributed result.
 */
export class TurnTrackingUnavailableError extends MobKitError {
  readonly identity: string;
  readonly operation: string;
  readonly admission: unknown;
  readonly code: string | null;
  readonly reason: string | null;

  constructor(
    identity: string,
    operation: string,
    admission: unknown,
    code: string | null,
    reason: string | null,
  ) {
    const detail = code !== null ? `${code}: ${reason}` : "no turn ticket returned";
    super(
      `${operation} for identity ${identity} was admitted but its turn ` +
        `cannot be tracked (${detail}); the admission result is retained, ` +
        `do not redispatch`,
    );
    this.name = "TurnTrackingUnavailableError";
    this.identity = identity;
    this.operation = operation;
    this.admission = admission;
    this.code = code;
    this.reason = reason;
  }
}

/**
 * Observing an ADMITTED delivery's turn failed for a transport or RPC reason
 * (the gateway did not answer the wait), and retrying that exact observation
 * ran out of the caller's deadline.
 *
 * The turn may still run, or may already have completed. `admission` is the
 * full send/dispatch result and `ticket` names the turn (`null` only on the
 * identity-wide wait). Read the turn later with `waitForTurn(ticket)` or
 * `turnResult(ticket)`; never redispatch the business work because of this
 * error. `cause` is the last observation failure and `attempts` counts
 * observation tries.
 */
export class PostAdmissionObservationError extends MobKitError {
  readonly identity: string;
  readonly operation: string;
  readonly admission: unknown;
  readonly ticket: string | null;
  readonly attempts: number;

  constructor(
    identity: string,
    operation: string,
    admission: unknown,
    ticket: string | null,
    attempts: number,
    cause: unknown,
  ) {
    const turn = ticket !== null ? `turn ${ticket}` : "its turn";
    super(
      `${operation} for identity ${identity} was admitted, but observing ` +
        `${turn} failed after ${attempts} attempt(s); the admission result is ` +
        `retained, do not redispatch`,
      { cause },
    );
    this.name = "PostAdmissionObservationError";
    this.identity = identity;
    this.operation = operation;
    this.admission = admission;
    this.ticket = ticket;
    this.attempts = attempts;
  }
}

/**
 * A ticketed turn did not settle by the caller's deadline. From an
 * `*AndWait` call it carries `admission` (the send/dispatch result) so the
 * turn can be read later instead of redispatched.
 */
export class TurnWaitTimeoutError extends MobKitError {
  readonly identity: string;
  readonly ticket: string;
  readonly timeoutMs: number;
  /** The send/dispatch result when an `*AndWait` call threw this, else `null`. */
  admission: unknown = null;

  constructor(identity: string, ticket: string, timeoutMs: number) {
    super(
      `turn ${ticket} of identity ${identity} did not complete within ` +
        `${timeoutMs}ms`,
    );
    this.name = "TurnWaitTimeoutError";
    this.identity = identity;
    this.ticket = ticket;
    this.timeoutMs = timeoutMs;
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
    candidate.name === "MemoryBackendUnavailableError" ||
    candidate.name === "ConsoleTimelineReplayUnavailableError" ||
    candidate.name === "StorageResolutionError" ||
    candidate.name === "WorkGraphUnavailableError" ||
    candidate.name === "WorkGraphConflictError" ||
    candidate.name === "StalePendingDecisionError"
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
