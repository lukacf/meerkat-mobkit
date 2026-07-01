"""Regression tests: verify Python SDK methods call the correct RPC names.

These tests mock the transport layer and verify that each SDK method sends
the expected RPC method name. This catches mismatches between the Python SDK
and the Rust RPC dispatch table.
"""

import asyncio
import json
from unittest.mock import AsyncMock, MagicMock

import pytest
from meerkat_mobkit.errors import NotConnectedError, RpcError


def make_mock_mob_handle(rpc_responses=None):
    """Create a MobHandle with a mocked RPC transport."""
    from meerkat_mobkit.runtime import MobHandle

    runtime = MagicMock()
    calls = []

    async def mock_rpc(method, params=None):
        calls.append((method, params))
        if rpc_responses and method in rpc_responses:
            return rpc_responses[method]
        return {}

    runtime._rpc = mock_rpc
    handle = MobHandle.__new__(MobHandle)
    handle._runtime = runtime
    return handle, calls


def make_http_mob_handle():
    from meerkat_mobkit.runtime import MobHandle

    runtime = MagicMock()
    runtime.rust_http_base_url = "http://127.0.0.1:8765"
    handle = MobHandle.__new__(MobHandle)
    handle._runtime = runtime
    return handle


class FakeHttpResponse:
    def __init__(self, payload):
        self.payload = payload

    def __enter__(self):
        return self

    def __exit__(self, exc_type, exc, tb):
        return False

    def read(self):
        return json.dumps(self.payload).encode("utf-8")


@pytest.mark.asyncio
async def test_attach_session_rpc_name():
    """P1 regression: attach_session must call mobkit/attach_existing_session."""
    handle, calls = make_mock_mob_handle({
        "mobkit/attach_existing_session": {
            "status": "active",
            "tokens_used": 0,
            "is_final": False,
        }
    })
    await handle.attach_session("worker", "w1", "sid_abc123")
    assert calls[0][0] == "mobkit/attach_existing_session"


@pytest.mark.asyncio
async def test_agent_memory_rpc_names_and_params():
    """Agent memory helpers must hit the first-class durable memory RPCs."""
    handle, calls = make_mock_mob_handle({
        "mobkit/agent_memory/remember": {
            "memory_id": "mem-1",
            "title": "School pickup",
            "body": "Pickup is before calendar planning.",
            "tags": ["calendar", "family"],
            "created_at_ms": 10,
            "updated_at_ms": 20,
        },
        "mobkit/agent_memory/recall": {
            "records": [{
                "memory_id": "mem-1",
                "title": "School pickup",
                "body": "Pickup is before calendar planning.",
                "tags": ["calendar", "family"],
                "created_at_ms": 10,
                "updated_at_ms": 20,
            }],
        },
        "mobkit/agent_memory/forget": {
            "memory_id": "mem-1",
            "deleted": True,
        },
    })

    remembered = await handle.remember_agent_memory(
        "identity:luka",
        realm="family",
        title="School pickup",
        body="Pickup is before calendar planning.",
        tags=["family", "calendar"],
    )
    recalled = await handle.recall_agent_memory(
        "identity:luka",
        realm="family",
        selection="contextual",
        query_text="Where is pickup?",
        query_terms=["pickup"],
        max_entries=4,
    )
    forgotten = await handle.forget_agent_memory(
        "identity:luka", "mem-1", realm="family"
    )

    assert calls == [
        (
            "mobkit/agent_memory/remember",
            {
                "identity": "identity:luka",
                "realm": "family",
                "title": "School pickup",
                "body": "Pickup is before calendar planning.",
                "tags": ["family", "calendar"],
            },
        ),
        (
            "mobkit/agent_memory/recall",
            {
                "identity": "identity:luka",
                "realm": "family",
                "selection": "contextual",
                "query_text": "Where is pickup?",
                "query_terms": ["pickup"],
                "max_entries": 4,
            },
        ),
        (
            "mobkit/agent_memory/forget",
            {
                "identity": "identity:luka",
                "memory_id": "mem-1",
                "realm": "family",
            },
        ),
    ]
    assert remembered.memory_id == "mem-1"
    assert recalled[0].memory_id == "mem-1"
    assert forgotten.deleted is True


@pytest.mark.asyncio
async def test_agent_memory_update_and_manifest_rpc_names_and_params():
    """update/manifest must hit the v2 durable-memory RPCs with exact params."""
    handle, calls = make_mock_mob_handle({
        "mobkit/agent_memory/update": {
            "memory_id": "mem-2",
            "supersedes": "mem-1",
        },
        "mobkit/agent_memory/manifest": {
            "records": [{
                "id": "mem-2",
                "kind": "fact",
                "title": "School pickup",
                "description": "When planning the family calendar",
                "age_days": 3,
                "rank": 1,
            }],
        },
    })

    updated = await handle.update_agent_memory(
        "identity:luka",
        "mem-1",
        realm="family",
        title="School pickup",
        body="Pickup moved to 15:30.",
        tags=["family"],
    )
    manifest = await handle.manifest_agent_memory(
        "identity:luka",
        realm="family",
        tier="working_set",
        k=4,
    )

    assert calls == [
        (
            "mobkit/agent_memory/update",
            {
                "identity": "identity:luka",
                "memory_id": "mem-1",
                "title": "School pickup",
                "body": "Pickup moved to 15:30.",
                "realm": "family",
                "tags": ["family"],
            },
        ),
        (
            "mobkit/agent_memory/manifest",
            {
                "identity": "identity:luka",
                "realm": "family",
                "tier": "working_set",
                "k": 4,
            },
        ),
    ]
    assert updated.memory_id == "mem-2"
    assert updated.supersedes == "mem-1"
    assert manifest[0].id == "mem-2"
    assert manifest[0].kind == "fact"
    assert manifest[0].rank == 1
    assert manifest[0].age_days == 3


