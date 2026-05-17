import React from "react";
import {
  forceCollide,
  forceLink,
  forceManyBody,
  forceSimulation,
  forceX,
  forceY,
  type SimulationLinkDatum,
  type SimulationNodeDatum,
} from "d3-force";
import {
  colourForRole,
  edgeKey,
  roleIndexFor,
  sampleEdges,
  type TopoActivity,
  type TopoAgent,
  type TopoGraph,
} from "./data";

interface DenseGraphMapProps {
  graph: TopoGraph;
  live: TopoActivity;
  selectedId?: string;
  onSelect: (id: string) => void;
}

interface LayoutNode extends SimulationNodeDatum {
  id: string;
  agent: TopoAgent;
  groupIndex: number;
}

interface LayoutLink extends SimulationLinkDatum<LayoutNode> {
  source: string | LayoutNode;
  target: string | LayoutNode;
}

interface GroupAnchor {
  name: string;
  x: number;
  y: number;
  count: number;
  colour: string;
}

interface Layout {
  nodes: LayoutNode[];
  byId: Map<string, LayoutNode>;
  links: LayoutLink[];
  groups: GroupAnchor[];
  width: number;
  height: number;
}

interface Viewport {
  scale: number;
  x: number;
  y: number;
}

const LAYOUT_EDGE_LIMIT = 3000;
const LABEL_LIMIT = 26;

function hash(value: string): number {
  let h = 2166136261;
  for (let i = 0; i < value.length; i++) {
    h ^= value.charCodeAt(i);
    h = Math.imul(h, 16777619);
  }
  return h >>> 0;
}

function cssVar(el: HTMLElement, name: string, fallback: string): string {
  const value = getComputedStyle(el).getPropertyValue(name).trim();
  return value || fallback;
}

function groupPalette(index: number): string {
  const colours = [
    "hsl(188 74% 66%)",
    "hsl(156 68% 62%)",
    "hsl(260 54% 72%)",
    "hsl(35 70% 69%)",
    "hsl(214 76% 66%)",
    "hsl(335 58% 70%)",
  ];
  return colours[index % colours.length];
}

function withAlpha(colour: string, alpha: number): string {
  if (colour.startsWith("hsl(") && !colour.includes("/")) {
    return colour.replace(")", ` / ${alpha})`);
  }
  return colour;
}

function buildLayout(graph: TopoGraph, width: number, height: number): Layout {
  const groupIndex = new Map<string, number>();
  graph.groups.forEach((group, index) => groupIndex.set(group, index));
  const cx = width / 2;
  const cy = height / 2;
  const rx = Math.max(160, width * 0.32);
  const ry = Math.max(110, height * 0.28);
  const groups = graph.groups.map((name, index) => {
    const t = (index / Math.max(1, graph.groups.length)) * Math.PI * 2 - Math.PI / 2;
    return {
      name,
      x: cx + Math.cos(t) * rx,
      y: cy + Math.sin(t) * ry,
      count: graph.agents.filter((a) => a.group === name).length,
      colour: groupPalette(index),
    };
  });

  const nodes: LayoutNode[] = graph.agents.map((agent) => {
    const gi = groupIndex.get(agent.group) ?? 0;
    const anchor = groups[gi] || { x: cx, y: cy };
    const seed = hash(agent.id);
    const angle = ((seed % 1000) / 1000) * Math.PI * 2;
    const radius = 22 + (seed % 90);
    return {
      id: agent.id,
      agent,
      groupIndex: gi,
      x: anchor.x + Math.cos(angle) * radius,
      y: anchor.y + Math.sin(angle) * radius,
    };
  });
  const byId = new Map(nodes.map((node) => [node.id, node]));
  const layoutEdges = sampleEdges(graph.edges, LAYOUT_EDGE_LIMIT);
  const links: LayoutLink[] = layoutEdges.map((edge) => ({
    source: edge.from,
    target: edge.to,
  }));

  const simulation = forceSimulation(nodes)
    .force("link", forceLink<LayoutNode, LayoutLink>(links).id((d) => d.id).distance(34).strength(0.035))
    .force("charge", forceManyBody<LayoutNode>().strength(-42).distanceMax(180))
    .force("collide", forceCollide<LayoutNode>().radius((d) => Math.min(9, 2.8 + Math.sqrt(graph.degree[d.id] || 0) * 0.55)).strength(0.42))
    .force("x", forceX<LayoutNode>((d) => groups[d.groupIndex]?.x ?? cx).strength(0.052))
    .force("y", forceY<LayoutNode>((d) => groups[d.groupIndex]?.y ?? cy).strength(0.052))
    .stop();

  const ticks = graph.agents.length > 450 ? 120 : 160;
  for (let i = 0; i < ticks; i++) simulation.tick();
  simulation.stop();

  return { nodes, byId, links, groups, width, height };
}

