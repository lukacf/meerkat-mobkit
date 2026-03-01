from __future__ import annotations

from urllib.parse import quote


def build_console_modules_route(auth_token: str | None = None) -> str:
    if not auth_token:
        return "/console/modules"
    return f"/console/modules?auth_token={quote(auth_token, safe='')}"


def define_module_spec(
    *,
    module_id: str,
    command: str,
    args: list[str] | None = None,
    restart_policy: str = "never",
) -> dict[str, object]:
    return {
        "id": module_id,
        "command": command,
        "args": args or [],
        "restart_policy": restart_policy,
    }
