import React from "react";
import "@console-components/styles";
import "./console-host.css";

import {
  ConsoleActivityRail,
  ConsoleDock,
  ConsoleSidebar,
  ConsoleWorkbench,
  ConversationPane,
  useConsoleDockController,
} from "@console-components";
import type { ConversationTimelineEntry } from "@console-core";

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
import { Icon } from "./icon";
import type {
  ConsoleAgent,
  ConsoleExperience,
  ConsoleFrame,
  ConsoleModulesResponse,
} from "./types";

// ---------------------------------------------------------------------------
// Chat composer (inline subcomponent)
// ---------------------------------------------------------------------------

function ChatComposer({
  agentLabel,
  disabled,
  onSend,
}: {
  agentLabel: string;
  disabled: boolean;
  onSend: (text: string) => void;
}) {
  const [message, setMessage] = React.useState("");

  function handleSubmit(event: React.FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const trimmed = message.trim();
    if (!trimmed || disabled) return;
    onSend(trimmed);
    setMessage("");
  }

  return (
    <form className="mc-composer" data-testid="chat-form" onSubmit={handleSubmit}>
      <textarea
        name="message"
        placeholder={`Message ${agentLabel}...`}
        value={message}
        onChange={(e) => setMessage(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter" && !e.shiftKey) {
            e.preventDefault();
            handleSubmit(e as unknown as React.FormEvent<HTMLFormElement>);
          }
        }}
      />
      <div className="mc-composer__actions">
        <button disabled={disabled || !message.trim()} type="submit">Send</button>
      </div>
    </form>
  );
}

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
  async function onSendMessage(memberId: string, text: string) {
    const agent = agents.find((a) => a.member_id === memberId) || null;

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
    }
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
  const sidebarVS = buildSidebarViewState({ agents, selectedMemberId: focusedMemberId });
  const activityVS = buildActivityRailViewState({ agents, eventFrames: activityFrames });

  return (
    <div className="cc-theme-scope" data-cc-theme="dark" data-testid="meerkat-console">
      <ConsoleWorkbench
        launcher={
          <ConsoleSidebar
            viewState={sidebarVS}
            Icon={Icon}
            onSelectItem={onSelectAgent}
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
              return (
                <ConversationPane
                  viewState={vs}
                  Icon={Icon}
                  footer={
                    <ChatComposer
                      agentLabel={panel.target?.title || "Agent"}
                      disabled={!memberId}
                      onSend={(text) => void onSendMessage(memberId, text)}
                    />
                  }
                />
              );
            }}
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
