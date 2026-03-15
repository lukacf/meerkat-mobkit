from __future__ import annotations

import asyncio
import json
import os
import subprocess
from typing import Any, Callable, Literal, Mapping, Protocol, TypedDict, cast
from urllib import request as urllib_request
from urllib.error import HTTPError, URLError


class JsonRpcRequest(TypedDict):
    jsonrpc: Literal["2.0"]
    id: str
    method: str
    params: dict[str, Any]


class JsonRpcSuccess(TypedDict):
    jsonrpc: Literal["2.0"]
    id: str
    result: Any


class JsonRpcErrorBody(TypedDict):
    code: int
    message: str


class JsonRpcErrorResponse(TypedDict):
    jsonrpc: Literal["2.0"]
    id: str
    error: JsonRpcErrorBody


JsonRpcResponse = JsonRpcSuccess | JsonRpcErrorResponse


class MobkitModelsCatalogResult(TypedDict):
    models: list[dict[str, Any]]
    provider_defaults: list[dict[str, Any]]


class MobkitStatusResult(TypedDict):
    contract_version: str
    running: bool
    loaded_modules: list[str]


class MobkitCapabilitiesResult(TypedDict):
    contract_version: str
    methods: list[str]
    loaded_modules: list[str]


class MobkitReconcileResult(TypedDict):
    accepted: bool
    reconciled_modules: list[str]
    added: int


class MobkitSpawnMemberResult(TypedDict):
    accepted: bool
    module_id: str


class MobkitSubscribeKeepAlive(TypedDict):
    interval_ms: int
    event: str


class MobkitSubscribeEventEnvelope(TypedDict):
    event_id: str
    source: str
    timestamp_ms: int
    event: Any


class MobkitSubscribeResult(TypedDict):
    scope: Literal["mob", "agent", "interaction"]
    replay_from_event_id: str | None
    keep_alive: MobkitSubscribeKeepAlive
    keep_alive_comment: str
    event_frames: list[str]
    events: list[MobkitSubscribeEventEnvelope]


class MobkitSubscribeParams(TypedDict, total=False):
    scope: Literal["mob", "agent", "interaction"]
    last_event_id: str
    agent_id: str


class AsyncRpcTransport(Protocol):
    async def __call__(self, request: JsonRpcRequest) -> Any:
        ...


class SyncRpcTransport(Protocol):
    def __call__(self, request: JsonRpcRequest) -> Any:
        ...


class MobkitRpcError(RuntimeError):
    def __init__(self, code: int, message: str, request_id: str, method: str):
        super().__init__(message)
        self.code = code
        self.request_id = request_id
        self.method = method


def create_gateway_sync_transport(gateway_bin: str) -> SyncRpcTransport:
    def transport(request: JsonRpcRequest) -> Any:
        request_json = json.dumps(request)
        proc = subprocess.run(
            [gateway_bin],
            check=False,
            capture_output=True,
            text=True,
            env={**os.environ, "MOBKIT_RPC_REQUEST": request_json},
        )
        if proc.returncode != 0:
            raise RuntimeError(
                f"gateway failed (status={proc.returncode}): {proc.stderr.strip()}"
            )

        try:
            return json.loads(proc.stdout)
        except json.JSONDecodeError as exc:
            raise ValueError("gateway returned non-JSON response") from exc

    return transport


def create_gateway_async_transport(gateway_bin: str) -> AsyncRpcTransport:
    async def transport(request: JsonRpcRequest) -> Any:
        request_json = json.dumps(request)
        proc = await asyncio.create_subprocess_exec(
            gateway_bin,
            env={**os.environ, "MOBKIT_RPC_REQUEST": request_json},
            stdout=asyncio.subprocess.PIPE,
            stderr=asyncio.subprocess.PIPE,
        )
        stdout, stderr = await proc.communicate()
        if proc.returncode != 0:
            stderr_text = stderr.decode("utf-8", errors="replace").strip()
            raise RuntimeError(
                f"gateway failed (status={proc.returncode}): {stderr_text}"
            )

        try:
            return json.loads(stdout.decode("utf-8"))
        except json.JSONDecodeError as exc:
            raise ValueError("gateway returned non-JSON response") from exc

    return transport


def create_http_transport(
    endpoint: str,
    *,
    headers: Mapping[str, str] | None = None,
    timeout_seconds: float = 10.0,
) -> AsyncRpcTransport:
    base_headers = {"content-type": "application/json", "accept": "application/json"}
    if headers:
        base_headers.update(dict(headers))

    async def transport(request: JsonRpcRequest) -> Any:
        request_bytes = json.dumps(request).encode("utf-8")
        http_request = urllib_request.Request(
            endpoint,
            data=request_bytes,
            method="POST",
            headers=base_headers,
        )
        try:
            body = await asyncio.to_thread(_read_http_body, http_request, timeout_seconds)
        except HTTPError as exc:
            body = exc.read().decode("utf-8", errors="replace")
            raise RuntimeError(
                f"http transport failed (status={exc.code}): {body}"
            ) from exc
        except URLError as exc:
            raise RuntimeError(f"http transport failed: {exc.reason}") from exc

        try:
            return json.loads(body)
        except json.JSONDecodeError as exc:
            raise ValueError("http transport returned non-JSON response") from exc

    return transport


