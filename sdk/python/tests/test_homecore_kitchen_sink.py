"""HomeCore Kitchen Sink: School Closure + Calendar Conflict + Household Coordination.

Exercises the identity-first control plane on top of real autonomous multi-agent
coordination: triage receives connector events and fans out to domain agents via
comms, gate evaluates proposed actions, family-facing delivery goes to addressable
identities. Runtime shutdown/restore, respawn, and roster reconciliation happen
mid-incident.

Agents run in autonomous mode (the default) with comms wiring via role_wiring
rules. The test dispatches events to triage and waits for the agent graph to
process — agents use the comms `send` tool to coordinate, not test puppeting.

Run:
    PYTHONPATH=sdk/python ANTHROPIC_API_KEY=... \
        python3 -m pytest sdk/python/tests/test_homecore_kitchen_sink.py -v --timeout=300
"""
from __future__ import annotations

import asyncio
import copy
import json
import os
import re
from urllib.parse import urlencode
from urllib.request import Request, urlopen

import pytest

from meerkat_mobkit.builder import MobKit
from meerkat_mobkit.identity_first_models import (
    DispatchInput,
    DurableAgentSpec,
    ManagedPeerEdge,
)
from meerkat_mobkit.errors import RpcError

# ---------------------------------------------------------------------------
# Environment / skip helpers
# ---------------------------------------------------------------------------

# The gateway-backed suites must exercise THIS worktree's freshly built
# rpc_gateway, not whichever binary the main checkout last built. The default
# path below is the main worktree's scripts/repo-cargo lane; running from a
# feature worktree would silently test the wrong artifact (and hide the wire
# drift the branch introduces). Prefer an explicit override:
#   MOBKIT_GATEWAY_BIN=$(./scripts/repo-cargo --print-env CARGO_TARGET_DIR)/debug/rpc_gateway
def _resolve_gateway_bin() -> str:
    override = os.environ.get("MOBKIT_GATEWAY_BIN", "").strip()
    if override:
        return override
    return os.path.join(
        os.path.expanduser("~/Library/Caches/rust-workspaces"),
        "meerkat-mobkit-2783c42580",
        "targets",
        "meerkat-mobkit-44eecf13a1",
        "debug",
        "rpc_gateway",
    )


_GATEWAY_BIN = _resolve_gateway_bin()


def _anthropic_key() -> str | None:
    return os.environ.get("RKAT_ANTHROPIC_API_KEY") or os.environ.get(
        "ANTHROPIC_API_KEY"
    )


_skip_no_key = pytest.mark.skipif(not _anthropic_key(), reason="No Anthropic API key")
_skip_no_binary = pytest.mark.skipif(
    not os.path.isfile(_GATEWAY_BIN), reason="Gateway binary not found"
)


# ---------------------------------------------------------------------------
# Mob definition — autonomous agents with comms wiring
# ---------------------------------------------------------------------------

_HOUSEHOLD_MOB_TOML = """\
[mob]
id = "homecore-household"

# Wiring rules: triage is the hub, wired to all domain agents and identities.
[wiring]
auto_wire_orchestrator = false

[[wiring.role_wiring]]
a = "personal"
b = "triage"

[[wiring.role_wiring]]
a = "family_group"
b = "triage"

[[wiring.role_wiring]]
a = "triage"
b = "school"

[[wiring.role_wiring]]
a = "triage"
b = "calendar"

[[wiring.role_wiring]]
a = "triage"
b = "gate"

[[wiring.role_wiring]]
a = "gate"
b = "family_group"

# --- Profiles ---
# All profiles use autonomous_host mode (the default). When a message is injected
# via the identity-first bridge, the autonomous loop picks it up and processes it.
# Agents can use the comms send tool to forward messages to wired peers.

[profiles.personal]
model = "claude-sonnet-4-5"
skills = ["personal_role"]
external_addressable = true


[profiles.personal.tools]
comms = true

[profiles.family_group]
model = "claude-sonnet-4-5"
skills = ["family_group_role"]
external_addressable = true


[profiles.family_group.tools]
comms = true

[profiles.triage]
model = "claude-sonnet-4-5"
skills = ["triage_role"]
external_addressable = false


[profiles.triage.tools]
comms = true

[profiles.school]
model = "claude-sonnet-4-5"
skills = ["school_role"]
external_addressable = false


[profiles.school.tools]
comms = true

[profiles.calendar]
model = "claude-sonnet-4-5"
skills = ["calendar_role"]
external_addressable = false


[profiles.calendar.tools]
comms = true

[profiles.gate]
model = "claude-sonnet-4-5"
skills = ["gate_role"]
external_addressable = false


[profiles.gate.tools]
comms = true

# --- Prompts ---
# A member's system prompt is assembled from its profile `skills`; a bare
# `system_prompt` key under [profiles.X] is not a meerkat Profile field and
# is silently dropped.

[skills.personal_role]
source = "inline"
content = "You are a personal assistant for a household member. When you receive information, acknowledge it briefly. Keep all responses to 1-2 sentences."

[skills.family_group_role]
source = "inline"
content = "You are a family group channel. When you receive household updates, acknowledge them briefly. Keep responses to 1-2 sentences."

[skills.triage_role]
source = "inline"
content = "You are the household triage agent. When you receive events from connectors, analyze them and forward relevant information to the appropriate domain agents using the send tool. For school-related events, send to the peer with 'school' in their name. For calendar/scheduling events, send to the peer with 'calendar' in their name. You MUST use the send tool to forward information. After forwarding, summarize what you did."

[skills.school_role]
source = "inline"
content = "You are the school domain agent. You track school schedules, closures, and logistics. When you receive school-related events, analyze the impact (childcare needs, pickup changes) and respond with a brief assessment."

[skills.calendar_role]
source = "inline"
content = "You are the calendar domain agent. You track appointments and schedules. When you receive scheduling events or conflicts, identify the conflict and propose a solution in 1-2 sentences."

[skills.gate_role]
source = "inline"
content = "You are the gate agent. You evaluate proposed actions before they reach family members. When you receive a proposed action, briefly approve or flag concerns. Keep responses to 1-2 sentences."
"""

