const SUMMARY_HEADER_RE = /^(\d+)\s+files?\s+changed(?:\s+\+([\d,]+)\s+-([\d,]+))?$/i;
const SUMMARY_FILE_RE = /^(.+?)\s+\+([\d,]+)\s+-([\d,]+)$/;
const FILE_CHANGE_RE = /^(Created|Updated|Modified|Deleted)\b/i;
const TERMINAL_DURATION_RE = /^Worked for\s+.+$/i;
const TERMINAL_STATUS_RE = /^(Success|Running|Failed|Cancelled)$/i;

export type ConversationTableAlignment = "left" | "center" | "right";

export interface ConversationParsedSummaryFile {
  name: string;
  plus: number;
  minus: number;
}

export interface ConversationParsedSummary {
  title: string;
  plus: number;
  minus: number;
  files: ConversationParsedSummaryFile[];
}

export interface ConversationRichParagraphBlock {
  type: "paragraph";
  text: string;
}

export interface ConversationRichHeadingBlock {
  type: "heading";
  level: number;
  text: string;
}

export interface ConversationRichCodeBlock {
  type: "code";
  language: string;
  body: string;
  highlightedHtml?: string | null;
}

export interface ConversationRichTableBlock {
  type: "table";
  headers: string[];
  alignments: ConversationTableAlignment[];
  rows: string[][];
}

export interface ConversationRichCommandBlock {
  type: "command";
  caption: string;
  title: string;
  body: string;
  output?: string;
  footer?: string;
}

export interface ConversationRichToolCallBlock {
  type: "tool-call";
  toolCallId: string;
  name: string;
  arguments: string;
  result?: string;
  status: "pending" | "success" | "error";
}

export interface ConversationRichFileChangeBlock {
  type: "file-change";
  verb: string;
  before?: string;
  name: string;
  after?: string;
  plus: number;
  minus: number;
}

export interface ConversationRichDividerBlock {
  type: "divider";
  text: string;
}

export interface ConversationRichThinkingBlock {
  type: "thinking";
  label: string;
  text: string;
  final?: boolean;
  persisted?: boolean;
}

export type ConversationRichBlock =
  | ConversationRichParagraphBlock
  | ConversationRichHeadingBlock
  | ConversationRichCodeBlock
  | ConversationRichTableBlock
  | ConversationRichCommandBlock
  | ConversationRichFileChangeBlock
  | ConversationRichDividerBlock
  | ConversationRichThinkingBlock
  | ConversationRichToolCallBlock;

function escapeHtml(value: string): string {
  return String(value || "")
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&#39;");
}

