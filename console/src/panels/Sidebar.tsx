import React from "react";
import type { ConsoleAgent, ConsoleFrame } from "../types";

type NavKind = "topology" | "timeline" | "gating" | "roster" | "routing" | "gates" | "logs";

interface SidebarProps {
  agents: ConsoleAgent[];
  selectedMemberId: string;
  recentActivity: ConsoleFrame[];
  collapsed: boolean;
  onSelect: (agent: ConsoleAgent) => void;
  onInspect: (agent: ConsoleAgent) => void;
  onOpenControl: (kind: NavKind) => void;
}

/// Nav-item visibility configuration via URL query params:
///
///   ?hide_nav=timeline,gating          ⇒ hide listed kinds (others visible)
///   ?show_nav=topology,roster          ⇒ show only listed kinds
///   ?show_nav=topology&hide_nav=...    ⇒ whitelist wins; hide_nav ignored
///
/// Embedders can drop the query string into the iframe URL without
/// touching the React tree. Reads once on mount; reload to change.
const ALL_NAV: NavKind[] = ["topology", "timeline", "gating", "roster", "routing", "gates", "logs"];
const NAV_LABEL: Record<NavKind, string> = {
  topology: "Topology",
  timeline: "Today",
  gating: "Gating",
  roster: "Roster",
  routing: "Routing",
  gates: "Gates",
  logs: "Logs",
};

function parseNavList(raw: string | null): Set<NavKind> {
  const out = new Set<NavKind>();
  if (!raw) return out;
  for (const token of raw.split(",")) {
    const trimmed = token.trim();
    if (ALL_NAV.includes(trimmed as NavKind)) out.add(trimmed as NavKind);
  }
  return out;
}

function visibleNavKinds(): NavKind[] {
  if (typeof window === "undefined") return ALL_NAV;
  const params = new URLSearchParams(window.location.search);
  const show = parseNavList(params.get("show_nav"));
  if (show.size > 0) return ALL_NAV.filter((k) => show.has(k));
  const hide = parseNavList(params.get("hide_nav"));
  if (hide.size > 0) return ALL_NAV.filter((k) => !hide.has(k));
  return ALL_NAV;
}

type Bucket = "Personal" | "Coordinators" | "Domains" | "Internal" | "Other";

function bucketOf(a: ConsoleAgent): Bucket {
  const g = (a.group || "").toLowerCase();
  const p = (a.role || a.kind || "").toLowerCase();
  if (g.includes("coordinator") || p.includes("coord") || p.includes("triage") || p.includes("router") || p.includes("commander")) return "Coordinators";
  if (g.includes("personal") || p.includes("personal") || p.includes("identity") || p.includes("lead")) return "Personal";
  if (g.includes("internal") || p.includes("gate") || p.includes("monitor") || p.includes("scribe")) return "Internal";
  if (g.includes("domain") || g.includes("responder") || g.includes("communication") || g.includes("specialist")) return "Domains";
  return "Domains";
}

const SECTION_ORDER: Bucket[] = ["Personal", "Coordinators", "Domains", "Internal", "Other"];

function deriveStateAttr(agent: ConsoleAgent): "active" | "degraded" | "retired" {
  const state = (agent.state || "").toLowerCase();
  if (state === "retired" || state === "stopped") return "retired";
  const degraded = agent.labels?.console_degraded === "true" ||
                   state.includes("degrade") ||
                   agent.lease_healthy === false;
  if (degraded) return "degraded";
  return "active";
}

function pulseSamples(activity: ConsoleFrame[], identity: string): number[] {
  const bucket = new Array<number>(10).fill(0);
  const now = Date.now();
  const window = 15 * 60 * 1000;
  for (const f of activity) {
    if (!f.timestampMs || (f.identity || "") !== identity) continue;
    const age = now - f.timestampMs;
    if (age < 0 || age > window) continue;
    const idx = 9 - Math.floor((age / window) * 10);
    if (idx >= 0 && idx < 10) bucket[idx]++;
  }
  return bucket;
}

