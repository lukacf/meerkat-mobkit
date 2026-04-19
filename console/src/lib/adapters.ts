import type {
  ActivityFilterPreset,
  ConsoleActivityPulseItem,
  ConsoleActivityRailViewState,
  ConsoleDockTarget,
  ConsoleDockTargetAddressingMode,
  ConsoleSidebarMetaTone,
  ConsoleSidebarViewState,
  ConversationEmptySuggestion,
  ConversationIdentity,
  ConversationRichToolCallBlock,
  ConversationTimelineEntry,
  ConversationViewState,
  ResponsePhase,
  RoutingSectionView,
} from "@console-core";
import {
  groupConversationTimelineEntries,
  normalizeRoutingSectionView,
  normalizeSidebarWatchFields,
  parseConversationRichBlocks,
} from "@console-core";
import type { ConsoleAgent, ConsoleFrame } from "../types";

export type MobKitDockTarget =
  | AgentChatTarget
  | IdentityInspectTarget
  | RoutingPanelTarget
  | GatingPanelTarget
  | TopologyPanelTarget
  | HealthPanelTarget
  | TimelinePanelTarget
  | RosterPanelTarget
  | GatesPanelTarget
  | LogsPanelTarget;

export interface AgentChatTarget extends ConsoleDockTarget {
  kind: "agent-chat";
  addressingMode: ConsoleDockTargetAddressingMode;
  memberId: string;
  identity?: string;
}

export interface IdentityInspectTarget extends ConsoleDockTarget {
  kind: "identity-inspect";
  identity: string;
  memberId: string;
}

export interface RoutingPanelTarget extends ConsoleDockTarget {
  kind: "routing";
}

export interface GatingPanelTarget extends ConsoleDockTarget {
  kind: "gating";
}

export interface TopologyPanelTarget extends ConsoleDockTarget {
  kind: "topology";
}

export interface HealthPanelTarget extends ConsoleDockTarget {
  kind: "health";
}

export interface TimelinePanelTarget extends ConsoleDockTarget {
  kind: "timeline";
}

export interface RosterPanelTarget extends ConsoleDockTarget {
  kind: "roster";
}

export interface GatesPanelTarget extends ConsoleDockTarget {
  kind: "gates";
}

export interface LogsPanelTarget extends ConsoleDockTarget {
  kind: "logs";
}

export function buildPanelConversationKey(
  panelId: string,
  target: Pick<MobKitDockTarget, "kind" | "identity" | "memberId" | "id" | "addressingMode"> | null,
): string {
  if (!target) {
    return `panel:${panelId}:none`;
  }
  if (target.kind !== "agent-chat") {
    return `panel:${panelId}:${target.kind}:${target.id}`;
  }
  const targetKey = target.addressingMode === "identity"
    ? target.identity || target.memberId || target.id
    : target.memberId || target.id;
  return `panel:${panelId}:${target.kind}:${targetKey}`;
}

export function buildDockTarget(agent: ConsoleAgent): AgentChatTarget {
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
    iconName: "i-team",
  };
}

export function buildInspectTarget(agent: ConsoleAgent): IdentityInspectTarget {
  return {
    id: `inspect:${agent.identity || agent.member_id}`,
    kind: "identity-inspect",
    identity: agent.identity || agent.member_id,
    memberId: agent.member_id,
    title: `${agent.label} Inspect`,
    subtitle: agent.identity || agent.member_id,
    iconName: "i-terminal",
  };
}

export type ControlTargetKind =
  | "routing" | "gating" | "topology" | "health"
  | "timeline" | "roster" | "gates" | "logs";

