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
import type { ConsoleComposerToolbarItem, ConversationTimelineEntry } from "@console-core";

import { normalizeAgents } from "./lib/agents";
import {
  buildActivityRailViewState,
  buildConversationViewState,
  buildDockTarget,
  buildSidebarViewState,
  createUserEntry,
  mapFramesToTimelineEntries,
  type MobKitDockTarget,
} from "./lib/adapters";
import { errorMessage } from "./lib/errors";
import { fetchJson, sendInteraction } from "./lib/network";
import { Icon, SpriteSheet } from "./icon";
import type {
  ConsoleAgent,
  ConsoleExperience,
  ConsoleFrame,
  ConsoleModulesResponse,
} from "./types";

// ---------------------------------------------------------------------------
// Console app
// ---------------------------------------------------------------------------

interface ConsoleAppProps {
  baseUrl: string;
}

export function ConsoleApp({ baseUrl }: ConsoleAppProps): React.JSX.Element {
  // ── Data state ──
  const [agents, setAgents] = React.useState<ConsoleAgent[]>([]);
  const [entriesByMemberId, setEntriesByMemberId] = React.useState<Record<string, ConversationTimelineEntry[]>>({});
  const [activityFrames, setActivityFrames] = React.useState<ConsoleFrame[]>([]);
  const [draftByMemberId, setDraftByMemberId] = React.useState<Record<string, string>>({});
  const [sendingMembers, setSendingMembers] = React.useState<Set<string>>(new Set());
  const [pinnedAgentIds, setPinnedAgentIds] = React.useState<Set<string>>(new Set());
  const [loading, setLoading] = React.useState(true);
  const [error, setError] = React.useState("");

  // ── Dock controller ──
  const dock = useConsoleDockController<MobKitDockTarget>({
    createPanelState: ({ target }) => ({
      id: `panel-${crypto.randomUUID()}`,
      target: target || null,
      mode: "console" as const,
    }),
  });

  // ── Load experience on mount ──
  React.useEffect(() => {
    let mounted = true;

    async function load() {
      setLoading(true);
      setError("");
      try {
        const [experienceJson, modulesJson] = await Promise.all([
          fetchJson<ConsoleExperience>(baseUrl, "/console/experience"),
          fetchJson<ConsoleModulesResponse>(baseUrl, "/console/modules"),
        ]);
        if (!mounted) return;

        const loadedModules = Array.isArray(modulesJson.modules)
          ? modulesJson.modules.map((moduleId) => String(moduleId))
          : [];
        const nextAgents = normalizeAgents(experienceJson, loadedModules);
        setAgents(nextAgents);

        // Open the first addressable agent in the dock
        const firstAddressable = nextAgents.find((a) =>
          a.addressable || a.affordances?.can_send_message
        ) || nextAgents[0];
        if (firstAddressable) {
          dock.openTarget(buildDockTarget(firstAddressable), "replace_focused");
        }
      } catch (loadError) {
        if (mounted) setError(errorMessage(loadError));
      } finally {
        if (mounted) setLoading(false);
      }
    }

    void load();
    return () => { mounted = false; };
  }, [baseUrl]); // eslint-disable-line react-hooks/exhaustive-deps

  // ── Select agent from sidebar → open in dock ──
  function onSelectAgent(
    _block: unknown,
    _section: unknown,
    item: { id: string },
  ) {
    const agent = agents.find((a) => a.member_id === item.id);
    if (agent) {
      dock.openTarget(buildDockTarget(agent), "replace_focused");
    }
  }

  // ── Send message to agent ──
  async function onSendMessage(memberId: string) {
    const text = (draftByMemberId[memberId] || "").trim();
    if (!text || !memberId) return;

    const agent = agents.find((a) => a.member_id === memberId) || null;

    // Clear draft and mark as sending
    setDraftByMemberId((d) => ({ ...d, [memberId]: "" }));
    setSendingMembers((s) => new Set(s).add(memberId));

    // Optimistic user entry
    const userEntry = createUserEntry(text);
    setEntriesByMemberId((current) => ({
      ...current,
      [memberId]: [...(current[memberId] || []), userEntry],
    }));

    try {
      const result = await sendInteraction(baseUrl, memberId, text);
      const agentEntries = mapFramesToTimelineEntries(agent, result.frames);

      setEntriesByMemberId((current) => ({
        ...current,
        [memberId]: [...(current[memberId] || []), ...agentEntries],
      }));

      setActivityFrames((current) => [...result.frames, ...current].slice(0, 64));
    } catch (submitError) {
      setError(errorMessage(submitError));
      // Roll back optimistic user entry
      setEntriesByMemberId((current) => ({
        ...current,
        [memberId]: (current[memberId] || []).filter((e) => e.id !== userEntry.id),
      }));
    } finally {
      setSendingMembers((s) => {
        const next = new Set(s);
        next.delete(memberId);
        return next;
      });
    }
  }

  // ── Sidebar resize (mirrors meerkat-app pattern) ──
  const SIDEBAR_MIN = 180;
  const SIDEBAR_MAX = 420;

  function handleSidebarResize(event: React.PointerEvent<HTMLDivElement>) {
    event.preventDefault();
    const startX = event.clientX;
    const root = event.currentTarget.closest("[data-console-workbench]") as HTMLElement | null;
    if (!root) return;

    const startWidth = parseInt(getComputedStyle(root).getPropertyValue("--cc-workbench-sidebar-width") || "260", 10) || 260;
    const handle = event.currentTarget;

    if ("setPointerCapture" in handle) {
      handle.setPointerCapture(event.pointerId);
    }
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

  // ── Activity rail resize ──
  const ACTIVITY_MIN = 200;
  const ACTIVITY_MAX = 480;

  function handleActivityResize(event: React.PointerEvent<HTMLDivElement>) {
    event.preventDefault();
    const startX = event.clientX;
    const root = event.currentTarget.closest("[data-console-workbench]") as HTMLElement | null;
    if (!root) return;

    const startWidth = parseInt(getComputedStyle(root).getPropertyValue("--cc-workbench-activity-width") || "280", 10) || 280;
    const handle = event.currentTarget;

    if ("setPointerCapture" in handle) {
      handle.setPointerCapture(event.pointerId);
    }
    document.documentElement.setAttribute("data-cc-resizing", "true");

    function onPointerMove(e: PointerEvent) {
      // Activity rail: dragging left makes it wider (reversed from sidebar)
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

  // ── Loading / error states ──
  if (loading) {
    return <div data-testid="console-loading">Loading console...</div>;
  }
  if (error) {
    return <div data-testid="console-error">{error}</div>;
  }

  // ── Build view states ──
  const focusedMemberId = dock.focusedTarget?.id || "";
  const sidebarVS = buildSidebarViewState({ agents, selectedMemberId: focusedMemberId, pinnedAgentIds });
  const activityVS = buildActivityRailViewState({ eventFrames: activityFrames });

  return (
    <div className="cc-theme-scope" data-cc-theme="dark" data-testid="meerkat-console">
      <SpriteSheet />
      <ConsoleWorkbench
        launcherResizeHandle={
          <div
            className="pane-resizer"
            aria-hidden="true"
            onPointerDown={handleSidebarResize}
          />
        }
        launcher={
          <ConsoleSidebar
            viewState={sidebarVS}
            Icon={Icon}
            onSelectItem={onSelectAgent}
            onItemAction={(_block, _section, item) => {
              // Toggle pin (only action on items)
              setPinnedAgentIds((current) => {
                const next = new Set(current);
                if (next.has(item.id)) {
                  next.delete(item.id);
                } else {
                  next.add(item.id);
                }
                return next;
              });
            }}
            onItemContextMenu={(_block, _section, item, event) => {
              event.preventDefault();
              // Open agent in dock on right-click as well
              const agent = agents.find((a) => a.member_id === item.id);
              if (agent) {
                dock.openTarget(buildDockTarget(agent), "replace_focused");
              }
            }}
          />
        }
        main={
          <ConsoleDock
            viewState={dock.viewState}
            Icon={Icon}
            onSelectTab={(tab) => dock.selectTab(tab.id)}
            onCloseTab={(tab) => dock.closeTab(tab.id)}
            onFocusPanel={(panel) => dock.focusPanel(panel.id)}
            onSplitPanel={(panel, dir) => dock.splitPanel(panel.id, dir)}
            onClosePanel={(panel) => dock.closePanel(panel.id)}
            onResizeSplit={(id, ratio) => dock.resizeSplit(id, ratio)}
            onCreateTab={() => dock.createTab()}
            renderPanelBody={(panel) => {
              const memberId = panel.target?.id || "";
              const entries = entriesByMemberId[memberId] || [];
              const vs = buildConversationViewState({
                memberId,
                agentLabel: panel.target?.title || "Agent",
                entries,
              });
              const draft = draftByMemberId[memberId] || "";
              const isSending = sendingMembers.has(memberId);
              const agent = agents.find((a) => a.member_id === memberId);
              const agentLabel = panel.target?.title || "Agent";
              const hasTarget = Boolean(memberId);

              const mainRowItems: ConsoleComposerToolbarItem[] = [];

              const footerLeftItems: ConsoleComposerToolbarItem[] = [
                { id: "target", kind: "sub-pill", label: `To: ${agentLabel}`, iconName: "i-team" },
                { id: "identity", kind: "sub-pill", label: agent?.member_id || memberId, iconName: "i-terminal" },
              ];

              const footerRightItems: ConsoleComposerToolbarItem[] = [
                { id: "profile", kind: "sub-pill", label: agent?.profile || "lead" },
                { id: "state", kind: "sub-pill", label: agent?.state || "unknown", iconName: "i-dot" },
              ];

              return (
                <ConversationPane
                  viewState={vs}
                  Icon={Icon}
                  footer={
                    <ConsoleComposer
                      Icon={Icon}
                      viewState={{
                        value: draft,
                        disabled: !hasTarget || isSending,
                        placeholder: hasTarget
                          ? `Message ${agentLabel}...`
                          : "Select an agent from the sidebar",
                        submitDisabled: !hasTarget || !draft.trim() || isSending,
                        submitLabel: hasTarget ? `Send to ${agentLabel}` : "Select an agent first",
                        mainRowItems,
                        footerLeftItems,
                        footerRightItems,
                      }}
                      onChange={(value) => setDraftByMemberId((d) => ({ ...d, [memberId]: value }))}
                      onSubmit={() => void onSendMessage(memberId)}
                      onKeyDown={(e) => {
                        if (e.key === "Enter" && !e.shiftKey) {
                          e.preventDefault();
                          void onSendMessage(memberId);
                        }
                      }}
                    />
                  }
                />
              );
            }}
          />
        }
        activityRailResizeHandle={
          <div
            className="pane-resizer pane-resizer--activity"
            aria-hidden="true"
            onPointerDown={handleActivityResize}
          />
        }
        activityRail={
          <ConsoleActivityRail
            viewState={activityVS}
            Icon={Icon}
            onTogglePicker={() => {}}
            onCollapse={() => {}}
            renderSlotPreview={() => null}
            onSelectItem={(focusId) => {
              const agent = agents.find((a) => a.member_id === focusId);
              if (agent) {
                dock.openTarget(buildDockTarget(agent), "replace_focused");
              }
            }}
          />
        }
      />
    </div>
  );
}
