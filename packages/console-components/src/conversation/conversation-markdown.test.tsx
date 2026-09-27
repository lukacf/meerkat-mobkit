import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";

import { buildConversationMarkdownBlocks, conversationRichBlockCopyText, conversationEntryText } from "@console-core";
import { mapFramesToTimelineEntries } from "../../../console-core/src/adapters";
import { buildConversationViewState } from "../../../console-core/src/adapters";
import { ConversationMarkdown, resolveMarkdownImage, resolveMarkdownLink } from "./conversation-markdown";
import { ConversationRichContent } from "./conversation-rich-content";
import { ConversationTranscript } from "./conversation-transcript";
import { MARKDOWN_CORPUS, markdownEditSequence } from "./markdown-corpus";

function normalizedDom(input: Node): unknown {
  const node = input.cloneNode(true);
  node.normalize();
  if (node.nodeType === Node.TEXT_NODE) return node.textContent;
  if (!(node instanceof Element)) return null;
  return {
    tag: node.tagName,
    attributes: [...node.attributes].map((attribute) => [
      attribute.name,
      attribute.name === "style" ? (node as HTMLElement).style.cssText : attribute.value,
    ]).sort(([a], [b]) => a.localeCompare(b)),
    children: [...node.childNodes].map(normalizedDom),
  };
}

const doc = (source: string, streaming = false) => buildConversationMarkdownBlocks(source, { documentId: "row:text:0", streaming })[0];

describe("Markdown document renderer", () => {
  for (const fixture of MARKDOWN_CORPUS) {
    it(`renders ${fixture.id}, prefixes and edits without altering source`, () => {
      const { container, rerender } = render(<ConversationMarkdown block={doc(fixture.source)} />);
      expect(container.querySelector(fixture.selector)).not.toBeNull();
      for (const source of markdownEditSequence(fixture.source)) {
        if (!source) continue;
        rerender(<ConversationMarkdown block={doc(source, true)} />);
        const actual = container.querySelector(".cc-markdown-document")!;
        const fresh = document.createElement("div");
        fresh.innerHTML = renderToStaticMarkup(<ConversationMarkdown block={doc(source, true)} />);
        expect(normalizedDom(actual)).toEqual(normalizedDom(fresh.firstElementChild!));
        expect(conversationRichBlockCopyText(doc(source))).toBe(source);
      }
      rerender(<ConversationMarkdown block={doc(fixture.source)} />);
      expect(container.querySelector(fixture.selector)).not.toBeNull();
    });
  }

  it("keeps root, descendants and visible selection on completion of unchanged source", () => {
    const source = "Selected **text** stays\n\n[link](https://example.test)";
    const { container, rerender } = render(<ConversationRichContent blocks={[doc(source, true)]} richStyle="streaming" />);
    const root = container.querySelector(".cc-markdown-document");
    const paragraph = container.querySelector("p")!;
    const text = paragraph.firstChild!;
    const range = document.createRange();
    range.setStart(text, 0); range.setEnd(text, 8);
    window.getSelection()!.removeAllRanges(); window.getSelection()!.addRange(range);
    rerender(<ConversationRichContent blocks={[doc(source)]} />);
    expect(container.querySelector(".cc-markdown-document")).toBe(root);
    expect(container.querySelector("p")).toBe(paragraph);
    expect(window.getSelection()!.toString()).toBe("Selected");
    expect(paragraph.getAttribute("data-source-start")).toBe("0");
    expect(paragraph.getAttribute("data-source-end")).toBe(String(source.indexOf("\n\n")));
  });

  it("escapes raw HTML and never loads images by default", () => {
    const source = '<script>window.attack=1</script>\n\n<img src="x" onerror="attack()">\n\n![secret](https://tracker.test/pixel)\n\n[bad](javascript:alert%281%29)';
    const { container } = render(<ConversationMarkdown block={doc(source)} />);
    expect(container.querySelector("script,img,a")).toBeNull();
    expect(container.textContent).toContain("<script>");
    expect(container.textContent).toContain("secret");
  });

  it("uses distinct link and image policy and safe external attributes", () => {
    const { container, rerender } = render(<ConversationMarkdown block={doc('[safe](https://example.test) [local](/route) [fragment](#part) ![photo](/photo)')} />);
    expect(screen.getByText("safe").closest("a")?.rel).toBe("noopener noreferrer");
    expect(screen.getByText("local").closest("a")).toBeNull();
    expect(screen.getByText("fragment").closest("a")?.getAttribute("href")).toBe("#part");
    rerender(<ConversationMarkdown block={doc('[local](app:item) ![photo](/photo)')} urlPolicy={{
      resolveLink: (url) => url === "app:item" ? "/authorized/item" : null,
      resolveImage: (url) => url === "/photo" ? "blob:https://console.test/approved" : null,
    }} />);
    expect(container.querySelector("a")?.getAttribute("href")).toBe("/authorized/item");
    expect(container.querySelector("img")?.getAttribute("src")).toBe("blob:https://console.test/approved");
    for (const url of ["javascript:attack()", "data:text/html,attack", "file:///secret", "//evil.test", "java\nscript:attack()"] ) {
      expect(resolveMarkdownLink(url, { resolveLink: () => url })).toBeNull();
      expect(resolveMarkdownImage(url, { resolveImage: () => url })).toBeNull();
    }
    expect(resolveMarkdownLink("unknown:item")).toBeNull();
    expect(resolveMarkdownLink("/relative")).toBeNull();
    expect(resolveMarkdownImage("https://example.test/image")).toBeNull();
  });

  it("opts reusable hosts in through the real shared mapper and forwards URL policy", () => {
    const frames = [{ id: "reply", event: "interaction_complete", data: { result: " [local](/route) ![photo](/photo)\n" }, timestampMs: 1 }];
    const legacy = mapFramesToTimelineEntries(null, frames);
    const entries = mapFramesToTimelineEntries(null, frames, { textMode: "markdown" });
    expect(legacy[0].kind === "message" && legacy[0].blocks?.[0].type).toBe("paragraph");
    expect(conversationEntryText(entries[0])).toBe(" [local](/route) ![photo](/photo)\n");
    const viewState = buildConversationViewState({ memberId: "test", agentLabel: "Test", entries });
    const { container } = render(<ConversationTranscript viewState={viewState} markdownUrlPolicy={{ resolveLink: (url) => url, resolveImage: (url) => url }} />);
    expect(container.querySelector('a[href="/route"]')).not.toBeNull();
    expect(container.querySelector('img[src="/photo"]')).not.toBeNull();
  });

  it("leaves caller-built legacy blocks and typed images on their existing path", () => {
    const { container } = render(<ConversationRichContent blocks={[
      { type: "paragraph", text: "# literal heading" },
      { type: "image", src: "https://example.test/typed.png", mediaType: "image/png", alt: "Typed image" },
    ]} />);
    expect(container.querySelector("h1")).toBeNull();
    expect(container.querySelector("img")?.src).toBe("https://example.test/typed.png");
  });
});