# ---------------------------------------------------------------------------
# Providers
# ---------------------------------------------------------------------------


class HouseholdRoster:
    def __init__(self, specs: list[DurableAgentSpec]):
        self._specs = list(specs)

    def update(self, specs: list[DurableAgentSpec]) -> None:
        self._specs = list(specs)

    async def roster(self, context: dict) -> list[DurableAgentSpec]:
        return list(self._specs)


class HouseholdTopology:
    def __init__(self, edges: list[tuple[str, str]]):
        self._edges = list(edges)

    def update(self, edges: list[tuple[str, str]]) -> None:
        self._edges = list(edges)

    async def compute_edges(self, target_identities, context) -> list[ManagedPeerEdge]:
        return [ManagedPeerEdge(a=a, b=b) for a, b in self._edges]


class HouseholdCustomizer:
    async def customize_build(self, context, spec, draft) -> None:
        identity = context.identity
        peers = []
        for edge in context.managed_edges:
            if edge.a == identity:
                peers.append(edge.b)
            elif edge.b == identity:
                peers.append(edge.a)
        if peers:
            peers.sort()
            # Appended after the profile's skill-assembled prompt. Setting
            # draft.system_prompt here would REPLACE that prompt with the
            # peer line alone.
            draft.additional_instructions.append(
                f"Your wired peers: {', '.join(peers)}"
            )

    async def after_create(self, identity, session_id, context) -> None:
        pass


# ---------------------------------------------------------------------------
# Roster and topology
# ---------------------------------------------------------------------------

_ROSTER = [
    DurableAgentSpec(identity="identity:luka", profile="personal", addressability="addressable"),
    DurableAgentSpec(identity="identity:louise", profile="personal", addressability="addressable"),
    DurableAgentSpec(identity="family-group:main", profile="family_group", addressability="addressable"),
    DurableAgentSpec(identity="triage:main", profile="triage", addressability="internal_only"),
    DurableAgentSpec(identity="domain:school", profile="school", addressability="internal_only"),
    DurableAgentSpec(identity="domain:calendar", profile="calendar", addressability="internal_only"),
    DurableAgentSpec(identity="gate:main", profile="gate", addressability="internal_only"),
]

_EDGES = [
    ("identity:luka", "triage:main"),
    ("identity:louise", "triage:main"),
    ("family-group:main", "triage:main"),
    ("triage:main", "domain:school"),
    ("triage:main", "domain:calendar"),
    ("triage:main", "gate:main"),
    ("gate:main", "family-group:main"),
]


# ---------------------------------------------------------------------------
# Boot helper
# ---------------------------------------------------------------------------


async def _boot(state_dir, roster, topology, customizer):
    return await (
        MobKit.builder()
        .gateway(_GATEWAY_BIN)
        .mob_inline(_HOUSEHOLD_MOB_TOML)
        .persistent_state(state_dir)
        .http_listen("127.0.0.1:0")
        .console_auth_required(False)
        .roster(roster)
        .topology_provider(topology)
        .agent_customizer(customizer)
        .build()
    )


_LUKA_CLOSURE_NOTICE = (
    "School closed tomorrow (pipe burst). Kids must stay home. "
    "This affects your morning schedule."
)

# The existing source-string school correlation canonicalizes to this UUID.
# Supplying the canonical value preserves identity and lets the test match the
# public interaction_id directly, without reimplementing gateway normalization.
_SCHOOL_DISPATCH_ID = "a1ebb4f0-201d-50a3-9733-590e176210c4"
_GATE_DISPATCH_ID = "e8d33c60-3d31-4f18-8c2c-07eb3aaf4ed9"


def _content_text(content):
    if isinstance(content, str):
        return content
    if not isinstance(content, list):
        return ""
    return "\n".join(
        block["text"] for block in content
        if isinstance(block, dict) and block.get("type") == "text"
        and isinstance(block.get("text"), str)
    )


def _school_delivery_frame_matches(frame, identity, session_id, sender_peer_id):
    if (
        frame.get("identity") != identity
        or frame.get("session_id") != session_id
        or frame.get("kind") != "system_notice"
        or frame.get("source", {}).get("kind") != "session_history"
    ):
        return False
    for block in frame.get("payload", {}).get("blocks", []):
        if not isinstance(block, dict):
            continue
        if (
            block.get("type") != "comms"
            or block.get("kind") not in ("message", "request")
            or block.get("direction") != "incoming"
            or (block.get("peer") or {}).get("id") != sender_peer_id
        ):
            continue
        text = _content_text(block.get("content")).lower()
        if "hillside" in text and "closed" in text and "pipe" in text and "burst" in text:
            return True
    return False


def _closure_notice_frame_matches(frame, identity, session_id):
    return (
        frame.get("identity") == identity
        and frame.get("session_id") == session_id
        and frame.get("kind") == "user_input"
        and frame.get("source", {}).get("kind") == "session_history"
        and _content_text(frame.get("payload", {}).get("content")) == _LUKA_CLOSURE_NOTICE
    )


def _remembers_school_closure(text):
    text = text.lower().replace("\u2019", "'")
    uncertain = any(phrase in text for phrase in (
        "do not know", "don't know", "not sure", "uncertain", "whether", "might be", "may be",
    ))
    negated = re.search(r"\b(?:not|isn't|won't be) closed\b", text)
    closure = re.search(r"\bclosed\b|\b(?:not (?:be )?|won't be )open\b", text)
    return (
        bool(closure) and "pipe" in text and "burst" in text
        and not uncertain and not negated and "?" not in text
    )


