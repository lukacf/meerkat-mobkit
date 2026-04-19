import React from "react";
import type { ConsoleFrame } from "../types";

interface LogsPanelProps {
  frames: ConsoleFrame[];
}

type Level = "info" | "warn" | "error";

function levelFor(frame: ConsoleFrame): Level {
  const ev = frame.event;
  if (ev.includes("failed") || ev.includes("error") || ev.includes("crash")) return "error";
  if (ev.includes("warn") || ev.includes("degraded") || ev.includes("gating_decision")) return "warn";
  return "info";
}

function formatTime(tsMs?: number): string {
  if (!tsMs) return "—";
  const d = new Date(tsMs);
  const hh = String(d.getHours()).padStart(2, "0");
  const mm = String(d.getMinutes()).padStart(2, "0");
  const ss = String(d.getSeconds()).padStart(2, "0");
  const ms = String(d.getMilliseconds()).padStart(3, "0");
  return `${hh}:${mm}:${ss}.${ms}`;
}

function summary(frame: ConsoleFrame): string {
  const d = (frame.data || {}) as Record<string, unknown>;
  const bits: string[] = [];
  for (const [k, v] of Object.entries(d).slice(0, 4)) {
    if (v === null || v === undefined) continue;
    let str: string;
    if (typeof v === "object") { try { str = JSON.stringify(v).slice(0, 40); } catch { str = "[obj]"; } }
    else str = String(v).slice(0, 60);
    bits.push(`${k}=${str}`);
  }
  return bits.join(" ");
}

export function LogsPanel({ frames }: LogsPanelProps): React.JSX.Element {
  const [q, setQ] = React.useState("");
  const [lvl, setLvl] = React.useState<"all" | Level>("all");

  const rows = React.useMemo(() => {
    return frames
      .map((f) => ({ f, level: levelFor(f) }))
      .filter(({ f, level }) => {
        if (lvl !== "all" && level !== lvl) return false;
        if (!q) return true;
        const needle = q.toLowerCase();
        return (
          f.event.toLowerCase().includes(needle) ||
          (f.identity || "").toLowerCase().includes(needle)
        );
      });
  }, [frames, q, lvl]);

  const counts = React.useMemo(() => {
    const c = { info: 0, warn: 0, error: 0 };
    frames.forEach((f) => { c[levelFor(f)]++; });
    return c;
  }, [frames]);

  return (
    <div className="view logs" data-testid="logs-panel">
      <div className="view__head">
        <h2>Logs</h2>
        <span className="view__sub">{rows.length} of {frames.length} events · live</span>
        <span className="view__spacer" />
        <input
          className="view__search"
          placeholder="Filter event, identity…"
          value={q}
          onChange={(e) => setQ(e.target.value)}
        />
        <div className="view__segs">
          <button className={lvl === "all" ? "is-active" : ""} onClick={() => setLvl("all")}>
            all <span className="n">{frames.length}</span>
          </button>
          <button className={lvl === "info" ? "is-active" : ""} onClick={() => setLvl("info")}>
            info <span className="n">{counts.info}</span>
          </button>
          <button className={`warn ${lvl === "warn" ? "is-active" : ""}`} onClick={() => setLvl("warn")}>
            warn <span className="n">{counts.warn}</span>
          </button>
          <button className={`bad ${lvl === "error" ? "is-active" : ""}`} onClick={() => setLvl("error")}>
            err <span className="n">{counts.error}</span>
          </button>
        </div>
      </div>
      <div className="logs__body">
        <pre className="logs__stream">
          {rows.map(({ f, level }, i) => (
            <div key={f.id || i} className={`logline logline--${level}`} data-testid={`log-line:${f.id || i}`}>
              <span className="logline__t">{formatTime(f.timestampMs)}</span>
              <span className={`logline__lvl logline__lvl--${level}`}>{level.toUpperCase()}</span>
              <span className="logline__src">{f.identity || "_system"}</span>
              <span className="logline__evt">{f.event}</span>
              <span className="logline__ctx dim">{f.interactionId ? `int=${f.interactionId.slice(0, 8)}` : ""}</span>
              <span className="logline__msg">{summary(f)}</span>
            </div>
          ))}
          {rows.length === 0 && (
            <div style={{ padding: 24, color: "var(--ink-dim)", fontFamily: "var(--mono)", fontSize: 12 }}>
              No matching events.
            </div>
          )}
        </pre>
      </div>
    </div>
  );
}
