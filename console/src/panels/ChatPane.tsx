import React from "react";
import type {
  ConversationTimelineEntry,
  ConversationRichBlock,
  ConversationCouncilEntry,
  ConversationWorkGraphEntry,
} from "@console-core";
import {
  conversationEntryText,
  conversationRichBlocksToText,
} from "@console-core";
import {
  ConversationRichContent,
  CouncilCard,
  WorkGraphCard,
  copyTextToClipboard,
  type WorkGraphCardActions,
} from "@console-components";
import type { ConsoleAgent } from "../types";
import { VoiceButton } from "./VoiceBar";
import {
  composerImageFileKey,
  consoleBlobReferencesFromText,
  consoleBlobUrlsFromText,
  dedupeComposerImageFiles,
  selectImageTransferFiles,
  stripConsoleBlobReferencesFromText,
} from "../lib/composer-attachment-text";
import { countRender } from "../lib/render-counts";

interface ChatPaneProps {
  agent: ConsoleAgent | null;
  agentLabel: string;
  identity: string;
  entries: ConversationTimelineEntry[];
  phase: "waiting" | "tool-executing" | "generating" | null;
  draft: string;
  sending: boolean;
  readOnly?: boolean;
  accessEnforcing?: boolean;
  staged: StagedAttachment[];
  onDraftChange: (value: string) => void;
  onStagedChange: React.Dispatch<React.SetStateAction<StagedAttachment[]>>;
  /// `text` is the composer's live value at submit time. The pane owns the
  /// live draft; `draft`/`onDraftChange` only carry the persisted copy used
  /// when a panel is switched or reopened.
  onSend: (attachments: File[], text: string) => boolean | Promise<boolean>;
  onInspect?: () => void;
  onRespawn?: () => void;
  onRetire?: () => void;
  inspectLabel?: string;
  respawnLabel?: string;
  retireLabel?: string;
  sendLabel?: string;
  hasOlderHistory?: boolean;
  loadingOlderHistory?: boolean;
  isLoadingHistory?: boolean;
  onLoadOlder?: () => void;
  /// Pending-message stack rendered between conversation body and
  /// composer. ConsoleApp owns the state + handlers; ChatPane just
  /// reserves the slot. Pass `null` (or omit) to suppress.
  stackSlot?: React.ReactNode;
  voiceSlot?: React.ReactNode;
  onVoiceToggle?: () => void;
  voiceActive?: boolean;
  voiceDisabled?: boolean;
  /// Operator actions for inline WorkGraph cards. ConsoleApp gates these on
  /// `experience.workgraph.can_manage` and read-only state; omitted callbacks
  /// render no buttons.
  workGraphActions?: WorkGraphCardActions | null;
}

export interface StagedAttachment {
  id: string;
  file: File;
  previewUrl: string;
}

const ALLOWED_IMAGE_TYPES = new Set(["image/png", "image/jpeg", "image/webp", "image/gif"]);
const MAX_ATTACHMENTS = 4;
const MAX_ATTACHMENT_BYTES = 25 * 1024 * 1024;

type MsgKind = "origin" | "user" | "agent" | "tool" | "thought" | "gate" | "workgraph" | "council";

interface Msg {
  id: string;
  kind: MsgKind;
  time: string;
  createdAt?: string;
  who?: string;
  text?: string;
  blocks?: ConversationRichBlock[];
  workGraphEntry?: ConversationWorkGraphEntry;
  councilEntry?: ConversationCouncilEntry;
  workedFor?: string;
  workedForCopyText?: string;
}

interface ChatTurn {
  id: string;
  messages: Msg[];
}

interface ChatTurnPreview {
  title: string;
  body: string;
}

function phaseLabel(_phase: "waiting" | "tool-executing" | "generating"): string {
  // Single label across all phases. The phase distinction is still
  // surfaced in the composer footer chip; the inline typing indicator
  // just signals "this agent is currently working".
  return "working";
}

function formatTime(iso?: string): string {
  if (!iso) return "";
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return "";
  const hh = String(d.getHours()).padStart(2, "0");
  const mm = String(d.getMinutes()).padStart(2, "0");
  const ss = String(d.getSeconds()).padStart(2, "0");
  return `${hh}:${mm}:${ss}`;
}

function parseTimeMs(iso?: string): number | null {
  if (!iso) return null;
  const ms = Date.parse(iso);
  return Number.isFinite(ms) ? ms : null;
}

function formatWorkedDuration(ms: number): string {
  const totalSeconds = Math.max(0, Math.round(ms / 1000));
  if (totalSeconds < 1) return "under 1s";
  if (totalSeconds < 60) return `${totalSeconds}s`;
  const totalMinutes = Math.floor(totalSeconds / 60);
  const seconds = totalSeconds % 60;
  if (totalMinutes < 60) {
    return seconds ? `${totalMinutes}m ${seconds}s` : `${totalMinutes}m`;
  }
  const hours = Math.floor(totalMinutes / 60);
  const minutes = totalMinutes % 60;
  return minutes ? `${hours}h ${minutes}m` : `${hours}h`;
}

function msgCopyText(message: Msg): string {
  if (message.text) return message.text.trim();
  return conversationRichBlocksToText(message.blocks).trim();
}

function msgHasTextualPayload(message: Msg): boolean {
  if (message.text?.trim()) return true;
  return Boolean(message.blocks?.some((block) => (
    block.type === "paragraph"
    || block.type === "heading"
    || block.type === "divider"
    || block.type === "code"
    || block.type === "command"
  )));
}

function buildChatTurns(messages: Msg[]): ChatTurn[] {
  const turns: ChatTurn[] = [];
  for (const message of messages) {
    const current = turns.at(-1);
    if (!current || message.kind === "user") {
      turns.push({
        id: `turn-${message.id}`,
        messages: [message],
      });
      continue;
    }
    current.messages.push(message);
  }
  return turns;
}

/** Turn-rail geometry: fixed per-tick footprint (item height + grid gap). */
export const TURN_RAIL_TICK_PX = 10;
/** Hard ceiling on rendered rail ticks, independent of viewport height. */
export const TURN_RAIL_MAX_TICKS = 48;

/// Transcript windowing. Only the newest `TRANSCRIPT_WINDOW_TURNS` turns
/// are mounted; scrolling to the top (or jumping to an older turn from the
/// rail) reveals `TRANSCRIPT_WINDOW_STEP` more, anchored so the content
/// under the viewport does not move. Rows are variable-height markdown
/// with code blocks, images and cards, and the pane relies on real DOM
/// geometry for bottom-anchoring, the older-history anchor and the turn
/// rail, so a tail window is used instead of a measured virtual list.
export const TRANSCRIPT_WINDOW_TURNS = 120;
export const TRANSCRIPT_WINDOW_STEP = 120;

/// Where the reader revealed to: the first mounted turn's id, and how many
/// turns were mounted at that moment. The id survives older-history
/// prepends (indexes shift, ids do not); the count is the fallback when the
/// anchored turn is gone (the per-identity log trimmed past it), so the
/// window keeps its size at the oldest retained turns instead of snapping
/// back to the tail.
export interface TranscriptWindowAnchor {
  turnId: string;
  mountedTurns: number;
}

/// First rendered turn index for `turnCount` turns given the reveal state.
/// `null` means the user has not scrolled into history: render the tail
/// window. A `""` turn id means everything is revealed (the window reached
/// the first turn, so server-side older history also shows as it loads).
export function transcriptWindowStart(
  turnCount: number,
  anchor: TranscriptWindowAnchor | null,
  indexOfTurn: (id: string) => number,
): number {
  if (anchor === null) return Math.max(0, turnCount - TRANSCRIPT_WINDOW_TURNS);
  if (anchor.turnId === "") return 0;
  const index = indexOfTurn(anchor.turnId);
  if (index >= 0) return index;
  return Math.max(0, turnCount - Math.max(anchor.mountedTurns, TRANSCRIPT_WINDOW_TURNS));
}

