#!/usr/bin/env python3

from __future__ import annotations

import os
import sys
from pathlib import Path
from typing import Any, Literal

from fastapi import FastAPI
from pydantic import BaseModel, Field

try:
    from meerkat_mobkit_sdk import MobkitAsyncTypedClient, MobkitRpcError
except ModuleNotFoundError:
    sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
    from meerkat_mobkit_sdk import (  # type: ignore[no-redef]
        MobkitAsyncTypedClient,
        MobkitRpcError,
    )


class RpcRequestIdPayload(BaseModel):
    request_id: str = Field(min_length=1)


class ReconcilePayload(BaseModel):
    request_id: str = Field(min_length=1)
    modules: list[str] = Field(default_factory=list)


class SpawnMemberPayload(BaseModel):
    request_id: str = Field(min_length=1)
    module_id: str | None = None


class EventsSubscribePayload(BaseModel):
    request_id: str = Field(min_length=1)
    scope: Literal["mob", "agent", "interaction"] = "mob"
    last_event_id: str | None = None
    agent_id: str | None = None


app = FastAPI(title="MobKit H2 Python RPC-Mode Reference App")


def _required_gateway_bin() -> str:
    gateway_bin = os.environ.get("MOBKIT_RPC_GATEWAY_BIN")
    if not gateway_bin:
        raise RuntimeError("MOBKIT_RPC_GATEWAY_BIN must be set for h2_reference_app")
    return gateway_bin


def _client() -> MobkitAsyncTypedClient:
    client = getattr(app.state, "mobkit_client", None)
    if not isinstance(client, MobkitAsyncTypedClient):
        raise RuntimeError("MobKit async client is not initialized")
    return client


async def _typed_call_result(awaitable: Any) -> tuple[Any | None, dict[str, Any] | None]:
    try:
        return await awaitable, None
    except MobkitRpcError as exc:
        return None, {
            "code": exc.code,
            "message": str(exc),
            "request_id": exc.request_id,
            "method": exc.method,
        }
    except Exception as exc:  # broad for transparent route diagnostics
        return None, {"kind": type(exc).__name__, "message": str(exc)}


@app.on_event("startup")
async def startup_event() -> None:
    gateway_bin = _required_gateway_bin()
    app.state.gateway_bin = gateway_bin
    app.state.mobkit_client = MobkitAsyncTypedClient.from_gateway_bin(gateway_bin)


@app.get("/healthz")
async def healthz() -> dict[str, Any]:
    return {
        "ok": True,
        "gateway_bin": getattr(app.state, "gateway_bin", None),
    }


@app.post("/rpc/status")
async def rpc_status(payload: RpcRequestIdPayload) -> dict[str, Any]:
    client = _client()
    envelope = await client.rpc(payload.request_id, "mobkit/status", {})
    typed_result, typed_error = await _typed_call_result(
        client.status(f"{payload.request_id}-typed")
    )
    return {
        "route": "mobkit/status",
        "jsonrpc_envelope": envelope,
        "typed_result": typed_result,
        "typed_error": typed_error,
    }


@app.post("/rpc/capabilities")
async def rpc_capabilities(payload: RpcRequestIdPayload) -> dict[str, Any]:
    client = _client()
    envelope = await client.rpc(payload.request_id, "mobkit/capabilities", {})
    typed_result, typed_error = await _typed_call_result(
        client.capabilities(f"{payload.request_id}-typed")
    )
    return {
        "route": "mobkit/capabilities",
        "jsonrpc_envelope": envelope,
        "typed_result": typed_result,
        "typed_error": typed_error,
    }


@app.post("/rpc/reconcile")
async def rpc_reconcile(payload: ReconcilePayload) -> dict[str, Any]:
    client = _client()
    envelope = await client.rpc(
        payload.request_id, "mobkit/reconcile", {"modules": payload.modules}
    )
    typed_result, typed_error = await _typed_call_result(
        client.reconcile(payload.modules, f"{payload.request_id}-typed")
    )
    return {
        "route": "mobkit/reconcile",
        "jsonrpc_envelope": envelope,
        "typed_result": typed_result,
        "typed_error": typed_error,
    }


@app.post("/rpc/spawn_member")
async def rpc_spawn_member(payload: SpawnMemberPayload) -> dict[str, Any]:
    client = _client()
    params: dict[str, Any] = {}
    if payload.module_id is not None:
        params["module_id"] = payload.module_id
    envelope = await client.rpc(payload.request_id, "mobkit/spawn_member", params)
    typed_result, typed_error = await _typed_call_result(
        client.spawn_member(payload.module_id or "", f"{payload.request_id}-typed")
    )
    return {
        "route": "mobkit/spawn_member",
        "jsonrpc_envelope": envelope,
        "typed_result": typed_result,
        "typed_error": typed_error,
    }


@app.post("/rpc/events/subscribe")
async def rpc_events_subscribe(payload: EventsSubscribePayload) -> dict[str, Any]:
    client = _client()
    params: dict[str, Any] = {"scope": payload.scope}
    if payload.last_event_id is not None:
        params["last_event_id"] = payload.last_event_id
    if payload.agent_id is not None:
        params["agent_id"] = payload.agent_id

    envelope = await client.rpc(payload.request_id, "mobkit/events/subscribe", params)
    typed_result, typed_error = await _typed_call_result(
        client.subscribe_events(params, f"{payload.request_id}-typed")
    )
    return {
        "route": "mobkit/events/subscribe",
        "jsonrpc_envelope": envelope,
        "typed_result": typed_result,
        "typed_error": typed_error,
    }


@app.get("/flow/reference")
async def flow_reference() -> dict[str, Any]:
    client = _client()
    status = await client.rpc("h2-flow-status", "mobkit/status", {})
    capabilities = await client.rpc("h2-flow-capabilities", "mobkit/capabilities", {})
    reconcile = await client.rpc(
        "h2-flow-reconcile", "mobkit/reconcile", {"modules": ["routing"]}
    )
    spawn_member = await client.rpc(
        "h2-flow-spawn", "mobkit/spawn_member", {"module_id": "routing"}
    )
    events_subscribe = await client.rpc(
        "h2-flow-events", "mobkit/events/subscribe", {"scope": "mob"}
    )

    typed = {
        "status": await client.status("h2-flow-status-typed"),
        "capabilities": await client.capabilities("h2-flow-capabilities-typed"),
        "reconcile": await client.reconcile(["routing"], "h2-flow-reconcile-typed"),
        "spawn_member": await client.spawn_member("routing", "h2-flow-spawn-typed"),
        "events_subscribe": await client.subscribe_events(
            {"scope": "mob"}, "h2-flow-events-typed"
        ),
    }

    return {
        "route": "h2-flow",
        "status": status,
        "capabilities": capabilities,
        "reconcile": reconcile,
        "spawn_member": spawn_member,
        "events_subscribe": events_subscribe,
        "typed": typed,
    }