@pytest.mark.asyncio
async def test_agent_memory_manifest_optional_params_omitted():
    """Optional tier/k/realm must not leak into params when not provided,
    and a rank-less manifest row parses with rank=None."""
    handle, calls = make_mock_mob_handle({
        "mobkit/agent_memory/manifest": {
            "records": [{
                "id": "mem-3",
                "kind": "gotcha",
                "title": "Unranked",
                "age_days": 0,
            }],
        },
    })

    manifest = await handle.manifest_agent_memory("identity:luka")

    assert calls[0][0] == "mobkit/agent_memory/manifest"
    assert calls[0][1] == {"identity": "identity:luka"}
    assert manifest[0].rank is None
    assert manifest[0].description == ""


@pytest.mark.asyncio
async def test_mobpack_editor_catalog_rpc_names():
    handle, calls = make_mock_mob_handle({
        "mobkit/tools/catalog": {
            "schema_version": "mobpack.editor.v1",
            "runtime_backed": False,
            "source": "mobkit/tool-config",
            "authoring_provider": {"id": "standalone_authoring"},
            "tool_catalog": [{"id": "shell"}],
        },
        "mobkit/skills/catalog": {
            "schema_version": "mobpack.editor.v1",
            "runtime_backed": False,
            "source": "mobkit/authoring-skill-realms",
            "authoring_provider": {"id": "standalone_authoring"},
            "skill_realms": [{"id": "mobkit/authoring"}],
        },
        "mobkit/agent_definitions/list": {
            "schema_version": "mobpack.editor.v1",
            "runtime_backed": False,
            "source": "mobkit/authoring-agent-definitions",
            "authoring_provider": {"id": "standalone_authoring"},
            "agent_definitions": [{"id": "authoring:reviewer"}],
        },
        "mobkit/mobpacks/templates": {
            "schema_version": "mobpack.editor.v1",
            "source": "mobkit/mobpack-templates",
            "authoring_provider": {"id": "standalone_authoring"},
            "blank_mobpack": {"document": {}},
            "sample_mobpacks": [{"id": "sample"}],
            "sample_agent_definitions": [{"id": "sample:reviewer"}],
            "templates": {"blank_mobpack": {"document": {}}},
        },
        "mobkit/mobpacks/catalogs": {
            "schema_version": "mobpack.editor.v1",
            "runtime_backed": False,
            "authoring_provider": {"id": "standalone_authoring"},
            "sources": {"tools": "mobkit/tools/catalog"},
            "templates": {},
            "tool_catalog": [{"id": "shell"}],
            "skill_realms": [{"id": "mobkit/authoring"}],
            "blank_mobpack": {"document": {}},
            "sample_mobpacks": [{"id": "sample"}],
            "agent_definitions": [{"id": "authoring:reviewer"}],
            "sample_agent_definitions": [{"id": "sample:reviewer"}],
            "models": [],
            "provider_defaults": [],
        },
    })

    assert (await handle.tools_catalog()).tool_catalog == [{"id": "shell"}]
    assert (await handle.skills_catalog()).skill_realms == [{"id": "mobkit/authoring"}]
    assert (await handle.agent_definitions()).agent_definitions == [{"id": "authoring:reviewer"}]
    assert (await handle.mobpack_templates()).sample_mobpacks == [{"id": "sample"}]
    assert (await handle.mobpack_catalogs()).sources == {"tools": "mobkit/tools/catalog"}
    assert [call[0] for call in calls] == [
        "mobkit/tools/catalog",
        "mobkit/skills/catalog",
        "mobkit/agent_definitions/list",
        "mobkit/mobpacks/templates",
        "mobkit/mobpacks/catalogs",
    ]


def _mobpack_validation_payload(ok=True):
    return {
        "ok": ok,
        "diagnostics": [],
        "display_rows": [{"kind": "ok", "glyph": "✓", "head": "valid", "sub": "", "meta": ""}],
        "mob_id": "demo",
        "flow_ids": ["flow_a"],
        "validation_source": "mobkit/mobpacks/validate",
        "deploy_command": "rkat mob deploy",
    }


def _mobpack_draft_row_payload(
    draft_id="f_demo", revision=1, can_undo=False, can_redo=False
):
    return {
        "id": draft_id,
        "name": "Demo",
        "version": "mobpack.editor.v1",
        "stage": "draft",
        "trigger": "MobKit authoring draft",
        "source": "mobkit/mobpacks/create",
        "revision": revision,
        "etag": f"{draft_id}:{revision}",
        "updated_at_unix_ms": 1700000000000,
        "document": {"mob_id": "demo"},
        "validation": {"ok": True},
        "can_undo": can_undo,
        "can_redo": can_redo,
    }


