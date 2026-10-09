/**
 * Rich content blocks for callback tool-handler results.
 *
 * A callback tool handler (registered via {@link SessionBuildOptions.registerTool}
 * or {@link AgentBuildDraft.registerTool}) normally returns a JSON-serializable
 * value, which the model sees as text. To hand the model an **image** or
 * multiple content blocks, wrap them with {@link toolContent} and return that —
 * the gateway then delivers them as real content blocks instead of text:
 *
 * ```ts
 * draft.registerTool("screenshot", async () => {
 *   const pngB64 = await capture();
 *   return toolContent(textBlock("Here is the screenshot:"), imageBlock("image/png", pngB64));
 * });
 * ```
 *
 * Rich content is **opt-in**: only a {@link ToolResultContent} (from
 * {@link toolContent}) is delivered as content blocks. A plain return value — a
 * string, object, or even a bare array — keeps the default text behavior, so a
 * tool that returns ordinary array/object data is never reinterpreted. The block
 * shapes mirror the runtime's `ContentBlock` wire format (tagged by `type`);
 * blocks are parsed strictly, so extra keys on a block are dropped.
 */

export type ContentBlock = Record<string, unknown>;
export type JsonContentValue = null | boolean | number | string | JsonContentValue[] | { [key: string]: JsonContentValue };

/** Application-owned data contract selected by widget type and version. */
export interface ConsoleWidgetMetadata {
  type: string;
  version: number;
  data: JsonContentValue;
  fallback: string;
}

/** JSON content preserved as structured data by the runtime. */
export function structuredBlock(data: JsonContentValue): ContentBlock {
  const encoded = JSON.stringify(data, (_key, value) => {
    if (value === undefined || typeof value === "function" || typeof value === "symbol"
      || (typeof value === "number" && !Number.isFinite(value))) throw new Error("Content must be JSON data");
    return value;
  });
  return { type: "structured", data: JSON.parse(encoded) };
}

/** Explicit console presentation metadata, with no execution or loading authority. */
export function consoleWidgetBlock(widget: ConsoleWidgetMetadata): ContentBlock {
  if (typeof widget.type !== "string" || !/^[a-zA-Z0-9][a-zA-Z0-9._-]*\/[a-zA-Z0-9][a-zA-Z0-9._-]*$/.test(widget.type)
    || widget.type.startsWith("mobkit/")) throw new Error("Widget type must be an application-owned namespaced name");
  if (!Number.isSafeInteger(widget.version) || widget.version < 1) throw new Error("Widget version must be a positive JSON-safe integer");
  if (typeof widget.fallback !== "string" || !widget.fallback.trim()) throw new Error("Widget fallback must be nonempty text");
  return structuredBlock({ console_widget: { type: widget.type, version: widget.version, data: widget.data, fallback: widget.fallback } });
}

/** A text content block. */
export function textBlock(text: string): ContentBlock {
  return { type: "text", text };
}

/**
 * An inline image content block.
 *
 * @param mediaType MIME type, e.g. `"image/png"` or `"image/jpeg"`.
 * @param data Base64-encoded image bytes.
 */
export function imageBlock(mediaType: string, data: string): ContentBlock {
  return { type: "image", media_type: mediaType, source: "inline", data };
}

/**
 * An image content block referencing a durable blob by id. Prefer
 * {@link imageBlock} when the handler has the bytes in hand.
 */
export function imageBlobBlock(mediaType: string, blobId: string): ContentBlock {
  return { type: "image", media_type: mediaType, source: "blob", blob_id: blobId };
}

/**
 * Explicit rich tool result: a list of content blocks for the model. Return one
 * (most easily via {@link toolContent}) from a callback tool handler to deliver
 * images / multiple blocks. Returning anything else keeps the default
 * single-text-block behavior.
 */
export class ToolResultContent {
  constructor(public readonly blocks: ContentBlock[]) {}
}

/** Bundle content blocks into a rich tool result. */
export function toolContent(...blocks: ContentBlock[]): ToolResultContent {
  return new ToolResultContent(blocks);
}
