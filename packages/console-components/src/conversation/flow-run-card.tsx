import { useState, type CSSProperties } from "react";

import {
  parseConversationRichBlocks,
  type ConversationFlowRunEntry,
  type ConversationFlowRunMemberRow,
  type FlowRunStatus,
} from "@console-core";

import { ConversationRichContent } from "./conversation-rich-content";
import { ConversationTranscript } from "./conversation-transcript";
import type { IconRenderer } from "../shared";

const STATUS_LABEL: Record<FlowRunStatus, string> = {
  idle: "Queued",
  running: "Working",
  completed: "Done",
  failed: "Failed",
};

function statusDotClass(status: FlowRunStatus): string {
  return `cc-flow-run__dot is-${status}`;
}

function MemberRow({
  row,
  Icon,
  onMessageMember,
}: {
  row: ConversationFlowRunMemberRow;
  Icon?: IconRenderer | null;
  onMessageMember?: ((memberKey: string) => void) | null;
}) {
  const [expanded, setExpanded] = useState(false);
  const hasDetail = Boolean(row.subView && row.subView.groups.length);
  const style = (row.tone?.variables || undefined) as CSSProperties | undefined;

  return (
    <li
      className={`cc-flow-run__member is-${row.status}${expanded ? " is-expanded" : ""}`}
      data-flow-run-member={row.memberKey}
      style={style}
    >
      <div className="cc-flow-run__member-line">
        <button
          type="button"
          className="cc-flow-run__member-row"
          disabled={!hasDetail}
          aria-expanded={hasDetail ? expanded : undefined}
          onClick={hasDetail ? () => setExpanded((value) => !value) : undefined}
        >
          <span className={statusDotClass(row.status)} aria-hidden="true" />
          <span className="cc-flow-run__member-label">{row.label}</span>
          <span className="cc-flow-run__member-caption">{row.caption}</span>
          {hasDetail ? (
            <span className="cc-flow-run__member-chevron" aria-hidden="true">{expanded ? "▾" : "▸"}</span>
          ) : (
            <span className="cc-flow-run__member-status">{STATUS_LABEL[row.status]}</span>
          )}
        </button>
        {onMessageMember ? (
          <button
            type="button"
            className="cc-flow-run__member-message"
            title={`Message ${row.label}`}
            onClick={(event) => {
              event.stopPropagation();
              onMessageMember(row.memberKey);
            }}
          >
            Message
          </button>
        ) : null}
      </div>
      {hasDetail && expanded && row.subView ? (
        <div className="cc-flow-run__member-detail">
          <ConversationTranscript
            viewState={row.subView}
            compact
            showTurnDiff={false}
            Icon={Icon}
          />
        </div>
      ) : null}
    </li>
  );
}

export function FlowRunCard({
  entry,
  Icon,
  onMessageMember,
  onRestore,
}: {
  entry: ConversationFlowRunEntry;
  Icon?: IconRenderer | null;
  onMessageMember?: ((memberKey: string) => void) | null;
  onRestore?: (() => void) | null;
}) {
  // Message targeting needs a live member behind the row; paused (restorable)
  // crews only carry persisted job history, so the affordance is Resume.
  const memberMessageHandler = entry.restorable ? null : onMessageMember;
  return (
    <section
      className={`cc-flow-run is-${entry.status}`}
      data-flow-run-card=""
      data-helper-id={entry.helperId}
      data-status={entry.status}
    >
      <header className="cc-flow-run__header">
        <span className="cc-flow-run__mark" aria-hidden="true">
          {Icon ? <Icon name="i-team" /> : "◇"}
        </span>
        <div className="cc-flow-run__heading">
          <span className="cc-flow-run__name">{entry.flowName}</span>
          {entry.objective ? <span className="cc-flow-run__objective">{entry.objective}</span> : null}
        </div>
        {entry.restorable && onRestore ? (
          <button
            type="button"
            className="cc-flow-run__restore"
            title="Resume this crew's helpers"
            onClick={(event) => {
              event.stopPropagation();
              onRestore();
            }}
          >
            Resume
          </button>
        ) : null}
        <span className={`cc-flow-run__badge is-${entry.status}`}>{STATUS_LABEL[entry.status]}</span>
      </header>
      {entry.rows.length ? (
        <ul className="cc-flow-run__members">
          {entry.rows.map((row) => (
            <MemberRow key={row.memberKey} row={row} Icon={Icon} onMessageMember={memberMessageHandler} />
          ))}
        </ul>
      ) : null}
      {entry.outcome ? (
        // Crew outcomes are markdown (headings, lists, fences) — a raw text
        // node rendered them as an unformatted wall. Route through the same
        // rich-block pipeline as assistant prose.
        <div className="cc-flow-run__outcome">
          <ConversationRichContent blocks={parseConversationRichBlocks(entry.outcome)} Icon={Icon} />
        </div>
      ) : null}
    </section>
  );
}