def _timeline_time():
    return asyncio.get_running_loop().time()


async def _timeline_pause(delay):
    await asyncio.sleep(delay)


async def _read_timeline_page(runtime, params, remaining):
    base = runtime.rust_http_base_url
    assert base, "gateway must expose its loopback console history endpoint"

    def read_page():
        with urlopen(
            base.rstrip("/") + "/console/timeline?" + urlencode(params),
            timeout=min(10, remaining),
        ) as response:
            return json.load(response)

    return await asyncio.to_thread(read_page)


async def _wait_for_timeline(runtime, identity, resolve, *, deadline):
    """Poll public frames under one absolute budget, including every page/read."""
    frames_by_id = {}
    while _timeline_time() < deadline:
        params = {"identity": identity, "mode": "since", "limit": 400}
        seen_cursors = set()
        while _timeline_time() < deadline:
            remaining = deadline - _timeline_time()
            page = await asyncio.wait_for(
                _read_timeline_page(runtime, params, remaining), timeout=remaining,
            )
            frames = page.get("frames")
            assert isinstance(frames, list), f"invalid console timeline page: {page}"
            for frame in frames:
                frames_by_id[frame["id"]] = frame
            if _timeline_time() >= deadline:
                break
            result = resolve(list(frames_by_id.values()))
            if result is not None:
                return result
            cursor = page.get("next_cursor")
            if page.get("exhausted") or not frames or cursor is None:
                break
            assert cursor not in seen_cursors, "console timeline pagination did not advance"
            seen_cursors.add(cursor)
            params["after"] = cursor
        await _timeline_pause(min(0.25, max(0, deadline - _timeline_time())))
    raise AssertionError(f"{identity}: required correlated committed evidence missing at deadline")


async def _wait_for_history_frame(runtime, identity, predicate, *, timeout=20):
    return await _wait_for_timeline(
        runtime, identity,
        lambda frames: next((frame for frame in frames if predicate(frame)), None),
        deadline=_timeline_time() + timeout,
    )


def _completed_interaction(frames, identity, session_id, interaction_id):
    owned = [frame for frame in frames if (
        frame.get("identity") == identity and frame.get("session_id") == session_id
        and frame.get("interaction_id") == interaction_id and frame.get("run_id")
    )]
    started = {frame["run_id"] for frame in owned if frame.get("kind") == "run_started"}
    for complete in owned:
        if (complete.get("kind") != "interaction_complete"
                or complete.get("status") != "completed" or complete["run_id"] not in started):
            continue
        output = complete.get("payload", {}).get("result")
        if not isinstance(output, str) or not output.strip():
            continue
        committed = next((frame for frame in owned if (
            frame.get("kind") == "text_complete"
            and frame.get("source", {}).get("kind") == "session_history"
            and frame["run_id"] == complete["run_id"]
            and frame.get("payload", {}).get("text") == output
        )), None)
        if committed:
            return {"run_id": complete["run_id"], "output": output, "history": committed}
    return None


def _school_fanout(frames, triage_session, dispatch_id, school_peer_id):
    completed = _completed_interaction(frames, "triage:main", triage_session, dispatch_id)
    if completed is None:
        return None
    owned = [frame for frame in frames if (
        frame.get("identity") == "triage:main" and frame.get("interaction_id") == dispatch_id
        and frame.get("run_id") == completed["run_id"]
        # Tool events omit session_id in the real gateway. Their exact run is
        # already anchored above to triage_session; reject contradictory owners.
        and frame.get("session_id") in (None, triage_session)
    )]
    for call in owned:
        payload = call.get("payload", {})
        name = payload.get("name")
        if call.get("kind") != "tool_call_requested" or name not in ("send_message", "send_request"):
            continue
        args = payload.get("args", {})
        if args.get("peer_id") != school_peer_id:
            continue
        incident = args.get("body") if name == "send_message" else json.dumps(args.get("params"))
        if not isinstance(incident, str) or not all(
            word in incident.lower() for word in ("hillside", "closed", "pipe", "burst")
        ):
            continue
        tool_id = payload.get("tool_call_id")
        if not tool_id:
            continue
        for result in owned:
            value = result.get("payload", {})
            if (result.get("kind") != "tool_execution_completed" or value.get("is_error")
                    or value.get("tool_call_id") != tool_id or value.get("name") != name):
                continue
            try:
                response = json.loads(value["result"])
            except (KeyError, TypeError, ValueError):
                continue
            if not isinstance(response, dict) or response.get("status") != "sent":
                continue
            receipt = response.get("receipt", {})
            kind = "peer_message" if name == "send_message" else "peer_request"
            if not isinstance(receipt, dict) or receipt.get("kind") != kind + "_sent":
                continue
            delivery = receipt.get("delivery")
            if not isinstance(delivery, dict) or delivery.get("durably_resolved") != {"outcome": "accepted"}:
                continue
            envelope = receipt.get("envelope_id")
            interaction = envelope if name == "send_message" else receipt.get("interaction_id")
            if isinstance(envelope, str) and envelope and isinstance(interaction, str) and interaction:
                return {**completed, "envelope_id": envelope, "interaction_id": interaction}
    return None


def _school_incident_result(frames, session_id, triage_peer_id, interaction_id):
    completed = _completed_interaction(frames, "domain:school", session_id, interaction_id)
    if completed is None:
        return None
    history = next((frame for frame in frames if _school_delivery_frame_matches(
        frame, "domain:school", session_id, triage_peer_id,
    )), None)
    return {**completed, "incident_history": history} if history else None


