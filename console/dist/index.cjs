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
var import_react6 = __toESM(require("react"));
var import_client = require("react-dom/client");

// src/ConsoleApp.tsx
var import_react5 = __toESM(require("react"));

// src/lib/agents.ts
function normalizeAgents(experience, modules) {
  const snapshotAgents = experience?.agent_sidebar?.live_snapshot?.agents;
  if (Array.isArray(snapshotAgents) && snapshotAgents.length > 0) {
    return snapshotAgents.map((entry) => ({
      agent_id: String(entry.agent_id || entry.member_id || ""),
      member_id: String(entry.member_id || entry.agent_id || ""),
      label: String(entry.label || entry.member_id || entry.agent_id || "unknown"),
      kind: String(entry.kind || "module_agent"),
      ...entry.profile !== void 0 && { profile: String(entry.profile) },
      ...entry.state !== void 0 && { state: String(entry.state) },
      ...entry.wired_to !== void 0 && { wired_to: entry.wired_to },
      ...entry.labels !== void 0 && { labels: entry.labels },
      ...entry.group !== void 0 && { group: String(entry.group) },
      ...entry.addressable !== void 0 && { addressable: Boolean(entry.addressable) },
      ...entry.affordances !== void 0 && { affordances: entry.affordances }
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

// src/shared-console.tsx
var import_react = __toESM(require("react"));
function toneClass(tone) {
  switch (tone) {
    case "accent":
      return "is-accent";
    case "positive":
      return "is-positive";
    case "negative":
      return "is-negative";
    case "muted":
      return "is-muted";
    default:
      return "";
  }
}
function groupConversationEntries(entries) {
  const groups = [];
  for (const entry of entries) {
    const current = groups[groups.length - 1];
    if (!current || current.identity.id !== entry.identity.id || current.identity.presentation !== entry.identity.presentation) {
      groups.push({
        id: `${entry.identity.id}:${entry.id}`,
        identity: entry.identity,
        entries: [entry]
      });
      continue;
    }
    current.entries.push(entry);
  }
  return groups;
}
function ConsoleWorkbench({
  sidebar,
  main
}) {
  return /* @__PURE__ */ import_react.default.createElement("section", { className: "mc-workbench" }, /* @__PURE__ */ import_react.default.createElement("aside", { className: "mc-workbench__sidebar" }, sidebar), /* @__PURE__ */ import_react.default.createElement("section", { className: "mc-workbench__main" }, main));
}
function ConsoleSidebar({
  viewState,
  onSelectItem,
  getItemButtonProps
}) {
  return /* @__PURE__ */ import_react.default.createElement("section", { className: "mc-sidebar", "data-testid": "agent-sidebar" }, viewState.blocks.map((block) => /* @__PURE__ */ import_react.default.createElement("div", { className: "mc-sidebar__block", key: block.id }, block.title ? /* @__PURE__ */ import_react.default.createElement("div", { className: "mc-sidebar__block-title" }, block.title) : null, /* @__PURE__ */ import_react.default.createElement("div", { className: "mc-sidebar__sections", "data-testid": "sidebar-list" }, block.sections.map((section) => /* @__PURE__ */ import_react.default.createElement("div", { className: "mc-sidebar__section", key: section.id }, /* @__PURE__ */ import_react.default.createElement("div", { className: "mc-sidebar__section-title" }, section.title), /* @__PURE__ */ import_react.default.createElement("div", { className: "mc-sidebar__items" }, section.items.map((item) => {
    const buttonProps = getItemButtonProps?.(item) || {};
    return /* @__PURE__ */ import_react.default.createElement(
      "button",
      {
        ...buttonProps,
        className: `mc-sidebar__item${item.selected ? " is-selected" : ""}${buttonProps.className ? ` ${buttonProps.className}` : ""}`,
        key: item.id,
        type: "button",
        onClick: (event) => {
          buttonProps.onClick?.(event);
          if (!event.defaultPrevented) {
            onSelectItem?.(item);
          }
        }
      },
      /* @__PURE__ */ import_react.default.createElement("span", { className: "mc-sidebar__item-copy" }, /* @__PURE__ */ import_react.default.createElement("span", { className: "mc-sidebar__item-title" }, item.title), item.subtitle ? /* @__PURE__ */ import_react.default.createElement("span", { className: "mc-sidebar__item-subtitle" }, item.subtitle) : null),
      item.meta?.length ? /* @__PURE__ */ import_react.default.createElement("span", { className: "mc-sidebar__item-meta" }, item.meta.map((meta) => /* @__PURE__ */ import_react.default.createElement("span", { className: `mc-sidebar__meta ${toneClass(meta.tone)}`.trim(), key: meta.id || meta.label }, meta.label))) : null
    );
  }))))))));
}
function ConversationPane({
  viewState,
  footer
}) {
  return /* @__PURE__ */ import_react.default.createElement("section", { className: "mc-conversation", "data-testid": "chat-inspector" }, /* @__PURE__ */ import_react.default.createElement("div", { className: "mc-conversation__header" }, /* @__PURE__ */ import_react.default.createElement("div", { className: "mc-conversation__title" }, viewState.title)), /* @__PURE__ */ import_react.default.createElement("div", { className: "mc-conversation__body" }, viewState.groups.length === 0 ? /* @__PURE__ */ import_react.default.createElement("div", { className: "mc-conversation__empty" }, /* @__PURE__ */ import_react.default.createElement("div", { className: "mc-conversation__empty-title" }, viewState.emptyTitle), /* @__PURE__ */ import_react.default.createElement("div", { className: "mc-conversation__empty-subtitle" }, viewState.emptySubtitle)) : /* @__PURE__ */ import_react.default.createElement("ul", { className: "mc-conversation__events", "data-testid": "chat-events" }, viewState.groups.map((group) => /* @__PURE__ */ import_react.default.createElement("li", { className: `mc-conversation__group is-${group.identity.presentation}`, key: group.id }, /* @__PURE__ */ import_react.default.createElement("div", { className: "mc-conversation__group-label" }, group.identity.label), /* @__PURE__ */ import_react.default.createElement("div", { className: "mc-conversation__messages" }, group.entries.map((entry) => /* @__PURE__ */ import_react.default.createElement("div", { className: "mc-conversation__message", key: entry.id }, entry.text))))))), footer ? /* @__PURE__ */ import_react.default.createElement("div", { className: "mc-conversation__footer" }, footer) : null);
}

// src/lib/console-adapters.ts
function groupLabel(agent) {
  return agent.group?.trim() || agent.profile?.trim() || agent.kind?.trim() || "Agents";
}
function subtitleForAgent(agent) {
  return [agent.profile, agent.kind].filter(Boolean).join(" \xB7 ") || "member";
}
function metaForAgent(agent) {
  const meta = [];
  if (agent.state) {
    meta.push({
      id: "state",
      label: agent.state,
      tone: agent.state === "running" ? "accent" : "muted"
    });
  }
  if (agent.addressable || agent.affordances?.can_send_message) {
    meta.push({
      id: "addressable",
      label: "addressable",
      tone: "muted"
    });
  }
  return meta;
}
function buildAgentSidebarViewState(args) {
  const grouped = /* @__PURE__ */ new Map();
  for (const agent of args.agents) {
    const label = groupLabel(agent);
    const bucket = grouped.get(label) || [];
    bucket.push(agent);
    grouped.set(label, bucket);
  }
  return {
    blocks: [{
      id: "agents",
      kind: "list",
      title: args.title,
      sections: Array.from(grouped.entries()).map(([label, members]) => ({
        id: label,
        title: label,
        items: members.map((agent) => ({
          id: agent.member_id,
          title: agent.label,
          subtitle: subtitleForAgent(agent),
          meta: metaForAgent(agent),
          selected: agent.member_id === args.selectedMemberId
        }))
      }))
    }]
  };
}
function summarizeFrameData(data) {
  if (typeof data === "string") {
    return data;
  }
  if (typeof data === "object" && data !== null) {
    const record = data;
    if (typeof record.delta === "string" && record.delta.trim()) {
      return record.delta;
    }
    if (typeof record.result === "string" && record.result.trim()) {
      return record.result;
    }
    if (typeof record.message === "string" && record.message.trim()) {
      return record.message;
    }
    if (typeof record.error === "string" && record.error.trim()) {
      return record.error;
    }
    if (typeof record.kind === "string" && typeof record.event_type === "string") {
      return "";
    }
    return JSON.stringify(record);
  }
  return String(data ?? "");
}
function identityForFrame(agent, frame) {
  if (frame.event === "subscribed") {
    return {
      id: "system",
      label: "System",
      presentation: "system"
    };
  }
  if (frame.event === "text_delta" || frame.event === "tool_call") {
    return {
      id: agent?.member_id || "agent",
      label: agent?.label || agent?.member_id || "Agent",
      presentation: "participant"
    };
  }
  return {
    id: "system",
    label: "System",
    presentation: "system"
  };
}
function createUserConversationEntry(message) {
  return {
    id: `user:${Date.now()}`,
    identity: {
      id: "user",
      label: "You",
      presentation: "user"
    },
    text: message
  };
}
function mapFramesToConversationEntries(agent, frames) {
  return frames.map((frame, index) => ({
    id: `${frame.id || frame.event || "frame"}:${index}`,
    identity: identityForFrame(agent, frame),
    text: `${frame.event}: ${summarizeFrameData(frame.data)}`.trim()
  }));
}
function buildConversationViewState(args) {
  return {
    conversationId: args.conversationId,
    title: args.title,
    entries: args.entries,
    groups: groupConversationEntries(args.entries),
    emptyTitle: `Talk to ${args.selectedAgentLabel}`,
    emptySubtitle: "Select an agent in the sidebar and send a message to start the console transcript."
  };
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
async function rpc(baseUrl, method, params) {
  const response = await fetch(`${baseUrl}/console/rpc`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({
      jsonrpc: "2.0",
      id: `${method}:${Date.now()}`,
      method,
      params
    })
  });
  if (!response.ok) {
    const text = await response.text();
    throw new Error(`${method} request failed ${response.status}: ${text}`);
  }
  const result = await response.json();
  if (result.error) {
    throw new Error(`${method} RPC error: ${result.error.message || JSON.stringify(result.error)}`);
  }
  return result.result;
}
async function sendMessage(baseUrl, memberId, message) {
  return rpc(baseUrl, "mobkit/send_message", {
    member_id: memberId,
    message
  });
}
var TERMINAL_SSE_EVENTS = /* @__PURE__ */ new Set([
  "interaction_complete",
  "run_completed",
  "interaction_failed",
  "run_failed"
]);
function hasMatchingTerminalEvent(rawText, sessionId) {
  const blocks = rawText.split(/\n\n+/);
  for (let i = 0; i < blocks.length - 1; i++) {
    const block = blocks[i].trim();
    if (!block) continue;
    let eventName = "";
    const dataLines = [];
    for (const line of block.split("\n")) {
      if (line.startsWith("event:")) eventName = line.slice(6).trim();
      else if (line.startsWith("data:")) dataLines.push(line.slice(5).trim());
    }
    if (!TERMINAL_SSE_EVENTS.has(eventName)) continue;
    if (!sessionId) return true;
    try {
      const data = JSON.parse(dataLines.join("\n"));
      if (data.session_id === sessionId) return true;
    } catch {
    }
  }
  return false;
}
async function drainInteractionResponse(response, sessionId) {
  if (!response.ok) {
    const text = await response.text();
    throw new Error(`interaction stream request failed ${response.status}: ${text}`);
  }
  if (!response.body || typeof response.body.getReader !== "function") {
    return parseSseFrames(await response.text());
  }
  const reader = response.body.getReader();
  const decoder = new TextDecoder();
  let rawText = "";
  try {
    while (!hasMatchingTerminalEvent(rawText, sessionId)) {
      const { value, done } = await reader.read();
      if (done) {
        break;
      }
      rawText += decoder.decode(value, { stream: true });
      if (rawText.length > 131072) {
        break;
      }
    }
    rawText += decoder.decode();
  } finally {
    try {
      await reader.cancel();
    } catch {
    }
  }
  const frames = parseSseFrames(rawText);
  if (!sessionId) return frames;
  return frames.filter((frame) => {
    const data = frame.data;
    if (data === null || typeof data !== "object") return false;
    if ("session_id" in data) return data.session_id === sessionId;
    return true;
  });
}
function persistedEventToFrame(raw, index) {
  const record = typeof raw === "object" && raw !== null ? raw : {};
  const event = typeof record.event === "object" && record.event !== null ? record.event : {};
  if (event.kind === "agent") {
    return {
      id: String(record.id ?? `event:${index}`),
      event: String(event.event_type ?? "agent_event"),
      data: event
    };
  }
  if (event.kind === "module") {
    return {
      id: String(record.id ?? `event:${index}`),
      event: String(event.event_type ?? "module_event"),
      data: event.payload ?? event
    };
  }
  return {
    id: String(record.id ?? `event:${index}`),
    event: String(record.type ?? "event"),
    data: raw
  };
}
async function queryEvents(baseUrl, memberId, limit = 40) {
  const result = await rpc(baseUrl, "mobkit/query_events", {
    limit
  });
  if (typeof result === "object" && result !== null && result.status === "no_event_log_configured") {
    return [];
  }
  if (!Array.isArray(result)) {
    return [];
  }
  return result.filter((raw) => {
    if (typeof raw !== "object" || raw === null) return true;
    const ev = raw.event;
    return !(typeof ev === "object" && ev !== null && ev.kind === "agent");
  }).map((event, index) => persistedEventToFrame(event, index));
}
async function sendInteraction(baseUrl, memberId, message) {
  const streamAbort = new AbortController();
  const streamResponsePromise = fetch(`${baseUrl}/interactions/stream`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ member_id: memberId }),
    signal: streamAbort.signal
  });
  void streamResponsePromise.catch(() => {
  });
  let sendResult;
  try {
    sendResult = await sendMessage(baseUrl, memberId, message);
  } catch (err) {
    streamAbort.abort();
    throw err;
  }
  let frames;
  try {
    frames = await drainInteractionResponse(
      await streamResponsePromise,
      sendResult.session_id
    );
  } catch {
    frames = [];
  }
  return { sendResult, frames };
}

// src/panels/ActivityPanel.tsx
var import_react2 = __toESM(require("react"));
function ActivityPanel({ title, frames }) {
  return /* @__PURE__ */ import_react2.default.createElement("section", { "data-testid": "activity-panel" }, /* @__PURE__ */ import_react2.default.createElement("h2", null, title), /* @__PURE__ */ import_react2.default.createElement("ul", { "data-testid": "activity-feed" }, frames.map((frame, index) => /* @__PURE__ */ import_react2.default.createElement("li", { key: `${frame.id || frame.event || "event"}:${index}` }, `${frame.event || "message"} ${frame.id || ""}`.trim()))));
}

// src/panels/HealthOverviewPanel.tsx
var import_react3 = __toESM(require("react"));
function HealthOverviewPanel({
  title,
  running,
  loadedModuleCount,
  loadedModules
}) {
  return /* @__PURE__ */ import_react3.default.createElement("section", { "data-testid": "health-overview" }, /* @__PURE__ */ import_react3.default.createElement("h2", null, title), /* @__PURE__ */ import_react3.default.createElement("p", { "data-testid": "health-running" }, `Running: ${running === null ? "unknown" : String(running)}`), /* @__PURE__ */ import_react3.default.createElement("p", { "data-testid": "health-loaded-module-count" }, `Loaded module count: ${loadedModuleCount}`), /* @__PURE__ */ import_react3.default.createElement("ul", { "data-testid": "health-loaded-modules" }, loadedModules.map((moduleId) => /* @__PURE__ */ import_react3.default.createElement("li", { key: moduleId }, moduleId))));
}

// src/panels/TopologyPanel.tsx
var import_react4 = __toESM(require("react"));
function TopologyPanel({ title, nodeCount, nodes }) {
  return /* @__PURE__ */ import_react4.default.createElement("section", { "data-testid": "topology-panel" }, /* @__PURE__ */ import_react4.default.createElement("h2", null, title), /* @__PURE__ */ import_react4.default.createElement("p", { "data-testid": "topology-node-count" }, `Node count: ${nodeCount}`), /* @__PURE__ */ import_react4.default.createElement("ul", { "data-testid": "topology-nodes" }, nodes.map((moduleId) => /* @__PURE__ */ import_react4.default.createElement("li", { key: moduleId }, moduleId))));
}

// src/ConsoleApp.tsx
function ConsoleApp({ baseUrl }) {
  const [experience, setExperience] = import_react5.default.useState(null);
  const [agents, setAgents] = import_react5.default.useState([]);
  const [selectedMemberId, setSelectedMemberId] = import_react5.default.useState("");
  const [message, setMessage] = import_react5.default.useState("");
  const [loading, setLoading] = import_react5.default.useState(true);
  const [error, setError] = import_react5.default.useState("");
  const [activityFrames, setActivityFrames] = import_react5.default.useState([]);
  const [framesByMemberId, setFramesByMemberId] = import_react5.default.useState({});
  const [entriesByMemberId, setEntriesByMemberId] = import_react5.default.useState({});
  const [historyLoadedByMemberId, setHistoryLoadedByMemberId] = import_react5.default.useState({});
  import_react5.default.useEffect(() => {
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
  const selectedAgent = import_react5.default.useMemo(
    () => agents.find((agent) => agent.member_id === selectedMemberId) || null,
    [agents, selectedMemberId]
  );
  import_react5.default.useEffect(() => {
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
        setFramesByMemberId((current) => ({
          ...current,
          [selectedMemberId]: [...frames, ...current[selectedMemberId] || []]
        }));
        setEntriesByMemberId((current) => ({
          ...current,
          [selectedMemberId]: [
            ...mapFramesToConversationEntries(selectedAgent, frames),
            ...current[selectedMemberId] || []
          ]
        }));
      } catch (_) {
      } finally {
        if (!cancelled) {
          setHistoryLoadedByMemberId((current) => ({
            ...current,
            [selectedMemberId]: true
          }));
        }
      }
    }
    void loadHistory();
    return () => {
      cancelled = true;
    };
  }, [baseUrl, historyLoadedByMemberId, selectedAgent, selectedMemberId]);
  async function onSubmit(event) {
    event.preventDefault();
    const trimmedMessage = message.trim();
    if (!selectedMemberId || !trimmedMessage) {
      return;
    }
    const userEntry = createUserConversationEntry(trimmedMessage);
    setError("");
    setEntriesByMemberId((current) => ({
      ...current,
      [selectedMemberId]: [...current[selectedMemberId] || [], userEntry]
    }));
    try {
      const result = await sendInteraction(baseUrl, selectedMemberId, trimmedMessage);
      const nextEntries = mapFramesToConversationEntries(selectedAgent, result.frames);
      setFramesByMemberId((current) => ({
        ...current,
        [selectedMemberId]: [...current[selectedMemberId] || [], ...result.frames]
      }));
      setEntriesByMemberId((current) => ({
        ...current,
        [selectedMemberId]: [...current[selectedMemberId] || [], ...nextEntries]
      }));
      setActivityFrames((current) => [...result.frames, ...current].slice(0, 64));
      setHistoryLoadedByMemberId((current) => ({
        ...current,
        [selectedMemberId]: false
      }));
      setMessage("");
    } catch (submitError) {
      setError(errorMessage(submitError));
      setEntriesByMemberId((current) => ({
        ...current,
        [selectedMemberId]: (current[selectedMemberId] || []).filter(
          (e) => e.id !== userEntry.id
        )
      }));
    }
  }
  if (loading) {
    return /* @__PURE__ */ import_react5.default.createElement("div", { "data-testid": "console-loading" }, "Loading console...");
  }
  if (error) {
    return /* @__PURE__ */ import_react5.default.createElement("div", { "data-testid": "console-error" }, error);
  }
  const topologySnapshot = experience?.topology?.live_snapshot || {};
  const topologyNodes = Array.isArray(topologySnapshot.nodes) ? topologySnapshot.nodes.map((node) => String(node)) : [];
  const topologyNodeCount = Number.isFinite(topologySnapshot.node_count) ? topologySnapshot.node_count : topologyNodes.length;
  const healthSnapshot = experience?.health_overview?.live_snapshot || {};
  const loadedModules = Array.isArray(healthSnapshot.loaded_modules) ? healthSnapshot.loaded_modules.map((moduleId) => String(moduleId)) : [];
  const loadedModuleCount = Number.isFinite(healthSnapshot.loaded_module_count) ? healthSnapshot.loaded_module_count : loadedModules.length;
  const running = typeof healthSnapshot.running === "boolean" ? healthSnapshot.running : null;
  const sidebarViewState = buildAgentSidebarViewState({
    title: experience?.agent_sidebar?.title || "Agents",
    agents,
    selectedMemberId
  });
  const conversationViewState = buildConversationViewState({
    conversationId: selectedMemberId || "console",
    title: selectedAgent?.label || (experience?.chat_inspector?.title || "Chat Inspector"),
    entries: selectedMemberId ? entriesByMemberId[selectedMemberId] || [] : [],
    selectedAgentLabel: selectedAgent?.label || selectedMemberId || "an agent"
  });
  return /* @__PURE__ */ import_react5.default.createElement("div", { "data-testid": "meerkat-console" }, /* @__PURE__ */ import_react5.default.createElement(
    ConsoleWorkbench,
    {
      main: /* @__PURE__ */ import_react5.default.createElement(
        ConversationPane,
        {
          footer: /* @__PURE__ */ import_react5.default.createElement("form", { className: "mc-composer", "data-testid": "chat-form", onSubmit }, /* @__PURE__ */ import_react5.default.createElement("div", { className: "mc-composer__header" }, /* @__PURE__ */ import_react5.default.createElement("span", { className: "mc-composer__eyebrow" }, "Target"), /* @__PURE__ */ import_react5.default.createElement("span", { className: "mc-composer__target" }, selectedAgent?.label || "Select an agent")), /* @__PURE__ */ import_react5.default.createElement("label", { className: "mc-composer__field" }, /* @__PURE__ */ import_react5.default.createElement("span", { className: "mc-composer__label" }, "Message"), /* @__PURE__ */ import_react5.default.createElement(
            "textarea",
            {
              name: "message",
              placeholder: selectedAgent ? `Message ${selectedAgent.label}` : "Select an agent to start",
              value: message,
              onChange: (changeEvent) => setMessage(changeEvent.target.value)
            }
          )), /* @__PURE__ */ import_react5.default.createElement("div", { className: "mc-composer__actions" }, /* @__PURE__ */ import_react5.default.createElement("button", { disabled: !selectedMemberId || !message.trim(), type: "submit" }, "Send"))),
          viewState: conversationViewState
        }
      ),
      sidebar: /* @__PURE__ */ import_react5.default.createElement(
        ConsoleSidebar,
        {
          getItemButtonProps: (item) => ({
            "data-agent-id": agents.find((agent) => agent.member_id === item.id)?.agent_id || item.id
          }),
          onSelectItem: (item) => setSelectedMemberId(item.id),
          viewState: sidebarViewState
        }
      )
    }
  ), /* @__PURE__ */ import_react5.default.createElement("div", { className: "mc-dashboard" }, /* @__PURE__ */ import_react5.default.createElement(
    ActivityPanel,
    {
      title: experience?.activity_feed?.title || "Activity",
      frames: activityFrames
    }
  ), /* @__PURE__ */ import_react5.default.createElement(
    TopologyPanel,
    {
      title: experience?.topology?.title || "Topology",
      nodeCount: topologyNodeCount,
      nodes: topologyNodes
    }
  ), /* @__PURE__ */ import_react5.default.createElement(
    HealthOverviewPanel,
    {
      title: experience?.health_overview?.title || "Health",
      running,
      loadedModuleCount,
      loadedModules
    }
  )));
}

// src/index.tsx
function createConsoleApp(target, options = {}) {
  if (!target) {
    throw new Error("target element is required");
  }
  const baseUrl = options.baseUrl || "";
  const root = (0, import_client.createRoot)(target);
  root.render(/* @__PURE__ */ import_react6.default.createElement(ConsoleApp, { baseUrl }));
  return {
    unmount() {
      root.unmount();
    }
  };
}
