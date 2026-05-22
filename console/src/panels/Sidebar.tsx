import React from "react";
import type {
  ConsoleAgent,
  ConsoleAgentListConfig,
  ConsoleFrame,
  ConsoleSidebarButtonConfig,
} from "../types";

export type NavKind = "topology" | "timeline" | "gating" | "roster" | "routing" | "logs" | "health";

interface SidebarProps {
  agents: ConsoleAgent[];
  selectedMemberId: string;
  recentActivity: ConsoleFrame[];
  collapsed: boolean;
  visibleControls?: NavKind[];
  customButtons?: ConsoleSidebarButtonConfig[];
  grouping?: ConsoleAgentListConfig;
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
export const ALL_NAV: NavKind[] = ["topology", "timeline", "gating", "roster", "routing", "logs", "health"];
const NAV_LABEL: Record<NavKind, string> = {
  topology: "Topology",
  timeline: "Today",
  gating: "Approvals",
  roster: "Roster",
  routing: "Routing",
  logs: "Logs",
  health: "Health",
};

export function normalizeNavKind(value: unknown): NavKind | null {
  return typeof value === "string" && ALL_NAV.includes(value as NavKind)
    ? value as NavKind
    : null;
}

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
  subgroup?: string | null;
}

type SidebarVirtualRow =
  | { kind: "section"; key: string; bucket: string; count: number; collapsed: boolean }
  | { kind: "empty"; key: string; bucket: string; sectionConfig: ReturnType<typeof sectionConfigFor> }
  | { kind: "subgroup"; key: string; bucket: string; label: string }
  | { kind: "agent"; key: string; bucket: string; row: AgentRow };

const SIDEBAR_ROW_HEIGHT = {
  section: 36,
  empty: 58,
  subgroup: 28,
  agent: 72,
} as const;

const SIDEBAR_OVERSCAN_PX = 360;

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

  const wiredNonWorkerHost = agents.find((candidate) =>
    candidate.member_id !== a.member_id && !isWorkerish(candidate) && isWiredTo(a, candidate),
  );
  if (wiredNonWorkerHost) return wiredNonWorkerHost;

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
  configuredAgentGroup,
  configuredAgentSubgroup,
  configuredAgentBadges,
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

function configuredSelectors(config: ConsoleAgentListConfig | undefined, key: "group_by" | "subgroup_by"): string[] {
  return (config?.[key] || []).map((value) => value.trim()).filter(Boolean);
}

function configuredFieldValue(agent: ConsoleAgent, selector: string): string | null {
  const normalized = selector.trim();
  if (!normalized) return null;
  if (normalized.startsWith("labels.")) {
    const key = normalized.slice("labels.".length);
    return agent.labels?.[key]?.trim() || null;
  }
  if (normalized.startsWith("label:")) {
    const key = normalized.slice("label:".length);
    return agent.labels?.[key]?.trim() || null;
  }
  switch (normalized) {
    case "group": return agent.group?.trim() || null;
    case "subgroup": return agent.subgroup?.trim() || null;
    case "role": return agent.role?.trim() || null;
    case "kind": return agent.kind?.trim() || null;
    case "identity": return agent.identity?.trim() || null;
    case "member_id": return agent.member_id?.trim() || null;
    case "agent_id": return agent.agent_id?.trim() || null;
    default:
      return agent.labels?.[normalized]?.trim() || null;
  }
}

function firstConfiguredValue(agent: ConsoleAgent, selectors: string[]): string | null {
  for (const selector of selectors) {
    const value = configuredFieldValue(agent, selector);
    if (value) return value;
  }
  return null;
}

function configuredAgentGroup(
  agent: ConsoleAgent,
  config: ConsoleAgentListConfig | undefined,
  parentById?: Map<string, string>,
  byId?: Map<string, ConsoleAgent>,
): string | null {
  const selectors = configuredSelectors(config, "group_by");
  if (selectors.length === 0) return null;
  const chain: ConsoleAgent[] = [];
  let current: ConsoleAgent | undefined = agent;
  const seen = new Set<string>();
  while (current && !seen.has(current.member_id)) {
    seen.add(current.member_id);
    chain.push(current);
    if (!parentById || !byId) break;
    const parentId = parentById.get(current.member_id);
    if (!parentId) break;
    current = byId.get(parentId);
  }
  const searchOrder = chain.length > 1 ? [...chain].reverse() : chain;
  for (const candidate of searchOrder) {
    const value = firstConfiguredValue(candidate, selectors);
    if (value) return value;
  }
  return config?.fallback_group?.trim() || "Agents";
}

