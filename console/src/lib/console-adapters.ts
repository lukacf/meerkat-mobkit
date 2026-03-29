import type { ConsoleAgent, ConsoleFrame } from "../types";
import {
  groupConversationEntries,
  type ConsoleSidebarMeta,
  type ConsoleSidebarViewState,
  type ConversationEntry,
  type ConversationIdentity,
  type ConversationViewState,
} from "../shared-console";

function groupLabel(agent: ConsoleAgent): string {
  return (
    agent.group?.trim()
    || agent.profile?.trim()
    || agent.kind?.trim()
    || "Agents"
  );
}

function subtitleForAgent(agent: ConsoleAgent): string {
  return [agent.profile, agent.kind].filter(Boolean).join(" · ") || "member";
}

function metaForAgent(agent: ConsoleAgent): ConsoleSidebarMeta[] {
  const meta: ConsoleSidebarMeta[] = [];

  if (agent.state) {
    meta.push({
      id: "state",
      label: agent.state,
      tone: agent.state === "running" ? "accent" : "muted",
    });
  }
  if (agent.addressable || agent.affordances?.can_send_message) {
    meta.push({
      id: "addressable",
      label: "addressable",
      tone: "muted",
    });
  }

  return meta;
}

export function buildAgentSidebarViewState(args: {
  title: string;
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
      title: args.title,
      sections: Array.from(grouped.entries()).map(([label, members]) => ({
        id: label,
        title: label,
        items: members.map((agent) => ({
          id: agent.member_id,
          title: agent.label,
          subtitle: subtitleForAgent(agent),
          meta: metaForAgent(agent),
          selected: agent.member_id === args.selectedMemberId,
        })),
      })),
    }],
  };
}

function summarizeFrameData(data: unknown): string {
  if (typeof data === "string") {
    return data;
  }
  if (typeof data === "object" && data !== null) {
    const record = data as Record<string, unknown>;
    if (typeof record.delta === "string" && record.delta.trim()) {
      return record.delta;
    }
    if (typeof record.result === "string" && record.result.trim()) {
      return record.result;
    }
    if (typeof record.message === "string" && record.message.trim()) {
      return record.message;
    }
    if (typeof record.error === "string" && record.error.trim()) {
      return record.error;
    }
    // Persisted UnifiedEvent summary (kind + event_type only, no payload).
    // Return empty so the entry renders as just the event type name.
    if (typeof record.kind === "string" && typeof record.event_type === "string") {
      return "";
    }
    return JSON.stringify(record);
  }
  return String(data ?? "");
}

function identityForFrame(agent: ConsoleAgent | null, frame: ConsoleFrame): ConversationIdentity {
  if (frame.event === "subscribed") {
    return {
      id: "system",
      label: "System",
      presentation: "system",
    };
  }

  if (frame.event === "text_delta" || frame.event === "tool_call") {
    return {
      id: agent?.member_id || "agent",
      label: agent?.label || agent?.member_id || "Agent",
      presentation: "participant",
    };
  }

  return {
    id: "system",
    label: "System",
    presentation: "system",
  };
}

export function createUserConversationEntry(message: string): ConversationEntry {
  return {
    id: `user:${Date.now()}`,
    identity: {
      id: "user",
      label: "You",
      presentation: "user",
    },
    text: message,
  };
}

export function mapFramesToConversationEntries(
  agent: ConsoleAgent | null,
  frames: ConsoleFrame[]
): ConversationEntry[] {
  return frames.map((frame, index) => ({
    id: `${frame.id || frame.event || "frame"}:${index}`,
    identity: identityForFrame(agent, frame),
    text: `${frame.event}: ${summarizeFrameData(frame.data)}`.trim(),
  }));
}

export function buildConversationViewState(args: {
  conversationId: string;
  title: string;
  entries: ConversationEntry[];
  selectedAgentLabel: string;
}): ConversationViewState {
  return {
    conversationId: args.conversationId,
    title: args.title,
    entries: args.entries,
    groups: groupConversationEntries(args.entries),
    emptyTitle: `Talk to ${args.selectedAgentLabel}`,
    emptySubtitle: "Select an agent in the sidebar and send a message to start the console transcript.",
  };
}