/**
 * Window the turn rail to the measured band (issue: long-running agents grew
 * a tick per turn and the ladder spilled past the pane — the list cannot
 * clip via overflow because the hover previews render outside it). The
 * newest turns keep individual ticks; everything older collapses into one
 * "earlier turns" jump slot. Returns the first railed turn index and the
 * number of railed turns; `overflow` is how many older turns collapsed.
 */
export function windowTurnRail(
  turnCount: number,
  railHeightPx: number | null,
): { start: number; overflow: number } {
  if (turnCount <= 1) return { start: 0, overflow: 0 };
  const byHeight =
    railHeightPx === null || !Number.isFinite(railHeightPx) || railHeightPx <= 0
      ? TURN_RAIL_MAX_TICKS
      : Math.floor(railHeightPx / TURN_RAIL_TICK_PX);
  // Keep at least a handful of ticks so tiny panes still navigate recents.
  const budget = Math.max(6, Math.min(TURN_RAIL_MAX_TICKS, byHeight));
  if (turnCount <= budget) return { start: 0, overflow: 0 };
  // Reserve one slot for the overflow jump tick.
  const visible = Math.max(5, budget - 1);
  return { start: turnCount - visible, overflow: turnCount - visible };
}

function chatTurnPreview(turn: ChatTurn): ChatTurnPreview {
  let title = "";
  let body = "";

  for (const message of turn.messages) {
    const text = msgCopyText(message);
    if (!text) {
      continue;
    }
    if (!title && message.kind === "user") {
      title = text;
      continue;
    }
    if (!body && message.kind !== "user") {
      body = text;
    }
  }

  if (!title) {
    title = msgCopyText(turn.messages[0]) || "Turn";
  }
  if (!body) {
    body = "No response yet.";
  }

  return { title, body };
}

function isScaffoldUserText(text: string): boolean {
  const normalized = text.trimStart();
  return /^you have been spawned\b/i.test(normalized)
    || /^\[peer update\]/i.test(normalized);
}

function isScaffoldUserMessage(message: Msg): boolean {
  return message.kind === "user" && isScaffoldUserText(msgCopyText(message));
}

function transcriptCopyText(messages: Msg[]): string {
  return messages
    .map((message) => {
      const text = msgCopyText(message);
      if (!text) return "";
      const label = message.kind === "user"
        ? "You"
        : message.kind === "agent"
          ? message.who || "Agent"
          : message.kind.toUpperCase();
      const time = message.time ? `[${message.time}] ` : "";
      const worked = message.workedFor ? `\nWorked for ${message.workedFor}` : "";
      return `${time}${label}: ${text}${worked}`.trim();
    })
    .filter(Boolean)
    .join("\n\n");
}

/// Classify a single rich block into the row "kind" used by the
/// surrounding bubble layout. `tool-call` and `thinking` get their
/// own visual lane; everything else rides the agent/user lane.
function richBlockKind(block: ConversationRichBlock, isUser: boolean): MsgKind {
  if (block.type === "tool-call") return "tool";
  if (block.type === "thinking") return "thought";
  return isUser ? "user" : "agent";
}

/// Group consecutive rich blocks of the same kind so peer-comms tool
/// calls (`send_request` to multiple peers) and consecutive
/// `tool-call` blocks render as one collapsible group via
/// `ConversationRichContent`'s `PeerToolGroup` / per-block render.
function flattenEntry(entry: ConversationTimelineEntry): Msg[] {
  if (entry.kind === "summary") {
    return [{
      id: entry.id,
      kind: "origin",
      time: formatTime(entry.createdAt),
      createdAt: entry.createdAt,
      text: `${entry.title} (+${entry.plus}/-${entry.minus})`,
    }];
  }

  if (entry.kind === "council") {
    return [{
      id: entry.id,
      kind: "council",
      time: formatTime(entry.createdAt),
      createdAt: entry.createdAt,
      // Copy/transcript surfaces read `text`; rendering goes through the card.
      text: conversationEntryText(entry),
      councilEntry: entry,
    }];
  }

  if (entry.kind === "workgraph") {
    return [{
      id: entry.id,
      kind: "workgraph",
      time: formatTime(entry.createdAt),
      createdAt: entry.createdAt,
      // Copy/transcript surfaces read `text`; rendering goes through the card.
      text: conversationEntryText(entry),
      workGraphEntry: entry,
    }];
  }

  if (entry.variant === "meta") {
    return [{
      id: entry.id,
      kind: "origin",
      time: formatTime(entry.createdAt),
      createdAt: entry.createdAt,
      text: entry.text || "",
    }];
  }

  const role = entry.identity.role;
  const isUser = role === "user";
  const label = entry.identity.label;
  const time = formatTime(entry.createdAt);

  if (entry.variant === "rich" && Array.isArray(entry.blocks) && entry.blocks.length > 0) {
    // Group consecutive blocks of the same kind so the peer-comms
    // "↗ Sent to a, b, c" collapsible blob keeps its grouping
    // (previously gutted by the Rams visual refresh — peer/tool
    // blocks were flattened to one-line strings per call).
    const msgs: Msg[] = [];
    let groupKind: MsgKind | null = null;
    let groupBlocks: ConversationRichBlock[] = [];
    let groupStart = 0;
    const flushGroup = (endIndex: number) => {
      if (groupKind === null || groupBlocks.length === 0) return;
      msgs.push({
        id: `${entry.id}:${groupStart}-${endIndex - 1}`,
        kind: groupKind,
        time,
        createdAt: entry.createdAt,
        who: groupKind === "agent" ? label : undefined,
        blocks: groupBlocks,
      });
      groupKind = null;
      groupBlocks = [];
    };
    for (let i = 0; i < entry.blocks.length; i++) {
      const block = entry.blocks[i];
      const kind = richBlockKind(block, isUser);
      if (kind !== groupKind) {
        flushGroup(i);
        groupKind = kind;
        groupStart = i;
      }
      groupBlocks.push(block);
    }
    flushGroup(entry.blocks.length);
    return msgs.length
      ? msgs
      : [{
          id: entry.id,
          kind: isUser ? "user" : "agent",
          time,
          createdAt: entry.createdAt,
          who: isUser ? undefined : label,
          text: "",
        }];
  }

  return [{
    id: entry.id,
    kind: isUser ? "user" : "agent",
    time,
    createdAt: entry.createdAt,
    who: isUser ? undefined : label,
    text: entry.text || "",
  }];
}

function textSignatureForMsg(message: Msg): string {
  if (message.text) {
    return message.text.replace(/\s+/g, " ").trim();
  }
  if (!Array.isArray(message.blocks) || message.blocks.length === 0) {
    return "";
  }
  const parts = message.blocks.map((block) => {
    if (block.type === "paragraph") return block.text || "";
    if (block.type === "heading") return block.text || "";
    if (block.type === "divider") return block.text || "";
    return "";
  });
  if (parts.some((part) => part.trim().length === 0)) {
    return "";
  }
  return parts.join("\n").replace(/\s+/g, " ").trim();
}

