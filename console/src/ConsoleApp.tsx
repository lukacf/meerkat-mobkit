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
import type { ConsoleComposerToolbarItem, ConversationTimelineEntry, IdentityInspectViewState } from "@console-core";
import { normalizeIdentityInspectViewState } from "@console-core";

import { normalizeAgents } from "./lib/agents";
import {
  buildActivityRailViewState,
  buildControlTarget,
  buildConversationViewState,
  buildDockTarget,
  buildQuickPromptSuggestions,
  buildInspectTarget,
  buildPanelConversationKey,
  buildRoutingSectionView,
  buildSidebarViewState,
  createUserEntry,
  mapFramesToTimelineEntries,
  sortConversationTimelineEntries,
  type MobKitDockTarget,
} from "./lib/adapters";
import { errorMessage } from "./lib/errors";
import {
  callConsoleRpc,
  fetchJson,
  queryTimeline,
  sendConsole,
  sendConsoleMultipart,
  sendMessage,
  sendMessageMultipart,
  subscribeTimelineEvents,
} from "./lib/network";
import { Icon, SpriteSheet } from "./icon";
import type {
  ConsoleAgent,
  ConsoleExperience,
  ConsoleFrame,
  ConsoleGatingActionPayload,
  ConsoleModulesResponse,
  ConsoleTopologyNode,
  IdentityStatusRow,
} from "./types";
import { TopologyPanel } from "./panels/TopologyPanel";
import { TimelinePanel } from "./panels/TimelinePanel";
import { GatingInboxPanel } from "./panels/GatingInboxPanel";
import { RosterPanel } from "./panels/RosterPanel";
import { RoutingPanel } from "./panels/RoutingPanel";
import { GatesPanel } from "./panels/GatesPanel";
import { LogsPanel } from "./panels/LogsPanel";
import { Topbar } from "./panels/Topbar";
import { useConsoleVariant, type ConsoleTheme } from "./panels/Tweaks";
import { Sidebar as DesignSidebar, type NavKind } from "./panels/Sidebar";
import { SignalsRail } from "./panels/SignalsRail";
import { ChatPane, type StagedAttachment } from "./panels/ChatPane";
import { MobKitDock } from "./panels/MobKitDock";
import { PendingStack, type PendingItem } from "./panels/PendingStack";

interface ConsoleAppProps {
  baseUrl: string;
}

type RoutingPanelData = ReturnType<typeof buildRoutingSectionView>;
type GatingPanelData = { pending: unknown[]; audit: unknown[] };

interface OptimisticUserMessage {
  interactionId: string;
  entry: ConversationTimelineEntry;
  sentAtMs: number;
  objectUrls?: string[];
}

interface IdentityLog {
  events: ConsoleFrame[];
  byKey: Map<string, number>;
  /// `null` while we haven't asked the server yet; `true` if the
  /// runtime has an EventLogStore (we'll fetch backfill); `false`
  /// once we've observed `available: false` (SSE is the only source).
  hasServerLog: boolean | null;
  optimisticUser: OptimisticUserMessage | null;
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
  ].join(" ").trim();
  if (scalarText.length > 0) return true;
  if (
    record.type === "image"
    && (typeof record.src === "string" || typeof record.blobId === "string")
  ) return true;
  if (Array.isArray(record.headers) && record.headers.some((v) => String(v || "").trim().length > 0)) return true;
  if (Array.isArray(record.rows) && record.rows.some((row) => Array.isArray(row) && row.some((v) => String(v || "").trim().length > 0))) return true;
  return false;
}