function screenToGraph(clientX: number, clientY: number, canvas: HTMLCanvasElement, viewport: Viewport): { x: number; y: number } {
  const rect = canvas.getBoundingClientRect();
  return {
    x: (clientX - rect.left - viewport.x) / viewport.scale,
    y: (clientY - rect.top - viewport.y) / viewport.scale,
  };
}

function applyViewport(ctx: CanvasRenderingContext2D, viewport: Viewport): void {
  ctx.translate(viewport.x, viewport.y);
  ctx.scale(viewport.scale, viewport.scale);
}

function drawCurve(ctx: CanvasRenderingContext2D, a: LayoutNode, b: LayoutNode, bend: number): void {
  const ax = a.x || 0;
  const ay = a.y || 0;
  const bx = b.x || 0;
  const by = b.y || 0;
  const mx = (ax + bx) / 2;
  const my = (ay + by) / 2;
  const dx = bx - ax;
  const dy = by - ay;
  const len = Math.hypot(dx, dy) || 1;
  const nx = -dy / len;
  const ny = dx / len;
  ctx.moveTo(ax, ay);
  ctx.quadraticCurveTo(mx + nx * bend, my + ny * bend, bx, by);
}

function nodeRadius(graph: TopoGraph, id: string): number {
  return Math.min(8.5, 2.1 + Math.sqrt(graph.degree[id] || 0) * 0.48);
}

