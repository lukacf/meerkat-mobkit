const React = require("react");
const { createRoot } = require("react-dom/client");

const e = React.createElement;

function parseSseFrames(rawText) {
  const blocks = rawText.split(/\n\n+/).map((part) => part.trim()).filter(Boolean);
  const frames = [];

  for (const block of blocks) {
    const lines = block.split("\n");
    let id = "";
    let event = "message";
    const dataLines = [];

    for (const line of lines) {
      if (line.startsWith("id:")) {
        id = line.slice(3).trim();
        continue;
      }
      if (line.startsWith("event:")) {
        event = line.slice(6).trim();
        continue;
      }
      if (line.startsWith("data:")) {
        dataLines.push(line.slice(5).trim());
      }
    }

    if (!id && dataLines.length === 0) {
      continue;
    }

    const rawData = dataLines.join("\n");
    let data = rawData;
    if (rawData) {
      try {
        data = JSON.parse(rawData);
      } catch (_) {
        data = rawData;
      }
    }

    frames.push({ id, event, data });
  }

  return frames;
}

function normalizeAgents(experience, modules) {
  const snapshotAgents = experience?.agent_sidebar?.live_snapshot?.agents;
  if (Array.isArray(snapshotAgents) && snapshotAgents.length > 0) {
    return snapshotAgents.map((entry) => ({
      agent_id: String(entry.agent_id || entry.member_id || ""),
      member_id: String(entry.member_id || entry.agent_id || ""),
      label: String(entry.label || entry.member_id || entry.agent_id || "unknown"),
      kind: String(entry.kind || "module_agent"),
    }));
  }

  if (Array.isArray(modules) && modules.length > 0) {
    return modules.map((moduleId) => ({
      agent_id: String(moduleId),
      member_id: String(moduleId),
      label: String(moduleId),
      kind: "module_agent",
    }));
  }

  return [];
}

async function fetchJson(baseUrl, path) {
  const response = await fetch(`${baseUrl}${path}`);
  if (!response.ok) {
    const text = await response.text();
    throw new Error(`Request failed ${response.status} for ${path}: ${text}`);
  }
  return response.json();
}

async function sendInteraction(baseUrl, memberId, message) {
  const response = await fetch(`${baseUrl}/interactions/stream`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ member_id: memberId, message }),
  });

  if (!response.ok) {
    const text = await response.text();
    throw new Error(`interaction request failed ${response.status}: ${text}`);
  }

  if (!response.body || typeof response.body.getReader !== "function") {
    return parseSseFrames(await response.text());
  }

  const reader = response.body.getReader();
  const decoder = new TextDecoder();
  let text = "";
  try {
    while (!text.includes("\n\n")) {
      const { value, done } = await reader.read();
      if (done) {
        break;
      }
      text += decoder.decode(value, { stream: true });
      if (text.length > 16_384) {
        break;
      }
    }
  } finally {
    try {
      await reader.cancel();
    } catch (_) {
      // No-op: stream may already be closed.
    }
  }
  return parseSseFrames(text);
}

