export function errorMessage(error: unknown): string {
  if (error instanceof Error) {
    return error.message;
  }
  return String(error);
}

/// Extract the JSON-RPC error code from an error thrown by the console
/// transport (which annotates errors with `rpcError.code`). Returns null when
/// the error is not a typed JSON-RPC failure.
export function jsonRpcErrorCode(error: unknown): number | null {
  const rpcError = (error as { rpcError?: { code?: unknown } } | null)?.rpcError;
  return typeof rpcError?.code === "number" ? rpcError.code : null;
}

/// Extract the HTTP status the console transport annotates on non-OK RPC
/// responses (`httpStatus`). Returns null when the error did not come from an
/// HTTP-level rejection (network failure, timeout, or a JSON-RPC error body).
export function httpStatusCode(error: unknown): number | null {
  const status = (error as { httpStatus?: unknown } | null)?.httpStatus;
  return typeof status === "number" ? status : null;
}