class MobkitTypedClient:
    def __init__(self, gateway_bin: str):
        self.gateway_bin = gateway_bin
        self._sync_transport = create_gateway_sync_transport(gateway_bin)

    @classmethod
    def from_persistent(cls, transport: SyncRpcTransport) -> "MobkitTypedClient":
        instance = cls.__new__(cls)
        instance.gateway_bin = ""
        instance._sync_transport = transport
        return instance

    def rpc(
        self, request_id: str, method: str, params: Mapping[str, Any] | None = None
    ) -> JsonRpcResponse:
        payload = self._sync_transport(_build_request(request_id, method, params))
        return _parse_json_rpc_response(payload, request_id)

    def status(self, request_id: str = "status") -> MobkitStatusResult:
        return cast(
            MobkitStatusResult,
            _unwrap_typed_result(
                self.rpc(request_id, "mobkit/status", {}),
                request_id,
                "mobkit/status",
                _is_status_result,
            ),
        )

    def capabilities(self, request_id: str = "capabilities") -> MobkitCapabilitiesResult:
        return cast(
            MobkitCapabilitiesResult,
            _unwrap_typed_result(
                self.rpc(request_id, "mobkit/capabilities", {}),
                request_id,
                "mobkit/capabilities",
                _is_capabilities_result,
            ),
        )

    def reconcile(
        self, modules: list[str], request_id: str = "reconcile"
    ) -> MobkitReconcileResult:
        return cast(
            MobkitReconcileResult,
            _unwrap_typed_result(
                self.rpc(request_id, "mobkit/reconcile", {"modules": modules}),
                request_id,
                "mobkit/reconcile",
                _is_reconcile_result,
            ),
        )

    def spawn_member(
        self, module_id: str, request_id: str = "spawn_member"
    ) -> MobkitSpawnMemberResult:
        return cast(
            MobkitSpawnMemberResult,
            _unwrap_typed_result(
                self.rpc(request_id, "mobkit/spawn_member", {"module_id": module_id}),
                request_id,
                "mobkit/spawn_member",
                _is_spawn_member_result,
            ),
        )

    def subscribe_events(
        self,
        params: MobkitSubscribeParams | None = None,
        request_id: str = "events_subscribe",
    ) -> MobkitSubscribeResult:
        return cast(
            MobkitSubscribeResult,
            _unwrap_typed_result(
                self.rpc(
                    request_id,
                    "mobkit/events/subscribe",
                    dict(params) if params is not None else {},
                ),
                request_id,
                "mobkit/events/subscribe",
                _is_subscribe_result,
            ),
        )

    def models_catalog(
        self, request_id: str = "models_catalog"
    ) -> MobkitModelsCatalogResult:
        return cast(
            MobkitModelsCatalogResult,
            _unwrap_typed_result(
                self.rpc(request_id, "mobkit/models/catalog", {}),
                request_id,
                "mobkit/models/catalog",
                _is_models_catalog_result,
            ),
        )


class MobkitAsyncTypedClient:
    def __init__(self, transport: AsyncRpcTransport):
        self._transport = transport

    @classmethod
    def from_gateway_bin(cls, gateway_bin: str) -> "MobkitAsyncTypedClient":
        return cls(create_gateway_async_transport(gateway_bin))

    @classmethod
    def from_http(
        cls,
        endpoint: str,
        *,
        headers: Mapping[str, str] | None = None,
        timeout_seconds: float = 10.0,
    ) -> "MobkitAsyncTypedClient":
        return cls(
            create_http_transport(
                endpoint,
                headers=headers,
                timeout_seconds=timeout_seconds,
            )
        )

    async def rpc(
        self, request_id: str, method: str, params: Mapping[str, Any] | None = None
    ) -> JsonRpcResponse:
        payload = await self._transport(_build_request(request_id, method, params))
        return _parse_json_rpc_response(payload, request_id)

    async def request(
        self,
        request_id: str,
        method: str,
        params: Mapping[str, Any] | None,
        validator: Callable[[Any], bool],
    ) -> Any:
        response = await self.rpc(request_id, method, params)
        return _unwrap_typed_result(response, request_id, method, validator)

    async def status(self, request_id: str = "status") -> MobkitStatusResult:
        return cast(
            MobkitStatusResult,
            await self.request(request_id, "mobkit/status", {}, _is_status_result),
        )

    async def capabilities(
        self, request_id: str = "capabilities"
    ) -> MobkitCapabilitiesResult:
        return cast(
            MobkitCapabilitiesResult,
            await self.request(
                request_id,
                "mobkit/capabilities",
                {},
                _is_capabilities_result,
            ),
        )

    async def reconcile(
        self, modules: list[str], request_id: str = "reconcile"
    ) -> MobkitReconcileResult:
        return cast(
            MobkitReconcileResult,
            await self.request(
                request_id,
                "mobkit/reconcile",
                {"modules": modules},
                _is_reconcile_result,
            ),
        )

    async def spawn_member(
        self, module_id: str, request_id: str = "spawn_member"
    ) -> MobkitSpawnMemberResult:
        return cast(
            MobkitSpawnMemberResult,
            await self.request(
                request_id,
                "mobkit/spawn_member",
                {"module_id": module_id},
                _is_spawn_member_result,
            ),
        )

    async def subscribe_events(
        self,
        params: MobkitSubscribeParams | None = None,
        request_id: str = "events_subscribe",
    ) -> MobkitSubscribeResult:
        return cast(
            MobkitSubscribeResult,
            await self.request(
                request_id,
                "mobkit/events/subscribe",
                dict(params) if params is not None else {},
                _is_subscribe_result,
            ),
        )

    async def models_catalog(
        self, request_id: str = "models_catalog"
    ) -> MobkitModelsCatalogResult:
        return cast(
            MobkitModelsCatalogResult,
            await self.request(
                request_id,
                "mobkit/models/catalog",
                {},
                _is_models_catalog_result,
            ),
        )