export function DenseGraphMap({
  graph,
  live,
  selectedId,
  onSelect,
}: DenseGraphMapProps): React.JSX.Element {
  const wrapRef = React.useRef<HTMLDivElement | null>(null);
  const canvasRef = React.useRef<HTMLCanvasElement | null>(null);
  const staticRef = React.useRef<HTMLCanvasElement | null>(null);
  const dragRef = React.useRef<{ x: number; y: number; viewport: Viewport } | null>(null);
  const liveRef = React.useRef(live);
  const [size, setSize] = React.useState({ width: 900, height: 420 });
  const [viewport, setViewport] = React.useState<Viewport>({ scale: 1, x: 0, y: 0 });
  const [hoverId, setHoverId] = React.useState<string | null>(null);
  const roleIndex = React.useMemo(() => roleIndexFor(graph.roles), [graph.roles]);
  const graphFingerprint = React.useMemo(
    () => [
      graph.agents.length,
      graph.edges.length,
      graph.groups.join("|"),
      graph.agents.map((a) => a.id).join("|"),
    ].join("::"),
    [graph],
  );
  const layout = React.useMemo(
    () => buildLayout(graph, size.width, size.height),
    // `graph` is rebuilt every console poll. The expensive force layout
    // should only rerun when graph shape changes, not when an equivalent
    // REST payload is normalized into fresh object identities.
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [graphFingerprint, size.width, size.height],
  );

  React.useEffect(() => {
    liveRef.current = live;
  }, [live]);

  React.useEffect(() => {
    const el = wrapRef.current;
    if (!el) return;
    const ro = new ResizeObserver((entries) => {
      const rect = entries[0]?.contentRect;
      if (!rect) return;
      setSize({
        width: Math.max(420, Math.floor(rect.width)),
        height: Math.max(320, Math.floor(rect.height)),
      });
    });
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  const drawStatic = React.useCallback(() => {
    const host = wrapRef.current;
    if (!host) return null;
    const dpr = Math.max(1, Math.min(2, window.devicePixelRatio || 1));
    const off = document.createElement("canvas");
    off.width = Math.floor(layout.width * dpr);
    off.height = Math.floor(layout.height * dpr);
    const ctx = off.getContext("2d");
    if (!ctx) return null;
    ctx.scale(dpr, dpr);
    ctx.clearRect(0, 0, layout.width, layout.height);

    const faint = cssVar(host, "--ink-faint", "rgba(148, 163, 184, 1)");
    const inkMuted = cssVar(host, "--ink-muted", "rgba(180, 190, 205, 1)");
    const edgeAlpha = graph.edges.length > 18000 ? 0.030 : graph.edges.length > 6000 ? 0.048 : 0.075;

    for (const group of layout.groups) {
      const grad = ctx.createRadialGradient(group.x, group.y, 10, group.x, group.y, Math.max(110, group.count * 2.1));
      grad.addColorStop(0, group.colour);
      grad.addColorStop(0.34, withAlpha(group.colour, 0.34));
      grad.addColorStop(1, "transparent");
      ctx.globalAlpha = 0.16;
      ctx.fillStyle = grad;
      ctx.beginPath();
      ctx.arc(group.x, group.y, Math.max(110, group.count * 2.1), 0, Math.PI * 2);
      ctx.fill();
    }

    ctx.lineWidth = graph.edges.length > 12000 ? 0.42 : 0.58;
    ctx.strokeStyle = faint;
    ctx.globalAlpha = edgeAlpha;
    ctx.beginPath();
    for (const edge of graph.edges) {
      const a = layout.byId.get(edge.from);
      const b = layout.byId.get(edge.to);
      if (!a || !b) continue;
      const seed = hash(edgeKey(edge.from, edge.to));
      const sameGroup = a.agent.group === b.agent.group;
      const bend = sameGroup ? ((seed % 13) - 6) : ((seed % 2 === 0 ? 1 : -1) * (18 + (seed % 42)));
      drawCurve(ctx, a, b, bend);
    }
    ctx.stroke();

    const labelNodes = layout.nodes
      .slice()
      .sort((a, b) => (graph.degree[b.id] || 0) - (graph.degree[a.id] || 0) || a.id.localeCompare(b.id))
      .slice(0, LABEL_LIMIT);
    const labelSet = new Set(labelNodes.map((n) => n.id));

    for (const node of layout.nodes) {
      const r = nodeRadius(graph, node.id);
      const x = node.x || 0;
      const y = node.y || 0;
      ctx.globalAlpha = labelSet.has(node.id) ? 0.97 : 0.78;
      ctx.fillStyle = colourForRole(node.agent.role, roleIndex);
      ctx.beginPath();
      ctx.arc(x, y, r, 0, Math.PI * 2);
      ctx.fill();
      if (labelSet.has(node.id)) {
        ctx.globalAlpha = 0.26;
        ctx.strokeStyle = colourForRole(node.agent.role, roleIndex);
        ctx.lineWidth = 1.2;
        ctx.beginPath();
        ctx.arc(x, y, r + 5, 0, Math.PI * 2);
        ctx.stroke();
      }
    }

    ctx.font = "600 12px Inter, system-ui, sans-serif";
    ctx.textAlign = "center";
    ctx.textBaseline = "middle";
    for (const node of labelNodes) {
      const x = node.x || 0;
      const y = (node.y || 0) - nodeRadius(graph, node.id) - 11;
      const text = node.agent.label.replace(/\s+(seat|sub-agent)\s+/i, " ");
      const metrics = ctx.measureText(text);
      ctx.globalAlpha = 0.76;
      ctx.fillStyle = "rgba(0,0,0,0.42)";
      ctx.fillRect(x - metrics.width / 2 - 5, y - 8, metrics.width + 10, 16);
      ctx.globalAlpha = 0.96;
      ctx.fillStyle = inkMuted;
      ctx.fillText(text, x, y);
    }

    ctx.globalAlpha = 1;
    return off;
  // `graph` is intentionally keyed through `graphFingerprint` here; fresh
  // poll objects with the same shape should reuse the same static layer.
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [graphFingerprint, layout, roleIndex]);

  React.useEffect(() => {
    staticRef.current = drawStatic();
  }, [drawStatic]);

  React.useEffect(() => {
    let raf = 0;
    let stopped = false;
    const draw = () => {
      if (stopped) return;
      const canvas = canvasRef.current;
      const host = wrapRef.current;
      const ctx = canvas?.getContext("2d");
      if (!canvas || !ctx || !host) return;
      const dpr = Math.max(1, Math.min(2, window.devicePixelRatio || 1));
      const targetW = Math.floor(layout.width * dpr);
      const targetH = Math.floor(layout.height * dpr);
      if (canvas.width !== targetW || canvas.height !== targetH) {
        canvas.width = targetW;
        canvas.height = targetH;
        canvas.style.width = `${layout.width}px`;
        canvas.style.height = `${layout.height}px`;
      }
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
      ctx.clearRect(0, 0, layout.width, layout.height);
      const staticCanvas = staticRef.current || drawStatic();
      ctx.save();
      applyViewport(ctx, viewport);
      if (staticCanvas) ctx.drawImage(staticCanvas, 0, 0, layout.width, layout.height);

      const focus = cssVar(host, "--focus", "rgb(90, 160, 255)");
      const ok = cssVar(host, "--ok", "rgb(70, 200, 130)");
      const warn = cssVar(host, "--warn", "rgb(245, 170, 70)");
      const ink = cssVar(host, "--ink", "rgb(235, 238, 245)");
      const currentLive = liveRef.current;
      const selected = layout.byId.get(hoverId || selectedId || "");
      const active = new Set(Object.keys(currentLive.active));
      const busy = new Set(Object.entries(currentLive.busy).filter(([, v]) => v).map(([k]) => k));

      if (selected) {
        const peerSet = new Set(selected.agent.wiredTo);
        ctx.globalAlpha = 0.88;
        ctx.lineWidth = 1.25 / Math.sqrt(viewport.scale);
        ctx.strokeStyle = focus;
        ctx.beginPath();
        for (const peerId of peerSet) {
          const peer = layout.byId.get(peerId);
          if (!peer) continue;
          drawCurve(ctx, selected, peer, selected.agent.group === peer.agent.group ? 8 : 28);
        }
        ctx.stroke();
        for (const peerId of peerSet) {
          const peer = layout.byId.get(peerId);
          if (!peer) continue;
          ctx.globalAlpha = 0.98;
          ctx.fillStyle = focus;
          ctx.beginPath();
          ctx.arc(peer.x || 0, peer.y || 0, 3.1 / Math.sqrt(viewport.scale), 0, Math.PI * 2);
          ctx.fill();
        }
      }

      for (const id of active) {
        const node = layout.byId.get(id);
        if (!node) continue;
        ctx.globalAlpha = 0.28;
        ctx.strokeStyle = ok;
        ctx.lineWidth = 2 / Math.sqrt(viewport.scale);
        ctx.beginPath();
        ctx.arc(node.x || 0, node.y || 0, 11 / Math.sqrt(viewport.scale), 0, Math.PI * 2);
        ctx.stroke();
      }
      for (const id of busy) {
        const node = layout.byId.get(id);
        if (!node) continue;
        const phase = (Date.now() / 820) % (Math.PI * 2);
        ctx.globalAlpha = 0.9;
        ctx.strokeStyle = warn;
        ctx.lineWidth = 2.1 / Math.sqrt(viewport.scale);
        ctx.beginPath();
        ctx.arc(node.x || 0, node.y || 0, 14 / Math.sqrt(viewport.scale), phase, phase + Math.PI * 1.35);
        ctx.stroke();
      }
      if (selected) {
        const r = nodeRadius(graph, selected.id) + 3.6 / Math.sqrt(viewport.scale);
        ctx.globalAlpha = 1;
        ctx.fillStyle = ink;
        ctx.strokeStyle = focus;
        ctx.lineWidth = 2.4 / Math.sqrt(viewport.scale);
        ctx.beginPath();
        ctx.arc(selected.x || 0, selected.y || 0, r, 0, Math.PI * 2);
        ctx.fill();
        ctx.stroke();
      }
      ctx.restore();
      raf = requestAnimationFrame(draw);
    };
    raf = requestAnimationFrame(draw);
    return () => {
      stopped = true;
      cancelAnimationFrame(raf);
    };
  // See `drawStatic`: equivalent polling payloads must not restart the
  // animation loop or re-run expensive canvas setup.
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [drawStatic, graphFingerprint, hoverId, layout, selectedId, viewport]);

  const nearestId = React.useCallback((clientX: number, clientY: number): string | null => {
    const canvas = canvasRef.current;
    if (!canvas) return null;
    const pos = screenToGraph(clientX, clientY, canvas, viewport);
    let best: { id: string; d2: number } | null = null;
    const threshold = Math.max(12, 18 / viewport.scale);
    for (const node of layout.nodes) {
      const dx = (node.x || 0) - pos.x;
      const dy = (node.y || 0) - pos.y;
      const d2 = dx * dx + dy * dy;
      if (d2 > threshold * threshold) continue;
      if (!best || d2 < best.d2) best = { id: node.id, d2 };
    }
    return best?.id || null;
  }, [layout, viewport]);

  const hover = hoverId ? graph.byId.get(hoverId) : null;
  const selected = selectedId ? graph.byId.get(selectedId) : null;

  return (
    <div
      ref={wrapRef}
      className="topo-dense"
      data-testid="topology-dense-map"
      onPointerDown={(event) => {
        const id = nearestId(event.clientX, event.clientY);
        if (id) onSelect(id);
        dragRef.current = { x: event.clientX, y: event.clientY, viewport };
        event.currentTarget.setPointerCapture(event.pointerId);
      }}
      onPointerMove={(event) => {
        if (dragRef.current) {
          const dx = event.clientX - dragRef.current.x;
          const dy = event.clientY - dragRef.current.y;
          setViewport({
            ...dragRef.current.viewport,
            x: dragRef.current.viewport.x + dx,
            y: dragRef.current.viewport.y + dy,
          });
          return;
        }
        setHoverId(nearestId(event.clientX, event.clientY));
      }}
      onPointerUp={(event) => {
        dragRef.current = null;
        event.currentTarget.releasePointerCapture(event.pointerId);
      }}
      onPointerCancel={(event) => {
        dragRef.current = null;
        event.currentTarget.releasePointerCapture(event.pointerId);
      }}
      onPointerLeave={() => setHoverId(null)}
      onWheel={(event) => {
        event.preventDefault();
        const canvas = canvasRef.current;
        if (!canvas) return;
        const rect = canvas.getBoundingClientRect();
        const mx = event.clientX - rect.left;
        const my = event.clientY - rect.top;
        const nextScale = Math.max(0.55, Math.min(4, viewport.scale * (event.deltaY < 0 ? 1.12 : 0.88)));
        const gx = (mx - viewport.x) / viewport.scale;
        const gy = (my - viewport.y) / viewport.scale;
        setViewport({
          scale: nextScale,
          x: mx - gx * nextScale,
          y: my - gy * nextScale,
        });
      }}
    >
      <canvas ref={canvasRef} className="topo-dense__canvas" aria-label="Dense topology force graph" />
      <div className="topo-dense__toolbar">
        <button type="button" onClick={() => setViewport((v) => ({ ...v, scale: Math.min(4, v.scale * 1.22) }))}>+</button>
        <button type="button" onClick={() => setViewport((v) => ({ ...v, scale: Math.max(0.55, v.scale / 1.22) }))}>-</button>
        <button type="button" onClick={() => setViewport({ scale: 1, x: 0, y: 0 })}>Reset</button>
      </div>
      <div className="topo-dense__labels" aria-hidden="true">
        {layout.groups.map((g) => (
          <div
            key={g.name}
            className="topo-dense__group-label"
            style={{
              left: `${g.x * viewport.scale + viewport.x}px`,
              top: `${g.y * viewport.scale + viewport.y + 86}px`,
              borderColor: g.colour,
            }}
          >
            <strong>{g.name}</strong>
            <span>{g.count} agents</span>
          </div>
        ))}
      </div>
      {(hover || selected) && (
        <div className="topo-dense__inspector">
          <strong>{(hover || selected)?.label}</strong>
          <span>{(hover || selected)?.group}</span>
          <span>{(hover || selected)?.wiredTo.length || 0} peers</span>
        </div>
      )}
    </div>
  );
}
