import React from "react";
import type { ConsoleAgent, ConsoleTopologyNode } from "../types";

interface TopologyPanelProps {
  nodes: ConsoleTopologyNode[];
  agents: ConsoleAgent[];
}

interface LaidOutNode {
  x: number;
  y: number;
  r: number;
}

function normalize(id: string): string {
  return (id || "").trim();
}

function nodeColor(state?: string, profile?: string): string {
  if (state === "degraded") return "var(--warn)";
  if (state === "retired" || state === "stopped") return "var(--ink-faint)";
  const p = (profile || "").toLowerCase();
  if (p.includes("coordinat") || p.includes("triage") || p.includes("router")) return "var(--accent)";
  if (p.includes("personal") || p.includes("lead") || p.includes("user")) return "var(--focus)";
  return "var(--ink-muted)";
}

function layOutNodes(keys: string[], width: number, height: number): Record<string, LaidOutNode> {
  const out: Record<string, LaidOutNode> = {};
  if (keys.length === 0) return out;

  const cx = width / 2;
  const cy = height / 2;
  const maxR = Math.min(width, height) / 2 - 80;
  const n = keys.length;

  if (n === 1) {
    out[keys[0]] = { x: cx, y: cy, r: 26 };
    return out;
  }

  const useRings = n > 6;
  if (!useRings) {
    keys.forEach((key, i) => {
      const theta = (i / n) * Math.PI * 2 - Math.PI / 2;
      out[key] = {
        x: cx + maxR * Math.cos(theta),
        y: cy + maxR * Math.sin(theta),
        r: 22,
      };
    });
    return out;
  }

  const innerCount = Math.min(3, Math.ceil(n / 3));
  const inner = keys.slice(0, innerCount);
  const outer = keys.slice(innerCount);
  const innerR = Math.min(maxR * 0.4, 90);
  const outerR = maxR;

  inner.forEach((key, i) => {
    const theta = (i / Math.max(inner.length, 1)) * Math.PI * 2 - Math.PI / 2;
    out[key] = {
      x: cx + innerR * Math.cos(theta),
      y: cy + innerR * Math.sin(theta),
      r: 24,
    };
  });
  outer.forEach((key, i) => {
    const theta = (i / Math.max(outer.length, 1)) * Math.PI * 2 - Math.PI / 2 + Math.PI / outer.length;
    out[key] = {
      x: cx + outerR * Math.cos(theta),
      y: cy + outerR * Math.sin(theta),
      r: 20,
    };
  });
  return out;
}

