"""Tests for low-level MobKit editor catalog typed-client wrappers."""
from __future__ import annotations

import pytest

from meerkat_mobkit._client import MobkitAsyncTypedClient, MobkitTypedClient


def _response(request, result):
    return {"jsonrpc": "2.0", "id": request["id"], "result": result}


def test_sync_typed_client_editor_catalog_methods_call_exact_rpc_names():
    calls = []
    responses = {
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
    }

    def transport(request):
        calls.append(request["method"])
        return _response(request, responses[request["method"]])

    client = MobkitTypedClient.from_persistent(transport)
    assert client.tools_catalog()["tool_catalog"] == [{"id": "shell"}]
    assert client.skills_catalog()["skill_realms"] == [{"id": "mobkit/authoring"}]
    assert client.agent_definitions()["agent_definitions"] == [{"id": "authoring:reviewer"}]
    assert client.mobpack_templates()["sample_mobpacks"] == [{"id": "sample"}]
    assert client.mobpack_catalogs()["sources"] == {"tools": "mobkit/tools/catalog"}
    assert calls == [
        "mobkit/tools/catalog",
        "mobkit/skills/catalog",
        "mobkit/agent_definitions/list",
        "mobkit/mobpacks/templates",
        "mobkit/mobpacks/catalogs",
    ]


@pytest.mark.asyncio
async def test_async_typed_client_editor_catalog_methods_call_exact_rpc_names():
    calls = []

    async def transport(request):
        calls.append(request["method"])
        return _response(
            request,
            {
                "schema_version": "mobpack.editor.v1",
                "runtime_backed": False,
                "source": "mobkit/tool-config",
                "authoring_provider": {"id": "standalone_authoring"},
                "tool_catalog": [],
                "skill_realms": [],
                "agent_definitions": [],
                "blank_mobpack": {},
                "sample_mobpacks": [],
                "sample_agent_definitions": [],
                "templates": {},
                "sources": {},
                "models": [],
                "provider_defaults": [],
            },
        )

    client = MobkitAsyncTypedClient(transport)
    await client.tools_catalog()
    await client.skills_catalog()
    await client.agent_definitions()
    await client.mobpack_templates()
    await client.mobpack_catalogs()
    assert calls == [
        "mobkit/tools/catalog",
        "mobkit/skills/catalog",
        "mobkit/agent_definitions/list",
        "mobkit/mobpacks/templates",
        "mobkit/mobpacks/catalogs",
    ]


def _validation_payload():
    return {
        "ok": True,
        "diagnostics": [],
        "display_rows": [],
        "flow_ids": ["flow_a"],
        "validation_source": "mobkit/mobpacks/validate",
        "deploy_command": "rkat mob deploy",
    }


def _draft_row_payload(revision=1, can_undo=False, can_redo=False):
    return {
        "id": "f_demo",
        "name": "Demo",
        "stage": "draft",
        "revision": revision,
        "etag": f"f_demo:{revision}",
        "document": {"mob_id": "demo"},
        "validation": {"ok": True},
        "can_undo": can_undo,
        "can_redo": can_redo,
    }


