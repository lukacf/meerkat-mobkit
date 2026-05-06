// Bullseye — concentric degree-ranked rings, hubs at the centre, leaves
// on the rim. Visual weight scales with graph size so the same layout
// reads cleanly at 8 nodes (chunky dots, labels visible) and at 240
// (tiny dots, only ring labels survive). Rings get explicit alternating
// solid/dashed strokes so they don't disappear into the background like
// 1-px hairline dashed lines do.

import React from "react";
import {
  buildGraph,
  colourForRole,
  edgeKey,
  roleIndexFor,
  useTopologyActivity,
  type TopoAgent,
} from "./data";
import { useZoomPan, viewportTransform } from "./zoom-pan";
import type { ConsoleAgent, ConsoleFrame, ConsoleTopologyNode } from "../../types";

type LabelsMode = "auto" | "on" | "off";

interface BullseyeProps {
  nodes: ConsoleTopologyNode[];
  agents: ConsoleAgent[];
  activity: ConsoleFrame[];
  width: number;
  height: number;
  labelsMode?: LabelsMode;
}

const RINGS = 5;

interface Pos { x: number; y: number; ringIdx: number }

interface VisualScale {
  nodeMin: number;
  nodeMax: number;
  edgeWidth: number;
}

function visualScale(N: number): VisualScale {
  if (N <= 20) return { nodeMin: 5, nodeMax: 12, edgeWidth: 1.0 };
  if (N <= 80) return { nodeMin: 3.5, nodeMax: 9, edgeWidth: 0.7 };
  return { nodeMin: 2.4, nodeMax: 7, edgeWidth: 0.5 };
}

