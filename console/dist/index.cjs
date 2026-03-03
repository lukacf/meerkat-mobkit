var __create = Object.create;
var __defProp = Object.defineProperty;
var __getOwnPropDesc = Object.getOwnPropertyDescriptor;
var __getOwnPropNames = Object.getOwnPropertyNames;
var __getProtoOf = Object.getPrototypeOf;
var __hasOwnProp = Object.prototype.hasOwnProperty;
var __export = (target, all) => {
  for (var name in all)
    __defProp(target, name, { get: all[name], enumerable: true });
};
var __copyProps = (to, from, except, desc) => {
  if (from && typeof from === "object" || typeof from === "function") {
    for (let key of __getOwnPropNames(from))
      if (!__hasOwnProp.call(to, key) && key !== except)
        __defProp(to, key, { get: () => from[key], enumerable: !(desc = __getOwnPropDesc(from, key)) || desc.enumerable });
  }
  return to;
};
var __toESM = (mod, isNodeMode, target) => (target = mod != null ? __create(__getProtoOf(mod)) : {}, __copyProps(
  // If the importer is in node compatibility mode or this is not an ESM
  // file that has been converted to a CommonJS file using a Babel-
  // compatible transform (i.e. "__esModule" has not been set), then set
  // "default" to the CommonJS "module.exports" for node compatibility.
  isNodeMode || !mod || !mod.__esModule ? __defProp(target, "default", { value: mod, enumerable: true }) : target,
  mod
));
var __toCommonJS = (mod) => __copyProps(__defProp({}, "__esModule", { value: true }), mod);

// src/index.tsx
var index_exports = {};
__export(index_exports, {
  ConsoleApp: () => ConsoleApp,
  createConsoleApp: () => createConsoleApp,
  parseSseFrames: () => parseSseFrames
});
module.exports = __toCommonJS(index_exports);
var import_react7 = __toESM(require("react"));
var import_client = require("react-dom/client");

// src/ConsoleApp.tsx
var import_react6 = __toESM(require("react"));

// src/lib/agents.ts
function normalizeAgents(experience, modules) {
  const snapshotAgents = experience?.agent_sidebar?.live_snapshot?.agents;
  if (Array.isArray(snapshotAgents) && snapshotAgents.length > 0) {
    return snapshotAgents.map((entry) => ({
      agent_id: String(entry.agent_id || entry.member_id || ""),
      member_id: String(entry.member_id || entry.agent_id || ""),
      label: String(entry.label || entry.member_id || entry.agent_id || "unknown"),
      kind: String(entry.kind || "module_agent")
    }));
  }
  if (Array.isArray(modules) && modules.length > 0) {
    return modules.map((moduleId) => ({
      agent_id: String(moduleId),
      member_id: String(moduleId),
      label: String(moduleId),
      kind: "module_agent"
    }));
  }
  return [];
}

// src/lib/errors.ts
function errorMessage(error) {
  if (error instanceof Error) {
    return error.message;
  }
  return String(error);
}

// src/lib/network.ts
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
    body: JSON.stringify({ member_id: memberId, message })
  });
  if (!response.ok) {
    const text2 = await response.text();
    throw new Error(`interaction request failed ${response.status}: ${text2}`);
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
      if (text.length > 16384) {
        break;
      }
    }
  } finally {
    try {
      await reader.cancel();
    } catch (_) {
    }
  }
  return parseSseFrames(text);
}

// src/panels/ActivityPanel.tsx
var import_react = __toESM(require("react"));
function ActivityPanel({ title, frames }) {
  return /* @__PURE__ */ import_react.default.createElement("section", { "data-testid": "activity-panel" }, /* @__PURE__ */ import_react.default.createElement("h2", null, title), /* @__PURE__ */ import_react.default.createElement("ul", { "data-testid": "activity-feed" }, frames.map((frame, index) => /* @__PURE__ */ import_react.default.createElement("li", { key: `${frame.id || frame.event || "event"}:${index}` }, `${frame.event || "message"} ${frame.id || ""}`.trim()))));
}

