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
  depth: number;
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

function agentKeys(a: ConsoleAgent | null): string[] {
  return [a?.identity, a?.member_id, a?.agent_id]
    .filter((value): value is string => Boolean(value))
    .map((value) => value.toLowerCase());
}

function referenceMatchesAgentKey(reference: string, key: string): boolean {
  const normalizedReference = reference.toLowerCase();
  const normalizedKey = key.toLowerCase();
  if (normalizedReference === normalizedKey) return true;
  const compactReference = normalizedReference.replace(/[^a-z0-9]+/g, "");
  const compactKey = normalizedKey.replace(/[^a-z0-9]+/g, "");
  if (compactKey && compactReference === compactKey) return true;
  const tokens = normalizedReference
    .split(/[/:#\s]+/)
    .filter(Boolean);
  if (tokens.includes(normalizedKey)) return true;
  if (!compactKey) return false;
  for (let start = 0; start < tokens.length; start++) {
    let compactSlice = "";
    for (let end = start; end < tokens.length; end++) {
      compactSlice += tokens[end].replace(/[^a-z0-9]+/g, "");
      if (compactSlice === compactKey) return true;
      if (compactSlice.length > compactKey.length) break;
    }
  }
  return false;
}

function isWiredTo(a: ConsoleAgent, host: ConsoleAgent | null): boolean {
  if (!host) return false;
  const wiredTo = a.wired_to || [];
  return agentKeys(host).some((key) =>
    wiredTo.some((peer) => referenceMatchesAgentKey(peer, key)),
  );
}

function isSpawnedDelegateLike(a: ConsoleAgent, host: ConsoleAgent | null): boolean {
  if (!isWorkerish(a)) return false;
  if (isWiredTo(a, host)) return true;

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

function explicitHostId(a: ConsoleAgent): string | null {
  return a.labels?.delegate_host_identity
    || a.labels?.host_identity
    || a.labels?.parent_identity
    || null;
}

function findSpawnHost(a: ConsoleAgent, agents: ConsoleAgent[], commander: ConsoleAgent | null): ConsoleAgent | null {
  if (!isWorkerish(a)) return null;
  const explicitHost = explicitHostId(a);
  if (explicitHost) {
    const match = agents.find((candidate) =>
      candidate.member_id !== a.member_id
        && agentKeys(candidate).some((key) => referenceMatchesAgentKey(explicitHost, key)),
    );
    if (match) return match;
  }

  const commanderHost = agents.find((candidate) =>
    candidate.member_id !== a.member_id && isCommanderLike(candidate) && isWiredTo(a, candidate),
  );
  if (commanderHost) return commanderHost;

  const workerHost = agents.find((candidate) =>
    candidate.member_id !== a.member_id && isWorkerish(candidate) && isWiredTo(a, candidate),
  );
  if (workerHost) return workerHost;

  if (commander && commander.member_id !== a.member_id && isSpawnedDelegateLike(a, commander)) return commander;
  return null;
}

export const __sidebarTest = {
  isCommanderLike,
  isSpawnedDelegateLike,
  findSpawnHost,
  groupSidebarAgents,
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

function bucketForAgent(a: ConsoleAgent, parentById: Map<string, string>, byId: Map<string, ConsoleAgent>): Bucket {
  const seen = new Set<string>();
  let current: ConsoleAgent | undefined = a;
  while (current) {
    if (seen.has(current.member_id)) break;
    seen.add(current.member_id);
    const parentId = parentById.get(current.member_id);
    if (!parentId) break;
    current = byId.get(parentId);
  }
  return bucketOf(current || a);
}

function depthForAgent(a: ConsoleAgent, parentById: Map<string, string>): number {
  const seen = new Set<string>();
  let depth = 0;
  let current = a.member_id;
  while (parentById.has(current) && !seen.has(current)) {
    seen.add(current);
    depth += 1;
    current = parentById.get(current)!;
  }
  return depth;
}

function compareRows(host: ConsoleAgent | null) {
  return (a: AgentRow, b: AgentRow): number => {
    if (host) {
      if (a.agent.member_id === host.member_id) return -1;
      if (b.agent.member_id === host.member_id) return 1;
    }
    if (a.childOfHost !== b.childOfHost) return a.childOfHost ? -1 : 1;
    return a.agent.label.localeCompare(b.agent.label);
  };
}

function orderRowsPreorder(rows: AgentRow[], parentById: Map<string, string>, host: ConsoleAgent | null): AgentRow[] {
  const byParent = new Map<string, AgentRow[]>();
  const rowById = new Map(rows.map((row) => [row.agent.member_id, row]));
  const roots: AgentRow[] = [];
  for (const row of rows) {
    const parentId = parentById.get(row.agent.member_id);
    if (parentId && rowById.has(parentId)) {
      if (!byParent.has(parentId)) byParent.set(parentId, []);
      byParent.get(parentId)!.push(row);
    } else {
      roots.push(row);
    }
  }
  const sortRows = compareRows(host);
  roots.sort(sortRows);
  for (const children of byParent.values()) children.sort(sortRows);

  const ordered: AgentRow[] = [];
  const visit = (row: AgentRow): void => {
    ordered.push(row);
    for (const child of byParent.get(row.agent.member_id) || []) visit(child);
  };
  for (const root of roots) visit(root);
  return ordered;
}

function groupSidebarAgents(filtered: ConsoleAgent[]): Map<Bucket, AgentRow[]> {
  const g = new Map<Bucket, AgentRow[]>();
  const host = filtered.find(isCommanderLike);
  const byId = new Map(filtered.map((a) => [a.member_id, a]));
  const parentById = new Map<string, string>();
  for (const a of filtered) {
    const parent = findSpawnHost(a, filtered, host || null);
    if (parent) parentById.set(a.member_id, parent.member_id);
  }
  for (const a of filtered) {
    const childOfHost = parentById.has(a.member_id);
    const key = bucketForAgent(a, parentById, byId);
    if (!g.has(key)) g.set(key, []);
    g.get(key)!.push({ agent: a, childOfHost, depth: depthForAgent(a, parentById) });
  }
  for (const [key, rows] of g.entries()) {
    g.set(key, orderRowsPreorder(rows, parentById, host || null));
  }
  return g;
}

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
    return groupSidebarAgents(filtered);
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
            {list.map(({ agent, childOfHost, depth }) => {
              const stateAttr = deriveStateAttr(agent);
              const pulse = pulseSamples(recentActivity, agent.identity || agent.member_id);
              const inbox = inboxCount(agent);
              return (
                <div
                  key={agent.member_id}
                  className={`agent ${childOfHost ? "agent--child" : ""} ${agent.member_id === selectedMemberId ? "is-active" : ""}`}
                  data-state={stateAttr}
                  data-child-of-host={childOfHost ? "true" : undefined}
                  data-depth={childOfHost ? String(Math.min(depth, 3)) : undefined}
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