@pytest.mark.asyncio
async def test_mobpack_authoring_rpc_names_and_params():
    """Every mobpack authoring wrapper must hit its exact RPC name with
    snake_case params and parse the typed result."""
    document = {"mob_id": "demo", "members": []}
    handle, calls = make_mock_mob_handle({
        "mobkit/mobpacks/validate": _mobpack_validation_payload(),
        "mobkit/mobpacks/source": {
            "filename": "demo.mobpack",
            "media_type": "application/vnd.meerkat.mobpack",
            "mob_toml": "[mob]\nid = \"demo\"\n",
            "source_files": [{
                "path": "mob.toml",
                "media_type": "text/x-toml",
                "size_bytes": 24,
                "content_base64": "W21vYl0=",
                "sha256": "abc",
            }],
            "validation": _mobpack_validation_payload(),
            "source": "mobkit/mobpacks/source",
        },
        "mobkit/mobpacks/export": {
            "filename": "demo.mobpack",
            "media_type": "application/vnd.meerkat.mobpack",
            "content_base64": "UEsDBA==",
            "mob_toml": "[mob]\nid = \"demo\"\n",
            "source_files": [],
            "validation": _mobpack_validation_payload(),
        },
        "mobkit/mobpacks/import": {
            "document": {"mob_id": "demo"},
            "validation": _mobpack_validation_payload(),
            "source": "mobkit/mobpacks/import:mob.toml",
            "source_label": "demo.toml",
            "source_media_type": "text/x-toml",
        },
        "mobkit/mobpacks/list": {
            "source": "mobkit/mobpacks/list",
            "store_path": "/tmp/drafts.json",
            "runtime_backed": False,
            "rows": [_mobpack_draft_row_payload()],
        },
        "mobkit/mobpacks/get": {
            "source": "mobkit/mobpacks/get",
            "store_path": "/tmp/drafts.json",
            "runtime_backed": False,
            "row": _mobpack_draft_row_payload(),
        },
        "mobkit/mobpacks/create": {
            "source": "mobkit/mobpacks/create",
            "store_path": "/tmp/drafts.json",
            "row": _mobpack_draft_row_payload(),
            "rows": [_mobpack_draft_row_payload()],
        },
        "mobkit/mobpacks/save": {
            "source": "mobkit/mobpacks/save",
            "store_path": "/tmp/drafts.json",
            "row": _mobpack_draft_row_payload(revision=2),
            "rows": [_mobpack_draft_row_payload(revision=2)],
        },
        "mobkit/mobpacks/undo": {
            "source": "mobkit/mobpacks/undo",
            "store_path": "/tmp/drafts.json",
            "stepped": True,
            "row": _mobpack_draft_row_payload(revision=3, can_undo=False, can_redo=True),
            "rows": [_mobpack_draft_row_payload(revision=3, can_undo=False, can_redo=True)],
        },
        "mobkit/mobpacks/redo": {
            "source": "mobkit/mobpacks/redo",
            "store_path": "/tmp/drafts.json",
            "stepped": True,
            "row": _mobpack_draft_row_payload(revision=4, can_undo=True, can_redo=False),
            "rows": [_mobpack_draft_row_payload(revision=4, can_undo=True, can_redo=False)],
        },
        "mobkit/mobpacks/delete": {
            "source": "mobkit/mobpacks/delete",
            "store_path": "/tmp/drafts.json",
            "id": "f_demo",
            "deleted": True,
            "rows": [],
        },
        "mobkit/mobpacks/apply_operation": {
            "source": "mobkit/mobpacks/apply_operation",
            "operation": "add_member",
            "ok": True,
            "document": {"mob_id": "demo", "members": [{"id": "reviewer"}]},
            "selection": {"kind": "agent", "id": "reviewer"},
            "validation": _mobpack_validation_payload(),
        },
        "mobkit/mobpacks/deploy_command": {
            "command": "rkat mob deploy demo.mobpack",
            "argv": ["rkat", "mob", "deploy", "demo.mobpack"],
            "deploy_command": "rkat mob deploy",
            "filename": "demo.mobpack",
            "validation": _mobpack_validation_payload(),
            "source": "meerkat_mobkit::mobpack::deploy_argv",
        },
        "mobkit/mobpacks/deploy": {
            "filename": "demo.mobpack",
            "pack_path": "/tmp/demo.mobpack",
            "pack_sha256": "deadbeef",
            "command": "rkat mob deploy /tmp/demo.mobpack",
            "argv": ["rkat", "mob", "deploy", "/tmp/demo.mobpack"],
            "plan_trace": [{"step": "validate"}],
            "executed": False,
            "success": False,
            "validation": _mobpack_validation_payload(),
            "display_rows": [],
        },
    })

    validation = await handle.mobpack_validate(document, rkat_validate=True)
    assert validation.ok is True
    assert validation.deploy_command == "rkat mob deploy"
    assert validation.display_rows[0].kind == "ok"
    assert validation.flow_ids == ["flow_a"]

    source = await handle.mobpack_source(document)
    assert source.mob_toml.startswith("[mob]")
    assert source.source_files[0].path == "mob.toml"
    assert source.validation.ok is True

    export = await handle.mobpack_export(document)
    assert export.content_base64 == "UEsDBA=="
    assert export.filename == "demo.mobpack"

    imported = await handle.mobpack_import(
        mob_toml="[mob]\nid = \"demo\"\n", source_name="demo.toml"
    )
    assert imported.document == {"mob_id": "demo"}
    assert imported.source == "mobkit/mobpacks/import:mob.toml"

    listed = await handle.mobpack_list()
    assert listed.rows[0].id == "f_demo"
    assert listed.rows[0].revision == 1
    assert listed.store_path == "/tmp/drafts.json"

    got = await handle.mobpack_get("f_demo")
    assert got.row.etag == "f_demo:1"
    assert got.row.stage == "draft"

    created = await handle.mobpack_create(template="blank", name="Demo")
    assert created.row.id == "f_demo"

    saved = await handle.mobpack_save(
        "f_demo",
        document,
        stage="draft",
        expected_revision=1,
        expected_etag="f_demo:1",
    )
    assert saved.row.revision == 2

    undone = await handle.mobpack_undo(
        "f_demo", expected_revision=2, expected_etag="f_demo:2"
    )
    assert undone.stepped is True
    assert undone.reason is None
    assert undone.row.revision == 3
    assert undone.row.can_undo is False
    assert undone.row.can_redo is True

    redone = await handle.mobpack_redo("f_demo")
    assert redone.stepped is True
    assert redone.row.revision == 4
    assert redone.row.can_undo is True
    assert redone.row.can_redo is False

    deleted = await handle.mobpack_delete("f_demo", expected_revision=2)
    assert deleted.deleted is True
    assert deleted.id == "f_demo"

    applied = await handle.mobpack_apply_operation(
        document,
        {"type": "add_member", "member": {"id": "reviewer"}},
        expected_catalog_snapshot_id="snap-1",
    )
    assert applied.ok is True
    assert applied.selection == {"kind": "agent", "id": "reviewer"}
    assert applied.validation.ok is True

    deploy_command = await handle.mobpack_deploy_command(document)
    assert deploy_command.argv == ["rkat", "mob", "deploy", "demo.mobpack"]

    deployed = await handle.mobpack_deploy(document, execute=False)
    assert deployed.executed is False
    assert deployed.pack_sha256 == "deadbeef"

    assert [call[0] for call in calls] == [
        "mobkit/mobpacks/validate",
        "mobkit/mobpacks/source",
        "mobkit/mobpacks/export",
        "mobkit/mobpacks/import",
        "mobkit/mobpacks/list",
        "mobkit/mobpacks/get",
        "mobkit/mobpacks/create",
        "mobkit/mobpacks/save",
        "mobkit/mobpacks/undo",
        "mobkit/mobpacks/redo",
        "mobkit/mobpacks/delete",
        "mobkit/mobpacks/apply_operation",
        "mobkit/mobpacks/deploy_command",
        "mobkit/mobpacks/deploy",
    ]
    assert calls[0][1] == {"document": document, "rkat_validate": True}
    assert calls[1][1] == {"document": document}
    assert calls[2][1] == {"document": document}
    assert calls[3][1] == {
        "mob_toml": "[mob]\nid = \"demo\"\n",
        "source_name": "demo.toml",
    }
    assert calls[4][1] == {}
    assert calls[5][1] == {"id": "f_demo"}
    assert calls[6][1] == {"template": "blank", "name": "Demo"}
    assert calls[7][1] == {
        "id": "f_demo",
        "document": document,
        "stage": "draft",
        "expected_revision": 1,
        "expected_etag": "f_demo:1",
    }
    assert calls[8][1] == {
        "id": "f_demo",
        "expected_revision": 2,
        "expected_etag": "f_demo:2",
    }
    assert calls[9][1] == {"id": "f_demo"}
    assert calls[10][1] == {"id": "f_demo", "expected_revision": 2}
    assert calls[11][1] == {
        "document": document,
        "operation": {"type": "add_member", "member": {"id": "reviewer"}},
        "expected_catalog_snapshot_id": "snap-1",
    }
    assert calls[12][1] == {"document": document}
    assert calls[13][1] == {"document": document, "execute": False}


