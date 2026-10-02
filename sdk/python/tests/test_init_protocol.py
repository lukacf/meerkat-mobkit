"""mobkit/init: accepted, then settled (#550, part 2), against stand-in gateways.

Each stand-in is a tiny script speaking the stdio protocol, so these tests
pin the SDK side of every outcome without a real gateway:
- a legal slow startup (a callback round trip, then a settlement later than
  the SDK's request timeout) settles ready;
- a failed settlement raises the typed error carrying ``durable_effects``;
- once the init request is written, a lost acceptance, a gateway exit, an
  init deadline or a foreign init id is ``InitOutcomeUnknownError``, never a
  refusal;
- an older gateway's single response still connects.
The real-gateway counterparts live in ``test_init_protocol_real_gateway.py``.
"""
from __future__ import annotations

import functools
import json
import sys
import textwrap
from pathlib import Path

import pytest

import meerkat_mobkit.runtime as runtime_module
from meerkat_mobkit import InitOutcomeUnknownError, RpcError, StorageResolutionError
from meerkat_mobkit._transport import PersistentTransport
from meerkat_mobkit.builder import MobKit
from meerkat_mobkit.runtime import MobKitRuntime

# Every stand-in shares this prelude: it reads the init request, records it,
# and gives the scenario helpers to answer.
_PRELUDE = """
import json, pathlib, sys, time
record = pathlib.Path(sys.argv[0]).with_suffix(".record")
def emit(obj):
    sys.stdout.write(json.dumps(obj) + "\\n"); sys.stdout.flush()
def log(entry):
    with record.open("a") as f:
        f.write(json.dumps(entry) + "\\n")
init = json.loads(sys.stdin.readline())
log({"init": init})
params = init.get("params", {})
init_id = params.get("init_id")
def accepted(**extra):
    emit({"jsonrpc": "2.0", "id": init["id"], "result": {
        "init_state": "accepted", "init_id": init_id,
        "provider_callback_timeout_ms": 130000,
        "stdio_shutdown_handshake": True, "stdio_shutdown_horizon_ms": 30000, **extra}})
def progress(phase, for_init=None):
    emit({"jsonrpc": "2.0", "method": "mobkit/init_progress",
          "params": {"init_id": for_init or init_id, "phase": phase}})
def settled(for_init=None, **params):
    emit({"jsonrpc": "2.0", "method": "mobkit/init_settled",
          "params": {"init_id": for_init or init_id, **params}})
def serve_shutdown():
    for raw in sys.stdin:
        message = json.loads(raw)
        log({"after_init": message})
        if message.get("method") == "mobkit/shutdown":
            emit({"jsonrpc": "2.0", "id": message["id"],
                  "result": {"shutdown": True, "runtime_cleanup_completed": True}})
            return
"""


def _gateway(tmp_path: Path, name: str, body: str) -> Path:
    script = tmp_path / f"{name}.py"
    script.write_text(f"#!{sys.executable}\n{_PRELUDE}\n{textwrap.dedent(body)}")
    script.chmod(0o755)
    return script


def _records(script: Path) -> list[dict]:
    path = script.with_suffix(".record")
    if not path.exists():
        return []
    return [json.loads(line) for line in path.read_text().splitlines()]


@pytest.fixture
def short_request_timeout(monkeypatch):
    """Scale the SDK's 60 s request timeout down to 0.5 s for these tests."""
    monkeypatch.setattr(
        runtime_module,
        "PersistentTransport",
        functools.partial(PersistentTransport, timeout=0.5),
    )


def _runtime(script: Path, *, init_deadline: float | None = None) -> MobKitRuntime:
    builder = MobKit.builder().gateway(str(script))
    if init_deadline is not None:
        builder.init_deadline(init_deadline)
    return MobKitRuntime(builder._config)


@pytest.mark.asyncio
async def test_slow_legal_startup_settles_ready_after_the_request_timeout(
    tmp_path, short_request_timeout
):
    script = _gateway(tmp_path, "slow_ready", """
        accepted()
        progress("storage")
        # A provider callback during startup: the SDK answers it while the
        # init is still settling.
        emit({"jsonrpc": "2.0", "id": "cb-1", "method": "callback/unknown_startup_probe", "params": {}})
        log({"callback_answer": json.loads(sys.stdin.readline())})
        progress("restore")
        time.sleep(1.5)  # three times the scaled request timeout
        settled(outcome="ready", http_base_url="http://127.0.0.1:9", stdio_shutdown_handshake=True)
        serve_shutdown()
    """)
    runtime = _runtime(script)
    try:
        await runtime.connect()
        assert runtime.is_running
        assert runtime.rust_http_base_url == "http://127.0.0.1:9"
    finally:
        await runtime.shutdown()
    records = _records(script)
    init_params = records[0]["init"]["params"]
    assert init_params["init_protocol"] == "accepted_then_settled"
    assert init_params["init_id"].startswith("init-")
    answer = next(entry["callback_answer"] for entry in records if "callback_answer" in entry)
    assert answer["id"] == "cb-1"


@pytest.mark.parametrize(
    ("code", "durable_effects", "error_type"),
    [
        (-32602, "none", RpcError),
        (-32014, "possible", StorageResolutionError),
    ],
)
@pytest.mark.asyncio
async def test_failed_settlement_raises_typed_error_with_durable_effects(
    tmp_path, code, durable_effects, error_type
):
    script = _gateway(tmp_path, f"failed_{durable_effects}", f"""
        accepted()
        progress("storage")
        settled(outcome="failed", code={code}, message="refused for the test",
                durable_effects="{durable_effects}")
    """)
    runtime = _runtime(script)
    with pytest.raises(error_type) as raised:
        await runtime.connect()
    assert not isinstance(raised.value, InitOutcomeUnknownError)
    assert raised.value.code == code
    assert raised.value.data["durable_effects"] == durable_effects
    assert not runtime.is_running


