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

function eventSortRank(event: string | undefined): number {
  switch (event) {
    case "user_input":
    case "interaction_started":
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
  "frame_updated",
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

function terminalFrameVisibleText(frame: ConsoleFrame): string {
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
  const timestamp = typeof frame.timestampMs === "number" ? frame.timestampMs : "";
  return signature ? `content:${timestamp}:${signature}` : "";
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
  const prompt = extractPromptText(record.prompt).trim();
  if (!prompt) {
    return [];
  }
  const createdAt = isoFromTimestampMs(frame.timestampMs);
  const entries: ConversationTimelineEntry[] = [];

  void options;
  void createdAt;

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
      const source = typeof record.source === "string" ? record.source : "";
      const blobId = typeof record.blob_id === "string"
        ? record.blob_id
        : typeof record.blobId === "string"
          ? record.blobId
          : "";
      const mediaType = typeof record.media_type === "string"
        ? record.media_type
        : typeof record.mediaType === "string"
          ? record.mediaType
          : "image/png";
      const inlineData = typeof record.data === "string"
        ? record.data
        : typeof record.base64 === "string"
          ? record.base64
          : "";
      const src = source === "blob" && blobId
        ? buildBlobUrl(blobId, blobBaseUrl)
        : inlineData
          ? `data:${mediaType};base64,${inlineData}`
          : "";
      if (!src) continue;
      const alt = typeof record.alt === "string" && record.alt.trim()
        ? record.alt.trim()
        : type === "image_ref"
          ? "referenced image"
          : "attached image";
      const width = typeof record.width === "number" ? record.width : undefined;
      const height = typeof record.height === "number" ? record.height : undefined;
      blocks.push({
        type: "image",
        src,
        mediaType,
        alt,
        ...(width !== undefined ? { width } : {}),
        ...(height !== undefined ? { height } : {}),
        ...(blobId ? { blobId } : {}),
      });
    }
  }
  return blocks;
}

function peerLastSegment(value: string): string {
  return value.split("/").pop() || value;
}

