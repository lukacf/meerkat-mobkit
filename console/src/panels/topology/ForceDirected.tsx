// Force-directed layout — physics sim with role colour and degree-scaled
// node radius. Visual weight scales with graph size: chunky nodes +
// inline labels for small graphs, tiny dots for dense ones.
//
// Edges: real `wired_to` pairs. Real pulses dot along them when the
// activity stream carries a resolvable send_* tool call.

import React from "react";
import {
  buildGraph,
  colourForRole,
  edgeKey,
  roleIndexFor,
  useTopologyActivity,
  type TopoActivity,
} from "./data";
import type { ConsoleAgent, ConsoleFrame, ConsoleTopologyNode } from "../../types";

interface SimNode {
  id: string;
  x: number;
  y: number;
  vx: number;
  vy: number;
}

interface ForceDirectedProps {
  nodes: ConsoleTopologyNode[];
  agents: ConsoleAgent[];
  activity: ConsoleFrame[];
  width: number;
  height: number;
}

interface VisualScale {
  nodeMin: number;
  nodeMax: number;
  showLabels: boolean;
  edgeWidth: number;
  idealEdgeLen: number;
}

function visualScale(N: number): VisualScale {
  if (N <= 20) return { nodeMin: 5, nodeMax: 12, showLabels: true,  edgeWidth: 1.0, idealEdgeLen: 110 };
  if (N <= 80) return { nodeMin: 3.5, nodeMax: 9, showLabels: false, edgeWidth: 0.7, idealEdgeLen: 80 };
  return { nodeMin: 2.4, nodeMax: 7, showLabels: false, edgeWidth: 0.5, idealEdgeLen: 60 };
}

