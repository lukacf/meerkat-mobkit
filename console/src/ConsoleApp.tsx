import React from "react";
import { normalizeAgents } from "./lib/agents";
import { errorMessage } from "./lib/errors";
import { fetchJson, sendInteraction } from "./lib/network";
import { ActivityPanel } from "./panels/ActivityPanel";
import { AgentSidebarPanel } from "./panels/AgentSidebarPanel";
import { ChatInspectorPanel } from "./panels/ChatInspectorPanel";
import { HealthOverviewPanel } from "./panels/HealthOverviewPanel";
import { TopologyPanel } from "./panels/TopologyPanel";
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
  const [inspectorFrames, setInspectorFrames] = React.useState<ConsoleFrame[]>([]);

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
          setSelectedMemberId(nextAgents[0].member_id);
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

  async function onSubmit(event: React.FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const form = event.currentTarget;

    const memberControl = form.elements.namedItem("member") as
      | { value?: string }
      | null;
    const messageControl = form.elements.namedItem("message") as
      | { value?: string }
      | null;

    const submittedMemberId =
      memberControl?.value?.trim() || selectedMemberId;
    const trimmedMessage = messageControl?.value?.trim() || message.trim();
    if (!submittedMemberId || !trimmedMessage) {
      return;
    }

    setError("");
    try {
      const frames = await sendInteraction(baseUrl, submittedMemberId, trimmedMessage);
      setInspectorFrames(frames);
      setActivityFrames((previous) => [...frames, ...previous].slice(0, 64));
      setMessage("");
    } catch (submitError) {
      setError(errorMessage(submitError));
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

  return (
    <div data-testid="meerkat-console">
      <AgentSidebarPanel
        title={experience?.agent_sidebar?.title || "Agents"}
        agents={agents}
        onSelectMember={setSelectedMemberId}
      />
      <ActivityPanel
        title={experience?.activity_feed?.title || "Activity"}
        frames={activityFrames}
      />
      <ChatInspectorPanel
        title={experience?.chat_inspector?.title || "Chat Inspector"}
        agents={agents}
        selectedMemberId={selectedMemberId}
        onSelectedMemberIdChange={setSelectedMemberId}
        message={message}
        onMessageChange={setMessage}
        onSubmit={onSubmit}
        frames={inspectorFrames}
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
  );
}
