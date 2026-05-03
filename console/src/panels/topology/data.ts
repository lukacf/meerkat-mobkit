// Shared topology data shaping + real-activity wiring.
//
// The console only exposes a flat node list (`identity, label, role, state,
// wired_to`). All four reference layouts are happy with that — what they
// actually need is:
//   * agents[] keyed by identity
//   * edges[] derived from `wired_to` (mirrored to undirected pairs)
//   * degree[identity] (so radius/ring-bucket can scale by hub-ness)
//   * roles[] in display order
//
// The "real pulses" layer reads the live activity stream:
//   * any frame with a known `identity` marks that node hot for `life` ms
//   * a `send_*` tool call with a resolvable `peer_id` becomes a directed
//     pulse along the (sender → recipient) edge
//   * the peer-id-to-identity registry is built from `peers` tool results
//     observed in the same stream — same trick the chat adapter uses
//
// No synthetic event bus, no fabricated traffic.

import React from "react";
import type { ConsoleAgent, ConsoleFrame, ConsoleTopologyNode } from "../../types";

export interface TopoAgent {
  id: string;
  label: string;
  role: string;
  state: string;
  wiredTo: string[];
}

export interface TopoEdge {
  from: string;
  to: string;
}

export interface TopoGraph {
  agents: TopoAgent[];
  byId: Map<string, TopoAgent>;
  edges: TopoEdge[];
  degree: Record<string, number>;
  roles: string[];
}

export interface TopoPulse {
  id: string;
  from: string;
  to: string;
  ts: number;
}

export interface TopoActivity {
  active: Record<string, number>;
  pulses: TopoPulse[];
}

const PEER_TOOL_NAMES = new Set(["send_request", "send_message", "send_response"]);

const ROLE_ORDER_HINT = [
  "user",
  "personal",
  "coordinator",
  "scribe",
  "review",
  "summarizer",
  "initiative",
  "channel",
  "responder",
  "domain",
  "internal",
  "approval",
  "monitor",
];

/// Stable colour per role. Six-slot palette cycles when we run past the
/// well-known names — keeps the legend deterministic.
export const ROLE_PALETTE = [
  "var(--focus)",
  "var(--accent)",
  "var(--ok)",
  "var(--warn)",
  "var(--c-init)",
  "var(--ink-muted)",
];

export function colourForRole(role: string, roleIndex: Record<string, number>): string {
  const idx = roleIndex[role] ?? 0;
  return ROLE_PALETTE[idx % ROLE_PALETTE.length];
}

function roleSortKey(role: string): number {
  const idx = ROLE_ORDER_HINT.findIndex((hint) => role.toLowerCase().includes(hint));
  return idx === -1 ? ROLE_ORDER_HINT.length : idx;
}

/// Build the canonical graph from the console's flat node list, falling
/// back to the agent registry when the topology snapshot is empty (the
/// gateway sometimes ships only one or the other).
export function buildGraph(
  nodes: ConsoleTopologyNode[],
  agents: ConsoleAgent[],
): TopoGraph {
  const source: ConsoleTopologyNode[] = nodes.length > 0
    ? nodes
    : agents.map((a) => ({
        identity: a.identity || a.member_id,
        label: a.label,
        role: a.role,
        state: a.state,
        wired_to: a.wired_to,
      }));

  const byId = new Map<string, TopoAgent>();
  const list: TopoAgent[] = [];
  for (const n of source) {
    const id = (n.identity || n.label || "").trim();
    if (!id || byId.has(id)) continue;
    const agent: TopoAgent = {
      id,
      label: (n.label || id).trim(),
      role: (n.role || "agent").trim(),
      state: (n.state || "").toLowerCase(),
      wiredTo: (n.wired_to || []).map((s) => s.trim()).filter(Boolean),
    };
    byId.set(id, agent);
    list.push(agent);
  }

  // Edges: mirror wired_to into a deduped pair set so each pair shows up
  // exactly once. The console's `wired_to` is asymmetric (each side lists
  // its peers) but visually the wire is undirected, so we collapse on the
  // sorted-tuple key.
  const seen = new Set<string>();
  const edges: TopoEdge[] = [];
  for (const a of list) {
    for (const t of a.wiredTo) {
      if (!byId.has(t) || t === a.id) continue;
      const key = a.id < t ? `${a.id}|${t}` : `${t}|${a.id}`;
      if (seen.has(key)) continue;
      seen.add(key);
      edges.push({ from: a.id, to: t });
    }
  }

  const degree: Record<string, number> = {};
  for (const e of edges) {
    degree[e.from] = (degree[e.from] || 0) + 1;
    degree[e.to] = (degree[e.to] || 0) + 1;
  }

  const roles = Array.from(new Set(list.map((a) => a.role))).sort((a, b) => {
    const ra = roleSortKey(a);
    const rb = roleSortKey(b);
    if (ra !== rb) return ra - rb;
    return a.localeCompare(b);
  });

  return { agents: list, byId, edges, degree, roles };
}