function inboxCount(agent: ConsoleAgent): number {
  const n = Number(agent.labels?.console_inbox_count ?? 0);
  return Number.isFinite(n) ? n : 0;
}

export function Sidebar({
  agents,
  selectedMemberId,
  recentActivity,
  collapsed,
  onSelect,
  onInspect,
  onOpenControl,
}: SidebarProps): React.JSX.Element {
  const [q, setQ] = React.useState("");
  // Computed once; URL-driven config doesn't change without a reload.
  const navKinds = React.useMemo(() => visibleNavKinds(), []);

  const filtered = React.useMemo(() => {
    if (!q) return agents;
    const needle = q.toLowerCase();
    return agents.filter((a) =>
      a.label.toLowerCase().includes(needle) ||
      (a.identity || "").toLowerCase().includes(needle) ||
      (a.member_id || "").toLowerCase().includes(needle) ||
      (a.role || "").toLowerCase().includes(needle),
    );
  }, [agents, q]);

  const grouped = React.useMemo(() => {
    const g = new Map<Bucket, ConsoleAgent[]>();
    for (const a of filtered) {
      const key = bucketOf(a);
      if (!g.has(key)) g.set(key, []);
      g.get(key)!.push(a);
    }
    return g;
  }, [filtered]);

  if (collapsed) {
    return (
      <aside
        className="sidebar sidebar--collapsed"
        data-collapsed="true"
        data-testid="sidebar-root"
      >
        <i className="sidebar__grip" aria-hidden="true" />
      </aside>
    );
  }

  return (
    <aside className="sidebar" data-testid="sidebar-root">
      <div className="sidebar__search">
        <input
          placeholder="Search agents, profiles, ids…"
          value={q}
          onChange={(e) => setQ(e.target.value)}
          data-testid="sidebar-search"
        />
      </div>

      {navKinds.length > 0 && (
        <div className="sidebar__section sidebar__section--nav">
          <div className="sidebar__sec-head">
            <span className="sidebar__sec-label">Views</span>
          </div>
          {navKinds.map((kind) => (
            <button
              key={kind}
              className="sidebar__navitem"
              onClick={() => onOpenControl(kind)}
              data-testid={`nav:${kind}`}
            >
              {NAV_LABEL[kind]}
            </button>
          ))}
        </div>
      )}

      {SECTION_ORDER.map((bucket) => {
        const list = grouped.get(bucket);
        if (!list || list.length === 0) return null;
        return (
          <div className="sidebar__section" key={bucket}>
            <div className="sidebar__sec-head">
              <span className="sidebar__sec-label">{bucket}</span>
              <span className="sidebar__sec-spacer" />
              <span className="sidebar__sec-count">{list.length}</span>
            </div>
            {list.map((agent) => {
              const stateAttr = deriveStateAttr(agent);
              const pulse = pulseSamples(recentActivity, agent.identity || agent.member_id);
              const inbox = inboxCount(agent);
              return (
                <div
                  key={agent.member_id}
                  className={`agent ${agent.member_id === selectedMemberId ? "is-active" : ""}`}
                  data-state={stateAttr}
                  data-testid={`sidebar-agent:${agent.member_id}`}
                  onClick={() => onSelect(agent)}
                  onContextMenu={(e) => { e.preventDefault(); onInspect(agent); }}
                  role="button"
                  tabIndex={0}
                >
                  <span className="agent__dot" />
                  <span className="agent__body">
                    <span className="agent__name">{agent.label}</span>
                    <span className="agent__id">{agent.identity || agent.member_id}</span>
                  </span>
                  <span className="agent__meta">
                    <span className="agent__pulse">
                      {pulse.map((v, i) => (
                        <span key={i} style={{ height: `${Math.max(1, Math.min(12, v * 2 + 1))}px` }} />
                      ))}
                    </span>
                    {inbox > 0 && <span className="agent__inbox">{inbox}</span>}
                  </span>
                </div>
              );
            })}
          </div>
        );
      })}
    </aside>
  );
}
