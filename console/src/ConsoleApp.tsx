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
  sortConversationTimelineEntries,
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
type GatingPanelData = {
  pending: unknown[];
  audit: unknown[];
};

function normalizeComparableTranscriptText(value: string): string {
  return value.replace(/^\[EVENT via rpc\]\s*/i, "").replace(/\s+/g, " ").trim();
}

function sameUserMessage(
  entry: ConversationTimelineEntry | null | undefined,
  candidate: ConversationTimelineEntry | null | undefined,
): boolean {
  if (!entry || !candidate || entry.kind !== "message" || candidate.kind !== "message") {
    return false;
  }
  if (entry.identity.id !== "user" || candidate.identity.id !== "user") {
    return false;
  }
  return normalizeComparableTranscriptText(entry.text || "") === normalizeComparableTranscriptText(candidate.text || "");
}

function clipTranscriptWindow(entries: ConversationTimelineEntry[]): ConversationTimelineEntry[] {
  const maxEntries = 100;
  return entries.slice(-maxEntries);
}

function hasVisibleConversationContent(entry: ConversationTimelineEntry): boolean {
  if (entry.kind !== "message") {
    return true;
  }
  if (Array.isArray(entry.blocks) && entry.blocks.length > 0) {
    return entry.blocks.some((block) => {
      const record = block as Record<string, unknown>;
      const text = [
        typeof record.text === "string" ? record.text : "",
        typeof record.label === "string" ? record.label : "",
        typeof record.result === "string" ? record.result : "",
        typeof record.body === "string" ? record.body : "",
        typeof record.title === "string" ? record.title : "",
      ].join(" ").trim();
      return text.length > 0;
    });
  }
  return Boolean(entry.text && entry.text.trim().length > 0);
}

function richBlockHasVisibleContent(block: unknown): boolean {
  if (!block || typeof block !== "object") {
    return false;
  }
  const record = block as Record<string, unknown>;
  const scalarText = [
    typeof record.text === "string" ? record.text : "",
    typeof record.label === "string" ? record.label : "",
    typeof record.result === "string" ? record.result : "",
    typeof record.body === "string" ? record.body : "",
    typeof record.title === "string" ? record.title : "",
    typeof record.name === "string" ? record.name : "",
  ].join(" ").trim();
  if (scalarText.length > 0) {
    return true;
  }
  if (Array.isArray(record.headers) && record.headers.some((value) => String(value || "").trim().length > 0)) {
    return true;
  }
  if (Array.isArray(record.rows) && record.rows.some((row) => Array.isArray(row) && row.some((value) => String(value || "").trim().length > 0))) {
    return true;
  }
  return false;
}

function sanitizeConversationEntries(entries: ConversationTimelineEntry[]): ConversationTimelineEntry[] {
  const sanitized: ConversationTimelineEntry[] = [];
  for (const entry of entries) {
    if (entry.kind !== "message") {
      sanitized.push(entry);
      continue;
    }
    if (entry.variant === "rich" && Array.isArray(entry.blocks)) {
      const blocks = entry.blocks.filter(richBlockHasVisibleContent);
      if (!blocks.length) {
        continue;
      }
      sanitized.push({ ...entry, blocks });
      continue;
    }
    if (hasVisibleConversationContent(entry)) {
      sanitized.push(entry);
    }
  }
  return sanitized;
}

const DEFAULT_APPROVER_ID = "console-ops-lead";

// --- Event sets for the SSE handler ---
const REFRESH_TRIGGER_EVENTS = new Set([
  "interaction_complete",
  "interaction_failed",
  "state_changed",
  "member_ready",
  "member_retired",
  "gating_decision",
  "route_changed",
]);
const PANEL_ROUTABLE_EVENTS = new Set([
  "interaction_started",
  "interaction_complete",
  "interaction_failed",
  "text_delta",
  "text_complete",
  "tool_call_requested",
  "tool_call",
  "tool_result_received",
  "tool_execution_started",
  "tool_execution_completed",
  "run_started",
  "run_completed",
  "run_failed",
]);
const HISTORY_REFRESH_EVENTS = new Set([
  "interaction_complete",
  "interaction_failed",
  "run_completed",
  "run_failed",
]);