@pytest.mark.asyncio
async def test_lost_acceptance_is_outcome_unknown(tmp_path, short_request_timeout):
    script = _gateway(tmp_path, "silent", """
        for raw in sys.stdin:  # hold the pipe open until the SDK closes it
            pass
    """)
    runtime = _runtime(script)
    with pytest.raises(InitOutcomeUnknownError) as raised:
        await runtime.connect()
    error = raised.value
    assert error.init_id == _records(script)[0]["init"]["params"]["init_id"]
    assert error.last_phase is None
    assert "no answer" in error.reason


@pytest.mark.asyncio
async def test_gateway_exit_after_acceptance_is_outcome_unknown_not_a_refusal(tmp_path):
    script = _gateway(tmp_path, "exits", """
        accepted()
        progress("prewarm")
        sys.exit(3)
    """)
    runtime = _runtime(script)
    with pytest.raises(InitOutcomeUnknownError) as raised:
        await runtime.connect()
    assert not isinstance(raised.value, RpcError)
    assert raised.value.last_phase == "prewarm"
    assert "closed its stdout" in raised.value.reason


@pytest.mark.asyncio
async def test_init_deadline_after_acceptance_is_outcome_unknown_and_shuts_down(tmp_path):
    script = _gateway(tmp_path, "never_settles", """
        accepted()
        progress("prewarm")
        serve_shutdown()
    """)
    runtime = _runtime(script, init_deadline=0.5)
    with pytest.raises(InitOutcomeUnknownError) as raised:
        await runtime.connect()
    assert raised.value.last_phase == "prewarm"
    assert "deadline" in raised.value.reason
    # The accepted response negotiated the handshake, so cleanup asked the
    # gateway to shut down instead of closing stdin under it.
    methods = [entry["after_init"].get("method") for entry in _records(script) if "after_init" in entry]
    assert methods == ["mobkit/shutdown"]


@pytest.mark.asyncio
async def test_settlement_for_another_init_is_ignored(tmp_path):
    script = _gateway(tmp_path, "foreign_then_ready", """
        accepted()
        settled(for_init="init-someone-else", outcome="failed", code=-32603,
                message="not this init", durable_effects="possible")
        progress("restore", for_init="init-someone-else")
        settled(outcome="ready", http_base_url="http://127.0.0.1:7")
        serve_shutdown()
    """)
    runtime = _runtime(script)
    try:
        await runtime.connect()
        assert runtime.rust_http_base_url == "http://127.0.0.1:7"
    finally:
        await runtime.shutdown()


@pytest.mark.asyncio
async def test_acceptance_for_a_different_init_id_is_outcome_unknown(tmp_path):
    script = _gateway(tmp_path, "wrong_id", """
        emit({"jsonrpc": "2.0", "id": init["id"], "result": {
            "init_state": "accepted", "init_id": "init-not-yours"}})
        for raw in sys.stdin:  # hold the pipe open until the SDK closes it
            pass
    """)
    runtime = _runtime(script)
    with pytest.raises(InitOutcomeUnknownError, match="different init_id"):
        await runtime.connect()


@pytest.mark.asyncio
async def test_older_gateway_single_response_still_connects(tmp_path):
    script = _gateway(tmp_path, "legacy", """
        emit({"jsonrpc": "2.0", "id": init["id"], "result": {"http_base_url": "http://127.0.0.1:5"}})
        for raw in sys.stdin:
            pass
    """)
    runtime = _runtime(script)
    try:
        await runtime.connect()
        assert runtime.is_running
        assert runtime.rust_http_base_url == "http://127.0.0.1:5"
    finally:
        await runtime.shutdown()


def test_init_deadline_must_be_positive_and_finite():
    builder = MobKit.builder()
    for bad in (0, -1, float("inf"), float("nan"), True, "5"):
        with pytest.raises(ValueError):
            builder.init_deadline(bad)
    assert builder.init_deadline(None)._config.init_deadline is None
    assert builder.init_deadline(2)._config.init_deadline == 2.0


@pytest.mark.asyncio
async def test_long_prewarm_past_the_callback_bound_settles_ready(tmp_path, monkeypatch):
    """No SDK timer ends an accepted init: prewarm on a large roster is
    legitimately unbounded. The loop clock is jumped well past 130 s while
    the stand-in sits in `prewarm`; any timer the SDK armed would fire then."""
    import asyncio

    release = tmp_path / "release-prewarm"
    script = _gateway(tmp_path, "long_prewarm", f"""
        accepted()
        progress("prewarm")
        release = pathlib.Path({json.dumps(str(release))})
        while not release.exists():
            time.sleep(0.02)
        settled(outcome="ready", http_base_url="http://127.0.0.1:3")
        serve_shutdown()
    """)
    runtime = _runtime(script)
    connect = asyncio.create_task(runtime.connect())
    try:
        while runtime._transport is None or runtime._transport._init_watch is None \
                or runtime._transport._init_watch.last_phase != "prewarm":
            assert not connect.done(), connect.exception()
            await asyncio.sleep(0.01)
        loop = asyncio.get_running_loop()
        real_time = loop.time
        monkeypatch.setattr(loop, "time", lambda: real_time() + 600.0)
        for _ in range(20):
            await asyncio.sleep(0)
        assert not connect.done(), "an SDK timer ended the init during prewarm"
        monkeypatch.setattr(loop, "time", real_time)
        release.write_text("go")
        await connect
        assert runtime.rust_http_base_url == "http://127.0.0.1:3"
    finally:
        if not connect.done():
            connect.cancel()
        await runtime.shutdown()
