import React from "react";
import type {
  ConversationTimelineEntry,
  ConversationRichBlock,
} from "@console-core";
import {
  conversationRichBlocksToText,
} from "@console-core";
import { ConversationRichContent } from "@console-components";
import type { ConsoleAgent } from "../types";
import {
  composerImageFileKey,
  consoleBlobReferencesFromText,
  consoleBlobUrlsFromText,
  dedupeComposerImageFiles,
  selectImageTransferFiles,
  stripConsoleBlobReferencesFromText,
} from "../lib/composer-attachment-text";

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
  onSend: (attachments?: File[]) => boolean | Promise<boolean>;
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
}

export interface StagedAttachment {
  id: string;
  file: File;
  previewUrl: string;
}

const ALLOWED_IMAGE_TYPES = new Set(["image/png", "image/jpeg", "image/webp", "image/gif"]);
const MAX_ATTACHMENTS = 4;
const MAX_ATTACHMENT_BYTES = 25 * 1024 * 1024;

type MsgKind = "origin" | "user" | "agent" | "tool" | "thought" | "gate";

interface Msg {
  id: string;
  kind: MsgKind;
  time: string;
  createdAt?: string;
  who?: string;
  text?: string;
  blocks?: ConversationRichBlock[];
  workedFor?: string;
  workedForCopyText?: string;
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
  label,
  className = "",
}: {
  text: string;
  label: string;
  className?: string;
}) {
  const [copied, setCopied] = React.useState(false);
  const disabled = !text.trim();

  async function copy() {
    if (disabled) return;
    try {
      await navigator.clipboard.writeText(text);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1400);
    } catch {
      // Clipboard can be unavailable in some browser contexts; no-op keeps
      // the hover affordance from breaking the conversation.
    }
  }

  return (
    <button
      aria-label={copied ? "Copied" : label}
      className={`msg__copy ${className}`}
      data-copied={copied ? "true" : undefined}
      disabled={disabled}
      onClick={(event) => {
        event.stopPropagation();
        void copy();
      }}
      title={copied ? "Copied" : label}
      type="button"
    >
      {copied ? "✓" : "⎘"}
    </button>
  );
}

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
}: ChatPaneProps): React.JSX.Element {
  const bodyRef = React.useRef<HTMLDivElement>(null);
  const preserveOlderHistoryScrollRef = React.useRef(false);
  const olderHistoryScrollHeightRef = React.useRef(0);
  const olderHistoryScrollTopRef = React.useRef(0);

  const messages = React.useMemo(() => {
    return buildChatMessages(entries);
  }, [entries]);
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
  }, [scrollSignature]);

  React.useEffect(() => {
    if (!loadingOlderHistory && preserveOlderHistoryScrollRef.current) {
      preserveOlderHistoryScrollRef.current = false;
    }
  }, [loadingOlderHistory]);

  function requestOlderHistory() {
    if (bodyRef.current) {
      preserveOlderHistoryScrollRef.current = true;
      olderHistoryScrollHeightRef.current = bodyRef.current.scrollHeight;
      olderHistoryScrollTopRef.current = bodyRef.current.scrollTop;
    }
    onLoadOlder?.();
  }

  const transcriptText = React.useMemo(() => transcriptCopyText(messages), [messages]);
  const initial = (agentLabel || "?").trim().charAt(0).toUpperCase() || "?";
  const state = (agent?.state || "unknown").toLowerCase();
  const canAttachImages = !readOnly && agent?.model_capabilities?.image_input === true;
  // Access control can grant view without send; `false` means the runtime
  // explicitly withheld the send affordance. Unknown (absent) stays sendable.
  const sendWithheld = accessEnforcing && agent?.affordances?.can_send_message === false;
  const [dragActive, setDragActive] = React.useState(false);
  const [attachmentError, setAttachmentError] = React.useState<string | null>(null);
  const resolvedDraftBlobRefs = React.useRef("");

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
    const refs = consoleBlobReferencesFromText(draft);
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
          onDraftChange(stripConsoleBlobReferencesFromText(draft, refs));
        } else {
          setAttachmentError("No usable image found");
        }
      })();
    }, 350);

    return () => {
      cancelled = true;
      window.clearTimeout(timer);
    };
  }, [canAttachImages, draft, onDraftChange]);

  async function submitComposer() {
    if (staged.length > 0 && !canAttachImages) {
      setAttachmentError("model cannot see images");
      return;
    }
    if (readOnly || sendWithheld) {
      return;
    }
    if (!draft.trim() && staged.length === 0) {
      return;
    }
    const files = staged.map((item) => item.file);
    try {
      const sent = await onSend(files);
      if (sent) {
        staged.forEach((item) => URL.revokeObjectURL(item.previewUrl));
        onStagedChange([]);
        setAttachmentError(null);
      }
    } catch {
      setAttachmentError("send failed; images retained");
    }
  }

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
      <div
        className="conv__body"
        onScroll={(event) => {
          if (event.currentTarget.scrollLeft !== 0) {
            event.currentTarget.scrollLeft = 0;
          }
          if (
            event.currentTarget.scrollTop <= 32 &&
            hasOlderHistory &&
            !loadingOlderHistory
          ) {
            requestOlderHistory();
          }
        }}
        ref={bodyRef}
      >
        <CopyInlineButton
          className="msg__copy--transcript"
          label="Copy transcript"
          text={transcriptText}
        />
        {hasOlderHistory && (
          <button
            className="conv__history"
            disabled={loadingOlderHistory}
            onClick={requestOlderHistory}
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
        {messages.map((m) => (
          <div className={`msg msg--${m.kind}`} key={m.id}>
            <div className="msg__time">{m.time}</div>
            <div className="msg__bubble">
              {(m.kind === "user" || m.kind === "agent") && (
                <CopyInlineButton label={`Copy ${m.kind === "user" ? "message" : "turn"}`} text={msgCopyText(m)} />
              )}
              {m.blocks && m.blocks.length > 0 ? (
                <ConversationRichContent blocks={m.blocks} />
              ) : (
                m.text && <span className="msg__text">{m.text}</span>
              )}
              {m.workedFor && !(phase && m.id === lastAgentMessageId) && (
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
        ))}
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
      {stackSlot}
      <div className="composer">
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
          <textarea
            placeholder={
              readOnly
                ? "View-only console"
                : sendWithheld
                  ? `You can view ${agentLabel} but not message it`
                  : `Message ${agentLabel}…`
            }
            value={draft}
            disabled={readOnly || sendWithheld}
            onChange={(e) => {
              if (!readOnly && !sendWithheld) onDraftChange(e.target.value);
            }}
            onKeyDown={(e) => { if (e.key === "Enter" && !e.shiftKey) { e.preventDefault(); submitComposer(); } }}
            rows={2}
            data-testid={`chat-composer:${identity}`}
          />
          <div className="composer__row">
            <span className="composer__chip mono">{agent?.role || "agent"}</span>
            <span className="composer__spacer" />
            <button
              className="composer__send"
              disabled={
                (!draft.trim() && staged.length === 0)
                || readOnly
                || sendWithheld
                || (staged.length > 0 && !canAttachImages)
                || (staged.length > 0 && sending)
              }
              onClick={submitComposer}
              data-testid={`chat-send:${identity}`}
            >
              {sendLabel}  ⏎
            </button>
          </div>
        </div>
        <div className="composer__footer">
          <span>To: <b style={{ color: "var(--ink-muted)" }}>{agentLabel}</b></span>
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
