import React from "react";
import type { ConsoleFrame } from "../types";

interface SignalsRailProps {
  frames: ConsoleFrame[];
  collapsed: boolean;
  onToggleCollapsed: () => void;
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

function severityOf(frame: ConsoleFrame): Severity {
  const ev = frame.event;
  if (ev.includes("fail") || ev.includes("error") || ev.includes("crash")) return "critical";
  if (ev === "gating_decision" || ev.includes("warn") || ev.includes("degraded") || ev.includes("retired")) return "warning";
  return "info";
}

function labelFor(frame: ConsoleFrame): string {
  const ev = frame.event;
  const d = (frame.data || {}) as Record<string, unknown>;
  switch (ev) {
    case "interaction_complete": return "Interaction complete";
    case "interaction_failed":   return "Interaction failed";
    case "interaction_started":  return "Interaction started";
    case "gating_decision":      return `Gate ${String(d.decision || "")}`;
    case "member_ready":         return "Member ready";
    case "member_retired":       return "Member retired";
    case "state_changed":        return `State → ${String(d.state || d.new_state || "")}`;
    case "route_changed":        return "Route changed";
    case "run_failed":           return "Run failed";
    default: return ev.replace(/_/g, " ");
  }
}

function detailFor(frame: ConsoleFrame): string {
  const d = (frame.data || {}) as Record<string, unknown>;
  const bits: string[] = [];
  if (d.action_id) bits.push(`action=${String(d.action_id)}`);
  if (d.reason) bits.push(String(d.reason));
  if (d.error) bits.push(String(d.error));
  if (frame.interactionId) bits.push(`int=${frame.interactionId.slice(0, 8)}`);
  return bits.join(" · ") || "—";
}

function timeFor(tsMs?: number): string {
  if (!tsMs) return "—";
  const diff = Date.now() - tsMs;
  if (diff < 60_000) return `${Math.max(1, Math.floor(diff / 1000))}s`;
  if (diff < 3_600_000) return `${Math.floor(diff / 60_000)}m`;
  return `${Math.floor(diff / 3_600_000)}h`;
}

export function SignalsRail({ frames, collapsed, onToggleCollapsed, onSelect }: SignalsRailProps): React.JSX.Element {
  const [filter, setFilter] = React.useState<"all" | "warning" | "critical">("all");

  const signals: Signal[] = React.useMemo(() => {
    return frames.slice(0, 200).map((f, i) => ({
      id: f.id || `${f.event}-${i}`,
      severity: severityOf(f),
      label: labelFor(f),
      detail: detailFor(f),
      agent: f.identity || "_system",
      at: timeFor(f.timestampMs),
      raw: f,
    }));
  }, [frames]);

  const counts = React.useMemo(() => ({
    all: signals.length,
    critical: signals.filter((s) => s.severity === "critical").length,
    warning: signals.filter((s) => s.severity === "warning").length,
  }), [signals]);

  const shown = signals.filter((s) =>
    filter === "all" ? true :
    filter === "critical" ? s.severity === "critical" :
    s.severity !== "info",
  );

  const recent15m = signals.filter((s) => (Date.now() - (s.raw.timestampMs || 0)) < 15 * 60 * 1000).length;

  if (collapsed) {
    return (
      <aside
        className="rail rail--collapsed"
        data-collapsed="true"
        data-testid="signals-rail"
      >
        <button
          type="button"
          className="rail__collapse"
          aria-label="Expand signals rail"
          title="Expand signals rail"
          onClick={onToggleCollapsed}
          data-testid="signals-rail-collapse-toggle"
        >
          ‹
        </button>
      </aside>
    );
  }

  return (
    <aside className="rail" data-testid="signals-rail">
      <button
        type="button"
        className="rail__collapse"
        aria-label="Collapse signals rail"
        title="Collapse signals rail"
        onClick={onToggleCollapsed}
        data-testid="signals-rail-collapse-toggle"
      >
        ›
      </button>
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
          <div style={{ padding: "20px 14px", color: "var(--ink-dim)", fontSize: 12, fontFamily: "var(--mono)" }}>
            No signals.
          </div>
        )}
        {shown.map((s) => (
          <div
            key={s.id}
            className="signal"
            data-sev={s.severity}
            data-testid={`signal:${s.id}`}
            onClick={() => onSelect?.(s.raw)}
            role={onSelect ? "button" : undefined}
            tabIndex={onSelect ? 0 : undefined}
          >
            <span className="signal__bar" />
            <span className="signal__body">
              <span className="signal__label">{s.label}</span>
              <span className="signal__detail">{s.detail}</span>
              <span className="signal__agent">{s.agent}</span>
            </span>
            <span className="signal__meta">
              <span className="signal__time">{s.at}</span>
            </span>
          </div>
        ))}
      </div>
    </aside>
  );
}
