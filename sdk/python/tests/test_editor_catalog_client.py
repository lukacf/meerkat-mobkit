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
