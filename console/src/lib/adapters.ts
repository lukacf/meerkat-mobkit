import type {
  ActivityFilterPreset,
  ConsoleActivityPulseItem,
  ConsoleActivityRailViewState,
  ConsoleDockTarget,
  ConsoleSidebarMetaTone,
  ConsoleSidebarViewState,
  ConversationEmptySuggestion,
  ConversationIdentity,
  ConversationRichBlock,
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
  addressingMode: "identity";
  memberId: string;
  identity: string;
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
  const targetKey = target.identity || target.memberId || target.id;
  return `panel:${panelId}:${target.kind}:${targetKey}`;
}

export interface OptimisticUserMessage {
  interactionId: string;
  entry: ConversationTimelineEntry;
  sentAtMs: number;
  objectUrls?: string[];
}

export function optimisticUserMessageForPanel(
  optimisticByPanelKey: Record<string, OptimisticUserMessage>,
  panelKey: string,
  identity: string,
): OptimisticUserMessage | null {
  const direct = optimisticByPanelKey[panelKey];
  if (direct) return direct;
  const identitySuffix = `:agent-chat:${identity}`;
  let latest: OptimisticUserMessage | null = null;
  for (const [key, optimistic] of Object.entries(optimisticByPanelKey)) {
    if (!key.endsWith(identitySuffix)) continue;
    if (!latest || optimistic.sentAtMs > latest.sentAtMs) latest = optimistic;
  }
  return latest;
}

export function buildDockTarget(agent: ConsoleAgent): AgentChatTarget {
  const subtitle = [agent.role, agent.kind].filter(Boolean).join(" \u00b7 ") || undefined;
  const identity = typeof agent.identity === "string" && agent.identity.trim()
    ? agent.identity.trim()
    : agent.member_id;
  return {
    id: agent.member_id,
    kind: "agent-chat",
    addressingMode: "identity",
    memberId: agent.member_id,
    identity,
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
    title: `${agent.label} Details`,
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
      return { id: "gating", kind, title: "Approvals", subtitle: "Pending approvals, audit, and policies", iconName: "i-bolt" };
    case "topology":
      return { id: "topology", kind, title: "Topology", subtitle: "Identity connectivity", iconName: "i-team" };
    case "health":
      return { id: "health", kind, title: "Health", subtitle: "Runtime and identity health", iconName: "i-gear" };
    case "timeline":
      return { id: "timeline", kind, title: "Today", subtitle: "Chronological events", iconName: "i-clock" };
    case "roster":
      return { id: "roster", kind, title: "Roster", subtitle: "All agents", iconName: "i-team" };
    case "gates":
      return { id: "gating", kind: "gating", title: "Approvals", subtitle: "Pending approvals, audit, and policies", iconName: "i-bolt" };
    case "logs":
      return { id: "logs", kind, title: "Logs", subtitle: "Event stream", iconName: "i-terminal" };
    default:
      return { id: "health", kind: "health", title: "Health" };
  }
}

