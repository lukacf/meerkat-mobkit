import assert from "node:assert/strict";
import test from "node:test";

import {
  parseConversationRichBlocks,
  parseConversationSummary,
  parseStreamingConversationRichBlocks,
  renderConversationInlineMarkdown,
  safeConsoleHref,
} from "./rich-content";

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

test("parseStreamingConversationRichBlocks renders only stable markdown blocks", () => {
  const blocks = parseStreamingConversationRichBlocks("## Risk\n\n- first\n- seco");

  assert.equal(blocks.length, 2);
  assert.deepEqual(blocks[0], {
    type: "heading",
    level: 2,
    text: "Risk",
  });
  assert.deepEqual(blocks[1], {
    type: "paragraph",
    text: "- first\n- seco",
  });
});

test("parseStreamingConversationRichBlocks keeps unclosed fences as plain tail text", () => {
  const blocks = parseStreamingConversationRichBlocks("Intro\n\n```ts\nconst answer = 1");

  assert.equal(blocks.length, 2);
  assert.deepEqual(blocks[0], {
    type: "paragraph",
    text: "Intro",
  });
  assert.deepEqual(blocks[1], {
    type: "paragraph",
    text: "```ts\nconst answer = 1",
  });
});

test("parseStreamingConversationRichBlocks hides unfinished inline markdown spans", () => {
  const unfinished = parseStreamingConversationRichBlocks("Having *proper render");
  assert.equal(unfinished.length, 1);
  assert.deepEqual(unfinished[0], {
    type: "paragraph",
    text: "Having",
    streaming: true,
  });

  const finished = parseStreamingConversationRichBlocks("Having *proper rendering* is essential.");
  assert.equal(finished.length, 1);
  assert.deepEqual(finished[0], {
    type: "paragraph",
    text: "Having *proper rendering* is essential.",
    streaming: true,
  });
  assert.equal(
    renderConversationInlineMarkdown(finished[0]?.type === "paragraph" ? finished[0].text : ""),
    "Having <em>proper rendering</em> is essential.",
  );
});

test("parseStreamingConversationRichBlocks does not treat identifiers as unfinished markdown", () => {
  const blocks = parseStreamingConversationRichBlocks("Use api_investigator and ORDER_THREE_OK.");
  assert.equal(blocks.length, 1);
  assert.equal(blocks[0]?.type === "paragraph" ? blocks[0].text : "", "Use api_investigator and ORDER_THREE_OK.");
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