function buildChatMessages(entries: ConversationTimelineEntry[]): Msg[] {
  // Defensive cross-entry merge: the adapter already groups
  // consecutive same-name tool calls into one entry, but the
  // merge breaks if a non-tool entry slips between adjacent tool
  // entries (e.g., a meta event the adapter rendered as its own
  // bubble). Walk the flattened message list and fold neighbouring
  // tool messages whose blocks all share the same tool `name` —
  // and, for peer tools, the same direction.
  const flat = entries.flatMap(flattenEntry);
  const merged: Msg[] = [];
  for (const m of flat) {
    const last = merged[merged.length - 1];
    const lastBlocks = last?.blocks;
    const mBlocks = m.blocks;
    const sameName = !!(
      last
      && last.kind === "tool"
      && m.kind === "tool"
      && Array.isArray(lastBlocks) && lastBlocks.length > 0
      && Array.isArray(mBlocks) && mBlocks.length > 0
      && lastBlocks.every((b) => b.type === "tool-call")
      && mBlocks.every((b) => b.type === "tool-call")
      && lastBlocks[0].type === "tool-call"
      && mBlocks[0].type === "tool-call"
      && lastBlocks.every((b) => b.type === "tool-call" && b.name === mBlocks[0].name)
      && mBlocks.every((b) => b.type === "tool-call" && b.name === mBlocks[0].name)
    );
    const peerCompatible = !sameName
      ? false
      : !((mBlocks![0] as { peerTarget?: unknown }).peerTarget)
        ? true
        : Boolean((lastBlocks![0] as { peerIncoming?: unknown }).peerIncoming)
          === Boolean((mBlocks![0] as { peerIncoming?: unknown }).peerIncoming);
    if (sameName && peerCompatible && last && lastBlocks && mBlocks) {
      last.blocks = [...lastBlocks, ...mBlocks];
      last.id = `${last.id}+${m.id}`;
    } else {
      const canDedupeAdjacent =
        (m.kind === "user" && last?.kind === "user")
        || (m.kind === "agent" && last?.kind === "agent" && last.who === m.who);
      if (last && canDedupeAdjacent) {
        const lastSignature = textSignatureForMsg(last);
        const nextSignature = textSignatureForMsg(m);
        if (lastSignature && lastSignature === nextSignature) {
          continue;
        }
      }
      merged.push({ ...m });
    }
  }
  let pendingUserStartedAt: number | null = null;
  return merged.map((message) => {
    if (message.kind === "user") {
      pendingUserStartedAt = isScaffoldUserMessage(message)
        ? null
        : parseTimeMs(message.createdAt);
      return message;
    }
    if (message.kind !== "agent" || !msgHasTextualPayload(message)) {
      return message;
    }
    const finishedAt = parseTimeMs(message.createdAt);
    if (pendingUserStartedAt === null || finishedAt === null || finishedAt < pendingUserStartedAt) {
      return message;
    }
    const workedFor = formatWorkedDuration(finishedAt - pendingUserStartedAt);
    pendingUserStartedAt = null;
    return {
      ...message,
      workedFor,
      workedForCopyText: `Worked for ${workedFor}`,
    };
  });
}

export const __chatPaneTest = {
  buildChatMessages,
  buildChatTurns,
  chatTurnPreview,
  isScaffoldUserText,
};

interface ImageTransferPayload {
  files: File[];
  textPayloads: string[];
}

function collectImageTransferPayload(data: DataTransfer): ImageTransferPayload {
  const directFiles = Array.from(data.files).filter((file) => file.type.startsWith("image/"));
  const itemFiles = Array.from(data.items)
    .filter((item) => item.kind === "file" && item.type.startsWith("image/"))
    .map((item) => item.getAsFile())
    .filter((file): file is File => Boolean(file));
  const textPayloads = [
    data.getData("text/html"),
    data.getData("text/uri-list"),
    data.getData("text/plain"),
  ].filter(Boolean);
  return { files: selectImageTransferFiles(directFiles, itemFiles), textPayloads };
}

function imageTransferPayloadHasImage(payload: ImageTransferPayload): boolean {
  return payload.files.length > 0
    || payload.textPayloads.some((text) => (
      imageDataUrlsFromText(text).length > 0 || consoleBlobUrlsFromText(text).length > 0
    ));
}

async function imageFilesFromTransferPayload(payload: ImageTransferPayload): Promise<File[]> {
  if (payload.files.length > 0) {
    return payload.files;
  }
  const files: File[] = [];
  const seen = new Set<string>();
  for (const text of payload.textPayloads) {
    for (const dataUrl of imageDataUrlsFromText(text)) {
      if (seen.has(dataUrl)) continue;
      seen.add(dataUrl);
      const file = fileFromImageDataUrl(dataUrl);
      if (file) files.push(file);
    }
    for (const blobUrl of consoleBlobUrlsFromText(text)) {
      if (seen.has(blobUrl)) continue;
      seen.add(blobUrl);
      const file = await fileFromConsoleBlobUrl(blobUrl);
      if (file) files.push(file);
    }
  }
  return files;
}

function imageDataUrlsFromText(value: string): string[] {
  const matches = value.match(/data:image\/(?:png|jpeg|webp|gif);base64,[A-Za-z0-9+/=]+/gi);
  return matches ?? [];
}

function fileFromImageDataUrl(dataUrl: string): File | null {
  const match = dataUrl.match(/^data:(image\/(?:png|jpeg|webp|gif));base64,([A-Za-z0-9+/=]+)$/i);
  if (!match) return null;
  const [, mediaType, base64] = match;
  try {
    const binary = atob(base64);
    const bytes = new Uint8Array(binary.length);
    for (let i = 0; i < binary.length; i++) {
      bytes[i] = binary.charCodeAt(i);
    }
    const ext = mediaType.split("/")[1]?.replace("jpeg", "jpg") || "png";
    return new File([bytes], `pasted-image.${ext}`, { type: mediaType });
  } catch {
    return null;
  }
}

async function fileFromConsoleBlobUrl(url: string): Promise<File | null> {
  try {
    const response = await fetch(url, { credentials: "same-origin" });
    if (!response.ok) return null;
    const mediaType = response.headers.get("content-type")?.split(";")[0]?.trim() || "";
    if (!ALLOWED_IMAGE_TYPES.has(mediaType)) return null;
    const blob = await response.blob();
    const ext = mediaType.split("/")[1]?.replace("jpeg", "jpg") || "png";
    const slug = decodeURIComponent(new URL(url).pathname.split("/").pop() || "blob")
      .replace(/[^A-Za-z0-9._-]/g, "-")
      .slice(0, 80) || "blob";
    return new File([blob], `${slug}.${ext}`, { type: mediaType });
  } catch {
    return null;
  }
}

function CopyInlineButton({
  text,
  getText,
  label,
  className = "",
}: {
  /// Static text, or `getText` for text that is expensive to build (the whole
  /// transcript) and only needed when the user actually clicks.
  text?: string;
  getText?: () => string;
  label: string;
  className?: string;
}) {
  const [outcome, setOutcome] = React.useState<"idle" | "copied" | "failed">("idle");
  // Cleared on unmount: an uncleared reset timer fires into a torn-down tree,
  // and React touches `window` before it notices the update is a no-op.
  const resetTimer = React.useRef<ReturnType<typeof setTimeout> | null>(null);
  React.useEffect(
    () => () => {
      if (resetTimer.current) clearTimeout(resetTimer.current);
    },
    [],
  );
  // A lazy source is assumed non-empty; the click resolves it.
  const disabled = getText ? false : !(text ?? "").trim();

  // NOT `navigator.clipboard` directly. That API exists only in a SECURE
  // CONTEXT - https, or localhost - and the console is routinely reached over
  // plain http on a LAN address, where it is `undefined` and
  // `navigator.clipboard.writeText(...)` throws before touching the clipboard.
  // The throw landed in a catch that swallowed it to protect the hover
  // affordance, so the button did nothing, silently, for every LAN user.
  //
  // `copyTextToClipboard` is the shared owner: the async API when genuinely
  // available, a `document.execCommand` fallback otherwise, and it returns
  // whether it worked so this button can be honest about the outcome.
  async function copy() {
    if (disabled) return;
    const value = getText ? getText() : text ?? "";
    if (!value.trim()) return;
    const ok = await copyTextToClipboard(value);
    setOutcome(ok ? "copied" : "failed");
    if (resetTimer.current) clearTimeout(resetTimer.current);
    resetTimer.current = setTimeout(() => setOutcome("idle"), 1400);
  }

  const title = outcome === "copied" ? "Copied" : outcome === "failed" ? "Copy failed" : label;

  return (
    <button
      aria-label={title}
      className={`msg__copy ${className}`}
      data-copied={outcome === "copied" ? "true" : undefined}
      data-copy-outcome={outcome === "idle" ? undefined : outcome}
      disabled={disabled}
      onClick={(event) => {
        event.stopPropagation();
        void copy();
      }}
      title={title}
      type="button"
    >
      {outcome === "copied" ? "✓" : outcome === "failed" ? "✗" : "⎘"}
    </button>
  );
}