@pytest.mark.asyncio
async def test_mobpack_authoring_optional_params_omitted():
    """Optional guards must not leak into params when not provided."""
    handle, calls = make_mock_mob_handle({
        "mobkit/mobpacks/validate": _mobpack_validation_payload(),
        "mobkit/mobpacks/delete": {
            "source": "mobkit/mobpacks/delete",
            "store_path": "/tmp/drafts.json",
            "id": "f_demo",
            "deleted": False,
            "rows": [],
        },
    })

    await handle.mobpack_validate({"mob_id": "demo"})
    await handle.mobpack_delete("f_demo")

    assert calls[0][1] == {"document": {"mob_id": "demo"}}
    assert calls[1][1] == {"id": "f_demo"}


@pytest.mark.asyncio
async def test_mobpack_undo_not_stepped_carries_reason():
    """When the history is empty, undo reports stepped=False with a reason
    and leaves the draft row untouched."""
    handle, calls = make_mock_mob_handle({
        "mobkit/mobpacks/undo": {
            "source": "mobkit/mobpacks/undo",
            "store_path": "/tmp/drafts.json",
            "stepped": False,
            "reason": "nothing to undo",
            "row": _mobpack_draft_row_payload(revision=1),
            "rows": [_mobpack_draft_row_payload(revision=1)],
        },
    })

    result = await handle.mobpack_undo("f_demo")
    assert calls[0][0] == "mobkit/mobpacks/undo"
    assert calls[0][1] == {"id": "f_demo"}
    assert result.stepped is False
    assert result.reason == "nothing to undo"
    assert result.row.revision == 1
    assert result.row.can_undo is False