_MOBPACK_AUTHORING_RESPONSES = {
    "mobkit/mobpacks/validate": _validation_payload(),
    "mobkit/mobpacks/source": {
        "filename": "demo.mobpack",
        "media_type": "application/vnd.meerkat.mobpack",
        "mob_toml": "[mob]\n",
        "source_files": [],
        "validation": _validation_payload(),
        "source": "mobkit/mobpacks/source",
    },
    "mobkit/mobpacks/export": {
        "filename": "demo.mobpack",
        "media_type": "application/vnd.meerkat.mobpack",
        "content_base64": "UEsDBA==",
        "mob_toml": "[mob]\n",
        "source_files": [],
        "validation": _validation_payload(),
    },
    "mobkit/mobpacks/import": {
        "document": {"mob_id": "demo"},
        "validation": _validation_payload(),
        "source": "mobkit/mobpacks/import:mob.toml",
        "source_label": "mob.toml",
        "source_media_type": "text/x-toml",
    },
    "mobkit/mobpacks/list": {
        "source": "mobkit/mobpacks/list",
        "store_path": "/tmp/drafts.json",
        "runtime_backed": False,
        "rows": [_draft_row_payload()],
    },
    "mobkit/mobpacks/get": {
        "source": "mobkit/mobpacks/get",
        "store_path": "/tmp/drafts.json",
        "runtime_backed": False,
        "row": _draft_row_payload(),
    },
    "mobkit/mobpacks/create": {
        "source": "mobkit/mobpacks/create",
        "store_path": "/tmp/drafts.json",
        "row": _draft_row_payload(),
        "rows": [_draft_row_payload()],
    },
    "mobkit/mobpacks/save": {
        "source": "mobkit/mobpacks/save",
        "store_path": "/tmp/drafts.json",
        "row": _draft_row_payload(revision=2),
        "rows": [_draft_row_payload(revision=2)],
    },
    "mobkit/mobpacks/undo": {
        "source": "mobkit/mobpacks/undo",
        "store_path": "/tmp/drafts.json",
        "stepped": True,
        "row": _draft_row_payload(revision=3, can_redo=True),
        "rows": [_draft_row_payload(revision=3, can_redo=True)],
    },
    "mobkit/mobpacks/redo": {
        "source": "mobkit/mobpacks/redo",
        "store_path": "/tmp/drafts.json",
        "stepped": False,
        "reason": "nothing to redo",
        "row": _draft_row_payload(revision=3, can_redo=True),
        "rows": [_draft_row_payload(revision=3, can_redo=True)],
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
        "document": {"mob_id": "demo"},
        "selection": {"kind": "agent", "id": "reviewer"},
        "validation": _validation_payload(),
    },
    "mobkit/mobpacks/deploy_command": {
        "command": "rkat mob deploy demo.mobpack",
        "argv": ["rkat", "mob", "deploy", "demo.mobpack"],
        "deploy_command": "rkat mob deploy",
        "filename": "demo.mobpack",
        "validation": _validation_payload(),
        "source": "meerkat_mobkit::mobpack::deploy_argv",
    },
    "mobkit/mobpacks/deploy": {
        "filename": "demo.mobpack",
        "pack_path": "/tmp/demo.mobpack",
        "pack_sha256": "deadbeef",
        "command": "rkat mob deploy /tmp/demo.mobpack",
        "argv": ["rkat", "mob", "deploy", "/tmp/demo.mobpack"],
        "plan_trace": [],
        "executed": False,
        "success": False,
        "validation": _validation_payload(),
        "display_rows": [],
    },
}

_EXPECTED_MOBPACK_AUTHORING_CALLS = [
    ("mobkit/mobpacks/validate", {"document": {"mob_id": "demo"}, "rkat_validate": True}),
    ("mobkit/mobpacks/source", {"document": {"mob_id": "demo"}}),
    ("mobkit/mobpacks/export", {"document": {"mob_id": "demo"}}),
    ("mobkit/mobpacks/import", {"mob_toml": "[mob]\n", "source_name": "mob.toml"}),
    ("mobkit/mobpacks/list", {}),
    ("mobkit/mobpacks/get", {"id": "f_demo"}),
    ("mobkit/mobpacks/create", {"template": "blank", "name": "Demo"}),
    (
        "mobkit/mobpacks/save",
        {
            "id": "f_demo",
            "document": {"mob_id": "demo"},
            "stage": "draft",
            "expected_revision": 1,
            "expected_etag": "f_demo:1",
        },
    ),
    (
        "mobkit/mobpacks/undo",
        {"id": "f_demo", "expected_revision": 2, "expected_etag": "f_demo:2"},
    ),
    ("mobkit/mobpacks/redo", {"id": "f_demo"}),
    ("mobkit/mobpacks/delete", {"id": "f_demo", "expected_revision": 2}),
    (
        "mobkit/mobpacks/apply_operation",
        {
            "document": {"mob_id": "demo"},
            "operation": {"type": "add_member"},
            "expected_catalog_snapshot_id": "snap-1",
        },
    ),
    ("mobkit/mobpacks/deploy_command", {"document": {"mob_id": "demo"}}),
    ("mobkit/mobpacks/deploy", {"document": {"mob_id": "demo"}, "execute": False}),
]


