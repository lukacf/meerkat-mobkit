import type {
  ConsoleDockTargetAddressingMode,
  ConsoleSidebarMetaTone,
  ConsoleSidebarViewState,
  ConversationViewState,
  ConversationTimelineEntry,
  ConversationIdentity,
  ConsoleActivityRailViewState,
  ConsoleActivityPulseItem,
  ConsoleDockTarget,
  RoutingSectionView,
} from "@console-core";
import {
  groupConversationTimelineEntries,
  normalizeRoutingSectionView,
  normalizeSidebarWatchFields,
  parseConversationRichBlocks,
} from "@console-core";
import type { ConsoleAgent, ConsoleFrame } from "../types";

// ---------------------------------------------------------------------------
// Dock target
// ---------------------------------------------------------------------------

export interface MobKitDockTarget extends ConsoleDockTarget {
  kind: "agent-chat";
  addressingMode: ConsoleDockTargetAddressingMode;
  memberId: string;
  identity?: string;
}

export function buildPanelConversationKey(
  panelId: string,
  target: Pick<MobKitDockTarget, "identity" | "memberId" | "id" | "addressingMode"> | null,
): string {
  const targetKey = target?.addressingMode === "identity"
    ? target.identity || target.memberId || target.id
    : target?.memberId || target?.id || "none";
  return `panel:${panelId}:${targetKey}`;
}

export function buildDockTarget(agent: ConsoleAgent): MobKitDockTarget {
  const subtitle = [agent.profile, agent.kind].filter(Boolean).join(" \u00b7 ") || undefined;
  const identity = typeof agent.identity === "string" && agent.identity.trim()
    ? agent.identity.trim()
    : undefined;
  const addressingMode: ConsoleDockTargetAddressingMode = identity ? "identity" : "member";
  return {
    id: agent.member_id,
    kind: "agent-chat",
    addressingMode,
    memberId: agent.member_id,
    ...(identity ? { identity } : {}),
    title: agent.label,
    subtitle,
  };
}

// ---------------------------------------------------------------------------
// Sidebar
// ---------------------------------------------------------------------------

function agentGroupKey(agent: ConsoleAgent): string {
  return agent.group?.trim() || agent.profile?.trim() || agent.kind?.trim() || "Agents";
}

function agentStateTone(state: string | undefined): ConsoleSidebarMetaTone {
  switch (state) {
    case "running": return "accent";
    case "active": return "positive";
    case "idle": return "muted";
    case "error": return "negative";
    default: return "muted";
  }
}

function sectionIconForGroup(group: string): string | null {
  const lower = group.toLowerCase();
  if (lower.includes("coordinator") || lower.includes("system")) return "i-bolt";
  if (lower.includes("domain") || lower.includes("specialist")) return "i-cube";
  if (lower.includes("internal") || lower.includes("infra")) return "i-gear";
  if (lower.includes("personal") || lower.includes("identity")) return "i-team";
  return "i-folder";
}

