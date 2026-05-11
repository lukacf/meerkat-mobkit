import React from "react";
import type { ConsoleAgent, ConsoleFrame } from "../types";

export type NavKind = "topology" | "timeline" | "gating" | "roster" | "routing" | "logs" | "health";

interface SidebarProps {
  agents: ConsoleAgent[];
  selectedMemberId: string;
  recentActivity: ConsoleFrame[];
  collapsed: boolean;
  visibleControls?: NavKind[];
  onSelect: (agent: ConsoleAgent) => void;
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
const ALL_NAV: NavKind[] = ["topology", "timeline", "gating", "roster", "routing", "logs", "health"];
const NAV_LABEL: Record<NavKind, string> = {
  topology: "Topology",
  timeline: "Today",
  gating: "Approvals",
  roster: "Roster",
  routing: "Routing",
  logs: "Logs",
  health: "Health",
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
interface AgentRow {
  agent: ConsoleAgent;
  childOfHost: boolean;
}

function isWorkerish(a: ConsoleAgent): boolean {
  const haystack = [a.label, a.identity, a.member_id, a.role].filter(Boolean).join(" ").toLowerCase();
  return (
    haystack.includes("worker")
    || haystack.includes("delegate")
    || haystack.includes("helper")
  );
}

function isCommanderLike(a: ConsoleAgent): boolean {
  if (isWorkerish(a)) return false;
  const haystack = [a.label, a.identity, a.member_id, a.role].filter(Boolean).join(" ").toLowerCase();
  return haystack.includes("commander") || haystack.includes("coordinator");
}

function isSpawnedDelegateLike(a: ConsoleAgent, host: ConsoleAgent | null): boolean {
  if (!isWorkerish(a)) return false;
  const wiredTo = new Set((a.wired_to || []).map((peer) => peer.toLowerCase()));
  const hostKeys = [host?.identity, host?.member_id, host?.agent_id]
    .filter((value): value is string => Boolean(value))
    .map((value) => value.toLowerCase());
  if (hostKeys.some((key) => wiredTo.has(key))) return true;

  const role = (a.role || "").toLowerCase();
  const group = (a.group || "").toLowerCase();
  return (
    !group
    || group === role
    || group === "worker"
    || group === "delegate"
    || group.includes("helper")
  );
}

export const __sidebarTest = {
  isCommanderLike,
  isSpawnedDelegateLike,
};

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
  if (state === "retired" || state === "retiring" || state === "stopped") return "retired";
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
  visibleControls,
  onSelect,
  onOpenControl,
}: SidebarProps): React.JSX.Element {
  const [q, setQ] = React.useState("");
  // Computed once; URL-driven config doesn't change without a reload.
  const navKinds = React.useMemo(() => {
    const configured = visibleNavKinds();
    if (!visibleControls) return configured;
    const allowed = new Set(visibleControls);
    return configured.filter((kind) => allowed.has(kind));
  }, [visibleControls]);

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
    const g = new Map<Bucket, AgentRow[]>();
    const host = filtered.find(isCommanderLike);
    for (const a of filtered) {
      const childOfHost = Boolean(host && host.member_id !== a.member_id && isSpawnedDelegateLike(a, host));
      const key = childOfHost && host ? bucketOf(host) : bucketOf(a);
      if (!g.has(key)) g.set(key, []);
      g.get(key)!.push({ agent: a, childOfHost });
    }
    if (host) {
      for (const rows of g.values()) {
        rows.sort((a, b) => {
          if (a.agent.member_id === host.member_id) return -1;
          if (b.agent.member_id === host.member_id) return 1;
          if (a.childOfHost !== b.childOfHost) return a.childOfHost ? -1 : 1;
          return a.agent.label.localeCompare(b.agent.label);
        });
      }
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
      <div className="sidebar__mast">
        <div>
          <div className="sidebar__mast-title">Roster</div>
          <div className="sidebar__mast-sub">{agents.length} agents</div>
        </div>
      </div>
      <div className="sidebar__search">
        <input
          placeholder="Search roster..."
          value={q}
          onChange={(e) => setQ(e.target.value)}
          data-testid="sidebar-search"
        />
      </div>

      {navKinds.length > 0 && (
          <div className="sidebar__section sidebar__section--nav">
            <div className="sidebar__sec-head">
            <span className="sidebar__sec-label">Workbench</span>
          </div>
          <div className="sidebar__navgrid">
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
            {list.map(({ agent, childOfHost }) => {
              const stateAttr = deriveStateAttr(agent);
              const pulse = pulseSamples(recentActivity, agent.identity || agent.member_id);
              const inbox = inboxCount(agent);
              return (
                <div
                  key={agent.member_id}
                  className={`agent ${childOfHost ? "agent--child" : ""} ${agent.member_id === selectedMemberId ? "is-active" : ""}`}
                  data-state={stateAttr}
                  data-child-of-host={childOfHost ? "true" : undefined}
                  data-testid={`sidebar-agent:${agent.member_id}`}
                  onClick={() => onSelect(agent)}
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