// src/panels/AgentSidebarPanel.tsx
var import_react2 = __toESM(require("react"));
function AgentSidebarPanel({
  title,
  agents,
  onSelectMember
}) {
  return /* @__PURE__ */ import_react2.default.createElement("section", { "data-testid": "agent-sidebar" }, /* @__PURE__ */ import_react2.default.createElement("h2", null, title), /* @__PURE__ */ import_react2.default.createElement("ul", { "data-testid": "sidebar-list" }, agents.map((agent) => /* @__PURE__ */ import_react2.default.createElement("li", { key: agent.member_id }, /* @__PURE__ */ import_react2.default.createElement(
    "button",
    {
      type: "button",
      "data-agent-id": agent.agent_id,
      onClick: () => onSelectMember(agent.member_id)
    },
    agent.label
  )))));
}

// src/panels/ChatInspectorPanel.tsx
var import_react3 = __toESM(require("react"));
function ChatInspectorPanel({
  title,
  agents,
  selectedMemberId,
  onSelectedMemberIdChange,
  message,
  onMessageChange,
  onSubmit,
  frames
}) {
  return /* @__PURE__ */ import_react3.default.createElement("section", { "data-testid": "chat-inspector" }, /* @__PURE__ */ import_react3.default.createElement("h2", null, title), /* @__PURE__ */ import_react3.default.createElement("form", { "data-testid": "chat-form", onSubmit }, /* @__PURE__ */ import_react3.default.createElement("label", null, "Member", /* @__PURE__ */ import_react3.default.createElement(
    "select",
    {
      name: "member",
      value: selectedMemberId,
      onChange: (event) => onSelectedMemberIdChange(event.target.value)
    },
    agents.map((agent) => /* @__PURE__ */ import_react3.default.createElement("option", { key: agent.member_id, value: agent.member_id }, agent.member_id))
  )), /* @__PURE__ */ import_react3.default.createElement("label", null, "Message", /* @__PURE__ */ import_react3.default.createElement(
    "textarea",
    {
      name: "message",
      value: message,
      onChange: (event) => onMessageChange(event.target.value)
    }
  )), /* @__PURE__ */ import_react3.default.createElement("button", { type: "submit" }, "Send")), /* @__PURE__ */ import_react3.default.createElement("ul", { "data-testid": "chat-events" }, frames.map((frame, index) => /* @__PURE__ */ import_react3.default.createElement("li", { key: `${frame.id || frame.event || "frame"}:${index}` }, `${frame.event || "message"} ${frame.id || ""}`.trim()))));
}

// src/panels/HealthOverviewPanel.tsx
var import_react4 = __toESM(require("react"));
function HealthOverviewPanel({
  title,
  running,
  loadedModuleCount,
  loadedModules
}) {
  return /* @__PURE__ */ import_react4.default.createElement("section", { "data-testid": "health-overview" }, /* @__PURE__ */ import_react4.default.createElement("h2", null, title), /* @__PURE__ */ import_react4.default.createElement("p", { "data-testid": "health-running" }, `Running: ${running === null ? "unknown" : String(running)}`), /* @__PURE__ */ import_react4.default.createElement("p", { "data-testid": "health-loaded-module-count" }, `Loaded module count: ${loadedModuleCount}`), /* @__PURE__ */ import_react4.default.createElement("ul", { "data-testid": "health-loaded-modules" }, loadedModules.map((moduleId) => /* @__PURE__ */ import_react4.default.createElement("li", { key: moduleId }, moduleId))));
}

// src/panels/TopologyPanel.tsx
var import_react5 = __toESM(require("react"));
function TopologyPanel({ title, nodeCount, nodes }) {
  return /* @__PURE__ */ import_react5.default.createElement("section", { "data-testid": "topology-panel" }, /* @__PURE__ */ import_react5.default.createElement("h2", null, title), /* @__PURE__ */ import_react5.default.createElement("p", { "data-testid": "topology-node-count" }, `Node count: ${nodeCount}`), /* @__PURE__ */ import_react5.default.createElement("ul", { "data-testid": "topology-nodes" }, nodes.map((moduleId) => /* @__PURE__ */ import_react5.default.createElement("li", { key: moduleId }, moduleId))));
}