def _conversation_result(frames, identity, session_id, interaction_id, content):
    completed = _completed_interaction(frames, identity, session_id, interaction_id)
    if completed is None:
        return None
    sent = next((frame for frame in frames if (
        frame.get("identity") == identity and frame.get("session_id") == session_id
        and frame.get("interaction_id") == interaction_id
        and frame.get("kind") == "user_input"
        and frame.get("source", {}).get("kind") == "session_history"
        and _content_text(frame.get("payload", {}).get("content")) == content
    )), None)
    return {**completed, "input_history": sent} if sent else None


async def _send_console_and_wait(runtime, identity, session_id, content, key, *, timeout):
    """Use the public send acceptance's interaction ID for conversational turns."""
    deadline = _timeline_time() + timeout
    base = runtime.rust_http_base_url
    assert base, "gateway must expose its loopback console send endpoint"

    def send():
        request = Request(base.rstrip("/") + "/console/send", data=json.dumps({
            "identity": identity, "content": content, "origin": "household-smoke",
            "idempotency_key": key, "handling_mode": "queue",
        }).encode(), headers={"Content-Type": "application/json"}, method="POST")
        with urlopen(request, timeout=min(10, timeout)) as response:
            return json.load(response)

    accepted = await asyncio.wait_for(asyncio.to_thread(send), timeout=timeout)
    assert accepted.get("identity") == identity and accepted.get("session_id") == session_id
    interaction_id = accepted.get("interaction_id")
    assert interaction_id, "console send must return its accepted interaction ID"
    return await _wait_for_timeline(
        runtime, identity,
        lambda frames: _conversation_result(frames, identity, session_id, interaction_id, content),
        deadline=deadline,
    )


def _school_history_fixture():
    return {
        "identity": "domain:school",
        "session_id": "school-session",
        "kind": "system_notice",
        "source": {"kind": "session_history"},
        "payload": {"blocks": [{
            "type": "comms",
            "kind": "request",
            "direction": "incoming",
            "peer": {"id": "triage-peer-id", "display_name": "household/triage/mk--triage_cmain"},
            "content": [{
                "type": "text",
                "text": "Hillside Elementary is closed tomorrow due to a pipe burst.",
            }],
        }]},
    }


@pytest.mark.parametrize("mismatch", [
    None, "old_output", "other_identity", "other_session", "outgoing",
    "other_peer", "terminal_response", "missing_incident", "body_only",
])
def test_kitchen_school_history_oracle_requires_received_incident(mismatch):
    frame = copy.deepcopy(_school_history_fixture())
    block = frame["payload"]["blocks"][0]
    if mismatch == "old_output":
        frame["source"]["kind"] = "console_event"
        frame["kind"] = "text_complete"
    elif mismatch == "other_identity":
        frame["identity"] = "domain:calendar"
    elif mismatch == "other_session":
        frame["session_id"] = "previous-school-session"
    elif mismatch == "outgoing":
        block["direction"] = "outgoing"
    elif mismatch == "other_peer":
        block["peer"]["id"] = "calendar-peer-id"
    elif mismatch == "terminal_response":
        block["kind"] = "response_terminal"
    elif mismatch == "missing_incident":
        block["content"][0]["text"] = "Ready to track school closures."
    elif mismatch == "body_only":
        frame["payload"]["body"] = block["content"][0]["text"]
        block["content"] = []
    assert _school_delivery_frame_matches(
        frame, "domain:school", "school-session", "triage-peer-id"
    ) is (mismatch is None)


@pytest.mark.parametrize("text, expected", [
    ("School is closed tomorrow because a pipe burst.", True),
    ("Hillside Elementary will not be open due to burst pipes.", True),
    ("School won't be open tomorrow because of the pipe burst.", True),
    ("Is school open or closed tomorrow?", False),
    ("School is closed tomorrow.", False),
    ("School is open despite the pipe burst.", False),
    ("I do not know whether school is closed after the pipe burst.", False),
    ("Is school closed due to a pipe burst?", False),
    ("School is not closed after the pipe burst.", False),
])
def test_kitchen_continuity_oracle_requires_closure_and_cause(text, expected):
    assert _remembers_school_closure(text) is expected


@pytest.mark.parametrize("mismatch", [None, "other_session", "live_echo", "changed_text"])
def test_kitchen_notice_oracle_requires_exact_persisted_content(mismatch):
    frame = {
        "identity": "identity:luka",
        "session_id": "luka-session",
        "kind": "user_input",
        "source": {"kind": "session_history", "source_cursor": "luka-session:4"},
        "payload": {"content": [{"type": "text", "text": _LUKA_CLOSURE_NOTICE}]},
    }
    if mismatch == "other_session":
        frame["session_id"] = "unrelated-session"
    elif mismatch == "live_echo":
        frame["source"]["kind"] = "send"
    elif mismatch == "changed_text":
        frame["payload"]["content"][0]["text"] = "Is school closed?"
    assert _closure_notice_frame_matches(frame, "identity:luka", "luka-session") is (mismatch is None)


def _interaction_fixture(identity="triage:main", session="triage-session",
                         interaction="dispatch-id", run="incident-run"):
    common = {
        "identity": identity, "session_id": session,
        "interaction_id": interaction, "run_id": run,
    }
    return [
        {**common, "id": f"{run}-start", "kind": "run_started",
         "source": {"kind": "console_event"}, "payload": {}},
        {**common, "id": f"{run}-complete", "kind": "interaction_complete",
         "status": "completed", "source": {"kind": "console_event"},
         "payload": {"result": "incident processed"}},
        {**common, "id": f"{run}-history", "kind": "text_complete",
         "source": {"kind": "session_history", "source_cursor": f"{session}:9"},
         "payload": {"text": "incident processed"}},
    ]


