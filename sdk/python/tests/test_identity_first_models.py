"""TDD tests for identity-first models: DurableAgentSpec, DispatchInput, ContentBlock (REQ-40, REQ-43)."""
import pytest

from meerkat_mobkit.identity_first_models import (
    ContentBlock,
    ContinuityHealth,
    DispatchInput,
    DurableAgentSpec,
    IdentityStatus,
    ImageBlock,
    LeaseInfo,
    ManagedPeerEdge,
    TextBlock,
)


class TestDurableAgentSpec:
    """REQ-40: DurableAgentSpec dataclass with all fields, from_dict/to_dict."""

    def test_all_fields(self):
        spec = DurableAgentSpec(
            identity="triage:main",
            profile="assistant",
            addressability="addressable",
            display_name="Triage Agent",
            labels={"env": "prod"},
            context={"key": "value"},
            additional_instructions=["Be concise."],
        )
        assert spec.identity == "triage:main"
        assert spec.profile == "assistant"
        assert spec.addressability == "addressable"
        assert spec.display_name == "Triage Agent"
        assert spec.labels == {"env": "prod"}
        assert spec.context == {"key": "value"}
        assert spec.additional_instructions == ["Be concise."]

    def test_addressability_defaults_to_addressable(self):
        spec = DurableAgentSpec(identity="a:main", profile="default")
        assert spec.addressability == "addressable"

    def test_optional_fields_default(self):
        spec = DurableAgentSpec(identity="a:main", profile="default")
        assert spec.display_name is None
        assert spec.labels == {}
        assert spec.context is None
        assert spec.additional_instructions == []

    def test_to_dict(self):
        spec = DurableAgentSpec(
            identity="triage:main",
            profile="assistant",
            display_name="Triage",
            labels={"env": "prod"},
            context={"k": "v"},
            additional_instructions=["inst1"],
        )
        d = spec.to_dict()
        assert d["identity"] == "triage:main"
        assert d["profile"] == "assistant"
        assert d["addressability"] == "addressable"
        assert d["display_name"] == "Triage"
        assert d["labels"] == {"env": "prod"}
        assert d["context"] == {"k": "v"}
        assert d["additional_instructions"] == ["inst1"]

    def test_to_dict_omits_none_optional_fields(self):
        spec = DurableAgentSpec(identity="a:main", profile="default")
        d = spec.to_dict()
        assert "display_name" not in d
        assert "context" not in d
        # labels and additional_instructions are always present (empty)
        assert d["labels"] == {}
        assert d["additional_instructions"] == []

    def test_from_dict_full(self):
        data = {
            "identity": "gate:main",
            "profile": "gatekeeper",
            "addressability": "internal_only",
            "display_name": "Gate",
            "labels": {"role": "gate"},
            "context": {"priority": 1},
            "additional_instructions": ["Check all."],
        }
        spec = DurableAgentSpec.from_dict(data)
        assert spec.identity == "gate:main"
        assert spec.profile == "gatekeeper"
        assert spec.addressability == "internal_only"
        assert spec.display_name == "Gate"
        assert spec.labels == {"role": "gate"}
        assert spec.context == {"priority": 1}
        assert spec.additional_instructions == ["Check all."]

    def test_from_dict_minimal(self):
        data = {"identity": "a:main", "profile": "default"}
        spec = DurableAgentSpec.from_dict(data)
        assert spec.identity == "a:main"
        assert spec.profile == "default"
        assert spec.addressability == "addressable"
        assert spec.display_name is None
        assert spec.labels == {}
        assert spec.context is None
        assert spec.additional_instructions == []

    def test_round_trip(self):
        original = DurableAgentSpec(
            identity="triage:main",
            profile="assistant",
            addressability="internal_only",
            display_name="Triage",
            labels={"env": "test"},
            context={"nested": {"deep": True}},
            additional_instructions=["a", "b"],
        )
        restored = DurableAgentSpec.from_dict(original.to_dict())
        assert restored.identity == original.identity
        assert restored.profile == original.profile
        assert restored.addressability == original.addressability
        assert restored.display_name == original.display_name
        assert restored.labels == original.labels
        assert restored.context == original.context
        assert restored.additional_instructions == original.additional_instructions