// src/ConsoleApp.tsx
function ConsoleApp({ baseUrl }) {
  const [experience, setExperience] = import_react6.default.useState(null);
  const [agents, setAgents] = import_react6.default.useState([]);
  const [selectedMemberId, setSelectedMemberId] = import_react6.default.useState("");
  const [message, setMessage] = import_react6.default.useState("");
  const [loading, setLoading] = import_react6.default.useState(true);
  const [error, setError] = import_react6.default.useState("");
  const [activityFrames, setActivityFrames] = import_react6.default.useState([]);
  const [inspectorFrames, setInspectorFrames] = import_react6.default.useState([]);
  import_react6.default.useEffect(() => {
    let mounted = true;
    async function load() {
      setLoading(true);
      setError("");
      try {
        const [experienceJson, modulesJson] = await Promise.all([
          fetchJson(baseUrl, "/console/experience"),
          fetchJson(baseUrl, "/console/modules")
        ]);
        if (!mounted) {
          return;
        }
        const loadedModules2 = Array.isArray(modulesJson.modules) ? modulesJson.modules.map((moduleId) => String(moduleId)) : [];
        const nextAgents = normalizeAgents(experienceJson, loadedModules2);
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
  async function onSubmit(event) {
    event.preventDefault();
    const form = event.currentTarget;
    const memberControl = form.elements.namedItem("member");
    const messageControl = form.elements.namedItem("message");
    const submittedMemberId = memberControl?.value?.trim() || selectedMemberId;
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
    return /* @__PURE__ */ import_react6.default.createElement("div", { "data-testid": "console-loading" }, "Loading console...");
  }
  if (error) {
    return /* @__PURE__ */ import_react6.default.createElement("div", { "data-testid": "console-error" }, error);
  }
  const topologySnapshot = experience?.topology?.live_snapshot || {};
  const topologyNodes = Array.isArray(topologySnapshot.nodes) ? topologySnapshot.nodes.map((node) => String(node)) : [];
  const topologyNodeCount = Number.isFinite(topologySnapshot.node_count) ? topologySnapshot.node_count : topologyNodes.length;
  const healthSnapshot = experience?.health_overview?.live_snapshot || {};
  const loadedModules = Array.isArray(healthSnapshot.loaded_modules) ? healthSnapshot.loaded_modules.map((moduleId) => String(moduleId)) : [];
  const loadedModuleCount = Number.isFinite(healthSnapshot.loaded_module_count) ? healthSnapshot.loaded_module_count : loadedModules.length;
  const running = typeof healthSnapshot.running === "boolean" ? healthSnapshot.running : null;
  return /* @__PURE__ */ import_react6.default.createElement("div", { "data-testid": "meerkat-console" }, /* @__PURE__ */ import_react6.default.createElement(
    AgentSidebarPanel,
    {
      title: experience?.agent_sidebar?.title || "Agents",
      agents,
      onSelectMember: setSelectedMemberId
    }
  ), /* @__PURE__ */ import_react6.default.createElement(
    ActivityPanel,
    {
      title: experience?.activity_feed?.title || "Activity",
      frames: activityFrames
    }
  ), /* @__PURE__ */ import_react6.default.createElement(
    ChatInspectorPanel,
    {
      title: experience?.chat_inspector?.title || "Chat Inspector",
      agents,
      selectedMemberId,
      onSelectedMemberIdChange: setSelectedMemberId,
      message,
      onMessageChange: setMessage,
      onSubmit,
      frames: inspectorFrames
    }
  ), /* @__PURE__ */ import_react6.default.createElement(
    TopologyPanel,
    {
      title: experience?.topology?.title || "Topology",
      nodeCount: topologyNodeCount,
      nodes: topologyNodes
    }
  ), /* @__PURE__ */ import_react6.default.createElement(
    HealthOverviewPanel,
    {
      title: experience?.health_overview?.title || "Health",
      running,
      loadedModuleCount,
      loadedModules
    }
  ));
}

// src/index.tsx
function createConsoleApp(target, options = {}) {
  if (!target) {
    throw new Error("target element is required");
  }
  const baseUrl = options.baseUrl || "";
  const root = (0, import_client.createRoot)(target);
  root.render(/* @__PURE__ */ import_react7.default.createElement(ConsoleApp, { baseUrl }));
  return {
    unmount() {
      root.unmount();
    }
  };
}
