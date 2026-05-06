import React from "react";
import type {
  ConversationTimelineEntry,
  ConversationRichBlock,
} from "@console-core";
import { ConversationRichContent } from "@console-components";
import type { ConsoleAgent } from "../types";

interface ChatPaneProps {
  agent: ConsoleAgent | null;
  agentLabel: string;
  identity: string;
  entries: ConversationTimelineEntry[];
  phase: "waiting" | "tool-executing" | "generating" | null;
  draft: string;
  sending: boolean;
  staged: StagedAttachment[];
  onDraftChange: (value: string) => void;
  onStagedChange: React.Dispatch<React.SetStateAction<StagedAttachment[]>>;
  onSend: (attachments?: File[]) => boolean | Promise<boolean>;
  onInspect: () => void;
  onRespawn: () => void;
  onRetire: () => void;
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
  who?: string;
  text?: string;
  blocks?: ConversationRichBlock[];
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
      text: `${entry.title} (+${entry.plus}/-${entry.minus})`,
    }];
  }

  if (entry.variant === "meta") {
    return [{
      id: entry.id,
      kind: "origin",
      time: formatTime(entry.createdAt),
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
          who: isUser ? undefined : label,
          text: "",
        }];
  }

  return [{
    id: entry.id,
    kind: isUser ? "user" : "agent",
    time,
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

function imageFilesFromClipboard(data: DataTransfer): File[] {
  const directFiles = Array.from(data.files).filter((file) => file.type.startsWith("image/"));
  if (directFiles.length > 0) {
    return directFiles;
  }
  return Array.from(data.items)
    .filter((item) => item.kind === "file" && item.type.startsWith("image/"))
    .map((item) => item.getAsFile())
    .filter((file): file is File => Boolean(file));
}

export function ChatPane({
  agent,
  agentLabel,
  identity,
  entries,
  phase,
  draft,
  sending,
  staged,
  onDraftChange,
  onStagedChange,
  onSend,
  onInspect,
  onRespawn,
  onRetire,
  stackSlot,
}: ChatPaneProps): React.JSX.Element {
  const bodyRef = React.useRef<HTMLDivElement>(null);
  React.useEffect(() => {
    if (bodyRef.current) bodyRef.current.scrollTop = bodyRef.current.scrollHeight;
  }, [entries.length, phase]);

  const messages = React.useMemo(() => {
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
        if (last && m.kind === "agent" && last.kind === "agent" && last.who === m.who) {
          const lastSignature = textSignatureForMsg(last);
          const nextSignature = textSignatureForMsg(m);
          if (lastSignature && lastSignature === nextSignature) {
            continue;
          }
        }
        merged.push({ ...m });
      }
    }
    return merged;
  }, [entries]);
  const initial = (agentLabel || "?").trim().charAt(0).toUpperCase() || "?";
  const state = (agent?.state || "unknown").toLowerCase();
  const canAttachImages = agent?.model_capabilities?.image_input === true;
  const [dragActive, setDragActive] = React.useState(false);
  const [attachmentError, setAttachmentError] = React.useState<string | null>(null);
  const fileInputRef = React.useRef<HTMLInputElement>(null);

  function addFiles(fileList: FileList | File[]) {
    if (!canAttachImages) return;
    const files = Array.from(fileList);
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
      const next = [...current, ...accepted].slice(0, MAX_ATTACHMENTS);
      if (current.length + accepted.length > MAX_ATTACHMENTS) {
        error = `Maximum ${MAX_ATTACHMENTS} images`;
      }
      accepted.slice(Math.max(0, MAX_ATTACHMENTS - current.length)).forEach((item) => {
        URL.revokeObjectURL(item.previewUrl);
      });
      return next;
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

  async function submitComposer() {
    if (staged.length > 0 && !canAttachImages) {
      setAttachmentError("model cannot see images");
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
          <button className="conv__action" onClick={onInspect} data-testid="conv-action:inspect">Inspect</button>
          <button className="conv__action" onClick={onRespawn} data-testid="conv-action:respawn" disabled={!agent?.affordances?.can_respawn}>Respawn</button>
          <button className="conv__action" onClick={onRetire} data-testid="conv-action:retire" disabled={!agent?.affordances?.can_retire}>Retire</button>
        </div>
      </div>
      <div className="conv__body" ref={bodyRef}>
        {messages.length === 0 && (
          <div className="msg msg--origin">
            <div className="msg__time" />
            <div className="msg__bubble"><span className="msg__text">No messages yet. Say hello to {agentLabel}.</span></div>
          </div>
        )}
        {messages.map((m) => (
          <div className={`msg msg--${m.kind}`} key={m.id}>
            <div className="msg__time">{m.time}</div>
            <div className="msg__bubble">
              {m.kind === "user" && m.who && <span className="msg__who"><b>{m.who}</b></span>}
              {m.kind === "agent" && m.who && <span className="msg__who"><b>{m.who}</b></span>}
              {m.blocks && m.blocks.length > 0 ? (
                <ConversationRichContent blocks={m.blocks} />
              ) : (
                m.text && <span className="msg__text">{m.text}</span>
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
            addFiles(event.dataTransfer.files);
          }}
          onPaste={(event) => {
            if (!canAttachImages) return;
            const files = imageFilesFromClipboard(event.clipboardData);
            if (files.length > 0) {
              event.preventDefault();
              addFiles(files);
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
            placeholder={`Message ${agentLabel}…    @ to mention, / for commands`}
            value={draft}
            onChange={(e) => onDraftChange(e.target.value)}
            onKeyDown={(e) => { if (e.key === "Enter" && !e.shiftKey) { e.preventDefault(); submitComposer(); } }}
            disabled={sending}
            rows={2}
            data-testid={`chat-composer:${identity}`}
          />
          <input
            accept="image/png,image/jpeg,image/webp,image/gif"
            hidden
            multiple
            onChange={(event) => {
              if (event.target.files) addFiles(event.target.files);
              event.currentTarget.value = "";
            }}
            ref={fileInputRef}
            type="file"
          />
          <div className="composer__row">
            {canAttachImages && (
              <button
                className="composer__chip composer__chip--button"
                onClick={() => fileInputRef.current?.click()}
                type="button"
              >
                +
              </button>
            )}
            <span className="composer__chip"><span className="k">/</span> commands</span>
            <span className="composer__chip"><span className="k">@</span> mention</span>
            <span className="composer__chip mono">{agent?.role || "agent"}</span>
            <span className="composer__spacer" />
            <button
              className="composer__send"
              disabled={
                (!draft.trim() && staged.length === 0)
                || (staged.length > 0 && !canAttachImages)
                || sending
              }
              onClick={submitComposer}
              data-testid={`chat-send:${identity}`}
            >
              Send  ⏎
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
          {!canAttachImages && <><span>·</span><span>model cannot see images</span></>}
          {attachmentError && <><span>·</span><span style={{ color: "var(--bad)" }}>{attachmentError}</span></>}
        </div>
      </div>
    </div>
  );
}