export function buildSidebarViewState(args: {
  agents: ConsoleAgent[];
  selectedMemberId: string;
  pinnedAgentIds?: Set<string>;
  sortMode?: "group" | "alpha" | "status";
}): ConsoleSidebarViewState {
  const { agents, selectedMemberId, pinnedAgentIds = new Set(), sortMode = "group" } = args;

  // Sort agents within groups
  const sorted = [...agents].sort((a, b) => {
    // Pinned first
    const aPinned = pinnedAgentIds.has(a.member_id) ? 0 : 1;
    const bPinned = pinnedAgentIds.has(b.member_id) ? 0 : 1;
    if (aPinned !== bPinned) return aPinned - bPinned;

    if (sortMode === "alpha") return a.label.localeCompare(b.label);
    if (sortMode === "status") {
      const stateOrder = (s: string | undefined) => s === "running" ? 0 : s === "active" ? 1 : 2;
      const diff = stateOrder(a.state) - stateOrder(b.state);
      if (diff !== 0) return diff;
    }
    return a.label.localeCompare(b.label);
  });

  // Group agents
  const grouped = new Map<string, ConsoleAgent[]>();
  for (const agent of sorted) {
    const key = agentGroupKey(agent);
    const bucket = grouped.get(key) || [];
    bucket.push(agent);
    grouped.set(key, bucket);
  }

  // Build sections
  const sections = Array.from(grouped.entries()).map(([group, members]) => ({
    id: group,
    title: group,
    iconName: sectionIconForGroup(group),
    meta: [{ id: "count", label: `${members.length}` }] as { id: string; label: string; tone?: "default" | "muted" | "accent" | "positive" | "negative" }[],
    actions: [
      { id: "spawn_in_group", label: `Spawn agent in ${group}`, iconName: "i-plus" },
    ],
    items: members.map((agent) => {
      const isAddressable = agent.addressable || agent.affordances?.can_send_message;
      const isPinned = pinnedAgentIds.has(agent.member_id);
      const watchFields = normalizeSidebarWatchFields(agent);
      return {
        id: agent.member_id,
        title: agent.label,
        subtitle: agent.member_id,
        selected: agent.member_id === selectedMemberId,
        pinned: isPinned,
        disabled: !isAddressable,
        ...watchFields,
        meta: [
          ...(agent.state
            ? [{ id: "state", label: agent.state, tone: agentStateTone(agent.state) }]
            : []),
        ],
        actions: [
          {
            id: "toggle_pin",
            label: isPinned ? "Unpin agent" : "Pin agent",
            iconName: "i-pin",
            active: isPinned,
          },
        ],
      };
    }),
  }));

  return {
    blocks: [
      // Action strip: top-level controls
      {
        id: "controls",
        kind: "action_strip" as const,
        actions: [
          { id: "spawn_agent", label: "Spawn agent", iconName: "i-plus" },
          { id: "reconcile", label: "Reconcile", iconName: "i-refresh" },
        ],
      },
      // Agent list — "Agents" header with inline sort/add actions (like meerkat-app "Threads")
      {
        id: "agents",
        kind: "list" as const,
        title: "Agents",
        actions: [
          { id: "spawn_agent", label: "Spawn agent", iconName: "i-plus" },
          { id: "filter_sort", label: "Sort & filter", iconName: "i-sliders" },
        ],
        sections,
      },
    ],
  };
}

// ---------------------------------------------------------------------------
// Routing
// ---------------------------------------------------------------------------

export function buildRoutingSectionView(args: {
  routesResponse: unknown;
  historyResponse: unknown;
}): RoutingSectionView {
  const routesRecord = typeof args.routesResponse === "object" && args.routesResponse !== null
    ? args.routesResponse as Record<string, unknown>
    : {};
  const historyRecord = typeof args.historyResponse === "object" && args.historyResponse !== null
    ? args.historyResponse as Record<string, unknown>
    : {};
  const normalized = normalizeRoutingSectionView({
    routes: Array.isArray(routesRecord.routes) ? routesRecord.routes : [],
    deliveries: Array.isArray(historyRecord.deliveries) ? historyRecord.deliveries : [],
  });

  return normalized ?? { routes: [], deliveries: [] };
}

// ---------------------------------------------------------------------------
// Conversation identities
// ---------------------------------------------------------------------------

const USER_IDENTITY: ConversationIdentity = {
  id: "user",
  label: "You",
  role: "user",
};

function agentIdentity(agent: ConsoleAgent | null): ConversationIdentity {
  return {
    id: agent?.member_id || "agent",
    label: agent?.label || "Agent",
    role: "assistant",
  };
}

const SYSTEM_IDENTITY: ConversationIdentity = {
  id: "system",
  label: "System",
  role: "system",
  presentation: "system",
  showLabel: true,
};

// ---------------------------------------------------------------------------
// Frame → timeline entry
// ---------------------------------------------------------------------------

function summarizeFrameData(data: unknown): string {
  if (typeof data === "string") return data;
  if (typeof data === "object" && data !== null) {
    const record = data as Record<string, unknown>;
    if (typeof record.delta === "string" && record.delta.trim()) return record.delta;
    if (typeof record.text === "string" && record.text.trim()) return record.text;
    if (typeof record.result === "string" && record.result.trim()) return record.result;
    if (typeof record.message === "string" && record.message.trim()) return record.message;
    if (typeof record.error === "string" && record.error.trim()) return record.error;
    if (typeof record.kind === "string" && typeof record.event_type === "string") return "";
    return JSON.stringify(record);
  }
  return String(data ?? "");
}

// Infrastructure events that should not appear in the conversation transcript.
const HIDDEN_EVENTS = new Set([
  "subscribed",
  "run_started",
  "run_completed",
  "turn_started",
  "turn_completed",
  "text_complete",
  "interaction_started",
  "run_failed",
  "keep-alive",
]);