export function ConsoleApp({ baseUrl }: ConsoleAppProps): React.JSX.Element {
  // --- React state (low-frequency, UI-driven) ---
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

  // --- Render trigger (the ONLY high-frequency React state) ---
  const [, setRenderTick] = React.useState(0);
  const forceRender = React.useCallback(() => setRenderTick((n) => n + 1), []);

  // --- Mutable refs for high-frequency event-driven data ---
  const transcriptRef = React.useRef<Record<string, ConversationTimelineEntry[]>>({});
  const pendingUserRef = React.useRef<Record<string, ConversationTimelineEntry | null>>({});
  const liveFramesRef = React.useRef<Record<string, ConsoleFrame[]>>({});
  const activityRef = React.useRef<ConsoleFrame[]>([]);
  const phaseRef = React.useRef<Record<string, "waiting" | "tool-executing" | "generating" | null>>({});

  // --- Scheduling refs ---
  const refreshInFlightRef = React.useRef<Set<string>>(new Set());
  const experienceTimerRef = React.useRef<number | null>(null);

  // --- Existing refs (phase debounce, history guards, multi-panel) ---
  const initialTargetOpened = React.useRef(false);
  const phaseValueByKey = React.useRef<Record<string, "waiting" | "tool-executing" | "generating" | null>>({});
  const phaseSinceByKey = React.useRef<Record<string, number>>({});
  const phaseTimerByKey = React.useRef<Record<string, number>>({});
  const historyLoadedByKey = React.useRef<Record<string, boolean>>({});
  const panelBaselineEntriesByKey = React.useRef<Record<string, ConversationTimelineEntry[]>>({});
  const identityPanelCountByIdentity = React.useRef<Record<string, number>>({});
  const previousIdentityPanelCountByIdentity = React.useRef<Record<string, number>>({});
  const agentsRef = React.useRef<ConsoleAgent[]>([]);
  const dockRef = React.useRef<{ panels: Array<{ id: string; target: MobKitDockTarget | null }> }>({ panels: [] });

  React.useEffect(() => {
    agentsRef.current = agents;
  }, [agents]);

  // --- Dock controller ---
  const dock = useConsoleDockController<MobKitDockTarget>({
    createPanelState: ({ target }) => ({
      id: `panel-${crypto.randomUUID()}`,
      target: target || null,
      mode: "console" as const,
    }),
  });

  // Keep dockRef in sync so SSE callback can read current panels
  React.useEffect(() => {
    dockRef.current = {
      panels: dock.viewState.panels.map((panel) => ({
        id: panel.id,
        target: panel.target as MobKitDockTarget | null,
      })),
    };
  }, [dock.viewState.panels]);

  // --- Panel count tracking ---
  React.useEffect(() => {
    const counts: Record<string, number> = {};
    for (const panel of dock.viewState.panels) {
      const target = panel.target;
      if (!target || target.kind !== "agent-chat") continue;
      const key = target.identity || target.memberId;
      counts[key] = (counts[key] || 0) + 1;
    }
    identityPanelCountByIdentity.current = counts;
  }, [dock.viewState.panels]);

  // --- Prune closed panels from refs ---
  React.useEffect(() => {
    const activePanelKeys = new Set(
      dock.viewState.panels
        .map((panel) => {
          if (!panel.target) return null;
          return buildPanelConversationKey(panel.id, panel.target as MobKitDockTarget);
        })
        .filter((value): value is string => Boolean(value)),
    );

    const pruneRef = <T,>(record: Record<string, T>) => {
      for (const key of Object.keys(record)) {
        if (!activePanelKeys.has(key)) {
          delete record[key];
        }
      }
    };

    pruneRef(transcriptRef.current);
    pruneRef(pendingUserRef.current);
    pruneRef(liveFramesRef.current);
    pruneRef(phaseRef.current);
    pruneRef(historyLoadedByKey.current);
    pruneRef(panelBaselineEntriesByKey.current);
    pruneRef(phaseValueByKey.current);
    pruneRef(phaseSinceByKey.current);
    for (const key of Object.keys(phaseTimerByKey.current)) {
      if (!activePanelKeys.has(key)) {
        window.clearTimeout(phaseTimerByKey.current[key]);
        delete phaseTimerByKey.current[key];
      }
    }

    setDraftByKey((current) => {
      let changed = false;
      const next: Record<string, string> = {};
      for (const [key, value] of Object.entries(current)) {
        if (activePanelKeys.has(key)) {
          next[key] = value;
        } else {
          changed = true;
        }
      }
      return changed ? next : current;
    });
  }, [dock.viewState.panels]);

  // --- Seed transcript for newly opened multi-panels ---
  React.useEffect(() => {
    const previousCounts = previousIdentityPanelCountByIdentity.current;
    const nextCounts = identityPanelCountByIdentity.current;
    for (const panel of dock.viewState.panels) {
      const target = panel.target;
      if (!target || target.kind !== "agent-chat") continue;
      const identityKey = target.identity || target.memberId;
      const nextCount = nextCounts[identityKey] || 0;
      const previousCount = previousCounts[identityKey] || 0;
      if (nextCount > 1 && nextCount > previousCount) {
        const siblingPanels = dock.viewState.panels.filter((candidate) => {
          const candidateTarget = candidate.target;
          return candidateTarget?.kind === "agent-chat" && (candidateTarget.identity || candidateTarget.memberId) === identityKey;
        });
        const seedTranscript = siblingPanels
          .map((candidate) => transcriptRef.current[buildPanelConversationKey(candidate.id, candidate.target as Extract<MobKitDockTarget, { kind: "agent-chat" }>)])
          .find((entries): entries is ConversationTimelineEntry[] => Array.isArray(entries) && entries.length > 0);
        if (seedTranscript?.length) {
          for (const sibling of siblingPanels) {
            const siblingTarget = sibling.target as Extract<MobKitDockTarget, { kind: "agent-chat" }>;
            const siblingKey = buildPanelConversationKey(sibling.id, siblingTarget);
            panelBaselineEntriesByKey.current[siblingKey] = seedTranscript;
            if (!transcriptRef.current[siblingKey]?.length) {
              transcriptRef.current[siblingKey] = seedTranscript;
            }
          }
        }
      }
    }
    previousIdentityPanelCountByIdentity.current = { ...nextCounts };
  }, [dock.viewState.panels]);

  // --- Load experience ---
  const loadExperience = React.useCallback(async () => {
    const [experienceJson, modulesJson] = await Promise.all([
      fetchJson<ConsoleExperience>(baseUrl, "/console/experience"),
      fetchJson<ConsoleModulesResponse>(baseUrl, "/console/modules"),
    ]);
    const loadedModules = Array.isArray(modulesJson.modules)
      ? modulesJson.modules.map((moduleId) => String(moduleId))
      : [];
    const nextAgents = normalizeAgents(experienceJson, loadedModules);
    setExperience(experienceJson);
    setAgents(nextAgents);
    setActiveActivityPresetId((current) => current || experienceJson.activity_feed?.active_preset_id || "all");
  }, [baseUrl]);

  // Initial experience load
  React.useEffect(() => {
    let mounted = true;
    setLoading(true);
    setError("");
    void loadExperience()
      .catch((loadError) => {
        if (mounted) setError(errorMessage(loadError));
      })
      .finally(() => {
        if (mounted) setLoading(false);
      });
    return () => { mounted = false; };
  }, [loadExperience]);

  // --- Open first agent on initial load ---
  React.useEffect(() => {
    if (initialTargetOpened.current || dock.focusedTarget || agents.length === 0) return;
    const firstAddressable =
      agents.find((agent) => agent.addressable || agent.affordances?.can_send_message) || agents[0];
    if (!firstAddressable) return;
    initialTargetOpened.current = true;
    dock.openTarget(buildDockTarget(firstAddressable), "replace_focused");
  }, [agents, dock]);

  // --- Refresh panel data (inspect/routing/gating) ---
  const refreshPanelData = React.useCallback(async () => {
    const openPanels = dockRef.current.panels.map((p) => p.target).filter(Boolean) as MobKitDockTarget[];
    const inspectTargets = openPanels.filter((t): t is Extract<MobKitDockTarget, { kind: "identity-inspect" }> => t.kind === "identity-inspect");
    const hasRouting = openPanels.some((t) => t.kind === "routing");
    const hasGating = openPanels.some((t) => t.kind === "gating");

    if (inspectTargets.length) {
      const entries = await Promise.all(
        inspectTargets.map(async (target) => {
          const result = await callConsoleRpc<IdentityInspectViewState>(baseUrl, "mobkit/inspect_identity", { identity: target.identity });
          return [target.identity, result] as const;
        }),
      );
      setInspectByIdentity((current) => ({ ...current, ...Object.fromEntries(entries) }));
    }
    if (hasRouting) {
      const [routesResponse, historyResponse] = await Promise.all([
        callConsoleRpc(baseUrl, "mobkit/routing/routes/list", {}),
        callConsoleRpc(baseUrl, "mobkit/delivery/history", {}),
      ]);
      setRoutingData(buildRoutingSectionView({ routesResponse, historyResponse }));
    }
    if (hasGating) {
      const [pendingResponse, auditResponse] = await Promise.all([
        callConsoleRpc<{ pending?: unknown[] }>(baseUrl, "mobkit/gating/pending", {}),
        callConsoleRpc<{ entries?: unknown[] }>(baseUrl, "mobkit/gating/audit", { limit: 50 }),
      ]);
      setGatingData({
        pending: Array.isArray(pendingResponse.pending) ? pendingResponse.pending : [],
        audit: Array.isArray(auditResponse.entries) ? auditResponse.entries : [],
      });
    }
  }, [baseUrl]);

  // Initial panel data load when panels change
  React.useEffect(() => {
    void refreshPanelData().catch(() => {});
  }, [dock.viewState.panels, refreshPanelData]);

  // --- Schedule experience + panel data refresh (debounced, called from SSE handler) ---
  const scheduleExperienceRefresh = React.useCallback(() => {
    if (experienceTimerRef.current !== null) return;
    experienceTimerRef.current = window.setTimeout(async () => {
      experienceTimerRef.current = null;
      await loadExperience().catch(() => {});
      await refreshPanelData().catch(() => {});
    }, 500);
  }, [loadExperience, refreshPanelData]);

  // --- Schedule history refresh for an identity (called from SSE handler on terminal events) ---
  const scheduleHistoryRefresh = React.useCallback((identity: string) => {
    if (refreshInFlightRef.current.has(identity)) return;
    refreshInFlightRef.current.add(identity);

    setTimeout(async () => {
      try {
        for (const panel of dockRef.current.panels) {
          const target = panel.target;
          if (!target || target.kind !== "agent-chat") continue;
          if ((target.identity || target.memberId) !== identity) continue;
          const panelKey = buildPanelConversationKey(panel.id, target);
          const agent = agentsRef.current.find((c) => c.member_id === target.memberId) || null;

          const frames = await queryEvents(baseUrl, {
            memberId: target.memberId,
            ...(target.identity ? { identity: target.identity } : {}),
          }, 400);

          const mapped = mapFramesToTimelineEntries(agent, frames, {
            renderInteractionStartsAsUser: true,
            renderTextDeltas: false,
          });

          // Reconcile pending user entry
          const persistedTexts = new Set(
            mapped
              .filter((e) => e.kind === "message" && e.identity.id === "user")
              .map((e) => normalizeComparableTranscriptText(e.text?.trim() || ""))
              .filter(Boolean),
          );
          const pending = pendingUserRef.current[panelKey];
          if (pending?.kind === "message") {
            const pendingText = normalizeComparableTranscriptText(pending.text?.trim() || "");
            if (pendingText && persistedTexts.has(pendingText)) {
              pendingUserRef.current[panelKey] = null;
            }
          }

          // Keep un-persisted optimistic user entries
          const existingOptimistic = (transcriptRef.current[panelKey] || []).filter((entry) => {
            if (entry.kind !== "message" || entry.identity.id !== "user" || !String(entry.id).startsWith("user:")) return false;
            const text = normalizeComparableTranscriptText(entry.text?.trim() || "");
            return text && !persistedTexts.has(text);
          });

          transcriptRef.current[panelKey] = clipTranscriptWindow([
            ...mapped,
            ...existingOptimistic,
          ]);
          liveFramesRef.current[panelKey] = [];
          phaseRef.current[panelKey] = null;
        }
        forceRender();
      } finally {
        refreshInFlightRef.current.delete(identity);
      }
    }, 200);
  }, [baseUrl, forceRender]);

  // --- Load initial history when panels open ---
  React.useEffect(() => {
    for (const panel of dock.viewState.panels) {
      const target = panel.target as MobKitDockTarget | null;
      if (!target || target.kind !== "agent-chat") continue;
      const panelKey = buildPanelConversationKey(panel.id, target);
      if (historyLoadedByKey.current[panelKey]) continue;
      historyLoadedByKey.current[panelKey] = true;

      void (async () => {
        try {
          const agent = agentsRef.current.find((c) => c.member_id === target.memberId) || null;
          const frames = await queryEvents(baseUrl, {
            memberId: target.memberId,
            ...(target.identity ? { identity: target.identity } : {}),
          }, 400);
          const mapped = mapFramesToTimelineEntries(agent, frames, {
            renderInteractionStartsAsUser: true,
            renderTextDeltas: false,
          });
          transcriptRef.current[panelKey] = clipTranscriptWindow(mapped);
          if (!liveFramesRef.current[panelKey]) liveFramesRef.current[panelKey] = [];
          forceRender();
        } catch {
          historyLoadedByKey.current[panelKey] = false;
        }
      })();
    }
  }, [baseUrl, dock.viewState.panels, forceRender]);

  // --- Global SSE event stream (the CORE event loop) ---
  React.useEffect(() => {
    // Seed activity with recent history
    void queryEvents(baseUrl, {}, 80)
      .then((frames) => {
        activityRef.current = dedupeFrames(frames).slice(-80).reverse();
        forceRender();
      })
      .catch(() => {});

    const unsubscribe = subscribeConsoleEvents(baseUrl, "/console/events/stream", (frame) => {
      // 1. Activity rail — mutate ref
      activityRef.current = [frame, ...activityRef.current].slice(0, 200);

      // 2. Route to open chat panels by identity — mutate ref
      const identity = frame.identity?.trim();
      if (PANEL_ROUTABLE_EVENTS.has(frame.event) && identity && identity !== "_system") {
        for (const panel of dockRef.current.panels) {
          const target = panel.target;
          if (!target || target.kind !== "agent-chat") continue;
          const panelIdentity = target.identity || target.memberId;
          if (panelIdentity !== identity) continue;
          const panelKey = buildPanelConversationKey(panel.id, target);
          if (!liveFramesRef.current[panelKey]) liveFramesRef.current[panelKey] = [];
          liveFramesRef.current[panelKey] = dedupeFrames([
            ...liveFramesRef.current[panelKey],
            frame,
          ]);
          updatePanelPhaseFromFrame(panelKey, frame);
        }
      }

      // 3. Force React re-render
      forceRender();

      // 4. On terminal events, schedule history refresh + experience reload
      if (HISTORY_REFRESH_EVENTS.has(frame.event) && identity && identity !== "_system") {
        scheduleHistoryRefresh(identity);
      }
      if (REFRESH_TRIGGER_EVENTS.has(frame.event)) {
        scheduleExperienceRefresh();
      }
    });

    return () => { unsubscribe(); };
  }, [baseUrl, forceRender, scheduleHistoryRefresh, scheduleExperienceRefresh]);

  // --- Timer cleanup on unmount ---
  React.useEffect(() => {
    return () => {
      for (const timer of Object.values(phaseTimerByKey.current)) {
        window.clearTimeout(timer);
      }
      if (experienceTimerRef.current !== null) {
        window.clearTimeout(experienceTimerRef.current);
      }
    };
  }, []);

  // --- Helpers ---
  function dedupeFrames(frames: ConsoleFrame[]): ConsoleFrame[] {
    const byId = new Map<string, ConsoleFrame>();
    const ordered: ConsoleFrame[] = [];
    for (const frame of frames) {
      const key = frame.id || `${frame.event}:${frame.timestampMs || 0}`;
      if (byId.has(key)) continue;
      byId.set(key, frame);
      ordered.push(frame);
    }
    return ordered;
  }

  function clearPhaseTimer(panelKey: string) {
    const timer = phaseTimerByKey.current[panelKey];
    if (timer !== undefined) {
      window.clearTimeout(timer);
      delete phaseTimerByKey.current[panelKey];
    }
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
        commitPanelPhase(panelKey, "waiting");
        break;
      case "tool_call_requested":
      case "tool_call":
      case "tool_execution_started":
      case "tool_result_received":
      case "tool_execution_completed":
        if (currentPhase === "waiting" && elapsedMs < 300) {
          schedulePanelPhase(panelKey, "tool-executing", 300 - elapsedMs);
          break;
        }
        commitPanelPhase(panelKey, "tool-executing");
        break;
      case "text_delta": {
        if (currentPhase === "tool-executing") {
          const remainingMs = Math.max(0, 300 - elapsedMs);
          if (remainingMs > 0) {
            schedulePanelPhase(panelKey, "generating", remainingMs);
            break;
          }
        }
        if (currentPhase === "waiting" && elapsedMs < 300) {
          schedulePanelPhase(panelKey, "generating", 300 - elapsedMs);
          break;
        }
        commitPanelPhase(panelKey, "generating");
        break;
      }
      case "interaction_complete":
      case "interaction_failed":
      case "run_completed":
      case "run_failed":
        commitPanelPhase(panelKey, null);
        break;
      default:
        break;
    }
  }

  // --- Agent selection ---
  function onSelectAgent(_block: unknown, _section: unknown, item: { id: string }) {
    const agent = agents.find((candidate) => candidate.member_id === item.id);
    if (agent) {
      dock.openTarget(buildDockTarget(agent), "replace_focused");
    }
  }

  // --- Send message (RPC-only, global SSE handles response events) ---
  async function onSendMessage(panelId: string, target: MobKitDockTarget | null) {
    if (!target || target.kind !== "agent-chat") return;
    const panelKey = buildPanelConversationKey(panelId, target);
    const text = (draftByKey[panelKey] || "").trim();
    if (!text) return;

    const userEntry = createUserEntry(text);
    setDraftByKey((current) => ({ ...current, [panelKey]: "" }));
    setSendingPanels((current) => new Set(current).add(panelKey));

    // Optimistic update — write to refs directly
    transcriptRef.current[panelKey] = sortConversationTimelineEntries([
      ...(transcriptRef.current[panelKey] || []),
      userEntry,
    ]);
    pendingUserRef.current[panelKey] = userEntry;
    phaseRef.current[panelKey] = "waiting";
    if (!liveFramesRef.current[panelKey]) liveFramesRef.current[panelKey] = [];
    forceRender();

    try {
      const identity = target.identity?.trim();
      if (identity) {
        await sendInteractRpc(baseUrl, identity, text, `console:${panelId}`);
      } else {
        await sendMessage(baseUrl, target.memberId, text);
      }
    } catch (submitError) {
      setError(errorMessage(submitError));
      // Rollback optimistic update
      transcriptRef.current[panelKey] = (transcriptRef.current[panelKey] || [])
        .filter((e) => e.id !== userEntry.id);
      pendingUserRef.current[panelKey] = null;
      phaseRef.current[panelKey] = null;
      forceRender();
    } finally {
      setSendingPanels((current) => {
        const next = new Set(current);
        next.delete(panelKey);
        return next;
      });
    }
  }

  // --- Lifecycle actions ---
  async function onLifecycleAction(identity: string, method: "mobkit/retire" | "mobkit/respawn" | "mobkit/reset") {
    await callConsoleRpc(baseUrl, method, { identity });
    await loadExperience();
  }

  async function onGatingDecision(pendingId: string, decision: "approve" | "reject" | "escalate") {
    await callConsoleRpc<unknown>(baseUrl, "mobkit/gating/decide", {
      pending_id: pendingId,
      approver_id: DEFAULT_APPROVER_ID,
      decision,
      reason: `console_${decision}`,
    } as ConsoleGatingActionPayload);
    const [pendingResponse, auditResponse] = await Promise.all([
      callConsoleRpc<{ pending?: unknown[] }>(baseUrl, "mobkit/gating/pending", {}),
      callConsoleRpc<{ entries?: unknown[] }>(baseUrl, "mobkit/gating/audit", { limit: 50 }),
    ]);
    setGatingData({
      pending: Array.isArray(pendingResponse.pending) ? pendingResponse.pending : [],
      audit: Array.isArray(auditResponse.entries) ? auditResponse.entries : [],
    });
  }

  // --- Resize handlers ---
  const SIDEBAR_MIN = 180;
  const SIDEBAR_MAX = 420;
  function handleSidebarResize(event: React.PointerEvent<HTMLDivElement>) {
    event.preventDefault();
    const startX = event.clientX;
    const root = event.currentTarget.closest("[data-console-workbench]") as HTMLElement | null;
    if (!root) return;
    const startWidth = parseInt(getComputedStyle(root).getPropertyValue("--cc-workbench-sidebar-width") || "260", 10) || 260;
    const handle = event.currentTarget;
    if ("setPointerCapture" in handle) handle.setPointerCapture(event.pointerId);
    document.documentElement.setAttribute("data-cc-resizing", "true");
    function onPointerMove(e: PointerEvent) {
      const next = Math.min(SIDEBAR_MAX, Math.max(SIDEBAR_MIN, startWidth + (e.clientX - startX)));
      root!.style.setProperty("--cc-workbench-sidebar-width", `${next}px`);
    }
    function cleanup() {
      document.documentElement.removeAttribute("data-cc-resizing");
      window.removeEventListener("pointermove", onPointerMove);
      window.removeEventListener("pointerup", cleanup);
      window.removeEventListener("pointercancel", cleanup);
      if ("hasPointerCapture" in handle && handle.hasPointerCapture(event.pointerId)) {
        handle.releasePointerCapture(event.pointerId);
      }
    }
    window.addEventListener("pointermove", onPointerMove);
    window.addEventListener("pointerup", cleanup);
    window.addEventListener("pointercancel", cleanup);
  }

  const ACTIVITY_MIN = 200;
  const ACTIVITY_MAX = 480;
  function handleActivityResize(event: React.PointerEvent<HTMLDivElement>) {
    event.preventDefault();
    const startX = event.clientX;
    const root = event.currentTarget.closest("[data-console-workbench]") as HTMLElement | null;
    if (!root) return;
    const startWidth = parseInt(getComputedStyle(root).getPropertyValue("--cc-workbench-activity-width") || "280", 10) || 280;
    const handle = event.currentTarget;
    if ("setPointerCapture" in handle) handle.setPointerCapture(event.pointerId);
    document.documentElement.setAttribute("data-cc-resizing", "true");
    function onPointerMove(e: PointerEvent) {
      const next = Math.min(ACTIVITY_MAX, Math.max(ACTIVITY_MIN, startWidth - (e.clientX - startX)));
      root!.style.setProperty("--cc-workbench-activity-width", `${next}px`);
    }
    function cleanup() {
      document.documentElement.removeAttribute("data-cc-resizing");
      window.removeEventListener("pointermove", onPointerMove);
      window.removeEventListener("pointerup", cleanup);
      window.removeEventListener("pointercancel", cleanup);
      if ("hasPointerCapture" in handle && handle.hasPointerCapture(event.pointerId)) {
        handle.releasePointerCapture(event.pointerId);
      }
    }
    window.addEventListener("pointermove", onPointerMove);
    window.addEventListener("pointerup", cleanup);
    window.addEventListener("pointercancel", cleanup);
  }

  // --- Render guards ---
  if (loading) {
    return <div data-testid="console-loading">Loading console...</div>;
  }
  if (error) {
    return <div data-testid="console-error">{error}</div>;
  }

  // --- Build view states (reads refs at render time) ---
  const focusedMemberId = dock.focusedTarget?.kind === "agent-chat" ? dock.focusedTarget.memberId : "";
  const sidebarVS = buildSidebarViewState({ agents, selectedMemberId: focusedMemberId, pinnedAgentIds });
  const activityVS = buildActivityRailViewState({
    agents,
    eventFrames: activityRef.current,
    filterPresets: experience?.activity_feed?.filter_presets,
    activePresetId: activeActivityPresetId,
  });

  // --- Panel renderers ---
  function renderChatPanel(panel: { id: string; target?: MobKitDockTarget | null }) {
    const target = panel.target;
    if (!target || target.kind !== "agent-chat") return null;
    const panelKey = buildPanelConversationKey(panel.id, target);
    const agent = agents.find((candidate) => candidate.member_id === target.memberId) || null;

    // Read from refs (not React state)
    const persistedEntries = transcriptRef.current[panelKey] || [];
    const latestPersistedAt = persistedEntries.reduce<number>((latest, entry) => {
      const parsed = Date.parse(String(entry.createdAt || ""));
      return Number.isFinite(parsed) ? Math.max(latest, parsed) : latest;
    }, Number.NEGATIVE_INFINITY);
    const combinedFrames = liveFramesRef.current[panelKey] || [];
    const liveFrames = Number.isFinite(latestPersistedAt)
      ? combinedFrames.filter((frame) => typeof frame.timestampMs !== "number" || frame.timestampMs > latestPersistedAt)
      : combinedFrames;
    const pendingUserEntry = pendingUserRef.current[panelKey];
    const baseEntries = [
      ...persistedEntries,
      ...mapFramesToTimelineEntries(agent, liveFrames, {
        renderInteractionStartsAsUser: false,
        renderTextDeltas: false,
        suppressEmbeddedRunStartedPrompt: true,
      }),
    ];
    const pendingAlreadyMaterialized = pendingUserEntry
      ? baseEntries.some((entry) => sameUserMessage(entry, pendingUserEntry))
      : false;
    const entries = sanitizeConversationEntries([
      ...(!pendingAlreadyMaterialized && pendingUserEntry ? [pendingUserEntry] : []),
      ...baseEntries,
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

    const quickPrompts = buildQuickPromptSuggestions(agent).map((suggestion) => ({
      id: suggestion.id,
      kind: "pill" as const,
      label: suggestion.label,
      iconName: suggestion.iconName || "i-bolt",
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
      <div
        className="console-panel console-panel--chat"
        data-panel-id={panel.id}
        data-panel-key={panelKey}
        data-testid={`chat-panel:${target.identity || target.memberId}:${panel.id}`}
      >
        <ConversationPane
          viewState={conversation}
          Icon={Icon}
          onApplySuggestion={(value) => setDraftByKey((current) => ({ ...current, [panelKey]: value }))}
          footer={
            <ConsoleComposer
              Icon={Icon}
              inputId={`composer-input:${panel.id}`}
              shellId={`composer-shell:${panel.id}`}
              submitButtonId={`composer-submit:${panel.id}`}
              viewState={{
                value: draft,
                disabled: isSending,
                placeholder: `Message ${target.title}...`,
                submitDisabled: !draft.trim() || isSending,
                submitLabel: `Send to ${target.title}`,
                mainRowItems: quickPrompts,
                footerLeftItems,
                footerRightItems,
              }}
              getToolbarButtonProps={({ zone, item }) => {
                const buttonProps: Record<string, unknown> = {
                  "data-testid": `composer-toolbar:${panel.id}:${zone}:${item.id}`,
                };
                if (zone === "main") {
                  const suggestion = buildQuickPromptSuggestions(agent).find((candidate) => candidate.id === item.id);
                  if (suggestion) {
                    buttonProps.onClick = () => setDraftByKey((current) => ({ ...current, [panelKey]: suggestion.value }));
                  }
                }
                return buttonProps;
              }}
              onChange={(value) => setDraftByKey((current) => ({ ...current, [panelKey]: value }))}
              onSubmit={() => void onSendMessage(panel.id, target)}
              onKeyDown={(event) => {
                if (event.key === "Enter" && !event.shiftKey) {
                  event.preventDefault();
                  void onSendMessage(panel.id, target);
                }
              }}
            />
          }
        />
      </div>
    );
  }

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
        <div className="console-panel__section">
          <h3>Routes</h3>
          <ul className="console-panel__list">
            {routingData.routes.map((route) => (
              <li data-testid={`routing-route:${route.route_key}`} key={route.route_key}>
                <strong>{route.route_key}</strong> → {route.recipient} via {route.sink}
              </li>
            ))}
          </ul>
        </div>
        <div className="console-panel__section">
          <h3>Deliveries</h3>
          <ul className="console-panel__list">
            {routingData.deliveries.map((delivery) => (
              <li data-testid={`routing-delivery:${delivery.delivery_id}`} key={delivery.delivery_id}>
                <strong>{delivery.delivery_id}</strong> · {delivery.status} · {delivery.recipient}
              </li>
            ))}
          </ul>
        </div>
      </div>
    );
  }

  function renderGatingPanel() {
    return (
      <div className="console-panel" data-testid="gating-panel">
        <div className="console-panel__section">
          <h3>Pending</h3>
          <ul className="console-panel__list">
            {gatingData.pending.map((entry, index) => {
              const record = entry as Record<string, unknown>;
              const pendingId = String(record.pending_id || `pending-${index}`);
              return (
                <li data-testid={`gating-pending:${pendingId}`} key={pendingId}>
                  <div><strong>{String(record.action_id || pendingId)}</strong> · {String(record.risk_tier || "unknown")}</div>
                  <div className="console-panel__actions">
                    <button data-testid={`gating-action:${pendingId}:escalate`} type="button" onClick={() => void onGatingDecision(pendingId, "escalate")}>Escalate</button>
                    <button data-testid={`gating-action:${pendingId}:approve`} type="button" onClick={() => void onGatingDecision(pendingId, "approve")}>Approve</button>
                    <button data-testid={`gating-action:${pendingId}:reject`} type="button" onClick={() => void onGatingDecision(pendingId, "reject")}>Reject</button>
                  </div>
                </li>
              );
            })}
          </ul>
        </div>
        <div className="console-panel__section">
          <h3>Audit</h3>
          <ul className="console-panel__list">
            {gatingData.audit.map((entry, index) => {
              const record = entry as Record<string, unknown>;
              return (
                <li data-testid={`gating-audit:${String(record.audit_id || index)}`} key={String(record.audit_id || index)}>
                  <strong>{String(record.event_type || "event")}</strong> · {String(record.action_id || "unknown")}
                </li>
              );
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
          {nodes.map((node) => (
            <li data-testid={`topology-node:${node.identity || node.label}`} key={node.identity || node.label}>
              <strong>{node.label || node.identity}</strong>
              <div>{node.profile || "unknown"} · {node.state || "unknown"}</div>
              <div>Peers: {node.wired_to?.join(", ") || "none"}</div>
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
          {identities.map((row) => (
            <li data-testid={`health-identity:${row.identity}`} key={row.identity}>
              <strong>{row.display_name || row.identity}</strong> · {row.state} · {row.addressability}
            </li>
          ))}
        </ul>
      </div>
    );
  }

  // --- Main render ---
  return (
    <div className="cc-theme-scope" data-cc-theme={theme} data-testid="meerkat-console">
      <SpriteSheet />
      <ConsoleWorkbench
        launcherResizeHandle={<div className="pane-resizer" aria-hidden="true" data-testid="resize:sidebar" onPointerDown={handleSidebarResize} />}
        launcherHeader={
          <button
            className="console-theme-toggle"
            data-testid="theme-toggle"
            type="button"
            title={`Switch to ${theme === "dark" ? "light" : "dark"} mode`}
            onClick={() => {
              const next = theme === "dark" ? "light" : "dark";
              setTheme(next);
              try { localStorage.setItem("mobkit-console-theme", next); } catch {}
            }}
          >
            {theme === "dark" ? "☀" : "☾"}
          </button>
        }
        launcher={(
          <ConsoleSidebar
            viewState={sidebarVS}
            Icon={Icon}
            getActionButtonProps={(scope) => {
              if (scope.kind === "block") {
                return { "data-testid": `sidebar-action:${scope.action.id}` };
              }
              if (scope.kind === "item") {
                return { "data-testid": `sidebar-item-action:${scope.item.id}:${scope.action.id}` };
              }
              return {};
            }}
            onBlockAction={(_block, action) => {
              switch (action.id) {
                case "open_routing":
                  dock.openTarget(buildControlTarget("routing"), "new_tab");
                  break;
                case "open_gating":
                  dock.openTarget(buildControlTarget("gating"), "new_tab");
                  break;
                case "open_topology":
                  dock.openTarget(buildControlTarget("topology"), "new_tab");
                  break;
                case "open_health":
                  dock.openTarget(buildControlTarget("health"), "new_tab");
                  break;
                default:
                  break;
              }
            }}
            onSelectItem={onSelectAgent}
            onItemAction={(_block, _section, item, action) => {
              const agent = agents.find((candidate) => candidate.member_id === item.id);
              if (!agent) return;
              if (action.id === "inspect_identity") {
                dock.openTarget(buildInspectTarget(agent), "new_tab");
                return;
              }
              if (action.id === "toggle_pin") {
                setPinnedAgentIds((current) => {
                  const next = new Set(current);
                  if (next.has(item.id)) next.delete(item.id);
                  else next.add(item.id);
                  return next;
                });
              }
            }}
            onItemContextMenu={(_block, _section, item, event) => {
              event.preventDefault();
              const agent = agents.find((candidate) => candidate.member_id === item.id);
              if (agent) {
                dock.openTarget(buildInspectTarget(agent), "new_tab");
              }
            }}
          />
        )}
        main={(
          <ConsoleDock
            viewState={dock.viewState}
            Icon={Icon}
            onSelectTab={(tab) => dock.selectTab(tab.id)}
            onCloseTab={(tab) => dock.closeTab(tab.id)}
            onFocusPanel={(panel) => dock.focusPanel(panel.id)}
            onSplitPanel={(panel, direction) => dock.splitPanel(panel.id, direction)}
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
            viewState={activityVS}
            Icon={Icon}
            onTogglePicker={() => {}}
            onCollapse={() => {}}
            onPanelAction={(_panelId, actionId) => setActiveActivityPresetId(actionId)}
            renderSlotPreview={() => null}
            onSelectItem={(focusId) => {
              const agent = agents.find((candidate) => candidate.member_id === focusId);
              if (agent) {
                dock.openTarget(buildDockTarget(agent), "replace_focused");
              }
            }}
          />
        )}
      />
    </div>
  );
}
