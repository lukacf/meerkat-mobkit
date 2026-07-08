import { Fragment } from "react";

import {
  conversationRichBlockCopyText,
  conversationRichBlocksToText,
  conversationIdentityPresentation,
  type ConversationTimelineEntry,
} from "@console-core";

import { ConversationRichContent } from "./conversation-rich-content";
import { FlowRunCard } from "./flow-run-card";
import { SummaryCard } from "./summary-card";
import { WorkGraphCard, type WorkGraphCardActions } from "./work-graph-card";
import { CopyButton } from "../copy-button";
import type { IconRenderer } from "../shared";

function renderMultilineText(text: string) {
  return text.split("\n").map((line, index) => (
    <Fragment key={`${line}-${index}`}>
      {index > 0 ? <br /> : null}
      {line}
    </Fragment>
  ));
}

type ConversationMessageViewProps = {
  entry: ConversationTimelineEntry;
  compact?: boolean;
  Icon?: IconRenderer | null;
  onFlowRunMessageMember?: ((memberKey: string) => void) | null;
  onFlowRunRestore?: (() => void) | null;
  workGraphActions?: WorkGraphCardActions | null;
};

export function ConversationMessageView({
  entry,
  compact = false,
  Icon,
  onFlowRunMessageMember = null,
  onFlowRunRestore = null,
  workGraphActions = null,
}: ConversationMessageViewProps) {
  const presentation = conversationIdentityPresentation(entry.identity);
  const assistantClassName = [
    "cc-message",
    "cc-message--assistant",
    presentation === "participant" ? "cc-message--participant" : "",
    presentation === "system" ? "cc-message--system" : "",
  ].filter(Boolean).join(" ");

  if (entry.kind === "flow_run") {
    return (
      <FlowRunCard
        entry={entry}
        Icon={Icon}
        onMessageMember={onFlowRunMessageMember}
        onRestore={onFlowRunRestore}
      />
    );
  }

  if (entry.kind === "workgraph") {
    return (
      <WorkGraphCard
        entry={entry}
        Icon={Icon}
        actions={workGraphActions}
      />
    );
  }

  if (entry.kind === "summary") {
    return <SummaryCard entry={entry} />;
  }

  if (entry.variant === "meta") {
    return <article className={`${assistantClassName} cc-message--meta`}><p>{entry.text}</p></article>;
  }

  if (presentation === "user") {
    const visibleRichBlocks = entry.variant === "rich" && entry.blocks?.length
      ? entry.blocks.filter((block) => conversationRichBlockCopyText(block).trim().length > 0)
      : [];
    const copyText = entry.copyText || entry.text || conversationRichBlocksToText(visibleRichBlocks) || "";
    return (
      <article className={`cc-message cc-message--user${visibleRichBlocks.length ? " cc-message--rich" : ""}`}>
        {!compact ? (
          <CopyButton
            className="cc-message__copy"
            copiedLabel="Copied message"
            Icon={Icon}
            label="Copy message"
            text={copyText}
          />
        ) : null}
        {visibleRichBlocks.length ? (
          <ConversationRichContent blocks={visibleRichBlocks} Icon={Icon} richStyle={entry.richStyle} />
        ) : (
          <p>{renderMultilineText(entry.text || "")}</p>
        )}
      </article>
    );
  }

  const visibleRichBlocks = entry.variant === "rich" && entry.blocks?.length
    ? entry.blocks.filter((block) => conversationRichBlockCopyText(block).trim().length > 0)
    : [];

  if (entry.variant === "rich" && visibleRichBlocks.length) {
    return (
      <article className={`${assistantClassName} cc-message--rich`}>
        <ConversationRichContent blocks={visibleRichBlocks} Icon={Icon} richStyle={entry.richStyle} />
      </article>
    );
  }

  if (entry.variant === "rich") {
    return null;
  }

  return (
    <article className={assistantClassName}>
      <p>{entry.text || ""}</p>
    </article>
  );
}