class TestContentBlock:
    """REQ-43: TextBlock and ImageBlock content blocks."""

    def test_text_block(self):
        block = TextBlock(text="Hello world")
        assert block.text == "Hello world"

    def test_text_block_to_dict(self):
        block = TextBlock(text="Hello")
        d = block.to_dict()
        assert d == {"type": "text", "text": "Hello"}

    def test_text_block_from_dict(self):
        block = ContentBlock.from_dict({"type": "text", "text": "Hi"})
        assert isinstance(block, TextBlock)
        assert block.text == "Hi"

    def test_image_block(self):
        block = ImageBlock(media_type="image/png", data="base64data")
        assert block.media_type == "image/png"
        assert block.data == "base64data"
        assert block.source == "inline"
        assert block.blob_id is None

    def test_image_block_to_dict(self):
        block = ImageBlock(media_type="image/jpeg", data="abc123")
        d = block.to_dict()
        assert d == {
            "type": "image",
            "media_type": "image/jpeg",
            "source": "inline",
            "data": "abc123",
        }

    def test_blob_image_block_to_dict(self):
        block = ImageBlock(
            media_type="image/png",
            source="blob",
            blob_id="sha256:" + "a" * 64,
        )
        d = block.to_dict()
        assert d == {
            "type": "image",
            "media_type": "image/png",
            "source": "blob",
            "blob_id": "sha256:" + "a" * 64,
        }

    def test_image_block_from_dict(self):
        block = ContentBlock.from_dict(
            {"type": "image", "media_type": "image/png", "data": "xyz"}
        )
        assert isinstance(block, ImageBlock)
        assert block.media_type == "image/png"
        assert block.data == "xyz"
        assert block.source == "inline"

    def test_blob_image_block_from_dict(self):
        block = ContentBlock.from_dict(
            {
                "type": "image",
                "media_type": "image/webp",
                "source": "blob",
                "blob_id": "sha256:" + "b" * 64,
            }
        )
        assert isinstance(block, ImageBlock)
        assert block.media_type == "image/webp"
        assert block.source == "blob"
        assert block.blob_id == "sha256:" + "b" * 64

    def test_unknown_image_source_raises(self):
        with pytest.raises(ValueError, match="image source"):
            ContentBlock.from_dict(
                {
                    "type": "image",
                    "media_type": "image/png",
                    "source": "remote_url",
                    "data": "abc",
                }
            )

    def test_unknown_type_raises(self):
        with pytest.raises(ValueError, match="unknown content block type"):
            ContentBlock.from_dict({"type": "video", "url": "..."})