@pytest.mark.parametrize("mismatch", [
    None, "unrelated_completion", "other_run", "other_session", "other_identity",
    "live_text_only", "history_only", "no_run_start", "failed",
])
def test_kitchen_correlated_completion_requires_exact_run_and_committed_output(mismatch):
    frames = _interaction_fixture()
    if mismatch == "unrelated_completion":
        frames[1]["interaction_id"] = "startup-id"
    elif mismatch == "other_run":
        frames[2]["run_id"] = "startup-run"
    elif mismatch == "other_session":
        frames[1]["session_id"] = "old-session"
    elif mismatch == "other_identity":
        frames[2]["identity"] = "domain:calendar"
    elif mismatch == "live_text_only":
        frames[2]["source"]["kind"] = "console_event"
    elif mismatch == "history_only":
        frames.pop(1)
    elif mismatch == "no_run_start":
        frames.pop(0)
    elif mismatch == "failed":
        frames[1]["status"] = "failed"
    result = _completed_interaction(frames, "triage:main", "triage-session", "dispatch-id")
    assert (result is not None) is (mismatch is None)
    if result:
        assert result["run_id"] == "incident-run"
        assert result["output"] == "incident processed"


def _school_fanout_fixture():
    frames = _interaction_fixture()
    common = {"identity": "triage:main", "interaction_id": "dispatch-id",
              "run_id": "incident-run", "source": {"kind": "console_event"}}
    frames.extend([
        {**common, "id": "send-call", "kind": "tool_call_requested", "payload": {
            "name": "send_message", "tool_call_id": "send-tool",
            "args": {"peer_id": "school-peer-id", "body":
                     "Hillside Elementary is closed tomorrow due to a pipe burst."},
        }},
        {**common, "id": "send-result", "kind": "tool_execution_completed", "payload": {
            "name": "send_message", "tool_call_id": "send-tool", "is_error": False,
            "result": json.dumps({"status": "sent", "kind": "peer_message", "receipt": {
                "kind": "peer_message_sent", "envelope_id": "school-envelope",
                "delivery": {"durably_resolved": {"outcome": "accepted"}},
            }}),
        }},
    ])
    return frames


@pytest.mark.parametrize("mismatch", [
    None, "unrelated_dispatch", "other_run", "other_peer", "other_call",
    "rejected", "queued_only", "tool_error", "unrelated_incident", "wrong_receipt_kind",
    "other_call_session", "other_result_session",
])
def test_kitchen_fanout_requires_accepted_receipt_from_exact_dispatch_tool(mismatch):
    frames = _school_fanout_fixture()
    call, result = frames[-2:]
    receipt = json.loads(result["payload"]["result"])
    if mismatch == "unrelated_dispatch":
        call["interaction_id"] = "startup-id"
    elif mismatch == "other_run":
        result["run_id"] = "startup-run"
    elif mismatch == "other_peer":
        call["payload"]["args"]["peer_id"] = "calendar-peer-id"
    elif mismatch == "other_call":
        result["payload"]["tool_call_id"] = "other-tool"
    elif mismatch == "rejected":
        receipt["receipt"]["delivery"]["durably_resolved"]["outcome"] = "rejected"
    elif mismatch == "queued_only":
        receipt["receipt"]["delivery"] = "queued"
    elif mismatch == "tool_error":
        result["payload"]["is_error"] = True
    elif mismatch == "unrelated_incident":
        call["payload"]["args"]["body"] = "Ready for school tasks."
    elif mismatch == "wrong_receipt_kind":
        receipt["receipt"]["kind"] = "peer_response_sent"
    elif mismatch == "other_call_session":
        call["session_id"] = "other-triage-session"
    elif mismatch == "other_result_session":
        result["session_id"] = "other-triage-session"
    result["payload"]["result"] = json.dumps(receipt)
    accepted = _school_fanout(
        frames, "triage-session", "dispatch-id", "school-peer-id"
    )
    assert (accepted is not None) is (mismatch is None)
    if accepted:
        assert accepted["envelope_id"] == "school-envelope"
        assert accepted["interaction_id"] == "school-envelope"


def test_kitchen_fanout_accepts_explicit_matching_session():
    frames = _school_fanout_fixture()
    for frame in frames[-2:]:
        frame["session_id"] = "triage-session"
    assert _school_fanout(frames, "triage-session", "dispatch-id", "school-peer-id")


def test_kitchen_request_fanout_uses_receipt_interaction_not_envelope_identity():
    frames = _school_fanout_fixture()
    frames[-2]["payload"].update(name="send_request", args={
        "peer_id": "school-peer-id", "intent": "school.closure",
        "params": {"event": "Hillside closed tomorrow due to pipe burst"},
    })
    frames[-1]["payload"].update(name="send_request", result=json.dumps({
        "status": "sent", "kind": "peer_request", "receipt": {
            "kind": "peer_request_sent", "envelope_id": "transport-envelope",
            "interaction_id": "school-request", "stream_reserved": True,
            "delivery": {"durably_resolved": {"outcome": "accepted"}},
        },
    }))
    accepted = _school_fanout(frames, "triage-session", "dispatch-id", "school-peer-id")
    assert accepted["envelope_id"] == "transport-envelope"
    assert accepted["interaction_id"] == "school-request"


@pytest.mark.parametrize("mismatch", [None, "unrelated_completion", "no_history", "live_ingest"])
def test_kitchen_school_requires_envelope_run_completion_and_durable_incident(mismatch):
    frames = _interaction_fixture("domain:school", "school-session", "school-envelope")
    notice = _school_history_fixture()
    notice["id"] = "school-notice"
    frames.append(notice)
    if mismatch == "unrelated_completion":
        frames[1]["interaction_id"] = "startup-envelope"
    elif mismatch == "no_history":
        frames.pop()
    elif mismatch == "live_ingest":
        notice["source"]["kind"] = "console_event"
        notice["kind"] = "peer_content_ingested"
    result = _school_incident_result(
        frames, "school-session", "triage-peer-id", "school-envelope"
    )
    assert (result is not None) is (mismatch is None)