function resolveLabelMode(N: number, mode: LabelsMode): "on" | "hover" {
  if (mode === "off") return "hover";
  if (mode === "on") return N <= 60 ? "on" : "hover";
  return N <= 20 ? "on" : "hover";
}

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
  const minR = Math.min(width, height) * 0.10;
  const maxR = Math.min(width, height) * 0.44;
  const ringR = (i: number) => minR + (i / Math.max(1, RINGS - 1)) * (maxR - minR);
  const pos: Record<string, Pos> = {};
  buckets.forEach((list, ri) => {
    list.sort((a, b) => a.role.localeCompare(b.role) || a.id.localeCompare(b.id));
    const r = ringR(ri);
    list.forEach((a, i) => {
      // Slight angular offset per ring so adjacent rings don't align
      // their first node at exactly 12 o'clock.
      const offset = (ri / RINGS) * (Math.PI / 6);
      const t = (i / Math.max(1, list.length)) * Math.PI * 2 - Math.PI / 2 + offset;
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
  labelsMode = "auto",
}: BullseyeProps): React.JSX.Element {
  const graph = React.useMemo(() => buildGraph(nodes, agents), [nodes, agents]);
  const roleIndex = React.useMemo(() => roleIndexFor(graph.roles), [graph.roles]);
  const live = useTopologyActivity(activity, graph, { life: 1100 });
  const scale = visualScale(graph.agents.length);
  const labelMode = resolveLabelMode(graph.agents.length, labelsMode);
  const [hoverId, setHoverId] = React.useState<string | null>(null);
  const zoom = useZoomPan(width, height);
  const { pos, ringR, cx, cy, buckets } = React.useMemo(
    () => layout(graph, width, height),
    [graph, width, height],
  );

  const hotEdges = React.useMemo(() => {
    const set = new Set<string>();
    for (const p of live.pulses) set.add(edgeKey(p.from, p.to));
    return set;
  }, [live.pulses]);

  const radiusOf = (deg: number): number => {
    const t = Math.sqrt(deg) / 4; // gentle 0..1-ish curve
    return scale.nodeMin + Math.min(1, t) * (scale.nodeMax - scale.nodeMin);
  };

  const innerR = ringR(0);

  // Pre-compute per-agent geometry once — feeds both the node circle
  // layer and the trailing label layer (so labels sit above all
  // circles, not just their own group).
  const renderItems = graph.agents.flatMap((agent) => {
    const p = pos[agent.id];
    if (!p) return [];
    const deg = graph.degree[agent.id] || 0;
    const r = radiusOf(deg);
    const dx = p.x - cx;
    const dy = p.y - cy;
    const d = Math.hypot(dx, dy) || 1;
    const ux = dx / d;
    const uy = dy / d;
    const labelX = p.x + ux * (r + 8);
    const labelY = p.y + uy * (r + 8);
    const anchor: "start" | "end" | "middle" = ux > 0.25 ? "start" : ux < -0.25 ? "end" : "middle";
    return [{
      agent, p, r, labelX, labelY, anchor,
      isHot: !!live.active[agent.id],
      isBusy: !!live.busy[agent.id],
      colour: colourForRole(agent.role, roleIndex),
    }];
  });
  const showInlineLabel = labelMode === "on";
  const hoveredItem = hoverId ? renderItems.find((it) => it.agent.id === hoverId) : null;

  return (
    <div className="topo__board">
      <div className="topo__zoombar" data-testid="topology-zoombar">
        <span className="topo__zoom-pct">{Math.round(zoom.viewport.scale * 100)}%</span>
        <button
          type="button"
          className="topo__zoom-reset"
          onClick={zoom.reset}
          title="Reset zoom and pan"
          data-testid="topology-zoom-reset"
        >
          Reset
        </button>
      </div>
      <svg
        ref={zoom.svgRef}
        className={`topo__svg-board${zoom.isDragging ? " is-panning" : ""}`}
        viewBox={`0 0 ${width} ${height}`}
        preserveAspectRatio="xMidYMid meet"
        onPointerDown={zoom.onPointerDown}
        onPointerMove={zoom.onPointerMove}
        onPointerUp={zoom.onPointerUp}
        onPointerCancel={zoom.onPointerUp}
      >
        <g transform={viewportTransform(zoom.viewport)}>
      {/* Ring guides: alternate solid + dashed so the rhythm is legible
          even with sparse populations. Both use line-strong on a low
          opacity so they read on light and dark themes. */}
      <g>
        {Array.from({ length: RINGS }).map((_, i) => (
          <circle
            key={i}
            cx={cx} cy={cy}
            r={ringR(i)}
            fill="none"
            stroke="var(--line-strong)"
            strokeWidth="1"
            strokeDasharray={i % 2 === 0 ? undefined : "3 5"}
            opacity={i === 0 ? 0.6 : 0.35}
          />
        ))}
      </g>

      {/* Ring labels: anchored to the right horizontal axis with a small
          background-stroke halo so they read across rings. Hidden for
          empty rings. */}
      <g>
        {buckets.map((list, i) => {
          if (list.length === 0) return null;
          const label = i === 0 ? "hubs" : i === RINGS - 1 ? "leaves" : `r${i}`;
          return (
            <text
              key={i}
              x={cx + ringR(i) + 6}
              y={cy + 3}
              textAnchor="start"
              className="topo__ring-label"
            >
              {label} · {list.length}
            </text>
          );
        })}
      </g>

      {/* Centre disk: shows total agent count. Reads as the visual
          anchor that the rings radiate from. */}
      <g>
        <circle
          cx={cx} cy={cy}
          r={Math.max(14, innerR - 14)}
          fill="var(--bg)"
          stroke="var(--line)"
          strokeWidth="1"
        />
        <text
          x={cx} y={cy + 4}
          textAnchor="middle"
          className="topo__center-label"
        >
          {graph.agents.length}
        </text>
      </g>

      {/* Edges: stronger stroke + higher opacity than the original
          design (which assumed dense graphs). Hot edges promote to
          accent colour. */}
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
              strokeWidth={hot ? scale.edgeWidth + 0.5 : scale.edgeWidth}
              opacity={hot ? 0.85 : 0.5}
            />
          );
        })}
      </g>

      {/* Live pulses (real activity only). */}
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
              cx={x} cy={y} r={3}
              fill="var(--ok)"
              opacity={1 - age}
              style={{ pointerEvents: "none" }}
            />
          );
        })}
      </g>

      {/* Nodes (circles, halos, busy rings) — labels live in the
          trailing layer below so they sit above all circles. */}
      <g>
        {renderItems.map(({ agent, p, r, isHot, isBusy, colour }) => (
          <g
            key={agent.id}
            data-testid={`topology-node:${agent.id}`}
            className={`topo__node${isBusy ? " is-busy" : ""}${isHot ? " is-hot" : ""}`}
            onMouseEnter={() => setHoverId(agent.id)}
            onMouseLeave={() => setHoverId((cur) => (cur === agent.id ? null : cur))}
            onFocus={() => setHoverId(agent.id)}
            onBlur={() => setHoverId((cur) => (cur === agent.id ? null : cur))}
            tabIndex={0}
          >
            {isBusy && (
              <circle
                className="topo__busy-ring"
                cx={p.x} cy={p.y}
                r={r + 6}
                fill="none"
                stroke="var(--accent)"
                strokeWidth="1.5"
                style={{ pointerEvents: "none" }}
              />
            )}
            {isHot && (
              <circle
                cx={p.x} cy={p.y}
                r={r + 4}
                fill="none"
                stroke={colour}
                strokeWidth="1"
                opacity="0.35"
                style={{ pointerEvents: "none" }}
              />
            )}
            <circle
              cx={p.x} cy={p.y}
              r={r}
              fill={colour}
              stroke="var(--bg)"
              strokeWidth="1.5"
            />
          </g>
        ))}
      </g>
      {/* Inline labels — trailing layer above all circles. */}
      {showInlineLabel && (
        <g style={{ pointerEvents: "none" }}>
          {renderItems.map(({ agent, labelX, labelY, anchor }) => (
            <text
              key={agent.id}
              x={labelX}
              y={labelY + 4}
              textAnchor={anchor}
              className="topo__node-label"
            >
              {agent.label}
            </text>
          ))}
        </g>
      )}
      {/* Hover pill — final, single layer. */}
      {!showInlineLabel && hoveredItem && (
        <BullseyeLabelPill
          x={hoveredItem.p.x}
          y={hoveredItem.p.y + hoveredItem.r + 8}
          text={hoveredItem.agent.label}
          sub={`${hoveredItem.agent.role}${hoveredItem.agent.state ? " · " + hoveredItem.agent.state : ""}${hoveredItem.isBusy ? " · working" : ""}`}
        />
      )}
        </g>
      </svg>
    </div>
  );
}

interface PillProps { x: number; y: number; text: string; sub?: string }

function BullseyeLabelPill({ x, y, text, sub }: PillProps): React.JSX.Element {
  const W = 180;
  const H = sub ? 32 : 18;
  return (
    <foreignObject
      x={x - W / 2}
      y={y}
      width={W}
      height={H}
      style={{ pointerEvents: "none", overflow: "visible" }}
    >
      <div className="topo__node-pill" role="tooltip">
        <span className="topo__node-pill-label">{text}</span>
        {sub && <span className="topo__node-pill-sub">{sub}</span>}
      </div>
    </foreignObject>
  );
}
