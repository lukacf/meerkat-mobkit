import clsx from "clsx";

import {
  conversationRichPeerBodyForDisplay,
  conversationRichPeerIntentForDisplay,
  conversationRichPeerTargetForDisplay,
  normalizeConversationDisplayText,
  renderConversationInlineMarkdown,
  type ConversationRichBlock,
  type ConversationRichCodeBlock,
  type ConversationRichCommandBlock,
  type ConversationRichFileChangeBlock,
  type ConversationRichImageBlock,
  type ConversationRichToolCallBlock,
  type ConversationTableAlignment,
  type ConversationRichThinkingBlock,
} from "@console-core";

import { cloneElement, useState, type KeyboardEvent, type ReactElement } from "react";

import { ChangeStatPair } from "./change-stat-pair";
import { CopyButton } from "../copy-button";
import type { IconRenderer } from "../shared";

type ConversationRichContentProps = {
  blocks: ConversationRichBlock[];
  richStyle?: "default" | "streaming";
  Icon?: IconRenderer | null;
  // Default true preserves meerkat-studio's display normalization; the MobKit
  // console passes false for faithful rendering of raw agent text (pairs with
  // the parse-side `displayNormalization` option).
  displayNormalization?: boolean;
};

function markdownHtml(text: string, displayNormalization = true) {
  return { __html: renderConversationInlineMarkdown(text, { displayNormalization }) };
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

function renderThinkingBlock(block: ConversationRichThinkingBlock, displayNormalization = true) {
  if (!block.label?.trim() && !block.text?.trim()) {
    return null;
  }
  const collapsedByDefault = Boolean(block.final && block.persisted);
  return (
    <details
      className={clsx(
        "cc-rich-thinking",
        block.final && "cc-rich-thinking--final",
        block.persisted && "cc-rich-thinking--persisted",
        collapsedByDefault && "cc-rich-thinking--collapsed",
      )}
      open={!collapsedByDefault}
    >
      <summary className="cc-rich-thinking__label">{block.label}</summary>
      <p className="cc-rich-paragraph cc-rich-thinking__body" dangerouslySetInnerHTML={markdownHtml(block.text, displayNormalization)} />
    </details>
  );
}

function renderBlock(
  block: ConversationRichBlock,
  index: number,
  Icon?: IconRenderer | null,
  displayNormalization = true,
) {
  if (block.type === "paragraph") {
    return <p className="cc-rich-paragraph" dangerouslySetInnerHTML={markdownHtml(block.text, displayNormalization)} key={`paragraph-${index}`} />;
  }

  if (block.type === "heading") {
    return (
      <h3
        className={`cc-rich-heading cc-rich-heading--${Number(block.level) || 2}`}
        dangerouslySetInnerHTML={markdownHtml(block.text, displayNormalization)}
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
                  dangerouslySetInnerHTML={markdownHtml(header, displayNormalization)}
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
                    dangerouslySetInnerHTML={markdownHtml(row[cellIndex] || "", displayNormalization)}
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
          {block.before ? <span className="cc-rich-file-change__context" dangerouslySetInnerHTML={markdownHtml(block.before, displayNormalization)} /> : null}
          <button className="cc-rich-file-change__link" type="button">{block.name}</button>
          {block.after ? <span className="cc-rich-file-change__context" dangerouslySetInnerHTML={markdownHtml(block.after, displayNormalization)} /> : null}
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

  if (block.type === "image") {
    const image = block as ConversationRichImageBlock;
    return (
      <button
        className="cc-rich-image-button"
        key={`image-${index}`}
        onClick={() => window.open(image.src, "_blank", "noopener,noreferrer")}
        type="button"
      >
        <img
          alt={image.alt || ""}
          className="cc-rich-image"
          height={image.height}
          loading="lazy"
          src={image.src}
          width={image.width}
        />
      </button>
    );
  }

  if (block.type === "tool-call") {
    return <ToolCallBlock block={block} key={`tool-call-${index}`} />;
  }

  const thinking = renderThinkingBlock(block, displayNormalization);
  if (!thinking) {
    return null;
  }
  return <div key={`thinking-${index}`}>{thinking}</div>;
}

const PEER_TOOL_NAMES = new Set(["send_request", "send_message", "send_response"]);

function copyText(text: string) {
  navigator.clipboard?.writeText(text).catch(() => {});
}

/// Pretty-print JSON-shaped strings into a 2-space-indented form so
/// the expanded peer/tool body shows readable params instead of one
/// long line. Non-JSON strings pass through unchanged.
function formatJsonIfPossible(text: string): string {
  const trimmed = text.trim();
  if (!trimmed) return text;
  if (
    !((trimmed.startsWith("{") && trimmed.endsWith("}")) ||
      (trimmed.startsWith("[") && trimmed.endsWith("]")))
  ) {
    return text;
  }
  try {
    const parsed = JSON.parse(trimmed);
    return JSON.stringify(parsed, null, 2);
  } catch {
    return text;
  }
}

function toolBlockCopyText(block: ConversationRichToolCallBlock): string {
  if (block.peerTarget) {
    const dir = block.peerIncoming ? "← from" : "→ to";
    const peerBody = conversationRichPeerBodyForDisplay(block.peerBody);
    const result = meaningfulPeerResult(block.result);
    return [
      `${dir} ${conversationRichPeerTargetForDisplay(block.peerTarget)}`,
      conversationRichPeerIntentForDisplay(block.peerIntent, peerBody),
      peerBody,
      result,
    ].filter(Boolean).join(": ").trim();
  }
  const parts = [`$ ${block.name}`];
  if (block.arguments) parts.push(`Input: ${block.arguments}`);
  if (block.result) parts.push(`Result: ${block.result}`);
  return parts.join("\n").trim();
}

function parseObjectJson(text: string | null | undefined): Record<string, unknown> | null {
  const trimmed = String(text || "").trim();
  if (!trimmed || !trimmed.startsWith("{") || !trimmed.endsWith("}")) {
    return null;
  }
  try {
    const parsed = JSON.parse(trimmed);
    return parsed && typeof parsed === "object" && !Array.isArray(parsed)
      ? parsed as Record<string, unknown>
      : null;
  } catch {
    return null;
  }
}

function textFromUnknown(value: unknown): string {
  if (value == null) {
    return "";
  }
  if (typeof value === "string") {
    return normalizeConversationDisplayText(value).trim();
  }
  return normalizeConversationDisplayText(JSON.stringify(value, null, 2));
}

function meaningfulPeerResult(value: string | null | undefined): string {
  const text = normalizeConversationDisplayText(String(value || "")).trim();
  if (!text || /^(completed|delivered|ok|success)$/i.test(text)) {
    return "";
  }
  return formatJsonIfPossible(text);
}

function peerDetailRows(block: ConversationRichToolCallBlock): Array<{ label: string; value: string }> {
  const args = parseObjectJson(block.arguments) || {};
  const peerBody = conversationRichPeerBodyForDisplay(block.peerBody);
  const peerIntent = conversationRichPeerIntentForDisplay(block.peerIntent, peerBody);
  const body = peerBody
    || textFromUnknown(args.body)
    || textFromUnknown(args.message)
    || textFromUnknown(args.content)
    || textFromUnknown(args.text);
  const params = textFromUnknown(args.params);
  const requestId = textFromUnknown(args.in_reply_to)
    || textFromUnknown(args.inReplyTo)
    || textFromUnknown(args.request_id)
    || textFromUnknown(args.requestId);
  const result = meaningfulPeerResult(block.result);
  const primaryLabel = block.name === "send_request"
    ? "Request"
    : block.name === "send_response"
      ? "Response"
      : "Message";
  return [
    body ? { label: primaryLabel, value: body } : null,
    peerIntent ? { label: "Intent", value: peerIntent } : null,
    params ? { label: "Params", value: params } : null,
    requestId ? { label: "Request ID", value: requestId } : null,
    result ? { label: "Result", value: result } : null,
  ].filter(Boolean) as Array<{ label: string; value: string }>;
}

function CopyBtn({ text, label = "Copy" }: { text: string; label?: string }) {
  const [copied, setCopied] = useState(false);
  return (
    <button
      className="cc-tool-call__copy"
      type="button"
      title={label}
      onClick={(e) => { e.stopPropagation(); copyText(text); setCopied(true); setTimeout(() => setCopied(false), 1500); }}
    >
      {copied ? "✓" : "⎘"}
    </button>
  );
}

function onToolHeaderKeyDown(event: KeyboardEvent<HTMLDivElement>, toggle: () => void) {
  if (event.key !== "Enter" && event.key !== " ") {
    return;
  }
  event.preventDefault();
  toggle();
}

function ToolCallBlock({
  block,
  className,
}: {
  block: ConversationRichToolCallBlock;
  className?: string;
}) {
  const [expanded, setExpanded] = useState(false);
  const isPeer = PEER_TOOL_NAMES.has(block.name);
  const statusIcon = block.status === "success" ? "✓" : block.status === "error" ? "✗" : "⋯";
  const statusClass = `cc-tool-call--${block.status}`;

  if (isPeer || block.peerIncoming) {
    const target = conversationRichPeerTargetForDisplay(block.peerTarget);
    const peerBody = conversationRichPeerBodyForDisplay(block.peerBody);
    const peerIntent = conversationRichPeerIntentForDisplay(block.peerIntent, peerBody);
    const content = peerBody || peerIntent || "";
    const arrow = block.peerIncoming ? "↙" : "↗";
    const detailRows = peerDetailRows(block);
    return (
      <section className={clsx("cc-tool-call cc-tool-call--peer", block.peerIncoming && "cc-tool-call--incoming", statusClass, className)}>
        <div
          className="cc-tool-call__header"
          role="button"
          tabIndex={0}
          onClick={() => setExpanded((prev) => !prev)}
          onKeyDown={(event) => onToolHeaderKeyDown(event, () => setExpanded((prev) => !prev))}
          aria-expanded={expanded}
        >
          <span className="cc-tool-call__chevron">{expanded ? "▾" : "▸"}</span>
          <span className="cc-tool-call__icon">{arrow}</span>
          <span className="cc-tool-call__name">{block.peerIncoming ? `Received from ${target}` : target}</span>
          {peerIntent && <span className="cc-tool-call__peer-intent">{peerIntent}</span>}
          {content && <span className="cc-tool-call__preview">{content}</span>}
          <span className="cc-tool-call__status">{statusIcon}</span>
          <CopyBtn text={toolBlockCopyText(block)} />
        </div>
        {block.peerImages && block.peerImages.length > 0 && (
          <div className="cc-tool-call__attachments">
            {block.peerImages.map((image, index) => (
              <button
                className="cc-tool-call__image-button"
                key={`${image.blobId || image.imageId || image.src}-${index}`}
                onClick={() => window.open(image.src, "_blank", "noopener,noreferrer")}
                type="button"
              >
                <img
                  alt={image.alt || ""}
                  className="cc-tool-call__image"
                  height={image.height}
                  loading="lazy"
                  src={image.src}
                  width={image.width}
                />
              </button>
            ))}
          </div>
        )}
        {expanded && detailRows.length > 0 && (
          <div className="cc-tool-call__body">
            {detailRows.map((row) => (
              <div className="cc-tool-call__section" key={`${row.label}:${row.value}`}>
                <div className="cc-tool-call__section-label">{row.label}</div>
                <pre className="cc-tool-call__pre">{formatJsonIfPossible(row.value)}</pre>
              </div>
            ))}
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
    <section className={clsx("cc-tool-call", statusClass, className)}>
      <div
        className="cc-tool-call__header"
        role="button"
        tabIndex={0}
        onClick={() => setExpanded((prev) => !prev)}
        onKeyDown={(event) => onToolHeaderKeyDown(event, () => setExpanded((prev) => !prev))}
        aria-expanded={expanded}
      >
        <span className="cc-tool-call__chevron">{expanded ? "▾" : "▸"}</span>
        <span className="cc-tool-call__icon">⚙</span>
        <span className="cc-tool-call__name">{block.name}</span>
        {argsPreview && <span className="cc-tool-call__preview">{argsPreview}</span>}
        <span className="cc-tool-call__status">{statusIcon} {block.status === "pending" ? "Running" : block.status === "success" ? "Success" : "Failed"}</span>
        <CopyBtn text={toolBlockCopyText(block)} />
      </div>
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

/// Group of N tool calls of the same `name` that aren't peer tools.
/// Renders one collapsible card with `<icon> <name> ×N <status>` in
/// the header; expanded body lists each call's input + result. The
/// status icon is composite — success only when every call succeeded.
function ToolCallGroup({ blocks }: { blocks: ConversationRichToolCallBlock[] }) {
  const [expanded, setExpanded] = useState(false);
  const allSuccess = blocks.every((b) => b.status === "success");
  const anyError = blocks.some((b) => b.status === "error");
  const statusIcon = anyError ? "✗" : allSuccess ? "✓" : "⋯";
  const statusLabel = anyError ? "Failed" : allSuccess ? "Success" : "Running";
  const statusClass = anyError
    ? "cc-tool-call--error"
    : allSuccess
      ? "cc-tool-call--success"
      : "cc-tool-call--pending";
  const name = blocks[0]?.name || "tool";

  return (
    <section className={clsx("cc-tool-call cc-tool-call--group", statusClass)}>
      <div
        className="cc-tool-call__header"
        role="button"
        tabIndex={0}
        onClick={() => setExpanded((prev) => !prev)}
        onKeyDown={(event) => onToolHeaderKeyDown(event, () => setExpanded((prev) => !prev))}
        aria-expanded={expanded}
      >
        <span className="cc-tool-call__chevron">{expanded ? "▾" : "▸"}</span>
        <span className="cc-tool-call__icon">⚙</span>
        <span className="cc-tool-call__name">{name}</span>
        <span className="cc-tool-call__count">×{blocks.length}</span>
        <span className="cc-tool-call__status">{statusIcon} {statusLabel}</span>
        <CopyBtn text={blocks.map((b) => toolBlockCopyText(b)).join("\n")} />
      </div>
      {expanded && (
        <div className="cc-tool-call__body">
          {blocks.map((block, i) => {
            const args = block.arguments
              ? formatJsonIfPossible(block.arguments)
              : "";
            const result = block.result
              ? formatJsonIfPossible(block.result)
              : "";
            return (
              <div className="cc-tool-call__sub" key={block.toolCallId || i}>
                <div className="cc-tool-call__sub-head">
                  <span className="cc-tool-call__sub-index">#{i + 1}</span>
                  <span className={`cc-tool-call__peer-status cc-tool-call__peer-status--${block.status}`}>
                    {block.status === "success" ? "✓" : block.status === "error" ? "✗" : "⋯"}
                  </span>
                </div>
                {args && (
                  <div className="cc-tool-call__section">
                    <div className="cc-tool-call__section-label">Input</div>
                    <pre className="cc-tool-call__pre">{args}</pre>
                  </div>
                )}
                {result && (
                  <div className="cc-tool-call__section">
                    <div className="cc-tool-call__section-label">Result</div>
                    <pre className="cc-tool-call__pre">{result}</pre>
                  </div>
                )}
              </div>
            );
          })}
        </div>
      )}
    </section>
  );
}

function PeerToolGroup({ blocks }: { blocks: ConversationRichToolCallBlock[] }) {
  const [expanded, setExpanded] = useState(false);
  const targets = Array.from(new Set(blocks.map((b) => conversationRichPeerTargetForDisplay(b.peerTarget))));
  const allSuccess = blocks.every((b) => b.status === "success");
  const anyError = blocks.some((b) => b.status === "error");
  const statusIcon = anyError ? "✗" : allSuccess ? "✓" : "⋯";
  const statusClass = anyError ? "cc-tool-call--error" : allSuccess ? "cc-tool-call--success" : "cc-tool-call--pending";
  const isIncoming = blocks[0]?.peerIncoming;
  const arrow = isIncoming ? "↙" : "↗";
  const label = isIncoming
    ? `Received from ${targets.join(", ")}`
    : `Sent to ${targets.join(", ")}`;

  return (
    <section className={clsx("cc-tool-call cc-tool-call--peer-group", isIncoming && "cc-tool-call--incoming", statusClass)}>
      <div
        className="cc-tool-call__header"
        role="button"
        tabIndex={0}
        onClick={() => setExpanded((prev) => !prev)}
        onKeyDown={(event) => onToolHeaderKeyDown(event, () => setExpanded((prev) => !prev))}
        aria-expanded={expanded}
      >
        <span className="cc-tool-call__chevron">{expanded ? "▾" : "▸"}</span>
        <span className="cc-tool-call__icon">{arrow}</span>
        <span className="cc-tool-call__name">{label}</span>
        <span className="cc-tool-call__status">{statusIcon}</span>
        <CopyBtn text={blocks.map((b) => toolBlockCopyText(b)).join("\n")} />
      </div>
      {expanded && (
        <div className="cc-tool-call__body">
          {blocks.map((block, i) => {
            const peerBody = conversationRichPeerBodyForDisplay(block.peerBody);
            const peerIntent = conversationRichPeerIntentForDisplay(block.peerIntent, peerBody);
            return (
              <div className="cc-tool-call__peer-row" key={block.toolCallId || i}>
                <span className="cc-tool-call__peer-target">{isIncoming ? "←" : "→"} {conversationRichPeerTargetForDisplay(block.peerTarget)}</span>
                {peerIntent ? (
                  <span className="cc-tool-call__peer-intent">
                    {peerIntent}
                  </span>
                ) : null}
                {peerBody && (
                  <span className="cc-tool-call__peer-body">{peerBody}</span>
                )}
                <span className={`cc-tool-call__peer-status cc-tool-call__peer-status--${block.status}`}>
                  {block.status === "success" ? "✓" : block.status === "error" ? "✗" : "⋯"}
                </span>
              </div>
            );
          })}
        </div>
      )}
    </section>
  );
}

export function ConversationRichContent({
  blocks,
  richStyle = "default",
  Icon,
  displayNormalization = true,
}: ConversationRichContentProps) {
  // Render multi-block tool runs as a single collapsible group:
  // peer tools get the `Sent to a, b, c` blob, generic same-name
  // tool runs get the `<name> ×N` blob. The adapter (and ChatPane's
  // defensive merge) only puts blocks in the same array when they
  // share a `name` and, for peer tools, the same direction — so
  // detection here is just a "are they all tool-call and same-name"
  // check.
  if (blocks.length > 1 && blocks.every((b) => b.type === "tool-call")) {
    const tools = blocks as ConversationRichToolCallBlock[];
    const firstName = tools[0].name;
    if (tools.every((b) => b.name === firstName)) {
      const allPeer = tools.every((b) => PEER_TOOL_NAMES.has(b.name) || b.peerIncoming);
      if (allPeer) {
        return <PeerToolGroup blocks={tools} />;
      }
      return <ToolCallGroup blocks={tools} />;
    }
  }

  const body = blocks
    .map((block, index) => renderBlock(block, index, Icon, displayNormalization))
    .filter((element): element is ReactElement<{ className?: string }> => element !== null);

  if (body.length === 0) {
    return null;
  }

  // Keep every rendered block at the same React depth when a streamed response
  // becomes final. A streaming-only wrapper forces React to replace the first
  // block (usually the live paragraph), while a permanent wrapper breaks host
  // selectors that intentionally target direct rich-message children.
  const renderedBody = richStyle === "streaming"
    ? body.map((element) => cloneElement(element, {
        className: clsx(element.props.className, "cc-rich-streaming"),
      }))
    : body;

  return <>{renderedBody}</>;
}