/// Msg objects are rebuilt from scratch on every transcript derivation, so a
/// reference check alone would re-render every mounted row per SSE burst.
/// Rows are compared by a signature over the fields that change when the
/// rendered row changes (ids, statuses, counts, text lengths plus a sampled
/// text hash), which is O(fields) per row rather than a full serialisation,
/// so a long revealed window stays cheap per flush. The signature is cached
/// per Msg object; a Msg is immutable once built.
const msgSignatures = new WeakMap<Msg, string>();

function textMark(value: string | undefined | null): string {
  if (!value) return "0";
  // Length plus 16 evenly sampled character codes: constant cost, and any
  // in-place edit that keeps the length still moves a sample in practice.
  let hash = value.length;
  const step = Math.max(1, Math.floor(value.length / 16));
  for (let i = 0; i < value.length; i += step) {
    hash = (hash * 31 + value.charCodeAt(i)) | 0;
  }
  return `${value.length}.${hash}`;
}

function blockSignature(block: ConversationRichBlock): string {
  switch (block.type) {
    case "paragraph":
      return `p${textMark(block.text)}`;
    case "heading":
      return `h${block.level}${textMark(block.text)}`;
    case "code":
      return `c${block.language}:${textMark(block.body)}`;
    case "table":
      return `t${block.headers.length}x${block.rows.length}`;
    case "command":
      return `m${textMark(block.title)}:${textMark(block.body)}:${textMark(block.output)}:${textMark(block.footer)}`;
    case "tool-call":
      return `tc${block.toolCallId}:${block.name}:${block.status}:${textMark(block.arguments)}:${textMark(block.result)}:${textMark(block.peerBody)}:${block.peerImages?.length ?? 0}`;
    case "file-change":
      return `f${block.verb}:${block.name}:${block.plus}:${block.minus}`;
    case "divider":
      return `d${textMark(block.text)}`;
    case "thinking":
      return `k${block.final ? 1 : 0}${block.persisted ? 1 : 0}:${textMark(block.text)}`;
    case "image":
      return `i${block.src}:${block.width ?? 0}x${block.height ?? 0}`;
    default:
      return JSON.stringify(block);
  }
}

function msgSignature(message: Msg): string {
  let signature = msgSignatures.get(message);
  if (signature !== undefined) return signature;
  const parts = [
    message.id,
    message.kind,
    message.time,
    message.who ?? "",
    textMark(message.text),
    message.workedFor ?? "",
    textMark(message.workedForCopyText),
  ];
  if (message.blocks) parts.push(message.blocks.map(blockSignature).join(","));
  const wg = message.workGraphEntry;
  if (wg) {
    parts.push(
      `wg${wg.id}:${wg.status}:${wg.progress.completed}/${wg.progress.total}:${wg.itemOverflowCount ?? 0}:${wg.recentEvents?.length ?? 0}`,
      wg.items.map((item) => `${item.itemId}:${item.status}:${item.revision ?? 0}:${item.priority ?? ""}:${item.ownerLabel ?? ""}`).join(","),
      wg.attention.map((row) => `${row.bindingId}:${row.mode}:${row.statusLabel}:${row.revision ?? 0}`).join(","),
    );
  }
  const council = message.councilEntry;
  if (council) {
    parts.push(
      `cc${council.id}:${council.status}:${council.exitReason}:${council.roundsCompleted}:${council.participants.length}`,
      council.exchanges.map((row) => `${row.round}.${row.sequence}:${row.status}:${textMark(row.text)}`).join(","),
    );
  }
  signature = parts.join("|");
  msgSignatures.set(message, signature);
  return signature;
}

type MessageRowProps = {
  message: Msg;
  suppressWorked: boolean;
  workGraphActions: WorkGraphCardActions | null;
};

function messageRowPropsEqual(prev: MessageRowProps, next: MessageRowProps): boolean {
  return (
    prev.suppressWorked === next.suppressWorked &&
    prev.workGraphActions === next.workGraphActions &&
    (prev.message === next.message || msgSignature(prev.message) === msgSignature(next.message))
  );
}

const MessageRow = React.memo(function MessageRow({
  message: m,
  suppressWorked,
  workGraphActions,
}: MessageRowProps) {
  countRender("MessageRow");
  return (
    <div className={`msg msg--${m.kind}`}>
      <div className="msg__time">{m.time}</div>
      <div className="msg__bubble">
        {(m.kind === "user" || m.kind === "agent") && (
          <CopyInlineButton label={`Copy ${m.kind === "user" ? "message" : "turn"}`} text={msgCopyText(m)} />
        )}
        {m.kind === "council" && m.councilEntry ? (
          // No actions prop: council participants are destroyed
          // before the tool returns, so the card is observational
          // by construction.
          <CouncilCard entry={m.councilEntry} />
        ) : null}
        {m.kind === "workgraph" && m.workGraphEntry ? (
          <WorkGraphCard entry={m.workGraphEntry} actions={workGraphActions} />
        ) : m.blocks && m.blocks.length > 0 ? (
          <ConversationRichContent blocks={m.blocks} displayNormalization={false} />
        ) : (
          m.text && <span className="msg__text">{m.text}</span>
        )}
        {m.workedFor && !suppressWorked && (
          <div className="msg__worked">
            <span>Worked for {m.workedFor}</span>
            <CopyInlineButton
              className="msg__copy--inline"
              label="Copy work time"
              text={m.workedForCopyText || `Worked for ${m.workedFor}`}
            />
          </div>
        )}
      </div>
    </div>
  );
}, messageRowPropsEqual);

