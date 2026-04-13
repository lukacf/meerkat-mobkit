import clsx from "clsx";

import {
  renderConversationInlineMarkdown,
  type ConversationRichBlock,
  type ConversationRichCodeBlock,
  type ConversationRichCommandBlock,
  type ConversationRichFileChangeBlock,
  type ConversationRichToolCallBlock,
  type ConversationTableAlignment,
  type ConversationRichThinkingBlock,
} from "@console-core";

import { useState } from "react";

import { ChangeStatPair } from "./change-stat-pair";
import { CopyButton } from "../copy-button";
import type { IconRenderer } from "../shared";

type ConversationRichContentProps = {
  blocks: ConversationRichBlock[];
  richStyle?: "default" | "streaming";
  Icon?: IconRenderer | null;
};

function markdownHtml(text: string) {
  return { __html: renderConversationInlineMarkdown(text) };
}

function commandCopyText(block: ConversationRichCommandBlock): string {
  return [block.title, block.body, block.output || "", block.footer || ""].filter(Boolean).join("\n").trim();
}

function fileChangeCopyText(block: ConversationRichFileChangeBlock): string {
  return [
    block.verb,
    block.before || "",
    block.name,
    block.after || "",
    `+${block.plus}`,
    `-${block.minus}`,
  ].filter(Boolean).join(" ").replace(/\s+/g, " ").trim();
}

function alignmentAttr(alignment: ConversationTableAlignment | null | undefined) {
  return alignment || "left";
}

function renderThinkingBlock(block: ConversationRichThinkingBlock) {
  if (!block.label?.trim() && !block.text?.trim()) {
    return null;
  }
  return (
    <div
      className={clsx(
        "cc-rich-thinking",
        block.final && "cc-rich-thinking--final",
        block.persisted && "cc-rich-thinking--persisted",
      )}
    >
      <div className="cc-rich-thinking__label">{block.label}</div>
      <p className="cc-rich-paragraph" dangerouslySetInnerHTML={markdownHtml(block.text)} />
    </div>
  );
}

