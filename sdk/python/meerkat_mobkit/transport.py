"""Persistent subprocess transport for MobKit JSON-RPC."""
from __future__ import annotations

import asyncio
import json
import os
import subprocess
import threading
from typing import Any


class PersistentTransport:
    """Long-lived mobkit-rpc subprocess communicating over stdin/stdout JSON-RPC.

    Unlike the per-call subprocess transport, this keeps the process alive
    so mob state persists across calls. stderr is sent to devnull to avoid
    backpressure deadlocks.
    """

    def __init__(self, gateway_bin: str, *, env: dict[str, str] | None = None):
        self.gateway_bin = gateway_bin
        self._env = {**os.environ, **(env or {})}
        self._process: subprocess.Popen[bytes] | None = None
        self._lock = threading.Lock()

    def start(self) -> None:
        if self._process is not None and self._process.poll() is None:
            return
        self._process = subprocess.Popen(
            [self.gateway_bin, "--persistent"],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL,
            env=self._env,
        )

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

    def send_sync(self, request: dict[str, Any]) -> Any:
        with self._lock:
            self._ensure_running()
            assert self._process is not None
            assert self._process.stdin is not None
            assert self._process.stdout is not None

            request_line = json.dumps(request) + "\n"
            self._process.stdin.write(request_line.encode("utf-8"))
            self._process.stdin.flush()

            response_line = self._process.stdout.readline()
            if not response_line:
                raise RuntimeError("persistent transport: subprocess closed stdout")

            try:
                return json.loads(response_line.decode("utf-8"))
            except json.JSONDecodeError as exc:
                raise ValueError("persistent transport: non-JSON response") from exc

    async def send_async(self, request: dict[str, Any]) -> Any:
        return await asyncio.to_thread(self.send_sync, request)

    def _ensure_running(self) -> None:
        if not self.is_running():
            self.start()

    def __del__(self) -> None:
        self.stop()


def create_persistent_transport(gateway_bin: str, **kwargs: Any) -> PersistentTransport:
    transport = PersistentTransport(gateway_bin, **kwargs)
    transport.start()
    return transport
