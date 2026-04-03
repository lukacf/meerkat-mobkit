import { parseConversationRichBlocks, parseConversationSummary } from "./rich-content";

describe("conversation rich-content parser", () => {
  test("parses markdown tables into structured table blocks", () => {
    const blocks = parseConversationRichBlocks(`
| Surface | Status | Notes |
| :--- | :---: | ---: |
| Transcript | done | 1 |
| Sidebar | next | 2 |
    `);

    expect(blocks).toHaveLength(1);
    expect(blocks[0]).toMatchObject({
      type: "table",
      headers: ["Surface", "Status", "Notes"],
      alignments: ["left", "center", "right"],
      rows: [
        ["Transcript", "done", "1"],
        ["Sidebar", "next", "2"],
      ],
    });
  });

  test("parses summary paragraphs into structured summary metadata", () => {
    const summary = parseConversationSummary(`
2 files changed +12 -3
packages/console-core/src/conversation.ts +8 -0
desktop/renderer/src/app/App.tsx +4 -3
    `);

    expect(summary).toMatchObject({
      title: "2 files changed",
      plus: 12,
      minus: 3,
      files: [
        { name: "packages/console-core/src/conversation.ts", plus: 8, minus: 0 },
        { name: "desktop/renderer/src/app/App.tsx", plus: 4, minus: 3 },
      ],
    });
  });
});
