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
  ) {
    super(message);
    this.name = "RpcError";
  }
}

// -- Capability errors ----------------------------------------------------

/** Raised when a requested capability is not available on the runtime. */
export class CapabilityUnavailableError extends MobKitError {
  constructor(message: string) {
    super(message);
    this.name = "CapabilityUnavailableError";
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

// -- Backward compatibility -----------------------------------------------

/** @deprecated Use {@link RpcError} instead. */
export const MobkitRpcError = RpcError;
/** @deprecated Use {@link RpcError} instead. */
export type MobkitRpcError = RpcError;