@pytest.mark.asyncio
async def test_send_with_attachments_uses_multipart(monkeypatch):
    from meerkat_mobkit import runtime as runtime_module

    captured = {}

    def fake_urlopen(req, timeout=60):
        captured["url"] = req.full_url
        body = req.data.decode("utf-8", errors="replace")
        captured["body"] = body
        assert 'name="payload"' in body
        assert 'name="file:upload-1"' in body
        assert '"method": "mobkit/send_message"' in body
        assert '"type": "image_upload"' in body
        return FakeHttpResponse({
            "jsonrpc": "2.0",
            "id": "test",
            "result": {"accepted": True, "member_id": "m-1", "session_id": "s-1"},
        })

    monkeypatch.setattr(runtime_module.urllib_request, "urlopen", fake_urlopen)
    handle = make_http_mob_handle()
    result = await handle.send("m-1", "look", attachments=[b"png"])
    assert captured["url"] == "http://127.0.0.1:8765/console/rpc/multipart"
    assert result.accepted is True
    assert result.session_id == "s-1"


@pytest.mark.asyncio
async def test_memory_query_uses_assertion_filter_params():
    handle, calls = make_mock_mob_handle({
        "mobkit/memory/query": {
            "assertions": [],
            "conflicts": [],
        }
    })

    await handle.memory_query({
        "entity": "identity:ops",
        "topic": "deployment",
        "store": "knowledge_graph",
    })

    assert calls[0][0] == "mobkit/memory/query"
    assert calls[0][1] == {
        "entity": "identity:ops",
        "topic": "deployment",
        "store": "knowledge_graph",
    }


@pytest.mark.asyncio
async def test_memory_query_keeps_legacy_string_wire_shape():
    handle, calls = make_mock_mob_handle({
        "mobkit/memory/query": {
            "assertions": [],
            "conflicts": [],
        }
    })

    await handle.memory_query("legacy search text", store="knowledge_graph")

    assert calls[0][0] == "mobkit/memory/query"
    assert calls[0][1] == {
        "query": "legacy search text",
        "store": "knowledge_graph",
    }


@pytest.mark.asyncio
async def test_send_with_structured_content_and_attachment_forwards_mode(monkeypatch):
    from meerkat_mobkit import runtime as runtime_module

    def fake_urlopen(req, timeout=60):
        body = req.data.decode("utf-8", errors="replace")
        assert '"handling_mode": "steer"' in body
        assert '"type": "text"' in body
        assert '"type": "image_upload"' in body
        assert '"media_type": "image/jpeg"' in body
        return FakeHttpResponse({
            "jsonrpc": "2.0",
            "id": "test",
            "result": {"accepted": True, "member_id": "m-1", "session_id": "s-2"},
        })

    monkeypatch.setattr(runtime_module.urllib_request, "urlopen", fake_urlopen)
    handle = make_http_mob_handle()
    result = await handle.send(
        "m-1",
        content=[{"type": "text", "text": "hello"}],
        attachments=[(b"jpg", "image/jpeg", "photo.jpg")],
        handling_mode="steer",
    )
    assert result.session_id == "s-2"


@pytest.mark.asyncio
async def test_send_attachment_requires_http_base():
    from meerkat_mobkit.runtime import MobHandle

    runtime = MagicMock()
    runtime.rust_http_base_url = None
    handle = MobHandle.__new__(MobHandle)
    handle._runtime = runtime
    with pytest.raises(NotConnectedError):
        await handle.send("m-1", "x", attachments=[b"png"])


@pytest.mark.asyncio
async def test_upload_blob_uses_multipart(monkeypatch):
    from meerkat_mobkit import runtime as runtime_module

    def fake_urlopen(req, timeout=60):
        body = req.data.decode("utf-8", errors="replace")
        assert '"method": "mobkit/blob/upload"' in body
        assert '"media_type": "image/png"' in body
        return FakeHttpResponse({
            "jsonrpc": "2.0",
            "id": "test",
            "result": {"blob_id": "sha256:abc", "media_type": "image/png", "size": 3},
        })

    monkeypatch.setattr(runtime_module.urllib_request, "urlopen", fake_urlopen)
    handle = make_http_mob_handle()
    result = await handle.upload_blob(b"png", media_type="image/png", filename="a.png")
    assert result["blob_id"] == "sha256:abc"
    assert result["size"] == 3


