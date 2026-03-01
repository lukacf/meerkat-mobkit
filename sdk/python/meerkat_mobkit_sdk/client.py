from __future__ import annotations

import json
import os
import subprocess
from typing import Any


class MobkitTypedClient:
    def __init__(self, gateway_bin: str):
        self.gateway_bin = gateway_bin

    def rpc(self, request_id: str, method: str, params: dict[str, Any]) -> dict[str, Any]:
        request = json.dumps(
            {
                "jsonrpc": "2.0",
                "id": request_id,
                "method": method,
                "params": params,
            }
        )
        proc = subprocess.run(
            [self.gateway_bin],
            check=False,
            capture_output=True,
            text=True,
            env={**os.environ, "MOBKIT_RPC_REQUEST": request},
        )
        if proc.returncode != 0:
            raise RuntimeError(
                f"gateway failed (status={proc.returncode}): {proc.stderr.strip()}"
            )

        payload = json.loads(proc.stdout)
        if not isinstance(payload, dict):
            raise ValueError("invalid JSON-RPC response envelope")

        if payload.get("jsonrpc") != "2.0" or payload.get("id") != request_id:
            raise ValueError("invalid JSON-RPC response envelope")

        has_result = "result" in payload
        has_error = "error" in payload
        if has_result == has_error:
            raise ValueError("invalid JSON-RPC response envelope")

        if has_error:
            error = payload.get("error")
            if not isinstance(error, dict):
                raise ValueError("invalid JSON-RPC response envelope")

            code = error.get("code")
            message = error.get("message")
            if not isinstance(code, int) or isinstance(code, bool) or not isinstance(message, str):
                raise ValueError("invalid JSON-RPC response envelope")

        return payload