function ConsoleApp({ baseUrl }) {
  const [experience, setExperience] = React.useState(null);
  const [modules, setModules] = React.useState([]);
  const [agents, setAgents] = React.useState([]);
  const [selectedMemberId, setSelectedMemberId] = React.useState("");
  const [message, setMessage] = React.useState("");
  const [loading, setLoading] = React.useState(true);
  const [error, setError] = React.useState("");
  const [activityFrames, setActivityFrames] = React.useState([]);
  const [inspectorFrames, setInspectorFrames] = React.useState([]);

  React.useEffect(() => {
    let mounted = true;

    async function load() {
      setLoading(true);
      setError("");
      try {
        const [experienceJson, modulesJson] = await Promise.all([
          fetchJson(baseUrl, "/console/experience"),
          fetchJson(baseUrl, "/console/modules"),
        ]);
        if (!mounted) {
          return;
        }
        const loadedModules = Array.isArray(modulesJson.modules) ? modulesJson.modules : [];
        const nextAgents = normalizeAgents(experienceJson, loadedModules);

        setExperience(experienceJson);
        setModules(loadedModules);
        setAgents(nextAgents);
        if (nextAgents.length > 0) {
          setSelectedMemberId(nextAgents[0].member_id);
        }
      } catch (loadError) {
        if (!mounted) {
          return;
        }
        setError(loadError.message);
      } finally {
        if (mounted) {
          setLoading(false);
        }
      }
    }

    load();
    return () => {
      mounted = false;
    };
  }, [baseUrl]);

  async function onSubmit(event) {
    event.preventDefault();
    const form = event.currentTarget;
    const submittedMemberId =
      form.elements.member?.value?.trim() || selectedMemberId;
    const trimmedMessage =
      form.elements.message?.value?.trim() || message.trim();
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
      setError(submitError.message);
    }
  }

  if (loading) {
    return e("div", { "data-testid": "console-loading" }, "Loading console...");
  }

  if (error) {
    return e("div", { "data-testid": "console-error" }, error);
  }

  const sidebarItems = agents.map((agent) =>
    e(
      "li",
      { key: agent.member_id },
      e(
        "button",
        {
          type: "button",
          "data-agent-id": agent.agent_id,
          onClick: () => setSelectedMemberId(agent.member_id),
        },
        agent.label
      )
    )
  );

  const feedItems = activityFrames.map((frame, index) =>
    e(
      "li",
      {
        key: `${frame.id || frame.event || "event"}:${index}`,
      },
      `${frame.event || "message"} ${frame.id || ""}`.trim()
    )
  );

  const inspectorItems = inspectorFrames.map((frame, index) =>
    e(
      "li",
      {
        key: `${frame.id || frame.event || "frame"}:${index}`,
      },
      `${frame.event || "message"} ${frame.id || ""}`.trim()
    )
  );

  const topologySnapshot = experience?.topology?.live_snapshot || {};
  const topologyNodes = Array.isArray(topologySnapshot.nodes)
    ? topologySnapshot.nodes.map((node) => String(node))
    : [];
  const topologyNodeCount = Number.isFinite(topologySnapshot.node_count)
    ? topologySnapshot.node_count
    : topologyNodes.length;

  const healthSnapshot = experience?.health_overview?.live_snapshot || {};
  const loadedModules = Array.isArray(healthSnapshot.loaded_modules)
    ? healthSnapshot.loaded_modules.map((moduleId) => String(moduleId))
    : [];
  const loadedModuleCount = Number.isFinite(healthSnapshot.loaded_module_count)
    ? healthSnapshot.loaded_module_count
    : loadedModules.length;
  const running =
    typeof healthSnapshot.running === "boolean"
      ? healthSnapshot.running
      : null;

  return e(
    "div",
    { "data-testid": "meerkat-console" },
    e(
      "section",
      { "data-testid": "agent-sidebar" },
      e("h2", null, experience?.agent_sidebar?.title || "Agents"),
      e("ul", { "data-testid": "sidebar-list" }, sidebarItems)
    ),
    e(
      "section",
      { "data-testid": "activity-panel" },
      e("h2", null, experience?.activity_feed?.title || "Activity"),
      e("ul", { "data-testid": "activity-feed" }, feedItems)
    ),
    e(
      "section",
      { "data-testid": "chat-inspector" },
      e("h2", null, experience?.chat_inspector?.title || "Chat Inspector"),
      e(
        "form",
        { "data-testid": "chat-form", onSubmit },
        e(
          "label",
          null,
          "Member",
          e(
            "select",
            {
              name: "member",
              value: selectedMemberId,
              onChange: (event) => setSelectedMemberId(event.target.value),
            },
            agents.map((agent) =>
              e(
                "option",
                { key: agent.member_id, value: agent.member_id },
                agent.member_id
              )
            )
          )
        ),
        e(
          "label",
          null,
          "Message",
          e("textarea", {
            name: "message",
            value: message,
            onChange: (event) => setMessage(event.target.value),
          })
        ),
        e("button", { type: "submit" }, "Send")
      ),
      e("ul", { "data-testid": "chat-events" }, inspectorItems)
    ),
    e(
      "section",
      { "data-testid": "topology-panel" },
      e("h2", null, experience?.topology?.title || "Topology"),
      e("p", { "data-testid": "topology-node-count" }, `Node count: ${topologyNodeCount}`),
      e(
        "ul",
        { "data-testid": "topology-nodes" },
        topologyNodes.map((moduleId) =>
          e("li", { key: moduleId }, moduleId)
        )
      )
    ),
    e(
      "section",
      { "data-testid": "health-overview" },
      e("h2", null, experience?.health_overview?.title || "Health"),
      e(
        "p",
        { "data-testid": "health-running" },
        `Running: ${running === null ? "unknown" : String(running)}`
      ),
      e(
        "p",
        { "data-testid": "health-loaded-module-count" },
        `Loaded module count: ${loadedModuleCount}`
      ),
      e(
        "ul",
        { "data-testid": "health-loaded-modules" },
        loadedModules.map((moduleId) =>
          e("li", { key: moduleId }, moduleId)
        )
      )
    )
  );
}

function createConsoleApp(target, options = {}) {
  if (!target) {
    throw new Error("target element is required");
  }

  const baseUrl = options.baseUrl || "";
  const root = createRoot(target);
  root.render(e(ConsoleApp, { baseUrl }));
  return {
    unmount() {
      root.unmount();
    },
  };
}

module.exports = {
  ConsoleApp,
  createConsoleApp,
  parseSseFrames,
};
