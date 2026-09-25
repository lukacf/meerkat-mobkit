import { useRef } from "react";
import { QuoteSelectionAction } from "./quote-selection-action";
import { JumpToLatest } from "./jump-to-latest";
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { readConsoleQuoteSelection } from "./context-selection";
import { QuoteContextChips } from "./context-chips";
import { createConsoleContextRecord } from "../../../console-core/src/context-record";
import { ConversationPane } from "./conversation-pane";
import { ChatPane } from "../../../../console/src/panels/ChatPane";
afterEach(() => { cleanup(); window.getSelection()?.removeAllRanges(); });
const select = (start: Node, end = start) => {
  const range = document.createRange(); range.setStart(start, 0); range.setEnd(end, end.textContent!.length);
  const selection = window.getSelection()!; selection.removeAllRanges(); selection.addRange(range); return selection;
};
describe("transcript quote selection", () => {
  it("captures a single opted-in message and exact rendered unicode text", () => {
    const { container } = render(<div data-quote-message-id="msg" data-quote-source="🌳 café">🌳 café</div>);
    const result = readConsoleQuoteSelection(container, select(container.firstChild!.firstChild!));
    expect(result).toEqual({ kind: "selected", quote: { text: "🌳 café", messageId: "msg", sourceText: "🌳 café" } });
  });
  it("rejects selection spanning messages and ignores navigation", () => {
    const { container } = render(<><p data-quote-message-id="a">first</p><p data-quote-message-id="b">second</p><nav>outside</nav></>);
    expect(readConsoleQuoteSelection(container, select(container.children[0].firstChild!, container.children[1].firstChild!))).toEqual({ kind: "rejected", message: "Select text from one message at a time." });
    expect(readConsoleQuoteSelection(container, select(container.children[2].firstChild!))).toEqual({ kind: "empty" });
  });
  it("excludes embedded metadata and controls", () => {
    const { container } = render(<p data-quote-message-id="a">hello<span data-quote-exclude="">tool metadata</span>world</p>);
    expect(readConsoleQuoteSelection(container, select(container.firstChild!.firstChild!, container.firstChild!.lastChild!)).kind).toBe("rejected");
  });
  it("shows destination, removable source chip and exact snapshot without sending", () => {
    const context = createConsoleContextRecord({ id: "q", sourceScope: "s", sourceIdentity: "agent", messageId: "a", label: "Source agent", quote: "exact  snapshot\nline" });
    const onRemove = vi.fn(); render(<QuoteContextChips records={[context]} destinationLabel="Target agent" onRemove={onRemove} />);
    expect(screen.getByRole("region", { name: "Quoted context for Target agent" })).toBeTruthy();
    expect(screen.getByText("Source agent")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Remove quote from Source agent" }));
    expect(onRemove).toHaveBeenCalledWith("q");
  });
  it("keeps the reusable composer clear when no transcript text is selected", () => {
    const viewState = { conversationId: "quote-feedback", entries: [], groups: [], emptyState: null, turnDiff: null };
    render(<ConversationPane viewState={viewState} onQuoteSelection={vi.fn()} footer={<textarea aria-label="Message" />} />);
    expect(screen.queryByRole("button", { name: "Add to message" })).toBeNull();
  });
  it("keeps the stock composer clear when no transcript text is selected", () => {
    render(<ChatPane identity="router:main" agentLabel="Router" agent={null} entries={[]} phase={null} draft="" sending={false}
      staged={[]} onDraftChange={vi.fn()} onStagedChange={vi.fn()} onSend={vi.fn()} onQuoteSelection={vi.fn()} />);
    expect(screen.queryByRole("button", { name: "Add to message" })).toBeNull();
  });
});

function SelectionFixture({ disabled = false, onQuote = vi.fn(), onError = vi.fn() }) {
  const viewportRef = useRef<HTMLDivElement>(null);
  return <div><div ref={viewportRef} data-testid="quote-viewport"><p data-quote-message-id="one" data-quote-source="🌳 café">🌳 café</p><p data-quote-message-id="two">second</p></div>
    <p data-testid="other-pane" data-quote-message-id="outside">another pane</p>
    <QuoteSelectionAction viewportRef={viewportRef} onQuote={onQuote} onError={onError} disabled={disabled} /></div>;
}
function announceSelection(start: Node, end = start) { select(start, end); fireEvent(document, new Event("selectionchange")); }
describe("contextual transcript controls", () => {
  it("shows an icon only for a local selection and adds its exact text without sending", () => {
    const onQuote = vi.fn(); render(<SelectionFixture onQuote={onQuote} />);
    expect(screen.queryByRole("button", { name: "Add to message" })).toBeNull();
    announceSelection(screen.getByText("🌳 café").firstChild!);
    const button = screen.getByRole("button", { name: "Add to message" });
    expect(button).toHaveTextContent(""); expect(button.querySelector("svg")).toBeTruthy();
    expect(onQuote).not.toHaveBeenCalled();
    fireEvent.click(button);
    expect(onQuote).toHaveBeenCalledExactlyOnceWith({ messageId: "one", sourceText: "🌳 café", text: "🌳 café" });
    expect(screen.queryByRole("button", { name: "Add to message" })).toBeNull();
    expect(window.getSelection()!.isCollapsed).toBe(true);
  });
  it("ignores other panes and disappears when selection clears, on Escape, and in read-only mode", () => {
    const view = render(<SelectionFixture />);
    announceSelection(screen.getByTestId("other-pane").firstChild!);
    expect(screen.queryByRole("button", { name: "Add to message" })).toBeNull();
    announceSelection(screen.getByText("🌳 café").firstChild!);
    expect(screen.getByRole("button", { name: "Add to message" })).toBeTruthy();
    fireEvent.keyDown(document, { key: "Escape" });
    expect(screen.queryByRole("button", { name: "Add to message" })).toBeNull();
    announceSelection(screen.getByText("🌳 café").firstChild!);
    window.getSelection()!.removeAllRanges(); fireEvent(document, new Event("selectionchange"));
    expect(screen.queryByRole("button", { name: "Add to message" })).toBeNull();
    announceSelection(screen.getByText("🌳 café").firstChild!);
    view.rerender(<SelectionFixture disabled />);
    expect(screen.queryByRole("button", { name: "Add to message" })).toBeNull();
  });
  it("retains the explicit rejection when the selection spans messages", () => {
    const onQuote = vi.fn(), onError = vi.fn(); render(<SelectionFixture onQuote={onQuote} onError={onError} />);
    announceSelection(screen.getByText("🌳 café").firstChild!, screen.getByText("second").firstChild!);
    fireEvent.click(screen.getByRole("button", { name: "Add to message" }));
    expect(onError).toHaveBeenCalledWith("Select text from one message at a time.");
    expect(onQuote).not.toHaveBeenCalled();
  });
  it("uses an accessible down-arrow icon whose activity follows the host", () => {
    const onClick = vi.fn(); const view = render(<JumpToLatest onClick={onClick} working />);
    const button = screen.getByRole("button", { name: "Jump to latest" });
    expect(button).toHaveTextContent(""); expect(button.querySelectorAll("svg")).toHaveLength(2);
    expect(button).toHaveAttribute("data-working", "true");
    fireEvent.click(button); expect(onClick).toHaveBeenCalledOnce();
    view.rerender(<JumpToLatest onClick={onClick} working={false} />);
    expect(button).toHaveAttribute("data-working", "false");
  });
});
