import clsx from "clsx";

import type { ConversationViewState } from "@console-core";

import { ConversationMessageGroup } from "./conversation-message-group";
import { TurnDiffCard } from "./turn-diff-card";
import type { IconRenderer } from "../shared";

export type ConversationTranscriptProps = {
  viewState: ConversationViewState;
  compact?: boolean;
  maxGroups?: number | null;
  showTurnDiff?: boolean;
  expandedDiffFile?: string | null;
  onToggleDiffFile?: ((filePath: string) => void) | null;
  Icon?: IconRenderer | null;
  className?: string;
};

export function ConversationTranscript({
  viewState,
  compact = false,
  maxGroups = null,
  showTurnDiff = true,
  expandedDiffFile = null,
  onToggleDiffFile = null,
  Icon,
  className,
}: ConversationTranscriptProps) {
  const canRenderTurnDiff = Boolean(showTurnDiff && viewState.turnDiff && onToggleDiffFile);
  const renderableTurnDiff = canRenderTurnDiff ? viewState.turnDiff : null;
  const groups = typeof maxGroups === "number" && maxGroups > 0
    ? viewState.groups.slice(-maxGroups)
    : viewState.groups;

  if (!groups.length && !renderableTurnDiff) {
    return null;
  }

  return (
    <div className={clsx("cc-theme-scope", "cc-conversation-transcript", compact && "is-compact", className)}>
      {groups.map((group) => (
        <ConversationMessageGroup compact={compact} group={group} Icon={Icon} key={group.id} />
      ))}
      {renderableTurnDiff && onToggleDiffFile ? (
        <TurnDiffCard
          expandedFile={expandedDiffFile}
          onToggleFile={onToggleDiffFile}
          turnDiff={renderableTurnDiff}
        />
      ) : null}
    </div>
  );
}