function configuredAgentSubgroup(
  agent: ConsoleAgent,
  config: ConsoleAgentListConfig | undefined,
  parentById?: Map<string, string>,
  byId?: Map<string, ConsoleAgent>,
): string | null {
  const selectors = configuredSelectors(config, "subgroup_by");
  if (selectors.length === 0) return null;
  let current: ConsoleAgent | undefined = agent;
  const seen = new Set<string>();
  while (current) {
    const value = firstConfiguredValue(current, selectors);
    if (value) return value;
    if (!parentById || !byId || seen.has(current.member_id)) break;
    seen.add(current.member_id);
    const parentId = parentById.get(current.member_id);
    if (!parentId) break;
    current = byId.get(parentId);
  }
  return config?.fallback_subgroup?.trim() || null;
}

function configuredAgentBadges(agent: ConsoleAgent, config: ConsoleAgentListConfig | undefined): Array<{
  id: string;
  label: string;
  value: string;
  tone?: string;
}> {
  return (config?.badges || [])
    .map((badge) => {
      const value = configuredFieldValue(agent, badge.field || "");
      if (!badge.id || !badge.label || !value) return null;
      return {
        id: badge.id,
        label: badge.label,
        value,
        tone: badge.tone,
      };
    })
    .filter((badge): badge is { id: string; label: string; value: string; tone?: string } => Boolean(badge));
}

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

function compareRows(host: ConsoleAgent | null, orderSubgroups = false) {
  return (a: AgentRow, b: AgentRow): number => {
    if (orderSubgroups && a.subgroup !== b.subgroup) {
      if (!a.subgroup) return 1;
      if (!b.subgroup) return -1;
      return a.subgroup.localeCompare(b.subgroup);
    }
    if (host) {
      if (a.agent.member_id === host.member_id) return -1;
      if (b.agent.member_id === host.member_id) return 1;
    }
    if (a.childOfHost !== b.childOfHost) return a.childOfHost ? -1 : 1;
    return a.agent.label.localeCompare(b.agent.label);
  };
}

