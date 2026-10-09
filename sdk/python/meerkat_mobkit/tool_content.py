"""Rich content blocks for callback tool-handler results.

A callback tool handler (registered via :meth:`SessionBuildOptions.register_tool`
or :meth:`AgentBuildDraft.register_tool`) normally returns a JSON-serializable
value, which the model sees as text. To hand the model an **image** or multiple
content blocks, wrap them with :func:`tool_content` and return that — the gateway
then delivers them as real content blocks instead of text::

    def screenshot_tool(args):
        png_b64 = capture()  # base64-encoded PNG bytes
        return tool_content(
            text_block("Here is the screenshot:"),
            image_block("image/png", png_b64),
        )

Rich content is **opt-in**: only a :class:`ToolResultContent` (from
:func:`tool_content`) is delivered as content blocks. A plain return value — a
string, dict, or even a bare ``list`` — keeps the default text behavior, so a
tool that returns ordinary list/dict data is never reinterpreted. The block
shapes mirror the runtime's ``ContentBlock`` wire format (internally tagged by
``type``); blocks are parsed strictly, so extra keys on a block are dropped.
"""
from __future__ import annotations

import json
import re
from dataclasses import dataclass
from typing import Any

__all__ = [
    "text_block",
    "structured_block",
    "console_widget_block",
    "image_block",
    "image_blob_block",
    "tool_content",
    "ToolResultContent",
]


def text_block(text: str) -> dict[str, Any]:
    """A text content block."""
    return {"type": "text", "text": text}


def structured_block(data: Any) -> dict[str, Any]:
    """Canonical JSON content, preserved as structured data by the runtime."""
    # Copy to JSON values and reject NaN/infinity before crossing the callback wire.
    return {"type": "structured", "data": json.loads(json.dumps(data, allow_nan=False))}


def console_widget_block(
    widget_type: str, *, version: int = 1, data: Any, fallback: str,
) -> dict[str, Any]:
    """Opt in to a registered console widget without changing tool execution.

    ``widget_type`` is an application-owned name such as ``acme/search-result``;
    ``version`` selects that renderer's data contract. The plain-text fallback
    remains readable when the renderer is unavailable. No code or module URL is
    loaded from tool output. Combine this with other blocks using ``tool_content``.
    """
    if (not isinstance(widget_type, str)
            or not re.fullmatch(r"[a-zA-Z0-9][a-zA-Z0-9._-]*/[a-zA-Z0-9][a-zA-Z0-9._-]*", widget_type)
            or widget_type.startswith("mobkit/")):
        raise ValueError("Widget type must be an application-owned namespaced name")
    if type(version) is not int or not 1 <= version <= 9007199254740991:
        raise ValueError("Widget version must be a positive JSON-safe integer")
    if not isinstance(fallback, str) or not fallback.strip():
        raise ValueError("Widget fallback must be nonempty text")
    return structured_block({"console_widget": {
        "type": widget_type, "version": version, "data": data, "fallback": fallback,
    }})


def image_block(media_type: str, data: str) -> dict[str, Any]:
    """An inline image content block.

    Args:
        media_type: MIME type, e.g. ``"image/png"`` or ``"image/jpeg"``.
        data: Base64-encoded image bytes.
    """
    return {"type": "image", "media_type": media_type, "source": "inline", "data": data}


def image_blob_block(media_type: str, blob_id: str) -> dict[str, Any]:
    """An image content block referencing a durable blob by id.

    Use when the image already lives in the runtime blob store; prefer
    :func:`image_block` when the handler has the bytes in hand.
    """
    return {"type": "image", "media_type": media_type, "source": "blob", "blob_id": blob_id}


@dataclass(frozen=True)
class ToolResultContent:
    """Explicit rich tool result: a list of content blocks for the model.

    Return an instance (most easily via :func:`tool_content`) from a callback
    tool handler to deliver images / multiple blocks. Returning anything else
    keeps the default single-text-block behavior.
    """

    blocks: list[dict[str, Any]]


def tool_content(*blocks: dict[str, Any]) -> ToolResultContent:
    """Bundle content blocks into a rich tool result.

    Build the blocks with :func:`text_block`, :func:`image_block`, or
    :func:`image_blob_block`, :func:`structured_block`, or :func:`console_widget_block`.
    """
    return ToolResultContent(list(blocks))
