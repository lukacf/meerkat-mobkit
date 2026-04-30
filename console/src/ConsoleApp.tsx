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
  type MobKitDockTarget,
} from "./lib/adapters";
import { errorMessage } from "./lib/errors";
import {
  callConsoleRpc,
  fetchJson,
  queryEvents,
  sendInteract as sendInteractRpc,
  sendMessage,
  subscribeConsoleEvents,
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
import { Tweaks, useConsoleVariant, type ConsoleTheme } from "./panels/Tweaks";
import { Sidebar as DesignSidebar } from "./panels/Sidebar";
import { SignalsRail } from "./panels/SignalsRail";
import { ChatPane } from "./panels/ChatPane";
import { MobKitDock } from "./panels/MobKitDock";

interface ConsoleAppProps {
  baseUrl: string;
}

type RoutingPanelData = ReturnType<typeof buildRoutingSectionView>;
type GatingPanelData = { pending: unknown[]; audit: unknown[] };

interface OptimisticUserMessage {
  interactionId: string;
  entry: ConversationTimelineEntry;
  sentAtMs: number;
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

const DEFAULT_APPROVER_ID = "console-ops-lead";

// --- Event sets for the SSE handler ---
const REFRESH_TRIGGER_EVENTS = new Set([
  "interaction_complete", "interaction_failed", "state_changed",
  "member_ready", "member_retired", "gating_decision", "route_changed",
]);
const PANEL_ROUTABLE_EVENTS = new Set([
  "interaction_started", "interaction_complete", "interaction_failed",
  "text_delta", "text_complete",
  "tool_call_requested", "tool_call", "tool_result_received",
  "tool_execution_started", "tool_execution_completed",
  "run_started", "run_completed", "run_failed",
]);
const HISTORY_REFRESH_EVENTS = new Set([
  "interaction_complete", "interaction_failed", "run_completed", "run_failed",
]);
// Events filtered from the activity rail — don't buffer them
const ACTIVITY_SKIP_EVENTS = new Set([
  "subscribed", "run_started", "run_completed", "turn_started", "turn_completed",
  "text_complete", "reasoning_delta", "reasoning_complete", "interaction_started",
  "run_failed", "keep-alive", "tool_config_changed", "tool_scope_changed",
  "text_delta", "tool_call_requested", "tool_call", "tool_execution_started",
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
    const key = frameKey(frame);
    if (log.byKey.has(key)) return false;
    log.byKey.set(key, log.events.length);
    log.events.push(frame);
    if (
      frame.event === "interaction_started"
      && log.optimisticUser
      && log.optimisticUser.interactionId
      && frame.interactionId === log.optimisticUser.interactionId
    ) {
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

  /// Render-time view: frames sorted ascending by `timestampMs`, with
  /// insertion order as a stable tiebreaker (preserves intra-tick
  /// ordering as the server emitted them).
  function getSortedFrames(identity: string): ConsoleFrame[] {
    const log = identityLogRef.current[identity];
    if (!log) return [];
    return log.events
      .map((frame, index) => ({ frame, index }))
      .sort((a, b) => {
        const ta = a.frame.timestampMs || 0;
        const tb = b.frame.timestampMs || 0;
        if (ta !== tb) return ta - tb;
        return a.index - b.index;
      })
      .map((entry) => entry.frame);
  }

  // Activity rail (global, unchanged)
  const activityRef = React.useRef<ConsoleFrame[]>([]);

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

  // Helper: update phase for ALL panels showing a given identity
  function updatePhaseForIdentity(identity: string, frame: ConsoleFrame) {
    for (const panel of dock.viewState.panels) {
      const target = panel.target as MobKitDockTarget | null;
      if (!target || target.kind !== "agent-chat") continue;
      if ((target.identity || target.memberId) !== identity) continue;
      updatePanelPhaseFromFrame(buildPanelConversationKey(panel.id, target), frame);
    }
  }

  // Helper: clear phase for all panels showing a given identity
  function clearPhaseForIdentity(identity: string) {
    for (const panel of dock.viewState.panels) {
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

  // =========================================================================
  // REFRESH PANEL DATA (inspect, routing, gating)
  // =========================================================================

  const refreshPanelData = React.useCallback(async () => {
    const openPanels = dock.viewState.panels.map((p) => p.target).filter(Boolean) as MobKitDockTarget[];
    const inspects = openPanels.filter((t): t is Extract<MobKitDockTarget, { kind: "identity-inspect" }> => t.kind === "identity-inspect");
    if (inspects.length) {
      const entries = await Promise.all(inspects.map(async (t) => {
        const r = await callConsoleRpc<IdentityInspectViewState>(baseUrl, "mobkit/inspect_identity", { identity: t.identity });
        return [t.identity, r] as const;
      }));
      setInspectByIdentity((c) => ({ ...c, ...Object.fromEntries(entries) }));
    }
    if (openPanels.some((t) => t.kind === "routing")) {
      const [routes, history] = await Promise.all([
        callConsoleRpc(baseUrl, "mobkit/routing/routes/list", {}),
        callConsoleRpc(baseUrl, "mobkit/delivery/history", {}),
      ]);
      setRoutingData(buildRoutingSectionView({ routesResponse: routes, historyResponse: history }));
    }
    if (openPanels.some((t) => t.kind === "gating")) {
      const [p, a] = await Promise.all([
        callConsoleRpc<{ pending?: unknown[] }>(baseUrl, "mobkit/gating/pending", {}),
        callConsoleRpc<{ entries?: unknown[] }>(baseUrl, "mobkit/gating/audit", { limit: 50 }),
      ]);
      setGatingData({ pending: Array.isArray(p.pending) ? p.pending : [], audit: Array.isArray(a.entries) ? a.entries : [] });
    }
  }, [baseUrl, dock.viewState.panels]);

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
        const { frames, available } = await queryEvents(baseUrl, { identity }, 400);
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
          const { frames, available } = await queryEvents(baseUrl, { identity }, 400);
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
    void queryEvents(baseUrl, {}, 200)
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

    const unsubscribe = subscribeConsoleEvents(baseUrl, "/console/events/stream", (frame) => {
      // Activity rail (independent buffer)
      if (!ACTIVITY_SKIP_EVENTS.has(frame.event)) {
        activityRef.current = [frame, ...activityRef.current].slice(0, 200);
      }

      // Identity log (single canonical store)
      const identity = frame.identity?.trim();
      if (PANEL_ROUTABLE_EVENTS.has(frame.event) && identity && identity !== "_system") {
        appendFrame(identity, frame);
        updatePhaseForIdentity(identity, frame);
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

  async function onSendMessage(panelId: string, target: MobKitDockTarget | null) {
    if (!target || target.kind !== "agent-chat") return;
    const panelKey = buildPanelConversationKey(panelId, target);
    const identity = target.identity || target.memberId;
    const text = (draftByKey[panelKey] || "").trim();
    if (!text) return;

    const userEntry = createUserEntry(text);
    setDraftByKey((c) => ({ ...c, [panelKey]: "" }));
    setSendingPanels((c) => new Set(c).add(panelKey));

    // Optimistic: store user message on the identity log. Cleared
    // when an interaction_started frame with matching interaction_id
    // is appended (see appendFrame).
    const log = getOrCreateLog(identity);
    log.optimisticUser = {
      interactionId: "",
      entry: userEntry,
      sentAtMs: Date.now(),
    };
    phaseRef.current[panelKey] = "waiting";
    forceRender();

    try {
      const id = target.identity?.trim();
      if (id) {
        const result = await sendInteractRpc(baseUrl, id, text, `console:${panelId}`);
        if (log.optimisticUser) {
          log.optimisticUser.interactionId = result.interaction_id;
          // The interaction_started frame may have arrived between
          // the send and the RPC response — reconcile retroactively.
          const matched = log.events.some(
            (f) => f.event === "interaction_started" && f.interactionId === result.interaction_id,
          );
          if (matched) log.optimisticUser = null;
        }
      } else {
        await sendMessage(baseUrl, target.memberId, text);
      }
    } catch (submitError) {
      log.optimisticUser = null;
      phaseRef.current[panelKey] = null;
      setError(errorMessage(submitError));
      forceRender();
    } finally {
      setSendingPanels((c) => { const n = new Set(c); n.delete(panelKey); return n; });
    }
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
    });

    // Optimistic user message: rendered until an interaction_started
    // with the matching interaction_id is appended to the log (which
    // clears it via appendFrame). Until then, it sits at the tail of
    // the conversation as a synthetic entry.
    const log = getOrCreateLog(identity);
    const optimisticEntry = log.optimisticUser ? log.optimisticUser.entry : null;

    const entries = sanitizeConversationEntries([
      ...conversationEntries,
      ...(optimisticEntry ? [optimisticEntry] : []),
    ]);

    const conversation = buildConversationViewState({
      memberId: target.memberId,
      agentLabel: target.title,
      agent,
      entries,
    });
    const draft = draftByKey[panelKey] || "";
    const isSending = sendingPanels.has(panelKey);
    const phase = phaseRef.current[panelKey] ?? agent?.response_phase ?? null;

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

    return (
      <ChatPane
        agent={agent}
        agentLabel={target.title || agent?.label || identity}
        identity={identity}
        entries={entries}
        phase={phase}
        draft={draft}
        sending={isSending}
        onDraftChange={(v) => setDraftByKey((c) => ({ ...c, [panelKey]: v }))}
        onSend={() => void onSendMessage(panel.id, target)}
        onInspect={() => { if (agent) dock.openTarget(buildInspectTarget(agent), "new_tab"); }}
        onRespawn={() => void onLifecycleAction(identity, "mobkit/respawn")}
        onRetire={() => void onLifecycleAction(identity, "mobkit/retire")}
      />
    );
  }

  // =========================================================================
  // RENDER: CONTROL PANELS (unchanged)
  // =========================================================================

  function renderInspectPanel(target: Extract<MobKitDockTarget, { kind: "identity-inspect" }>) {
    const inspect = inspectByIdentity[target.identity];
    return (
      <div className="console-panel" data-testid={`inspect-panel:${target.identity}`}>
        <div className="console-panel__header">
          <h3>{target.identity}</h3>
          <div className="console-panel__actions">
            <button data-testid={`inspect-action:${target.identity}:respawn`} type="button" onClick={() => void onLifecycleAction(target.identity, "mobkit/respawn")}>Respawn</button>
            <button data-testid={`inspect-action:${target.identity}:reset`} type="button" onClick={() => void onLifecycleAction(target.identity, "mobkit/reset")}>Reset</button>
            <button data-testid={`inspect-action:${target.identity}:retire`} type="button" onClick={() => void onLifecycleAction(target.identity, "mobkit/retire")}>Retire</button>
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
          onToggleCollapsed={toggleSidebarCollapsed}
          onSelect={(a) => dock.openTarget(buildDockTarget(a), "replace_focused")}
          onInspect={(a) => dock.openTarget(buildInspectTarget(a), "replace_focused")}
          onOpenControl={(kind) => dock.openTarget(buildControlTarget(kind), "replace_focused")}
        />
        <div className="pane-resizer" aria-hidden="true" data-testid="resize:sidebar" onPointerDown={handleSidebarResize} />
        <div className="main">
          <MobKitDock
            viewState={dock.viewState}
            agents={agents}
            renderPanelBody={renderPanelBody}
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
          onToggleCollapsed={toggleRailCollapsed}
        />
      </div>
      <Tweaks
        variant={variant}
        theme={theme}
        onVariant={setVariant}
        onTheme={(t) => { setTheme(t); try { localStorage.setItem("mobkit-console-theme", t); } catch { /* ignore */ } }}
      />
    </div>
  );
}
