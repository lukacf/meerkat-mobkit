import { act, fireEvent, render, screen } from "@testing-library/react";
import { useLayoutEffect, useRef } from "react";
import { afterEach, describe, expect, test, vi } from "vitest";

import {
  CONVERSATION_POSITION_LIMIT,
  ConversationPositionCache,
  captureConversationAnchor,
  conversationIsAtEnd,
  isConversationScrollTarget,
  restoreConversationAnchor,
} from "./scroll-geometry";
import { useConversationScrollController, type ConversationScrollControllerOptions, type ConversationViewportKey } from "./scroll-controller";

describe("scroll geometry", () => {
  test("preserves a visible row through prepend and chooses its nearest retained neighbor", () => {
    const anchor = captureConversationAnchor([{ id: "a", top: -80, bottom: -20 }, { id: "b", top: -10, bottom: 60 }, { id: "c", top: 70, bottom: 100 }])!;
    expect(anchor.rowId).toBe("b");
    expect(restoreConversationAnchor(anchor, [{ id: "b", top: 240, bottom: 300 }], 100, 800)).toEqual({ rowId: "b", scrollTop: 350, exact: true });
    expect(restoreConversationAnchor(anchor, [{ id: "c", top: 130, bottom: 160 }], 100, 800)).toEqual({ rowId: "c", scrollTop: 160, exact: false });
    expect(restoreConversationAnchor(anchor, [], 100, 800)).toBeNull();
  });
  test("uses a small tested live-edge threshold and clamps overscroll", () => {
    expect(conversationIsAtEnd(168, 300, 100)).toBe(true);
    expect(conversationIsAtEnd(167, 300, 100)).toBe(false);
    expect(conversationIsAtEnd(-10, 80, 100)).toBe(true);
  });
  test("bounds position retention and clears only the previous authority", () => {
    const cache = new ConversationPositionCache();
    const position = { mode: "reading-history" as const, anchor: null, scrollTop: 20 };
    for (let i = 0; i < 101; i += 1) cache.remember(JSON.stringify(["a", i]), position);
    expect(cache.size).toBe(CONVERSATION_POSITION_LIMIT);
    expect(cache.read(JSON.stringify(["a", 0]))).toBeUndefined();
    cache.remember(JSON.stringify(["b", 0]), position);
    cache.deleteAuthority("a");
    expect(cache.size).toBe(1);
    expect(cache.read(JSON.stringify(["b", 0]))).toBe(position);
  });
  test("nested scroll surfaces consume their own vertical and horizontal gestures", () => {
    const viewport = document.createElement("div");
    const nested = document.createElement("pre");
    const target = document.createElement("code");
    viewport.append(nested); nested.append(target); document.body.append(viewport);
    nested.style.overflowY = "auto";
    Object.defineProperties(nested, { scrollHeight: { value: 400 }, clientHeight: { value: 100 } });
    nested.scrollTop = 30;
    expect(isConversationScrollTarget(target, viewport, -10)).toBe(false);
    expect(isConversationScrollTarget(target, viewport, 10)).toBe(false);
    nested.scrollTop = 0;
    expect(isConversationScrollTarget(target, viewport, -10)).toBe(true);
    expect(isConversationScrollTarget(target, viewport, 1, 20)).toBe(false);
    viewport.remove();
  });
});