/// The scrolling transcript. Memoised so composer keystrokes, which re-render
/// the owning ChatPane, never touch a transcript row: only a change in the
/// turns, phase, history state, or the stable handlers reaches it.
const TranscriptView = React.memo(function TranscriptView({
  identity,
  agentLabel,
  turns,
  messages,
  phase,
  lastAgentMessageId,
  workGraphActions,
  isLoadingHistory,
  hasOlderHistory,
  loadingOlderHistory,
  windowStart,
  onRevealEarlier,
  bodyRef,
  onScroll,
  onRequestOlderHistory,
}: {
  identity: string;
  agentLabel: string;
  turns: ChatTurn[];
  messages: Msg[];
  phase: ChatPaneProps["phase"];
  lastAgentMessageId: string | null;
  workGraphActions: WorkGraphCardActions | null;
  isLoadingHistory: boolean;
  hasOlderHistory: boolean;
  loadingOlderHistory: boolean;
  windowStart: number;
  onRevealEarlier: () => void;
  bodyRef: React.RefObject<HTMLDivElement | null>;
  onScroll: React.UIEventHandler<HTMLDivElement>;
  onRequestOlderHistory: () => void;
}) {
  countRender("TranscriptView");
  const windowedTurns = React.useMemo(
    () => (windowStart > 0 ? turns.slice(windowStart) : turns),
    [turns, windowStart],
  );
  // Serialised on click only: the whole transcript as text is the single most
  // expensive derivation in this pane and nobody reads it until they copy.
  const getTranscriptText = React.useCallback(() => transcriptCopyText(messages), [messages]);
  return (
    <div className="conv__body" onScroll={onScroll} ref={bodyRef}>
      <CopyInlineButton
        className="msg__copy--transcript"
        label="Copy transcript"
        getText={getTranscriptText}
      />
      {windowStart > 0 ? (
        <button
          className="conv__history"
          data-testid={`chat-reveal-earlier:${identity}`}
          onClick={onRevealEarlier}
          type="button"
        >
          Show earlier messages
        </button>
      ) : hasOlderHistory && (
        <button
          className="conv__history"
          disabled={loadingOlderHistory}
          onClick={onRequestOlderHistory}
          type="button"
        >
          {loadingOlderHistory ? "Loading history" : "Load older history"}
        </button>
      )}
      {messages.length === 0 && isLoadingHistory && (
        <div
          className="msg msg--origin"
          data-testid={`chat-loading-history:${identity}`}
          aria-live="polite"
          aria-busy="true"
        >
          <div className="msg__time" />
          <div className="msg__bubble">
            <span className="msg__typing">
              <span className="msg__typing-dots" aria-hidden="true">
                <span /><span /><span />
              </span>
              <span className="msg__typing-label">Loading conversation…</span>
            </span>
          </div>
        </div>
      )}
      {messages.length === 0 && !isLoadingHistory && (
        <div className="msg msg--origin">
          <div className="msg__time" />
          <div className="msg__bubble"><span className="msg__text">No messages yet. Say hello to {agentLabel}.</span></div>
        </div>
      )}
      {windowedTurns.map((turn, offset) => {
        const turnIndex = windowStart + offset;
        return (
        <div
          aria-label={`Turn ${turnIndex + 1}`}
          className="conv-turn"
          data-chat-turn-index={turnIndex}
          data-testid={`chat-turn:${identity}:${turnIndex}`}
          key={turn.id}
        >
          {turn.messages.map((m) => (
            <MessageRow
              key={m.id}
              message={m}
              suppressWorked={Boolean(phase && m.id === lastAgentMessageId)}
              workGraphActions={workGraphActions}
            />
          ))}
        </div>
        );
      })}
      {phase && (
        <div
          className="msg msg--typing"
          data-testid={`chat-typing:${identity}`}
          aria-live="polite"
          aria-label={`${agentLabel} is ${phaseLabel(phase)}`}
        >
          <div className="msg__time" />
          <div className="msg__bubble">
            <span className="msg__typing">
              <span className="msg__typing-dots" aria-hidden="true">
                <span /><span /><span />
              </span>
              <span className="msg__typing-label">{phaseLabel(phase)}</span>
            </span>
          </div>
        </div>
      )}
    </div>
  );
});

/// The composer's text input and send row. Owns the live textarea value so
/// each keystroke re-renders only this small component; ChatPane learns the
/// value through `onLiveChange` (kept in a ref there) and can push a new
/// value down through `externalValue` (send cleared it, a blob reference
/// was stripped, or the persisted draft changed on panel switch).
const ComposerTextarea = React.memo(function ComposerTextarea({
  identity,
  agentLabel,
  agentRole,
  initialValue,
  externalValue,
  readOnly,
  sendWithheld,
  voiceActive,
  voiceDisabled,
  onVoiceToggle,
  stagedCount,
  canAttachImages,
  sending,
  sendLabel,
  onLiveChange,
  onBlur,
  onSubmit,
}: {
  identity: string;
  agentLabel: string;
  agentRole: string | null;
  initialValue: string;
  externalValue: { value: string; at: number } | null;
  readOnly: boolean;
  sendWithheld: boolean;
  voiceActive: boolean;
  voiceDisabled: boolean;
  onVoiceToggle?: () => void;
  stagedCount: number;
  canAttachImages: boolean;
  sending: boolean;
  sendLabel: string;
  onLiveChange: (value: string) => void;
  onBlur: () => void;
  onSubmit: () => void;
}) {
  countRender("ComposerTextarea");
  const [value, setValue] = React.useState(initialValue);
  const appliedExternalRef = React.useRef<number | null>(null);
  React.useEffect(() => {
    if (!externalValue || appliedExternalRef.current === externalValue.at) return;
    appliedExternalRef.current = externalValue.at;
    setValue(externalValue.value);
  }, [externalValue]);
  return (
    <>
      <textarea
        placeholder={
          readOnly
            ? "View-only console"
            : sendWithheld
              ? `You can view ${agentLabel} but not message it`
              : voiceActive
                ? `Message ${agentLabel} (background agent)…`
                : `Message ${agentLabel}…`
        }
        value={value}
        disabled={readOnly || sendWithheld}
        onChange={(e) => {
          if (readOnly || sendWithheld) return;
          setValue(e.target.value);
          onLiveChange(e.target.value);
        }}
        onBlur={onBlur}
        onKeyDown={(e) => { if (e.key === "Enter" && !e.shiftKey) { e.preventDefault(); onSubmit(); } }}
        rows={2}
        data-testid={`chat-composer:${identity}`}
      />
      <div className="composer__row">
        <span className="composer__chip mono">{agentRole || "agent"}</span>
        <span className="composer__spacer" />
        {onVoiceToggle && !readOnly && !sendWithheld && (
          <VoiceButton
            agentLabel={agentLabel}
            active={voiceActive}
            disabled={voiceDisabled}
            onClick={onVoiceToggle}
          />
        )}
        <button
          className="composer__send"
          disabled={
            (!value.trim() && stagedCount === 0)
            || readOnly
            || sendWithheld
            || (stagedCount > 0 && !canAttachImages)
            || (stagedCount > 0 && sending)
          }
          onClick={onSubmit}
          data-testid={`chat-send:${identity}`}
        >
          {sendLabel}  ⏎
        </button>
      </div>
    </>
  );
});

