import React from "react";
import "@console-components/styles";
import "./console-host.css";

import {
  ConsoleActivityRail,
  ConsoleComposer,
  ConsoleDock,
  ConsoleSidebar,
  ConsoleWorkbench,
  ConversationPane,
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
  const [theme, setTheme] = React.useState<"dark" | "light">(() => {
    try { return (localStorage.getItem("mobkit-console-theme") as "dark" | "light") || "dark"; } catch { return "dark"; }
  });

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
        const frames = await queryEvents(baseUrl, { identity }, 400);
        serverHistoryRef.current[identity] = frames;  // REPLACE
        liveOverlayRef.current[identity] = [];        // CLEAR overlay

        // Reconcile optimistic user message by interaction_id
        const optimistic = optimisticUserRef.current[identity];
        if (optimistic && optimistic.interactionId) {
          const found = frames.some((f) =>
            f.event === "interaction_started" && f.interactionId === optimistic.interactionId
          );
          if (found) optimisticUserRef.current[identity] = null;
        }

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
          const frames = await queryEvents(baseUrl, { identity }, 400);
          serverHistoryRef.current[identity] = frames;
          liveOverlayRef.current[identity] = [];
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
    // Seed activity with recent history (only on mount)
    void queryEvents(baseUrl, {}, 80)
      .then((frames) => { activityRef.current = dedupeFrames(frames).slice(-80).reverse(); forceRender(); })
      .catch(() => {});

    const unsubscribe = subscribeConsoleEvents(baseUrl, "/console/events/stream", (frame) => {
      // 1. Activity rail
      activityRef.current = [frame, ...activityRef.current].slice(0, 200);

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
    const liveEntries = mapFramesToTimelineEntries(agent, newLiveFrames, {
      renderInteractionStartsAsUser: false,
      renderTextDeltas: false,
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
      ...(agent?.profile ? [{ id: "profile", kind: "sub-pill" as const, label: agent.profile }] : []),
      ...(phase ? [{ id: "phase", kind: "sub-pill" as const, label: phase, iconName: "i-bolt" }] : []),
      { id: "state", kind: "sub-pill" as const, label: agent?.state || "unknown", iconName: "i-dot" },
    ];

    return (
      <div className="console-panel console-panel--chat" data-panel-id={panel.id} data-panel-key={panelKey} data-testid={`chat-panel:${identity}:${panel.id}`}>
        <ConversationPane
          viewState={conversation}
          Icon={Icon}
          onApplySuggestion={(v) => setDraftByKey((c) => ({ ...c, [panelKey]: v }))}
          footer={
            <ConsoleComposer
              Icon={Icon}
              inputId={`composer-input:${panel.id}`}
              shellId={`composer-shell:${panel.id}`}
              submitButtonId={`composer-submit:${panel.id}`}
              viewState={{
                value: draft, disabled: isSending,
                placeholder: `Message ${target.title}...`,
                submitDisabled: !draft.trim() || isSending,
                submitLabel: `Send to ${target.title}`,
                mainRowItems: quickPrompts, footerLeftItems, footerRightItems,
              }}
              getToolbarButtonProps={({ zone, item }) => {
                const props: Record<string, unknown> = { "data-testid": `composer-toolbar:${panel.id}:${zone}:${item.id}` };
                if (zone === "main") {
                  const s = buildQuickPromptSuggestions(agent).find((c) => c.id === item.id);
                  if (s) props.onClick = () => setDraftByKey((c) => ({ ...c, [panelKey]: s.value }));
                }
                return props;
              }}
              onChange={(v) => setDraftByKey((c) => ({ ...c, [panelKey]: v }))}
              onSubmit={() => void onSendMessage(panel.id, target)}
              onKeyDown={(e) => { if (e.key === "Enter" && !e.shiftKey) { e.preventDefault(); void onSendMessage(panel.id, target); } }}
            />
          }
        />
      </div>
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
            <dt>Profile</dt><dd>{inspect.profile || "n/a"}</dd>
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

  function renderRoutingPanel() {
    return (
      <div className="console-panel" data-testid="routing-panel">
        <div className="console-panel__section"><h3>Routes</h3>
          <ul className="console-panel__list">
            {routingData.routes.map((r) => <li data-testid={`routing-route:${r.route_key}`} key={r.route_key}><strong>{r.route_key}</strong> → {r.recipient} via {r.sink}</li>)}
          </ul>
        </div>
        <div className="console-panel__section"><h3>Deliveries</h3>
          <ul className="console-panel__list">
            {routingData.deliveries.map((d) => <li data-testid={`routing-delivery:${d.delivery_id}`} key={d.delivery_id}><strong>{d.delivery_id}</strong> · {d.status} · {d.recipient}</li>)}
          </ul>
        </div>
      </div>
    );
  }

  function renderGatingPanel() {
    return (
      <div className="console-panel" data-testid="gating-panel">
        <div className="console-panel__section"><h3>Pending</h3>
          <ul className="console-panel__list">
            {gatingData.pending.map((entry, index) => {
              const r = entry as Record<string, unknown>;
              const pid = String(r.pending_id || `pending-${index}`);
              return (
                <li data-testid={`gating-pending:${pid}`} key={pid}>
                  <div><strong>{String(r.action_id || pid)}</strong> · {String(r.risk_tier || "unknown")}</div>
                  <div className="console-panel__actions">
                    <button data-testid={`gating-action:${pid}:escalate`} type="button" onClick={() => void onGatingDecision(pid, "escalate")}>Escalate</button>
                    <button data-testid={`gating-action:${pid}:approve`} type="button" onClick={() => void onGatingDecision(pid, "approve")}>Approve</button>
                    <button data-testid={`gating-action:${pid}:reject`} type="button" onClick={() => void onGatingDecision(pid, "reject")}>Reject</button>
                  </div>
                </li>
              );
            })}
          </ul>
        </div>
        <div className="console-panel__section"><h3>Audit</h3>
          <ul className="console-panel__list">
            {gatingData.audit.map((entry, index) => {
              const r = entry as Record<string, unknown>;
              return <li data-testid={`gating-audit:${String(r.audit_id || index)}`} key={String(r.audit_id || index)}><strong>{String(r.event_type || "event")}</strong> · {String(r.action_id || "unknown")}</li>;
            })}
          </ul>
        </div>
      </div>
    );
  }

  function renderTopologyPanel(nodes: ConsoleTopologyNode[]) {
    return (
      <div className="console-panel" data-testid="topology-panel">
        <ul className="console-panel__list">
          {nodes.map((n) => (
            <li data-testid={`topology-node:${n.identity || n.label}`} key={n.identity || n.label}>
              <strong>{n.label || n.identity}</strong>
              <div>{n.profile || "unknown"} · {n.state || "unknown"}</div>
              <div>Peers: {n.wired_to?.join(", ") || "none"}</div>
            </li>
          ))}
        </ul>
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

  // =========================================================================
  // MAIN RENDER
  // =========================================================================

  return (
    <div className="cc-theme-scope" data-cc-theme={theme} data-testid="meerkat-console">
      <SpriteSheet />
      <ConsoleWorkbench
        launcherResizeHandle={<div className="pane-resizer" aria-hidden="true" data-testid="resize:sidebar" onPointerDown={handleSidebarResize} />}
        launcherHeader={
          <button className="console-theme-toggle" data-testid="theme-toggle" type="button"
            title={`Switch to ${theme === "dark" ? "light" : "dark"} mode`}
            onClick={() => { const next = theme === "dark" ? "light" : "dark"; setTheme(next); try { localStorage.setItem("mobkit-console-theme", next); } catch {} }}>
            {theme === "dark" ? "☀" : "☾"}
          </button>
        }
        launcher={(
          <ConsoleSidebar
            viewState={sidebarVS} Icon={Icon}
            getActionButtonProps={(scope) => {
              if (scope.kind === "block") return { "data-testid": `sidebar-action:${scope.action.id}` };
              if (scope.kind === "item") return { "data-testid": `sidebar-item-action:${scope.item.id}:${scope.action.id}` };
              return {};
            }}
            onBlockAction={(_b, action) => {
              switch (action.id) {
                case "open_routing": dock.openTarget(buildControlTarget("routing"), "new_tab"); break;
                case "open_gating": dock.openTarget(buildControlTarget("gating"), "new_tab"); break;
                case "open_topology": dock.openTarget(buildControlTarget("topology"), "new_tab"); break;
                case "open_health": dock.openTarget(buildControlTarget("health"), "new_tab"); break;
              }
            }}
            onSelectItem={onSelectAgent}
            onItemAction={(_b, _s, item, action) => {
              const agent = agents.find((c) => c.member_id === item.id);
              if (!agent) return;
              if (action.id === "inspect_identity") { dock.openTarget(buildInspectTarget(agent), "new_tab"); return; }
              if (action.id === "toggle_pin") { setPinnedAgentIds((c) => { const n = new Set(c); if (n.has(item.id)) n.delete(item.id); else n.add(item.id); return n; }); }
            }}
            onItemContextMenu={(_b, _s, item, event) => {
              event.preventDefault();
              const agent = agents.find((c) => c.member_id === item.id);
              if (agent) dock.openTarget(buildInspectTarget(agent), "new_tab");
            }}
          />
        )}
        main={(
          <ConsoleDock
            viewState={dock.viewState} Icon={Icon}
            onSelectTab={(tab) => dock.selectTab(tab.id)}
            onCloseTab={(tab) => dock.closeTab(tab.id)}
            onFocusPanel={(panel) => dock.focusPanel(panel.id)}
            onSplitPanel={(panel, dir) => dock.splitPanel(panel.id, dir)}
            onClosePanel={(panel) => dock.closePanel(panel.id)}
            onResizeSplit={(id, ratio) => dock.resizeSplit(id, ratio)}
            onCreateTab={() => dock.createTab()}
            renderPanelBody={(panel) => {
              const target = panel.target as MobKitDockTarget | null;
              if (!target) return <div className="console-panel">No panel target</div>;
              if (target.kind === "agent-chat") return renderChatPanel(panel);
              if (target.kind === "identity-inspect") return renderInspectPanel(target);
              if (target.kind === "routing") return renderRoutingPanel();
              if (target.kind === "gating") return renderGatingPanel();
              if (target.kind === "topology") return renderTopologyPanel(experience?.topology?.live_snapshot?.nodes || []);
              if (target.kind === "health") return renderHealthPanel(experience?.health_overview?.live_snapshot?.identities || []);
              return <div className="console-panel">Unsupported panel</div>;
            }}
          />
        )}
        activityRailResizeHandle={<div className="pane-resizer pane-resizer--activity" aria-hidden="true" data-testid="resize:activity" onPointerDown={handleActivityResize} />}
        activityRail={(
          <ConsoleActivityRail
            viewState={activityVS} Icon={Icon}
            onTogglePicker={() => {}} onCollapse={() => {}}
            onPanelAction={(_pid, actionId) => setActiveActivityPresetId(actionId)}
            renderSlotPreview={() => null}
            onSelectItem={(focusId) => {
              const agent = agents.find((c) => c.member_id === focusId);
              if (agent) dock.openTarget(buildDockTarget(agent), "replace_focused");
            }}
          />
        )}
      />
    </div>
  );
}
