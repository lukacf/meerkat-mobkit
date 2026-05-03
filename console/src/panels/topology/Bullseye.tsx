// Bullseye layout — concentric degree-ranked rings. Hubs (highest degree)
// land in ring 0 at the centre; leaves (degree 0/1) settle on the outer
// ring. Within a ring nodes sort by role then id for visual stability so
// the same graph always lays out the same way.
//
// Real pulses dot along edges; the source halo on the receiver is the
// only ambient motion.

import React from "react";
import {
  buildGraph,
  colourForRole,
  edgeKey,
  roleIndexFor,
  useTopologyActivity,
  type TopoAgent,
} from "./data";
import type { ConsoleAgent, ConsoleFrame, ConsoleTopologyNode } from "../../types";

interface BullseyeProps {
  nodes: ConsoleTopologyNode[];
  agents: ConsoleAgent[];
  activity: ConsoleFrame[];
  width: number;
  height: number;
}

const RINGS = 5;

interface Pos { x: number; y: number; ringIdx: number }

function layout(graph: ReturnType<typeof buildGraph>, width: number, height: number) {
  const cx = width / 2;
  const cy = height / 2;
  const sorted = graph.agents.slice().sort((a, b) =>
    (graph.degree[b.id] || 0) - (graph.degree[a.id] || 0)
  );
  const maxDeg = sorted.length ? (graph.degree[sorted[0].id] || 1) : 1;
  const buckets: TopoAgent[][] = Array.from({ length: RINGS }, () => []);
  for (const a of sorted) {
    const d = graph.degree[a.id] || 0;
    const t = d / Math.max(1, maxDeg);
    // Concave mapping (t^0.6) so the bulk of low-degree nodes don't all
    // collide on the outermost ring; gives heavy-tailed graphs more
    // breathing room across the middle rings.
    const ringIdx = Math.min(RINGS - 1, Math.floor((1 - Math.pow(t, 0.6)) * RINGS));
    buckets[ringIdx].push(a);
  }
  const minR = Math.min(width, height) * 0.08;
  const maxR = Math.min(width, height) * 0.46;
  const ringR = (i: number) => minR + (i / Math.max(1, RINGS - 1)) * (maxR - minR);
  const pos: Record<string, Pos> = {};
  buckets.forEach((list, ri) => {
    list.sort((a, b) => a.role.localeCompare(b.role) || a.id.localeCompare(b.id));
    const r = ringR(ri);
    list.forEach((a, i) => {
      const t = (i / Math.max(1, list.length)) * Math.PI * 2 - Math.PI / 2;
      pos[a.id] = { x: cx + Math.cos(t) * r, y: cy + Math.sin(t) * r, ringIdx: ri };
    });
  });
  return { pos, ringR, cx, cy, buckets };
}

export function Bullseye({
  nodes,
  agents,
  activity,
  width,
  height,
}: BullseyeProps): React.JSX.Element {
  const graph = React.useMemo(() => buildGraph(nodes, agents), [nodes, agents]);
  const roleIndex = React.useMemo(() => roleIndexFor(graph.roles), [graph.roles]);
  const live = useTopologyActivity(activity, graph, { life: 1100 });
  const { pos, ringR, cx, cy, buckets } = React.useMemo(
    () => layout(graph, width, height),
    [graph, width, height],
  );

  const hotEdges = React.useMemo(() => {
    const set = new Set<string>();
    for (const p of live.pulses) set.add(edgeKey(p.from, p.to));
    return set;
  }, [live.pulses]);

  return (
    <svg
      className="topo__svg-board"
      viewBox={`0 0 ${width} ${height}`}
      preserveAspectRatio="xMidYMid meet"
    >
      <g>
        {Array.from({ length: RINGS }).map((_, i) => (
          <circle
            key={i}
            cx={cx} cy={cy}
            r={ringR(i)}
            fill="none"
            stroke="var(--line)"
            strokeWidth="1"
            strokeDasharray="2 4"
            opacity="0.5"
          />
        ))}
      </g>
      <g>
        {buckets.map((list, i) => {
          if (list.length === 0) return null;
          const label = i === 0 ? "hubs" : i === RINGS - 1 ? "leaves" : `r${i}`;
          return (
            <text
              key={i}
              x={cx + ringR(i)}
              y={cy - 4}
              className="topo__ring-label"
            >
              {label} · {list.length}
            </text>
          );
        })}
      </g>
      <g>
        {graph.edges.map((e, i) => {
          const a = pos[e.from];
          const b = pos[e.to];
          if (!a || !b) return null;
          const hot = hotEdges.has(edgeKey(e.from, e.to));
          return (
            <line
              key={i}
              x1={a.x} y1={a.y}
              x2={b.x} y2={b.y}
              stroke={hot ? "var(--ok)" : "var(--ink-faint)"}
              strokeWidth={hot ? 0.9 : 0.5}
              opacity={hot ? 0.85 : 0.28}
            />
          );
        })}
      </g>
      <g>
        {live.pulses.map((p) => {
          const a = pos[p.from];
          const b = pos[p.to];
          if (!a || !b) return null;
          const age = (Date.now() - p.ts) / 1100;
          if (age > 1) return null;
          const x = a.x + (b.x - a.x) * age;
          const y = a.y + (b.y - a.y) * age;
          return (
            <circle
              key={p.id}
              cx={x} cy={y} r={2.6}
              fill="var(--ok)"
              opacity={1 - age}
              style={{ pointerEvents: "none" }}
            />
          );
        })}
      </g>
      <g>
        {graph.agents.map((agent) => {
          const p = pos[agent.id];
          if (!p) return null;
          const deg = graph.degree[agent.id] || 0;
          const r = Math.max(2.2, Math.min(8, 1.6 + Math.sqrt(deg) * 1.3));
          const isHot = !!live.active[agent.id];
          const colour = colourForRole(agent.role, roleIndex);
          return (
            <g
              key={agent.id}
              transform={`translate(${p.x},${p.y})`}
              data-testid={`topology-node:${agent.id}`}
            >
              <title>{`${agent.label} · ${agent.role} · degree ${deg}${agent.state ? " · " + agent.state : ""}`}</title>
              {isHot && (
                <circle r={r + 4} fill="none" stroke={colour} strokeWidth="1" opacity="0.45" style={{ pointerEvents: "none" }} />
              )}
              <circle r={r} fill={colour} stroke="var(--bg)" strokeWidth="0.8" />
            </g>
          );
        })}
      </g>
    </svg>
  );
}
