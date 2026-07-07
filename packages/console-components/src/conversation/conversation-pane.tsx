import clsx from "clsx";
import {
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from "react";

import type { ConversationViewState } from "@console-core";

import { ConversationEmptyState } from "./conversation-empty-state";
import { ConversationTranscript } from "./conversation-transcript";
import {
  conversationTurnPreview,
  groupConversationTranscriptTurns,
} from "./conversation-turns";
import type { IconRenderer } from "../shared";

export type ConversationPaneProps = {
  viewState: ConversationViewState;
  Icon?: IconRenderer | null;
  footer?: ReactNode;
  className?: string;
  scrollClassName?: string;
  bodyClassName?: string;
  compact?: boolean;
  maxGroups?: number | null;
  showTurnDiff?: boolean;
  expandedDiffFile?: string | null;
  onApplySuggestion?: (value: string) => void;
  onToggleDiffFile?: ((filePath: string) => void) | null;
  onFlowRunMessageMember?: ((memberKey: string) => void) | null;
  onFlowRunRestore?: (() => void) | null;
  // The jump-to-turn rail on the pane's edge. Default true (the MobKit
  // console ships it); meerkat-studio opts out until it adopts the rail
  // deliberately.
  showTurnRail?: boolean;
};

function visibleTranscriptGroups(viewState: ConversationViewState, maxGroups: number | null) {
  return typeof maxGroups === "number" && maxGroups > 0
    ? viewState.groups.slice(-maxGroups)
    : viewState.groups;
}

export function ConversationPane({
  viewState,
  Icon,
  footer = null,
  className,
  scrollClassName,
  bodyClassName,
  compact = false,
  maxGroups = null,
  showTurnDiff = true,
  expandedDiffFile = null,
  onApplySuggestion,
  onToggleDiffFile = null,
  onFlowRunMessageMember = null,
  onFlowRunRestore = null,
  showTurnRail: showTurnRailProp = true,
}: ConversationPaneProps) {
  const scrollRef = useRef<HTMLElement | null>(null);
  const [visibleTurnIndexes, setVisibleTurnIndexes] = useState<number[]>([]);
  const canRenderTurnDiff = Boolean(showTurnDiff && viewState.turnDiff && onToggleDiffFile);
  const showEmptyState = Boolean(viewState.emptyState && viewState.entries.length === 0 && !canRenderTurnDiff);
  const previousConversationRef = useRef<string | null>(null);
  const previousEntryCountRef = useRef(0);

  useLayoutEffect(() => {
    const scrollEl = scrollRef.current;
    if (!scrollEl) {
      return;
    }

    const isNewConversation = previousConversationRef.current !== viewState.conversationId;
    const entryCount = viewState.entries.length;
    const appended = entryCount >= previousEntryCountRef.current;
    const distanceFromBottom = scrollEl.scrollHeight - scrollEl.clientHeight - scrollEl.scrollTop;
    const shouldStickToBottom = isNewConversation || appended || distanceFromBottom < 96;

    previousConversationRef.current = viewState.conversationId;
    previousEntryCountRef.current = entryCount;

    if (!shouldStickToBottom) {
      return;
    }

    scrollEl.scrollTop = scrollEl.scrollHeight;
  }, [
    viewState.conversationId,
    viewState.entries.length,
    viewState.groups.length,
    viewState.turnDiff,
  ]);

  const visibleTurns = useMemo(
    () => groupConversationTranscriptTurns(visibleTranscriptGroups(viewState, maxGroups)),
    [maxGroups, viewState],
  );
  const railTurns = visibleTurns.length
    ? visibleTurns
    : canRenderTurnDiff
      ? [{ id: "turn-diff", groups: [] }]
      : [];
  const showTurnRail = showTurnRailProp && !showEmptyState && railTurns.length > 1;

  useEffect(() => {
    const scrollNode = scrollRef.current;
    if (!scrollNode || railTurns.length <= 1) {
      setVisibleTurnIndexes([]);
      return;
    }

    let frame = 0;
    const updateActiveTurn = () => {
      frame = 0;
      const turnNodes = Array.from(
        scrollNode.querySelectorAll<HTMLElement>("[data-cc-conversation-turn-index]"),
      );
      if (!turnNodes.length) {
        setVisibleTurnIndexes([]);
        return;
      }

      const scrollRect = scrollNode.getBoundingClientRect();
      const visibleTop = scrollRect.top;
      const visibleBottom = scrollRect.bottom;
      const targetY = scrollRect.top + Math.min(128, Math.max(48, scrollRect.height * 0.24));
      let nextIndex = 0;
      const nextVisibleIndexes: number[] = [];

      for (const turnNode of turnNodes) {
        const rawIndex = Number(turnNode.dataset.ccConversationTurnIndex);
        if (!Number.isFinite(rawIndex)) {
          continue;
        }
        const turnRect = turnNode.getBoundingClientRect();
        if (turnRect.bottom >= visibleTop && turnRect.top <= visibleBottom) {
          nextVisibleIndexes.push(rawIndex);
        }
        if (turnRect.top <= targetY) {
          nextIndex = rawIndex;
        }
      }

      const nextIndexes = nextVisibleIndexes.length > 0 ? nextVisibleIndexes : [nextIndex];
      setVisibleTurnIndexes((current) => {
        if (current.length === nextIndexes.length && current.every((value, index) => value === nextIndexes[index])) {
          return current;
        }
        return nextIndexes;
      });
    };

    const scheduleUpdate = () => {
      if (frame) {
        return;
      }
      frame = window.requestAnimationFrame(updateActiveTurn);
    };

    updateActiveTurn();
    scrollNode.addEventListener("scroll", scheduleUpdate, { passive: true });
    window.addEventListener("resize", scheduleUpdate);

    const Observer = window.ResizeObserver;
    const resizeObserver = Observer ? new Observer(scheduleUpdate) : null;
    resizeObserver?.observe(scrollNode);

    return () => {
      if (frame) {
        window.cancelAnimationFrame(frame);
      }
      scrollNode.removeEventListener("scroll", scheduleUpdate);
      window.removeEventListener("resize", scheduleUpdate);
      resizeObserver?.disconnect();
    };
  }, [railTurns.length]);

  function scrollToTurn(turnIndex: number) {
    const scrollNode = scrollRef.current;
    const turnNode = scrollNode?.querySelector<HTMLElement>(
      `[data-cc-conversation-turn-index="${turnIndex}"]`,
    );
    if (!turnNode) {
      return;
    }
    turnNode.scrollIntoView({
      block: "start",
      behavior: "smooth",
    });
  }

  return (
    <div className={clsx("cc-theme-scope", "cc-conversation-pane", className)}>
      {showTurnRail ? (
        <nav className="cc-conversation-turn-rail" aria-label="Conversation turns">
          <ol className="cc-conversation-turn-rail__list">
            {railTurns.map((turn, turnIndex) => {
              const isLastVisibleTurn = turnIndex === visibleTurns.length - 1;
              const isVisibleTurn = visibleTurnIndexes.includes(turnIndex);
              const preview = visibleTurns[turnIndex]
                ? conversationTurnPreview(
                    visibleTurns[turnIndex],
                    isLastVisibleTurn && showTurnDiff ? viewState.turnDiff : null,
                  )
                : null;
              return (
                <li className="cc-conversation-turn-rail__item" key={turn.id || `turn-${turnIndex}`}>
                  <button
                    aria-current={isVisibleTurn ? "true" : undefined}
                    aria-label={preview ? `Jump to turn ${turnIndex + 1}: ${preview.title}` : `Jump to turn ${turnIndex + 1}`}
                    className={clsx(
                      "cc-conversation-turn-rail__button",
                      isVisibleTurn && "is-active",
                    )}
                    data-testid={`conversation-turn-rail:${turnIndex}`}
                    onClick={(event) => {
                      scrollToTurn(turnIndex);
                      if (event.detail > 0) {
                        event.currentTarget.blur();
                      }
                    }}
                    type="button"
                  >
                    <span className="cc-conversation-turn-rail__tick" aria-hidden="true" />
                  </button>
                  {preview ? (
                    <div className="cc-conversation-turn-preview" role="presentation">
                      <div className="cc-conversation-turn-preview__title">{preview.title}</div>
                      <div className="cc-conversation-turn-preview__body">{preview.body}</div>
                      {preview.files.length || preview.hiddenFileCount ? (
                        <div className="cc-conversation-turn-preview__files">
                          {preview.files.map((file) => (
                            <span className="cc-conversation-turn-preview__file" key={file.name}>
                              {file.iconName && Icon ? (
                                <Icon className="cc-conversation-turn-preview__file-icon" name={file.iconName} />
                              ) : (
                                <span className="cc-conversation-turn-preview__file-icon" aria-hidden="true" />
                              )}
                              <span className="cc-conversation-turn-preview__file-name">{file.name}</span>
                            </span>
                          ))}
                          {preview.hiddenFileCount ? (
                            <span className="cc-conversation-turn-preview__file cc-conversation-turn-preview__file--more">
                              +{preview.hiddenFileCount}
                            </span>
                          ) : null}
                        </div>
                      ) : null}
                    </div>
                  ) : null}
                </li>
              );
            })}
          </ol>
        </nav>
      ) : null}
      <section ref={scrollRef} className={clsx("cc-conversation-pane__scroll", scrollClassName)}>
        <div className={clsx("cc-conversation-pane__body", bodyClassName)}>
          {showEmptyState && viewState.emptyState ? (
            <ConversationEmptyState Icon={Icon} onApplySuggestion={onApplySuggestion} state={viewState.emptyState} />
          ) : (
            <ConversationTranscript
              Icon={Icon}
              compact={compact}
              expandedDiffFile={expandedDiffFile}
              maxGroups={maxGroups}
              onFlowRunMessageMember={onFlowRunMessageMember}
              onFlowRunRestore={onFlowRunRestore}
              onToggleDiffFile={onToggleDiffFile}
              showTurnDiff={showTurnDiff}
              viewState={viewState}
            />
          )}
        </div>
      </section>
      {footer ? <div className="cc-conversation-pane__footer">{footer}</div> : null}
    </div>
  );
}
