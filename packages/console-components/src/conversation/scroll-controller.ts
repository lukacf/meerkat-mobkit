import { useCallback, useLayoutEffect, useRef, useState, type RefObject } from "react";

import {
  CONVERSATION_ANCHOR_OFFSET_PX,
  ConversationPositionCache,
  captureConversationAnchor,
  conversationIsAtEnd,
  conversationScrollEnd,
  isConversationScrollTarget,
  restoreConversationAnchor,
  type ConversationRowGeometry,
  type ConversationScrollAnchor,
  type ConversationScrollMode,
} from "./scroll-geometry";

export type ConversationViewportKey = {
  /** Runtime, realm and principal authority, not a display label. */
  authority: string;
  identity: string;
  conversation: string;
  pane: string;
};

export type ConversationScrollControllerOptions = {
  viewportRef: RefObject<HTMLElement | null>;
  contentRef?: RefObject<HTMLElement | null>;
  /** Without this key, memory stays local to this mounted controller. */
  viewportKey?: ConversationViewportKey;
  conversationId: string;
  /** Change when content changes. DOM resize/load observers also cover late layout. */
  contentVersion?: unknown;
  /** Canonical accepted row only. A queued draft is not a submitted row. */
  submittedRowId?: string | null;
  /**
   * Resolve after bounded history/reveal work completes, false when unavailable.
   * The controller verifies the row in the DOM; completion never proves existence.
   * A legacy true boolean requests one local render, then falls back if absent.
   */
  revealAnchor?: (rowId: string, signal: AbortSignal) => boolean | Promise<boolean>;
  /** Upper bound for a host reveal, default 15 seconds, maximum 60 seconds. */
  revealTimeoutMs?: number;
};

const sharedPositions = new ConversationPositionCache();
const ROW_SELECTOR = "[data-conversation-row-id]";

type Session = {
  key: string;
  authority: string | undefined;
  mode: ConversationScrollMode;
  anchor: ConversationScrollAnchor | null;
  pendingSubmittedRow: string | null;
  lastSubmittedRow: string | null;
  expectedScrollTop: number | null;
  requestedAnchor: string | null;
  awaitingAnchor: boolean;
  missingAnchor: boolean;
  reveal?: { controller: AbortController; timer: number | null; frame: number | null };
};

function cancelReveal(session: Session): void {
  if (session.reveal) {
    session.reveal.controller.abort();
    if (session.reveal.timer !== null) window.clearTimeout(session.reveal.timer);
    if (session.reveal.frame !== null) window.cancelAnimationFrame(session.reveal.frame);
    session.reveal = undefined;
  }
  session.awaitingAnchor = false;
}

function rowGeometry(viewport: HTMLElement): ConversationRowGeometry[] {
  const top = viewport.getBoundingClientRect().top + viewport.clientTop;
  return Array.from(viewport.querySelectorAll<HTMLElement>(ROW_SELECTOR)).filter((row) => !row.closest("details:not([open])")).map((row) => {
    const rect = row.getBoundingClientRect();
    return { id: row.dataset.conversationRowId!, top: rect.top - top, bottom: rect.bottom - top };
  });
}

