import clsx from "clsx";

import {
  conversationEntryText,
  conversationIdentityPresentation,
  conversationIdentityShowsLabel,
  conversationRichBlockHasCopyAction,
  type ConversationTimelineGroup,
} from "@console-core";

import { ConversationMessageView } from "./conversation-message-view";
import type { FlowRunRestoreHandler } from "./flow-run-card";
import type { WorkGraphCardActions } from "./work-graph-card";
import { CopyButton } from "../copy-button";
import { toneStyle, type IconRenderer } from "../shared";

function initialsForIdentity(group: ConversationTimelineGroup): string {
  const explicit = group.identity.avatarLabel?.trim();
  if (explicit) {
    return explicit.slice(0, 3).toUpperCase();
  }

  const tokens = group.identity.label
    .split(/\s+/u)
    .map((token) => token.trim())
    .filter(Boolean);
  if (!tokens.length) {
    return "?";
  }
  return tokens
    .slice(0, 2)
    .map((token) => token[0] || "")
    .join("")
    .toUpperCase();
}

function groupHasNestedCopyButton(group: ConversationTimelineGroup): boolean {
  return group.entries.length > 0 && group.entries.every((entry) => (
    entry.kind === "message"
    && entry.variant === "rich"
    && Boolean(entry.blocks?.length)
    && entry.blocks?.every((block) => conversationRichBlockHasCopyAction(block))
  ));
}

// Exported for unit tests (not re-exported from the package index).
export function groupCopyText(group: ConversationTimelineGroup): string {
  // Tool-call-only entries are excluded from "Copy response" (the #289
  // tool-chatter exclusion) UNLESS they carry peer conversation: peer
  // send_message/send_request cards are tool-call blocks whose bodies are
  // legitimate displayed conversational content and must copy with the group
  // (suppressing peer traffic content is a repo anti-goal). A text-emptiness
  // probe cannot make this distinction — plain tool calls always render
  // "$ <name>".
  const substantiveEntries = group.entries.filter((entry) => !(
    entry.kind === "message"
    && entry.variant === "rich"
    && entry.blocks?.length
    && entry.blocks.every((block) => block.type === "tool-call")
    && !entry.blocks.some(
      (block) =>
        block.type === "tool-call"
        && (block.peerBody?.trim() || block.peerTarget?.trim() || block.peerIncoming),
    )
  ));
  const copyEntries = substantiveEntries.length ? substantiveEntries : group.entries;
  if (copyEntries.length === group.entries.length && group.copyText) {
    return group.copyText;
  }
  return copyEntries.map((entry) => conversationEntryText(entry)).filter(Boolean).join("\n\n");
}

type ConversationMessageGroupProps = {
  group: ConversationTimelineGroup;
  compact?: boolean;
  Icon?: IconRenderer | null;
  onFlowRunMessageMember?: ((memberKey: string) => void) | null;
  onFlowRunRestore?: FlowRunRestoreHandler | null;
  workGraphActions?: WorkGraphCardActions | null;
};

export function ConversationMessageGroup({
  group,
  compact = false,
  Icon,
  onFlowRunMessageMember = null,
  onFlowRunRestore = null,
  workGraphActions = null,
}: ConversationMessageGroupProps) {
  const presentation = conversationIdentityPresentation(group.identity);
  const isUserGroup = presentation === "user";

  if (isUserGroup) {
    return (
      <>
        {group.entries.map((entry) => (
          <ConversationMessageView
            compact={compact}
            entry={entry}
            Icon={Icon}
            key={entry.id}
            onFlowRunMessageMember={onFlowRunMessageMember}
            onFlowRunRestore={onFlowRunRestore}
            workGraphActions={workGraphActions}
          />
        ))}
      </>
    );
  }

  const copyText = groupCopyText(group);
  const showGroupCopy = !compact && !groupHasNestedCopyButton(group);
  const showIdentity = conversationIdentityShowsLabel(group.identity);

  return (
    <section
      className={clsx(
        "cc-message-group",
        compact && "is-compact",
        `is-${presentation}`,
        showIdentity && "has-identity",
      )}
      style={toneStyle(group.identity.tone)}
    >
      {showIdentity ? (
        <div className="cc-message-group__identity">
          <span className="cc-message-group__identity-mark" aria-hidden="true">{initialsForIdentity(group)}</span>
          <span className="cc-message-group__identity-copy">
            <span className="cc-message-group__identity-label">{group.identity.label}</span>
            {group.identity.meta ? <span className="cc-message-group__identity-meta">{group.identity.meta}</span> : null}
          </span>
        </div>
      ) : null}
      <div className="cc-message-group__body">
        {group.entries.map((entry) => (
          <ConversationMessageView
            compact={compact}
            entry={entry}
            Icon={Icon}
            key={entry.id}
            onFlowRunMessageMember={onFlowRunMessageMember}
            onFlowRunRestore={onFlowRunRestore}
            workGraphActions={workGraphActions}
          />
        ))}
      </div>
      {showGroupCopy ? (
        <div className="cc-message-group__actions">
          <CopyButton
            className="cc-message-group__copy"
            copiedLabel="Copied response"
            Icon={Icon}
            label="Copy response"
            text={copyText}
          />
        </div>
      ) : null}
    </section>
  );
}
