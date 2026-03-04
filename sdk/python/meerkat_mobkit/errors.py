"""Typed error hierarchy for MobKit SDK."""
from __future__ import annotations


class MobKitError(Exception):
    """Base exception for all MobKit SDK errors."""


class TransportError(MobKitError):
    """Raised when the transport layer fails (subprocess died, connection refused, etc.)."""


class RpcError(MobKitError):
    """Raised when a JSON-RPC call returns an error response."""

    def __init__(self, code: int, message: str, *, request_id: str = "", method: str = ""):
        super().__init__(message)
        self.code = code
        self.request_id = request_id
        self.method = method


class CapabilityUnavailableError(MobKitError):
    """Raised when a requested capability is not available on the runtime."""


class ContractMismatchError(MobKitError):
    """Raised when the SDK and runtime contract versions are incompatible."""


class NotConnectedError(MobKitError):
    """Raised when an operation requires a connected runtime but none is available."""
