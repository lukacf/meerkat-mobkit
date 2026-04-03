import { Fragment } from "react";

import {
  conversationIdentityPresentation,
  type ConversationTimelineEntry,
} from "@console-core";

import { ConversationRichContent } from "./conversation-rich-content";
import { SummaryCard } from "./summary-card";
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
};

export function ConversationMessageView({
  entry,
  compact = false,
  Icon,
}: ConversationMessageViewProps) {
  const presentation = conversationIdentityPresentation(entry.identity);
  const assistantClassName = [
    "cc-message",
    "cc-message--assistant",
    presentation === "participant" ? "cc-message--participant" : "",
    presentation === "system" ? "cc-message--system" : "",
  ].filter(Boolean).join(" ");

  if (entry.kind === "summary") {
    return <SummaryCard entry={entry} />;
  }

  if (entry.variant === "meta") {
    return <article className={`${assistantClassName} cc-message--meta`}><p>{entry.text}</p></article>;
  }

  if (presentation === "user") {
    const copyText = entry.copyText || entry.text || "";
    return (
      <article className="cc-message cc-message--user">
        {!compact ? (
          <CopyButton
            className="cc-message__copy"
            copiedLabel="Copied message"
            Icon={Icon}
            label="Copy message"
            text={copyText}
          />
        ) : null}
        <p>{renderMultilineText(entry.text || "")}</p>
      </article>
    );
  }

  if (entry.variant === "rich" && entry.blocks?.length) {
    return (
      <article className={`${assistantClassName} cc-message--rich`}>
        <ConversationRichContent blocks={entry.blocks} Icon={Icon} richStyle={entry.richStyle} />
      </article>
    );
  }

  return (
    <article className={assistantClassName}>
      <p>{entry.text || ""}</p>
    </article>
  );
}
