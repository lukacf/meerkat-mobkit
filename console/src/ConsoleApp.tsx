import React from "react";
import "@console-components/styles";
import "./console-host.css";

import {
  ConsoleActivityRail,
  ConsoleComposer,
  ConsoleDock,
  ConsoleSidebar,
  ConsoleWorkbench,
  useConsoleDockController,
} from "@console-components";
import type { WorkGraphCardActions } from "@console-components";
import type {
  ConsoleDockState,
  ConsoleWorkbenchTarget,
  ConversationTimelineEntry,
  IdentityInspectViewState,
  IdentityStatusRow,
} from "@console-core";
import {
  migrateConsoleWorkbenchTarget,
  normalizeConsoleDockState,
  normalizeIdentityInspectViewState,
} from "@console-core";

import { canonicalConsoleIdentity, normalizeAgents } from "./lib/agents";
import {
  buildActivityRailViewState,
  buildControlTarget,
  buildConversationViewState,
  buildDockTarget,
  buildInspectTarget,
  buildPanelConversationKey,
  buildRoutingSectionView,
  buildSidebarViewState,
  buildWorkGraphOperatorResultFrame,
  createUserEntry,
  appendOptimisticConversationEntry,
  framesContainWorkGraphCards,
  inferResponsePhaseFromFrames,
  mapFramesToTimelineEntries,
  optimisticUserMessageForPanel,
  resolvePanelResponsePhase,
  systemNoticeClearsBusyState,
  type MobKitDockTarget,
  type OptimisticUserMessage,
} from "./lib/adapters";
import { errorMessage, jsonRpcErrorCode } from "./lib/errors";
import {
  DEFAULT_CONSOLE_FETCH_TIMEOUT_MS,
} from "./lib/network";
import {
  CONSOLE_COMMAND_NAMES,
  consoleCommandMethod,
  createHttpConsoleTransport,
  createMobKitConsoleController,
} from "./lib/headless";
import {
  WORKGRAPH_CONFLICT_CODE,
  resolveWorkGraphBindingRevision,
  resolveWorkGraphGoalItemRevision,
  resolveWorkGraphItemRevision,
  workGraphClaimOwnerId,
  workGraphConflictRefreshRequest,
  type WorkGraphCommandRunner,
} from "./lib/workgraph-actions";
import { createConsoleId } from "./lib/id";
import { findPaneResizeRoot } from "./lib/pane-resize";
import { resolveConsoleReadOnlyOverride } from "./lib/read-only-override";
import { Icon, SpriteSheet } from "./icon";
import type {
  ConsoleAccessConfig,
  ConsoleAccessRule,
  ConsoleAccessStatus,
  ConsoleActionsUiConfig,
  ConsoleAgent,
  ConsoleExperience,
  ConsoleFrame,
  ConsoleGatingActionPayload,
  ConsoleReplayUnavailablePayload,
  ConsoleTimelinePage,
  ConsoleTopologyNode,
  MemoryAuditVerdictEntry,
  MemoryDreamRun,
  MemoryDreamRunSheet,
  MemoryEvidenceRef,
  MemoryHarvestEntry,
  MemoryLedgerEntry,
  MemoryPanelAuditVerdictsResult,
  MemoryPanelDreamRunsResult,
  MemoryPanelDreamsResult,
  MemoryPanelHarvestsResult,
  MemoryPanelInjectionsResult,
  MemoryPanelOverviewResult,
  MemoryPanelProposalsResult,
  MemoryPanelQuarantineResult,
  MemoryPanelRecord,
  MemoryPanelRecordResult,
  MemoryPanelRecordsResult,
  MemoryPendingPromotion,
  MemoryProposalEntry,
  WorkGraphEventsResult,
  WorkGraphSnapshotResult,
  WorkGraphWireEvent,
} from "./types";
import { TopologyPanel } from "./panels/TopologyPanel";
import { TimelinePanel } from "./panels/TimelinePanel";
import { GatingInboxPanel } from "./panels/GatingInboxPanel";
import { AccessPanel, type AccessPreviewResult } from "./panels/AccessPanel";
import {
  MemoryPanel,
  memoryFramePivot,
  memorySectionOutcome,
  type MemoryRecordDetail,
} from "./panels/MemoryPanel";
import { RosterPanel } from "./panels/RosterPanel";
import {
  WorkGraphPanel,
  createWorkGraphRefreshSequencer,
  workGraphEventsNewestFirst,
  workGraphEventsParams,
  type WorkGraphPanelData,
} from "./panels/WorkGraphPanel";
import { RoutingPanel } from "./panels/RoutingPanel";
import { LogsPanel } from "./panels/LogsPanel";
import { Topbar } from "./panels/Topbar";
import { useConsoleVariant, type ConsoleTheme } from "./panels/Tweaks";
import {
  Sidebar as DesignSidebar,
  normalizeNavKind,
  pruneStaleSidebarStorage,
  readSidebarStringSet,
  sidebarAgentPinId,
  sidebarPinnedFamilyPinIds,
  sidebarStorageKey,
  writeSidebarStringSet,
  SIDEBAR_PINS_STORAGE_PREFIX,
  type NavKind,
} from "./panels/Sidebar";
import { SignalsRail } from "./panels/SignalsRail";
import { ChatPane, type StagedAttachment } from "./panels/ChatPane";
import { MobKitDock } from "./panels/MobKitDock";
import { PendingStack, type PendingItem } from "./panels/PendingStack";

interface ConsoleAppProps {
  baseUrl: string;
}

type RoutingPanelData = ReturnType<typeof buildRoutingSectionView>;
type GatingPanelData = { pending: unknown[]; audit: unknown[] };
type AccessPanelData = {
  status: ConsoleAccessStatus | null;
  config: ConsoleAccessConfig | null;
  error: string | null;
};
type MemoryPanelData = {
  records: MemoryPanelRecord[];
  realms: string[];
  quarantineRecords: MemoryPanelRecord[];
  pendingPromotions: MemoryPendingPromotion[];
  dreams: MemoryDreamRun[];
  detail: MemoryRecordDetail | null;
  detailLoading: boolean;
  unavailable: boolean;
  error: string | null;
  /// Keyset cursor from the base records page (single-realm only).
  nextCursor: string | null;
  /// Per-section -32030 outcomes so the panel can render "no grant"
  /// (never green) instead of an indistinguishable empty section.
  recordsDenied: boolean;
  dreamsDenied: boolean;
  /// One-row scope-probe outcomes: the scope exists behind a grant this
  /// principal lacks (the unfiltered listing row-filters them silently).
  operatorScopeDenied: boolean;
  mobScopeDenied: boolean;
  /// Phase-2 read surfaces, each with its own -32030 outcome.
  overview: MemoryPanelOverviewResult | null;
  overviewDenied: boolean;
  proposals: MemoryProposalEntry[];
  proposalsDenied: boolean;
  injections: MemoryLedgerEntry[];
  injectionsDenied: boolean;
  harvests: MemoryHarvestEntry[];
  harvestsDenied: boolean;
  dreamRuns: MemoryDreamRunSheet[];
  dreamRunsDenied: boolean;
  auditVerdicts: MemoryAuditVerdictEntry[];
  auditVerdictsDenied: boolean;
};
type DockPresetId = "single" | "two_columns" | "two_rows" | "grid";

interface IdentityLog {
  events: ConsoleFrame[];
  byKey: Map<string, number>;
  /// `null` while we haven't asked the server yet; `true` if the
  /// runtime has an EventLogStore (we'll fetch backfill); `false`
  /// once we've observed `available: false` (SSE is the only source).
  hasServerLog: boolean | null;
  oldestTimelineCursor?: string;
  latestTimelineCursor?: string;
  olderHistoryExhausted?: boolean;
  olderHistoryExhaustedAtCursor?: string;
  olderHistoryLoading?: boolean;
}

function normalizeConsoleTheme(value: unknown): ConsoleTheme | null {
  return value === "dark" || value === "light" ? value : null;
}

function normalizeConsoleVariant(
  value: unknown,
): "rams" | "terminal" | "graphite" | null {
  return value === "rams" || value === "terminal" || value === "graphite"
    ? value
    : null;
}

function normalizeDockPreset(value: unknown): DockPresetId | null {
  return value === "single" ||
    value === "two_columns" ||
    value === "two_rows" ||
    value === "grid"
    ? value
    : null;
}

function actionLabel(
  actions: ConsoleActionsUiConfig | undefined,
  key: keyof ConsoleActionsUiConfig,
  fallback: string,
): string {
  const value = actions?.[key];
  return typeof value === "string" && value.trim() ? value.trim() : fallback;
}

function actionVisible(
  actions: ConsoleActionsUiConfig | undefined,
  key: keyof ConsoleActionsUiConfig,
): boolean {
  return actions?.[key] !== false;
}

// --- Visibility helpers (unchanged) ---

function richBlockHasVisibleContent(block: unknown): boolean {
  if (!block || typeof block !== "object") return false;
  const record = block as Record<string, unknown>;
  const scalarText = [
    typeof record.text === "string" ? record.text : "",
    typeof record.label === "string" ? record.label : "",
    typeof record.result === "string" ? record.result : "",
    typeof record.body === "string" ? record.body : "",
    typeof record.title === "string" ? record.title : "",
    typeof record.name === "string" ? record.name : "",
  ]
    .join(" ")
    .trim();
  if (scalarText.length > 0) return true;
  if (
    record.type === "image" &&
    (typeof record.src === "string" || typeof record.blobId === "string")
  )
    return true;
  if (
    Array.isArray(record.headers) &&
    record.headers.some((v) => String(v || "").trim().length > 0)
  )
    return true;
  if (
    Array.isArray(record.rows) &&
    record.rows.some(
      (row) =>
        Array.isArray(row) &&
        row.some((v) => String(v || "").trim().length > 0),
    )
  )
    return true;
  return false;
}

function sanitizeConversationEntries(
  entries: ConversationTimelineEntry[],
): ConversationTimelineEntry[] {
  const sanitized: ConversationTimelineEntry[] = [];
  for (const entry of entries) {
    if (entry.kind !== "message") {
      sanitized.push(entry);
      continue;
    }
    if (entry.variant === "rich" && Array.isArray(entry.blocks)) {
      const blocks = entry.blocks.filter(richBlockHasVisibleContent);
      if (!blocks.length) continue;
      sanitized.push({ ...entry, blocks });
      continue;
    }
    if (entry.text && entry.text.trim().length > 0) sanitized.push(entry);
  }
  return sanitized;
}

function normalizeConsoleInspectResult(
  value: unknown,
): IdentityInspectViewState | null {
  const direct = normalizeIdentityInspectViewState(value);
  if (direct) return direct;
  const record =
    value && typeof value === "object"
      ? (value as Record<string, unknown>)
      : {};
  const identityRecord =
    record.identity && typeof record.identity === "object"
      ? (record.identity as Record<string, unknown>)
      : null;
  if (!identityRecord) return null;
  return normalizeIdentityInspectViewState({
    identity: identityRecord.identity,
    display_name: identityRecord.display_name,
    role:
      identityRecord.labels && typeof identityRecord.labels === "object"
        ? (identityRecord.labels as Record<string, unknown>).role
        : undefined,
    state: identityRecord.health,
    addressability:
      identityRecord.addressable === true ? "addressable" : "internal_only",
    session_id: identityRecord.session_id,
    labels: identityRecord.labels,
    continuity: {
      session_id: identityRecord.session_id,
      agent_runtime_id: identityRecord.runtime_member_id,
    },
    topology_peers: Array.isArray(record.peers) ? record.peers : [],
    lease: null,
  });
}

const DEFAULT_APPROVER_ID = "console-ops-lead";

/// Live signals that should freshen a docked WorkGraph panel: dedicated
/// workgraph.* timeline frames or workgraph_* tool completions in any turn.
function isWorkGraphSignalFrame(frame: ConsoleFrame): boolean {
  if (frame.event.startsWith("workgraph.")) return true;
  if (frame.event !== "tool_execution_completed" && frame.event !== "tool_result_received") {
    return false;
  }
  const data = frame.data && typeof frame.data === "object"
    ? (frame.data as Record<string, unknown>)
    : null;
  const name = typeof data?.name === "string"
    ? data.name
    : typeof data?.tool_name === "string"
      ? data.tool_name
      : "";
  return name.startsWith("workgraph_");
}
const DOCK_LAYOUT_STORAGE_PREFIX = "mobkit-console-dock-state";

function createIdempotencyKey(): string {
  return createConsoleId("console");
}

function dockLayoutStorageKey(
  baseUrl: string,
  experience: ConsoleExperience | null,
): string {
  const runtimeId = experience?.runtime_id?.trim();
  const title = experience?.console_config?.title?.trim();
  return `${DOCK_LAYOUT_STORAGE_PREFIX}:${runtimeId || title || baseUrl}`;
}

function stableHash(value: string): string {
  let hash = 5381;
  for (let i = 0; i < value.length; i += 1) {
    hash = ((hash << 5) + hash) ^ value.charCodeAt(i);
  }
  return (hash >>> 0).toString(36);
}

function sidebarAgentListConfigIdentity(experience: ConsoleExperience | null): string {
  const agentList = experience?.console_config?.agent_list;
  if (!agentList) return "no-agent-list";
  const sections = (agentList.sections || []).map((section) => ({
    name: section.name,
    empty_title: section.empty_title,
    empty_text: section.empty_text,
  }));
  return stableHash(JSON.stringify({
    group_by: agentList.group_by || [],
    subgroup_by: agentList.subgroup_by || [],
    section_order: agentList.section_order || [],
    fallback_group: agentList.fallback_group || "",
    fallback_subgroup: agentList.fallback_subgroup || "",
    collapse_single_subgroup: agentList.collapse_single_subgroup !== false,
    sections,
  }));
}

function sidebarPreferencesScope(
  baseUrl: string,
  experience: ConsoleExperience | null,
): string {
  const runtimeId = experience?.runtime_id?.trim();
  const title = experience?.console_config?.title?.trim();
  return runtimeId || title || baseUrl;
}

function sidebarPreferencesNamespace(
  baseUrl: string,
  experience: ConsoleExperience | null,
): string {
  return [sidebarPreferencesScope(baseUrl, experience), sidebarAgentListConfigIdentity(experience)]
    .map((part) => encodeURIComponent(part))
    .join(":");
}

function browserLocalStorage(): Storage | null {
  if (typeof window === "undefined") return null;
  try {
    return window.localStorage;
  } catch {
    return null;
  }
}

function cursorSeq(cursor: string | undefined): number | null {
  if (!cursor) return null;
  const match = /^console:(\d+)$/.exec(cursor);
  if (!match) return null;
  const parsed = Number(match[1]);
  return Number.isFinite(parsed) ? parsed : null;
}

function isTerminalTurnCompletedFrame(frame: ConsoleFrame): boolean {
  if (frame.event !== "turn_completed") return false;
  const data =
    frame.data && typeof frame.data === "object"
      ? (frame.data as Record<string, unknown>)
      : {};
  const stopReason = data.stop_reason ?? data.stopReason;
  return typeof stopReason === "string" ? stopReason !== "tool_use" : true;
}

function isActiveServerToolContentFrame(frame: ConsoleFrame): boolean {
  if (frame.event !== "server_tool_content") return false;
  const record =
    frame.data && typeof frame.data === "object"
      ? (frame.data as Record<string, unknown>)
      : null;
  const content =
    record?.content && typeof record.content === "object"
      ? (record.content as Record<string, unknown>)
      : null;
  const type =
    typeof content?.type === "string"
      ? content.type
      : typeof record?.type === "string"
        ? record.type
        : "";
  if (
    type === "message_annotations" ||
    Array.isArray(content?.annotations) ||
    type.includes(".completed") ||
    type.includes(".done") ||
    type.includes(".failed") ||
    type.includes(".error")
  ) {
    return false;
  }
  return (
    type.includes(".in_progress") ||
    type.includes(".searching") ||
    type.includes(".started") ||
    type.includes("_call")
  );
}

function isTerminalServerToolContentFrame(frame: ConsoleFrame): boolean {
  if (frame.event !== "server_tool_content") return false;
  const record =
    frame.data && typeof frame.data === "object"
      ? (frame.data as Record<string, unknown>)
      : null;
  const content =
    record?.content && typeof record.content === "object"
      ? (record.content as Record<string, unknown>)
      : null;
  const type =
    typeof content?.type === "string"
      ? content.type
      : typeof record?.type === "string"
        ? record.type
        : "";
  const status =
    typeof content?.status === "string"
      ? content.status
      : typeof record?.status === "string"
        ? record.status
        : "";
  if (type === "message_annotations" || Array.isArray(content?.annotations)) {
    return false;
  }
  return (
    type.includes(".completed") ||
    type.includes(".done") ||
    type.includes(".failed") ||
    type.includes(".error") ||
    status === "completed" ||
    status === "done" ||
    status === "succeeded" ||
    status === "failed" ||
    status === "error"
  );
}

// --- Event sets for the SSE handler ---
const REFRESH_TRIGGER_EVENTS = new Set([
  "interaction_complete",
  "interaction_failed",
  "state_changed",
  "member_ready",
  "member_retired",
  "topology_updated",
  "gating_decision",
  "route_changed",
  "tool_call_requested",
  "tool_call",
  "tool_result_received",
  "tool_execution_started",
  "tool_execution_completed",
  "server_tool_content",
]);
const PANEL_ROUTABLE_EVENTS = new Set([
  "user_input",
  "interaction_started",
  "interaction_complete",
  "interaction_failed",
  "assistant_image",
  "assistant_image_appended",
  "text_delta",
  "text_complete",
  "reasoning_delta",
  "reasoning_complete",
  "turn_completed",
  "tool_call_requested",
  "tool_call",
  "tool_result_received",
  "tool_execution_started",
  "tool_execution_completed",
  "server_tool_content",
  "run_started",
  "run_completed",
  "run_failed",
  "message_delivery_failed",
  "system_notice",
  "frame_updated",
]);
const HISTORY_REFRESH_EVENTS = new Set([
  "interaction_complete",
  "interaction_failed",
  "run_completed",
  "run_failed",
  "message_delivery_failed",
]);
// Events filtered from the activity rail — don't buffer them
const ACTIVITY_SKIP_EVENTS = new Set([
  "subscribed",
  "run_started",
  "run_completed",
  "turn_started",
  "turn_completed",
  "text_complete",
  "reasoning_delta",
  "reasoning_complete",
  "snapshot_complete",
  "snapshot_started",
  "run_failed",
  "keep-alive",
  "tool_config_changed",
  "tool_scope_changed",
  "frame_updated",
  "text_delta",
  "tool_call",
  "tool_execution_started",
  "tool_result_received",
  "tool_execution_completed",
  "server_tool_content",
]);

// ============================================================================
// CONSOLE APP
// ============================================================================