const rect = (top: number, height: number) => ({ top, bottom: top + height, height, left: 0, right: 200, width: 200, x: 0, y: top, toJSON() {} });
type Row = { id: string; height: number; text?: string };
const baseRows: Row[] = Array.from({ length: 10 }, (_, i) => ({ id: `row-${i}`, height: 100 }));
function Harness({ rows = baseRows, conversation = "test", viewportKey, submittedRowId, revealAnchor, revealTimeoutMs, height = 200 }: {
  rows?: Row[]; conversation?: string; viewportKey?: ConversationViewportKey; submittedRowId?: string | null; revealAnchor?: ConversationScrollControllerOptions["revealAnchor"]; revealTimeoutMs?: number; height?: number;
}) {
  const viewportRef = useRef<HTMLDivElement | null>(null);
  useLayoutEffect(() => {
    const viewport = viewportRef.current!;
    Object.defineProperties(viewport, {
      scrollHeight: { configurable: true, get: () => rows.reduce((sum, row) => sum + row.height, 0) },
      clientHeight: { configurable: true, get: () => height },
    });
    viewport.getBoundingClientRect = () => rect(0, height);
    let top = 0;
    for (const row of rows) {
      const node = Array.from(viewport.querySelectorAll<HTMLElement>("[data-conversation-row-id]")).find((node) => node.dataset.conversationRowId === row.id)!;
      const rowTop = top;
      node.getBoundingClientRect = () => rect(rowTop - viewport.scrollTop, row.height);
      top += row.height;
    }
  });
  const scroll = useConversationScrollController({ viewportRef, viewportKey, conversationId: conversation, contentVersion: rows, submittedRowId, revealAnchor, revealTimeoutMs });
  return <>
    <div data-testid="viewport" tabIndex={0} ref={viewportRef}>{rows.map((row) => <div data-conversation-row-id={row.id} key={row.id}>{row.text ?? row.id}</div>)}</div>
    <span data-testid="mode">{scroll.mode}</span>
    {scroll.missingAnchor ? <div role="status">Earlier position is unavailable</div> : null}
    {scroll.revealingAnchor ? <div role="status">Restoring earlier position</div> : null}
    {scroll.awayFromEnd ? <button onClick={scroll.jumpToLatest}>Jump to latest</button> : null}
    <button onClick={() => scroll.jumpToRow("row-2")}>Turn 2</button>
    <button onClick={scroll.captureBeforePrepend}>Capture before prepend</button>
    <button onClick={scroll.notifyLayoutChange}>Notify layout</button>
  </>;
}
function userScroll(viewport: HTMLElement, top: number) { viewport.scrollTop = top; fireEvent.scroll(viewport); }
afterEach(() => { vi.restoreAllMocks(); vi.useRealTimers(); });