function blockAssistantToolBlocks(
  blocks: unknown[],
  peerRegistry?: Map<string, string>,
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
    toolBlocks.push({
      type: "tool-call",
      toolCallId: id,
      name,
      arguments: argumentsText,
      status: "success",
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

function typedSystemNoticeBlocksToRich(
  blocks: unknown,
  body: unknown,
  blobBaseUrl?: string,
): ConversationRichBlock[] {
  const rich: ConversationRichBlock[] = [];
  const bodyText = textFromUnknown(body);
  if (!Array.isArray(blocks)) {
    if (bodyText) rich.push({ type: "paragraph", text: bodyText });
    return rich;
  }

  for (const block of blocks) {
    if (!block || typeof block !== "object") continue;
    const record = block as Record<string, unknown>;
    const type = textFromUnknown(record.type);
    if (type === "comms") {
      const peer = record.peer && typeof record.peer === "object"
        ? record.peer as Record<string, unknown>
        : {};
      const peerLabel = peerLastSegment(textFromUnknown(peer.display_name) || textFromUnknown(peer.id) || "peer");
      const kind = textFromUnknown(record.kind) || "message";
      const intent = textFromUnknown(record.intent);
      const requestId = textFromUnknown(record.request_id) || `typed-comms:${peerLabel}:${kind}`;
      const contentBlocks = typedNoticeContentBlocks(record.content, blobBaseUrl);
      const contentText = contentBlocks
        .map((item) => item.type === "paragraph" ? item.text : "")
        .filter(Boolean)
        .join("\n")
        .trim();
      const displayBody = (contentText || typedNoticeBlockText(record))
        .replace(/^Peer\s+(?:message|request|response)\s+from\s+[^\n:]+:\s*/i, "")
        .trim();
      rich.push({
        type: "tool-call",
        toolCallId: requestId,
        name: `peer_${kind}`,
        arguments: JSON.stringify(record.payload ?? {}, null, 2),
        status: "success",
        peerIncoming: true,
        peerTarget: peerLabel,
        ...(intent ? { peerIntent: intent } : {}),
        peerBody: displayBody || undefined,
      });
      rich.push(...contentBlocks.filter((item) => item.type !== "paragraph"));
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
      const text = typedNoticeBlockText(payload) || typedNoticeBlockText(record) || label;
      rich.push({ type: "divider", text });
      continue;
    }
    if (type === "background_job" || type === "auth" || type === "runtime_notice") {
      const text = typedNoticeBlockText(record) || type.replace(/_/g, " ");
      rich.push({ type: "paragraph", text });
      continue;
    }
    rich.push({ type: "divider", text: typedNoticeBlockText(record) || "Runtime metadata" });
  }
  if (rich.length === 0 && bodyText) rich.push({ type: "paragraph", text: bodyText });
  return rich;
}

function historyMessageText(
  message: unknown,
  peerRegistry?: Map<string, string>,
  blobBaseUrl?: string,
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
      const blocks = typedSystemNoticeBlocksToRich(record.blocks, record.body, blobBaseUrl);
      const text = typeof record.body === "string"
        ? record.body
        : blocks.map((block) => block.type === "paragraph" || block.type === "divider" ? block.text : "").filter(Boolean).join("\n");
      return { role: "meta", text, ...(blocks.length > 0 ? { blocks } : {}) };
    }
    case "assistant":
      return { role: "assistant", text: typeof record.content === "string" ? record.content : "" };
    case "block_assistant": {
      const blocks = Array.isArray(record.blocks) ? record.blocks : [];
      const toolBlocks = blockAssistantToolBlocks(blocks, peerRegistry);
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
    peerRegistry?: Map<string, string>;
    blobBaseUrl?: string;
  } = {},
): ConversationTimelineEntry | null {
  if (frame.sourceKind !== "session_history") return null;
  const record = frame.data && typeof frame.data === "object"
    ? frame.data as Record<string, unknown>
    : {};
  const parsed = historyMessageText(record.message, options.peerRegistry, options.blobBaseUrl);
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
    blobBaseUrl?: string;
  } = {},
): ConversationTimelineEntry | null {
  if (frame.event !== "system_notice") return null;
  const record = frame.data && typeof frame.data === "object"
    ? frame.data as Record<string, unknown>
    : {};
  const message = record.message && typeof record.message === "object"
    ? record.message
    : {
        role: "system_notice",
        body: record.body,
        blocks: record.blocks,
      };
  const parsed = historyMessageText(message, undefined, options.blobBaseUrl);
  if (parsed.role !== "meta") return null;
  const parsedBlocks = Array.isArray(parsed.blocks) ? parsed.blocks : [];
  const filteredParsedBlocks = options.consumeDuplicateToolBlock
    ? parsedBlocks.filter((block) => {
        if (block.type !== "tool-call") return true;
        return !options.consumeDuplicateToolBlock?.(block);
      })
    : parsedBlocks;
  const hasConversationNoticeBlock = filteredParsedBlocks.some(
    (block) => block.type === "tool-call" || block.type === "image",
  );
  if (!hasConversationNoticeBlock) return null;
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
  const emittedToolCalls = new Set<string>();
  const {
    liveToolCallIds,
    liveToolSignatureCounts,
  } = liveToolDedupeState(orderedFrames, toolBlocks);
  const liveAssistantTerminalTexts = liveAssistantTerminalTextSignatures(orderedFrames);
  const emittedImages = new Set<string>();
  const emittedUserInputs = new Set<string>();

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
        suppressEmbeddedRpcPrompt:
          options.renderInteractionStartsAsUser === true
          || options.suppressEmbeddedRunStartedPrompt === true,
      });
      if (promptEntries.length > 0) {
        entries.push(...promptEntries);
        continue;
      }
    }

    if (frame.event === "system_notice") {
      flushPendingText();
      const noticeEntry = renderSystemNoticeEntry(frame, entryId, {
        blobBaseUrl: options.blobBaseUrl,
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
      const streamedText = pendingText;
      flushPendingText();
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
      case "text_complete":
      case "interaction_complete":
      case "interaction_failed":
      case "run_completed":
      case "run_failed":
        phase = null;
        break;
      case "turn_completed": {
        const data = frame.data && typeof frame.data === "object" ? frame.data as Record<string, unknown> : {};
        if (data.stop_reason === "end_turn") phase = null;
        break;
      }
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