def _read_http_body(http_request: urllib_request.Request, timeout_seconds: float) -> str:
    with urllib_request.urlopen(http_request, timeout=timeout_seconds) as response:
        return response.read().decode("utf-8")


def _build_request(
    request_id: str,
    method: str,
    params: Mapping[str, Any] | None,
) -> JsonRpcRequest:
    return {
        "jsonrpc": "2.0",
        "id": request_id,
        "method": method,
        "params": dict(params) if params is not None else {},
    }


def _parse_json_rpc_response(payload: Any, request_id: str) -> JsonRpcResponse:
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
        if not isinstance(code, int) or isinstance(code, bool):
            raise ValueError("invalid JSON-RPC response envelope")
        if not isinstance(message, str):
            raise ValueError("invalid JSON-RPC response envelope")

    return cast(JsonRpcResponse, payload)


def _unwrap_typed_result(
    response: JsonRpcResponse,
    request_id: str,
    method: str,
    validator: Callable[[Any], bool],
) -> Any:
    if "error" in response:
        error = response["error"]
        raise MobkitRpcError(error["code"], error["message"], request_id, method)

    result = response["result"]
    if not validator(result):
        raise ValueError(f"invalid result payload for {method}")
    return result


def _is_status_result(value: Any) -> bool:
    return (
        isinstance(value, dict)
        and isinstance(value.get("contract_version"), str)
        and isinstance(value.get("running"), bool)
        and _is_string_list(value.get("loaded_modules"))
    )


def _is_capabilities_result(value: Any) -> bool:
    return (
        isinstance(value, dict)
        and isinstance(value.get("contract_version"), str)
        and _is_string_list(value.get("methods"))
        and _is_string_list(value.get("loaded_modules"))
    )


def _is_reconcile_result(value: Any) -> bool:
    return (
        isinstance(value, dict)
        and isinstance(value.get("accepted"), bool)
        and _is_string_list(value.get("reconciled_modules"))
        and isinstance(value.get("added"), int)
        and not isinstance(value.get("added"), bool)
    )


def _is_spawn_member_result(value: Any) -> bool:
    return (
        isinstance(value, dict)
        and isinstance(value.get("accepted"), bool)
        and isinstance(value.get("module_id"), str)
    )


def _is_subscribe_result(value: Any) -> bool:
    if not isinstance(value, dict):
        return False

    scope = value.get("scope")
    if scope not in {"mob", "agent", "interaction"}:
        return False

    replay = value.get("replay_from_event_id")
    if replay is not None and not isinstance(replay, str):
        return False

    keep_alive = value.get("keep_alive")
    if not isinstance(keep_alive, dict):
        return False

    interval = keep_alive.get("interval_ms")
    if not isinstance(interval, int) or isinstance(interval, bool):
        return False
    if not isinstance(keep_alive.get("event"), str):
        return False

    if not isinstance(value.get("keep_alive_comment"), str):
        return False

    if not _is_string_list(value.get("event_frames")):
        return False

    events = value.get("events")
    if not isinstance(events, list):
        return False

    for event in events:
        if not isinstance(event, dict):
            return False
        timestamp = event.get("timestamp_ms")
        if (
            not isinstance(event.get("event_id"), str)
            or not isinstance(event.get("source"), str)
            or not isinstance(timestamp, int)
            or isinstance(timestamp, bool)
            or "event" not in event
        ):
            return False

    return True


def _is_models_catalog_result(value: Any) -> bool:
    return (
        isinstance(value, dict)
        and isinstance(value.get("models"), list)
        and isinstance(value.get("provider_defaults"), list)
    )


def _is_string_list(value: Any) -> bool:
    return isinstance(value, list) and all(isinstance(item, str) for item in value)