describe("conversation scroll intent", () => {
  test("pointer interaction captures a browser-revealed position before a focus render can restore the old anchor", () => {
    const view = render(<Harness />);
    const viewport = screen.getByTestId("viewport");
    userScroll(viewport, 225);
    // Native focus/reveal can scroll before the browser emits its scroll event.
    // A render between mouse down and up must not move the target away.
    viewport.scrollTop = 525;
    fireEvent.pointerDown(viewport.querySelector('[data-conversation-row-id="row-6"]')!);
    view.rerender(<Harness />);
    expect(viewport.scrollTop).toBe(525);
    expect(screen.getByTestId("mode")).toHaveTextContent("reading-history");
  });
  test("starts at end, then 100 content updates preserve the historical row within 2px", () => {
    const view = render(<Harness />);
    const viewport = screen.getByTestId("viewport");
    expect(viewport.scrollTop).toBe(800);
    userScroll(viewport, 225);
    expect(screen.getByTestId("mode")).toHaveTextContent("reading-history");
    for (let i = 1; i <= 100; i += 1) {
      view.rerender(<Harness rows={baseRows.map((row, index) => index === 9 ? { ...row, height: 100 + i, text: `token ${i}` } : row)} />);
      expect(Math.abs(viewport.scrollTop - 225)).toBeLessThanOrEqual(2);
    }
    fireEvent.click(screen.getByRole("button", { name: "Jump to latest" }));
    expect(viewport.scrollTop).toBe(900);
    expect(screen.getByTestId("mode")).toHaveTextContent("following-end");
  });
  test("preserves the row through history prepend, earlier disclosure growth and viewport resize", () => {
    const view = render(<Harness />);
    const viewport = screen.getByTestId("viewport");
    userScroll(viewport, 225);
    fireEvent.click(screen.getByRole("button", { name: "Capture before prepend" }));
    const prepended = [{ id: "older", height: 150 }, ...baseRows];
    view.rerender(<Harness rows={prepended} />);
    expect(viewport.scrollTop).toBe(375);
    view.rerender(<Harness rows={prepended.map((row) => row.id === "row-0" ? { ...row, height: 220 } : row)} height={150} />);
    expect(viewport.scrollTop).toBe(495);
    expect(viewport.querySelector('[data-conversation-row-id="row-2"]')!.getBoundingClientRect().top).toBe(-25);
  });
  test("restores local conversation position and keeps independently mounted panes separate", () => {
    const view = render(<Harness conversation="one" />);
    const viewport = screen.getByTestId("viewport");
    userScroll(viewport, 225);
    view.rerender(<Harness conversation="two" />);
    expect(viewport.scrollTop).toBe(800);
    userScroll(viewport, 450);
    view.rerender(<Harness conversation="one" />);
    expect(viewport.scrollTop).toBe(225);
    view.unmount();
    render(<Harness conversation="one" />);
    expect(screen.getByTestId("viewport").scrollTop).toBe(800);
  });
  test("explicit pane keys retain remount position and discard the previous principal on authority change", () => {
    const key = { authority: "principal-scroll-test", identity: "agent", conversation: "one", pane: "left" };
    const view = render(<Harness viewportKey={key} />);
    userScroll(screen.getByTestId("viewport"), 250);
    view.unmount();
    const second = render(<Harness viewportKey={key} />);
    expect(screen.getByTestId("viewport").scrollTop).toBe(250);
    second.rerender(<Harness viewportKey={{ ...key, authority: "different-principal" }} />);
    expect(screen.getByTestId("viewport").scrollTop).toBe(800);
    second.rerender(<Harness viewportKey={key} />);
    expect(screen.getByTestId("viewport").scrollTop).toBe(800);
  });
  test("rail jumps enter reading mode and cannot scroll an outer document", () => {
    const outerScroll = vi.fn(); HTMLElement.prototype.scrollIntoView = outerScroll;
    render(<Harness />);
    fireEvent.click(screen.getByRole("button", { name: "Turn 2" }));
    expect(screen.getByTestId("viewport").scrollTop).toBe(176);
    expect(screen.getByTestId("mode")).toHaveTextContent("reading-history");
    expect(outerScroll).not.toHaveBeenCalled();
  });
  test("a queued/nonexistent row cannot move history, a canonical submitted row can anchor once present", () => {
    const view = render(<Harness />);
    const viewport = screen.getByTestId("viewport");
    userScroll(viewport, 225);
    view.rerender(<Harness submittedRowId="accepted" />);
    expect(viewport.scrollTop).toBe(225);
    view.rerender(<Harness submittedRowId="accepted" rows={[...baseRows, { id: "accepted", height: 100 }]} />);
    expect(viewport.scrollTop).toBe(900);
    expect(screen.getByTestId("mode")).toHaveTextContent("anchoring-submitted-turn");
    view.rerender(<Harness submittedRowId="accepted" rows={[...baseRows, { id: "accepted", height: 100 }, { id: "response", height: 100 }]} />);
    expect(viewport.scrollTop).toBe(900);
  });
  test("does not treat a consumed acceptance as a new send after remount or identity return", () => {
    const key = { authority: "accepted-remount", identity: "agent", conversation: "one", pane: "left" };
    const view = render(<Harness viewportKey={key} submittedRowId="row-8" />);
    expect(screen.getByTestId("mode")).toHaveTextContent("anchoring-submitted-turn");
    userScroll(screen.getByTestId("viewport"), 225);
    view.unmount();
    const next = render(<Harness viewportKey={key} submittedRowId="row-8" />);
    expect(screen.getByTestId("viewport").scrollTop).toBe(225);
    next.rerender(<Harness viewportKey={{ ...key, identity: "other" }} submittedRowId={null} />);
    next.rerender(<Harness viewportKey={key} submittedRowId="row-8" />);
    expect(screen.getByTestId("viewport").scrollTop).toBe(225);
    next.rerender(<Harness viewportKey={{ ...key, pane: "right" }} submittedRowId="row-8" />);
    expect(screen.getByTestId("viewport").scrollTop).toBe(776);
  });
  test("retains an accepted row awaiting DOM arrival across a remount", () => {
    const key = { authority: "pending-accepted-remount", identity: "agent", conversation: "one", pane: "left" };
    const view = render(<Harness viewportKey={key} submittedRowId="accepted-later" />);
    view.unmount();
    render(<Harness viewportKey={key} submittedRowId="accepted-later" rows={[...baseRows, { id: "accepted-later", height: 100 }]} />);
    expect(screen.getByTestId("mode")).toHaveTextContent("anchoring-submitted-turn");
  });
  test("failed asynchronous reveal restores a retained neighbor and exposes unavailability", async () => {
    vi.useFakeTimers();
    const key = { authority: "failed-reveal", identity: "agent", conversation: "one", pane: "left" };
    const view = render(<Harness viewportKey={key} />);
    userScroll(screen.getByTestId("viewport"), 225);
    view.unmount();
    let fail!: (reason: Error) => void;
    const reveal = vi.fn(() => new Promise<boolean>((_resolve, reject) => { fail = reject; }));
    const retained = [{ id: "replacement", height: 300 }, ...baseRows.slice(3)];
    const next = render(<Harness viewportKey={key} rows={retained} revealAnchor={reveal} />);
    expect(screen.getByRole("status")).toHaveTextContent("Restoring earlier position");
    await act(async () => { fail(new Error("history unavailable")); await Promise.resolve(); vi.advanceTimersByTime(20); });
    expect(screen.getByTestId("viewport").scrollTop).toBe(225);
    expect(screen.getByRole("status")).toHaveTextContent("Earlier position is unavailable");
    next.rerender(<Harness viewportKey={key} rows={retained} revealAnchor={reveal} />);
    expect(reveal).toHaveBeenCalledTimes(1);
    expect(screen.getByRole("status")).toHaveTextContent("Earlier position is unavailable");
  });
  test("bounds never-settling and legacy boolean reveals without claiming the row exists", async () => {
    vi.useFakeTimers();
    const key = { authority: "bounded-reveal", identity: "agent", conversation: "one", pane: "left" };
    const view = render(<Harness viewportKey={key} />);
    userScroll(screen.getByTestId("viewport"), 225); view.unmount();
    let signal!: AbortSignal;
    const reveal = vi.fn((_id: string, requestSignal: AbortSignal) => { signal = requestSignal; return new Promise<boolean>(() => {}); });
    const next = render(<Harness viewportKey={key} rows={baseRows.slice(3)} revealAnchor={reveal} revealTimeoutMs={100} />);
    await act(async () => { vi.advanceTimersByTime(120); });
    expect(signal.aborted).toBe(true);
    expect(screen.getByRole("status")).toHaveTextContent("Earlier position is unavailable");
    next.unmount();
    const legacyKey = { ...key, pane: "legacy" };
    const seed = render(<Harness viewportKey={legacyKey} />);
    userScroll(screen.getByTestId("viewport"), 225); seed.unmount();
    render(<Harness viewportKey={legacyKey} rows={baseRows.slice(3)} revealAnchor={() => true} />);
    await act(async () => { vi.advanceTimersByTime(40); });
    expect(screen.getByRole("status")).toHaveTextContent("Earlier position is unavailable");
  });
  test("aborts an old reveal on identity change and ignores its late completion", async () => {
    vi.useFakeTimers();
    const key = { authority: "cancel-reveal", identity: "agent", conversation: "one", pane: "left" };
    const view = render(<Harness viewportKey={key} />);
    userScroll(screen.getByTestId("viewport"), 225); view.unmount();
    let signal!: AbortSignal;
    let finish!: (found: boolean) => void;
    const reveal = (_id: string, requestSignal: AbortSignal) => { signal = requestSignal; return new Promise<boolean>((resolve) => { finish = resolve; }); };
    const next = render(<Harness viewportKey={key} rows={baseRows.slice(3)} revealAnchor={reveal} />);
    next.rerender(<Harness viewportKey={{ ...key, identity: "other" }} />);
    expect(signal.aborted).toBe(true);
    await act(async () => { finish(true); await Promise.resolve(); vi.advanceTimersByTime(20); });
    expect(screen.getByTestId("viewport").scrollTop).toBe(800);
    expect(screen.queryByRole("status")).toBeNull();
  });
  test("requests a missing saved row once and restores it after the host reveals it", () => {
    const key = { authority: "reveal-scroll-test", identity: "agent", conversation: "one", pane: "left" };
    const view = render(<Harness viewportKey={key} />);
    userScroll(screen.getByTestId("viewport"), 225);
    view.unmount();
    const reveal = vi.fn(() => true);
    const next = render(<Harness viewportKey={key} rows={baseRows.slice(5)} revealAnchor={reveal} />);
    expect(reveal).toHaveBeenCalledWith("row-2", expect.any(AbortSignal));
    next.rerender(<Harness viewportKey={key} revealAnchor={reveal} />);
    expect(screen.getByTestId("viewport").scrollTop).toBe(225);
    expect(reveal).toHaveBeenCalledTimes(1);
  });
  test("user scrolling back within the live edge resumes following", () => {
    const view = render(<Harness />);
    const viewport = screen.getByTestId("viewport");
    userScroll(viewport, 100);
    userScroll(viewport, 780);
    expect(screen.getByTestId("mode")).toHaveTextContent("following-end");
    view.rerender(<Harness rows={[...baseRows, { id: "new", height: 100 }]} />);
    expect(viewport.scrollTop).toBe(900);
  });
});