export function ConsoleApp({ baseUrl }: ConsoleAppProps): React.JSX.Element {
  const consoleFetchTimeoutMsRef = React.useRef(DEFAULT_CONSOLE_FETCH_TIMEOUT_MS);
  const consoleTransport = React.useMemo(
    () =>
      createHttpConsoleTransport({
        baseUrl,
        fetchTimeoutMs: () => consoleFetchTimeoutMsRef.current,
      }),
    [baseUrl],
  );
  const consoleController = React.useMemo(
    () => createMobKitConsoleController({ transport: consoleTransport }),
    [consoleTransport],
  );

  // --- Low-frequency React state (UI-driven) ---
  const [experience, setExperience] = React.useState<ConsoleExperience | null>(
    null,
  );
  const [agents, setAgents] = React.useState<ConsoleAgent[]>([]);
  const [draftByKey, setDraftByKey] = React.useState<Record<string, string>>(
    {},
  );
  const [stagedAttachmentsByIdentity, setStagedAttachmentsByIdentity] =
    React.useState<Record<string, StagedAttachment[]>>({});
  const [sendingPanels, setSendingPanels] = React.useState<Set<string>>(
    new Set(),
  );
  const [pinnedAgentIds, setPinnedAgentIds] = React.useState<Set<string>>(
    new Set(),
  );
  const [inspectByIdentity, setInspectByIdentity] = React.useState<
    Record<string, IdentityInspectViewState | null>
  >({});
  const [routingData, setRoutingData] = React.useState<RoutingPanelData>({
    routes: [],
    deliveries: [],
  });
  const [gatingData, setGatingData] = React.useState<GatingPanelData>({
    pending: [],
    audit: [],
  });
  const [accessData, setAccessData] = React.useState<AccessPanelData>({
    status: null,
    config: null,
    error: null,
  });
  const [memoryData, setMemoryData] = React.useState<MemoryPanelData>({
    records: [],
    realms: [],
    quarantineRecords: [],
    pendingPromotions: [],
    dreams: [],
    detail: null,
    detailLoading: false,
    unavailable: false,
    error: null,
    nextCursor: null,
    recordsDenied: false,
    dreamsDenied: false,
    operatorScopeDenied: false,
    mobScopeDenied: false,
    overview: null,
    overviewDenied: false,
    proposals: [],
    proposalsDenied: false,
    injections: [],
    injectionsDenied: false,
    harvests: [],
    harvestsDenied: false,
    dreamRuns: [],
    dreamRunsDenied: false,
    auditVerdicts: [],
    auditVerdictsDenied: false,
  });
  const [workGraphData, setWorkGraphData] = React.useState<WorkGraphPanelData>({
    items: [],
    edges: [],
    attention: [],
    events: [],
    capturedAt: null,
    unavailable: false,
    denied: false,
    error: null,
  });
  const [activeActivityPresetId, setActiveActivityPresetId] =
    React.useState("");
  const [selectedRosterMemberId, setSelectedRosterMemberId] =
    React.useState("");
  const [loading, setLoading] = React.useState(true);
  // Per-identity reactive flag for an in-flight initial session-history fetch, so
  // the chat pane can show a loading indicator instead of an empty "No messages
  // yet" while a (potentially large) history is loading. (timelineFetchInFlightRef
  // is a ref and doesn't trigger re-renders.)
  const [loadingHistory, setLoadingHistory] = React.useState<Record<string, boolean>>({});
  const [error, setError] = React.useState("");
  // Recoverable per-action failures (e.g. an access-denied send). Rendered
  // as a dismissible banner inside the shell — never the fatal error screen.
  const [actionError, setActionError] = React.useState("");
  const [theme, setTheme] = React.useState<ConsoleTheme>(() => {
    try {
      return (
        (localStorage.getItem("mobkit-console-theme") as ConsoleTheme) ||
        "light"
      );
    } catch {
      return "light";
    }
  });
  const [variant, setVariant] = useConsoleVariant();
  const sidebarStorageScope = React.useMemo(
    () => sidebarPreferencesScope(baseUrl, experience),
    [baseUrl, experience],
  );
  const sidebarStorageNamespace = React.useMemo(
    () => sidebarPreferencesNamespace(baseUrl, experience),
    [baseUrl, experience],
  );
  const sidebarPinsStorageKey = React.useMemo(
    () => sidebarStorageKey(SIDEBAR_PINS_STORAGE_PREFIX, sidebarStorageNamespace),
    [sidebarStorageNamespace],
  );
  React.useEffect(() => {
    pruneStaleSidebarStorage(browserLocalStorage(), sidebarStorageScope, sidebarStorageNamespace);
  }, [sidebarStorageScope, sidebarStorageNamespace]);

  const [sidebarCollapsed, setSidebarCollapsed] = React.useState<boolean>(
    () => {
      try {
        return localStorage.getItem("mobkit-console-sidebar-collapsed") === "1";
      } catch {
        return false;
      }
    },
  );
  const toggleSidebarCollapsed = React.useCallback(() => {
    setSidebarCollapsed((c) => {
      const next = !c;
      try {
        localStorage.setItem(
          "mobkit-console-sidebar-collapsed",
          next ? "1" : "0",
        );
      } catch {
        /* ignore */
      }
      return next;
    });
  }, []);

  const [railCollapsed, setRailCollapsed] = React.useState<boolean>(() => {
    try {
      return localStorage.getItem("mobkit-console-rail-collapsed") === "1";
    } catch {
      return false;
    }
  });
  const toggleRailCollapsed = React.useCallback(() => {
    setRailCollapsed((c) => {
      const next = !c;
      try {
        localStorage.setItem("mobkit-console-rail-collapsed", next ? "1" : "0");
      } catch {
        /* ignore */
      }
      return next;
    });
  }, []);

  const defaultPinnedAgentIdsKey = React.useMemo(
    () => JSON.stringify(experience?.console_config?.agent_list?.default_pinned_agent_ids || []),
    [experience?.console_config?.agent_list?.default_pinned_agent_ids],
  );
  React.useEffect(() => {
    const defaults = new Set(experience?.console_config?.agent_list?.default_pinned_agent_ids || []);
    const stored = readSidebarStringSet(
      browserLocalStorage(),
      sidebarPinsStorageKey,
    );
    setPinnedAgentIds(stored ?? defaults);
  }, [defaultPinnedAgentIdsKey, experience?.console_config?.agent_list, sidebarPinsStorageKey]);

  const togglePinnedAgent = React.useCallback((agent: ConsoleAgent, renderedFamilyPinIds?: Set<string>) => {
    const pinId = sidebarAgentPinId(agent);
    setPinnedAgentIds((current) => {
      const next = new Set(current);
      const familyPinIds = renderedFamilyPinIds && renderedFamilyPinIds.size > 0
        ? renderedFamilyPinIds
        : sidebarPinnedFamilyPinIds(agent, agents);
      const familyPinned = Array.from(familyPinIds).some((id) => next.has(id));
      // Pins are matched on either durable ids or volatile member_ids. When a
      // descendant pin pulls an ancestor into Pinned for context, unpinning the
      // ancestor should clear the visible pinned family instead of looking inert.
      if (next.has(pinId) || next.has(agent.member_id) || familyPinned) {
        for (const id of familyPinIds) next.delete(id);
      } else {
        next.add(pinId);
      }
      writeSidebarStringSet(
        browserLocalStorage(),
        sidebarPinsStorageKey,
        next,
      );
      return next;
    });
  }, [agents, sidebarPinsStorageKey]);

  // --- Render trigger ---
  const [, setRenderTick] = React.useState(0);
  const forceRender = React.useCallback(() => setRenderTick((n) => n + 1), []);
  const stagedAttachmentsRef = React.useRef(stagedAttachmentsByIdentity);
  React.useEffect(() => {
    stagedAttachmentsRef.current = stagedAttachmentsByIdentity;
  }, [stagedAttachmentsByIdentity]);
  React.useEffect(
    () => () => {
      for (const items of Object.values(stagedAttachmentsRef.current)) {
        items.forEach((item) => URL.revokeObjectURL(item.previewUrl));
      }
    },
    [],
  );

  function setStagedAttachmentsForIdentity(
    identity: string,
    action: React.SetStateAction<StagedAttachment[]>,
  ) {
    setStagedAttachmentsByIdentity((current) => {
      const previous = current[identity] ?? [];
      const next = typeof action === "function" ? action(previous) : action;
      const updated = { ...current };
      if (next.length > 0) updated[identity] = next;
      else delete updated[identity];
      return updated;
    });
  }

  async function inspectIdentityViaHeadless(identity: string): Promise<unknown> {
    return executeHeadlessCommand(
      CONSOLE_COMMAND_NAMES.inspectIdentity,
      identityWorkbenchTarget(identity, "inspect"),
    );
  }

  function requireWorkbenchTarget(input: unknown): ConsoleWorkbenchTarget {
    const target = migrateConsoleWorkbenchTarget(input);
    if (!target) {
      throw new Error("invalid MobKit console target");
    }
    return target;
  }

  function identityWorkbenchTarget(identity: string, mode: "chat" | "inspect"): ConsoleWorkbenchTarget {
    return requireWorkbenchTarget({
      id: mode === "inspect" ? `inspect:${identity}` : `chat:${identity}`,
      kind: mode === "inspect" ? "identity-inspect" : "agent-chat",
      title: identity,
      identity,
    });
  }

  function controlWorkbenchTarget(kind: "routing" | "gating" | "access" | "memory" | "workgraph"): ConsoleWorkbenchTarget {
    return requireWorkbenchTarget(buildControlTarget(kind));
  }

  async function executeHeadlessCommand(
    command: typeof CONSOLE_COMMAND_NAMES[keyof typeof CONSOLE_COMMAND_NAMES],
    target: ConsoleWorkbenchTarget,
    params?: Record<string, unknown>,
  ): Promise<unknown> {
    return (await consoleController.commands.execute({
      command,
      target,
      params,
    })).result;
  }

  // =========================================================================
  // DATA MODEL — single canonical event log per identity
  //
  // The previous design split state across `serverHistoryRef`,
  // `liveOverlayRef`, `serverHasEventLogRef`, and `optimisticUserRef`.
  // Two adapter passes ran independently (one per side); their outputs
  // were concatenated at render time. Same logical event arriving via
  // RPC and SSE produced different keys depending on which path
  // normalized it, so cross-store dedup was unreliable, tool-call
  // grouping fragmented across the boundary, and refetches racing the
  // SSE handler caused entries to vanish or duplicate.
  //
  // We now keep a single sorted, deduped log per identity. SSE appends
  // into it. Server fetches reconcile into it (insert-by-key, no
  // wholesale replacement, no live-overlay wipe). The renderer makes
  // exactly one adapter pass over the merged log.
  const identityLogRef = React.useRef<Record<string, IdentityLog>>({});
  const timelineFetchInFlightRef = React.useRef<Record<string, Promise<void>>>(
    {},
  );
  const optimisticUserByPanelKeyRef = React.useRef<
    Record<string, OptimisticUserMessage>
  >({});

  function getOrCreateLog(identity: string): IdentityLog {
    let log = identityLogRef.current[identity];
    if (!log) {
      log = {
        events: [],
        byKey: new Map(),
        hasServerLog: null,
        olderHistoryExhausted: false,
        olderHistoryLoading: false,
      };
      identityLogRef.current[identity] = log;
    }
    return log;
  }

  function clearOptimisticUserByInteraction(interactionId: string): boolean {
    const clearedPanelKeys: string[] = [];
    for (const [panelKey, optimistic] of Object.entries(
      optimisticUserByPanelKeyRef.current,
    )) {
      if (optimistic.interactionId !== interactionId) continue;
      optimistic.objectUrls?.forEach((url) => URL.revokeObjectURL(url));
      delete optimisticUserByPanelKeyRef.current[panelKey];
      clearedPanelKeys.push(panelKey);
    }
    if (clearedPanelKeys.length > 0) {
      setSendingPanels((current) => {
        const next = new Set(current);
        for (const panelKey of clearedPanelKeys) next.delete(panelKey);
        return next;
      });
    }
    return clearedPanelKeys.length > 0;
  }

  function clearSendingPanelsForIdentity(identity: string): void {
    if (!identity.trim()) return;
    setSendingPanels((current) => {
      let changed = false;
      const next = new Set(current);
      const suffix = `:agent-chat:${identity}`;
      for (const panelKey of current) {
        if (panelKey.endsWith(suffix)) {
          next.delete(panelKey);
          changed = true;
        }
      }
      return changed ? next : current;
    });
  }

  function clearOptimisticUserByContent(
    identity: string,
    frame: ConsoleFrame,
  ): boolean {
    if (
      frame.event !== "interaction_started" &&
      frame.event !== "user_input" &&
      frame.event !== "run_started"
    )
      return false;
    const record =
      frame.data && typeof frame.data === "object"
        ? (frame.data as Record<string, unknown>)
        : {};
    const contentValue = frame.event === "run_started"
      ? record.prompt
      : record.content;
    const content =
      typeof contentValue === "string" ? contentValue.trim() : "";
    if (!content) return false;
    const clearedPanelKeys: string[] = [];
    for (const [panelKey, optimistic] of Object.entries(
      optimisticUserByPanelKeyRef.current,
    )) {
      if (!panelKey.endsWith(`:agent-chat:${identity}`)) continue;
      if (optimistic.interactionId) continue;
      if (
        !("text" in optimistic.entry) ||
        typeof optimistic.entry.text !== "string"
      )
        continue;
      if (optimistic.entry.text.trim() !== content) continue;
      optimistic.objectUrls?.forEach((url) => URL.revokeObjectURL(url));
      delete optimisticUserByPanelKeyRef.current[panelKey];
      clearedPanelKeys.push(panelKey);
    }
    if (clearedPanelKeys.length > 0) {
      setSendingPanels((current) => {
        const next = new Set(current);
        for (const panelKey of clearedPanelKeys) next.delete(panelKey);
        return next;
      });
    }
    return clearedPanelKeys.length > 0;
  }

  function clearOptimisticUserForFrame(identity: string, frame: ConsoleFrame): void {
    if (
      (frame.event === "interaction_started" ||
        frame.event === "user_input" ||
        frame.event === "run_started") &&
      frame.interactionId &&
      clearOptimisticUserByInteraction(frame.interactionId)
    ) {
      return;
    }
    clearOptimisticUserByContent(identity, frame);
  }

  /// Stable identity for a frame across RPC and SSE pipelines. Both
  /// produce `event_id`-shaped IDs (e.g. `evt-agent-019dde54-…`) for
  /// the same logical event, so `frame.id` is the primary key. The
  /// fallback only fires for synthetic frames without an id; including
  /// `interactionId` keeps interaction-bound events from colliding.
  function frameKey(frame: ConsoleFrame): string {
    if (frame.id) return frame.id;
    if (frame.cursor) return frame.cursor;
    return `${frame.event}:${frame.identity || ""}:${frame.interactionId || ""}:${frame.timestampMs || 0}`;
  }

  /// Append one frame to the identity log, deduped by key. Appended
  /// frames are kept in insertion order; the read-side sorts by
  /// timestamp at render time. If the appended frame is an
  /// `interaction_started` whose interaction_id matches a pending
  /// optimistic user message, drop the optimistic — the server is now
  /// rendering the user turn itself.
  function appendFrame(identity: string, frame: ConsoleFrame): boolean {
    const log = getOrCreateLog(identity);
    if (
      frame.event === "frame_updated" &&
      frame.data &&
      typeof frame.data === "object"
    ) {
      const updated = (frame.data as Record<string, unknown>).frame as
        | ConsoleFrame
        | undefined;
      if (updated && updated.id) {
        const existingIndex = log.byKey.get(updated.id);
        if (existingIndex !== undefined && log.events[existingIndex]) {
          const existingVersion = log.events[existingIndex].frameVersion ?? 0;
          const updatedVersion = updated.frameVersion ?? existingVersion;
          if (updatedVersion < existingVersion) return false;
          log.events[existingIndex] = {
            ...log.events[existingIndex],
            ...updated,
          };
          clearOptimisticUserForFrame(identity, updated);
          return true;
        }
      }
      return false;
    }
    const key = frameKey(frame);
    if (log.byKey.has(key)) return false;
    log.byKey.set(key, log.events.length);
    log.events.push(frame);
    clearOptimisticUserForFrame(identity, frame);
    return true;
  }

  function busyTransitionForFrame(frame: ConsoleFrame): boolean | null {
    if (frame.event === "user_input") {
      return isTerminalUserInputStatus(frame.status) ? false : true;
    }
    if (
      frame.event === "interaction_started" ||
      frame.event === "run_started" ||
      frame.event === "reasoning_delta" ||
      frame.event === "reasoning_complete" ||
      frame.event === "tool_call_requested" ||
      frame.event === "tool_call" ||
      frame.event === "tool_execution_started" ||
      (frame.event === "server_tool_content" && isActiveServerToolContentFrame(frame)) ||
      (frame.event === "server_tool_content" && isTerminalServerToolContentFrame(frame)) ||
      frame.event === "tool_result_received" ||
      frame.event === "tool_execution_completed"
    ) {
      return true;
    }
    if (
      (frame.event === "turn_completed" && isTerminalTurnCompletedFrame(frame)) ||
      frame.event === "interaction_complete" ||
      frame.event === "interaction_failed" ||
      frame.event === "run_completed" ||
      frame.event === "run_failed" ||
      (frame.event === "system_notice" && systemNoticeClearsBusyState(frame)) ||
      frame.event === "message_delivery_failed"
    ) {
      // Queue draining follows run-level terminals so server-side
      // interaction correlation advances before the next send leaves.
      return false;
    }
    return null;
  }

  function isTerminalUserInputStatus(status?: string): boolean {
    return status === "completed" || status === "delivery_failed" || status === "failed";
  }

  function busyTransitionSortRank(frame: ConsoleFrame): number {
    const transition = busyTransitionForFrame(frame);
    // When session-history projection gives lifecycle frames the same
    // timestamp, a terminal event must win over its matching start/user
    // frame. Otherwise backfilled history can leave an idle agent marked
    // busy forever and trap future sends in the pending stack.
    return transition === false ? 1 : 0;
  }

  function applyBusyState(identity: string, nextBusy: boolean): void {
    const wasBusy = identityBusyRef.current[identity] === true;
    identityBusyRef.current[identity] = nextBusy;
    if (wasBusy && !nextBusy) {
      clearSendingPanelsForIdentity(identity);
      maybeDrainHead(identity);
    }
  }

  function updateBusyStateForFrame(
    identity: string,
    frame: ConsoleFrame,
  ): void {
    const lifecycle = identityLifecycleRef.current[identity] ?? {
      interactionOpen: false,
      runOpen: false,
    };
    let sawLifecycle = true;
    switch (frame.event) {
      case "interaction_started":
        lifecycle.interactionOpen = true;
        break;
      case "run_started":
        lifecycle.runOpen = true;
        break;
      case "run_completed":
      case "run_failed":
        lifecycle.runOpen = false;
        break;
      case "interaction_complete":
      case "interaction_failed":
      case "message_delivery_failed":
        lifecycle.interactionOpen = false;
        lifecycle.runOpen = false;
        break;
      case "system_notice":
        if (systemNoticeClearsBusyState(frame)) {
          lifecycle.interactionOpen = false;
          lifecycle.runOpen = false;
        } else {
          sawLifecycle = false;
        }
        break;
      default:
        sawLifecycle = false;
        break;
    }
    identityLifecycleRef.current[identity] = lifecycle;
    if (sawLifecycle) {
      applyBusyState(identity, lifecycle.interactionOpen || lifecycle.runOpen);
      return;
    }

    const transition = busyTransitionForFrame(frame);
    if (transition !== null) {
      applyBusyState(
        identity,
        transition || lifecycle.interactionOpen || lifecycle.runOpen,
      );
    }
  }

  function recomputeBusyStateFromLog(identity: string): void {
    const log = getOrCreateLog(identity);
    const lifecycleFrames = log.events
      .filter((frame) => busyTransitionForFrame(frame) !== null)
      .sort((a, b) => {
        const timeDelta = (a.timestampMs || 0) - (b.timestampMs || 0);
        if (timeDelta !== 0) return timeDelta;
        const rankDelta = busyTransitionSortRank(a) - busyTransitionSortRank(b);
        if (rankDelta !== 0) return rankDelta;
        return (a.cursor || a.id || "").localeCompare(b.cursor || b.id || "");
      });
    const lifecycle = { interactionOpen: false, runOpen: false };
    let legacyBusy = false;
    for (const frame of lifecycleFrames) {
      switch (frame.event) {
        case "interaction_started":
          lifecycle.interactionOpen = true;
          break;
        case "run_started":
          lifecycle.runOpen = true;
          break;
        case "run_completed":
        case "run_failed":
          lifecycle.runOpen = false;
          break;
        case "interaction_complete":
        case "interaction_failed":
        case "message_delivery_failed":
          lifecycle.interactionOpen = false;
          lifecycle.runOpen = false;
          legacyBusy = false;
          break;
        case "system_notice":
          if (systemNoticeClearsBusyState(frame)) {
            lifecycle.interactionOpen = false;
            lifecycle.runOpen = false;
            legacyBusy = false;
          }
          break;
        default: {
          const transition = busyTransitionForFrame(frame);
          if (transition !== null) legacyBusy = transition;
          break;
        }
      }
    }
    identityLifecycleRef.current[identity] = lifecycle;
    applyBusyState(
      identity,
      lifecycle.interactionOpen || lifecycle.runOpen || legacyBusy,
    );
  }

  /// Reconcile a server-history fetch into the identity log. Frames
  /// already present (by key) are skipped; new frames are appended.
  /// The live overlay is preserved — both sides feed the same log now.
  ///
  /// `available: false` means the runtime has no `EventLogStore`
  /// configured — the response is the in-memory recent buffer rather
  /// than authoritative replay. We still ingest those frames (they're
  /// the only backfill we'll ever get for this identity) and just
  /// remember that a refetch wouldn't get anything more, so SSE is the
  /// going-forward source of truth.
  function reconcileServerLog(
    identity: string,
    frames: ConsoleFrame[],
    available: boolean,
  ): boolean {
    const log = getOrCreateLog(identity);
    log.hasServerLog = available;
    let changed = false;
    for (const frame of frames) {
      if (!appendFrame(identity, frame)) continue;
      changed = true;
      if (updatePhaseForIdentity(identity, frame)) changed = true;
    }
    recomputeBusyStateFromLog(identity);
    if (recomputePhaseForIdentity(identity)) changed = true;
    return changed;
  }

  function newerCursor(a: string | undefined, b: string | undefined): string | undefined {
    const aSeq = cursorSeq(a);
    const bSeq = cursorSeq(b);
    if (aSeq === null) return b || a;
    if (bSeq === null) return a || b;
    return bSeq > aSeq ? b : a;
  }

  function olderCursor(a: string | undefined, b: string | undefined): string | undefined {
    const aSeq = cursorSeq(a);
    const bSeq = cursorSeq(b);
    if (aSeq === null) return b || a;
    if (bSeq === null) return a || b;
    return bSeq < aSeq ? b : a;
  }

  function noteIdentityTimelinePage(
    identity: string,
    page: ConsoleTimelinePage,
    target: { mode: "recent" | "since"; before?: string },
  ): boolean {
    const log = getOrCreateLog(identity);
    const previousOldest = log.oldestTimelineCursor;
    const previousLatest = log.latestTimelineCursor;
    const previousExhausted = log.olderHistoryExhausted;
    const previousExhaustedAtCursor = log.olderHistoryExhaustedAtCursor;
    for (const frame of page.frames) {
      log.oldestTimelineCursor = olderCursor(log.oldestTimelineCursor, frame.cursor);
      log.latestTimelineCursor = newerCursor(log.latestTimelineCursor, frame.cursor);
    }
    if (target.mode === "recent") {
      log.latestTimelineCursor = newerCursor(log.latestTimelineCursor, page.latestCursor);
      if (target.before) {
        log.olderHistoryExhausted = page.exhausted === true;
        log.olderHistoryExhaustedAtCursor =
          page.exhausted === true ? log.oldestTimelineCursor : undefined;
      } else if (!log.olderHistoryExhaustedAtCursor) {
        log.olderHistoryExhausted = page.exhausted === true;
      }
    } else {
      log.latestTimelineCursor = newerCursor(
        log.latestTimelineCursor,
        page.nextCursor || page.latestCursor,
      );
    }
    return (
      previousOldest !== log.oldestTimelineCursor ||
      previousLatest !== log.latestTimelineCursor ||
      previousExhausted !== log.olderHistoryExhausted ||
      previousExhaustedAtCursor !== log.olderHistoryExhaustedAtCursor
    );
  }

  function resetIdentityTimelineReplayMetadata(identity: string): boolean {
    const log = getOrCreateLog(identity);
    const changed =
      log.events.length > 0 ||
      log.byKey.size > 0 ||
      log.oldestTimelineCursor !== undefined ||
      log.latestTimelineCursor !== undefined ||
      log.olderHistoryExhausted !== false ||
      log.olderHistoryExhaustedAtCursor !== undefined;
    log.events = [];
    log.byKey.clear();
    log.oldestTimelineCursor = undefined;
    log.latestTimelineCursor = undefined;
    log.olderHistoryExhausted = false;
    log.olderHistoryExhaustedAtCursor = undefined;
    return changed;
  }

  async function queryIdentityTimelinePage(
    identity: string,
    target: { mode: "recent" | "since"; after?: string; before?: string; limit?: number },
  ): Promise<{ page: ConsoleTimelinePage; metadataChanged: boolean }> {
    const pageFact = await consoleController.timeline.query(
      {
        identity,
        mode: target.mode,
        after: target.after,
        before: target.before,
        limit: target.limit ?? 200,
      },
    );
    const page = pageFact.value;
    const metadataChanged = noteIdentityTimelinePage(identity, page, target);
    return { page, metadataChanged };
  }

  function refreshIdentityTimelineNow(
    identity: string,
    options: { clearPhase?: boolean } = {},
  ): Promise<void> {
    const normalized = identity.trim();
    if (!normalized) return Promise.resolve();
    const inFlight = timelineFetchInFlightRef.current[normalized];
    if (inFlight) {
      return inFlight.then(() => {
        if (options.clearPhase) {
          clearPhaseForIdentity(normalized);
          forceRender();
        }
      });
    }

    setLoadingHistory((current) =>
      current[normalized] ? current : { ...current, [normalized]: true },
    );
    const request = (async () => {
      const { page } = await queryIdentityTimelinePage(normalized, {
        mode: "recent",
        limit: 200,
      });
      reconcileServerLog(normalized, page.frames, page.available);
      if (options.clearPhase) clearPhaseForIdentity(normalized);
      forceRender();
    })().finally(() => {
      delete timelineFetchInFlightRef.current[normalized];
      setLoadingHistory((current) => {
        if (!current[normalized]) return current;
        const next = { ...current };
        delete next[normalized];
        return next;
      });
    });
    timelineFetchInFlightRef.current[normalized] = request;
    return request;
  }

  async function loadOlderIdentityTimeline(identity: string): Promise<void> {
    const normalized = identity.trim();
    if (!normalized) return;
    const log = getOrCreateLog(normalized);
    if (log.olderHistoryLoading || log.olderHistoryExhausted) return;
    log.olderHistoryLoading = true;
    forceRender();
    try {
      const { page } = await queryIdentityTimelinePage(normalized, {
        mode: "recent",
        before: log.oldestTimelineCursor,
        limit: 200,
      });
      reconcileServerLog(normalized, page.frames, page.available);
    } catch {
      // The current view remains usable; a later scroll can retry.
    } finally {
      log.olderHistoryLoading = false;
      forceRender();
    }
  }

  /// Render-time chat view: transcript time is the primary order. Aggregate
  /// cursor order is useful for replay paging, but it can arrive out of
  /// conversational order when delayed peer-message/session-history frames are
  /// backfilled after newer tool or completion frames. Use cursor only as a
  /// stable tie-breaker for same-timestamp frames.
  function getSortedFrames(identity: string): ConsoleFrame[] {
    const log = identityLogRef.current[identity];
    if (!log) return [];
    return log.events
      .map((frame, index) => ({ frame, index }))
      .sort((a, b) => {
        const ta =
          typeof a.frame.timestampMs === "number"
            ? a.frame.timestampMs
            : Number.MAX_SAFE_INTEGER;
        const tb =
          typeof b.frame.timestampMs === "number"
            ? b.frame.timestampMs
            : Number.MAX_SAFE_INTEGER;
        if (ta !== tb) return ta - tb;
        const ca = cursorSeq(a.frame.cursor);
        const cb = cursorSeq(b.frame.cursor);
        if (ca !== null && cb !== null && ca !== cb) return ca - cb;
        return a.index - b.index;
      })
      .map((entry) => entry.frame);
  }

  function framesVisibleInPanel(
    frames: ConsoleFrame[],
    panelId: string,
  ): ConsoleFrame[] {
    void panelId;
    // Panel ids are ephemeral UI instance ids. Persisted user_input frames
    // keep the original `console:<panel-id>` origin, so filtering by the
    // current panel id hides the operator's historical prompts after a
    // refresh or reopen. Identity-scoped logs are already routed before
    // this point, so every frame in the identity log belongs in the pane.
    return frames;
  }

  // Activity rail (global, unchanged)
  const activityRef = React.useRef<ConsoleFrame[]>([]);
  // Unfiltered recent-frames ring for topology-class panels that need to
  // see tool calls (peer-comms send_*, etc.) in addition to interaction
  // lifecycle. The activity rail filters tool events out; this buffer
  // doesn't.
  const liveFramesRef = React.useRef<ConsoleFrame[]>([]);
  const [liveFrames, setLiveFrames] = React.useState<ConsoleFrame[]>([]);
  function commitLiveFrames(frames: ConsoleFrame[]): void {
    liveFramesRef.current = frames;
    setLiveFrames(frames);
  }

  // ──────────────────────────────────────────────────────────────
  // Pending message stack (per-identity, persisted, cross-tab synced)
  //
  // Codex-style queue: when the user sends while an agent is busy,
  // the message lands here instead of going straight to the wire.
  // The stack drains FIFO on busy→idle. Each item supports Steer
  // (cut the line, sends immediately with HandlingMode::Steer),
  // Trash (client-side delete, never sent), Edit (in-place text
  // mutation), and Reorder (drag-and-drop or keyboard).
  //
  // Persistence: localStorage under `mobkit-pending-stack:<identity>`.
  // Cross-tab sync: a `storage` event listener mirrors changes from
  // other tabs into the in-memory ref. Items in transient animation
  // states are stripped or leased before persisting so reloads do not
  // resurrect an in-flight animation without the timer that owned it.
  // ──────────────────────────────────────────────────────────────
  const pendingStackRef = React.useRef<Record<string, PendingItem[]>>({});
  const PENDING_STACK_KEY_PREFIX = "mobkit-pending-stack:";
  const PENDING_DRAIN_CLAIM_TTL_MS = 15_000;
  const stackKeyFor = (identity: string) =>
    `${PENDING_STACK_KEY_PREFIX}${identity}`;

  function loadPendingStack(
    identity: string,
    opts: { preserveFreshDraining?: boolean } = {},
  ): PendingItem[] {
    try {
      const raw = localStorage.getItem(stackKeyFor(identity));
      if (!raw) return [];
      const parsed = JSON.parse(raw) as unknown;
      if (!Array.isArray(parsed)) return [];
      const now = Date.now();
      return parsed
        .filter((it): it is PendingItem => {
          if (!it || typeof it !== "object") return false;
          const r = it as Record<string, unknown>;
          return (
            typeof r.id === "string" &&
            typeof r.text === "string" &&
            typeof r.addedAt === "number"
          );
        })
        .map((it) => {
          const r = it as Record<string, unknown>;
          const drainClaimedAt =
            typeof r.drainClaimedAt === "number"
              ? r.drainClaimedAt
              : undefined;
          const freshDrainClaim =
            opts.preserveFreshDraining === true &&
            r.status === "draining" &&
            typeof r.drainClaim === "string" &&
            typeof drainClaimedAt === "number" &&
            now - drainClaimedAt < PENDING_DRAIN_CLAIM_TTL_MS;
          return {
            id: it.id,
            text: it.text,
            addedAt: it.addedAt,
            status: freshDrainClaim ? ("draining" as const) : null,
            drainClaim: freshDrainClaim ? r.drainClaim : undefined,
            drainClaimedAt: freshDrainClaim ? drainClaimedAt : undefined,
          };
        });
    } catch {
      return [];
    }
  }

  function persistPendingStack(identity: string, items: PendingItem[]) {
    try {
      // Strip purely visual transient flags before persisting. Keep fresh
      // draining claims so multiple open tabs do not all auto-drain the same
      // queued item when they observe the same busy→idle transition.
      const clean = items
        .filter(
          (it) =>
            it.status !== "trashing" &&
            it.status !== "promoting",
        )
        .map((it) => ({
          id: it.id,
          text: it.text,
          addedAt: it.addedAt,
          ...(it.status === "draining"
            ? {
                status: "draining",
                drainClaim: it.drainClaim,
                drainClaimedAt: it.drainClaimedAt,
              }
            : {}),
        }));
      if (clean.length === 0) {
        localStorage.removeItem(stackKeyFor(identity));
      } else {
        localStorage.setItem(stackKeyFor(identity), JSON.stringify(clean));
      }
    } catch {
      /* quota / private mode — silently degrade */
    }
  }

  function getPendingStack(identity: string): PendingItem[] {
    if (!pendingStackRef.current[identity]) {
      pendingStackRef.current[identity] = loadPendingStack(identity);
    }
    return pendingStackRef.current[identity];
  }

  function setPendingStack(
    identity: string,
    update: (prev: PendingItem[]) => PendingItem[],
  ) {
    const prev = getPendingStack(identity);
    const next = update(prev);
    pendingStackRef.current[identity] = next;
    persistPendingStack(identity, next);
    forceRender();
  }

  // Cross-tab sync: a write to `mobkit-pending-stack:<identity>` in
  // another tab fires a `storage` event here. Reload the affected
  // stack into the ref and re-render. Same-tab writes don't fire
  // `storage` so this is one-way only — the sender does its own update.
  React.useEffect(() => {
    const onStorage = (e: StorageEvent) => {
      if (!e.key || !e.key.startsWith(PENDING_STACK_KEY_PREFIX)) return;
      const identity = e.key.slice(PENDING_STACK_KEY_PREFIX.length);
      pendingStackRef.current[identity] = loadPendingStack(identity, {
        preserveFreshDraining: true,
      });
      forceRender();
    };
    window.addEventListener("storage", onStorage);
    return () => window.removeEventListener("storage", onStorage);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Per-identity busy state — driven by interaction lifecycle events on
  // the SSE stream. Used both for the stack's "agent busy" indicator
  // and to decide whether a fresh Send should bypass the stack
  // (idle + empty stack) or push to it (anything else).
  const identityBusyRef = React.useRef<Record<string, boolean>>({});
  const identityLifecycleRef = React.useRef<
    Record<string, { interactionOpen: boolean; runOpen: boolean }>
  >({});
  const isIdentityBusy = (identity: string) =>
    identityBusyRef.current[identity] === true;

  // Phase tracking (per-panel, unchanged)
  const phaseRef = React.useRef<
    Record<string, "waiting" | "tool-executing" | "generating" | null>
  >({});
  const phaseValueByKey = React.useRef<
    Record<string, "waiting" | "tool-executing" | "generating" | null>
  >({});
  const phaseSinceByKey = React.useRef<Record<string, number>>({});
  const phaseTimerByKey = React.useRef<Record<string, number>>({});

  // Per-identity refresh debounce timers
  const refreshTimersRef = React.useRef<Record<string, number>>({});

  // Experience refresh debounce
  const experienceTimerRef = React.useRef<number | null>(null);
  const experienceLoadInFlightRef = React.useRef<Promise<ConsoleAgent[]> | null>(
    null,
  );
  // Stable agent ref for async callbacks
  const agentsRef = React.useRef<ConsoleAgent[]>([]);
  React.useEffect(() => {
    agentsRef.current = agents;
  }, [agents]);

  const initialTargetOpened = React.useRef(false);
  const dockLayoutHydrated = React.useRef(false);
  const dockLayoutRestored = React.useRef(false);
  const dockLayoutRestoring = React.useRef(false);

  // =========================================================================
  // DOCK CONTROLLER
  // =========================================================================

  const dock = useConsoleDockController<MobKitDockTarget>({
    createPanelState: ({ target }) => ({
      id: createConsoleId("panel"),
      target: target || null,
      mode: "console" as const,
    }),
  });
  const currentDockLayoutStorageKey = React.useMemo(
    () => dockLayoutStorageKey(baseUrl, experience),
    [baseUrl, experience?.runtime_id, experience?.console_config?.title],
  );

  React.useEffect(() => {
    if (!experience || dockLayoutHydrated.current) return;
    dockLayoutHydrated.current = true;
    try {
      const raw = localStorage.getItem(currentDockLayoutStorageKey);
      if (!raw) return;
      const parsed = JSON.parse(raw) as ConsoleDockState<MobKitDockTarget>;
      const restored = normalizeConsoleDockState(parsed);
      if (restored.tabs.length === 0 || restored.panels.length === 0) return;
      dockLayoutRestored.current = true;
      dockLayoutRestoring.current = true;
      dock.setState(restored);
    } catch {
      /* ignore corrupt saved layout */
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [currentDockLayoutStorageKey, experience]);

  React.useEffect(() => {
    if (!experience || !dockLayoutHydrated.current) return;
    if (dockLayoutRestoring.current) {
      dockLayoutRestoring.current = false;
      return;
    }
    try {
      localStorage.setItem(
        currentDockLayoutStorageKey,
        JSON.stringify(dock.state),
      );
    } catch {
      /* ignore storage failures */
    }
  }, [currentDockLayoutStorageKey, dock.state, experience]);

  // =========================================================================
  // PHASE TRACKING (unchanged logic)
  // =========================================================================

  function clearPhaseTimer(panelKey: string) {
    const timer = phaseTimerByKey.current[panelKey];
    if (timer !== undefined) {
      window.clearTimeout(timer);
      delete phaseTimerByKey.current[panelKey];
    }
  }

  function commitPanelPhase(
    panelKey: string,
    phase: "waiting" | "tool-executing" | "generating" | null,
  ): boolean {
    const previous = phaseValueByKey.current[panelKey] ?? null;
    clearPhaseTimer(panelKey);
    phaseValueByKey.current[panelKey] = phase;
    phaseSinceByKey.current[panelKey] = Date.now();
    phaseRef.current[panelKey] = phase;
    return previous !== phase;
  }

  function schedulePanelPhase(
    panelKey: string,
    phase: "waiting" | "tool-executing" | "generating" | null,
    delayMs: number,
  ) {
    clearPhaseTimer(panelKey);
    phaseTimerByKey.current[panelKey] = window.setTimeout(() => {
      delete phaseTimerByKey.current[panelKey];
      phaseValueByKey.current[panelKey] = phase;
      phaseSinceByKey.current[panelKey] = Date.now();
      phaseRef.current[panelKey] = phase;
      forceRender();
    }, delayMs);
  }

  function updatePanelPhaseFromFrame(
    panelKey: string,
    frame: ConsoleFrame,
    lifecycleBusy = false,
  ): boolean {
    const currentPhase = phaseValueByKey.current[panelKey] ?? null;
    const elapsedMs = Date.now() - (phaseSinceByKey.current[panelKey] ?? 0);
    switch (frame.event) {
      case "user_input":
        if (isTerminalUserInputStatus(frame.status)) return commitPanelPhase(panelKey, null);
        return commitPanelPhase(panelKey, "waiting");
      case "interaction_started":
        return commitPanelPhase(panelKey, "waiting");
      case "tool_call_requested":
      case "tool_call":
      case "tool_execution_started":
      case "server_tool_content":
        if (frame.event === "server_tool_content") {
          if (isTerminalServerToolContentFrame(frame)) {
            return commitPanelPhase(panelKey, "waiting");
          }
          if (!isActiveServerToolContentFrame(frame)) {
            return false;
          }
        }
        if (currentPhase === "waiting" && elapsedMs < 300) {
          schedulePanelPhase(panelKey, "tool-executing", 300 - elapsedMs);
          return true;
        }
        return commitPanelPhase(panelKey, "tool-executing");
      case "tool_result_received":
      case "tool_execution_completed":
        // A completed tool means this specific operation is done, but the
        // agent turn is still active until a terminal text/run frame arrives.
        // Keep the pane visibly busy so mid-turn sends queue instead of
        // slipping into the runtime as a live boundary input.
        return commitPanelPhase(panelKey, "waiting");
      case "reasoning_delta":
        return commitPanelPhase(panelKey, "generating");
      case "reasoning_complete":
        return commitPanelPhase(panelKey, "waiting");
      case "text_delta": {
        if (currentPhase === "tool-executing") {
          const r = Math.max(0, 300 - elapsedMs);
          if (r > 0) {
            schedulePanelPhase(panelKey, "generating", r);
            return true;
          }
        }
        if (currentPhase === "waiting" && elapsedMs < 300) {
          schedulePanelPhase(panelKey, "generating", 300 - elapsedMs);
          return true;
        }
        return commitPanelPhase(panelKey, "generating");
      }
      case "text_complete":
        return commitPanelPhase(panelKey, lifecycleBusy ? "waiting" : null);
      case "interaction_complete":
      case "interaction_failed":
        return commitPanelPhase(panelKey, null);
      case "run_completed":
      case "run_failed":
        return commitPanelPhase(panelKey, lifecycleBusy ? "waiting" : null);
      case "system_notice":
        if (systemNoticeClearsBusyState(frame)) return commitPanelPhase(panelKey, null);
        return false;
      case "turn_completed":
        if (isTerminalTurnCompletedFrame(frame)) {
          return commitPanelPhase(panelKey, lifecycleBusy ? "waiting" : null);
        }
        return false;
      case "message_delivery_failed":
        return commitPanelPhase(panelKey, null);
      default:
        return false;
    }
  }

  // The SSE handler runs from inside the stream effect, so its
  // closure captures `dock` from the first render — when panels[] was empty.
  // Route panel-iterating phase updates through a ref so they always see the
  // current panel set; otherwise interaction_started/text_delta/
  // interaction_complete arrive at panel:none and the typing indicator
  // sticks at "waiting" indefinitely (and the "still busy" perception breaks
  // the pending-stack auto-queue, which depends on `identityBusyRef`).
  const dockRef = React.useRef(dock);
  dockRef.current = dock;

  // Helper: update phase for ALL panels showing a given identity
  function updatePhaseForIdentity(identity: string, frame: ConsoleFrame): boolean {
    let changed = false;
    const lifecycleBusy = isIdentityBusy(identity);
    for (const panel of dockRef.current.viewState.panels) {
      const target = panel.target as MobKitDockTarget | null;
      if (!target || target.kind !== "agent-chat") continue;
      if ((target.identity || target.memberId) !== identity) continue;
      if (updatePanelPhaseFromFrame(
        buildPanelConversationKey(panel.id, target),
        frame,
        lifecycleBusy,
      )) changed = true;
    }
    return changed;
  }

  // Helper: clear phase for all panels showing a given identity
  function clearPhaseForIdentity(identity: string): boolean {
    let changed = false;
    for (const panel of dockRef.current.viewState.panels) {
      const target = panel.target as MobKitDockTarget | null;
      if (!target || target.kind !== "agent-chat") continue;
      if ((target.identity || target.memberId) !== identity) continue;
      if (commitPanelPhase(buildPanelConversationKey(panel.id, target), null)) {
        changed = true;
      }
    }
    return changed;
  }

  function commitPhaseForIdentity(
    identity: string,
    phase: "waiting" | "tool-executing" | "generating" | null,
  ): boolean {
    let changed = false;
    for (const panel of dockRef.current.viewState.panels) {
      const target = panel.target as MobKitDockTarget | null;
      if (!target || target.kind !== "agent-chat") continue;
      if ((target.identity || target.memberId) !== identity) continue;
      if (commitPanelPhase(buildPanelConversationKey(panel.id, target), phase)) {
        changed = true;
      }
    }
    return changed;
  }

  function recomputePhaseForIdentity(identity: string): boolean {
    const frames = getSortedFrames(identity).filter((frame) =>
      PANEL_ROUTABLE_EVENTS.has(frame.event)
    );
    const phase = inferResponsePhaseFromFrames(frames, null);
    let changed = false;
    for (const panel of dockRef.current.viewState.panels) {
      const target = panel.target as MobKitDockTarget | null;
      if (!target || target.kind !== "agent-chat") continue;
      if ((target.identity || target.memberId) !== identity) continue;
      if (commitPanelPhase(buildPanelConversationKey(panel.id, target), phase)) {
        changed = true;
      }
    }
    return changed;
  }

  // =========================================================================
  // LOAD EXPERIENCE
  // =========================================================================

  const loadExperience = React.useCallback(() => {
    if (experienceLoadInFlightRef.current) {
      return experienceLoadInFlightRef.current;
    }

    let request: Promise<ConsoleAgent[]>;
    request = (async () => {
      const [experienceJson, modulesJson] = await Promise.all([
        consoleTransport.loadExperience(),
        consoleTransport.loadModules?.() ?? Promise.resolve({ modules: [] }),
      ]);
      const configuredTimeoutMs = experienceJson.console_policy?.fetch_timeout_ms;
      if (
        typeof configuredTimeoutMs === "number" &&
        Number.isFinite(configuredTimeoutMs) &&
        configuredTimeoutMs > 0
      ) {
        consoleFetchTimeoutMsRef.current = configuredTimeoutMs;
      }
      const loadedModules = Array.isArray(modulesJson.modules)
        ? modulesJson.modules.map(String)
        : [];
      const nextAgents = normalizeAgents(experienceJson, loadedModules);
      setExperience(experienceJson);
      setAgents(nextAgents);
      setActiveActivityPresetId(
        (c) =>
          c ||
          experienceJson.console_config?.rail?.active_preset_id ||
          experienceJson.activity_feed?.active_preset_id ||
          "all",
      );
      return nextAgents;
    })().finally(() => {
      if (experienceLoadInFlightRef.current === request) {
        experienceLoadInFlightRef.current = null;
      }
    });

    experienceLoadInFlightRef.current = request;
    return request;
  }, [consoleTransport]);

  React.useEffect(() => {
    let mounted = true;
    setLoading(true);
    setError("");
    void loadExperience()
      .catch((e) => {
        if (mounted) setError(errorMessage(e));
      })
      .finally(() => {
        if (mounted) setLoading(false);
      });
    return () => {
      mounted = false;
    };
  }, [loadExperience]);

  React.useEffect(() => {
    const timer = window.setInterval(() => {
      void loadExperience().catch(() => {});
    }, 15_000);
    return () => window.clearInterval(timer);
  }, [loadExperience]);

  React.useEffect(() => {
    const appearance = experience?.console_config?.appearance;
    if (!appearance) return;
    const configuredTheme = normalizeConsoleTheme(appearance.default_theme);
    if (configuredTheme) {
      try {
        if (!localStorage.getItem("mobkit-console-theme"))
          setTheme(configuredTheme);
      } catch {
        setTheme(configuredTheme);
      }
    }
    const configuredVariant = normalizeConsoleVariant(
      appearance.default_variant,
    );
    if (configuredVariant) {
      try {
        if (!localStorage.getItem("mobkit-console-variant"))
          setVariant(configuredVariant);
      } catch {
        setVariant(configuredVariant);
      }
    }
  }, [experience?.console_config?.appearance, setVariant]);

  React.useEffect(() => {
    const configured = experience?.console_config?.layout?.sidebar_collapsed;
    if (typeof configured !== "boolean") return;
    try {
      if (localStorage.getItem("mobkit-console-sidebar-collapsed") !== null)
        return;
    } catch {
      /* ignore */
    }
    setSidebarCollapsed(configured);
  }, [experience?.console_config?.layout?.sidebar_collapsed]);

  React.useEffect(() => {
    const configured = experience?.console_config?.rail?.collapsed;
    if (typeof configured !== "boolean") return;
    try {
      if (localStorage.getItem("mobkit-console-rail-collapsed") !== null)
        return;
    } catch {
      /* ignore */
    }
    setRailCollapsed(configured);
  }, [experience?.console_config?.rail?.collapsed]);

  const hasMobControlSurface = experience?.runtime_id !== "console-aggregator";
  const frontendReadOnly = React.useMemo(() => resolveConsoleReadOnlyOverride(), []);
  // When access control is enforcing, `can_send_messages` is intersected
  // with the caller's per-agent send grants, so it can be false simply
  // because they currently have no send-able agent. That must NOT flip the
  // whole console to deployment read-only (which would also suppress retire/
  // respawn affordances they may still hold) — per-agent affordances are the
  // authoritative gate. Only the deployment policy and the frontend override
  // make the console globally read-only under access control.
  const accessEnforcing = experience?.access?.enabled === true;
  const consoleReadOnly =
    frontendReadOnly ||
    experience?.console_policy?.read_only === true ||
    (!accessEnforcing &&
      experience?.runtime_capabilities?.can_send_messages === false);
  const consoleReadOnlyRef = React.useRef(false);
  consoleReadOnlyRef.current = consoleReadOnly;
  const visibleControls = React.useMemo<NavKind[]>(() => {
    const runtimeControls: NavKind[] = hasMobControlSurface
      ? [
          "topology",
          "timeline",
          "gating",
          "roster",
          "routing",
          "logs",
          "health",
        ]
      : ["topology", "timeline", "roster", "logs", "health"];
    const sidebarConfig = experience?.console_config?.sidebar;
    const allowedByRuntime = new Set(runtimeControls);
    const configuredVisible = (sidebarConfig?.visible_controls || [])
      .map(normalizeNavKind)
      .filter(
        (kind): kind is NavKind => Boolean(kind) && allowedByRuntime.has(kind),
      );
    if (configuredVisible.length > 0) {
      const extra: NavKind[] = [];
      if (experience?.access?.can_administer === true) extra.push("access");
      // The Memory panel is gated server-side per principal, never by view
      // config: `can_read` alone decides whether the nav entry appears.
      if (experience?.memory?.can_read === true) extra.push("memory");
      // WorkGraph is likewise server-gated: configured service + view grant.
      if (experience?.workgraph?.available === true && experience?.workgraph?.can_view === true) {
        extra.push("workgraph");
      }
      return extra.length > 0 ? [...configuredVisible, ...extra] : configuredVisible;
    }
    const hidden = new Set(
      (sidebarConfig?.hidden_controls || [])
        .map(normalizeNavKind)
        .filter((kind): kind is NavKind => Boolean(kind)),
    );
    const controls = runtimeControls.filter((kind) => !hidden.has(kind));
    // The Access admin surface is gated server-side per principal, never by
    // view config: administrators always get it, nobody else ever does.
    if (experience?.access?.can_administer === true) controls.push("access");
    // Same for the Memory panel: server-side `can_read` gates the entry.
    if (experience?.memory?.can_read === true) controls.push("memory");
    // Same for WorkGraph: configured service + per-caller view grant.
    if (experience?.workgraph?.available === true && experience?.workgraph?.can_view === true) {
      controls.push("workgraph");
    }
    return controls;
  }, [
    experience?.console_config?.sidebar,
    experience?.access?.can_administer,
    experience?.memory?.can_read,
    experience?.workgraph?.available,
    experience?.workgraph?.can_view,
    hasMobControlSurface,
  ]);

  // =========================================================================
  // OPEN INITIAL TARGET
  // =========================================================================

  React.useEffect(() => {
    if (initialTargetOpened.current || dock.focusedTarget || !experience)
      return;
    if (!dockLayoutHydrated.current) return;
    if (dockLayoutRestored.current) {
      initialTargetOpened.current = true;
      return;
    }
    const layoutConfig = experience.console_config?.layout;
    let target: MobKitDockTarget | null = null;
    const configuredControl = normalizeNavKind(layoutConfig?.initial_control);
    if (configuredControl && visibleControls.includes(configuredControl)) {
      target = buildControlTarget(
        configuredControl as Parameters<typeof buildControlTarget>[0],
      );
    }
    const configuredAgent = layoutConfig?.initial_agent?.trim().toLowerCase();
    if (!target && configuredAgent) {
      const match = agents.find((agent) => {
        return [
          agent.identity,
          agent.member_id,
          agent.agent_id,
          agent.label,
        ].some((value) => value?.toLowerCase() === configuredAgent);
      });
      if (match) target = buildDockTarget(match);
    }
    initialTargetOpened.current = true;
    if (!target) return;
    const preset = normalizeDockPreset(layoutConfig?.initial_preset);
    if (preset) dock.applyPreset(preset);
    dock.openTarget(target, "replace_focused");
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [agents, dock, experience, visibleControls]);

  React.useEffect(() => {
    const target = dock.focusedTarget;
    if (!target || target.kind !== "agent-chat" || agents.length === 0) return;
    const identity = target.identity || target.memberId;
    if (
      agents.some(
        (agent) => agent.identity === identity || agent.member_id === identity,
      )
    )
      return;
    const fallback =
      agents.find(
        (agent) => agent.addressable || agent.affordances?.can_send_message,
      ) || agents[0];
    if (fallback) {
      openAgentChat(fallback, "replace_focused");
    } else {
      dock.openTarget(buildControlTarget("roster"), "replace_focused");
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [agents, dock.focusedTarget]);

  // =========================================================================
  // REFRESH PANEL DATA (inspect, routing, gating)
  // =========================================================================

  const refreshAccessData = React.useCallback(async () => {
    const accessTarget = controlWorkbenchTarget("access");
    try {
      const status =
        ((await executeHeadlessCommand(
          CONSOLE_COMMAND_NAMES.accessStatus,
          accessTarget,
        )) as ConsoleAccessStatus | null) || null;
      let config: ConsoleAccessConfig | null = null;
      if (status?.available && status?.can_administer) {
        const result = (await executeHeadlessCommand(
          CONSOLE_COMMAND_NAMES.getAccessConfig,
          accessTarget,
        )) as { config?: ConsoleAccessConfig } | null;
        config = result?.config || null;
      }
      setAccessData({ status, config, error: null });
    } catch (err) {
      setAccessData((current) => ({ ...current, error: errorMessage(err) }));
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [baseUrl]);

  const refreshMemoryData = React.useCallback(async () => {
    const memoryTarget = controlWorkbenchTarget("memory");
    try {
      let records: MemoryPanelRecord[] = [];
      let realms: string[] = [];
      let nextCursor: string | null = null;
      let recordsDenied = false;
      try {
        const recordsResult = (await executeHeadlessCommand(
          CONSOLE_COMMAND_NAMES.listMemoryRecords,
          memoryTarget,
        )) as MemoryPanelRecordsResult | null;
        records = recordsResult?.records || [];
        realms = recordsResult?.realms || [];
        nextCursor = recordsResult?.next_cursor ?? null;
      } catch (err) {
        // Access denied to records leaves the section empty but flagged, so
        // the panel renders "no grant" instead of an empty store.
        if (memorySectionOutcome(err) !== "denied") throw err;
        recordsDenied = true;
      }

      // The unfiltered listing row-filters restricted scopes silently, so a
      // one-row probe per restricted scope kind is the only way to render
      // the spec'd access-denied Holdings row instead of the scope silently
      // not existing. Skipped when the listing itself was denied.
      let operatorScopeDenied = false;
      let mobScopeDenied = false;
      if (!recordsDenied) {
        const probeScope = async (scope: "operator" | "mob"): Promise<boolean> => {
          try {
            await executeHeadlessCommand(CONSOLE_COMMAND_NAMES.listMemoryRecords, memoryTarget, {
              scope,
              limit: 1,
            });
            return false;
          } catch (err) {
            if (memorySectionOutcome(err) === "denied") return true;
            throw err;
          }
        };
        [operatorScopeDenied, mobScopeDenied] = await Promise.all([
          probeScope("operator"),
          probeScope("mob"),
        ]);
      }

      let quarantineRecords: MemoryPanelRecord[] = [];
      let pendingPromotions: MemoryPendingPromotion[] = [];
      if (experience?.memory?.can_review_quarantine === true) {
        try {
          const quarantineResult = (await executeHeadlessCommand(
            CONSOLE_COMMAND_NAMES.listMemoryQuarantine,
            memoryTarget,
          )) as MemoryPanelQuarantineResult | null;
          quarantineRecords = quarantineResult?.records || [];
          pendingPromotions = quarantineResult?.pending_promotions || [];
        } catch (err) {
          // Access denied to the quarantine queue leaves it empty rather than
          // failing the whole panel.
          if (jsonRpcErrorCode(err) !== -32030) throw err;
        }
      }

      let dreams: MemoryDreamRun[] = [];
      let dreamsDenied = false;
      try {
        const dreamsResult = (await executeHeadlessCommand(
          CONSOLE_COMMAND_NAMES.listMemoryDreams,
          memoryTarget,
        )) as MemoryPanelDreamsResult | null;
        dreams = dreamsResult?.runs || [];
      } catch (err) {
        // Tolerate access-denied for dreams: leave the list empty but flagged.
        if (memorySectionOutcome(err) !== "denied") throw err;
        dreamsDenied = true;
      }

      // Phase-2 read surfaces, fetched in parallel with the same per-section
      // -32030 tolerance: a denied surface renders "no grant", never an
      // indistinguishable empty section, and never aborts the panel load.
      const section = async <T,>(
        command: (typeof CONSOLE_COMMAND_NAMES)[keyof typeof CONSOLE_COMMAND_NAMES],
        empty: T,
        pick: (result: unknown) => T,
      ): Promise<{ value: T; denied: boolean }> => {
        try {
          const result = await executeHeadlessCommand(command, memoryTarget);
          return { value: pick(result), denied: false };
        } catch (err) {
          if (memorySectionOutcome(err) !== "denied") throw err;
          return { value: empty, denied: true };
        }
      };
      const [overview, proposals, injections, harvests, dreamRuns, auditVerdicts] =
        await Promise.all([
          section<MemoryPanelOverviewResult | null>(
            CONSOLE_COMMAND_NAMES.getMemoryOverview,
            null,
            (result) => (result as MemoryPanelOverviewResult | null) ?? null,
          ),
          section<MemoryProposalEntry[]>(
            CONSOLE_COMMAND_NAMES.listMemoryProposals,
            [],
            (result) => (result as MemoryPanelProposalsResult | null)?.proposals || [],
          ),
          section<MemoryLedgerEntry[]>(
            CONSOLE_COMMAND_NAMES.listMemoryInjections,
            [],
            (result) => (result as MemoryPanelInjectionsResult | null)?.injections || [],
          ),
          section<MemoryHarvestEntry[]>(
            CONSOLE_COMMAND_NAMES.listMemoryHarvests,
            [],
            (result) => (result as MemoryPanelHarvestsResult | null)?.harvests || [],
          ),
          section<MemoryDreamRunSheet[]>(
            CONSOLE_COMMAND_NAMES.listMemoryDreamRuns,
            [],
            (result) => (result as MemoryPanelDreamRunsResult | null)?.runs || [],
          ),
          section<MemoryAuditVerdictEntry[]>(
            CONSOLE_COMMAND_NAMES.listMemoryAuditVerdicts,
            [],
            (result) => (result as MemoryPanelAuditVerdictsResult | null)?.verdicts || [],
          ),
        ]);

      setMemoryData((current) => ({
        ...current,
        records,
        realms,
        quarantineRecords,
        pendingPromotions,
        dreams,
        nextCursor,
        recordsDenied,
        dreamsDenied,
        operatorScopeDenied,
        mobScopeDenied,
        overview: overview.value,
        overviewDenied: overview.denied,
        proposals: proposals.value,
        proposalsDenied: proposals.denied,
        injections: injections.value,
        injectionsDenied: injections.denied,
        harvests: harvests.value,
        harvestsDenied: harvests.denied,
        dreamRuns: dreamRuns.value,
        dreamRunsDenied: dreamRuns.denied,
        auditVerdicts: auditVerdicts.value,
        auditVerdictsDenied: auditVerdicts.denied,
        unavailable: false,
        error: null,
      }));
    } catch (err) {
      // -32601 means the panel is not configured on this runtime.
      if (jsonRpcErrorCode(err) === -32601) {
        setMemoryData((current) => ({ ...current, unavailable: true, error: null }));
        return;
      }
      setMemoryData((current) => ({ ...current, error: errorMessage(err) }));
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [baseUrl, experience?.memory?.can_review_quarantine]);

  // Overlapping refreshes (debounced live signals, manual refresh, post-
  // mutation re-reads) can resolve out of order — sequence them so a stale
  // snapshot never overwrites a fresher one.
  const workGraphRefreshSequencerRef = React.useRef(createWorkGraphRefreshSequencer());
  const refreshWorkGraphData = React.useCallback(async () => {
    const workGraphTarget = controlWorkbenchTarget("workgraph");
    const isCurrent = workGraphRefreshSequencerRef.current.begin();
    try {
      let snapshot: WorkGraphSnapshotResult | null = null;
      let denied = false;
      try {
        snapshot = (await executeHeadlessCommand(
          CONSOLE_COMMAND_NAMES.workgraphSnapshot,
          workGraphTarget,
        )) as WorkGraphSnapshotResult | null;
      } catch (err) {
        // Access denied renders as "no grant", never an empty graph.
        if (jsonRpcErrorCode(err) !== -32030) throw err;
        denied = true;
      }
      let events: WorkGraphWireEvent[] = [];
      if (!denied) {
        try {
          // Page the tail from the snapshot's high-water mark: upstream
          // returns events ASCENDING truncated to limit, so a bare {limit}
          // query would freeze on the oldest window forever.
          const eventsResult = (await executeHeadlessCommand(
            CONSOLE_COMMAND_NAMES.workgraphEvents,
            workGraphTarget,
            workGraphEventsParams(snapshot?.event_high_water_mark, 50),
          )) as WorkGraphEventsResult | null;
          events = workGraphEventsNewestFirst(eventsResult?.events || []);
        } catch (err) {
          // The events tail is optional; a denied ledger leaves it empty.
          if (jsonRpcErrorCode(err) !== -32030) throw err;
        }
      }
      if (!isCurrent()) return;
      setWorkGraphData({
        items: snapshot?.items || [],
        edges: snapshot?.edges || [],
        attention: snapshot?.attention || [],
        events,
        capturedAt: snapshot?.captured_at || null,
        unavailable: false,
        denied,
        error: null,
      });
    } catch (err) {
      if (!isCurrent()) return;
      // -32601 (method absent) and -32041 (workgraph_unavailable) both mean
      // no WorkGraph service on this runtime; so does a missing capability
      // advertisement (the headless layer throws before dispatch).
      const code = jsonRpcErrorCode(err);
      const capabilityMissing =
        err instanceof Error && err.message.startsWith("MobKit capability missing");
      if (code === -32601 || code === -32041 || capabilityMissing) {
        setWorkGraphData((current) => ({ ...current, unavailable: true, error: null }));
        return;
      }
      setWorkGraphData((current) => ({ ...current, error: errorMessage(err) }));
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [baseUrl]);

  /// Filtered/paged panel/records query for the Records filter bar, the
  /// keyset load-more, and the lattice page-walk. Resolves null STRICTLY on
  /// -32030 (denied — consumers render "no grant", never an empty store);
  /// other failures surface via the panel error banner and rethrow so the
  /// pager keeps the prior page instead of clobbering it.
  const queryMemoryRecords = React.useCallback(
    async (params: Record<string, unknown>): Promise<MemoryPanelRecordsResult | null> => {
      try {
        return (await executeHeadlessCommand(
          CONSOLE_COMMAND_NAMES.listMemoryRecords,
          controlWorkbenchTarget("memory"),
          params,
        )) as MemoryPanelRecordsResult | null;
      } catch (err) {
        if (memorySectionOutcome(err) === "denied") return null;
        setMemoryData((current) => ({ ...current, error: errorMessage(err) }));
        throw err;
      }
    },
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [baseUrl],
  );

  /// Evidence click-through: resolve a MemoryEvidenceRef against the
  /// contracted query_timeline surface. Resolves null when the session is no
  /// longer in the timeline — the Biography degrades to the evidenceLabel text.
  const loadMemoryEvidence = React.useCallback(
    async (
      identity: string | undefined,
      evidence: MemoryEvidenceRef,
    ): Promise<ConversationTimelineEntry[] | null> => {
      if (!evidence.session_id) return null;
      try {
        const pageFact = await consoleController.timeline.query({
          ...(identity ? { identity } : {}),
          mode: "recent",
          limit: 1000,
        });
        const page = pageFact.value;
        if (!page.available) return null;
        const frames = page.frames.filter(
          (frame) => frame.sessionId === evidence.session_id,
        );
        if (frames.length === 0) return null;
        return mapFramesToTimelineEntries(null, frames, {
          renderInteractionStartsAsUser: true,
        });
      } catch {
        return null;
      }
    },
    [consoleController],
  );

  const loadMemoryRecordDetail = React.useCallback(
    async (realm: string | undefined, memoryId: string) => {
      setMemoryData((current) => ({ ...current, detail: null, detailLoading: true, error: null }));
      try {
        const result = (await executeHeadlessCommand(
          CONSOLE_COMMAND_NAMES.getMemoryRecord,
          controlWorkbenchTarget("memory"),
          realm ? { realm, memory_id: memoryId } : { memory_id: memoryId },
        )) as MemoryPanelRecordResult | null;
        const detail: MemoryRecordDetail | null = result?.record
          ? {
              realm: result.realm,
              record: result.record,
              chain: result.chain || [],
              injections: result.injections || [],
            }
          : null;
        setMemoryData((current) => ({ ...current, detail, detailLoading: false }));
      } catch (err) {
        setMemoryData((current) => ({
          ...current,
          detail: null,
          detailLoading: false,
          error: errorMessage(err),
        }));
      }
    },
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [baseUrl],
  );

  const runAccessMutation = React.useCallback(
    async (
      command:
        | typeof CONSOLE_COMMAND_NAMES.setAccessConfig
        | typeof CONSOLE_COMMAND_NAMES.enableAccess
        | typeof CONSOLE_COMMAND_NAMES.upsertAccessRule
        | typeof CONSOLE_COMMAND_NAMES.deleteAccessRule
        | typeof CONSOLE_COMMAND_NAMES.setAccessGroup
        | typeof CONSOLE_COMMAND_NAMES.deleteAccessGroup,
      params: Record<string, unknown>,
    ) => {
      try {
        await executeHeadlessCommand(command, controlWorkbenchTarget("access"), params);
        setAccessData((current) => ({ ...current, error: null }));
      } catch (err) {
        setAccessData((current) => ({ ...current, error: errorMessage(err) }));
      }
      await refreshAccessData();
      // Enforcement may have changed what this caller can see.
      await loadExperience().catch(() => {});
    },
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [baseUrl, refreshAccessData, loadExperience],
  );

  const refreshPanelData = React.useCallback(async () => {
    const openPanels = dock.viewState.panels
      .map((p) => p.target)
      .filter(Boolean) as MobKitDockTarget[];
    const inspects = openPanels.filter(
      (t): t is Extract<MobKitDockTarget, { kind: "identity-inspect" }> =>
        t.kind === "identity-inspect",
    );
    if (inspects.length) {
      const entries = await Promise.all(
        inspects.map(async (t) => {
          const r = await inspectIdentityViaHeadless(t.identity);
          return [t.identity, normalizeConsoleInspectResult(r)] as const;
        }),
      );
      setInspectByIdentity((c) => ({ ...c, ...Object.fromEntries(entries) }));
    }
    if (hasMobControlSurface && openPanels.some((t) => t.kind === "routing")) {
      const routingTarget = controlWorkbenchTarget("routing");
      const [routes, history] = await Promise.all([
        executeHeadlessCommand(CONSOLE_COMMAND_NAMES.listRoutingRoutes, routingTarget),
        executeHeadlessCommand(CONSOLE_COMMAND_NAMES.listDeliveryHistory, routingTarget),
      ]);
      setRoutingData(
        buildRoutingSectionView({
          routesResponse: routes,
          historyResponse: history,
        }),
      );
    }
    if (openPanels.some((t) => t.kind === "access")) {
      await refreshAccessData();
    }
    if (openPanels.some((t) => t.kind === "memory")) {
      await refreshMemoryData();
    }
    if (openPanels.some((t) => t.kind === "workgraph")) {
      await refreshWorkGraphData();
    }
    if (
      hasMobControlSurface &&
      openPanels.some((t) => t.kind === "gating" || t.kind === "gates")
    ) {
      const gatingTarget = controlWorkbenchTarget("gating");
      const [p, a] = await Promise.all([
        executeHeadlessCommand(CONSOLE_COMMAND_NAMES.listGatingPending, gatingTarget),
        executeHeadlessCommand(CONSOLE_COMMAND_NAMES.listGatingAudit, gatingTarget, { limit: 50 }),
      ]);
      const pending = p && typeof p === "object" ? p as { pending?: unknown[] } : {};
      const audit = a && typeof a === "object" ? a as { entries?: unknown[] } : {};
      setGatingData({
        pending: Array.isArray(pending.pending) ? pending.pending : [],
        audit: Array.isArray(audit.entries) ? audit.entries : [],
      });
    }
  }, [baseUrl, dock.viewState.panels, hasMobControlSurface, refreshAccessData, refreshMemoryData, refreshWorkGraphData]);

  React.useEffect(() => {
    void refreshPanelData().catch(() => {});
  }, [dock.viewState.panels, refreshPanelData]);

  const scheduleExperienceRefresh = React.useCallback(() => {
    if (experienceTimerRef.current !== null) return;
    experienceTimerRef.current = window.setTimeout(async () => {
      experienceTimerRef.current = null;
      await loadExperience().catch(() => {});
      await refreshPanelData().catch(() => {});
    }, 150);
  }, [loadExperience, refreshPanelData]);

  // =========================================================================
  // HISTORY REFRESH — server is the single source of truth
  // =========================================================================

  const scheduleHistoryRefresh = React.useCallback(
    (identity: string) => {
      clearTimeout(refreshTimersRef.current[identity]);
      refreshTimersRef.current[identity] = window.setTimeout(async () => {
        const log = getOrCreateLog(identity);
        // No event log on this runtime — SSE is the canonical source,
        // there's nothing to backfill.
        if (log.hasServerLog === false) {
          clearPhaseForIdentity(identity);
          forceRender();
          return;
        }
        try {
          await refreshIdentityTimelineNow(identity, { clearPhase: true });
        } catch {
          /* silent — will retry on next terminal event */
        }
      }, 200);
      // eslint-disable-next-line react-hooks/exhaustive-deps
    },
    [baseUrl, forceRender],
  );

  // =========================================================================
  // PANEL OPEN / SWITCH — fetch history for new identities
  // =========================================================================

  // Operator workgraph mutations live only as client-local echo frames, so a
  // page reload silently reverts the inline cards to agent-observed state
  // until an agent next touches the graph. Once per identity per mount, after
  // the initial history backfill, a pane whose restored transcript folds a
  // workgraph card fetches ONE snapshot and folds it through the refresh-echo
  // path (`refresh: true` — a read, never an action outcome) so the cards
  // re-hydrate to live status/revisions. The done-set is marked before the
  // fetch so concurrent effect runs never double-fetch; identities whose
  // transcript shows no cards are marked done without fetching.
  const workGraphHydrationDoneRef = React.useRef(new Set<string>());
  async function hydrateWorkGraphCardsForIdentity(identity: string): Promise<void> {
    if (workGraphHydrationDoneRef.current.has(identity)) return;
    // Experience may still be loading: skip WITHOUT marking done so the
    // availability flip re-triggers the check.
    if (experience?.workgraph?.available !== true) return;
    if (!framesContainWorkGraphCards(getSortedFrames(identity))) {
      workGraphHydrationDoneRef.current.add(identity);
      return;
    }
    workGraphHydrationDoneRef.current.add(identity);
    try {
      const snapshot = await executeHeadlessCommand(
        CONSOLE_COMMAND_NAMES.workgraphSnapshot,
        controlWorkbenchTarget("workgraph"),
      );
      appendFrame(
        identity,
        buildWorkGraphOperatorResultFrame({
          method: consoleCommandMethod(CONSOLE_COMMAND_NAMES.workgraphSnapshot),
          params: {},
          // The RPC returns the WorkGraphSnapshot verbatim; the fold expects
          // the tool-result wrapper shape.
          result: { snapshot },
          identity,
          refresh: true,
        }),
      );
      forceRender();
    } catch {
      // Best-effort hydration: the card keeps its restored state and the
      // next operator action heals revisions through the conflict path.
    }
  }

  React.useEffect(() => {
    for (const panel of dock.viewState.panels) {
      const target = panel.target as MobKitDockTarget | null;
      if (!target || target.kind !== "agent-chat") continue;
      const identity = target.identity || target.memberId;
      const log = getOrCreateLog(identity);
      // Only fetch backfill once per identity — when we don't yet
      // know whether the runtime has an event log. Subsequent panel
      // re-opens reuse the existing log; we never wipe it.
      if (log.hasServerLog !== null) {
        void hydrateWorkGraphCardsForIdentity(identity);
        continue;
      }
      void refreshIdentityTimelineNow(identity)
        .then(() => hydrateWorkGraphCardsForIdentity(identity))
        .catch(() => {});
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [baseUrl, dock.viewState.panels, forceRender, experience?.workgraph?.available]);

  React.useEffect(() => {
    const refreshOpenChatPanels = async () => {
      const identities = new Set<string>();
      for (const panel of dock.viewState.panels) {
        const target = panel.target as MobKitDockTarget | null;
        if (!target || target.kind !== "agent-chat") continue;
        identities.add(target.identity || target.memberId);
      }
      if (identities.size === 0) return;
      let changed = false;
      for (const identity of identities) {
        const log = getOrCreateLog(identity);
        if (log.hasServerLog === false) continue;
        try {
          const sinceCursor =
            log.latestTimelineCursor &&
            !(log.olderHistoryExhausted === true && !log.olderHistoryExhaustedAtCursor)
              ? log.latestTimelineCursor
              : undefined;
          const { page, metadataChanged } = await queryIdentityTimelinePage(identity, {
            mode: sinceCursor ? "since" : "recent",
            after: sinceCursor,
            limit: sinceCursor ? 1000 : 200,
          });
          if (reconcileServerLog(identity, page.frames, page.available) || metadataChanged) {
            changed = true;
          }
        } catch (error) {
          const replay = error as Error & {
            replayError?: ConsoleReplayUnavailablePayload;
            timelineReplayUnavailable?: boolean;
          };
          if (replay.timelineReplayUnavailable || replay.replayError?.stream === "timeline") {
            if (resetIdentityTimelineReplayMetadata(identity)) {
              changed = true;
            }
            try {
              const { page, metadataChanged } = await queryIdentityTimelinePage(identity, {
                mode: "recent",
                limit: 200,
              });
              if (reconcileServerLog(identity, page.frames, page.available) || metadataChanged) {
                changed = true;
              }
            } catch {
              // Keep the panel usable; the next refresh will retry.
            }
            continue;
          }
          // Keep the panel usable; the next refresh will retry.
        }
      }
      if (changed) forceRender();
    };

    const timer = window.setInterval(() => {
      void refreshOpenChatPanels();
    }, 2_000);
    void refreshOpenChatPanels();
    return () => window.clearInterval(timer);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [baseUrl, dock.viewState.panels, forceRender]);

  // =========================================================================
  // GLOBAL SSE EVENT STREAM — the core event loop
  // =========================================================================

  // Stable refs for callbacks used in SSE handler — prevents effect re-runs
  const scheduleHistoryRefreshRef = React.useRef(scheduleHistoryRefresh);
  scheduleHistoryRefreshRef.current = scheduleHistoryRefresh;
  const scheduleExperienceRefreshRef = React.useRef(scheduleExperienceRefresh);
  scheduleExperienceRefreshRef.current = scheduleExperienceRefresh;
  // Memory panels anchor on durable SQLite state; live memory.* frames are
  // freshness signals only. Debounced so dream-commit bursts coalesce into
  // one re-read, and gated on a memory panel actually being docked.
  const refreshMemoryDataRef = React.useRef(refreshMemoryData);
  refreshMemoryDataRef.current = refreshMemoryData;
  const memoryPanelDockedRef = React.useRef(false);
  memoryPanelDockedRef.current = dock.viewState.panels.some(
    (panel) => (panel.target as MobKitDockTarget | null)?.kind === "memory",
  );
  const memoryRefreshTimerRef = React.useRef<number | null>(null);
  // WorkGraph panel mirrors the memory freshness pattern: live workgraph
  // signals (workgraph.* frames or workgraph_* tool completions) trigger one
  // debounced snapshot re-read, only while a WorkGraph panel is docked.
  const refreshWorkGraphDataRef = React.useRef(refreshWorkGraphData);
  refreshWorkGraphDataRef.current = refreshWorkGraphData;
  const workGraphPanelDockedRef = React.useRef(false);
  workGraphPanelDockedRef.current = dock.viewState.panels.some(
    (panel) => (panel.target as MobKitDockTarget | null)?.kind === "workgraph",
  );
  const workGraphRefreshTimerRef = React.useRef<number | null>(null);

  React.useEffect(() => {
    // Seed activity with recent history (only on mount) — apply same
    // filter as SSE. The activity rail is its own concern; it doesn't
    // share state with the per-identity logs.
    const handleLiveFrame = (incomingFrame: ConsoleFrame) => {
      const canonicalIdentity = canonicalConsoleIdentity(
        incomingFrame.identity,
        agentsRef.current,
      );
      const frame =
        canonicalIdentity && canonicalIdentity !== incomingFrame.identity
          ? { ...incomingFrame, identity: canonicalIdentity }
          : incomingFrame;
      // Activity rail (independent buffer)
      if (!ACTIVITY_SKIP_EVENTS.has(frame.event)) {
        activityRef.current = [frame, ...activityRef.current].slice(0, 200);
      }

      // Topology-class buffer keeps tool events (peer-comms etc.) which
      // the activity rail filters out. Capped at 300; older frames roll
      // off naturally as live pulses age past their lifetime.
      if (PANEL_ROUTABLE_EVENTS.has(frame.event)) {
        commitLiveFrames([frame, ...liveFramesRef.current].slice(0, 300));
      }

      // Identity log (single canonical store)
      const identity = canonicalIdentity || frame.identity?.trim();
      if (
        PANEL_ROUTABLE_EVENTS.has(frame.event) &&
        identity &&
        identity !== "_system"
      ) {
        appendFrame(identity, frame);
        updateBusyStateForFrame(identity, frame);
        updatePhaseForIdentity(identity, frame);
      }

      forceRender();

      // Terminal events → reconcile server backfill (idempotent — keys
      // already seen via SSE are skipped). If hasServerLog is false,
      // scheduleHistoryRefresh short-circuits.
      if (
        (HISTORY_REFRESH_EVENTS.has(frame.event) || isTerminalTurnCompletedFrame(frame)) &&
        identity &&
        identity !== "_system"
      ) {
        scheduleHistoryRefreshRef.current(identity);
      }
      if (REFRESH_TRIGGER_EVENTS.has(frame.event)) {
        scheduleExperienceRefreshRef.current();
      }
      if (
        frame.event.startsWith("memory.") &&
        memoryPanelDockedRef.current &&
        memoryRefreshTimerRef.current === null
      ) {
        memoryRefreshTimerRef.current = window.setTimeout(() => {
          memoryRefreshTimerRef.current = null;
          void refreshMemoryDataRef.current().catch(() => {});
        }, 250);
      }
      if (
        isWorkGraphSignalFrame(frame) &&
        workGraphPanelDockedRef.current &&
        workGraphRefreshTimerRef.current === null
      ) {
        workGraphRefreshTimerRef.current = window.setTimeout(() => {
          workGraphRefreshTimerRef.current = null;
          void refreshWorkGraphDataRef.current().catch(() => {});
        }, 250);
      }
    };

    let stopped = false;
    let unsubscribe: (() => void) | null = null;

    void consoleController.timeline.subscribeWithBackfill({ limit: 200 }, (frame) => {
      if (!stopped) handleLiveFrame(frame.value);
    })
      .then((nextUnsubscribe) => {
        if (stopped) {
          nextUnsubscribe();
        } else {
          unsubscribe = nextUnsubscribe;
        }
      })
      .catch(() => {
        if (!stopped) unsubscribe = consoleTransport.subscribeTimeline({}, handleLiveFrame);
      });

    return () => {
      stopped = true;
      unsubscribe?.();
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [consoleController, consoleTransport]);

  // Timer cleanup on unmount
  React.useEffect(() => {
    return () => {
      for (const timer of Object.values(phaseTimerByKey.current))
        window.clearTimeout(timer);
      for (const timer of Object.values(refreshTimersRef.current))
        window.clearTimeout(timer);
      if (experienceTimerRef.current !== null)
        window.clearTimeout(experienceTimerRef.current);
      if (memoryRefreshTimerRef.current !== null)
        window.clearTimeout(memoryRefreshTimerRef.current);
      if (workGraphRefreshTimerRef.current !== null)
        window.clearTimeout(workGraphRefreshTimerRef.current);
    };
  }, []);

  // =========================================================================
  // AGENT SELECTION
  // =========================================================================

  function openAgentChat(
    agent: ConsoleAgent,
    intent:
      | "replace_focused"
      | "new_tab"
      | "split_right"
      | "split_down" = "replace_focused",
  ) {
    const target = buildDockTarget(agent);
    void refreshIdentityTimelineNow(target.identity || target.memberId).catch(
      () => {},
    );
    dock.openTarget(target, intent);
  }

  function openDockTarget(
    target: MobKitDockTarget,
    intent:
      | "replace_focused"
      | "new_tab"
      | "split_right"
      | "split_down" = "replace_focused",
  ) {
    if (target.kind === "agent-chat") {
      void refreshIdentityTimelineNow(target.identity || target.memberId).catch(
        () => {},
      );
    }
    dock.openTarget(target, intent);
  }

  function onSelectAgent(
    _block: unknown,
    _section: unknown,
    item: { id: string },
  ) {
    const agent = agents.find((c) => c.member_id === item.id);
    if (agent) openAgentChat(agent);
  }

  // =========================================================================
  // SEND MESSAGE — optimistic + interaction_id reconciliation
  // =========================================================================

  /// Inner "actually fire the message at the wire" step. Used both by
  /// `onSendMessage` (idle-bypass path), by the stack auto-drain hook,
  /// and by the Steer button (which passes `handlingMode = "steer"`).
  /// Marks the identity as busy optimistically so a Steer click that
  /// races with the wire response doesn't accidentally bypass the
  /// stack on the very next Send.
  async function submitMessageNow(
    panelId: string,
    target: MobKitDockTarget,
    text: string,
    handlingMode: "queue" | "steer",
    attachments: File[] = [],
  ): Promise<boolean> {
    if (target.kind !== "agent-chat") return false;
    if (consoleReadOnlyRef.current) return false;
    const panelKey = buildPanelConversationKey(panelId, target);
    const identity = target.identity || target.memberId;

    const optimisticObjectUrls = attachments.map((file) =>
      URL.createObjectURL(file),
    );
    const userEntry = createUserEntry(
      text,
      attachments.map((file, index) => ({
        src: optimisticObjectUrls[index] || "",
        mediaType: file.type || "application/octet-stream",
        alt: file.name,
      })),
    );
    setSendingPanels((c) => new Set(c).add(panelKey));
    const log = getOrCreateLog(identity);
    optimisticUserByPanelKeyRef.current[panelKey] = {
      interactionId: "",
      entry: userEntry,
      sentAtMs: Date.now(),
      objectUrls: optimisticObjectUrls,
    };
    // Use commitPanelPhase (not bare phaseRef assignment) so the
    // value/since bookkeeping is consistent with what
    // `updatePanelPhaseFromFrame` reads — otherwise the next text_delta's
    // "elapsedMs since waiting" check is computed against `since=0`,
    // which we want anyway, but `currentPhase` would read undefined.
    commitPhaseForIdentity(identity, "waiting");
    identityBusyRef.current[identity] = true;
    commitLiveFrames([{
      id: `optimistic-topology:${identity}:${Date.now()}`,
      event: "interaction_started",
      identity,
      interactionId: "",
      timestampMs: Date.now(),
      data: {
        origin: `console:${panelId}`,
        handling_mode: handlingMode,
      },
    }, ...liveFramesRef.current].slice(0, 300));
    forceRender();

    try {
      const workbenchTarget = migrateConsoleWorkbenchTarget(target);
      if (!workbenchTarget) {
        throw new Error("console send requires an identity-addressed target");
      }
      const result = (await consoleController.commands.sendMessage(
        workbenchTarget,
        {
          content: text,
          origin: `console:${panelId}`,
          idempotencyKey: createIdempotencyKey(),
          handlingMode,
          attachments,
        },
      )).accepted.value;
      const optimisticUser = optimisticUserByPanelKeyRef.current[panelKey];
      if (optimisticUser) {
        optimisticUser.interactionId = result.interaction_id;
        // The interaction_started frame may have arrived between
        // the send and the RPC response — reconcile retroactively.
        const matched = log.events.some(
          (f) =>
            (f.event === "interaction_started" ||
              f.event === "user_input" ||
              f.event === "run_started") &&
            f.interactionId === result.interaction_id,
        );
        if (matched) {
          optimisticUser.objectUrls?.forEach((url) =>
            URL.revokeObjectURL(url),
          );
          delete optimisticUserByPanelKeyRef.current[panelKey];
        }
      }
      setActionError("");
      return true;
    } catch (submitError) {
      optimisticUserByPanelKeyRef.current[panelKey]?.objectUrls?.forEach(
        (url) => URL.revokeObjectURL(url),
      );
      delete optimisticUserByPanelKeyRef.current[panelKey];
      commitPanelPhase(panelKey, null);
      identityBusyRef.current[identity] = false;
      setActionError(errorMessage(submitError));
      forceRender();
      return false;
    } finally {
      setSendingPanels((c) => {
        const n = new Set(c);
        n.delete(panelKey);
        return n;
      });
    }
  }

  async function onSendMessage(
    panelId: string,
    target: MobKitDockTarget | null,
    attachments: File[] = [],
  ): Promise<boolean> {
    if (!target || target.kind !== "agent-chat") return false;
    if (consoleReadOnly) return false;
    const panelKey = buildPanelConversationKey(panelId, target);
    const identity = target.identity || target.memberId;
    const rawDraft = draftByKey[panelKey] || "";
    const text = rawDraft.trim();
    if (!text && attachments.length === 0) return false;

    const stack = getPendingStack(identity);
    const visiblePhase =
      phaseValueByKey.current[panelKey] ?? phaseRef.current[panelKey] ?? null;
    const agentPhase =
      agentsRef.current.find((candidate) =>
        [candidate.identity, candidate.member_id, candidate.agent_id].includes(
          identity,
        ),
      )?.response_phase ?? null;
    const shouldQueue =
      isIdentityBusy(identity) ||
      visiblePhase !== null ||
      agentPhase !== null ||
      stack.length > 0;

    const clearSubmittedDraft = () => {
      setDraftByKey((current) => {
        if ((current[panelKey] || "") !== rawDraft) return current;
        return { ...current, [panelKey]: "" };
      });
    };
    const restoreSubmittedDraftIfEmpty = () => {
      setDraftByKey((current) => {
        if ((current[panelKey] || "") !== "") return current;
        return { ...current, [panelKey]: rawDraft };
      });
    };

    if (!shouldQueue || attachments.length > 0) {
      // Idle + empty stack: bypass straight to the wire.
      // Clear the text before awaiting the RPC so a busy runtime cannot
      // freeze the visible composer with the just-submitted draft still in it.
      if (attachments.length === 0) {
        clearSubmittedDraft();
      }
      const sent = await submitMessageNow(
        panelId,
        target,
        text,
        "queue",
        attachments,
      );
      if (sent) {
        clearSubmittedDraft();
      } else if (attachments.length === 0) {
        restoreSubmittedDraftIfEmpty();
      }
      return sent;
    }

    // Push onto the stack instead. The animation flag clears itself
    // shortly after so subsequent reorders/edits don't see an
    // is-entering ghost.
    const newId = `pmsg-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 7)}`;
    setPendingStack(identity, (prev) => [
      ...prev,
      { id: newId, text, addedAt: Date.now(), status: "entering" },
    ]);
    clearSubmittedDraft();
    window.setTimeout(() => {
      setPendingStack(identity, (prev) =>
        prev.map((it) =>
          it.id === newId && it.status === "entering"
            ? { ...it, status: null }
            : it,
        ),
      );
    }, 240);
    return true;
  }

  // ── Pending-stack action handlers ────────────────────────────────
  //
  // Each handler that ends with "send to wire" (Steer, auto-drain)
  // first marks the item with the corresponding animation flag, then
  // — after the animation duration — removes the item and calls
  // `submitMessageNow`. The animation timing matches `pending-stack.css`
  // (steer 360ms, drain 420ms, trash 320ms). `reduced-motion` collapses
  // these to 0 so the item leaves the DOM immediately.
  const reducedMotion =
    typeof window !== "undefined"
      ? (window.matchMedia?.("(prefers-reduced-motion: reduce)").matches ??
        false)
      : false;
  const animMs = (ms: number) => (reducedMotion ? 0 : ms);
  const pendingDrainOwnerRef = React.useRef(
    `tab-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 8)}`,
  );

  function findChatTargetFor(
    identity: string,
  ): { panelId: string; target: MobKitDockTarget } | null {
    // This is also called from the long-lived SSE subscription closure via
    // maybeDrainHead(); read the dock ref so pending queue auto-drain sees
    // panels opened after the first render.
    for (const panel of dockRef.current.viewState.panels) {
      const t = panel.target as MobKitDockTarget | null;
      if (!t || t.kind !== "agent-chat") continue;
      if ((t.identity || t.memberId) === identity) {
        return { panelId: panel.id, target: t };
      }
    }
    return null;
  }

  function onStackSteer(identity: string, id: string) {
    if (consoleReadOnlyRef.current) return;
    setPendingStack(identity, (prev) =>
      prev.map((it) =>
        it.id === id ? { ...it, status: "promoting", editing: false } : it,
      ),
    );
    window.setTimeout(() => {
      if (consoleReadOnlyRef.current) return;
      const stack = getPendingStack(identity);
      const item = stack.find((it) => it.id === id);
      if (!item) return;
      setPendingStack(identity, (prev) => prev.filter((it) => it.id !== id));
      const target = findChatTargetFor(identity);
      if (target) {
        void submitMessageNow(
          target.panelId,
          target.target,
          item.text,
          "steer",
        );
      }
    }, animMs(360));
  }

  function onStackTrash(identity: string, id: string) {
    setPendingStack(identity, (prev) =>
      prev.map((it) =>
        it.id === id ? { ...it, status: "trashing", editing: false } : it,
      ),
    );
    window.setTimeout(() => {
      setPendingStack(identity, (prev) => prev.filter((it) => it.id !== id));
    }, animMs(320));
  }

  function onStackEdit(identity: string, id: string) {
    setPendingStack(identity, (prev) =>
      prev.map((it) =>
        it.id === id ? { ...it, editing: true } : { ...it, editing: false },
      ),
    );
  }

  function onStackCommitEdit(identity: string, id: string, text: string) {
    const trimmed = text.trim();
    if (!trimmed) return;
    setPendingStack(identity, (prev) =>
      prev.map((it) =>
        it.id === id
          ? { ...it, text: trimmed, editing: false, addedAt: Date.now() }
          : it,
      ),
    );
  }

  function onStackCancelEdit(identity: string, id: string) {
    setPendingStack(identity, (prev) =>
      prev.map((it) => (it.id === id ? { ...it, editing: false } : it)),
    );
  }

  function onStackReorder(
    identity: string,
    dragId: string,
    dropId: string,
    where: "above" | "below",
  ) {
    setPendingStack(identity, (prev) => {
      const fromIdx = prev.findIndex((it) => it.id === dragId);
      const toIdx = prev.findIndex((it) => it.id === dropId);
      if (fromIdx === -1 || toIdx === -1) return prev;
      const next = prev.slice();
      const [moved] = next.splice(fromIdx, 1);
      let insertAt = next.findIndex((it) => it.id === dropId);
      if (where === "below") insertAt += 1;
      next.splice(insertAt, 0, moved);
      return next;
    });
  }

  function onStackClearAll(identity: string) {
    setPendingStack(identity, (prev) =>
      prev.map((it) => ({ ...it, status: "trashing", editing: false })),
    );
    window.setTimeout(() => {
      setPendingStack(identity, () => []);
    }, animMs(320));
  }

  function onStackToggleExpand(identity: string, id: string) {
    setPendingStack(identity, (prev) =>
      prev.map((it) => (it.id === id ? { ...it, expanded: !it.expanded } : it)),
    );
  }

  /// Auto-drain hook — fires when an identity transitions busy→idle
  /// AND has pending items. Pops the head, plays the drain animation,
  /// then submits via `submitMessageNow` with normal queue handling.
  function maybeDrainHead(identity: string) {
    if (consoleReadOnlyRef.current) return;
    const stack = getPendingStack(identity);
    if (stack.length === 0) return;
    const target = findChatTargetFor(identity);
    if (!target) return;
    // Only drain if no item is already mid-drain or mid-promotion.
    if (
      stack.some((it) => it.status === "draining" || it.status === "promoting")
    )
      return;
    const head = stack.find((it) => !it.status || it.status === "entering");
    if (!head) return;
    const drainClaim = `${pendingDrainOwnerRef.current}:${head.id}:${Date.now().toString(36)}`;
    const drainClaimedAt = Date.now();
    setPendingStack(identity, (prev) =>
      prev.map((it) =>
        it.id === head.id
          ? { ...it, status: "draining", drainClaim, drainClaimedAt }
          : it,
      ),
    );
    window.setTimeout(() => {
      if (consoleReadOnlyRef.current) return;
      const persistedHead = loadPendingStack(identity, {
        preserveFreshDraining: true,
      }).find((it) => it.id === head.id);
      if (persistedHead?.drainClaim !== drainClaim) return;
      const target = findChatTargetFor(identity);
      if (!target) {
        setPendingStack(identity, (prev) =>
          prev.map((it) =>
            it.id === head.id && it.drainClaim === drainClaim
              ? { ...it, status: null, drainClaim: undefined }
              : it,
          ),
        );
        return;
      }
      setPendingStack(identity, (prev) =>
        prev.filter(
          (it) => it.id !== head.id || it.drainClaim !== drainClaim,
        ),
      );
      void submitMessageNow(
        target.panelId,
        target.target,
        head.text,
        "queue",
      );
    }, animMs(420));
  }

  // =========================================================================
  // LIFECYCLE ACTIONS
  // =========================================================================

  async function onLifecycleAction(
    identity: string,
    method: "mobkit/retire" | "mobkit/respawn" | "mobkit/reset",
  ) {
    if (consoleReadOnly) return;
    const command =
      method === "mobkit/retire"
        ? CONSOLE_COMMAND_NAMES.retireIdentity
        : method === "mobkit/respawn"
          ? CONSOLE_COMMAND_NAMES.respawnIdentity
          : CONSOLE_COMMAND_NAMES.resetIdentity;
    try {
      await executeHeadlessCommand(command, identityWorkbenchTarget(identity, "chat"), {
        identity,
      });
      setActionError("");
    } catch (lifecycleError) {
      // Capability/grant gates and RPC failures surface in the action-error
      // banner instead of escaping as an unhandled rejection (the callers all
      // `void` this promise).
      setActionError(errorMessage(lifecycleError));
      return;
    }
    const nextAgents = await loadExperience();
    if (method !== "mobkit/retire") return;
    if (
      nextAgents.some(
        (agent) => agent.identity === identity || agent.member_id === identity,
      )
    )
      return;
    const fallback =
      nextAgents.find(
        (agent) => agent.addressable || agent.affordances?.can_send_message,
      ) || nextAgents[0];
    if (fallback) {
      openAgentChat(fallback, "replace_focused");
    } else {
      dock.openTarget(buildControlTarget("roster"), "replace_focused");
    }
  }

  async function onGatingDecision(
    pendingId: string,
    decision: "approve" | "reject" | "escalate",
  ) {
    if (consoleReadOnly) return;
    const gatingTarget = controlWorkbenchTarget("gating");
    await executeHeadlessCommand(CONSOLE_COMMAND_NAMES.decideGating, gatingTarget, {
      pending_id: pendingId,
      approver_id: DEFAULT_APPROVER_ID,
      decision,
      reason: `console_${decision}`,
    } as ConsoleGatingActionPayload);
    const [p, a] = await Promise.all([
      executeHeadlessCommand(CONSOLE_COMMAND_NAMES.listGatingPending, gatingTarget),
      executeHeadlessCommand(CONSOLE_COMMAND_NAMES.listGatingAudit, gatingTarget, { limit: 50 }),
    ]);
    const pending = p && typeof p === "object" ? p as { pending?: unknown[] } : {};
    const audit = a && typeof a === "object" ? a as { entries?: unknown[] } : {};
    setGatingData({
      pending: Array.isArray(pending.pending) ? pending.pending : [],
      audit: Array.isArray(audit.entries) ? audit.entries : [],
    });
  }

  // =========================================================================
  // WORKGRAPH OPERATOR ACTIONS (inline card + panel)
  // =========================================================================

  const canManageWorkGraph =
    experience?.workgraph?.can_manage === true && !consoleReadOnly;

  const runWorkGraphCommand = React.useCallback(
    async (
      command:
        | typeof CONSOLE_COMMAND_NAMES.workgraphClaim
        | typeof CONSOLE_COMMAND_NAMES.workgraphRelease
        | typeof CONSOLE_COMMAND_NAMES.workgraphClose
        | typeof CONSOLE_COMMAND_NAMES.workgraphGoalConfirm
        | typeof CONSOLE_COMMAND_NAMES.workgraphGoalRequestClose
        | typeof CONSOLE_COMMAND_NAMES.workgraphAttentionPause
        | typeof CONSOLE_COMMAND_NAMES.workgraphAttentionResume
        | typeof CONSOLE_COMMAND_NAMES.workgraphAttentionReassign,
      params: Record<string, unknown>,
      cardIdentity?: string,
    ) => {
      if (consoleReadOnlyRef.current) return;
      // Operator RPCs run outside any agent turn, so the server emits no
      // console frames for them. Echo the RPC outcome into the identity log
      // as a client-local synthetic frame: the inline card folds the fresh
      // item/binding revisions (or the failure note) immediately, so
      // consecutive card actions CAS against the updated state instead of
      // conflicting on a stale one.
      const echoResultToCard = (result?: unknown, failureMessage?: string) => {
        if (!cardIdentity) return;
        appendFrame(
          cardIdentity,
          buildWorkGraphOperatorResultFrame({
            method: consoleCommandMethod(command),
            params,
            ...(failureMessage !== undefined
              ? { errorMessage: failureMessage }
              : { result }),
            identity: cardIdentity,
          }),
        );
        forceRender();
      };
      try {
        const result = await executeHeadlessCommand(command, controlWorkbenchTarget("workgraph"), params);
        setActionError("");
        echoResultToCard(result);
      } catch (err) {
        // CAS conflicts and grant gates land in the action-error banner and,
        // through the same synthetic frame, as a failure note on the card.
        const message = errorMessage(err);
        setActionError(message);
        echoResultToCard(undefined, message);
        // A -32042 conflict proves the revision the card folded is stale;
        // without a fresh sighting every next click would resend it forever.
        // Re-read the entity and fold the live state through the same echo
        // path (marked `refresh` so the failure flag survives): the NEXT
        // action CASes against the live revision. Best-effort — the banner
        // and failure note above already surfaced the conflict.
        if (cardIdentity && jsonRpcErrorCode(err) === WORKGRAPH_CONFLICT_CODE) {
          const refresh = workGraphConflictRefreshRequest(params);
          if (refresh) {
            try {
              const fresh = await executeHeadlessCommand(
                refresh.command,
                controlWorkbenchTarget("workgraph"),
                refresh.params,
              );
              appendFrame(
                cardIdentity,
                buildWorkGraphOperatorResultFrame({
                  method: consoleCommandMethod(refresh.command),
                  params: refresh.params,
                  result: fresh,
                  identity: cardIdentity,
                  refresh: true,
                }),
              );
              forceRender();
            } catch {
              // The stale card stays; the conflict is already visible.
            }
          }
        }
      }
      if (workGraphPanelDockedRef.current) {
        await refreshWorkGraphData().catch(() => {});
      }
    },
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [baseUrl, refreshWorkGraphData],
  );

  const runWorkGraphQuery = React.useCallback<WorkGraphCommandRunner>(
    (command, params) =>
      executeHeadlessCommand(command, controlWorkbenchTarget("workgraph"), params),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [baseUrl],
  );

  // Operator handlers shared by the inline card and the docked panel.
  // `cardIdentity` scopes the synthetic result echo to that identity's chat
  // log; the panel passes none (it re-reads the snapshot instead). A payload
  // without a revision means the UI never observed the CAS token — resolve
  // the live one first and, if that fails, surface the banner without
  // sending (a guessed 0 is a guaranteed conflict).
  const makeWorkGraphOperatorHandlers = React.useCallback(
    (cardIdentity?: string) => {
      const dispatch = (
        resolveRevision: () => Promise<number>,
        send: (expectedRevision: number) => Promise<void>,
      ) => {
        void (async () => {
          let expectedRevision: number;
          try {
            expectedRevision = await resolveRevision();
          } catch (err) {
            setActionError(errorMessage(err));
            return;
          }
          await send(expectedRevision);
        })();
      };
      const revisionOr = (
        revision: number | undefined,
        resolve: () => Promise<number>,
      ) => (revision !== undefined ? () => Promise.resolve(revision) : resolve);
      return {
        onClaim: ({ itemId, revision }: { itemId: string; revision?: number }) =>
          dispatch(
            revisionOr(revision, () => resolveWorkGraphItemRevision(runWorkGraphQuery, itemId)),
            (expectedRevision) =>
              runWorkGraphCommand(CONSOLE_COMMAND_NAMES.workgraphClaim, {
                id: itemId,
                expected_revision: expectedRevision,
                owner: {
                  kind: "principal",
                  id: workGraphClaimOwnerId(experience?.access?.subject, DEFAULT_APPROVER_ID),
                },
              }, cardIdentity),
          ),
        onClose: ({ itemId, revision }: { itemId: string; revision?: number }) =>
          dispatch(
            revisionOr(revision, () => resolveWorkGraphItemRevision(runWorkGraphQuery, itemId)),
            (expectedRevision) =>
              runWorkGraphCommand(CONSOLE_COMMAND_NAMES.workgraphClose, {
                id: itemId,
                expected_revision: expectedRevision,
              }, cardIdentity),
          ),
        onGoalConfirm: ({ bindingId, revision }: { bindingId: string; revision?: number }) =>
          dispatch(
            revisionOr(revision, () => resolveWorkGraphGoalItemRevision(runWorkGraphQuery, bindingId)),
            (expectedRevision) =>
              runWorkGraphCommand(CONSOLE_COMMAND_NAMES.workgraphGoalConfirm, {
                binding_id: bindingId,
                expected_revision: expectedRevision,
              }, cardIdentity),
          ),
        onGoalRequestClose: ({ bindingId, revision }: { bindingId: string; revision?: number }) =>
          dispatch(
            revisionOr(revision, () => resolveWorkGraphGoalItemRevision(runWorkGraphQuery, bindingId)),
            (expectedRevision) =>
              runWorkGraphCommand(CONSOLE_COMMAND_NAMES.workgraphGoalRequestClose, {
                binding_id: bindingId,
                expected_revision: expectedRevision,
              }, cardIdentity),
          ),
        onAttentionPause: ({ bindingId, revision }: { bindingId: string; revision?: number }) =>
          dispatch(
            revisionOr(revision, () => resolveWorkGraphBindingRevision(runWorkGraphQuery, bindingId)),
            (expectedRevision) =>
              runWorkGraphCommand(CONSOLE_COMMAND_NAMES.workgraphAttentionPause, {
                binding_id: bindingId,
                expected_revision: expectedRevision,
              }, cardIdentity),
          ),
        onAttentionResume: ({ bindingId, revision }: { bindingId: string; revision?: number }) =>
          dispatch(
            revisionOr(revision, () => resolveWorkGraphBindingRevision(runWorkGraphQuery, bindingId)),
            (expectedRevision) =>
              runWorkGraphCommand(CONSOLE_COMMAND_NAMES.workgraphAttentionResume, {
                binding_id: bindingId,
                expected_revision: expectedRevision,
              }, cardIdentity),
          ),
        onAttentionReassign: ({ bindingId, revision, identity }: { bindingId: string; revision?: number; identity: string }) =>
          dispatch(
            revisionOr(revision, () => resolveWorkGraphBindingRevision(runWorkGraphQuery, bindingId)),
            (expectedRevision) =>
              runWorkGraphCommand(CONSOLE_COMMAND_NAMES.workgraphAttentionReassign, {
                binding_id: bindingId,
                expected_revision: expectedRevision,
                target: { kind: "identity", identity },
              }, cardIdentity),
          ),
      };
    },
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [runWorkGraphCommand, runWorkGraphQuery, experience?.access?.subject],
  );

  // Inline-card affordances: undefined when the caller lacks the manage
  // grant (or the console is read-only) — the card then renders no buttons.
  // Reassign needs a typed-in target identity, so only the panel offers it.
  const workGraphCardActions = React.useCallback(
    (cardIdentity: string): WorkGraphCardActions | undefined => {
      if (!canManageWorkGraph) return undefined;
      const { onAttentionReassign: _panelOnly, ...cardHandlers } =
        makeWorkGraphOperatorHandlers(cardIdentity);
      return cardHandlers;
    },
    [canManageWorkGraph, makeWorkGraphOperatorHandlers],
  );

  // =========================================================================
  // RESIZE HANDLERS (unchanged)
  // =========================================================================

  const SIDEBAR_MIN = 180,
    SIDEBAR_MAX = 420;
  function handleSidebarResize(event: React.PointerEvent<HTMLDivElement>) {
    event.preventDefault();
    const startX = event.clientX;
    const root = findPaneResizeRoot(event.currentTarget);
    if (!root) return;
    const startWidth =
      parseInt(
        getComputedStyle(root).getPropertyValue(
          "--cc-workbench-sidebar-width",
        ) || "260",
        10,
      ) || 260;
    const handle = event.currentTarget;
    if ("setPointerCapture" in handle)
      handle.setPointerCapture(event.pointerId);
    document.documentElement.setAttribute("data-cc-resizing", "true");
    function onPointerMove(e: PointerEvent) {
      root!.style.setProperty(
        "--cc-workbench-sidebar-width",
        `${Math.min(SIDEBAR_MAX, Math.max(SIDEBAR_MIN, startWidth + (e.clientX - startX)))}px`,
      );
    }
    function cleanup() {
      document.documentElement.removeAttribute("data-cc-resizing");
      window.removeEventListener("pointermove", onPointerMove);
      window.removeEventListener("pointerup", cleanup);
      window.removeEventListener("pointercancel", cleanup);
      if (
        "hasPointerCapture" in handle &&
        handle.hasPointerCapture(event.pointerId)
      )
        handle.releasePointerCapture(event.pointerId);
    }
    window.addEventListener("pointermove", onPointerMove);
    window.addEventListener("pointerup", cleanup);
    window.addEventListener("pointercancel", cleanup);
  }

  const ACTIVITY_MIN = 200,
    ACTIVITY_MAX = 480;
  function handleActivityResize(event: React.PointerEvent<HTMLDivElement>) {
    event.preventDefault();
    const startX = event.clientX;
    const root = findPaneResizeRoot(event.currentTarget);
    if (!root) return;
    const startWidth =
      parseInt(
        getComputedStyle(root).getPropertyValue(
          "--cc-workbench-activity-width",
        ) || "280",
        10,
      ) || 280;
    const handle = event.currentTarget;
    if ("setPointerCapture" in handle)
      handle.setPointerCapture(event.pointerId);
    document.documentElement.setAttribute("data-cc-resizing", "true");
    function onPointerMove(e: PointerEvent) {
      root!.style.setProperty(
        "--cc-workbench-activity-width",
        `${Math.min(ACTIVITY_MAX, Math.max(ACTIVITY_MIN, startWidth - (e.clientX - startX)))}px`,
      );
    }
    function cleanup() {
      document.documentElement.removeAttribute("data-cc-resizing");
      window.removeEventListener("pointermove", onPointerMove);
      window.removeEventListener("pointerup", cleanup);
      window.removeEventListener("pointercancel", cleanup);
      if (
        "hasPointerCapture" in handle &&
        handle.hasPointerCapture(event.pointerId)
      )
        handle.releasePointerCapture(event.pointerId);
    }
    window.addEventListener("pointermove", onPointerMove);
    window.addEventListener("pointerup", cleanup);
    window.addEventListener("pointercancel", cleanup);
  }

  // =========================================================================
  // RENDER GUARDS
  // =========================================================================

  if (loading)
    return (
      <div
        data-testid="console-loading"
        aria-live="polite"
        aria-busy="true"
        style={{
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
          gap: "0.6rem",
          minHeight: "100vh",
        }}
      >
        <span className="msg__typing-dots" aria-hidden="true">
          <span /><span /><span />
        </span>
        <span>Loading console…</span>
      </div>
    );
  if (error) return <div data-testid="console-error">{error}</div>;

  // =========================================================================
  // BUILD VIEW STATES
  // =========================================================================

  const focusedMemberId =
    dock.focusedTarget?.kind === "agent-chat"
      ? dock.focusedTarget.memberId
      : selectedRosterMemberId;
  const sidebarVS = buildSidebarViewState({
    agents,
    selectedMemberId: focusedMemberId,
    pinnedAgentIds,
  });
  const activityVS = buildActivityRailViewState({
    agents,
    eventFrames: activityRef.current,
    filterPresets:
      experience?.console_config?.rail?.filter_presets ||
      experience?.activity_feed?.filter_presets,
    activePresetId:
      activeActivityPresetId ||
      experience?.console_config?.rail?.active_preset_id ||
      "all",
  });
  const actionConfig = experience?.console_config?.actions;
  const configuredActionLabels = {
    inspect: actionLabel(actionConfig, "inspect_label", "Details"),
    chat: actionLabel(actionConfig, "chat_label", "Open chat"),
    send: actionLabel(actionConfig, "send_label", "Send"),
    respawn: actionLabel(actionConfig, "respawn_label", "Respawn"),
    retire: actionLabel(actionConfig, "retire_label", "Retire"),
    reset: actionLabel(actionConfig, "reset_label", "Reset"),
  };
  const configuredActionVisibility = {
    inspect: actionVisible(actionConfig, "show_inspect"),
    chat: actionVisible(actionConfig, "show_chat"),
    respawn: actionVisible(actionConfig, "show_respawn"),
    retire: actionVisible(actionConfig, "show_retire"),
    reset: actionVisible(actionConfig, "show_reset"),
  };

  // =========================================================================
  // RENDER: CHAT PANEL — reads from 3 identity-keyed refs
  // =========================================================================

  function renderChatPanel(panel: {
    id: string;
    target?: MobKitDockTarget | null;
  }) {
    const target = panel.target;
    if (!target || target.kind !== "agent-chat") return null;
    const panelKey = buildPanelConversationKey(panel.id, target);
    const identity = target.identity || target.memberId;
    const agent = agents.find((c) => c.member_id === target.memberId) || null;

    // Single adapter pass over the canonical sorted log. No more
    // server/live split, no cross-store dedup, no duplicate grouping.
    // Text deltas are rendered as the interaction streams; the
    // adapter's `streamedText === terminalText` check suppresses the
    // duplicate when text_complete/interaction_complete arrives.
    const sortedFrames = framesVisibleInPanel(
      getSortedFrames(identity),
      panel.id,
    );
    const conversationEntries = mapFramesToTimelineEntries(
      agent,
      sortedFrames,
      {
        renderInteractionStartsAsUser: true,
        renderTextDeltas: true,
        blobBaseUrl: baseUrl,
      },
    );

    // Optimistic user message: rendered until an interaction_started
    // with the matching interaction_id is appended to the log (which
    // clears it via appendFrame). Until then, it sits at the tail of
    // the conversation as a synthetic entry.
    const optimisticUser = optimisticUserMessageForPanel(
      optimisticUserByPanelKeyRef.current,
      panelKey,
      identity,
    );
    const optimisticEntry = optimisticUser ? optimisticUser.entry : null;

    // `conversationEntries` are already in transcript order. Re-sorting
    // rendered entries by createdAt here would lose the adapter's
    // same-turn grouping/tie-break rules.
    const entries = sanitizeConversationEntries(
      appendOptimisticConversationEntry(conversationEntries, optimisticEntry),
    );

    const conversation = buildConversationViewState({
      memberId: target.memberId,
      agentLabel: target.title,
      agent,
      entries,
    });
    const draft = draftByKey[panelKey] || "";
    const staged = stagedAttachmentsByIdentity[identity] ?? [];
    const identityLog = getOrCreateLog(identity);
    const isSending = sendingPanels.has(panelKey);
    const hasLocalPhase = Object.prototype.hasOwnProperty.call(
      phaseRef.current,
      panelKey,
    );
    const honorLocalPhase = hasLocalPhase && (isSending || optimisticEntry !== null);
    const phase = resolvePanelResponsePhase({
      frames: sortedFrames.filter((frame) => PANEL_ROUTABLE_EVENTS.has(frame.event)),
      localPhase: honorLocalPhase ? phaseRef.current[panelKey] ?? null : null,
      hasLocalPhase: honorLocalPhase,
      serverPhase: agent?.response_phase ?? null,
    });
    const canRespawn =
      !consoleReadOnly &&
      configuredActionVisibility.respawn &&
      agent?.affordances?.can_respawn === true;
    const canRetire =
      !consoleReadOnly &&
      configuredActionVisibility.retire &&
      agent?.affordances?.can_retire === true;

    const stackItems = getPendingStack(identity);
    const agentBusy = isIdentityBusy(identity);
    const stackSlot =
      stackItems.length > 0 ? (
        <PendingStack
          items={stackItems}
          agentBusy={agentBusy}
          reducedMotion={reducedMotion}
          onSteer={(itemId) => onStackSteer(identity, itemId)}
          onTrash={(itemId) => onStackTrash(identity, itemId)}
          onEdit={(itemId) => onStackEdit(identity, itemId)}
          onCommitEdit={(itemId, t) => onStackCommitEdit(identity, itemId, t)}
          onCancelEdit={(itemId) => onStackCancelEdit(identity, itemId)}
          onReorder={(dragId, dropId, where) =>
            onStackReorder(identity, dragId, dropId, where)
          }
          onClearAll={() => onStackClearAll(identity)}
          onToggleExpand={(itemId) => onStackToggleExpand(identity, itemId)}
        />
      ) : null;

    return (
      <ChatPane
        agent={agent}
        agentLabel={target.title || agent?.label || identity}
        identity={identity}
        entries={entries}
        phase={phase}
        isLoadingHistory={Boolean(loadingHistory[identity])}
        draft={draft}
        sending={isSending}
        readOnly={consoleReadOnly}
        accessEnforcing={accessEnforcing}
        staged={staged}
        onDraftChange={(v) => setDraftByKey((c) => ({ ...c, [panelKey]: v }))}
        onStagedChange={(action) =>
          setStagedAttachmentsForIdentity(identity, action)
        }
        onSend={(attachments) => onSendMessage(panel.id, target, attachments)}
        onInspect={
          configuredActionVisibility.inspect
            ? () => {
                if (agent) handleShowRosterDetails(agent);
              }
            : undefined
        }
        onRespawn={
          canRespawn
            ? () => void onLifecycleAction(identity, "mobkit/respawn")
            : undefined
        }
        onRetire={
          canRetire
            ? () => void onLifecycleAction(identity, "mobkit/retire")
            : undefined
        }
        inspectLabel={configuredActionLabels.inspect}
        respawnLabel={configuredActionLabels.respawn}
        retireLabel={configuredActionLabels.retire}
        sendLabel={configuredActionLabels.send}
        hasOlderHistory={
          identityLog.hasServerLog === true &&
          Boolean(identityLog.oldestTimelineCursor) &&
          identityLog.olderHistoryExhausted !== true
        }
        loadingOlderHistory={identityLog.olderHistoryLoading === true}
        onLoadOlder={() => void loadOlderIdentityTimeline(identity)}
        stackSlot={stackSlot}
        workGraphActions={workGraphCardActions(identity)}
      />
    );
  }

  // =========================================================================
  // RENDER: CONTROL PANELS (unchanged)
  // =========================================================================

  function renderInspectPanel(
    target: Extract<MobKitDockTarget, { kind: "identity-inspect" }>,
  ) {
    const inspect = inspectByIdentity[target.identity];
    const agent = agents.find(
      (candidate) =>
        candidate.identity === target.identity ||
        candidate.member_id === target.identity,
    );
    const canRespawn =
      !consoleReadOnly &&
      configuredActionVisibility.respawn &&
      agent?.affordances?.can_respawn === true;
    const canRetire =
      !consoleReadOnly &&
      configuredActionVisibility.retire &&
      agent?.affordances?.can_retire === true;
    const canReset =
      !consoleReadOnly &&
      configuredActionVisibility.reset &&
      experience?.runtime_capabilities?.can_retire_members === true;
    return (
      <div
        className="console-panel"
        data-testid={`inspect-panel:${target.identity}`}
      >
        <div className="console-panel__header">
          <h3>{target.identity}</h3>
          <div className="console-panel__actions">
            {canRespawn ? (
              <button
                data-testid={`inspect-action:${target.identity}:respawn`}
                type="button"
                onClick={() =>
                  void onLifecycleAction(target.identity, "mobkit/respawn")
                }
              >
                {configuredActionLabels.respawn}
              </button>
            ) : null}
            {canReset ? (
              <button
                data-testid={`inspect-action:${target.identity}:reset`}
                type="button"
                onClick={() =>
                  void onLifecycleAction(target.identity, "mobkit/reset")
                }
              >
                {configuredActionLabels.reset}
              </button>
            ) : null}
            {canRetire ? (
              <button
                data-testid={`inspect-action:${target.identity}:retire`}
                type="button"
                onClick={() =>
                  void onLifecycleAction(target.identity, "mobkit/retire")
                }
              >
                {configuredActionLabels.retire}
              </button>
            ) : null}
          </div>
        </div>
        {!inspect ? (
          <p>Loading identity details…</p>
        ) : (
          <dl className="console-panel__grid">
            <dt>State</dt>
            <dd>{inspect.state}</dd>
            <dt>Role</dt>
            <dd>{inspect.role || "n/a"}</dd>
            <dt>Addressability</dt>
            <dd>{inspect.addressability}</dd>
            <dt>Generation</dt>
            <dd>{inspect.continuity?.generation ?? "n/a"}</dd>
            <dt>Checkpoint</dt>
            <dd>{inspect.continuity?.checkpoint_version ?? "n/a"}</dd>
            <dt>Session</dt>
            <dd>{inspect.continuity?.session_id || "n/a"}</dd>
            <dt>Runtime</dt>
            <dd>{inspect.continuity?.agent_runtime_id || "n/a"}</dd>
            <dt>Lease Healthy</dt>
            <dd>
              {String(inspect.lease_healthy ?? inspect.lease?.healthy ?? false)}
            </dd>
            <dt>Peers</dt>
            <dd>{inspect.topology_peers?.join(", ") || "none"}</dd>
            <dt>Output Preview</dt>
            <dd>{inspect.output_preview || "n/a"}</dd>
          </dl>
        )}
      </div>
    );
  }

  function renderHealthPanel(identities: IdentityStatusRow[]) {
    return (
      <div className="console-panel" data-testid="health-panel">
        <ul className="console-panel__list">
          {identities.map((r) => (
            <li data-testid={`health-identity:${r.identity}`} key={r.identity}>
              <strong>{r.display_name || r.identity}</strong> · {r.state} ·{" "}
              {r.addressability}
            </li>
          ))}
        </ul>
      </div>
    );
  }

  async function refreshInspectIdentity(identity: string): Promise<void> {
    const r = await inspectIdentityViaHeadless(identity);
    setInspectByIdentity((current) => ({
      ...current,
      [identity]: normalizeConsoleInspectResult(r),
    }));
  }

  function handleShowRosterDetails(agent: ConsoleAgent) {
    setSelectedRosterMemberId(agent.member_id);
    const target = buildInspectTarget(agent);
    dock.openTarget(target, "replace_focused");
    void refreshInspectIdentity(target.identity).catch(() => {});
  }

  // =========================================================================
  // MAIN RENDER
  // =========================================================================

  const mobName =
    experience?.console_config?.title ||
    experience?.agent_sidebar?.title ||
    "mob";
  const brand = experience?.console_config?.brand;
  const environmentLabel =
    experience?.console_config?.environment?.label || "dev";
  const railConfig = experience?.console_config?.rail;
  const railVisible = railConfig?.visible !== false;
  const watchedIdentities = new Set(
    agents
      .filter((agent) => agent.watched)
      .map((agent) => agent.identity || agent.member_id)
      .filter((value): value is string => Boolean(value)),
  );
  const mobStatus =
    experience?.health_overview?.live_snapshot?.running === false
      ? "stopped"
      : "running";

  function toggleTheme() {
    const next: ConsoleTheme = theme === "dark" ? "light" : "dark";
    setTheme(next);
    try {
      localStorage.setItem("mobkit-console-theme", next);
    } catch {
      /* ignore */
    }
  }

  function renderPanelBody(panel: {
    id: string;
    target?: MobKitDockTarget | null;
  }) {
    const target = panel.target as MobKitDockTarget | null;
    if (!target) return <div className="console-panel">No panel target</div>;
    if (target.kind === "agent-chat") return renderChatPanel(panel);
    if (target.kind === "identity-inspect") {
      return renderInspectPanel(target);
    }
    if (
      (target.kind === "routing" ||
        target.kind === "gating" ||
        target.kind === "gates" ||
        target.kind === "workgraph") &&
      !hasMobControlSurface
    ) {
      return (
        <div className="console-panel">
          This view requires a mob runtime control surface.
        </div>
      );
    }
    if (target.kind === "routing") return <RoutingPanel data={routingData} />;
    if (target.kind === "gating")
      return (
        <GatingInboxPanel
          pending={gatingData.pending}
          audit={gatingData.audit}
          onDecide={(pid, decision) => void onGatingDecision(pid, decision)}
          readOnly={consoleReadOnly}
        />
      );
    if (target.kind === "topology")
      return (
        <TopologyPanel
          nodes={experience?.topology?.live_snapshot?.nodes || []}
          agents={agents}
          activity={liveFrames}
        />
      );
    if (target.kind === "health")
      return renderHealthPanel(
        experience?.health_overview?.live_snapshot?.identities || [],
      );
    if (target.kind === "timeline")
      return <TimelinePanel frames={activityRef.current} />;
    if (target.kind === "roster")
      return (
        <RosterPanel
          agents={agents}
          selectedMemberId={selectedRosterMemberId}
          onSelect={(a) => setSelectedRosterMemberId(a.member_id)}
          onChat={(a) => openAgentChat(a)}
          onDetails={(a) => handleShowRosterDetails(a)}
          onLifecycle={(identity, method) =>
            void onLifecycleAction(identity, method)
          }
          canResetLifecycle={!consoleReadOnly && hasMobControlSurface}
          actionLabels={configuredActionLabels}
          actionVisibility={{
            ...configuredActionVisibility,
            respawn: !consoleReadOnly && configuredActionVisibility.respawn,
            retire: !consoleReadOnly && configuredActionVisibility.retire,
            reset: !consoleReadOnly && configuredActionVisibility.reset,
          }}
        />
      );
    if (target.kind === "gates")
      return (
        <GatingInboxPanel
          pending={gatingData.pending}
          audit={gatingData.audit}
          onDecide={(pid, decision) => void onGatingDecision(pid, decision)}
          readOnly={consoleReadOnly}
        />
      );
    if (target.kind === "logs")
      return <LogsPanel frames={activityRef.current} />;
    if (target.kind === "access")
      return (
        <AccessPanel
          status={accessData.status}
          config={accessData.config}
          error={accessData.error}
          readOnly={frontendReadOnly || experience?.console_policy?.read_only === true}
          agents={agents.map((agent) => ({
            identity: agent.identity || agent.member_id,
            label: agent.label,
          }))}
          onRefresh={() => void refreshAccessData()}
          onSetEnabled={(enabled) =>
            void runAccessMutation(CONSOLE_COMMAND_NAMES.enableAccess, { enabled })
          }
          onSaveAdmins={(admins) => {
            const config = {
              ...(accessData.config || {}),
              admins,
            };
            void runAccessMutation(CONSOLE_COMMAND_NAMES.setAccessConfig, { config });
          }}
          onUpsertRule={(rule) =>
            void runAccessMutation(CONSOLE_COMMAND_NAMES.upsertAccessRule, { rule })
          }
          onDeleteRule={(id) =>
            void runAccessMutation(CONSOLE_COMMAND_NAMES.deleteAccessRule, { id })
          }
          onSaveGroup={(name, group) =>
            void runAccessMutation(CONSOLE_COMMAND_NAMES.setAccessGroup, { name, group })
          }
          onDeleteGroup={(name) =>
            void runAccessMutation(CONSOLE_COMMAND_NAMES.deleteAccessGroup, { name })
          }
          onPreview={async (subject, action, identity) => {
            try {
              return (
                ((await executeHeadlessCommand(
                  CONSOLE_COMMAND_NAMES.previewAccess,
                  controlWorkbenchTarget("access"),
                  identity ? { subject, action, identity } : { subject, action },
                )) as AccessPreviewResult | null) || null
              );
            } catch (err) {
              setAccessData((current) => ({ ...current, error: errorMessage(err) }));
              return null;
            }
          }}
        />
      );
    if (target.kind === "memory")
      return (
        <MemoryPanel
          records={memoryData.records}
          realms={memoryData.realms}
          quarantineRecords={memoryData.quarantineRecords}
          pendingPromotions={memoryData.pendingPromotions}
          dreams={memoryData.dreams}
          detail={memoryData.detail}
          detailLoading={memoryData.detailLoading}
          canReviewQuarantine={experience?.memory?.can_review_quarantine === true}
          unavailable={memoryData.unavailable}
          error={memoryData.error}
          nextCursor={memoryData.nextCursor}
          recordsDenied={memoryData.recordsDenied}
          dreamsDenied={memoryData.dreamsDenied}
          operatorScopeDenied={memoryData.operatorScopeDenied}
          mobScopeDenied={memoryData.mobScopeDenied}
          overview={memoryData.overview}
          overviewDenied={memoryData.overviewDenied}
          proposals={memoryData.proposals}
          proposalsDenied={memoryData.proposalsDenied}
          injections={memoryData.injections}
          injectionsDenied={memoryData.injectionsDenied}
          harvests={memoryData.harvests}
          harvestsDenied={memoryData.harvestsDenied}
          dreamRuns={memoryData.dreamRuns}
          dreamRunsDenied={memoryData.dreamRunsDenied}
          auditVerdicts={memoryData.auditVerdicts}
          auditVerdictsDenied={memoryData.auditVerdictsDenied}
          liveFrames={activityRef.current}
          onRefresh={() => void refreshMemoryData()}
          onSelectRecord={(realm, memoryId) => void loadMemoryRecordDetail(realm, memoryId)}
          onClearDetail={() =>
            setMemoryData((current) => ({ ...current, detail: null, detailLoading: false }))
          }
          onQueryRecords={queryMemoryRecords}
          onLoadEvidence={loadMemoryEvidence}
          onOpenGating={
            // Only offered where the nav itself offers gating — on runtimes
            // without a mob control surface (or with gating hidden) the
            // target would land on a dead-end placeholder.
            visibleControls.includes("gating")
              ? () => dock.openTarget(buildControlTarget("gating"), "replace_focused")
              : undefined
          }
        />
      );
    if (target.kind === "workgraph") {
      const workGraphPanelHandlers = makeWorkGraphOperatorHandlers();
      return (
        <WorkGraphPanel
          data={workGraphData}
          canManage={canManageWorkGraph}
          onRefresh={() => void refreshWorkGraphData()}
          onClaim={workGraphPanelHandlers.onClaim}
          onClose={workGraphPanelHandlers.onClose}
          onGoalConfirm={workGraphPanelHandlers.onGoalConfirm}
          onGoalRequestClose={workGraphPanelHandlers.onGoalRequestClose}
          onAttentionPause={workGraphPanelHandlers.onAttentionPause}
          onAttentionResume={workGraphPanelHandlers.onAttentionResume}
          onAttentionReassign={workGraphPanelHandlers.onAttentionReassign}
        />
      );
    }
    return <div className="console-panel">Unsupported panel</div>;
  }

  return (
    <div
      className="cc-theme-scope mobkit-shell"
      data-cc-theme={theme}
      data-cc-variant={variant}
      data-testid="meerkat-console"
    >
      <SpriteSheet />
      {actionError && (
        <div className="mobkit-action-error" data-testid="console-action-error" role="alert">
          <span>{actionError}</span>
          <button
            aria-label="Dismiss error"
            data-testid="console-action-error-dismiss"
            onClick={() => setActionError("")}
            type="button"
          >
            ×
          </button>
        </div>
      )}
      <Topbar
        mobName={mobName}
        brandLabel={brand?.label}
        brandLogoUrl={brand?.logo_url}
        brandLogoAlt={brand?.logo_alt}
        mobStatus={mobStatus}
        environment={environmentLabel}
        theme={theme}
        onToggleTheme={toggleTheme}
        sidebarCollapsed={sidebarCollapsed}
        railCollapsed={railCollapsed}
        railVisible={railVisible}
        onToggleSidebar={toggleSidebarCollapsed}
        onToggleRail={toggleRailCollapsed}
      />
      <div
        className="shell"
        data-console-workbench="root"
        data-sidebar-collapsed={sidebarCollapsed ? "true" : "false"}
        data-rail-collapsed={railCollapsed ? "true" : "false"}
      >
        <DesignSidebar
          agents={agents}
          selectedMemberId={focusedMemberId}
          recentActivity={activityRef.current}
          collapsed={sidebarCollapsed}
          visibleControls={visibleControls}
          customButtons={experience?.console_config?.sidebar?.buttons}
          grouping={experience?.console_config?.agent_list}
          storageNamespace={sidebarStorageNamespace}
          pinnedAgentIds={pinnedAgentIds}
          onSelect={(a) => openAgentChat(a)}
          onTogglePinnedAgent={togglePinnedAgent}
          onOpenControl={(kind) => {
            dock.openTarget(buildControlTarget(kind), "replace_focused");
          }}
        />
        <div
          className="pane-resizer"
          aria-hidden="true"
          data-testid="resize:sidebar"
          onPointerDown={handleSidebarResize}
        />
        <div className="main">
          <MobKitDock
            viewState={dock.viewState}
            agents={agents}
            renderPanelBody={renderPanelBody}
            visibleControls={visibleControls}
            onSelectTab={(id) => dock.selectTab(id)}
            onCloseTab={(id) => dock.closeTab(id)}
            onCreateTab={() => dock.createTab()}
            onFocusPanel={(id) => dock.focusPanel(id)}
            onSplitPanel={(id, dir) => dock.splitPanel(id, dir)}
            onClosePanel={(id) => dock.closePanel(id)}
            onResizeSplit={(id, ratio) => dock.resizeSplit(id, ratio)}
            onOpenTargetInPanel={(panelId, target) => {
              dock.focusPanel(panelId);
              openDockTarget(target);
            }}
          />
        </div>
        {railVisible ? (
          <>
            <div
              className="pane-resizer pane-resizer--activity"
              aria-hidden="true"
              data-testid="resize:activity"
              onPointerDown={handleActivityResize}
            />
            <SignalsRail
              frames={activityRef.current}
              collapsed={railCollapsed}
              filterPresets={railConfig?.filter_presets}
              activePresetId={
                activeActivityPresetId || railConfig?.active_preset_id
              }
              emptyText={railConfig?.empty_text}
              watchedIdentities={watchedIdentities}
              onPresetChange={setActiveActivityPresetId}
              onSelect={
                // "State here" pivot: a live memory signal opens the Memory
                // panel, and lands on the record's Biography when the frame
                // names one. Offered only when the server-projected
                // experience grants memory.can_read — the affordance must
                // never outrun the nav gate.
                experience?.memory?.can_read === true
                  ? (frame) => {
                      if (!frame.event.startsWith("memory.")) return;
                      dock.openTarget(buildControlTarget("memory"), "replace_focused");
                      const pivot = memoryFramePivot(frame);
                      if (pivot) void loadMemoryRecordDetail(pivot.realm, pivot.recordId);
                    }
                  : undefined
              }
            />
          </>
        ) : null}
      </div>
    </div>
  );
}
