"""Persistent subprocess transport for MobKit JSON-RPC."""
from __future__ import annotations

import asyncio
from concurrent.futures import TimeoutError as FutureTimeoutError
import json
import logging
import math
import os
import subprocess
import threading
import time
from typing import Any, Callable
from uuid import uuid4

from .errors import TransportReaderFailedError

_log = logging.getLogger("meerkat_mobkit")
_CURRENT_TRANSPORT = object()

# Provider operations are publicly required to finish within 120 seconds.
# Python gives their event-loop coroutine another five seconds of host
# completion margin before cancellation; the stock Rust gateway owns the
# final 130-second wire deadline.
_PROVIDER_CALLBACK_COMPLETION_SECONDS = 125.0

# The fallback matches the stock gateway's advertised shutdown horizon. It
# covers two 130-second callback windows, runtime event/mob drains, bounded
# RPC/HTTP/stdout phases, and response-delivery/process-reap margin.
# It is also the safe fallback for handshake-capable custom gateways which do
# not yet advertise an explicit horizon.
_GATEWAY_SHUTDOWN_GRACE_SECONDS = 337.0
_MAX_GATEWAY_SHUTDOWN_HORIZON_MS = 2_147_483_647
_PROCESS_TERMINATE_GRACE_SECONDS = 5.0
_PROCESS_KILL_GRACE_SECONDS = 5.0
_GATEWAY_SHUTDOWN_METHOD = "mobkit/shutdown"

# mobkit/init: accepted, then settled (#550). The SDK opts in with these
# params; the gateway answers `accepted` at once, then sends progress
# notifications and exactly one settlement carrying the same `init_id`.
INIT_PROTOCOL_ACCEPTED_THEN_SETTLED = "accepted_then_settled"
_INIT_PROGRESS_METHOD = "mobkit/init_progress"
_INIT_SETTLED_METHOD = "mobkit/init_settled"


class InitWatch:
    """Correlates one accepted-then-settled ``mobkit/init``.

    Registered before the init request is written, so a progress or
    settlement notification can never arrive unclaimed. The reader thread
    delivers into an asyncio future on the host loop, so waiting for the
    settlement is cancellable and holds no thread.
    """

    def __init__(self, init_id: str, loop: asyncio.AbstractEventLoop):
        self.init_id = init_id
        #: The last ``mobkit/init_progress`` phase seen, for diagnostics.
        self.last_phase: str | None = None
        self._loop = loop
        self._settlement: asyncio.Future[dict[str, Any]] = loop.create_future()

    def _resolve(self, settle: Callable[[asyncio.Future[dict[str, Any]]], None]) -> None:
        def apply() -> None:
            if not self._settlement.done():
                settle(self._settlement)

        try:
            self._loop.call_soon_threadsafe(apply)
        except RuntimeError:
            # The host loop is closed; nobody is waiting any more.
            pass

    def _deliver(self, method: str, params: dict[str, Any]) -> None:
        if method == _INIT_PROGRESS_METHOD:
            phase = params.get("phase")
            if isinstance(phase, str):
                self.last_phase = phase
        elif method == _INIT_SETTLED_METHOD:
            self._resolve(lambda future: future.set_result(params))

    def _fail(self, error: BaseException) -> None:
        self._resolve(lambda future: future.set_exception(error))

    async def settlement(self, deadline: float | None = None) -> dict[str, Any]:
        """Wait for ``mobkit/init_settled``. ``deadline`` is the caller's own
        cap in seconds (``None``: wait until the gateway settles, exits, or
        the reader fails); exceeding it raises ``asyncio.TimeoutError``."""
        if deadline is None:
            return await asyncio.shield(self._settlement)
        return await asyncio.wait_for(asyncio.shield(self._settlement), deadline)


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


def _contains_non_finite(obj: Any) -> bool:
    if isinstance(obj, float):
        return not math.isfinite(obj)
    if isinstance(obj, dict):
        return any(_contains_non_finite(value) for value in obj.values())
    if isinstance(obj, list):
        return any(_contains_non_finite(value) for value in obj)
    return False


