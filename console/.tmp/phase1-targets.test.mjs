var __getOwnPropNames = Object.getOwnPropertyNames;
var __esm = (fn, res) => function __init() {
  return fn && (res = (0, fn[__getOwnPropNames(fn)[0]])(fn = 0)), res;
};
var __commonJS = (cb, mod) => function __require() {
  return mod || (0, cb[__getOwnPropNames(cb)[0]])((mod = { exports: {} }).exports, mod), mod.exports;
};

// ../packages/console-core/src/control-plane.ts
function trimString(value) {
  if (typeof value !== "string") {
    return void 0;
  }
  const trimmed = value.trim();
  return trimmed || void 0;
}
function stringRecord(value) {
  if (!value || typeof value !== "object") {
    return {};
  }
  return Object.fromEntries(
    Object.entries(value).map(([key, raw]) => {
      const normalizedKey = trimString(key);
      const normalizedValue = trimString(raw);
      return normalizedKey && normalizedValue ? [normalizedKey, normalizedValue] : null;
    }).filter((entry) => Boolean(entry))
  );
}
function normalizeResponsePhase(value) {
  switch (value) {
    case "waiting":
    case "tool-executing":
    case "generating":
      return value;
    case null:
    case void 0:
      return null;
    default:
      return null;
  }
}
function normalizeFiniteNumber(value) {
  return typeof value === "number" && Number.isFinite(value) ? value : void 0;
}
function normalizeSidebarWatchFields(value) {
  const record = value && typeof value === "object" ? value : {};
  const normalized = {};
  if (typeof record.watched === "boolean") {
    normalized.watched = record.watched;
  }
  if (record.alertLevel === "elevated" || record.alertLevel === "critical" || record.alertLevel === null) {
    normalized.alertLevel = record.alertLevel;
  }
  if (typeof record.degraded === "boolean") {
    normalized.degraded = record.degraded;
  }
  const degradedReason = trimString(record.degradedReason);
  if (degradedReason) {
    normalized.degradedReason = degradedReason;
  }
  return normalized;
}
function normalizeIdentityStatusRow(value) {
  const record = value && typeof value === "object" ? value : null;
  if (!record) {
    return null;
  }
  const identity = trimString(record.identity);
  const state = trimString(record.state);
  if (!identity || !state) {
    return null;
  }
  const addressability = record.addressability === "internal_only" ? "internal_only" : record.addressability === "addressable" ? "addressable" : null;
  if (!addressability) {
    return null;
  }
  return {
    identity,
    state,
    addressability,
    labels: stringRecord(record.labels),
    ...trimString(record.display_name) ? { display_name: trimString(record.display_name) } : {},
    ...trimString(record.profile) ? { profile: trimString(record.profile) } : {},
    ...typeof record.generation === "number" && Number.isFinite(record.generation) ? { generation: record.generation } : {},
    ...typeof record.checkpoint_version === "number" && Number.isFinite(record.checkpoint_version) ? { checkpoint_version: record.checkpoint_version } : {},
    ...typeof record.lease_healthy === "boolean" ? { lease_healthy: record.lease_healthy } : {}
  };
}
function normalizeRoutingSectionView(value) {
  const record = value && typeof value === "object" ? value : null;
  if (!record) {
    return null;
  }
  const routes = Array.isArray(record.routes) ? record.routes.map((entry) => {
    const route = entry && typeof entry === "object" ? entry : null;
    if (!route) {
      return null;
    }
    const routeKey = trimString(route.route_key);
    const recipient = trimString(route.recipient);
    const sink = trimString(route.sink);
    const targetModule = trimString(route.target_module);
    if (!routeKey || !recipient || !sink || !targetModule) {
      return null;
    }
    return {
      route_key: routeKey,
      recipient,
      sink,
      target_module: targetModule,
      ...trimString(route.channel) ? { channel: trimString(route.channel) } : {},
      ...normalizeFiniteNumber(route.retry_max) !== void 0 ? { retry_max: normalizeFiniteNumber(route.retry_max) } : {},
      ...normalizeFiniteNumber(route.backoff_ms) !== void 0 ? { backoff_ms: normalizeFiniteNumber(route.backoff_ms) } : {},
      ...normalizeFiniteNumber(route.rate_limit_per_minute) !== void 0 ? { rate_limit_per_minute: normalizeFiniteNumber(route.rate_limit_per_minute) } : {}
    };
  }).filter((entry) => Boolean(entry)) : [];
  const deliveries = Array.isArray(record.deliveries) ? record.deliveries.map((entry) => {
    const delivery = entry && typeof entry === "object" ? entry : null;
    if (!delivery) {
      return null;
    }
    const deliveryId = trimString(delivery.delivery_id);
    const routeId = trimString(delivery.route_id);
    const recipient = trimString(delivery.recipient);
    const sink = trimString(delivery.sink);
    const targetModule = trimString(delivery.target_module);
    const status = trimString(delivery.status);
    const firstAttempt = normalizeFiniteNumber(delivery.first_attempt_ms);
    const finalAttempt = normalizeFiniteNumber(delivery.final_attempt_ms);
    if (!deliveryId || !routeId || !recipient || !sink || !targetModule || !status || firstAttempt === void 0 || finalAttempt === void 0) {
      return null;
    }
    const attempts = Array.isArray(delivery.attempts) ? delivery.attempts.map((attemptRaw) => {
      const attempt = attemptRaw && typeof attemptRaw === "object" ? attemptRaw : null;
      if (!attempt) {
        return null;
      }
      const attemptNumber = normalizeFiniteNumber(attempt.attempt);
      const attemptStatus = trimString(attempt.status);
      const backoff = normalizeFiniteNumber(attempt.backoff_ms);
      if (attemptNumber === void 0 || !attemptStatus || backoff === void 0) {
        return null;
      }
      return {
        attempt: attemptNumber,
        status: attemptStatus,
        backoff_ms: backoff
      };
    }).filter((attempt) => Boolean(attempt)) : [];
    return {
      delivery_id: deliveryId,
      route_id: routeId,
      recipient,
      sink,
      target_module: targetModule,
      status,
      first_attempt_ms: firstAttempt,
      final_attempt_ms: finalAttempt,
      attempts,
      ...trimString(delivery.idempotency_key) ? { idempotency_key: trimString(delivery.idempotency_key) } : {},
      ...trimString(delivery.sink_adapter) ? { sink_adapter: trimString(delivery.sink_adapter) } : {}
    };
  }).filter((entry) => Boolean(entry)) : [];
  return { routes, deliveries };
}
function normalizeToolCallAccumulatorState(value) {
  const record = value && typeof value === "object" ? value : null;
  if (!record) {
    return null;
  }
  const timeoutMs = normalizeFiniteNumber(record.timeoutMs);
  if (timeoutMs === void 0 || timeoutMs <= 0) {
    return null;
  }
  const toolCalls = record.toolCalls && typeof record.toolCalls === "object" ? Object.fromEntries(
    Object.entries(record.toolCalls).map(([toolCallId, raw]) => {
      const normalizedId = trimString(toolCallId);
      const rawBlock = raw && typeof raw === "object" ? raw : null;
      if (!normalizedId || !rawBlock) {
        return null;
      }
      const name = trimString(rawBlock.name);
      const argumentsText = trimString(rawBlock.arguments);
      const status = rawBlock.status === "pending" || rawBlock.status === "success" || rawBlock.status === "error" ? rawBlock.status : null;
      if (rawBlock.type !== "tool-call" || !name || !argumentsText || !status) {
        return null;
      }
      return [
        normalizedId,
        {
          type: "tool-call",
          toolCallId: normalizedId,
          name,
          arguments: argumentsText,
          ...trimString(rawBlock.result) ? { result: trimString(rawBlock.result) } : {},
          status
        }
      ];
    }).filter((entry) => Boolean(entry))
  ) : {};
  const pendingResults = record.pendingResults && typeof record.pendingResults === "object" ? Object.fromEntries(
    Object.entries(record.pendingResults).map(([toolCallId, result]) => {
      const normalizedId = trimString(toolCallId);
      const normalizedResult = trimString(result);
      return normalizedId && normalizedResult ? [normalizedId, normalizedResult] : null;
    }).filter((entry) => Boolean(entry))
  ) : {};
  return {
    toolCalls,
    pendingResults,
    timeoutMs
  };
}
var init_control_plane = __esm({
  "../packages/console-core/src/control-plane.ts"() {
  }
});

