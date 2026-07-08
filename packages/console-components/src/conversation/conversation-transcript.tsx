import clsx from "clsx";

import type { ConversationViewState } from "@console-core";

import { ConversationMessageGroup } from "./conversation-message-group";
import type { WorkGraphCardActions } from "./work-graph-card";
import { groupConversationTranscriptTurns } from "./conversation-turns";
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
  onFlowRunMessageMember?: ((memberKey: string) => void) | null;
  onFlowRunRestore?: (() => void) | null;
  workGraphActions?: WorkGraphCardActions | null;
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
  onFlowRunMessageMember = null,
  onFlowRunRestore = null,
  workGraphActions = null,
}: ConversationTranscriptProps) {
  const canRenderTurnDiff = Boolean(showTurnDiff && viewState.turnDiff && onToggleDiffFile);
  const renderableTurnDiff = canRenderTurnDiff ? viewState.turnDiff : null;
  const groups = typeof maxGroups === "number" && maxGroups > 0
    ? viewState.groups.slice(-maxGroups)
    : viewState.groups;
  const turns = groupConversationTranscriptTurns(groups);

  if (!groups.length && !renderableTurnDiff) {
    return null;
  }

  return (
    <div className={clsx("cc-theme-scope", "cc-conversation-transcript", compact && "is-compact", className)}>
      {turns.map((turn, turnIndex) => {
        const isLastTurn = turnIndex === turns.length - 1;
        return (
          <section
            aria-label={`Turn ${turnIndex + 1}`}
            className="cc-conversation-turn"
            data-cc-conversation-turn-index={turnIndex}
            data-testid={`conversation-turn:${turnIndex}`}
            key={turn.id}
          >
            {turn.groups.map((group) => (
              <ConversationMessageGroup
                compact={compact}
                group={group}
                Icon={Icon}
                key={group.id}
                onFlowRunMessageMember={onFlowRunMessageMember}
                onFlowRunRestore={onFlowRunRestore}
                workGraphActions={workGraphActions}
              />
            ))}
            {isLastTurn && renderableTurnDiff && onToggleDiffFile ? (
              <TurnDiffCard
                expandedFile={expandedDiffFile}
                onToggleFile={onToggleDiffFile}
                turnDiff={renderableTurnDiff}
              />
            ) : null}
          </section>
        );
      })}
      {!turns.length && renderableTurnDiff && onToggleDiffFile ? (
        <section
          aria-label="Turn 1"
          className="cc-conversation-turn"
          data-cc-conversation-turn-index={0}
          data-testid="conversation-turn:0"
        >
          <TurnDiffCard
            expandedFile={expandedDiffFile}
            onToggleFile={onToggleDiffFile}
            turnDiff={renderableTurnDiff}
          />
        </section>
      ) : null}
    </div>
  );
}
