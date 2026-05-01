import React from "react";
import type { ConsoleAgent, ConsoleTopologyNode } from "../types";

interface TopologyPanelProps {
  nodes: ConsoleTopologyNode[];
  agents: ConsoleAgent[];
}

type Kind =
  | "personal"
  | "coordinator"
  | "domain"
  | "internal"
  | "channel"
  | "initiative";

interface LayoutNode {
  id: string;
  label: string;
  identity: string;
  kind: Kind;
  state: string;
  x: number;
  y: number;
  vx: number;
  vy: number;
}

interface LayoutEdge {
  from: string;
  to: string;
}

const KIND_COLOR: Record<Kind, string> = {
  personal: "var(--focus)",
  coordinator: "var(--accent)",
  domain: "var(--ink-muted)",
  internal: "var(--ink-dim)",
  channel: "var(--ok)",
  initiative: "var(--warn)",
};

const KIND_LABEL: Record<Kind, string> = {
  personal: "Personal",
  coordinator: "Coordinator",
  domain: "Domain",
  internal: "Internal",
  channel: "Channel",
  initiative: "Initiative",
};

/// Identify a node's kind from its prefix (`personal:`, `channel:`,
/// `initiative:`) or, for un-prefixed agent identities, from the
/// `role` heuristic the existing console code uses elsewhere. The
/// prefix-based path matches what the production deployment uses;
/// the role-based fallback covers the example fixtures.
function classify(identity: string, role: string | undefined): Kind {
  const id = identity.toLowerCase();
  if (id.startsWith("personal:") || id.includes("@")) return "personal";
  if (id.startsWith("channel:") || id.startsWith("#")) return "channel";
  if (id.startsWith("initiative:")) return "initiative";
  const r = (role || "").toLowerCase();
  if (r.includes("coord") || r.includes("triage") || r.includes("router") || r.includes("commander") || r.includes("scribe")) return "coordinator";
  if (r.includes("gate") || r.includes("monitor") || r.includes("internal") || r.includes("approval") || r.includes("health")) return "internal";
  return "domain";
}

