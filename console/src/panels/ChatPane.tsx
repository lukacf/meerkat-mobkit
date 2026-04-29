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
  onDraftChange: (value: string) => void;
  onSend: () => void;
  onInspect: () => void;
  onRespawn: () => void;
  onRetire: () => void;
}

type MsgKind = "origin" | "user" | "agent" | "tool" | "thought" | "gate";

interface Msg {
  id: string;
  kind: MsgKind;
  time: string;
  who?: string;
  text?: string;
  blocks?: ConversationRichBlock[];
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

export function ChatPane({
  agent,
  agentLabel,
  identity,
  entries,
  phase,
  draft,
  sending,
  onDraftChange,
  onSend,
  onInspect,
  onRespawn,
  onRetire,
}: ChatPaneProps): React.JSX.Element {
  const bodyRef = React.useRef<HTMLDivElement>(null);
  React.useEffect(() => {
    if (bodyRef.current) bodyRef.current.scrollTop = bodyRef.current.scrollHeight;
  }, [entries.length, phase]);

  const messages = React.useMemo(() => {
    // Defensive cross-entry merge: the adapter groups consecutive
    // peer tool calls into one entry only when the previous entry is
    // already a peer group. Any interleaved frame (a text delta, a
    // module event, a non-peer tool) breaks the chain — leaving us
    // with N adjacent rich entries each holding one peer tool block.
    // Re-merge them at the message level so `ConversationRichContent`
    // sees `blocks.length > 1` and renders `PeerToolGroup`.
    const flat = entries.flatMap(flattenEntry);
    const merged: Msg[] = [];
    for (const m of flat) {
      const last = merged[merged.length - 1];
      const canMerge =
        last &&
        last.kind === "tool" &&
        m.kind === "tool" &&
        Array.isArray(last.blocks) &&
        Array.isArray(m.blocks) &&
        // Only fold blocks that are all peer tool calls (regardless
        // of direction). Generic tool calls keep their own row.
        last.blocks.every(
          (b) => b.type === "tool-call" && (b.peerTarget !== undefined || b.peerIncoming === true),
        ) &&
        m.blocks.every(
          (b) => b.type === "tool-call" && (b.peerTarget !== undefined || b.peerIncoming === true),
        ) &&
        // Don't fold incoming + outgoing into the same group.
        last.blocks[0].type === "tool-call" &&
        m.blocks[0].type === "tool-call" &&
        Boolean((last.blocks[0]).peerIncoming) === Boolean((m.blocks[0]).peerIncoming);
      if (canMerge && last && last.blocks && m.blocks) {
        last.blocks = [...last.blocks, ...m.blocks];
        last.id = `${last.id}+${m.id}`;
      } else {
        merged.push({ ...m });
      }
    }
    return merged;
  }, [entries]);
  const initial = (agentLabel || "?").trim().charAt(0).toUpperCase() || "?";
  const state = (agent?.state || "unknown").toLowerCase();

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
          <div className="msg msg--origin" data-testid={`chat-phase:${phase}`}>
            <div className="msg__time" />
            <div className="msg__bubble"><span className="msg__text">{agentLabel} is {phase.replace("-", " ")}…</span></div>
          </div>
        )}
      </div>
      <div className="composer">
        <div className="composer__shell">
          <textarea
            placeholder={`Message ${agentLabel}…    @ to mention, / for commands`}
            value={draft}
            onChange={(e) => onDraftChange(e.target.value)}
            onKeyDown={(e) => { if (e.key === "Enter" && !e.shiftKey) { e.preventDefault(); onSend(); } }}
            disabled={sending}
            rows={2}
            data-testid={`chat-composer:${identity}`}
          />
          <div className="composer__row">
            <span className="composer__chip"><span className="k">/</span> commands</span>
            <span className="composer__chip"><span className="k">@</span> mention</span>
            <span className="composer__chip mono">{agent?.role || "agent"}</span>
            <span className="composer__spacer" />
            <button
              className="composer__send"
              disabled={!draft.trim() || sending}
              onClick={onSend}
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
        </div>
      </div>
    </div>
  );
}
