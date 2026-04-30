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

  // --- Render trigger ---
  const [, setRenderTick] = React.useState(0);
  const forceRender = React.useCallback(() => setRenderTick((n) => n + 1), []);

  // =========================================================================
  // NEW DATA MODEL — 3 identity-keyed refs
  // =========================================================================

  // 1. Server history: last queryEvents result per identity. REPLACED wholesale on fetch.
  const serverHistoryRef = React.useRef<Record<string, ConsoleFrame[]>>({});

  // 2. Live overlay: SSE frames since last server fetch. CLEARED on server fetch.
  const liveOverlayRef = React.useRef<Record<string, ConsoleFrame[]>>({});

  // 3. Optimistic user message: at most one per identity. Reconciled by interaction_id.
  const optimisticUserRef = React.useRef<Record<string, OptimisticUserMessage | null>>({});

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
  // FRAME DEDUP HELPER
  // =========================================================================

  function dedupeFrames(frames: ConsoleFrame[]): ConsoleFrame[] {
    const seen = new Set<string>();
    const result: ConsoleFrame[] = [];
    for (const frame of frames) {
      const key = frame.id || `${frame.event}:${frame.timestampMs || 0}`;
      if (seen.has(key)) continue;
      seen.add(key);
      result.push(frame);
    }
    return result;
  }

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
      try {
        const { frames, available } = await queryEvents(baseUrl, { identity }, 400);
        if (available) {
          // Server has authoritative history — replace local state and
          // drop the live overlay (it's now redundant / a possible
          // duplicate source).
          serverHistoryRef.current[identity] = frames;
          liveOverlayRef.current[identity] = [];

          // Reconcile optimistic user message by interaction_id.
          const optimistic = optimisticUserRef.current[identity];
          if (optimistic && optimistic.interactionId) {
            const found = frames.some((f) =>
              f.event === "interaction_started" && f.interactionId === optimistic.interactionId
            );
            if (found) optimisticUserRef.current[identity] = null;
          }
        }
        // No `else` clear: when the runtime has no event log
        // configured, the live overlay IS the only source of truth.
        // Wiping it on every terminal event was the old bug — the
        // rich transcript flickered into a near-empty replay because
        // server frames were always [] and the overlay got cleared.
        clearPhaseForIdentity(identity);
        forceRender();
      } catch { /* silent — will retry on next terminal event */ }
    }, 200);
  }, [baseUrl, forceRender]);

  // =========================================================================
  // PANEL OPEN / SWITCH — fetch history for new identities
  // =========================================================================

  React.useEffect(() => {
    for (const panel of dock.viewState.panels) {
      const target = panel.target as MobKitDockTarget | null;
      if (!target || target.kind !== "agent-chat") continue;
      const identity = target.identity || target.memberId;

      // If history exists and no stale live overlay, skip fetch
      const hasHistory = Boolean(serverHistoryRef.current[identity]);
      const hasStaleOverlay = (liveOverlayRef.current[identity]?.length || 0) > 0;
      if (hasHistory && !hasStaleOverlay) continue;

      // Clear live overlay immediately to prevent stale frames flashing
      liveOverlayRef.current[identity] = [];

      void (async () => {
        try {
          const { frames, available } = await queryEvents(baseUrl, { identity }, 400);
          if (available) {
            serverHistoryRef.current[identity] = frames;
            liveOverlayRef.current[identity] = [];
          } else {
            // No event log → don't wipe the overlay on initial open;
            // mark history as "fetched but empty" so we don't re-fetch.
            serverHistoryRef.current[identity] = [];
          }
          forceRender();
        } catch { /* silent */ }
      })();
    }
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
    // Seed activity with recent history (only on mount) — apply same filter as SSE
    void queryEvents(baseUrl, {}, 200)
      .then(({ frames }) => {
        const filtered = dedupeFrames(frames).filter((f) => !ACTIVITY_SKIP_EVENTS.has(f.event));
        activityRef.current = filtered.slice(-200).reverse();
        forceRender();
      })
      .catch(() => {});

    const unsubscribe = subscribeConsoleEvents(baseUrl, "/console/events/stream", (frame) => {
      // 1. Activity rail — only buffer events that pass the display filter
      if (!ACTIVITY_SKIP_EVENTS.has(frame.event)) {
        activityRef.current = [frame, ...activityRef.current].slice(0, 200);
      }

      // 2. Live overlay — append by identity with event_id dedup
      const identity = frame.identity?.trim();
      if (PANEL_ROUTABLE_EVENTS.has(frame.event) && identity && identity !== "_system") {
        const existing = liveOverlayRef.current[identity] || [];
        if (!existing.some((f) => f.id === frame.id)) {
          liveOverlayRef.current[identity] = [...existing, frame];
        }
        updatePhaseForIdentity(identity, frame);
      }

      // 3. Force render
      forceRender();

      // 4. Terminal events → refetch server history
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

    // Optimistic: store user message (reconciled by interaction_id later)
    optimisticUserRef.current[identity] = {
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
        // Attach interaction_id for reconciliation
        if (optimisticUserRef.current[identity]) {
          optimisticUserRef.current[identity]!.interactionId = result.interaction_id;
        }
      } else {
        await sendMessage(baseUrl, target.memberId, text);
      }
      // Response events arrive via SSE → terminal event → scheduleHistoryRefresh
      // → serverHistoryRef updated → optimistic cleared by interaction_id
    } catch (submitError) {
      optimisticUserRef.current[identity] = null;
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

    // 1. Server history → timeline entries (authoritative, server-ordered)
    const serverFrames = serverHistoryRef.current[identity] || [];
    const serverEntries = mapFramesToTimelineEntries(agent, serverFrames, {
      renderInteractionStartsAsUser: true,
      renderTextDeltas: false,
    });

    // 2. Live overlay → timeline entries (only frames not in server history)
    const liveFrames = liveOverlayRef.current[identity] || [];
    const serverIds = new Set(serverFrames.map((f) => f.id));
    const newLiveFrames = liveFrames.filter((f) => !serverIds.has(f.id));
    // Render text deltas on the live path so the assistant message grows
    // token-by-token while the interaction is in flight. Server history keeps
    // `renderTextDeltas: false` because `interaction_complete.data.result`
    // already carries the final text and repeating the deltas would be noise.
    // Duplicate suppression when the interaction finally completes is handled
    // by the `streamedText === terminalText` check in `renderTerminalEntry`.
    const liveEntries = mapFramesToTimelineEntries(agent, newLiveFrames, {
      renderInteractionStartsAsUser: false,
      suppressEmbeddedRunStartedPrompt: true,
    });

    // 3. Optimistic user message (if not yet reconciled against SERVER history)
    // Only reconcile against serverHistoryRef — never against live overlay.
    // The optimistic stays visible until the server confirms the interaction.
    const optimistic = optimisticUserRef.current[identity];
    let optimisticEntry: ConversationTimelineEntry | null = null;
    if (optimistic) {
      const reconciled = optimistic.interactionId &&
        serverFrames.some((f) => f.event === "interaction_started" && f.interactionId === optimistic.interactionId);
      if (reconciled) {
        optimisticUserRef.current[identity] = null;
      } else {
        optimisticEntry = optimistic.entry;
      }
    }

    // 4. Merge: server + optimistic + live (all in natural order, no sorting)
    const entries = sanitizeConversationEntries([
      ...serverEntries,
      ...(optimisticEntry ? [optimisticEntry] : []),
      ...liveEntries,
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
      <div className="shell">
        <DesignSidebar
          agents={agents}
          selectedMemberId={focusedMemberId}
          recentActivity={activityRef.current}
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