/// Strip the prefix and tighten long identities into something that
/// fits below a node (`personal:luka.crnkovicfriis@king.com` →
/// `luka.crnkovicfriis`). Full identity lives in the tooltip.
function shortLabel(label: string, identity: string): string {
  const explicit = label && label !== identity ? label : "";
  const candidate = explicit || identity;
  const stripped = candidate
    .replace(/^personal:/i, "")
    .replace(/^channel:/i, "")
    .replace(/^initiative:/i, "")
    .replace(/^#/, "");
  const noEmail = stripped.replace(/@[^@]+$/, "");
  if (noEmail.length <= 24) return noEmail;
  return noEmail.slice(0, 22) + "…";
}

/// Anchor point per kind, in unit space (0..1). The force simulation
/// pulls each node toward its kind's anchor; same-kind nodes cluster,
/// different kinds separate. Geometry chosen so the grid reads
/// roughly: people on the left, agents in the centre, structures on
/// the right.
const ANCHORS: Record<Kind, { x: number; y: number }> = {
  personal:    { x: 0.12, y: 0.50 },
  coordinator: { x: 0.40, y: 0.30 },
  domain:      { x: 0.50, y: 0.55 },
  internal:    { x: 0.40, y: 0.82 },
  channel:     { x: 0.78, y: 0.30 },
  initiative:  { x: 0.84, y: 0.62 },
};

/// Tiny 2-D force simulator. We don't pull in a library; ~30 nodes
/// over ~250 ticks runs in microseconds. The forces are:
///   - cluster gravity: each node pulled toward its kind's anchor
///   - edge spring: connected nodes attract
///   - mutual repulsion: prevents pile-up
///   - boundary damping: keep everyone in the canvas
function simulate(
  nodes: LayoutNode[],
  edges: LayoutEdge[],
  width: number,
  height: number,
): void {
  const idIndex = new Map<string, number>();
  nodes.forEach((n, i) => idIndex.set(n.id, i));

  // Seed positions on each kind's anchor with a small jitter so the
  // first tick has a non-degenerate gradient.
  nodes.forEach((n) => {
    const a = ANCHORS[n.kind];
    n.x = a.x * width + (Math.random() - 0.5) * 40;
    n.y = a.y * height + (Math.random() - 0.5) * 40;
    n.vx = 0;
    n.vy = 0;
  });

  const TICKS = 320;
  const REPEL_K = 4200;       // node-node repulsion strength
  const SPRING_K = 0.024;     // edge spring constant
  const SPRING_LEN = 110;     // ideal edge length
  const ANCHOR_K = 0.006;     // per-kind cluster gravity (loose)
  const DAMPING = 0.84;
  const PADDING = 60;

  for (let t = 0; t < TICKS; t++) {
    // Repulsion (O(n²), fine for 30-100 nodes).
    for (let i = 0; i < nodes.length; i++) {
      for (let j = i + 1; j < nodes.length; j++) {
        const a = nodes[i];
        const b = nodes[j];
        const dx = a.x - b.x;
        const dy = a.y - b.y;
        const dist2 = dx * dx + dy * dy + 0.01;
        const force = REPEL_K / dist2;
        const dist = Math.sqrt(dist2);
        const fx = (dx / dist) * force;
        const fy = (dy / dist) * force;
        a.vx += fx;
        a.vy += fy;
        b.vx -= fx;
        b.vy -= fy;
      }
    }

    // Edge springs.
    for (const e of edges) {
      const a = nodes[idIndex.get(e.from) ?? -1];
      const b = nodes[idIndex.get(e.to) ?? -1];
      if (!a || !b) continue;
      const dx = b.x - a.x;
      const dy = b.y - a.y;
      const dist = Math.sqrt(dx * dx + dy * dy) + 0.01;
      const force = (dist - SPRING_LEN) * SPRING_K;
      const fx = (dx / dist) * force;
      const fy = (dy / dist) * force;
      a.vx += fx;
      a.vy += fy;
      b.vx -= fx;
      b.vy -= fy;
    }

    // Cluster gravity.
    for (const n of nodes) {
      const a = ANCHORS[n.kind];
      n.vx += (a.x * width - n.x) * ANCHOR_K;
      n.vy += (a.y * height - n.y) * ANCHOR_K;
    }

    // Integrate + clamp inside the canvas.
    for (const n of nodes) {
      n.vx *= DAMPING;
      n.vy *= DAMPING;
      n.x += n.vx;
      n.y += n.vy;
      n.x = Math.max(PADDING, Math.min(width - PADDING, n.x));
      n.y = Math.max(PADDING, Math.min(height - PADDING, n.y));
    }
  }
}

function nodeColor(kind: Kind, state: string): string {
  if (state === "degraded") return "var(--warn)";
  if (state === "retired" || state === "stopped") return "var(--ink-faint)";
  return KIND_COLOR[kind];
}

export function TopologyPanel({ nodes, agents }: TopologyPanelProps): React.JSX.Element {
  const width = 980;
  const height = 580;

  const nodeList: ConsoleTopologyNode[] = nodes.length > 0
    ? nodes
    : agents.map((a) => ({
        identity: a.identity || a.member_id,
        label: a.label,
        role: a.role,
        state: a.state,
        wired_to: a.wired_to,
      }));

  // The `useMemo` key is the structural fingerprint — node IDs and
  // their wired_to lists. Force-layout is deterministic-ish; we
  // freeze on the first computation per fingerprint so live state
  // updates don't restart the simulation.
  const layout = React.useMemo(() => {
    const laid: LayoutNode[] = [];
    const byId = new Map<string, LayoutNode>();
    for (const n of nodeList) {
      const id = (n.identity || n.label || "").trim();
      if (!id) continue;
      if (byId.has(id)) continue;
      const node: LayoutNode = {
        id,
        label: shortLabel(n.label || "", id),
        identity: id,
        kind: classify(id, n.role),
        state: (n.state || "").toLowerCase(),
        x: 0, y: 0, vx: 0, vy: 0,
      };
      laid.push(node);
      byId.set(id, node);
    }
    const edges: LayoutEdge[] = [];
    for (const n of nodeList) {
      const from = (n.identity || n.label || "").trim();
      if (!from || !byId.has(from)) continue;
      for (const t of n.wired_to || []) {
        const to = t.trim();
        if (!to || !byId.has(to) || to === from) continue;
        edges.push({ from, to });
      }
    }
    simulate(laid, edges, width, height);

    // Auto-fit: tighten the viewBox around the laid-out nodes so
    // small graphs don't get lost in dead space.
    let minX = Infinity, minY = Infinity, maxX = -Infinity, maxY = -Infinity;
    for (const n of laid) {
      if (n.x < minX) minX = n.x;
      if (n.y < minY) minY = n.y;
      if (n.x > maxX) maxX = n.x;
      if (n.y > maxY) maxY = n.y;
    }
    const PAD = 80;
    const view = laid.length === 0
      ? { x: 0, y: 0, w: width, h: height }
      : {
          x: minX - PAD,
          y: minY - PAD,
          w: Math.max(maxX - minX + PAD * 2, 240),
          h: Math.max(maxY - minY + PAD * 2, 180),
        };

    return { nodes: laid, edges, view };
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [
    nodeList.map((n) => `${n.identity}:${(n.wired_to || []).join(",")}`).join("|"),
  ]);

  const [hovered, setHovered] = React.useState<string | null>(null);

  const positions = React.useMemo(() => {
    const m = new Map<string, LayoutNode>();
    for (const n of layout.nodes) m.set(n.id, n);
    return m;
  }, [layout]);

  const incidentSet = React.useMemo(() => {
    if (!hovered) return null;
    const set = new Set<string>([hovered]);
    for (const e of layout.edges) {
      if (e.from === hovered) set.add(e.to);
      if (e.to === hovered) set.add(e.from);
    }
    return set;
  }, [hovered, layout.edges]);

  const kindCounts = React.useMemo(() => {
    const c: Partial<Record<Kind, number>> = {};
    for (const n of layout.nodes) c[n.kind] = (c[n.kind] || 0) + 1;
    return c;
  }, [layout.nodes]);

  return (
    <div className="topo" data-testid="topology-panel">
      <div className="topo__head">
        <h2>Topology</h2>
        <p>{layout.nodes.length} nodes · {layout.edges.length} edges</p>
      </div>
      <svg
        className="topo__svg"
        viewBox={`${layout.view.x} ${layout.view.y} ${layout.view.w} ${layout.view.h}`}
        preserveAspectRatio="xMidYMid meet"
        onMouseLeave={() => setHovered(null)}
      >
        <defs>
          <marker id="topo-arr" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto">
            <path d="M 0 0 L 10 5 L 0 10 Z" fill="currentColor" opacity="0.4" />
          </marker>
        </defs>

        {/* Edges. Always rendered; hover dims everything except the
            edges incident on the focused node. */}
        <g fill="none" strokeWidth="1">
          {layout.edges.map((e, i) => {
            const a = positions.get(e.from);
            const b = positions.get(e.to);
            if (!a || !b) return null;
            const focused = !incidentSet
              || (incidentSet.has(e.from) && incidentSet.has(e.to));
            return (
              <line
                key={`edge-${i}`}
                x1={a.x} y1={a.y}
                x2={b.x} y2={b.y}
                stroke="var(--line-strong)"
                opacity={focused ? 0.55 : 0.08}
                style={{ transition: "opacity 120ms ease" }}
              />
            );
          })}
        </g>

        {/* Nodes. */}
        {layout.nodes.map((n) => {
          const color = nodeColor(n.kind, n.state);
          const isActive = n.state === "active" || n.state === "running";
          const focused = !incidentSet || incidentSet.has(n.id);
          const r = 10;
          return (
            <g
              key={n.id}
              transform={`translate(${n.x},${n.y})`}
              data-testid={`topology-node:${n.id}`}
              onMouseEnter={() => setHovered(n.id)}
              style={{
                cursor: "pointer",
                opacity: focused ? 1 : 0.35,
                transition: "opacity 120ms ease",
              }}
            >
              <title>{`${n.identity} · ${KIND_LABEL[n.kind]}${n.state ? ` · ${n.state}` : ""}`}</title>
              {isActive && (
                <circle
                  r={r + 4}
                  fill="none"
                  stroke={color}
                  strokeWidth="1"
                  opacity="0.22"
                  style={{ pointerEvents: "none" }}
                />
              )}
              <circle
                r={r}
                fill="var(--bg)"
                stroke={color}
                strokeWidth={hovered === n.id ? 2 : 1.5}
              />
              <text
                y={r + 14}
                textAnchor="middle"
                fontSize="11"
                fontFamily="var(--disp)"
                fill="var(--ink)"
                style={{ pointerEvents: "none" }}
              >
                {n.label}
              </text>
            </g>
          );
        })}
      </svg>
      <div className="topo__legend">
        {(Object.keys(KIND_LABEL) as Kind[])
          .filter((k) => (kindCounts[k] || 0) > 0)
          .map((k) => (
            <div key={k} className="topo__legend-item">
              <span className="topo__legend-dot" style={{ background: KIND_COLOR[k] }} />
              {KIND_LABEL[k]}
              <span className="topo__legend-count">{kindCounts[k]}</span>
            </div>
          ))}
      </div>
    </div>
  );
}
