"""Persistent subprocess transport for MobKit JSON-RPC."""
from __future__ import annotations

import asyncio
import json
import logging
import math
import os
import subprocess
import threading
import time
from typing import Any, Callable
from uuid import uuid4

_log = logging.getLogger("meerkat_mobkit")

# Stock gateways bound their complete shutdown path to 300 seconds: two
# five-second admission drains, a 285-second runtime cleanup window (long
# enough for an admitted 120-second provider callback and the final
# 120-second lease-release callback), and a five-second stdout drain. The
# advertised 310-second horizon adds response-delivery/process-reap margin.
# It is also the safe fallback for handshake-capable custom gateways which do
# not yet advertise an explicit horizon.
_GATEWAY_SHUTDOWN_GRACE_SECONDS = 310.0
_MAX_GATEWAY_SHUTDOWN_HORIZON_MS = 2_147_483_647
_PROCESS_TERMINATE_GRACE_SECONDS = 5.0
_PROCESS_KILL_GRACE_SECONDS = 5.0
_GATEWAY_SHUTDOWN_METHOD = "mobkit/shutdown"


def _sanitize_for_json(obj: Any) -> Any:
    """Recursively sanitize a value so json.dumps won't fail.

    Non-serializable leaves (callables, custom objects) are converted to
    their string representation so the callback response always reaches Rust.
    """
    if obj is None or isinstance(obj, (bool, int, float, str)):
        return obj
    if isinstance(obj, dict):
        return {str(k): _sanitize_for_json(v) for k, v in obj.items()}
    if isinstance(obj, (list, tuple)):
        return [_sanitize_for_json(v) for v in obj]
    # Fall back to string repr for non-serializable objects (e.g. tool callables)
    try:
        json.dumps(obj)
        return obj
    except (TypeError, ValueError):
        return str(obj)


