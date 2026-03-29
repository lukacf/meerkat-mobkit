import React from "react";
import { normalizeAgents } from "./lib/agents";
import {
  buildAgentSidebarViewState,
  buildConversationViewState,
  createUserConversationEntry,
  mapFramesToConversationEntries,
} from "./lib/console-adapters";
import { errorMessage } from "./lib/errors";
import { fetchJson, queryEvents, sendInteraction } from "./lib/network";
import { ActivityPanel } from "./panels/ActivityPanel";
import { HealthOverviewPanel } from "./panels/HealthOverviewPanel";
import { TopologyPanel } from "./panels/TopologyPanel";
import {
  ConsoleSidebar,
  ConsoleWorkbench,
  ConversationPane,
  type ConversationEntry,
} from "./shared-console";
import type {
  ConsoleAgent,
  ConsoleExperience,
  ConsoleFrame,
  ConsoleModulesResponse,
} from "./types";

interface ConsoleAppProps {
  baseUrl: string;
}

export function ConsoleApp({ baseUrl }: ConsoleAppProps): React.JSX.Element {
  const [experience, setExperience] = React.useState<ConsoleExperience | null>(null);
  const [agents, setAgents] = React.useState<ConsoleAgent[]>([]);
  const [selectedMemberId, setSelectedMemberId] = React.useState("");
  const [message, setMessage] = React.useState("");
  const [loading, setLoading] = React.useState(true);
  const [error, setError] = React.useState("");
  const [activityFrames, setActivityFrames] = React.useState<ConsoleFrame[]>([]);
  const [framesByMemberId, setFramesByMemberId] = React.useState<Record<string, ConsoleFrame[]>>({});
  const [entriesByMemberId, setEntriesByMemberId] = React.useState<Record<string, ConversationEntry[]>>({});
  const [historyLoadedByMemberId, setHistoryLoadedByMemberId] = React.useState<Record<string, boolean>>({});

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
        if (!mounted) {
          return;
        }

        const loadedModules = Array.isArray(modulesJson.modules)
          ? modulesJson.modules.map((moduleId) => String(moduleId))
          : [];
        const nextAgents = normalizeAgents(experienceJson, loadedModules);

        setExperience(experienceJson);
        setAgents(nextAgents);
        if (nextAgents.length > 0) {
          setSelectedMemberId((current) => current || nextAgents[0]?.member_id || "");
        }
      } catch (loadError) {
        if (!mounted) {
          return;
        }
        setError(errorMessage(loadError));
      } finally {
        if (mounted) {
          setLoading(false);
        }
      }
    }

    void load();
    return () => {
      mounted = false;
    };
  }, [baseUrl]);

  const selectedAgent = React.useMemo(
    () => agents.find((agent) => agent.member_id === selectedMemberId) || null,
    [agents, selectedMemberId],
  );

  React.useEffect(() => {
    if (!selectedMemberId || historyLoadedByMemberId[selectedMemberId]) {
      return;
    }

    let cancelled = false;

    async function loadHistory() {
      try {
        const frames = await queryEvents(baseUrl, selectedMemberId, 40);
        if (cancelled) {
          return;
        }

        // Prepend history before any live frames. History contains only
        // module events (queryEvents filters agent-kind rows); live SSE emits
        // only agent events. The two sets are disjoint — no dedup needed.
        setFramesByMemberId((current) => ({
          ...current,
          [selectedMemberId]: [...frames, ...(current[selectedMemberId] || [])],
        }));
        setEntriesByMemberId((current) => ({
          ...current,
          [selectedMemberId]: [
            ...mapFramesToConversationEntries(selectedAgent, frames),
            ...(current[selectedMemberId] || []),
          ],
        }));
      } catch (_) {
        // History is optional; the console still works with live interaction frames only.
      } finally {
        if (!cancelled) {
          setHistoryLoadedByMemberId((current) => ({
            ...current,
            [selectedMemberId]: true,
          }));
        }
      }
    }

    void loadHistory();
    return () => {
      cancelled = true;
    };
  }, [baseUrl, historyLoadedByMemberId, selectedAgent, selectedMemberId]);

  async function onSubmit(event: React.FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const trimmedMessage = message.trim();
    if (!selectedMemberId || !trimmedMessage) {
      return;
    }

    const userEntry = createUserConversationEntry(trimmedMessage);
    setError("");
    setEntriesByMemberId((current) => ({
      ...current,
      [selectedMemberId]: [...(current[selectedMemberId] || []), userEntry],
    }));

    try {
      const result = await sendInteraction(baseUrl, selectedMemberId, trimmedMessage);
      const nextEntries = mapFramesToConversationEntries(selectedAgent, result.frames);

      setFramesByMemberId((current) => ({
        ...current,
        [selectedMemberId]: [...(current[selectedMemberId] || []), ...result.frames],
      }));
      setEntriesByMemberId((current) => ({
        ...current,
        [selectedMemberId]: [...(current[selectedMemberId] || []), ...nextEntries],
      }));
      setActivityFrames((current) => [...result.frames, ...current].slice(0, 64));
      // Invalidate history cache so the next visit re-fetches including this turn.
      setHistoryLoadedByMemberId((current) => ({
        ...current,
        [selectedMemberId]: false,
      }));
      setMessage("");
    } catch (submitError) {
      setError(errorMessage(submitError));
      // Roll back the optimistic user entry — the backend never accepted the message.
      setEntriesByMemberId((current) => ({
        ...current,
        [selectedMemberId]: (current[selectedMemberId] || []).filter(
          (e) => e.id !== userEntry.id,
        ),
      }));
    }
  }

  if (loading) {
    return <div data-testid="console-loading">Loading console...</div>;
  }

  if (error) {
    return <div data-testid="console-error">{error}</div>;
  }

  const topologySnapshot = experience?.topology?.live_snapshot || {};
  const topologyNodes = Array.isArray(topologySnapshot.nodes)
    ? topologySnapshot.nodes.map((node) => String(node))
    : [];
  const topologyNodeCount = Number.isFinite(topologySnapshot.node_count)
    ? (topologySnapshot.node_count as number)
    : topologyNodes.length;

  const healthSnapshot = experience?.health_overview?.live_snapshot || {};
  const loadedModules = Array.isArray(healthSnapshot.loaded_modules)
    ? healthSnapshot.loaded_modules.map((moduleId) => String(moduleId))
    : [];
  const loadedModuleCount = Number.isFinite(healthSnapshot.loaded_module_count)
    ? (healthSnapshot.loaded_module_count as number)
    : loadedModules.length;
  const running =
    typeof healthSnapshot.running === "boolean"
      ? healthSnapshot.running
      : null;

  const sidebarViewState = buildAgentSidebarViewState({
    title: experience?.agent_sidebar?.title || "Agents",
    agents,
    selectedMemberId,
  });
  const conversationViewState = buildConversationViewState({
    conversationId: selectedMemberId || "console",
    title: selectedAgent?.label || (experience?.chat_inspector?.title || "Chat Inspector"),
    entries: selectedMemberId ? (entriesByMemberId[selectedMemberId] || []) : [],
    selectedAgentLabel: selectedAgent?.label || selectedMemberId || "an agent",
  });

  return (
    <div data-testid="meerkat-console">
      <ConsoleWorkbench
        main={(
          <ConversationPane
            footer={(
              <form className="mc-composer" data-testid="chat-form" onSubmit={onSubmit}>
                <div className="mc-composer__header">
                  <span className="mc-composer__eyebrow">Target</span>
                  <span className="mc-composer__target">{selectedAgent?.label || "Select an agent"}</span>
                </div>
                <label className="mc-composer__field">
                  <span className="mc-composer__label">Message</span>
                  <textarea
                    name="message"
                    placeholder={selectedAgent ? `Message ${selectedAgent.label}` : "Select an agent to start"}
                    value={message}
                    onChange={(changeEvent) => setMessage(changeEvent.target.value)}
                  />
                </label>
                <div className="mc-composer__actions">
                  <button disabled={!selectedMemberId || !message.trim()} type="submit">Send</button>
                </div>
              </form>
            )}
            viewState={conversationViewState}
          />
        )}
        sidebar={(
          <ConsoleSidebar
            getItemButtonProps={(item) => ({
              "data-agent-id": agents.find((agent) => agent.member_id === item.id)?.agent_id || item.id,
            })}
            onSelectItem={(item) => setSelectedMemberId(item.id)}
            viewState={sidebarViewState}
          />
        )}
      />

      <div className="mc-dashboard">
        <ActivityPanel
          title={experience?.activity_feed?.title || "Activity"}
          frames={activityFrames}
        />
        <TopologyPanel
          title={experience?.topology?.title || "Topology"}
          nodeCount={topologyNodeCount}
          nodes={topologyNodes}
        />
        <HealthOverviewPanel
          title={experience?.health_overview?.title || "Health"}
          running={running}
          loadedModuleCount={loadedModuleCount}
          loadedModules={loadedModules}
        />
      </div>
    </div>
  );
}