@pytest.mark.asyncio
async def test_upload_blob_raises_rpc_error(monkeypatch):
    from meerkat_mobkit import runtime as runtime_module

    def fake_urlopen(req, timeout=60):
        return FakeHttpResponse({
            "jsonrpc": "2.0",
            "id": "test",
            "error": {"code": -32602, "message": "bad upload", "data": {"reason": "unit"}},
        })

    monkeypatch.setattr(runtime_module.urllib_request, "urlopen", fake_urlopen)
    handle = make_http_mob_handle()
    with pytest.raises(RpcError) as exc:
        await handle.upload_blob(b"png", media_type="image/png")
    assert exc.value.code == -32602
    assert exc.value.method == "mobkit/blob/upload"


@pytest.mark.asyncio
async def test_upload_blob_requires_http_base():
    from meerkat_mobkit.runtime import MobHandle

    runtime = MagicMock()
    runtime.rust_http_base_url = None
    handle = MobHandle.__new__(MobHandle)
    handle._runtime = runtime
    with pytest.raises(NotConnectedError):
        await handle.upload_blob(b"png", media_type="image/png")


@pytest.mark.asyncio
async def test_collect_completed_parses_wrapped_response():
    """P2 regression: collect_completed response is {"completed": [...]}, not bare list."""
    handle, calls = make_mock_mob_handle({
        "mobkit/collect_completed": {
            "completed": [
                {
                    "member_id": "w1",
                    "snapshot": {
                        "status": "completed",
                        "tokens_used": 100,
                        "is_final": True,
                    },
                }
            ]
        }
    })
    result = await handle.collect_completed()
    assert calls[0][0] == "mobkit/collect_completed"
    assert len(result) == 1
    assert result[0][0] == "w1"
    assert result[0][1].status == "completed"
    assert result[0][1].is_final is True


@pytest.mark.asyncio
async def test_collect_completed_empty():
    """collect_completed returns empty list when no members are terminal."""
    handle, calls = make_mock_mob_handle({
        "mobkit/collect_completed": {"completed": []}
    })
    result = await handle.collect_completed()
    assert result == []


@pytest.mark.asyncio
async def test_member_status_rpc_name():
    handle, calls = make_mock_mob_handle({
        "mobkit/member_status": {
            "status": "active",
            "tokens_used": 42,
            "is_final": False,
        }
    })
    result = await handle.member_status("w1")
    assert calls[0][0] == "mobkit/member_status"
    assert result.tokens_used == 42


@pytest.mark.asyncio
async def test_identity_resolved_tools_rpc_name():
    handle, calls = make_mock_mob_handle({
        "mobkit/identity/resolved_tools": {
            "identity": "domain:security",
            "session_id": "sid-1",
            "tools": ["shell", "send_message"],
        }
    })
    result = await handle.identity_resolved_tools("domain:security")
    assert calls[0][0] == "mobkit/identity/resolved_tools"
    assert calls[0][1] == {"identity": "domain:security"}
    assert result == ["shell", "send_message"]


@pytest.mark.asyncio
async def test_identity_resolved_tools_detail_rpc_name():
    handle, calls = make_mock_mob_handle({
        "mobkit/identity/resolved_tools": {
            "identity": "domain:security",
            "session_id": "sid-1",
            "tools": ["shell"],
        }
    })
    result = await handle.identity_resolved_tools_detail("domain:security")
    assert calls[0][0] == "mobkit/identity/resolved_tools"
    assert result.identity == "domain:security"
    assert result.session_id == "sid-1"
    assert result.tools == ["shell"]


@pytest.mark.asyncio
async def test_force_cancel_member_rpc_name():
    handle, calls = make_mock_mob_handle()
    await handle.force_cancel_member("w1")
    assert calls[0][0] == "mobkit/force_cancel_member"


@pytest.mark.asyncio
async def test_spawn_helper_rpc_name():
    handle, calls = make_mock_mob_handle({
        "mobkit/spawn_helper": {"output": "done", "tokens_used": 10}
    })
    result = await handle.spawn_helper("h1", "do stuff", role="worker")
    assert calls[0][0] == "mobkit/spawn_helper"
    assert calls[0][1]["agent_identity"] == "h1"
    assert calls[0][1]["task"] == "do stuff"
    assert calls[0][1]["options"]["role"] == "worker"
    assert result.output == "done"


@pytest.mark.asyncio
async def test_fork_helper_rpc_name_and_params():
    handle, calls = make_mock_mob_handle({
        "mobkit/fork_helper": {"output": "forked result", "tokens_used": 5}
    })
    result = await handle.fork_helper(
        "lead",
        "fork-1",
        "review this",
        fork_context={"type": "last_messages", "count": 10},
        runtime_mode="turn_driven",
    )
    assert calls[0][0] == "mobkit/fork_helper"
    assert calls[0][1]["source_member_id"] == "lead"
    assert calls[0][1]["fork_context"] == {"type": "last_messages", "count": 10}
    assert calls[0][1]["options"]["runtime_mode"] == "turn_driven"


@pytest.mark.asyncio
async def test_cancel_flow_rpc_name():
    handle, calls = make_mock_mob_handle()
    await handle.cancel_flow("run_123")
    assert calls[0][0] == "mobkit/cancel_flow"