class PersistentTransport:
    """Long-lived mobkit-rpc subprocess communicating over stdin/stdout JSON-RPC.

    Uses a background reader thread to multiplex responses and callbacks.
    Unlike the per-call subprocess transport, this keeps the process alive
    so mob state persists across calls. stderr is sent to devnull to avoid
    backpressure deadlocks.
    """

    def __init__(
        self,
        gateway_bin: str,
        *,
        env: dict[str, str] | None = None,
        timeout: float = 60.0,
    ):
        self.gateway_bin = gateway_bin
        self._env = {**os.environ, **(env or {})}
        self._process: subprocess.Popen[bytes] | None = None
        self._timeout = timeout
        self._write_lock = threading.Lock()      # protects stdin writes
        self._pending_lock = threading.Lock()     # protects _pending and _results
        self._pending: dict[str, threading.Event] = {}
        self._results: dict[str, Any] = {}
        self._reader_thread: threading.Thread | None = None
        self._callback_handler: Callable | None = None
        self._loop: asyncio.AbstractEventLoop | None = None
        self._stderr_file = None
        self._supports_shutdown_handshake = False
        self._shutdown_horizon_seconds = _GATEWAY_SHUTDOWN_GRACE_SECONDS

    def set_callback_handler(self, handler: Callable) -> None:
        self._callback_handler = handler

    @property
    def request_timeout(self) -> float:
        """Default timeout, in seconds, for one outbound RPC request."""
        return self._timeout

    def start(self) -> None:
        if self._process is not None and self._process.poll() is None:
            return
        # Transport capabilities belong to one gateway process. A restarted
        # child must negotiate them again through mobkit/init.
        self._supports_shutdown_handshake = False
        self._shutdown_horizon_seconds = _GATEWAY_SHUTDOWN_GRACE_SECONDS
        # Capture event loop for async callback dispatch
        try:
            self._loop = asyncio.get_running_loop()
        except RuntimeError:
            self._loop = None
        stderr_target: Any = subprocess.DEVNULL
        stderr_path = self._env.get("MOBKIT_GATEWAY_STDERR_FILE", "").strip()
        if stderr_path:
            self._stderr_file = open(stderr_path, "ab", buffering=0)
            stderr_target = self._stderr_file

        self._process = subprocess.Popen(
            [self.gateway_bin, "--persistent"],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=stderr_target,
            env=self._env,
        )
        self._reader_thread = threading.Thread(
            target=self._reader_loop, daemon=True, name="mobkit-reader"
        )
        self._reader_thread.start()

    def _reader_loop(self) -> None:
        assert self._process is not None and self._process.stdout is not None
        while True:
            line = self._process.stdout.readline()
            if not line:
                # Process closed stdout — store error for all pending callers and wake them
                with self._pending_lock:
                    for msg_id in self._pending:
                        if msg_id not in self._results:
                            self._results[msg_id] = {
                                "error": {"code": -32099, "message": "subprocess died"}
                            }
                    for event in self._pending.values():
                        event.set()
                break
            try:
                msg = json.loads(line.decode("utf-8"))
            except json.JSONDecodeError:
                _log.warning("transport: non-JSON line from subprocess: %s", line[:200])
                continue

            if "method" in msg:
                # Callback or notification FROM Rust
                self._handle_callback(msg)
            elif "id" in msg:
                # Response to a pending request
                msg_id = str(msg["id"])
                with self._pending_lock:
                    event = self._pending.get(msg_id)
                    if event is not None:
                        self._results[msg_id] = msg
                if event is not None:
                    event.set()
                else:
                    # The caller may already have timed out and removed its
                    # pending entry. Do not retain an unclaimable late result.
                    _log.debug(
                        "transport: dropping response for non-pending id=%s",
                        msg_id,
                    )
            else:
                _log.warning(
                    "transport: unrecognized message (no id or method): %s",
                    str(msg)[:200],
                )

    def _handle_callback(self, msg: dict) -> None:
        """Dispatch callback in a separate thread so the reader loop is not blocked."""
        if self._callback_handler is None:
            _log.warning(
                "transport: received callback but no handler registered: %s",
                msg.get("method"),
            )
            return
        # Dispatch in a daemon thread to avoid blocking the reader loop
        t = threading.Thread(
            target=self._dispatch_callback, args=(msg,), daemon=True,
            name="mobkit-callback",
        )
        t.start()

    def _dispatch_callback(self, msg: dict) -> None:
        method = msg.get("method", "")
        params = msg.get("params", {})
        callback_id = msg.get("id")  # None for notifications
        try:
            if self._loop is not None and self._loop.is_running():
                future = asyncio.run_coroutine_threadsafe(
                    self._callback_handler(method, params), self._loop
                )
                result = future.result(timeout=self._timeout)
            else:
                raise RuntimeError(
                    "PersistentTransport: no running event loop for callback dispatch"
                )
            # Notifications (no id) are fire-and-forget — no response sent
            if callback_id is None:
                return
            # Ensure result is JSON-serializable before building response.
            # Tools or other callback results may contain non-serializable objects;
            # sanitize them to strings to prevent json.dumps failures in _write_line.
            response = {"jsonrpc": "2.0", "id": callback_id, "result": _sanitize_for_json(result)}
            self._write_line(response)
        except Exception as exc:
            # Notifications: log only, don't try to send error response
            if callback_id is None:
                _log.warning("notification dispatch error (%s): %s", method, exc)
                return
            _log.warning("callback dispatch error: %s", exc)
            error_response = {
                "jsonrpc": "2.0",
                "id": callback_id,
                "error": {"code": -32000, "message": str(exc)},
            }
            try:
                self._write_line(error_response)
            except Exception:
                _log.error("failed to send callback error response for id=%s", callback_id)

    def _write_line(self, obj: dict) -> None:
        with self._write_lock:
            if self._process and self._process.stdin:
                data = json.dumps(obj) + "\n"
                self._process.stdin.write(data.encode("utf-8"))
                self._process.stdin.flush()

    def send_sync(
        self,
        request: dict[str, Any],
        *,
        timeout: float | None = None,
    ) -> Any:
        self._ensure_running()
        response = self._send_sync_running(request, timeout=timeout)
        if request.get("method") == "mobkit/init":
            result = response.get("result") if isinstance(response, dict) else None
            self._supports_shutdown_handshake = bool(
                isinstance(result, dict)
                and result.get("stdio_shutdown_handshake") is True
            )
            self._shutdown_horizon_seconds = _GATEWAY_SHUTDOWN_GRACE_SECONDS
            if self._supports_shutdown_handshake and isinstance(result, dict):
                horizon_ms = result.get("stdio_shutdown_horizon_ms")
                if (
                    isinstance(horizon_ms, int)
                    and not isinstance(horizon_ms, bool)
                    and 0 < horizon_ms <= _MAX_GATEWAY_SHUTDOWN_HORIZON_MS
                ):
                    self._shutdown_horizon_seconds = horizon_ms / 1000.0
        return response

    def _send_sync_running(
        self,
        request: dict[str, Any],
        *,
        timeout: float | None = None,
    ) -> Any:
        """Send on the current child without starting a replacement process."""
        request_timeout = self._timeout if timeout is None else timeout
        if (
            isinstance(request_timeout, bool)
            or not isinstance(request_timeout, (int, float))
            or not math.isfinite(request_timeout)
            or request_timeout <= 0
        ):
            raise ValueError(
                "persistent transport: timeout must be a positive finite number"
            )
        # Pre-fix, requests with no `id` (or two callers using the
        # same id) collided on `self._pending[""]`: the second
        # `_pending[msg_id] = event` clobbered the first caller's
        # Event, blocking it for the full timeout. Reject either
        # condition explicitly so the deadlock surfaces as a clear
        # ValueError at the call site.
        raw_id = request.get("id")
        if raw_id is None or (isinstance(raw_id, str) and not raw_id):
            raise ValueError(
                "persistent transport: request must carry a non-empty `id` "
                "(use uuid4 or similar) — empty/missing ids collide on the "
                "in-flight pending map and deadlock concurrent callers"
            )
        msg_id = str(raw_id)
        event = threading.Event()
        with self._pending_lock:
            if msg_id in self._pending:
                raise ValueError(
                    f"persistent transport: request id {msg_id!r} is already "
                    f"in flight; concurrent callers must use distinct ids"
                )
            self._pending[msg_id] = event
        # Write request (lock only for write, release before wait)
        self._write_line(request)
        # Wait for response — no locks held
        if not event.wait(timeout=request_timeout):
            with self._pending_lock:
                self._pending.pop(msg_id, None)
                self._results.pop(msg_id, None)
            raise RuntimeError(
                f"persistent transport: timeout after {request_timeout}s "
                "waiting for response"
            )
        with self._pending_lock:
            self._pending.pop(msg_id, None)
            result = self._results.pop(msg_id, None)
        if result is None:
            raise RuntimeError("persistent transport: subprocess closed stdout")
        return result

    def _request_gateway_shutdown(
        self,
        process: subprocess.Popen[bytes],
        *,
        timeout: float,
    ) -> None:
        """Wait for runtime cleanup while the current child's stdin stays open."""
        if self._process is not process or process.poll() is not None:
            raise RuntimeError("gateway exited before shutdown handshake")
        response = self._send_sync_running(
            {
                "jsonrpc": "2.0",
                "id": f"mobkit-shutdown-{uuid4()}",
                "method": _GATEWAY_SHUTDOWN_METHOD,
                "params": {},
            },
            timeout=timeout,
        )
        if not isinstance(response, dict):
            raise RuntimeError("gateway shutdown returned a malformed response")
        error = response.get("error")
        if isinstance(error, dict):
            message = error.get("message", "unknown gateway error")
            raise RuntimeError(f"gateway shutdown failed: {message}")
        result = response.get("result")
        if (
            not isinstance(result, dict)
            or result.get("shutdown") is not True
            or result.get("runtime_cleanup_completed") is False
        ):
            raise RuntimeError(
                "gateway shutdown did not complete runtime-owned cleanup"
            )

    async def send_async(
        self,
        request: dict[str, Any],
        *,
        timeout: float | None = None,
    ) -> Any:
        return await asyncio.to_thread(self.send_sync, request, timeout=timeout)

    def stop(self) -> None:
        process = getattr(self, "_process", None)
        if process is None:
            return
        shutdown_horizon = getattr(
            self,
            "_shutdown_horizon_seconds",
            _GATEWAY_SHUTDOWN_GRACE_SECONDS,
        )
        shutdown_error: Exception | None = None
        shutdown_started = time.monotonic()
        try:
            # The gateway may need to round-trip lease/continuity provider
            # callbacks while UnifiedRuntime shuts down. Keep stdin open until
            # a capable gateway acknowledges that cleanup is complete. Older
            # or custom gateways stay on the EOF protocol below.
            if getattr(self, "_supports_shutdown_handshake", False):
                try:
                    self._request_gateway_shutdown(
                        process,
                        timeout=shutdown_horizon,
                    )
                except Exception as exc:
                    _log.debug("gateway shutdown handshake failed: %s", exc)
                    shutdown_error = exc

            elapsed = time.monotonic() - shutdown_started
            remaining_grace = max(0.0, shutdown_horizon - elapsed)
            if process.stdin:
                try:
                    process.stdin.close()
                except OSError:
                    # A gateway that already closed its pipe still needs to be
                    # reaped below.
                    pass
            try:
                process.wait(timeout=remaining_grace)
            except subprocess.TimeoutExpired:
                try:
                    process.terminate()
                except OSError:
                    # The child can exit between the timed wait and signal.
                    pass
                try:
                    process.wait(timeout=_PROCESS_TERMINATE_GRACE_SECONDS)
                except subprocess.TimeoutExpired:
                    try:
                        process.kill()
                    except OSError:
                        pass
                    try:
                        process.wait(timeout=_PROCESS_KILL_GRACE_SECONDS)
                    except (OSError, subprocess.TimeoutExpired):
                        # Signals are best effort; never make SDK teardown
                        # unbounded if the OS cannot reap the child promptly.
                        pass
            except OSError:
                # Already-reaped children need no further cleanup.
                pass
        finally:
            self._process = None
            stderr_file = getattr(self, "_stderr_file", None)
            if stderr_file is not None:
                stderr_file.close()
                self._stderr_file = None
        if shutdown_error is not None:
            raise RuntimeError(
                "persistent transport: gateway shutdown failed after bounded cleanup"
            ) from shutdown_error

    def is_running(self) -> bool:
        return self._process is not None and self._process.poll() is None

    def _ensure_running(self) -> None:
        if not self.is_running():
            self.start()

    def __del__(self) -> None:
        try:
            self.stop()
        except Exception:
            # Explicit shutdown surfaces handshake failures. Destructors run
            # outside a caller-owned error channel and must stay best effort.
            pass


def create_persistent_transport(gateway_bin: str, **kwargs: Any) -> PersistentTransport:
    transport = PersistentTransport(gateway_bin, **kwargs)
    transport.start()
    return transport