export function ForceDirected({
  nodes,
  agents,
  activity,
  width,
  height,
}: ForceDirectedProps): React.JSX.Element {
  const graph = React.useMemo(() => buildGraph(nodes, agents), [nodes, agents]);
  const roleIndex = React.useMemo(() => roleIndexFor(graph.roles), [graph.roles]);
  const liveActivity: TopoActivity = useTopologyActivity(activity, graph, { life: 900 });
  const scale = visualScale(graph.agents.length);

  // Sim state lives in a ref so React render cadence doesn't throttle
  // the integrator. We schedule a re-render every 2 frames.
  const simRef = React.useRef<{ nodes: SimNode[]; byId: Map<string, SimNode>; alpha: number; frame: number } | null>(null);
  const [, setTick] = React.useState(0);
  const fingerprint = React.useMemo(
    () => `${graph.agents.map((a) => a.id).join(",")}|${graph.edges.map((e) => `${e.from}-${e.to}`).join(",")}|${width}x${height}`,
    [graph, width, height],
  );

  React.useEffect(() => {
    const N = graph.agents.length;
    if (N === 0) {
      simRef.current = null;
      return;
    }
    // Seed nodes around centre with deterministic jitter so re-mount
    // settles to a similar layout instead of a different blob each time.
    const seeded: SimNode[] = graph.agents.map((a, i) => {
      const t = (i / Math.max(1, N)) * Math.PI * 2;
      return {
        id: a.id,
        x: width / 2 + Math.cos(t) * (50 + (i * 13) % 80),
        y: height / 2 + Math.sin(t) * (50 + (i * 7) % 80),
        vx: 0, vy: 0,
      };
    });
    const byId = new Map<string, SimNode>();
    seeded.forEach((n) => byId.set(n.id, n));
    simRef.current = { nodes: seeded, byId, alpha: 1, frame: 0 };

    let raf = 0;
    let stopped = false;
    const step = () => {
      if (stopped) return;
      const sim = simRef.current;
      if (!sim) return;
      const cx = width / 2;
      const cy = height / 2;
      const REP = Math.max(220, 70000 / sim.nodes.length);
      // Pairwise repulsion (O(N²) — fine up to ~500).
      for (let i = 0; i < sim.nodes.length; i++) {
        const ni = sim.nodes[i];
        for (let j = i + 1; j < sim.nodes.length; j++) {
          const nj = sim.nodes[j];
          const dx = ni.x - nj.x;
          const dy = ni.y - nj.y;
          const d2 = dx * dx + dy * dy + 0.01;
          const f = REP / d2;
          const d = Math.sqrt(d2);
          const ux = dx / d;
          const uy = dy / d;
          ni.vx += ux * f;
          ni.vy += uy * f;
          nj.vx -= ux * f;
          nj.vy -= uy * f;
        }
      }
      // Edge spring — ideal length scales with the graph so a small
      // graph spreads out and a dense one stays compact.
      for (const e of graph.edges) {
        const a = sim.byId.get(e.from);
        const b = sim.byId.get(e.to);
        if (!a || !b) continue;
        const dx = b.x - a.x;
        const dy = b.y - a.y;
        const d = Math.sqrt(dx * dx + dy * dy) || 0.01;
        const f = 0.025 * (d - scale.idealEdgeLen);
        const ux = dx / d;
        const uy = dy / d;
        a.vx += ux * f;
        a.vy += uy * f;
        b.vx -= ux * f;
        b.vy -= uy * f;
      }
      // Centring pull + damping.
      for (const n of sim.nodes) {
        n.vx += (cx - n.x) * 0.0035;
        n.vy += (cy - n.y) * 0.0035;
        n.vx *= 0.78;
        n.vy *= 0.78;
        n.x += n.vx * sim.alpha;
        n.y += n.vy * sim.alpha;
        const margin = 18;
        n.x = Math.max(margin, Math.min(width - margin, n.x));
        n.y = Math.max(margin, Math.min(height - margin, n.y));
      }
      sim.alpha = Math.max(0.04, sim.alpha * 0.992);
      sim.frame++;
      if (sim.frame % 2 === 0) setTick((t) => t + 1);
      raf = requestAnimationFrame(step);
    };
    raf = requestAnimationFrame(step);
    return () => {
      stopped = true;
      cancelAnimationFrame(raf);
    };
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [fingerprint]);

  const sim = simRef.current;
  const hotEdges = React.useMemo(() => {
    const set = new Set<string>();
    for (const p of liveActivity.pulses) set.add(edgeKey(p.from, p.to));
    return set;
  }, [liveActivity.pulses]);

  return (
    <svg
      className="topo__svg-board"
      viewBox={`0 0 ${width} ${height}`}
      preserveAspectRatio="xMidYMid meet"
    >
      {sim && (
        <>
          <g>
            {graph.edges.map((e, i) => {
              const a = sim.byId.get(e.from);
              const b = sim.byId.get(e.to);
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
          <g>
            {liveActivity.pulses.map((p) => {
              const a = sim.byId.get(p.from);
              const b = sim.byId.get(p.to);
              if (!a || !b) return null;
              const age = (Date.now() - p.ts) / 900;
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
          <g>
            {graph.agents.map((agent) => {
              const n = sim.byId.get(agent.id);
              if (!n) return null;
              const deg = graph.degree[agent.id] || 0;
              const t = Math.sqrt(deg) / 4;
              const r = scale.nodeMin + Math.min(1, t) * (scale.nodeMax - scale.nodeMin);
              const isHot = !!liveActivity.active[agent.id];
              const colour = colourForRole(agent.role, roleIndex);
              return (
                <g
                  key={agent.id}
                  data-testid={`topology-node:${agent.id}`}
                >
                  <title>{`${agent.label} · ${agent.role} · degree ${deg}${agent.state ? " · " + agent.state : ""}`}</title>
                  {isHot && (
                    <circle
                      cx={n.x} cy={n.y}
                      r={r + 5}
                      fill="none"
                      stroke={colour}
                      strokeWidth="1.5"
                      opacity="0.5"
                      style={{ pointerEvents: "none" }}
                    />
                  )}
                  <circle
                    cx={n.x} cy={n.y}
                    r={r}
                    fill={colour}
                    stroke="var(--bg)"
                    strokeWidth="1.5"
                  />
                  {scale.showLabels && (
                    <text
                      x={n.x}
                      y={n.y + r + 12}
                      textAnchor="middle"
                      className="topo__node-label"
                    >
                      {agent.label}
                    </text>
                  )}
                </g>
              );
            })}
          </g>
        </>
      )}
    </svg>
  );
}
