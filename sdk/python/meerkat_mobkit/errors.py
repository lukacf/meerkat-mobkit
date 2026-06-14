"""Typed error hierarchy for MobKit SDK."""
from __future__ import annotations

from typing import Any

# JSON-RPC error code returned by `mobkit/mob_events/{query,subscribe}` and
# the `/mobkit/mob_events/stream` SSE route when the caller's `after_seq`
# is past the current ledger frontier.
MOB_EVENTS_STALE_CURSOR_CODE: int = -32010
CAPABILITY_UNAVAILABLE_CODE: int = -32004
# Transient/recoverable identity-plane lease loss on a send/dispatch. Distinct
# from CAPABILITY_UNAVAILABLE_CODE (-32004) so a lease that merely needs
# re-acquisition is not mis-typed as a permanent capability gap.
LEASE_LOST_CODE: int = -32005
MEMORY_BACKEND_UNAVAILABLE_CODE: int = -32012
CONSOLE_TIMELINE_REPLAY_UNAVAILABLE_CODE: int = -32013


class MobKitError(Exception):
    """Base exception for all MobKit SDK errors."""


class TransportError(MobKitError):
    """Raised when the transport layer fails (subprocess died, connection refused, etc.)."""


class RpcError(MobKitError):
    """Raised when a JSON-RPC call returns an error response."""

    def __init__(
        self,
        code: int,
        message: str,
        *,
        request_id: str = "",
        method: str = "",
        data: Any | None = None,
    ):
        super().__init__(message)
        self.code = code
        self.request_id = request_id
        self.method = method
        self.data = data


class MobEventsStaleError(RpcError):
    """Raised when the caller passes an ``after_seq`` past the current ledger frontier.

    The server's structured ``data`` payload carries ``after_cursor`` and
    ``latest_cursor``. Use ``latest_cursor`` to rewind and resume.
    """

    def __init__(
        self,
        message: str,
        *,
        after_cursor: int,
        latest_cursor: int,
        request_id: str = "",
        method: str = "",
        data: Any | None = None,
    ):
        super().__init__(
            MOB_EVENTS_STALE_CURSOR_CODE,
            message,
            request_id=request_id,
            method=method,
            data=data,
        )
        self.after_cursor = after_cursor
        self.latest_cursor = latest_cursor

    @classmethod
    def from_rpc_error(cls, err: RpcError) -> MobEventsStaleError:
        """Reify a generic ``RpcError`` with code ``-32010`` into the typed form.

        Reads ``after_cursor`` / ``latest_cursor`` from ``err.data`` (the
        JSON-RPC ``error.data`` payload). Missing fields fall back to ``0``.
        """
        payload = err.data if isinstance(err.data, dict) else {}
        return cls(
            str(err),
            after_cursor=int(payload.get("after_cursor", 0)),
            latest_cursor=int(payload.get("latest_cursor", 0)),
            request_id=err.request_id,
            method=err.method,
            data=err.data,
        )


class CapabilityUnavailableError(RpcError):
    """Raised when a requested capability is not available on the runtime."""

    def __init__(
        self,
        message: str,
        *,
        request_id: str = "",
        method: str = "",
        data: Any | None = None,
    ):
        super().__init__(
            CAPABILITY_UNAVAILABLE_CODE,
            message,
            request_id=request_id,
            method=method,
            data=data,
        )


class LeaseLostError(RpcError):
    """Raised when an identity's lease was lost mid send/dispatch.

    This is transient and recoverable: the identity simply needs to
    re-acquire its lease. Distinct from :class:`CapabilityUnavailableError`
    so callers do not treat a recoverable lease loss as a permanent
    capability gap.
    """

    def __init__(
        self,
        message: str,
        *,
        request_id: str = "",
        method: str = "",
        data: Any | None = None,
    ):
        super().__init__(
            LEASE_LOST_CODE,
            message,
            request_id=request_id,
            method=method,
            data=data,
        )


class MemoryBackendUnavailableError(RpcError):
    """Raised when the configured memory backend cannot serve a request."""

    def __init__(
        self,
        message: str,
        *,
        request_id: str = "",
        method: str = "",
        data: Any | None = None,
    ):
        super().__init__(
            MEMORY_BACKEND_UNAVAILABLE_CODE,
            message,
            request_id=request_id,
            method=method,
            data=data,
        )


class ConsoleTimelineReplayUnavailableError(RpcError):
    """Raised when a console timeline cursor cannot be replayed."""

    def __init__(
        self,
        message: str,
        *,
        request_id: str = "",
        method: str = "",
        data: Any | None = None,
    ):
        super().__init__(
            CONSOLE_TIMELINE_REPLAY_UNAVAILABLE_CODE,
            message,
            request_id=request_id,
            method=method,
            data=data,
        )


class ContractMismatchError(MobKitError):
    """Raised when the SDK and runtime contract versions are incompatible."""


class NotConnectedError(MobKitError):
    """Raised when an operation requires a connected runtime but none is available."""
