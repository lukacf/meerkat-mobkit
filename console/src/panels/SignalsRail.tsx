import React from "react";
import type { ConsoleFrame } from "../types";

interface SignalsRailProps {
  frames: ConsoleFrame[];
  collapsed: boolean;
  onSelect?: (frame: ConsoleFrame) => void;
}

type Severity = "critical" | "warning" | "info";

interface Signal {
  id: string;
  severity: Severity;
  label: string;
  detail: string;
  agent: string;
  at: string;
  raw: ConsoleFrame;
}

interface SignalGroup {
  id: string;
  severity: Severity;
  title: string;
  detail: string;
  agent: string;
  at: string;
  items: Signal[];
}

const PEER_TOOLS = new Set(["send_request", "send_message", "send_response"]);
const LOW_VALUE_REPLIES = new Set(["done", "ok", "okay", "acknowledged"]);
const LOW_VALUE_REPLY_PATTERNS = [
  /^acknowledged[.!]?\s+(i[’']?m\s+)?(online|acting as|ready|scribe|incident commander)/i,
  /^acknowledged\b[\s\S]{0,60}\bonline\b/i,
  /^[\w-]+\s+online\b/i,
  /\bonline\.?\s+ready\b/i,
  /\b(is|am)\s+online\s+(as|for)\b/i,
  /\bwill\s+(coordinate|maintain|focus|act|draft)\b/i,
];

function recordOf(value: unknown): Record<string, unknown> {
  return value && typeof value === "object" ? value as Record<string, unknown> : {};
}

function textFromValue(value: unknown): string {
  if (typeof value === "string") {
    const trimmed = value.trim();
    if (!trimmed) return "";
    try {
      return textFromValue(JSON.parse(trimmed)) || trimmed;
    } catch {
      return trimmed;
    }
  }
  if (Array.isArray(value)) {
    return value.map(textFromValue).filter(Boolean).join(" ").trim();
  }
  if (value && typeof value === "object") {
    const record = value as Record<string, unknown>;
    const direct =
      record.summary ?? record.message ?? record.text ?? record.body ?? record.reply ??
      record.result ?? record.content ?? record.subject ?? record.request_subject ??
      record.prompt ?? record.description ?? record.token;
    const text = textFromValue(direct);
    if (text) return text;
  }
  return "";
}

function truncate(value: string, max = 110): string {
  const normalized = value
    .replace(/\*\*([^*]+)\*\*/g, "$1")
    .replace(/`([^`]+)`/g, "$1")
    .replace(/\s+/g, " ")
    .trim();
  if (normalized.length <= max) return normalized;
  return `${normalized.slice(0, Math.max(0, max - 1)).trimEnd()}...`;
}

function displayName(value: string): string {
  if (!value || value === "_system") return "System";
  return value
    .split(/[-_\s]+/)
    .filter(Boolean)
    .map((part) => `${part.slice(0, 1).toUpperCase()}${part.slice(1)}`)
    .join(" ");
}

function isMeaningfulReply(value: string): boolean {
  const normalized = value.trim().replace(/[.!]+$/g, "").toLowerCase();
  if (!normalized || LOW_VALUE_REPLIES.has(normalized)) return false;
  return !LOW_VALUE_REPLY_PATTERNS.some((pattern) => pattern.test(value.trim()));
}

function lastSegment(value: string): string {
  return value.split("/").pop() || value;
}

function agentFor(frame: ConsoleFrame): string {
  return frame.identity?.trim() || "_system";
}

function peerTarget(args: Record<string, unknown>): string {
  if (typeof args.display_name === "string" && args.display_name.trim()) {
    return lastSegment(args.display_name.trim());
  }
  if (typeof args.to === "string" && args.to.trim()) {
    return lastSegment(args.to.trim());
  }
  return "peer";
}

function isScaffoldRequest(value: string): boolean {
  return /^You have been spawned as\b/i.test(value.trim());
}

function blobKey(frame: ConsoleFrame): string {
  const data = recordOf(frame.data);
  const image = recordOf(data.image);
  const blobRef = recordOf(image.blob_ref ?? data.blob_ref);
  const blobId = typeof blobRef.blob_id === "string"
    ? blobRef.blob_id
    : typeof data.blob_id === "string"
      ? data.blob_id
      : "";
  const imageId = typeof image.image_id === "string"
    ? image.image_id
    : typeof data.image_id === "string"
      ? data.image_id
      : "";
  return blobId || imageId || frame.interactionId || frame.id;
}

function severityOf(frame: ConsoleFrame): Severity {
  const ev = frame.event;
  if (ev.includes("fail") || ev.includes("error") || ev.includes("crash")) return "critical";
  if (ev === "gating_decision" || ev.includes("warn") || ev.includes("degraded") || ev.includes("retired")) return "warning";
  return "info";
}

function signalFromFrame(frame: ConsoleFrame): Signal | null {
  const data = recordOf(frame.data);
  const severity = severityOf(frame);
  const base = {
    id: frame.id || `${frame.event}:${frame.timestampMs || 0}`,
    severity,
    agent: agentFor(frame),
    at: timeFor(frame.timestampMs),
    raw: frame,
  };

  if (severity === "critical") {
    return {
      ...base,
      label: frame.event === "interaction_failed" ? "Agent turn failed" : frame.event.replace(/_/g, " "),
      detail: truncate(textFromValue(data.error ?? data.reason ?? data.message) || "Needs attention"),
    };
  }

  switch (frame.event) {
    case "user_input":
    case "interaction_started": {
      const request = textFromValue(data.content ?? data.text ?? data.prompt);
      if (!request) return null;
      if (isScaffoldRequest(request)) return null;
      return {
        ...base,
        id: `user:${frame.id || frame.interactionId || frame.timestampMs || request}`,
        label: `You asked ${displayName(base.agent)}`,
        detail: truncate(request),
      };
    }
    case "interaction_complete": {
      const reply = textFromValue(data.result ?? data.text ?? data.content);
      if (!isMeaningfulReply(reply)) return null;
      return {
        ...base,
        label: `${displayName(base.agent)} replied`,
        detail: truncate(reply),
      };
    }
    case "assistant_image":
    case "assistant_image_appended": {
      return {
        ...base,
        id: `image:${blobKey(frame)}`,
        label: `${displayName(base.agent)} generated image`,
        detail: textFromValue(data.prompt ?? recordOf(data.image).prompt ?? recordOf(data.image).alt) || "Generated image attached",
      };
    }
    case "tool_call_requested": {
      const name = typeof data.name === "string" ? data.name : "";
      if (!PEER_TOOLS.has(name)) return null;
      const args = recordOf(data.args);
      const target = peerTarget(args);
      const body = textFromValue(args.body ?? args.params ?? args.result) || textFromValue(args.intent);
      const verb = name === "send_request"
        ? "asked"
        : name === "send_response"
          ? "replied to"
          : "sent to";
      return {
        ...base,
        id: `peer:${frame.id || frame.interactionId || `${target}:${body}`}`,
        label: `${displayName(base.agent)} ${verb} ${displayName(target)}`,
        detail: truncate(body || "Peer comms"),
      };
    }
    case "gating_decision":
      return {
        ...base,
        label: `Gate ${String(data.decision || "decision")}`,
        detail: truncate(textFromValue(data.reason) || "Gating decision recorded"),
      };
    case "member_retired":
      return { ...base, label: "Member retired", detail: truncate(textFromValue(data.reason) || "Lifecycle change") };
    case "state_changed":
      return { ...base, label: `State -> ${String(data.state || data.new_state || "changed")}`, detail: base.agent };
    case "route_changed":
      return { ...base, label: "Route changed", detail: truncate(textFromValue(data.reason) || "Routing updated") };
    default:
      return null;
  }
}

function groupKeyFor(signal: Signal): string {
  const interactionId = signal.raw.interactionId?.trim();
  if (interactionId) return `interaction:${interactionId}`;
  return `single:${signal.id}`;
}

function strongerSeverity(a: Severity, b: Severity): Severity {
  if (a === "critical" || b === "critical") return "critical";
  if (a === "warning" || b === "warning") return "warning";
  return "info";
}

function groupSignals(signals: Signal[]): SignalGroup[] {
  const groups: SignalGroup[] = [];
  for (const signal of signals) {
    const key = groupKeyFor(signal);
    const current = groups[groups.length - 1];
    if (current && current.id === key) {
      current.items.push(signal);
      current.severity = strongerSeverity(current.severity, signal.severity);
      current.title = titleForGroup(current.items);
      current.detail = detailForGroup(current.items);
      current.agent = current.items[0]?.agent || signal.agent;
      current.at = current.items[0]?.at || signal.at;
      continue;
    }
    groups.push({
      id: key,
      severity: signal.severity,
      title: titleForGroup([signal]),
      detail: detailForGroup([signal]),
      agent: signal.agent,
      at: signal.at,
      items: [signal],
    });
  }
  return groups;
}

function titleForGroup(items: Signal[]): string {
  if (items.length === 1) return items[0].label;
  const hasUser = items.some((item) => item.raw.event === "user_input" || item.raw.event === "interaction_started");
  const peerCount = items.filter((item) => item.raw.event === "tool_call_requested").length;
  const replyCount = items.filter((item) => item.raw.event === "interaction_complete").length;
  if (hasUser && (peerCount > 0 || replyCount > 0)) return "Turn activity";
  if (peerCount > 1) return "Peer conversation";
  return `${items.length} related events`;
}

function detailForGroup(items: Signal[]): string {
  if (items.length === 1) return items[0].detail;
  const newestReply = items.find((item) => item.raw.event === "interaction_complete");
  const userRequest = items.find((item) => item.raw.event === "user_input" || item.raw.event === "interaction_started");
  return newestReply?.detail || userRequest?.detail || items[0]?.detail || "";
}

function timeFor(tsMs?: number): string {
  if (!tsMs) return "--";
  const diff = Date.now() - tsMs;
  if (diff < 60_000) return `${Math.max(1, Math.floor(diff / 1000))}s`;
  if (diff < 3_600_000) return `${Math.floor(diff / 60_000)}m`;
  return `${Math.floor(diff / 3_600_000)}h`;
}

export function SignalsRail({ frames, collapsed, onSelect }: SignalsRailProps): React.JSX.Element {
  const [filter, setFilter] = React.useState<"all" | "warning" | "critical">("all");
  const [expandedGroups, setExpandedGroups] = React.useState<Set<string>>(() => new Set());

  const groups: SignalGroup[] = React.useMemo(() => {
    const seen = new Set<string>();
    const next: Signal[] = [];
    for (const frame of frames.slice(0, 260)) {
      const signal = signalFromFrame(frame);
      if (!signal) continue;
      if (seen.has(signal.id)) continue;
      seen.add(signal.id);
      next.push(signal);
      if (next.length >= 80) break;
    }
    return groupSignals(next);
  }, [frames]);

  const counts = React.useMemo(() => ({
    all: groups.length,
    critical: groups.filter((s) => s.severity === "critical").length,
    warning: groups.filter((s) => s.severity === "warning").length,
  }), [groups]);

  const shown = groups.filter((s) =>
    filter === "all" ? true :
    filter === "critical" ? s.severity === "critical" :
    s.severity !== "info",
  );

  const recent15m = groups.filter((s) => (Date.now() - (s.items[0]?.raw.timestampMs || 0)) < 15 * 60 * 1000).length;

  function toggleGroup(group: SignalGroup): void {
    if (group.items.length <= 1) {
      onSelect?.(group.items[0].raw);
      return;
    }
    setExpandedGroups((prev) => {
      const next = new Set(prev);
      if (next.has(group.id)) next.delete(group.id);
      else next.add(group.id);
      return next;
    });
  }

  if (collapsed) {
    return (
      <aside
        className="rail rail--collapsed"
        data-collapsed="true"
        data-testid="signals-rail"
      >
        <i className="rail__grip" aria-hidden="true" />
      </aside>
    );
  }

  return (
    <aside className="rail" data-testid="signals-rail">
      <div className="rail__head">
        <span className="rail__title">Signals</span>
        <span className="rail__sub">{recent15m} in 15m</span>
      </div>
      <div className="rail__filters">
        <button
          className={`rail__filter ${filter === "all" ? "is-active" : ""}`}
          onClick={() => setFilter("all")}
          data-testid="signals-filter:all"
        >
          All <span className="rail__filter-count">{counts.all}</span>
        </button>
        <button
          className={`rail__filter ${filter === "warning" ? "is-active" : ""}`}
          onClick={() => setFilter("warning")}
          data-testid="signals-filter:warning"
        >
          Attn <span className="rail__filter-count">{counts.warning + counts.critical}</span>
        </button>
        <button
          className={`rail__filter ${filter === "critical" ? "is-active" : ""}`}
          onClick={() => setFilter("critical")}
          data-testid="signals-filter:critical"
        >
          Crit <span className="rail__filter-count">{counts.critical}</span>
        </button>
      </div>
      <div className="rail__list">
        {shown.length === 0 && (
          <div className="rail__empty">
            No meaningful signals yet.
          </div>
        )}
        {shown.map((s) => {
          const expanded = expandedGroups.has(s.id);
          return (
          <div
            key={s.id}
            className="signal"
            data-sev={s.severity}
            data-testid={`signal:${s.id}`}
            data-expanded={expanded ? "true" : "false"}
            onClick={() => toggleGroup(s)}
            role="button"
            tabIndex={0}
          >
            <span className="signal__bar" />
            <span className="signal__body">
              <span className="signal__label">
                {s.items.length > 1 && <span className="signal__chevron">{expanded ? "▾" : "▸"}</span>}
                {s.title}
                {s.items.length > 1 && <span className="signal__count">{s.items.length}</span>}
              </span>
              <span className="signal__detail">{s.detail}</span>
              <span className="signal__agent">{s.agent}</span>
              {s.items.length > 1 && expanded && (
                <span className="signal__events">
                  {s.items.map((item) => (
                    <button
                      key={item.id}
                      className="signal__event"
                      type="button"
                      onClick={(event) => {
                        event.stopPropagation();
                        onSelect?.(item.raw);
                      }}
                    >
                      <span className="signal__event-label">{item.label}</span>
                      <span className="signal__event-detail">{item.detail}</span>
                    </button>
                  ))}
                </span>
              )}
            </span>
            <span className="signal__meta">
              <span className="signal__time">{s.at}</span>
            </span>
          </div>
        );})}
      </div>
    </aside>
  );
}