export function ChatPane({
  agent,
  agentLabel,
  identity,
  entries,
  phase,
  draft,
  sending,
  readOnly = false,
  accessEnforcing = false,
  staged,
  onDraftChange,
  onStagedChange,
  onSend,
  onInspect,
  onRespawn,
  onRetire,
  inspectLabel = "Details",
  respawnLabel = "Respawn",
  retireLabel = "Retire",
  sendLabel = "Send",
  hasOlderHistory = false,
  loadingOlderHistory = false,
  isLoadingHistory = false,
  onLoadOlder,
  stackSlot,
  voiceSlot,
  onVoiceToggle,
  voiceActive = false,
  voiceDisabled = false,
  workGraphActions = null,
}: ChatPaneProps): React.JSX.Element {
  countRender("ChatPane");
  // The live composer value lives inside ComposerTextarea (below) so a
  // keystroke re-renders only that component. ChatPane keeps a ref to the
  // current value for submit and for the blob-reference effect, and a
  // debounced publisher back to the parent's persisted per-panel draft.
  const liveDraftRef = React.useRef(draft);
  const lastPublishedDraftRef = React.useRef(draft);
  const publishDraftTimerRef = React.useRef<number | null>(null);
  const onDraftChangeRef = React.useRef(onDraftChange);
  onDraftChangeRef.current = onDraftChange;
  const publishDraft = React.useCallback(() => {
    if (publishDraftTimerRef.current !== null) {
      window.clearTimeout(publishDraftTimerRef.current);
      publishDraftTimerRef.current = null;
    }
    const value = liveDraftRef.current;
    if (value === lastPublishedDraftRef.current) return;
    lastPublishedDraftRef.current = value;
    onDraftChangeRef.current(value);
  }, []);
  // Bumped when ChatPane needs the blob-reference effect to re-run for a
  // new live value; the textarea reports through `onLiveChange`.
  const [liveDraftTick, setLiveDraftTick] = React.useState(0);
  const onLiveChange = React.useCallback(
    (value: string) => {
      liveDraftRef.current = value;
      if (publishDraftTimerRef.current !== null) {
        window.clearTimeout(publishDraftTimerRef.current);
      }
      publishDraftTimerRef.current = window.setTimeout(publishDraft, 400);
      // Only the blob-reference scan needs ChatPane to notice a change, and
      // only when the text can carry a reference at all.
      if (value.includes("blob")) setLiveDraftTick((n) => n + 1);
    },
    [publishDraft],
  );
  // Values pushed down into the textarea, each stamped with a sequence so
  // the textarea applies every new one exactly once: the persisted copy
  // changed underneath us (send cleared it, panel navigation swapped the
  // draft), or ChatPane itself rewrote the text (a blob reference was
  // stripped after being staged as an attachment).
  const externalSeqRef = React.useRef(0);
  const [externalValue, setExternalValue] = React.useState<{ value: string; at: number } | null>(null);
  React.useEffect(() => {
    if (draft === lastPublishedDraftRef.current) return;
    lastPublishedDraftRef.current = draft;
    liveDraftRef.current = draft;
    externalSeqRef.current += 1;
    setExternalValue({ value: draft, at: externalSeqRef.current });
  }, [draft]);
  const setDraft = React.useCallback(
    (value: string) => {
      liveDraftRef.current = value;
      externalSeqRef.current += 1;
      setExternalValue({ value, at: externalSeqRef.current });
      publishDraft();
    },
    [publishDraft],
  );
  React.useEffect(() => () => publishDraft(), [publishDraft]);
  const bodyRef = React.useRef<HTMLDivElement>(null);
  const preserveOlderHistoryScrollRef = React.useRef(false);
  const olderHistoryScrollHeightRef = React.useRef(0);
  const olderHistoryScrollTopRef = React.useRef(0);
  const activeTurnFrameRef = React.useRef(0);
  const [visibleTurnIndexes, setVisibleTurnIndexes] = React.useState<number[]>([]);

  const messages = React.useMemo(() => {
    return buildChatMessages(entries);
  }, [entries]);
  const turns = React.useMemo(() => buildChatTurns(messages), [messages]);
  // Transcript window: see TRANSCRIPT_WINDOW_TURNS. Keyed by identity so a
  // pane that navigates to another agent starts at that agent's tail again.
  const [revealedFrom, setRevealedFrom] = React.useState<
    ({ identity: string } & TranscriptWindowAnchor) | null
  >(null);
  const windowAnchor = revealedFrom && revealedFrom.identity === identity ? revealedFrom : null;
  const turnIndexById = React.useMemo(() => {
    const index = new Map<string, number>();
    turns.forEach((turn, i) => index.set(turn.id, i));
    return index;
  }, [turns]);
  const windowStart = transcriptWindowStart(
    turns.length,
    windowAnchor,
    (id) => turnIndexById.get(id) ?? -1,
  );
  const pendingScrollToTurnRef = React.useRef<number | null>(null);
  /// Reveal turns down to `firstIndex`, keeping the content under the
  /// viewport in place (same anchor as an older-history prepend).
  const revealTurnsFrom = React.useCallback(
    (firstIndex: number) => {
      const target = Math.max(0, firstIndex);
      if (bodyRef.current) {
        preserveOlderHistoryScrollRef.current = true;
        olderHistoryScrollHeightRef.current = bodyRef.current.scrollHeight;
        olderHistoryScrollTopRef.current = bodyRef.current.scrollTop;
      }
      const turnId = target === 0 ? "" : turns[target]?.id ?? "";
      setRevealedFrom({ identity, turnId, mountedTurns: turns.length - target });
    },
    [identity, turns],
  );
  const revealEarlier = React.useCallback(() => {
    revealTurnsFrom(windowStart - TRANSCRIPT_WINDOW_STEP);
  }, [revealTurnsFrom, windowStart]);
  // The id of the in-progress (latest) agent turn. While `phase` is non-null the
  // turn is still working, so we suppress that turn's "Worked for Ns" summary —
  // otherwise it renders alongside the "working…" indicator (the done + working
  // contradiction). Earlier, completed turns keep their summary regardless.
  const lastAgentMessageId = React.useMemo(() => {
    for (let i = messages.length - 1; i >= 0; i -= 1) {
      if (messages[i].kind === "agent") return messages[i].id;
    }
    return null;
  }, [messages]);
  const scrollSignature = React.useMemo(() => {
    const last = messages[messages.length - 1];
    const lastTextLength = last?.text?.length ?? 0;
    const lastBlockLength = last?.blocks
      ? JSON.stringify(last.blocks).length
      : last?.workGraphEntry
        ? JSON.stringify(last.workGraphEntry).length
        : 0;
    return [
      identity,
      messages.length,
      last?.id ?? "",
      lastTextLength,
      lastBlockLength,
      phase ?? "",
    ].join(":");
  }, [identity, messages, phase]);

  React.useLayoutEffect(() => {
    if (preserveOlderHistoryScrollRef.current && bodyRef.current) {
      const node = bodyRef.current;
      const addedHeight = node.scrollHeight - olderHistoryScrollHeightRef.current;
      node.scrollTop = olderHistoryScrollTopRef.current + Math.max(0, addedHeight);
      node.scrollLeft = 0;
      preserveOlderHistoryScrollRef.current = false;
      return;
    }
    const resetTranscriptScroll = () => {
      if (bodyRef.current) {
        bodyRef.current.scrollTop = bodyRef.current.scrollHeight;
        bodyRef.current.scrollLeft = 0;
      }
    };
    resetTranscriptScroll();
    const firstFrame = window.requestAnimationFrame(resetTranscriptScroll);
    const secondFrame = window.requestAnimationFrame(resetTranscriptScroll);
    return () => {
      window.cancelAnimationFrame(firstFrame);
      window.cancelAnimationFrame(secondFrame);
    };
  }, [scrollSignature, windowStart]);

  // A rail jump into the hidden range reveals first, then scrolls once the
  // target turn is mounted.
  React.useEffect(() => {
    const turnIndex = pendingScrollToTurnRef.current;
    if (turnIndex === null || turnIndex < windowStart) return;
    pendingScrollToTurnRef.current = null;
    bodyRef.current
      ?.querySelector<HTMLElement>(`[data-chat-turn-index="${turnIndex}"]`)
      ?.scrollIntoView({ block: "start", behavior: "smooth" });
  }, [windowStart]);

  React.useEffect(() => {
    if (!loadingOlderHistory && preserveOlderHistoryScrollRef.current) {
      preserveOlderHistoryScrollRef.current = false;
    }
  }, [loadingOlderHistory]);

  const updateActiveTurn = React.useCallback(() => {
    activeTurnFrameRef.current = 0;
    const body = bodyRef.current;
    if (!body || turns.length <= 1) {
      setVisibleTurnIndexes([]);
      return;
    }

    const turnNodes = Array.from(
      body.querySelectorAll<HTMLElement>("[data-chat-turn-index]"),
    );
    if (turnNodes.length === 0) {
      setVisibleTurnIndexes([]);
      return;
    }

    const bodyRect = body.getBoundingClientRect();
    const visibleTop = bodyRect.top;
    const visibleBottom = bodyRect.bottom;
    const targetY = bodyRect.top + Math.min(128, Math.max(48, bodyRect.height * 0.24));
    let nextIndex = 0;
    const nextVisibleIndexes: number[] = [];
    for (const turnNode of turnNodes) {
      const rawIndex = Number(turnNode.dataset.chatTurnIndex);
      if (!Number.isFinite(rawIndex)) {
        continue;
      }
      const turnRect = turnNode.getBoundingClientRect();
      if (turnRect.bottom >= visibleTop && turnRect.top <= visibleBottom) {
        nextVisibleIndexes.push(rawIndex);
      }
      if (turnRect.top <= targetY) {
        nextIndex = rawIndex;
      }
    }
    const nextIndexes = nextVisibleIndexes.length > 0 ? nextVisibleIndexes : [nextIndex];
    setVisibleTurnIndexes((current) => {
      if (current.length === nextIndexes.length && current.every((value, index) => value === nextIndexes[index])) {
        return current;
      }
      return nextIndexes;
    });
  }, [turns.length]);

  const scheduleActiveTurnUpdate = React.useCallback(() => {
    if (activeTurnFrameRef.current) {
      return;
    }
    activeTurnFrameRef.current = window.requestAnimationFrame(updateActiveTurn);
  }, [updateActiveTurn]);

  React.useEffect(() => {
    scheduleActiveTurnUpdate();
  }, [scheduleActiveTurnUpdate, scrollSignature]);

  React.useEffect(() => {
    updateActiveTurn();
    window.addEventListener("resize", scheduleActiveTurnUpdate);
    return () => {
      if (activeTurnFrameRef.current) {
        window.cancelAnimationFrame(activeTurnFrameRef.current);
        activeTurnFrameRef.current = 0;
      }
      window.removeEventListener("resize", scheduleActiveTurnUpdate);
    };
  }, [scheduleActiveTurnUpdate, updateActiveTurn]);

  function scrollToTurn(turnIndex: number) {
    if (turnIndex < windowStart) {
      pendingScrollToTurnRef.current = turnIndex;
      revealTurnsFrom(turnIndex);
      return;
    }
    const turnNode = bodyRef.current?.querySelector<HTMLElement>(
      `[data-chat-turn-index="${turnIndex}"]`,
    );
    turnNode?.scrollIntoView({ block: "start", behavior: "smooth" });
  }

  const onLoadOlderRef = React.useRef(onLoadOlder);
  onLoadOlderRef.current = onLoadOlder;
  const requestOlderHistory = React.useCallback(() => {
    if (bodyRef.current) {
      preserveOlderHistoryScrollRef.current = true;
      olderHistoryScrollHeightRef.current = bodyRef.current.scrollHeight;
      olderHistoryScrollTopRef.current = bodyRef.current.scrollTop;
    }
    onLoadOlderRef.current?.();
  }, []);
  const hasOlderHistoryRef = React.useRef(hasOlderHistory);
  hasOlderHistoryRef.current = hasOlderHistory;
  const loadingOlderHistoryRef = React.useRef(loadingOlderHistory);
  loadingOlderHistoryRef.current = loadingOlderHistory;
  const windowStartRef = React.useRef(windowStart);
  windowStartRef.current = windowStart;
  const revealEarlierRef = React.useRef(revealEarlier);
  revealEarlierRef.current = revealEarlier;
  const onBodyScroll = React.useCallback<React.UIEventHandler<HTMLDivElement>>(
    (event) => {
      if (event.currentTarget.scrollLeft !== 0) {
        event.currentTarget.scrollLeft = 0;
      }
      scheduleActiveTurnUpdate();
      if (event.currentTarget.scrollTop > 32) return;
      if (windowStartRef.current > 0) {
        revealEarlierRef.current();
        return;
      }
      if (hasOlderHistoryRef.current && !loadingOlderHistoryRef.current) {
        requestOlderHistory();
      }
    },
    [requestOlderHistory, scheduleActiveTurnUpdate],
  );
  const initial = (agentLabel || "?").trim().charAt(0).toUpperCase() || "?";
  const state = (agent?.state || "unknown").toLowerCase();
  const canAttachImages = !readOnly && agent?.model_capabilities?.image_input === true;
  // Access control can grant view without send; `false` means the runtime
  // explicitly withheld the send affordance. Unknown (absent) stays sendable.
  const sendWithheld = accessEnforcing && agent?.affordances?.can_send_message === false;
  const [dragActive, setDragActive] = React.useState(false);
  const [attachmentError, setAttachmentError] = React.useState<string | null>(null);
  const resolvedDraftBlobRefs = React.useRef("");

  const railRef = React.useRef<HTMLElement | null>(null);
  const [railHeight, setRailHeight] = React.useState<number | null>(null);
  React.useEffect(() => {
    const nav = railRef.current;
    if (!nav || typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver((entries) => {
      for (const entry of entries) {
        setRailHeight(entry.contentRect.height);
      }
    });
    observer.observe(nav);
    return () => observer.disconnect();
  }, [turns.length > 1]);

  const railWindow = windowTurnRail(turns.length, railHeight);
  const turnRail = turns.length > 1 ? (
    <nav className="conv-turn-rail" aria-label="Conversation turns" ref={railRef}>
      <ol className="conv-turn-rail__list">
        {railWindow.overflow > 0 && (
          <li className="conv-turn-rail__item" key="rail-overflow">
            <button
              aria-label={`Jump to the ${railWindow.overflow} earlier turns`}
              className="conv-turn-rail__button"
              data-testid={`chat-turn-rail:${identity}:overflow`}
              onClick={(event) => {
                scrollToTurn(0);
                if (event.detail > 0) {
                  event.currentTarget.blur();
                }
              }}
              type="button"
            >
              <span
                className="conv-turn-rail__tick conv-turn-rail__tick--overflow"
                aria-hidden="true"
              />
            </button>
            <div className="conv-turn-preview" role="presentation">
              <div className="conv-turn-preview__title">
                {railWindow.overflow} earlier turns
              </div>
              <div className="conv-turn-preview__body">
                Jump to the start of the visible history.
              </div>
            </div>
          </li>
        )}
        {turns.slice(railWindow.start).map((turn, railIndex) => {
          const turnIndex = railWindow.start + railIndex;
          const preview = chatTurnPreview(turn);
          const isVisibleTurn = visibleTurnIndexes.includes(turnIndex);
          return (
            <li className="conv-turn-rail__item" key={turn.id}>
              <button
                aria-current={isVisibleTurn ? "true" : undefined}
                aria-label={`Jump to turn ${turnIndex + 1}: ${preview.title}`}
                className={`conv-turn-rail__button${isVisibleTurn ? " is-active" : ""}`}
                data-testid={`chat-turn-rail:${identity}:${turnIndex}`}
                onClick={(event) => {
                  scrollToTurn(turnIndex);
                  if (event.detail > 0) {
                    event.currentTarget.blur();
                  }
                }}
                type="button"
              >
                <span className="conv-turn-rail__tick" aria-hidden="true" />
              </button>
              <div className="conv-turn-preview" role="presentation">
                <div className="conv-turn-preview__title">{preview.title}</div>
                <div className="conv-turn-preview__body">{preview.body}</div>
              </div>
            </li>
          );
        })}
      </ol>
    </nav>
  ) : null;

  function addFiles(fileList: FileList | File[]) {
    if (readOnly || !canAttachImages) return;
    const files = dedupeComposerImageFiles(Array.from(fileList));
    const accepted: StagedAttachment[] = [];
    let error: string | null = null;
    for (const file of files) {
      if (!ALLOWED_IMAGE_TYPES.has(file.type)) {
        error = "Unsupported image type";
        continue;
      }
      if (file.size > MAX_ATTACHMENT_BYTES) {
        error = "Image exceeds 25 MiB";
        continue;
      }
      accepted.push({
        id: `${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 8)}`,
        file,
        previewUrl: URL.createObjectURL(file),
      });
    }
    onStagedChange((current) => {
      const currentKeys = new Set(current.map((item) => composerImageFileKey(item.file)));
      const append: StagedAttachment[] = [];
      for (const item of accepted) {
        const key = composerImageFileKey(item.file);
        if (currentKeys.has(key)) {
          URL.revokeObjectURL(item.previewUrl);
          continue;
        }
        currentKeys.add(key);
        if (current.length + append.length >= MAX_ATTACHMENTS) {
          URL.revokeObjectURL(item.previewUrl);
          error = `Maximum ${MAX_ATTACHMENTS} images`;
          continue;
        }
        append.push(item);
      }
      return [...current, ...append];
    });
    setAttachmentError(error);
  }

  function removeAttachment(id: string) {
    onStagedChange((current) => {
      const removed = current.find((item) => item.id === id);
      if (removed) URL.revokeObjectURL(removed.previewUrl);
      return current.filter((item) => item.id !== id);
    });
  }

  React.useEffect(() => {
    if (!canAttachImages) return;
    const refs = consoleBlobReferencesFromText(liveDraftRef.current);
    if (refs.length === 0) {
      resolvedDraftBlobRefs.current = "";
      return;
    }
    const signature = refs.map((ref) => ref.href).join("\n");
    if (signature === resolvedDraftBlobRefs.current) return;

    let cancelled = false;
    const timer = window.setTimeout(() => {
      void (async () => {
        const files: File[] = [];
        const seen = new Set<string>();
        for (const ref of refs) {
          if (seen.has(ref.href)) continue;
          seen.add(ref.href);
          const file = await fileFromConsoleBlobUrl(ref.href);
          if (file) files.push(file);
        }
        if (cancelled) return;
        if (files.length > 0) {
          resolvedDraftBlobRefs.current = signature;
          addFiles(files);
          setDraft(stripConsoleBlobReferencesFromText(liveDraftRef.current, refs));
          publishDraft();
        } else {
          setAttachmentError("No usable image found");
        }
      })();
    }, 350);

    return () => {
      cancelled = true;
      window.clearTimeout(timer);
    };
  }, [canAttachImages, liveDraftTick, publishDraft, setDraft]);

  const submitComposerRef = React.useRef<() => Promise<void>>(async () => {});
  const submitComposer = React.useCallback(() => submitComposerRef.current(), []);
  submitComposerRef.current = async function submitComposerNow() {
    if (staged.length > 0 && !canAttachImages) {
      setAttachmentError("model cannot see images");
      return;
    }
    if (readOnly || sendWithheld) {
      return;
    }
    const text = liveDraftRef.current;
    if (!text.trim() && staged.length === 0) {
      return;
    }
    const files = staged.map((item) => item.file);
    // The submitted text must not be re-published as a draft by a pending
    // debounce after the send cleared it; hold the publish until we know.
    if (publishDraftTimerRef.current !== null) {
      window.clearTimeout(publishDraftTimerRef.current);
      publishDraftTimerRef.current = null;
    }
    const setComposerText = (value: string) => {
      liveDraftRef.current = value;
      lastPublishedDraftRef.current = value;
      externalSeqRef.current += 1;
      setExternalValue({ value, at: externalSeqRef.current });
    };
    // Text-only sends empty the composer synchronously, in the same update
    // as the parent's queue or send bookkeeping, so a busy runtime cannot
    // freeze the visible composer with the submitted text still in it; the
    // text comes back if the send is refused. Sends with attachments keep
    // the text until the upload succeeded.
    const clearedEarly = files.length === 0;
    if (clearedEarly) setComposerText("");
    const restoreIfUntouched = () => {
      if (clearedEarly && liveDraftRef.current === "") setComposerText(text);
    };
    try {
      const sent = await onSend(files, text);
      if (sent) {
        staged.forEach((item) => URL.revokeObjectURL(item.previewUrl));
        onStagedChange([]);
        setAttachmentError(null);
        if (!clearedEarly && liveDraftRef.current === text) setComposerText("");
        return;
      }
      restoreIfUntouched();
      publishDraft();
    } catch {
      setAttachmentError("send failed; images retained");
      restoreIfUntouched();
      publishDraft();
    }
  };

  return (
    <div className="conv" data-testid={`chat-pane:${identity}`}>
      <div className="conv__head">
        <div className="conv__avatar">{initial}</div>
        <div style={{ minWidth: 0 }}>
          <div className="conv__title">{agentLabel}</div>
          <div className="conv__identity">
            {identity}{agent?.role ? ` · ${agent.role}` : ""}
          </div>
        </div>
        <div className="conv__actions">
          {onInspect ? <button className="conv__action" onClick={onInspect} data-testid="conv-action:details">{inspectLabel}</button> : null}
          {agent?.affordances?.can_respawn && onRespawn ? (
            <button className="conv__action" onClick={onRespawn} data-testid="conv-action:respawn">{respawnLabel}</button>
          ) : null}
          {agent?.affordances?.can_retire && onRetire ? (
            <button className="conv__action" onClick={onRetire} data-testid="conv-action:retire">{retireLabel}</button>
          ) : null}
        </div>
      </div>
      <TranscriptView
        identity={identity}
        agentLabel={agentLabel}
        turns={turns}
        messages={messages}
        phase={phase}
        lastAgentMessageId={lastAgentMessageId}
        workGraphActions={workGraphActions}
        isLoadingHistory={isLoadingHistory}
        hasOlderHistory={hasOlderHistory}
        loadingOlderHistory={loadingOlderHistory}
        windowStart={windowStart}
        onRevealEarlier={revealEarlier}
        bodyRef={bodyRef}
        onScroll={onBodyScroll}
        onRequestOlderHistory={requestOlderHistory}
      />
      {turnRail}
      {stackSlot}
      <div className="composer">
        {voiceSlot}
        <div
          className={`composer__shell${dragActive && canAttachImages ? " is-drag-active" : ""}`}
          onDragLeave={() => setDragActive(false)}
          onDragOver={(event) => {
            if (!canAttachImages) return;
            event.preventDefault();
            setDragActive(true);
          }}
          onDrop={(event) => {
            if (!canAttachImages) return;
            event.preventDefault();
            setDragActive(false);
            const payload = collectImageTransferPayload(event.dataTransfer);
            void imageFilesFromTransferPayload(payload).then((files) => {
              if (files.length > 0) {
                addFiles(files);
              } else {
                setAttachmentError("No usable image found");
              }
            });
          }}
          onPaste={(event) => {
            if (!canAttachImages) return;
            const payload = collectImageTransferPayload(event.clipboardData);
            if (imageTransferPayloadHasImage(payload)) {
              event.preventDefault();
              void imageFilesFromTransferPayload(payload).then((files) => {
                if (files.length > 0) {
                  addFiles(files);
                } else {
                  setAttachmentError("No usable image found");
                }
              });
            }
          }}
        >
          {staged.length > 0 && (
            <div className="composer__attachments">
              {staged.map((item) => (
                <div className="composer__attachment" key={item.id}>
                  <img alt="" src={item.previewUrl} />
                  <button aria-label="Remove attachment" onClick={() => removeAttachment(item.id)} type="button">×</button>
                </div>
              ))}
            </div>
          )}
          <ComposerTextarea
            identity={identity}
            agentLabel={agentLabel}
            agentRole={agent?.role ?? null}
            initialValue={draft}
            externalValue={externalValue}
            readOnly={readOnly}
            sendWithheld={sendWithheld}
            voiceActive={voiceActive}
            voiceDisabled={voiceDisabled}
            onVoiceToggle={onVoiceToggle}
            stagedCount={staged.length}
            canAttachImages={canAttachImages}
            sending={sending}
            sendLabel={sendLabel}
            onLiveChange={onLiveChange}
            onBlur={publishDraft}
            onSubmit={submitComposer}
          />
        </div>
        <div className="composer__footer">
          <span>To: <b style={{ color: "var(--ink-muted)" }}>{agentLabel}</b></span>
          {voiceActive && <span>· text to background agent</span>}
          <span>·</span>
          <span className="mono">{identity}</span>
          {agent?.role && (<>
            <span>·</span>
            <span>{agent.role}</span>
          </>)}
          <span>·</span>
          <span className="dot" style={{
            background: state === "active" || state === "running" ? "var(--ok)" :
                        state.includes("degrade") ? "var(--warn)" :
                        state === "retired" ? "var(--ink-faint)" : "var(--ink-dim)",
          }} />
          <span>{state}</span>
          {phase && <><span>·</span><span style={{ color: "var(--accent)" }}>{phase}</span></>}
          {readOnly && <><span>·</span><span>view only</span></>}
          {!readOnly && sendWithheld && <><span>·</span><span>send not permitted</span></>}
          {!readOnly && !canAttachImages && <><span>·</span><span>model cannot see images</span></>}
          {attachmentError && <><span>·</span><span style={{ color: "var(--bad)" }}>{attachmentError}</span></>}
        </div>
      </div>
    </div>
  );
}
