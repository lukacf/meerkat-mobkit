import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { CallbackDispatcher } from "../src/agent-builder.js";
import { consoleWidgetBlock, imageBlock, structuredBlock, textBlock, toolContent } from "../src/tool-content.js";

test("rich widgets and images survive the real callback dispatcher", async () => {
  const fixture = JSON.parse(readFileSync(new URL("../../../crates/meerkat-mobkit/tests/fixtures/console-widget-rich-result.json", import.meta.url), "utf8"));
  const dispatcher = new CallbackDispatcher();
  dispatcher.registerBuilder({ async buildAgent(options) {
    options.registerTool("find_records", () => toolContent(
      textBlock("Search receipt"),
      imageBlock("image/png", fixture.callback.content_blocks[1].data),
      consoleWidgetBlock({ type: "example/result-count", version: 1, data: { count: 7 }, fallback: "Found seven matching records." }),
    ));
  } });
  await dispatcher.handleCallback("callback/build_agent", { options: { scope_id: "fixture" } });
  const result = await dispatcher.handleCallback("callback/call_tool", { scope_id: "fixture", tool: "find_records", arguments: {} });
  assert.deepEqual(result, fixture.callback);
});

test("widget metadata requires a valid namespaced contract and JSON data", () => {
  const valid = { type: "example/result", version: 1, data: {}, fallback: "Result" };
  for (const type of ["plain", "mobkit/reserved", "app/nested/name"]) {
    assert.throws(() => consoleWidgetBlock({ ...valid, type }));
  }
  for (const version of [0, -1, 1.5, Number.MAX_SAFE_INTEGER + 1]) {
    assert.throws(() => consoleWidgetBlock({ ...valid, version }));
  }
  assert.throws(() => consoleWidgetBlock({ ...valid, fallback: " " }));
  assert.throws(() => structuredBlock({ count: Infinity }));
});