export function buildControlTarget(kind: ControlTargetKind): MobKitDockTarget {
  switch (kind) {
    case "routing":
      return { id: "routing", kind, title: "Routing", subtitle: "Routes and delivery history", iconName: "i-swap" };
    case "gating":
      return { id: "gating", kind, title: "Gating", subtitle: "Pending approvals and audit", iconName: "i-bolt" };
    case "topology":
      return { id: "topology", kind, title: "Topology", subtitle: "Identity connectivity", iconName: "i-team" };
    case "health":
      return { id: "health", kind, title: "Health", subtitle: "Runtime and identity health", iconName: "i-gear" };
    case "timeline":
      return { id: "timeline", kind, title: "Today", subtitle: "Chronological events", iconName: "i-clock" };
    case "roster":
      return { id: "roster", kind, title: "Roster", subtitle: "All agents", iconName: "i-team" };
    case "gates":
      return { id: "gates", kind, title: "Gates", subtitle: "Approval policies", iconName: "i-bolt" };
    case "logs":
      return { id: "logs", kind, title: "Logs", subtitle: "Event stream", iconName: "i-terminal" };
    default:
      return { id: "health", kind: "health", title: "Health" };
  }
}

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

  const sorted = [...agents].sort((a, b) => {
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

  const grouped = new Map<string, ConsoleAgent[]>();
  for (const agent of sorted) {
    const key = agentGroupKey(agent);
    const bucket = grouped.get(key) || [];
    bucket.push(agent);
    grouped.set(key, bucket);
  }

  const sections = Array.from(grouped.entries()).map(([group, members]) => ({
    id: group,
    title: group,
    iconName: sectionIconForGroup(group),
    meta: [{ id: "count", label: `${members.length}` }] as Array<{ id: string; label: string; tone?: ConsoleSidebarMetaTone }>,
    items: members.map((agent) => {
      const isAddressable = agent.addressable || agent.affordances?.can_send_message;
      const isPinned = pinnedAgentIds.has(agent.member_id);
      const watchFields = normalizeSidebarWatchFields(agent);
      return {
        id: agent.member_id,
        title: agent.label,
        subtitle: agent.identity || agent.member_id,
        selected: agent.member_id === selectedMemberId,
        pinned: isPinned,
        disabled: !isAddressable,
        ...watchFields,
        meta: [
          ...(agent.state ? [{ id: "state", label: agent.state, tone: agentStateTone(agent.state) }] : []),
          ...(agent.response_phase ? [{ id: "phase", label: agent.response_phase, tone: "accent" as const }] : []),
        ],
        actions: [
          {
            id: "inspect_identity",
            label: "Inspect identity",
            iconName: "i-terminal",
          },
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
      {
        id: "controls",
        kind: "action_strip" as const,
        actions: [
          { id: "open_routing", label: "Routing", iconName: "i-swap" },
          { id: "open_gating", label: "Gating", iconName: "i-bolt" },
          { id: "open_topology", label: "Topology", iconName: "i-team" },
          { id: "open_health", label: "Health", iconName: "i-gear" },
        ],
      },
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

function summarizeFrameData(data: unknown): string {
  if (typeof data === "string") {
    const trimmed = data.trim();
    if ((trimmed.startsWith("{") && trimmed.endsWith("}")) || (trimmed.startsWith("[") && trimmed.endsWith("]"))) {
      try {
        return summarizeFrameData(JSON.parse(trimmed));
      } catch {
        return data;
      }
    }
    return data;
  }
  if (typeof data === "object" && data !== null) {
    const record = data as Record<string, unknown>;
    if (typeof record.delta === "string") return record.delta;
    if (typeof record.text === "string" && record.text.trim()) return record.text;
    if (typeof record.result === "string" && record.result.trim()) return record.result;
    if (typeof record.message === "string" && record.message.trim()) return record.message;
    if (typeof record.error === "string" && record.error.trim()) return record.error;
    if (typeof record.reason === "string" && record.reason.trim()) return record.reason;
    if (typeof record.kind === "string" && typeof record.event_type === "string") return "";
    return JSON.stringify(record);
  }
  return String(data ?? "");
}

function eventSortRank(event: string | undefined): number {
  switch (event) {
    case "interaction_started":
      return 0;
    case "tool_call_requested":
    case "tool_call":
    case "tool_execution_started":
      return 20;
    case "tool_result_received":
    case "tool_execution_completed":
      return 30;
    case "text_delta":
      return 40;
    case "text_complete":
      return 45;
    case "interaction_complete":
    case "interaction_failed":
    case "run_completed":
    case "run_failed":
      return 90;
    default:
      return 50;
  }
}

function sortFramesForTranscript(frames: ConsoleFrame[]): ConsoleFrame[] {
  const interactionStartMs = new Map<string, number>();
  for (const frame of frames) {
    const interactionId = frame.interactionId?.trim();
    const timestampMs = typeof frame.timestampMs === "number" ? frame.timestampMs : Number.MAX_SAFE_INTEGER;
    if (!interactionId) continue;
    const current = interactionStartMs.get(interactionId);
    if (current === undefined || timestampMs < current) {
      interactionStartMs.set(interactionId, timestampMs);
    }
  }

  return frames
    .map((frame, index) => ({ frame, index }))
    .sort((left, right) => {
      const leftInteraction = left.frame.interactionId?.trim() || "";
      const rightInteraction = right.frame.interactionId?.trim() || "";
      const leftGroupTs =
        (leftInteraction && interactionStartMs.get(leftInteraction))
        ?? (typeof left.frame.timestampMs === "number" ? left.frame.timestampMs : Number.MAX_SAFE_INTEGER);
      const rightGroupTs =
        (rightInteraction && interactionStartMs.get(rightInteraction))
        ?? (typeof right.frame.timestampMs === "number" ? right.frame.timestampMs : Number.MAX_SAFE_INTEGER);
      if (leftGroupTs !== rightGroupTs) {
        return leftGroupTs - rightGroupTs;
      }
      if (leftInteraction && rightInteraction && leftInteraction === rightInteraction) {
        const leftRank = eventSortRank(left.frame.event);
        const rightRank = eventSortRank(right.frame.event);
        if (leftRank !== rightRank) {
          return leftRank - rightRank;
        }
      }
      const leftTs = typeof left.frame.timestampMs === "number" ? left.frame.timestampMs : Number.MAX_SAFE_INTEGER;
      const rightTs = typeof right.frame.timestampMs === "number" ? right.frame.timestampMs : Number.MAX_SAFE_INTEGER;
      if (leftTs !== rightTs) {
        return leftTs - rightTs;
      }
      return left.index - right.index;
    })
    .map(({ frame }) => frame);
}

const HIDDEN_EVENTS = new Set([
  "subscribed",
  "run_started",
  "run_completed",
  "turn_started",
  "turn_completed",
  "text_complete",
  "reasoning_delta",
  "reasoning_complete",
  "interaction_started",
  "run_failed",
  "keep-alive",
  "tool_config_changed",
  "tool_scope_changed",
]);

const ACTIVITY_HIDDEN_EVENTS = new Set([
  ...HIDDEN_EVENTS,
  "text_delta",
  "tool_call_requested",
  "tool_call",
  "tool_execution_started",
  "tool_result_received",
  "tool_execution_completed",
]);

export function mergeConversationFrames(...frameSets: Array<ConsoleFrame[] | undefined>): ConsoleFrame[] {
  const byId = new Map<string, ConsoleFrame>();
  const ordered: ConsoleFrame[] = [];

  for (const frameSet of frameSets) {
    for (const frame of frameSet || []) {
      const key = frame.id || `${frame.event}:${frame.timestampMs || 0}`;
      if (byId.has(key)) {
        continue;
      }
      byId.set(key, frame);
      ordered.push(frame);
    }
  }

  return ordered;
}

function isoFromTimestampMs(timestampMs: number | undefined): string | undefined {
  if (typeof timestampMs !== "number" || !Number.isFinite(timestampMs)) {
    return undefined;
  }
  return new Date(timestampMs).toISOString();
}

function parseToolCallId(frame: ConsoleFrame): string | null {
  const record = frame.data && typeof frame.data === "object" ? frame.data as Record<string, unknown> : null;
  const id = record?.tool_call_id ?? record?.id;
  return typeof id === "string" && id.trim() ? id.trim() : null;
}

function parseToolName(frame: ConsoleFrame): string {
  const record = frame.data && typeof frame.data === "object" ? frame.data as Record<string, unknown> : null;
  return typeof record?.name === "string" && record.name.trim() ? record.name : "tool";
}

function parseToolArguments(frame: ConsoleFrame): string {
  const record = frame.data && typeof frame.data === "object" ? frame.data as Record<string, unknown> : null;
  if (typeof record?.arguments === "string" && record.arguments.trim()) {
    return record.arguments;
  }
  if ("args" in (record || {}) && record?.args !== undefined) {
    return JSON.stringify(record.args);
  }
  return JSON.stringify(record || {});
}

function parseToolResult(frame: ConsoleFrame): { result?: string; status: "pending" | "success" | "error" } {
  const record = frame.data && typeof frame.data === "object" ? frame.data as Record<string, unknown> : null;
  const isError = Boolean(record?.is_error) || frame.event === "interaction_failed";

  // Extract actual result content — prefer tool_execution_completed which has the real result
  let result = "";
  if (typeof record?.result === "string") {
    // Try to parse JSON result and format it readably
    try {
      const parsed = JSON.parse(record.result);
      if (typeof parsed === "object" && parsed !== null) {
        // Remove metadata keys, keep the actual content
        const clean = { ...parsed };
        delete clean.source_event_type;
        delete clean.type;
        result = JSON.stringify(clean, null, 2);
      } else {
        result = record.result;
      }
    } catch {
      result = record.result;
    }
  } else if (typeof record?.result === "object" && record.result !== null) {
    const clean = { ...(record.result as Record<string, unknown>) };
    delete clean.source_event_type;
    delete clean.type;
    result = JSON.stringify(clean, null, 2);
  }

  // For tool_result_received without a result field, don't use the metadata dump
  if (!result && frame.event === "tool_result_received") {
    return { status: isError ? "error" : "success" };
  }

  return {
    ...(result ? { result } : {}),
    status: isError ? "error" : "success",
  };
}

function buildToolBlocks(frames: ConsoleFrame[]): Map<string, ConversationRichToolCallBlock> {
  const toolCalls = new Map<string, ConversationRichToolCallBlock>();
  const pendingResults = new Map<string, { result?: string; status: "success" | "error" }>();

  for (const frame of frames) {
    if (frame.event === "tool_result_received" || frame.event === "tool_execution_completed") {
      const toolCallId = parseToolCallId(frame);
      if (!toolCallId) continue;
      const parsed = parseToolResult(frame);
      if (toolCalls.has(toolCallId)) {
        const current = toolCalls.get(toolCallId)!;
        toolCalls.set(toolCallId, {
          ...current,
          ...(parsed.result ? { result: parsed.result } : {}),
          status: parsed.status,
        });
      } else {
        pendingResults.set(toolCallId, parsed);
      }
    }

    if (frame.event === "tool_call_requested" || frame.event === "tool_call" || frame.event === "tool_execution_started") {
      const toolCallId = parseToolCallId(frame);
      if (!toolCallId || toolCalls.has(toolCallId)) continue;
      const pending = pendingResults.get(toolCallId);
      const name = parseToolName(frame);
      const args = frame.data && typeof frame.data === "object" ? (frame.data as Record<string, unknown>).args : null;
      const argsRecord = args && typeof args === "object" ? args as Record<string, unknown> : null;

      // Extract peer comms metadata for send_* tools
      const isPeerTool = name === "send_request" || name === "send_message" || name === "send_response";
      const peerTarget = isPeerTool && typeof argsRecord?.to === "string"
        ? argsRecord.to.split("/").pop() || argsRecord.to as string
        : undefined;
      const peerIntent = isPeerTool && typeof argsRecord?.intent === "string"
        ? argsRecord.intent as string
        : undefined;
      const peerBody = isPeerTool
        ? typeof argsRecord?.body === "string"
          ? argsRecord.body as string
          : typeof argsRecord?.params === "object" && argsRecord.params !== null
            ? JSON.stringify(argsRecord.params)
            : undefined
        : undefined;

      toolCalls.set(toolCallId, {
        type: "tool-call",
        toolCallId,
        name,
        arguments: parseToolArguments(frame),
        ...(pending?.result ? { result: pending.result } : {}),
        status: pending?.status || "pending",
        ...(peerTarget ? { peerTarget } : {}),
        ...(peerIntent ? { peerIntent } : {}),
        ...(peerBody ? { peerBody } : {}),
      });
    }
  }

  return toolCalls;
}

function parsePeerSummary(text: string): { verb: string; summary: string } | null {
  // Match "Peer response: ..." / "Peer request: ..." / "Peer message: ..." at any position
  const match = text.match(/Peer\s+(response|request|message):\s*(.+?)(?:\s*Status:\s|$)/s);
  if (!match) return null;

  const [, verb, body] = match;
  let summary = body.trim();

  // Try to parse JSON payload and extract clean text
  try {
    const parsed = JSON.parse(summary);
    if (typeof parsed === "object" && parsed !== null) {
      if (typeof parsed.summary === "string") summary = parsed.summary;
      else if (typeof parsed.text === "string") summary = parsed.text;
      else if (typeof parsed.body === "string") summary = parsed.body;
      else if (typeof parsed.message === "string") summary = parsed.message;
    }
  } catch {
    summary = summary.replace(/^["']|["']$/g, "");
  }

  return { verb, summary };
}

function renderPeerEntry(
  frame: ConsoleFrame,
  entryId: string,
): ConversationTimelineEntry | null {
  const rawText = summarizeFrameData(frame.data);
  if (!rawText) return null;

  const peer = parsePeerSummary(rawText);
  if (!peer) return null;

  return {
    kind: "message",
    id: entryId,
    identity: SYSTEM_IDENTITY,
    variant: "meta",
    createdAt: isoFromTimestampMs(frame.timestampMs),
    text: `↩ ${peer.verb}: ${peer.summary}`,
  };
}

function renderTerminalEntry(
  agent: ConsoleAgent | null,
  frame: ConsoleFrame,
  entryId: string,
  streamedText = "",
): ConversationTimelineEntry | null {
  if (frame.event === "interaction_complete") {
    const text = summarizeFrameData(frame.data).trim();
    if (!text) return null;

    // Peer responses always render as compact meta, even if text was streamed
    const peer = parsePeerSummary(text);
    if (peer) {
      return {
        kind: "message",
        id: entryId,
        identity: SYSTEM_IDENTITY,
        variant: "meta",
        createdAt: isoFromTimestampMs(frame.timestampMs),
        text: `↩ ${peer.verb}: ${peer.summary}`,
      };
    }

    if (streamedText.trim() && normalizeComparableText(streamedText) === normalizeComparableText(text)) {
      return null;
    }

    const blocks = parseConversationRichBlocks(text);
    return {
      kind: "message",
      id: entryId,
      identity: agentIdentity(agent),
      variant: blocks.length > 0 ? "rich" : "plain",
      createdAt: isoFromTimestampMs(frame.timestampMs),
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
      createdAt: isoFromTimestampMs(frame.timestampMs),
      text,
    };
  }

  return null;
}

function normalizeComparableText(value: string): string {
  return value.replace(/\s+/g, " ").trim();
}

export function buildQuickPromptSuggestions(agent: ConsoleAgent | null): ConversationEmptySuggestion[] {
  const labels = agent?.labels ?? {};
  const suggestions: ConversationEmptySuggestion[] = [];
  for (let index = 1; index <= 4; index++) {
    const label = labels[`console_prompt_${index}_label`]?.trim();
    const value = labels[`console_prompt_${index}_value`]?.trim();
    if (!label || !value) continue;
    suggestions.push({
      id: `prompt-${index}`,
      label,
      value,
      iconName: "i-bolt",
    });
  }
  return suggestions;
}

function renderHistoryUserEntry(frame: ConsoleFrame, entryId: string): ConversationTimelineEntry | null {
  if (frame.event !== "interaction_started" || typeof frame.data !== "object" || frame.data === null) {
    return null;
  }
  const record = frame.data as Record<string, unknown>;
  const content = typeof record.content === "string" ? record.content.trim() : "";
  if (!content) return null;
  return {
    kind: "message",
    id: entryId,
    identity: USER_IDENTITY,
    variant: "plain",
    createdAt: isoFromTimestampMs(frame.timestampMs),
    text: content,
  };
}

function renderRunStartedPromptEntries(
  frame: ConsoleFrame,
  entryId: string,
  options: { suppressEmbeddedRpcPrompt?: boolean } = {},
): ConversationTimelineEntry[] {
  if (frame.event !== "run_started" || typeof frame.data !== "object" || frame.data === null) {
    return [];
  }
  const record = frame.data as Record<string, unknown>;
  const prompt = typeof record.prompt === "string" ? record.prompt.trim() : "";
  if (!prompt) {
    return [];
  }
  const createdAt = isoFromTimestampMs(frame.timestampMs);
  const entries: ConversationTimelineEntry[] = [];

  const embeddedPrompt = extractEmbeddedRpcPrompt(prompt);
  if (embeddedPrompt && !options.suppressEmbeddedRpcPrompt) {
    entries.push({
      kind: "message",
      id: `${entryId}:event`,
      identity: USER_IDENTITY,
      variant: "plain",
      ...(createdAt ? { createdAt } : {}),
      text: embeddedPrompt,
    });
  }

  if (prompt.startsWith("[COMMS")) {
    const incomingBlocks = parseIncomingCommsBlocks(prompt);
    if (incomingBlocks.length > 0) {
      // All blocks from a single prompt go into one entry (they're batched)
      entries.push({
        kind: "message",
        id: entryId,
        identity: { id: "comms", label: "", role: "system" as const, showLabel: false },
        variant: "rich",
        ...(createdAt ? { createdAt } : {}),
        blocks: incomingBlocks,
      });
    } else {
      const summarized = summarizeCommsTransport(prompt).trim();
      if (summarized) {
        entries.push({
          kind: "message",
          id: entryId,
          identity: { id: "comms", label: "", role: "system" as const, showLabel: false },
          variant: "meta",
          ...(createdAt ? { createdAt } : {}),
          text: summarized,
        });
      }
    }
  }

  return entries;
}

function extractTextFromContentBlocks(blocks: unknown): string {
  if (typeof blocks === "string") {
    return blocks;
  }
  if (!Array.isArray(blocks)) {
    return "";
  }
  return blocks
    .map((block) => {
      if (typeof block === "string") return block;
      if (!block || typeof block !== "object") return "";
      const record = block as Record<string, unknown>;
      if (typeof record.text === "string") return record.text;
      if (typeof record.content === "string") return record.content;
      return "";
    })
    .filter((value) => value.trim().length > 0)
    .join("");
}

function stripRpcEventPrefix(text: string): string {
  return text.replace(/^\[EVENT via rpc\]\s*/i, "").trim();
}

function summarizeCommsTransport(text: string): string {
  const lines = text.split("\n").map((line) => line.trim()).filter(Boolean);
  if (lines.length === 0) {
    return "";
  }
  const header = lines[0] || "";
  const headerTail = header.includes("]") ? header.slice(header.indexOf("]") + 1).trim() : "";
  const body = lines
    .slice(1)
    .filter((line) => !line.startsWith("[EVENT via rpc]"));
  if (header.startsWith("[COMMS REQUEST")) {
    const intentLine = body.find((line) => line.startsWith("Intent:"));
    if (intentLine) {
      const intent = intentLine.replace(/^Intent:\s*/, "").trim();
      if (intent === "mob.peer_added" || intent === "mob.peer_removed") {
        return "";
      }
      return `↪ request: ${intent}`;
    }
    return "↪ request received";
  }
  if (header.startsWith("[COMMS RESPONSE")) {
    // Extract status line
    const statusLine = body.find((line) => line.startsWith("Status:"));
    const status = statusLine ? statusLine.replace(/^Status:\s*/, "").trim() : "";
    // Extract result JSON and parse it cleanly
    const resultIndex = body.findIndex((line) => line.startsWith("Result:"));
    if (resultIndex >= 0) {
      // Collect result lines until we hit another control line (Status:, [COMMS, etc.)
      const resultLines: string[] = [];
      for (let i = resultIndex; i < body.length; i++) {
        const line = body[i];
        if (i > resultIndex && (line.startsWith("Status:") || line.startsWith("[COMMS "))) break;
        resultLines.push(line);
      }
      const resultText = resultLines.join(" ").replace(/^Result:\s*/, "").trim();
      let summary = resultText;
      try {
        const parsed = JSON.parse(resultText);
        if (typeof parsed === "string") {
          summary = parsed;
        } else if (typeof parsed === "object" && parsed !== null) {
          const val = parsed.summary ?? parsed.text ?? parsed.body ?? parsed.message ?? parsed.reply ?? parsed.result ?? parsed.content;
          if (typeof val === "string") summary = val;
        }
      } catch { /* use raw */ }
      const label = status ? `↩ response (${status})` : "↩ response";
      return `${label}: ${summary}`;
    }
    return status ? `↩ response (${status})` : "↩ response received";
  }
  if (header.startsWith("[COMMS MESSAGE")) {
    const joined = [headerTail, ...body].join(" ").trim();
    return joined ? `↩ message: ${joined}` : "↩ message received";
  }
  return text;
}

function parseIncomingCommsBlocks(prompt: string): ConversationRichToolCallBlock[] {
  // Split prompt into individual [COMMS ...] sections
  const sections: string[] = [];
  let current = "";
  for (const line of prompt.split("\n")) {
    if (line.trimStart().startsWith("[COMMS ") && current) {
      sections.push(current);
      current = line + "\n";
    } else {
      current += line + "\n";
    }
  }
  if (current.trim()) sections.push(current);

  const blocks: ConversationRichToolCallBlock[] = [];
  let counter = 0;

  for (const section of sections) {
    const lines = section.split("\n").map((l) => l.trim()).filter(Boolean);
    const header = lines[0] || "";
    if (!header.startsWith("[COMMS")) continue;

    const senderMatch = header.match(/\[COMMS\s+\w+\s+from\s+\S+\/([^/\s\]]+)/);
    const sender = senderMatch ? senderMatch[1] : null;
    if (!sender) continue;

    const body = lines.slice(1).filter((l) => !l.startsWith("[COMMS ") && !l.startsWith("[EVENT via rpc]"));
    counter++;

    if (header.startsWith("[COMMS RESPONSE")) {
      const statusLine = body.find((l) => l.startsWith("Status:"));
      const status = statusLine ? statusLine.replace(/^Status:\s*/, "").trim() : "";
      const resultIndex = body.findIndex((l) => l.startsWith("Result:"));
      let resultSummary = "";
      if (resultIndex >= 0) {
        const resultLines: string[] = [];
        for (let i = resultIndex; i < body.length; i++) {
          if (i > resultIndex && (body[i].startsWith("Status:") || body[i].startsWith("[COMMS "))) break;
          resultLines.push(body[i]);
        }
        const raw = resultLines.join(" ").replace(/^Result:\s*/, "").trim();
        try {
          const parsed = JSON.parse(raw);
          if (typeof parsed === "string") {
            resultSummary = parsed;
          } else if (typeof parsed === "object" && parsed !== null) {
            // Try common result field names
            const val = parsed.summary ?? parsed.text ?? parsed.body ?? parsed.message ?? parsed.reply ?? parsed.result ?? parsed.content;
            resultSummary = typeof val === "string" ? val : raw;
          } else { resultSummary = raw; }
        } catch { resultSummary = raw; }
      }
      blocks.push({
        type: "tool-call",
        toolCallId: `incoming-${sender}-${counter}`,
        name: "response",
        arguments: "",
        status: status === "failed" ? "error" : "success",
        peerTarget: sender,
        peerIntent: resultSummary || status || "response",
        peerIncoming: true,
      });
    } else if (header.startsWith("[COMMS REQUEST")) {
      const intentLine = body.find((l) => l.startsWith("Intent:"));
      const intent = intentLine ? intentLine.replace(/^Intent:\s*/, "").trim() : "";
      if (intent === "mob.peer_added" || intent === "mob.peer_removed") continue;
      blocks.push({
        type: "tool-call",
        toolCallId: `incoming-${sender}-${counter}`,
        name: "request",
        arguments: "",
        status: "success",
        peerTarget: sender,
        peerIntent: intent || "request",
        peerIncoming: true,
      });
    } else if (header.startsWith("[COMMS MESSAGE")) {
      const joined = body.join(" ").trim();
      blocks.push({
        type: "tool-call",
        toolCallId: `incoming-${sender}-${counter}`,
        name: "message",
        arguments: "",
        status: "success",
        peerTarget: sender,
        peerIntent: joined || "message",
        peerIncoming: true,
      });
    }
  }

  return blocks;
}

function extractEmbeddedRpcPrompt(text: string): string | null {
  const match = text.match(/^\[EVENT via rpc\]\s*(.+)$/im);
  return match?.[1]?.trim() || null;
}

function historyMessageText(message: unknown): { role: "user" | "assistant" | "system" | "meta" | null; text: string; blocks?: ConversationRichToolCallBlock[] } {
  if (!message || typeof message !== "object") {
    return { role: null, text: "" };
  }
  const record = message as Record<string, unknown>;
  const role = typeof record.role === "string" ? record.role : null;
  switch (role) {
    case "user": {
      const text = extractTextFromContentBlocks(record.content);
      if (text.startsWith("[COMMS")) {
        return { role: "meta", text: summarizeCommsTransport(text) };
      }
      return { role: "user", text: stripRpcEventPrefix(text) };
    }
    case "assistant":
      return { role: "assistant", text: typeof record.content === "string" ? record.content : "" };
    case "block_assistant": {
      const blocks = Array.isArray(record.blocks) ? record.blocks : [];
      const text = blocks
        .map((block) => {
          if (!block || typeof block !== "object") return "";
          const item = block as Record<string, unknown>;
          const blockType = typeof item.block_type === "string"
            ? item.block_type
            : typeof item.type === "string"
              ? item.type
              : "";
          const data = item.data && typeof item.data === "object"
            ? item.data as Record<string, unknown>
            : {};
          if (blockType === "text") {
            if (typeof data.text === "string") return data.text;
            if (typeof item.text === "string") return item.text;
          }
          return "";
        })
        .filter((value) => value.trim().length > 0)
        .join("");
      return { role: "assistant", text };
    }
    case "system":
      return { role: "system", text: typeof record.content === "string" ? record.content : "" };
    default:
      return { role: null, text: "" };
  }
}

export function mapSessionHistoryToTimelineEntries(
  historyPage: unknown,
  agent: ConsoleAgent | null,
): ConversationTimelineEntry[] {
  if (!historyPage || typeof historyPage !== "object") {
    return [];
  }
  const record = historyPage as Record<string, unknown>;
  const messages = Array.isArray(record.messages) ? record.messages : [];
  const entries: ConversationTimelineEntry[] = [];
  for (const [index, message] of messages.entries()) {
    const parsed = historyMessageText(message);
    const text = parsed.text.trim();
    const messageRecord = message && typeof message === "object"
      ? message as Record<string, unknown>
      : null;
    const rawContent = typeof messageRecord?.content === "string" ? messageRecord.content : "";
    const createdAt = typeof messageRecord?.created_at === "string"
      ? messageRecord.created_at
      : typeof messageRecord?.createdAt === "string"
        ? messageRecord.createdAt
        : undefined;
    if (!text) {
      continue;
    }
    if (parsed.role === "system") {
      continue;
    }
    if (parsed.role === "user") {
      if (
        text.startsWith("## Incident Comms Protocol")
        || text.startsWith("You have been spawned as")
        || text.startsWith("[SYSTEM NOTICE][TOOL_SCOPE]")
      ) {
        continue;
      }
    }
    if (parsed.role === "meta") {
      const embeddedPrompt = rawContent ? extractEmbeddedRpcPrompt(rawContent) : null;
      if (embeddedPrompt) {
        entries.push({
          kind: "message",
          id: `history:${index}:event`,
          identity: USER_IDENTITY,
          variant: "plain",
          ...(createdAt ? { createdAt } : {}),
          text: embeddedPrompt,
        });
      }
      if (!text) {
        continue;
      }
      entries.push({
        kind: "message",
        id: `history:${index}`,
        identity: SYSTEM_IDENTITY,
        variant: "meta",
        ...(createdAt ? { createdAt } : {}),
        text,
      });
      continue;
    }
    if (parsed.role === "user") {
      entries.push({
        kind: "message",
        id: `history:${index}`,
        identity: USER_IDENTITY,
        variant: "plain",
        ...(createdAt ? { createdAt } : {}),
        text,
      });
      continue;
    }
    if (
      parsed.role === "assistant"
      && /^I have acknowledged the addition of the following peers:/i.test(text)
    ) {
      continue;
    }
    const blocks = parseConversationRichBlocks(text);
    entries.push({
      kind: "message",
      id: `history:${index}`,
      identity: agentIdentity(agent),
      variant: blocks.length > 0 ? "rich" : "plain",
      ...(createdAt ? { createdAt } : {}),
      ...(blocks.length > 0 ? { blocks } : { text }),
    });
  }
  let lastOperatorPromptIndex = -1;
  let lastPeerActivityIndex = -1;
  for (let index = 0; index < entries.length; index++) {
    const entry = entries[index];
    if (entry?.kind === "message" && entry.identity.id === USER_IDENTITY.id) {
      lastOperatorPromptIndex = index;
    }
    if (
      entry?.kind === "message"
      && entry.identity.id === SYSTEM_IDENTITY.id
      && "text" in entry
      && typeof entry.text === "string"
      && /^(Peer request:|Peer response:|Peer message:)/.test(entry.text)
    ) {
      lastPeerActivityIndex = index;
    }
  }
  if (lastOperatorPromptIndex >= 0) {
    return entries.slice(lastOperatorPromptIndex);
  }
  if (lastPeerActivityIndex >= 0) {
    return entries.slice(lastPeerActivityIndex);
  }
  return entries.slice(-8);
}

export function mapFramesToTimelineEntries(
  agent: ConsoleAgent | null,
  frames: ConsoleFrame[],
  options: {
    renderInteractionStartsAsUser?: boolean;
    renderTextDeltas?: boolean;
    suppressEmbeddedRunStartedPrompt?: boolean;
  } = {},
): ConversationTimelineEntry[] {
  // Use frames in their original store order — sortFramesForTranscript
  // reorders by interaction group timestamp which interleaves unscoped
  // comms events into the middle of user interactions.
  const orderedFrames = frames;
  const entries: ConversationTimelineEntry[] = [];
  const toolBlocks = buildToolBlocks(orderedFrames);
  const emittedToolCalls = new Set<string>();


  let pendingText = "";
  let pendingId = "";
  let pendingCreatedAt: string | undefined;

  function flushPendingText() {
    if (!pendingText) return;
    const blocks = parseConversationRichBlocks(pendingText);
    entries.push({
      kind: "message",
      id: pendingId,
      identity: agentIdentity(agent),
      variant: blocks.length > 0 ? "rich" : "plain",
      ...(pendingCreatedAt ? { createdAt: pendingCreatedAt } : {}),
      ...(blocks.length > 0 ? { blocks } : { text: pendingText }),
    });
    pendingText = "";
    pendingId = "";
    pendingCreatedAt = undefined;
  }

  for (let i = 0; i < orderedFrames.length; i++) {
    const frame = orderedFrames[i];
    const entryId = `${frame.id || frame.event || "frame"}:${i}`;

    if (frame.event === "text_delta") {
      if (options.renderTextDeltas === false) {
        continue;
      }
      if (!pendingId) {
        pendingId = entryId;
        pendingCreatedAt = isoFromTimestampMs(frame.timestampMs);
      }
      pendingText += summarizeFrameData(frame.data);
      continue;
    }

    const toolCallId = parseToolCallId(frame);
    if (
      toolCallId
      && (frame.event === "tool_call_requested" || frame.event === "tool_call" || frame.event === "tool_execution_started")
      && !emittedToolCalls.has(toolCallId)
    ) {
      flushPendingText();
      const block = toolBlocks.get(toolCallId);
      if (block) {
        // Group consecutive peer tool calls into one entry
        const isPeer = block.peerTarget !== undefined;
        const lastEntry = entries[entries.length - 1];
        const lastIsPeerGroup = lastEntry
          && lastEntry.variant === "rich"
          && Array.isArray(lastEntry.blocks)
          && lastEntry.blocks.length > 0
          && lastEntry.blocks.every((b: Record<string, unknown>) => b.type === "tool-call" && b.peerTarget);

        if (isPeer && lastIsPeerGroup) {
          // Append to existing peer group
          (lastEntry.blocks as unknown[]).push(block);
        } else {
          entries.push({
            kind: "message",
            id: entryId,
            identity: agentIdentity(agent),
            variant: "rich",
            createdAt: isoFromTimestampMs(frame.timestampMs),
            blocks: [block],
          });
        }
        emittedToolCalls.add(toolCallId);
      }
      continue;
    }

    if (frame.event === "tool_result_received" || frame.event === "tool_execution_completed") {
      continue;
    }

    if (options.renderInteractionStartsAsUser && frame.event === "interaction_started") {
      flushPendingText();
      const userEntry = renderHistoryUserEntry(frame, entryId);
      if (userEntry) {
        entries.push(userEntry);
      }
      continue;
    }

    if (frame.event === "run_started") {
      flushPendingText();
      const promptEntries = renderRunStartedPromptEntries(frame, entryId, {
        suppressEmbeddedRpcPrompt:
          options.renderInteractionStartsAsUser === true
          || options.suppressEmbeddedRunStartedPrompt === true,
      });
      if (promptEntries.length > 0) {
        entries.push(...promptEntries);
        continue;
      }
    }

    if (frame.event === "text_complete") {
      continue;
    }

    if (HIDDEN_EVENTS.has(frame.event)) {
      continue;
    }

    const streamedText = pendingText;
    flushPendingText();

    const terminalEntry = renderTerminalEntry(agent, frame, entryId, streamedText);
    if (terminalEntry) {
      entries.push(terminalEntry);
      continue;
    }
    if (frame.event === "interaction_complete") {
      continue;
    }

    // Try to render as a clean peer message
    const peerEntry = renderPeerEntry(frame, entryId);
    if (peerEntry) {
      entries.push(peerEntry);
      continue;
    }

    // Skip remaining tool lifecycle events (handled by tool blocks above)
    if (frame.event === "tool_call_requested" || frame.event === "tool_call" || frame.event === "tool_execution_started"
      || frame.event === "tool_result_received" || frame.event === "tool_execution_completed") {
      continue;
    }

    const text = `${frame.event}: ${summarizeFrameData(frame.data)}`.trim();
    entries.push({
      kind: "message",
      id: entryId,
      identity: SYSTEM_IDENTITY,
      variant: "meta",
      createdAt: isoFromTimestampMs(frame.timestampMs),
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
    createdAt: new Date().toISOString(),
    text: message,
  };
}

export function sortConversationTimelineEntries(
  entries: ConversationTimelineEntry[],
): ConversationTimelineEntry[] {
  return entries
    .map((entry, index) => ({ entry, index }))
    .sort((left, right) => {
      const leftTs = Date.parse(String(left.entry.createdAt || ""));
      const rightTs = Date.parse(String(right.entry.createdAt || ""));
      const safeLeft = Number.isFinite(leftTs) ? leftTs : Number.NaN;
      const safeRight = Number.isFinite(rightTs) ? rightTs : Number.NaN;
      if (Number.isFinite(safeLeft) && Number.isFinite(safeRight) && safeLeft !== safeRight) {
        return safeLeft - safeRight;
      }
      if (Number.isFinite(safeLeft) && !Number.isFinite(safeRight)) {
        return 1;
      }
      if (!Number.isFinite(safeLeft) && Number.isFinite(safeRight)) {
        return -1;
      }
      return left.index - right.index;
    })
    .map(({ entry }) => entry);
}

export function inferResponsePhaseFromFrames(
  frames: ConsoleFrame[],
  fallback: ResponsePhase = null,
): ResponsePhase {
  let phase: ResponsePhase = fallback;
  for (const frame of frames) {
    switch (frame.event) {
      case "interaction_started":
        phase = "waiting";
        break;
      case "tool_call_requested":
      case "tool_call":
      case "tool_execution_started":
      case "tool_result_received":
      case "tool_execution_completed":
        phase = "tool-executing";
        break;
      case "text_delta":
        phase = "generating";
        break;
      case "interaction_complete":
      case "interaction_failed":
      case "run_completed":
      case "run_failed":
        phase = null;
        break;
      default:
        break;
    }
  }
  return phase;
}

export function buildConversationViewState(args: {
  memberId: string;
  agentLabel: string;
  agent?: ConsoleAgent | null;
  entries: ConversationTimelineEntry[];
}): ConversationViewState {
  const groups = groupConversationTimelineEntries(args.entries);
  const suggestions = buildQuickPromptSuggestions(args.agent ?? null);
  return {
    conversationId: args.memberId || "console",
    title: args.agentLabel,
    entries: args.entries,
    groups,
    turnDiff: null,
    emptyState: args.entries.length === 0 ? {
      title: args.agentLabel,
      subtitle: "Send a message to start the conversation.",
      ...(suggestions.length ? { suggestions } : {}),
    } : null,
  };
}

export function buildActivityRailViewState(args: {
  agents: ConsoleAgent[];
  eventFrames: ConsoleFrame[];
  filterPresets?: ActivityFilterPreset[];
  activePresetId?: string;
}): ConsoleActivityRailViewState {
  const presets = args.filterPresets || [];
  const activePreset = presets.find((preset) => preset.id === args.activePresetId) || null;
  const agentByIdentity = new Map<string, ConsoleAgent>();
  const watchedIdentities = new Set<string>();
  const criticalIdentities = new Set<string>();

  for (const agent of args.agents) {
    if (agent.identity) agentByIdentity.set(agent.identity, agent);
    agentByIdentity.set(agent.member_id, agent);
    if (agent.watched && (agent.identity || agent.member_id)) {
      watchedIdentities.add(agent.identity || agent.member_id);
    }
    if (agent.alertLevel === "critical" && (agent.identity || agent.member_id)) {
      criticalIdentities.add(agent.identity || agent.member_id);
    }
  }

  const filteredFrames = args.eventFrames.filter((frame) => {
    if (ACTIVITY_HIDDEN_EVENTS.has(frame.event)) {
      return false;
    }
    const frameIdentity = frame.identity?.trim();
    if (!activePreset) return true;
    if (activePreset.watchedOnly && frameIdentity && !watchedIdentities.has(frameIdentity)) {
      return false;
    }
    if (activePreset.alertLevels?.length && frameIdentity) {
      const agent = agentByIdentity.get(frameIdentity);
      if (!agent?.alertLevel || !activePreset.alertLevels.includes(agent.alertLevel)) {
        return false;
      }
    }
    if (activePreset.eventTypeFilter?.length && !activePreset.eventTypeFilter.includes(frame.event)) {
      return false;
    }
    return true;
  });

  const pulseItems: ConsoleActivityPulseItem[] = filteredFrames
    .slice(0, 200)
    .map((frame, index) => {
      const frameIdentity = frame.identity?.trim();
      const agent = frameIdentity ? agentByIdentity.get(frameIdentity) : null;
      const ts = typeof frame.timestampMs === "number"
        ? new Date(frame.timestampMs).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", second: "2-digit" })
        : "";
      return {
        id: `event:${frame.id || index}`,
        title: agent?.label || frameIdentity || frame.event || "event",
        line: summarizeFrameData(frame.data).slice(0, 120) || frame.event,
        meta: `${frame.event}${ts ? ` · ${ts}` : ""}`,
        ...(agent ? { focusId: agent.member_id } : {}),
      };
    });

  return {
    panels: [
      {
        id: "pulse",
        kind: "pulse" as const,
        title: "Activity",
        actions: presets.map((preset) => ({
          id: preset.id,
          label: preset.label,
          active: preset.id === (activePreset?.id || "all"),
        })),
        items: pulseItems,
        emptyText: "No events yet",
      },
    ],
  };
}