function renderBlock(
  block: ConversationRichBlock,
  index: number,
  Icon?: IconRenderer | null,
) {
  if (block.type === "paragraph") {
    return <p className="cc-rich-paragraph" dangerouslySetInnerHTML={markdownHtml(block.text)} key={`paragraph-${index}`} />;
  }

  if (block.type === "heading") {
    return (
      <h3
        className={`cc-rich-heading cc-rich-heading--${Number(block.level) || 2}`}
        dangerouslySetInnerHTML={markdownHtml(block.text)}
        key={`heading-${index}`}
      />
    );
  }

  if (block.type === "code") {
    const codeBlock = block as ConversationRichCodeBlock;
    return (
      <section className="cc-rich-code-card" key={`code-${index}`}>
        <div className="cc-rich-code-card__header">
          <span className="cc-rich-code-language">{codeBlock.language || "text"}</span>
          <CopyButton
            copiedLabel="Copied code"
            Icon={Icon}
            label="Copy code"
            text={codeBlock.body}
          />
        </div>
        <pre className="cc-rich-code-body">
          {codeBlock.highlightedHtml ? (
            <code
              className={`cc-rich-code-content language-${codeBlock.language || "text"}`}
              dangerouslySetInnerHTML={{ __html: codeBlock.highlightedHtml }}
            />
          ) : (
            <code className={`cc-rich-code-content language-${codeBlock.language || "text"}`}>{codeBlock.body}</code>
          )}
        </pre>
      </section>
    );
  }

  if (block.type === "table") {
    return (
      <div className="cc-rich-table-wrap" key={`table-${index}`}>
        <table className="cc-rich-table">
          <thead>
            <tr>
              {block.headers.map((header, cellIndex) => (
                <th
                  data-align={alignmentAttr(block.alignments[cellIndex])}
                  dangerouslySetInnerHTML={markdownHtml(header)}
                  key={`header-${cellIndex}`}
                />
              ))}
            </tr>
          </thead>
          <tbody>
            {block.rows.map((row, rowIndex) => (
              <tr key={`row-${rowIndex}`}>
                {block.headers.map((_header, cellIndex) => (
                  <td
                    data-align={alignmentAttr(block.alignments[cellIndex])}
                    dangerouslySetInnerHTML={markdownHtml(row[cellIndex] || "")}
                    key={`cell-${rowIndex}-${cellIndex}`}
                  />
                ))}
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    );
  }

  if (block.type === "command") {
    return (
      <div className="cc-rich-command-stack" key={`command-${index}`}>
        <div className="cc-rich-command-caption">{block.caption}</div>
        <div className="cc-rich-command-card">
          <div className="cc-rich-command-card__header">
            <div className="cc-rich-command-card__title">{block.title}</div>
            <CopyButton
              copiedLabel="Copied command output"
              Icon={Icon}
              label="Copy command output"
              text={commandCopyText(block)}
            />
          </div>
          <pre className="cc-rich-command-card__body">{block.body}</pre>
          {block.output ? <pre className="cc-rich-command-card__output">{block.output}</pre> : null}
          {block.footer ? <div className="cc-rich-command-card__footer">{block.footer}</div> : null}
        </div>
      </div>
    );
  }

  if (block.type === "file-change") {
    return (
      <section className="cc-rich-file-change" key={`file-change-${index}`}>
        <div className="cc-rich-file-change__main">
          <span className="cc-rich-file-change__verb">{block.verb}</span>
          {block.before ? <span className="cc-rich-file-change__context" dangerouslySetInnerHTML={markdownHtml(block.before)} /> : null}
          <button className="cc-rich-file-change__link" type="button">{block.name}</button>
          {block.after ? <span className="cc-rich-file-change__context" dangerouslySetInnerHTML={markdownHtml(block.after)} /> : null}
        </div>
        <div className="cc-rich-file-change__stats">
          <ChangeStatPair minus={block.minus} plus={block.plus} />
          <span className="cc-rich-file-change__dot" />
          <CopyButton
            copiedLabel="Copied file change"
            Icon={Icon}
            label="Copy file change"
            text={fileChangeCopyText(block)}
          />
        </div>
      </section>
    );
  }

  if (block.type === "divider") {
    return (
      <div className="cc-rich-divider" key={`divider-${index}`}>
        <span className="cc-rich-divider__line" />
        <span className="cc-rich-divider__label">{block.text}</span>
        <span className="cc-rich-divider__line" />
      </div>
    );
  }

  if (block.type === "tool-call") {
    return <ToolCallBlock block={block} key={`tool-call-${index}`} />;
  }

  const thinking = renderThinkingBlock(block);
  if (!thinking) {
    return null;
  }
  return <div key={`thinking-${index}`}>{thinking}</div>;
}

const PEER_TOOL_NAMES = new Set(["send_request", "send_message", "send_response"]);

function ToolCallBlock({ block }: { block: ConversationRichToolCallBlock }) {
  const [expanded, setExpanded] = useState(false);
  const isPeer = PEER_TOOL_NAMES.has(block.name);
  const statusIcon = block.status === "success" ? "✓" : block.status === "error" ? "✗" : "⋯";
  const statusClass = `cc-tool-call--${block.status}`;

  if (isPeer) {
    const target = block.peerTarget || "peer";
    const content = block.peerBody || block.peerIntent || "";
    return (
      <section className={clsx("cc-tool-call cc-tool-call--peer", statusClass)}>
        <button
          className="cc-tool-call__header"
          type="button"
          onClick={() => setExpanded((prev) => !prev)}
          aria-expanded={expanded}
        >
          <span className="cc-tool-call__chevron">{expanded ? "▾" : "▸"}</span>
          <span className="cc-tool-call__icon">↗</span>
          <span className="cc-tool-call__name">{target}</span>
          {content && <span className="cc-tool-call__preview">{content}</span>}
          <span className="cc-tool-call__status">{statusIcon}</span>
        </button>
        {expanded && block.result && (
          <div className="cc-tool-call__body">
            <pre className="cc-tool-call__pre">{block.result}</pre>
          </div>
        )}
      </section>
    );
  }

  // Generic tool call
  let argsPreview = block.arguments || "";
  try {
    const parsed = JSON.parse(argsPreview);
    if (typeof parsed === "object" && parsed !== null) {
      argsPreview = Object.entries(parsed)
        .map(([k, v]) => `${k}: ${typeof v === "string" ? v : JSON.stringify(v)}`)
        .join(", ");
    }
  } catch { /* use raw */ }

  return (
    <section className={clsx("cc-tool-call", statusClass)}>
      <button
        className="cc-tool-call__header"
        type="button"
        onClick={() => setExpanded((prev) => !prev)}
        aria-expanded={expanded}
      >
        <span className="cc-tool-call__chevron">{expanded ? "▾" : "▸"}</span>
        <span className="cc-tool-call__icon">⚙</span>
        <span className="cc-tool-call__name">{block.name}</span>
        {argsPreview && <span className="cc-tool-call__preview">{argsPreview}</span>}
        <span className="cc-tool-call__status">{statusIcon} {block.status === "pending" ? "Running" : block.status === "success" ? "Success" : "Failed"}</span>
      </button>
      {expanded && (
        <div className="cc-tool-call__body">
          {argsPreview && (
            <div className="cc-tool-call__section">
              <div className="cc-tool-call__section-label">Input</div>
              <pre className="cc-tool-call__pre">{argsPreview}</pre>
            </div>
          )}
          {block.result && (
            <div className="cc-tool-call__section">
              <div className="cc-tool-call__section-label">Result</div>
              <pre className="cc-tool-call__pre">{block.result}</pre>
            </div>
          )}
        </div>
      )}
    </section>
  );
}

function PeerToolGroup({ blocks }: { blocks: ConversationRichToolCallBlock[] }) {
  const [expanded, setExpanded] = useState(false);
  const targets = blocks.map((b) => b.peerTarget || "peer");
  const allSuccess = blocks.every((b) => b.status === "success");
  const anyError = blocks.some((b) => b.status === "error");
  const statusIcon = anyError ? "✗" : allSuccess ? "✓" : "⋯";
  const statusClass = anyError ? "cc-tool-call--error" : allSuccess ? "cc-tool-call--success" : "cc-tool-call--pending";

  return (
    <section className={clsx("cc-tool-call cc-tool-call--peer-group", statusClass)}>
      <button
        className="cc-tool-call__header"
        type="button"
        onClick={() => setExpanded((prev) => !prev)}
        aria-expanded={expanded}
      >
        <span className="cc-tool-call__chevron">{expanded ? "▾" : "▸"}</span>
        <span className="cc-tool-call__icon">↗</span>
        <span className="cc-tool-call__name">Sent to {targets.join(", ")}</span>
        <span className="cc-tool-call__status">{statusIcon}</span>
      </button>
      {expanded && (
        <div className="cc-tool-call__body">
          {blocks.map((block, i) => (
            <div className="cc-tool-call__peer-row" key={block.toolCallId || i}>
              <span className="cc-tool-call__peer-target">→ {block.peerTarget || "peer"}</span>
              {block.peerIntent && <span className="cc-tool-call__peer-intent">{block.peerIntent}</span>}
              {block.peerBody && <span className="cc-tool-call__peer-body">{block.peerBody}</span>}
              <span className={`cc-tool-call__peer-status cc-tool-call__peer-status--${block.status}`}>
                {block.status === "success" ? "✓" : block.status === "error" ? "✗" : "⋯"}
              </span>
            </div>
          ))}
        </div>
      )}
    </section>
  );
}

export function ConversationRichContent({
  blocks,
  richStyle = "default",
  Icon,
}: ConversationRichContentProps) {
  // Check if all blocks are peer tool calls — render as grouped blob
  const allPeerTools = blocks.length > 1
    && blocks.every((b) => b.type === "tool-call" && PEER_TOOL_NAMES.has((b as ConversationRichToolCallBlock).name));

  if (allPeerTools) {
    return <PeerToolGroup blocks={blocks as ConversationRichToolCallBlock[]} />;
  }

  const body = blocks
    .map((block, index) => renderBlock(block, index, Icon))
    .filter(Boolean);

  if (body.length === 0) {
    return null;
  }

  if (richStyle === "streaming") {
    return <div className="cc-rich-streaming">{body}</div>;
  }

  return <>{body}</>;
}