export function TopologyPanel({ nodes, agents }: TopologyPanelProps): React.JSX.Element {
  const width = 880;
  const height = 520;

  const nodeList = nodes.length > 0
    ? nodes
    : agents.map<ConsoleTopologyNode>((a) => ({
        identity: a.identity || a.member_id,
        label: a.label,
        profile: a.profile,
        state: a.state,
        wired_to: a.wired_to,
      }));

  const keys = nodeList.map((n) => normalize(n.identity || n.label || "")).filter(Boolean);
  const positions = React.useMemo(() => layOutNodes(keys, width, height), [keys.join("|")]);

  const edges: Array<{ from: string; to: string }> = [];
  nodeList.forEach((n) => {
    const fromKey = normalize(n.identity || n.label || "");
    (n.wired_to || []).forEach((t) => {
      const toKey = normalize(t);
      if (positions[fromKey] && positions[toKey]) {
        edges.push({ from: fromKey, to: toKey });
      }
    });
  });

  const flows: Array<{ from: string; to: string; color: string; dur: number; delay: number }> = [];
  const maxFlows = Math.min(edges.length, 4);
  const colors = ["var(--accent)", "var(--warn)", "var(--focus)", "var(--crit)"];
  for (let i = 0; i < maxFlows; i++) {
    const step = Math.max(1, Math.floor(edges.length / maxFlows));
    const e = edges[i * step] || edges[i];
    if (!e) continue;
    flows.push({
      from: e.from,
      to: e.to,
      color: colors[i % colors.length],
      dur: 1.8 + (i * 0.3),
      delay: i * 0.5,
    });
  }

  return (
    <div className="topo" data-testid="topology-panel">
      <div className="topo__head">
        <h2>Topology</h2>
        <p>{nodeList.length} nodes · {edges.length} edges · live</p>
      </div>
      <svg
        className="topo__svg"
        viewBox={`0 0 ${width} ${height}`}
        preserveAspectRatio="xMidYMid meet"
      >
        <defs>
          <marker id="topo-arr" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto">
            <path d="M 0 0 L 10 5 L 0 10 Z" fill="currentColor" opacity="0.5" />
          </marker>
        </defs>

        <g fill="none" strokeWidth="1" style={{ color: "var(--ink-dim)" }}>
          {edges.map((e, i) => {
            const a = positions[e.from];
            const b = positions[e.to];
            if (!a || !b) return null;
            return (
              <line
                key={`edge-${i}`}
                x1={a.x} y1={a.y}
                x2={b.x} y2={b.y}
                stroke="var(--line-strong)"
                markerEnd="url(#topo-arr)"
              />
            );
          })}
        </g>

        {flows.map((f, i) => {
          const a = positions[f.from];
          const b = positions[f.to];
          if (!a || !b) return null;
          return (
            <circle key={`flow-${i}`} r="3.5" fill={f.color}>
              <animate attributeName="cx" values={`${a.x};${b.x}`} dur={`${f.dur}s`} begin={`${f.delay}s`} repeatCount="indefinite" />
              <animate attributeName="cy" values={`${a.y};${b.y}`} dur={`${f.dur}s`} begin={`${f.delay}s`} repeatCount="indefinite" />
              <animate attributeName="opacity" values="0;1;1;0" dur={`${f.dur}s`} begin={`${f.delay}s`} repeatCount="indefinite" />
            </circle>
          );
        })}

        {nodeList.map((n) => {
          const key = normalize(n.identity || n.label || "");
          const pos = positions[key];
          if (!pos) return null;
          const color = nodeColor(n.state, n.profile);
          const isActive = (n.state || "").toLowerCase() === "active" || (n.state || "").toLowerCase() === "running";
          return (
            <g
              key={key}
              transform={`translate(${pos.x},${pos.y})`}
              data-testid={`topology-node:${key}`}
            >
              <circle r={pos.r} fill="var(--bg-elev-2)" stroke={color} strokeWidth="1.5" />
              {isActive && (
                <circle r={pos.r} fill="none" stroke={color} strokeWidth="1" opacity="0.3">
                  <animate attributeName="r" values={`${pos.r};${pos.r + 8}`} dur="2.4s" repeatCount="indefinite" />
                  <animate attributeName="opacity" values="0.3;0" dur="2.4s" repeatCount="indefinite" />
                </circle>
              )}
              <text y={-pos.r - 8} textAnchor="middle" fontSize="11" fontWeight="500" fill="var(--ink)" fontFamily="var(--disp)">
                {n.label || n.identity || "unknown"}
              </text>
              <text y={pos.r + 14} textAnchor="middle" fontSize="9.5" fill="var(--ink-dim)" fontFamily="var(--mono)">
                {n.identity || ""}
              </text>
            </g>
          );
        })}
      </svg>
      <div className="topo__legend">
        <div className="topo__legend-item"><span className="topo__legend-dot" style={{ background: "var(--focus)" }} /> Personal</div>
        <div className="topo__legend-item"><span className="topo__legend-dot" style={{ background: "var(--accent)" }} /> Coordinator</div>
        <div className="topo__legend-item"><span className="topo__legend-dot" style={{ background: "var(--ink-muted)" }} /> Domain / internal</div>
        <div className="topo__legend-item"><span className="topo__legend-dot" style={{ background: "var(--warn)" }} /> Degraded</div>
        <div className="topo__legend-item"><span className="topo__legend-dot" style={{ background: "var(--ink-faint)" }} /> Retired</div>
      </div>
    </div>
  );
}