@pytest.mark.parametrize("mismatch", [None, "other_interaction", "live_echo", "changed_content"])
def test_kitchen_conversation_requires_accepted_interaction_input_history(mismatch):
    frames = _interaction_fixture("identity:luka", "luka-session", "accepted-send")
    notice = {
        "identity": "identity:luka", "session_id": "luka-session",
        "interaction_id": "accepted-send", "kind": "user_input",
        "source": {"kind": "session_history"},
        "payload": {"content": [{"type": "text", "text": _LUKA_CLOSURE_NOTICE}]},
    }
    frames.append(notice)
    if mismatch == "other_interaction":
        notice["interaction_id"] = "prior-send"
    elif mismatch == "live_echo":
        notice["source"]["kind"] = "send"
    elif mismatch == "changed_content":
        notice["payload"]["content"] = "Ready for school tasks."
    result = _conversation_result(
        frames, "identity:luka", "luka-session", "accepted-send", _LUKA_CLOSURE_NOTICE,
    )
    assert (result is not None) is (mismatch is None)


@pytest.mark.asyncio
async def test_kitchen_timeline_wait_ignores_startup_then_requires_commit(monkeypatch):
    startup = _interaction_fixture(interaction="startup-id", run="startup-run")
    actual = _interaction_fixture()
    pages = [startup, startup + actual[:2], startup + actual]
    reads = []

    async def read_page(runtime, params, remaining):
        reads.append(remaining)
        return {"frames": pages.pop(0), "exhausted": True}

    async def no_sleep(delay):
        pass

    monkeypatch.setitem(globals(), "_read_timeline_page", read_page)
    monkeypatch.setitem(globals(), "_timeline_pause", no_sleep)
    result = await _wait_for_timeline(
        object(), "triage:main",
        lambda frames: _completed_interaction(frames, "triage:main", "triage-session", "dispatch-id"),
        deadline=_timeline_time() + 90,
    )
    assert len(reads) == 3
    assert result["run_id"] == "incident-run"


@pytest.mark.asyncio
async def test_kitchen_timeline_deadline_does_not_reset_on_progress_or_pagination(monkeypatch):
    clock = [0.0]
    reads = []

    async def read_page(runtime, params, remaining):
        reads.append((dict(params), remaining))
        clock[0] += 3
        return {"frames": [{"id": str(len(reads))}], "next_cursor": f"opaque-{len(reads)}"}

    async def pause(delay):
        clock[0] += delay

    monkeypatch.setitem(globals(), "_timeline_time", lambda: clock[0])
    monkeypatch.setitem(globals(), "_read_timeline_page", read_page)
    monkeypatch.setitem(globals(), "_timeline_pause", pause)
    with pytest.raises(AssertionError, match="deadline"):
        await _wait_for_timeline(object(), "triage:main", lambda frames: None, deadline=9)
    assert [remaining for _, remaining in reads] == [9, 6, 3]
    assert [params.get("after") for params, _ in reads] == [None, "opaque-1", "opaque-2"]
    assert clock[0] == 9


@pytest.mark.asyncio
@pytest.mark.parametrize("mismatch", [None, "other_identity", "other_session"])
async def test_kitchen_console_send_uses_acceptance_id_and_original_deadline(monkeypatch, mismatch):
    from types import SimpleNamespace

    clock = [0.0]
    requests = []
    remaining_budgets = []
    accepted = {"identity": "identity:luka", "session_id": "luka-session",
                "interaction_id": "accepted-send"}
    if mismatch == "other_identity":
        accepted["identity"] = "identity:louise"
    elif mismatch == "other_session":
        accepted["session_id"] = "old-session"

    class Response:
        def __enter__(self):
            return self

        def __exit__(self, *args):
            pass

        def read(self):
            return json.dumps(accepted).encode()

    def send(request, timeout):
        requests.append((request, timeout))
        clock[0] = 5
        return Response()

    async def read_page(runtime, params, remaining):
        remaining_budgets.append(remaining)
        frames = _interaction_fixture("identity:luka", "luka-session", "accepted-send")
        frames.append({
            "id": "input-history", "identity": "identity:luka", "session_id": "luka-session",
            "interaction_id": "accepted-send", "kind": "user_input",
            "source": {"kind": "session_history"}, "payload": {"content": _LUKA_CLOSURE_NOTICE},
        })
        return {"frames": frames, "exhausted": True}

    monkeypatch.setitem(globals(), "urlopen", send)
    monkeypatch.setitem(globals(), "_timeline_time", lambda: clock[0])
    monkeypatch.setitem(globals(), "_read_timeline_page", read_page)
    operation = _send_console_and_wait(
        SimpleNamespace(rust_http_base_url="http://127.0.0.1:8080"),
        "identity:luka", "luka-session", _LUKA_CLOSURE_NOTICE,
        "school:luka-notice-1", timeout=60,
    )
    if mismatch:
        with pytest.raises(AssertionError):
            await operation
        assert not remaining_budgets
    else:
        result = await operation
        assert result["history"]["interaction_id"] == "accepted-send"
        assert remaining_budgets == [55]
    request, timeout = requests[0]
    assert request.full_url == "http://127.0.0.1:8080/console/send"
    assert json.loads(request.data)["idempotency_key"] == "school:luka-notice-1"
    assert timeout == 10


# ===========================================================================
# THE KITCHEN SINK
# ===========================================================================


