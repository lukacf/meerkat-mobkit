import React from "react";
import type {
  ConversationTimelineEntry,
  ConversationRichBlock,
} from "@console-core";
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

function summariseRichBlock(block: ConversationRichBlock): Msg["text"] {
  switch (block.type) {
    case "paragraph":   return block.text;
    case "heading":     return block.text;
    case "code":        return block.code;
    case "command":     return block.command;
    case "divider":     return block.text;
    case "file-change": return `${block.verb} ${block.name} (+${block.plus}/-${block.minus})`;
    case "table":       return block.rows.map((r) => r.join(" · ")).join("\n");
    case "tool-call": {
      const parts = [block.name];
      if (block.peerTarget) parts.push(`→ ${block.peerTarget}`);
      if (block.peerIntent) parts.push(`(${block.peerIntent})`);
      if (block.peerBody) parts.push(block.peerBody.slice(0, 160));
      else if (block.arguments) parts.push(block.arguments.slice(0, 160));
      return parts.join(" ");
    }
    case "thinking":    return block.text;
    default:            return "";
  }
}

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

  if (entry.variant === "rich" && Array.isArray(entry.blocks) && entry.blocks.length > 0) {
    const msgs: Msg[] = [];
    for (let i = 0; i < entry.blocks.length; i++) {
      const block = entry.blocks[i];
      const text = summariseRichBlock(block);
      if (!text) continue;
      if (block.type === "tool-call") {
        msgs.push({
          id: `${entry.id}:${i}`,
          kind: "tool",
          time: formatTime(entry.createdAt),
          text,
        });
      } else if (block.type === "thinking") {
        msgs.push({
          id: `${entry.id}:${i}`,
          kind: "thought",
          time: formatTime(entry.createdAt),
          text,
        });
      } else {
        msgs.push({
          id: `${entry.id}:${i}`,
          kind: isUser ? "user" : "agent",
          time: formatTime(entry.createdAt),
          who: isUser ? undefined : label,
          text,
        });
      }
    }
    return msgs.length
      ? msgs
      : [{
          id: entry.id,
          kind: isUser ? "user" : "agent",
          time: formatTime(entry.createdAt),
          who: isUser ? undefined : label,
          text: "",
        }];
  }

  return [{
    id: entry.id,
    kind: isUser ? "user" : "agent",
    time: formatTime(entry.createdAt),
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

  const messages = React.useMemo(() => entries.flatMap(flattenEntry), [entries]);
  const initial = (agentLabel || "?").trim().charAt(0).toUpperCase() || "?";
  const state = (agent?.state || "unknown").toLowerCase();

  return (
    <div className="conv" data-testid={`chat-pane:${identity}`}>
      <div className="conv__head">
        <div className="conv__avatar">{initial}</div>
        <div style={{ minWidth: 0 }}>
          <div className="conv__title">{agentLabel}</div>
          <div className="conv__identity">
            {identity}{agent?.profile ? ` · ${agent.profile}` : ""}
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
              {m.text && <span className="msg__text">{m.text}</span>}
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
            <span className="composer__chip mono">{agent?.profile || "agent"}</span>
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
          {agent?.profile && (<>
            <span>·</span>
            <span>{agent.profile}</span>
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
