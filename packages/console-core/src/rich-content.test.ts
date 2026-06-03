import assert from "node:assert/strict";
import test from "node:test";

import { parseConversationRichBlocks, parseConversationSummary, renderConversationInlineMarkdown, safeConsoleHref } from "./rich-content";

test("parseConversationRichBlocks parses markdown tables into structured table blocks", () => {
  const blocks = parseConversationRichBlocks(`
| Surface | Status | Notes |
| :--- | :---: | ---: |
| Transcript | done | 1 |
| Sidebar | next | 2 |
    `);

  assert.equal(blocks.length, 1);
  assert.deepEqual(blocks[0], {
    type: "table",
    headers: ["Surface", "Status", "Notes"],
    alignments: ["left", "center", "right"],
    rows: [
      ["Transcript", "done", "1"],
      ["Sidebar", "next", "2"],
    ],
  });
});

test("parseConversationSummary parses summary paragraphs into structured summary metadata", () => {
  const summary = parseConversationSummary(`
2 files changed +12 -3
packages/console-core/src/conversation.ts +8 -0
desktop/renderer/src/app/App.tsx +4 -3
    `);

  assert.deepEqual(summary, {
    title: "2 files changed",
    plus: 12,
    minus: 3,
    files: [
      { name: "packages/console-core/src/conversation.ts", plus: 8, minus: 0 },
      { name: "desktop/renderer/src/app/App.tsx", plus: 4, minus: 3 },
    ],
  });
});

test("renderConversationInlineMarkdown does not italicize underscores inside identifiers", () => {
  assert.equal(renderConversationInlineMarkdown("ORDER_THREE_OK"), "ORDER_THREE_OK");
  assert.equal(renderConversationInlineMarkdown("api_investigator"), "api_investigator");
  assert.equal(renderConversationInlineMarkdown("Use _emphasis_ here."), "Use <em>emphasis</em> here.");
});

test("renderConversationInlineMarkdown allowlists link schemes", () => {
  assert.equal(
    renderConversationInlineMarkdown("[Docs](https://example.test/docs)"),
    '<a href="https://example.test/docs" rel="noreferrer">Docs</a>',
  );
  assert.equal(
    renderConversationInlineMarkdown("[Local](/console) [Unsafe](javascript:alert(1))"),
    '<a href="/console" rel="noreferrer">Local</a> Unsafe',
  );
  assert.equal(
    renderConversationInlineMarkdown("[Data](data:text/plain;base64,abc)"),
    "Data",
  );
  assert.equal(safeConsoleHref("//evil.test/phish"), null);
  assert.equal(safeConsoleHref("mailto:ops@example.test\r\nbcc:evil@example.test"), null);
});