@_skip_no_key
@_skip_no_binary
class TestHouseholdIncident:

    @pytest.mark.asyncio
    @pytest.mark.timeout(300)
    async def test_school_closure_with_autonomous_coordination(self, tmp_path):
        state_dir = str(tmp_path / "state")
        os.makedirs(state_dir, exist_ok=True)

        roster = HouseholdRoster(_ROSTER)
        topology = HouseholdTopology(_EDGES)
        customizer = HouseholdCustomizer()

        # =============================================================
        # Phase 1: Bootstrap — 7 actors, assert topology wiring
        # =============================================================
        print("\n--- Phase 1: Bootstrap + topology validation ---")
        rt = await _boot(state_dir, roster, topology, customizer)
        try:
            # Agent handles — identity-scoped, no member IDs
            all_names = [
                "identity:luka", "identity:louise", "family-group:main",
                "triage:main", "domain:school", "domain:calendar", "gate:main",
            ]
            agents = {name: rt.agent(name) for name in all_names}
            triage = agents["triage:main"]
            luka = agents["identity:luka"]
            school = agents["domain:school"]
            gate = agents["gate:main"]

            # All 7 Active
            for name, agent in agents.items():
                s = await agent.status()
                assert s.state == "active", f"{name}: {s.state}"

            # ASSERT topology wiring via identity-first inspect
            triage_inspection = await triage.inspect()
            assert triage_inspection.peer_reachable_count >= 5, (
                f"triage should be wired to at least 5 peers (school, calendar, gate, "
                f"luka, louise), got {triage_inspection.peer_reachable_count}"
            )
            print(f"[Phase 1] triage peers: {triage_inspection.peer_reachable_count} reachable")
            school_session = (await school.status()).session_id
            luka_session = (await luka.status()).session_id
            triage_session = (await triage.status()).session_id
            gate_session = (await gate.status()).session_id
            triage_peer = await rt.mob_handle().peer_info("triage:main")
            assert triage_peer.get("peer_id"), "triage must expose its canonical comms peer ID"
            school_peer = await rt.mob_handle().peer_info("domain:school")
            assert school_peer.get("peer_id"), "school must expose its canonical comms peer ID"

            # Addressability enforcement
            with pytest.raises(RpcError, match="not addressable"):
                await rt.send("triage:main", "should fail")
            with pytest.raises(RpcError, match="not addressable"):
                await rt.send("domain:school", "should fail")
            with pytest.raises(RpcError, match="not addressable"):
                await rt.send("gate:main", "should fail")
            print("[Phase 1] InternalOnly enforcement OK for triage, school, gate")

            # Wait for autonomous kickoff turns to complete
            await rt.wait_until_ready([
                "triage:main", "domain:school", "domain:calendar",
                "gate:main", "identity:luka", "identity:louise",
                "family-group:main",
            ], timeout=30)

            # =============================================================
            # Phase 2: School closure → triage → ASSERT domain fan-out
            # =============================================================
            print("\n--- Phase 2: School closure + autonomous fan-out ---")

            triage_deadline = _timeline_time() + 90
            await asyncio.wait_for(triage.dispatch(DispatchInput(
                content=(
                    "URGENT from school connector: Hillside Elementary closed tomorrow "
                    "due to pipe burst. All students must stay home. This affects the "
                    "family's morning schedule. Forward this to the school domain agent."
                ),
                origin="connector",
                correlation_id=_SCHOOL_DISPATCH_ID,
                idempotency_key="school:school-closure-1",
            )), timeout=max(0, triage_deadline - _timeline_time()))

            # Startup peer replies can advance identity-wide completion cursors.
            # Require this dispatch's committed run and its accepted school send.
            fanout = await _wait_for_timeline(
                rt, "triage:main",
                lambda frames: _school_fanout(
                    frames, triage_session, _SCHOOL_DISPATCH_ID, school_peer["peer_id"],
                ),
                deadline=triage_deadline,
            )
            print(f"[Phase 2] triage output: {fanout['output']}")

            # Match the accepted envelope's interaction, then insist the exact
            # incident and assistant output are committed within the same budget.
            school_result = await _wait_for_timeline(
                rt, "domain:school",
                lambda frames: _school_incident_result(
                    frames, school_session, triage_peer["peer_id"], fanout["interaction_id"],
                ),
                deadline=_timeline_time() + 60,
            )
            received = school_result["incident_history"]
            print(f"[Phase 2] school accepted triage incident in history frame {received['id']}")
            print(f"[Phase 2] domain:school received comms: {school_result['output']}")

            # Deliver closure notice to luka (simulates end of triage→domain→gate→identity chain).
            # Console acceptance names the exact conversational interaction.
            luka_notice = await _send_console_and_wait(
                rt, "identity:luka", luka_session, _LUKA_CLOSURE_NOTICE,
                "school:luka-notice-1", timeout=60,
            )
            closure_history = luka_notice["input_history"]
            print("[Phase 2] identity:luka notified about school closure")

            # =============================================================
            # Phase 3: Calendar event + gate evaluation
            # =============================================================
            print("\n--- Phase 3: Calendar conflict + gate ---")

            # Dispatch to gate for policy evaluation
            gate_deadline = _timeline_time() + 60
            await asyncio.wait_for(gate.dispatch_text(
                "Proposed action: notify family group that school is closed tomorrow "
                "and Luka's dentist at 09:00 conflicts with childcare. "
                "Evaluate whether this notification is appropriate to send.",
                origin="system",
                correlation_id=_GATE_DISPATCH_ID,
                idempotency_key="school:gate-evaluation-1",
            ), timeout=max(0, gate_deadline - _timeline_time()))
            gate_result = await _wait_for_timeline(
                rt, "gate:main",
                lambda frames: _completed_interaction(
                    frames, "gate:main", gate_session, _GATE_DISPATCH_ID,
                ),
                deadline=gate_deadline,
            )
            print(f"[Phase 3] gate evaluated: {gate_result['output']}")

            # =============================================================
            # Phase 4: Shutdown MID-FLIGHT (not after idle)
            # =============================================================
            print("\n--- Phase 4: Mid-flight shutdown ---")

            # Dispatch calendar event and shutdown IMMEDIATELY
            # without waiting for processing. This tests checkpoint/restore
            # during active work, not after completion.
            await triage.dispatch(DispatchInput(
                content=(
                    "Calendar connector: Luka has dentist appointment at 09:00 tomorrow. "
                    "School is closed. Forward to calendar domain agent for conflict analysis."
                ),
                origin="connector",
                correlation_id="calendar-dentist-1",
                idempotency_key="calendar:calendar-dentist-1",
            ))
            # Record state BEFORE waiting for calendar processing
            pre_shutdown = {}
            for name in all_names:
                pre_shutdown[name] = await agents[name].status()

            # Shutdown while triage/calendar may still be processing
            await rt.shutdown()
            print("[Phase 4] Runtime shut down with calendar event potentially in-flight")

        except Exception:
            await rt.shutdown()
            raise

        # =============================================================
        # Phase 5: Restore — assert continuity content, not just IDs
        # =============================================================
        print("\n--- Phase 5: Restore + continuity verification ---")
        rt2 = await _boot(state_dir, roster, topology, customizer)
        try:
            # Re-create agent handles on the new runtime
            agents2 = {name: rt2.agent(name) for name in all_names}
            luka2 = agents2["identity:luka"]

            # Stable IDs across restart
            for name, before in pre_shutdown.items():
                after = await agents2[name].status()
                assert after.session_id == before.session_id, (
                    f"{name} session changed: {before.session_id} -> {after.session_id}"
                )
                assert after.agent_runtime_id == before.agent_runtime_id, (
                    f"{name} runtime_id changed"
                )
            print("[Phase 5] All 7 actors restored with stable IDs")

            # A resumed member does not replay its one-time kickoff turn.
            # Wait on the typed lifecycle readiness barrier rather than output
            # previews, which may stay empty until fresh work is delivered.
            startup = await rt2.wait_identity_bootstrap(
                target="startup_ready", timeout=30
            )
            assert startup.startup_ready is True, startup.to_dict()

            restored_history = await _wait_for_history_frame(
                rt2, "identity:luka",
                lambda frame: _closure_notice_frame_matches(frame, "identity:luka", luka_session),
            )
            assert restored_history["source"]["source_cursor"] == closure_history["source"]["source_cursor"], (
                "restart must preserve the exact durable school notice in Luka's session"
            )

            # ASSERT conversational continuity via LLM content.
            # Luka received the school closure notice before shutdown.
            # After restore, asking about school should reference the closure.
            # Correlate this question's accepted interaction and committed answer;
            # a resumed peer reply must not satisfy the continuity assertion.
            luka_result = await _send_console_and_wait(
                rt2, "identity:luka", luka_session,
                "Is school open or closed tomorrow, and why? Answer in one sentence only.",
                "school:luka-recall-1", timeout=90,
            )
            luka_output = luka_result["output"]
            assert _remembers_school_closure(luka_output), (
                "Luka must recall both the closure and its pipe-burst cause from persisted "
                f"history, not merely respond after restart: {luka_output}"
            )
            print(f"[Phase 5] luka remembers school closure: {luka_output}")

            # =============================================================
            # Phase 6: Respawn domain:calendar
            # =============================================================
            print("\n--- Phase 6: Respawn domain:calendar ---")

            calendar2 = rt2.agent("domain:calendar")
            school2 = rt2.agent("domain:school")

            cal_before = await calendar2.status()
            await calendar2.respawn()
            cal_after = await calendar2.status()

            assert cal_after.agent_runtime_id == cal_before.agent_runtime_id
            assert cal_after.session_id == cal_before.session_id
            assert cal_after.generation == cal_before.generation
            print("[Phase 6] domain:calendar respawned — identity preserved")

            school_after = await school2.status()
            assert school_after.session_id == pre_shutdown["domain:school"].session_id
            print("[Phase 6] domain:school unaffected")

            # =============================================================
            # Phase 7: Reconcile — add olivia
            # =============================================================
            print("\n--- Phase 7: Reconcile ---")

            olivia = DurableAgentSpec(
                identity="identity:olivia",
                profile="personal",
                addressability="addressable",
            )
            roster.update([*_ROSTER, olivia])
            topology.update([*_EDGES, ("identity:olivia", "triage:main")])

            await rt2.reconcile()

            olivia2 = rt2.agent("identity:olivia")
            olivia_status = await olivia2.status()
            assert olivia_status.state == "active"
            assert olivia_status.generation == 0

            luka_post = await luka2.status()
            assert luka_post.session_id == pre_shutdown["identity:luka"].session_id
            triage_post = await rt2.agent("triage:main").status()
            assert triage_post.session_id == pre_shutdown["triage:main"].session_id
            print("[Phase 7] olivia added, existing actors preserved")

            # =============================================================
            # Phase 8: Final coherence
            # =============================================================
            print("\n--- Phase 8: Final coherence ---")

            all_identities = [
                "identity:luka", "identity:louise", "identity:olivia",
                "family-group:main", "triage:main", "domain:school",
                "domain:calendar", "gate:main",
            ]
            for name in all_identities:
                s = await rt2.status(name)
                assert s.state == "active", f"{name}: {s.state}"

            print(f"[Phase 8] All {len(all_identities)} actors Active")
            print("\n=== HOUSEHOLD KITCHEN SINK PASSED ===")

        finally:
            await rt2.shutdown()
