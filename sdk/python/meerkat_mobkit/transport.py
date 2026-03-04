"""Persistent subprocess transport for MobKit JSON-RPC."""
from __future__ import annotations

import asyncio
import json
import logging
import os
import subprocess
import threading
from typing import Any, Callable

_log = logging.getLogger("meerkat_mobkit")


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

    def set_callback_handler(self, handler: Callable) -> None:
        self._callback_handler = handler

    def start(self) -> None:
        if self._process is not None and self._process.poll() is None:
            return
        # Capture event loop for async callback dispatch
        try:
            self._loop = asyncio.get_running_loop()
        except RuntimeError:
            self._loop = None
        self._process = subprocess.Popen(
            [self.gateway_bin, "--persistent"],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL,
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

            if "method" in msg and "id" in msg:
                # Callback FROM Rust
                self._handle_callback(msg)
            elif "id" in msg:
                # Response to a pending request
                msg_id = str(msg["id"])
                with self._pending_lock:
                    self._results[msg_id] = msg
                    event = self._pending.get(msg_id)
                if event:
                    event.set()
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
        callback_id = msg.get("id")
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
            # Ensure result is JSON-serializable before building response.
            # Tools or other callback results may contain non-serializable objects;
            # sanitize them to strings to prevent json.dumps failures in _write_line.
            response = {"jsonrpc": "2.0", "id": callback_id, "result": _sanitize_for_json(result)}
            self._write_line(response)
        except Exception as exc:
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

    def send_sync(self, request: dict[str, Any]) -> Any:
        self._ensure_running()
        msg_id = str(request.get("id", ""))
        event = threading.Event()
        with self._pending_lock:
            self._pending[msg_id] = event
        # Write request (lock only for write, release before wait)
        self._write_line(request)
        # Wait for response — no locks held
        if not event.wait(timeout=self._timeout):
            with self._pending_lock:
                self._pending.pop(msg_id, None)
                self._results.pop(msg_id, None)
            raise RuntimeError(
                f"persistent transport: timeout after {self._timeout}s waiting for response"
            )
        with self._pending_lock:
            self._pending.pop(msg_id, None)
            result = self._results.pop(msg_id, None)
        if result is None:
            raise RuntimeError("persistent transport: subprocess closed stdout")
        return result

    async def send_async(self, request: dict[str, Any]) -> Any:
        return await asyncio.to_thread(self.send_sync, request)

    def stop(self) -> None:
        if self._process is None:
            return
        try:
            if self._process.stdin:
                self._process.stdin.close()
            self._process.wait(timeout=5)
        except Exception:
            self._process.kill()
        finally:
            self._process = None

    def is_running(self) -> bool:
        return self._process is not None and self._process.poll() is None

    def _ensure_running(self) -> None:
        if not self.is_running():
            self.start()

    def __del__(self) -> None:
        self.stop()


def create_persistent_transport(gateway_bin: str, **kwargs: Any) -> PersistentTransport:
    transport = PersistentTransport(gateway_bin, **kwargs)
    transport.start()
    return transport