@pytest.mark.asyncio
async def test_flow_status_rpc_name():
    handle, calls = make_mock_mob_handle({
        "mobkit/flow_status": {
            "run_id": "r1",
            "mob_id": "m1",
            "flow_id": "f1",
            "status": "running",
            "step_ledger": [],
            "failure_ledger": [],
        }
    })
    result = await handle.flow_status("r1")
    assert calls[0][0] == "mobkit/flow_status"
    assert result.status == "running"


@pytest.mark.asyncio
async def test_list_flows_rpc_name():
    """list_flows must call mobkit/list_flows and parse the {flows: [...]} envelope."""
    handle, calls = make_mock_mob_handle({
        "mobkit/list_flows": {"flows": ["demo", "pipeline"]}
    })
    result = await handle.list_flows()
    assert calls[0][0] == "mobkit/list_flows"
    assert calls[0][1] is None
    assert result == ["demo", "pipeline"]


@pytest.mark.asyncio
async def test_run_flow_rpc_name_and_params():
    """run_flow must call mobkit/run_flow with flow_id+params and return run_id."""
    handle, calls = make_mock_mob_handle({
        "mobkit/run_flow": {"run_id": "run_abc123"}
    })
    run_id = await handle.run_flow("demo", {"choice": "a"})
    assert calls[0][0] == "mobkit/run_flow"
    assert calls[0][1] == {"flow_id": "demo", "params": {"choice": "a"}}
    assert run_id == "run_abc123"


@pytest.mark.asyncio
async def test_list_runs_rpc_name_no_filter():
    """list_runs must call mobkit/list_runs with no flow_id and parse the full ledger."""
    handle, calls = make_mock_mob_handle({
        "mobkit/list_runs": {
            "runs": [
                {
                    "run_id": "r-1",
                    "mob_id": "m-1",
                    "flow_id": "alpha",
                    "status": "completed",
                    "flow_state": {"phase": "done"},
                    "activation_params": {"k": "v"},
                    "created_at": "2026-04-29T12:00:00Z",
                    "completed_at": "2026-04-29T12:00:05Z",
                    "step_ledger": [
                        {
                            "step_id": "s1",
                            "agent_identity": "lead",
                            "status": "completed",
                            "output": {"text": "ok"},
                            "timestamp": "2026-04-29T12:00:01Z",
                        }
                    ],
                    "failure_ledger": [],
                    "frames": {
                        "frame-1": {"kernel_state": {"opaque": True}},
                    },
                    "loops": {
                        "loop-1": {"kernel_state": {}},
                    },
                    "loop_iteration_ledger": [
                        {
                            "loop_instance_id": "loop-1",
                            "iteration": 0,
                            "frame_id": "frame-2",
                        }
                    ],
                    "schema_version": 4,
                    "root_step_outputs": {"s1": {"text": "ok"}},
                    "loop_iteration_outputs": {"loop-1": []},
                }
            ]
        }
    })
    runs = await handle.list_runs()
    assert calls[0][0] == "mobkit/list_runs"
    assert calls[0][1] == {}
    assert len(runs) == 1
    run = runs[0]
    assert run.run_id == "r-1"
    assert run.flow_id == "alpha"
    assert run.status.value == "completed"
    assert run.activation_params == {"k": "v"}
    assert run.completed_at == "2026-04-29T12:00:05Z"
    assert len(run.step_ledger) == 1
    assert run.step_ledger[0].step_id == "s1"
    # frames / loops are MAPS keyed by id, not arrays.
    assert "frame-1" in run.frames
    assert "loop-1" in run.loops
    assert run.loop_iteration_ledger[0].iteration == 0
    assert run.schema_version == 4


@pytest.mark.asyncio
async def test_list_runs_rpc_name_with_flow_id_filter():
    """list_runs(flow_id=...) must forward flow_id."""
    handle, calls = make_mock_mob_handle({"mobkit/list_runs": {"runs": []}})
    runs = await handle.list_runs(flow_id="alpha")
    assert calls[0][0] == "mobkit/list_runs"
    assert calls[0][1] == {"flow_id": "alpha"}
    assert runs == []


@pytest.mark.asyncio
async def test_query_mob_events_stale_raises_typed_error():
    """A -32010 RpcError must be reified into MobEventsStaleError carrying both cursors."""
    from meerkat_mobkit.errors import MobEventsStaleError, RpcError

    async def stale_rpc(method, params=None):
        raise RpcError(
            code=-32010,
            message="stale mob event cursor: requested 999, latest 42",
            request_id="rid",
            method=method,
            data={"after_cursor": 999, "latest_cursor": 42},
        )

    from meerkat_mobkit.runtime import MobHandle

    runtime = MagicMock()
    runtime._rpc = stale_rpc
    handle = MobHandle.__new__(MobHandle)
    handle._runtime = runtime

    with pytest.raises(MobEventsStaleError) as info:
        await handle.query_mob_events({"after_seq": 999})

    assert info.value.after_cursor == 999
    assert info.value.latest_cursor == 42
    assert info.value.code == -32010


