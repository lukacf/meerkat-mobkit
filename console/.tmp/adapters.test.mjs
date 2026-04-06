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

// src/lib/adapters.ts
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
function mergeConversationFrames(...frameSets) {
  const byId = /* @__PURE__ */ new Map();
  const ordered = [];
  for (const frameSet of frameSets) {
    for (const frame of frameSet || []) {
      const key = frame.id || `${frame.event}:${frame.timestampMs || 0}`;
      if (byId.has(key)) {
        continue;
      }
      byId.set(key, frame);
      ordered.push(frame);
    }
  }
  return ordered;
}
function isoFromTimestampMs(timestampMs) {
  if (typeof timestampMs !== "number" || !Number.isFinite(timestampMs)) {
    return void 0;
  }
  return new Date(timestampMs).toISOString();
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
      createdAt: isoFromTimestampMs(frame.timestampMs),
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
      createdAt: isoFromTimestampMs(frame.timestampMs),
      text
    };
  }
  return null;
}
function normalizeComparableText(value) {
  return value.replace(/\s+/g, " ").trim();
}
function buildQuickPromptSuggestions(agent) {
  const labels = agent?.labels ?? {};
  const suggestions = [];
  for (let index = 1; index <= 4; index++) {
    const label = labels[`console_prompt_${index}_label`]?.trim();
    const value = labels[`console_prompt_${index}_value`]?.trim();
    if (!label || !value) continue;
    suggestions.push({
      id: `prompt-${index}`,
      label,
      value,
      iconName: "i-bolt"
    });
  }
  return suggestions;
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
    createdAt: isoFromTimestampMs(frame.timestampMs),
    text: content
  };
}
function extractTextFromContentBlocks(blocks) {
  if (typeof blocks === "string") {
    return blocks;
  }
  if (!Array.isArray(blocks)) {
    return "";
  }
  return blocks.map((block) => {
    if (typeof block === "string") return block;
    if (!block || typeof block !== "object") return "";
    const record = block;
    if (typeof record.text === "string") return record.text;
    if (typeof record.content === "string") return record.content;
    return "";
  }).filter((value) => value.trim().length > 0).join("");
}
function historyMessageText(message) {
  if (!message || typeof message !== "object") {
    return { role: null, text: "" };
  }
  const record = message;
  const role = typeof record.role === "string" ? record.role : null;
  switch (role) {
    case "user":
      return { role: "user", text: extractTextFromContentBlocks(record.content) };
    case "assistant":
      return { role: "assistant", text: typeof record.content === "string" ? record.content : "" };
    case "block_assistant": {
      const blocks = Array.isArray(record.blocks) ? record.blocks : [];
      const text = blocks.map((block) => {
        if (!block || typeof block !== "object") return "";
        const item = block;
        const blockType = typeof item.block_type === "string" ? item.block_type : typeof item.type === "string" ? item.type : "";
        const data = item.data && typeof item.data === "object" ? item.data : {};
        if (blockType === "text") {
          if (typeof data.text === "string") return data.text;
          if (typeof item.text === "string") return item.text;
        }
        return "";
      }).filter((value) => value.trim().length > 0).join("");
      return { role: "assistant", text };
    }
    case "system":
      return { role: "system", text: typeof record.content === "string" ? record.content : "" };
    default:
      return { role: null, text: "" };
  }
}
function mapSessionHistoryToTimelineEntries(historyPage, agent) {
  if (!historyPage || typeof historyPage !== "object") {
    return [];
  }
  const record = historyPage;
  const messages = Array.isArray(record.messages) ? record.messages : [];
  const entries = [];
  for (const [index, message] of messages.entries()) {
    const parsed = historyMessageText(message);
    const text = parsed.text.trim();
    const messageRecord = message && typeof message === "object" ? message : null;
    const createdAt = typeof messageRecord?.created_at === "string" ? messageRecord.created_at : typeof messageRecord?.createdAt === "string" ? messageRecord.createdAt : void 0;
    if (!text) {
      continue;
    }
    if (parsed.role === "system") {
      if (!text.startsWith("[COMMS") && !text.startsWith("[SYSTEM NOTICE")) {
        continue;
      }
      if (text.startsWith("[SYSTEM NOTICE][TOOL_SCOPE]")) {
        continue;
      }
      entries.push({
        kind: "message",
        id: `history:${index}`,
        identity: SYSTEM_IDENTITY,
        variant: "meta",
        ...createdAt ? { createdAt } : {},
        text
      });
      continue;
    }
    if (parsed.role === "user" && text.startsWith("[SYSTEM NOTICE][TOOL_SCOPE]")) {
      continue;
    }
    const blocks = parseConversationRichBlocks(text);
    entries.push({
      kind: "message",
      id: `history:${index}`,
      identity: parsed.role === "user" ? USER_IDENTITY : agentIdentity(agent),
      variant: blocks.length > 0 ? "rich" : "plain",
      ...createdAt ? { createdAt } : {},
      ...blocks.length > 0 ? { blocks } : { text }
    });
  }
  return entries;
}
function mapFramesToTimelineEntries(agent, frames, options = {}) {
  const orderedFrames = sortFramesForTranscript(frames);
  const entries = [];
  const toolBlocks = buildToolBlocks(orderedFrames);
  const emittedToolCalls = /* @__PURE__ */ new Set();
  let pendingText = "";
  let pendingId = "";
  let pendingCreatedAt;
  function flushPendingText() {
    if (!pendingText) return;
    const blocks = parseConversationRichBlocks(pendingText);
    entries.push({
      kind: "message",
      id: pendingId,
      identity: agentIdentity(agent),
      variant: blocks.length > 0 ? "rich" : "plain",
      ...pendingCreatedAt ? { createdAt: pendingCreatedAt } : {},
      ...blocks.length > 0 ? { blocks } : { text: pendingText }
    });
    pendingText = "";
    pendingId = "";
    pendingCreatedAt = void 0;
  }
  for (let i = 0; i < orderedFrames.length; i++) {
    const frame = orderedFrames[i];
    const entryId = `${frame.id || frame.event || "frame"}:${i}`;
    if (frame.event === "text_delta") {
      if (!pendingId) {
        pendingId = entryId;
        pendingCreatedAt = isoFromTimestampMs(frame.timestampMs);
      }
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
          createdAt: isoFromTimestampMs(frame.timestampMs),
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
      createdAt: isoFromTimestampMs(frame.timestampMs),
      text
    });
  }
  flushPendingText();
  return entries;
}
function sortConversationTimelineEntries(entries) {
  return entries.map((entry, index) => ({ entry, index })).sort((left, right) => {
    const leftTs = Date.parse(String(left.entry.createdAt || ""));
    const rightTs = Date.parse(String(right.entry.createdAt || ""));
    const safeLeft = Number.isFinite(leftTs) ? leftTs : Number.NaN;
    const safeRight = Number.isFinite(rightTs) ? rightTs : Number.NaN;
    if (Number.isFinite(safeLeft) && Number.isFinite(safeRight) && safeLeft !== safeRight) {
      return safeLeft - safeRight;
    }
    if (Number.isFinite(safeLeft) && !Number.isFinite(safeRight)) {
      return 1;
    }
    if (!Number.isFinite(safeLeft) && Number.isFinite(safeRight)) {
      return -1;
    }
    return left.index - right.index;
  }).map(({ entry }) => entry);
}
function buildActivityRailViewState(args) {
  const presets = args.filterPresets || [];
  const activePreset = presets.find((preset) => preset.id === args.activePresetId) || null;
  const agentByIdentity = /* @__PURE__ */ new Map();
  const watchedIdentities = /* @__PURE__ */ new Set();
  const criticalIdentities = /* @__PURE__ */ new Set();
  for (const agent of args.agents) {
    if (agent.identity) agentByIdentity.set(agent.identity, agent);
    agentByIdentity.set(agent.member_id, agent);
    if (agent.watched && (agent.identity || agent.member_id)) {
      watchedIdentities.add(agent.identity || agent.member_id);
    }
    if (agent.alertLevel === "critical" && (agent.identity || agent.member_id)) {
      criticalIdentities.add(agent.identity || agent.member_id);
    }
  }
  const filteredFrames = args.eventFrames.filter((frame) => {
    if (ACTIVITY_HIDDEN_EVENTS.has(frame.event)) {
      return false;
    }
    const frameIdentity = frame.identity?.trim();
    if (!activePreset) return true;
    if (activePreset.watchedOnly && frameIdentity && !watchedIdentities.has(frameIdentity)) {
      return false;
    }
    if (activePreset.alertLevels?.length && frameIdentity) {
      const agent = agentByIdentity.get(frameIdentity);
      if (!agent?.alertLevel || !activePreset.alertLevels.includes(agent.alertLevel)) {
        return false;
      }
    }
    if (activePreset.eventTypeFilter?.length && !activePreset.eventTypeFilter.includes(frame.event)) {
      return false;
    }
    return true;
  });
  const pulseItems = filteredFrames.slice(0, 50).map((frame, index) => {
    const frameIdentity = frame.identity?.trim();
    const agent = frameIdentity ? agentByIdentity.get(frameIdentity) : null;
    return {
      id: `event:${frame.id || index}`,
      title: agent?.label || frameIdentity || frame.event || "event",
      line: summarizeFrameData(frame.data).slice(0, 120) || frame.event,
      meta: frame.event || frame.id || "",
      ...agent ? { focusId: agent.member_id } : {}
    };
  });
  return {
    panels: [
      {
        id: "pulse",
        kind: "pulse",
        title: "Activity",
        actions: presets.map((preset) => ({
          id: preset.id,
          label: preset.label,
          active: preset.id === (activePreset?.id || "all")
        })),
        items: pulseItems,
        emptyText: "No events yet"
      }
    ]
  };
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

// src/lib/adapters.test.ts
import assert from "node:assert/strict";
import test from "node:test";
var require_adapters_test = __commonJS({
  "src/lib/adapters.test.ts"() {
    init_adapters();
    test("buildSidebarViewState preserves host-derived watch and degraded fields", () => {
      const viewState = buildSidebarViewState({
        selectedMemberId: "member-1",
        pinnedAgentIds: /* @__PURE__ */ new Set(["member-1"]),
        agents: [
          {
            agent_id: "identity:luka",
            member_id: "member-1",
            label: "Luka",
            kind: "operator",
            profile: "console",
            state: "running",
            watched: true,
            alertLevel: "elevated",
            degraded: true,
            degradedReason: "lease_expired"
          }
        ]
      });
      const item = viewState.blocks[1]?.sections?.[0]?.items?.[0];
      assert.equal(item?.id, "member-1");
      assert.equal(item?.pinned, true);
      assert.equal(item?.selected, true);
      assert.equal(item?.watched, true);
      assert.equal(item?.alertLevel, "elevated");
      assert.equal(item?.degraded, true);
      assert.equal(item?.degradedReason, "lease_expired");
      assert.equal(item?.meta?.[0]?.tone, "accent");
    });
    test("buildRoutingSectionView projects runtime routing and delivery results without host invention", () => {
      const view = buildRoutingSectionView({
        routesResponse: {
          routes: [
            {
              route_key: "vip-route",
              recipient: "vip@example.com",
              channel: "notification",
              sink: "sms",
              target_module: "delivery",
              retry_max: 0,
              backoff_ms: 5,
              rate_limit_per_minute: 9
            }
          ]
        },
        historyResponse: {
          deliveries: [
            {
              delivery_id: "delivery-1",
              route_id: "route-000001",
              recipient: "vip@example.com",
              sink: "sms",
              target_module: "delivery",
              status: "sent",
              first_attempt_ms: 100,
              final_attempt_ms: 200,
              idempotency_key: "delivery-key-1",
              sink_adapter: "sms-mock",
              attempts: [
                { attempt: 1, status: "sent", backoff_ms: 0 }
              ]
            }
          ]
        }
      });
      assert.deepEqual(view, {
        routes: [
          {
            route_key: "vip-route",
            recipient: "vip@example.com",
            channel: "notification",
            sink: "sms",
            target_module: "delivery",
            retry_max: 0,
            backoff_ms: 5,
            rate_limit_per_minute: 9
          }
        ],
        deliveries: [
          {
            delivery_id: "delivery-1",
            route_id: "route-000001",
            recipient: "vip@example.com",
            sink: "sms",
            target_module: "delivery",
            status: "sent",
            first_attempt_ms: 100,
            final_attempt_ms: 200,
            idempotency_key: "delivery-key-1",
            sink_adapter: "sms-mock",
            attempts: [
              { attempt: 1, status: "sent", backoff_ms: 0 }
            ]
          }
        ]
      });
    });
    test("mapFramesToTimelineEntries suppresses duplicate terminal text after streamed deltas", () => {
      const entries = mapFramesToTimelineEntries(
        {
          agent_id: "incident-commander",
          member_id: "incident-commander",
          label: "Incident Commander",
          kind: "identity"
        },
        [
          {
            id: "evt-1",
            event: "text_delta",
            data: { delta: "Hello! How can I assist you today?" }
          },
          {
            id: "evt-2",
            event: "interaction_complete",
            data: { text: "Hello! How can I assist you today?" }
          }
        ]
      );
      assert.equal(entries.length, 1);
      assert.equal(entries[0]?.kind, "message");
      assert.equal(
        entries[0] && "blocks" in entries[0] && Array.isArray(entries[0].blocks) ? entries[0].blocks[0]?.type === "paragraph" ? entries[0].blocks[0].text : "" : "",
        "Hello! How can I assist you today?"
      );
    });
    test("mapFramesToTimelineEntries ignores text_complete so the terminal event does not duplicate the same answer", () => {
      const entries = mapFramesToTimelineEntries(
        {
          agent_id: "incident-commander",
          member_id: "incident-commander",
          label: "Incident Commander",
          kind: "identity"
        },
        [
          {
            id: "evt-1",
            event: "text_delta",
            data: { delta: "Status is stable." }
          },
          {
            id: "evt-2",
            event: "text_complete",
            data: { content: "Status is stable." }
          },
          {
            id: "evt-3",
            event: "interaction_complete",
            data: { text: "Status is stable." }
          }
        ]
      );
      assert.equal(entries.length, 1);
      assert.equal(
        entries[0] && "blocks" in entries[0] && Array.isArray(entries[0].blocks) ? entries[0].blocks[0]?.type === "paragraph" ? entries[0].blocks[0].text : "" : "",
        "Status is stable."
      );
    });
    test("mapFramesToTimelineEntries ignores hidden turn markers before terminal completion", () => {
      const entries = mapFramesToTimelineEntries(
        {
          agent_id: "incident-commander",
          member_id: "incident-commander",
          label: "Incident Commander",
          kind: "identity"
        },
        [
          { id: "evt-1", event: "text_delta", data: { delta: "Status is stable." } },
          { id: "evt-2", event: "text_complete", data: { content: "Status is stable." } },
          { id: "evt-3", event: "turn_completed", data: { stop_reason: "end_turn" } },
          { id: "evt-4", event: "interaction_complete", data: { text: "Status is stable." } }
        ]
      );
      assert.equal(entries.length, 1);
      assert.equal(
        entries[0] && "blocks" in entries[0] && Array.isArray(entries[0].blocks) ? entries[0].blocks[0]?.type === "paragraph" ? entries[0].blocks[0].text : "" : "",
        "Status is stable."
      );
    });
    test("mapSessionHistoryToTimelineEntries preserves real session ordering and inbound comms notices", () => {
      const entries = mapSessionHistoryToTimelineEntries(
        {
          session_id: "session-1",
          message_count: 5,
          offset: 0,
          has_more: false,
          messages: [
            {
              role: "system",
              content: "You are the incident commander."
            },
            {
              role: "user",
              content: "Talk to scribe."
            },
            {
              role: "user",
              content: "[SYSTEM NOTICE][TOOL_SCOPE] Tool configuration changed at turn boundary"
            },
            {
              role: "system",
              content: "[COMMS MESSAGE from incident-command-center/incident_commander/incident-commander] Please summarize the timeline."
            },
            {
              role: "block_assistant",
              blocks: [
                { block_type: "text", data: { text: "Scribe is preparing a summary." } }
              ],
              stop_reason: "end_turn"
            }
          ]
        },
        {
          agent_id: "scribe",
          member_id: "scribe",
          label: "Scribe",
          kind: "identity"
        }
      );
      assert.equal(entries.length, 3);
      assert.equal(entries[0]?.identity.label, "You");
      assert.equal(entries[1]?.identity.label, "System");
      assert.equal(entries[2]?.identity.label, "Scribe");
      assert.equal(
        entries[2] && "blocks" in entries[2] && Array.isArray(entries[2].blocks) ? entries[2].blocks[0]?.type === "paragraph" ? entries[2].blocks[0].text : "" : "",
        "Scribe is preparing a summary."
      );
    });
    test("sortConversationTimelineEntries keeps optimistic user messages after older assistant replies", () => {
      const entries = sortConversationTimelineEntries([
        {
          kind: "message",
          id: "assistant-1",
          identity: { id: "agent-1", label: "Agent", role: "assistant" },
          variant: "plain",
          text: "Assistant 1",
          createdAt: "2026-04-04T10:00:01.000Z"
        },
        {
          kind: "message",
          id: "user-1",
          identity: { id: "user", label: "You", role: "user" },
          variant: "plain",
          text: "User 1",
          createdAt: "2026-04-04T10:00:00.000Z"
        },
        {
          kind: "message",
          id: "user-2",
          identity: { id: "user", label: "You", role: "user" },
          variant: "plain",
          text: "User 2",
          createdAt: "2026-04-04T10:00:02.000Z"
        }
      ]);
      assert.deepEqual(entries.map((entry) => entry.id), ["user-1", "assistant-1", "user-2"]);
    });
    test("mapFramesToTimelineEntries renders tool turns without raw tool lifecycle system noise", () => {
      const entries = mapFramesToTimelineEntries(
        {
          agent_id: "incident-commander",
          member_id: "incident-commander",
          label: "Incident Commander",
          kind: "identity"
        },
        [
          {
            id: "evt-1",
            event: "tool_call_requested",
            data: { id: "call-1", name: "send", args: { to: "payments-sre", body: "Check status" } }
          },
          {
            id: "evt-2",
            event: "tool_execution_started",
            data: { id: "call-1", name: "send" }
          },
          {
            id: "evt-3",
            event: "tool_execution_completed",
            data: { id: "call-1", name: "send", result: '{"status":"sent"}' }
          },
          {
            id: "evt-4",
            event: "text_delta",
            data: { delta: "Sent the status check." }
          },
          {
            id: "evt-5",
            event: "interaction_complete",
            data: { text: "Sent the status check." }
          }
        ]
      );
      assert.equal(entries.length, 2);
      assert.equal(entries[0]?.kind, "message");
      assert.equal(entries[0]?.identity.role, "assistant");
      assert.equal(
        entries[0] && "blocks" in entries[0] && Array.isArray(entries[0].blocks) ? entries[0].blocks[0]?.type : "",
        "tool-call"
      );
      assert.equal(entries[1]?.identity.role, "assistant");
      assert.equal(
        entries[1] && "blocks" in entries[1] && Array.isArray(entries[1].blocks) ? entries[1].blocks[0]?.type === "paragraph" ? entries[1].blocks[0].text : "" : "",
        "Sent the status check."
      );
    });
    test("mapFramesToTimelineEntries can render historical interaction_started frames as user messages", () => {
      const entries = mapFramesToTimelineEntries(
        null,
        [
          {
            id: "evt-1",
            event: "interaction_started",
            data: { content: "Run a status sweep." }
          }
        ],
        { renderInteractionStartsAsUser: true }
      );
      assert.equal(entries.length, 1);
      assert.equal(entries[0]?.identity.role, "user");
      assert.equal(entries[0] && "text" in entries[0] ? entries[0].text : "", "Run a status sweep.");
    });
    test("mapFramesToTimelineEntries orders persisted interaction history by interaction semantics, not raw arrival order", () => {
      const entries = mapFramesToTimelineEntries(
        {
          agent_id: "incident-commander",
          member_id: "incident-commander",
          label: "Incident Commander",
          kind: "identity"
        },
        [
          {
            id: "evt-2",
            event: "text_delta",
            interactionId: "turn-1",
            timestampMs: 10,
            data: { delta: "Working on it." }
          },
          {
            id: "evt-1",
            event: "interaction_started",
            interactionId: "turn-1",
            timestampMs: 11,
            data: { content: "Run a status sweep." }
          },
          {
            id: "evt-3",
            event: "interaction_complete",
            interactionId: "turn-1",
            timestampMs: 12,
            data: { text: "Working on it." }
          }
        ],
        { renderInteractionStartsAsUser: true }
      );
      assert.equal(entries.length, 2);
      assert.equal(entries[0]?.identity.role, "user");
      assert.equal(entries[0] && "text" in entries[0] ? entries[0].text : "", "Run a status sweep.");
      assert.equal(entries[1]?.identity.role, "assistant");
      assert.equal(
        entries[1] && "blocks" in entries[1] && Array.isArray(entries[1].blocks) ? entries[1].blocks[0]?.type === "paragraph" ? entries[1].blocks[0].text : "" : "",
        "Working on it."
      );
    });
    test("mapFramesToTimelineEntries decodes stringified delta payloads from persisted history", () => {
      const entries = mapFramesToTimelineEntries(
        {
          agent_id: "merchant-success",
          member_id: "merchant-success",
          label: "Merchant Success",
          kind: "identity"
        },
        [
          {
            id: "evt-1",
            event: "text_delta",
            timestampMs: 1,
            data: '{"delta":"Enterprise merchants are experiencing significant payment failures.","source_event_type":"text_delta","type":"text_delta"}'
          },
          {
            id: "evt-2",
            event: "interaction_complete",
            timestampMs: 2,
            data: { text: "Enterprise merchants are experiencing significant payment failures." }
          }
        ]
      );
      assert.equal(entries.length, 1);
      assert.equal(
        entries[0] && "blocks" in entries[0] && Array.isArray(entries[0].blocks) ? entries[0].blocks[0]?.type === "paragraph" ? entries[0].blocks[0].text : "" : "",
        "Enterprise merchants are experiencing significant payment failures."
      );
    });
    test("mapFramesToTimelineEntries preserves whitespace-only text deltas instead of stringifying the payload", () => {
      const entries = mapFramesToTimelineEntries(
        {
          agent_id: "payments-sre",
          member_id: "payments-sre",
          label: "Payments SRE",
          kind: "identity"
        },
        [
          { id: "evt-1", event: "text_delta", data: { delta: "Payments-API remains degraded at" } },
          { id: "evt-2", event: "text_delta", data: { delta: " " } },
          { id: "evt-3", event: "text_delta", data: { delta: "38%" } },
          { id: "evt-4", event: "interaction_complete", data: { text: "Payments-API remains degraded at 38%" } }
        ]
      );
      assert.equal(entries.length, 1);
      assert.equal(
        entries[0] && "blocks" in entries[0] && Array.isArray(entries[0].blocks) ? entries[0].blocks[0]?.type === "paragraph" ? entries[0].blocks[0].text : "" : "",
        "Payments-API remains degraded at 38%"
      );
    });
    test("mergeConversationFrames deduplicates history and live copies of the same event", () => {
      const merged = mergeConversationFrames(
        [
          {
            id: "evt-1",
            event: "interaction_started",
            interactionId: "turn-1",
            data: { content: "Run a status sweep." }
          },
          {
            id: "evt-2",
            event: "text_delta",
            interactionId: "turn-1",
            data: { delta: "Working" }
          }
        ],
        [
          {
            id: "evt-2",
            event: "text_delta",
            interactionId: "turn-1",
            data: { delta: "Working" }
          },
          {
            id: "evt-3",
            event: "interaction_complete",
            interactionId: "turn-1",
            data: { text: "Working" }
          }
        ]
      );
      assert.deepEqual(merged.map((frame) => frame.id), ["evt-1", "evt-2", "evt-3"]);
    });
    test("buildActivityRailViewState hides text deltas and internal config churn", () => {
      const view = buildActivityRailViewState({
        agents: [
          {
            agent_id: "incident-commander",
            member_id: "incident-commander",
            identity: "incident-commander",
            label: "Incident Commander",
            kind: "identity"
          }
        ],
        eventFrames: [
          {
            id: "evt-1",
            event: "text_delta",
            identity: "incident-commander",
            data: { delta: "hello" }
          },
          {
            id: "evt-2",
            event: "tool_config_changed",
            identity: "incident-commander",
            data: { target: "tool_scope" }
          },
          {
            id: "evt-3",
            event: "tool_call_requested",
            identity: "incident-commander",
            data: { id: "call-1", name: "send" }
          },
          {
            id: "evt-4",
            event: "tool_execution_started",
            identity: "incident-commander",
            data: { id: "call-1", name: "send" }
          },
          {
            id: "evt-5",
            event: "interaction_complete",
            identity: "incident-commander",
            data: { text: "done" }
          }
        ]
      });
      assert.deepEqual(view.panels[0]?.items.map((item) => item.id), ["event:evt-5"]);
    });
    test("buildQuickPromptSuggestions projects stock prompt labels into runnable suggestions", () => {
      const suggestions = buildQuickPromptSuggestions({
        agent_id: "incident-commander",
        member_id: "incident-commander",
        label: "Incident Commander",
        kind: "identity",
        labels: {
          console_prompt_1_label: "Status sweep",
          console_prompt_1_value: "Run a status sweep.",
          console_prompt_2_label: "Merchant impact",
          console_prompt_2_value: "Summarize merchant impact."
        }
      });
      assert.deepEqual(
        suggestions.map((suggestion) => ({ label: suggestion.label, value: suggestion.value })),
        [
          { label: "Status sweep", value: "Run a status sweep." },
          { label: "Merchant impact", value: "Summarize merchant impact." }
        ]
      );
    });
  }
});
export default require_adapters_test();
