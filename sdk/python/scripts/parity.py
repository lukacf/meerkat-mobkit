#!/usr/bin/env python3

from __future__ import annotations

import json
import os
import sys
from pathlib import Path
from typing import Any, Callable

try:
    from meerkat_mobkit._client import MobkitTypedClient
    from meerkat_mobkit.helpers import build_console_modules_route, define_module_spec
except ModuleNotFoundError:
    sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
    from meerkat_mobkit._client import MobkitTypedClient  # type: ignore[no-redef]
    from meerkat_mobkit.helpers import build_console_modules_route, define_module_spec  # type: ignore[no-redef]


def main() -> int:
    gateway_bin = os.environ.get("MOBKIT_RPC_GATEWAY_BIN")
    if not gateway_bin:
        print("MOBKIT_RPC_GATEWAY_BIN must be set", file=sys.stderr)
        return 2

    checks: list[dict[str, Any]] = []

    def check(name: str, fn: Callable[[], None]) -> None:
        try:
            fn()
            checks.append({"name": name, "ok": True})
        except Exception as exc:  # broad for per-check reporting
            checks.append({"name": name, "ok": False, "error": str(exc)})

    client = MobkitTypedClient(gateway_bin)

    check("typed client status success", lambda: _assert_eq(
        client.rpc("py-status", "mobkit/status", {}),
        {
            "jsonrpc": "2.0",
            "id": "py-status",
            "result": {
                "contract_version": "0.1.0",
                "running": True,
                "loaded_modules": ["routing"],
            },
        },
        "unexpected status",
    ))

    def _caps_check() -> None:
        response = client.rpc("py-caps", "mobkit/capabilities", {})
        methods = response.get("result", {}).get("methods")
        if not isinstance(methods, list) or "mobkit/events/subscribe" not in methods:
            raise AssertionError(f"unexpected capabilities methods: {response}")

    check("typed client capabilities success", _caps_check)

    check("typed client invalid params exact json-rpc error", lambda: _assert_eq(
        client.rpc("py-invalid", "mobkit/spawn_member", {}),
        {
            "jsonrpc": "2.0",
            "id": "py-invalid",
            "error": {
                "code": -32602,
                "message": "Invalid params: module_id required",
            },
        },
        "unexpected invalid params error shape",
    ))

    check("typed client unloaded module exact json-rpc error", lambda: _assert_eq(
        client.rpc("py-unloaded", "delivery/tools.list", {"probe": "parity"}),
        {
            "jsonrpc": "2.0",
            "id": "py-unloaded",
            "error": {
                "code": -32601,
                "message": "Module 'delivery' not loaded",
            },
        },
        "unexpected unloaded error shape",
    ))

    def _console_check() -> None:
        route = build_console_modules_route("token+/=?")
        if route != "/console/modules?auth_token=token%2B%2F%3D%3F":
            raise AssertionError(f"unexpected console route: {route}")

    check("console route helper encodes auth token", _console_check)

    check("module-authoring helper normalizes schema", lambda: _assert_eq(
        define_module_spec(
            module_id="router",
            command="python3",
            args=["router.py"],
            restart_policy="on_failure",
        ),
        {
            "id": "router",
            "command": "python3",
            "args": ["router.py"],
            "restart_policy": "on_failure",
        },
        "unexpected module spec",
    ))

    failed = sum(1 for c in checks if not c["ok"])
    passed = len(checks) - failed
    print(json.dumps({"sdk": "python", "passed": passed, "failed": failed, "checks": checks}), end="")
    return 0 if failed == 0 else 1


def _assert_eq(observed: Any, expected: Any, label: str) -> None:
    if observed != expected:
        raise AssertionError(f"{label}: observed={observed} expected={expected}")


if __name__ == "__main__":
    raise SystemExit(main())