def test_console_timeline_replay_unavailable_uses_distinct_typed_error():
    """Console timeline replay gaps must not be reified as MobEventsStaleError."""
    from meerkat_mobkit.errors import ConsoleTimelineReplayUnavailableError
    from meerkat_mobkit.runtime import _rpc_error_from_payload

    err = _rpc_error_from_payload(
        {
            "code": -32013,
            "message": "query_timeline failed: replay unavailable",
            "data": {
                "error": "replay_unavailable",
                "stream": "timeline",
                "requested_cursor": "console:500",
                "latest_cursor": "console:42",
            },
        },
        request_id="rid",
        method="mobkit/console/query_timeline",
    )

    assert isinstance(err, ConsoleTimelineReplayUnavailableError)
    assert err.code == -32013
    assert err.method == "mobkit/console/query_timeline"
    assert err.data["requested_cursor"] == "console:500"
    assert err.data["latest_cursor"] == "console:42"


def test_lease_lost_reifies_as_lease_lost_error_not_capability_unavailable():
    """Identity-plane lease loss (-32005) must NOT collide with -32004.

    -32004 is CAPABILITY_UNAVAILABLE_CODE which reifies into the
    permanent-capability-gap error type. A transient/recoverable lease loss
    must surface as the distinct LeaseLostError so callers do not give up on
    an identity that merely needs a lease re-acquire.
    """
    from meerkat_mobkit.errors import (
        CapabilityUnavailableError,
        LeaseLostError,
    )
    from meerkat_mobkit.runtime import _rpc_error_from_payload

    err = _rpc_error_from_payload(
        {"code": -32005, "message": "lease lost: review:singleton"},
        request_id="rid",
        method="mobkit/send",
    )

    assert isinstance(err, LeaseLostError)
    assert not isinstance(err, CapabilityUnavailableError)
    assert err.code == -32005
    assert err.method == "mobkit/send"


@pytest.mark.asyncio
async def test_wait_ready_rpc_name():
    """wait_ready must call mobkit/wait_ready and forward timeout in ms."""
    handle, calls = make_mock_mob_handle({
        "mobkit/wait_ready": {"ready": [], "timeout": False}
    })
    result = await handle.wait_ready(timeout=2.5)
    assert calls[0][0] == "mobkit/wait_ready"
    assert calls[0][1] == {"timeout_ms": 2500}
    assert result == {"ready": [], "timeout": False}



@pytest.mark.asyncio
async def test_peer_pubkey_rpc_name():
    """peer_pubkey must call mobkit/peer_pubkey and unwrap pubkey_b64."""
    handle, calls = make_mock_mob_handle({
        "mobkit/peer_pubkey": {"pubkey_b64": "AAAA"}
    })
    result = await handle.peer_pubkey()
    assert calls[0][0] == "mobkit/peer_pubkey"
    assert result == "AAAA"


@pytest.mark.asyncio
async def test_scheduling_evaluate_and_dispatch_rpc_names():
    handle, calls = make_mock_mob_handle({
        "mobkit/scheduling/evaluate": {"due": []},
        "mobkit/scheduling/dispatch": {"dispatched": []},
    })

    evaluate = await handle.scheduling_evaluate([{"id": "daily"}], 1234)
    dispatch = await handle.scheduling_dispatch([{"id": "daily"}], 5678)

    assert calls[0][0] == "mobkit/scheduling/evaluate"
    assert calls[0][1] == {"schedules": [{"id": "daily"}], "tick_ms": 1234}
    assert calls[1][0] == "mobkit/scheduling/dispatch"
    assert calls[1][1] == {"schedules": [{"id": "daily"}], "tick_ms": 5678}
    assert evaluate == {"due": []}
    assert dispatch == {"dispatched": []}


@pytest.mark.asyncio
async def test_session_store_bigquery_rpc_name():
    handle, calls = make_mock_mob_handle({
        "mobkit/session_store/bigquery": {"rows": 1}
    })

    result = await handle.session_store_bigquery(operation="probe")

    assert calls[0][0] == "mobkit/session_store/bigquery"
    assert calls[0][1] == {"operation": "probe"}
    assert result == {"rows": 1}


@pytest.mark.asyncio
async def test_wire_local_forwards_optional_pubkey():
    """wire_local must forward remote_pubkey_b64 when provided."""
    handle, calls = make_mock_mob_handle()
    await handle.wire_local(
        "alice",
        "remote-name",
        "00000000-0000-4000-8000-000000000001",
        "tcp://10.0.0.2:9001",
        remote_pubkey_b64="KioqKioqKioqKioqKioqKioqKioqKioqKioqKioqKio=",
    )
    assert calls[0][0] == "mobkit/cross_mob/wire_local"
    params = calls[0][1]
    assert params["remote_pubkey_b64"] == "KioqKioqKioqKioqKioqKioqKioqKioqKioqKioqKio="
    assert params["remote_address"] == "tcp://10.0.0.2:9001"


@pytest.mark.asyncio
async def test_wire_local_omits_pubkey_when_absent():
    """Backward compat: inproc-only callers must not see remote_pubkey_b64
    leak into the wire-local params dict."""
    handle, calls = make_mock_mob_handle()
    await handle.wire_local(
        "alice",
        "remote-name",
        "00000000-0000-4000-8000-000000000001",
        "inproc://remote-name",
    )
    assert calls[0][0] == "mobkit/cross_mob/wire_local"
    assert "remote_pubkey_b64" not in calls[0][1]