function renderTerminalEntry(
  agent: ConsoleAgent | null,
  frame: ConsoleFrame,
  entryId: string,
): ConversationTimelineEntry | null {
  if (frame.event === "interaction_complete") {
    const text = summarizeFrameData(frame.data).trim();
    if (!text) return null;
    const blocks = parseConversationRichBlocks(text);
    return {
      kind: "message",
      id: entryId,
      identity: agentIdentity(agent),
      variant: blocks.length > 0 ? "rich" : "plain",
      ...(blocks.length > 0 ? { blocks } : { text }),
    };
  }

  if (frame.event === "interaction_failed" || frame.event === "run_failed") {
    const text = `${frame.event}: ${summarizeFrameData(frame.data)}`.trim();
    if (!text || text === `${frame.event}:`) return null;
    return {
      kind: "message",
      id: entryId,
      identity: SYSTEM_IDENTITY,
      variant: "meta",
      text,
    };
  }

  return null;
}

export function mapFramesToTimelineEntries(
  agent: ConsoleAgent | null,
  frames: ConsoleFrame[],
): ConversationTimelineEntry[] {
  const entries: ConversationTimelineEntry[] = [];

  // Accumulate consecutive text_delta frames into a single message
  let pendingText = "";
  let pendingId = "";

  function flushPendingText() {
    if (!pendingText) return;
    const blocks = parseConversationRichBlocks(pendingText);
    entries.push({
      kind: "message",
      id: pendingId,
      identity: agentIdentity(agent),
      variant: blocks.length > 0 ? "rich" : "plain",
      ...(blocks.length > 0 ? { blocks } : { text: pendingText }),
    });
    pendingText = "";
    pendingId = "";
  }

  for (let i = 0; i < frames.length; i++) {
    const frame = frames[i];
    const entryId = `${frame.id || frame.event || "frame"}:${i}`;

    if (frame.event === "text_delta") {
      if (!pendingId) pendingId = entryId;
      const delta = typeof (frame.data as Record<string, unknown>)?.delta === "string"
        ? (frame.data as Record<string, unknown>).delta as string
        : summarizeFrameData(frame.data);
      pendingText += delta;
      continue;
    }

    // Flush any accumulated text before processing a non-text event
    flushPendingText();

    const terminalEntry = renderTerminalEntry(agent, frame, entryId);
    if (terminalEntry) {
      entries.push(terminalEntry);
      continue;
    }

    // Tool calls → command block
    if (frame.event === "tool_call") {
      const record = frame.data as Record<string, unknown> | null;
      const toolName = typeof record?.name === "string" ? record.name : "tool";
      const args = typeof record?.arguments === "string" ? record.arguments : JSON.stringify(record || {});
      entries.push({
        kind: "message",
        id: entryId,
        identity: agentIdentity(agent),
        variant: "rich",
        blocks: [{
          type: "command",
          caption: "Tool call",
          title: toolName,
          body: args,
        }],
      });
      continue;
    }

    // Skip infrastructure noise
    if (HIDDEN_EVENTS.has(frame.event)) continue;

    // Remaining events → system meta (errors, unexpected events)
    const text = `${frame.event}: ${summarizeFrameData(frame.data)}`.trim();
    entries.push({
      kind: "message",
      id: entryId,
      identity: SYSTEM_IDENTITY,
      variant: "meta",
      text,
    });
  }

  flushPendingText();
  return entries;
}

export function createUserEntry(message: string): ConversationTimelineEntry {
  return {
    kind: "message",
    id: `user:${Date.now()}`,
    identity: USER_IDENTITY,
    variant: "plain",
    text: message,
  };
}

// ---------------------------------------------------------------------------
// Conversation view state
// ---------------------------------------------------------------------------

export function buildConversationViewState(args: {
  memberId: string;
  agentLabel: string;
  entries: ConversationTimelineEntry[];
}): ConversationViewState {
  const groups = groupConversationTimelineEntries(args.entries);
  return {
    conversationId: args.memberId || "console",
    title: args.agentLabel,
    entries: args.entries,
    groups,
    turnDiff: null,
    emptyState: args.entries.length === 0 ? {
      title: args.agentLabel,
      subtitle: "Send a message to start the conversation.",
    } : null,
  };
}

// ---------------------------------------------------------------------------
// Activity rail
// ---------------------------------------------------------------------------

export function buildActivityRailViewState(args: {
  eventFrames: ConsoleFrame[];
}): ConsoleActivityRailViewState {
  // Chronological event feed only
  const pulseItems: ConsoleActivityPulseItem[] = args.eventFrames
    .slice(0, 50)
    .map((frame, index) => ({
      id: `event:${frame.id || index}`,
      title: frame.event || "event",
      line: summarizeFrameData(frame.data).slice(0, 120) || frame.event,
      meta: frame.id || "",
    }));

  return {
    panels: [
      {
        id: "pulse",
        kind: "pulse" as const,
        title: "Events",
        items: pulseItems,
        emptyText: "No events yet",
      },
    ],
  };
}
