import { Fragment } from "react";

import {
  conversationRichBlockCopyText,
  conversationRichBlocksToText,
  conversationIdentityPresentation,
  type ConversationTimelineEntry,
} from "@console-core";

import { ConversationRichContent } from "./conversation-rich-content";
import { ConversationConnectionEventView } from "./conversation-connection-event";
import { FlowRunCard, type FlowRunRestoreHandler } from "./flow-run-card";
import { SummaryCard } from "./summary-card";
import { CouncilCard } from "./council-card";
import { WorkGraphCard, type WorkGraphCardActions } from "./work-graph-card";
import { CopyButton } from "../copy-button";
import type { IconRenderer } from "../shared";

const SYSTEM_TASK_PROMPT_STYLE = { whiteSpace: "pre-wrap" } as const;

function renderMultilineText(text: string) {
  return text.split("\n").map((line, index) => (
    <Fragment key={`${line}-${index}`}>
      {index > 0 ? <br /> : null}
      {line}
    </Fragment>
  ));
}

function humanizeSystemTaskMetadata(value: string | null | undefined) {
  const normalized = String(value || "").trim().replace(/[_-]+/gu, " ");
  if (!normalized) {
    return "";
  }
  return `${normalized[0]?.toUpperCase() || ""}${normalized.slice(1)}`;
}

type ConversationMessageViewProps = {
  entry: ConversationTimelineEntry;
  compact?: boolean;
  Icon?: IconRenderer | null;
  onFlowRunMessageMember?: ((memberKey: string) => void) | null;
  onFlowRunRestore?: FlowRunRestoreHandler | null;
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

  if (entry.kind === "council") {
    // The shared render path, not just ChatPane. Without this branch a council
    // entry falls through to the generic message render and shows NOTHING,
    // because it carries no text and no blocks - the card would appear in the
    // app that wires it explicitly and vanish in every other consumer of
    // ConversationPane.
    return <CouncilCard entry={entry} Icon={Icon} />;
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

  if (entry.taskKind || entry.taskLabel) {
    const taskLabel = entry.taskLabel?.trim() || "System task";
    const taskMetadata = [
      humanizeSystemTaskMetadata(entry.taskKind),
      humanizeSystemTaskMetadata(entry.taskStatus),
    ].filter(Boolean).join(" · ");
    const systemTaskClassName = [
      "cc-message",
      "cc-message--assistant",
      "cc-message--system",
      "cc-message--system-task",
      "cc-summary-card",
      "cc-rich-thinking",
    ].join(" ");
    return (
      <details
        aria-label={taskLabel}
        className={systemTaskClassName}
        data-task-kind={entry.taskKind}
        data-task-status={entry.taskStatus}
      >
        <summary className="cc-rich-thinking__label">
          <span className="cc-summary-card__title">{taskLabel}</span>
          {taskMetadata ? (
            <span className="cc-message-group__identity-meta"> · {taskMetadata}</span>
          ) : null}
        </summary>
        <div className="cc-rich-thinking__body">
          <p className="cc-rich-paragraph" style={SYSTEM_TASK_PROMPT_STYLE}>
            {entry.text || ""}
          </p>
        </div>
      </details>
    );
  }

  if (entry.variant === "meta") {
    if (entry.connectionEvent?.peers?.length) {
      return <ConversationConnectionEventView event={entry.connectionEvent} />;
    }
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