export function renderConversationInlineMarkdown(text: string): string {
  const codeTokens: string[] = [];
  const escaped = escapeHtml(text || "")
    .replace(/`([^`]+)`/g, (_match, code) => {
      const index = codeTokens.push(`<code class="cc-rich-inline-code">${code}</code>`) - 1;
      return `@@CODE_${index}@@`;
    })
    .replace(/\*\*([^*]+)\*\*/g, "<strong>$1</strong>")
    .replace(/\[([^\]]+)\]\(([^)]+)\)/g, '<a href="$2">$1</a>')
    .replace(/\n/g, "<br />");

  return escaped.replace(/@@CODE_(\d+)@@/g, (_match, index) => codeTokens[Number(index)] || "");
}

export function conversationRichBlockHasCopyAction(block: ConversationRichBlock): boolean {
  return block.type === "code" || block.type === "command" || block.type === "file-change";
}

export function conversationRichBlockCopyText(block: ConversationRichBlock): string {
  switch (block.type) {
    case "code":
      return block.body.trim();
    case "command":
      return [block.title, block.body, block.output || "", block.footer || ""].filter(Boolean).join("\n").trim();
    case "file-change":
      return [
        block.verb,
        block.before || "",
        block.name,
        block.after || "",
        `+${block.plus}`,
        `-${block.minus}`,
      ].filter(Boolean).join(" ").replace(/\s+/g, " ").trim();
    case "table":
      return [
        block.headers.join(" | "),
        ...block.rows.map((row) => row.join(" | ")),
      ].join("\n").trim();
    case "heading":
      return block.text.trim();
    case "paragraph":
    case "divider":
      return block.text.trim();
    case "thinking":
      return [block.label, block.text].filter(Boolean).join("\n").trim();
    default:
      return "";
  }
}

export function conversationRichBlocksToText(blocks: ConversationRichBlock[] | null | undefined): string {
  return (blocks || [])
    .map((block) => conversationRichBlockCopyText(block))
    .filter(Boolean)
    .join("\n\n")
    .trim();
}

export function parseConversationSummary(content: string): ConversationParsedSummary | null {
  const lines = String(content || "")
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter(Boolean);

  if (lines.length < 2) {
    return null;
  }

  const headerMatch = lines[0].match(SUMMARY_HEADER_RE);
  if (!headerMatch) {
    return null;
  }

  const files: ConversationParsedSummaryFile[] = [];
  for (const line of lines.slice(1)) {
    const fileMatch = line.match(SUMMARY_FILE_RE);
    if (!fileMatch) {
      break;
    }
    files.push({
      name: fileMatch[1].trim(),
      plus: Number.parseInt(fileMatch[2].replaceAll(",", ""), 10) || 0,
      minus: Number.parseInt(fileMatch[3].replaceAll(",", ""), 10) || 0,
    });
  }

  if (files.length === 0) {
    return null;
  }

  return {
    title: lines[0].replace(/\s+\+[\d,]+\s+-[\d,]+$/u, ""),
    plus: Number.parseInt((headerMatch[2] || "0").replaceAll(",", ""), 10) || files.reduce((sum, file) => sum + file.plus, 0),
    minus: Number.parseInt((headerMatch[3] || "0").replaceAll(",", ""), 10) || files.reduce((sum, file) => sum + file.minus, 0),
    files,
  };
}

export function parseConversationRichBlocks(content: string): ConversationRichBlock[] {
  const source = String(content || "").trim();
  if (!source) {
    return [];
  }

  const blocks: ConversationRichBlock[] = [];
  const fenceRe = /```([^\n`]*)\n([\s\S]*?)```/g;
  let lastIndex = 0;
  let match: RegExpExecArray | null;

  while ((match = fenceRe.exec(source))) {
    const before = source.slice(lastIndex, match.index);
    blocks.push(...parseConversationTextBlocks(before));
    blocks.push({
      type: "code",
      language: (match[1] || "text").trim() || "text",
      body: match[2].replace(/\n+$/u, ""),
    });
    lastIndex = fenceRe.lastIndex;
  }

  blocks.push(...parseConversationTextBlocks(source.slice(lastIndex)));
  return compactConversationBlocks(blocks);
}

function parseConversationTextBlocks(fragment: string): ConversationRichBlock[] {
  const source = String(fragment || "").trim();
  if (!source) {
    return [];
  }

  const sections = source
    .split(/\n{2,}/u)
    .map((section) => section.trim())
    .filter(Boolean);
  const blocks: ConversationRichBlock[] = [];

  for (const section of sections) {
    const heading = parseConversationHeadingBlock(section);
    if (heading) {
      blocks.push(...heading);
      continue;
    }

    const table = parseConversationTableBlock(section);
    if (table) {
      blocks.push(table);
      continue;
    }

    const fileChange = parseConversationFileChangeBlock(section);
    if (fileChange) {
      blocks.push(fileChange);
      continue;
    }

    const command = parseConversationCommandBlock(section);
    if (command) {
      blocks.push(command);
      continue;
    }

    if (TERMINAL_DURATION_RE.test(section)) {
      blocks.push({ type: "divider", text: section });
      continue;
    }

    const normalized = section
      .replace(/^\s*[-*]\s+/gm, "")
      .replace(/\n{2,}/g, "\n")
      .trim();

    if (normalized) {
      blocks.push({ type: "paragraph", text: normalized });
    }
  }

  return blocks;
}

function compactConversationBlocks(blocks: ConversationRichBlock[]): ConversationRichBlock[] {
  const deduped: ConversationRichBlock[] = [];
  for (const block of blocks) {
    const previous = deduped.at(-1);
    if (
      block.type === "paragraph"
      && previous?.type === "file-change"
      && previous.name
      && block.text.startsWith(previous.name)
    ) {
      continue;
    }
    deduped.push(block);
  }
  return deduped;
}

function parseConversationHeadingBlock(section: string): ConversationRichBlock[] | null {
  const lines = String(section || "")
    .split(/\n/u)
    .map((line) => line.trim())
    .filter(Boolean);
  if (!lines.length || !lines[0].startsWith("#")) {
    return null;
  }

  const headingMatch = lines[0].match(/^(#{1,6})\s+(.+)$/u);
  if (!headingMatch) {
    return null;
  }

  const blocks: ConversationRichBlock[] = [{
    type: "heading",
    level: headingMatch[1].length,
    text: headingMatch[2].trim(),
  }];
  const rest = lines.slice(1).join("\n").trim();
  if (rest) {
    blocks.push({ type: "paragraph", text: rest });
  }
  return blocks;
}

function splitMarkdownTableRow(line: string): string[] {
  const source = String(line || "")
    .trim()
    .replace(/^\|/u, "")
    .replace(/\|$/u, "");

  const cells: string[] = [];
  let current = "";
  let escaping = false;
  let codeFenceDepth = 0;

  for (const character of source) {
    if (escaping) {
      current += character;
      escaping = false;
      continue;
    }
    if (character === "\\") {
      escaping = true;
      continue;
    }
    if (character === "`") {
      codeFenceDepth = codeFenceDepth === 0 ? 1 : 0;
      current += character;
      continue;
    }
    if (character === "|" && codeFenceDepth === 0) {
      cells.push(current.trim());
      current = "";
      continue;
    }
    current += character;
  }

  cells.push(current.trim());
  return cells;
}

function parseTableAlignment(cells: string[]): ConversationTableAlignment[] | null {
  if (!cells.length || !cells.every((cell) => /^:?-{3,}:?$/u.test(cell))) {
    return null;
  }

  return cells.map((cell) => {
    const trimmed = cell.trim();
    if (trimmed.startsWith(":") && trimmed.endsWith(":")) {
      return "center";
    }
    if (trimmed.endsWith(":")) {
      return "right";
    }
    return "left";
  });
}

function parseConversationTableBlock(section: string): ConversationRichTableBlock | null {
  const lines = String(section || "")
    .split(/\n/u)
    .map((line) => line.trim())
    .filter(Boolean);

  if (lines.length < 2) {
    return null;
  }

  const headers = splitMarkdownTableRow(lines[0]);
  const alignments = parseTableAlignment(splitMarkdownTableRow(lines[1]));
  if (!headers.length || !alignments || headers.length !== alignments.length) {
    return null;
  }

  const rows = lines
    .slice(2)
    .map((line) => splitMarkdownTableRow(line))
    .filter((cells) => cells.length > 0 && cells.some((cell) => cell.length > 0))
    .map((cells) => headers.map((_header, index) => cells[index] || ""));

  return {
    type: "table",
    headers,
    alignments,
    rows,
  };
}

function parseConversationFileChangeBlock(section: string): ConversationRichFileChangeBlock | null {
  const compact = String(section || "").replace(/\s*\n\s*/g, " ").trim();
  if (!compact) {
    return null;
  }

  const header = compact.match(FILE_CHANGE_RE);
  if (!header) {
    return null;
  }

  const verb = header[1];
  const statsMatch = compact.match(/\s+\+([\d,]+)\s+-([\d,]+)\s*$/u);
  const plus = Number.parseInt((statsMatch?.[1] || "1").replaceAll(",", ""), 10) || 0;
  const minus = Number.parseInt((statsMatch?.[2] || "0").replaceAll(",", ""), 10) || 0;
  const body = statsMatch ? compact.slice(0, statsMatch.index).trim() : compact;
  const fileMatches = [...body.matchAll(/`([^`]+)`/gu)];
  const fileMatch = fileMatches.find((candidate) => !candidate[1].includes("/")) || fileMatches[0];
  if (!fileMatch) {
    return null;
  }

  const fileToken = fileMatch[0];
  const fileName = fileMatch[1].trim();
  const bodyAfterVerb = body.slice(verb.length).trim();
  const tokenIndex = bodyAfterVerb.indexOf(fileToken);
  const before = tokenIndex >= 0 ? bodyAfterVerb.slice(0, tokenIndex).trim() : "";
  const after = tokenIndex >= 0
    ? bodyAfterVerb.slice(tokenIndex + fileToken.length).trim()
    : bodyAfterVerb.replace(fileToken, "").trim();

  return {
    type: "file-change",
    verb,
    before,
    name: fileName,
    after,
    plus,
    minus,
  };
}

function parseConversationCommandBlock(section: string): ConversationRichCommandBlock | null {
  const lines = String(section || "")
    .split(/\n/u)
    .map((line) => line.replace(/\s+$/u, ""))
    .filter((line) => line.trim().length > 0);
  if (!lines.length) {
    return null;
  }

  const commandIndex = lines.findIndex((line) => line.trim().startsWith("$ "));
  if (commandIndex === -1) {
    return null;
  }

  const command = lines[commandIndex].trim();
  const prefix = lines.slice(0, commandIndex).filter(Boolean);
  const footerCandidate = lines.at(-1)?.trim() || "";
  const footer = TERMINAL_STATUS_RE.test(footerCandidate) ? footerCandidate : "";
  const outputStart = commandIndex + 1;
  const outputEnd = footer ? lines.length - 1 : lines.length;
  const output = lines.slice(outputStart, outputEnd).join("\n").trim();

  return {
    type: "command",
    caption: prefix[0] || "Ran command",
    title: prefix[1] || "Shell",
    body: command,
    output,
    footer,
  };
}