class TestDispatchInput:
    """REQ-43: DispatchInput with typed content, validated origin, wire format."""

    def test_string_content(self):
        di = DispatchInput(content="Hello", origin="connector")
        assert di.content == "Hello"
        assert di.origin == "connector"

    def test_block_content(self):
        blocks = [TextBlock(text="Hi"), ImageBlock(media_type="image/png", data="abc")]
        di = DispatchInput(content=blocks, origin="scheduler")
        assert len(di.content) == 2

    def test_valid_origins(self):
        for origin in ("connector", "scheduler", "policy", "flow", "system"):
            di = DispatchInput(content="x", origin=origin)
            assert di.origin == origin

    def test_invalid_origin_raises(self):
        with pytest.raises(ValueError, match="origin"):
            DispatchInput(content="x", origin="invalid")

    def test_optional_fields_default_none(self):
        di = DispatchInput(content="x", origin="system")
        assert di.correlation_id is None
        assert di.idempotency_key is None

    def test_optional_fields_set(self):
        di = DispatchInput(
            content="x",
            origin="flow",
            correlation_id="corr-1",
            idempotency_key="idem-1",
        )
        assert di.correlation_id == "corr-1"
        assert di.idempotency_key == "idem-1"

    def test_to_dict_string_content(self):
        di = DispatchInput(content="Hello", origin="connector")
        d = di.to_dict()
        assert d["content"] == "Hello"
        assert d["origin"] == "connector"
        assert "correlation_id" not in d
        assert "idempotency_key" not in d

    def test_to_dict_block_content(self):
        blocks = [TextBlock(text="Hi"), ImageBlock(media_type="image/png", data="abc")]
        di = DispatchInput(content=blocks, origin="system")
        d = di.to_dict()
        assert d["content"] == [
            {"type": "text", "text": "Hi"},
            {
                "type": "image",
                "media_type": "image/png",
                "source": "inline",
                "data": "abc",
            },
        ]

    def test_to_dict_with_optional_fields(self):
        di = DispatchInput(
            content="x",
            origin="policy",
            correlation_id="c1",
            idempotency_key="k1",
        )
        d = di.to_dict()
        assert d["correlation_id"] == "c1"
        assert d["idempotency_key"] == "k1"

    def test_from_dict_string_content(self):
        di = DispatchInput.from_dict({
            "content": "Hello",
            "origin": "connector",
        })
        assert di.content == "Hello"
        assert di.origin == "connector"

    def test_from_dict_block_content(self):
        di = DispatchInput.from_dict({
            "content": [
                {"type": "text", "text": "Hi"},
                {"type": "image", "media_type": "image/png", "data": "abc"},
            ],
            "origin": "scheduler",
        })
        assert isinstance(di.content, list)
        assert len(di.content) == 2
        assert isinstance(di.content[0], TextBlock)
        assert isinstance(di.content[1], ImageBlock)

    def test_from_dict_with_optional_fields(self):
        di = DispatchInput.from_dict({
            "content": "x",
            "origin": "flow",
            "correlation_id": "c1",
            "idempotency_key": "k1",
        })
        assert di.correlation_id == "c1"
        assert di.idempotency_key == "k1"

    def test_round_trip(self):
        original = DispatchInput(
            content=[TextBlock(text="Hi")],
            origin="policy",
            correlation_id="c-1",
            idempotency_key="k-1",
        )
        restored = DispatchInput.from_dict(original.to_dict())
        assert restored.origin == original.origin
        assert restored.correlation_id == original.correlation_id
        assert restored.idempotency_key == original.idempotency_key
        assert isinstance(restored.content, list)
        assert len(restored.content) == 1
        assert isinstance(restored.content[0], TextBlock)
        assert restored.content[0].text == "Hi"


class TestManagedPeerEdge:
    """REQ-43a: ManagedPeerEdge dataclass."""

    def test_fields(self):
        edge = ManagedPeerEdge(a="agent:alpha", b="agent:beta")
        assert edge.a == "agent:alpha"
        assert edge.b == "agent:beta"

    def test_to_dict(self):
        edge = ManagedPeerEdge(a="a:main", b="b:main")
        assert edge.to_dict() == {"a": "a:main", "b": "b:main"}

    def test_from_dict(self):
        edge = ManagedPeerEdge.from_dict({"a": "x:1", "b": "y:2"})
        assert edge.a == "x:1"
        assert edge.b == "y:2"

    def test_round_trip(self):
        original = ManagedPeerEdge(a="p:1", b="q:2")
        restored = ManagedPeerEdge.from_dict(original.to_dict())
        assert restored.a == original.a
        assert restored.b == original.b


class TestCrossSdkGracefulParsing:
    """Python from_dict must degrade gracefully (like TS) on missing keys,
    not raise KeyError. Regression for the cross-SDK divergence."""

    def test_identity_status_defaults_missing_keys(self):
        status = IdentityStatus.from_dict({})
        assert status.identity == ""
        assert status.state == ""
        assert status.addressability == "addressable"
        assert status.lease is None
        assert status.continuity_health is None

    def test_lease_info_defaults_missing_keys(self):
        lease = LeaseInfo.from_dict({})
        assert lease.fencing_token == 0
        assert lease.ttl_remaining_ms == 0
        assert lease.healthy is False

    def test_continuity_health_defaults_missing_keys(self):
        health = ContinuityHealth.from_dict({})
        assert health.store_reachable is False
        assert health.durability_policy.kind == "sync_write_through"
        assert health.last_checkpoint_version is None
