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
  mergeConversationFrames,
  buildPanelConversationKey,
  buildRoutingSectionView,
  buildSidebarViewState,
  createUserEntry,
  mapSessionHistoryToTimelineEntries,
  mapFramesToTimelineEntries,
  sortConversationTimelineEntries,
  type MobKitDockTarget,
} from "./lib/adapters";
import { errorMessage } from "./lib/errors";
import {
  callConsoleRpc,
  fetchJson,
  queryEvents,
  readSessionHistory,
  sendAddressedInteractionStreaming,
  subscribeConsoleEvents,
  subscribeIdentityEvents,
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

const DEFAULT_APPROVER_ID = "console-ops-lead";

export function ConsoleApp({ baseUrl }: ConsoleAppProps): React.JSX.Element {
  const [experience, setExperience] = React.useState<ConsoleExperience | null>(null);
  const [agents, setAgents] = React.useState<ConsoleAgent[]>([]);
  const [historyFramesByKey, setHistoryFramesByKey] = React.useState<Record<string, ConsoleFrame[]>>({});
  const [historyEntriesByKey, setHistoryEntriesByKey] = React.useState<Record<string, ConversationTimelineEntry[]>>({});
  const [panelFramesByKey, setPanelFramesByKey] = React.useState<Record<string, ConsoleFrame[]>>({});
  const [localEntriesByKey, setLocalEntriesByKey] = React.useState<Record<string, ReturnType<typeof createUserEntry>[]>>({});
  const [activityFrames, setActivityFrames] = React.useState<ConsoleFrame[]>([]);
  const [draftByKey, setDraftByKey] = React.useState<Record<string, string>>({});
  const [sendingPanels, setSendingPanels] = React.useState<Set<string>>(new Set());
  const [pinnedAgentIds, setPinnedAgentIds] = React.useState<Set<string>>(new Set());
  const [panelPhaseByKey, setPanelPhaseByKey] = React.useState<Record<string, "waiting" | "tool-executing" | "generating" | null>>({});
  const [inspectByIdentity, setInspectByIdentity] = React.useState<Record<string, IdentityInspectViewState | null>>({});
  const [routingData, setRoutingData] = React.useState<RoutingPanelData>({ routes: [], deliveries: [] });
  const [gatingData, setGatingData] = React.useState<GatingPanelData>({ pending: [], audit: [] });
  const [activeActivityPresetId, setActiveActivityPresetId] = React.useState("all");
  const [loading, setLoading] = React.useState(true);
  const [error, setError] = React.useState("");
  const initialTargetOpened = React.useRef(false);
  const phaseValueByKey = React.useRef<Record<string, "waiting" | "tool-executing" | "generating" | null>>({});
  const phaseSinceByKey = React.useRef<Record<string, number>>({});
  const phaseTimerByKey = React.useRef<Record<string, number>>({});
  const historyLoadedByKey = React.useRef<Record<string, boolean>>({});
  const historySourceByKey = React.useRef<Record<string, string>>({});

  const refreshChatPanelHistory = React.useCallback(async (
    panelId: string,
    target: Extract<MobKitDockTarget, { kind: "agent-chat" }>,
    force = false,
  ) => {
    const panelKey = buildPanelConversationKey(panelId, target);
    const agent = agents.find((candidate) => candidate.member_id === target.memberId) || null;
    const sourceKey = agent?.session_id?.trim()
      ? `session:${agent.session_id.trim()}`
      : `events:${target.identity || target.memberId}`;
    if (
      !force
      && historyLoadedByKey.current[panelKey]
      && historySourceByKey.current[panelKey] === sourceKey
    ) {
      return;
    }
    historyLoadedByKey.current[panelKey] = true;
    historySourceByKey.current[panelKey] = sourceKey;
    try {
      const historyPage = agent?.session_id
        ? await readSessionHistory(baseUrl, agent.session_id, 200)
        : null;
      const frames = historyPage
        ? []
        : await queryEvents(baseUrl, {
          memberId: target.memberId,
          ...(target.identity ? { identity: target.identity } : {}),
        }, 120);
      setHistoryFramesByKey((current) => ({ ...current, [panelKey]: dedupeFrames(frames) }));
      setHistoryEntriesByKey((current) => ({
        ...current,
        [panelKey]: historyPage ? mapSessionHistoryToTimelineEntries(historyPage, agent) : [],
      }));
      if (historyPage) {
        setPanelFramesByKey((current) => ({ ...current, [panelKey]: [] }));
        setLocalEntriesByKey((current) => ({ ...current, [panelKey]: [] }));
      }
    } catch {
      historyLoadedByKey.current[panelKey] = false;
      delete historySourceByKey.current[panelKey];
    }
  }, [agents, baseUrl]);

  const dock = useConsoleDockController<MobKitDockTarget>({
    createPanelState: ({ target }) => ({
      id: `panel-${crypto.randomUUID()}`,
      target: target || null,
      mode: "console" as const,
    }),
  });

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

    const interval = window.setInterval(() => {
      void loadExperience().catch(() => {});
    }, 5000);

    return () => {
      mounted = false;
      window.clearInterval(interval);
    };
  }, [loadExperience]);

  React.useEffect(() => {
    if (initialTargetOpened.current || dock.focusedTarget || agents.length === 0) {
      return;
    }
    const firstAddressable =
      agents.find((agent) => agent.addressable || agent.affordances?.can_send_message) || agents[0];
    if (!firstAddressable) {
      return;
    }
    initialTargetOpened.current = true;
    dock.openTarget(buildDockTarget(firstAddressable), "replace_focused");
  }, [agents, dock]);

  React.useEffect(() => {
    let cancelled = false;
    void queryEvents(baseUrl, {}, 80)
      .then((frames) => {
        if (!cancelled) {
          setActivityFrames(dedupeFrames(frames).slice(-80).reverse());
        }
      })
      .catch(() => {});
    const unsubscribe = subscribeConsoleEvents(baseUrl, "/console/events/stream", (frame) => {
      setActivityFrames((current) => [frame, ...current].slice(0, 200));
    });
    return () => {
      cancelled = true;
      unsubscribe();
    };
  }, [baseUrl]);

  React.useEffect(() => {
    return () => {
      for (const timer of Object.values(phaseTimerByKey.current)) {
        window.clearTimeout(timer);
      }
    };
  }, []);

  React.useEffect(() => {
    const openPanels = dock.viewState.panels.map((panel) => panel.target).filter(Boolean) as MobKitDockTarget[];
    const inspectTargets = openPanels.filter((target): target is Extract<MobKitDockTarget, { kind: "identity-inspect" }> => target.kind === "identity-inspect");
    const hasRouting = openPanels.some((target) => target.kind === "routing");
    const hasGating = openPanels.some((target) => target.kind === "gating");

    let cancelled = false;
    async function refreshPanelData() {
      try {
        if (inspectTargets.length) {
          const inspectEntries = await Promise.all(
            inspectTargets.map(async (target) => {
              const result = await callConsoleRpc<IdentityInspectViewState>(baseUrl, "mobkit/inspect_identity", {
                identity: target.identity,
              });
              return [target.identity, result] as const;
            }),
          );
          if (!cancelled) {
            setInspectByIdentity((current) => ({ ...current, ...Object.fromEntries(inspectEntries) }));
          }
        }

        if (hasRouting) {
          const [routesResponse, historyResponse] = await Promise.all([
            callConsoleRpc(baseUrl, "mobkit/routing/routes/list", {}),
            callConsoleRpc(baseUrl, "mobkit/delivery/history", {}),
          ]);
          if (!cancelled) {
            setRoutingData(buildRoutingSectionView({ routesResponse, historyResponse }));
          }
        }

        if (hasGating) {
          const [pendingResponse, auditResponse] = await Promise.all([
            callConsoleRpc<{ pending?: unknown[] }>(baseUrl, "mobkit/gating/pending", {}),
            callConsoleRpc<{ entries?: unknown[] }>(baseUrl, "mobkit/gating/audit", { limit: 50 }),
          ]);
          if (!cancelled) {
            setGatingData({
              pending: Array.isArray(pendingResponse.pending) ? pendingResponse.pending : [],
              audit: Array.isArray(auditResponse.entries) ? auditResponse.entries : [],
            });
          }
        }
      } catch (panelError) {
        if (!cancelled) {
          setError(errorMessage(panelError));
        }
      }
    }

    void refreshPanelData();
    const interval = window.setInterval(() => void refreshPanelData(), 5000);
    return () => {
      cancelled = true;
      window.clearInterval(interval);
    };
  }, [baseUrl, dock.viewState.panels]);

  React.useEffect(() => {
    const chatPanels = dock.viewState.panels
      .map((panel) => ({ id: panel.id, target: panel.target }))
      .filter((panel): panel is { id: string; target: Extract<MobKitDockTarget, { kind: "agent-chat" }> } =>
        panel.target?.kind === "agent-chat");
    let cancelled = false;

    async function loadPanelHistory() {
      for (const panel of chatPanels) {
        if (cancelled) return;
        try {
          await refreshChatPanelHistory(panel.id, panel.target);
        } catch {
          if (!cancelled) {
            const panelKey = buildPanelConversationKey(panel.id, panel.target);
            historyLoadedByKey.current[panelKey] = false;
          }
        }
      }
    }

    void loadPanelHistory();
    return () => {
      cancelled = true;
    };
  }, [dock.viewState.panels, refreshChatPanelHistory]);

  React.useEffect(() => {
    const chatPanels = dock.viewState.panels
      .map((panel) => ({ id: panel.id, target: panel.target }))
      .filter((panel): panel is { id: string; target: Extract<MobKitDockTarget, { kind: "agent-chat" }> } =>
        panel.target?.kind === "agent-chat");
    const unsubscribers = chatPanels
      .filter((panel) => Boolean(panel.target.identity))
      .map((panel) => {
        return subscribeIdentityEvents(baseUrl, panel.target.identity!, (frame) => {
          appendPanelFrame(panel.id, panel.target, frame);
        });
      });
    return () => {
      for (const unsubscribe of unsubscribers) {
        unsubscribe();
      }
    };
  }, [baseUrl, dock.viewState.panels]);

  function onSelectAgent(
    _block: unknown,
    _section: unknown,
    item: { id: string },
  ) {
    const agent = agents.find((candidate) => candidate.member_id === item.id);
    if (agent) {
      dock.openTarget(buildDockTarget(agent), "replace_focused");
    }
  }

  function appendPanelFrame(
    panelId: string,
    target: Extract<MobKitDockTarget, { kind: "agent-chat" }>,
    frame: ConsoleFrame,
  ) {
    const panelKey = buildPanelConversationKey(panelId, target);
    if (frame.event === "interaction_started") {
      const content = frame.data && typeof frame.data === "object"
        ? (frame.data as Record<string, unknown>).content
        : undefined;
      const normalizedContent = typeof content === "string" ? content.trim() : "";
      setLocalEntriesByKey((current) => {
        const existing = current[panelKey] || [];
        if (existing.length === 0) {
          return current;
        }
        const matchIndex = existing.findIndex((entry) => {
          const entryText = typeof entry.text === "string" ? entry.text.trim() : "";
          return normalizedContent ? entryText === normalizedContent : true;
        });
        if (matchIndex === -1) {
          return current;
        }
        const nextEntries = existing.filter((_, index) => index !== matchIndex);
        return { ...current, [panelKey]: nextEntries };
      });
    }
    setPanelFramesByKey((current) => {
      const nextFrames = dedupeFrames([...(current[panelKey] || []), frame]);
      return { ...current, [panelKey]: nextFrames };
    });
    updatePanelPhaseFromFrame(panelKey, frame);
    if (
      frame.event === "interaction_complete"
      || frame.event === "interaction_failed"
      || frame.event === "run_completed"
      || frame.event === "run_failed"
    ) {
      void refreshChatPanelHistory(panelId, target, true);
    }
  }

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

  function commitPanelPhase(
    panelKey: string,
    phase: "waiting" | "tool-executing" | "generating" | null,
  ) {
    clearPhaseTimer(panelKey);
    phaseValueByKey.current[panelKey] = phase;
    phaseSinceByKey.current[panelKey] = Date.now();
    setPanelPhaseByKey((current) => ({ ...current, [panelKey]: phase }));
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
      setPanelPhaseByKey((current) => ({ ...current, [panelKey]: phase }));
    }, delayMs);
  }

  function updatePanelPhaseFromFrame(panelKey: string, frame: ConsoleFrame) {
    switch (frame.event) {
      case "interaction_started":
        commitPanelPhase(panelKey, "waiting");
        break;
      case "tool_call_requested":
      case "tool_call":
      case "tool_execution_started":
      case "tool_result_received":
      case "tool_execution_completed":
        commitPanelPhase(panelKey, "tool-executing");
        break;
      case "text_delta": {
        const currentPhase = phaseValueByKey.current[panelKey] ?? null;
        if (currentPhase === "tool-executing") {
          const elapsedMs = Date.now() - (phaseSinceByKey.current[panelKey] ?? 0);
          const remainingMs = Math.max(0, 300 - elapsedMs);
          if (remainingMs > 0) {
            schedulePanelPhase(panelKey, "generating", remainingMs);
            break;
          }
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

  async function onSendMessage(panelId: string, target: MobKitDockTarget | null) {
    if (!target || target.kind !== "agent-chat") return;
    const panelKey = buildPanelConversationKey(panelId, target);
    const text = (draftByKey[panelKey] || "").trim();
    if (!text) return;

    const userEntry = createUserEntry(text);
    setDraftByKey((current) => ({ ...current, [panelKey]: "" }));
    setSendingPanels((current) => new Set(current).add(panelKey));
    commitPanelPhase(panelKey, "waiting");
    setLocalEntriesByKey((current) => ({
      ...current,
      [panelKey]: [...(current[panelKey] || []), userEntry],
    }));

    try {
      await sendAddressedInteractionStreaming(
        baseUrl,
        {
          addressingMode: target.addressingMode,
          memberId: target.memberId,
          ...(target.identity ? { identity: target.identity } : {}),
        },
        text,
        `console:${panelId}`,
        (frame) => appendPanelFrame(panelId, target, frame),
      );
      commitPanelPhase(panelKey, null);
    } catch (submitError) {
      setError(errorMessage(submitError));
      setLocalEntriesByKey((current) => ({
        ...current,
        [panelKey]: (current[panelKey] || []).filter((entry) => entry.id !== userEntry.id),
      }));
      commitPanelPhase(panelKey, null);
    } finally {
      setSendingPanels((current) => {
        const next = new Set(current);
        next.delete(panelKey);
        return next;
      });
    }
  }

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
      root.style.setProperty("--cc-workbench-sidebar-width", `${next}px`);
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
      root.style.setProperty("--cc-workbench-activity-width", `${next}px`);
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

  if (loading) {
    return <div data-testid="console-loading">Loading console...</div>;
  }
  if (error) {
    return <div data-testid="console-error">{error}</div>;
  }

  const focusedMemberId = dock.focusedTarget?.kind === "agent-chat" ? dock.focusedTarget.memberId : "";
  const sidebarVS = buildSidebarViewState({ agents, selectedMemberId: focusedMemberId, pinnedAgentIds });
  const activityVS = buildActivityRailViewState({
    agents,
    eventFrames: activityFrames,
    filterPresets: experience?.activity_feed?.filter_presets,
    activePresetId: activeActivityPresetId,
  });

  function renderChatPanel(panel: { id: string; target?: MobKitDockTarget | null }) {
    const target = panel.target;
    if (!target || target.kind !== "agent-chat") {
      return null;
    }
    const panelKey = buildPanelConversationKey(panel.id, target);
    const agent = agents.find((candidate) => candidate.member_id === target.memberId) || null;
    const combinedFrames = mergeConversationFrames(
      historyFramesByKey[panelKey],
      panelFramesByKey[panelKey],
    );
    const entries = sortConversationTimelineEntries([
      ...(historyEntriesByKey[panelKey] || []),
      ...mapFramesToTimelineEntries(agent, combinedFrames, { renderInteractionStartsAsUser: true }),
      ...(localEntriesByKey[panelKey] || []),
    ]);
    const conversation = buildConversationViewState({
      memberId: target.memberId,
      agentLabel: target.title,
      agent,
      entries,
    });
    const draft = draftByKey[panelKey] || "";
    const isSending = sendingPanels.has(panelKey);
    const phase = panelPhaseByKey[panelKey] ?? agent?.response_phase ?? null;

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

  return (
    <div className="cc-theme-scope" data-cc-theme="dark" data-testid="meerkat-console">
      <SpriteSheet />
      <ConsoleWorkbench
        launcherResizeHandle={<div className="pane-resizer" aria-hidden="true" data-testid="resize:sidebar" onPointerDown={handleSidebarResize} />}
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
