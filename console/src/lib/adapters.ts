import type {
  ConsoleSidebarViewState,
  ConversationViewState,
  ConversationTimelineEntry,
  ConversationIdentity,
  ConsoleActivityRailViewState,
  ConsoleActivityPulseItem,
  ConsoleActivityItem,
  ConsoleDockTarget,
} from "@console-core";
import {
  groupConversationTimelineEntries,
  parseConversationRichBlocks,
} from "@console-core";
import type { ConsoleAgent, ConsoleFrame } from "../types";

// ---------------------------------------------------------------------------
// Dock target
// ---------------------------------------------------------------------------

export interface MobKitDockTarget extends ConsoleDockTarget {
  kind: "agent-chat";
}

export function buildDockTarget(agent: ConsoleAgent): MobKitDockTarget {
  const subtitle = [agent.profile, agent.kind].filter(Boolean).join(" \u00b7 ") || undefined;
  return {
    id: agent.member_id,
    kind: "agent-chat",
    title: agent.label,
    subtitle,
  };
}

// ---------------------------------------------------------------------------
// Sidebar
// ---------------------------------------------------------------------------

function groupLabel(agent: ConsoleAgent): string {
  return agent.group?.trim() || agent.profile?.trim() || agent.kind?.trim() || "Agents";
}

export function buildSidebarViewState(args: {
  agents: ConsoleAgent[];
  selectedMemberId: string;
}): ConsoleSidebarViewState {
  const grouped = new Map<string, ConsoleAgent[]>();
  for (const agent of args.agents) {
    const label = groupLabel(agent);
    const bucket = grouped.get(label) || [];
    bucket.push(agent);
    grouped.set(label, bucket);
  }

  return {
    blocks: [{
      id: "agents",
      kind: "list",
      title: "Agents",
      sections: Array.from(grouped.entries()).map(([label, members]) => ({
        id: label,
        title: label,
        items: members.map((agent) => ({
          id: agent.member_id,
          title: agent.label,
          subtitle: [agent.profile, agent.kind].filter(Boolean).join(" \u00b7 ") || "member",
          selected: agent.member_id === args.selectedMemberId,
          meta: [
            ...(agent.state
              ? [{ id: "state", label: agent.state, tone: agent.state === "running" ? "accent" as const : "muted" as const }]
              : []),
            ...((agent.addressable || agent.affordances?.can_send_message)
              ? [{ id: "addressable", label: "addressable", tone: "muted" as const }]
              : []),
          ],
        })),
      })),
    }],
  };
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
  "interaction_complete",
  "interaction_failed",
  "run_failed",
  "keep-alive",
]);

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
      title: `Talk to ${args.agentLabel}`,
      subtitle: "Send a message to start the conversation.",
    } : null,
  };
}

// ---------------------------------------------------------------------------
// Activity rail
// ---------------------------------------------------------------------------

export function buildActivityRailViewState(args: {
  agents: ConsoleAgent[];
  eventFrames: ConsoleFrame[];
}): ConsoleActivityRailViewState {
  // Roster panel: agents grouped by state
  const stateGroups = new Map<string, ConsoleActivityItem[]>();
  for (const agent of args.agents) {
    const state = agent.state || "unknown";
    const bucket = stateGroups.get(state) || [];
    bucket.push({
      id: agent.member_id,
      focusId: agent.member_id,
      title: agent.label,
      subtitle: agent.profile || agent.kind || "member",
      meta: agent.member_id,
    });
    stateGroups.set(state, bucket);
  }

  const rosterGroups = Array.from(stateGroups.entries()).map(([state, items]) => ({
    id: state,
    title: state.charAt(0).toUpperCase() + state.slice(1),
    meta: `${items.length}`,
    items,
  }));

  // Pulse panel: recent event frames
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
        id: "roster",
        kind: "roster" as const,
        title: "Agents",
        meta: `${args.agents.length}`,
        groups: rosterGroups,
        emptyText: "No agents loaded",
      },
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