class PersistentTransport:
    """Long-lived mobkit-rpc subprocess communicating over stdin/stdout JSON-RPC.

    Uses a background reader thread to multiplex responses and callbacks.
    Unlike the per-call subprocess transport, this keeps the process alive
    so mob state persists across calls.

    Gateway stderr (tracing lines, panic hooks, migration progress) is
    INHERITED by default so it reaches the host process's stderr. Override
    with either environment variable:

    - ``MOBKIT_GATEWAY_STDERR_FILE=<path>``: append gateway stderr to a file.
    - ``MOBKIT_GATEWAY_STDERR=devnull``: discard gateway stderr (the pre-0.8.9
      default). A production fleet lost a week of panic-hook lines to the old
      silent default; opt out only when the host genuinely cannot carry the
      child's stderr.
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
        # Serializes stdin writes line by line. Callback responses take
        # priority: they wait only for the line being written, never behind
        # queued requests, so a gateway callback is not held up by host RPCs.
        self._write_cond = threading.Condition()
        self._writing = False
        self._priority_writers_waiting = 0
        self._pending_lock = threading.Lock()     # protects _pending and _results
        self._pending: dict[str, threading.Event] = {}
        self._results: dict[str, Any] = {}
        self._reader_thread: threading.Thread | None = None
        self._callback_handler: Callable | None = None
        self._loop: asyncio.AbstractEventLoop | None = None
        self._stderr_file = None
        self._supports_shutdown_handshake = False
        self._shutdown_horizon_seconds = _GATEWAY_SHUTDOWN_GRACE_SECONDS
        # Set once the reader for the current gateway process has stopped;
        # every waiter and every later request fails with it.
        self._reader_failure: TransportReaderFailedError | None = None
        # The accepted-then-settled init in flight on this process, if any.
        self._init_watch: InitWatch | None = None

    def set_callback_handler(self, handler: Callable) -> None:
        self._callback_handler = handler

    def open_init_watch(self, init_id: str) -> InitWatch:
        """Register the watch for ``init_id`` before writing ``mobkit/init``."""
        loop = self._loop if self._loop is not None else asyncio.get_running_loop()
        watch = InitWatch(init_id, loop)
        with self._pending_lock:
            self._init_watch = watch
            failure = getattr(self, "_reader_failure", None)
        if failure is not None:
            watch._fail(failure)
        return watch

    def close_init_watch(self, watch: InitWatch) -> None:
        with self._pending_lock:
            if self._init_watch is watch:
                self._init_watch = None

    @property
    def request_timeout(self) -> float:
        """Default timeout, in seconds, for one outbound RPC request."""
        return self._timeout

    def start(self) -> None:
        if self._process is not None and self._process.poll() is None:
            return
        # Capture event loop for async callback dispatch
        try:
            loop = asyncio.get_running_loop()
        except RuntimeError:
            loop = None
        # Default: the child gateway INHERITS this process's stderr (None).
        # The old DEVNULL default silently discarded tracing, panic hooks,
        # and migration progress — a week of panic lines went to /dev/null in
        # one production fleet, and a supervisor aborted a deploy because a
        # working migration looked like a hang. Opt out explicitly with
        # MOBKIT_GATEWAY_STDERR=devnull, or redirect to a file with
        # MOBKIT_GATEWAY_STDERR_FILE=<path>.
        stderr_target: Any = None
        stderr_file = None
        stderr_path = self._env.get("MOBKIT_GATEWAY_STDERR_FILE", "").strip()
        if stderr_path:
            stderr_file = open(stderr_path, "ab", buffering=0)
            stderr_target = stderr_file
        elif self._env.get("MOBKIT_GATEWAY_STDERR", "").strip().lower() == "devnull":
            stderr_target = subprocess.DEVNULL

        process = subprocess.Popen(
            [self.gateway_bin, "--persistent"],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=stderr_target,
            env=self._env,
        )
        failure = TransportReaderFailedError("the gateway process was replaced")
        events = []
        watch = None
        with self._pending_lock:
            # A caller may restart an exited child before its reader sees EOF.
            # Finish old waiters against their own maps before publishing the
            # replacement. Already received replies keep their exact outcome.
            if self._process is not None or self._reader_failure is not None:
                for msg_id in self._pending:
                    self._results.setdefault(msg_id, failure)
                events = list(self._pending.values())
                watch = self._init_watch
                self._pending = {}
                self._results = {}
                self._init_watch = None
            self._process = process
            self._stderr_file = stderr_file
            self._loop = loop
            self._reader_failure = None
            # Capabilities and streams belong to this exact child.
            self._supports_shutdown_handshake = False
            self._shutdown_horizon_seconds = _GATEWAY_SHUTDOWN_GRACE_SECONDS
        for event in events:
            event.set()
        if watch is not None:
            watch._fail(failure)
        self._reader_thread = threading.Thread(
            target=self._reader_loop, args=(process,), daemon=True, name="mobkit-reader"
        )
        self._reader_thread.start()

    def _reader_loop(self, process=_CURRENT_TRANSPORT) -> None:
        if process is _CURRENT_TRANSPORT:
            process = self._process
        assert process is not None and process.stdout is not None
        reason = "the gateway closed its stdout"
        try:
            while True:
                line = process.stdout.readline()
                if not line or process is not self._process:
                    break
                self._read_line(line, process=process)
        except Exception as exc:  # noqa: BLE001 - every exit must fail waiters
            reason = f"reader failed: {exc!r}"
            _log.error("transport: reader stopped: %s", reason, exc_info=True)
        finally:
            self._fail_pending_after_reader_exit(reason, process=process)

    def _read_line(self, line: bytes, *, process=_CURRENT_TRANSPORT) -> None:
        """Handle one stdout line. A bad line is skipped, never fatal."""
        if process is _CURRENT_TRANSPORT:
            process = self._process
        if process is not self._process:
            return
        try:
            msg = json.loads(line.decode("utf-8"))
        except (UnicodeDecodeError, json.JSONDecodeError):
            _log.warning("transport: skipping non-JSON line from subprocess: %r", line[:200])
            return
        if not isinstance(msg, dict):
            _log.warning(
                "transport: skipping JSON line that is not an object: %r", line[:200]
            )
            return

        if "method" in msg:
            method = msg.get("method")
            if "id" not in msg and method in (_INIT_PROGRESS_METHOD, _INIT_SETTLED_METHOD):
                self._deliver_init_event(method, msg.get("params"), process=process)
                return
            # Callback or notification FROM Rust
            self._handle_callback(msg, process=process)
        elif "id" in msg:
            # Response to a pending request
            msg_id = str(msg["id"])
            with self._pending_lock:
                if process is not self._process:
                    return
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

    def _deliver_init_event(self, method: str, params: Any, *, process=_CURRENT_TRANSPORT) -> None:
        if process is _CURRENT_TRANSPORT:
            process = self._process
        if not isinstance(params, dict):
            _log.warning("transport: %s without object params; ignored", method)
            return
        with self._pending_lock:
            if process is not self._process:
                return
            watch = self._init_watch
        if watch is None or params.get("init_id") != watch.init_id:
            # Not the init this process is running (a late or foreign line).
            _log.debug("transport: %s for an init that is not in flight; ignored", method)
            return
        watch._deliver(method, params)

    def _fail_pending_after_reader_exit(self, reason: str, *, process=_CURRENT_TRANSPORT) -> None:
        """No response can arrive any more: fail every waiter, typed.

        A response that arrived before the reader stopped (for example a
        fail-closed init error written just before the gateway exits) is
        kept for its waiter.
        """
        if process is _CURRENT_TRANSPORT:
            process = self._process
        failure = TransportReaderFailedError(reason)
        with self._pending_lock:
            if process is not self._process:
                return
            self._reader_failure = failure
            for msg_id in self._pending:
                self._results.setdefault(msg_id, failure)
            events = list(self._pending.values())
            watch = getattr(self, "_init_watch", None)
        for event in events:
            event.set()
        if watch is not None:
            watch._fail(failure)

    def _handle_callback(self, msg: dict, *, process=_CURRENT_TRANSPORT) -> None:
        """Dispatch callback in a separate thread so the reader loop is not blocked."""
        if process is _CURRENT_TRANSPORT:
            process = self._process
        with self._pending_lock:
            if process is not self._process:
                return
            handler, loop = self._callback_handler, self._loop
        if handler is None:
            method = msg.get("method")
            _log.warning(
                "transport: received callback but no handler registered: %s",
                method,
            )
            callback_id = msg.get("id")
            if callback_id is not None:
                # Answer now: the gateway would otherwise wait out its full
                # callback deadline for a response that never comes.
                try:
                    self._write_line(
                        {
                            "jsonrpc": "2.0",
                            "id": callback_id,
                            "error": {
                                "code": -32000,
                                "message": f"no callback handler is registered for {method}",
                                "data": {
                                    "kind": "callback_handler_unavailable",
                                    "method": method,
                                },
                            },
                        },
                        callback_response=True,
                        process=process,
                    )
                except Exception:
                    _log.error(
                        "failed to send callback error response for id=%s", callback_id
                    )
            return
        # Dispatch in a daemon thread to avoid blocking the reader loop
        t = threading.Thread(
            target=self._dispatch_callback, args=(msg,), daemon=True,
            kwargs={"process": process, "handler": handler, "loop": loop},
            name="mobkit-callback",
        )
        t.start()

    def _dispatch_callback(
        self, msg: dict, *, process=_CURRENT_TRANSPORT,
        handler=_CURRENT_TRANSPORT, loop=_CURRENT_TRANSPORT,
    ) -> None:
        if process is _CURRENT_TRANSPORT:
            process = self._process
        if handler is _CURRENT_TRANSPORT:
            handler = self._callback_handler
        if loop is _CURRENT_TRANSPORT:
            loop = self._loop
        if process is not self._process:
            return
        method = msg.get("method", "")
        params = msg.get("params", {})
        callback_id = msg.get("id")  # None for notifications
        try:
            if loop is not None and loop.is_running():
                async def invoke_current_handler():
                    # Scheduling on the captured loop can outlive the child.
                    # Check again before any host handler body starts.
                    if process is not self._process:
                        return None
                    return await handler(method, params)

                future = asyncio.run_coroutine_threadsafe(
                    invoke_current_handler(), loop
                )
                try:
                    result = future.result(
                        timeout=_PROVIDER_CALLBACK_COMPLETION_SECONDS
                    )
                except FutureTimeoutError as exc:
                    # `Future.result()` also propagates a TimeoutError raised
                    # by the provider itself. A completed future therefore
                    # represents the provider's own failure, not exhaustion
                    # of the host wait, and must retain its original error.
                    if future.done():
                        raise
                    # The ordinary request timeout is intentionally unrelated
                    # to provider execution. If the dedicated host deadline is
                    # exhausted, cancel the event-loop coroutine before Rust's
                    # 130-second wire deadline so a timed-out provider cannot
                    # continue toward a late authority commit unnoticed.
                    future.cancel()
                    raise TimeoutError(
                        "provider callback exceeded its 125s host completion deadline"
                    ) from exc
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
            if _contains_non_finite(response["result"]):
                # JSON has no NaN or Infinity. Sending the token would make
                # the gateway drop the line and wait out its deadline;
                # sending null would change the provider's answer. Refuse it.
                self._write_line(
                    {
                        "jsonrpc": "2.0",
                        "id": callback_id,
                        "error": {
                            "code": -32000,
                            "message": (
                                f"{method} returned a non-finite number (NaN or "
                                "Infinity), which JSON cannot carry"
                            ),
                            "data": {"kind": "non_finite_result"},
                        },
                    },
                    callback_response=True,
                    process=process,
                )
                return
            self._write_line(response, callback_response=True, process=process)
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
                self._write_line(error_response, callback_response=True, process=process)
            except Exception:
                _log.error("failed to send callback error response for id=%s", callback_id)

    def _write_line(
        self, obj: dict, *, callback_response: bool = False,
        process=_CURRENT_TRANSPORT,
    ) -> None:
        """Write one JSON line to the gateway's stdin.

        Lines never interleave. A callback response (``callback_response``)
        waits only for the line currently being written; queued requests wait
        until no callback response is waiting. One pipe cannot do better: a
        line already being written must finish first.
        """
        if process is _CURRENT_TRANSPORT:
            process = self._process
        # Strict JSON, encoded before taking a turn: the gateway cannot parse
        # NaN/Infinity tokens, and a refusal must not hold the pipe.
        data = (json.dumps(obj, allow_nan=False) + "\n").encode("utf-8")
        with self._write_cond:
            if callback_response:
                self._priority_writers_waiting += 1
            try:
                while self._writing or (
                    not callback_response and self._priority_writers_waiting
                ):
                    self._write_cond.wait()
            finally:
                if callback_response:
                    self._priority_writers_waiting -= 1
            self._writing = True
        try:
            # A callback may finish, or wait for the writer, across restart.
            # Its reply belongs only to the child that issued the invocation.
            if process is not self._process:
                return
            if process and process.stdin:
                process.stdin.write(data)
                process.stdin.flush()
        finally:
            with self._write_cond:
                self._writing = False
                self._write_cond.notify_all()

    def send_sync(
        self,
        request: dict[str, Any],
        *,
        timeout: float | None = None,
    ) -> Any:
        self._ensure_running()
        with self._pending_lock:
            process = self._process
        response = self._send_sync_running(request, timeout=timeout, process=process)
        if request.get("method") == "mobkit/init":
            result = response.get("result") if isinstance(response, dict) else None
            supports_shutdown_handshake = bool(
                isinstance(result, dict)
                and result.get("stdio_shutdown_handshake") is True
            )
            shutdown_horizon = _GATEWAY_SHUTDOWN_GRACE_SECONDS
            if supports_shutdown_handshake and isinstance(result, dict):
                horizon_ms = result.get("stdio_shutdown_horizon_ms")
                if (
                    isinstance(horizon_ms, int)
                    and not isinstance(horizon_ms, bool)
                    and 0 < horizon_ms <= _MAX_GATEWAY_SHUTDOWN_HORIZON_MS
                ):
                    shutdown_horizon = horizon_ms / 1000.0
            with self._pending_lock:
                if process is self._process:
                    self._supports_shutdown_handshake = supports_shutdown_handshake
                    self._shutdown_horizon_seconds = shutdown_horizon
        return response

    def _send_sync_running(
        self,
        request: dict[str, Any],
        *,
        timeout: float | None = None,
        process=_CURRENT_TRANSPORT,
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
            if process is _CURRENT_TRANSPORT:
                process = self._process
            elif process is not self._process:
                raise TransportReaderFailedError("the gateway process was replaced")
            reader_failure = getattr(self, "_reader_failure", None)
            if reader_failure is not None:
                raise reader_failure
            pending, results = self._pending, self._results
            if msg_id in pending:
                raise ValueError(
                    f"persistent transport: request id {msg_id!r} is already "
                    f"in flight; concurrent callers must use distinct ids"
                )
            pending[msg_id] = event
        try:
            # Admission, write, response and cleanup belong to this child.
            self._write_line(request, process=process)
            if not event.wait(timeout=request_timeout):
                raise RuntimeError(
                    f"persistent transport: timeout after {request_timeout}s "
                    "waiting for response"
                )
            with self._pending_lock:
                result = results.get(msg_id)
        finally:
            with self._pending_lock:
                pending.pop(msg_id, None)
                results.pop(msg_id, None)
        if isinstance(result, TransportReaderFailedError):
            raise result
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
            process=process,
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
            or result.get("runtime_cleanup_completed") is not True
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
        with self._pending_lock:
            process = getattr(self, "_process", None)
            stderr_file = getattr(self, "_stderr_file", None)
            supports_shutdown_handshake = getattr(self, "_supports_shutdown_handshake", False)
            shutdown_horizon = getattr(
                self,
                "_shutdown_horizon_seconds",
                _GATEWAY_SHUTDOWN_GRACE_SECONDS,
            )
        if process is None:
            return
        shutdown_error: Exception | None = None
        process_reaped = False
        reap_error: Exception | None = None
        shutdown_started = time.monotonic()
        try:
            # The gateway may need to round-trip lease/continuity provider
            # callbacks while UnifiedRuntime shuts down. Keep stdin open until
            # a capable gateway acknowledges that cleanup is complete. Older
            # or custom gateways stay on the EOF protocol below.
            if supports_shutdown_handshake:
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
                process_reaped = True
            except (OSError, subprocess.TimeoutExpired):
                try:
                    process_reaped = process.poll() is not None
                except OSError:
                    process_reaped = False
                if not process_reaped:
                    try:
                        process.terminate()
                    except OSError:
                        # The child can exit between the timed wait and signal.
                        pass
                    try:
                        process.wait(timeout=_PROCESS_TERMINATE_GRACE_SECONDS)
                        process_reaped = True
                    except (OSError, subprocess.TimeoutExpired):
                        try:
                            process_reaped = process.poll() is not None
                        except OSError:
                            process_reaped = False
                        if not process_reaped:
                            try:
                                process.kill()
                            except OSError:
                                pass
                            try:
                                process.wait(timeout=_PROCESS_KILL_GRACE_SECONDS)
                                process_reaped = True
                            except (OSError, subprocess.TimeoutExpired) as exc:
                                try:
                                    process_reaped = process.poll() is not None
                                except OSError:
                                    process_reaped = False
                                if not process_reaped:
                                    # Keep teardown bounded, but never attest
                                    # success or discard ownership while the
                                    # child remains live.
                                    reap_error = exc
        finally:
            if process_reaped:
                self._fail_pending_after_reader_exit(
                    "the gateway process stopped", process=process,
                )
                with self._pending_lock:
                    if self._process is process:
                        self._process = None
                    if self._stderr_file is stderr_file:
                        self._stderr_file = None
                if stderr_file is not None:
                    stderr_file.close()
        if reap_error is not None:
            raise RuntimeError(
                "persistent transport: gateway process did not terminate after bounded cleanup"
            ) from reap_error
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