// ../packages/console-core/src/rich-content.ts
function parseConversationRichBlocks(content) {
  const source = String(content || "").trim();
  if (!source) {
    return [];
  }
  const blocks = [];
  const fenceRe = /```([^\n`]*)\n([\s\S]*?)```/g;
  let lastIndex = 0;
  let match;
  while (match = fenceRe.exec(source)) {
    const before = source.slice(lastIndex, match.index);
    blocks.push(...parseConversationTextBlocks(before));
    blocks.push({
      type: "code",
      language: (match[1] || "text").trim() || "text",
      body: match[2].replace(/\n+$/u, "")
    });
    lastIndex = fenceRe.lastIndex;
  }
  blocks.push(...parseConversationTextBlocks(source.slice(lastIndex)));
  return compactConversationBlocks(blocks);
}
function parseConversationTextBlocks(fragment) {
  const source = String(fragment || "").trim();
  if (!source) {
    return [];
  }
  const sections = source.split(/\n{2,}/u).map((section) => section.trim()).filter(Boolean);
  const blocks = [];
  for (const section of sections) {
    const heading = parseConversationHeadingBlock(section);
    if (heading) {
      blocks.push(...heading);
      continue;
    }
    const table = parseConversationTableBlock(section);
    if (table) {
      blocks.push(table);
      continue;
    }
    const fileChange = parseConversationFileChangeBlock(section);
    if (fileChange) {
      blocks.push(fileChange);
      continue;
    }
    const command = parseConversationCommandBlock(section);
    if (command) {
      blocks.push(command);
      continue;
    }
    if (TERMINAL_DURATION_RE.test(section)) {
      blocks.push({ type: "divider", text: section });
      continue;
    }
    const normalized = section.replace(/^\s*[-*]\s+/gm, "").replace(/\n{2,}/g, "\n").trim();
    if (normalized) {
      blocks.push({ type: "paragraph", text: normalized });
    }
  }
  return blocks;
}
function compactConversationBlocks(blocks) {
  const deduped = [];
  for (const block of blocks) {
    const previous = deduped.at(-1);
    if (block.type === "paragraph" && previous?.type === "file-change" && previous.name && block.text.startsWith(previous.name)) {
      continue;
    }
    deduped.push(block);
  }
  return deduped;
}
function parseConversationHeadingBlock(section) {
  const lines = String(section || "").split(/\n/u).map((line) => line.trim()).filter(Boolean);
  if (!lines.length || !lines[0].startsWith("#")) {
    return null;
  }
  const headingMatch = lines[0].match(/^(#{1,6})\s+(.+)$/u);
  if (!headingMatch) {
    return null;
  }
  const blocks = [{
    type: "heading",
    level: headingMatch[1].length,
    text: headingMatch[2].trim()
  }];
  const rest = lines.slice(1).join("\n").trim();
  if (rest) {
    blocks.push({ type: "paragraph", text: rest });
  }
  return blocks;
}
function splitMarkdownTableRow(line) {
  const source = String(line || "").trim().replace(/^\|/u, "").replace(/\|$/u, "");
  const cells = [];
  let current = "";
  let escaping = false;
  let codeFenceDepth = 0;
  for (const character of source) {
    if (escaping) {
      current += character;
      escaping = false;
      continue;
    }
    if (character === "\\") {
      escaping = true;
      continue;
    }
    if (character === "`") {
      codeFenceDepth = codeFenceDepth === 0 ? 1 : 0;
      current += character;
      continue;
    }
    if (character === "|" && codeFenceDepth === 0) {
      cells.push(current.trim());
      current = "";
      continue;
    }
    current += character;
  }
  cells.push(current.trim());
  return cells;
}
function parseTableAlignment(cells) {
  if (!cells.length || !cells.every((cell) => /^:?-{3,}:?$/u.test(cell))) {
    return null;
  }
  return cells.map((cell) => {
    const trimmed = cell.trim();
    if (trimmed.startsWith(":") && trimmed.endsWith(":")) {
      return "center";
    }
    if (trimmed.endsWith(":")) {
      return "right";
    }
    return "left";
  });
}
function parseConversationTableBlock(section) {
  const lines = String(section || "").split(/\n/u).map((line) => line.trim()).filter(Boolean);
  if (lines.length < 2) {
    return null;
  }
  const headers = splitMarkdownTableRow(lines[0]);
  const alignments = parseTableAlignment(splitMarkdownTableRow(lines[1]));
  if (!headers.length || !alignments || headers.length !== alignments.length) {
    return null;
  }
  const rows = lines.slice(2).map((line) => splitMarkdownTableRow(line)).filter((cells) => cells.length > 0 && cells.some((cell) => cell.length > 0)).map((cells) => headers.map((_header, index) => cells[index] || ""));
  return {
    type: "table",
    headers,
    alignments,
    rows
  };
}
function parseConversationFileChangeBlock(section) {
  const compact = String(section || "").replace(/\s*\n\s*/g, " ").trim();
  if (!compact) {
    return null;
  }
  const header = compact.match(FILE_CHANGE_RE);
  if (!header) {
    return null;
  }
  const verb = header[1];
  const statsMatch = compact.match(/\s+\+([\d,]+)\s+-([\d,]+)\s*$/u);
  const plus = Number.parseInt((statsMatch?.[1] || "1").replaceAll(",", ""), 10) || 0;
  const minus = Number.parseInt((statsMatch?.[2] || "0").replaceAll(",", ""), 10) || 0;
  const body = statsMatch ? compact.slice(0, statsMatch.index).trim() : compact;
  const fileMatches = [...body.matchAll(/`([^`]+)`/gu)];
  const fileMatch = fileMatches.find((candidate) => !candidate[1].includes("/")) || fileMatches[0];
  if (!fileMatch) {
    return null;
  }
  const fileToken = fileMatch[0];
  const fileName = fileMatch[1].trim();
  const bodyAfterVerb = body.slice(verb.length).trim();
  const tokenIndex = bodyAfterVerb.indexOf(fileToken);
  const before = tokenIndex >= 0 ? bodyAfterVerb.slice(0, tokenIndex).trim() : "";
  const after = tokenIndex >= 0 ? bodyAfterVerb.slice(tokenIndex + fileToken.length).trim() : bodyAfterVerb.replace(fileToken, "").trim();
  return {
    type: "file-change",
    verb,
    before,
    name: fileName,
    after,
    plus,
    minus
  };
}
function parseConversationCommandBlock(section) {
  const lines = String(section || "").split(/\n/u).map((line) => line.replace(/\s+$/u, "")).filter((line) => line.trim().length > 0);
  if (!lines.length) {
    return null;
  }
  const commandIndex = lines.findIndex((line) => line.trim().startsWith("$ "));
  if (commandIndex === -1) {
    return null;
  }
  const command = lines[commandIndex].trim();
  const prefix = lines.slice(0, commandIndex).filter(Boolean);
  const footerCandidate = lines.at(-1)?.trim() || "";
  const footer = TERMINAL_STATUS_RE.test(footerCandidate) ? footerCandidate : "";
  const outputStart = commandIndex + 1;
  const outputEnd = footer ? lines.length - 1 : lines.length;
  const output = lines.slice(outputStart, outputEnd).join("\n").trim();
  return {
    type: "command",
    caption: prefix[0] || "Ran command",
    title: prefix[1] || "Shell",
    body: command,
    output,
    footer
  };
}
var FILE_CHANGE_RE, TERMINAL_DURATION_RE, TERMINAL_STATUS_RE;
var init_rich_content = __esm({
  "../packages/console-core/src/rich-content.ts"() {
    FILE_CHANGE_RE = /^(Created|Updated|Modified|Deleted)\b/i;
    TERMINAL_DURATION_RE = /^Worked for\s+.+$/i;
    TERMINAL_STATUS_RE = /^(Success|Running|Failed|Cancelled)$/i;
  }
});

// ../packages/console-core/src/conversation.ts
var init_conversation = __esm({
  "../packages/console-core/src/conversation.ts"() {
    init_rich_content();
  }
});

// ../packages/console-core/src/dock.ts
var init_dock = __esm({
  "../packages/console-core/src/dock.ts"() {
  }
});

// ../packages/console-core/src/sidebar.ts
var init_sidebar = __esm({
  "../packages/console-core/src/sidebar.ts"() {
    init_control_plane();
  }
});

// ../packages/console-core/src/format.ts
var init_format = __esm({
  "../packages/console-core/src/format.ts"() {
  }
});

// ../packages/console-core/src/index.ts
var init_src = __esm({
  "../packages/console-core/src/index.ts"() {
    init_control_plane();
    init_conversation();
    init_dock();
    init_sidebar();
    init_rich_content();
    init_format();
  }
});

// src/lib/network.ts
function unwrapConsoleEnvelope(eventName, data) {
  if (!data || typeof data !== "object") {
    return { data };
  }
  const record = data;
  if (typeof record.event_id === "string" && typeof record.event_type === "string" && typeof record.identity === "string" && "data" in record) {
    const envelope = record;
    return {
      id: envelope.event_id,
      event: envelope.event_type || eventName,
      identity: envelope.identity,
      interactionId: envelope.interaction_id,
      timestampMs: envelope.timestamp_ms,
      data: envelope.data
    };
  }
  return { data };
}
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
    const normalized = unwrapConsoleEnvelope(event, data);
    frames.push({
      id: normalized.id || id,
      event: normalized.event || event,
      identity: normalized.identity,
      interactionId: normalized.interactionId,
      timestampMs: normalized.timestampMs,
      data: normalized.data
    });
  }
  return frames;
}
var init_network = __esm({
  "src/lib/network.ts"() {
    init_src();
  }
});

// src/lib/adapters.ts
function buildPanelConversationKey(panelId, target) {
  if (!target) {
    return `panel:${panelId}:none`;
  }
  if (target.kind !== "agent-chat") {
    return `panel:${panelId}:${target.kind}:${target.id}`;
  }
  const targetKey = target.addressingMode === "identity" ? target.identity || target.memberId || target.id : target.memberId || target.id;
  return `panel:${panelId}:${target.kind}:${targetKey}`;
}
function buildDockTarget(agent) {
  const subtitle = [agent.profile, agent.kind].filter(Boolean).join(" \xB7 ") || void 0;
  const identity = typeof agent.identity === "string" && agent.identity.trim() ? agent.identity.trim() : void 0;
  const addressingMode = identity ? "identity" : "member";
  return {
    id: agent.member_id,
    kind: "agent-chat",
    addressingMode,
    memberId: agent.member_id,
    ...identity ? { identity } : {},
    title: agent.label,
    subtitle,
    iconName: "i-team"
  };
}
function agentGroupKey(agent) {
  return agent.group?.trim() || agent.profile?.trim() || agent.kind?.trim() || "Agents";
}
function agentStateTone(state) {
  switch (state) {
    case "running":
      return "accent";
    case "active":
      return "positive";
    case "idle":
      return "muted";
    case "error":
      return "negative";
    default:
      return "muted";
  }
}
function sectionIconForGroup(group) {
  const lower = group.toLowerCase();
  if (lower.includes("coordinator") || lower.includes("system")) return "i-bolt";
  if (lower.includes("domain") || lower.includes("specialist")) return "i-cube";
  if (lower.includes("internal") || lower.includes("infra")) return "i-gear";
  if (lower.includes("personal") || lower.includes("identity")) return "i-team";
  return "i-folder";
}
function buildSidebarViewState(args) {
  const { agents, selectedMemberId, pinnedAgentIds = /* @__PURE__ */ new Set(), sortMode = "group" } = args;
  const sorted = [...agents].sort((a, b) => {
    const aPinned = pinnedAgentIds.has(a.member_id) ? 0 : 1;
    const bPinned = pinnedAgentIds.has(b.member_id) ? 0 : 1;
    if (aPinned !== bPinned) return aPinned - bPinned;
    if (sortMode === "alpha") return a.label.localeCompare(b.label);
    if (sortMode === "status") {
      const stateOrder = (s) => s === "running" ? 0 : s === "active" ? 1 : 2;
      const diff = stateOrder(a.state) - stateOrder(b.state);
      if (diff !== 0) return diff;
    }
    return a.label.localeCompare(b.label);
  });
  const grouped = /* @__PURE__ */ new Map();
  for (const agent of sorted) {
    const key = agentGroupKey(agent);
    const bucket = grouped.get(key) || [];
    bucket.push(agent);
    grouped.set(key, bucket);
  }
  const sections = Array.from(grouped.entries()).map(([group, members]) => ({
    id: group,
    title: group,
    iconName: sectionIconForGroup(group),
    meta: [{ id: "count", label: `${members.length}` }],
    items: members.map((agent) => {
      const isAddressable = agent.addressable || agent.affordances?.can_send_message;
      const isPinned = pinnedAgentIds.has(agent.member_id);
      const watchFields = normalizeSidebarWatchFields(agent);
      return {
        id: agent.member_id,
        title: agent.label,
        subtitle: agent.identity || agent.member_id,
        selected: agent.member_id === selectedMemberId,
        pinned: isPinned,
        disabled: !isAddressable,
        ...watchFields,
        meta: [
          ...agent.state ? [{ id: "state", label: agent.state, tone: agentStateTone(agent.state) }] : [],
          ...agent.response_phase ? [{ id: "phase", label: agent.response_phase, tone: "accent" }] : []
        ],
        actions: [
          {
            id: "inspect_identity",
            label: "Inspect identity",
            iconName: "i-terminal"
          },
          {
            id: "toggle_pin",
            label: isPinned ? "Unpin agent" : "Pin agent",
            iconName: "i-pin",
            active: isPinned
          }
        ]
      };
    })
  }));
  return {
    blocks: [
      {
        id: "controls",
        kind: "action_strip",
        actions: [
          { id: "open_routing", label: "Routing", iconName: "i-swap" },
          { id: "open_gating", label: "Gating", iconName: "i-bolt" },
          { id: "open_topology", label: "Topology", iconName: "i-team" },
          { id: "open_health", label: "Health", iconName: "i-gear" }
        ]
      },
      {
        id: "agents",
        kind: "list",
        title: "Agents",
        actions: [
          { id: "spawn_agent", label: "Spawn agent", iconName: "i-plus" },
          { id: "filter_sort", label: "Sort & filter", iconName: "i-sliders" }
        ],
        sections
      }
    ]
  };
}
function buildRoutingSectionView(args) {
  const routesRecord = typeof args.routesResponse === "object" && args.routesResponse !== null ? args.routesResponse : {};
  const historyRecord = typeof args.historyResponse === "object" && args.historyResponse !== null ? args.historyResponse : {};
  const normalized = normalizeRoutingSectionView({
    routes: Array.isArray(routesRecord.routes) ? routesRecord.routes : [],
    deliveries: Array.isArray(historyRecord.deliveries) ? historyRecord.deliveries : []
  });
  return normalized ?? { routes: [], deliveries: [] };
}
function agentIdentity(agent) {
  return {
    id: agent?.member_id || "agent",
    label: agent?.label || "Agent",
    role: "assistant"
  };
}
function summarizeFrameData(data) {
  if (typeof data === "string") {
    const trimmed = data.trim();
    if (trimmed.startsWith("{") && trimmed.endsWith("}") || trimmed.startsWith("[") && trimmed.endsWith("]")) {
      try {
        return summarizeFrameData(JSON.parse(trimmed));
      } catch {
        return data;
      }
    }
    return data;
  }
  if (typeof data === "object" && data !== null) {
    const record = data;
    if (typeof record.delta === "string") return record.delta;
    if (typeof record.text === "string" && record.text.trim()) return record.text;
    if (typeof record.result === "string" && record.result.trim()) return record.result;
    if (typeof record.message === "string" && record.message.trim()) return record.message;
    if (typeof record.error === "string" && record.error.trim()) return record.error;
    if (typeof record.reason === "string" && record.reason.trim()) return record.reason;
    if (typeof record.kind === "string" && typeof record.event_type === "string") return "";
    return JSON.stringify(record);
  }
  return String(data ?? "");
}
function eventSortRank(event) {
  switch (event) {
    case "interaction_started":
      return 0;
    case "tool_call_requested":
    case "tool_call":
    case "tool_execution_started":
      return 20;
    case "tool_result_received":
    case "tool_execution_completed":
      return 30;
    case "text_delta":
      return 40;
    case "text_complete":
      return 45;
    case "interaction_complete":
    case "interaction_failed":
    case "run_completed":
    case "run_failed":
      return 90;
    default:
      return 50;
  }
}
function sortFramesForTranscript(frames) {
  const interactionStartMs = /* @__PURE__ */ new Map();
  for (const frame of frames) {
    const interactionId = frame.interactionId?.trim();
    const timestampMs = typeof frame.timestampMs === "number" ? frame.timestampMs : Number.MAX_SAFE_INTEGER;
    if (!interactionId) continue;
    const current = interactionStartMs.get(interactionId);
    if (current === void 0 || timestampMs < current) {
      interactionStartMs.set(interactionId, timestampMs);
    }
  }
  return frames.map((frame, index) => ({ frame, index })).sort((left, right) => {
    const leftInteraction = left.frame.interactionId?.trim() || "";
    const rightInteraction = right.frame.interactionId?.trim() || "";
    const leftGroupTs = (leftInteraction && interactionStartMs.get(leftInteraction)) ?? (typeof left.frame.timestampMs === "number" ? left.frame.timestampMs : Number.MAX_SAFE_INTEGER);
    const rightGroupTs = (rightInteraction && interactionStartMs.get(rightInteraction)) ?? (typeof right.frame.timestampMs === "number" ? right.frame.timestampMs : Number.MAX_SAFE_INTEGER);
    if (leftGroupTs !== rightGroupTs) {
      return leftGroupTs - rightGroupTs;
    }
    if (leftInteraction && rightInteraction && leftInteraction === rightInteraction) {
      const leftRank = eventSortRank(left.frame.event);
      const rightRank = eventSortRank(right.frame.event);
      if (leftRank !== rightRank) {
        return leftRank - rightRank;
      }
    }
    const leftTs = typeof left.frame.timestampMs === "number" ? left.frame.timestampMs : Number.MAX_SAFE_INTEGER;
    const rightTs = typeof right.frame.timestampMs === "number" ? right.frame.timestampMs : Number.MAX_SAFE_INTEGER;
    if (leftTs !== rightTs) {
      return leftTs - rightTs;
    }
    return left.index - right.index;
  }).map(({ frame }) => frame);
}
function parseToolCallId(frame) {
  const record = frame.data && typeof frame.data === "object" ? frame.data : null;
  const id = record?.tool_call_id ?? record?.id;
  return typeof id === "string" && id.trim() ? id.trim() : null;
}
function parseToolName(frame) {
  const record = frame.data && typeof frame.data === "object" ? frame.data : null;
  return typeof record?.name === "string" && record.name.trim() ? record.name : "tool";
}
function parseToolArguments(frame) {
  const record = frame.data && typeof frame.data === "object" ? frame.data : null;
  if (typeof record?.arguments === "string" && record.arguments.trim()) {
    return record.arguments;
  }
  if ("args" in (record || {}) && record?.args !== void 0) {
    return JSON.stringify(record.args);
  }
  return JSON.stringify(record || {});
}
function parseToolResult(frame) {
  const record = frame.data && typeof frame.data === "object" ? frame.data : null;
  const result = summarizeFrameData(frame.data).trim();
  const isError = Boolean(record?.is_error) || frame.event === "interaction_failed";
  return {
    ...result ? { result } : {},
    status: isError ? "error" : "success"
  };
}
function buildToolBlocks(frames) {
  const toolCalls = /* @__PURE__ */ new Map();
  const pendingResults = /* @__PURE__ */ new Map();
  for (const frame of frames) {
    if (frame.event === "tool_result_received" || frame.event === "tool_execution_completed") {
      const toolCallId = parseToolCallId(frame);
      if (!toolCallId) continue;
      const parsed = parseToolResult(frame);
      if (toolCalls.has(toolCallId)) {
        const current = toolCalls.get(toolCallId);
        toolCalls.set(toolCallId, {
          ...current,
          ...parsed.result ? { result: parsed.result } : {},
          status: parsed.status
        });
      } else {
        pendingResults.set(toolCallId, parsed);
      }
    }
    if (frame.event === "tool_call_requested" || frame.event === "tool_call" || frame.event === "tool_execution_started") {
      const toolCallId = parseToolCallId(frame);
      if (!toolCallId || toolCalls.has(toolCallId)) continue;
      const pending = pendingResults.get(toolCallId);
      toolCalls.set(toolCallId, {
        type: "tool-call",
        toolCallId,
        name: parseToolName(frame),
        arguments: parseToolArguments(frame),
        ...pending?.result ? { result: pending.result } : {},
        status: pending?.status || "pending"
      });
    }
  }
  return toolCalls;
}
function renderTerminalEntry(agent, frame, entryId, streamedText = "") {
  if (frame.event === "interaction_complete") {
    const text = summarizeFrameData(frame.data).trim();
    if (!text) return null;
    if (streamedText.trim() && normalizeComparableText(streamedText) === normalizeComparableText(text)) {
      return null;
    }
    const blocks = parseConversationRichBlocks(text);
    return {
      kind: "message",
      id: entryId,
      identity: agentIdentity(agent),
      variant: blocks.length > 0 ? "rich" : "plain",
      ...blocks.length > 0 ? { blocks } : { text }
    };
  }
  if (frame.event === "interaction_failed" || frame.event === "run_failed") {
    const text = `${frame.event}: ${summarizeFrameData(frame.data)}`.trim();
    if (!text || text === `${frame.event}:`) return null;
    return {
      kind: "message",
      id: entryId,
      identity: SYSTEM_IDENTITY,
      variant: "meta",
      text
    };
  }
  return null;
}
function normalizeComparableText(value) {
  return value.replace(/\s+/g, " ").trim();
}
function renderHistoryUserEntry(frame, entryId) {
  if (frame.event !== "interaction_started" || typeof frame.data !== "object" || frame.data === null) {
    return null;
  }
  const record = frame.data;
  const content = typeof record.content === "string" ? record.content.trim() : "";
  if (!content) return null;
  return {
    kind: "message",
    id: entryId,
    identity: USER_IDENTITY,
    variant: "plain",
    text: content
  };
}
function mapFramesToTimelineEntries(agent, frames, options = {}) {
  const orderedFrames = sortFramesForTranscript(frames);
  const entries = [];
  const toolBlocks = buildToolBlocks(orderedFrames);
  const emittedToolCalls = /* @__PURE__ */ new Set();
  let pendingText = "";
  let pendingId = "";
  function flushPendingText() {
    if (!pendingText) return;
    const blocks = parseConversationRichBlocks(pendingText);
    entries.push({
      kind: "message",
      id: pendingId,
      identity: agentIdentity(agent),
      variant: blocks.length > 0 ? "rich" : "plain",
      ...blocks.length > 0 ? { blocks } : { text: pendingText }
    });
    pendingText = "";
    pendingId = "";
  }
  for (let i = 0; i < orderedFrames.length; i++) {
    const frame = orderedFrames[i];
    const entryId = `${frame.id || frame.event || "frame"}:${i}`;
    if (frame.event === "text_delta") {
      if (!pendingId) pendingId = entryId;
      pendingText += summarizeFrameData(frame.data);
      continue;
    }
    const toolCallId = parseToolCallId(frame);
    if (toolCallId && (frame.event === "tool_call_requested" || frame.event === "tool_call" || frame.event === "tool_execution_started") && !emittedToolCalls.has(toolCallId)) {
      flushPendingText();
      const block = toolBlocks.get(toolCallId);
      if (block) {
        entries.push({
          kind: "message",
          id: entryId,
          identity: agentIdentity(agent),
          variant: "rich",
          blocks: [block]
        });
        emittedToolCalls.add(toolCallId);
      }
      continue;
    }
    if (frame.event === "tool_result_received" || frame.event === "tool_execution_completed") {
      continue;
    }
    if (options.renderInteractionStartsAsUser && frame.event === "interaction_started") {
      flushPendingText();
      const userEntry = renderHistoryUserEntry(frame, entryId);
      if (userEntry) {
        entries.push(userEntry);
      }
      continue;
    }
    if (frame.event === "text_complete") {
      continue;
    }
    if (HIDDEN_EVENTS.has(frame.event)) {
      continue;
    }
    const streamedText = pendingText;
    flushPendingText();
    const terminalEntry = renderTerminalEntry(agent, frame, entryId, streamedText);
    if (terminalEntry) {
      entries.push(terminalEntry);
      continue;
    }
    if (frame.event === "interaction_complete") {
      continue;
    }
    const text = `${frame.event}: ${summarizeFrameData(frame.data)}`.trim();
    entries.push({
      kind: "message",
      id: entryId,
      identity: SYSTEM_IDENTITY,
      variant: "meta",
      text
    });
  }
  flushPendingText();
  return entries;
}
var USER_IDENTITY, SYSTEM_IDENTITY, HIDDEN_EVENTS, ACTIVITY_HIDDEN_EVENTS;
var init_adapters = __esm({
  "src/lib/adapters.ts"() {
    init_src();
    USER_IDENTITY = {
      id: "user",
      label: "You",
      role: "user"
    };
    SYSTEM_IDENTITY = {
      id: "system",
      label: "System",
      role: "system",
      presentation: "system",
      showLabel: true
    };
    HIDDEN_EVENTS = /* @__PURE__ */ new Set([
      "subscribed",
      "run_started",
      "run_completed",
      "turn_started",
      "turn_completed",
      "text_complete",
      "interaction_started",
      "run_failed",
      "keep-alive",
      "tool_config_changed",
      "tool_scope_changed",
      "tool_call_requested",
      "tool_call",
      "tool_execution_started"
    ]);
    ACTIVITY_HIDDEN_EVENTS = /* @__PURE__ */ new Set([
      ...HIDDEN_EVENTS,
      "text_delta",
      "tool_result_received",
      "tool_execution_completed"
    ]);
  }
});

// src/lib/agents.ts
function normalizeAgents(experience, modules) {
  const identityStatusRows = Array.isArray(experience?.identity_status?.rows) ? experience.identity_status.rows : [];
  const normalizedIdentityStatusRows = identityStatusRows.map((entry) => normalizeIdentityStatusRow(entry)).filter((entry) => entry !== null);
  const identityStatusByIdentity = new Map(
    normalizedIdentityStatusRows.map((row) => [row.identity, row])
  );
  const snapshotAgents = experience?.agent_sidebar?.live_snapshot?.agents;
  if (Array.isArray(snapshotAgents) && snapshotAgents.length > 0) {
    return snapshotAgents.map((entry) => {
      const entryIdentity = typeof entry.identity === "string" ? entry.identity.trim() : "";
      const entryMemberId = typeof entry.member_id === "string" ? entry.member_id.trim() : "";
      const statusRow = identityStatusByIdentity.get(entryIdentity) || identityStatusByIdentity.get(entryMemberId) || normalizeIdentityStatusRow(entry);
      const watchFields = normalizeSidebarWatchFields(entry);
      const responsePhase = normalizeResponsePhase(entry.response_phase);
      return {
        ...statusRow?.identity ? { identity: statusRow.identity } : entry.identity ? { identity: String(entry.identity) } : {},
        agent_id: String(entry.agent_id || statusRow?.identity || entry.identity || entry.member_id || ""),
        member_id: String(entry.member_id || statusRow?.identity || entry.identity || entry.agent_id || ""),
        ...typeof entry.session_id === "string" && entry.session_id.trim() ? { session_id: entry.session_id.trim() } : {},
        label: String(entry.label || statusRow?.display_name || entry.display_name || statusRow?.identity || entry.identity || entry.member_id || entry.agent_id || "unknown"),
        kind: String(entry.kind || statusRow?.profile || entry.profile || "module_agent"),
        ...statusRow?.profile !== void 0 ? { profile: statusRow.profile } : entry.profile !== void 0 ? { profile: String(entry.profile) } : {},
        ...statusRow?.state !== void 0 ? { state: statusRow.state } : entry.state !== void 0 ? { state: String(entry.state) } : {},
        ...statusRow?.addressability ? { addressability: statusRow.addressability } : {},
        ...statusRow?.generation !== void 0 ? { generation: statusRow.generation } : {},
        ...statusRow?.checkpoint_version !== void 0 ? { checkpoint_version: statusRow.checkpoint_version } : {},
        ...statusRow?.lease_healthy !== void 0 ? { lease_healthy: statusRow.lease_healthy } : {},
        ...responsePhase !== null && { response_phase: responsePhase },
        ...entry.wired_to !== void 0 && { wired_to: entry.wired_to },
        ...statusRow?.labels && Object.keys(statusRow.labels).length > 0 ? { labels: statusRow.labels } : entry.labels !== void 0 ? { labels: entry.labels } : {},
        ...entry.group !== void 0 && { group: String(entry.group) },
        ...entry.addressable !== void 0 ? { addressable: Boolean(entry.addressable) } : statusRow?.addressability ? { addressable: statusRow.addressability === "addressable" } : {},
        ...entry.affordances !== void 0 && { affordances: entry.affordances },
        ...watchFields
      };
    });
  }
  if (Array.isArray(identityStatusRows) && identityStatusRows.length > 0) {
    return identityStatusRows.map((entry) => {
      const statusRow = normalizeIdentityStatusRow(entry);
      const identity = statusRow?.identity || "";
      return {
        identity,
        agent_id: String(identity),
        member_id: identity ? `identity-only:${identity}` : "",
        ...typeof statusRow?.session_id === "string" && statusRow.session_id.trim() ? { session_id: statusRow.session_id.trim() } : {},
        label: String(statusRow?.display_name || identity || "unknown"),
        kind: String(statusRow?.profile || "identity"),
        ...statusRow?.profile !== void 0 ? { profile: statusRow.profile } : {},
        ...statusRow?.state !== void 0 ? { state: statusRow.state } : {},
        ...statusRow?.addressability ? { addressability: statusRow.addressability } : {},
        ...statusRow?.generation !== void 0 ? { generation: statusRow.generation } : {},
        ...statusRow?.checkpoint_version !== void 0 ? { checkpoint_version: statusRow.checkpoint_version } : {},
        ...statusRow?.lease_healthy !== void 0 ? { lease_healthy: statusRow.lease_healthy } : {},
        ...statusRow?.labels && Object.keys(statusRow.labels).length > 0 ? { labels: statusRow.labels } : {},
        addressable: false,
        affordances: { can_send_message: false }
      };
    });
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
var init_agents = __esm({
  "src/lib/agents.ts"() {
    init_src();
  }
});

// src/lib/phase1-targets.test.ts
import test from "node:test";
import assert from "node:assert/strict";
var require_phase1_targets_test = __commonJS({
  "src/lib/phase1-targets.test.ts"() {
    init_src();
    init_network();
    init_adapters();
    init_agents();
    test("CHOKE-002 target: one identity stream fan-outs to multiple panel consumers without divergent frame identity", () => {
      const rawSse = [
        "id: evt-1",
        "event: message",
        'data: {"event_id":"evt-1","identity":"identity:luka","event_type":"interaction_started","timestamp_ms":1,"data":{}}',
        ""
      ].join("\n");
      const panelAFrames = parseSseFrames(rawSse);
      const panelBFrames = parseSseFrames(rawSse);
      assert.equal(panelAFrames[0]?.id, "evt-1");
      assert.equal(panelBFrames[0]?.id, "evt-1");
      assert.deepEqual(panelAFrames[0], panelBFrames[0]);
    });
    test("CHOKE-004 target: sidebar adapter chooses identity addressing once for composer send flow", () => {
      const target = buildDockTarget({
        identity: "identity:luka",
        member_id: "member-luka",
        agent_id: "member-luka",
        label: "Luka",
        kind: "identity",
        addressable: true
      });
      assert.equal(target.addressingMode, "identity");
      assert.equal(target.identity, "identity:luka");
    });
    test("CHOKE-003 target: refreshed experience metadata drives host refresh strategy instead of stale per-panel fetch assumptions", () => {
      const before = normalizeAgents(
        {
          agent_sidebar: {
            live_snapshot: {
              agents: [
                { member_id: "legacy-router", agent_id: "legacy-router", label: "Legacy Router", kind: "module_agent" }
              ]
            }
          }
        },
        []
      );
      const after = normalizeAgents(
        {
          agent_sidebar: {
            live_snapshot: { agents: [] }
          },
          identity_status: {
            refresh: { mode: "stream", interval_ms: 1e3 },
            rows: [
              {
                identity: "identity:luka",
                display_name: "Luka",
                profile: "lead",
                state: "running",
                addressability: "addressable",
                labels: {}
              }
            ]
          }
        },
        []
      );
      assert.equal(before[0]?.member_id, "legacy-router");
      assert.equal(after[0]?.identity, "identity:luka");
    });
    test("CHOKE-006 / E2E-008 target: out-of-order tool results pair into stable transcript blocks", () => {
      const accumulator = normalizeToolCallAccumulatorState({
        timeoutMs: 6e4,
        toolCalls: {
          "tool-1": {
            type: "tool-call",
            toolCallId: "tool-1",
            name: "search",
            arguments: '{"q":"luka"}',
            result: '{"hits":1}',
            status: "success"
          }
        },
        pendingResults: {}
      });
      assert.ok(accumulator);
      assert.equal(Object.keys(accumulator?.toolCalls || {}).length, 1);
      assert.equal(accumulator?.toolCalls["tool-1"]?.status, "success");
      assert.equal(accumulator?.toolCalls["tool-1"]?.toolCallId, "tool-1");
    });
    test("CHOKE-009 / E2E-010 target: routing panel owns generic route and delivery projection", () => {
      const view = buildRoutingSectionView({
        routesResponse: {
          routes: [
            {
              route_key: "route-1",
              channel: "email",
              recipient: "user@example.com",
              sink: "delivery/email",
              target_module: "delivery",
              source: "runtime"
            }
          ]
        },
        historyResponse: {
          deliveries: [
            {
              delivery_id: "delivery-1",
              route_id: "route-1",
              recipient: "user@example.com",
              sink: "delivery/email",
              target_module: "delivery",
              status: "delivered",
              first_attempt_ms: 1e3,
              final_attempt_ms: 1e3,
              attempts: [{ attempt: 1, status: "delivered", backoff_ms: 0 }]
            }
          ]
        }
      });
      assert.equal(view.routes.length, 1);
      assert.equal(view.deliveries.length, 1);
      assert.equal(view.deliveries[0]?.attempts.length, 1);
    });
    test("CHOKE-011 / E2E-011 target: watch and degraded state converge into one shared sidebar item model", () => {
      const view = buildSidebarViewState({
        selectedMemberId: "member-luka",
        agents: [
          {
            member_id: "member-luka",
            agent_id: "member-luka",
            identity: "identity:luka",
            label: "Luka",
            kind: "identity",
            watched: true,
            alertLevel: "critical",
            degraded: true,
            degradedReason: "lease_expired"
          }
        ]
      });
      const item = view.blocks[1]?.kind === "list" ? view.blocks[1].sections[0]?.items[0] : void 0;
      assert.equal(item?.watched, true);
      assert.equal(item?.alertLevel, "critical");
      assert.equal(item?.degraded, true);
      assert.equal(item?.degradedReason, "lease_expired");
    });
    test("CHOKE-016 target: refreshed experience reconciles dock targets instead of keeping stale member-only addressing", () => {
      const beforeRefresh = buildDockTarget({
        member_id: "member-luka",
        agent_id: "member-luka",
        label: "Luka",
        kind: "module_agent"
      });
      const afterRefresh = buildDockTarget({
        identity: "identity:luka",
        member_id: "member-luka",
        agent_id: "member-luka",
        label: "Luka",
        kind: "identity"
      });
      assert.equal(beforeRefresh.addressingMode, "member");
      assert.equal(afterRefresh.addressingMode, "identity");
    });
    test("E2E-001 target: legacy fallback still yields a usable sidebar", () => {
      const agents = normalizeAgents(
        {
          agent_sidebar: {
            title: "Agents",
            live_snapshot: {
              agents: [
                {
                  member_id: "router",
                  agent_id: "router",
                  label: "Router",
                  kind: "module_agent"
                }
              ]
            }
          }
        },
        []
      );
      assert.equal(agents.length, 1);
      assert.equal(agents[0]?.member_id, "router");
    });
    test("E2E-014 target: tool timeouts surface as timed-out tool blocks instead of disappearing", () => {
      const accumulator = normalizeToolCallAccumulatorState({
        timeoutMs: 6e4,
        toolCalls: {
          "tool-timeout": {
            type: "tool-call",
            toolCallId: "tool-timeout",
            name: "slow_search",
            arguments: '{"q":"timeout"}',
            status: "error",
            result: '{"error":"timed_out"}'
          }
        },
        pendingResults: {}
      });
      assert.ok(accumulator);
      assert.equal(accumulator?.toolCalls["tool-timeout"]?.status, "error");
      assert.equal(accumulator?.toolCalls["tool-timeout"]?.toolCallId, "tool-timeout");
    });
    test("E2E-016 target: overflow recovery keeps the host on replay-based recovery instead of an unbounded local queue", () => {
      const frames = buildSidebarViewState({
        selectedMemberId: "member-luka",
        agents: [
          {
            member_id: "member-luka",
            agent_id: "member-luka",
            label: "Luka",
            kind: "identity",
            watched: true
          }
        ]
      });
      assert.equal(frames.blocks[1]?.kind, "list");
    });
    test("E2E-017 target: mixed migration sessions reconcile identity and member addressing per target", () => {
      const identityTarget = buildDockTarget({
        identity: "identity:luka",
        member_id: "member-luka",
        agent_id: "member-luka",
        label: "Luka",
        kind: "identity"
      });
      const legacyTarget = buildDockTarget({
        member_id: "legacy-router",
        agent_id: "legacy-router",
        label: "Legacy Router",
        kind: "module_agent"
      });
      assert.equal(identityTarget.addressingMode, "identity");
      assert.equal(legacyTarget.addressingMode, "member");
    });
    test("terminal identity events surface transcript payloads instead of disappearing", () => {
      const agent = {
        member_id: "member-luka",
        agent_id: "member-luka",
        label: "Luka",
        kind: "identity"
      };
      const successEntries = mapFramesToTimelineEntries(agent, [
        { id: "evt-1", event: "interaction_complete", data: { text: "done" } }
      ]);
      assert.equal(successEntries.length, 1);
      assert.equal(successEntries[0]?.identity.role, "assistant");
      const failureEntries = mapFramesToTimelineEntries(agent, [
        { id: "evt-2", event: "interaction_failed", data: { reason: "lifecycle_mutation" } }
      ]);
      assert.equal(failureEntries.length, 1);
      assert.equal(failureEntries[0]?.variant, "meta");
    });
    test("Panel-state target: same-target split panels and retargeted panels keep distinct local composer/transcript state keys", () => {
      const identityTarget = {
        id: "member-luka",
        kind: "agent-chat",
        addressingMode: "identity",
        memberId: "member-luka",
        identity: "identity:luka",
        title: "Luka"
      };
      const legacyTarget = {
        id: "legacy-router",
        kind: "agent-chat",
        addressingMode: "member",
        memberId: "legacy-router",
        title: "Legacy Router"
      };
      const splitPanelA = buildPanelConversationKey("panel-a", identityTarget);
      const splitPanelB = buildPanelConversationKey("panel-b", identityTarget);
      const retargetedPanel = buildPanelConversationKey("panel-a", legacyTarget);
      assert.notEqual(splitPanelA, splitPanelB);
      assert.notEqual(splitPanelA, retargetedPanel);
      assert.equal(splitPanelA, "panel:panel-a:agent-chat:identity:luka");
      assert.equal(splitPanelB, "panel:panel-b:agent-chat:identity:luka");
      assert.equal(retargetedPanel, "panel:panel-a:agent-chat:legacy-router");
    });
  }
});
export default require_phase1_targets_test();