/** Preserve intent across streaming and layout without moving an outer document. */
export function useConversationScrollController(options: ConversationScrollControllerOptions) {
  const optionsRef = useRef(options);
  optionsRef.current = options;
  const localPositions = useRef(new ConversationPositionCache());
  const sessionRef = useRef<Session | null>(null);
  const frameRef = useRef<number | null>(null);
  const applyLayoutRef = useRef<() => void>(() => {});
  const [state, setState] = useState({ mode: "following-end" as ConversationScrollMode, awayFromEnd: false, missingAnchor: false, revealingAnchor: false });
  const key = options.viewportKey
    ? JSON.stringify([options.viewportKey.authority, options.viewportKey.identity, options.viewportKey.conversation, options.viewportKey.pane])
    : options.conversationId;
  const authority = options.viewportKey?.authority;

  const publish = useCallback((missing?: boolean) => {
    const viewport = optionsRef.current.viewportRef.current;
    const session = sessionRef.current;
    if (!viewport || !session) return;
    if (missing !== undefined) session.missingAnchor = missing;
    const missingAnchor = session.missingAnchor;
    const revealingAnchor = session.awaitingAnchor;
    const awayFromEnd = !conversationIsAtEnd(viewport.scrollTop, viewport.scrollHeight, viewport.clientHeight);
    setState((old) => old.mode === session.mode && old.awayFromEnd === awayFromEnd && old.missingAnchor === missingAnchor && old.revealingAnchor === revealingAnchor
      ? old : { mode: session.mode, awayFromEnd, missingAnchor, revealingAnchor });
    const cache = session.authority === undefined ? localPositions.current : sharedPositions;
    cache.remember(session.key, { mode: session.mode, anchor: session.anchor, scrollTop: viewport.scrollTop, lastSubmittedRow: session.lastSubmittedRow, pendingSubmittedRow: session.pendingSubmittedRow });
  }, []);

  const writeScroll = useCallback((top: number) => {
    const viewport = optionsRef.current.viewportRef.current;
    const session = sessionRef.current;
    if (!viewport || !session) return;
    const bounded = Math.max(0, Math.min(conversationScrollEnd(viewport.scrollHeight, viewport.clientHeight), top));
    session.expectedScrollTop = bounded;
    if (Math.abs(viewport.scrollTop - bounded) > 0.1) viewport.scrollTop = bounded;
  }, []);

  const applyLayout = useCallback(() => {
    const viewport = optionsRef.current.viewportRef.current;
    const session = sessionRef.current;
    if (!viewport || !session) return;
    let rows = rowGeometry(viewport);
    if (session.pendingSubmittedRow) {
      const submitted = rows.find((row) => row.id === session.pendingSubmittedRow);
      if (submitted) {
        cancelReveal(session);
        session.missingAnchor = false;
        session.mode = "anchoring-submitted-turn";
        session.pendingSubmittedRow = null;
        writeScroll(viewport.scrollTop + submitted.top - CONVERSATION_ANCHOR_OFFSET_PX);
        rows = rowGeometry(viewport);
        const actual = rows.find((row) => row.id === submitted.id)!;
        session.anchor = { rowId: actual.id, offset: actual.top, neighbors: [] };
      }
    }
    if (session.mode === "following-end") {
      writeScroll(conversationScrollEnd(viewport.scrollHeight, viewport.clientHeight));
      session.anchor = captureConversationAnchor(rowGeometry(viewport));
      publish(false);
      return;
    }
    const anchor = session.anchor;
    let missing = session.missingAnchor;
    if (anchor) {
      const found = rows.some((row) => row.id === anchor.rowId);
      if (!found && session.requestedAnchor !== anchor.rowId) {
        session.requestedAnchor = anchor.rowId;
        cancelReveal(session);
        const reveal = { controller: new AbortController(), timer: null as number | null, frame: null as number | null };
        session.reveal = reveal;
        const isCurrent = () => sessionRef.current === session && session.reveal === reveal && !reveal.controller.signal.aborted;
        const finish = () => {
          if (!isCurrent()) return;
          cancelReveal(session);
          applyLayoutRef.current();
        };
        try {
          const result = optionsRef.current.revealAnchor?.(anchor.rowId, reveal.controller.signal) ?? false;
          if (result) {
            session.awaitingAnchor = true;
            if (typeof result === "boolean") {
              reveal.frame = window.requestAnimationFrame(finish);
            } else {
              const timeout = optionsRef.current.revealTimeoutMs;
              reveal.timer = window.setTimeout(finish, Number.isFinite(timeout) ? Math.max(1, Math.min(timeout!, 60_000)) : 15_000);
              Promise.resolve(result).then((available) => {
                if (!isCurrent()) return;
                if (available) reveal.frame = window.requestAnimationFrame(finish);
                else finish();
              }, finish);
            }
          } else cancelReveal(session);
        } catch {
          cancelReveal(session);
        }
      }
      if (!found && session.awaitingAnchor) {
        // Publishing can render again before the host reveal lands. Keep the
        // requested anchor until the host has mounted the promised row.
        publish(false);
        return;
      }
      cancelReveal(session);
      const restored = restoreConversationAnchor(anchor, rows, viewport.scrollTop, conversationScrollEnd(viewport.scrollHeight, viewport.clientHeight));
      if (!found) missing = true;
      if (restored) writeScroll(restored.scrollTop);
      // An unavailable row falls back to its nearest retained neighbor. Once
      // chosen it becomes the new anchor, instead of repeating reveal requests.
      if (!found) session.anchor = captureConversationAnchor(rowGeometry(viewport));
    } else {
      session.anchor = captureConversationAnchor(rows);
    }
    publish(missing);
  }, [publish, writeScroll]);
  applyLayoutRef.current = applyLayout;

  const notifyLayoutChange = useCallback(() => {
    if (frameRef.current !== null) return;
    frameRef.current = window.requestAnimationFrame(() => {
      frameRef.current = null;
      applyLayout();
    });
  }, [applyLayout]);

  const readHistory = useCallback(() => {
    const viewport = optionsRef.current.viewportRef.current;
    const session = sessionRef.current;
    if (!viewport || !session) return;
    session.mode = "reading-history";
    session.pendingSubmittedRow = null;
    session.requestedAnchor = null;
    cancelReveal(session);
    session.missingAnchor = false;
    session.anchor = captureConversationAnchor(rowGeometry(viewport));
    publish();
  }, [publish]);

  const jumpToLatest = useCallback(() => {
    const session = sessionRef.current;
    if (!session) return;
    session.mode = "following-end";
    session.pendingSubmittedRow = null;
    session.requestedAnchor = null;
    cancelReveal(session);
    session.missingAnchor = false;
    applyLayout();
  }, [applyLayout]);

  const jumpToRow = useCallback((rowId: string): boolean => {
    const viewport = optionsRef.current.viewportRef.current;
    const session = sessionRef.current;
    if (!viewport || !session) return false;
    session.mode = "reading-history";
    session.pendingSubmittedRow = null;
    session.requestedAnchor = null;
    cancelReveal(session);
    session.missingAnchor = false;
    session.anchor = { rowId, offset: CONVERSATION_ANCHOR_OFFSET_PX, neighbors: [] };
    applyLayout();
    // No smooth scrolling: a rail jump is exact, including under reduced motion.
    return rowGeometry(viewport).some((row) => row.id === rowId);
  }, [applyLayout]);

  useLayoutEffect(() => {
    const previous = sessionRef.current;
    if (!previous || previous.key !== key || previous.authority !== authority) {
      if (previous) cancelReveal(previous);
      if (previous && previous.authority !== authority) {
        if (previous.authority !== undefined) sharedPositions.deleteAuthority(previous.authority);
        localPositions.current.clear();
      }
      const remembered = (authority === undefined ? localPositions.current : sharedPositions).read(key);
      sessionRef.current = {
        key, authority,
        mode: remembered?.mode ?? "following-end",
        anchor: remembered?.anchor ?? null,
        pendingSubmittedRow: remembered?.pendingSubmittedRow ?? null,
        lastSubmittedRow: remembered?.lastSubmittedRow ?? null,
        expectedScrollTop: null,
        requestedAnchor: null,
        awaitingAnchor: false,
        missingAnchor: false,
      };
    }
    const session = sessionRef.current!;
    if (options.submittedRowId && session.lastSubmittedRow !== options.submittedRowId) {
      session.lastSubmittedRow = options.submittedRowId;
      session.pendingSubmittedRow = options.submittedRowId;
    }
    applyLayout();
  });

  useLayoutEffect(() => {
    const viewport = options.viewportRef.current;
    if (!viewport) return;
    const observedSession = sessionRef.current;
    const previousOverflowAnchor = viewport.style.overflowAnchor;
    const previousSnap = viewport.style.scrollSnapType;
    viewport.style.overflowAnchor = "none";
    viewport.style.scrollSnapType = "none";
    const onScroll = () => {
      const session = sessionRef.current;
      if (!session) return;
      // Clearing rows while an identity's authorized history loads can clamp
      // scrollTop and emit a native scroll event. Preserve the pending anchor;
      // explicit pointer, wheel and keyboard intent cancel it via readHistory.
      if (session.awaitingAnchor) {
        publish();
        return;
      }
      if (session.expectedScrollTop !== null && Math.abs(viewport.scrollTop - session.expectedScrollTop) <= 1) {
        publish();
        return;
      }
      session.expectedScrollTop = null;
      session.mode = conversationIsAtEnd(viewport.scrollTop, viewport.scrollHeight, viewport.clientHeight) ? "following-end" : "reading-history";
      session.pendingSubmittedRow = null;
      session.requestedAnchor = null;
      cancelReveal(session);
      session.missingAnchor = false;
      session.anchor = captureConversationAnchor(rowGeometry(viewport));
      publish();
    };
    const onWheel = (event: WheelEvent) => {
      if (isConversationScrollTarget(event.target, viewport, event.deltaY, event.deltaX)) readHistory();
    };
    const onKey = (event: KeyboardEvent) => {
      if (event.target instanceof Element && event.target.closest("input,textarea,select,[contenteditable=true]")) return;
      const delta = ["ArrowUp", "PageUp", "Home"].includes(event.key) || (event.key === " " && event.shiftKey) ? -1
        : ["ArrowDown", "PageDown", "End", " "].includes(event.key) ? 1 : 0;
      if (isConversationScrollTarget(event.target, viewport, delta)) readHistory();
    };
    const onSelection = () => {
      const selection = viewport.ownerDocument.getSelection();
      if (selection && !selection.isCollapsed && selection.anchorNode && viewport.contains(selection.anchorNode)) readHistory();
    };
    // A browser reveal or focus scroll can precede its scroll event. Capture
    // the visible position before a pointer-triggered render applies layout,
    // otherwise the old anchor can move a button between mouse down and up.
    const onPointerDown = () => readHistory();
    viewport.addEventListener("scroll", onScroll, { passive: true });
    viewport.addEventListener("pointerdown", onPointerDown, true);
    viewport.addEventListener("wheel", onWheel, { passive: true });
    viewport.addEventListener("keydown", onKey);
    viewport.addEventListener("load", notifyLayoutChange, true);
    viewport.ownerDocument.addEventListener("selectionchange", onSelection);
    window.addEventListener("resize", notifyLayoutChange);
    const resize = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(notifyLayoutChange);
    const observeRows = () => {
      resize?.disconnect();
      resize?.observe(viewport);
      if (optionsRef.current.contentRef?.current) resize?.observe(optionsRef.current.contentRef.current);
      else viewport.querySelectorAll(ROW_SELECTOR).forEach((row) => resize?.observe(row));
    };
    observeRows();
    const mutation = typeof MutationObserver === "undefined" ? null : new MutationObserver((records) => {
      if (records.some((record) => record.type === "childList")) observeRows();
      notifyLayoutChange();
    });
    mutation?.observe(viewport, { childList: true, subtree: true, characterData: true, attributes: true, attributeFilter: ["open", "hidden"] });
    return () => {
      if (observedSession) {
        if (observedSession.awaitingAnchor) observedSession.requestedAnchor = null;
        cancelReveal(observedSession);
      }
      if (frameRef.current !== null) window.cancelAnimationFrame(frameRef.current);
      frameRef.current = null;
      viewport.removeEventListener("scroll", onScroll);
      viewport.removeEventListener("pointerdown", onPointerDown, true);
      viewport.removeEventListener("wheel", onWheel);
      viewport.removeEventListener("keydown", onKey);
      viewport.removeEventListener("load", notifyLayoutChange, true);
      viewport.ownerDocument.removeEventListener("selectionchange", onSelection);
      window.removeEventListener("resize", notifyLayoutChange);
      resize?.disconnect();
      mutation?.disconnect();
      viewport.style.overflowAnchor = previousOverflowAnchor;
      viewport.style.scrollSnapType = previousSnap;
    };
  }, [key, authority, options.viewportRef, notifyLayoutChange, publish, readHistory]);

  return { ...state, jumpToLatest, jumpToRow, readHistory, captureBeforePrepend: readHistory, notifyLayoutChange };
}