function agentGroupKey(agent: ConsoleAgent): string {
  return agent.group?.trim() || agent.role?.trim() || agent.kind?.trim() || "Agents";
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

/**
 * Durable pin identity for an agent. Prefers the stable identity (or the
 * configured `labels.agent_identity`) so pins survive respawns, falling back to
 * the volatile `member_id` only when no durable identity exists.
 */
export function sidebarAgentPinId(agent: ConsoleAgent): string {
  return agent.identity?.trim()
    || agent.labels?.agent_identity?.trim()
    || agent.member_id.trim();
}

export function isAgentPinned(agent: ConsoleAgent, pinnedAgentIds: Set<string> | undefined): boolean {
  if (!pinnedAgentIds) return false;
  return pinnedAgentIds.has(sidebarAgentPinId(agent)) || pinnedAgentIds.has(agent.member_id);
}

export function buildSidebarViewState(args: {
  agents: ConsoleAgent[];
  selectedMemberId: string;
  pinnedAgentIds?: Set<string>;
  sortMode?: "group" | "alpha" | "status";
}): ConsoleSidebarViewState {
  const { agents, selectedMemberId, pinnedAgentIds = new Set(), sortMode = "group" } = args;

  const sorted = [...agents].sort((a, b) => {
    const aPinned = isAgentPinned(a, pinnedAgentIds) ? 0 : 1;
    const bPinned = isAgentPinned(b, pinnedAgentIds) ? 0 : 1;
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
      const isPinned = isAgentPinned(agent, pinnedAgentIds);
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
            label: "Open roster details",
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

const COMMS_IDENTITY: ConversationIdentity = {
  id: "comms",
  label: "",
  role: "system",
  showLabel: false,
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
    // For canonical text-bearing frames (`interaction_complete`,
    // `tool_execution_completed`, etc.) `record.result` IS the
    // canonical text. Return it verbatim — including the empty
    // string. Pre-fix the `.trim()` guard skipped empty results and
    // we fell through to `JSON.stringify(record)`, which dumped the
    // whole envelope (`session_id`, `type`, `usage`, ...) into the
    // assistant's chat bubble.
    if (typeof record.result === "string") return record.result;
    if (typeof record.message === "string" && record.message.trim()) return record.message;
    if (typeof record.error === "string" && record.error.trim()) return record.error;
    if (typeof record.reason === "string" && record.reason.trim()) return record.reason;
    if (typeof record.kind === "string" && typeof record.event_type === "string") return "";
    return JSON.stringify(record);
  }
  return String(data ?? "");
}

function isSteerDeliveryTerminalFrame(frame: Pick<ConsoleFrame, "event" | "data">): boolean {
  if (frame.event !== "interaction_complete") return false;
  if (!frame.data || typeof frame.data !== "object") return false;
  const record = frame.data as Record<string, unknown>;
  return record.reason === "steer_delivered";
}

function eventSortRank(event: string | undefined): number {
  switch (event) {
    case "user_input":
    case "interaction_started":
    case "run_started":
      return 0;
    case "tool_call_requested":
    case "tool_call":
    case "tool_execution_started":
      return 20;
    case "tool_result_received":
    case "tool_execution_completed":
      return 30;
    case "assistant_image":
    case "assistant_image_appended":
      return 35;
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

function isInteractionStartEvent(event: string | undefined): boolean {
  return event === "user_input" || event === "interaction_started" || event === "run_started";
}

function cursorSeq(cursor: string | undefined): number | null {
  if (!cursor) return null;
  const match = /^console:(\d+)$/.exec(cursor);
  if (!match) return null;
  const parsed = Number(match[1]);
  return Number.isFinite(parsed) ? parsed : null;
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

  const transcriptGroupTimestamp = (frame: ConsoleFrame): number => {
    const interactionId = frame.interactionId?.trim() || "";
    const ownTimestamp =
      typeof frame.timestampMs === "number"
        ? frame.timestampMs
        : Number.MAX_SAFE_INTEGER;
    if (!interactionId) return ownTimestamp;
    return interactionStartMs.get(interactionId) ?? ownTimestamp;
  };

  return frames
    .map((frame, index) => ({ frame, index }))
    .sort((left, right) => {
      const leftInteraction = left.frame.interactionId?.trim() || "";
      const rightInteraction = right.frame.interactionId?.trim() || "";
      const leftGroupTs = transcriptGroupTimestamp(left.frame);
      const rightGroupTs = transcriptGroupTimestamp(right.frame);
      if (leftGroupTs !== rightGroupTs) {
        return leftGroupTs - rightGroupTs;
      }
      if (leftInteraction && rightInteraction && leftInteraction === rightInteraction) {
        const leftStarts = isInteractionStartEvent(left.frame.event);
        const rightStarts = isInteractionStartEvent(right.frame.event);
        if (leftStarts !== rightStarts) {
          return leftStarts ? -1 : 1;
        }
      }
      const leftTs = typeof left.frame.timestampMs === "number" ? left.frame.timestampMs : Number.MAX_SAFE_INTEGER;
      const rightTs = typeof right.frame.timestampMs === "number" ? right.frame.timestampMs : Number.MAX_SAFE_INTEGER;
      if (leftTs !== rightTs) {
        return leftTs - rightTs;
      }
      if (leftInteraction && rightInteraction && leftInteraction === rightInteraction) {
        const leftRank = eventSortRank(left.frame.event);
        const rightRank = eventSortRank(right.frame.event);
        if (leftRank !== rightRank) {
          return leftRank - rightRank;
        }
      }
      const leftCursor = cursorSeq(left.frame.cursor);
      const rightCursor = cursorSeq(right.frame.cursor);
      if (leftCursor !== null && rightCursor !== null && leftCursor !== rightCursor) {
        return leftCursor - rightCursor;
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
  "frame_updated",
  "snapshot_complete",
  "snapshot_started",
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

function normalizeToolArgumentsForSignature(argumentsText: string | undefined): string {
  const trimmed = (argumentsText || "").trim();
  if (!trimmed) return "";
  try {
    return JSON.stringify(JSON.parse(trimmed));
  } catch {
    return trimmed.replace(/\s+/g, " ");
  }
}

function toolBlockSignature(block: ConversationRichToolCallBlock): string {
  return `${block.name}\u0000${normalizeToolArgumentsForSignature(block.arguments)}`;
}

function addToolSignatureCount(
  counts: Map<string, number>,
  block: ConversationRichToolCallBlock,
): void {
  const key = toolBlockSignature(block);
  counts.set(key, (counts.get(key) || 0) + 1);
}

function consumeToolSignatureCount(
  counts: Map<string, number>,
  block: ConversationRichToolCallBlock,
): boolean {
  const key = toolBlockSignature(block);
  const count = counts.get(key) || 0;
  if (count <= 0) return false;
  if (count === 1) counts.delete(key);
  else counts.set(key, count - 1);
  return true;
}

function liveToolDedupeState(
  frames: ConsoleFrame[],
  toolBlocks: Map<string, ConversationRichToolCallBlock>,
): { liveToolCallIds: Set<string>; liveToolSignatureCounts: Map<string, number> } {
  const liveToolCallIds = new Set<string>();
  const liveToolSignatureCounts = new Map<string, number>();

  for (const frame of frames) {
    if (frame.sourceKind === "session_history") continue;
    if (
      frame.event !== "tool_call_requested"
      && frame.event !== "tool_call"
      && frame.event !== "tool_execution_started"
    ) {
      continue;
    }
    const toolCallId = parseToolCallId(frame);
    if (!toolCallId || liveToolCallIds.has(toolCallId)) continue;
    const block = toolBlocks.get(toolCallId);
    if (!block) continue;
    liveToolCallIds.add(toolCallId);
    addToolSignatureCount(liveToolSignatureCounts, block);
  }

  return { liveToolCallIds, liveToolSignatureCounts };
}

const TECHNICAL_PEER_INTENTS = new Set(["checksum_token"]);
const PEER_PAYLOAD_TEXT_KEYS = [
  "message",
  "body",
  "text",
  "summary",
  "reply",
  "content",
  "subject",
  "question",
  "prompt",
  "description",
  "request",
  "request_subject",
  "token",
  "status_line",
];

function isTechnicalPeerIntent(intent: string | undefined): boolean {
  return Boolean(intent && TECHNICAL_PEER_INTENTS.has(intent.trim()));
}

function displayPeerIntent(intent: string | undefined): string | undefined {
  if (!intent) return undefined;
  const trimmed = intent.trim();
  if (!trimmed || isTechnicalPeerIntent(trimmed)) return undefined;
  return trimmed;
}

function parseJsonPayload(value: string): unknown {
  try {
    return JSON.parse(value);
  } catch {
    return null;
  }
}

function summarizePeerPayload(value: unknown): string | undefined {
  if (typeof value === "string") {
    const trimmed = value.trim();
    if (!trimmed) return undefined;
    if (
      (trimmed.startsWith("{") && trimmed.endsWith("}")) ||
      (trimmed.startsWith("[") && trimmed.endsWith("]")) ||
      (trimmed.startsWith("\"") && trimmed.endsWith("\""))
    ) {
      const parsed = parseJsonPayload(trimmed);
      if (parsed !== null) {
        return summarizePeerPayload(parsed) || trimmed;
      }
    }
    return trimmed.replace(/^["']|["']$/g, "");
  }
  if (Array.isArray(value)) {
    const parts = value
      .map((item) => summarizePeerPayload(item))
      .filter((item): item is string => Boolean(item));
    return parts.length ? parts.join(" ") : undefined;
  }
  if (value && typeof value === "object") {
    const record = value as Record<string, unknown>;
    const type = typeof record.type === "string" ? record.type : "";
    if (type === "image" || type === "image_ref" || type === "image_upload") {
      return typeof record.alt === "string" && record.alt.trim()
        ? record.alt.trim()
        : type === "image_ref"
          ? "referenced image"
          : "attached image";
    }
    for (const key of PEER_PAYLOAD_TEXT_KEYS) {
      const summary = summarizePeerPayload(record[key]);
      if (summary) return summary;
    }
    return JSON.stringify(record);
  }
  return undefined;
}

function extractLabeledCommsValue(lines: string[], label: string): string | undefined {
  const labelPattern = new RegExp(`\\b${label}:\\s*(.*)$`);
  const startIndex = lines.findIndex((line) => labelPattern.test(line));
  if (startIndex < 0) return undefined;
  const match = lines[startIndex]?.match(labelPattern);
  const first = match?.[1]?.trim() || "";
  if (!first) return undefined;

  const chunks = [first];
  const startsJson = first.startsWith("{") || first.startsWith("[");
  if (!startsJson || parseJsonPayload(first) !== null) {
    return first;
  }

  for (let i = startIndex + 1; i < lines.length; i++) {
    const next = lines[i].trim();
    if (/^(Intent|Body|Params|Request ID|Status|Result):\s*/.test(next)) break;
    chunks.push(next);
    const candidate = chunks.join("\n").trim();
    if (parseJsonPayload(candidate) !== null) return candidate;
  }

  return chunks.join("\n").trim();
}

function extractPeerBodyFromArgs(argsRecord: Record<string, unknown> | null): string | undefined {
  if (!argsRecord) return undefined;
  const directBody = summarizePeerPayload(argsRecord.body);
  if (directBody) return directBody;
  const paramsBody = summarizePeerPayload(argsRecord.params);
  if (paramsBody) return paramsBody;
  const resultBody = summarizePeerPayload(argsRecord.result);
  if (resultBody) return resultBody;
  return undefined;
}

function capturePeersResult(peerRegistry: Map<string, string>, rawResult: unknown): void {
  const resultText = typeof rawResult === "string"
    ? rawResult
    : rawResult && typeof rawResult === "object"
      ? JSON.stringify(rawResult)
      : "";
  if (!resultText) return;
  try {
    const parsed = JSON.parse(resultText) as { peers?: Array<{ peer_id?: unknown; name?: unknown }> };
    if (!Array.isArray(parsed.peers)) return;
    for (const peer of parsed.peers) {
      if (typeof peer.peer_id === "string" && typeof peer.name === "string") {
        peerRegistry.set(peer.peer_id, peer.name);
      }
    }
  } catch {
    // ignore non-JSON peers payloads
  }
}

function peerTargetFromArgs(
  argsRecord: Record<string, unknown> | null,
  peerRegistry?: Map<string, string>,
): string | undefined {
  const peerId = typeof argsRecord?.peer_id === "string" ? argsRecord.peer_id.trim() : "";
  const registryName = peerId ? peerRegistry?.get(peerId) : undefined;
  return registryName
    ? peerLastSegment(registryName)
    : typeof argsRecord?.display_name === "string" && argsRecord.display_name.trim()
      ? peerLastSegment(argsRecord.display_name.trim())
      : typeof argsRecord?.to === "string" && argsRecord.to.trim()
        ? peerLastSegment(argsRecord.to.trim())
        : peerId
          ? peerId.slice(0, 8)
          : undefined;
}

function parseToolResult(frame: ConsoleFrame): { result?: string; status: "pending" | "success" | "error" } {
  const record = frame.data && typeof frame.data === "object" ? frame.data as Record<string, unknown> : null;
  const isError = Boolean(record?.is_error) || frame.event === "interaction_failed";

  // Extract actual result content — prefer tool_execution_completed which has the real result
  let result = "";
  const toolName = typeof record?.name === "string"
    ? record.name
    : typeof record?.tool_name === "string"
      ? record.tool_name
      : undefined;
  if (typeof record?.result === "string") {
    const display = summarizeToolResultForDisplay(toolName, record.result);
    if (display) {
      result = display;
    } else {
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
    }
  } else if (typeof record?.result === "object" && record.result !== null) {
    result = summarizeToolResultForDisplay(toolName, record.result) || "";
    if (!result) {
      const clean = { ...(record.result as Record<string, unknown>) };
      delete clean.source_event_type;
      delete clean.type;
      result = JSON.stringify(clean, null, 2);
    }
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
  // Peer registry built from `peers` tool results: peer_id (uuid) -> name.
  // The LLM-supplied `display_name` field on send_* args is unreliable
  // (agents have been observed filling it with their own name on every
  // call), so we derive the recipient label from the canonical `peers`
  // listing whenever possible. The list is ordered, so by the time a
  // send_request appears the relevant peers result has already been
  // processed.
  const peerRegistry = buildPeerRegistry(frames);

  for (const frame of frames) {
    if (frame.event === "tool_result_received" || frame.event === "tool_execution_completed") {
      const toolCallId = parseToolCallId(frame);
      const data = frame.data as Record<string, unknown> | undefined;
      // Capture peer registry from the `peers` tool result.
      if (data && (data.name === "peers" || data.tool_name === "peers")) {
        capturePeersResult(peerRegistry, data.result);
      }
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

      // Extract peer comms metadata for send_* tools.
      //
      // Wire shape: send_* tools take `peer_id` (UUID, authoritative) +
      // an optional `display_name` hint. The hint is LLM-controlled and
      // has been observed to be wrong (agents fill it with their own
      // name), so we resolve `peer_id` against the peer registry built
      // from the most recent `peers` tool result. Fall back order:
      // registry → display_name → short peer_id → legacy `to` field.
      const isPeerTool = name === "send_request" || name === "send_message" || name === "send_response";
      const peerTarget = isPeerTool ? peerTargetFromArgs(argsRecord, peerRegistry) : undefined;
      const rawPeerIntent = isPeerTool && typeof argsRecord?.intent === "string"
        ? argsRecord.intent as string
        : undefined;
      const peerIntent = displayPeerIntent(rawPeerIntent);
      const peerBody = isPeerTool ? extractPeerBodyFromArgs(argsRecord) : undefined;

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

function buildPeerRegistry(frames: ConsoleFrame[]): Map<string, string> {
  const peerRegistry = new Map<string, string>();
  for (const frame of frames) {
    if (frame.event !== "tool_result_received" && frame.event !== "tool_execution_completed") continue;
    const data = frame.data && typeof frame.data === "object" ? frame.data as Record<string, unknown> : null;
    if (!data || (data.name !== "peers" && data.tool_name !== "peers")) continue;
    capturePeersResult(peerRegistry, data.result);
  }
  return peerRegistry;
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
    if (isSteerDeliveryTerminalFrame(frame)) return null;
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

    if (streamedTextMatchesTerminal(streamedText, text)) {
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

function terminalFrameVisibleText(frame: ConsoleFrame): string {
  if (isSteerDeliveryTerminalFrame(frame)) return "";
  if (frame.event === "text_complete") {
    const record = frame.data && typeof frame.data === "object" ? frame.data as Record<string, unknown> : null;
    if (typeof record?.content === "string") return record.content;
    if (typeof record?.text === "string") return record.text;
  }
  if (
    frame.event === "interaction_complete"
    || frame.event === "run_completed"
    || frame.event === "text_complete"
  ) {
    return summarizeFrameData(frame.data);
  }
  return "";
}

function liveAssistantTerminalTextSignatures(frames: ConsoleFrame[]): Set<string> {
  const signatures = new Set<string>();
  for (const frame of frames) {
    if (frame.sourceKind === "session_history") continue;
    const text = terminalFrameVisibleText(frame).trim();
    if (!text) continue;
    signatures.add(normalizeComparableText(text));
  }
  return signatures;
}

function buildBlobUrl(blobId: string, baseUrl?: string): string {
  const path = `/blobs/${encodeURIComponent(blobId)}`;
  const base = baseUrl?.trim();
  if (!base) return path;
  return `${base.replace(/\/+$/, "")}${path}`;
}

function renderAssistantImageEntry(
  agent: ConsoleAgent,
  frame: ConsoleFrame,
  entryId: string,
  blobBaseUrl?: string,
): ConversationTimelineEntry | null {
  const data = frame.data && typeof frame.data === "object"
    ? frame.data as Record<string, unknown>
    : {};
  const image = data.image && typeof data.image === "object"
    ? data.image as Record<string, unknown>
    : data;
  const blobRef = image.blob_ref && typeof image.blob_ref === "object"
    ? image.blob_ref as Record<string, unknown>
    : null;
  const blobId = typeof image.blob_id === "string"
    ? image.blob_id
    : typeof blobRef?.blob_id === "string"
      ? blobRef.blob_id
      : "";
  if (!blobId) return null;
  const mediaType = typeof image.media_type === "string"
    ? image.media_type
    : typeof blobRef?.media_type === "string"
      ? blobRef.media_type
      : "image/png";
  const width = typeof image.width === "number" ? image.width : undefined;
  const height = typeof image.height === "number" ? image.height : undefined;
  const imageId = typeof image.image_id === "string" ? image.image_id : undefined;
  return {
    kind: "message",
    id: entryId,
    identity: agentIdentity(agent),
    variant: "rich",
    createdAt: isoFromTimestampMs(frame.timestampMs),
    blocks: [{
      type: "image",
      src: buildBlobUrl(blobId, blobBaseUrl),
      mediaType,
      alt: "generated image",
      ...(width !== undefined ? { width } : {}),
      ...(height !== undefined ? { height } : {}),
      blobId,
      ...(imageId ? { imageId } : {}),
    }],
  };
}

function renderGeneratedImageToolResultEntries(
  agent: ConsoleAgent,
  frame: ConsoleFrame,
  entryId: string,
  blobBaseUrl?: string,
): ConversationTimelineEntry[] {
  const data = frame.data && typeof frame.data === "object"
    ? frame.data as Record<string, unknown>
    : {};
  const name = typeof data.name === "string"
    ? data.name
    : typeof data.tool_name === "string"
      ? data.tool_name
      : "";
  if (name !== "generate_image") return [];

  let result = data.result;
  if (typeof result === "string") {
    try {
      result = JSON.parse(result);
    } catch {
      return [];
    }
  }
  if (!result || typeof result !== "object") return [];
  const images = (result as Record<string, unknown>).images;
  if (!Array.isArray(images)) return [];

  return images.flatMap((image, index) => {
    if (!image || typeof image !== "object") return [];
    const imageFrame: ConsoleFrame = {
      ...frame,
      data: { image },
    };
    const imageEntry = renderAssistantImageEntry(
      agent,
      imageFrame,
      `${entryId}:generated-image:${index}`,
      blobBaseUrl,
    );
    return imageEntry ? [imageEntry] : [];
  });
}

function imageEntryKey(entry: ConversationTimelineEntry): string | null {
  if (entry.kind !== "message" || entry.variant !== "rich" || !("blocks" in entry)) {
    return null;
  }
  const block = entry.blocks?.[0];
  if (!block || block.type !== "image") return null;
  if (typeof block.blobId === "string" && block.blobId.trim()) {
    return `blob:${block.blobId.trim()}`;
  }
  if (typeof block.imageId === "string" && block.imageId.trim()) {
    return `image:${block.imageId.trim()}`;
  }
  if (typeof block.src === "string" && block.src.trim()) {
    return `src:${block.src.trim()}`;
  }
  return null;
}

function normalizeComparableText(value: string): string {
  return value.replace(/\s+/g, " ").trim();
}

function normalizeTextWithoutWhitespace(value: string): string {
  return value.replace(/\s+/g, "");
}

function streamedTextMatchesTerminal(streamedText: string, terminalText: string): boolean {
  const streamed = normalizeComparableText(streamedText);
  const terminal = normalizeComparableText(terminalText);
  if (!streamed || !terminal) return false;
  return streamed === terminal
    || normalizeTextWithoutWhitespace(streamed) === normalizeTextWithoutWhitespace(terminal);
}

function conversationEntryVisibleText(entry: ConversationTimelineEntry): string {
  if (entry.kind !== "message") return "";
  if ("text" in entry && typeof entry.text === "string") return entry.text;
  if (!("blocks" in entry) || !Array.isArray(entry.blocks)) return "";
  return entry.blocks
    .map((block) => {
      if (!block || typeof block !== "object") return "";
      const record = block as Record<string, unknown>;
      if (typeof record.text === "string") return record.text;
      if (typeof record.peerBody === "string") return record.peerBody;
      return "";
    })
    .filter(Boolean)
    .join("\n");
}

function shouldSuppressRepeatedAssistantEntry(
  entry: ConversationTimelineEntry,
  priorEntries: ConversationTimelineEntry[],
): boolean {
  if (entry.kind !== "message") return false;
  if (entry.identity.id === USER_IDENTITY.id || entry.identity.id === COMMS_IDENTITY.id || entry.identity.id === SYSTEM_IDENTITY.id) {
    return false;
  }
  const signature = normalizeComparableText(conversationEntryVisibleText(entry));
  if (!signature) return false;
  const entryTs = Date.parse(String(entry.createdAt || ""));
  for (let index = priorEntries.length - 1; index >= 0; index--) {
    const prior = priorEntries[index];
    if (prior.kind !== "message") continue;
    if (prior.identity.id === USER_IDENTITY.id) {
      const userText = normalizeComparableText(conversationEntryVisibleText(prior));
      if (userText) return false;
      continue;
    }
    if (prior.identity.id !== entry.identity.id) continue;
    const priorSignature = normalizeComparableText(conversationEntryVisibleText(prior));
    if (priorSignature !== signature) continue;
    const priorTs = Date.parse(String(prior.createdAt || ""));
    if (Number.isFinite(entryTs) && Number.isFinite(priorTs) && Math.abs(entryTs - priorTs) > 15_000) {
      return false;
    }
    return true;
  }
  return false;
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

function renderHistoryUserEntry(
  frame: ConsoleFrame,
  entryId: string,
  blobBaseUrl?: string,
): ConversationTimelineEntry | null {
  if (
    frame.event !== "interaction_started"
    && frame.event !== "user_input"
  ) {
    return null;
  }
  if (typeof frame.data !== "object" || frame.data === null) {
    return null;
  }
  const record = frame.data as Record<string, unknown>;
  const content = record.content;
  if (Array.isArray(content)) {
    const blocks = contentToUserBlocks(content, blobBaseUrl);
    if (blocks.length === 0) return null;
    return {
      kind: "message",
      id: entryId,
      identity: USER_IDENTITY,
      variant: "rich",
      createdAt: isoFromTimestampMs(frame.timestampMs),
      blocks,
    };
  }
  const text = extractTextFromContentBlocks(content).trim();
  if (!text) return null;
  return {
    kind: "message",
    id: entryId,
    identity: USER_IDENTITY,
    variant: "plain",
    createdAt: isoFromTimestampMs(frame.timestampMs),
    text,
  };
}

function userEntryTextSignature(entry: ConversationTimelineEntry): string {
  if (entry.kind !== "message") return "";
  if ("text" in entry && typeof entry.text === "string") {
    return entry.text.replace(/\s+/g, " ").trim();
  }
  if ("blocks" in entry && Array.isArray(entry.blocks)) {
    return JSON.stringify(entry.blocks);
  }
  return "";
}

function userEntryDedupeKey(frame: ConsoleFrame, entry: ConversationTimelineEntry): string {
  const interactionId = frame.interactionId?.trim();
  if (interactionId) return `interaction:${interactionId}`;
  const signature = userEntryTextSignature(entry);
  if (frame.sourceKind === "session_history" && /^You are\b/i.test(signature)) {
    return `history-kickoff:${signature}`;
  }
  const occurrence =
    typeof frame.timestampMs === "number"
      ? `ts:${frame.timestampMs}`
      : frame.cursor
        ? `cursor:${frame.cursor}`
        : `frame:${frame.id}`;
  return signature ? `content:${occurrence}:${signature}` : "";
}

function userPromptDedupeKey(frame: ConsoleFrame, entry: ConversationTimelineEntry): string {
  return userEntryDedupeKey(frame, entry);
}

function renderRunStartedPromptEntries(
  frame: ConsoleFrame,
  entryId: string,
  options: {
    suppressEmbeddedRpcPrompt?: boolean;
    suppressStructuredCommsPrompt?: boolean;
    blobBaseUrl?: string;
  } = {},
): ConversationTimelineEntry[] {
  if (frame.event !== "run_started" || typeof frame.data !== "object" || frame.data === null) {
    return [];
  }
  const record = frame.data as Record<string, unknown>;
  const promptBlocks = contentToUserBlocks(record.prompt, options.blobBaseUrl);
  const prompt = extractPromptText(record.prompt).trim();
  if (!prompt) {
    return [];
  }
  if (isCommsLikeRunStartedPrompt(prompt) && runStartedPromptHasImagePlaceholder(frame)) {
    return [];
  }
  if (options.suppressStructuredCommsPrompt) {
    return [];
  }
  const createdAt = isoFromTimestampMs(frame.timestampMs);
  const entries: ConversationTimelineEntry[] = [];

  if (!options.suppressEmbeddedRpcPrompt) {
    if (promptBlocks.length > 0 && promptBlocks.some((block) => block.type === "image")) {
      entries.push({
        kind: "message",
        id: entryId,
        identity: USER_IDENTITY,
        variant: "rich",
        ...(createdAt ? { createdAt } : {}),
        blocks: promptBlocks,
      });
      return entries;
    }
    entries.push({
      kind: "message",
      id: entryId,
      identity: USER_IDENTITY,
      variant: "plain",
      ...(createdAt ? { createdAt } : {}),
      text: prompt,
    });
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

function extractPromptText(prompt: unknown): string {
  if (typeof prompt === "string") return prompt;
  if (!Array.isArray(prompt)) return "";
  return prompt
    .map((block) => {
      if (typeof block === "string") return block;
      if (!block || typeof block !== "object") return "";
      const record = block as Record<string, unknown>;
      if (typeof record.text === "string") return record.text;
      if (typeof record.content === "string") return record.content;
      return "";
    })
    .filter((value) => value.trim().length > 0)
    .join("\n");
}

function contentToUserBlocks(content: unknown, blobBaseUrl?: string): ConversationRichBlock[] {
  if (typeof content === "string") {
    return parseConversationRichBlocks(content);
  }
  if (!Array.isArray(content)) {
    return [];
  }
  const blocks: ConversationRichBlock[] = [];
  for (const block of content) {
    if (typeof block === "string") {
      blocks.push(...parseConversationRichBlocks(block));
      continue;
    }
    if (!block || typeof block !== "object") continue;
    const record = block as Record<string, unknown>;
    const type = typeof record.type === "string" ? record.type : "";
    if (type === "text") {
      const text = typeof record.text === "string"
        ? record.text
        : typeof record.content === "string"
          ? record.content
          : "";
      blocks.push(...parseConversationRichBlocks(text));
      continue;
    }
    if (type === "image" || type === "image_ref") {
      const image = record.image && typeof record.image === "object"
        ? record.image as Record<string, unknown>
        : record;
      const blobRef = image.blob_ref && typeof image.blob_ref === "object"
        ? image.blob_ref as Record<string, unknown>
        : image.blobRef && typeof image.blobRef === "object"
          ? image.blobRef as Record<string, unknown>
          : null;
      const source = typeof image.source === "string" ? image.source : "";
      const blobId = typeof record.blob_id === "string"
        ? record.blob_id
        : typeof image.blob_id === "string"
          ? image.blob_id
          : typeof record.blobId === "string"
            ? record.blobId
            : typeof image.blobId === "string"
              ? image.blobId
              : typeof blobRef?.blob_id === "string"
                ? blobRef.blob_id
                : typeof blobRef?.blobId === "string"
                  ? blobRef.blobId
                  : "";
      const mediaType = typeof image.media_type === "string"
        ? image.media_type
        : typeof image.mediaType === "string"
          ? image.mediaType
          : typeof blobRef?.media_type === "string"
            ? blobRef.media_type
            : typeof blobRef?.mediaType === "string"
              ? blobRef.mediaType
              : "image/png";
      const inlineData = typeof image.data === "string"
        ? image.data
        : typeof image.base64 === "string"
          ? image.base64
          : "";
      const directSrc = typeof image.src === "string" && image.src.trim()
        ? image.src.trim()
        : typeof image.url === "string" && image.url.trim()
          ? image.url.trim()
          : "";
      const src = blobId && (source === "blob" || !directSrc)
        ? buildBlobUrl(blobId, blobBaseUrl)
        : inlineData
          ? `data:${mediaType};base64,${inlineData}`
          : directSrc;
      if (!src) continue;
      const alt = typeof image.alt === "string" && image.alt.trim()
        ? image.alt.trim()
        : type === "image_ref"
          ? "referenced image"
          : "attached image";
      const width = typeof image.width === "number" ? image.width : undefined;
      const height = typeof image.height === "number" ? image.height : undefined;
      const imageId = typeof image.image_id === "string" ? image.image_id : undefined;
      blocks.push({
        type: "image",
        src,
        mediaType,
        alt,
        ...(width !== undefined ? { width } : {}),
        ...(height !== undefined ? { height } : {}),
        ...(blobId ? { blobId } : {}),
        ...(imageId ? { imageId } : {}),
      });
    }
  }
  return blocks;
}

function peerLastSegment(value: string): string {
  return value.split("/").pop() || value;
}

function summarizePeersResult(result: unknown): string | null {
  let parsed = typeof result === "string"
    ? parseJsonPayload(result)
    : result && typeof result === "object"
      ? result
      : null;
  if (typeof parsed === "string") {
    parsed = parseJsonPayload(parsed);
  }
  if (!parsed || typeof parsed !== "object") return null;
  const peers = (parsed as Record<string, unknown>).peers;
  if (!Array.isArray(peers)) return null;
  const roleCounts = new Map<string, number>();
  const preview: string[] = [];
  for (const peer of peers) {
    if (!peer || typeof peer !== "object") continue;
    const record = peer as Record<string, unknown>;
    const rawName = typeof record.name === "string" && record.name.trim()
      ? record.name.trim()
      : typeof (record.address as Record<string, unknown> | undefined)?.endpoint === "string"
        ? String((record.address as Record<string, unknown>).endpoint).trim()
        : typeof record.peer_id === "string"
          ? record.peer_id.trim()
          : "";
    if (!rawName) continue;
    const parts = rawName.split("/").filter(Boolean);
    const role = parts.length >= 2 ? parts[parts.length - 2] : "peer";
    roleCounts.set(role, (roleCounts.get(role) || 0) + 1);
    if (preview.length < 8) preview.push(peerLastSegment(rawName));
  }
  const roles = [...roleCounts.entries()]
    .sort((left, right) => right[1] - left[1] || left[0].localeCompare(right[0]))
    .slice(0, 5)
    .map(([role, count]) => `${role} ${count}`)
    .join(", ");
  const lines = [`${peers.length} peers${roles ? ` · ${roles}` : ""}`];
  if (preview.length > 0) {
    lines.push(`First peers: ${preview.join(", ")}`);
  }
  return lines.join("\n");
}

function summarizeToolResultForDisplay(toolName: string | undefined, result: unknown): string | null {
  if (toolName === "peers") {
    const summary = summarizePeersResult(result);
    if (summary) return summary;
  }
  return null;
}

type HistoryToolResult = {
  result?: string;
  status: "success" | "error";
};

function toolResultTextFromContent(content: unknown): string {
  if (typeof content === "string") return content;
  if (!Array.isArray(content)) return "";
  return content
    .map((block) => {
      if (typeof block === "string") return block;
      if (!block || typeof block !== "object") return "";
      const record = block as Record<string, unknown>;
      if (typeof record.text === "string") return record.text;
      if (typeof record.content === "string") return record.content;
      const data = record.data && typeof record.data === "object"
        ? record.data as Record<string, unknown>
        : null;
      if (typeof data?.text === "string") return data.text;
      if (typeof data?.content === "string") return data.content;
      return "";
    })
    .filter((value) => value.trim().length > 0)
    .join("");
}

function historyToolResults(frames: ConsoleFrame[]): Map<string, HistoryToolResult> {
  const results = new Map<string, HistoryToolResult>();
  for (const frame of frames) {
    if (
      frame.sourceKind !== "session_history"
      || (frame.event !== "tool_execution_completed" && frame.event !== "tool_result_received")
    ) {
      continue;
    }
    const data = frame.data && typeof frame.data === "object"
      ? frame.data as Record<string, unknown>
      : null;
    const toolCallId = typeof data?.tool_call_id === "string" && data.tool_call_id.trim()
      ? data.tool_call_id.trim()
      : typeof data?.id === "string" && data.id.trim()
        ? data.id.trim()
        : "";
    if (!toolCallId) continue;
    const rawResult = data?.result ?? data?.content;
    const result = rawResult !== undefined
      ? summarizeToolResultForDisplay(undefined, rawResult) || toolResultTextFromContent(rawResult)
      : "";
    const status = data?.is_error === true || data?.status === "error" ? "error" : "success";
    results.set(toolCallId, {
      status,
      ...(result.trim() ? { result } : {}),
    });
  }
  return results;
}

function blockAssistantToolBlocks(
  blocks: unknown[],
  peerRegistry?: Map<string, string>,
  toolResults?: Map<string, HistoryToolResult>,
): ConversationRichToolCallBlock[] {
  const toolBlocks: ConversationRichToolCallBlock[] = [];
  for (const block of blocks) {
    if (!block || typeof block !== "object") continue;
    const item = block as Record<string, unknown>;
    const blockType = typeof item.block_type === "string"
      ? item.block_type
      : typeof item.type === "string"
        ? item.type
        : "";
    if (blockType !== "tool_use") continue;
    const data = item.data && typeof item.data === "object"
      ? item.data as Record<string, unknown>
      : item;
    const name = typeof data.name === "string" && data.name.trim()
      ? data.name.trim()
      : "tool";
    const id = typeof data.id === "string" && data.id.trim()
      ? data.id.trim()
      : `history-tool-${toolBlocks.length + 1}`;
    const args = data.args !== undefined ? data.args : data.arguments;
    const argsRecord = args && typeof args === "object" ? args as Record<string, unknown> : null;
    const argumentsText = args === undefined
      ? ""
      : typeof args === "string"
        ? args
        : JSON.stringify(args);
    const isPeerTool = name === "send_request" || name === "send_message" || name === "send_response";
    const peerTarget = isPeerTool ? peerTargetFromArgs(argsRecord, peerRegistry) : undefined;
    const rawPeerIntent = isPeerTool && typeof argsRecord?.intent === "string"
      ? argsRecord.intent
      : undefined;
    const peerIntent = displayPeerIntent(rawPeerIntent);
    const peerBody = isPeerTool ? extractPeerBodyFromArgs(argsRecord) : undefined;
    const result = toolResults?.get(id);
    const displayResult = result?.result
      ? summarizeToolResultForDisplay(name, result.result) || result.result
      : undefined;
    toolBlocks.push({
      type: "tool-call",
      toolCallId: id,
      name,
      arguments: argumentsText,
      ...(displayResult ? { result: displayResult } : {}),
      status: result?.status || "success",
      ...(peerTarget ? { peerTarget } : {}),
      ...(peerIntent ? { peerIntent } : {}),
      ...(peerBody ? { peerBody } : {}),
    });
  }
  return toolBlocks;
}

function textFromUnknown(value: unknown): string {
  return typeof value === "string" ? value.trim() : "";
}

function typedNoticeContentBlocks(content: unknown, blobBaseUrl?: string): ConversationRichBlock[] {
  return contentToUserBlocks(content, blobBaseUrl);
}

function typedNoticeBlockText(block: Record<string, unknown>): string {
  const parts = [
    textFromUnknown(block.summary),
    textFromUnknown(block.body),
    textFromUnknown(block.detail),
    textFromUnknown(block.state),
    textFromUnknown(block.status),
  ].filter(Boolean);
  return parts.join("\n");
}

function typedCommsStableBodyText(block: Record<string, unknown>): string {
  const parts = [
    textFromUnknown(block.summary),
    textFromUnknown(block.body),
    textFromUnknown(block.detail),
  ].filter(Boolean);
  return parts.join("\n");
}

function stripCommsIntentBodyPrefix(text: string, peerAliases: string[] = []): string {
  const match = text.match(/^\s*\[COMMS\s+(?:MESSAGE|REQUEST|RESPONSE)\s+from\s+([^\]\n]+)\]\s*\n\s*Intent:\s*[^\n]*\n\s*Body:\s*([\s\S]+)$/i)
    || text.match(/^\s*Peer\s+(?:message|request|response)\s+from\s+(.+):\s*\n\s*Intent:\s*[^\n]*\n\s*Body:\s*([\s\S]+)$/i);
  if (match?.[1] && peerAliases.length > 0) {
    const peer = normalizePeerAlias(match[1]);
    if (!peerAliases.includes(peer)) return text.trim();
  }
  return (match?.[2] || text).trim();
}

function stripBareCommsIntentBodyPrefix(text: string): string {
  const match = text.match(/^\s*Intent:\s*[^\n]*\n\s*Body:\s*([\s\S]+)$/i);
  return (match?.[1] || text).trim();
}

function isExternalEventOnlySystemNotice(message: unknown): boolean {
  if (!message || typeof message !== "object") return false;
  const record = message as Record<string, unknown>;
  if (textFromUnknown(record.kind) === "external_event") return true;
  const blocks = record.blocks;
  if (!Array.isArray(blocks)) return false;
  let sawExternalEventBlock = false;
  for (const block of blocks) {
    if (!block || typeof block !== "object") continue;
    const type = textFromUnknown((block as Record<string, unknown>).type);
    if (!type) continue;
    if (type !== "external_event") return false;
    sawExternalEventBlock = true;
  }
  return sawExternalEventBlock;
}

function systemNoticeMessageRecord(frame: ConsoleFrame): Record<string, unknown> | null {
  if (frame.event !== "system_notice" || !frame.data || typeof frame.data !== "object") {
    return null;
  }
  const data = frame.data as Record<string, unknown>;
  if (data.message && typeof data.message === "object") {
    return data.message as Record<string, unknown>;
  }
  return data;
}

function commsNoticeMessageRecord(frame: ConsoleFrame): Record<string, unknown> | null {
  const systemNotice = systemNoticeMessageRecord(frame);
  if (systemNotice) return systemNotice;
  if (frame.sourceKind !== "session_history" || !frame.data || typeof frame.data !== "object") {
    return null;
  }
  if (
    frame.event !== "text_complete"
    && frame.event !== "interaction_complete"
    && frame.event !== "interaction_failed"
    && frame.event !== "run_failed"
  ) {
    return null;
  }
  const message = (frame.data as Record<string, unknown>).message;
  if (!message || typeof message !== "object") return null;
  const record = message as Record<string, unknown>;
  return textFromUnknown(record.role) === "system_notice" ? record : null;
}

function systemNoticeBlockRecords(record: Record<string, unknown>): Record<string, unknown>[] {
  const blocks = record.blocks;
  if (!Array.isArray(blocks)) return [];
  return blocks.filter((block): block is Record<string, unknown> => (
    Boolean(block) && typeof block === "object"
  ));
}

function legacyPeerNoticeTextCandidates(record: Record<string, unknown>): string[] {
  const candidates: string[] = [];
  const body = textFromUnknown(record.body).trim();
  if (body) candidates.push(body);
  for (const block of systemNoticeBlockRecords(record)) {
    const blockText = typedNoticeBlockText(block).trim();
    if (blockText) candidates.push(blockText);
    const content = block.content;
    if (!Array.isArray(content)) continue;
    for (const item of content) {
      if (!item || typeof item !== "object") continue;
      const itemRecord = item as Record<string, unknown>;
      const itemText = textFromUnknown(itemRecord.text).trim();
      if (itemText) candidates.push(itemText);
      const data = itemRecord.data;
      if (data && typeof data === "object") {
        const dataText = textFromUnknown((data as Record<string, unknown>).text).trim();
        if (dataText) candidates.push(dataText);
      }
    }
  }
  return candidates;
}

function isLegacyPeerNoticeText(text: string): boolean {
  return /^(Peer (?:message|request|response) from|\[COMMS (?:MESSAGE|REQUEST|RESPONSE)\b)/i.test(text.trim());
}

function isCommsLikeRunStartedPrompt(text: string): boolean {
  const trimmed = text.trim();
  return /(^|\n)\s*Peer (?:message|request|response)(?:\s+from\b|$)/i.test(trimmed)
    || /(^|\n)\s*\[COMMS (?:MESSAGE|REQUEST|RESPONSE)\b/i.test(trimmed);
}

const PEER_ENVELOPE_LINE_RE = /^Peer\s+(?:message|request|response)\s+from\s+(.+):(.*)$/i;
const BRACKETED_COMMS_LINE_RE = /^\[COMMS\s+(?:MESSAGE|REQUEST|RESPONSE)\s+from\s+([^\]]+)\](.*)$/i;

function escapeRegExp(value: string): string {
  return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

function stripPeerEnvelopeByAlias(text: string, peerAliases: string[]): string | null {
  const normalized = text.replace(/\r/g, "\n").trim();
  const aliases = [...peerAliases].filter(Boolean).sort((a, b) => b.length - a.length);
  for (const alias of aliases) {
    const escaped = escapeRegExp(alias);
    const peerEnvelope = new RegExp(
      `^Peer\\s+(?:message|request|response)\\s+from\\s+${escaped}:(?:\\s+|$)`,
      "i",
    );
    const bracketedEnvelope = new RegExp(
      `^\\[COMMS\\s+(?:MESSAGE|REQUEST|RESPONSE)\\s+from\\s+${escaped}\\]\\s*`,
      "i",
    );
    const lines = normalized.split("\n").map((line) => line.trim());
    for (let index = 0; index < lines.length; index++) {
      const line = lines[index];
      if (!line || /^Peer (?:message|request|response)$/i.test(line)) continue;
      const peerMatch = line.match(peerEnvelope);
      const bracketMatch = line.match(bracketedEnvelope);
      if (!peerMatch && !bracketMatch) return null;
      const bodyOnEnvelopeLine = line.replace(peerEnvelope, "").replace(bracketedEnvelope, "").trim();
      const bodyLines = [
        ...(bodyOnEnvelopeLine ? [bodyOnEnvelopeLine] : []),
        ...lines
          .slice(index + 1)
          .filter((candidate) => !isPeerEnvelopeScaffoldLine(candidate, aliases, true)),
      ];
      return bodyLines.join("\n").trim();
    }
  }
  return null;
}

function isPeerEnvelopeScaffoldLine(
  line: string,
  peerAliases: string[] = [],
  allowStandaloneScaffold = false,
): boolean {
  if (!line) return true;
  if (allowStandaloneScaffold && /^Peer (?:message|request|response)$/i.test(line)) return true;
  if (
    peerAliases.length === 0
    && /^\[COMMS (?:MESSAGE|REQUEST|RESPONSE) from [^\]]+\]$/i.test(line)
  ) return true;
  for (const alias of peerAliases) {
    if (!alias) continue;
    const escaped = escapeRegExp(alias);
    if (new RegExp(`^Peer\\s+(?:message|request|response)\\s+from\\s+${escaped}:\\s*$`, "i").test(line)) {
      return true;
    }
    if (new RegExp(`^\\[COMMS\\s+(?:MESSAGE|REQUEST|RESPONSE)\\s+from\\s+${escaped}\\]\\s*$`, "i").test(line)) {
      return true;
    }
  }
  return false;
}

function isImagePlaceholderLine(line: string): boolean {
  return /^\[image:\s*[^\]]+\]$/i.test(line.trim());
}

function normalizePeerEnvelopeText(text: string, peerAliases: string[] = []): string {
  const allowGenericEnvelopeStrip = peerAliases.length === 0;
  const intentBodyStripped = stripCommsIntentBodyPrefix(text, peerAliases);
  const envelopeStripped = intentBodyStripped === text.trim()
    ? stripPeerEnvelopeByAlias(text, peerAliases)
    : null;
  const aliasStripped = envelopeStripped ?? (
    intentBodyStripped === text.trim() ? null : intentBodyStripped
  );
  let normalized = (envelopeStripped !== null
    ? stripBareCommsIntentBodyPrefix(envelopeStripped)
    : aliasStripped ?? text)
    .replace(/\r/g, "\n")
    .split("\n")
    .map((line) => line.trim())
    .filter((line) => {
      if (
        isPeerEnvelopeScaffoldLine(
          line,
          peerAliases,
          allowGenericEnvelopeStrip || aliasStripped !== null,
        )
      ) return false;
      if (isImagePlaceholderLine(line)) return false;
      if (
        allowGenericEnvelopeStrip
        && PEER_ENVELOPE_LINE_RE.test(line)
        && !line.replace(PEER_ENVELOPE_LINE_RE, "$2").trim()
      ) return false;
      return true;
    })
    .join("\n")
  if (allowGenericEnvelopeStrip) {
    normalized = normalized
      .replace(PEER_ENVELOPE_LINE_RE, "$2")
      .replace(/^\[COMMS\s+(?:MESSAGE|REQUEST|RESPONSE)\s+from\s+[^\]]+\]\s*/i, "");
  }
  return normalized.replace(/\s+/g, " ").trim();
}

function normalizeStructuredCommsBodyText(text: string, peerAliases: string[] = []): string {
  const trimmed = text.trim();
  if (trimmed) {
    const stripped = stripPeerEnvelopeByAlias(trimmed, peerAliases);
    if (stripped !== null) return stripped.replace(/\s+/g, " ").trim();
  }
  return trimmed
    .replace(/\r/g, "\n")
    .split("\n")
    .map((line) => line.trim())
    .filter(Boolean)
    .join(" ");
}

function normalizePeerAlias(value: string): string {
  return value.trim().toLowerCase();
}

function peerFromCommsText(text: string): string {
  const trimmed = text.trim();
  const peerLine = trimmed.match(/(?:^|\n)\s*Peer\s+(?:message|request|response)\s+from\s+(.+):(?:[^\n]*)/i);
  if (peerLine?.[1]) return normalizePeerAlias(peerLine[1]);
  const bracketed = trimmed.match(/(?:^|\n)\s*\[COMMS\s+(?:MESSAGE|REQUEST|RESPONSE)\s+from\s+([^\]]+)\]/i);
  if (bracketed?.[1]) return normalizePeerAlias(bracketed[1]);
  return "";
}

type StructuredCommsNoticeSignature = {
  peer: string;
  peerAliases: string[];
  body: string;
  kind?: string;
  direction?: string;
  occurrenceId?: string;
  timestampMs?: number;
  sourceIndex?: number;
  sourceKind?: string;
  consumed?: boolean;
};

const STRUCTURED_COMMS_PROMPT_MATCH_WINDOW_MS = 30_000;

function normalizedPeerAliases(...values: string[]): string[] {
  const aliases: string[] = [];
  for (const value of values) {
    const alias = normalizePeerAlias(value);
    if (alias && !aliases.includes(alias)) aliases.push(alias);
  }
  return aliases;
}

function commsKindFromText(text: string): string {
  const match = text.trim().match(/(?:^|\n)\s*Peer\s+(message|request|response)\s+from\s+/i)
    || text.trim().match(/(?:^|\n)\s*\[COMMS\s+(MESSAGE|REQUEST|RESPONSE)\s+from\s+/i);
  return match?.[1]?.toLowerCase() || "";
}

function systemNoticeCommsSignatures(frame: ConsoleFrame): StructuredCommsNoticeSignature[] {
  const record = commsNoticeMessageRecord(frame);
  if (!record || isExternalEventOnlySystemNotice(record)) return [];
  const isCommsNotice = textFromUnknown(record.kind) === "comms"
    || systemNoticeBlockRecords(record).some((block) => textFromUnknown(block.type) === "comms")
    || (canUseLegacyPeerNoticeText(record)
      && legacyPeerNoticeTextCandidates(record).some(isLegacyPeerNoticeText));
  if (!isCommsNotice) return [];

  const signatures: StructuredCommsNoticeSignature[] = [];
  const seenSignatures = new Set<string>();
  const pushCandidate = (
    candidate: string,
    peerAliases: string[] = [],
    occurrenceId?: string,
    kind?: string,
    direction?: string,
  ) => {
    const aliases = peerAliases.length ? peerAliases : normalizedPeerAliases(peerFromCommsText(candidate));
    const body = normalizePeerEnvelopeText(candidate, aliases);
    if (!body) return;
    const candidateKind = kind || commsKindFromText(candidate);
    const candidateDirection = direction || (commsKindFromText(candidate) ? "incoming" : "");
    const key = [
      aliases.join("|"),
      body,
      candidateKind,
      candidateDirection,
      occurrenceId || "",
    ].join("\u0000");
    if (seenSignatures.has(key)) return;
    seenSignatures.add(key);
    signatures.push({
      peer: aliases[0] || "",
      peerAliases: aliases,
      body,
      kind: candidateKind,
      direction: candidateDirection,
      occurrenceId,
      timestampMs: frame.timestampMs,
      sourceKind: frame.sourceKind,
    });
  };

  const noticeOccurrenceId = textFromUnknown(record.request_id)
    || textFromUnknown(record.correlation_id)
    || textFromUnknown(record.id);
  const noticeBlocks = systemNoticeBlockRecords(record);
  const typedCommsBlocks = noticeBlocks
    .filter((block) => textFromUnknown(block.type) === "comms");
  if (!typedCommsBlocks.length) {
    for (const candidate of legacyPeerNoticeTextCandidates(record)) {
      pushCandidate(candidate, [], noticeOccurrenceId);
    }
  }
  const body = textFromUnknown(record.body);
  if (body && !typedCommsBlocks.length) pushCandidate(body, [], noticeOccurrenceId);
  for (let index = 0; index < typedCommsBlocks.length; index++) {
    const block = typedCommsBlocks[index];
    const peer = block.peer && typeof block.peer === "object"
      ? block.peer as Record<string, unknown>
      : {};
    const peerAliases = normalizedPeerAliases(
      textFromUnknown(peer.display_name),
      textFromUnknown(peer.id),
    );
    const blockOccurrenceId = textFromUnknown(block.request_id)
      || textFromUnknown(block.correlation_id)
      || textFromUnknown(block.id)
      || (noticeOccurrenceId ? `${noticeOccurrenceId}:${index}` : `${index}`);
    const blockKind = textFromUnknown(block.kind);
    const blockDirection = textFromUnknown(block.direction);
    const contentText = typedNoticeContentBlocks(block.content)
      .map((item) => item.type === "paragraph" ? item.text : "")
      .filter(Boolean)
      .join("\n");
    const stableBodyText = typedCommsStableBodyText(block);
    const candidateText = contentText || stableBodyText || body;
    if (candidateText) pushCandidate(candidateText, peerAliases, blockOccurrenceId, blockKind, blockDirection);
  }
  return signatures;
}

function structuredCommsNoticeTextSignatures(frames: ConsoleFrame[]): StructuredCommsNoticeSignature[] {
  const signatures: StructuredCommsNoticeSignature[] = [];
  const seen = new Set<string>();
  for (let index = 0; index < frames.length; index++) {
    const frame = frames[index];
    for (const signature of systemNoticeCommsSignatures(frame)) {
      const primaryPeerAlias = signature.peer || signature.peerAliases[0] || "";
      const key = [
        frame.id || `${frame.event}:${index}`,
        primaryPeerAlias,
        signature.kind || "",
        signature.direction || "",
        signature.occurrenceId || "",
        signature.body,
      ].join("\u0000");
      if (seen.has(key)) continue;
      seen.add(key);
      signatures.push({
        ...signature,
        sourceIndex: index,
      });
    }
  }
  return signatures;
}

function runStartedPromptMatchesStructuredCommsNotice(
  frame: ConsoleFrame,
  signature: StructuredCommsNoticeSignature,
): boolean {
  if (frame.event !== "run_started" || typeof frame.data !== "object" || frame.data === null) {
    return false;
  }
  const prompt = extractPromptText((frame.data as Record<string, unknown>).prompt).trim();
  if (!prompt || !isCommsLikeRunStartedPrompt(prompt)) return false;
  const promptPeer = peerFromCommsText(prompt);
  const promptKind = commsKindFromText(prompt);
  const normalizedPrompt = normalizePeerEnvelopeText(prompt, signature.peerAliases);
  if (!normalizedPrompt) return false;
  if (promptKind && (!signature.kind || promptKind !== signature.kind)) return false;
  if (signature.direction === "outgoing") return false;
  const matchedByAlias = stripPeerEnvelopeByAlias(prompt, signature.peerAliases) !== null;
  if (promptPeer && signature.peerAliases.length === 0) return false;
  if (
    promptPeer
    && signature.peerAliases.length > 0
    && !signature.peerAliases.includes(promptPeer)
    && !matchedByAlias
  ) {
    return false;
  }
  if (
    typeof frame.timestampMs === "number"
    && typeof signature.timestampMs === "number"
    && Math.abs(frame.timestampMs - signature.timestampMs) > STRUCTURED_COMMS_PROMPT_MATCH_WINDOW_MS
  ) {
    return false;
  }
  return Boolean(signature.body && normalizedPrompt === signature.body);
}

function runStartedPromptHasImagePlaceholder(frame: ConsoleFrame): boolean {
  if (frame.event !== "run_started" || typeof frame.data !== "object" || frame.data === null) {
    return false;
  }
  const prompt = extractPromptText((frame.data as Record<string, unknown>).prompt);
  return prompt
    .replace(/\r/g, "\n")
    .split("\n")
    .some(isImagePlaceholderLine);
}

function structuredCommsPromptSuppressionKeys(
  frames: ConsoleFrame[],
  structuredCommsSignatures: StructuredCommsNoticeSignature[],
): Set<string> {
  const keys = new Set<string>();
  const consumed = new Set<string>();
  const consumedStructuredNotices = new Set<string>();
  for (const signature of structuredCommsSignatures) {
    const signatureKey = [
      signature.sourceIndex ?? "",
      signature.peerAliases.join("|"),
      signature.body,
      signature.kind || "",
      signature.direction || "",
      signature.occurrenceId || "",
    ].join("\u0000");
    if (consumedStructuredNotices.has(signatureKey)) continue;
    consumedStructuredNotices.add(signatureKey);
    let best: { key: string; distance: number } | null = null;
    for (let index = 0; index < frames.length; index++) {
      const frame = frames[index];
      const key = `${frame.id || frame.event || "frame"}:${index}`;
      if (consumed.has(key)) continue;
      if (
        typeof signature.timestampMs === "number"
        && typeof frame.timestampMs === "number"
      ) {
        if (frame.timestampMs > signature.timestampMs && !runStartedPromptHasImagePlaceholder(frame)) {
          continue;
        }
        if (
          frame.timestampMs === signature.timestampMs
          && typeof signature.sourceIndex === "number"
          && index > signature.sourceIndex
        ) continue;
      } else if (typeof signature.sourceIndex === "number" && index > signature.sourceIndex) {
        continue;
      }
      if (!runStartedPromptMatchesStructuredCommsNotice(frame, signature)) continue;
      const distance = typeof frame.timestampMs === "number" && typeof signature.timestampMs === "number"
        ? Math.abs(frame.timestampMs - signature.timestampMs)
        : Math.abs(index - (signature.sourceIndex ?? index));
      if (!best || distance < best.distance) {
        best = {
          key,
          distance,
        };
      }
    }
    if (best) {
      keys.add(best.key);
      consumed.add(best.key);
    }
  }
  return keys;
}

function commsNoticeDedupeKeys(frame: ConsoleFrame): string[] {
  const signatures = systemNoticeCommsSignatures(frame);
  const keys: string[] = [];
  for (const signature of signatures) {
    if (!signature.body) continue;
    const key = [
      signature.peer || "unknown",
      signature.kind || "message",
      signature.direction || "incoming",
      signature.occurrenceId || "",
      signature.body,
    ].join(":");
    if (!keys.includes(key)) keys.push(key);
  }
  return keys;
}

function commsNoticeDuplicateKey(
  key: string,
  frame: ConsoleFrame,
  emitted: Map<string, { sourceKind?: string; timestampMs?: number }>,
): boolean {
  const previous = emitted.get(key);
  if (!previous) return false;
  const sourceKind = frame.sourceKind || "live";
  const previousSourceKind = previous.sourceKind || "live";
  const mixedLiveHistory = sourceKind !== previousSourceKind
    && (sourceKind === "session_history" || previousSourceKind === "session_history");
  const closeInTime = typeof frame.timestampMs !== "number"
    || typeof previous.timestampMs !== "number"
    || Math.abs(frame.timestampMs - previous.timestampMs) <= 60_000;
  return mixedLiveHistory && closeInTime;
}

function markCommsNoticeDedupeKey(
  key: string,
  frame: ConsoleFrame,
  emitted: Map<string, { sourceKind?: string; timestampMs?: number }>,
): void {
  emitted.set(key, { sourceKind: frame.sourceKind, timestampMs: frame.timestampMs });
}

function commsNoticeDedupeKeysFromBlock(
  record: Record<string, unknown>,
  fallbackBody: string,
  index: number,
): string[] {
  const type = textFromUnknown(record.type);
  const keys: string[] = [];
  const pushKey = (
    candidate: string,
    peerAliases: string[] = [],
    occurrenceId?: string,
    kind?: string,
    direction?: string,
  ) => {
    const aliases = peerAliases.length ? peerAliases : normalizedPeerAliases(peerFromCommsText(candidate));
    const body = normalizePeerEnvelopeText(candidate, aliases);
    if (!body) return;
    const candidateKind = kind || commsKindFromText(candidate) || "message";
    const candidateDirection = direction || (commsKindFromText(candidate) ? "incoming" : "incoming");
    const key = [
      aliases[0] || "unknown",
      candidateKind,
      candidateDirection,
      occurrenceId || "",
      body,
    ].join(":");
    if (!keys.includes(key)) keys.push(key);
  };

  if (type === "comms") {
    const peer = record.peer && typeof record.peer === "object"
      ? record.peer as Record<string, unknown>
      : {};
    const peerAliases = normalizedPeerAliases(
      textFromUnknown(peer.display_name),
      textFromUnknown(peer.id),
    );
    const contentText = typedNoticeContentBlocks(record.content)
      .map((item) => item.type === "paragraph" ? item.text : "")
      .filter(Boolean)
      .join("\n");
    const stableBodyText = typedCommsStableBodyText(record);
    const occurrenceId = textFromUnknown(record.request_id)
      || textFromUnknown(record.correlation_id)
      || textFromUnknown(record.id)
      || `${index}`;
    pushKey(
      contentText || stableBodyText || fallbackBody,
      peerAliases,
      occurrenceId,
      textFromUnknown(record.kind) || "message",
      textFromUnknown(record.direction) || "incoming",
    );
    return keys;
  }

  if (type && type !== "text") return keys;
  const blockText = typedNoticeBlockText(record).trim();
  if (blockText && isLegacyPeerNoticeText(blockText)) {
    pushKey(blockText);
  }
  const content = record.content;
  if (Array.isArray(content)) {
    for (const item of content) {
      if (!item || typeof item !== "object") continue;
      const itemRecord = item as Record<string, unknown>;
      const itemText = textFromUnknown(itemRecord.text).trim();
      if (itemText && isLegacyPeerNoticeText(itemText)) {
        pushKey(itemText);
      }
      const data = itemRecord.data;
      if (data && typeof data === "object") {
        const dataText = textFromUnknown((data as Record<string, unknown>).text).trim();
        if (dataText && isLegacyPeerNoticeText(dataText)) {
          pushKey(dataText);
        }
      }
    }
  }
  return keys;
}

function consumeCommsNoticeBlockDedupeKeys(
  keys: string[],
  consumeDuplicateCommsBlock?: (key: string) => boolean,
): boolean {
  if (keys.length === 0 || !consumeDuplicateCommsBlock) return false;
  let duplicateCount = 0;
  for (const key of keys) {
    if (consumeDuplicateCommsBlock(key)) duplicateCount += 1;
  }
  return duplicateCount === keys.length;
}

function shouldSuppressDuplicateCommsNotice(
  frame: ConsoleFrame,
  emitted: Map<string, { sourceKind?: string; timestampMs?: number }>,
): boolean {
  const keys = commsNoticeDedupeKeys(frame);
  if (keys.length === 0) return false;
  let duplicateCount = 0;
  for (const key of keys) {
    if (commsNoticeDuplicateKey(key, frame, emitted)) duplicateCount += 1;
  }
  if (duplicateCount === keys.length) {
    return true;
  }
  const record = systemNoticeMessageRecord(frame);
  const hasBlockLevelComms = record
    ? systemNoticeBlockRecords(record).some((block, index) => (
        commsNoticeDedupeKeysFromBlock(block, textFromUnknown(record.body), index).length > 0
      ))
    : false;
  if (!hasBlockLevelComms) {
    for (const key of keys) {
      markCommsNoticeDedupeKey(key, frame, emitted);
    }
  }
  return false;
}

function structuredCommsBodyShouldPreserveLeadingEnvelope(
  body: string,
  peerAliases: string[],
): boolean {
  if (!body.match(/^\s*(?:Peer\s+(?:message|request|response)\s+from\s+.+:|\[COMMS\s+(?:MESSAGE|REQUEST|RESPONSE)\s+from\s+[^\]]+\])\s*\n/i)) {
    return false;
  }
  return peerAliases.some((alias) => alias && !alias.startsWith("implicit-"));
}

function canUseLegacyPeerNoticeText(record: Record<string, unknown>): boolean {
  const kind = textFromUnknown(record.kind);
  if (kind && kind !== "generic") return false;
  const blockTypes = systemNoticeBlockRecords(record)
    .map((block) => textFromUnknown(block.type))
    .filter(Boolean);
  return blockTypes.every((type) => type === "text");
}

export function systemNoticeClearsBusyState(frame: ConsoleFrame): boolean {
  const record = systemNoticeMessageRecord(frame);
  if (!record || isExternalEventOnlySystemNotice(record)) return false;
  if (textFromUnknown(record.kind) === "comms") return true;
  const blocks = systemNoticeBlockRecords(record);
  if (blocks.some((block) => textFromUnknown(block.type) === "comms")) return true;
  if (!canUseLegacyPeerNoticeText(record)) return false;
  return legacyPeerNoticeTextCandidates(record).some(isLegacyPeerNoticeText);
}

function typedSystemNoticeBlocksToRich(
  blocks: unknown,
  body: unknown,
  blobBaseUrl?: string,
  sourceKind?: string,
  consumeDuplicateCommsBlock?: (key: string) => boolean,
): ConversationRichBlock[] {
  const rich: ConversationRichBlock[] = [];
  const bodyText = textFromUnknown(body);
  if (!Array.isArray(blocks)) {
    if (bodyText) rich.push({ type: "paragraph", text: bodyText });
    return rich;
  }

  let consumedDuplicateCommsBlock = false;
  for (let index = 0; index < blocks.length; index++) {
    const block = blocks[index];
    if (!block || typeof block !== "object") continue;
    const record = block as Record<string, unknown>;
    const type = textFromUnknown(record.type);
    if (type === "comms") {
      const dedupeKeys = commsNoticeDedupeKeysFromBlock(record, bodyText, index);
      if (consumeCommsNoticeBlockDedupeKeys(dedupeKeys, consumeDuplicateCommsBlock)) {
        consumedDuplicateCommsBlock = true;
        continue;
      }
      const peer = record.peer && typeof record.peer === "object"
        ? record.peer as Record<string, unknown>
        : {};
      const peerLabel = peerLastSegment(textFromUnknown(peer.display_name) || textFromUnknown(peer.id) || "peer");
      const peerAliases = normalizedPeerAliases(
        textFromUnknown(peer.display_name),
        textFromUnknown(peer.id),
      );
      const kind = textFromUnknown(record.kind) || "message";
      const direction = textFromUnknown(record.direction);
      const intent = textFromUnknown(record.intent);
      const requestId = textFromUnknown(record.request_id) || `typed-comms:${peerLabel}:${kind}`;
      const contentBlocks = typedNoticeContentBlocks(record.content, blobBaseUrl);
      const contentText = contentBlocks
        .map((item) => item.type === "paragraph" ? item.text : "")
        .filter(Boolean)
        .join("\n")
        .trim();
      const peerImages = contentBlocks.filter((item) => item.type === "image");
      const displayBodySource = contentText || typedCommsStableBodyText(record) || bodyText;
      const preserveStructuredContentEnvelope = structuredCommsBodyShouldPreserveLeadingEnvelope(
        displayBodySource,
        peerAliases,
      );
      const displayBody = normalizeStructuredCommsBodyText(
        displayBodySource,
        preserveStructuredContentEnvelope ? [] : peerAliases,
      );
      rich.push({
        type: "tool-call",
        toolCallId: requestId,
        name: `peer_${kind}`,
        arguments: JSON.stringify(record.payload ?? {}, null, 2),
        status: "success",
        peerIncoming: direction !== "outgoing",
        peerTarget: peerLabel,
        ...(intent ? { peerIntent: intent } : {}),
        peerBody: displayBody || undefined,
        ...(peerImages.length > 0 ? { peerImages } : {}),
      });
      continue;
    }
    const legacyDedupeKeys = commsNoticeDedupeKeysFromBlock(record, bodyText, index);
    if (consumeCommsNoticeBlockDedupeKeys(legacyDedupeKeys, consumeDuplicateCommsBlock)) {
      consumedDuplicateCommsBlock = true;
      continue;
    }
    if (type === "external_event") {
      // External events are model-facing delivery envelopes for operator
      // sends. The canonical user-facing render is the user_input frame; if
      // we render these notices too, rich/image sends appear duplicated.
      continue;
    }
    if (type === "tool_config" || type === "mcp") {
      const payload = record.payload && typeof record.payload === "object"
        ? record.payload as Record<string, unknown>
        : record;
      const label = type === "mcp" ? "MCP" : "Tool config";
      const text = bodyText || typedNoticeBlockText(payload) || typedNoticeBlockText(record) || label;
      rich.push({ type: "divider", text });
      continue;
    }
    if (type === "background_job" || type === "auth" || type === "runtime_notice") {
      const text = typedNoticeBlockText(record) || type.replace(/_/g, " ");
      rich.push({ type: "paragraph", text });
      continue;
    }
    const contentBlocks = typedNoticeContentBlocks(record.content, blobBaseUrl);
    if (contentBlocks.length > 0) {
      rich.push(...contentBlocks);
      continue;
    }
    rich.push({ type: "divider", text: typedNoticeBlockText(record) || "Runtime metadata" });
  }
  if (rich.length === 0 && bodyText && !consumedDuplicateCommsBlock) {
    rich.push({ type: "paragraph", text: bodyText });
  }
  return rich;
}

function historyMessageText(
  message: unknown,
  peerRegistry?: Map<string, string>,
  blobBaseUrl?: string,
  toolResults?: Map<string, HistoryToolResult>,
  sourceKind?: string,
  consumeDuplicateCommsBlock?: (key: string) => boolean,
): { role: "user" | "assistant" | "system" | "meta" | null; text: string; blocks?: ConversationRichBlock[] } {
  if (!message || typeof message !== "object") {
    return { role: null, text: "" };
  }
  const record = message as Record<string, unknown>;
  const role = typeof record.role === "string" ? record.role : null;
  switch (role) {
    case "user": {
      const text = extractTextFromContentBlocks(record.content);
      return { role: "user", text };
    }
    case "system_notice": {
      const blocks = typedSystemNoticeBlocksToRich(
        record.blocks,
        record.body,
        blobBaseUrl,
        sourceKind,
        consumeDuplicateCommsBlock,
      );
      const duplicateCommsConsumed = Boolean(
        consumeDuplicateCommsBlock
        && blocks.length === 0
        && systemNoticeBlockRecords(record).some((block, index) => (
          commsNoticeDedupeKeysFromBlock(
            block,
            textFromUnknown(record.body),
            index,
          ).length > 0
        )),
      );
      const text = duplicateCommsConsumed
        ? ""
        : typeof record.body === "string"
        ? record.body
        : blocks.map((block) => block.type === "paragraph" || block.type === "divider" ? block.text : "").filter(Boolean).join("\n");
      return { role: "meta", text, ...(blocks.length > 0 ? { blocks } : {}) };
    }
    case "assistant":
      return { role: "assistant", text: typeof record.content === "string" ? record.content : "" };
    case "block_assistant": {
      const blocks = Array.isArray(record.blocks) ? record.blocks : [];
      const toolBlocks = blockAssistantToolBlocks(blocks, peerRegistry, toolResults);
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
        .join("\n\n");
      return { role: "assistant", text, ...(toolBlocks.length > 0 ? { blocks: toolBlocks } : {}) };
    }
    case "system":
      return { role: "system", text: typeof record.content === "string" ? record.content : "" };
    default:
      return { role: null, text: "" };
  }
}

function renderSessionHistoryTextCompleteEntry(
  agent: ConsoleAgent | null,
  frame: ConsoleFrame,
  entryId: string,
  options: {
    consumeDuplicateToolBlock?: (block: ConversationRichToolCallBlock) => boolean;
    consumeDuplicateCommsBlock?: (key: string) => boolean;
    peerRegistry?: Map<string, string>;
    blobBaseUrl?: string;
    toolResults?: Map<string, HistoryToolResult>;
  } = {},
): ConversationTimelineEntry | null {
  if (frame.sourceKind !== "session_history") return null;
  const record = frame.data && typeof frame.data === "object"
    ? frame.data as Record<string, unknown>
    : {};
  const parsed = historyMessageText(
    record.message,
    options.peerRegistry,
    options.blobBaseUrl,
    options.toolResults,
    frame.sourceKind,
    options.consumeDuplicateCommsBlock,
  );
  const text = parsed.text.trim();
  const parsedBlocks = Array.isArray(parsed.blocks) ? parsed.blocks : [];
  if (parsed.role === "meta") {
    const filteredParsedBlocks = options.consumeDuplicateToolBlock
      ? parsedBlocks.filter((block) => {
          if (block.type !== "tool-call") return true;
          return !options.consumeDuplicateToolBlock?.(block);
        })
      : parsedBlocks;
    if (!text && filteredParsedBlocks.length === 0) return null;
    const blocks = filteredParsedBlocks.length > 0
      ? filteredParsedBlocks
      : parseConversationRichBlocks(text);
    return {
      kind: "message",
      id: entryId,
      identity: COMMS_IDENTITY,
      variant: blocks.length > 0 ? "rich" : "meta",
      createdAt: isoFromTimestampMs(frame.timestampMs),
      ...(blocks.length > 0 ? { blocks } : { text }),
    };
  }
  if (parsed.role !== "assistant" || (!text && parsedBlocks.length === 0)) return null;
  if (/^I have acknowledged the addition of the following peers:/i.test(text)) {
    return null;
  }
  const filteredParsedBlocks = options.consumeDuplicateToolBlock
    ? parsedBlocks.filter((block) => {
        if (block.type !== "tool-call") return true;
        return !options.consumeDuplicateToolBlock?.(block);
      })
    : parsedBlocks;
  if (!text && filteredParsedBlocks.length === 0) return null;
  const blocks = filteredParsedBlocks.length > 0
    ? filteredParsedBlocks
    : parseConversationRichBlocks(text);
  return {
    kind: "message",
    id: entryId,
    identity: agentIdentity(agent),
    variant: blocks.length > 0 ? "rich" : "plain",
    createdAt: isoFromTimestampMs(frame.timestampMs),
    ...(blocks.length > 0 ? { blocks } : { text }),
  };
}

function renderSystemNoticeEntry(
  frame: ConsoleFrame,
  entryId: string,
  options: {
    consumeDuplicateToolBlock?: (block: ConversationRichToolCallBlock) => boolean;
    consumeDuplicateCommsBlock?: (key: string) => boolean;
    blobBaseUrl?: string;
  } = {},
): ConversationTimelineEntry | null {
  if (frame.event !== "system_notice") return null;
  const record = frame.data && typeof frame.data === "object"
    ? frame.data as Record<string, unknown>
    : {};
  const rawMessage = record.message && typeof record.message === "object"
    ? record.message as Record<string, unknown>
    : null;
  const message = rawMessage
    ? (textFromUnknown(rawMessage.role) ? rawMessage : { role: "system_notice", ...rawMessage })
    : {
        role: "system_notice",
        kind: record.kind,
        render_class: record.render_class,
        body: record.body,
        blocks: record.blocks,
      };
  if (isExternalEventOnlySystemNotice(message)) return null;
  const parsed = historyMessageText(
    message,
    undefined,
    options.blobBaseUrl,
    undefined,
    frame.sourceKind,
    options.consumeDuplicateCommsBlock,
  );
  if (parsed.role !== "meta") return null;
  const parsedBlocks = Array.isArray(parsed.blocks) ? parsed.blocks : [];
  const filteredParsedBlocks = options.consumeDuplicateToolBlock
    ? parsedBlocks.filter((block) => {
        if (block.type !== "tool-call") return true;
        return !options.consumeDuplicateToolBlock?.(block);
      })
    : parsedBlocks;
  const text = parsed.text.trim();
  if (!text && filteredParsedBlocks.length === 0) return null;
  const blocks = filteredParsedBlocks.length > 0
    ? filteredParsedBlocks
    : parseConversationRichBlocks(text);
  return {
    kind: "message",
    id: entryId,
    identity: COMMS_IDENTITY,
    variant: blocks.length > 0 ? "rich" : "meta",
    createdAt: isoFromTimestampMs(frame.timestampMs),
    ...(blocks.length > 0 ? { blocks } : { text }),
  };
}

export function mapFramesToTimelineEntries(
  agent: ConsoleAgent | null,
  frames: ConsoleFrame[],
  options: {
    renderInteractionStartsAsUser?: boolean;
    renderTextDeltas?: boolean;
    suppressEmbeddedRunStartedPrompt?: boolean;
    blobBaseUrl?: string;
  } = {},
): ConversationTimelineEntry[] {
  // Live streams keep store order so unscoped comms events do not jump
  // into active turns. Persisted interaction history asks for user prompts,
  // so restore the turn-local semantic order before rendering.
  const orderedFrames = options.renderInteractionStartsAsUser
    ? sortFramesForTranscript(frames)
    : frames;
  const entries: ConversationTimelineEntry[] = [];
  const toolBlocks = buildToolBlocks(orderedFrames);
  const peerRegistry = buildPeerRegistry(orderedFrames);
  const sessionToolResults = historyToolResults(orderedFrames);
  const structuredCommsSignatures = structuredCommsNoticeTextSignatures(orderedFrames);
  const structuredCommsPromptSuppression = structuredCommsPromptSuppressionKeys(
    orderedFrames,
    structuredCommsSignatures,
  );
  const emittedToolCalls = new Set<string>();
  const {
    liveToolCallIds,
    liveToolSignatureCounts,
  } = liveToolDedupeState(orderedFrames, toolBlocks);
  const liveAssistantTerminalTexts = liveAssistantTerminalTextSignatures(orderedFrames);
  const emittedImages = new Set<string>();
  const emittedUserInputs = new Set<string>();
  const emittedCommsNotices = new Map<string, { sourceKind?: string; timestampMs?: number }>();

  let pendingText = "";
  let pendingId = "";
  let pendingCreatedAt: string | undefined;
  let streamedInteractionText = "";
  let streamedInteractionId = "";

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
      const frameInteractionId = frame.interactionId?.trim() || "";
      if (frameInteractionId !== streamedInteractionId) {
        streamedInteractionText = "";
        streamedInteractionId = frameInteractionId;
      }
      const delta = summarizeFrameData(frame.data);
      if (!pendingId) {
        pendingId = entryId;
        pendingCreatedAt = isoFromTimestampMs(frame.timestampMs);
      }
      pendingText += delta;
      streamedInteractionText += delta;
      continue;
    }

    if (frame.event === "assistant_image" || frame.event === "assistant_image_appended") {
      flushPendingText();
      const imageEntry = renderAssistantImageEntry(agent, frame, entryId, options.blobBaseUrl);
      if (imageEntry) {
        const key = imageEntryKey(imageEntry);
        if (key && emittedImages.has(key)) {
          continue;
        }
        if (key) emittedImages.add(key);
        entries.push(imageEntry);
      }
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
        // Group consecutive tool calls of the same `name` into one
        // rich entry. Any frame that produces its own visible entry
        // (text bubble, user message, system notice) lands between
        // the tool entries in `entries[]` and breaks this match — so
        // grouping naturally fires only for runs of same-tool calls
        // with no user-facing output between them. Peer tools have
        // an extra direction constraint (incoming vs outgoing) so a
        // fresh `send_response` doesn't fold into a `send_request`
        // group.
        const lastEntry = entries[entries.length - 1];
        const lastBlocks = lastEntry
          && lastEntry.kind === "message"
          && lastEntry.variant === "rich"
          && Array.isArray(lastEntry.blocks)
          ? lastEntry.blocks as Array<Record<string, unknown>>
          : null;
        const lastIsToolGroup = !!(lastBlocks
          && lastBlocks.length > 0
          && lastBlocks.every((b) => b.type === "tool-call"));
        const lastSameName = lastIsToolGroup
          && lastBlocks!.every((b) => b.name === block.name);
        const newIncoming = block.peerIncoming === true;
        const peerCompatible = !block.peerTarget
          || (lastIsToolGroup
              && lastBlocks!.every((b) => Boolean(b.peerIncoming) === newIncoming));

        if (lastSameName && peerCompatible) {
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
      const imageEntries = renderGeneratedImageToolResultEntries(
        agent,
        frame,
        entryId,
        options.blobBaseUrl,
      );
      for (const imageEntry of imageEntries) {
        const key = imageEntryKey(imageEntry);
        if (key && emittedImages.has(key)) continue;
        if (key) emittedImages.add(key);
        entries.push(imageEntry);
      }
      continue;
    }

    if (options.renderInteractionStartsAsUser && (frame.event === "interaction_started" || frame.event === "user_input")) {
      flushPendingText();
      const frameInteractionId = frame.interactionId?.trim() || "";
      if (frameInteractionId !== streamedInteractionId) {
        streamedInteractionText = "";
        streamedInteractionId = frameInteractionId;
      }
      const userEntry = renderHistoryUserEntry(frame, entryId, options.blobBaseUrl);
      if (userEntry) {
        const userKey = userEntryDedupeKey(frame, userEntry);
        if (userKey && emittedUserInputs.has(userKey)) {
          continue;
        }
        if (userKey) emittedUserInputs.add(userKey);
        entries.push(userEntry);
      }
      continue;
    }

    if (frame.event === "run_started") {
      flushPendingText();
      const promptEntries = renderRunStartedPromptEntries(frame, entryId, {
        suppressEmbeddedRpcPrompt: options.suppressEmbeddedRunStartedPrompt === true,
        suppressStructuredCommsPrompt: structuredCommsPromptSuppression.has(entryId),
        blobBaseUrl: options.blobBaseUrl,
      });
      if (promptEntries.length > 0) {
        for (const promptEntry of promptEntries) {
          const userKey = userPromptDedupeKey(frame, promptEntry);
          if (userKey && emittedUserInputs.has(userKey)) {
            continue;
          }
          if (userKey) emittedUserInputs.add(userKey);
          entries.push(promptEntry);
        }
        continue;
      }
    }

    if (frame.event === "system_notice") {
      flushPendingText();
      if (shouldSuppressDuplicateCommsNotice(frame, emittedCommsNotices)) {
        continue;
      }
      const noticeEntry = renderSystemNoticeEntry(frame, entryId, {
        blobBaseUrl: options.blobBaseUrl,
        consumeDuplicateCommsBlock: (key) => {
          if (commsNoticeDuplicateKey(key, frame, emittedCommsNotices)) {
            return true;
          }
          markCommsNoticeDedupeKey(key, frame, emittedCommsNotices);
          return false;
        },
        consumeDuplicateToolBlock: (block) => (
          liveToolCallIds.has(block.toolCallId)
          || consumeToolSignatureCount(liveToolSignatureCounts, block)
        ),
      });
      if (noticeEntry) {
        entries.push(noticeEntry);
      }
      continue;
    }

    if (frame.event === "text_complete") {
      if (frame.sourceKind !== "session_history") {
        const text = terminalFrameVisibleText(frame).trim();
        if (
          text
          && pendingText
          && normalizeComparableText(pendingText) === normalizeComparableText(text)
        ) {
          continue;
        }
        const interactionId = frame.interactionId?.trim();
        const duplicateTerminalFollows = text
          && orderedFrames.slice(i + 1).some((later) => {
            if (
              later.event !== "interaction_complete"
              && later.event !== "run_completed"
            ) {
              return false;
            }
            if (interactionId && later.interactionId?.trim() !== interactionId) {
              return false;
            }
            return normalizeComparableText(terminalFrameVisibleText(later)) === normalizeComparableText(text);
          });
        if (duplicateTerminalFollows) {
          continue;
        }
      }
      const historyText = frame.sourceKind === "session_history"
        ? terminalFrameVisibleText(frame).trim()
        : "";
      if (
        historyText
        && liveAssistantTerminalTexts.has(normalizeComparableText(historyText))
      ) {
        continue;
      }
        const historyEntry = renderSessionHistoryTextCompleteEntry(agent, frame, entryId, {
          peerRegistry,
          blobBaseUrl: options.blobBaseUrl,
          toolResults: sessionToolResults,
          consumeDuplicateCommsBlock: (key) => {
            if (commsNoticeDuplicateKey(key, frame, emittedCommsNotices)) {
              return true;
            }
            markCommsNoticeDedupeKey(key, frame, emittedCommsNotices);
            return false;
          },
          consumeDuplicateToolBlock: (block) => (
            liveToolCallIds.has(block.toolCallId)
            || consumeToolSignatureCount(liveToolSignatureCounts, block)
          ),
        });
        if (historyEntry) {
          flushPendingText();
          if (shouldSuppressRepeatedAssistantEntry(historyEntry, entries)) {
            continue;
          }
          entries.push(historyEntry);
        }
        continue;
      }

    if (
      frame.event === "interaction_complete"
      || frame.event === "interaction_failed"
      || frame.event === "run_failed"
    ) {
      const streamedText = streamedInteractionText || pendingText;
      flushPendingText();
      streamedInteractionText = "";
      streamedInteractionId = "";
      if (frame.sourceKind === "session_history") {
        const historyText = terminalFrameVisibleText(frame).trim();
        if (
          historyText
          && liveAssistantTerminalTexts.has(normalizeComparableText(historyText))
        ) {
          continue;
        }
        const historyEntry = renderSessionHistoryTextCompleteEntry(agent, frame, entryId, {
          peerRegistry,
          blobBaseUrl: options.blobBaseUrl,
          toolResults: sessionToolResults,
          consumeDuplicateCommsBlock: (key) => {
            if (commsNoticeDuplicateKey(key, frame, emittedCommsNotices)) {
              return true;
            }
            markCommsNoticeDedupeKey(key, frame, emittedCommsNotices);
            return false;
          },
          consumeDuplicateToolBlock: (block) => (
            liveToolCallIds.has(block.toolCallId)
            || consumeToolSignatureCount(liveToolSignatureCounts, block)
          ),
        });
        if (historyEntry) {
          if (shouldSuppressRepeatedAssistantEntry(historyEntry, entries)) {
            continue;
          }
          entries.push(historyEntry);
        }
        continue;
      }
      const terminalEntry = renderTerminalEntry(agent, frame, entryId, streamedText);
      if (terminalEntry) {
        if (shouldSuppressRepeatedAssistantEntry(terminalEntry, entries)) {
          continue;
        }
        entries.push(terminalEntry);
      }
      continue;
    }

    if (HIDDEN_EVENTS.has(frame.event)) {
      continue;
    }

    flushPendingText();

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

export function createUserEntry(
  message: string,
  images: Array<{ src: string; mediaType: string; alt?: string }> = [],
): ConversationTimelineEntry {
  if (images.length > 0) {
    const blocks: ConversationRichBlock[] = [
      ...parseConversationRichBlocks(message),
      ...images.map((image) => ({
        type: "image" as const,
        src: image.src,
        mediaType: image.mediaType,
        alt: image.alt || "attached image",
      })),
    ];
    return {
      kind: "message",
      id: `user:${Date.now()}`,
      identity: USER_IDENTITY,
      variant: "rich",
      createdAt: new Date().toISOString(),
      blocks,
    };
  }
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

export function appendOptimisticConversationEntry(
  entries: ConversationTimelineEntry[],
  optimisticEntry: ConversationTimelineEntry | null | undefined,
): ConversationTimelineEntry[] {
  return optimisticEntry ? [...entries, optimisticEntry] : entries;
}

export function inferResponsePhaseFromFrames(
  frames: ConsoleFrame[],
  fallback: ResponsePhase = null,
): ResponsePhase {
  let phase: ResponsePhase = fallback;
  for (const frame of frames) {
    switch (frame.event) {
      case "user_input":
        if (isTerminalUserInputStatus(frame.status)) phase = null;
        else phase = "waiting";
        break;
      case "interaction_started":
        phase = "waiting";
        break;
      case "tool_call_requested":
      case "tool_call":
      case "tool_execution_started":
        phase = "tool-executing";
        break;
      case "tool_result_received":
      case "tool_execution_completed":
        // A completed tool is not the same as a completed turn. In spawned
        // worker histories the runtime may not project run_started/
        // interaction_started, so keep the pane busy until text/run terminal
        // evidence arrives; otherwise operator sends bypass the local queue.
        phase = "waiting";
        break;
      case "reasoning_delta":
        phase = "generating";
        break;
      case "reasoning_complete":
        phase = "waiting";
        break;
      case "text_delta":
        phase = "generating";
        break;
      case "text_complete":
      case "interaction_complete":
      case "interaction_failed":
      case "run_completed":
      case "run_failed":
        phase = null;
        break;
      case "system_notice":
        if (systemNoticeClearsBusyState(frame)) phase = null;
        break;
      case "turn_completed": {
        const data = frame.data && typeof frame.data === "object" ? frame.data as Record<string, unknown> : {};
        const stopReason = data.stop_reason ?? data.stopReason;
        if (typeof stopReason === "string" ? stopReason !== "tool_use" : true) phase = null;
        break;
      }
      default:
        break;
    }
  }
  return phase;
}

function isTerminalUserInputStatus(status?: string): boolean {
  return status === "completed" || status === "delivery_failed" || status === "failed";
}

export function resolvePanelResponsePhase(args: {
  frames: ConsoleFrame[];
  serverPhase?: ResponsePhase;
  localPhase?: ResponsePhase;
  hasLocalPhase?: boolean;
}): ResponsePhase {
  if (args.hasLocalPhase) {
    return args.localPhase ?? null;
  }
  if (args.frames.length > 0) {
    const localPhase = inferResponsePhaseFromFrames(args.frames, null);
    if (args.serverPhase && localPhase === null && !latestRoutableFrameIsTerminal(args.frames)) {
      return args.serverPhase;
    }
    return localPhase;
  }
  return args.serverPhase ?? null;
}

function latestRoutableFrameIsTerminal(frames: ConsoleFrame[]): boolean {
  for (let index = frames.length - 1; index >= 0; index -= 1) {
    const frame = frames[index];
    switch (frame.event) {
      case "user_input":
        return isTerminalUserInputStatus(frame.status);
      case "text_complete":
      case "interaction_complete":
      case "interaction_failed":
      case "run_completed":
      case "run_failed":
      case "message_delivery_failed":
        return true;
      case "system_notice":
        return systemNoticeClearsBusyState(frame);
      case "turn_completed": {
        const data = frame.data && typeof frame.data === "object" ? frame.data as Record<string, unknown> : {};
        const stopReason = data.stop_reason ?? data.stopReason;
        return typeof stopReason === "string" ? stopReason !== "tool_use" : true;
      }
      case "interaction_started":
      case "run_started":
      case "tool_call_requested":
      case "tool_call":
      case "tool_execution_started":
      case "tool_result_received":
      case "tool_execution_completed":
      case "reasoning_delta":
      case "reasoning_complete":
      case "text_delta":
        return false;
      default:
        break;
    }
  }
  return false;
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
    if (frame.sourceKind === "session_history") {
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
