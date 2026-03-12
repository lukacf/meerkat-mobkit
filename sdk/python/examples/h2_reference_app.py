#!/usr/bin/env python3

from __future__ import annotations

import os
import sys
from pathlib import Path
from typing import Any, Literal

from fastapi import FastAPI
from pydantic import BaseModel, Field

try:
    from meerkat_mobkit import MobKit, MobKitRuntime, RpcError
except ModuleNotFoundError:
    sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
    from meerkat_mobkit import (  # type: ignore[no-redef]
        MobKit,
        MobKitRuntime,
        RpcError,
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


def _runtime() -> MobKitRuntime:
    rt = getattr(app.state, "mobkit_runtime", None)
    if not isinstance(rt, MobKitRuntime):
        raise RuntimeError("MobKit runtime is not initialized")
    return rt


async def _typed_call_result(awaitable: Any) -> tuple[Any | None, dict[str, Any] | None]:
    try:
        return await awaitable, None
    except RpcError as exc:
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
    rt = await MobKit.builder().gateway(gateway_bin).build()
    await rt.connect()
    app.state.mobkit_runtime = rt


@app.get("/healthz")
async def healthz() -> dict[str, Any]:
    return {
        "ok": True,
        "gateway_bin": getattr(app.state, "gateway_bin", None),
    }


@app.post("/rpc/status")
async def rpc_status(payload: RpcRequestIdPayload) -> dict[str, Any]:
    handle = _runtime().mob_handle()
    typed_result, typed_error = await _typed_call_result(handle.status())
    return {
        "route": "mobkit/status",
        "typed_result": typed_result,
        "typed_error": typed_error,
    }


@app.post("/rpc/capabilities")
async def rpc_capabilities(payload: RpcRequestIdPayload) -> dict[str, Any]:
    handle = _runtime().mob_handle()
    typed_result, typed_error = await _typed_call_result(handle.capabilities())
    return {
        "route": "mobkit/capabilities",
        "typed_result": typed_result,
        "typed_error": typed_error,
    }


@app.post("/rpc/reconcile")
async def rpc_reconcile(payload: ReconcilePayload) -> dict[str, Any]:
    handle = _runtime().mob_handle()
    typed_result, typed_error = await _typed_call_result(
        handle.reconcile(payload.modules)
    )
    return {
        "route": "mobkit/reconcile",
        "typed_result": typed_result,
        "typed_error": typed_error,
    }


@app.post("/rpc/spawn_member")
async def rpc_spawn_member(payload: SpawnMemberPayload) -> dict[str, Any]:
    handle = _runtime().mob_handle()
    typed_result, typed_error = await _typed_call_result(
        handle.spawn_member(payload.module_id or "")
    )
    return {
        "route": "mobkit/spawn_member",
        "typed_result": typed_result,
        "typed_error": typed_error,
    }


@app.post("/rpc/events/subscribe")
async def rpc_events_subscribe(payload: EventsSubscribePayload) -> dict[str, Any]:
    handle = _runtime().mob_handle()
    typed_result, typed_error = await _typed_call_result(
        handle.subscribe_events(
            scope=payload.scope,
            last_event_id=payload.last_event_id,
            agent_id=payload.agent_id,
        )
    )
    return {
        "route": "mobkit/events/subscribe",
        "typed_result": typed_result,
        "typed_error": typed_error,
    }


@app.get("/flow/reference")
async def flow_reference() -> dict[str, Any]:
    handle = _runtime().mob_handle()
    status = await handle.status()
    capabilities = await handle.capabilities()
    reconcile = await handle.reconcile(["routing"])
    spawn_member = await handle.spawn_member("routing")
    events_subscribe = await handle.subscribe_events(scope="mob")

    return {
        "route": "h2-flow",
        "status": status,
        "capabilities": capabilities,
        "reconcile": reconcile,
        "spawn_member": spawn_member,
        "events_subscribe": events_subscribe,
    }