export function roleIndexFor(roles: string[]): Record<string, number> {
  const idx: Record<string, number> = {};
  roles.forEach((r, i) => { idx[r] = i; });
  return idx;
}

/// Live activity hook — reads the global activity stream and produces:
///   * `active`: identity → most-recent-frame-ts (used for halos)
///   * `pulses`: list of in-flight directional pulses derived from
///     resolvable peer-comms tool calls
///
/// "Resolvable" means we've seen a `peers` tool result frame in the
/// stream that maps the recipient's `peer_id` (UUID) back to a known
/// identity. Until that mapping arrives, peer pulses for that pair are
/// silently dropped — we don't fabricate a direction we can't prove.
export function useTopologyActivity(
  frames: ConsoleFrame[],
  graph: TopoGraph,
  options: { life?: number } = {},
): TopoActivity {
  const life = options.life ?? 1500;
  const [now, setNow] = React.useState(() => Date.now());

  // Heartbeat that drives pulse-fade animation. Stops when no pulses are
  // in flight — checked via the ref to avoid pinning re-renders.
  const ticking = React.useRef(false);
  React.useEffect(() => {
    if (ticking.current) return;
    let raf = 0;
    let stopped = false;
    ticking.current = true;
    const step = () => {
      if (stopped) return;
      setNow(Date.now());
      raf = requestAnimationFrame(step);
    };
    raf = requestAnimationFrame(step);
    return () => {
      stopped = true;
      ticking.current = false;
      cancelAnimationFrame(raf);
    };
  }, []);

  return React.useMemo(() => {
    const active: Record<string, number> = {};
    const pulses: TopoPulse[] = [];
    const peerRegistry = new Map<string, string>();

    // Walk frames oldest → newest so the peers-tool registry is populated
    // before any send_* call that depends on it. The activity buffer is
    // maintained newest-first by ConsoleApp, so reverse a shallow copy.
    const ordered = frames.slice().reverse();
    for (const frame of ordered) {
      const ts = frame.timestampMs || 0;
      if (!ts) continue;

      const identity = frame.identity?.trim();
      if (identity && graph.byId.has(identity)) {
        if ((active[identity] || 0) < ts) active[identity] = ts;
      }

      const data = frame.data as Record<string, unknown> | undefined;
      const name = data && typeof data.name === "string" ? data.name : "";

      // Build peer-id → identity registry from `peers` tool results.
      if (name === "peers" && (frame.event === "tool_execution_completed" || frame.event === "tool_result_received")) {
        const raw = typeof data?.result === "string" ? data.result : null;
        if (raw) {
          try {
            const parsed = JSON.parse(raw) as { peers?: Array<{ peer_id?: unknown; name?: unknown }> };
            for (const p of parsed.peers || []) {
              if (typeof p.peer_id === "string" && typeof p.name === "string") {
                // Registry value is the LAST path segment so the resolved
                // identity matches what the topology snapshot uses
                // (e.g. `incident-command-center/scribe/scribe` → `scribe`).
                const lastSeg = p.name.split("/").pop() || p.name;
                peerRegistry.set(p.peer_id, lastSeg);
              }
            }
          } catch { /* ignore non-JSON */ }
        }
      }

      // Real pulse: a peer-comms tool call whose recipient resolves to a
      // known identity. Anything else (UUID we haven't learnt yet,
      // identity not in the topology) is dropped — better to render fewer
      // truthful pulses than guess directions.
      if (
        PEER_TOOL_NAMES.has(name)
        && (frame.event === "tool_call_requested" || frame.event === "tool_call" || frame.event === "tool_execution_started")
        && identity
        && graph.byId.has(identity)
      ) {
        const args = data && typeof data.args === "object" ? data.args as Record<string, unknown> : null;
        const peerId = typeof args?.peer_id === "string" ? args.peer_id : null;
        const recipient = peerId ? peerRegistry.get(peerId) : null;
        if (recipient && graph.byId.has(recipient) && recipient !== identity) {
          pulses.push({
            id: typeof data?.id === "string" ? data.id : `${frame.id || ts}-${pulses.length}`,
            from: identity,
            to: recipient,
            ts,
          });
        }
      }
    }

    // Drop expired entries.
    const cutoff = now - life;
    for (const [k, v] of Object.entries(active)) {
      if (v < cutoff) delete active[k];
    }
    const live = pulses.filter((p) => p.ts >= cutoff);

    return { active, pulses: live };
  // `now` triggers re-fade; `frames`/`graph` trigger re-derivation.
  }, [frames, graph, life, now]);
}

/// Convenience: per-edge "is this edge currently carrying a pulse?"
/// derivative. Useful for emphasising hot edges in any layout.
export function edgeKey(a: string, b: string): string {
  return a < b ? `${a}|${b}` : `${b}|${a}`;
}