def test_sync_typed_client_mobpack_authoring_methods_call_exact_rpc_names():
    calls = []

    def transport(request):
        calls.append((request["method"], request["params"]))
        return _response(request, _MOBPACK_AUTHORING_RESPONSES[request["method"]])

    document = {"mob_id": "demo"}
    client = MobkitTypedClient.from_persistent(transport)
    assert client.mobpack_validate(document, rkat_validate=True)["ok"] is True
    assert client.mobpack_source(document)["mob_toml"] == "[mob]\n"
    assert client.mobpack_export(document)["content_base64"] == "UEsDBA=="
    assert (
        client.mobpack_import(mob_toml="[mob]\n", source_name="mob.toml")["document"]
        == {"mob_id": "demo"}
    )
    assert client.mobpack_list()["rows"][0]["id"] == "f_demo"
    assert client.mobpack_get("f_demo")["row"]["etag"] == "f_demo:1"
    assert client.mobpack_create(template="blank", name="Demo")["row"]["id"] == "f_demo"
    assert (
        client.mobpack_save(
            "f_demo",
            document,
            stage="draft",
            expected_revision=1,
            expected_etag="f_demo:1",
        )["row"]["revision"]
        == 2
    )
    undone = client.mobpack_undo(
        "f_demo", expected_revision=2, expected_etag="f_demo:2"
    )
    assert undone["stepped"] is True
    assert undone["row"]["can_redo"] is True
    redone = client.mobpack_redo("f_demo")
    assert redone["stepped"] is False
    assert redone["reason"] == "nothing to redo"
    assert client.mobpack_delete("f_demo", expected_revision=2)["deleted"] is True
    assert (
        client.mobpack_apply_operation(
            document,
            {"type": "add_member"},
            expected_catalog_snapshot_id="snap-1",
        )["selection"]
        == {"kind": "agent", "id": "reviewer"}
    )
    assert client.mobpack_deploy_command(document)["deploy_command"] == "rkat mob deploy"
    assert client.mobpack_deploy(document, execute=False)["executed"] is False
    assert calls == _EXPECTED_MOBPACK_AUTHORING_CALLS


@pytest.mark.asyncio
async def test_async_typed_client_mobpack_authoring_methods_call_exact_rpc_names():
    calls = []

    async def transport(request):
        calls.append((request["method"], request["params"]))
        return _response(request, _MOBPACK_AUTHORING_RESPONSES[request["method"]])

    document = {"mob_id": "demo"}
    client = MobkitAsyncTypedClient(transport)
    await client.mobpack_validate(document, rkat_validate=True)
    await client.mobpack_source(document)
    await client.mobpack_export(document)
    await client.mobpack_import(mob_toml="[mob]\n", source_name="mob.toml")
    await client.mobpack_list()
    await client.mobpack_get("f_demo")
    await client.mobpack_create(template="blank", name="Demo")
    await client.mobpack_save(
        "f_demo",
        document,
        stage="draft",
        expected_revision=1,
        expected_etag="f_demo:1",
    )
    await client.mobpack_undo(
        "f_demo", expected_revision=2, expected_etag="f_demo:2"
    )
    await client.mobpack_redo("f_demo")
    await client.mobpack_delete("f_demo", expected_revision=2)
    await client.mobpack_apply_operation(
        document,
        {"type": "add_member"},
        expected_catalog_snapshot_id="snap-1",
    )
    await client.mobpack_deploy_command(document)
    await client.mobpack_deploy(document, execute=False)
    assert calls == _EXPECTED_MOBPACK_AUTHORING_CALLS


def test_sync_typed_client_mobpack_validate_rejects_invalid_payload():
    def transport(request):
        return _response(request, {"ok": "yes"})

    client = MobkitTypedClient.from_persistent(transport)
    with pytest.raises(ValueError, match="invalid result payload"):
        client.mobpack_validate({"mob_id": "demo"})
