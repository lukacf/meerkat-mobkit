import clsx from "clsx";
import type { ReactNode } from "react";

import type { ConversationViewState } from "@console-core";

import { ConversationEmptyState } from "./conversation-empty-state";
import { ConversationTranscript } from "./conversation-transcript";
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
};

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
}: ConversationPaneProps) {
  const canRenderTurnDiff = Boolean(showTurnDiff && viewState.turnDiff && onToggleDiffFile);
  const showEmptyState = Boolean(viewState.emptyState && viewState.entries.length === 0 && !canRenderTurnDiff);

  return (
    <div className={clsx("cc-theme-scope", "cc-conversation-pane", className)}>
      <section className={clsx("cc-conversation-pane__scroll", scrollClassName)}>
        <div className={clsx("cc-conversation-pane__body", bodyClassName)}>
          {showEmptyState && viewState.emptyState ? (
            <ConversationEmptyState Icon={Icon} onApplySuggestion={onApplySuggestion} state={viewState.emptyState} />
          ) : (
            <ConversationTranscript
              Icon={Icon}
              compact={compact}
              expandedDiffFile={expandedDiffFile}
              maxGroups={maxGroups}
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