function orderRowsPreorder(rows: AgentRow[], parentById: Map<string, string>, host: ConsoleAgent | null, orderSubgroups = false): AgentRow[] {
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
  const sortRows = compareRows(host, orderSubgroups);
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

function groupSidebarAgents(filtered: ConsoleAgent[], config?: ConsoleAgentListConfig): Map<string, AgentRow[]> {
  const g = new Map<string, AgentRow[]>();
  const host = filtered.find(isCommanderLike);
  const byId = new Map(filtered.map((a) => [a.member_id, a]));
  const parentById = new Map<string, string>();
  for (const a of filtered) {
    const parent = findSpawnHost(a, filtered, host || null);
    if (parent) parentById.set(a.member_id, parent.member_id);
  }
  for (const a of filtered) {
    const childOfHost = parentById.has(a.member_id);
    const configuredGroup = configuredAgentGroup(a, config, parentById, byId);
    const key = configuredGroup || bucketForAgent(a, parentById, byId);
    const subgroup = configuredAgentSubgroup(a, config, parentById, byId);
    if (!g.has(key)) g.set(key, []);
    g.get(key)!.push({ agent: a, childOfHost, depth: depthForAgent(a, parentById), subgroup });
  }
  for (const [key, rows] of g.entries()) {
    g.set(key, orderRowsPreorder(rows, parentById, host || null, configuredSelectors(config, "subgroup_by").length > 0));
  }
  return g;
}

function orderedSectionNames(grouped: Map<string, AgentRow[]>, config?: ConsoleAgentListConfig): string[] {
  const names = Array.from(new Set([
    ...Array.from(grouped.keys()),
    ...(config?.sections || []).map((section) => section.name).filter(Boolean),
  ]));
  const configuredOrder = (config?.section_order || []).map((value) => value.trim()).filter(Boolean);
  const order = configuredOrder.length > 0 ? configuredOrder : SECTION_ORDER;
  const rank = new Map(order.map((name, index) => [name.toLowerCase(), index] as const));
  return names.sort((a, b) => {
    const ar = rank.get(a.toLowerCase()) ?? Number.MAX_SAFE_INTEGER;
    const br = rank.get(b.toLowerCase()) ?? Number.MAX_SAFE_INTEGER;
    if (ar !== br) return ar - br;
    return a.localeCompare(b);
  });
}

function sectionConfigFor(name: string, config?: ConsoleAgentListConfig) {
  const needle = name.toLowerCase();
  return (config?.sections || []).find((section) => section.name?.toLowerCase() === needle) || null;
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

function virtualRowHeight(row: SidebarVirtualRow): number {
  return SIDEBAR_ROW_HEIGHT[row.kind];
}

function lowerBound(values: number[], needle: number): number {
  let lo = 0;
  let hi = values.length;
  while (lo < hi) {
    const mid = Math.floor((lo + hi) / 2);
    if (values[mid] < needle) lo = mid + 1;
    else hi = mid;
  }
  return lo;
}

function useMeasuredHeight<T extends HTMLElement>(): [React.RefObject<T>, number] {
  const ref = React.useRef<T>(null);
  const [height, setHeight] = React.useState(0);
  React.useLayoutEffect(() => {
    const element = ref.current;
    if (!element) return undefined;
    const update = () => setHeight(element.clientHeight);
    update();
    if (typeof ResizeObserver === "undefined") {
      window.addEventListener("resize", update);
      return () => window.removeEventListener("resize", update);
    }
    const ro = new ResizeObserver((entries) => {
      const box = entries[0]?.contentRect;
      setHeight(box ? box.height : element.clientHeight);
    });
    ro.observe(element);
    return () => ro.disconnect();
  }, []);
  return [ref, height];
}

function renderAgentRow(
  row: AgentRow,
  selectedMemberId: string,
  recentActivity: ConsoleFrame[],
  grouping: ConsoleAgentListConfig | undefined,
  onSelect: (agent: ConsoleAgent) => void,
): React.JSX.Element {
  const { agent, childOfHost, depth } = row;
  const stateAttr = deriveStateAttr(agent);
  const pulse = pulseSamples(recentActivity, agent.identity || agent.member_id);
  const inbox = inboxCount(agent);
  const badges = configuredAgentBadges(agent, grouping);
  return (
    <div
      className={`agent ${childOfHost ? "agent--child" : ""} ${agent.member_id === selectedMemberId ? "is-active" : ""}`}
      data-state={stateAttr}
      data-child-of-host={childOfHost ? "true" : undefined}
      data-depth={childOfHost ? String(Math.min(depth, 3)) : undefined}
      data-testid={`sidebar-agent:${agent.member_id}`}
      onClick={() => onSelect(agent)}
      onKeyDown={(event) => {
        if (event.key === "Enter" || event.key === " ") {
          event.preventDefault();
          onSelect(agent);
        }
      }}
      role="button"
      tabIndex={0}
    >
      <span className="agent__dot" />
      <span className="agent__body">
        <span className="agent__name">{agent.label}</span>
        <span className="agent__id">{agent.identity || agent.member_id}</span>
        {badges.length > 0 ? (
          <span className="agent__badges">
            {badges.map((badge) => (
              <span
                className="agent__badge"
                data-tone={badge.tone || "neutral"}
                key={badge.id}
                title={`${badge.label}: ${badge.value}`}
              >
                <span>{badge.label}</span>
                <strong>{badge.value}</strong>
              </span>
            ))}
          </span>
        ) : null}
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
  customButtons,
  grouping,
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
    return groupSidebarAgents(filtered, grouping);
  }, [filtered, grouping]);
  const sectionNames = React.useMemo(() => orderedSectionNames(grouped, grouping), [grouped, grouping]);
  const defaultCollapsedKey = React.useMemo(
    () => JSON.stringify((grouping?.sections || []).map((section) => [section.name, section.collapsed === true])),
    [grouping?.sections],
  );
  const [collapsedSections, setCollapsedSections] = React.useState<Set<string>>(() => {
    return new Set((grouping?.sections || []).filter((section) => section.collapsed === true).map((section) => section.name));
  });
  React.useEffect(() => {
    setCollapsedSections(new Set((grouping?.sections || []).filter((section) => section.collapsed === true).map((section) => section.name)));
  }, [defaultCollapsedKey]);
  const customSidebarButtons = React.useMemo(
    () => (customButtons || []).filter((button) => button.id && button.label && (button.control || button.href)),
    [customButtons],
  );
  const virtualRows = React.useMemo<SidebarVirtualRow[]>(() => {
    const rows: SidebarVirtualRow[] = [];
    for (const bucket of sectionNames) {
      const list = grouped.get(bucket) || [];
      const sectionConfig = sectionConfigFor(bucket, grouping);
      if (list.length === 0 && !sectionConfig) continue;
      const collapsedSection = collapsedSections.has(bucket);
      rows.push({
        kind: "section",
        key: `section:${bucket}`,
        bucket,
        count: list.length,
        collapsed: collapsedSection,
      });
      if (collapsedSection) continue;
      if (list.length === 0) {
        rows.push({
          kind: "empty",
          key: `empty:${bucket}`,
          bucket,
          sectionConfig,
        });
        continue;
      }
      const subgroups = new Set(list.map((row) => row.subgroup).filter((value): value is string => Boolean(value)));
      const showSubgroups = configuredSelectors(grouping, "subgroup_by").length > 0
        && subgroups.size > (grouping?.collapse_single_subgroup === false ? 0 : 1);
      let lastSubgroup: string | null = null;
      for (const row of list) {
        if (showSubgroups && row.subgroup && row.subgroup !== lastSubgroup) {
          lastSubgroup = row.subgroup;
          rows.push({
            kind: "subgroup",
            key: `subgroup:${bucket}:${row.subgroup}`,
            bucket,
            label: row.subgroup,
          });
        }
        rows.push({
          kind: "agent",
          key: `agent:${row.agent.member_id}`,
          bucket,
          row,
        });
      }
    }
    return rows;
  }, [sectionNames, grouped, grouping, collapsedSections]);
  const virtualOffsets = React.useMemo(() => {
    const offsets: number[] = [];
    let total = 0;
    for (const row of virtualRows) {
      offsets.push(total);
      total += virtualRowHeight(row);
    }
    return { offsets, total };
  }, [virtualRows]);
  const [listRef, listHeight] = useMeasuredHeight<HTMLDivElement>();
  const [scrollTop, setScrollTop] = React.useState(0);
  React.useEffect(() => {
    setScrollTop(0);
    if (listRef.current) listRef.current.scrollTop = 0;
  }, [q, grouping, listRef]);
  const visibleRange = React.useMemo(() => {
    if (virtualRows.length === 0) return { start: 0, end: 0 };
    const startNeedle = Math.max(0, scrollTop - SIDEBAR_OVERSCAN_PX);
    const endNeedle = Math.min(virtualOffsets.total, scrollTop + Math.max(1, listHeight) + SIDEBAR_OVERSCAN_PX);
    const start = Math.max(0, lowerBound(virtualOffsets.offsets, startNeedle) - 1);
    const end = Math.min(virtualRows.length, lowerBound(virtualOffsets.offsets, endNeedle) + 1);
    return { start, end };
  }, [listHeight, scrollTop, virtualOffsets, virtualRows.length]);
  const visibleRows = React.useMemo(
    () => virtualRows.slice(visibleRange.start, visibleRange.end),
    [virtualRows, visibleRange],
  );

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

      {(navKinds.length > 0 || customSidebarButtons.length > 0) && (
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
          {customSidebarButtons.map((button) => {
            const control = normalizeNavKind(button.control);
            if (control) {
              return (
                <button
                  key={button.id}
                  className="sidebar__navitem"
                  onClick={() => onOpenControl(control)}
                  data-testid={`nav-custom:${button.id}`}
                  title={button.label}
                >
                  {button.label}
                </button>
              );
            }
            if (button.href) {
              return (
                <a
                  key={button.id}
                  className="sidebar__navitem"
                  href={button.href}
                  target={button.target || undefined}
                  rel={button.target === "_blank" ? "noreferrer" : undefined}
                  data-testid={`nav-custom:${button.id}`}
                  title={button.label}
                >
                  {button.label}
                </a>
              );
            }
            return null;
          })}
          </div>
        </div>
      )}

      <div
        className="sidebar__virtual-list"
        ref={listRef}
        onScroll={(event) => setScrollTop(event.currentTarget.scrollTop)}
        data-testid="sidebar-agent-list"
      >
        <div className="sidebar__virtual-space" style={{ height: `${virtualOffsets.total}px` }}>
          {visibleRows.map((row, index) => {
            const rowIndex = visibleRange.start + index;
            const top = virtualOffsets.offsets[rowIndex] || 0;
            const height = virtualRowHeight(row);
            return (
              <div
                className={`sidebar__virtual-row sidebar__virtual-row--${row.kind}`}
                key={row.key}
                style={{ transform: `translateY(${top}px)`, height: `${height}px` }}
              >
                {row.kind === "section" ? (
                  <div className="sidebar__section" data-collapsed={row.collapsed ? "true" : undefined}>
                    <button
                      type="button"
                      className="sidebar__sec-head sidebar__sec-head--button"
                      onClick={() => {
                        setCollapsedSections((current) => {
                          const next = new Set(current);
                          if (next.has(row.bucket)) next.delete(row.bucket);
                          else next.add(row.bucket);
                          return next;
                        });
                      }}
                      data-testid={`sidebar-section-toggle:${row.bucket}`}
                    >
                      <span className="sidebar__sec-label">{row.bucket}</span>
                      <span className="sidebar__sec-spacer" />
                      <span className="sidebar__sec-count">{row.count}</span>
                    </button>
                  </div>
                ) : row.kind === "empty" ? (
                  <div className="sidebar__empty" data-testid={`sidebar-section-empty:${row.bucket}`}>
                    {row.sectionConfig?.empty_title ? <span className="sidebar__empty-title">{row.sectionConfig.empty_title}</span> : null}
                    <span>{row.sectionConfig?.empty_text || "No agents in this section."}</span>
                  </div>
                ) : row.kind === "subgroup" ? (
                  <div className="sidebar__subgroup">
                    <span>{row.label}</span>
                  </div>
                ) : (
                  renderAgentRow(row.row, selectedMemberId, recentActivity, grouping, onSelect)
                )}
              </div>
            );
          })}
        </div>
      </div>
    </aside>
  );
}