function sanitizeConversationEntries(entries: ConversationTimelineEntry[]): ConversationTimelineEntry[] {
  const sanitized: ConversationTimelineEntry[] = [];
  for (const entry of entries) {
    if (entry.kind !== "message") { sanitized.push(entry); continue; }
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

function normalizeConsoleInspectResult(value: unknown): IdentityInspectViewState | null {
  const direct = normalizeIdentityInspectViewState(value);
  if (direct) return direct;
  const record = value && typeof value === "object" ? value as Record<string, unknown> : {};
  const identityRecord = record.identity && typeof record.identity === "object"
    ? record.identity as Record<string, unknown>
    : null;
  if (!identityRecord) return null;
  return normalizeIdentityInspectViewState({
    identity: identityRecord.identity,
    display_name: identityRecord.display_name,
    role: identityRecord.labels && typeof identityRecord.labels === "object"
      ? (identityRecord.labels as Record<string, unknown>).role
      : undefined,
    state: identityRecord.health,
    addressability: identityRecord.addressable === true ? "addressable" : "internal_only",
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

function createIdempotencyKey(): string {
  try {
    if (typeof crypto !== "undefined" && typeof crypto.randomUUID === "function") {
      return crypto.randomUUID();
    }
  } catch {
    // Fall through to timestamp-based key.
  }
  return `console-${Date.now().toString(36)}-${Math.random().toString(36).slice(2)}`;
}

function cursorSeq(cursor: string | undefined): number | null {
  if (!cursor) return null;
  const match = /^console:(\d+)$/.exec(cursor);
  if (!match) return null;
  const parsed = Number(match[1]);
  return Number.isFinite(parsed) ? parsed : null;
}

// --- Event sets for the SSE handler ---
const REFRESH_TRIGGER_EVENTS = new Set([
  "interaction_complete", "interaction_failed", "state_changed",
  "member_ready", "member_retired", "gating_decision", "route_changed",
]);
const PANEL_ROUTABLE_EVENTS = new Set([
  "user_input", "interaction_started", "interaction_complete", "interaction_failed",
  "assistant_image",
  "text_delta", "text_complete",
  "tool_call_requested", "tool_call", "tool_result_received",
  "tool_execution_started", "tool_execution_completed",
  "run_started", "run_completed", "run_failed",
]);
const HISTORY_REFRESH_EVENTS = new Set([
  "interaction_complete", "interaction_failed", "run_completed", "run_failed", "message_delivery_failed",
]);
// Events filtered from the activity rail — don't buffer them
const ACTIVITY_SKIP_EVENTS = new Set([
  "subscribed", "run_started", "run_completed", "turn_started", "turn_completed",
  "text_complete", "reasoning_delta", "reasoning_complete", "interaction_started",
  "run_failed", "keep-alive", "tool_config_changed", "tool_scope_changed",
  "user_input", "text_delta", "tool_call_requested", "tool_call", "tool_execution_started",
  "tool_result_received", "tool_execution_completed",
]);

// ============================================================================
// CONSOLE APP
// ============================================================================

export function ConsoleApp({ baseUrl }: ConsoleAppProps): React.JSX.Element {
  // --- Low-frequency React state (UI-driven) ---
  const [experience, setExperience] = React.useState<ConsoleExperience | null>(null);
  const [agents, setAgents] = React.useState<ConsoleAgent[]>([]);
  const [draftByKey, setDraftByKey] = React.useState<Record<string, string>>({});
  const [stagedAttachmentsByIdentity, setStagedAttachmentsByIdentity] = React.useState<Record<string, StagedAttachment[]>>({});
  const [sendingPanels, setSendingPanels] = React.useState<Set<string>>(new Set());
  const [pinnedAgentIds, setPinnedAgentIds] = React.useState<Set<string>>(new Set());
  const [inspectByIdentity, setInspectByIdentity] = React.useState<Record<string, IdentityInspectViewState | null>>({});
  const [routingData, setRoutingData] = React.useState<RoutingPanelData>({ routes: [], deliveries: [] });
  const [gatingData, setGatingData] = React.useState<GatingPanelData>({ pending: [], audit: [] });
  const [activeActivityPresetId, setActiveActivityPresetId] = React.useState("all");
  const [loading, setLoading] = React.useState(true);
  const [error, setError] = React.useState("");
  const [theme, setTheme] = React.useState<ConsoleTheme>(() => {
    try { return (localStorage.getItem("mobkit-console-theme") as ConsoleTheme) || "light"; } catch { return "light"; }
  });
  const [variant, setVariant] = useConsoleVariant();

  const [sidebarCollapsed, setSidebarCollapsed] = React.useState<boolean>(() => {
    try { return localStorage.getItem("mobkit-console-sidebar-collapsed") === "1"; } catch { return false; }
  });
  const toggleSidebarCollapsed = React.useCallback(() => {
    setSidebarCollapsed((c) => {
      const next = !c;
      try { localStorage.setItem("mobkit-console-sidebar-collapsed", next ? "1" : "0"); } catch { /* ignore */ }
      return next;
    });
  }, []);

  const [railCollapsed, setRailCollapsed] = React.useState<boolean>(() => {
    try { return localStorage.getItem("mobkit-console-rail-collapsed") === "1"; } catch { return false; }
  });
  const toggleRailCollapsed = React.useCallback(() => {
    setRailCollapsed((c) => {
      const next = !c;
      try { localStorage.setItem("mobkit-console-rail-collapsed", next ? "1" : "0"); } catch { /* ignore */ }
      return next;
    });
  }, []);

  // --- Render trigger ---
  const [, setRenderTick] = React.useState(0);
  const forceRender = React.useCallback(() => setRenderTick((n) => n + 1), []);
  const stagedAttachmentsRef = React.useRef(stagedAttachmentsByIdentity);
  React.useEffect(() => {
    stagedAttachmentsRef.current = stagedAttachmentsByIdentity;
  }, [stagedAttachmentsByIdentity]);
  React.useEffect(() => () => {
    for (const items of Object.values(stagedAttachmentsRef.current)) {
      items.forEach((item) => URL.revokeObjectURL(item.previewUrl));
    }
  }, []);

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

  function getOrCreateLog(identity: string): IdentityLog {
    let log = identityLogRef.current[identity];
    if (!log) {
      log = {
        events: [],
        byKey: new Map(),
        hasServerLog: null,
        optimisticUser: null,
      };
      identityLogRef.current[identity] = log;
    }
    return log;
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
    if (frame.event === "frame_updated" && frame.data && typeof frame.data === "object") {
      const updated = (frame.data as Record<string, unknown>).frame as ConsoleFrame | undefined;
      if (updated && updated.id) {
        const existingIndex = log.byKey.get(updated.id);
        if (existingIndex !== undefined && log.events[existingIndex]) {
          const existingVersion = log.events[existingIndex].frameVersion ?? 0;
          const updatedVersion = updated.frameVersion ?? existingVersion;
          if (updatedVersion < existingVersion) return false;
          log.events[existingIndex] = { ...log.events[existingIndex], ...updated };
          return true;
        }
      }
      return false;
    }
    const key = frameKey(frame);
    if (log.byKey.has(key)) return false;
    log.byKey.set(key, log.events.length);
    log.events.push(frame);
    if (
      (frame.event === "interaction_started" || frame.event === "user_input")
      && log.optimisticUser
      && log.optimisticUser.interactionId
      && frame.interactionId === log.optimisticUser.interactionId
    ) {
      log.optimisticUser.objectUrls?.forEach((url) => URL.revokeObjectURL(url));
      log.optimisticUser = null;
    }
    return true;
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
  ): void {
    const log = getOrCreateLog(identity);
    log.hasServerLog = available;
    for (const frame of frames) appendFrame(identity, frame);
  }

  /// Render-time chat view: aggregate cursor is the canonical order. The
  /// server admits user input before dispatch, so cursor order preserves
  /// causality without timestamp-only restore drift.
  function getSortedFrames(identity: string): ConsoleFrame[] {
    const log = identityLogRef.current[identity];
    if (!log) return [];
    return log.events
      .map((frame, index) => ({ frame, index }))
      .sort((a, b) => {
        const ca = cursorSeq(a.frame.cursor);
        const cb = cursorSeq(b.frame.cursor);
        if (ca !== null && cb !== null && ca !== cb) return ca - cb;
        const ta = typeof a.frame.timestampMs === "number" ? a.frame.timestampMs : Number.MAX_SAFE_INTEGER;
        const tb = typeof b.frame.timestampMs === "number" ? b.frame.timestampMs : Number.MAX_SAFE_INTEGER;
        if (ta !== tb) return ta - tb;
        return a.index - b.index;
      })
      .map((entry) => entry.frame);
  }

  // Activity rail (global, unchanged)
  const activityRef = React.useRef<ConsoleFrame[]>([]);
  // Unfiltered recent-frames ring for topology-class panels that need to
  // see tool calls (peer-comms send_*, etc.) in addition to interaction
  // lifecycle. The activity rail filters tool events out; this buffer
  // doesn't.
  const liveFramesRef = React.useRef<ConsoleFrame[]>([]);

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
  // states (entering/promoting/trashing/draining) are stripped before
  // persisting so the next reload doesn't render mid-animation rows.
  // ──────────────────────────────────────────────────────────────
  const pendingStackRef = React.useRef<Record<string, PendingItem[]>>({});
  const PENDING_STACK_KEY_PREFIX = "mobkit-pending-stack:";
  const stackKeyFor = (identity: string) => `${PENDING_STACK_KEY_PREFIX}${identity}`;

  function loadPendingStack(identity: string): PendingItem[] {
    try {
      const raw = localStorage.getItem(stackKeyFor(identity));
      if (!raw) return [];
      const parsed = JSON.parse(raw) as unknown;
      if (!Array.isArray(parsed)) return [];
      return parsed
        .filter((it): it is PendingItem => {
          if (!it || typeof it !== "object") return false;
          const r = it as Record<string, unknown>;
          return typeof r.id === "string" && typeof r.text === "string" && typeof r.addedAt === "number";
        })
        .map((it) => ({ id: it.id, text: it.text, addedAt: it.addedAt }));
    } catch { return []; }
  }

  function persistPendingStack(identity: string, items: PendingItem[]) {
    try {
      // Strip transient animation flags before persisting — the next
      // reload would otherwise paint a row mid-fade.
      const clean = items
        .filter((it) => it.status !== "trashing" && it.status !== "draining" && it.status !== "promoting")
        .map((it) => ({ id: it.id, text: it.text, addedAt: it.addedAt }));
      if (clean.length === 0) {
        localStorage.removeItem(stackKeyFor(identity));
      } else {
        localStorage.setItem(stackKeyFor(identity), JSON.stringify(clean));
      }
    } catch { /* quota / private mode — silently degrade */ }
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
      pendingStackRef.current[identity] = loadPendingStack(identity);
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
  const isIdentityBusy = (identity: string) => identityBusyRef.current[identity] === true;

  // Phase tracking (per-panel, unchanged)
  const phaseRef = React.useRef<Record<string, "waiting" | "tool-executing" | "generating" | null>>({});
  const phaseValueByKey = React.useRef<Record<string, "waiting" | "tool-executing" | "generating" | null>>({});
  const phaseSinceByKey = React.useRef<Record<string, number>>({});
  const phaseTimerByKey = React.useRef<Record<string, number>>({});

  // Per-identity refresh debounce timers
  const refreshTimersRef = React.useRef<Record<string, number>>({});

  // Experience refresh debounce
  const experienceTimerRef = React.useRef<number | null>(null);

  // Stable agent ref for async callbacks
  const agentsRef = React.useRef<ConsoleAgent[]>([]);
  React.useEffect(() => { agentsRef.current = agents; }, [agents]);

  const initialTargetOpened = React.useRef(false);

  // =========================================================================
  // DOCK CONTROLLER
  // =========================================================================

  const dock = useConsoleDockController<MobKitDockTarget>({
    createPanelState: ({ target }) => ({
      id: `panel-${crypto.randomUUID()}`,
      target: target || null,
      mode: "console" as const,
    }),
  });

  // =========================================================================
  // PHASE TRACKING (unchanged logic)
  // =========================================================================

  function clearPhaseTimer(panelKey: string) {
    const timer = phaseTimerByKey.current[panelKey];
    if (timer !== undefined) { window.clearTimeout(timer); delete phaseTimerByKey.current[panelKey]; }
  }

  function commitPanelPhase(panelKey: string, phase: "waiting" | "tool-executing" | "generating" | null) {
    clearPhaseTimer(panelKey);
    phaseValueByKey.current[panelKey] = phase;
    phaseSinceByKey.current[panelKey] = Date.now();
    phaseRef.current[panelKey] = phase;
  }

  function schedulePanelPhase(panelKey: string, phase: "waiting" | "tool-executing" | "generating" | null, delayMs: number) {
    clearPhaseTimer(panelKey);
    phaseTimerByKey.current[panelKey] = window.setTimeout(() => {
      delete phaseTimerByKey.current[panelKey];
      phaseValueByKey.current[panelKey] = phase;
      phaseSinceByKey.current[panelKey] = Date.now();
      phaseRef.current[panelKey] = phase;
      forceRender();
    }, delayMs);
  }

  function updatePanelPhaseFromFrame(panelKey: string, frame: ConsoleFrame) {
    const currentPhase = phaseValueByKey.current[panelKey] ?? null;
    const elapsedMs = Date.now() - (phaseSinceByKey.current[panelKey] ?? 0);
    switch (frame.event) {
      case "interaction_started":
        commitPanelPhase(panelKey, "waiting"); break;
      case "tool_call_requested": case "tool_call": case "tool_execution_started":
      case "tool_result_received": case "tool_execution_completed":
        if (currentPhase === "waiting" && elapsedMs < 300) { schedulePanelPhase(panelKey, "tool-executing", 300 - elapsedMs); break; }
        commitPanelPhase(panelKey, "tool-executing"); break;
      case "text_delta": {
        if (currentPhase === "tool-executing") { const r = Math.max(0, 300 - elapsedMs); if (r > 0) { schedulePanelPhase(panelKey, "generating", r); break; } }
        if (currentPhase === "waiting" && elapsedMs < 300) { schedulePanelPhase(panelKey, "generating", 300 - elapsedMs); break; }
        commitPanelPhase(panelKey, "generating"); break;
      }
      case "interaction_complete": case "interaction_failed": case "run_completed": case "run_failed":
        commitPanelPhase(panelKey, null); break;
      default: break;
    }
  }

  // The SSE handler runs from inside an effect with `[baseUrl]` deps so its
  // closure captures `dock` from the first render — when panels[] was empty.
  // Route panel-iterating phase updates through a ref so they always see the
  // current panel set; otherwise interaction_started/text_delta/
  // interaction_complete arrive at panel:none and the typing indicator
  // sticks at "waiting" indefinitely (and the "still busy" perception breaks
  // the pending-stack auto-queue, which depends on `identityBusyRef`).
  const dockRef = React.useRef(dock);
  dockRef.current = dock;

  // Helper: update phase for ALL panels showing a given identity
  function updatePhaseForIdentity(identity: string, frame: ConsoleFrame) {
    for (const panel of dockRef.current.viewState.panels) {
      const target = panel.target as MobKitDockTarget | null;
      if (!target || target.kind !== "agent-chat") continue;
      if ((target.identity || target.memberId) !== identity) continue;
      updatePanelPhaseFromFrame(buildPanelConversationKey(panel.id, target), frame);
    }
  }

  // Helper: clear phase for all panels showing a given identity
  function clearPhaseForIdentity(identity: string) {
    for (const panel of dockRef.current.viewState.panels) {
      const target = panel.target as MobKitDockTarget | null;
      if (!target || target.kind !== "agent-chat") continue;
      if ((target.identity || target.memberId) !== identity) continue;
      commitPanelPhase(buildPanelConversationKey(panel.id, target), null);
    }
  }

  // =========================================================================
  // LOAD EXPERIENCE
  // =========================================================================

  const loadExperience = React.useCallback(async () => {
    const [experienceJson, modulesJson] = await Promise.all([
      fetchJson<ConsoleExperience>(baseUrl, "/console/experience"),
      fetchJson<ConsoleModulesResponse>(baseUrl, "/console/modules"),
    ]);
    const loadedModules = Array.isArray(modulesJson.modules) ? modulesJson.modules.map(String) : [];
    setExperience(experienceJson);
    setAgents(normalizeAgents(experienceJson, loadedModules));
    setActiveActivityPresetId((c) => c || experienceJson.activity_feed?.active_preset_id || "all");
  }, [baseUrl]);

  React.useEffect(() => {
    let mounted = true;
    setLoading(true); setError("");
    void loadExperience()
      .catch((e) => { if (mounted) setError(errorMessage(e)); })
      .finally(() => { if (mounted) setLoading(false); });
    return () => { mounted = false; };
  }, [loadExperience]);

  // =========================================================================
  // OPEN FIRST AGENT
  // =========================================================================

  React.useEffect(() => {
    if (initialTargetOpened.current || dock.focusedTarget || agents.length === 0) return;
    const first = agents.find((a) => a.addressable || a.affordances?.can_send_message) || agents[0];
    if (!first) return;
    initialTargetOpened.current = true;
    dock.openTarget(buildDockTarget(first), "replace_focused");
  }, [agents, dock]);

  const hasMobControlSurface = experience?.runtime_id !== "console-aggregator";
  const visibleControls = React.useMemo<NavKind[]>(
    () => hasMobControlSurface
      ? ["topology", "timeline", "gating", "roster", "routing", "gates", "logs", "health"]
      : ["topology", "timeline", "roster", "logs", "health"],
    [hasMobControlSurface],
  );

  // =========================================================================
  // REFRESH PANEL DATA (inspect, routing, gating)
  // =========================================================================

  const refreshPanelData = React.useCallback(async () => {
    const openPanels = dock.viewState.panels.map((p) => p.target).filter(Boolean) as MobKitDockTarget[];
    const inspects = openPanels.filter((t): t is Extract<MobKitDockTarget, { kind: "identity-inspect" }> => t.kind === "identity-inspect");
    if (inspects.length) {
      const entries = await Promise.all(inspects.map(async (t) => {
        const r = await callConsoleRpc<unknown>(baseUrl, "mobkit/console/inspect_identity", { identity: t.identity })
          .catch(() => callConsoleRpc<unknown>(baseUrl, "mobkit/inspect_identity", { identity: t.identity }));
        return [t.identity, normalizeConsoleInspectResult(r)] as const;
      }));
      setInspectByIdentity((c) => ({ ...c, ...Object.fromEntries(entries) }));
    }
    if (hasMobControlSurface && openPanels.some((t) => t.kind === "routing")) {
      const [routes, history] = await Promise.all([
        callConsoleRpc(baseUrl, "mobkit/routing/routes/list", {}),
        callConsoleRpc(baseUrl, "mobkit/delivery/history", {}),
      ]);
      setRoutingData(buildRoutingSectionView({ routesResponse: routes, historyResponse: history }));
    }
    if (hasMobControlSurface && openPanels.some((t) => t.kind === "gating")) {
      const [p, a] = await Promise.all([
        callConsoleRpc<{ pending?: unknown[] }>(baseUrl, "mobkit/gating/pending", {}),
        callConsoleRpc<{ entries?: unknown[] }>(baseUrl, "mobkit/gating/audit", { limit: 50 }),
      ]);
      setGatingData({ pending: Array.isArray(p.pending) ? p.pending : [], audit: Array.isArray(a.entries) ? a.entries : [] });
    }
  }, [baseUrl, dock.viewState.panels, hasMobControlSurface]);

  React.useEffect(() => { void refreshPanelData().catch(() => {}); }, [dock.viewState.panels, refreshPanelData]);

  const scheduleExperienceRefresh = React.useCallback(() => {
    if (experienceTimerRef.current !== null) return;
    experienceTimerRef.current = window.setTimeout(async () => {
      experienceTimerRef.current = null;
      await loadExperience().catch(() => {});
      await refreshPanelData().catch(() => {});
    }, 500);
  }, [loadExperience, refreshPanelData]);

  // =========================================================================
  // HISTORY REFRESH — server is the single source of truth
  // =========================================================================

  const scheduleHistoryRefresh = React.useCallback((identity: string) => {
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
        const { frames, available } = await queryTimeline(baseUrl, { identity }, 400);
        reconcileServerLog(identity, frames, available);
        clearPhaseForIdentity(identity);
        forceRender();
      } catch { /* silent — will retry on next terminal event */ }
    }, 200);
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [baseUrl, forceRender]);

  // =========================================================================
  // PANEL OPEN / SWITCH — fetch history for new identities
  // =========================================================================

  React.useEffect(() => {
    for (const panel of dock.viewState.panels) {
      const target = panel.target as MobKitDockTarget | null;
      if (!target || target.kind !== "agent-chat") continue;
      const identity = target.identity || target.memberId;
      const log = getOrCreateLog(identity);
      // Only fetch backfill once per identity — when we don't yet
      // know whether the runtime has an event log. Subsequent panel
      // re-opens reuse the existing log; we never wipe it.
      if (log.hasServerLog !== null) continue;
      void (async () => {
        try {
          const { frames, available } = await queryTimeline(baseUrl, { identity }, 400);
          reconcileServerLog(identity, frames, available);
          forceRender();
        } catch { /* silent */ }
      })();
    }
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

  React.useEffect(() => {
    // Seed activity with recent history (only on mount) — apply same
    // filter as SSE. The activity rail is its own concern; it doesn't
    // share state with the per-identity logs.
    void queryTimeline(baseUrl, {}, 200)
      .then(({ frames }) => {
        const seen = new Set<string>();
        const filtered: ConsoleFrame[] = [];
        for (const frame of frames) {
          if (ACTIVITY_SKIP_EVENTS.has(frame.event)) continue;
          const key = frame.id || `${frame.event}:${frame.timestampMs || 0}`;
          if (seen.has(key)) continue;
          seen.add(key);
          filtered.push(frame);
        }
        activityRef.current = filtered.slice(-200).reverse();
        forceRender();
      })
      .catch(() => {});

    const unsubscribe = subscribeTimelineEvents(baseUrl, {}, (frame) => {
      // Activity rail (independent buffer)
      if (!ACTIVITY_SKIP_EVENTS.has(frame.event)) {
        activityRef.current = [frame, ...activityRef.current].slice(0, 200);
      }

      // Topology-class buffer keeps tool events (peer-comms etc.) which
      // the activity rail filters out. Capped at 300; older frames roll
      // off naturally as live pulses age past their lifetime.
      if (PANEL_ROUTABLE_EVENTS.has(frame.event)) {
        liveFramesRef.current = [frame, ...liveFramesRef.current].slice(0, 300);
      }

      // Identity log (single canonical store)
      const identity = frame.identity?.trim();
      if (PANEL_ROUTABLE_EVENTS.has(frame.event) && identity && identity !== "_system") {
        appendFrame(identity, frame);
        updatePhaseForIdentity(identity, frame);

        // Per-identity busy tracking. Used by the pending-stack
        // auto-drain hook + by the Send handler to decide whether
        // to bypass the stack (idle + empty) or push to it.
        const wasBusy = identityBusyRef.current[identity] === true;
        if (frame.event === "user_input" || frame.event === "interaction_started" || frame.event === "run_started") {
          identityBusyRef.current[identity] = true;
        } else if (
          frame.event === "interaction_complete"
          || frame.event === "interaction_failed"
          || frame.event === "run_completed"
          || frame.event === "run_failed"
          || frame.event === "message_delivery_failed"
        ) {
          identityBusyRef.current[identity] = false;
          // busy → idle transition: drain the head item if the
          // user has stacked something while we were busy.
          if (wasBusy) maybeDrainHead(identity);
        }
      }

      forceRender();

      // Terminal events → reconcile server backfill (idempotent — keys
      // already seen via SSE are skipped). If hasServerLog is false,
      // scheduleHistoryRefresh short-circuits.
      if (HISTORY_REFRESH_EVENTS.has(frame.event) && identity && identity !== "_system") {
        scheduleHistoryRefreshRef.current(identity);
      }
      if (REFRESH_TRIGGER_EVENTS.has(frame.event)) {
        scheduleExperienceRefreshRef.current();
      }
    });

    return () => { unsubscribe(); };
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [baseUrl]);

  // Timer cleanup on unmount
  React.useEffect(() => {
    return () => {
      for (const timer of Object.values(phaseTimerByKey.current)) window.clearTimeout(timer);
      for (const timer of Object.values(refreshTimersRef.current)) window.clearTimeout(timer);
      if (experienceTimerRef.current !== null) window.clearTimeout(experienceTimerRef.current);
    };
  }, []);

  // =========================================================================
  // AGENT SELECTION
  // =========================================================================

  function onSelectAgent(_block: unknown, _section: unknown, item: { id: string }) {
    const agent = agents.find((c) => c.member_id === item.id);
    if (agent) dock.openTarget(buildDockTarget(agent), "replace_focused");
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
    const panelKey = buildPanelConversationKey(panelId, target);
    const identity = target.identity || target.memberId;

    const optimisticObjectUrls = attachments.map((file) => URL.createObjectURL(file));
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
    log.optimisticUser = {
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
    commitPanelPhase(panelKey, "waiting");
    identityBusyRef.current[identity] = true;
    forceRender();

    try {
      const id = target.identity?.trim();
      if (attachments.length > 0 && id) {
        const result = await sendConsoleMultipart(
          baseUrl,
          id,
          text,
          attachments,
          `console:${panelId}`,
          createIdempotencyKey(),
          handlingMode,
        );
        if (log.optimisticUser) {
          log.optimisticUser.interactionId = result.interaction_id;
          const matched = log.events.some(
            (f) => (f.event === "interaction_started" || f.event === "user_input") && f.interactionId === result.interaction_id,
          );
          if (matched) {
            log.optimisticUser.objectUrls?.forEach((url) => URL.revokeObjectURL(url));
            log.optimisticUser = null;
          }
        }
      } else if (attachments.length > 0) {
        const result = await sendMessageMultipart(baseUrl, target.memberId, text, attachments, handlingMode);
        if (log.optimisticUser) {
          log.optimisticUser.interactionId = result.interaction_id || "";
          const matched = result.interaction_id
            ? log.events.some(
                (f) => f.event === "interaction_started" && f.interactionId === result.interaction_id,
              )
            : false;
          if (matched) {
            log.optimisticUser.objectUrls?.forEach((url) => URL.revokeObjectURL(url));
            log.optimisticUser = null;
          }
        }
      } else if (id) {
        const result = await sendConsole(
          baseUrl,
          id,
          text,
          `console:${panelId}`,
          createIdempotencyKey(),
          handlingMode,
        );
        if (log.optimisticUser) {
          log.optimisticUser.interactionId = result.interaction_id;
          // The interaction_started frame may have arrived between
          // the send and the RPC response — reconcile retroactively.
          const matched = log.events.some(
            (f) => (f.event === "interaction_started" || f.event === "user_input") && f.interactionId === result.interaction_id,
          );
          if (matched) {
            log.optimisticUser.objectUrls?.forEach((url) => URL.revokeObjectURL(url));
            log.optimisticUser = null;
          }
        }
      } else {
        await sendMessage(baseUrl, target.memberId, text, handlingMode);
      }
      return true;
    } catch (submitError) {
      log.optimisticUser?.objectUrls?.forEach((url) => URL.revokeObjectURL(url));
      log.optimisticUser = null;
      commitPanelPhase(panelKey, null);
      identityBusyRef.current[identity] = false;
      setError(errorMessage(submitError));
      forceRender();
      return false;
    } finally {
      setSendingPanels((c) => { const n = new Set(c); n.delete(panelKey); return n; });
    }
  }

  async function onSendMessage(panelId: string, target: MobKitDockTarget | null, attachments: File[] = []): Promise<boolean> {
    if (!target || target.kind !== "agent-chat") return false;
    const panelKey = buildPanelConversationKey(panelId, target);
    const identity = target.identity || target.memberId;
    const text = (draftByKey[panelKey] || "").trim();
    if (!text && attachments.length === 0) return false;

    const stack = getPendingStack(identity);
    const shouldQueue = isIdentityBusy(identity) || stack.length > 0;

    if (!shouldQueue || attachments.length > 0) {
      // Idle + empty stack: bypass straight to the wire.
      const sent = await submitMessageNow(panelId, target, text, "queue", attachments);
      if (sent) setDraftByKey((c) => ({ ...c, [panelKey]: "" }));
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
    setDraftByKey((c) => ({ ...c, [panelKey]: "" }));
    window.setTimeout(() => {
      setPendingStack(identity, (prev) =>
        prev.map((it) => (it.id === newId && it.status === "entering" ? { ...it, status: null } : it)),
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
  const reducedMotion = typeof window !== "undefined"
    ? window.matchMedia?.("(prefers-reduced-motion: reduce)").matches ?? false
    : false;
  const animMs = (ms: number) => reducedMotion ? 0 : ms;

  function findChatTargetFor(identity: string): { panelId: string; target: MobKitDockTarget } | null {
    for (const panel of dock.viewState.panels) {
      const t = panel.target as MobKitDockTarget | null;
      if (!t || t.kind !== "agent-chat") continue;
      if ((t.identity || t.memberId) === identity) {
        return { panelId: panel.id, target: t };
      }
    }
    return null;
  }

  function onStackSteer(identity: string, id: string) {
    setPendingStack(identity, (prev) =>
      prev.map((it) => (it.id === id ? { ...it, status: "promoting", editing: false } : it)),
    );
    window.setTimeout(() => {
      const stack = getPendingStack(identity);
      const item = stack.find((it) => it.id === id);
      if (!item) return;
      setPendingStack(identity, (prev) => prev.filter((it) => it.id !== id));
      const target = findChatTargetFor(identity);
      if (target) {
        void submitMessageNow(target.panelId, target.target, item.text, "steer");
      }
    }, animMs(360));
  }

  function onStackTrash(identity: string, id: string) {
    setPendingStack(identity, (prev) =>
      prev.map((it) => (it.id === id ? { ...it, status: "trashing", editing: false } : it)),
    );
    window.setTimeout(() => {
      setPendingStack(identity, (prev) => prev.filter((it) => it.id !== id));
    }, animMs(320));
  }

  function onStackEdit(identity: string, id: string) {
    setPendingStack(identity, (prev) =>
      prev.map((it) => (it.id === id ? { ...it, editing: true } : { ...it, editing: false })),
    );
  }

  function onStackCommitEdit(identity: string, id: string, text: string) {
    const trimmed = text.trim();
    if (!trimmed) return;
    setPendingStack(identity, (prev) =>
      prev.map((it) => (it.id === id ? { ...it, text: trimmed, editing: false, addedAt: Date.now() } : it)),
    );
  }

  function onStackCancelEdit(identity: string, id: string) {
    setPendingStack(identity, (prev) =>
      prev.map((it) => (it.id === id ? { ...it, editing: false } : it)),
    );
  }

  function onStackReorder(identity: string, dragId: string, dropId: string, where: "above" | "below") {
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
  /// then submits via `submitMessageNow` with mode `Queue`. The next
  /// `interaction_started` will flip identityBusyRef back to true,
  /// so the remaining queue waits for the next idle window.
  function maybeDrainHead(identity: string) {
    const stack = getPendingStack(identity);
    if (stack.length === 0) return;
    // Only drain if no item is already mid-drain or mid-promotion.
    if (stack.some((it) => it.status === "draining" || it.status === "promoting")) return;
    const head = stack.find((it) => !it.status || it.status === "entering");
    if (!head) return;
    setPendingStack(identity, (prev) =>
      prev.map((it) => (it.id === head.id ? { ...it, status: "draining" } : it)),
    );
    window.setTimeout(() => {
      setPendingStack(identity, (prev) => prev.filter((it) => it.id !== head.id));
      const target = findChatTargetFor(identity);
      if (target) {
        void submitMessageNow(target.panelId, target.target, head.text, "queue");
      }
    }, animMs(420));
  }

  // =========================================================================
  // LIFECYCLE ACTIONS
  // =========================================================================

  async function onLifecycleAction(identity: string, method: "mobkit/retire" | "mobkit/respawn" | "mobkit/reset") {
    await callConsoleRpc(baseUrl, method, { identity });
    await loadExperience();
  }

  async function onGatingDecision(pendingId: string, decision: "approve" | "reject" | "escalate") {
    await callConsoleRpc<unknown>(baseUrl, "mobkit/gating/decide", {
      pending_id: pendingId, approver_id: DEFAULT_APPROVER_ID, decision, reason: `console_${decision}`,
    } as ConsoleGatingActionPayload);
    const [p, a] = await Promise.all([
      callConsoleRpc<{ pending?: unknown[] }>(baseUrl, "mobkit/gating/pending", {}),
      callConsoleRpc<{ entries?: unknown[] }>(baseUrl, "mobkit/gating/audit", { limit: 50 }),
    ]);
    setGatingData({ pending: Array.isArray(p.pending) ? p.pending : [], audit: Array.isArray(a.entries) ? a.entries : [] });
  }

  // =========================================================================
  // RESIZE HANDLERS (unchanged)
  // =========================================================================

  const SIDEBAR_MIN = 180, SIDEBAR_MAX = 420;
  function handleSidebarResize(event: React.PointerEvent<HTMLDivElement>) {
    event.preventDefault();
    const startX = event.clientX;
    const root = event.currentTarget.closest("[data-console-workbench]") as HTMLElement | null;
    if (!root) return;
    const startWidth = parseInt(getComputedStyle(root).getPropertyValue("--cc-workbench-sidebar-width") || "260", 10) || 260;
    const handle = event.currentTarget;
    if ("setPointerCapture" in handle) handle.setPointerCapture(event.pointerId);
    document.documentElement.setAttribute("data-cc-resizing", "true");
    function onPointerMove(e: PointerEvent) { root!.style.setProperty("--cc-workbench-sidebar-width", `${Math.min(SIDEBAR_MAX, Math.max(SIDEBAR_MIN, startWidth + (e.clientX - startX)))}px`); }
    function cleanup() { document.documentElement.removeAttribute("data-cc-resizing"); window.removeEventListener("pointermove", onPointerMove); window.removeEventListener("pointerup", cleanup); window.removeEventListener("pointercancel", cleanup); if ("hasPointerCapture" in handle && handle.hasPointerCapture(event.pointerId)) handle.releasePointerCapture(event.pointerId); }
    window.addEventListener("pointermove", onPointerMove); window.addEventListener("pointerup", cleanup); window.addEventListener("pointercancel", cleanup);
  }

  const ACTIVITY_MIN = 200, ACTIVITY_MAX = 480;
  function handleActivityResize(event: React.PointerEvent<HTMLDivElement>) {
    event.preventDefault();
    const startX = event.clientX;
    const root = event.currentTarget.closest("[data-console-workbench]") as HTMLElement | null;
    if (!root) return;
    const startWidth = parseInt(getComputedStyle(root).getPropertyValue("--cc-workbench-activity-width") || "280", 10) || 280;
    const handle = event.currentTarget;
    if ("setPointerCapture" in handle) handle.setPointerCapture(event.pointerId);
    document.documentElement.setAttribute("data-cc-resizing", "true");
    function onPointerMove(e: PointerEvent) { root!.style.setProperty("--cc-workbench-activity-width", `${Math.min(ACTIVITY_MAX, Math.max(ACTIVITY_MIN, startWidth - (e.clientX - startX)))}px`); }
    function cleanup() { document.documentElement.removeAttribute("data-cc-resizing"); window.removeEventListener("pointermove", onPointerMove); window.removeEventListener("pointerup", cleanup); window.removeEventListener("pointercancel", cleanup); if ("hasPointerCapture" in handle && handle.hasPointerCapture(event.pointerId)) handle.releasePointerCapture(event.pointerId); }
    window.addEventListener("pointermove", onPointerMove); window.addEventListener("pointerup", cleanup); window.addEventListener("pointercancel", cleanup);
  }

  // =========================================================================
  // RENDER GUARDS
  // =========================================================================

  if (loading) return <div data-testid="console-loading">Loading console...</div>;
  if (error) return <div data-testid="console-error">{error}</div>;

  // =========================================================================
  // BUILD VIEW STATES
  // =========================================================================

  const focusedMemberId = dock.focusedTarget?.kind === "agent-chat" ? dock.focusedTarget.memberId : "";
  const sidebarVS = buildSidebarViewState({ agents, selectedMemberId: focusedMemberId, pinnedAgentIds });
  const activityVS = buildActivityRailViewState({
    agents,
    eventFrames: activityRef.current,
    filterPresets: experience?.activity_feed?.filter_presets,
    activePresetId: activeActivityPresetId,
  });

  // =========================================================================
  // RENDER: CHAT PANEL — reads from 3 identity-keyed refs
  // =========================================================================

  function renderChatPanel(panel: { id: string; target?: MobKitDockTarget | null }) {
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
    const sortedFrames = getSortedFrames(identity);
    const conversationEntries = mapFramesToTimelineEntries(agent, sortedFrames, {
      renderInteractionStartsAsUser: true,
      renderTextDeltas: true,
      blobBaseUrl: baseUrl,
    });

    // Optimistic user message: rendered until an interaction_started
    // with the matching interaction_id is appended to the log (which
    // clears it via appendFrame). Until then, it sits at the tail of
    // the conversation as a synthetic entry.
    const log = getOrCreateLog(identity);
    const optimisticEntry = log.optimisticUser ? log.optimisticUser.entry : null;

    const entries = sanitizeConversationEntries(sortConversationTimelineEntries([
      ...conversationEntries,
      ...(optimisticEntry ? [optimisticEntry] : []),
    ]));

    const conversation = buildConversationViewState({
      memberId: target.memberId,
      agentLabel: target.title,
      agent,
      entries,
    });
    const draft = draftByKey[panelKey] || "";
    const staged = stagedAttachmentsByIdentity[identity] ?? [];
    const isSending = sendingPanels.has(panelKey);
    const phase = Object.prototype.hasOwnProperty.call(phaseRef.current, panelKey)
      ? phaseRef.current[panelKey]
      : agent?.response_phase ?? null;
    const canRespawn = agent?.affordances?.can_respawn === true;
    const canRetire = agent?.affordances?.can_retire === true;

    const quickPrompts = buildQuickPromptSuggestions(agent).map((s) => ({
      id: s.id, kind: "pill" as const, label: s.label, iconName: s.iconName || "i-bolt",
    }));
    const footerLeftItems: ConsoleComposerToolbarItem[] = [
      { id: "target", kind: "sub-pill", label: `To: ${target.title}`, iconName: "i-team" },
      { id: "identity", kind: "sub-pill", label: target.identity || target.memberId, iconName: "i-terminal" },
    ];
    const footerRightItems: ConsoleComposerToolbarItem[] = [
      ...(agent?.role ? [{ id: "role", kind: "sub-pill" as const, label: agent.role }] : []),
      ...(phase ? [{ id: "phase", kind: "sub-pill" as const, label: phase, iconName: "i-bolt" }] : []),
      { id: "state", kind: "sub-pill" as const, label: agent?.state || "unknown", iconName: "i-dot" },
    ];

    const stackItems = getPendingStack(identity);
    const agentBusy = isIdentityBusy(identity);
    const stackSlot = stackItems.length > 0 ? (
      <PendingStack
        items={stackItems}
        agentBusy={agentBusy}
        reducedMotion={reducedMotion}
        onSteer={(itemId) => onStackSteer(identity, itemId)}
        onTrash={(itemId) => onStackTrash(identity, itemId)}
        onEdit={(itemId) => onStackEdit(identity, itemId)}
        onCommitEdit={(itemId, t) => onStackCommitEdit(identity, itemId, t)}
        onCancelEdit={(itemId) => onStackCancelEdit(identity, itemId)}
        onReorder={(dragId, dropId, where) => onStackReorder(identity, dragId, dropId, where)}
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
        draft={draft}
        sending={isSending}
        staged={staged}
        onDraftChange={(v) => setDraftByKey((c) => ({ ...c, [panelKey]: v }))}
        onStagedChange={(action) => setStagedAttachmentsForIdentity(identity, action)}
        onSend={(attachments) => onSendMessage(panel.id, target, attachments)}
        onInspect={() => { if (agent) dock.openTarget(buildInspectTarget(agent), "new_tab"); }}
        onRespawn={canRespawn ? () => void onLifecycleAction(identity, "mobkit/respawn") : undefined}
        onRetire={canRetire ? () => void onLifecycleAction(identity, "mobkit/retire") : undefined}
        stackSlot={stackSlot}
      />
    );
  }

  // =========================================================================
  // RENDER: CONTROL PANELS (unchanged)
  // =========================================================================

  function renderInspectPanel(target: Extract<MobKitDockTarget, { kind: "identity-inspect" }>) {
    const inspect = inspectByIdentity[target.identity];
    const agent = agents.find((candidate) => candidate.identity === target.identity || candidate.member_id === target.identity);
    const canRespawn = agent?.affordances?.can_respawn === true;
    const canRetire = agent?.affordances?.can_retire === true;
    const canReset = experience?.runtime_capabilities?.can_retire_members === true;
    return (
      <div className="console-panel" data-testid={`inspect-panel:${target.identity}`}>
        <div className="console-panel__header">
          <h3>{target.identity}</h3>
          <div className="console-panel__actions">
            {canRespawn ? <button data-testid={`inspect-action:${target.identity}:respawn`} type="button" onClick={() => void onLifecycleAction(target.identity, "mobkit/respawn")}>Respawn</button> : null}
            {canReset ? <button data-testid={`inspect-action:${target.identity}:reset`} type="button" onClick={() => void onLifecycleAction(target.identity, "mobkit/reset")}>Reset</button> : null}
            {canRetire ? <button data-testid={`inspect-action:${target.identity}:retire`} type="button" onClick={() => void onLifecycleAction(target.identity, "mobkit/retire")}>Retire</button> : null}
          </div>
        </div>
        {!inspect ? <p>Loading identity details…</p> : (
          <dl className="console-panel__grid">
            <dt>State</dt><dd>{inspect.state}</dd>
            <dt>Role</dt><dd>{inspect.role || "n/a"}</dd>
            <dt>Addressability</dt><dd>{inspect.addressability}</dd>
            <dt>Generation</dt><dd>{inspect.continuity?.generation ?? "n/a"}</dd>
            <dt>Checkpoint</dt><dd>{inspect.continuity?.checkpoint_version ?? "n/a"}</dd>
            <dt>Session</dt><dd>{inspect.continuity?.session_id || "n/a"}</dd>
            <dt>Runtime</dt><dd>{inspect.continuity?.agent_runtime_id || "n/a"}</dd>
            <dt>Lease Healthy</dt><dd>{String(inspect.lease_healthy ?? inspect.lease?.healthy ?? false)}</dd>
            <dt>Peers</dt><dd>{inspect.topology_peers?.join(", ") || "none"}</dd>
            <dt>Output Preview</dt><dd>{inspect.output_preview || "n/a"}</dd>
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
              <strong>{r.display_name || r.identity}</strong> · {r.state} · {r.addressability}
            </li>
          ))}
        </ul>
      </div>
    );
  }

  function handleInspectAgent(agent: ConsoleAgent) {
    dock.openTarget(buildInspectTarget(agent), "new_tab");
  }

  // =========================================================================
  // MAIN RENDER
  // =========================================================================

  const mobName = experience?.agent_sidebar?.title || "mob";
  const mobStatus = experience?.health_overview?.live_snapshot?.running === false ? "stopped" : "running";

  function toggleTheme() {
    const next: ConsoleTheme = theme === "dark" ? "light" : "dark";
    setTheme(next);
    try { localStorage.setItem("mobkit-console-theme", next); } catch { /* ignore */ }
  }

  function renderPanelBody(panel: { id: string; target?: MobKitDockTarget | null }) {
    const target = panel.target as MobKitDockTarget | null;
    if (!target) return <div className="console-panel">No panel target</div>;
    if (target.kind === "agent-chat") return renderChatPanel(panel);
    if (target.kind === "identity-inspect") return renderInspectPanel(target);
    if ((target.kind === "routing" || target.kind === "gating" || target.kind === "gates") && !hasMobControlSurface) {
      return <div className="console-panel">This view requires a mob runtime control surface.</div>;
    }
    if (target.kind === "routing") return <RoutingPanel data={routingData} />;
    if (target.kind === "gating") return (
      <GatingInboxPanel
        pending={gatingData.pending}
        audit={gatingData.audit}
        onDecide={(pid, decision) => void onGatingDecision(pid, decision)}
      />
    );
    if (target.kind === "topology") return (
      <TopologyPanel
        nodes={experience?.topology?.live_snapshot?.nodes || []}
        agents={agents}
        activity={liveFramesRef.current}
      />
    );
    if (target.kind === "health") return renderHealthPanel(experience?.health_overview?.live_snapshot?.identities || []);
    if (target.kind === "timeline") return <TimelinePanel frames={activityRef.current} />;
    if (target.kind === "roster") return (
      <RosterPanel
        agents={agents}
        onSelect={(a) => dock.openTarget(buildDockTarget(a), "replace_focused")}
        onInspect={handleInspectAgent}
        onLifecycle={(identity, method) => void onLifecycleAction(identity, method)}
        canResetLifecycle={hasMobControlSurface}
      />
    );
    if (target.kind === "gates") return <GatesPanel audit={gatingData.audit} />;
    if (target.kind === "logs") return <LogsPanel frames={activityRef.current} />;
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
      <Topbar
        mobName={mobName}
        mobStatus={mobStatus}
        theme={theme}
        onToggleTheme={toggleTheme}
        sidebarCollapsed={sidebarCollapsed}
        railCollapsed={railCollapsed}
        onToggleSidebar={toggleSidebarCollapsed}
        onToggleRail={toggleRailCollapsed}
      />
      <div
        className="shell"
        data-sidebar-collapsed={sidebarCollapsed ? "true" : "false"}
        data-rail-collapsed={railCollapsed ? "true" : "false"}
      >
        <DesignSidebar
          agents={agents}
          selectedMemberId={focusedMemberId}
          recentActivity={activityRef.current}
          collapsed={sidebarCollapsed}
          visibleControls={visibleControls}
          onSelect={(a) => dock.openTarget(buildDockTarget(a), "replace_focused")}
          onInspect={(a) => dock.openTarget(buildInspectTarget(a), "replace_focused")}
          onOpenControl={(kind) => {
            if (!visibleControls.includes(kind)) return;
            dock.openTarget(buildControlTarget(kind), "replace_focused");
          }}
        />
        <div className="pane-resizer" aria-hidden="true" data-testid="resize:sidebar" onPointerDown={handleSidebarResize} />
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
              dock.openTarget(target, "replace_focused");
            }}
          />
        </div>
        <div className="pane-resizer pane-resizer--activity" aria-hidden="true" data-testid="resize:activity" onPointerDown={handleActivityResize} />
        <SignalsRail
          frames={activityRef.current}
          collapsed={railCollapsed}
        />
      </div>
    </div>
  );
}
