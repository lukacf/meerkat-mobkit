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
var import_client = require("react-dom/client");

// src/ConsoleApp.tsx
var import_react25 = __toESM(require("react"));

// node_modules/clsx/dist/clsx.mjs
function r(e) {
  var t, f, n = "";
  if ("string" == typeof e || "number" == typeof e) n += e;
  else if ("object" == typeof e) if (Array.isArray(e)) {
    var o = e.length;
    for (t = 0; t < o; t++) e[t] && (f = r(e[t])) && (n && (n += " "), n += f);
  } else for (f in e) e[f] && (n && (n += " "), n += f);
  return n;
}
function clsx() {
  for (var e, t, f = 0, n = "", o = arguments.length; f < o; f++) (e = arguments[f]) && (t = r(e)) && (n && (n += " "), n += t);
  return n;
}
var clsx_default = clsx;

// ../packages/console-components/src/shared.ts
function fallbackCopyTextToClipboard(text) {
  if (typeof document === "undefined" || !document.body || typeof document.execCommand !== "function") {
    return false;
  }
  const textarea = document.createElement("textarea");
  textarea.value = text;
  textarea.setAttribute("readonly", "true");
  textarea.style.position = "fixed";
  textarea.style.top = "0";
  textarea.style.left = "0";
  textarea.style.opacity = "0";
  textarea.style.pointerEvents = "none";
  document.body.appendChild(textarea);
  const selection = typeof document.getSelection === "function" ? document.getSelection() : null;
  const existingRanges = selection ? Array.from({ length: selection.rangeCount }, (_value, index) => selection.getRangeAt(index)) : [];
  textarea.focus();
  textarea.select();
  textarea.setSelectionRange(0, textarea.value.length);
  let copied = false;
  try {
    copied = document.execCommand("copy");
  } catch {
    copied = false;
  }
  document.body.removeChild(textarea);
  if (selection) {
    selection.removeAllRanges();
    existingRanges.forEach((range) => selection.addRange(range));
  }
  return copied;
}
async function copyTextToClipboard(text) {
  if (!text.trim()) {
    return false;
  }
  if (typeof navigator !== "undefined" && navigator.clipboard?.writeText) {
    try {
      await navigator.clipboard.writeText(text);
      return true;
    } catch {
    }
  }
  return fallbackCopyTextToClipboard(text);
}

// ../packages/console-components/src/activity/console-activity-rail.tsx
var import_jsx_runtime = require("react/jsx-runtime");

// ../packages/console-components/src/conversation/conversation-empty-state.tsx
var import_jsx_runtime2 = require("react/jsx-runtime");

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
function normalizeStringArray(value) {
  if (!Array.isArray(value)) {
    return void 0;
  }
  const normalized = Array.from(new Set(value.map(trimString).filter((entry) => Boolean(entry))));
  return normalized.length > 0 ? normalized : void 0;
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
    ...trimString(record.role) ? { role: trimString(record.role) } : {},
    ...typeof record.generation === "number" && Number.isFinite(record.generation) ? { generation: record.generation } : {},
    ...typeof record.checkpoint_version === "number" && Number.isFinite(record.checkpoint_version) ? { checkpoint_version: record.checkpoint_version } : {},
    ...typeof record.lease_healthy === "boolean" ? { lease_healthy: record.lease_healthy } : {}
  };
}
function normalizeIdentityInspectViewState(value) {
  const record = value && typeof value === "object" ? value : null;
  const statusRow = normalizeIdentityStatusRow(value);
  if (!record || !statusRow) {
    return null;
  }
  const continuityRecord = record.continuity && typeof record.continuity === "object" ? record.continuity : {};
  const leaseRecord = record.lease && typeof record.lease === "object" ? record.lease : record.lease === null ? null : void 0;
  return {
    ...statusRow,
    continuity: {
      ...normalizeFiniteNumber(continuityRecord.generation) !== void 0 ? { generation: normalizeFiniteNumber(continuityRecord.generation) } : {},
      ...normalizeFiniteNumber(continuityRecord.checkpoint_version) !== void 0 ? { checkpoint_version: normalizeFiniteNumber(continuityRecord.checkpoint_version) } : {},
      ...trimString(continuityRecord.session_id) ? { session_id: trimString(continuityRecord.session_id) } : {},
      ...trimString(continuityRecord.agent_runtime_id) ? { agent_runtime_id: trimString(continuityRecord.agent_runtime_id) } : {}
    },
    ...leaseRecord === null ? { lease: null } : leaseRecord && normalizeFiniteNumber(leaseRecord.fencing_token) !== void 0 && normalizeFiniteNumber(leaseRecord.ttl_remaining_ms) !== void 0 && typeof leaseRecord.healthy === "boolean" ? {
      lease: {
        fencing_token: normalizeFiniteNumber(leaseRecord.fencing_token),
        ttl_remaining_ms: normalizeFiniteNumber(leaseRecord.ttl_remaining_ms),
        healthy: leaseRecord.healthy
      }
    } : {},
    ...trimString(record.output_preview) !== void 0 ? { output_preview: trimString(record.output_preview) ?? null } : {},
    ...typeof record.is_final === "boolean" || record.is_final === null ? { is_final: record.is_final } : {},
    ...normalizeFiniteNumber(record.peer_reachable_count) !== void 0 ? { peer_reachable_count: normalizeFiniteNumber(record.peer_reachable_count) } : record.peer_reachable_count === null ? { peer_reachable_count: null } : {},
    ...normalizeStringArray(record.topology_peers) ? { topology_peers: normalizeStringArray(record.topology_peers) } : {},
    ...Array.isArray(record.recent_tool_calls) ? { recent_tool_calls: record.recent_tool_calls } : {},
    ...normalizeFiniteNumber(record.last_activity_ms) !== void 0 ? { last_activity_ms: normalizeFiniteNumber(record.last_activity_ms) } : record.last_activity_ms === null ? { last_activity_ms: null } : {}
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
function normalizeReplayUnavailableError(value) {
  const record = value && typeof value === "object" ? value : null;
  if (!record || record.error !== "replay_unavailable") {
    return null;
  }
  const stream = record.stream === "identity" || record.stream === "all_events" ? record.stream : null;
  const requested = trimString(record.requested_last_event_id);
  const latest = trimString(record.latest_event_id);
  if (!stream || !requested || !latest) {
    return null;
  }
  return {
    error: "replay_unavailable",
    stream,
    requested_last_event_id: requested,
    latest_event_id: latest
  };
}
function normalizeConsoleInteractionRejectedError(value) {
  const record = value && typeof value === "object" ? value : null;
  if (!record) {
    return null;
  }
  const code = record.code;
  const message = trimString(record.message);
  if (code !== -32001 && code !== -32002 && code !== -32003 && code !== -32004 && code !== -32602 && code !== -32603) {
    return null;
  }
  if (!message) {
    return null;
  }
  return { code, message };
}

// ../packages/console-core/src/rich-content.ts
var FILE_CHANGE_RE = /^(Created|Updated|Modified|Deleted)\b/i;
var TERMINAL_DURATION_RE = /^Worked for\s+.+$/i;
var TERMINAL_STATUS_RE = /^(Success|Running|Failed|Cancelled)$/i;
function escapeHtml(value) {
  return String(value || "").replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;").replace(/'/g, "&#39;");
}
function renderConversationInlineMarkdown(text) {
  const codeTokens = [];
  const escaped = escapeHtml(text || "").replace(/`([^`]+)`/g, (_match, code) => {
    const index = codeTokens.push(`<code class="cc-rich-inline-code">${code}</code>`) - 1;
    return `@@CODE_${index}@@`;
  }).replace(/\*\*([^*\n]+)\*\*/g, "<strong>$1</strong>").replace(/(^|[^A-Za-z0-9_*])\*([^*\n]+)\*(?![A-Za-z0-9_*])/g, "$1<em>$2</em>").replace(/(^|[^A-Za-z0-9_])_([^_\n]+)_(?![A-Za-z0-9_])/g, "$1<em>$2</em>").replace(/\[([^\]]+)\]\(([^)]+)\)/g, '<a href="$2">$1</a>').replace(/\n/g, "<br />");
  return escaped.replace(/@@CODE_(\d+)@@/g, (_match, index) => codeTokens[Number(index)] || "");
}
function conversationRichBlockCopyText(block) {
  switch (block.type) {
    case "code":
      return block.body.trim();
    case "command":
      return [block.title, block.body, block.output || "", block.footer || ""].filter(Boolean).join("\n").trim();
    case "file-change":
      return [
        block.verb,
        block.before || "",
        block.name,
        block.after || "",
        `+${block.plus}`,
        `-${block.minus}`
      ].filter(Boolean).join(" ").replace(/\s+/g, " ").trim();
    case "table":
      return [
        block.headers.join(" | "),
        ...block.rows.map((row) => row.join(" | "))
      ].join("\n").trim();
    case "heading":
      return block.text.trim();
    case "paragraph":
    case "divider":
      return block.text.trim();
    case "thinking":
      return [block.label, block.text].filter(Boolean).join("\n").trim();
    case "image":
      return [block.alt || "image", block.blobId || block.src].filter(Boolean).join(" ").trim();
    case "tool-call": {
      if (block.peerTarget) {
        const dir = block.peerIncoming ? "\u2190 from" : "\u2192 to";
        return [`${dir} ${block.peerTarget}`, block.peerIntent, block.peerBody, block.result].filter(Boolean).join(": ").trim();
      }
      const parts = [`$ ${block.name}`];
      if (block.arguments) parts.push(`Input: ${block.arguments}`);
      if (block.result) parts.push(`Result: ${block.result}`);
      return parts.join("\n").trim();
    }
    default:
      return "";
  }
}
function conversationRichBlocksToText(blocks) {
  return (blocks || []).map((block) => conversationRichBlockCopyText(block)).filter(Boolean).join("\n\n").trim();
}
function tryParseJson(source) {
  const trimmed = source.trim();
  if (trimmed.length < 2) return null;
  const first = trimmed[0];
  const last = trimmed[trimmed.length - 1];
  const looksObj = first === "{" && last === "}";
  const looksArr = first === "[" && last === "]";
  if (!looksObj && !looksArr) return null;
  try {
    return JSON.parse(trimmed);
  } catch {
    return null;
  }
}
function parseConversationRichBlocks(content) {
  const source = String(content || "").trim();
  if (!source) {
    return [];
  }
  const wholeJson = tryParseJson(source);
  if (wholeJson !== null) {
    return [{
      type: "code",
      language: "json",
      body: JSON.stringify(wholeJson, null, 2)
    }];
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
    const sectionJson = tryParseJson(section);
    if (sectionJson !== null) {
      blocks.push({
        type: "code",
        language: "json",
        body: JSON.stringify(sectionJson, null, 2)
      });
      continue;
    }
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

// ../packages/console-core/src/conversation.ts
function conversationIdentityPresentation(identity) {
  if (identity?.presentation) {
    return identity.presentation;
  }
  if (identity?.role === "user") {
    return "user";
  }
  if (identity?.role === "system") {
    return "system";
  }
  if (identity?.role === "other") {
    return "participant";
  }
  return "assistant";
}
function conversationIdentityShowsLabel(identity) {
  if (!identity?.label) {
    return false;
  }
  if (typeof identity.showLabel === "boolean") {
    return identity.showLabel;
  }
  const presentation = conversationIdentityPresentation(identity);
  return presentation === "participant" || presentation === "system";
}
function conversationIdentityGroupKey(identity) {
  if (!identity) {
    return "unknown:assistant:hidden";
  }
  return [
    identity.id || "unknown",
    conversationIdentityPresentation(identity),
    conversationIdentityShowsLabel(identity) ? "label" : "hidden"
  ].join(":");
}
function conversationEntryText(entry) {
  if (entry.kind === "summary") {
    const fileLines = entry.files.map((file) => `${file.name} +${file.plus} -${file.minus}`).join("\n");
    return [entry.title, fileLines].filter(Boolean).join("\n");
  }
  return String(entry.copyText || entry.text || conversationRichBlocksToText(entry.blocks)).trim();
}
function groupConversationTimelineEntries(entries) {
  const groups = [];
  for (const entry of entries) {
    const current = groups.at(-1);
    if (!current || conversationIdentityGroupKey(current.identity) !== conversationIdentityGroupKey(entry.identity)) {
      groups.push({
        id: `${entry.identity.id}-${entry.id}`,
        identity: entry.identity,
        entries: [entry],
        copyText: conversationEntryText(entry)
      });
      continue;
    }
    current.entries.push(entry);
    const nextCopyText = conversationEntryText(entry);
    current.copyText = [current.copyText, nextCopyText].filter(Boolean).join("\n\n");
  }
  return groups;
}

// ../packages/console-core/src/dock.ts
var CONSOLE_DOCK_PRESETS = [
  {
    id: "single",
    label: "Single",
    description: "One focused panel.",
    iconName: "i-compose"
  },
  {
    id: "two_columns",
    label: "Two columns",
    description: "Side-by-side work.",
    iconName: "i-sidebar-toggle"
  },
  {
    id: "two_rows",
    label: "Two rows",
    description: "Top and bottom pairing.",
    iconName: "i-swap"
  },
  {
    id: "grid",
    label: "Grid",
    description: "A 2x2 comparison layout.",
    iconName: "i-team"
  }
];
function isDockPanelNode(node) {
  return Boolean(node && node.kind === "panel" && node.panelId);
}
function isDockSplitNode(node) {
  return Boolean(
    node && node.kind === "split" && node.id && (node.direction === "horizontal" || node.direction === "vertical") && node.first && node.second
  );
}
function normalizeTarget(target) {
  if (!target?.id || !target?.kind || !target?.title) {
    return null;
  }
  return target;
}
function normalizePanelState(panel) {
  if (!panel?.id) {
    return null;
  }
  return {
    id: panel.id,
    target: normalizeTarget(panel.target),
    mode: panel.mode === "terminal" ? "terminal" : "console"
  };
}
function normalizeNode(node, validPanelIds) {
  if (isDockPanelNode(node)) {
    return validPanelIds.has(node.panelId) ? { kind: "panel", panelId: node.panelId } : null;
  }
  if (!isDockSplitNode(node)) {
    return null;
  }
  const first = normalizeNode(node.first, validPanelIds);
  const second = normalizeNode(node.second, validPanelIds);
  if (first && second) {
    return {
      kind: "split",
      id: node.id,
      direction: node.direction,
      ratio: typeof node.ratio === "number" && node.ratio > 0 && node.ratio < 1 ? node.ratio : 0.5,
      first,
      second
    };
  }
  return first || second;
}
function panelNode(panelId) {
  return { kind: "panel", panelId };
}
function presetMeta(presetId) {
  return CONSOLE_DOCK_PRESETS.find((entry) => entry.id === presetId) || CONSOLE_DOCK_PRESETS[0];
}
function uniqueTargets(values, excludedIds) {
  const usedIds = new Set(excludedIds);
  const results = [];
  for (const target of values) {
    if (!target) {
      results.push(null);
      continue;
    }
    if (usedIds.has(target.id)) {
      continue;
    }
    usedIds.add(target.id);
    results.push(target);
  }
  return results;
}
function suggestDockTargets({
  count,
  preferred = null,
  excludedIds = [],
  suggestTargets
}) {
  const suggested = uniqueTargets(
    suggestTargets?.({ count, preferred: preferred || null, excludedIds }) || [],
    excludedIds
  );
  const results = [];
  const usedIds = new Set(excludedIds);
  for (const target of suggested) {
    if (!target) {
      results.push(null);
      continue;
    }
    if (usedIds.has(target.id)) {
      continue;
    }
    usedIds.add(target.id);
    results.push(target);
    if (results.length >= count) {
      return results;
    }
  }
  while (results.length < count) {
    if (preferred && !usedIds.has(preferred.id)) {
      usedIds.add(preferred.id);
      results.push(preferred);
    } else {
      results.push(null);
    }
  }
  return results;
}
function replacePanelStates(panels, nextPanels) {
  const nextById = new Map(nextPanels.map((panel) => [panel.id, panel]));
  const filtered = panels.filter((panel) => !nextById.has(panel.id));
  return [...filtered, ...nextPanels];
}
function consoleDockPresets() {
  return CONSOLE_DOCK_PRESETS;
}
function collectConsoleDockPanelIds(node) {
  if (isDockPanelNode(node)) {
    return [node.panelId];
  }
  if (!isDockSplitNode(node)) {
    return [];
  }
  return [
    ...collectConsoleDockPanelIds(node.first),
    ...collectConsoleDockPanelIds(node.second)
  ];
}
function findConsoleDockFirstPanelId(node) {
  if (isDockPanelNode(node)) {
    return node.panelId;
  }
  if (!isDockSplitNode(node)) {
    return null;
  }
  return findConsoleDockFirstPanelId(node.first) || findConsoleDockFirstPanelId(node.second);
}
function replaceConsoleDockPanelNode(node, panelId, replacement) {
  if (node.kind === "panel") {
    return node.panelId === panelId ? replacement : node;
  }
  return {
    ...node,
    first: replaceConsoleDockPanelNode(node.first, panelId, replacement),
    second: replaceConsoleDockPanelNode(node.second, panelId, replacement)
  };
}
function removeConsoleDockPanelNode(node, panelId) {
  if (!node) {
    return null;
  }
  if (node.kind === "panel") {
    return node.panelId === panelId ? null : node;
  }
  const nextFirst = removeConsoleDockPanelNode(node.first, panelId);
  const nextSecond = removeConsoleDockPanelNode(node.second, panelId);
  if (nextFirst && nextSecond) {
    return {
      ...node,
      first: nextFirst,
      second: nextSecond
    };
  }
  return nextFirst || nextSecond;
}
function clampConsoleDockSplitRatio(ratio) {
  if (typeof ratio !== "number" || Number.isNaN(ratio)) {
    return 0.5;
  }
  return Math.min(0.88, Math.max(0.12, ratio));
}
function updateConsoleDockSplitRatio(node, splitId, ratio) {
  if (node.kind === "panel") {
    return node;
  }
  if (node.id === splitId) {
    return {
      ...node,
      ratio: clampConsoleDockSplitRatio(ratio)
    };
  }
  return {
    ...node,
    first: updateConsoleDockSplitRatio(node.first, splitId, ratio),
    second: updateConsoleDockSplitRatio(node.second, splitId, ratio)
  };
}
function consoleDockSplitDirectionAxis(direction) {
  return direction === "left" || direction === "right" ? "horizontal" : "vertical";
}
function consoleDockSplitDirectionPrecedes(direction) {
  return direction === "left" || direction === "up";
}
function normalizeConsoleDockState(state) {
  const panels = (state?.panels || []).map((panel) => normalizePanelState(panel)).filter(Boolean);
  const validPanelIds = new Set(panels.map((panel) => panel.id));
  const tabs = (state?.tabs || []).filter((tab) => Boolean(tab?.id)).map((tab) => ({
    id: tab.id,
    presetId: tab.presetId || "single",
    layout: normalizeNode(tab.layout, validPanelIds)
  })).filter((tab) => Boolean(tab.layout));
  const activeTabId = tabs.some((tab) => tab.id === state?.activeTabId) ? state?.activeTabId || null : tabs[0]?.id || null;
  const activeTab = tabs.find((tab) => tab.id === activeTabId) || null;
  const activePanelIds = activeTab ? collectConsoleDockPanelIds(activeTab.layout) : [];
  const focusedPanelId = state?.focusedPanelId && activePanelIds.includes(state.focusedPanelId) ? state.focusedPanelId : activePanelIds[0] || null;
  return {
    tabs,
    panels,
    activeTabId,
    focusedPanelId
  };
}
function buildConsoleDockPresetState({
  presetId,
  preferredTarget = null,
  preferredPanel = null,
  createPanelState,
  createSplitId,
  suggestTargets
}) {
  const requestedCount = presetId === "grid" ? 4 : presetId === "single" ? 1 : 2;
  const [firstTarget, secondTarget, thirdTarget, fourthTarget] = suggestDockTargets({
    count: requestedCount,
    preferred: preferredTarget,
    excludedIds: [],
    suggestTargets
  });
  const primary = createPanelState({
    target: preferredPanel ? preferredTarget ?? preferredPanel.target : firstTarget || null,
    sourcePanel: preferredPanel || null
  });
  if (presetId === "single") {
    return {
      presetId,
      layout: panelNode(primary.id),
      panels: [primary],
      focusedPanelId: primary.id
    };
  }
  if (presetId === "two_columns") {
    const right = createPanelState({ target: secondTarget || null, sourcePanel: preferredPanel || primary });
    return {
      presetId,
      layout: {
        kind: "split",
        id: createSplitId(),
        direction: "horizontal",
        ratio: 0.5,
        first: panelNode(primary.id),
        second: panelNode(right.id)
      },
      panels: [primary, right],
      focusedPanelId: primary.id
    };
  }
  if (presetId === "two_rows") {
    const bottom = createPanelState({ target: secondTarget || null, sourcePanel: preferredPanel || primary });
    return {
      presetId,
      layout: {
        kind: "split",
        id: createSplitId(),
        direction: "vertical",
        ratio: 0.5,
        first: panelNode(primary.id),
        second: panelNode(bottom.id)
      },
      panels: [primary, bottom],
      focusedPanelId: primary.id
    };
  }
  const rightTop = createPanelState({ target: secondTarget || null, sourcePanel: preferredPanel || primary });
  const leftBottom = createPanelState({ target: thirdTarget || null, sourcePanel: preferredPanel || primary });
  const rightBottom = createPanelState({ target: fourthTarget || null, sourcePanel: preferredPanel || primary });
  return {
    presetId,
    layout: {
      kind: "split",
      id: createSplitId(),
      direction: "horizontal",
      ratio: 0.5,
      first: {
        kind: "split",
        id: createSplitId(),
        direction: "vertical",
        ratio: 0.5,
        first: panelNode(primary.id),
        second: panelNode(leftBottom.id)
      },
      second: {
        kind: "split",
        id: createSplitId(),
        direction: "vertical",
        ratio: 0.5,
        first: panelNode(rightTop.id),
        second: panelNode(rightBottom.id)
      }
    },
    panels: [primary, rightTop, leftBottom, rightBottom],
    focusedPanelId: primary.id
  };
}
function createConsoleDockState({
  initialTarget = null,
  initialPresetId = "single",
  createPanelState,
  createTabId,
  createSplitId,
  suggestTargets
}) {
  const initial = buildConsoleDockPresetState({
    presetId: initialPresetId,
    preferredTarget: initialTarget,
    createPanelState,
    createSplitId,
    suggestTargets
  });
  const firstTabId = createTabId();
  return {
    tabs: [{
      id: firstTabId,
      presetId: initialPresetId,
      layout: initial.layout
    }],
    panels: initial.panels,
    activeTabId: firstTabId,
    focusedPanelId: initial.focusedPanelId
  };
}
function selectConsoleDockTab(state, tabId) {
  const normalized = normalizeConsoleDockState(state);
  const tab = normalized.tabs.find((entry) => entry.id === tabId) || null;
  const focusedPanelId = tab ? findConsoleDockFirstPanelId(tab.layout) : normalized.focusedPanelId;
  return {
    ...normalized,
    activeTabId: tab ? tab.id : normalized.activeTabId,
    focusedPanelId
  };
}
function focusConsoleDockPanel(state, panelId) {
  const normalized = normalizeConsoleDockState(state);
  return normalized.panels.some((panel) => panel.id === panelId) ? {
    ...normalized,
    focusedPanelId: panelId
  } : normalized;
}
function setConsoleDockPanelTarget(state, panelId, target) {
  const normalized = normalizeConsoleDockState(state);
  return {
    ...normalized,
    panels: normalized.panels.map((panel) => panel.id === panelId ? {
      ...panel,
      target: normalizeTarget(target)
    } : panel)
  };
}
function setConsoleDockPanelMode(state, panelId, mode) {
  const normalized = normalizeConsoleDockState(state);
  return {
    ...normalized,
    panels: normalized.panels.map((panel) => panel.id === panelId ? {
      ...panel,
      mode
    } : panel)
  };
}
function createConsoleDockTab(state, options) {
  const normalized = normalizeConsoleDockState(state);
  const preferredPanel = normalized.focusedPanelId ? normalized.panels.find((panel) => panel.id === normalized.focusedPanelId) || null : null;
  const presetState = buildConsoleDockPresetState({
    presetId: "single",
    preferredTarget: preferredPanel?.target || null,
    preferredPanel,
    createPanelState: options.createPanelState,
    createSplitId: options.createSplitId,
    suggestTargets: options.suggestTargets
  });
  const tabId = options.createTabId();
  return {
    ...normalized,
    tabs: [
      ...normalized.tabs,
      {
        id: tabId,
        presetId: "single",
        layout: presetState.layout
      }
    ],
    panels: replacePanelStates(normalized.panels, presetState.panels),
    activeTabId: tabId,
    focusedPanelId: presetState.focusedPanelId
  };
}
function closeConsoleDockTab(state, tabId, options) {
  const normalized = normalizeConsoleDockState(state);
  const closingIndex = normalized.tabs.findIndex((tab) => tab.id === tabId);
  if (closingIndex < 0) {
    return normalized;
  }
  if (normalized.tabs.length <= 1) {
    return createConsoleDockState({
      initialPresetId: "single",
      createPanelState: options.createPanelState,
      createSplitId: options.createSplitId,
      createTabId: () => normalized.tabs[0]?.id || options.createTabId(),
      suggestTargets: options.suggestTargets
    });
  }
  const closingTab = normalized.tabs[closingIndex];
  const removePanelIds = new Set(collectConsoleDockPanelIds(closingTab.layout));
  const nextTabs = normalized.tabs.filter((tab) => tab.id !== tabId);
  const nextActiveTabId = normalized.activeTabId === tabId ? nextTabs[Math.max(0, closingIndex - 1)]?.id || nextTabs[0]?.id || null : normalized.activeTabId;
  const nextState = {
    tabs: nextTabs,
    panels: normalized.panels.filter((panel) => !removePanelIds.has(panel.id)),
    activeTabId: nextActiveTabId,
    focusedPanelId: normalized.focusedPanelId
  };
  return normalizeConsoleDockState(nextState);
}
function openConsoleDockTarget(state, target, options) {
  const intent = options.intent || "replace_focused";
  const normalized = normalizeConsoleDockState(state);
  if (intent === "new_tab") {
    const presetState = buildConsoleDockPresetState({
      presetId: "single",
      preferredTarget: target,
      createPanelState: options.createPanelState,
      createSplitId: options.createSplitId,
      suggestTargets: options.suggestTargets
    });
    const tabId = options.createTabId();
    return {
      ...normalized,
      tabs: [
        ...normalized.tabs,
        {
          id: tabId,
          presetId: "single",
          layout: presetState.layout
        }
      ],
      panels: replacePanelStates(normalized.panels, presetState.panels),
      activeTabId: tabId,
      focusedPanelId: presetState.focusedPanelId
    };
  }
  if (intent === "split_right" || intent === "split_down") {
    const activeTab = normalized.tabs.find((tab) => tab.id === normalized.activeTabId) || null;
    const focusedPanel = normalized.focusedPanelId ? normalized.panels.find((panel) => panel.id === normalized.focusedPanelId) || null : null;
    if (!activeTab || !focusedPanel) {
      return normalized;
    }
    const direction = intent === "split_right" ? "right" : "down";
    const nextPanel = options.createPanelState({
      target,
      sourcePanel: focusedPanel
    });
    const replacement = {
      kind: "split",
      id: options.createSplitId(),
      direction: consoleDockSplitDirectionAxis(direction),
      ratio: 0.5,
      first: panelNode(focusedPanel.id),
      second: panelNode(nextPanel.id)
    };
    return {
      ...normalized,
      tabs: normalized.tabs.map((tab) => tab.id === activeTab.id ? {
        ...tab,
        layout: replaceConsoleDockPanelNode(tab.layout, focusedPanel.id, replacement)
      } : tab),
      panels: replacePanelStates(normalized.panels, [nextPanel]),
      focusedPanelId: nextPanel.id
    };
  }
  if (!normalized.focusedPanelId) {
    return normalized;
  }
  return setConsoleDockPanelTarget(normalized, normalized.focusedPanelId, target);
}
function resizeConsoleDockSplit(state, splitId, ratio) {
  const normalized = normalizeConsoleDockState(state);
  const activeTab = normalized.tabs.find((tab) => tab.id === normalized.activeTabId) || null;
  if (!activeTab) {
    return normalized;
  }
  return {
    ...normalized,
    tabs: normalized.tabs.map((tab) => tab.id === activeTab.id ? {
      ...tab,
      layout: updateConsoleDockSplitRatio(tab.layout, splitId, ratio)
    } : tab)
  };
}
function splitConsoleDockPanel(state, panelId, direction, options) {
  const normalized = normalizeConsoleDockState(state);
  const activeTab = normalized.tabs.find((tab) => tab.id === normalized.activeTabId) || null;
  const panel = normalized.panels.find((entry) => entry.id === panelId) || null;
  if (!activeTab || !panel) {
    return normalized;
  }
  const excludedIds = collectConsoleDockPanelIds(activeTab.layout).map((id) => normalized.panels.find((entry) => entry.id === id)?.target?.id || "").filter(Boolean);
  const suggestedTarget = suggestDockTargets({
    count: 1,
    preferred: panel.target,
    excludedIds,
    suggestTargets: options.suggestTargets
  })[0] || panel.target || null;
  const nextPanel = options.createPanelState({
    target: suggestedTarget,
    sourcePanel: panel
  });
  const replacement = {
    kind: "split",
    id: options.createSplitId(),
    direction: consoleDockSplitDirectionAxis(direction),
    ratio: 0.5,
    first: consoleDockSplitDirectionPrecedes(direction) ? panelNode(nextPanel.id) : panelNode(panelId),
    second: consoleDockSplitDirectionPrecedes(direction) ? panelNode(panelId) : panelNode(nextPanel.id)
  };
  return {
    ...normalized,
    tabs: normalized.tabs.map((tab) => tab.id === activeTab.id ? {
      ...tab,
      layout: replaceConsoleDockPanelNode(tab.layout, panelId, replacement)
    } : tab),
    panels: replacePanelStates(normalized.panels, [nextPanel]),
    focusedPanelId: nextPanel.id
  };
}
function closeConsoleDockPanel(state, panelId) {
  const normalized = normalizeConsoleDockState(state);
  const activeTab = normalized.tabs.find((tab) => tab.id === normalized.activeTabId) || null;
  const panel = normalized.panels.find((entry) => entry.id === panelId) || null;
  if (!activeTab || !panel) {
    return normalized;
  }
  if (collectConsoleDockPanelIds(activeTab.layout).length <= 1) {
    return {
      ...normalized,
      panels: normalized.panels.map((entry) => entry.id === panelId ? {
        ...entry,
        target: null
      } : entry),
      focusedPanelId: panelId
    };
  }
  const nextLayout = removeConsoleDockPanelNode(activeTab.layout, panelId);
  if (!nextLayout) {
    return normalized;
  }
  return {
    ...normalized,
    tabs: normalized.tabs.map((tab) => tab.id === activeTab.id ? {
      ...tab,
      layout: nextLayout
    } : tab),
    panels: normalized.panels.filter((entry) => entry.id !== panelId),
    focusedPanelId: findConsoleDockFirstPanelId(nextLayout)
  };
}
function applyConsoleDockPreset(state, options) {
  const normalized = normalizeConsoleDockState(state);
  const activeTab = normalized.tabs.find((tab) => tab.id === normalized.activeTabId) || null;
  const focusedPanel = normalized.focusedPanelId ? normalized.panels.find((panel) => panel.id === normalized.focusedPanelId) || null : null;
  if (!activeTab) {
    return normalized;
  }
  const presetState = buildConsoleDockPresetState({
    presetId: options.presetId,
    preferredTarget: focusedPanel?.target || null,
    preferredPanel: focusedPanel,
    createPanelState: options.createPanelState,
    createSplitId: options.createSplitId,
    suggestTargets: options.suggestTargets
  });
  const currentPanelIds = new Set(collectConsoleDockPanelIds(activeTab.layout));
  return {
    ...normalized,
    tabs: normalized.tabs.map((tab) => tab.id === activeTab.id ? {
      ...tab,
      presetId: options.presetId,
      layout: presetState.layout
    } : tab),
    panels: replacePanelStates(
      normalized.panels.filter((panel) => !currentPanelIds.has(panel.id)),
      presetState.panels
    ),
    focusedPanelId: presetState.focusedPanelId
  };
}
function applyConsoleDockAction(state, action, options) {
  switch (action.type) {
    case "create_tab":
      return createConsoleDockTab(state, options);
    case "select_tab":
      return action.tabId ? selectConsoleDockTab(state, action.tabId) : state;
    case "close_tab":
      return action.tabId ? closeConsoleDockTab(state, action.tabId, options) : state;
    case "focus_panel":
      return action.panelId ? focusConsoleDockPanel(state, action.panelId) : state;
    case "set_panel_target":
      return action.panelId ? setConsoleDockPanelTarget(state, action.panelId, action.target || null) : state;
    case "set_panel_mode":
      return action.panelId && action.mode ? setConsoleDockPanelMode(state, action.panelId, action.mode) : state;
    case "open_target":
      return action.target ? openConsoleDockTarget(state, action.target, {
        ...options,
        intent: action.intent
      }) : state;
    case "resize_split":
      return action.splitId && typeof action.ratio === "number" ? resizeConsoleDockSplit(state, action.splitId, action.ratio) : state;
    case "split_panel":
      return action.panelId && action.direction ? splitConsoleDockPanel(state, action.panelId, action.direction, options) : state;
    case "close_panel":
      return action.panelId ? closeConsoleDockPanel(state, action.panelId) : state;
    case "apply_preset":
      return action.presetId ? applyConsoleDockPreset(state, {
        presetId: action.presetId,
        createPanelState: options.createPanelState,
        createSplitId: options.createSplitId,
        suggestTargets: options.suggestTargets
      }) : state;
    default:
      return state;
  }
}
function buildConsoleDockViewState(state, options = {}) {
  const normalized = normalizeConsoleDockState(state);
  const panelsById = new Map(normalized.panels.map((panel) => [panel.id, panel]));
  return {
    activeTabId: normalized.activeTabId,
    focusedPanelId: normalized.focusedPanelId,
    tabs: normalized.tabs.map((tab) => {
      const panelStates = collectConsoleDockPanelIds(tab.layout).map((panelId) => panelsById.get(panelId)).filter(Boolean);
      const firstTarget = panelStates.find((panel) => panel.target)?.target || null;
      const preset = presetMeta(tab.presetId);
      const resolved = options.resolveTabView?.({
        tab,
        panels: panelStates,
        active: tab.id === normalized.activeTabId,
        focusedPanelId: normalized.focusedPanelId
      }) || {};
      return {
        id: tab.id,
        title: resolved.title || firstTarget?.title || preset.label,
        subtitle: resolved.subtitle ?? firstTarget?.subtitle ?? preset.description,
        iconName: resolved.iconName ?? firstTarget?.iconName ?? preset.iconName,
        badgeLabel: resolved.badgeLabel ?? (panelStates.length > 1 ? `x${panelStates.length}` : null),
        closable: resolved.closable ?? true,
        dirty: resolved.dirty ?? false,
        layout: tab.layout
      };
    }),
    panels: normalized.tabs.flatMap((tab) => {
      const activePanelIds = collectConsoleDockPanelIds(tab.layout);
      const activePanelCount = activePanelIds.length;
      return activePanelIds.flatMap((panelId) => {
        const panel = panelsById.get(panelId);
        if (!panel) {
          return [];
        }
        const resolved = options.resolvePanelView?.({
          panel,
          activePanelCount,
          focused: normalized.focusedPanelId === panel.id
        }) || {};
        return [{
          id: panel.id,
          title: resolved.title || panel.target?.title || "Open something",
          subtitle: resolved.subtitle ?? panel.target?.subtitle ?? "Use the launcher or activity rail to open a target.",
          iconName: resolved.iconName ?? panel.target?.iconName ?? "i-compose",
          target: panel.target,
          mode: panel.mode,
          statusLabel: resolved.statusLabel ?? (panel.target ? "Active target" : "Ready"),
          badgeLabel: resolved.badgeLabel ?? panel.target?.badgeLabel ?? null,
          dirty: resolved.dirty ?? false,
          closable: resolved.closable ?? activePanelCount > 1
        }];
      });
    })
  };
}

// ../packages/console-core/src/format.ts
function formatCount(value) {
  return new Intl.NumberFormat("en-US").format(Number(value) || 0);
}

// ../packages/console-components/src/conversation/conversation-message-view.tsx
var import_react3 = require("react");

// ../packages/console-components/src/conversation/conversation-rich-content.tsx
var import_react2 = require("react");

// ../packages/console-components/src/conversation/change-stat-pair.tsx
var import_jsx_runtime3 = require("react/jsx-runtime");
function ChangeStatPair({
  plus,
  minus,
  className
}) {
  return /* @__PURE__ */ (0, import_jsx_runtime3.jsxs)("span", { className: clsx_default("cc-change-stat", className), children: [
    /* @__PURE__ */ (0, import_jsx_runtime3.jsxs)("span", { className: "cc-change-stat__value is-plus", children: [
      "+",
      formatCount(plus)
    ] }),
    /* @__PURE__ */ (0, import_jsx_runtime3.jsxs)("span", { className: "cc-change-stat__value is-minus", children: [
      "-",
      formatCount(minus)
    ] })
  ] });
}

// ../packages/console-components/src/copy-button.tsx
var import_react = require("react");
var import_jsx_runtime4 = require("react/jsx-runtime");
function CopyButton({
  text,
  label,
  copiedLabel = "Copied",
  className,
  Icon: Icon2
}) {
  const [copied, setCopied] = (0, import_react.useState)(false);
  const resetTimerRef = (0, import_react.useRef)(null);
  const disabled = !text.trim();
  (0, import_react.useEffect)(() => () => {
    if (resetTimerRef.current != null) {
      window.clearTimeout(resetTimerRef.current);
    }
  }, []);
  async function handleClick() {
    if (disabled) {
      return;
    }
    const wasCopied = await copyTextToClipboard(text);
    if (!wasCopied) {
      return;
    }
    setCopied(true);
    if (resetTimerRef.current != null) {
      window.clearTimeout(resetTimerRef.current);
    }
    resetTimerRef.current = window.setTimeout(() => {
      setCopied(false);
      resetTimerRef.current = null;
    }, 1600);
  }
  return /* @__PURE__ */ (0, import_jsx_runtime4.jsx)(
    "button",
    {
      className: clsx_default("cc-copy-btn", className),
      type: "button",
      "aria-label": copied ? copiedLabel : label,
      title: copied ? copiedLabel : label,
      "data-copied": copied ? "true" : void 0,
      disabled,
      onClick: () => {
        void handleClick();
      },
      children: Icon2 ? /* @__PURE__ */ (0, import_jsx_runtime4.jsx)(Icon2, { name: copied ? "i-check" : "i-copy" }) : copied ? "Copied" : "Copy"
    }
  );
}

// ../packages/console-components/src/conversation/conversation-rich-content.tsx
var import_jsx_runtime5 = require("react/jsx-runtime");
function markdownHtml(text) {
  return { __html: renderConversationInlineMarkdown(text) };
}
function commandCopyText(block) {
  return [block.title, block.body, block.output || "", block.footer || ""].filter(Boolean).join("\n").trim();
}
function fileChangeCopyText(block) {
  return [
    block.verb,
    block.before || "",
    block.name,
    block.after || "",
    `+${block.plus}`,
    `-${block.minus}`
  ].filter(Boolean).join(" ").replace(/\s+/g, " ").trim();
}
function alignmentAttr(alignment) {
  return alignment || "left";
}
function renderThinkingBlock(block) {
  if (!block.label?.trim() && !block.text?.trim()) {
    return null;
  }
  return /* @__PURE__ */ (0, import_jsx_runtime5.jsxs)(
    "div",
    {
      className: clsx_default(
        "cc-rich-thinking",
        block.final && "cc-rich-thinking--final",
        block.persisted && "cc-rich-thinking--persisted"
      ),
      children: [
        /* @__PURE__ */ (0, import_jsx_runtime5.jsx)("div", { className: "cc-rich-thinking__label", children: block.label }),
        /* @__PURE__ */ (0, import_jsx_runtime5.jsx)("p", { className: "cc-rich-paragraph", dangerouslySetInnerHTML: markdownHtml(block.text) })
      ]
    }
  );
}
function renderBlock(block, index, Icon2) {
  if (block.type === "paragraph") {
    return /* @__PURE__ */ (0, import_jsx_runtime5.jsx)("p", { className: "cc-rich-paragraph", dangerouslySetInnerHTML: markdownHtml(block.text) }, `paragraph-${index}`);
  }
  if (block.type === "heading") {
    return /* @__PURE__ */ (0, import_jsx_runtime5.jsx)(
      "h3",
      {
        className: `cc-rich-heading cc-rich-heading--${Number(block.level) || 2}`,
        dangerouslySetInnerHTML: markdownHtml(block.text)
      },
      `heading-${index}`
    );
  }
  if (block.type === "code") {
    const codeBlock = block;
    return /* @__PURE__ */ (0, import_jsx_runtime5.jsxs)("section", { className: "cc-rich-code-card", children: [
      /* @__PURE__ */ (0, import_jsx_runtime5.jsxs)("div", { className: "cc-rich-code-card__header", children: [
        /* @__PURE__ */ (0, import_jsx_runtime5.jsx)("span", { className: "cc-rich-code-language", children: codeBlock.language || "text" }),
        /* @__PURE__ */ (0, import_jsx_runtime5.jsx)(
          CopyButton,
          {
            copiedLabel: "Copied code",
            Icon: Icon2,
            label: "Copy code",
            text: codeBlock.body
          }
        )
      ] }),
      /* @__PURE__ */ (0, import_jsx_runtime5.jsx)("pre", { className: "cc-rich-code-body", children: codeBlock.highlightedHtml ? /* @__PURE__ */ (0, import_jsx_runtime5.jsx)(
        "code",
        {
          className: `cc-rich-code-content language-${codeBlock.language || "text"}`,
          dangerouslySetInnerHTML: { __html: codeBlock.highlightedHtml }
        }
      ) : /* @__PURE__ */ (0, import_jsx_runtime5.jsx)("code", { className: `cc-rich-code-content language-${codeBlock.language || "text"}`, children: codeBlock.body }) })
    ] }, `code-${index}`);
  }
  if (block.type === "table") {
    return /* @__PURE__ */ (0, import_jsx_runtime5.jsx)("div", { className: "cc-rich-table-wrap", children: /* @__PURE__ */ (0, import_jsx_runtime5.jsxs)("table", { className: "cc-rich-table", children: [
      /* @__PURE__ */ (0, import_jsx_runtime5.jsx)("thead", { children: /* @__PURE__ */ (0, import_jsx_runtime5.jsx)("tr", { children: block.headers.map((header, cellIndex) => /* @__PURE__ */ (0, import_jsx_runtime5.jsx)(
        "th",
        {
          "data-align": alignmentAttr(block.alignments[cellIndex]),
          dangerouslySetInnerHTML: markdownHtml(header)
        },
        `header-${cellIndex}`
      )) }) }),
      /* @__PURE__ */ (0, import_jsx_runtime5.jsx)("tbody", { children: block.rows.map((row, rowIndex) => /* @__PURE__ */ (0, import_jsx_runtime5.jsx)("tr", { children: block.headers.map((_header, cellIndex) => /* @__PURE__ */ (0, import_jsx_runtime5.jsx)(
        "td",
        {
          "data-align": alignmentAttr(block.alignments[cellIndex]),
          dangerouslySetInnerHTML: markdownHtml(row[cellIndex] || "")
        },
        `cell-${rowIndex}-${cellIndex}`
      )) }, `row-${rowIndex}`)) })
    ] }) }, `table-${index}`);
  }
  if (block.type === "command") {
    return /* @__PURE__ */ (0, import_jsx_runtime5.jsxs)("div", { className: "cc-rich-command-stack", children: [
      /* @__PURE__ */ (0, import_jsx_runtime5.jsx)("div", { className: "cc-rich-command-caption", children: block.caption }),
      /* @__PURE__ */ (0, import_jsx_runtime5.jsxs)("div", { className: "cc-rich-command-card", children: [
        /* @__PURE__ */ (0, import_jsx_runtime5.jsxs)("div", { className: "cc-rich-command-card__header", children: [
          /* @__PURE__ */ (0, import_jsx_runtime5.jsx)("div", { className: "cc-rich-command-card__title", children: block.title }),
          /* @__PURE__ */ (0, import_jsx_runtime5.jsx)(
            CopyButton,
            {
              copiedLabel: "Copied command output",
              Icon: Icon2,
              label: "Copy command output",
              text: commandCopyText(block)
            }
          )
        ] }),
        /* @__PURE__ */ (0, import_jsx_runtime5.jsx)("pre", { className: "cc-rich-command-card__body", children: block.body }),
        block.output ? /* @__PURE__ */ (0, import_jsx_runtime5.jsx)("pre", { className: "cc-rich-command-card__output", children: block.output }) : null,
        block.footer ? /* @__PURE__ */ (0, import_jsx_runtime5.jsx)("div", { className: "cc-rich-command-card__footer", children: block.footer }) : null
      ] })
    ] }, `command-${index}`);
  }
  if (block.type === "file-change") {
    return /* @__PURE__ */ (0, import_jsx_runtime5.jsxs)("section", { className: "cc-rich-file-change", children: [
      /* @__PURE__ */ (0, import_jsx_runtime5.jsxs)("div", { className: "cc-rich-file-change__main", children: [
        /* @__PURE__ */ (0, import_jsx_runtime5.jsx)("span", { className: "cc-rich-file-change__verb", children: block.verb }),
        block.before ? /* @__PURE__ */ (0, import_jsx_runtime5.jsx)("span", { className: "cc-rich-file-change__context", dangerouslySetInnerHTML: markdownHtml(block.before) }) : null,
        /* @__PURE__ */ (0, import_jsx_runtime5.jsx)("button", { className: "cc-rich-file-change__link", type: "button", children: block.name }),
        block.after ? /* @__PURE__ */ (0, import_jsx_runtime5.jsx)("span", { className: "cc-rich-file-change__context", dangerouslySetInnerHTML: markdownHtml(block.after) }) : null
      ] }),
      /* @__PURE__ */ (0, import_jsx_runtime5.jsxs)("div", { className: "cc-rich-file-change__stats", children: [
        /* @__PURE__ */ (0, import_jsx_runtime5.jsx)(ChangeStatPair, { minus: block.minus, plus: block.plus }),
        /* @__PURE__ */ (0, import_jsx_runtime5.jsx)("span", { className: "cc-rich-file-change__dot" }),
        /* @__PURE__ */ (0, import_jsx_runtime5.jsx)(
          CopyButton,
          {
            copiedLabel: "Copied file change",
            Icon: Icon2,
            label: "Copy file change",
            text: fileChangeCopyText(block)
          }
        )
      ] })
    ] }, `file-change-${index}`);
  }
  if (block.type === "divider") {
    return /* @__PURE__ */ (0, import_jsx_runtime5.jsxs)("div", { className: "cc-rich-divider", children: [
      /* @__PURE__ */ (0, import_jsx_runtime5.jsx)("span", { className: "cc-rich-divider__line" }),
      /* @__PURE__ */ (0, import_jsx_runtime5.jsx)("span", { className: "cc-rich-divider__label", children: block.text }),
      /* @__PURE__ */ (0, import_jsx_runtime5.jsx)("span", { className: "cc-rich-divider__line" })
    ] }, `divider-${index}`);
  }
  if (block.type === "image") {
    const image = block;
    return /* @__PURE__ */ (0, import_jsx_runtime5.jsx)(
      "button",
      {
        className: "cc-rich-image-button",
        onClick: () => window.open(image.src, "_blank", "noopener,noreferrer"),
        type: "button",
        children: /* @__PURE__ */ (0, import_jsx_runtime5.jsx)(
          "img",
          {
            alt: image.alt || "",
            className: "cc-rich-image",
            height: image.height,
            loading: "lazy",
            src: image.src,
            width: image.width
          }
        )
      },
      `image-${index}`
    );
  }
  if (block.type === "tool-call") {
    return /* @__PURE__ */ (0, import_jsx_runtime5.jsx)(ToolCallBlock, { block }, `tool-call-${index}`);
  }
  const thinking = renderThinkingBlock(block);
  if (!thinking) {
    return null;
  }
  return /* @__PURE__ */ (0, import_jsx_runtime5.jsx)("div", { children: thinking }, `thinking-${index}`);
}
var PEER_TOOL_NAMES = /* @__PURE__ */ new Set(["send_request", "send_message", "send_response"]);
function copyText(text) {
  navigator.clipboard?.writeText(text).catch(() => {
  });
}
function formatJsonIfPossible(text) {
  const trimmed = text.trim();
  if (!trimmed) return text;
  if (!(trimmed.startsWith("{") && trimmed.endsWith("}") || trimmed.startsWith("[") && trimmed.endsWith("]"))) {
    return text;
  }
  try {
    const parsed = JSON.parse(trimmed);
    return JSON.stringify(parsed, null, 2);
  } catch {
    return text;
  }
}
function toolBlockCopyText(block) {
  if (block.peerTarget) {
    const dir = block.peerIncoming ? "\u2190 from" : "\u2192 to";
    return [
      `${dir} ${block.peerTarget}`,
      block.peerIntent,
      block.peerBody,
      block.result
    ].filter(Boolean).join(": ").trim();
  }
  const parts = [`$ ${block.name}`];
  if (block.arguments) parts.push(`Input: ${block.arguments}`);
  if (block.result) parts.push(`Result: ${block.result}`);
  return parts.join("\n").trim();
}
function CopyBtn({ text, label = "Copy" }) {
  const [copied, setCopied] = (0, import_react2.useState)(false);
  return /* @__PURE__ */ (0, import_jsx_runtime5.jsx)(
    "button",
    {
      className: "cc-tool-call__copy",
      type: "button",
      title: label,
      onClick: (e) => {
        e.stopPropagation();
        copyText(text);
        setCopied(true);
        setTimeout(() => setCopied(false), 1500);
      },
      children: copied ? "\u2713" : "\u2398"
    }
  );
}
function ToolCallBlock({ block }) {
  const [expanded, setExpanded] = (0, import_react2.useState)(false);
  const isPeer = PEER_TOOL_NAMES.has(block.name);
  const statusIcon = block.status === "success" ? "\u2713" : block.status === "error" ? "\u2717" : "\u22EF";
  const statusClass = `cc-tool-call--${block.status}`;
  if (isPeer || block.peerIncoming) {
    const target = block.peerTarget || "peer";
    const content = block.peerBody || block.peerIntent || "";
    const arrow = block.peerIncoming ? "\u2199" : "\u2197";
    const inputDetail = block.arguments && block.arguments.trim() ? formatJsonIfPossible(block.arguments) : content;
    return /* @__PURE__ */ (0, import_jsx_runtime5.jsxs)("section", { className: clsx_default("cc-tool-call cc-tool-call--peer", block.peerIncoming && "cc-tool-call--incoming", statusClass), children: [
      /* @__PURE__ */ (0, import_jsx_runtime5.jsxs)(
        "button",
        {
          className: "cc-tool-call__header",
          type: "button",
          onClick: () => setExpanded((prev) => !prev),
          "aria-expanded": expanded,
          children: [
            /* @__PURE__ */ (0, import_jsx_runtime5.jsx)("span", { className: "cc-tool-call__chevron", children: expanded ? "\u25BE" : "\u25B8" }),
            /* @__PURE__ */ (0, import_jsx_runtime5.jsx)("span", { className: "cc-tool-call__icon", children: arrow }),
            /* @__PURE__ */ (0, import_jsx_runtime5.jsx)("span", { className: "cc-tool-call__name", children: block.peerIncoming ? `Received from ${target}` : target }),
            block.peerIntent && /* @__PURE__ */ (0, import_jsx_runtime5.jsx)("span", { className: "cc-tool-call__peer-intent", children: block.peerIntent }),
            content && /* @__PURE__ */ (0, import_jsx_runtime5.jsx)("span", { className: "cc-tool-call__preview", children: content }),
            /* @__PURE__ */ (0, import_jsx_runtime5.jsx)("span", { className: "cc-tool-call__status", children: statusIcon }),
            /* @__PURE__ */ (0, import_jsx_runtime5.jsx)(CopyBtn, { text: toolBlockCopyText(block) })
          ]
        }
      ),
      expanded && /* @__PURE__ */ (0, import_jsx_runtime5.jsxs)("div", { className: "cc-tool-call__body", children: [
        /* @__PURE__ */ (0, import_jsx_runtime5.jsxs)("div", { className: "cc-tool-call__section", children: [
          /* @__PURE__ */ (0, import_jsx_runtime5.jsx)("div", { className: "cc-tool-call__section-label", children: "Tool" }),
          /* @__PURE__ */ (0, import_jsx_runtime5.jsx)("pre", { className: "cc-tool-call__pre", children: block.name })
        ] }),
        block.peerIntent && /* @__PURE__ */ (0, import_jsx_runtime5.jsxs)("div", { className: "cc-tool-call__section", children: [
          /* @__PURE__ */ (0, import_jsx_runtime5.jsx)("div", { className: "cc-tool-call__section-label", children: "Intent" }),
          /* @__PURE__ */ (0, import_jsx_runtime5.jsx)("pre", { className: "cc-tool-call__pre", children: block.peerIntent })
        ] }),
        inputDetail && /* @__PURE__ */ (0, import_jsx_runtime5.jsxs)("div", { className: "cc-tool-call__section", children: [
          /* @__PURE__ */ (0, import_jsx_runtime5.jsx)("div", { className: "cc-tool-call__section-label", children: block.peerIncoming ? "Params" : "Input" }),
          /* @__PURE__ */ (0, import_jsx_runtime5.jsx)("pre", { className: "cc-tool-call__pre", children: inputDetail })
        ] }),
        block.result && /* @__PURE__ */ (0, import_jsx_runtime5.jsxs)("div", { className: "cc-tool-call__section", children: [
          /* @__PURE__ */ (0, import_jsx_runtime5.jsx)("div", { className: "cc-tool-call__section-label", children: "Result" }),
          /* @__PURE__ */ (0, import_jsx_runtime5.jsx)("pre", { className: "cc-tool-call__pre", children: formatJsonIfPossible(block.result) })
        ] })
      ] })
    ] });
  }
  let argsPreview = block.arguments || "";
  try {
    const parsed = JSON.parse(argsPreview);
    if (typeof parsed === "object" && parsed !== null) {
      argsPreview = Object.entries(parsed).map(([k, v]) => `${k}: ${typeof v === "string" ? v : JSON.stringify(v)}`).join(", ");
    }
  } catch {
  }
  return /* @__PURE__ */ (0, import_jsx_runtime5.jsxs)("section", { className: clsx_default("cc-tool-call", statusClass), children: [
    /* @__PURE__ */ (0, import_jsx_runtime5.jsxs)(
      "button",
      {
        className: "cc-tool-call__header",
        type: "button",
        onClick: () => setExpanded((prev) => !prev),
        "aria-expanded": expanded,
        children: [
          /* @__PURE__ */ (0, import_jsx_runtime5.jsx)("span", { className: "cc-tool-call__chevron", children: expanded ? "\u25BE" : "\u25B8" }),
          /* @__PURE__ */ (0, import_jsx_runtime5.jsx)("span", { className: "cc-tool-call__icon", children: "\u2699" }),
          /* @__PURE__ */ (0, import_jsx_runtime5.jsx)("span", { className: "cc-tool-call__name", children: block.name }),
          argsPreview && /* @__PURE__ */ (0, import_jsx_runtime5.jsx)("span", { className: "cc-tool-call__preview", children: argsPreview }),
          /* @__PURE__ */ (0, import_jsx_runtime5.jsxs)("span", { className: "cc-tool-call__status", children: [
            statusIcon,
            " ",
            block.status === "pending" ? "Running" : block.status === "success" ? "Success" : "Failed"
          ] }),
          /* @__PURE__ */ (0, import_jsx_runtime5.jsx)(CopyBtn, { text: toolBlockCopyText(block) })
        ]
      }
    ),
    expanded && /* @__PURE__ */ (0, import_jsx_runtime5.jsxs)("div", { className: "cc-tool-call__body", children: [
      argsPreview && /* @__PURE__ */ (0, import_jsx_runtime5.jsxs)("div", { className: "cc-tool-call__section", children: [
        /* @__PURE__ */ (0, import_jsx_runtime5.jsx)("div", { className: "cc-tool-call__section-label", children: "Input" }),
        /* @__PURE__ */ (0, import_jsx_runtime5.jsx)("pre", { className: "cc-tool-call__pre", children: argsPreview })
      ] }),
      block.result && /* @__PURE__ */ (0, import_jsx_runtime5.jsxs)("div", { className: "cc-tool-call__section", children: [
        /* @__PURE__ */ (0, import_jsx_runtime5.jsx)("div", { className: "cc-tool-call__section-label", children: "Result" }),
        /* @__PURE__ */ (0, import_jsx_runtime5.jsx)("pre", { className: "cc-tool-call__pre", children: block.result })
      ] })
    ] })
  ] });
}
function ToolCallGroup({ blocks }) {
  const [expanded, setExpanded] = (0, import_react2.useState)(false);
  const allSuccess = blocks.every((b) => b.status === "success");
  const anyError = blocks.some((b) => b.status === "error");
  const statusIcon = anyError ? "\u2717" : allSuccess ? "\u2713" : "\u22EF";
  const statusLabel = anyError ? "Failed" : allSuccess ? "Success" : "Running";
  const statusClass = anyError ? "cc-tool-call--error" : allSuccess ? "cc-tool-call--success" : "cc-tool-call--pending";
  const name = blocks[0]?.name || "tool";
  return /* @__PURE__ */ (0, import_jsx_runtime5.jsxs)("section", { className: clsx_default("cc-tool-call cc-tool-call--group", statusClass), children: [
    /* @__PURE__ */ (0, import_jsx_runtime5.jsxs)(
      "button",
      {
        className: "cc-tool-call__header",
        type: "button",
        onClick: () => setExpanded((prev) => !prev),
        "aria-expanded": expanded,
        children: [
          /* @__PURE__ */ (0, import_jsx_runtime5.jsx)("span", { className: "cc-tool-call__chevron", children: expanded ? "\u25BE" : "\u25B8" }),
          /* @__PURE__ */ (0, import_jsx_runtime5.jsx)("span", { className: "cc-tool-call__icon", children: "\u2699" }),
          /* @__PURE__ */ (0, import_jsx_runtime5.jsx)("span", { className: "cc-tool-call__name", children: name }),
          /* @__PURE__ */ (0, import_jsx_runtime5.jsxs)("span", { className: "cc-tool-call__count", children: [
            "\xD7",
            blocks.length
          ] }),
          /* @__PURE__ */ (0, import_jsx_runtime5.jsxs)("span", { className: "cc-tool-call__status", children: [
            statusIcon,
            " ",
            statusLabel
          ] }),
          /* @__PURE__ */ (0, import_jsx_runtime5.jsx)(CopyBtn, { text: blocks.map((b) => toolBlockCopyText(b)).join("\n") })
        ]
      }
    ),
    expanded && /* @__PURE__ */ (0, import_jsx_runtime5.jsx)("div", { className: "cc-tool-call__body", children: blocks.map((block, i) => {
      const args = block.arguments ? formatJsonIfPossible(block.arguments) : "";
      const result = block.result ? formatJsonIfPossible(block.result) : "";
      return /* @__PURE__ */ (0, import_jsx_runtime5.jsxs)("div", { className: "cc-tool-call__sub", children: [
        /* @__PURE__ */ (0, import_jsx_runtime5.jsxs)("div", { className: "cc-tool-call__sub-head", children: [
          /* @__PURE__ */ (0, import_jsx_runtime5.jsxs)("span", { className: "cc-tool-call__sub-index", children: [
            "#",
            i + 1
          ] }),
          /* @__PURE__ */ (0, import_jsx_runtime5.jsx)("span", { className: `cc-tool-call__peer-status cc-tool-call__peer-status--${block.status}`, children: block.status === "success" ? "\u2713" : block.status === "error" ? "\u2717" : "\u22EF" })
        ] }),
        args && /* @__PURE__ */ (0, import_jsx_runtime5.jsxs)("div", { className: "cc-tool-call__section", children: [
          /* @__PURE__ */ (0, import_jsx_runtime5.jsx)("div", { className: "cc-tool-call__section-label", children: "Input" }),
          /* @__PURE__ */ (0, import_jsx_runtime5.jsx)("pre", { className: "cc-tool-call__pre", children: args })
        ] }),
        result && /* @__PURE__ */ (0, import_jsx_runtime5.jsxs)("div", { className: "cc-tool-call__section", children: [
          /* @__PURE__ */ (0, import_jsx_runtime5.jsx)("div", { className: "cc-tool-call__section-label", children: "Result" }),
          /* @__PURE__ */ (0, import_jsx_runtime5.jsx)("pre", { className: "cc-tool-call__pre", children: result })
        ] })
      ] }, block.toolCallId || i);
    }) })
  ] });
}
function PeerToolGroup({ blocks }) {
  const [expanded, setExpanded] = (0, import_react2.useState)(false);
  const targets = blocks.map((b) => b.peerTarget || "peer");
  const allSuccess = blocks.every((b) => b.status === "success");
  const anyError = blocks.some((b) => b.status === "error");
  const statusIcon = anyError ? "\u2717" : allSuccess ? "\u2713" : "\u22EF";
  const statusClass = anyError ? "cc-tool-call--error" : allSuccess ? "cc-tool-call--success" : "cc-tool-call--pending";
  const isIncoming = blocks[0]?.peerIncoming;
  const arrow = isIncoming ? "\u2199" : "\u2197";
  const label = isIncoming ? `Received from ${targets.join(", ")}` : `Sent to ${targets.join(", ")}`;
  return /* @__PURE__ */ (0, import_jsx_runtime5.jsxs)("section", { className: clsx_default("cc-tool-call cc-tool-call--peer-group", isIncoming && "cc-tool-call--incoming", statusClass), children: [
    /* @__PURE__ */ (0, import_jsx_runtime5.jsxs)(
      "button",
      {
        className: "cc-tool-call__header",
        type: "button",
        onClick: () => setExpanded((prev) => !prev),
        "aria-expanded": expanded,
        children: [
          /* @__PURE__ */ (0, import_jsx_runtime5.jsx)("span", { className: "cc-tool-call__chevron", children: expanded ? "\u25BE" : "\u25B8" }),
          /* @__PURE__ */ (0, import_jsx_runtime5.jsx)("span", { className: "cc-tool-call__icon", children: arrow }),
          /* @__PURE__ */ (0, import_jsx_runtime5.jsx)("span", { className: "cc-tool-call__name", children: label }),
          /* @__PURE__ */ (0, import_jsx_runtime5.jsx)("span", { className: "cc-tool-call__status", children: statusIcon }),
          /* @__PURE__ */ (0, import_jsx_runtime5.jsx)(CopyBtn, { text: blocks.map((b) => toolBlockCopyText(b)).join("\n") })
        ]
      }
    ),
    expanded && /* @__PURE__ */ (0, import_jsx_runtime5.jsx)("div", { className: "cc-tool-call__body", children: blocks.map((block, i) => /* @__PURE__ */ (0, import_jsx_runtime5.jsxs)("div", { className: "cc-tool-call__peer-row", children: [
      /* @__PURE__ */ (0, import_jsx_runtime5.jsxs)("span", { className: "cc-tool-call__peer-target", children: [
        isIncoming ? "\u2190" : "\u2192",
        " ",
        block.peerTarget || "peer"
      ] }),
      block.peerIntent && /* @__PURE__ */ (0, import_jsx_runtime5.jsx)("span", { className: "cc-tool-call__peer-intent", children: block.peerIntent }),
      block.peerBody && /* @__PURE__ */ (0, import_jsx_runtime5.jsx)("span", { className: "cc-tool-call__peer-body", children: block.peerBody }),
      /* @__PURE__ */ (0, import_jsx_runtime5.jsx)("span", { className: `cc-tool-call__peer-status cc-tool-call__peer-status--${block.status}`, children: block.status === "success" ? "\u2713" : block.status === "error" ? "\u2717" : "\u22EF" })
    ] }, block.toolCallId || i)) })
  ] });
}
function ConversationRichContent({
  blocks,
  richStyle = "default",
  Icon: Icon2
}) {
  if (blocks.length > 1 && blocks.every((b) => b.type === "tool-call")) {
    const tools = blocks;
    const firstName = tools[0].name;
    if (tools.every((b) => b.name === firstName)) {
      const allPeer = tools.every((b) => PEER_TOOL_NAMES.has(b.name) || b.peerIncoming);
      if (allPeer) {
        return /* @__PURE__ */ (0, import_jsx_runtime5.jsx)(PeerToolGroup, { blocks: tools });
      }
      return /* @__PURE__ */ (0, import_jsx_runtime5.jsx)(ToolCallGroup, { blocks: tools });
    }
  }
  const body = blocks.map((block, index) => renderBlock(block, index, Icon2)).filter(Boolean);
  if (body.length === 0) {
    return null;
  }
  if (richStyle === "streaming") {
    return /* @__PURE__ */ (0, import_jsx_runtime5.jsx)("div", { className: "cc-rich-streaming", children: body });
  }
  return /* @__PURE__ */ (0, import_jsx_runtime5.jsx)(import_jsx_runtime5.Fragment, { children: body });
}

// ../packages/console-components/src/conversation/summary-card.tsx
var import_jsx_runtime6 = require("react/jsx-runtime");

// ../packages/console-components/src/conversation/conversation-message-view.tsx
var import_jsx_runtime7 = require("react/jsx-runtime");

// ../packages/console-components/src/conversation/conversation-message-group.tsx
var import_jsx_runtime8 = require("react/jsx-runtime");

// ../packages/console-components/src/conversation/turn-diff-card.tsx
var import_jsx_runtime9 = require("react/jsx-runtime");

// ../packages/console-components/src/conversation/conversation-transcript.tsx
var import_jsx_runtime10 = require("react/jsx-runtime");

// ../packages/console-components/src/conversation/conversation-pane.tsx
var import_jsx_runtime11 = require("react/jsx-runtime");

// ../packages/console-components/src/dock/console-dock.tsx
var import_react4 = require("react");
var import_jsx_runtime12 = require("react/jsx-runtime");

// ../packages/console-components/src/dock/use-console-dock-controller.ts
var import_react5 = require("react");
function useConsoleDockController({
  initialTarget = null,
  initialPresetId = "single",
  createPanelState,
  suggestTargets,
  resolvePanelView,
  resolveTabView
}) {
  const panelCounterRef = (0, import_react5.useRef)(1);
  const splitCounterRef = (0, import_react5.useRef)(1);
  const tabCounterRef = (0, import_react5.useRef)(1);
  function nextPanelId() {
    return `panel-${panelCounterRef.current++}`;
  }
  function nextSplitId() {
    return `split-${splitCounterRef.current++}`;
  }
  function nextTabId() {
    return `tab-${tabCounterRef.current++}`;
  }
  const [state, setState] = (0, import_react5.useState)(() => createConsoleDockState({
    initialTarget,
    initialPresetId,
    createPanelState: (args) => {
      const nextState = createPanelState(args);
      return {
        ...nextState,
        id: nextState.id || nextPanelId()
      };
    },
    createSplitId: nextSplitId,
    createTabId: nextTabId,
    suggestTargets
  }));
  const viewState = (0, import_react5.useMemo)(() => buildConsoleDockViewState(state, {
    resolvePanelView,
    resolveTabView
  }), [resolvePanelView, resolveTabView, state]);
  const focusedPanel = (0, import_react5.useMemo)(
    () => state.panels.find((panel) => panel.id === state.focusedPanelId) || null,
    [state.focusedPanelId, state.panels]
  );
  function dispatch(action) {
    setState((current) => applyConsoleDockAction(current, action, {
      createPanelState: (args) => {
        const nextState = createPanelState(args);
        return {
          ...nextState,
          id: nextState.id || nextPanelId()
        };
      },
      createSplitId: nextSplitId,
      createTabId: nextTabId,
      suggestTargets
    }));
  }
  return {
    state,
    setState,
    viewState,
    presets: consoleDockPresets(),
    focusedPanel,
    focusedPanelId: state.focusedPanelId,
    focusedTarget: focusedPanel?.target || null,
    dispatch,
    createTab: () => dispatch({ type: "create_tab" }),
    selectTab: (tabId) => dispatch({ type: "select_tab", tabId }),
    closeTab: (tabId) => dispatch({ type: "close_tab", tabId }),
    focusPanel: (panelId) => dispatch({ type: "focus_panel", panelId }),
    closePanel: (panelId) => dispatch({ type: "close_panel", panelId }),
    splitPanel: (panelId, direction) => dispatch({ type: "split_panel", panelId, direction }),
    resizeSplit: (splitId, ratio) => dispatch({ type: "resize_split", splitId, ratio }),
    applyPreset: (presetId) => dispatch({ type: "apply_preset", presetId }),
    openTarget: (target, intent) => dispatch({ type: "open_target", target, intent }),
    setPanelTarget: (panelId, target) => dispatch({ type: "set_panel_target", panelId, target }),
    setPanelMode: (panelId, mode) => dispatch({ type: "set_panel_mode", panelId, mode })
  };
}

// ../packages/console-components/src/sidebar/console-sidebar.tsx
var import_react6 = require("react");
var import_jsx_runtime13 = require("react/jsx-runtime");

// ../packages/console-components/src/workbench/console-workbench.tsx
var import_jsx_runtime14 = require("react/jsx-runtime");

// ../packages/console-components/src/composer/console-composer.tsx
var import_jsx_runtime15 = require("react/jsx-runtime");

// src/lib/agents.ts
function normalizeModelCapabilities(entry) {
  const record = entry && typeof entry === "object" ? entry : {};
  const caps = record.model_capabilities && typeof record.model_capabilities === "object" ? record.model_capabilities : {};
  return { image_input: caps.image_input === true };
}
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
      const modelCapabilities = entry.model_capabilities !== void 0 ? normalizeModelCapabilities(entry) : normalizeModelCapabilities(identityStatusRows.find((row) => {
        const normalized = normalizeIdentityStatusRow(row);
        return normalized?.identity === statusRow?.identity;
      }));
      return {
        ...statusRow?.identity ? { identity: statusRow.identity } : entry.identity ? { identity: String(entry.identity) } : {},
        agent_id: String(entry.agent_id || statusRow?.identity || entry.identity || entry.member_id || ""),
        member_id: String(entry.member_id || statusRow?.identity || entry.identity || entry.agent_id || ""),
        ...typeof entry.session_id === "string" && entry.session_id.trim() ? { session_id: entry.session_id.trim() } : {},
        label: String(entry.label || statusRow?.display_name || entry.display_name || statusRow?.identity || entry.identity || entry.member_id || entry.agent_id || "unknown"),
        kind: String(entry.kind || statusRow?.role || entry.role || "module_agent"),
        ...statusRow?.role !== void 0 ? { role: statusRow.role } : entry.role !== void 0 ? { role: String(entry.role) } : {},
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
        model_capabilities: modelCapabilities,
        ...watchFields
      };
    });
  }
  if (Array.isArray(identityStatusRows) && identityStatusRows.length > 0) {
    return identityStatusRows.map((entry) => {
      const statusRow = normalizeIdentityStatusRow(entry);
      const identity = statusRow?.identity || "";
      const modelCapabilities = normalizeModelCapabilities(entry);
      return {
        identity,
        agent_id: String(identity),
        member_id: identity ? `identity-only:${identity}` : "",
        ...typeof statusRow?.session_id === "string" && statusRow.session_id.trim() ? { session_id: statusRow.session_id.trim() } : {},
        label: String(statusRow?.display_name || identity || "unknown"),
        kind: String(statusRow?.role || "identity"),
        ...statusRow?.role !== void 0 ? { role: statusRow.role } : {},
        ...statusRow?.state !== void 0 ? { state: statusRow.state } : {},
        ...statusRow?.addressability ? { addressability: statusRow.addressability } : {},
        ...statusRow?.generation !== void 0 ? { generation: statusRow.generation } : {},
        ...statusRow?.checkpoint_version !== void 0 ? { checkpoint_version: statusRow.checkpoint_version } : {},
        ...statusRow?.lease_healthy !== void 0 ? { lease_healthy: statusRow.lease_healthy } : {},
        ...statusRow?.labels && Object.keys(statusRow.labels).length > 0 ? { labels: statusRow.labels } : {},
        addressable: false,
        affordances: { can_send_message: false },
        model_capabilities: modelCapabilities
      };
    });
  }
  if (Array.isArray(modules) && modules.length > 0) {
    return modules.map((moduleId) => ({
      agent_id: String(moduleId),
      member_id: String(moduleId),
      label: String(moduleId),
      kind: "module_agent",
      model_capabilities: { image_input: false }
    }));
  }
  return [];
}

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
  const subtitle = [agent.role, agent.kind].filter(Boolean).join(" \xB7 ") || void 0;
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
function buildInspectTarget(agent) {
  return {
    id: `inspect:${agent.identity || agent.member_id}`,
    kind: "identity-inspect",
    identity: agent.identity || agent.member_id,
    memberId: agent.member_id,
    title: `${agent.label} Inspect`,
    subtitle: agent.identity || agent.member_id,
    iconName: "i-terminal"
  };
}
function buildControlTarget(kind) {
  switch (kind) {
    case "routing":
      return { id: "routing", kind, title: "Routing", subtitle: "Routes and delivery history", iconName: "i-swap" };
    case "gating":
      return { id: "gating", kind, title: "Gating", subtitle: "Pending approvals and audit", iconName: "i-bolt" };
    case "topology":
      return { id: "topology", kind, title: "Topology", subtitle: "Identity connectivity", iconName: "i-team" };
    case "health":
      return { id: "health", kind, title: "Health", subtitle: "Runtime and identity health", iconName: "i-gear" };
    case "timeline":
      return { id: "timeline", kind, title: "Today", subtitle: "Chronological events", iconName: "i-clock" };
    case "roster":
      return { id: "roster", kind, title: "Roster", subtitle: "All agents", iconName: "i-team" };
    case "gates":
      return { id: "gates", kind, title: "Gates", subtitle: "Approval policies", iconName: "i-bolt" };
    case "logs":
      return { id: "logs", kind, title: "Logs", subtitle: "Event stream", iconName: "i-terminal" };
    default:
      return { id: "health", kind: "health", title: "Health" };
  }
}
function agentGroupKey(agent) {
  return agent.group?.trim() || agent.role?.trim() || agent.kind?.trim() || "Agents";
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
var USER_IDENTITY = {
  id: "user",
  label: "You",
  role: "user"
};
function agentIdentity(agent) {
  return {
    id: agent?.member_id || "agent",
    label: agent?.label || "Agent",
    role: "assistant"
  };
}
var SYSTEM_IDENTITY = {
  id: "system",
  label: "System",
  role: "system",
  presentation: "system",
  showLabel: true
};
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
    if (typeof record.result === "string") return record.result;
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
    case "user_input":
    case "interaction_started":
      return 0;
    case "tool_call_requested":
    case "tool_call":
    case "tool_execution_started":
      return 20;
    case "tool_result_received":
    case "tool_execution_completed":
      return 30;
    case "assistant_image":
      return 35;
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
var HIDDEN_EVENTS = /* @__PURE__ */ new Set([
  "subscribed",
  "run_started",
  "run_completed",
  "turn_started",
  "turn_completed",
  "text_complete",
  "reasoning_delta",
  "reasoning_complete",
  "interaction_started",
  "frame_updated",
  "run_failed",
  "keep-alive",
  "tool_config_changed",
  "tool_scope_changed"
]);
var ACTIVITY_HIDDEN_EVENTS = /* @__PURE__ */ new Set([
  ...HIDDEN_EVENTS,
  "text_delta",
  "tool_call_requested",
  "tool_call",
  "tool_execution_started",
  "tool_result_received",
  "tool_execution_completed"
]);
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
  const isError = Boolean(record?.is_error) || frame.event === "interaction_failed";
  let result = "";
  if (typeof record?.result === "string") {
    try {
      const parsed = JSON.parse(record.result);
      if (typeof parsed === "object" && parsed !== null) {
        const clean = { ...parsed };
        delete clean.source_event_type;
        delete clean.type;
        result = JSON.stringify(clean, null, 2);
      } else {
        result = record.result;
      }
    } catch {
      result = record.result;
    }
  } else if (typeof record?.result === "object" && record.result !== null) {
    const clean = { ...record.result };
    delete clean.source_event_type;
    delete clean.type;
    result = JSON.stringify(clean, null, 2);
  }
  if (!result && frame.event === "tool_result_received") {
    return { status: isError ? "error" : "success" };
  }
  return {
    ...result ? { result } : {},
    status: isError ? "error" : "success"
  };
}
function buildToolBlocks(frames) {
  const toolCalls = /* @__PURE__ */ new Map();
  const pendingResults = /* @__PURE__ */ new Map();
  const peerRegistry = /* @__PURE__ */ new Map();
  const lastSegment = (s) => s.split("/").pop() || s;
  for (const frame of frames) {
    if (frame.event === "tool_result_received" || frame.event === "tool_execution_completed") {
      const toolCallId = parseToolCallId(frame);
      const data = frame.data;
      if (data && (data.name === "peers" || data.tool_name === "peers")) {
        const rawResult = typeof data.result === "string" ? data.result : null;
        if (rawResult) {
          try {
            const parsed2 = JSON.parse(rawResult);
            if (Array.isArray(parsed2.peers)) {
              for (const p of parsed2.peers) {
                if (typeof p.peer_id === "string" && typeof p.name === "string") {
                  peerRegistry.set(p.peer_id, p.name);
                }
              }
            }
          } catch {
          }
        }
      }
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
      const name = parseToolName(frame);
      const args = frame.data && typeof frame.data === "object" ? frame.data.args : null;
      const argsRecord = args && typeof args === "object" ? args : null;
      const isPeerTool = name === "send_request" || name === "send_message" || name === "send_response";
      const registryName = isPeerTool && typeof argsRecord?.peer_id === "string" ? peerRegistry.get(argsRecord.peer_id.trim()) : void 0;
      const peerTarget = !isPeerTool ? void 0 : registryName ? lastSegment(registryName) : typeof argsRecord?.display_name === "string" && argsRecord.display_name.trim() ? lastSegment(argsRecord.display_name.trim()) : typeof argsRecord?.peer_id === "string" && argsRecord.peer_id.trim() ? argsRecord.peer_id.trim().slice(0, 8) : typeof argsRecord?.to === "string" ? lastSegment(argsRecord.to) : void 0;
      const peerIntent = isPeerTool && typeof argsRecord?.intent === "string" ? argsRecord.intent : void 0;
      const peerBody = isPeerTool ? typeof argsRecord?.body === "string" ? argsRecord.body : typeof argsRecord?.params === "object" && argsRecord.params !== null ? JSON.stringify(argsRecord.params) : void 0 : void 0;
      toolCalls.set(toolCallId, {
        type: "tool-call",
        toolCallId,
        name,
        arguments: parseToolArguments(frame),
        ...pending?.result ? { result: pending.result } : {},
        status: pending?.status || "pending",
        ...peerTarget ? { peerTarget } : {},
        ...peerIntent ? { peerIntent } : {},
        ...peerBody ? { peerBody } : {}
      });
    }
  }
  return toolCalls;
}
function parsePeerSummary(text) {
  const match = text.match(/Peer\s+(response|request|message):\s*(.+?)(?:\s*Status:\s|$)/s);
  if (!match) return null;
  const [, verb, body] = match;
  let summary2 = body.trim();
  try {
    const parsed = JSON.parse(summary2);
    if (typeof parsed === "object" && parsed !== null) {
      if (typeof parsed.summary === "string") summary2 = parsed.summary;
      else if (typeof parsed.text === "string") summary2 = parsed.text;
      else if (typeof parsed.body === "string") summary2 = parsed.body;
      else if (typeof parsed.message === "string") summary2 = parsed.message;
    }
  } catch {
    summary2 = summary2.replace(/^["']|["']$/g, "");
  }
  return { verb, summary: summary2 };
}
function renderPeerEntry(frame, entryId) {
  const rawText = summarizeFrameData(frame.data);
  if (!rawText) return null;
  const peer = parsePeerSummary(rawText);
  if (!peer) return null;
  return {
    kind: "message",
    id: entryId,
    identity: SYSTEM_IDENTITY,
    variant: "meta",
    createdAt: isoFromTimestampMs(frame.timestampMs),
    text: `\u21A9 ${peer.verb}: ${peer.summary}`
  };
}
function renderTerminalEntry(agent, frame, entryId, streamedText = "") {
  if (frame.event === "interaction_complete") {
    const text = summarizeFrameData(frame.data).trim();
    if (!text) return null;
    const peer = parsePeerSummary(text);
    if (peer) {
      return {
        kind: "message",
        id: entryId,
        identity: SYSTEM_IDENTITY,
        variant: "meta",
        createdAt: isoFromTimestampMs(frame.timestampMs),
        text: `\u21A9 ${peer.verb}: ${peer.summary}`
      };
    }
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
function buildBlobUrl(blobId, baseUrl) {
  const path = `/blobs/${encodeURIComponent(blobId)}`;
  const base = baseUrl?.trim();
  if (!base) return path;
  return `${base.replace(/\/+$/, "")}${path}`;
}
function renderAssistantImageEntry(agent, frame, entryId, blobBaseUrl) {
  const data = frame.data && typeof frame.data === "object" ? frame.data : {};
  const blobId = typeof data.blob_id === "string" ? data.blob_id : "";
  if (!blobId) return null;
  const mediaType = typeof data.media_type === "string" ? data.media_type : "image/png";
  const width = typeof data.width === "number" ? data.width : void 0;
  const height = typeof data.height === "number" ? data.height : void 0;
  const imageId = typeof data.image_id === "string" ? data.image_id : void 0;
  return {
    kind: "message",
    id: entryId,
    identity: agentIdentity(agent),
    variant: "rich",
    createdAt: isoFromTimestampMs(frame.timestampMs),
    blocks: [{
      type: "image",
      src: buildBlobUrl(blobId, blobBaseUrl),
      mediaType,
      alt: "generated image",
      ...width !== void 0 ? { width } : {},
      ...height !== void 0 ? { height } : {},
      blobId,
      ...imageId ? { imageId } : {}
    }]
  };
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
function renderHistoryUserEntry(frame, entryId, blobBaseUrl) {
  if (frame.event !== "interaction_started" && frame.event !== "user_input") {
    return null;
  }
  if (typeof frame.data !== "object" || frame.data === null) {
    return null;
  }
  const record = frame.data;
  const content = record.content;
  if (Array.isArray(content)) {
    const blocks = contentToUserBlocks(content, blobBaseUrl);
    if (blocks.length === 0) return null;
    return {
      kind: "message",
      id: entryId,
      identity: USER_IDENTITY,
      variant: "rich",
      createdAt: isoFromTimestampMs(frame.timestampMs),
      blocks
    };
  }
  const text = extractTextFromContentBlocks(content).trim();
  if (!text) return null;
  return {
    kind: "message",
    id: entryId,
    identity: USER_IDENTITY,
    variant: "plain",
    createdAt: isoFromTimestampMs(frame.timestampMs),
    text
  };
}
function renderRunStartedPromptEntries(frame, entryId, options = {}) {
  if (frame.event !== "run_started" || typeof frame.data !== "object" || frame.data === null) {
    return [];
  }
  const record = frame.data;
  const prompt = typeof record.prompt === "string" ? record.prompt.trim() : "";
  if (!prompt) {
    return [];
  }
  const createdAt = isoFromTimestampMs(frame.timestampMs);
  const entries = [];
  const embeddedPrompt = extractEmbeddedRpcPrompt(prompt);
  if (embeddedPrompt && !options.suppressEmbeddedRpcPrompt) {
    entries.push({
      kind: "message",
      id: `${entryId}:event`,
      identity: USER_IDENTITY,
      variant: "plain",
      ...createdAt ? { createdAt } : {},
      text: embeddedPrompt
    });
  }
  if (prompt.startsWith("[COMMS")) {
    const summarized = summarizeCommsTransport(prompt).trim();
    if (summarized) {
      entries.push({
        kind: "message",
        id: entryId,
        identity: SYSTEM_IDENTITY,
        variant: "meta",
        ...createdAt ? { createdAt } : {},
        text: summarized
      });
    }
  } else if (prompt.startsWith("[SYSTEM NOTICE][PEER_")) {
    const incomingBlocks = parseIncomingCommsBlocks(prompt);
    if (incomingBlocks.length > 0) {
      entries.push({
        kind: "message",
        id: entryId,
        identity: { id: "comms", label: "", role: "system", showLabel: false },
        variant: "rich",
        ...createdAt ? { createdAt } : {},
        blocks: incomingBlocks
      });
    } else {
      const summarized = summarizeCommsTransport(prompt).trim();
      if (summarized) {
        entries.push({
          kind: "message",
          id: entryId,
          identity: SYSTEM_IDENTITY,
          variant: "meta",
          ...createdAt ? { createdAt } : {},
          text: summarized
        });
      }
    }
  }
  return entries;
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
function contentToUserBlocks(content, blobBaseUrl) {
  if (typeof content === "string") {
    return parseConversationRichBlocks(stripRpcEventPrefix(content));
  }
  if (!Array.isArray(content)) {
    return [];
  }
  const blocks = [];
  for (const block of content) {
    if (typeof block === "string") {
      blocks.push(...parseConversationRichBlocks(stripRpcEventPrefix(block)));
      continue;
    }
    if (!block || typeof block !== "object") continue;
    const record = block;
    const type = typeof record.type === "string" ? record.type : "";
    if (type === "text") {
      const text = typeof record.text === "string" ? record.text : typeof record.content === "string" ? record.content : "";
      blocks.push(...parseConversationRichBlocks(stripRpcEventPrefix(text)));
      continue;
    }
    if (type === "image") {
      const source = typeof record.source === "string" ? record.source : "";
      const blobId = typeof record.blob_id === "string" ? record.blob_id : typeof record.blobId === "string" ? record.blobId : "";
      const mediaType = typeof record.media_type === "string" ? record.media_type : typeof record.mediaType === "string" ? record.mediaType : "image/png";
      const inlineData = typeof record.data === "string" ? record.data : typeof record.base64 === "string" ? record.base64 : "";
      const src = source === "blob" && blobId ? buildBlobUrl(blobId, blobBaseUrl) : inlineData ? `data:${mediaType};base64,${inlineData}` : "";
      if (!src) continue;
      const alt = typeof record.alt === "string" && record.alt.trim() ? record.alt.trim() : "attached image";
      const width = typeof record.width === "number" ? record.width : void 0;
      const height = typeof record.height === "number" ? record.height : void 0;
      blocks.push({
        type: "image",
        src,
        mediaType,
        alt,
        ...width !== void 0 ? { width } : {},
        ...height !== void 0 ? { height } : {},
        ...blobId ? { blobId } : {}
      });
    }
  }
  return blocks;
}
function stripRpcEventPrefix(text) {
  return text.replace(/^\[EVENT via rpc\]\s*/i, "").trim();
}
function summarizeCommsTransport(text) {
  const lines = text.split("\n").map((line) => line.trim()).filter(Boolean);
  if (lines.length === 0) {
    return "";
  }
  const header = lines[0] || "";
  const headerTail = header.includes("]") ? header.slice(header.indexOf("]") + 1).trim() : "";
  const body = lines.slice(1).filter((line) => !line.startsWith("[EVENT via rpc]"));
  if (header.startsWith("[COMMS REQUEST")) {
    const intentLine = body.find((line) => line.startsWith("Intent:"));
    if (intentLine) {
      const intent = intentLine.replace(/^Intent:\s*/, "").trim();
      if (intent === "mob.peer_added" || intent === "mob.peer_removed") {
        return "";
      }
      return `Peer request: ${intent}`;
    }
    return "Peer request received";
  }
  if (header.startsWith("[COMMS RESPONSE")) {
    const statusLine = body.find((line) => line.startsWith("Status:"));
    const status = statusLine ? statusLine.replace(/^Status:\s*/, "").trim() : "";
    const resultIndex = body.findIndex((line) => line.startsWith("Result:"));
    if (resultIndex >= 0) {
      const resultLines = [];
      for (let i = resultIndex; i < body.length; i++) {
        const line = body[i];
        if (i > resultIndex && (line.startsWith("Status:") || line.startsWith("[COMMS "))) break;
        resultLines.push(line);
      }
      const resultText = resultLines.join(" ").replace(/^Result:\s*/, "").trim();
      let summary2 = resultText;
      try {
        const parsed = JSON.parse(resultText);
        if (typeof parsed === "string") {
          summary2 = parsed;
        } else if (typeof parsed === "object" && parsed !== null) {
          const val = parsed.summary ?? parsed.text ?? parsed.body ?? parsed.message ?? parsed.reply ?? parsed.result ?? parsed.content;
          if (typeof val === "string") summary2 = val;
        }
      } catch {
      }
      const label = status ? `Peer response (${status})` : "Peer response";
      return `${label}: ${summary2}`;
    }
    return status ? `Peer response (${status})` : "Peer response received";
  }
  if (header.startsWith("[COMMS MESSAGE")) {
    const joined = [headerTail, ...body].join(" ").trim();
    return joined ? `Peer message: ${joined}` : "Peer message received";
  }
  return text;
}
function isCommsHeaderLine(line) {
  const trimmed = line.trimStart();
  if (trimmed.startsWith("[COMMS ")) return true;
  if (trimmed.startsWith("[SYSTEM NOTICE][PEER_")) return true;
  return false;
}
function extractSystemNoticeSender(header, body) {
  const merged = [header, ...body].join(" ");
  const displayMatch = merged.match(/display_name:\s*([^).]+?)(?=\)|\.|$)/i);
  if (displayMatch) {
    const raw = displayMatch[1].trim();
    const last = raw.split("/").pop() || raw;
    if (last) return last;
  }
  const peerIdMatch = merged.match(/peer_id\s+([0-9a-f-]{6,})/i);
  if (peerIdMatch) return peerIdMatch[1].slice(0, 8);
  return null;
}
function parseIncomingCommsBlocks(prompt) {
  const sections = [];
  let current = "";
  for (const line of prompt.split("\n")) {
    if (isCommsHeaderLine(line) && current) {
      sections.push(current);
      current = line + "\n";
    } else {
      current += line + "\n";
    }
  }
  if (current.trim()) sections.push(current);
  const blocks = [];
  let counter = 0;
  for (const section of sections) {
    const lines = section.split("\n").map((l) => l.trim()).filter(Boolean);
    const header = lines[0] || "";
    if (!isCommsHeaderLine(header)) continue;
    const isLegacy = header.startsWith("[COMMS");
    let sender;
    if (isLegacy) {
      const senderMatch = header.match(/\[COMMS\s+\w+\s+from\s+\S+\/([^/\s\]]+)/);
      sender = senderMatch ? senderMatch[1] : null;
    } else {
      sender = extractSystemNoticeSender(header, lines.slice(1));
    }
    if (!sender) continue;
    const body = lines.slice(1).filter((l) => !isCommsHeaderLine(l) && !l.startsWith("[EVENT via rpc]"));
    counter++;
    const isResponse = header.startsWith("[COMMS RESPONSE") || header.startsWith("[SYSTEM NOTICE][PEER_RESPONSE_TERMINAL]") || header.startsWith("[SYSTEM NOTICE][PEER_RESPONSE_PROGRESS]");
    const isRequest = header.startsWith("[COMMS REQUEST") || header.startsWith("[SYSTEM NOTICE][PEER_REQUEST]");
    const isMessage = header.startsWith("[COMMS MESSAGE") || header.startsWith("[SYSTEM NOTICE][PEER_MESSAGE]");
    if (isResponse) {
      const haystack = [header, ...body].join("\n");
      const statusMatch = haystack.match(/Status:\s*([A-Za-z_]+)/);
      const status = statusMatch ? statusMatch[1].trim() : "";
      const resultMatch = haystack.match(/Result:\s*([\s\S]+?)(?:\n\[|\.\s*$|$)/);
      let resultSummary = "";
      if (resultMatch) {
        const raw = resultMatch[1].trim().replace(/\.$/, "");
        try {
          const parsed = JSON.parse(raw);
          if (typeof parsed === "string") {
            resultSummary = parsed;
          } else if (typeof parsed === "object" && parsed !== null) {
            const val = parsed.summary ?? parsed.text ?? parsed.body ?? parsed.message ?? parsed.reply ?? parsed.result ?? parsed.content;
            resultSummary = typeof val === "string" ? val : raw;
          } else {
            resultSummary = raw;
          }
        } catch {
          resultSummary = raw;
        }
      }
      blocks.push({
        type: "tool-call",
        toolCallId: `incoming-${sender}-${counter}`,
        name: "response",
        arguments: "",
        status: status === "failed" ? "error" : "success",
        peerTarget: sender,
        peerIntent: resultSummary || status || "response",
        peerIncoming: true
      });
    } else if (isRequest) {
      const haystack = [header, ...body].join("\n");
      const intentMatch = haystack.match(/Intent:\s*([^.\n]+?)(?:\.\s|\n|$)/);
      const intent = intentMatch ? intentMatch[1].trim() : "";
      if (intent === "mob.peer_added" || intent === "mob.peer_removed") continue;
      const requestIdMatch = haystack.match(/Request ID:\s*([0-9a-fA-F-]+)/);
      const paramsMatch = haystack.match(/Params:\s*(\{[\s\S]*?\}|"[^"]*"|[^.\n]+)/);
      const requestId = requestIdMatch ? requestIdMatch[1].trim() : "";
      let paramsBody = "";
      if (paramsMatch) {
        const raw = paramsMatch[1].trim();
        try {
          const parsed = JSON.parse(raw);
          paramsBody = typeof parsed === "object" && parsed !== null ? JSON.stringify(parsed) : raw;
        } catch {
          paramsBody = raw;
        }
      }
      const peerBody = [
        paramsBody,
        requestId ? `(req: ${requestId.slice(0, 8)})` : ""
      ].filter(Boolean).join(" ").trim();
      blocks.push({
        type: "tool-call",
        toolCallId: `incoming-${sender}-${counter}`,
        name: "request",
        arguments: paramsBody,
        status: "success",
        peerTarget: sender,
        peerIntent: intent || "request",
        ...peerBody ? { peerBody } : {},
        peerIncoming: true
      });
    } else if (isMessage) {
      const joined = body.join(" ").trim();
      blocks.push({
        type: "tool-call",
        toolCallId: `incoming-${sender}-${counter}`,
        name: "message",
        arguments: "",
        status: "success",
        peerTarget: sender,
        peerIntent: joined || "message",
        peerIncoming: true
      });
    }
  }
  return blocks;
}
function extractEmbeddedRpcPrompt(text) {
  const match = text.match(/^\[EVENT via rpc\]\s*(.+)$/im);
  return match?.[1]?.trim() || null;
}
function mapFramesToTimelineEntries(agent, frames, options = {}) {
  const orderedFrames = options.renderInteractionStartsAsUser ? sortFramesForTranscript(frames) : frames;
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
      if (options.renderTextDeltas === false) {
        continue;
      }
      if (!pendingId) {
        pendingId = entryId;
        pendingCreatedAt = isoFromTimestampMs(frame.timestampMs);
      }
      pendingText += summarizeFrameData(frame.data);
      continue;
    }
    if (frame.event === "assistant_image") {
      flushPendingText();
      const imageEntry = renderAssistantImageEntry(agent, frame, entryId, options.blobBaseUrl);
      if (imageEntry) {
        entries.push(imageEntry);
      }
      continue;
    }
    const toolCallId = parseToolCallId(frame);
    if (toolCallId && (frame.event === "tool_call_requested" || frame.event === "tool_call" || frame.event === "tool_execution_started") && !emittedToolCalls.has(toolCallId)) {
      flushPendingText();
      const block = toolBlocks.get(toolCallId);
      if (block) {
        const lastEntry = entries[entries.length - 1];
        const lastBlocks = lastEntry && lastEntry.kind === "message" && lastEntry.variant === "rich" && Array.isArray(lastEntry.blocks) ? lastEntry.blocks : null;
        const lastIsToolGroup = !!(lastBlocks && lastBlocks.length > 0 && lastBlocks.every((b) => b.type === "tool-call"));
        const lastSameName = lastIsToolGroup && lastBlocks.every((b) => b.name === block.name);
        const newIncoming = block.peerIncoming === true;
        const peerCompatible = !block.peerTarget || lastIsToolGroup && lastBlocks.every((b) => Boolean(b.peerIncoming) === newIncoming);
        if (lastSameName && peerCompatible) {
          lastEntry.blocks.push(block);
        } else {
          entries.push({
            kind: "message",
            id: entryId,
            identity: agentIdentity(agent),
            variant: "rich",
            createdAt: isoFromTimestampMs(frame.timestampMs),
            blocks: [block]
          });
        }
        emittedToolCalls.add(toolCallId);
      }
      continue;
    }
    if (frame.event === "tool_result_received" || frame.event === "tool_execution_completed") {
      continue;
    }
    if (options.renderInteractionStartsAsUser && (frame.event === "interaction_started" || frame.event === "user_input")) {
      flushPendingText();
      const userEntry = renderHistoryUserEntry(frame, entryId, options.blobBaseUrl);
      if (userEntry) {
        entries.push(userEntry);
      }
      continue;
    }
    if (frame.event === "run_started") {
      flushPendingText();
      const promptEntries = renderRunStartedPromptEntries(frame, entryId, {
        suppressEmbeddedRpcPrompt: options.renderInteractionStartsAsUser === true || options.suppressEmbeddedRunStartedPrompt === true
      });
      if (promptEntries.length > 0) {
        entries.push(...promptEntries);
        continue;
      }
    }
    if (frame.event === "text_complete") {
      continue;
    }
    if (frame.event === "interaction_complete" || frame.event === "interaction_failed" || frame.event === "run_failed") {
      const streamedText = pendingText;
      flushPendingText();
      const terminalEntry = renderTerminalEntry(agent, frame, entryId, streamedText);
      if (terminalEntry) {
        entries.push(terminalEntry);
      }
      continue;
    }
    if (HIDDEN_EVENTS.has(frame.event)) {
      continue;
    }
    flushPendingText();
    const peerEntry = renderPeerEntry(frame, entryId);
    if (peerEntry) {
      entries.push(peerEntry);
      continue;
    }
    if (frame.event === "tool_call_requested" || frame.event === "tool_call" || frame.event === "tool_execution_started" || frame.event === "tool_result_received" || frame.event === "tool_execution_completed") {
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
function createUserEntry(message, images = []) {
  if (images.length > 0) {
    const blocks = [
      ...parseConversationRichBlocks(message),
      ...images.map((image) => ({
        type: "image",
        src: image.src,
        mediaType: image.mediaType,
        alt: image.alt || "attached image"
      }))
    ];
    return {
      kind: "message",
      id: `user:${Date.now()}`,
      identity: USER_IDENTITY,
      variant: "rich",
      createdAt: (/* @__PURE__ */ new Date()).toISOString(),
      blocks
    };
  }
  return {
    kind: "message",
    id: `user:${Date.now()}`,
    identity: USER_IDENTITY,
    variant: "plain",
    createdAt: (/* @__PURE__ */ new Date()).toISOString(),
    text: message
  };
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
function buildConversationViewState(args) {
  const groups = groupConversationTimelineEntries(args.entries);
  const suggestions = buildQuickPromptSuggestions(args.agent ?? null);
  return {
    conversationId: args.memberId || "console",
    title: args.agentLabel,
    entries: args.entries,
    groups,
    turnDiff: null,
    emptyState: args.entries.length === 0 ? {
      title: args.agentLabel,
      subtitle: "Send a message to start the conversation.",
      ...suggestions.length ? { suggestions } : {}
    } : null
  };
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
  const pulseItems = filteredFrames.slice(0, 200).map((frame, index) => {
    const frameIdentity = frame.identity?.trim();
    const agent = frameIdentity ? agentByIdentity.get(frameIdentity) : null;
    const ts = typeof frame.timestampMs === "number" ? new Date(frame.timestampMs).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", second: "2-digit" }) : "";
    return {
      id: `event:${frame.id || index}`,
      title: agent?.label || frameIdentity || frame.event || "event",
      line: summarizeFrameData(frame.data).slice(0, 120) || frame.event,
      meta: `${frame.event}${ts ? ` \xB7 ${ts}` : ""}`,
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

// src/lib/errors.ts
function errorMessage(error) {
  if (error instanceof Error) {
    return error.message;
  }
  return String(error);
}

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
  if (typeof record.type === "string" && "frame" in record) {
    const frame = timelineFrameToConsoleFrame(record.frame);
    const isUpdateEnvelope = eventName === "frame_updated";
    return {
      id: frame.id,
      event: isUpdateEnvelope ? "frame_updated" : frame.event,
      identity: frame.identity,
      interactionId: frame.interactionId,
      timestampMs: frame.timestampMs,
      cursor: frame.cursor,
      runtimeKey: frame.runtimeKey,
      sessionId: frame.sessionId,
      status: frame.status,
      frameVersion: frame.frameVersion,
      updatedAtMs: frame.updatedAtMs,
      turnId: frame.turnId,
      runId: frame.runId,
      data: isUpdateEnvelope ? frame.event === "frame_updated" ? frame.data : { frame } : frame.data
    };
  }
  return { data };
}
function timelineFrameToConsoleFrame(raw) {
  if (!raw || typeof raw !== "object") {
    return { id: "", event: "event", data: raw };
  }
  const record = raw;
  const cursor = typeof record.cursor === "string" ? record.cursor : void 0;
  const payload = "payload" in record ? record.payload : record;
  if (record.kind === "frame_updated" && payload && typeof payload === "object" && "frame" in payload) {
    const updated = timelineFrameToConsoleFrame(payload.frame);
    return {
      id: String(record.id || cursor || ""),
      event: "frame_updated",
      identity: typeof record.identity === "string" ? record.identity : updated.identity,
      interactionId: typeof record.interaction_id === "string" ? record.interaction_id : updated.interactionId,
      timestampMs: typeof record.timestamp_ms === "number" ? record.timestamp_ms : void 0,
      cursor,
      runtimeKey: typeof record.runtime_key === "string" ? record.runtime_key : updated.runtimeKey,
      sessionId: typeof record.session_id === "string" ? record.session_id : updated.sessionId,
      status: typeof record.status === "string" ? record.status : updated.status,
      frameVersion: typeof record.frame_version === "number" ? record.frame_version : updated.frameVersion,
      updatedAtMs: typeof record.updated_at_ms === "number" ? record.updated_at_ms : updated.updatedAtMs,
      turnId: typeof record.turn_id === "string" ? record.turn_id : updated.turnId,
      runId: typeof record.run_id === "string" ? record.run_id : updated.runId,
      data: { frame: updated }
    };
  }
  return {
    id: String(record.id || cursor || ""),
    event: String(record.kind || "event"),
    identity: typeof record.identity === "string" ? record.identity : void 0,
    interactionId: typeof record.interaction_id === "string" ? record.interaction_id : void 0,
    timestampMs: typeof record.timestamp_ms === "number" ? record.timestamp_ms : void 0,
    cursor,
    runtimeKey: typeof record.runtime_key === "string" ? record.runtime_key : void 0,
    sessionId: typeof record.session_id === "string" ? record.session_id : void 0,
    status: typeof record.status === "string" ? record.status : void 0,
    frameVersion: typeof record.frame_version === "number" ? record.frame_version : void 0,
    updatedAtMs: typeof record.updated_at_ms === "number" ? record.updated_at_ms : void 0,
    turnId: typeof record.turn_id === "string" ? record.turn_id : void 0,
    runId: typeof record.run_id === "string" ? record.run_id : void 0,
    data: payload
  };
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
      cursor: normalized.cursor,
      runtimeKey: normalized.runtimeKey,
      sessionId: normalized.sessionId,
      status: normalized.status,
      frameVersion: normalized.frameVersion,
      updatedAtMs: normalized.updatedAtMs,
      turnId: normalized.turnId,
      runId: normalized.runId,
      data: normalized.data
    });
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
    const typedError = normalizeConsoleInteractionRejectedError(result.error);
    if (typedError) {
      const error = new Error(`${method} RPC error ${typedError.code}: ${typedError.message}`);
      error.rpcError = typedError;
      throw error;
    }
    throw new Error(`${method} RPC error: ${result.error.message || JSON.stringify(result.error)}`);
  }
  return result.result;
}
async function sendMessage(baseUrl, memberId, message, handlingMode = "queue") {
  return rpc(baseUrl, "mobkit/send_message", {
    member_id: memberId,
    message,
    handling_mode: handlingMode
  });
}
async function sendMessageMultipart(baseUrl, memberId, message, attachments, handlingMode = "queue") {
  const content = [];
  if (message.trim()) {
    content.push({ type: "text", text: message });
  }
  const form = new FormData();
  attachments.forEach((file, index) => {
    const uploadId = `upload-${Date.now().toString(36)}-${index}`;
    content.push({
      type: "image_upload",
      upload_id: uploadId,
      media_type: file.type || "application/octet-stream",
      alt: file.name
    });
    form.append(`file:${uploadId}`, file, file.name);
  });
  form.append("payload", JSON.stringify({
    jsonrpc: "2.0",
    id: `mobkit/send_message:${Date.now()}`,
    method: "mobkit/send_message",
    params: {
      member_id: memberId,
      content,
      handling_mode: handlingMode
    }
  }));
  const response = await fetch(`${baseUrl}/console/rpc/multipart`, {
    method: "POST",
    body: form
  });
  if (!response.ok) {
    const text = await response.text();
    throw new Error(`mobkit/send_message multipart failed ${response.status}: ${text}`);
  }
  const result = await response.json();
  if (result.error) {
    throw new Error(`mobkit/send_message RPC error: ${result.error.message || JSON.stringify(result.error)}`);
  }
  return result.result;
}
async function sendConsoleMultipart(baseUrl, identity, message, attachments, origin, idempotencyKey, handlingMode = "queue") {
  const content = [];
  if (message.trim()) {
    content.push({ type: "text", text: message });
  }
  const form = new FormData();
  attachments.forEach((file, index) => {
    const uploadId = `upload-${Date.now().toString(36)}-${index}`;
    content.push({
      type: "image_upload",
      upload_id: uploadId,
      media_type: file.type || "application/octet-stream",
      alt: file.name
    });
    form.append(`file:${uploadId}`, file, file.name);
  });
  form.append("payload", JSON.stringify({
    jsonrpc: "2.0",
    id: `mobkit/console/send:${Date.now()}`,
    method: "mobkit/console/send",
    params: {
      identity,
      content,
      origin,
      idempotency_key: idempotencyKey,
      handling_mode: handlingMode
    }
  }));
  const response = await fetch(`${baseUrl}/console/rpc/multipart`, {
    method: "POST",
    body: form
  });
  if (!response.ok) {
    const text = await response.text();
    throw new Error(`mobkit/console/send multipart failed ${response.status}: ${text}`);
  }
  const result = await response.json();
  if (result.error) {
    throw new Error(`mobkit/console/send RPC error: ${result.error.message || JSON.stringify(result.error)}`);
  }
  return normalizeConsoleTimelineAccepted(result.result, identity);
}
var TERMINAL_SSE_EVENTS = /* @__PURE__ */ new Set([
  "interaction_complete",
  "run_completed",
  "interaction_failed",
  "run_failed"
]);
function matchesCorrelation(candidate, correlation, allowUnscoped = true) {
  if (!correlation?.sessionId && !correlation?.interactionId) {
    return true;
  }
  if (candidate === null || typeof candidate !== "object") {
    return allowUnscoped;
  }
  const record = candidate;
  const sessionId = record.session_id ?? record.sessionId;
  const interactionId = record.interaction_id ?? record.interactionId;
  const hasScopedField = sessionId !== void 0 || interactionId !== void 0;
  if (!hasScopedField) {
    return allowUnscoped;
  }
  if (correlation.sessionId && sessionId === correlation.sessionId) {
    return true;
  }
  if (correlation.interactionId && interactionId === correlation.interactionId) {
    return true;
  }
  return false;
}
async function streamFramesFromResponse(response, options = {}) {
  const stopOnTerminal = options.stopOnTerminal ?? Boolean(options.correlation);
  if (!response.ok) {
    const text = await response.text();
    let parsed = null;
    try {
      parsed = JSON.parse(text);
    } catch {
      parsed = null;
    }
    const replayError = normalizeReplayUnavailableError(parsed);
    if (replayError) {
      const error = new Error(
        `interaction stream replay unavailable for ${replayError.stream}: ${replayError.requested_last_event_id} -> ${replayError.latest_event_id}`
      );
      error.replayError = replayError;
      throw error;
    }
    throw new Error(`interaction stream request failed ${response.status}: ${text}`);
  }
  if (!response.body || typeof response.body.getReader !== "function") {
    const frames2 = parseSseFrames(await response.text());
    for (const frame of frames2) {
      if (matchesCorrelation(frame, options.correlation, true)) {
        options.onFrame?.(frame);
      }
    }
    return !options.correlation ? frames2 : frames2.filter((frame) => matchesCorrelation(frame, options.correlation, true));
  }
  const reader = response.body.getReader();
  const decoder = new TextDecoder();
  let frameBuffer = "";
  const frames = [];
  try {
    while (true) {
      const { value, done } = await reader.read();
      if (done) {
        break;
      }
      const chunk = decoder.decode(value, { stream: true });
      frameBuffer += chunk;
      let sawTerminal = false;
      frameBuffer = flushSseBlocks(frameBuffer, (frame) => {
        if (matchesCorrelation(frame, options.correlation, true)) {
          frames.push(frame);
          options.onFrame?.(frame);
          if (stopOnTerminal && TERMINAL_SSE_EVENTS.has(frame.event || "")) {
            sawTerminal = true;
          }
        }
      });
      if (sawTerminal) {
        break;
      }
    }
    const finalChunk = decoder.decode();
    frameBuffer += finalChunk;
    frameBuffer = flushSseBlocks(frameBuffer, (frame) => {
      if (matchesCorrelation(frame, options.correlation, true)) {
        frames.push(frame);
        options.onFrame?.(frame);
      }
    });
    flushTrailingSseBlock(frameBuffer, (frame) => {
      if (matchesCorrelation(frame, options.correlation, true)) {
        frames.push(frame);
        options.onFrame?.(frame);
      }
    });
  } finally {
    try {
      await reader.cancel();
    } catch {
    }
  }
  return frames;
}
function flushSseBlocks(buffer, onFrame) {
  let searchIndex = 0;
  while (true) {
    const boundaryIndex = buffer.indexOf("\n\n", searchIndex);
    if (boundaryIndex === -1) {
      break;
    }
    const block = buffer.slice(0, boundaryIndex + 2);
    buffer = buffer.slice(boundaryIndex + 2);
    searchIndex = 0;
    for (const frame of parseSseFrames(block)) {
      onFrame(frame);
    }
  }
  return buffer;
}
function flushTrailingSseBlock(buffer, onFrame) {
  if (!buffer.trim()) {
    return;
  }
  for (const frame of parseSseFrames(`${buffer}

`)) {
    onFrame(frame);
  }
}
async function queryTimeline(baseUrl, target, limit = 400) {
  const result = await rpc(baseUrl, "mobkit/console/query_timeline", {
    limit,
    ...target.identity?.trim() ? { identity: target.identity.trim() } : {},
    ...target.conversationId?.trim() ? { conversation_id: target.conversationId.trim() } : {},
    ...target.after?.trim() ? { after: target.after.trim() } : {}
  });
  if (!result || typeof result !== "object") {
    return { frames: [], available: false };
  }
  const record = result;
  const rawFrames = Array.isArray(record.frames) ? record.frames : [];
  return {
    frames: rawFrames.map(timelineFrameToConsoleFrame),
    nextCursor: typeof record.next_cursor === "string" ? record.next_cursor : void 0,
    available: true
  };
}
async function sendConsole(baseUrl, identity, content, origin, idempotencyKey, handlingMode = "queue") {
  const accepted = await rpc(baseUrl, "mobkit/console/send", {
    identity,
    content,
    origin,
    idempotency_key: idempotencyKey,
    handling_mode: handlingMode
  });
  if (!accepted || typeof accepted !== "object") {
    throw new Error("mobkit/console/send returned an invalid acceptance payload");
  }
  const record = accepted;
  return normalizeConsoleTimelineAccepted(record, identity);
}
function normalizeConsoleTimelineAccepted(accepted, fallbackIdentity) {
  const record = accepted && typeof accepted === "object" ? accepted : {};
  return {
    interaction_id: String(record.interaction_id || ""),
    identity: String(record.identity || fallbackIdentity),
    conversation_id: typeof record.conversation_id === "string" ? record.conversation_id : void 0,
    session_id: typeof record.session_id === "string" ? record.session_id : void 0,
    input_frame_id: typeof record.input_frame_id === "string" ? record.input_frame_id : void 0,
    cursor: typeof record.cursor === "string" ? record.cursor : void 0,
    status: typeof record.status === "string" ? record.status : void 0
  };
}
async function callConsoleRpc(baseUrl, method, params = {}) {
  return rpc(baseUrl, method, params);
}
function timelineStreamPath(target) {
  const params = new URLSearchParams();
  if (target.identity?.trim()) params.set("identity", target.identity.trim());
  if (target.conversationId?.trim()) params.set("conversation_id", target.conversationId.trim());
  if (target.after?.trim()) params.set("after", target.after.trim());
  return `/console/timeline/stream${params.size > 0 ? `?${params.toString()}` : ""}`;
}
function cursorFromTimelineFrame(frame) {
  const cursor = frame.cursor?.trim();
  if (cursor) return cursor;
  if (frame.event === "snapshot_complete") {
    const id = frame.id?.trim();
    if (id?.startsWith("console:")) return id;
  }
  return void 0;
}
function replayUnavailableFrame(error) {
  const replayError = error.replayError;
  return {
    id: `replay_unavailable:${Date.now()}`,
    event: "replay_unavailable",
    data: replayError || {
      message: error instanceof Error ? error.message : String(error)
    }
  };
}
function sleep(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}
function subscribeTimelineEvents(baseUrl, target, onFrame) {
  let stopped = false;
  let controller = null;
  let after = target.after?.trim() || void 0;
  void (async () => {
    let retryDelayMs = 250;
    while (!stopped) {
      controller = new AbortController();
      try {
        await streamFramesFromResponse(
          await fetch(`${baseUrl}${timelineStreamPath({ ...target, after })}`, {
            method: "GET",
            headers: { "content-type": "application/json" },
            signal: controller.signal
          }),
          {
            stopOnTerminal: false,
            onFrame: (frame) => {
              const nextCursor = cursorFromTimelineFrame(frame);
              if (nextCursor) {
                after = nextCursor;
              }
              onFrame(frame);
            }
          }
        );
        retryDelayMs = 250;
      } catch (error) {
        if (stopped || controller.signal.aborted) {
          break;
        }
        onFrame(replayUnavailableFrame(error));
      }
      if (!stopped) {
        await sleep(retryDelayMs);
        retryDelayMs = Math.min(retryDelayMs * 2, 2e3);
      }
    }
  })();
  return () => {
    stopped = true;
    controller?.abort();
  };
}

// src/icon.tsx
var import_jsx_runtime16 = require("react/jsx-runtime");
function SpriteSheet() {
  return /* @__PURE__ */ (0, import_jsx_runtime16.jsxs)("svg", { className: "sprite-root", width: "0", height: "0", style: { position: "absolute" }, "aria-hidden": "true", children: [
    /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("symbol", { id: "i-plus", viewBox: "0 0 24 24", children: /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("path", { d: "M12 5v14M5 12h14" }) }),
    /* @__PURE__ */ (0, import_jsx_runtime16.jsxs)("symbol", { id: "i-compose", viewBox: "0 0 24 24", children: [
      /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("path", { d: "m4 20 4.5-1 9.5-9.5-3.5-3.5L5 15.5 4 20z" }),
      /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("path", { d: "m13.5 4.5 3.5 3.5" }),
      /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("path", { d: "M9 19h11" })
    ] }),
    /* @__PURE__ */ (0, import_jsx_runtime16.jsxs)("symbol", { id: "i-new-thread", viewBox: "0 0 24 24", children: [
      /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("rect", { x: "4", y: "4", width: "16", height: "16", rx: "3" }),
      /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("path", { d: "m9 15 5.5-5.5 2 2L11 17H9v-2z" }),
      /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("path", { d: "m13 9 2 2" })
    ] }),
    /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("symbol", { id: "i-bolt", viewBox: "0 0 24 24", children: /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("path", { d: "M13 2 6 13h5l-1 9 8-12h-5z" }) }),
    /* @__PURE__ */ (0, import_jsx_runtime16.jsxs)("symbol", { id: "i-sliders", viewBox: "0 0 24 24", children: [
      /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("path", { d: "M4 6h16M4 12h16M4 18h16" }),
      /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("circle", { cx: "8", cy: "12", r: "2" })
    ] }),
    /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("symbol", { id: "i-folder", viewBox: "0 0 24 24", children: /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("path", { d: "M3 6h7l2 2h9v10a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V6z" }) }),
    /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("symbol", { id: "i-play", viewBox: "0 0 24 24", children: /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("path", { d: "m9 7 9 5-9 5z" }) }),
    /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("symbol", { id: "i-stop", viewBox: "0 0 24 24", children: /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("path", { d: "M8 8h8v8H8z" }) }),
    /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("symbol", { id: "i-chevron", viewBox: "0 0 24 24", children: /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("path", { d: "m7 10 5 5 5-5" }) }),
    /* @__PURE__ */ (0, import_jsx_runtime16.jsxs)("symbol", { id: "i-terminal", viewBox: "0 0 24 24", children: [
      /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("path", { d: "m4 6 7 6-7 6" }),
      /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("path", { d: "M13 18h7" })
    ] }),
    /* @__PURE__ */ (0, import_jsx_runtime16.jsxs)("symbol", { id: "i-team", viewBox: "0 0 24 24", children: [
      /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("circle", { cx: "9", cy: "9", r: "3" }),
      /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("circle", { cx: "17", cy: "10", r: "2.5" }),
      /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("path", { d: "M4 19a5 5 0 0 1 10 0" }),
      /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("path", { d: "M13.5 19a4 4 0 0 1 7 0" })
    ] }),
    /* @__PURE__ */ (0, import_jsx_runtime16.jsxs)("symbol", { id: "i-branch", viewBox: "0 0 24 24", children: [
      /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("path", { d: "M6 3v6a4 4 0 0 0 4 4h8" }),
      /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("path", { d: "M14 7h4v4" }),
      /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("circle", { cx: "6", cy: "3", r: "2" }),
      /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("circle", { cx: "6", cy: "15", r: "2" }),
      /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("circle", { cx: "18", cy: "13", r: "2" })
    ] }),
    /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("symbol", { id: "i-shield", viewBox: "0 0 24 24", children: /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("path", { d: "M12 3 4 6v6c0 5 3.5 8 8 9 4.5-1 8-4 8-9V6z" }) }),
    /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("symbol", { id: "i-dot", viewBox: "0 0 24 24", children: /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("circle", { cx: "12", cy: "12", r: "4" }) }),
    /* @__PURE__ */ (0, import_jsx_runtime16.jsxs)("symbol", { id: "i-clock", viewBox: "0 0 24 24", children: [
      /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("circle", { cx: "12", cy: "12", r: "9" }),
      /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("path", { d: "M12 7v6l4 2" })
    ] }),
    /* @__PURE__ */ (0, import_jsx_runtime16.jsxs)("symbol", { id: "i-cube", viewBox: "0 0 24 24", children: [
      /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("path", { d: "m12 3 8 4.5v9L12 21l-8-4.5v-9z" }),
      /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("path", { d: "m12 12 8-4.5" }),
      /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("path", { d: "m12 12-8-4.5" })
    ] }),
    /* @__PURE__ */ (0, import_jsx_runtime16.jsxs)("symbol", { id: "i-sidebar-toggle", viewBox: "0 0 24 24", children: [
      /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("rect", { x: "3", y: "5", width: "18", height: "14", rx: "2" }),
      /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("path", { d: "M9 5v14" }),
      /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("path", { d: "m14 12 3-3" }),
      /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("path", { d: "m14 12 3 3" })
    ] }),
    /* @__PURE__ */ (0, import_jsx_runtime16.jsxs)("symbol", { id: "i-open", viewBox: "0 0 24 24", children: [
      /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("path", { d: "M4 12V6a2 2 0 0 1 2-2h12" }),
      /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("path", { d: "M20 4v6h-6" }),
      /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("path", { d: "m20 4-9 9" }),
      /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("path", { d: "M20 14v4a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2v-4" })
    ] }),
    /* @__PURE__ */ (0, import_jsx_runtime16.jsxs)("symbol", { id: "i-swap", viewBox: "0 0 24 24", children: [
      /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("path", { d: "M15 7h6" }),
      /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("path", { d: "m18 4 3 3-3 3" }),
      /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("path", { d: "M9 17H3" }),
      /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("path", { d: "m6 14-3 3 3 3" }),
      /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("path", { d: "M21 7H9a4 4 0 0 0-4 4v6" })
    ] }),
    /* @__PURE__ */ (0, import_jsx_runtime16.jsxs)("symbol", { id: "i-copy", viewBox: "0 0 24 24", children: [
      /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("rect", { x: "9", y: "9", width: "11", height: "11", rx: "2" }),
      /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("rect", { x: "4", y: "4", width: "11", height: "11", rx: "2" })
    ] }),
    /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("symbol", { id: "i-check", viewBox: "0 0 24 24", children: /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("path", { d: "m5 12 4.2 4.2L19 6.5" }) }),
    /* @__PURE__ */ (0, import_jsx_runtime16.jsxs)("symbol", { id: "i-archive", viewBox: "0 0 24 24", children: [
      /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("path", { d: "M4 7h16" }),
      /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("path", { d: "M6 7v11a2 2 0 0 0 2 2h8a2 2 0 0 0 2-2V7" }),
      /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("path", { d: "M9 11h6" }),
      /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("path", { d: "M10 3h4l1 2H9l1-2z" })
    ] }),
    /* @__PURE__ */ (0, import_jsx_runtime16.jsxs)("symbol", { id: "i-square-plus", viewBox: "0 0 24 24", children: [
      /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("rect", { x: "3", y: "3", width: "18", height: "18", rx: "3" }),
      /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("path", { d: "M12 8v8M8 12h8" })
    ] }),
    /* @__PURE__ */ (0, import_jsx_runtime16.jsxs)("symbol", { id: "i-info", viewBox: "0 0 24 24", children: [
      /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("circle", { cx: "12", cy: "12", r: "9" }),
      /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("path", { d: "M12 10v6" }),
      /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("path", { d: "M12 7h.01" })
    ] }),
    /* @__PURE__ */ (0, import_jsx_runtime16.jsxs)("symbol", { id: "i-refresh", viewBox: "0 0 24 24", children: [
      /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("path", { d: "M21 12a9 9 0 0 1-15.4 6.4" }),
      /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("path", { d: "M3 12A9 9 0 0 1 18.4 5.6" }),
      /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("path", { d: "M3 16v-4h4" }),
      /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("path", { d: "M21 8v4h-4" })
    ] }),
    /* @__PURE__ */ (0, import_jsx_runtime16.jsxs)("symbol", { id: "i-mic", viewBox: "0 0 24 24", children: [
      /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("path", { d: "M12 3a3 3 0 0 1 3 3v6a3 3 0 0 1-6 0V6a3 3 0 0 1 3-3z" }),
      /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("path", { d: "M19 11a7 7 0 0 1-14 0" }),
      /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("path", { d: "M12 18v3" }),
      /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("path", { d: "M8 21h8" })
    ] }),
    /* @__PURE__ */ (0, import_jsx_runtime16.jsxs)("symbol", { id: "i-ellipsis", viewBox: "0 0 24 24", children: [
      /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("circle", { cx: "5", cy: "12", r: "2" }),
      /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("circle", { cx: "12", cy: "12", r: "2" }),
      /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("circle", { cx: "19", cy: "12", r: "2" })
    ] }),
    /* @__PURE__ */ (0, import_jsx_runtime16.jsxs)("symbol", { id: "i-gear", viewBox: "0 0 24 24", children: [
      /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("path", { d: "M12 8a4 4 0 1 1 0 8 4 4 0 0 1 0-8z" }),
      /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("path", { d: "M12 2v3M12 19v3M2 12h3M19 12h3M4.9 4.9l2.2 2.2M16.9 16.9l2.2 2.2M19.1 4.9l-2.2 2.2M7.1 16.9l-2.2 2.2" })
    ] }),
    /* @__PURE__ */ (0, import_jsx_runtime16.jsxs)("symbol", { id: "i-search", viewBox: "0 0 24 24", children: [
      /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("circle", { cx: "11", cy: "11", r: "6" }),
      /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("path", { d: "m20 20-4.35-4.35" })
    ] }),
    /* @__PURE__ */ (0, import_jsx_runtime16.jsxs)("symbol", { id: "i-pin", viewBox: "0 0 24 24", children: [
      /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("path", { d: "m14 4 6 6" }),
      /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("path", { d: "M11 7l6 6" }),
      /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("path", { d: "m8 10 6 6" }),
      /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("path", { d: "M6 12l6 6" }),
      /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("path", { d: "m11 13-7 7" })
    ] }),
    /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("symbol", { id: "i-star", viewBox: "0 0 24 24", children: /* @__PURE__ */ (0, import_jsx_runtime16.jsx)("path", { d: "m12 3 2.8 5.7 6.2.9-4.5 4.4 1.1 6.2L12 17.2 6.4 20.2l1.1-6.2L3 9.6l6.2-.9L12 3z" }) })
  ] });
}

// src/panels/TopologyPanel.tsx
var import_react12 = __toESM(require("react"));

// src/panels/topology/ForceDirected.tsx
var import_react9 = __toESM(require("react"));

// src/panels/topology/data.ts
var import_react7 = __toESM(require("react"));
var PEER_TOOL_NAMES2 = /* @__PURE__ */ new Set(["send_request", "send_message", "send_response"]);
var ROLE_ORDER_HINT = [
  "user",
  "personal",
  "coordinator",
  "scribe",
  "review",
  "summarizer",
  "initiative",
  "channel",
  "responder",
  "domain",
  "internal",
  "approval",
  "monitor"
];
var ROLE_PALETTE = [
  "var(--focus)",
  "var(--accent)",
  "var(--ok)",
  "var(--warn)",
  "var(--c-init)",
  "var(--ink-muted)"
];
function colourForRole(role, roleIndex) {
  const idx = roleIndex[role] ?? 0;
  return ROLE_PALETTE[idx % ROLE_PALETTE.length];
}
function roleSortKey(role) {
  const idx = ROLE_ORDER_HINT.findIndex((hint) => role.toLowerCase().includes(hint));
  return idx === -1 ? ROLE_ORDER_HINT.length : idx;
}
function buildGraph(nodes, agents) {
  const source = nodes.length > 0 ? nodes : agents.map((a) => ({
    identity: a.identity || a.member_id,
    label: a.label,
    role: a.role,
    state: a.state,
    wired_to: a.wired_to
  }));
  const byId = /* @__PURE__ */ new Map();
  const list = [];
  for (const n of source) {
    const id = (n.identity || n.label || "").trim();
    if (!id || byId.has(id)) continue;
    const agent = {
      id,
      label: (n.label || id).trim(),
      role: (n.role || "agent").trim(),
      state: (n.state || "").toLowerCase(),
      wiredTo: (n.wired_to || []).map((s) => s.trim()).filter(Boolean)
    };
    byId.set(id, agent);
    list.push(agent);
  }
  const seen = /* @__PURE__ */ new Set();
  const edges = [];
  for (const a of list) {
    for (const t of a.wiredTo) {
      if (!byId.has(t) || t === a.id) continue;
      const key = a.id < t ? `${a.id}|${t}` : `${t}|${a.id}`;
      if (seen.has(key)) continue;
      seen.add(key);
      edges.push({ from: a.id, to: t });
    }
  }
  const degree = {};
  for (const e of edges) {
    degree[e.from] = (degree[e.from] || 0) + 1;
    degree[e.to] = (degree[e.to] || 0) + 1;
  }
  const roles = Array.from(new Set(list.map((a) => a.role))).sort((a, b) => {
    const ra = roleSortKey(a);
    const rb = roleSortKey(b);
    if (ra !== rb) return ra - rb;
    return a.localeCompare(b);
  });
  return { agents: list, byId, edges, degree, roles };
}
function roleIndexFor(roles) {
  const idx = {};
  roles.forEach((r2, i) => {
    idx[r2] = i;
  });
  return idx;
}
function useTopologyActivity(frames, graph, options = {}) {
  const life = options.life ?? 1500;
  const [now, setNow] = import_react7.default.useState(() => Date.now());
  const ticking = import_react7.default.useRef(false);
  import_react7.default.useEffect(() => {
    if (ticking.current) return;
    let raf = 0;
    let stopped = false;
    ticking.current = true;
    const step = () => {
      if (stopped) return;
      setNow(Date.now());
      raf = requestAnimationFrame(step);
    };
    raf = requestAnimationFrame(step);
    return () => {
      stopped = true;
      ticking.current = false;
      cancelAnimationFrame(raf);
    };
  }, []);
  return import_react7.default.useMemo(() => {
    const active = {};
    const pulses = [];
    const peerRegistry = /* @__PURE__ */ new Map();
    const busy = {};
    const ordered = frames.slice().reverse();
    for (const frame of ordered) {
      const ts = frame.timestampMs || 0;
      if (!ts) continue;
      const identity = frame.identity?.trim();
      if (identity && graph.byId.has(identity)) {
        if ((active[identity] || 0) < ts) active[identity] = ts;
        if (frame.event === "interaction_started" || frame.event === "run_started") {
          busy[identity] = true;
        } else if (frame.event === "interaction_complete" || frame.event === "interaction_failed" || frame.event === "run_completed" || frame.event === "run_failed" || frame.event === "run_canceled") {
          busy[identity] = false;
        }
      }
      const data = frame.data;
      const name = data && typeof data.name === "string" ? data.name : "";
      if (name === "peers" && (frame.event === "tool_execution_completed" || frame.event === "tool_result_received")) {
        const raw = typeof data?.result === "string" ? data.result : null;
        if (raw) {
          try {
            const parsed = JSON.parse(raw);
            for (const p of parsed.peers || []) {
              if (typeof p.peer_id === "string" && typeof p.name === "string") {
                const lastSeg = p.name.split("/").pop() || p.name;
                peerRegistry.set(p.peer_id, lastSeg);
              }
            }
          } catch {
          }
        }
      }
      if (PEER_TOOL_NAMES2.has(name) && (frame.event === "tool_call_requested" || frame.event === "tool_call" || frame.event === "tool_execution_started") && identity && graph.byId.has(identity)) {
        const args = data && typeof data.args === "object" ? data.args : null;
        const peerId = typeof args?.peer_id === "string" ? args.peer_id : null;
        const recipient = peerId ? peerRegistry.get(peerId) : null;
        if (recipient && graph.byId.has(recipient) && recipient !== identity) {
          pulses.push({
            id: typeof data?.id === "string" ? data.id : `${frame.id || ts}-${pulses.length}`,
            from: identity,
            to: recipient,
            ts
          });
        }
      }
    }
    const cutoff = now - life;
    for (const [k, v] of Object.entries(active)) {
      if (v < cutoff) delete active[k];
    }
    const live = pulses.filter((p) => p.ts >= cutoff);
    return { active, pulses: live, busy };
  }, [frames, graph, life, now]);
}
function edgeKey(a, b) {
  return a < b ? `${a}|${b}` : `${b}|${a}`;
}

// src/panels/topology/zoom-pan.ts
var import_react8 = __toESM(require("react"));
var MIN_SCALE = 0.4;
var MAX_SCALE = 6;
function clientToViewBox(el, clientX, clientY, viewBoxW, viewBoxH) {
  const rect = el.getBoundingClientRect();
  const renderScale = Math.min(rect.width / viewBoxW, rect.height / viewBoxH);
  const contentW = viewBoxW * renderScale;
  const contentH = viewBoxH * renderScale;
  const offsetX = rect.left + (rect.width - contentW) / 2;
  const offsetY = rect.top + (rect.height - contentH) / 2;
  return {
    x: (clientX - offsetX) / renderScale,
    y: (clientY - offsetY) / renderScale
  };
}
function useZoomPan(width, height) {
  const [viewport, setViewport] = import_react8.default.useState({ tx: 0, ty: 0, scale: 1 });
  const dragRef = import_react8.default.useRef(null);
  const [isDragging, setIsDragging] = import_react8.default.useState(false);
  const svgRef = import_react8.default.useRef(null);
  const reset = import_react8.default.useCallback(() => {
    setViewport({ tx: 0, ty: 0, scale: 1 });
  }, []);
  import_react8.default.useEffect(() => {
    const el = svgRef.current;
    if (!el) return;
    const handler = (e) => {
      e.preventDefault();
      const { x: cx, y: cy } = clientToViewBox(el, e.clientX, e.clientY, width, height);
      setViewport((prev) => {
        const factor = e.deltaY < 0 ? 1.12 : 1 / 1.12;
        const nextScale = Math.max(MIN_SCALE, Math.min(MAX_SCALE, prev.scale * factor));
        if (nextScale === prev.scale) return prev;
        const wx = (cx - prev.tx) / prev.scale;
        const wy = (cy - prev.ty) / prev.scale;
        return {
          scale: nextScale,
          tx: cx - wx * nextScale,
          ty: cy - wy * nextScale
        };
      });
    };
    el.addEventListener("wheel", handler, { passive: false });
    return () => el.removeEventListener("wheel", handler);
  }, [width, height]);
  const onPointerDown = import_react8.default.useCallback((e) => {
    if (e.button !== 0) return;
    const tag = e.target?.tagName?.toLowerCase();
    if (tag !== "svg" && tag !== "g" && tag !== "rect" && tag !== "line") return;
    e.currentTarget.setPointerCapture(e.pointerId);
    dragRef.current = { pointerId: e.pointerId, lastX: e.clientX, lastY: e.clientY };
    setIsDragging(true);
  }, []);
  const onPointerMove = import_react8.default.useCallback((e) => {
    const drag = dragRef.current;
    if (!drag || drag.pointerId !== e.pointerId) return;
    const dx = e.clientX - drag.lastX;
    const dy = e.clientY - drag.lastY;
    drag.lastX = e.clientX;
    drag.lastY = e.clientY;
    const rect = e.currentTarget.getBoundingClientRect();
    const renderScale = Math.min(rect.width / width, rect.height / height);
    const vbDx = dx / renderScale;
    const vbDy = dy / renderScale;
    setViewport((prev) => ({ ...prev, tx: prev.tx + vbDx, ty: prev.ty + vbDy }));
  }, [width, height]);
  const onPointerUp = import_react8.default.useCallback((e) => {
    const drag = dragRef.current;
    if (!drag || drag.pointerId !== e.pointerId) return;
    e.currentTarget.releasePointerCapture(e.pointerId);
    dragRef.current = null;
    setIsDragging(false);
  }, []);
  return { viewport, reset, svgRef, onPointerDown, onPointerMove, onPointerUp, isDragging };
}
function viewportTransform(v) {
  return `translate(${v.tx} ${v.ty}) scale(${v.scale})`;
}

// src/panels/topology/ForceDirected.tsx
var import_jsx_runtime17 = require("react/jsx-runtime");
function visualScale(N) {
  if (N <= 20) return { nodeMin: 5, nodeMax: 12, edgeWidth: 1, idealEdgeLen: 110 };
  if (N <= 80) return { nodeMin: 3.5, nodeMax: 9, edgeWidth: 0.7, idealEdgeLen: 80 };
  return { nodeMin: 2.4, nodeMax: 7, edgeWidth: 0.5, idealEdgeLen: 60 };
}
function resolveLabelMode(N, mode) {
  if (mode === "off") return "hover";
  if (mode === "on") return N <= 60 ? "on" : "hover";
  return N <= 20 ? "on" : "hover";
}
function ForceDirected({
  nodes,
  agents,
  activity,
  width,
  height,
  labelsMode = "auto"
}) {
  const graph = import_react9.default.useMemo(() => buildGraph(nodes, agents), [nodes, agents]);
  const roleIndex = import_react9.default.useMemo(() => roleIndexFor(graph.roles), [graph.roles]);
  const liveActivity = useTopologyActivity(activity, graph, { life: 900 });
  const scale = visualScale(graph.agents.length);
  const labelMode = resolveLabelMode(graph.agents.length, labelsMode);
  const [hoverId, setHoverId] = import_react9.default.useState(null);
  const zoom = useZoomPan(width, height);
  const simRef = import_react9.default.useRef(null);
  const [, setTick] = import_react9.default.useState(0);
  const fingerprint = import_react9.default.useMemo(
    () => `${graph.agents.map((a) => a.id).join(",")}|${graph.edges.map((e) => `${e.from}-${e.to}`).join(",")}|${width}x${height}`,
    [graph, width, height]
  );
  const showLabelsInSim = labelMode === "on";
  const labelWidthCache = import_react9.default.useMemo(() => {
    const m = /* @__PURE__ */ new Map();
    for (const a of graph.agents) {
      const chars = (a.label || a.id).length;
      m.set(a.id, Math.min(140, Math.max(40, chars * 6.5)));
    }
    return m;
  }, [graph.agents]);
  import_react9.default.useEffect(() => {
    const N = graph.agents.length;
    if (N === 0) {
      simRef.current = null;
      return;
    }
    const seeded = graph.agents.map((a, i) => {
      const t = i / Math.max(1, N) * Math.PI * 2;
      return {
        id: a.id,
        x: width / 2 + Math.cos(t) * (50 + i * 13 % 80),
        y: height / 2 + Math.sin(t) * (50 + i * 7 % 80),
        vx: 0,
        vy: 0
      };
    });
    const byId = /* @__PURE__ */ new Map();
    seeded.forEach((n) => byId.set(n.id, n));
    simRef.current = { nodes: seeded, byId, alpha: 1, frame: 0 };
    let raf = 0;
    let stopped = false;
    const step = () => {
      if (stopped) return;
      const sim2 = simRef.current;
      if (!sim2) return;
      const cx = width / 2;
      const cy = height / 2;
      const REP = Math.max(220, 7e4 / sim2.nodes.length);
      for (let i = 0; i < sim2.nodes.length; i++) {
        const ni = sim2.nodes[i];
        for (let j = i + 1; j < sim2.nodes.length; j++) {
          const nj = sim2.nodes[j];
          const dx = ni.x - nj.x;
          const dy = ni.y - nj.y;
          const d2 = dx * dx + dy * dy + 0.01;
          const f = REP / d2;
          const d = Math.sqrt(d2);
          const ux = dx / d;
          const uy = dy / d;
          ni.vx += ux * f;
          ni.vy += uy * f;
          nj.vx -= ux * f;
          nj.vy -= uy * f;
        }
      }
      for (const e of graph.edges) {
        const a = sim2.byId.get(e.from);
        const b = sim2.byId.get(e.to);
        if (!a || !b) continue;
        const dx = b.x - a.x;
        const dy = b.y - a.y;
        const d = Math.sqrt(dx * dx + dy * dy) || 0.01;
        const f = 0.025 * (d - scale.idealEdgeLen);
        const ux = dx / d;
        const uy = dy / d;
        a.vx += ux * f;
        a.vy += uy * f;
        b.vx -= ux * f;
        b.vy -= uy * f;
      }
      if (showLabelsInSim && sim2.alpha > 0.12) {
        for (const a of sim2.nodes) {
          const labelW = labelWidthCache.get(a.id) ?? 60;
          const halfW = labelW / 2;
          const labelTop = a.y + scale.nodeMax + 4;
          const labelBot = labelTop + 14;
          for (const b of sim2.nodes) {
            if (a === b) continue;
            const overlapX = b.x > a.x - halfW - scale.nodeMax && b.x < a.x + halfW + scale.nodeMax;
            const overlapY = b.y > labelTop - scale.nodeMax && b.y < labelBot + scale.nodeMax;
            if (!overlapX || !overlapY) continue;
            const dx = (b.x - a.x) * 0.04;
            const dy = (b.y - (labelTop + labelBot) / 2) * 0.18;
            b.vx += dx;
            b.vy += dy;
            a.vx -= dx * 0.5;
            a.vy -= dy * 0.5;
          }
        }
      }
      for (const n of sim2.nodes) {
        n.vx += (cx - n.x) * 35e-4;
        n.vy += (cy - n.y) * 35e-4;
        n.vx *= 0.78;
        n.vy *= 0.78;
        n.x += n.vx * sim2.alpha;
        n.y += n.vy * sim2.alpha;
        const margin = 18;
        n.x = Math.max(margin, Math.min(width - margin, n.x));
        n.y = Math.max(margin, Math.min(height - margin, n.y));
      }
      sim2.alpha = Math.max(0.04, sim2.alpha * 0.992);
      sim2.frame++;
      if (sim2.frame % 2 === 0) setTick((t) => t + 1);
      raf = requestAnimationFrame(step);
    };
    raf = requestAnimationFrame(step);
    return () => {
      stopped = true;
      cancelAnimationFrame(raf);
    };
  }, [fingerprint]);
  const sim = simRef.current;
  const hotEdges = import_react9.default.useMemo(() => {
    const set = /* @__PURE__ */ new Set();
    for (const p of liveActivity.pulses) set.add(edgeKey(p.from, p.to));
    return set;
  }, [liveActivity.pulses]);
  const renderItems = sim ? graph.agents.flatMap((agent) => {
    const n = sim.byId.get(agent.id);
    if (!n) return [];
    const deg = graph.degree[agent.id] || 0;
    const t = Math.sqrt(deg) / 4;
    const r2 = scale.nodeMin + Math.min(1, t) * (scale.nodeMax - scale.nodeMin);
    return [{
      agent,
      n,
      r: r2,
      isHot: !!liveActivity.active[agent.id],
      isBusy: !!liveActivity.busy[agent.id],
      colour: colourForRole(agent.role, roleIndex)
    }];
  }) : [];
  const showInlineLabel = labelMode === "on";
  const hoveredItem = hoverId ? renderItems.find((it) => it.agent.id === hoverId) : null;
  return /* @__PURE__ */ (0, import_jsx_runtime17.jsxs)("div", { className: "topo__board", children: [
    /* @__PURE__ */ (0, import_jsx_runtime17.jsxs)("div", { className: "topo__zoombar", "data-testid": "topology-zoombar", children: [
      /* @__PURE__ */ (0, import_jsx_runtime17.jsxs)("span", { className: "topo__zoom-pct", children: [
        Math.round(zoom.viewport.scale * 100),
        "%"
      ] }),
      /* @__PURE__ */ (0, import_jsx_runtime17.jsx)(
        "button",
        {
          type: "button",
          className: "topo__zoom-reset",
          onClick: zoom.reset,
          title: "Reset zoom and pan",
          "data-testid": "topology-zoom-reset",
          children: "Reset"
        }
      )
    ] }),
    /* @__PURE__ */ (0, import_jsx_runtime17.jsx)(
      "svg",
      {
        ref: zoom.svgRef,
        className: `topo__svg-board${zoom.isDragging ? " is-panning" : ""}`,
        viewBox: `0 0 ${width} ${height}`,
        preserveAspectRatio: "xMidYMid meet",
        onPointerDown: zoom.onPointerDown,
        onPointerMove: zoom.onPointerMove,
        onPointerUp: zoom.onPointerUp,
        onPointerCancel: zoom.onPointerUp,
        children: sim && /* @__PURE__ */ (0, import_jsx_runtime17.jsxs)("g", { transform: viewportTransform(zoom.viewport), children: [
          /* @__PURE__ */ (0, import_jsx_runtime17.jsx)("g", { children: graph.edges.map((e, i) => {
            const a = sim.byId.get(e.from);
            const b = sim.byId.get(e.to);
            if (!a || !b) return null;
            const hot = hotEdges.has(edgeKey(e.from, e.to));
            return /* @__PURE__ */ (0, import_jsx_runtime17.jsx)(
              "line",
              {
                x1: a.x,
                y1: a.y,
                x2: b.x,
                y2: b.y,
                stroke: hot ? "var(--ok)" : "var(--ink-faint)",
                strokeWidth: hot ? scale.edgeWidth + 0.5 : scale.edgeWidth,
                opacity: hot ? 0.85 : 0.5
              },
              i
            );
          }) }),
          /* @__PURE__ */ (0, import_jsx_runtime17.jsx)("g", { children: liveActivity.pulses.map((p) => {
            const a = sim.byId.get(p.from);
            const b = sim.byId.get(p.to);
            if (!a || !b) return null;
            const age = (Date.now() - p.ts) / 900;
            if (age > 1) return null;
            const x = a.x + (b.x - a.x) * age;
            const y = a.y + (b.y - a.y) * age;
            return /* @__PURE__ */ (0, import_jsx_runtime17.jsx)(
              "circle",
              {
                cx: x,
                cy: y,
                r: 3,
                fill: "var(--ok)",
                opacity: 1 - age,
                style: { pointerEvents: "none" }
              },
              p.id
            );
          }) }),
          /* @__PURE__ */ (0, import_jsx_runtime17.jsx)("g", { children: renderItems.map(({ agent, n, r: r2, isHot, isBusy, colour }) => /* @__PURE__ */ (0, import_jsx_runtime17.jsxs)(
            "g",
            {
              "data-testid": `topology-node:${agent.id}`,
              className: `topo__node${isBusy ? " is-busy" : ""}${isHot ? " is-hot" : ""}`,
              onMouseEnter: () => setHoverId(agent.id),
              onMouseLeave: () => setHoverId((cur) => cur === agent.id ? null : cur),
              onFocus: () => setHoverId(agent.id),
              onBlur: () => setHoverId((cur) => cur === agent.id ? null : cur),
              tabIndex: 0,
              children: [
                isBusy && /* @__PURE__ */ (0, import_jsx_runtime17.jsx)(
                  "circle",
                  {
                    className: "topo__busy-ring",
                    cx: n.x,
                    cy: n.y,
                    r: r2 + 6,
                    fill: "none",
                    stroke: "var(--accent)",
                    strokeWidth: "1.5",
                    style: { pointerEvents: "none" }
                  }
                ),
                isHot && /* @__PURE__ */ (0, import_jsx_runtime17.jsx)(
                  "circle",
                  {
                    cx: n.x,
                    cy: n.y,
                    r: r2 + 4,
                    fill: "none",
                    stroke: colour,
                    strokeWidth: "1",
                    opacity: "0.35",
                    style: { pointerEvents: "none" }
                  }
                ),
                /* @__PURE__ */ (0, import_jsx_runtime17.jsx)(
                  "circle",
                  {
                    cx: n.x,
                    cy: n.y,
                    r: r2,
                    fill: colour,
                    stroke: "var(--bg)",
                    strokeWidth: "1.5"
                  }
                )
              ]
            },
            agent.id
          )) }),
          showInlineLabel && /* @__PURE__ */ (0, import_jsx_runtime17.jsx)("g", { style: { pointerEvents: "none" }, children: renderItems.map(({ agent, n, r: r2 }) => /* @__PURE__ */ (0, import_jsx_runtime17.jsx)(
            "text",
            {
              x: n.x,
              y: n.y + r2 + 12,
              textAnchor: "middle",
              className: "topo__node-label",
              children: agent.label
            },
            agent.id
          )) }),
          !showInlineLabel && hoveredItem && /* @__PURE__ */ (0, import_jsx_runtime17.jsx)(
            NodeLabelPill,
            {
              x: hoveredItem.n.x,
              y: hoveredItem.n.y + hoveredItem.r + 8,
              text: hoveredItem.agent.label,
              sub: `${hoveredItem.agent.role}${hoveredItem.agent.state ? " \xB7 " + hoveredItem.agent.state : ""}${hoveredItem.isBusy ? " \xB7 working" : ""}`
            }
          )
        ] })
      }
    )
  ] });
}
function NodeLabelPill({ x, y, text, sub }) {
  const W2 = 180;
  const H2 = sub ? 32 : 18;
  return /* @__PURE__ */ (0, import_jsx_runtime17.jsx)(
    "foreignObject",
    {
      x: x - W2 / 2,
      y,
      width: W2,
      height: H2,
      style: { pointerEvents: "none", overflow: "visible" },
      children: /* @__PURE__ */ (0, import_jsx_runtime17.jsxs)("div", { className: "topo__node-pill", role: "tooltip", children: [
        /* @__PURE__ */ (0, import_jsx_runtime17.jsx)("span", { className: "topo__node-pill-label", children: text }),
        sub && /* @__PURE__ */ (0, import_jsx_runtime17.jsx)("span", { className: "topo__node-pill-sub", children: sub })
      ] })
    }
  );
}

// src/panels/topology/Bullseye.tsx
var import_react10 = __toESM(require("react"));
var import_jsx_runtime18 = require("react/jsx-runtime");
var RINGS = 5;
function visualScale2(N) {
  if (N <= 20) return { nodeMin: 5, nodeMax: 12, edgeWidth: 1 };
  if (N <= 80) return { nodeMin: 3.5, nodeMax: 9, edgeWidth: 0.7 };
  return { nodeMin: 2.4, nodeMax: 7, edgeWidth: 0.5 };
}
function resolveLabelMode2(N, mode) {
  if (mode === "off") return "hover";
  if (mode === "on") return N <= 60 ? "on" : "hover";
  return N <= 20 ? "on" : "hover";
}
function layout(graph, width, height) {
  const cx = width / 2;
  const cy = height / 2;
  const sorted = graph.agents.slice().sort(
    (a, b) => (graph.degree[b.id] || 0) - (graph.degree[a.id] || 0)
  );
  const maxDeg = sorted.length ? graph.degree[sorted[0].id] || 1 : 1;
  const buckets = Array.from({ length: RINGS }, () => []);
  for (const a of sorted) {
    const d = graph.degree[a.id] || 0;
    const t = d / Math.max(1, maxDeg);
    const ringIdx = Math.min(RINGS - 1, Math.floor((1 - Math.pow(t, 0.6)) * RINGS));
    buckets[ringIdx].push(a);
  }
  const minR = Math.min(width, height) * 0.1;
  const maxR = Math.min(width, height) * 0.44;
  const ringR = (i) => minR + i / Math.max(1, RINGS - 1) * (maxR - minR);
  const pos = {};
  buckets.forEach((list, ri) => {
    list.sort((a, b) => a.role.localeCompare(b.role) || a.id.localeCompare(b.id));
    const r2 = ringR(ri);
    list.forEach((a, i) => {
      const offset = ri / RINGS * (Math.PI / 6);
      const t = i / Math.max(1, list.length) * Math.PI * 2 - Math.PI / 2 + offset;
      pos[a.id] = { x: cx + Math.cos(t) * r2, y: cy + Math.sin(t) * r2, ringIdx: ri };
    });
  });
  return { pos, ringR, cx, cy, buckets };
}
function Bullseye({
  nodes,
  agents,
  activity,
  width,
  height,
  labelsMode = "auto"
}) {
  const graph = import_react10.default.useMemo(() => buildGraph(nodes, agents), [nodes, agents]);
  const roleIndex = import_react10.default.useMemo(() => roleIndexFor(graph.roles), [graph.roles]);
  const live = useTopologyActivity(activity, graph, { life: 1100 });
  const scale = visualScale2(graph.agents.length);
  const labelMode = resolveLabelMode2(graph.agents.length, labelsMode);
  const [hoverId, setHoverId] = import_react10.default.useState(null);
  const zoom = useZoomPan(width, height);
  const { pos, ringR, cx, cy, buckets } = import_react10.default.useMemo(
    () => layout(graph, width, height),
    [graph, width, height]
  );
  const hotEdges = import_react10.default.useMemo(() => {
    const set = /* @__PURE__ */ new Set();
    for (const p of live.pulses) set.add(edgeKey(p.from, p.to));
    return set;
  }, [live.pulses]);
  const radiusOf = (deg) => {
    const t = Math.sqrt(deg) / 4;
    return scale.nodeMin + Math.min(1, t) * (scale.nodeMax - scale.nodeMin);
  };
  const innerR = ringR(0);
  const renderItems = graph.agents.flatMap((agent) => {
    const p = pos[agent.id];
    if (!p) return [];
    const deg = graph.degree[agent.id] || 0;
    const r2 = radiusOf(deg);
    const dx = p.x - cx;
    const dy = p.y - cy;
    const d = Math.hypot(dx, dy) || 1;
    const ux = dx / d;
    const uy = dy / d;
    const labelX = p.x + ux * (r2 + 8);
    const labelY = p.y + uy * (r2 + 8);
    const anchor = ux > 0.25 ? "start" : ux < -0.25 ? "end" : "middle";
    return [{
      agent,
      p,
      r: r2,
      labelX,
      labelY,
      anchor,
      isHot: !!live.active[agent.id],
      isBusy: !!live.busy[agent.id],
      colour: colourForRole(agent.role, roleIndex)
    }];
  });
  const showInlineLabel = labelMode === "on";
  const hoveredItem = hoverId ? renderItems.find((it) => it.agent.id === hoverId) : null;
  return /* @__PURE__ */ (0, import_jsx_runtime18.jsxs)("div", { className: "topo__board", children: [
    /* @__PURE__ */ (0, import_jsx_runtime18.jsxs)("div", { className: "topo__zoombar", "data-testid": "topology-zoombar", children: [
      /* @__PURE__ */ (0, import_jsx_runtime18.jsxs)("span", { className: "topo__zoom-pct", children: [
        Math.round(zoom.viewport.scale * 100),
        "%"
      ] }),
      /* @__PURE__ */ (0, import_jsx_runtime18.jsx)(
        "button",
        {
          type: "button",
          className: "topo__zoom-reset",
          onClick: zoom.reset,
          title: "Reset zoom and pan",
          "data-testid": "topology-zoom-reset",
          children: "Reset"
        }
      )
    ] }),
    /* @__PURE__ */ (0, import_jsx_runtime18.jsx)(
      "svg",
      {
        ref: zoom.svgRef,
        className: `topo__svg-board${zoom.isDragging ? " is-panning" : ""}`,
        viewBox: `0 0 ${width} ${height}`,
        preserveAspectRatio: "xMidYMid meet",
        onPointerDown: zoom.onPointerDown,
        onPointerMove: zoom.onPointerMove,
        onPointerUp: zoom.onPointerUp,
        onPointerCancel: zoom.onPointerUp,
        children: /* @__PURE__ */ (0, import_jsx_runtime18.jsxs)("g", { transform: viewportTransform(zoom.viewport), children: [
          /* @__PURE__ */ (0, import_jsx_runtime18.jsx)("g", { children: Array.from({ length: RINGS }).map((_, i) => /* @__PURE__ */ (0, import_jsx_runtime18.jsx)(
            "circle",
            {
              cx,
              cy,
              r: ringR(i),
              fill: "none",
              stroke: "var(--line-strong)",
              strokeWidth: "1",
              strokeDasharray: i % 2 === 0 ? void 0 : "3 5",
              opacity: i === 0 ? 0.6 : 0.35
            },
            i
          )) }),
          /* @__PURE__ */ (0, import_jsx_runtime18.jsx)("g", { children: buckets.map((list, i) => {
            if (list.length === 0) return null;
            const label = i === 0 ? "hubs" : i === RINGS - 1 ? "leaves" : `r${i}`;
            return /* @__PURE__ */ (0, import_jsx_runtime18.jsxs)(
              "text",
              {
                x: cx + ringR(i) + 6,
                y: cy + 3,
                textAnchor: "start",
                className: "topo__ring-label",
                children: [
                  label,
                  " \xB7 ",
                  list.length
                ]
              },
              i
            );
          }) }),
          /* @__PURE__ */ (0, import_jsx_runtime18.jsxs)("g", { children: [
            /* @__PURE__ */ (0, import_jsx_runtime18.jsx)(
              "circle",
              {
                cx,
                cy,
                r: Math.max(14, innerR - 14),
                fill: "var(--bg)",
                stroke: "var(--line)",
                strokeWidth: "1"
              }
            ),
            /* @__PURE__ */ (0, import_jsx_runtime18.jsx)(
              "text",
              {
                x: cx,
                y: cy + 4,
                textAnchor: "middle",
                className: "topo__center-label",
                children: graph.agents.length
              }
            )
          ] }),
          /* @__PURE__ */ (0, import_jsx_runtime18.jsx)("g", { children: graph.edges.map((e, i) => {
            const a = pos[e.from];
            const b = pos[e.to];
            if (!a || !b) return null;
            const hot = hotEdges.has(edgeKey(e.from, e.to));
            return /* @__PURE__ */ (0, import_jsx_runtime18.jsx)(
              "line",
              {
                x1: a.x,
                y1: a.y,
                x2: b.x,
                y2: b.y,
                stroke: hot ? "var(--ok)" : "var(--ink-faint)",
                strokeWidth: hot ? scale.edgeWidth + 0.5 : scale.edgeWidth,
                opacity: hot ? 0.85 : 0.5
              },
              i
            );
          }) }),
          /* @__PURE__ */ (0, import_jsx_runtime18.jsx)("g", { children: live.pulses.map((p) => {
            const a = pos[p.from];
            const b = pos[p.to];
            if (!a || !b) return null;
            const age = (Date.now() - p.ts) / 1100;
            if (age > 1) return null;
            const x = a.x + (b.x - a.x) * age;
            const y = a.y + (b.y - a.y) * age;
            return /* @__PURE__ */ (0, import_jsx_runtime18.jsx)(
              "circle",
              {
                cx: x,
                cy: y,
                r: 3,
                fill: "var(--ok)",
                opacity: 1 - age,
                style: { pointerEvents: "none" }
              },
              p.id
            );
          }) }),
          /* @__PURE__ */ (0, import_jsx_runtime18.jsx)("g", { children: renderItems.map(({ agent, p, r: r2, isHot, isBusy, colour }) => /* @__PURE__ */ (0, import_jsx_runtime18.jsxs)(
            "g",
            {
              "data-testid": `topology-node:${agent.id}`,
              className: `topo__node${isBusy ? " is-busy" : ""}${isHot ? " is-hot" : ""}`,
              onMouseEnter: () => setHoverId(agent.id),
              onMouseLeave: () => setHoverId((cur) => cur === agent.id ? null : cur),
              onFocus: () => setHoverId(agent.id),
              onBlur: () => setHoverId((cur) => cur === agent.id ? null : cur),
              tabIndex: 0,
              children: [
                isBusy && /* @__PURE__ */ (0, import_jsx_runtime18.jsx)(
                  "circle",
                  {
                    className: "topo__busy-ring",
                    cx: p.x,
                    cy: p.y,
                    r: r2 + 6,
                    fill: "none",
                    stroke: "var(--accent)",
                    strokeWidth: "1.5",
                    style: { pointerEvents: "none" }
                  }
                ),
                isHot && /* @__PURE__ */ (0, import_jsx_runtime18.jsx)(
                  "circle",
                  {
                    cx: p.x,
                    cy: p.y,
                    r: r2 + 4,
                    fill: "none",
                    stroke: colour,
                    strokeWidth: "1",
                    opacity: "0.35",
                    style: { pointerEvents: "none" }
                  }
                ),
                /* @__PURE__ */ (0, import_jsx_runtime18.jsx)(
                  "circle",
                  {
                    cx: p.x,
                    cy: p.y,
                    r: r2,
                    fill: colour,
                    stroke: "var(--bg)",
                    strokeWidth: "1.5"
                  }
                )
              ]
            },
            agent.id
          )) }),
          showInlineLabel && /* @__PURE__ */ (0, import_jsx_runtime18.jsx)("g", { style: { pointerEvents: "none" }, children: renderItems.map(({ agent, labelX, labelY, anchor }) => /* @__PURE__ */ (0, import_jsx_runtime18.jsx)(
            "text",
            {
              x: labelX,
              y: labelY + 4,
              textAnchor: anchor,
              className: "topo__node-label",
              children: agent.label
            },
            agent.id
          )) }),
          !showInlineLabel && hoveredItem && /* @__PURE__ */ (0, import_jsx_runtime18.jsx)(
            BullseyeLabelPill,
            {
              x: hoveredItem.p.x,
              y: hoveredItem.p.y + hoveredItem.r + 8,
              text: hoveredItem.agent.label,
              sub: `${hoveredItem.agent.role}${hoveredItem.agent.state ? " \xB7 " + hoveredItem.agent.state : ""}${hoveredItem.isBusy ? " \xB7 working" : ""}`
            }
          )
        ] })
      }
    )
  ] });
}
function BullseyeLabelPill({ x, y, text, sub }) {
  const W2 = 180;
  const H2 = sub ? 32 : 18;
  return /* @__PURE__ */ (0, import_jsx_runtime18.jsx)(
    "foreignObject",
    {
      x: x - W2 / 2,
      y,
      width: W2,
      height: H2,
      style: { pointerEvents: "none", overflow: "visible" },
      children: /* @__PURE__ */ (0, import_jsx_runtime18.jsxs)("div", { className: "topo__node-pill", role: "tooltip", children: [
        /* @__PURE__ */ (0, import_jsx_runtime18.jsx)("span", { className: "topo__node-pill-label", children: text }),
        sub && /* @__PURE__ */ (0, import_jsx_runtime18.jsx)("span", { className: "topo__node-pill-sub", children: sub })
      ] })
    }
  );
}

// src/panels/topology/RoleTree.tsx
var import_react11 = __toESM(require("react"));
var import_jsx_runtime19 = require("react/jsx-runtime");
var STATE_COLOUR = {
  active: "var(--ok)",
  running: "var(--ok)",
  idle: "var(--ink-faint)",
  degraded: "var(--warn)",
  retired: "var(--ink-faint)",
  stopped: "var(--ink-faint)"
};
function stateColour(state) {
  return STATE_COLOUR[state] || "var(--ink-muted)";
}
var COLLAPSE_THRESHOLD = 12;
function RoleTree({
  nodes,
  agents,
  activity
}) {
  const graph = import_react11.default.useMemo(() => buildGraph(nodes, agents), [nodes, agents]);
  const roleIndex = import_react11.default.useMemo(() => roleIndexFor(graph.roles), [graph.roles]);
  const live = useTopologyActivity(activity, graph, { life: 1500 });
  const grouped = import_react11.default.useMemo(() => {
    var _a;
    const g = {};
    for (const r2 of graph.roles) g[r2] = [];
    for (const a of graph.agents) (g[_a = a.role] || (g[_a] = [])).push(a);
    return g;
  }, [graph]);
  const [expanded, setExpanded] = import_react11.default.useState(() => {
    const initial = { __root: true };
    for (const r2 of graph.roles) {
      const count = grouped[r2]?.length || 0;
      initial[r2] = count > 0 && count <= COLLAPSE_THRESHOLD;
    }
    return initial;
  });
  const toggle = (key) => setExpanded((s) => ({ ...s, [key]: !s[key] }));
  const rootHot = graph.agents.some((a) => live.active[a.id]);
  const rootBusy = graph.agents.some((a) => live.busy[a.id]);
  return /* @__PURE__ */ (0, import_jsx_runtime19.jsxs)("div", { className: "topo-roletree", children: [
    /* @__PURE__ */ (0, import_jsx_runtime19.jsx)("div", { className: "topo-roletree__row", children: /* @__PURE__ */ (0, import_jsx_runtime19.jsxs)(
      "button",
      {
        type: "button",
        className: `topo-roletree__mob ${rootHot ? "is-hot" : ""}${rootBusy ? " is-busy" : ""}`,
        onClick: () => toggle("__root"),
        children: [
          /* @__PURE__ */ (0, import_jsx_runtime19.jsx)(
            "span",
            {
              className: "topo-roletree__chevron",
              style: { transform: expanded.__root ? "rotate(90deg)" : "rotate(0)" },
              children: "\u25B8"
            }
          ),
          /* @__PURE__ */ (0, import_jsx_runtime19.jsx)("span", { className: "topo-roletree__dot", style: { background: "var(--ok)" } }),
          /* @__PURE__ */ (0, import_jsx_runtime19.jsx)("span", { className: "topo-roletree__label", children: "mob" }),
          /* @__PURE__ */ (0, import_jsx_runtime19.jsxs)("span", { className: "topo-roletree__count", children: [
            graph.agents.length,
            " agents \xB7 ",
            graph.roles.length,
            " roles"
          ] }),
          rootBusy && /* @__PURE__ */ (0, import_jsx_runtime19.jsx)("span", { className: "topo-roletree__busy", "aria-label": "agents working" })
        ]
      }
    ) }),
    expanded.__root && graph.roles.map((role) => {
      const list = grouped[role] || [];
      if (list.length === 0) return null;
      const isOpen = !!expanded[role];
      const sectionHot = list.some((a) => live.active[a.id]);
      const sectionBusy = list.some((a) => live.busy[a.id]);
      const sectionBusyCount = list.filter((a) => live.busy[a.id]).length;
      const colour = colourForRole(role, roleIndex);
      return /* @__PURE__ */ (0, import_jsx_runtime19.jsxs)("div", { className: "topo-roletree__section", children: [
        /* @__PURE__ */ (0, import_jsx_runtime19.jsxs)(
          "button",
          {
            type: "button",
            className: `topo-roletree__role ${sectionHot ? "is-hot" : ""}${sectionBusy ? " is-busy" : ""}`,
            onClick: () => toggle(role),
            children: [
              /* @__PURE__ */ (0, import_jsx_runtime19.jsx)(
                "span",
                {
                  className: "topo-roletree__chevron",
                  style: { transform: isOpen ? "rotate(90deg)" : "rotate(0)" },
                  children: "\u25B8"
                }
              ),
              /* @__PURE__ */ (0, import_jsx_runtime19.jsx)("span", { className: "topo-roletree__dot", style: { background: colour } }),
              /* @__PURE__ */ (0, import_jsx_runtime19.jsx)("span", { className: "topo-roletree__label", children: role }),
              /* @__PURE__ */ (0, import_jsx_runtime19.jsx)("span", { className: "topo-roletree__count", children: list.length }),
              sectionBusy && /* @__PURE__ */ (0, import_jsx_runtime19.jsx)("span", { className: "topo-roletree__busy", "aria-label": `${sectionBusyCount} working`, children: /* @__PURE__ */ (0, import_jsx_runtime19.jsx)("span", { className: "topo-roletree__busy-count", children: sectionBusyCount }) })
            ]
          }
        ),
        isOpen && /* @__PURE__ */ (0, import_jsx_runtime19.jsx)("div", { className: "topo-roletree__pod", children: list.map((agent) => {
          const isHot = !!live.active[agent.id];
          const isBusy = !!live.busy[agent.id];
          return /* @__PURE__ */ (0, import_jsx_runtime19.jsxs)(
            "div",
            {
              className: `topo-roletree__agent ${isHot ? "is-hot" : ""}${isBusy ? " is-busy" : ""}`,
              "data-testid": `topology-node:${agent.id}`,
              title: `${agent.id}${agent.state ? " \xB7 " + agent.state : ""}${isBusy ? " \xB7 working" : ""}`,
              children: [
                /* @__PURE__ */ (0, import_jsx_runtime19.jsx)(
                  "span",
                  {
                    className: "topo-roletree__agent-dot",
                    style: { background: stateColour(agent.state) }
                  }
                ),
                /* @__PURE__ */ (0, import_jsx_runtime19.jsx)("span", { className: "topo-roletree__agent-label", children: agent.label || agent.id }),
                isBusy && /* @__PURE__ */ (0, import_jsx_runtime19.jsx)("span", { className: "topo-roletree__busy", "aria-label": "working" })
              ]
            },
            agent.id
          );
        }) })
      ] }, role);
    })
  ] });
}

// src/panels/TopologyPanel.tsx
var import_jsx_runtime20 = require("react/jsx-runtime");
var VIEW_STORAGE = "mobkit-console-topology-view";
var LABELS_STORAGE = "mobkit-console-topology-labels";
var VIEWS = [
  { id: "force", label: "Force", help: "Physics sim \xB7 communities + hubs emerge" },
  { id: "bullseye", label: "Bullseye", help: "Degree-ranked rings \xB7 hubs at centre" },
  { id: "roles", label: "Roles", help: "Flat mob \xB7 agents grouped by role" }
];
var LABEL_MODES = [
  { id: "auto", label: "Auto", help: "Always-on for \u226420 agents \xB7 hover for denser graphs" },
  { id: "on", label: "All", help: "Force labels on regardless of density" },
  { id: "off", label: "Hover", help: "Hidden until hovered or focused" }
];
var W = 980;
var H = 580;
function TopologyPanel({
  nodes,
  agents,
  activity
}) {
  const [view, setView] = import_react12.default.useState(() => {
    try {
      const stored = localStorage.getItem(VIEW_STORAGE);
      if (stored === "force" || stored === "bullseye" || stored === "roles") return stored;
    } catch {
    }
    return "force";
  });
  const pickView = (next) => {
    setView(next);
    try {
      localStorage.setItem(VIEW_STORAGE, next);
    } catch {
    }
  };
  const [labelsMode, setLabelsMode] = import_react12.default.useState(() => {
    try {
      const stored = localStorage.getItem(LABELS_STORAGE);
      if (stored === "auto" || stored === "on" || stored === "off") return stored;
    } catch {
    }
    return "auto";
  });
  const pickLabelsMode = (next) => {
    setLabelsMode(next);
    try {
      localStorage.setItem(LABELS_STORAGE, next);
    } catch {
    }
  };
  const graph = import_react12.default.useMemo(() => buildGraph(nodes, agents), [nodes, agents]);
  const live = useTopologyActivity(activity, graph, { life: 1500 });
  const roleIndex = import_react12.default.useMemo(() => roleIndexFor(graph.roles), [graph.roles]);
  const liveCount = Object.keys(live.active).length;
  const busyCount = Object.values(live.busy).filter(Boolean).length;
  return /* @__PURE__ */ (0, import_jsx_runtime20.jsxs)("div", { className: "topo", "data-testid": "topology-panel", children: [
    /* @__PURE__ */ (0, import_jsx_runtime20.jsxs)("div", { className: "topo__head", children: [
      /* @__PURE__ */ (0, import_jsx_runtime20.jsx)("h2", { children: "Topology" }),
      /* @__PURE__ */ (0, import_jsx_runtime20.jsxs)("span", { className: "topo__head-meta", children: [
        graph.agents.length,
        " agents \xB7 ",
        graph.edges.length,
        " edges",
        busyCount > 0 ? ` \xB7 ${busyCount} working` : "",
        liveCount > 0 && busyCount === 0 ? ` \xB7 ${liveCount} live` : ""
      ] }),
      view !== "roles" && /* @__PURE__ */ (0, import_jsx_runtime20.jsxs)("div", { className: "topo__viewbar topo__viewbar--labels", role: "group", "aria-label": "Labels", children: [
        /* @__PURE__ */ (0, import_jsx_runtime20.jsx)("span", { className: "topo__viewbar-tag", children: "Labels" }),
        LABEL_MODES.map((m) => /* @__PURE__ */ (0, import_jsx_runtime20.jsx)(
          "button",
          {
            type: "button",
            className: `topo__viewbtn ${labelsMode === m.id ? "is-active" : ""}`,
            onClick: () => pickLabelsMode(m.id),
            title: m.help,
            "data-testid": `topology-labels:${m.id}`,
            children: m.label
          },
          m.id
        ))
      ] }),
      /* @__PURE__ */ (0, import_jsx_runtime20.jsx)("div", { className: "topo__viewbar", children: VIEWS.map((v) => /* @__PURE__ */ (0, import_jsx_runtime20.jsx)(
        "button",
        {
          type: "button",
          className: `topo__viewbtn ${view === v.id ? "is-active" : ""}`,
          onClick: () => pickView(v.id),
          title: v.help,
          "data-testid": `topology-view:${v.id}`,
          children: v.label
        },
        v.id
      )) })
    ] }),
    /* @__PURE__ */ (0, import_jsx_runtime20.jsxs)("div", { className: "topo__body", children: [
      view === "force" && /* @__PURE__ */ (0, import_jsx_runtime20.jsx)(
        ForceDirected,
        {
          nodes,
          agents,
          activity,
          width: W,
          height: H,
          labelsMode
        }
      ),
      view === "bullseye" && /* @__PURE__ */ (0, import_jsx_runtime20.jsx)(
        Bullseye,
        {
          nodes,
          agents,
          activity,
          width: W,
          height: H,
          labelsMode
        }
      ),
      view === "roles" && /* @__PURE__ */ (0, import_jsx_runtime20.jsx)(
        RoleTree,
        {
          nodes,
          agents,
          activity
        }
      )
    ] }),
    view !== "roles" && graph.roles.length > 0 && /* @__PURE__ */ (0, import_jsx_runtime20.jsx)("div", { className: "topo__legend", children: graph.roles.map((role) => {
      const count = graph.agents.filter((a) => a.role === role).length;
      return /* @__PURE__ */ (0, import_jsx_runtime20.jsxs)("div", { className: "topo__legend-item", children: [
        /* @__PURE__ */ (0, import_jsx_runtime20.jsx)(
          "span",
          {
            className: "topo__legend-dot",
            style: { background: colourForRole(role, roleIndex) }
          }
        ),
        role,
        /* @__PURE__ */ (0, import_jsx_runtime20.jsx)("span", { className: "topo__legend-count", children: count })
      ] }, role);
    }) })
  ] });
}

// src/panels/TimelinePanel.tsx
var import_react13 = __toESM(require("react"));
var import_jsx_runtime21 = require("react/jsx-runtime");
function classifyFrame(frame) {
  const ev = frame.event;
  if (ev === "gating_decision" || ev.startsWith("gate_")) return "gate";
  if (ev === "run_failed" || ev === "interaction_failed") return "warn";
  if (ev === "route_changed" || ev === "topology_updated") return "topology";
  if (ev === "member_ready" || ev === "member_retired" || ev === "state_changed") return "lifecycle";
  if (ev === "interaction_complete" || ev === "interaction_started") return "interaction";
  return "dispatch";
}
function formatTime(tsMs) {
  if (!tsMs) return "\u2014";
  const d = new Date(tsMs);
  const hh = String(d.getHours()).padStart(2, "0");
  const mm = String(d.getMinutes()).padStart(2, "0");
  return `${hh}:${mm}`;
}
function summarizeFrame(frame) {
  const ev = frame.event;
  const data = frame.data || {};
  switch (ev) {
    case "interaction_complete":
      return `Completed interaction ${String(frame.interactionId || "").slice(0, 8)}`;
    case "interaction_failed":
      return `Failed: ${String(data.error || data.reason || "error")}`;
    case "interaction_started":
      return `Started interaction ${String(frame.interactionId || "").slice(0, 8)}`;
    case "gating_decision":
      return `Gate ${String(data.decision || "")}: ${String(data.action_id || data.pending_id || "")}`;
    case "member_ready":
      return `Member ready`;
    case "member_retired":
      return `Member retired`;
    case "state_changed":
      return `State \u2192 ${String(data.state || data.new_state || "")}`;
    case "route_changed":
      return `Route updated`;
    default:
      return ev.replace(/_/g, " ");
  }
}
function TimelinePanel({ frames }) {
  const entries = import_react13.default.useMemo(() => {
    const todayMs = (() => {
      const d = /* @__PURE__ */ new Date();
      d.setHours(0, 0, 0, 0);
      return d.getTime();
    })();
    return frames.filter((f) => (f.timestampMs || 0) >= todayMs).slice(0, 80).map((f) => ({
      time: formatTime(f.timestampMs),
      type: classifyFrame(f),
      text: summarizeFrame(f),
      who: f.identity || "_system"
    }));
  }, [frames]);
  const today = /* @__PURE__ */ new Date();
  const dateLabel = today.toLocaleDateString(void 0, { month: "short", day: "numeric", year: "numeric" });
  return /* @__PURE__ */ (0, import_jsx_runtime21.jsxs)("div", { className: "tl", "data-testid": "timeline-panel", children: [
    /* @__PURE__ */ (0, import_jsx_runtime21.jsxs)("div", { className: "tl__head", children: [
      /* @__PURE__ */ (0, import_jsx_runtime21.jsx)("h2", { children: "Today" }),
      /* @__PURE__ */ (0, import_jsx_runtime21.jsxs)("p", { children: [
        "\xB7 ",
        entries.length,
        " events \xB7 ",
        dateLabel
      ] })
    ] }),
    /* @__PURE__ */ (0, import_jsx_runtime21.jsxs)("div", { className: "tl__body", children: [
      entries.length === 0 && /* @__PURE__ */ (0, import_jsx_runtime21.jsx)("div", { style: { gridColumn: "1 / -1", padding: "40px 0", color: "var(--ink-dim)", fontFamily: "var(--mono)", fontSize: 12, textAlign: "center" }, children: "No events yet today." }),
      entries.map((e, i) => /* @__PURE__ */ (0, import_jsx_runtime21.jsxs)("div", { className: "tl__row", "data-type": e.type, children: [
        /* @__PURE__ */ (0, import_jsx_runtime21.jsx)("div", { className: "tl__time", children: e.time }),
        /* @__PURE__ */ (0, import_jsx_runtime21.jsx)("div", { className: "tl__rail", children: /* @__PURE__ */ (0, import_jsx_runtime21.jsx)("span", { className: "tl__dot" }) }),
        /* @__PURE__ */ (0, import_jsx_runtime21.jsxs)("div", { className: "tl__card", children: [
          /* @__PURE__ */ (0, import_jsx_runtime21.jsxs)("div", { children: [
            /* @__PURE__ */ (0, import_jsx_runtime21.jsx)("span", { className: "tl__type", children: e.type }),
            /* @__PURE__ */ (0, import_jsx_runtime21.jsx)("span", { children: e.text })
          ] }),
          /* @__PURE__ */ (0, import_jsx_runtime21.jsx)("div", { className: "tl__who", children: e.who })
        ] })
      ] }, i))
    ] })
  ] });
}

// src/panels/GatingInboxPanel.tsx
var import_react14 = __toESM(require("react"));
var import_jsx_runtime22 = require("react/jsx-runtime");
function getRisk(entry) {
  const tier = String(entry.risk_tier || entry.risk || "").toLowerCase();
  if (tier === "high" || tier === "crit" || tier === "critical") return "high";
  if (tier === "medium" || tier === "med" || tier === "warn") return "medium";
  return "low";
}
function formatWaited(entry) {
  const waited = entry.waited_ms || entry.waited || entry.age_ms;
  if (typeof waited !== "number") return "\u2014";
  const seconds = Math.floor(waited / 1e3);
  if (seconds < 60) return `${seconds}s`;
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `${minutes}m`;
  const hours = Math.floor(minutes / 60);
  return `${hours}h ${minutes % 60}m`;
}
function payloadSummary(entry) {
  const payload = entry.payload;
  if (typeof payload === "string") return payload;
  if (payload && typeof payload === "object") {
    try {
      const parts = [];
      for (const [k, v] of Object.entries(payload).slice(0, 3)) {
        parts.push(`${k}=${String(v).slice(0, 20)}`);
      }
      return parts.join(" ");
    } catch {
      return "";
    }
  }
  return String(entry.summary || entry.reason || "");
}
function GatingInboxPanel({ pending, audit, onDecide }) {
  const [tab, setTab] = import_react14.default.useState("pending");
  const [selectedId, setSelectedId] = import_react14.default.useState(null);
  const autoApproved = audit.filter((e) => {
    const r2 = e;
    return String(r2.decision || "").toLowerCase() === "auto_approve" || String(r2.event_type || "").includes("auto");
  });
  const currentList = tab === "pending" ? pending : tab === "auto" ? autoApproved : audit;
  return /* @__PURE__ */ (0, import_jsx_runtime22.jsxs)("div", { className: "gating", "data-testid": "gating-panel", children: [
    /* @__PURE__ */ (0, import_jsx_runtime22.jsxs)("div", { className: "gating__head", children: [
      /* @__PURE__ */ (0, import_jsx_runtime22.jsx)("h2", { children: "Gating inbox" }),
      /* @__PURE__ */ (0, import_jsx_runtime22.jsxs)("p", { children: [
        "\xB7 ",
        pending.length,
        " pending \xB7 ",
        autoApproved.length,
        " auto-approved today"
      ] })
    ] }),
    /* @__PURE__ */ (0, import_jsx_runtime22.jsxs)("div", { className: "gating__tabs", children: [
      /* @__PURE__ */ (0, import_jsx_runtime22.jsxs)(
        "button",
        {
          className: `gating__tab ${tab === "pending" ? "is-active" : ""}`,
          onClick: () => setTab("pending"),
          "data-testid": "gating-tab:pending",
          children: [
            "Pending ",
            /* @__PURE__ */ (0, import_jsx_runtime22.jsx)("span", { className: "n", children: pending.length })
          ]
        }
      ),
      /* @__PURE__ */ (0, import_jsx_runtime22.jsxs)(
        "button",
        {
          className: `gating__tab ${tab === "auto" ? "is-active" : ""}`,
          onClick: () => setTab("auto"),
          "data-testid": "gating-tab:auto",
          children: [
            "Auto ",
            /* @__PURE__ */ (0, import_jsx_runtime22.jsx)("span", { className: "n", children: autoApproved.length })
          ]
        }
      ),
      /* @__PURE__ */ (0, import_jsx_runtime22.jsxs)(
        "button",
        {
          className: `gating__tab ${tab === "audit" ? "is-active" : ""}`,
          onClick: () => setTab("audit"),
          "data-testid": "gating-tab:audit",
          children: [
            "Audit ",
            /* @__PURE__ */ (0, import_jsx_runtime22.jsx)("span", { className: "n", children: audit.length })
          ]
        }
      )
    ] }),
    /* @__PURE__ */ (0, import_jsx_runtime22.jsxs)("div", { className: "gating__list", children: [
      currentList.length === 0 && /* @__PURE__ */ (0, import_jsx_runtime22.jsxs)("div", { className: "gating__empty", children: [
        "No ",
        tab,
        " items."
      ] }),
      currentList.map((entry, index) => {
        const r2 = entry;
        const pid = String(r2.pending_id || r2.audit_id || `item-${index}`);
        const action = String(r2.action_id || r2.event_type || "unknown action");
        const agent = String(r2.agent || r2.identity || r2.actor || "");
        const waited = formatWaited(r2);
        const risk = getRisk(r2);
        const payload = payloadSummary(r2);
        const selected = selectedId === pid;
        const showActions = tab === "pending";
        return /* @__PURE__ */ (0, import_jsx_runtime22.jsxs)(
          "div",
          {
            className: `gitem ${selected ? "is-selected" : ""}`,
            "data-risk": risk,
            "data-testid": `gating-pending:${pid}`,
            onClick: () => setSelectedId(pid),
            children: [
              /* @__PURE__ */ (0, import_jsx_runtime22.jsx)("span", { className: "gitem__risk" }),
              /* @__PURE__ */ (0, import_jsx_runtime22.jsx)("span", { className: "gitem__id", children: pid.slice(0, 8) }),
              /* @__PURE__ */ (0, import_jsx_runtime22.jsxs)("span", { children: [
                /* @__PURE__ */ (0, import_jsx_runtime22.jsx)("div", { className: "gitem__action", children: action }),
                payload && /* @__PURE__ */ (0, import_jsx_runtime22.jsx)("div", { className: "gitem__payload", children: payload }),
                agent && /* @__PURE__ */ (0, import_jsx_runtime22.jsx)("div", { className: "gitem__agent", children: agent })
              ] }),
              showActions ? /* @__PURE__ */ (0, import_jsx_runtime22.jsxs)("span", { className: "gitem__actions", children: [
                /* @__PURE__ */ (0, import_jsx_runtime22.jsx)(
                  "button",
                  {
                    className: "approve",
                    "data-testid": `gating-action:${pid}:approve`,
                    onClick: (e) => {
                      e.stopPropagation();
                      onDecide(pid, "approve");
                    },
                    children: "Approve"
                  }
                ),
                /* @__PURE__ */ (0, import_jsx_runtime22.jsx)(
                  "button",
                  {
                    className: "reject",
                    "data-testid": `gating-action:${pid}:reject`,
                    onClick: (e) => {
                      e.stopPropagation();
                      onDecide(pid, "reject");
                    },
                    children: "Reject"
                  }
                ),
                /* @__PURE__ */ (0, import_jsx_runtime22.jsx)(
                  "button",
                  {
                    "data-testid": `gating-action:${pid}:escalate`,
                    onClick: (e) => {
                      e.stopPropagation();
                      onDecide(pid, "escalate");
                    },
                    children: "Escalate"
                  }
                )
              ] }) : /* @__PURE__ */ (0, import_jsx_runtime22.jsx)("span", { className: "gitem__actions" }),
              /* @__PURE__ */ (0, import_jsx_runtime22.jsxs)("span", { className: "gitem__waited", children: [
                "waited",
                /* @__PURE__ */ (0, import_jsx_runtime22.jsx)("br", {}),
                waited
              ] })
            ]
          },
          pid
        );
      })
    ] })
  ] });
}

// src/panels/RosterPanel.tsx
var import_react15 = __toESM(require("react"));
var import_jsx_runtime23 = require("react/jsx-runtime");
var ROLE_BUCKETS = ["all", "personal", "coordinator", "domain", "internal"];
function roleOf(a) {
  const p = (a.role || a.kind || "").toLowerCase();
  const g = (a.group || "").toLowerCase();
  if (p.includes("personal") || g.includes("personal")) return "personal";
  if (p.includes("coord") || p.includes("triage") || p.includes("router")) return "coordinator";
  if (p.includes("monitor") || p.includes("scribe") || p.includes("gate")) return "internal";
  return "domain";
}
function stateLabel(state) {
  return (state || "unknown").toLowerCase();
}
function RosterPanel({ agents, onSelect, onInspect, onLifecycle, canResetLifecycle = false }) {
  const [q, setQ] = import_react15.default.useState("");
  const [role, setRole] = import_react15.default.useState("all");
  const [sel, setSel] = import_react15.default.useState(agents[0]?.member_id || "");
  const rows = import_react15.default.useMemo(() => {
    return agents.filter((a) => {
      if (role !== "all" && roleOf(a) !== role) return false;
      if (!q) return true;
      const hay = `${a.label} ${a.member_id} ${a.identity || ""} ${a.role || ""} ${a.kind || ""}`.toLowerCase();
      return hay.includes(q.toLowerCase());
    });
  }, [agents, q, role]);
  const active = rows.find((r2) => r2.member_id === sel) || rows[0];
  const activeIdentity = active?.identity || active?.member_id || "";
  return /* @__PURE__ */ (0, import_jsx_runtime23.jsxs)("div", { className: "view roster", "data-testid": "roster-panel", children: [
    /* @__PURE__ */ (0, import_jsx_runtime23.jsxs)("div", { className: "view__head", children: [
      /* @__PURE__ */ (0, import_jsx_runtime23.jsx)("h2", { children: "Roster" }),
      /* @__PURE__ */ (0, import_jsx_runtime23.jsxs)("span", { className: "view__sub", children: [
        rows.length,
        " of ",
        agents.length,
        " agents"
      ] }),
      /* @__PURE__ */ (0, import_jsx_runtime23.jsx)("span", { className: "view__spacer" }),
      /* @__PURE__ */ (0, import_jsx_runtime23.jsx)(
        "input",
        {
          className: "view__search",
          placeholder: "Filter agents, profiles, ids\u2026",
          value: q,
          onChange: (e) => setQ(e.target.value)
        }
      ),
      /* @__PURE__ */ (0, import_jsx_runtime23.jsx)("div", { className: "view__segs", children: ROLE_BUCKETS.map((r2) => /* @__PURE__ */ (0, import_jsx_runtime23.jsx)("button", { className: role === r2 ? "is-active" : "", onClick: () => setRole(r2), children: r2 }, r2)) })
    ] }),
    /* @__PURE__ */ (0, import_jsx_runtime23.jsxs)("div", { className: "roster__body", children: [
      /* @__PURE__ */ (0, import_jsx_runtime23.jsxs)("div", { className: "roster__table", children: [
        /* @__PURE__ */ (0, import_jsx_runtime23.jsxs)("div", { className: "roster__row roster__row--head", children: [
          /* @__PURE__ */ (0, import_jsx_runtime23.jsx)("span", { children: "Name" }),
          /* @__PURE__ */ (0, import_jsx_runtime23.jsx)("span", { children: "Role" }),
          /* @__PURE__ */ (0, import_jsx_runtime23.jsx)("span", { children: "State" }),
          /* @__PURE__ */ (0, import_jsx_runtime23.jsx)("span", { children: "Profile" }),
          /* @__PURE__ */ (0, import_jsx_runtime23.jsx)("span", { children: "Gen" }),
          /* @__PURE__ */ (0, import_jsx_runtime23.jsx)("span", { children: "Chk" }),
          /* @__PURE__ */ (0, import_jsx_runtime23.jsx)("span", { children: "Lease" })
        ] }),
        rows.map((r2) => {
          const isSel = active && r2.member_id === active.member_id;
          return /* @__PURE__ */ (0, import_jsx_runtime23.jsxs)(
            "div",
            {
              className: `roster__row ${isSel ? "is-selected" : ""}`,
              "data-state": stateLabel(r2.state),
              onClick: () => {
                setSel(r2.member_id);
                onSelect(r2);
              },
              "data-testid": `roster-row:${r2.member_id}`,
              children: [
                /* @__PURE__ */ (0, import_jsx_runtime23.jsxs)("span", { className: "roster__name", children: [
                  /* @__PURE__ */ (0, import_jsx_runtime23.jsx)("span", { className: "roster__dot" }),
                  /* @__PURE__ */ (0, import_jsx_runtime23.jsxs)("span", { children: [
                    /* @__PURE__ */ (0, import_jsx_runtime23.jsx)("div", { children: r2.label }),
                    /* @__PURE__ */ (0, import_jsx_runtime23.jsx)("div", { className: "roster__id", children: r2.identity || r2.member_id })
                  ] })
                ] }),
                /* @__PURE__ */ (0, import_jsx_runtime23.jsx)("span", { children: roleOf(r2) }),
                /* @__PURE__ */ (0, import_jsx_runtime23.jsx)("span", { className: "roster__state", children: stateLabel(r2.state) }),
                /* @__PURE__ */ (0, import_jsx_runtime23.jsx)("span", { className: "mono dim", children: r2.role || "\u2014" }),
                /* @__PURE__ */ (0, import_jsx_runtime23.jsx)("span", { className: "mono", children: r2.generation ?? "\u2014" }),
                /* @__PURE__ */ (0, import_jsx_runtime23.jsx)("span", { className: "mono", children: r2.checkpoint_version ?? "\u2014" }),
                /* @__PURE__ */ (0, import_jsx_runtime23.jsx)("span", { className: "mono dim", children: r2.lease_healthy === false ? "unhealthy" : "ok" })
              ]
            },
            r2.member_id
          );
        })
      ] }),
      /* @__PURE__ */ (0, import_jsx_runtime23.jsx)("aside", { className: "roster__detail", children: active && /* @__PURE__ */ (0, import_jsx_runtime23.jsxs)(import_jsx_runtime23.Fragment, { children: [
        /* @__PURE__ */ (0, import_jsx_runtime23.jsxs)("div", { className: "rd__head", children: [
          /* @__PURE__ */ (0, import_jsx_runtime23.jsx)("div", { className: "rd__title", children: active.label }),
          /* @__PURE__ */ (0, import_jsx_runtime23.jsx)("div", { className: "rd__id", children: active.identity || active.member_id }),
          /* @__PURE__ */ (0, import_jsx_runtime23.jsx)("div", { className: "rd__tags", children: [active.role, active.kind, roleOf(active)].filter(Boolean).map((t) => /* @__PURE__ */ (0, import_jsx_runtime23.jsx)("span", { className: "chip", children: String(t) }, String(t))) })
        ] }),
        /* @__PURE__ */ (0, import_jsx_runtime23.jsxs)("dl", { className: "rd__grid", children: [
          /* @__PURE__ */ (0, import_jsx_runtime23.jsx)("dt", { children: "Profile" }),
          /* @__PURE__ */ (0, import_jsx_runtime23.jsx)("dd", { children: active.role || "\u2014" }),
          /* @__PURE__ */ (0, import_jsx_runtime23.jsx)("dt", { children: "Kind" }),
          /* @__PURE__ */ (0, import_jsx_runtime23.jsx)("dd", { children: active.kind || "\u2014" }),
          /* @__PURE__ */ (0, import_jsx_runtime23.jsx)("dt", { children: "Role" }),
          /* @__PURE__ */ (0, import_jsx_runtime23.jsx)("dd", { children: roleOf(active) }),
          /* @__PURE__ */ (0, import_jsx_runtime23.jsx)("dt", { children: "State" }),
          /* @__PURE__ */ (0, import_jsx_runtime23.jsx)("dd", { children: /* @__PURE__ */ (0, import_jsx_runtime23.jsx)("span", { className: "roster__state", children: stateLabel(active.state) }) }),
          /* @__PURE__ */ (0, import_jsx_runtime23.jsx)("dt", { children: "Member" }),
          /* @__PURE__ */ (0, import_jsx_runtime23.jsx)("dd", { className: "mono", children: active.member_id }),
          /* @__PURE__ */ (0, import_jsx_runtime23.jsx)("dt", { children: "Identity" }),
          /* @__PURE__ */ (0, import_jsx_runtime23.jsx)("dd", { className: "mono", children: active.identity || "\u2014" }),
          /* @__PURE__ */ (0, import_jsx_runtime23.jsx)("dt", { children: "Session" }),
          /* @__PURE__ */ (0, import_jsx_runtime23.jsx)("dd", { className: "mono", children: active.session_id || "\u2014" }),
          /* @__PURE__ */ (0, import_jsx_runtime23.jsx)("dt", { children: "Generation" }),
          /* @__PURE__ */ (0, import_jsx_runtime23.jsx)("dd", { className: "mono", children: active.generation ?? "\u2014" }),
          /* @__PURE__ */ (0, import_jsx_runtime23.jsx)("dt", { children: "Checkpoint" }),
          /* @__PURE__ */ (0, import_jsx_runtime23.jsx)("dd", { className: "mono", children: active.checkpoint_version ?? "\u2014" }),
          /* @__PURE__ */ (0, import_jsx_runtime23.jsx)("dt", { children: "Lease" }),
          /* @__PURE__ */ (0, import_jsx_runtime23.jsx)("dd", { className: "mono", children: active.lease_healthy === false ? "unhealthy" : "ok" }),
          /* @__PURE__ */ (0, import_jsx_runtime23.jsx)("dt", { children: "Wired" }),
          /* @__PURE__ */ (0, import_jsx_runtime23.jsxs)("dd", { className: "mono", children: [
            (active.wired_to || []).length,
            " peers"
          ] })
        ] }),
        /* @__PURE__ */ (0, import_jsx_runtime23.jsxs)("div", { className: "rd__actions", children: [
          /* @__PURE__ */ (0, import_jsx_runtime23.jsx)("button", { onClick: () => onInspect(active), children: "Inspect" }),
          active.affordances?.can_respawn ? /* @__PURE__ */ (0, import_jsx_runtime23.jsx)("button", { onClick: () => onLifecycle(activeIdentity, "mobkit/respawn"), children: "Respawn" }) : null,
          canResetLifecycle ? /* @__PURE__ */ (0, import_jsx_runtime23.jsx)("button", { onClick: () => onLifecycle(activeIdentity, "mobkit/reset"), children: "Reset" }) : null,
          active.affordances?.can_retire ? /* @__PURE__ */ (0, import_jsx_runtime23.jsx)("button", { className: "danger", onClick: () => onLifecycle(activeIdentity, "mobkit/retire"), children: "Retire" }) : null
        ] })
      ] }) })
    ] })
  ] });
}

// src/panels/RoutingPanel.tsx
var import_react16 = __toESM(require("react"));
var import_jsx_runtime24 = require("react/jsx-runtime");
function RoutingPanel({ data }) {
  const routes = data.routes || [];
  const deliveries = data.deliveries || [];
  const [q, setQ] = import_react16.default.useState("");
  const [sel, setSel] = import_react16.default.useState(routes[0]?.route_key || "");
  const rows = import_react16.default.useMemo(() => {
    if (!q) return routes;
    const needle = q.toLowerCase();
    return routes.filter(
      (r2) => r2.route_key.toLowerCase().includes(needle) || r2.recipient.toLowerCase().includes(needle) || r2.sink.toLowerCase().includes(needle) || r2.target_module.toLowerCase().includes(needle)
    );
  }, [routes, q]);
  const active = rows.find((r2) => r2.route_key === sel) || rows[0];
  const recentDeliveries = deliveries.slice(0, 40);
  const trafficForRoute = (routeKey) => deliveries.filter((d) => d.route_id === routeKey).length;
  return /* @__PURE__ */ (0, import_jsx_runtime24.jsxs)("div", { className: "view routing", "data-testid": "routing-panel", children: [
    /* @__PURE__ */ (0, import_jsx_runtime24.jsxs)("div", { className: "view__head", children: [
      /* @__PURE__ */ (0, import_jsx_runtime24.jsx)("h2", { children: "Routing" }),
      /* @__PURE__ */ (0, import_jsx_runtime24.jsxs)("span", { className: "view__sub", children: [
        rows.length,
        " routes \xB7 ",
        deliveries.length,
        " deliveries (recent)"
      ] }),
      /* @__PURE__ */ (0, import_jsx_runtime24.jsx)("span", { className: "view__spacer" }),
      /* @__PURE__ */ (0, import_jsx_runtime24.jsx)(
        "input",
        {
          className: "view__search",
          placeholder: "Filter route, recipient, sink\u2026",
          value: q,
          onChange: (e) => setQ(e.target.value)
        }
      )
    ] }),
    /* @__PURE__ */ (0, import_jsx_runtime24.jsxs)("div", { className: "routing__body", children: [
      /* @__PURE__ */ (0, import_jsx_runtime24.jsxs)("div", { className: "routing__table", children: [
        /* @__PURE__ */ (0, import_jsx_runtime24.jsxs)("div", { className: "routing__row routing__row--head", children: [
          /* @__PURE__ */ (0, import_jsx_runtime24.jsx)("span", { children: "Route" }),
          /* @__PURE__ */ (0, import_jsx_runtime24.jsx)("span", { children: "Channel" }),
          /* @__PURE__ */ (0, import_jsx_runtime24.jsx)("span", { children: "Recipient" }),
          /* @__PURE__ */ (0, import_jsx_runtime24.jsx)("span", { children: "Sink" }),
          /* @__PURE__ */ (0, import_jsx_runtime24.jsx)("span", { children: "Module" }),
          /* @__PURE__ */ (0, import_jsx_runtime24.jsx)("span", { children: "24h" })
        ] }),
        rows.map((r2) => {
          const isSel = active && r2.route_key === active.route_key;
          return /* @__PURE__ */ (0, import_jsx_runtime24.jsxs)(
            "div",
            {
              className: `routing__row ${isSel ? "is-selected" : ""}`,
              onClick: () => setSel(r2.route_key),
              "data-testid": `routing-route:${r2.route_key}`,
              children: [
                /* @__PURE__ */ (0, import_jsx_runtime24.jsx)("span", { className: "routing__intent mono", children: r2.route_key }),
                /* @__PURE__ */ (0, import_jsx_runtime24.jsx)("span", { className: "mono dim", children: r2.channel || "\u2014" }),
                /* @__PURE__ */ (0, import_jsx_runtime24.jsx)("span", { className: "mono", children: r2.recipient }),
                /* @__PURE__ */ (0, import_jsx_runtime24.jsx)("span", { className: "dim", children: r2.sink }),
                /* @__PURE__ */ (0, import_jsx_runtime24.jsx)("span", { className: "mono dim", children: r2.target_module }),
                /* @__PURE__ */ (0, import_jsx_runtime24.jsx)("span", { className: "mono", children: trafficForRoute(r2.route_key) })
              ]
            },
            r2.route_key
          );
        }),
        rows.length === 0 && /* @__PURE__ */ (0, import_jsx_runtime24.jsx)("div", { style: { padding: 24, color: "var(--ink-dim)", fontFamily: "var(--mono)", fontSize: 12 }, children: "No routes configured." })
      ] }),
      /* @__PURE__ */ (0, import_jsx_runtime24.jsx)("aside", { className: "routing__flow", children: active && /* @__PURE__ */ (0, import_jsx_runtime24.jsxs)(import_jsx_runtime24.Fragment, { children: [
        /* @__PURE__ */ (0, import_jsx_runtime24.jsx)("div", { className: "rf__title", children: "Flow" }),
        /* @__PURE__ */ (0, import_jsx_runtime24.jsxs)("div", { className: "rf__diagram", children: [
          /* @__PURE__ */ (0, import_jsx_runtime24.jsxs)("div", { className: "rf__node rf__node--intent", children: [
            /* @__PURE__ */ (0, import_jsx_runtime24.jsx)("div", { className: "rf__lbl", children: "Route" }),
            /* @__PURE__ */ (0, import_jsx_runtime24.jsx)("div", { className: "rf__val mono", children: active.route_key })
          ] }),
          /* @__PURE__ */ (0, import_jsx_runtime24.jsx)("svg", { className: "rf__arrow", viewBox: "0 0 40 12", children: /* @__PURE__ */ (0, import_jsx_runtime24.jsx)("path", { d: "M0 6 H 34 M 28 2 L 34 6 L 28 10", stroke: "currentColor", fill: "none", strokeWidth: "1" }) }),
          /* @__PURE__ */ (0, import_jsx_runtime24.jsxs)("div", { className: "rf__node rf__node--handler", children: [
            /* @__PURE__ */ (0, import_jsx_runtime24.jsxs)("div", { className: "rf__lbl", children: [
              "via ",
              active.sink
            ] }),
            /* @__PURE__ */ (0, import_jsx_runtime24.jsx)("div", { className: "rf__val mono", children: active.recipient })
          ] }),
          /* @__PURE__ */ (0, import_jsx_runtime24.jsx)("svg", { className: "rf__arrow rf__arrow--drop", viewBox: "0 0 12 40", children: /* @__PURE__ */ (0, import_jsx_runtime24.jsx)("path", { d: "M6 0 V 34 M 2 28 L 6 34 L 10 28", stroke: "currentColor", fill: "none", strokeWidth: "1" }) }),
          /* @__PURE__ */ (0, import_jsx_runtime24.jsxs)("div", { className: "rf__node rf__node--gate", children: [
            /* @__PURE__ */ (0, import_jsx_runtime24.jsx)("div", { className: "rf__lbl", children: "Module" }),
            /* @__PURE__ */ (0, import_jsx_runtime24.jsx)("div", { className: "rf__val mono", children: active.target_module })
          ] })
        ] }),
        /* @__PURE__ */ (0, import_jsx_runtime24.jsxs)("div", { className: "rf__stats", children: [
          /* @__PURE__ */ (0, import_jsx_runtime24.jsxs)("div", { children: [
            /* @__PURE__ */ (0, import_jsx_runtime24.jsx)("dt", { children: "Retry max" }),
            /* @__PURE__ */ (0, import_jsx_runtime24.jsx)("dd", { children: active.retry_max ?? "\u2014" })
          ] }),
          /* @__PURE__ */ (0, import_jsx_runtime24.jsxs)("div", { children: [
            /* @__PURE__ */ (0, import_jsx_runtime24.jsx)("dt", { children: "Backoff" }),
            /* @__PURE__ */ (0, import_jsx_runtime24.jsx)("dd", { children: active.backoff_ms ? `${active.backoff_ms} ms` : "\u2014" })
          ] }),
          /* @__PURE__ */ (0, import_jsx_runtime24.jsxs)("div", { children: [
            /* @__PURE__ */ (0, import_jsx_runtime24.jsx)("dt", { children: "Rate limit" }),
            /* @__PURE__ */ (0, import_jsx_runtime24.jsx)("dd", { children: active.rate_limit_per_minute ? `${active.rate_limit_per_minute}/m` : "\u2014" })
          ] })
        ] }),
        /* @__PURE__ */ (0, import_jsx_runtime24.jsx)("div", { className: "rf__title", style: { marginTop: 12 }, children: "Recent deliveries" }),
        /* @__PURE__ */ (0, import_jsx_runtime24.jsxs)("div", { style: { display: "flex", flexDirection: "column", gap: 4, fontFamily: "var(--mono)", fontSize: 11, color: "var(--ink-muted)" }, children: [
          recentDeliveries.filter((d) => d.route_id === active.route_key).slice(0, 8).map((d) => /* @__PURE__ */ (0, import_jsx_runtime24.jsxs)("div", { "data-testid": `routing-delivery:${d.delivery_id}`, children: [
            /* @__PURE__ */ (0, import_jsx_runtime24.jsx)("span", { style: { color: d.status === "delivered" ? "var(--ok)" : d.status === "failed" ? "var(--crit)" : "var(--warn)" }, children: d.status }),
            " ",
            /* @__PURE__ */ (0, import_jsx_runtime24.jsxs)("span", { className: "dim", children: [
              "\xB7 ",
              d.delivery_id.slice(0, 8)
            ] }),
            " ",
            /* @__PURE__ */ (0, import_jsx_runtime24.jsxs)("span", { children: [
              "\u2192 ",
              d.recipient
            ] })
          ] }, d.delivery_id)),
          recentDeliveries.filter((d) => d.route_id === active.route_key).length === 0 && /* @__PURE__ */ (0, import_jsx_runtime24.jsx)("span", { className: "dim", children: "No recent deliveries." })
        ] })
      ] }) })
    ] })
  ] });
}

// src/panels/GatesPanel.tsx
var import_react17 = __toESM(require("react"));
var import_jsx_runtime25 = require("react/jsx-runtime");
function derivePolicies(audit) {
  const byAction = /* @__PURE__ */ new Map();
  for (const entry of audit) {
    const r2 = entry;
    const action = String(r2.action_id || r2.event_type || "unknown");
    const decision = String(r2.decision || "").toLowerCase();
    const approver = String(r2.approver_id || r2.actor || "");
    const cur = byAction.get(action) || { approved: 0, rejected: 0, escalated: 0, approvers: /* @__PURE__ */ new Set() };
    if (decision === "approve" || decision === "auto_approve") cur.approved++;
    else if (decision === "reject") cur.rejected++;
    else if (decision === "escalate") cur.escalated++;
    if (approver) cur.approvers.add(approver);
    byAction.set(action, cur);
  }
  return Array.from(byAction.entries()).map(([action, s], i) => ({
    id: `pol-${i + 1}`,
    action,
    scope: "*",
    state: s.approved + s.rejected > 0 ? "active" : "paused",
    thresh: s.rejected > s.approved ? "High rejection rate" : "Auto on low risk",
    approvers: Array.from(s.approvers),
    sla: "\u2014 \xB7 p95 n/a",
    approved: s.approved,
    rejected: s.rejected,
    escalated: s.escalated
  }));
}
function GatesPanel({ audit }) {
  const policies = import_react17.default.useMemo(() => derivePolicies(audit), [audit]);
  const [sel, setSel] = import_react17.default.useState(policies[0]?.id || "");
  const active = policies.find((p) => p.id === sel) || policies[0];
  return /* @__PURE__ */ (0, import_jsx_runtime25.jsxs)("div", { className: "view gates", "data-testid": "gates-panel", children: [
    /* @__PURE__ */ (0, import_jsx_runtime25.jsxs)("div", { className: "view__head", children: [
      /* @__PURE__ */ (0, import_jsx_runtime25.jsx)("h2", { children: "Gates" }),
      /* @__PURE__ */ (0, import_jsx_runtime25.jsxs)("span", { className: "view__sub", children: [
        policies.length,
        " policies \xB7 ",
        audit.length,
        " decisions (recent)"
      ] }),
      /* @__PURE__ */ (0, import_jsx_runtime25.jsx)("span", { className: "view__spacer" })
    ] }),
    /* @__PURE__ */ (0, import_jsx_runtime25.jsxs)("div", { className: "gates__body", children: [
      /* @__PURE__ */ (0, import_jsx_runtime25.jsxs)("div", { className: "gates__list", children: [
        policies.map((g) => /* @__PURE__ */ (0, import_jsx_runtime25.jsxs)(
          "div",
          {
            className: `gate ${g.id === sel ? "is-selected" : ""}`,
            "data-state": g.state,
            onClick: () => setSel(g.id),
            "data-testid": `gate-policy:${g.id}`,
            children: [
              /* @__PURE__ */ (0, import_jsx_runtime25.jsxs)("div", { className: "gate__head", children: [
                /* @__PURE__ */ (0, import_jsx_runtime25.jsx)("span", { className: "gate__action mono", children: g.action }),
                /* @__PURE__ */ (0, import_jsx_runtime25.jsx)("span", { className: `gate__state gate__state--${g.state}`, children: g.state })
              ] }),
              /* @__PURE__ */ (0, import_jsx_runtime25.jsxs)("div", { className: "gate__scope", children: [
                "scope: ",
                g.scope
              ] }),
              /* @__PURE__ */ (0, import_jsx_runtime25.jsx)("div", { className: "gate__thresh", children: g.thresh }),
              /* @__PURE__ */ (0, import_jsx_runtime25.jsxs)("div", { className: "gate__stats", children: [
                /* @__PURE__ */ (0, import_jsx_runtime25.jsxs)("span", { children: [
                  /* @__PURE__ */ (0, import_jsx_runtime25.jsx)("b", { children: g.approved }),
                  /* @__PURE__ */ (0, import_jsx_runtime25.jsx)("span", { className: "dim", children: " approved" })
                ] }),
                /* @__PURE__ */ (0, import_jsx_runtime25.jsxs)("span", { children: [
                  /* @__PURE__ */ (0, import_jsx_runtime25.jsx)("b", { children: g.rejected }),
                  /* @__PURE__ */ (0, import_jsx_runtime25.jsx)("span", { className: "dim", children: " rejected" })
                ] }),
                /* @__PURE__ */ (0, import_jsx_runtime25.jsxs)("span", { children: [
                  /* @__PURE__ */ (0, import_jsx_runtime25.jsx)("b", { children: g.escalated }),
                  /* @__PURE__ */ (0, import_jsx_runtime25.jsx)("span", { className: "dim", children: " escalated" })
                ] })
              ] })
            ]
          },
          g.id
        )),
        policies.length === 0 && /* @__PURE__ */ (0, import_jsx_runtime25.jsx)("div", { style: { padding: 24, color: "var(--ink-dim)", fontFamily: "var(--mono)", fontSize: 12 }, children: "No gate policies inferred from recent audit." })
      ] }),
      /* @__PURE__ */ (0, import_jsx_runtime25.jsx)("aside", { className: "gates__detail", children: active && /* @__PURE__ */ (0, import_jsx_runtime25.jsxs)(import_jsx_runtime25.Fragment, { children: [
        /* @__PURE__ */ (0, import_jsx_runtime25.jsx)("div", { className: "gd__title", children: active.action }),
        /* @__PURE__ */ (0, import_jsx_runtime25.jsxs)("div", { className: "gd__scope dim", children: [
          "scope: ",
          active.scope
        ] }),
        /* @__PURE__ */ (0, import_jsx_runtime25.jsxs)("div", { className: "gd__section", children: [
          /* @__PURE__ */ (0, import_jsx_runtime25.jsx)("div", { className: "gd__label", children: "Policy" }),
          /* @__PURE__ */ (0, import_jsx_runtime25.jsx)("div", { className: "gd__body", children: active.thresh })
        ] }),
        /* @__PURE__ */ (0, import_jsx_runtime25.jsxs)("div", { className: "gd__section", children: [
          /* @__PURE__ */ (0, import_jsx_runtime25.jsx)("div", { className: "gd__label", children: "Approvers" }),
          /* @__PURE__ */ (0, import_jsx_runtime25.jsxs)("div", { className: "gd__approvers", children: [
            active.approvers.length === 0 && /* @__PURE__ */ (0, import_jsx_runtime25.jsx)("span", { className: "chip", children: "none recorded" }),
            active.approvers.map((a) => /* @__PURE__ */ (0, import_jsx_runtime25.jsx)("span", { className: "chip", children: a }, a))
          ] })
        ] }),
        /* @__PURE__ */ (0, import_jsx_runtime25.jsxs)("div", { className: "gd__section", children: [
          /* @__PURE__ */ (0, import_jsx_runtime25.jsx)("div", { className: "gd__label", children: "SLA" }),
          /* @__PURE__ */ (0, import_jsx_runtime25.jsx)("div", { className: "gd__body mono", children: active.sla })
        ] }),
        /* @__PURE__ */ (0, import_jsx_runtime25.jsxs)("div", { className: "gd__chart", children: [
          /* @__PURE__ */ (0, import_jsx_runtime25.jsx)("div", { className: "gd__chart-label", children: "Decisions (recent audit)" }),
          /* @__PURE__ */ (0, import_jsx_runtime25.jsxs)("div", { className: "gd__bar", children: [
            /* @__PURE__ */ (0, import_jsx_runtime25.jsx)("span", { className: "gd__bar-ok", style: { flex: active.approved || 1e-3 } }),
            /* @__PURE__ */ (0, import_jsx_runtime25.jsx)("span", { className: "gd__bar-no", style: { flex: active.rejected || 1e-3 } }),
            /* @__PURE__ */ (0, import_jsx_runtime25.jsx)("span", { className: "gd__bar-up", style: { flex: active.escalated || 1e-3 } })
          ] }),
          /* @__PURE__ */ (0, import_jsx_runtime25.jsxs)("div", { className: "gd__legend", children: [
            /* @__PURE__ */ (0, import_jsx_runtime25.jsxs)("span", { children: [
              /* @__PURE__ */ (0, import_jsx_runtime25.jsx)("i", { className: "dot ok" }),
              " ",
              active.approved,
              " approved"
            ] }),
            /* @__PURE__ */ (0, import_jsx_runtime25.jsxs)("span", { children: [
              /* @__PURE__ */ (0, import_jsx_runtime25.jsx)("i", { className: "dot no" }),
              " ",
              active.rejected,
              " rejected"
            ] }),
            /* @__PURE__ */ (0, import_jsx_runtime25.jsxs)("span", { children: [
              /* @__PURE__ */ (0, import_jsx_runtime25.jsx)("i", { className: "dot up" }),
              " ",
              active.escalated,
              " escalated"
            ] })
          ] })
        ] })
      ] }) })
    ] })
  ] });
}

// src/panels/LogsPanel.tsx
var import_react18 = __toESM(require("react"));
var import_jsx_runtime26 = require("react/jsx-runtime");
function levelFor(frame) {
  const ev = frame.event;
  if (ev.includes("failed") || ev.includes("error") || ev.includes("crash")) return "error";
  if (ev.includes("warn") || ev.includes("degraded") || ev.includes("gating_decision")) return "warn";
  return "info";
}
function formatTime2(tsMs) {
  if (!tsMs) return "\u2014";
  const d = new Date(tsMs);
  const hh = String(d.getHours()).padStart(2, "0");
  const mm = String(d.getMinutes()).padStart(2, "0");
  const ss = String(d.getSeconds()).padStart(2, "0");
  const ms = String(d.getMilliseconds()).padStart(3, "0");
  return `${hh}:${mm}:${ss}.${ms}`;
}
function summary(frame) {
  const d = frame.data || {};
  const bits = [];
  for (const [k, v] of Object.entries(d).slice(0, 4)) {
    if (v === null || v === void 0) continue;
    let str;
    if (typeof v === "object") {
      try {
        str = JSON.stringify(v).slice(0, 40);
      } catch {
        str = "[obj]";
      }
    } else str = String(v).slice(0, 60);
    bits.push(`${k}=${str}`);
  }
  return bits.join(" ");
}
function formatFrameData(frame) {
  const data = frame.data ?? null;
  if (data === null || data === void 0) return "(no data)";
  try {
    const out = JSON.stringify(data, null, 2);
    if (out.length > 1e4) return out.slice(0, 1e4) + "\n\u2026 (truncated)";
    return out;
  } catch {
    return String(data);
  }
}
function hasStructuredOutput(frame) {
  const d = frame.data;
  if (!d || typeof d !== "object") return false;
  return d.structured_output != null;
}
function LogsPanel({ frames }) {
  const [q, setQ] = import_react18.default.useState("");
  const [lvl, setLvl] = import_react18.default.useState("all");
  const [expanded, setExpanded] = import_react18.default.useState(/* @__PURE__ */ new Set());
  const toggle = (key) => setExpanded((prev) => {
    const next = new Set(prev);
    if (next.has(key)) next.delete(key);
    else next.add(key);
    return next;
  });
  const rows = import_react18.default.useMemo(() => {
    return frames.map((f) => ({ f, level: levelFor(f) })).filter(({ f, level }) => {
      if (lvl !== "all" && level !== lvl) return false;
      if (!q) return true;
      const needle = q.toLowerCase();
      return f.event.toLowerCase().includes(needle) || (f.identity || "").toLowerCase().includes(needle);
    });
  }, [frames, q, lvl]);
  const counts = import_react18.default.useMemo(() => {
    const c = { info: 0, warn: 0, error: 0 };
    frames.forEach((f) => {
      c[levelFor(f)]++;
    });
    return c;
  }, [frames]);
  return /* @__PURE__ */ (0, import_jsx_runtime26.jsxs)("div", { className: "view logs", "data-testid": "logs-panel", children: [
    /* @__PURE__ */ (0, import_jsx_runtime26.jsxs)("div", { className: "view__head", children: [
      /* @__PURE__ */ (0, import_jsx_runtime26.jsx)("h2", { children: "Logs" }),
      /* @__PURE__ */ (0, import_jsx_runtime26.jsxs)("span", { className: "view__sub", children: [
        rows.length,
        " of ",
        frames.length,
        " events \xB7 live"
      ] }),
      /* @__PURE__ */ (0, import_jsx_runtime26.jsx)("span", { className: "view__spacer" }),
      /* @__PURE__ */ (0, import_jsx_runtime26.jsx)(
        "input",
        {
          className: "view__search",
          placeholder: "Filter event, identity\u2026",
          value: q,
          onChange: (e) => setQ(e.target.value)
        }
      ),
      /* @__PURE__ */ (0, import_jsx_runtime26.jsxs)("div", { className: "view__segs", children: [
        /* @__PURE__ */ (0, import_jsx_runtime26.jsxs)("button", { className: lvl === "all" ? "is-active" : "", onClick: () => setLvl("all"), children: [
          "all ",
          /* @__PURE__ */ (0, import_jsx_runtime26.jsx)("span", { className: "n", children: frames.length })
        ] }),
        /* @__PURE__ */ (0, import_jsx_runtime26.jsxs)("button", { className: lvl === "info" ? "is-active" : "", onClick: () => setLvl("info"), children: [
          "info ",
          /* @__PURE__ */ (0, import_jsx_runtime26.jsx)("span", { className: "n", children: counts.info })
        ] }),
        /* @__PURE__ */ (0, import_jsx_runtime26.jsxs)("button", { className: `warn ${lvl === "warn" ? "is-active" : ""}`, onClick: () => setLvl("warn"), children: [
          "warn ",
          /* @__PURE__ */ (0, import_jsx_runtime26.jsx)("span", { className: "n", children: counts.warn })
        ] }),
        /* @__PURE__ */ (0, import_jsx_runtime26.jsxs)("button", { className: `bad ${lvl === "error" ? "is-active" : ""}`, onClick: () => setLvl("error"), children: [
          "err ",
          /* @__PURE__ */ (0, import_jsx_runtime26.jsx)("span", { className: "n", children: counts.error })
        ] })
      ] })
    ] }),
    /* @__PURE__ */ (0, import_jsx_runtime26.jsx)("div", { className: "logs__body", children: /* @__PURE__ */ (0, import_jsx_runtime26.jsxs)("div", { className: "logs__stream", children: [
      rows.map(({ f, level }, i) => {
        const key = f.id || `${f.event}:${f.timestampMs}:${i}`;
        const isOpen = expanded.has(key);
        const hasStructured = hasStructuredOutput(f);
        return /* @__PURE__ */ (0, import_jsx_runtime26.jsxs)(
          "div",
          {
            className: `logline logline--${level}${isOpen ? " is-open" : ""}`,
            "data-testid": `log-line:${f.id || i}`,
            children: [
              /* @__PURE__ */ (0, import_jsx_runtime26.jsxs)(
                "button",
                {
                  type: "button",
                  className: "logline__row",
                  onClick: () => toggle(key),
                  "aria-expanded": isOpen,
                  "data-testid": `log-line:${f.id || i}:toggle`,
                  children: [
                    /* @__PURE__ */ (0, import_jsx_runtime26.jsx)("span", { className: "logline__chevron", children: isOpen ? "\u25BE" : "\u25B8" }),
                    /* @__PURE__ */ (0, import_jsx_runtime26.jsx)("span", { className: "logline__t", children: formatTime2(f.timestampMs) }),
                    /* @__PURE__ */ (0, import_jsx_runtime26.jsx)("span", { className: `logline__lvl logline__lvl--${level}`, children: level.toUpperCase() }),
                    /* @__PURE__ */ (0, import_jsx_runtime26.jsx)("span", { className: "logline__src", children: f.identity || "_system" }),
                    /* @__PURE__ */ (0, import_jsx_runtime26.jsx)("span", { className: "logline__evt", children: f.event }),
                    /* @__PURE__ */ (0, import_jsx_runtime26.jsx)("span", { className: "logline__ctx dim", children: f.interactionId ? `int=${f.interactionId.slice(0, 8)}` : "" }),
                    /* @__PURE__ */ (0, import_jsx_runtime26.jsx)("span", { className: "logline__msg", children: summary(f) }),
                    hasStructured && /* @__PURE__ */ (0, import_jsx_runtime26.jsx)("span", { className: "logline__badge", title: "Carries structured_output", children: "\u21B3 struct" })
                  ]
                }
              ),
              isOpen && /* @__PURE__ */ (0, import_jsx_runtime26.jsx)("pre", { className: "logline__detail", "data-testid": `log-line:${f.id || i}:detail`, children: formatFrameData(f) })
            ]
          },
          key
        );
      }),
      rows.length === 0 && /* @__PURE__ */ (0, import_jsx_runtime26.jsx)("div", { style: { padding: 24, color: "var(--ink-dim)", fontFamily: "var(--mono)", fontSize: 12 }, children: "No matching events." })
    ] }) })
  ] });
}

// src/panels/Topbar.tsx
var import_jsx_runtime27 = require("react/jsx-runtime");
function PanelGlyph({ side, open }) {
  const dividerLeft = side === "left";
  const cx = dividerLeft ? 16.5 : 7.5;
  const point = open ? dividerLeft ? 1 : -1 : dividerLeft ? -1 : 1;
  const x1 = cx + point * 1.6;
  const x2 = cx - point * 1.6;
  return /* @__PURE__ */ (0, import_jsx_runtime27.jsxs)(
    "svg",
    {
      viewBox: "0 0 24 24",
      "aria-hidden": "true",
      focusable: "false",
      children: [
        /* @__PURE__ */ (0, import_jsx_runtime27.jsx)("rect", { x: "3", y: "5", width: "18", height: "14", rx: "1.5" }),
        /* @__PURE__ */ (0, import_jsx_runtime27.jsx)("path", { d: dividerLeft ? "M9 5 L9 19" : "M15 5 L15 19" }),
        /* @__PURE__ */ (0, import_jsx_runtime27.jsx)("path", { d: `M${x1} 9.5 L${x2} 12 L${x1} 14.5` })
      ]
    }
  );
}
function Topbar({
  mobName,
  mobStatus = "idle",
  environment = "dev",
  theme,
  onToggleTheme,
  sidebarCollapsed,
  railCollapsed,
  onToggleSidebar,
  onToggleRail
}) {
  return /* @__PURE__ */ (0, import_jsx_runtime27.jsxs)("div", { className: "mobkit-topbar", "data-testid": "mobkit-topbar", children: [
    /* @__PURE__ */ (0, import_jsx_runtime27.jsx)(
      "button",
      {
        type: "button",
        className: "mobkit-topbar__toggle mobkit-topbar__toggle--left",
        onClick: onToggleSidebar,
        "aria-pressed": !sidebarCollapsed,
        "aria-label": sidebarCollapsed ? "Expand sidebar" : "Collapse sidebar",
        title: sidebarCollapsed ? "Expand sidebar" : "Collapse sidebar",
        "data-testid": "sidebar-collapse-toggle",
        children: /* @__PURE__ */ (0, import_jsx_runtime27.jsx)(PanelGlyph, { side: "left", open: !sidebarCollapsed })
      }
    ),
    /* @__PURE__ */ (0, import_jsx_runtime27.jsxs)("div", { className: "mobkit-topbar__brand", children: [
      /* @__PURE__ */ (0, import_jsx_runtime27.jsx)("span", { className: "mobkit-topbar__brand-mark" }),
      /* @__PURE__ */ (0, import_jsx_runtime27.jsx)("span", { children: "MobKit" })
    ] }),
    /* @__PURE__ */ (0, import_jsx_runtime27.jsxs)("div", { className: "mobkit-topbar__mob", children: [
      /* @__PURE__ */ (0, import_jsx_runtime27.jsx)("span", { className: "mobkit-topbar__mob-status", title: mobStatus }),
      /* @__PURE__ */ (0, import_jsx_runtime27.jsx)("span", { children: mobName }),
      /* @__PURE__ */ (0, import_jsx_runtime27.jsxs)("span", { className: "dim", children: [
        "\xB7 ",
        mobStatus
      ] })
    ] }),
    /* @__PURE__ */ (0, import_jsx_runtime27.jsxs)("div", { className: "mobkit-topbar__mob", children: [
      /* @__PURE__ */ (0, import_jsx_runtime27.jsx)("span", { children: "env:" }),
      /* @__PURE__ */ (0, import_jsx_runtime27.jsx)("span", { children: environment })
    ] }),
    /* @__PURE__ */ (0, import_jsx_runtime27.jsx)("div", { className: "mobkit-topbar__spacer" }),
    /* @__PURE__ */ (0, import_jsx_runtime27.jsx)("div", { className: "mobkit-topbar__util", children: /* @__PURE__ */ (0, import_jsx_runtime27.jsx)(
      "button",
      {
        type: "button",
        onClick: onToggleTheme,
        "data-testid": "theme-toggle",
        title: `Switch to ${theme === "dark" ? "light" : "dark"} mode`,
        children: theme === "dark" ? "\u2600 light" : "\u263E dark"
      }
    ) }),
    /* @__PURE__ */ (0, import_jsx_runtime27.jsx)(
      "button",
      {
        type: "button",
        className: "mobkit-topbar__toggle mobkit-topbar__toggle--right",
        onClick: onToggleRail,
        "aria-pressed": !railCollapsed,
        "aria-label": railCollapsed ? "Expand signals rail" : "Collapse signals rail",
        title: railCollapsed ? "Expand signals rail" : "Collapse signals rail",
        "data-testid": "signals-rail-collapse-toggle",
        children: /* @__PURE__ */ (0, import_jsx_runtime27.jsx)(PanelGlyph, { side: "right", open: !railCollapsed })
      }
    )
  ] });
}

// src/panels/Tweaks.tsx
var import_react19 = __toESM(require("react"));
var VARIANT_STORAGE = "mobkit-console-variant";
function useConsoleVariant() {
  const [v, setV] = import_react19.default.useState(() => {
    try {
      const stored = localStorage.getItem(VARIANT_STORAGE);
      if (stored === "rams" || stored === "terminal" || stored === "graphite") return stored;
    } catch {
    }
    return "rams";
  });
  const set = import_react19.default.useCallback((next) => {
    setV(next);
    try {
      localStorage.setItem(VARIANT_STORAGE, next);
    } catch {
    }
  }, []);
  return [v, set];
}

// src/panels/Sidebar.tsx
var import_react20 = __toESM(require("react"));
var import_jsx_runtime28 = require("react/jsx-runtime");
var ALL_NAV = ["topology", "timeline", "gating", "roster", "routing", "gates", "logs", "health"];
var NAV_LABEL = {
  topology: "Topology",
  timeline: "Today",
  gating: "Gating",
  roster: "Roster",
  routing: "Routing",
  gates: "Gates",
  logs: "Logs",
  health: "Health"
};
function parseNavList(raw) {
  const out = /* @__PURE__ */ new Set();
  if (!raw) return out;
  for (const token of raw.split(",")) {
    const trimmed = token.trim();
    if (ALL_NAV.includes(trimmed)) out.add(trimmed);
  }
  return out;
}
function visibleNavKinds() {
  if (typeof window === "undefined") return ALL_NAV;
  const params = new URLSearchParams(window.location.search);
  const show = parseNavList(params.get("show_nav"));
  if (show.size > 0) return ALL_NAV.filter((k) => show.has(k));
  const hide = parseNavList(params.get("hide_nav"));
  if (hide.size > 0) return ALL_NAV.filter((k) => !hide.has(k));
  return ALL_NAV;
}
function bucketOf(a) {
  const g = (a.group || "").toLowerCase();
  const p = (a.role || a.kind || "").toLowerCase();
  if (g.includes("coordinator") || p.includes("coord") || p.includes("triage") || p.includes("router") || p.includes("commander")) return "Coordinators";
  if (g.includes("personal") || p.includes("personal") || p.includes("identity") || p.includes("lead")) return "Personal";
  if (g.includes("internal") || p.includes("gate") || p.includes("monitor") || p.includes("scribe")) return "Internal";
  if (g.includes("domain") || g.includes("responder") || g.includes("communication") || g.includes("specialist")) return "Domains";
  return "Domains";
}
var SECTION_ORDER = ["Personal", "Coordinators", "Domains", "Internal", "Other"];
function deriveStateAttr(agent) {
  const state = (agent.state || "").toLowerCase();
  if (state === "retired" || state === "stopped") return "retired";
  const degraded = agent.labels?.console_degraded === "true" || state.includes("degrade") || agent.lease_healthy === false;
  if (degraded) return "degraded";
  return "active";
}
function pulseSamples(activity, identity) {
  const bucket = new Array(10).fill(0);
  const now = Date.now();
  const window2 = 15 * 60 * 1e3;
  for (const f of activity) {
    if (!f.timestampMs || (f.identity || "") !== identity) continue;
    const age = now - f.timestampMs;
    if (age < 0 || age > window2) continue;
    const idx = 9 - Math.floor(age / window2 * 10);
    if (idx >= 0 && idx < 10) bucket[idx]++;
  }
  return bucket;
}
function inboxCount(agent) {
  const n = Number(agent.labels?.console_inbox_count ?? 0);
  return Number.isFinite(n) ? n : 0;
}
function Sidebar({
  agents,
  selectedMemberId,
  recentActivity,
  collapsed,
  visibleControls,
  onSelect,
  onInspect,
  onOpenControl
}) {
  const [q, setQ] = import_react20.default.useState("");
  const navKinds = import_react20.default.useMemo(() => {
    const configured = visibleNavKinds();
    if (!visibleControls) return configured;
    const allowed = new Set(visibleControls);
    return configured.filter((kind) => allowed.has(kind));
  }, [visibleControls]);
  const filtered = import_react20.default.useMemo(() => {
    if (!q) return agents;
    const needle = q.toLowerCase();
    return agents.filter(
      (a) => a.label.toLowerCase().includes(needle) || (a.identity || "").toLowerCase().includes(needle) || (a.member_id || "").toLowerCase().includes(needle) || (a.role || "").toLowerCase().includes(needle)
    );
  }, [agents, q]);
  const grouped = import_react20.default.useMemo(() => {
    const g = /* @__PURE__ */ new Map();
    for (const a of filtered) {
      const key = bucketOf(a);
      if (!g.has(key)) g.set(key, []);
      g.get(key).push(a);
    }
    return g;
  }, [filtered]);
  if (collapsed) {
    return /* @__PURE__ */ (0, import_jsx_runtime28.jsx)(
      "aside",
      {
        className: "sidebar sidebar--collapsed",
        "data-collapsed": "true",
        "data-testid": "sidebar-root",
        children: /* @__PURE__ */ (0, import_jsx_runtime28.jsx)("i", { className: "sidebar__grip", "aria-hidden": "true" })
      }
    );
  }
  return /* @__PURE__ */ (0, import_jsx_runtime28.jsxs)("aside", { className: "sidebar", "data-testid": "sidebar-root", children: [
    /* @__PURE__ */ (0, import_jsx_runtime28.jsx)("div", { className: "sidebar__search", children: /* @__PURE__ */ (0, import_jsx_runtime28.jsx)(
      "input",
      {
        placeholder: "Search agents, profiles, ids\u2026",
        value: q,
        onChange: (e) => setQ(e.target.value),
        "data-testid": "sidebar-search"
      }
    ) }),
    navKinds.length > 0 && /* @__PURE__ */ (0, import_jsx_runtime28.jsxs)("div", { className: "sidebar__section sidebar__section--nav", children: [
      /* @__PURE__ */ (0, import_jsx_runtime28.jsx)("div", { className: "sidebar__sec-head", children: /* @__PURE__ */ (0, import_jsx_runtime28.jsx)("span", { className: "sidebar__sec-label", children: "Views" }) }),
      navKinds.map((kind) => /* @__PURE__ */ (0, import_jsx_runtime28.jsx)(
        "button",
        {
          className: "sidebar__navitem",
          onClick: () => onOpenControl(kind),
          "data-testid": `nav:${kind}`,
          children: NAV_LABEL[kind]
        },
        kind
      ))
    ] }),
    SECTION_ORDER.map((bucket) => {
      const list = grouped.get(bucket);
      if (!list || list.length === 0) return null;
      return /* @__PURE__ */ (0, import_jsx_runtime28.jsxs)("div", { className: "sidebar__section", children: [
        /* @__PURE__ */ (0, import_jsx_runtime28.jsxs)("div", { className: "sidebar__sec-head", children: [
          /* @__PURE__ */ (0, import_jsx_runtime28.jsx)("span", { className: "sidebar__sec-label", children: bucket }),
          /* @__PURE__ */ (0, import_jsx_runtime28.jsx)("span", { className: "sidebar__sec-spacer" }),
          /* @__PURE__ */ (0, import_jsx_runtime28.jsx)("span", { className: "sidebar__sec-count", children: list.length })
        ] }),
        list.map((agent) => {
          const stateAttr = deriveStateAttr(agent);
          const pulse = pulseSamples(recentActivity, agent.identity || agent.member_id);
          const inbox = inboxCount(agent);
          return /* @__PURE__ */ (0, import_jsx_runtime28.jsxs)(
            "div",
            {
              className: `agent ${agent.member_id === selectedMemberId ? "is-active" : ""}`,
              "data-state": stateAttr,
              "data-testid": `sidebar-agent:${agent.member_id}`,
              onClick: () => onSelect(agent),
              onContextMenu: (e) => {
                e.preventDefault();
                onInspect(agent);
              },
              role: "button",
              tabIndex: 0,
              children: [
                /* @__PURE__ */ (0, import_jsx_runtime28.jsx)("span", { className: "agent__dot" }),
                /* @__PURE__ */ (0, import_jsx_runtime28.jsxs)("span", { className: "agent__body", children: [
                  /* @__PURE__ */ (0, import_jsx_runtime28.jsx)("span", { className: "agent__name", children: agent.label }),
                  /* @__PURE__ */ (0, import_jsx_runtime28.jsx)("span", { className: "agent__id", children: agent.identity || agent.member_id })
                ] }),
                /* @__PURE__ */ (0, import_jsx_runtime28.jsxs)("span", { className: "agent__meta", children: [
                  /* @__PURE__ */ (0, import_jsx_runtime28.jsx)("span", { className: "agent__pulse", children: pulse.map((v, i) => /* @__PURE__ */ (0, import_jsx_runtime28.jsx)("span", { style: { height: `${Math.max(1, Math.min(12, v * 2 + 1))}px` } }, i)) }),
                  inbox > 0 && /* @__PURE__ */ (0, import_jsx_runtime28.jsx)("span", { className: "agent__inbox", children: inbox })
                ] })
              ]
            },
            agent.member_id
          );
        })
      ] }, bucket);
    })
  ] });
}

// src/panels/SignalsRail.tsx
var import_react21 = __toESM(require("react"));
var import_jsx_runtime29 = require("react/jsx-runtime");
function severityOf(frame) {
  const ev = frame.event;
  if (ev.includes("fail") || ev.includes("error") || ev.includes("crash")) return "critical";
  if (ev === "gating_decision" || ev.includes("warn") || ev.includes("degraded") || ev.includes("retired")) return "warning";
  return "info";
}
function labelFor(frame) {
  const ev = frame.event;
  const d = frame.data || {};
  switch (ev) {
    case "interaction_complete":
      return "Interaction complete";
    case "interaction_failed":
      return "Interaction failed";
    case "interaction_started":
      return "Interaction started";
    case "gating_decision":
      return `Gate ${String(d.decision || "")}`;
    case "member_ready":
      return "Member ready";
    case "member_retired":
      return "Member retired";
    case "state_changed":
      return `State \u2192 ${String(d.state || d.new_state || "")}`;
    case "route_changed":
      return "Route changed";
    case "run_failed":
      return "Run failed";
    default:
      return ev.replace(/_/g, " ");
  }
}
function detailFor(frame) {
  const d = frame.data || {};
  const bits = [];
  if (d.action_id) bits.push(`action=${String(d.action_id)}`);
  if (d.reason) bits.push(String(d.reason));
  if (d.error) bits.push(String(d.error));
  if (frame.interactionId) bits.push(`int=${frame.interactionId.slice(0, 8)}`);
  return bits.join(" \xB7 ") || "\u2014";
}
function timeFor(tsMs) {
  if (!tsMs) return "\u2014";
  const diff = Date.now() - tsMs;
  if (diff < 6e4) return `${Math.max(1, Math.floor(diff / 1e3))}s`;
  if (diff < 36e5) return `${Math.floor(diff / 6e4)}m`;
  return `${Math.floor(diff / 36e5)}h`;
}
function SignalsRail({ frames, collapsed, onSelect }) {
  const [filter, setFilter] = import_react21.default.useState("all");
  const signals = import_react21.default.useMemo(() => {
    return frames.slice(0, 200).map((f, i) => ({
      id: f.id || `${f.event}-${i}`,
      severity: severityOf(f),
      label: labelFor(f),
      detail: detailFor(f),
      agent: f.identity || "_system",
      at: timeFor(f.timestampMs),
      raw: f
    }));
  }, [frames]);
  const counts = import_react21.default.useMemo(() => ({
    all: signals.length,
    critical: signals.filter((s) => s.severity === "critical").length,
    warning: signals.filter((s) => s.severity === "warning").length
  }), [signals]);
  const shown = signals.filter(
    (s) => filter === "all" ? true : filter === "critical" ? s.severity === "critical" : s.severity !== "info"
  );
  const recent15m = signals.filter((s) => Date.now() - (s.raw.timestampMs || 0) < 15 * 60 * 1e3).length;
  if (collapsed) {
    return /* @__PURE__ */ (0, import_jsx_runtime29.jsx)(
      "aside",
      {
        className: "rail rail--collapsed",
        "data-collapsed": "true",
        "data-testid": "signals-rail",
        children: /* @__PURE__ */ (0, import_jsx_runtime29.jsx)("i", { className: "rail__grip", "aria-hidden": "true" })
      }
    );
  }
  return /* @__PURE__ */ (0, import_jsx_runtime29.jsxs)("aside", { className: "rail", "data-testid": "signals-rail", children: [
    /* @__PURE__ */ (0, import_jsx_runtime29.jsxs)("div", { className: "rail__head", children: [
      /* @__PURE__ */ (0, import_jsx_runtime29.jsx)("span", { className: "rail__title", children: "Signals" }),
      /* @__PURE__ */ (0, import_jsx_runtime29.jsxs)("span", { className: "rail__sub", children: [
        recent15m,
        " in 15m"
      ] })
    ] }),
    /* @__PURE__ */ (0, import_jsx_runtime29.jsxs)("div", { className: "rail__filters", children: [
      /* @__PURE__ */ (0, import_jsx_runtime29.jsxs)(
        "button",
        {
          className: `rail__filter ${filter === "all" ? "is-active" : ""}`,
          onClick: () => setFilter("all"),
          "data-testid": "signals-filter:all",
          children: [
            "All ",
            /* @__PURE__ */ (0, import_jsx_runtime29.jsx)("span", { className: "rail__filter-count", children: counts.all })
          ]
        }
      ),
      /* @__PURE__ */ (0, import_jsx_runtime29.jsxs)(
        "button",
        {
          className: `rail__filter ${filter === "warning" ? "is-active" : ""}`,
          onClick: () => setFilter("warning"),
          "data-testid": "signals-filter:warning",
          children: [
            "Attn ",
            /* @__PURE__ */ (0, import_jsx_runtime29.jsx)("span", { className: "rail__filter-count", children: counts.warning + counts.critical })
          ]
        }
      ),
      /* @__PURE__ */ (0, import_jsx_runtime29.jsxs)(
        "button",
        {
          className: `rail__filter ${filter === "critical" ? "is-active" : ""}`,
          onClick: () => setFilter("critical"),
          "data-testid": "signals-filter:critical",
          children: [
            "Crit ",
            /* @__PURE__ */ (0, import_jsx_runtime29.jsx)("span", { className: "rail__filter-count", children: counts.critical })
          ]
        }
      )
    ] }),
    /* @__PURE__ */ (0, import_jsx_runtime29.jsxs)("div", { className: "rail__list", children: [
      shown.length === 0 && /* @__PURE__ */ (0, import_jsx_runtime29.jsx)("div", { style: { padding: "20px 14px", color: "var(--ink-dim)", fontSize: 12, fontFamily: "var(--mono)" }, children: "No signals." }),
      shown.map((s) => /* @__PURE__ */ (0, import_jsx_runtime29.jsxs)(
        "div",
        {
          className: "signal",
          "data-sev": s.severity,
          "data-testid": `signal:${s.id}`,
          onClick: () => onSelect?.(s.raw),
          role: onSelect ? "button" : void 0,
          tabIndex: onSelect ? 0 : void 0,
          children: [
            /* @__PURE__ */ (0, import_jsx_runtime29.jsx)("span", { className: "signal__bar" }),
            /* @__PURE__ */ (0, import_jsx_runtime29.jsxs)("span", { className: "signal__body", children: [
              /* @__PURE__ */ (0, import_jsx_runtime29.jsx)("span", { className: "signal__label", children: s.label }),
              /* @__PURE__ */ (0, import_jsx_runtime29.jsx)("span", { className: "signal__detail", children: s.detail }),
              /* @__PURE__ */ (0, import_jsx_runtime29.jsx)("span", { className: "signal__agent", children: s.agent })
            ] }),
            /* @__PURE__ */ (0, import_jsx_runtime29.jsx)("span", { className: "signal__meta", children: /* @__PURE__ */ (0, import_jsx_runtime29.jsx)("span", { className: "signal__time", children: s.at }) })
          ]
        },
        s.id
      ))
    ] })
  ] });
}

// src/panels/ChatPane.tsx
var import_react22 = __toESM(require("react"));

// src/lib/composer-attachment-text.ts
function composerImageFileKey(file) {
  return [
    file.name || "",
    file.type || "",
    String(file.size),
    String(file.lastModified ?? 0)
  ].join("\0");
}
function dedupeComposerImageFiles(files) {
  const seen = /* @__PURE__ */ new Set();
  const deduped = [];
  for (const file of files) {
    const key = composerImageFileKey(file);
    if (seen.has(key)) continue;
    seen.add(key);
    deduped.push(file);
  }
  return deduped;
}
function defaultBaseHref() {
  if (typeof window !== "undefined") return window.location.href;
  return "http://localhost/";
}
function defaultOrigin(baseHref) {
  if (typeof window !== "undefined") return window.location.origin;
  return new URL(baseHref).origin;
}
function normalizeConsoleBlobUrl(raw, baseHref = defaultBaseHref(), origin = defaultOrigin(baseHref)) {
  try {
    const url = new URL(raw.trim(), baseHref);
    if (url.origin !== origin) return null;
    if (!url.pathname.startsWith("/blobs/")) return null;
    return url.href;
  } catch {
    return null;
  }
}
function consoleBlobReferencesFromText(value, baseHref = defaultBaseHref(), origin = defaultOrigin(baseHref)) {
  const normalized = value.replace(/&amp;/g, "&");
  const candidates = [
    ...Array.from(normalized.matchAll(/\b(?:src|href)=["']([^"']+)["']/gi)).map((match) => match[1]),
    ...Array.from(normalized.matchAll(/(?:https?:\/\/[^\s"'<>]+|\/blobs\/[^\s"'<>]+)/gi)).map((match) => match[0])
  ];
  const refs = [];
  const seen = /* @__PURE__ */ new Set();
  for (const candidate of candidates) {
    const href = normalizeConsoleBlobUrl(candidate, baseHref, origin);
    if (!href || seen.has(href)) continue;
    seen.add(href);
    refs.push({ href, raw: candidate });
  }
  return refs;
}
function consoleBlobUrlsFromText(value) {
  return consoleBlobReferencesFromText(value).map((ref) => ref.href);
}
function stripConsoleBlobReferencesFromText(value, references = consoleBlobReferencesFromText(value)) {
  let next = value;
  for (const ref of references) {
    next = next.split(ref.raw).join("");
    next = next.split(ref.href).join("");
  }
  return next.replace(/[ \t]+\n/g, "\n").replace(/\n{3,}/g, "\n\n").replace(/[ \t]{2,}/g, " ").trim();
}

// src/panels/ChatPane.tsx
var import_jsx_runtime30 = require("react/jsx-runtime");
var ALLOWED_IMAGE_TYPES = /* @__PURE__ */ new Set(["image/png", "image/jpeg", "image/webp", "image/gif"]);
var MAX_ATTACHMENTS = 4;
var MAX_ATTACHMENT_BYTES = 25 * 1024 * 1024;
function phaseLabel(_phase) {
  return "working";
}
function formatTime3(iso) {
  if (!iso) return "";
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return "";
  const hh = String(d.getHours()).padStart(2, "0");
  const mm = String(d.getMinutes()).padStart(2, "0");
  const ss = String(d.getSeconds()).padStart(2, "0");
  return `${hh}:${mm}:${ss}`;
}
function richBlockKind(block, isUser) {
  if (block.type === "tool-call") return "tool";
  if (block.type === "thinking") return "thought";
  return isUser ? "user" : "agent";
}
function flattenEntry(entry) {
  if (entry.kind === "summary") {
    return [{
      id: entry.id,
      kind: "origin",
      time: formatTime3(entry.createdAt),
      text: `${entry.title} (+${entry.plus}/-${entry.minus})`
    }];
  }
  if (entry.variant === "meta") {
    return [{
      id: entry.id,
      kind: "origin",
      time: formatTime3(entry.createdAt),
      text: entry.text || ""
    }];
  }
  const role = entry.identity.role;
  const isUser = role === "user";
  const label = entry.identity.label;
  const time = formatTime3(entry.createdAt);
  if (entry.variant === "rich" && Array.isArray(entry.blocks) && entry.blocks.length > 0) {
    const msgs = [];
    let groupKind = null;
    let groupBlocks = [];
    let groupStart = 0;
    const flushGroup = (endIndex) => {
      if (groupKind === null || groupBlocks.length === 0) return;
      msgs.push({
        id: `${entry.id}:${groupStart}-${endIndex - 1}`,
        kind: groupKind,
        time,
        who: groupKind === "agent" ? label : void 0,
        blocks: groupBlocks
      });
      groupKind = null;
      groupBlocks = [];
    };
    for (let i = 0; i < entry.blocks.length; i++) {
      const block = entry.blocks[i];
      const kind = richBlockKind(block, isUser);
      if (kind !== groupKind) {
        flushGroup(i);
        groupKind = kind;
        groupStart = i;
      }
      groupBlocks.push(block);
    }
    flushGroup(entry.blocks.length);
    return msgs.length ? msgs : [{
      id: entry.id,
      kind: isUser ? "user" : "agent",
      time,
      who: isUser ? void 0 : label,
      text: ""
    }];
  }
  return [{
    id: entry.id,
    kind: isUser ? "user" : "agent",
    time,
    who: isUser ? void 0 : label,
    text: entry.text || ""
  }];
}
function textSignatureForMsg(message) {
  if (message.text) {
    return message.text.replace(/\s+/g, " ").trim();
  }
  if (!Array.isArray(message.blocks) || message.blocks.length === 0) {
    return "";
  }
  const parts = message.blocks.map((block) => {
    if (block.type === "paragraph") return block.text || "";
    if (block.type === "heading") return block.text || "";
    if (block.type === "divider") return block.text || "";
    return "";
  });
  if (parts.some((part) => part.trim().length === 0)) {
    return "";
  }
  return parts.join("\n").replace(/\s+/g, " ").trim();
}
function collectImageTransferPayload(data) {
  const directFiles = Array.from(data.files).filter((file) => file.type.startsWith("image/"));
  const itemFiles = Array.from(data.items).filter((item) => item.kind === "file" && item.type.startsWith("image/")).map((item) => item.getAsFile()).filter((file) => Boolean(file));
  const textPayloads = [
    data.getData("text/html"),
    data.getData("text/uri-list"),
    data.getData("text/plain")
  ].filter(Boolean);
  return { files: dedupeComposerImageFiles([...directFiles, ...itemFiles]), textPayloads };
}
function imageTransferPayloadHasImage(payload) {
  return payload.files.length > 0 || payload.textPayloads.some((text) => imageDataUrlsFromText(text).length > 0 || consoleBlobUrlsFromText(text).length > 0);
}
async function imageFilesFromTransferPayload(payload) {
  if (payload.files.length > 0) {
    return payload.files;
  }
  const files = [];
  const seen = /* @__PURE__ */ new Set();
  for (const text of payload.textPayloads) {
    for (const dataUrl of imageDataUrlsFromText(text)) {
      if (seen.has(dataUrl)) continue;
      seen.add(dataUrl);
      const file = fileFromImageDataUrl(dataUrl);
      if (file) files.push(file);
    }
    for (const blobUrl of consoleBlobUrlsFromText(text)) {
      if (seen.has(blobUrl)) continue;
      seen.add(blobUrl);
      const file = await fileFromConsoleBlobUrl(blobUrl);
      if (file) files.push(file);
    }
  }
  return files;
}
function imageDataUrlsFromText(value) {
  const matches = value.match(/data:image\/(?:png|jpeg|webp|gif);base64,[A-Za-z0-9+/=]+/gi);
  return matches ?? [];
}
function fileFromImageDataUrl(dataUrl) {
  const match = dataUrl.match(/^data:(image\/(?:png|jpeg|webp|gif));base64,([A-Za-z0-9+/=]+)$/i);
  if (!match) return null;
  const [, mediaType, base64] = match;
  try {
    const binary = atob(base64);
    const bytes = new Uint8Array(binary.length);
    for (let i = 0; i < binary.length; i++) {
      bytes[i] = binary.charCodeAt(i);
    }
    const ext = mediaType.split("/")[1]?.replace("jpeg", "jpg") || "png";
    return new File([bytes], `pasted-image.${ext}`, { type: mediaType });
  } catch {
    return null;
  }
}
async function fileFromConsoleBlobUrl(url) {
  try {
    const response = await fetch(url, { credentials: "same-origin" });
    if (!response.ok) return null;
    const mediaType = response.headers.get("content-type")?.split(";")[0]?.trim() || "";
    if (!ALLOWED_IMAGE_TYPES.has(mediaType)) return null;
    const blob = await response.blob();
    const ext = mediaType.split("/")[1]?.replace("jpeg", "jpg") || "png";
    const slug = decodeURIComponent(new URL(url).pathname.split("/").pop() || "blob").replace(/[^A-Za-z0-9._-]/g, "-").slice(0, 80) || "blob";
    return new File([blob], `${slug}.${ext}`, { type: mediaType });
  } catch {
    return null;
  }
}
function ChatPane({
  agent,
  agentLabel,
  identity,
  entries,
  phase,
  draft,
  sending,
  staged,
  onDraftChange,
  onStagedChange,
  onSend,
  onInspect,
  onRespawn,
  onRetire,
  stackSlot
}) {
  const bodyRef = import_react22.default.useRef(null);
  import_react22.default.useEffect(() => {
    const resetTranscriptScroll = () => {
      if (bodyRef.current) {
        bodyRef.current.scrollTop = bodyRef.current.scrollHeight;
        bodyRef.current.scrollLeft = 0;
      }
    };
    resetTranscriptScroll();
    const frame = window.requestAnimationFrame(resetTranscriptScroll);
    return () => window.cancelAnimationFrame(frame);
  }, [entries.length, phase]);
  const messages = import_react22.default.useMemo(() => {
    const flat = entries.flatMap(flattenEntry);
    const merged = [];
    for (const m of flat) {
      const last = merged[merged.length - 1];
      const lastBlocks = last?.blocks;
      const mBlocks = m.blocks;
      const sameName = !!(last && last.kind === "tool" && m.kind === "tool" && Array.isArray(lastBlocks) && lastBlocks.length > 0 && Array.isArray(mBlocks) && mBlocks.length > 0 && lastBlocks.every((b) => b.type === "tool-call") && mBlocks.every((b) => b.type === "tool-call") && lastBlocks[0].type === "tool-call" && mBlocks[0].type === "tool-call" && lastBlocks.every((b) => b.type === "tool-call" && b.name === mBlocks[0].name) && mBlocks.every((b) => b.type === "tool-call" && b.name === mBlocks[0].name));
      const peerCompatible = !sameName ? false : !mBlocks[0].peerTarget ? true : Boolean(lastBlocks[0].peerIncoming) === Boolean(mBlocks[0].peerIncoming);
      if (sameName && peerCompatible && last && lastBlocks && mBlocks) {
        last.blocks = [...lastBlocks, ...mBlocks];
        last.id = `${last.id}+${m.id}`;
      } else {
        if (last && m.kind === "agent" && last.kind === "agent" && last.who === m.who) {
          const lastSignature = textSignatureForMsg(last);
          const nextSignature = textSignatureForMsg(m);
          if (lastSignature && lastSignature === nextSignature) {
            continue;
          }
        }
        merged.push({ ...m });
      }
    }
    return merged;
  }, [entries]);
  const initial = (agentLabel || "?").trim().charAt(0).toUpperCase() || "?";
  const state = (agent?.state || "unknown").toLowerCase();
  const canAttachImages = agent?.model_capabilities?.image_input === true;
  const [dragActive, setDragActive] = import_react22.default.useState(false);
  const [attachmentError, setAttachmentError] = import_react22.default.useState(null);
  const fileInputRef = import_react22.default.useRef(null);
  const resolvedDraftBlobRefs = import_react22.default.useRef("");
  function addFiles(fileList) {
    if (!canAttachImages) return;
    const files = dedupeComposerImageFiles(Array.from(fileList));
    const accepted = [];
    let error = null;
    for (const file of files) {
      if (!ALLOWED_IMAGE_TYPES.has(file.type)) {
        error = "Unsupported image type";
        continue;
      }
      if (file.size > MAX_ATTACHMENT_BYTES) {
        error = "Image exceeds 25 MiB";
        continue;
      }
      accepted.push({
        id: `${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 8)}`,
        file,
        previewUrl: URL.createObjectURL(file)
      });
    }
    onStagedChange((current) => {
      const currentKeys = new Set(current.map((item) => composerImageFileKey(item.file)));
      const append = [];
      for (const item of accepted) {
        const key = composerImageFileKey(item.file);
        if (currentKeys.has(key)) {
          URL.revokeObjectURL(item.previewUrl);
          continue;
        }
        currentKeys.add(key);
        if (current.length + append.length >= MAX_ATTACHMENTS) {
          URL.revokeObjectURL(item.previewUrl);
          error = `Maximum ${MAX_ATTACHMENTS} images`;
          continue;
        }
        append.push(item);
      }
      return [...current, ...append];
    });
    setAttachmentError(error);
  }
  function removeAttachment(id) {
    onStagedChange((current) => {
      const removed = current.find((item) => item.id === id);
      if (removed) URL.revokeObjectURL(removed.previewUrl);
      return current.filter((item) => item.id !== id);
    });
  }
  import_react22.default.useEffect(() => {
    if (!canAttachImages) return;
    const refs = consoleBlobReferencesFromText(draft);
    if (refs.length === 0) {
      resolvedDraftBlobRefs.current = "";
      return;
    }
    const signature = refs.map((ref) => ref.href).join("\n");
    if (signature === resolvedDraftBlobRefs.current) return;
    let cancelled = false;
    const timer = window.setTimeout(() => {
      void (async () => {
        const files = [];
        const seen = /* @__PURE__ */ new Set();
        for (const ref of refs) {
          if (seen.has(ref.href)) continue;
          seen.add(ref.href);
          const file = await fileFromConsoleBlobUrl(ref.href);
          if (file) files.push(file);
        }
        if (cancelled) return;
        if (files.length > 0) {
          resolvedDraftBlobRefs.current = signature;
          addFiles(files);
          onDraftChange(stripConsoleBlobReferencesFromText(draft, refs));
        } else {
          setAttachmentError("No usable image found");
        }
      })();
    }, 350);
    return () => {
      cancelled = true;
      window.clearTimeout(timer);
    };
  }, [canAttachImages, draft, onDraftChange]);
  async function submitComposer() {
    if (staged.length > 0 && !canAttachImages) {
      setAttachmentError("model cannot see images");
      return;
    }
    if (!draft.trim() && staged.length === 0) {
      return;
    }
    const files = staged.map((item) => item.file);
    try {
      const sent = await onSend(files);
      if (sent) {
        staged.forEach((item) => URL.revokeObjectURL(item.previewUrl));
        onStagedChange([]);
        setAttachmentError(null);
      }
    } catch {
      setAttachmentError("send failed; images retained");
    }
  }
  return /* @__PURE__ */ (0, import_jsx_runtime30.jsxs)("div", { className: "conv", "data-testid": `chat-pane:${identity}`, children: [
    /* @__PURE__ */ (0, import_jsx_runtime30.jsxs)("div", { className: "conv__head", children: [
      /* @__PURE__ */ (0, import_jsx_runtime30.jsx)("div", { className: "conv__avatar", children: initial }),
      /* @__PURE__ */ (0, import_jsx_runtime30.jsxs)("div", { style: { minWidth: 0 }, children: [
        /* @__PURE__ */ (0, import_jsx_runtime30.jsx)("div", { className: "conv__title", children: agentLabel }),
        /* @__PURE__ */ (0, import_jsx_runtime30.jsxs)("div", { className: "conv__identity", children: [
          identity,
          agent?.role ? ` \xB7 ${agent.role}` : ""
        ] })
      ] }),
      /* @__PURE__ */ (0, import_jsx_runtime30.jsxs)("div", { className: "conv__actions", children: [
        /* @__PURE__ */ (0, import_jsx_runtime30.jsx)("button", { className: "conv__action", onClick: onInspect, "data-testid": "conv-action:inspect", children: "Inspect" }),
        agent?.affordances?.can_respawn && onRespawn ? /* @__PURE__ */ (0, import_jsx_runtime30.jsx)("button", { className: "conv__action", onClick: onRespawn, "data-testid": "conv-action:respawn", children: "Respawn" }) : null,
        agent?.affordances?.can_retire && onRetire ? /* @__PURE__ */ (0, import_jsx_runtime30.jsx)("button", { className: "conv__action", onClick: onRetire, "data-testid": "conv-action:retire", children: "Retire" }) : null
      ] })
    ] }),
    /* @__PURE__ */ (0, import_jsx_runtime30.jsxs)(
      "div",
      {
        className: "conv__body",
        onScroll: (event) => {
          if (event.currentTarget.scrollLeft !== 0) {
            event.currentTarget.scrollLeft = 0;
          }
        },
        ref: bodyRef,
        children: [
          messages.length === 0 && /* @__PURE__ */ (0, import_jsx_runtime30.jsxs)("div", { className: "msg msg--origin", children: [
            /* @__PURE__ */ (0, import_jsx_runtime30.jsx)("div", { className: "msg__time" }),
            /* @__PURE__ */ (0, import_jsx_runtime30.jsx)("div", { className: "msg__bubble", children: /* @__PURE__ */ (0, import_jsx_runtime30.jsxs)("span", { className: "msg__text", children: [
              "No messages yet. Say hello to ",
              agentLabel,
              "."
            ] }) })
          ] }),
          messages.map((m) => /* @__PURE__ */ (0, import_jsx_runtime30.jsxs)("div", { className: `msg msg--${m.kind}`, children: [
            /* @__PURE__ */ (0, import_jsx_runtime30.jsx)("div", { className: "msg__time", children: m.time }),
            /* @__PURE__ */ (0, import_jsx_runtime30.jsxs)("div", { className: "msg__bubble", children: [
              m.kind === "user" && m.who && /* @__PURE__ */ (0, import_jsx_runtime30.jsx)("span", { className: "msg__who", children: /* @__PURE__ */ (0, import_jsx_runtime30.jsx)("b", { children: m.who }) }),
              m.kind === "agent" && m.who && /* @__PURE__ */ (0, import_jsx_runtime30.jsx)("span", { className: "msg__who", children: /* @__PURE__ */ (0, import_jsx_runtime30.jsx)("b", { children: m.who }) }),
              m.blocks && m.blocks.length > 0 ? /* @__PURE__ */ (0, import_jsx_runtime30.jsx)(ConversationRichContent, { blocks: m.blocks }) : m.text && /* @__PURE__ */ (0, import_jsx_runtime30.jsx)("span", { className: "msg__text", children: m.text })
            ] })
          ] }, m.id)),
          phase && /* @__PURE__ */ (0, import_jsx_runtime30.jsxs)(
            "div",
            {
              className: "msg msg--typing",
              "data-testid": `chat-typing:${identity}`,
              "aria-live": "polite",
              "aria-label": `${agentLabel} is ${phaseLabel(phase)}`,
              children: [
                /* @__PURE__ */ (0, import_jsx_runtime30.jsx)("div", { className: "msg__time" }),
                /* @__PURE__ */ (0, import_jsx_runtime30.jsx)("div", { className: "msg__bubble", children: /* @__PURE__ */ (0, import_jsx_runtime30.jsxs)("span", { className: "msg__typing", children: [
                  /* @__PURE__ */ (0, import_jsx_runtime30.jsxs)("span", { className: "msg__typing-dots", "aria-hidden": "true", children: [
                    /* @__PURE__ */ (0, import_jsx_runtime30.jsx)("span", {}),
                    /* @__PURE__ */ (0, import_jsx_runtime30.jsx)("span", {}),
                    /* @__PURE__ */ (0, import_jsx_runtime30.jsx)("span", {})
                  ] }),
                  /* @__PURE__ */ (0, import_jsx_runtime30.jsx)("span", { className: "msg__typing-label", children: phaseLabel(phase) })
                ] }) })
              ]
            }
          )
        ]
      }
    ),
    stackSlot,
    /* @__PURE__ */ (0, import_jsx_runtime30.jsxs)("div", { className: "composer", children: [
      /* @__PURE__ */ (0, import_jsx_runtime30.jsxs)(
        "div",
        {
          className: `composer__shell${dragActive && canAttachImages ? " is-drag-active" : ""}`,
          onDragLeave: () => setDragActive(false),
          onDragOver: (event) => {
            if (!canAttachImages) return;
            event.preventDefault();
            setDragActive(true);
          },
          onDrop: (event) => {
            if (!canAttachImages) return;
            event.preventDefault();
            setDragActive(false);
            const payload = collectImageTransferPayload(event.dataTransfer);
            void imageFilesFromTransferPayload(payload).then((files) => {
              if (files.length > 0) {
                addFiles(files);
              } else {
                setAttachmentError("No usable image found");
              }
            });
          },
          onPaste: (event) => {
            if (!canAttachImages) return;
            const payload = collectImageTransferPayload(event.clipboardData);
            if (imageTransferPayloadHasImage(payload)) {
              event.preventDefault();
              void imageFilesFromTransferPayload(payload).then((files) => {
                if (files.length > 0) {
                  addFiles(files);
                } else {
                  setAttachmentError("No usable image found");
                }
              });
            }
          },
          children: [
            staged.length > 0 && /* @__PURE__ */ (0, import_jsx_runtime30.jsx)("div", { className: "composer__attachments", children: staged.map((item) => /* @__PURE__ */ (0, import_jsx_runtime30.jsxs)("div", { className: "composer__attachment", children: [
              /* @__PURE__ */ (0, import_jsx_runtime30.jsx)("img", { alt: "", src: item.previewUrl }),
              /* @__PURE__ */ (0, import_jsx_runtime30.jsx)("button", { "aria-label": "Remove attachment", onClick: () => removeAttachment(item.id), type: "button", children: "\xD7" })
            ] }, item.id)) }),
            /* @__PURE__ */ (0, import_jsx_runtime30.jsx)(
              "textarea",
              {
                placeholder: `Message ${agentLabel}\u2026    @ to mention, / for commands`,
                value: draft,
                onChange: (e) => onDraftChange(e.target.value),
                onKeyDown: (e) => {
                  if (e.key === "Enter" && !e.shiftKey) {
                    e.preventDefault();
                    submitComposer();
                  }
                },
                disabled: sending,
                rows: 2,
                "data-testid": `chat-composer:${identity}`
              }
            ),
            /* @__PURE__ */ (0, import_jsx_runtime30.jsx)(
              "input",
              {
                accept: "image/png,image/jpeg,image/webp,image/gif",
                hidden: true,
                multiple: true,
                onChange: (event) => {
                  if (event.target.files) addFiles(event.target.files);
                  event.currentTarget.value = "";
                },
                ref: fileInputRef,
                type: "file"
              }
            ),
            /* @__PURE__ */ (0, import_jsx_runtime30.jsxs)("div", { className: "composer__row", children: [
              canAttachImages && /* @__PURE__ */ (0, import_jsx_runtime30.jsx)(
                "button",
                {
                  className: "composer__chip composer__chip--button",
                  onClick: () => fileInputRef.current?.click(),
                  type: "button",
                  children: "+"
                }
              ),
              /* @__PURE__ */ (0, import_jsx_runtime30.jsxs)("span", { className: "composer__chip", children: [
                /* @__PURE__ */ (0, import_jsx_runtime30.jsx)("span", { className: "k", children: "/" }),
                " commands"
              ] }),
              /* @__PURE__ */ (0, import_jsx_runtime30.jsxs)("span", { className: "composer__chip", children: [
                /* @__PURE__ */ (0, import_jsx_runtime30.jsx)("span", { className: "k", children: "@" }),
                " mention"
              ] }),
              /* @__PURE__ */ (0, import_jsx_runtime30.jsx)("span", { className: "composer__chip mono", children: agent?.role || "agent" }),
              /* @__PURE__ */ (0, import_jsx_runtime30.jsx)("span", { className: "composer__spacer" }),
              /* @__PURE__ */ (0, import_jsx_runtime30.jsx)(
                "button",
                {
                  className: "composer__send",
                  disabled: !draft.trim() && staged.length === 0 || staged.length > 0 && !canAttachImages || sending,
                  onClick: submitComposer,
                  "data-testid": `chat-send:${identity}`,
                  children: "Send  \u23CE"
                }
              )
            ] })
          ]
        }
      ),
      /* @__PURE__ */ (0, import_jsx_runtime30.jsxs)("div", { className: "composer__footer", children: [
        /* @__PURE__ */ (0, import_jsx_runtime30.jsxs)("span", { children: [
          "To: ",
          /* @__PURE__ */ (0, import_jsx_runtime30.jsx)("b", { style: { color: "var(--ink-muted)" }, children: agentLabel })
        ] }),
        /* @__PURE__ */ (0, import_jsx_runtime30.jsx)("span", { children: "\xB7" }),
        /* @__PURE__ */ (0, import_jsx_runtime30.jsx)("span", { className: "mono", children: identity }),
        agent?.role && /* @__PURE__ */ (0, import_jsx_runtime30.jsxs)(import_jsx_runtime30.Fragment, { children: [
          /* @__PURE__ */ (0, import_jsx_runtime30.jsx)("span", { children: "\xB7" }),
          /* @__PURE__ */ (0, import_jsx_runtime30.jsx)("span", { children: agent.role })
        ] }),
        /* @__PURE__ */ (0, import_jsx_runtime30.jsx)("span", { children: "\xB7" }),
        /* @__PURE__ */ (0, import_jsx_runtime30.jsx)("span", { className: "dot", style: {
          background: state === "active" || state === "running" ? "var(--ok)" : state.includes("degrade") ? "var(--warn)" : state === "retired" ? "var(--ink-faint)" : "var(--ink-dim)"
        } }),
        /* @__PURE__ */ (0, import_jsx_runtime30.jsx)("span", { children: state }),
        phase && /* @__PURE__ */ (0, import_jsx_runtime30.jsxs)(import_jsx_runtime30.Fragment, { children: [
          /* @__PURE__ */ (0, import_jsx_runtime30.jsx)("span", { children: "\xB7" }),
          /* @__PURE__ */ (0, import_jsx_runtime30.jsx)("span", { style: { color: "var(--accent)" }, children: phase })
        ] }),
        !canAttachImages && /* @__PURE__ */ (0, import_jsx_runtime30.jsxs)(import_jsx_runtime30.Fragment, { children: [
          /* @__PURE__ */ (0, import_jsx_runtime30.jsx)("span", { children: "\xB7" }),
          /* @__PURE__ */ (0, import_jsx_runtime30.jsx)("span", { children: "model cannot see images" })
        ] }),
        attachmentError && /* @__PURE__ */ (0, import_jsx_runtime30.jsxs)(import_jsx_runtime30.Fragment, { children: [
          /* @__PURE__ */ (0, import_jsx_runtime30.jsx)("span", { children: "\xB7" }),
          /* @__PURE__ */ (0, import_jsx_runtime30.jsx)("span", { style: { color: "var(--bad)" }, children: attachmentError })
        ] })
      ] })
    ] })
  ] });
}

// src/panels/MobKitDock.tsx
var import_react23 = __toESM(require("react"));
var import_jsx_runtime31 = require("react/jsx-runtime");
function tabPanelCount(node) {
  if (!node) return 0;
  if (node.kind === "panel") return 1;
  return tabPanelCount(node.first) + tabPanelCount(node.second);
}
function MobKitDock({
  viewState,
  agents,
  renderPanelBody,
  visibleControls,
  onSelectTab,
  onCloseTab,
  onCreateTab,
  onFocusPanel,
  onSplitPanel,
  onClosePanel,
  onResizeSplit,
  onOpenTargetInPanel
}) {
  const activeTab = viewState.tabs.find((t) => t.id === viewState.activeTabId) || viewState.tabs[0];
  import_react23.default.useEffect(() => {
    function onKey(e) {
      const mod = e.metaKey || e.ctrlKey;
      if (!mod) return;
      const focusedId = viewState.focusedPanelId;
      if (e.key === "d" && !e.shiftKey) {
        if (!focusedId) return;
        e.preventDefault();
        onSplitPanel(focusedId, "right");
      } else if (e.key === "D" && e.shiftKey) {
        if (!focusedId) return;
        e.preventDefault();
        onSplitPanel(focusedId, "down");
      } else if (e.key === "w" && !e.shiftKey) {
        if (!focusedId) return;
        e.preventDefault();
        onClosePanel(focusedId);
      } else if (e.key === "t" && !e.shiftKey) {
        e.preventDefault();
        onCreateTab();
      } else if (e.key === "]" && e.shiftKey) {
        e.preventDefault();
        const idx = viewState.tabs.findIndex((t) => t.id === viewState.activeTabId);
        const next = viewState.tabs[(idx + 1) % viewState.tabs.length];
        if (next) onSelectTab(next.id);
      } else if (e.key === "[" && e.shiftKey) {
        e.preventDefault();
        const idx = viewState.tabs.findIndex((t) => t.id === viewState.activeTabId);
        const prev = viewState.tabs[(idx - 1 + viewState.tabs.length) % viewState.tabs.length];
        if (prev) onSelectTab(prev.id);
      }
    }
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [viewState, onSplitPanel, onClosePanel, onCreateTab, onSelectTab]);
  return /* @__PURE__ */ (0, import_jsx_runtime31.jsxs)("div", { className: "mkdock", "data-testid": "mkdock", children: [
    /* @__PURE__ */ (0, import_jsx_runtime31.jsxs)("div", { className: "wstabs", children: [
      viewState.tabs.map((t) => {
        const isActive = t.id === activeTab?.id;
        const count = tabPanelCount(t.layout);
        return /* @__PURE__ */ (0, import_jsx_runtime31.jsxs)(
          "div",
          {
            className: `wstab ${isActive ? "is-active" : ""}`,
            onClick: () => onSelectTab(t.id),
            "data-testid": `wstab:${t.id}`,
            children: [
              /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("span", { className: "wstab__mark" }),
              /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("span", { className: "wstab__name", children: t.title || "untitled" }),
              count > 1 && /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("span", { className: "wstab__count", children: count }),
              viewState.tabs.length > 1 && /* @__PURE__ */ (0, import_jsx_runtime31.jsx)(
                "button",
                {
                  className: "wstab__close",
                  onClick: (e) => {
                    e.stopPropagation();
                    onCloseTab(t.id);
                  },
                  "data-testid": `wstab-close:${t.id}`,
                  "aria-label": "Close workspace",
                  children: "\xD7"
                }
              )
            ]
          },
          t.id
        );
      }),
      /* @__PURE__ */ (0, import_jsx_runtime31.jsx)(
        "button",
        {
          className: "wstab__add",
          onClick: onCreateTab,
          "data-testid": "wstab-add",
          title: "New workspace (\u2318T)",
          "aria-label": "New workspace",
          children: "+"
        }
      )
    ] }),
    /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("div", { className: "dock", children: activeTab && /* @__PURE__ */ (0, import_jsx_runtime31.jsx)(
      DockLayout,
      {
        node: activeTab.layout,
        viewState,
        agents,
        visibleControls,
        renderPanelBody,
        onFocusPanel,
        onSplitPanel,
        onClosePanel,
        onResizeSplit,
        onOpenTargetInPanel
      }
    ) })
  ] });
}
function DockLayout(props) {
  const { node } = props;
  if (node.kind === "panel") {
    return /* @__PURE__ */ (0, import_jsx_runtime31.jsx)(PaneView, { panelId: node.panelId, ...props });
  }
  return /* @__PURE__ */ (0, import_jsx_runtime31.jsx)(SplitView, { node, ...props });
}
function SplitView(props) {
  const { node } = props;
  if (node.kind !== "split") return null;
  const ratio = typeof node.ratio === "number" ? Math.max(0.1, Math.min(0.9, node.ratio)) : 0.5;
  const direction = node.direction;
  const style = direction === "horizontal" ? { gridTemplateColumns: `${ratio * 100}% 6px ${(1 - ratio) * 100}%` } : { gridTemplateRows: `${ratio * 100}% 6px ${(1 - ratio) * 100}%` };
  const hostRef = import_react23.default.useRef(null);
  function startDrag(e) {
    e.preventDefault();
    const host = hostRef.current;
    if (!host) return;
    const rect = host.getBoundingClientRect();
    e.currentTarget.setPointerCapture(e.pointerId);
    function move(ev) {
      const r2 = direction === "horizontal" ? (ev.clientX - rect.left) / rect.width : (ev.clientY - rect.top) / rect.height;
      props.onResizeSplit(node.id, Math.max(0.1, Math.min(0.9, r2)));
    }
    function end() {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", end);
      window.removeEventListener("pointercancel", end);
    }
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", end);
    window.addEventListener("pointercancel", end);
  }
  return /* @__PURE__ */ (0, import_jsx_runtime31.jsxs)(
    "div",
    {
      ref: hostRef,
      className: `split split--${direction === "horizontal" ? "h" : "v"}`,
      style,
      children: [
        /* @__PURE__ */ (0, import_jsx_runtime31.jsx)(DockLayout, { ...props, node: node.first }),
        /* @__PURE__ */ (0, import_jsx_runtime31.jsx)(
          "div",
          {
            className: `split__handle split__handle--${direction === "horizontal" ? "h" : "v"}`,
            onPointerDown: startDrag,
            "data-testid": `split-handle:${node.id}`
          }
        ),
        /* @__PURE__ */ (0, import_jsx_runtime31.jsx)(DockLayout, { ...props, node: node.second })
      ]
    }
  );
}
function PaneView({
  panelId,
  viewState,
  agents,
  renderPanelBody,
  visibleControls,
  onFocusPanel,
  onSplitPanel,
  onClosePanel,
  onOpenTargetInPanel
}) {
  const panel = viewState.panels.find((p) => p.id === panelId);
  if (!panel) return null;
  const isFocused = viewState.focusedPanelId === panelId;
  const title = panel.title || panel.target?.title || "untitled";
  const target = panel.target;
  const subId = target?.kind === "agent-chat" ? target.identity || target.memberId : target?.kind === "identity-inspect" ? target.identity : void 0;
  const [menuOpen, setMenuOpen] = import_react23.default.useState(false);
  return /* @__PURE__ */ (0, import_jsx_runtime31.jsxs)(
    "div",
    {
      className: `pane ${isFocused ? "is-focused" : ""}`,
      onMouseDown: () => onFocusPanel(panelId),
      "data-testid": `pane:${panelId}`,
      children: [
        /* @__PURE__ */ (0, import_jsx_runtime31.jsxs)("div", { className: "pane__bar", children: [
          /* @__PURE__ */ (0, import_jsx_runtime31.jsxs)(
            "button",
            {
              className: "pane__title",
              onClick: (e) => {
                e.stopPropagation();
                onFocusPanel(panelId);
                setMenuOpen((v) => !v);
              },
              "data-testid": `pane-title:${panelId}`,
              title: "Retarget pane",
              children: [
                /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("span", { className: "pane__title-text", children: title }),
                /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("span", { className: "pane__caret", children: "\u25BE" })
              ]
            }
          ),
          subId && /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("span", { className: "pane__id", children: subId }),
          /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("span", { className: "pane__spacer" }),
          /* @__PURE__ */ (0, import_jsx_runtime31.jsx)(
            "button",
            {
              className: "pane__btn",
              onClick: (e) => {
                e.stopPropagation();
                onSplitPanel(panelId, "right");
              },
              title: "Split right (\u2318D)",
              "data-testid": `pane-split-right:${panelId}`,
              children: "\u25E8"
            }
          ),
          /* @__PURE__ */ (0, import_jsx_runtime31.jsx)(
            "button",
            {
              className: "pane__btn",
              onClick: (e) => {
                e.stopPropagation();
                onSplitPanel(panelId, "down");
              },
              title: "Split down (\u2318\u21E7D)",
              "data-testid": `pane-split-down:${panelId}`,
              children: "\u2B13"
            }
          ),
          /* @__PURE__ */ (0, import_jsx_runtime31.jsx)(
            "button",
            {
              className: "pane__btn pane__close",
              onClick: (e) => {
                e.stopPropagation();
                onClosePanel(panelId);
              },
              title: "Close pane (\u2318W)",
              "data-testid": `pane-close:${panelId}`,
              children: "\xD7"
            }
          ),
          menuOpen && /* @__PURE__ */ (0, import_jsx_runtime31.jsx)(
            PaneMenu,
            {
              agents,
              visibleControls,
              onClose: () => setMenuOpen(false),
              onPick: (target2) => {
                setMenuOpen(false);
                onOpenTargetInPanel(panelId, target2);
              }
            }
          )
        ] }),
        /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("div", { className: "pane__body", children: renderPanelBody({ id: panelId, target }) })
      ]
    }
  );
}
function PaneMenu({ agents, visibleControls, onClose, onPick }) {
  const controls = [
    ["topology", "Topology"],
    ["timeline", "Today"],
    ["gating", "Gating"],
    ["roster", "Roster"],
    ["routing", "Routing"],
    ["gates", "Gates"],
    ["logs", "Logs"],
    ["health", "Health"]
  ].filter(([kind]) => !visibleControls || visibleControls.includes(kind));
  return /* @__PURE__ */ (0, import_jsx_runtime31.jsxs)(import_jsx_runtime31.Fragment, { children: [
    /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("div", { className: "pane-menu__scrim", onMouseDown: onClose }),
    /* @__PURE__ */ (0, import_jsx_runtime31.jsxs)("div", { className: "pane-menu", onMouseDown: (e) => e.stopPropagation(), children: [
      /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("div", { className: "pane-menu__label", children: "Views" }),
      controls.map(([kind, label]) => /* @__PURE__ */ (0, import_jsx_runtime31.jsxs)(
        "button",
        {
          className: "pane-menu__item",
          onClick: () => onPick(buildControlTarget(kind)),
          "data-testid": `pane-menu-view:${kind}`,
          children: [
            /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("span", {}),
            /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("span", { children: label }),
            /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("span", { className: "pane-menu__id", children: "view" })
          ]
        },
        kind
      )),
      /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("div", { className: "pane-menu__sep" }),
      /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("div", { className: "pane-menu__label", children: "Agents" }),
      agents.slice(0, 14).map((a) => /* @__PURE__ */ (0, import_jsx_runtime31.jsxs)(
        "button",
        {
          className: "pane-menu__item",
          "data-state": (a.state || "").toLowerCase(),
          onClick: () => onPick(buildDockTarget(a)),
          "data-testid": `pane-menu-agent:${a.member_id}`,
          children: [
            /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("span", { className: "agent__dot" }),
            /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("span", { children: a.label }),
            /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("span", { className: "pane-menu__id", children: a.identity || a.member_id })
          ]
        },
        a.member_id
      ))
    ] })
  ] });
}

// src/panels/PendingStack.tsx
var import_react24 = __toESM(require("react"));
var import_jsx_runtime32 = require("react/jsx-runtime");
function StackHead({
  count,
  agentBusy,
  collapsed,
  onToggleCollapsed,
  onClear
}) {
  return /* @__PURE__ */ (0, import_jsx_runtime32.jsxs)("div", { className: "stack__head", children: [
    /* @__PURE__ */ (0, import_jsx_runtime32.jsx)(
      "button",
      {
        type: "button",
        className: "stack__head-btn",
        onClick: onToggleCollapsed,
        "aria-expanded": !collapsed,
        "aria-label": collapsed ? "Expand pending queue" : "Collapse pending queue",
        title: collapsed ? "Expand queue" : "Collapse queue",
        children: /* @__PURE__ */ (0, import_jsx_runtime32.jsx)("span", { className: "stack__head-chev", children: collapsed ? "\u25B8" : "\u25BE" })
      }
    ),
    /* @__PURE__ */ (0, import_jsx_runtime32.jsx)("span", { children: "Queue" }),
    /* @__PURE__ */ (0, import_jsx_runtime32.jsx)("span", { className: "stack__head-count", children: String(count).padStart(2, "0") }),
    !collapsed && count > 1 && /* @__PURE__ */ (0, import_jsx_runtime32.jsx)("span", { className: "stack__head-hint", children: "\xB7 drains top \u2192 bottom" }),
    /* @__PURE__ */ (0, import_jsx_runtime32.jsx)("span", { className: "stack__head-spacer" }),
    /* @__PURE__ */ (0, import_jsx_runtime32.jsxs)("span", { className: `stack__head-phase ${agentBusy ? "" : "is-idle"}`, children: [
      /* @__PURE__ */ (0, import_jsx_runtime32.jsx)("b", {}),
      agentBusy ? "Agent busy" : "Agent idle \xB7 draining"
    ] }),
    count > 0 && /* @__PURE__ */ (0, import_jsx_runtime32.jsx)(
      "button",
      {
        type: "button",
        className: "stack__head-btn",
        onClick: onClear,
        "aria-label": "Clear all queued messages",
        title: "Clear all",
        children: "Clear"
      }
    )
  ] });
}
function timeAgo(ts) {
  const s = Math.max(0, Math.round((Date.now() - ts) / 1e3));
  if (s < 5) return "just now";
  if (s < 60) return `${s}s`;
  const m = Math.floor(s / 60);
  if (m < 60) return `${m}m`;
  return `${Math.floor(m / 60)}h`;
}
function StackItem({
  item,
  isHead,
  dragging,
  dropHint,
  onSteer,
  onTrash,
  onEdit,
  onCommitEdit,
  onCancelEdit,
  onToggleExpand,
  onDragStart,
  onDragOver,
  onDragLeave,
  onDrop,
  onDragEnd
}) {
  const taRef = import_react24.default.useRef(null);
  const [draft, setDraft] = import_react24.default.useState(item.text);
  import_react24.default.useEffect(() => {
    if (item.editing && taRef.current) {
      taRef.current.focus();
      const len = taRef.current.value.length;
      taRef.current.setSelectionRange(len, len);
      taRef.current.style.height = "auto";
      taRef.current.style.height = taRef.current.scrollHeight + "px";
    }
  }, [item.editing]);
  import_react24.default.useEffect(() => {
    setDraft(item.text);
  }, [item.text, item.editing]);
  const handleEditKey = (e) => {
    if (e.key === "Escape") {
      e.preventDefault();
      onCancelEdit(item.id);
    } else if (e.key === "Enter" && (!e.shiftKey || e.metaKey || e.ctrlKey)) {
      e.preventDefault();
      onCommitEdit(item.id, draft);
    }
  };
  const cls = [
    "stk-item",
    isHead ? "is-head" : "",
    item.editing ? "is-editing" : "",
    item.status === "promoting" ? "is-promoting" : "",
    item.status === "trashing" ? "is-trashing" : "",
    item.status === "draining" ? "is-draining" : "",
    item.status === "entering" ? "is-entering" : "",
    dragging ? "is-dragging" : "",
    dropHint === "above" ? "drop-target drop-above" : "",
    dropHint === "below" ? "drop-target drop-below" : ""
  ].filter(Boolean).join(" ");
  const longText = item.text.length > 90 || /\n/.test(item.text);
  return /* @__PURE__ */ (0, import_jsx_runtime32.jsxs)(
    "li",
    {
      className: cls,
      role: "listitem",
      tabIndex: 0,
      "data-id": item.id,
      "data-testid": `pending-item:${item.id}`,
      draggable: !item.editing && item.status !== "promoting",
      onDragStart: (e) => onDragStart(e, item.id),
      onDragOver: (e) => onDragOver(e, item.id),
      onDragLeave: (e) => onDragLeave(e, item.id),
      onDrop: (e) => onDrop(e, item.id),
      onDragEnd,
      onKeyDown: (e) => {
        if (item.editing) return;
        if ((e.key === "Delete" || e.key === "Backspace") && (e.metaKey || e.ctrlKey)) {
          e.preventDefault();
          onTrash(item.id);
        }
      },
      children: [
        /* @__PURE__ */ (0, import_jsx_runtime32.jsxs)("div", { className: "stk-item__lead", children: [
          /* @__PURE__ */ (0, import_jsx_runtime32.jsxs)("span", { className: "stk-item__grip", "aria-label": "Drag to reorder", title: "Drag to reorder", children: [
            /* @__PURE__ */ (0, import_jsx_runtime32.jsx)("span", {}),
            /* @__PURE__ */ (0, import_jsx_runtime32.jsx)("span", {}),
            /* @__PURE__ */ (0, import_jsx_runtime32.jsx)("span", {}),
            /* @__PURE__ */ (0, import_jsx_runtime32.jsx)("span", {}),
            /* @__PURE__ */ (0, import_jsx_runtime32.jsx)("span", {}),
            /* @__PURE__ */ (0, import_jsx_runtime32.jsx)("span", {})
          ] }),
          /* @__PURE__ */ (0, import_jsx_runtime32.jsx)("span", { className: "stk-item__queue-glyph", "aria-hidden": "true", children: "\u2935" })
        ] }),
        item.editing ? /* @__PURE__ */ (0, import_jsx_runtime32.jsxs)("div", { className: "stk-item__edit", children: [
          /* @__PURE__ */ (0, import_jsx_runtime32.jsx)(
            "textarea",
            {
              ref: taRef,
              value: draft,
              onChange: (e) => {
                setDraft(e.target.value);
                const el = e.target;
                el.style.height = "auto";
                el.style.height = el.scrollHeight + "px";
              },
              onKeyDown: handleEditKey,
              placeholder: "Rewrite this message\u2026",
              "data-testid": `pending-item-edit:${item.id}`
            }
          ),
          /* @__PURE__ */ (0, import_jsx_runtime32.jsxs)("div", { className: "stk-item__edit-row", children: [
            /* @__PURE__ */ (0, import_jsx_runtime32.jsxs)("span", { children: [
              /* @__PURE__ */ (0, import_jsx_runtime32.jsx)("span", { className: "stk-kbd", children: "Esc" }),
              " cancel"
            ] }),
            /* @__PURE__ */ (0, import_jsx_runtime32.jsxs)("span", { children: [
              /* @__PURE__ */ (0, import_jsx_runtime32.jsx)("span", { className: "stk-kbd", children: "\u21B5" }),
              " save \xB7 ",
              /* @__PURE__ */ (0, import_jsx_runtime32.jsx)("span", { className: "stk-kbd", children: "\u21E7\u21B5" }),
              " newline"
            ] }),
            /* @__PURE__ */ (0, import_jsx_runtime32.jsx)("span", { className: "stk-item__edit-spacer" }),
            /* @__PURE__ */ (0, import_jsx_runtime32.jsx)(
              "button",
              {
                type: "button",
                className: "stk-btn",
                onClick: () => onCancelEdit(item.id),
                children: "Cancel"
              }
            ),
            /* @__PURE__ */ (0, import_jsx_runtime32.jsx)(
              "button",
              {
                type: "button",
                className: "stk-btn stk-btn--save",
                onClick: () => onCommitEdit(item.id, draft),
                children: "Save"
              }
            )
          ] })
        ] }) : /* @__PURE__ */ (0, import_jsx_runtime32.jsxs)("div", { className: "stk-item__body", children: [
          /* @__PURE__ */ (0, import_jsx_runtime32.jsx)(
            "div",
            {
              className: `stk-item__text ${item.expanded ? "stk-item__text--expanded" : ""}`,
              onClick: longText ? () => onToggleExpand(item.id) : void 0,
              style: longText ? { cursor: "pointer" } : void 0,
              title: longText && !item.expanded ? item.text : void 0,
              children: item.text
            }
          ),
          /* @__PURE__ */ (0, import_jsx_runtime32.jsxs)("div", { className: "stk-item__meta", children: [
            isHead && /* @__PURE__ */ (0, import_jsx_runtime32.jsx)("span", { className: "stk-item__head-tag", children: "Next" }),
            /* @__PURE__ */ (0, import_jsx_runtime32.jsx)("span", { children: timeAgo(item.addedAt) }),
            item.status === "promoting" && /* @__PURE__ */ (0, import_jsx_runtime32.jsx)("span", { className: "stk-item__sending", children: "SENDING\u2026" })
          ] })
        ] }),
        !item.editing && /* @__PURE__ */ (0, import_jsx_runtime32.jsxs)("div", { className: "stk-item__actions", children: [
          /* @__PURE__ */ (0, import_jsx_runtime32.jsxs)(
            "button",
            {
              type: "button",
              className: "stk-btn stk-btn--steer",
              onClick: () => onSteer(item.id),
              disabled: item.status === "promoting",
              "aria-label": "Steer \u2014 send now and interrupt at next cooperative pause",
              title: "Send now and interrupt at the next cooperative pause",
              "data-testid": `pending-steer:${item.id}`,
              children: [
                /* @__PURE__ */ (0, import_jsx_runtime32.jsx)("span", { className: "stk-btn__glyph", children: "\u21AA" }),
                " Steer"
              ]
            }
          ),
          /* @__PURE__ */ (0, import_jsx_runtime32.jsx)(
            "button",
            {
              type: "button",
              className: "stk-btn stk-btn--icon",
              onClick: () => onEdit(item.id),
              "aria-label": "Edit message",
              title: "Edit message",
              "data-testid": `pending-edit:${item.id}`,
              children: /* @__PURE__ */ (0, import_jsx_runtime32.jsx)("span", { className: "stk-btn__glyph", children: "\u270E" })
            }
          ),
          /* @__PURE__ */ (0, import_jsx_runtime32.jsx)(
            "button",
            {
              type: "button",
              className: "stk-btn stk-btn--icon stk-btn--trash",
              onClick: () => onTrash(item.id),
              "aria-label": "Remove from queue",
              title: "Remove from queue",
              "data-testid": `pending-trash:${item.id}`,
              children: /* @__PURE__ */ (0, import_jsx_runtime32.jsx)("span", { className: "stk-btn__glyph", children: "\xD7" })
            }
          )
        ] })
      ]
    }
  );
}
function PendingStack({
  items,
  agentBusy,
  reducedMotion,
  onSteer,
  onTrash,
  onEdit,
  onCommitEdit,
  onCancelEdit,
  onReorder,
  onClearAll,
  onToggleExpand
}) {
  const [, setTick] = import_react24.default.useState(0);
  import_react24.default.useEffect(() => {
    const t = window.setInterval(() => setTick((n) => n + 1), 1e4);
    return () => window.clearInterval(t);
  }, []);
  const [dragId, setDragId] = import_react24.default.useState(null);
  const [dropTarget, setDropTarget] = import_react24.default.useState({ id: null, where: null });
  const [collapsed, setCollapsed] = import_react24.default.useState(false);
  const lastCount = import_react24.default.useRef(0);
  import_react24.default.useEffect(() => {
    if (items.length > lastCount.current) setCollapsed(false);
    lastCount.current = items.length;
  }, [items.length]);
  if (items.length === 0) return null;
  const onDragStart = (e, id) => {
    setDragId(id);
    try {
      e.dataTransfer.setData("text/plain", String(id));
    } catch {
    }
    e.dataTransfer.effectAllowed = "move";
  };
  const onDragOver = (e, id) => {
    if (dragId == null || dragId === id) return;
    e.preventDefault();
    e.dataTransfer.dropEffect = "move";
    const rect = e.currentTarget.getBoundingClientRect();
    const where = e.clientY - rect.top < rect.height / 2 ? "above" : "below";
    setDropTarget((dt) => dt.id === id && dt.where === where ? dt : { id, where });
  };
  const onDragLeave = (e, id) => {
    if (dropTarget.id === id) {
      const related = e.relatedTarget;
      if (!related || !e.currentTarget.contains(related)) {
        setDropTarget({ id: null, where: null });
      }
    }
  };
  const onDrop = (e, id) => {
    e.preventDefault();
    if (dragId == null || dragId === id) return;
    onReorder(dragId, id, dropTarget.where || "above");
    setDragId(null);
    setDropTarget({ id: null, where: null });
  };
  const onDragEnd = () => {
    setDragId(null);
    setDropTarget({ id: null, where: null });
  };
  return /* @__PURE__ */ (0, import_jsx_runtime32.jsxs)(
    "section",
    {
      className: `stack ${collapsed ? "is-collapsed" : ""} ${reducedMotion ? "reduced-motion" : ""}`,
      "aria-label": "Pending message queue",
      "data-testid": "pending-stack",
      children: [
        /* @__PURE__ */ (0, import_jsx_runtime32.jsx)(
          StackHead,
          {
            count: items.length,
            agentBusy,
            collapsed,
            onToggleCollapsed: () => setCollapsed((c) => !c),
            onClear: onClearAll
          }
        ),
        /* @__PURE__ */ (0, import_jsx_runtime32.jsx)("ol", { className: "stack__list", role: "list", children: items.map((item, i) => /* @__PURE__ */ (0, import_jsx_runtime32.jsx)(
          StackItem,
          {
            item,
            isHead: i === 0,
            dragging: dragId === item.id,
            dropHint: dropTarget.id === item.id ? dropTarget.where : null,
            onSteer,
            onTrash,
            onEdit,
            onCommitEdit,
            onCancelEdit,
            onToggleExpand,
            onDragStart,
            onDragOver,
            onDragLeave,
            onDrop,
            onDragEnd
          },
          item.id
        )) })
      ]
    }
  );
}

// src/ConsoleApp.tsx
var import_jsx_runtime33 = require("react/jsx-runtime");
function richBlockHasVisibleContent(block) {
  if (!block || typeof block !== "object") return false;
  const record = block;
  const scalarText = [
    typeof record.text === "string" ? record.text : "",
    typeof record.label === "string" ? record.label : "",
    typeof record.result === "string" ? record.result : "",
    typeof record.body === "string" ? record.body : "",
    typeof record.title === "string" ? record.title : "",
    typeof record.name === "string" ? record.name : ""
  ].join(" ").trim();
  if (scalarText.length > 0) return true;
  if (record.type === "image" && (typeof record.src === "string" || typeof record.blobId === "string")) return true;
  if (Array.isArray(record.headers) && record.headers.some((v) => String(v || "").trim().length > 0)) return true;
  if (Array.isArray(record.rows) && record.rows.some((row) => Array.isArray(row) && row.some((v) => String(v || "").trim().length > 0))) return true;
  return false;
}
function sanitizeConversationEntries(entries) {
  const sanitized = [];
  for (const entry of entries) {
    if (entry.kind !== "message") {
      sanitized.push(entry);
      continue;
    }
    if (entry.variant === "rich" && Array.isArray(entry.blocks)) {
      const blocks = entry.blocks.filter(richBlockHasVisibleContent);
      if (!blocks.length) continue;
      sanitized.push({ ...entry, blocks });
      continue;
    }
    if (entry.text && entry.text.trim().length > 0) sanitized.push(entry);
  }
  return sanitized;
}
function normalizeConsoleInspectResult(value) {
  const direct = normalizeIdentityInspectViewState(value);
  if (direct) return direct;
  const record = value && typeof value === "object" ? value : {};
  const identityRecord = record.identity && typeof record.identity === "object" ? record.identity : null;
  if (!identityRecord) return null;
  return normalizeIdentityInspectViewState({
    identity: identityRecord.identity,
    display_name: identityRecord.display_name,
    role: identityRecord.labels && typeof identityRecord.labels === "object" ? identityRecord.labels.role : void 0,
    state: identityRecord.health,
    addressability: identityRecord.addressable === true ? "addressable" : "internal_only",
    session_id: identityRecord.session_id,
    labels: identityRecord.labels,
    continuity: {
      session_id: identityRecord.session_id,
      agent_runtime_id: identityRecord.runtime_member_id
    },
    topology_peers: Array.isArray(record.peers) ? record.peers : [],
    lease: null
  });
}
var DEFAULT_APPROVER_ID = "console-ops-lead";
function createIdempotencyKey() {
  try {
    if (typeof crypto !== "undefined" && typeof crypto.randomUUID === "function") {
      return crypto.randomUUID();
    }
  } catch {
  }
  return `console-${Date.now().toString(36)}-${Math.random().toString(36).slice(2)}`;
}
function cursorSeq(cursor) {
  if (!cursor) return null;
  const match = /^console:(\d+)$/.exec(cursor);
  if (!match) return null;
  const parsed = Number(match[1]);
  return Number.isFinite(parsed) ? parsed : null;
}
var REFRESH_TRIGGER_EVENTS = /* @__PURE__ */ new Set([
  "interaction_complete",
  "interaction_failed",
  "state_changed",
  "member_ready",
  "member_retired",
  "gating_decision",
  "route_changed"
]);
var PANEL_ROUTABLE_EVENTS = /* @__PURE__ */ new Set([
  "user_input",
  "interaction_started",
  "interaction_complete",
  "interaction_failed",
  "assistant_image",
  "text_delta",
  "text_complete",
  "tool_call_requested",
  "tool_call",
  "tool_result_received",
  "tool_execution_started",
  "tool_execution_completed",
  "run_started",
  "run_completed",
  "run_failed"
]);
var HISTORY_REFRESH_EVENTS = /* @__PURE__ */ new Set([
  "interaction_complete",
  "interaction_failed",
  "run_completed",
  "run_failed",
  "message_delivery_failed"
]);
var ACTIVITY_SKIP_EVENTS = /* @__PURE__ */ new Set([
  "subscribed",
  "run_started",
  "run_completed",
  "turn_started",
  "turn_completed",
  "text_complete",
  "reasoning_delta",
  "reasoning_complete",
  "interaction_started",
  "run_failed",
  "keep-alive",
  "tool_config_changed",
  "tool_scope_changed",
  "user_input",
  "text_delta",
  "tool_call_requested",
  "tool_call",
  "tool_execution_started",
  "tool_result_received",
  "tool_execution_completed"
]);
function ConsoleApp({ baseUrl }) {
  const [experience, setExperience] = import_react25.default.useState(null);
  const [agents, setAgents] = import_react25.default.useState([]);
  const [draftByKey, setDraftByKey] = import_react25.default.useState({});
  const [stagedAttachmentsByIdentity, setStagedAttachmentsByIdentity] = import_react25.default.useState({});
  const [sendingPanels, setSendingPanels] = import_react25.default.useState(/* @__PURE__ */ new Set());
  const [pinnedAgentIds, setPinnedAgentIds] = import_react25.default.useState(/* @__PURE__ */ new Set());
  const [inspectByIdentity, setInspectByIdentity] = import_react25.default.useState({});
  const [routingData, setRoutingData] = import_react25.default.useState({ routes: [], deliveries: [] });
  const [gatingData, setGatingData] = import_react25.default.useState({ pending: [], audit: [] });
  const [activeActivityPresetId, setActiveActivityPresetId] = import_react25.default.useState("all");
  const [loading, setLoading] = import_react25.default.useState(true);
  const [error, setError] = import_react25.default.useState("");
  const [theme, setTheme] = import_react25.default.useState(() => {
    try {
      return localStorage.getItem("mobkit-console-theme") || "light";
    } catch {
      return "light";
    }
  });
  const [variant, setVariant] = useConsoleVariant();
  const [sidebarCollapsed, setSidebarCollapsed] = import_react25.default.useState(() => {
    try {
      return localStorage.getItem("mobkit-console-sidebar-collapsed") === "1";
    } catch {
      return false;
    }
  });
  const toggleSidebarCollapsed = import_react25.default.useCallback(() => {
    setSidebarCollapsed((c) => {
      const next = !c;
      try {
        localStorage.setItem("mobkit-console-sidebar-collapsed", next ? "1" : "0");
      } catch {
      }
      return next;
    });
  }, []);
  const [railCollapsed, setRailCollapsed] = import_react25.default.useState(() => {
    try {
      return localStorage.getItem("mobkit-console-rail-collapsed") === "1";
    } catch {
      return false;
    }
  });
  const toggleRailCollapsed = import_react25.default.useCallback(() => {
    setRailCollapsed((c) => {
      const next = !c;
      try {
        localStorage.setItem("mobkit-console-rail-collapsed", next ? "1" : "0");
      } catch {
      }
      return next;
    });
  }, []);
  const [, setRenderTick] = import_react25.default.useState(0);
  const forceRender = import_react25.default.useCallback(() => setRenderTick((n) => n + 1), []);
  const stagedAttachmentsRef = import_react25.default.useRef(stagedAttachmentsByIdentity);
  import_react25.default.useEffect(() => {
    stagedAttachmentsRef.current = stagedAttachmentsByIdentity;
  }, [stagedAttachmentsByIdentity]);
  import_react25.default.useEffect(() => () => {
    for (const items of Object.values(stagedAttachmentsRef.current)) {
      items.forEach((item) => URL.revokeObjectURL(item.previewUrl));
    }
  }, []);
  function setStagedAttachmentsForIdentity(identity, action) {
    setStagedAttachmentsByIdentity((current) => {
      const previous = current[identity] ?? [];
      const next = typeof action === "function" ? action(previous) : action;
      const updated = { ...current };
      if (next.length > 0) updated[identity] = next;
      else delete updated[identity];
      return updated;
    });
  }
  const identityLogRef = import_react25.default.useRef({});
  const optimisticUserByPanelKeyRef = import_react25.default.useRef({});
  function getOrCreateLog(identity) {
    let log = identityLogRef.current[identity];
    if (!log) {
      log = {
        events: [],
        byKey: /* @__PURE__ */ new Map(),
        hasServerLog: null
      };
      identityLogRef.current[identity] = log;
    }
    return log;
  }
  function clearOptimisticUserByInteraction(interactionId) {
    for (const [panelKey, optimistic] of Object.entries(optimisticUserByPanelKeyRef.current)) {
      if (optimistic.interactionId !== interactionId) continue;
      optimistic.objectUrls?.forEach((url) => URL.revokeObjectURL(url));
      delete optimisticUserByPanelKeyRef.current[panelKey];
    }
  }
  function frameKey(frame) {
    if (frame.id) return frame.id;
    if (frame.cursor) return frame.cursor;
    return `${frame.event}:${frame.identity || ""}:${frame.interactionId || ""}:${frame.timestampMs || 0}`;
  }
  function appendFrame(identity, frame) {
    const log = getOrCreateLog(identity);
    if (frame.event === "frame_updated" && frame.data && typeof frame.data === "object") {
      const updated = frame.data.frame;
      if (updated && updated.id) {
        const existingIndex = log.byKey.get(updated.id);
        if (existingIndex !== void 0 && log.events[existingIndex]) {
          const existingVersion = log.events[existingIndex].frameVersion ?? 0;
          const updatedVersion = updated.frameVersion ?? existingVersion;
          if (updatedVersion < existingVersion) return false;
          log.events[existingIndex] = { ...log.events[existingIndex], ...updated };
          return true;
        }
      }
      return false;
    }
    const key = frameKey(frame);
    if (log.byKey.has(key)) return false;
    log.byKey.set(key, log.events.length);
    log.events.push(frame);
    if ((frame.event === "interaction_started" || frame.event === "user_input") && frame.interactionId) {
      clearOptimisticUserByInteraction(frame.interactionId);
    }
    return true;
  }
  function reconcileServerLog(identity, frames, available) {
    const log = getOrCreateLog(identity);
    log.hasServerLog = available;
    for (const frame of frames) appendFrame(identity, frame);
  }
  async function queryIdentityTimeline(identity) {
    const frames = [];
    let available = true;
    let after;
    for (let pageIndex = 0; pageIndex < 100; pageIndex += 1) {
      const page = await queryTimeline(baseUrl, { identity, after }, 1e3);
      available = page.available;
      frames.push(...page.frames);
      const next = page.nextCursor?.trim();
      if (!next || next === after || page.frames.length === 0) break;
      after = next;
    }
    return { frames, available };
  }
  function getSortedFrames(identity) {
    const log = identityLogRef.current[identity];
    if (!log) return [];
    return log.events.map((frame, index) => ({ frame, index })).sort((a, b) => {
      const ca = cursorSeq(a.frame.cursor);
      const cb = cursorSeq(b.frame.cursor);
      if (ca !== null && cb !== null && ca !== cb) return ca - cb;
      const ta = typeof a.frame.timestampMs === "number" ? a.frame.timestampMs : Number.MAX_SAFE_INTEGER;
      const tb = typeof b.frame.timestampMs === "number" ? b.frame.timestampMs : Number.MAX_SAFE_INTEGER;
      if (ta !== tb) return ta - tb;
      return a.index - b.index;
    }).map((entry) => entry.frame);
  }
  function frameOrigin(frame) {
    if (!frame.data || typeof frame.data !== "object") return null;
    const origin = frame.data.origin;
    return typeof origin === "string" ? origin : null;
  }
  function frameVisibleInPanel(frame, panelId) {
    if (frame.event !== "user_input" && frame.event !== "interaction_started") return true;
    const origin = frameOrigin(frame);
    if (!origin?.startsWith("console:")) return true;
    return origin === `console:${panelId}`;
  }
  function framesVisibleInPanel(frames, panelId) {
    const hiddenInteractionIds = /* @__PURE__ */ new Set();
    const visibleFrames = [];
    for (const frame of frames) {
      if (frameVisibleInPanel(frame, panelId)) {
        visibleFrames.push(frame);
        continue;
      }
      const interactionId = frame.interactionId?.trim();
      if (interactionId) hiddenInteractionIds.add(interactionId);
    }
    if (hiddenInteractionIds.size === 0) return visibleFrames;
    return visibleFrames.filter((frame) => {
      if (frame.event !== "run_started") return true;
      const interactionId = frame.interactionId?.trim();
      return !interactionId || !hiddenInteractionIds.has(interactionId);
    });
  }
  const activityRef = import_react25.default.useRef([]);
  const liveFramesRef = import_react25.default.useRef([]);
  const pendingStackRef = import_react25.default.useRef({});
  const PENDING_STACK_KEY_PREFIX = "mobkit-pending-stack:";
  const stackKeyFor = (identity) => `${PENDING_STACK_KEY_PREFIX}${identity}`;
  function loadPendingStack(identity) {
    try {
      const raw = localStorage.getItem(stackKeyFor(identity));
      if (!raw) return [];
      const parsed = JSON.parse(raw);
      if (!Array.isArray(parsed)) return [];
      return parsed.filter((it) => {
        if (!it || typeof it !== "object") return false;
        const r2 = it;
        return typeof r2.id === "string" && typeof r2.text === "string" && typeof r2.addedAt === "number";
      }).map((it) => ({ id: it.id, text: it.text, addedAt: it.addedAt }));
    } catch {
      return [];
    }
  }
  function persistPendingStack(identity, items) {
    try {
      const clean = items.filter((it) => it.status !== "trashing" && it.status !== "draining" && it.status !== "promoting").map((it) => ({ id: it.id, text: it.text, addedAt: it.addedAt }));
      if (clean.length === 0) {
        localStorage.removeItem(stackKeyFor(identity));
      } else {
        localStorage.setItem(stackKeyFor(identity), JSON.stringify(clean));
      }
    } catch {
    }
  }
  function getPendingStack(identity) {
    if (!pendingStackRef.current[identity]) {
      pendingStackRef.current[identity] = loadPendingStack(identity);
    }
    return pendingStackRef.current[identity];
  }
  function setPendingStack(identity, update) {
    const prev = getPendingStack(identity);
    const next = update(prev);
    pendingStackRef.current[identity] = next;
    persistPendingStack(identity, next);
    forceRender();
  }
  import_react25.default.useEffect(() => {
    const onStorage = (e) => {
      if (!e.key || !e.key.startsWith(PENDING_STACK_KEY_PREFIX)) return;
      const identity = e.key.slice(PENDING_STACK_KEY_PREFIX.length);
      pendingStackRef.current[identity] = loadPendingStack(identity);
      forceRender();
    };
    window.addEventListener("storage", onStorage);
    return () => window.removeEventListener("storage", onStorage);
  }, []);
  const identityBusyRef = import_react25.default.useRef({});
  const isIdentityBusy = (identity) => identityBusyRef.current[identity] === true;
  const phaseRef = import_react25.default.useRef({});
  const phaseValueByKey = import_react25.default.useRef({});
  const phaseSinceByKey = import_react25.default.useRef({});
  const phaseTimerByKey = import_react25.default.useRef({});
  const refreshTimersRef = import_react25.default.useRef({});
  const experienceTimerRef = import_react25.default.useRef(null);
  const agentsRef = import_react25.default.useRef([]);
  import_react25.default.useEffect(() => {
    agentsRef.current = agents;
  }, [agents]);
  const initialTargetOpened = import_react25.default.useRef(false);
  const dock = useConsoleDockController({
    createPanelState: ({ target }) => ({
      id: `panel-${crypto.randomUUID()}`,
      target: target || null,
      mode: "console"
    })
  });
  function clearPhaseTimer(panelKey) {
    const timer = phaseTimerByKey.current[panelKey];
    if (timer !== void 0) {
      window.clearTimeout(timer);
      delete phaseTimerByKey.current[panelKey];
    }
  }
  function commitPanelPhase(panelKey, phase) {
    clearPhaseTimer(panelKey);
    phaseValueByKey.current[panelKey] = phase;
    phaseSinceByKey.current[panelKey] = Date.now();
    phaseRef.current[panelKey] = phase;
  }
  function schedulePanelPhase(panelKey, phase, delayMs) {
    clearPhaseTimer(panelKey);
    phaseTimerByKey.current[panelKey] = window.setTimeout(() => {
      delete phaseTimerByKey.current[panelKey];
      phaseValueByKey.current[panelKey] = phase;
      phaseSinceByKey.current[panelKey] = Date.now();
      phaseRef.current[panelKey] = phase;
      forceRender();
    }, delayMs);
  }
  function updatePanelPhaseFromFrame(panelKey, frame) {
    const currentPhase = phaseValueByKey.current[panelKey] ?? null;
    const elapsedMs = Date.now() - (phaseSinceByKey.current[panelKey] ?? 0);
    switch (frame.event) {
      case "interaction_started":
        commitPanelPhase(panelKey, "waiting");
        break;
      case "tool_call_requested":
      case "tool_call":
      case "tool_execution_started":
      case "tool_result_received":
      case "tool_execution_completed":
        if (currentPhase === "waiting" && elapsedMs < 300) {
          schedulePanelPhase(panelKey, "tool-executing", 300 - elapsedMs);
          break;
        }
        commitPanelPhase(panelKey, "tool-executing");
        break;
      case "text_delta": {
        if (currentPhase === "tool-executing") {
          const r2 = Math.max(0, 300 - elapsedMs);
          if (r2 > 0) {
            schedulePanelPhase(panelKey, "generating", r2);
            break;
          }
        }
        if (currentPhase === "waiting" && elapsedMs < 300) {
          schedulePanelPhase(panelKey, "generating", 300 - elapsedMs);
          break;
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
  const dockRef = import_react25.default.useRef(dock);
  dockRef.current = dock;
  function updatePhaseForIdentity(identity, frame) {
    for (const panel of dockRef.current.viewState.panels) {
      const target = panel.target;
      if (!target || target.kind !== "agent-chat") continue;
      if ((target.identity || target.memberId) !== identity) continue;
      updatePanelPhaseFromFrame(buildPanelConversationKey(panel.id, target), frame);
    }
  }
  function clearPhaseForIdentity(identity) {
    for (const panel of dockRef.current.viewState.panels) {
      const target = panel.target;
      if (!target || target.kind !== "agent-chat") continue;
      if ((target.identity || target.memberId) !== identity) continue;
      commitPanelPhase(buildPanelConversationKey(panel.id, target), null);
    }
  }
  const loadExperience = import_react25.default.useCallback(async () => {
    const [experienceJson, modulesJson] = await Promise.all([
      fetchJson(baseUrl, "/console/experience"),
      fetchJson(baseUrl, "/console/modules")
    ]);
    const loadedModules = Array.isArray(modulesJson.modules) ? modulesJson.modules.map(String) : [];
    setExperience(experienceJson);
    setAgents(normalizeAgents(experienceJson, loadedModules));
    setActiveActivityPresetId((c) => c || experienceJson.activity_feed?.active_preset_id || "all");
  }, [baseUrl]);
  import_react25.default.useEffect(() => {
    let mounted = true;
    setLoading(true);
    setError("");
    void loadExperience().catch((e) => {
      if (mounted) setError(errorMessage(e));
    }).finally(() => {
      if (mounted) setLoading(false);
    });
    return () => {
      mounted = false;
    };
  }, [loadExperience]);
  import_react25.default.useEffect(() => {
    if (initialTargetOpened.current || dock.focusedTarget || agents.length === 0) return;
    const first = agents.find((a) => a.addressable || a.affordances?.can_send_message) || agents[0];
    if (!first) return;
    initialTargetOpened.current = true;
    dock.openTarget(buildDockTarget(first), "replace_focused");
  }, [agents, dock]);
  const hasMobControlSurface = experience?.runtime_id !== "console-aggregator";
  const visibleControls = import_react25.default.useMemo(
    () => hasMobControlSurface ? ["topology", "timeline", "gating", "roster", "routing", "gates", "logs", "health"] : ["topology", "timeline", "roster", "logs", "health"],
    [hasMobControlSurface]
  );
  const refreshPanelData = import_react25.default.useCallback(async () => {
    const openPanels = dock.viewState.panels.map((p) => p.target).filter(Boolean);
    const inspects = openPanels.filter((t) => t.kind === "identity-inspect");
    if (inspects.length) {
      const entries = await Promise.all(inspects.map(async (t) => {
        const r2 = await callConsoleRpc(baseUrl, "mobkit/console/inspect_identity", { identity: t.identity }).catch(() => callConsoleRpc(baseUrl, "mobkit/inspect_identity", { identity: t.identity }));
        return [t.identity, normalizeConsoleInspectResult(r2)];
      }));
      setInspectByIdentity((c) => ({ ...c, ...Object.fromEntries(entries) }));
    }
    if (hasMobControlSurface && openPanels.some((t) => t.kind === "routing")) {
      const [routes, history] = await Promise.all([
        callConsoleRpc(baseUrl, "mobkit/routing/routes/list", {}),
        callConsoleRpc(baseUrl, "mobkit/delivery/history", {})
      ]);
      setRoutingData(buildRoutingSectionView({ routesResponse: routes, historyResponse: history }));
    }
    if (hasMobControlSurface && openPanels.some((t) => t.kind === "gating")) {
      const [p, a] = await Promise.all([
        callConsoleRpc(baseUrl, "mobkit/gating/pending", {}),
        callConsoleRpc(baseUrl, "mobkit/gating/audit", { limit: 50 })
      ]);
      setGatingData({ pending: Array.isArray(p.pending) ? p.pending : [], audit: Array.isArray(a.entries) ? a.entries : [] });
    }
  }, [baseUrl, dock.viewState.panels, hasMobControlSurface]);
  import_react25.default.useEffect(() => {
    void refreshPanelData().catch(() => {
    });
  }, [dock.viewState.panels, refreshPanelData]);
  const scheduleExperienceRefresh = import_react25.default.useCallback(() => {
    if (experienceTimerRef.current !== null) return;
    experienceTimerRef.current = window.setTimeout(async () => {
      experienceTimerRef.current = null;
      await loadExperience().catch(() => {
      });
      await refreshPanelData().catch(() => {
      });
    }, 500);
  }, [loadExperience, refreshPanelData]);
  const scheduleHistoryRefresh = import_react25.default.useCallback((identity) => {
    clearTimeout(refreshTimersRef.current[identity]);
    refreshTimersRef.current[identity] = window.setTimeout(async () => {
      const log = getOrCreateLog(identity);
      if (log.hasServerLog === false) {
        clearPhaseForIdentity(identity);
        forceRender();
        return;
      }
      try {
        const { frames, available } = await queryIdentityTimeline(identity);
        reconcileServerLog(identity, frames, available);
        clearPhaseForIdentity(identity);
        forceRender();
      } catch {
      }
    }, 200);
  }, [baseUrl, forceRender]);
  import_react25.default.useEffect(() => {
    for (const panel of dock.viewState.panels) {
      const target = panel.target;
      if (!target || target.kind !== "agent-chat") continue;
      const identity = target.identity || target.memberId;
      const log = getOrCreateLog(identity);
      if (log.hasServerLog !== null) continue;
      void (async () => {
        try {
          const { frames, available } = await queryIdentityTimeline(identity);
          reconcileServerLog(identity, frames, available);
          forceRender();
        } catch {
        }
      })();
    }
  }, [baseUrl, dock.viewState.panels, forceRender]);
  const scheduleHistoryRefreshRef = import_react25.default.useRef(scheduleHistoryRefresh);
  scheduleHistoryRefreshRef.current = scheduleHistoryRefresh;
  const scheduleExperienceRefreshRef = import_react25.default.useRef(scheduleExperienceRefresh);
  scheduleExperienceRefreshRef.current = scheduleExperienceRefresh;
  import_react25.default.useEffect(() => {
    void queryTimeline(baseUrl, {}, 200).then(({ frames }) => {
      const seen = /* @__PURE__ */ new Set();
      const filtered = [];
      for (const frame of frames) {
        if (ACTIVITY_SKIP_EVENTS.has(frame.event)) continue;
        const key = frame.id || `${frame.event}:${frame.timestampMs || 0}`;
        if (seen.has(key)) continue;
        seen.add(key);
        filtered.push(frame);
      }
      activityRef.current = filtered.slice(-200).reverse();
      forceRender();
    }).catch(() => {
    });
    const unsubscribe = subscribeTimelineEvents(baseUrl, {}, (frame) => {
      if (!ACTIVITY_SKIP_EVENTS.has(frame.event)) {
        activityRef.current = [frame, ...activityRef.current].slice(0, 200);
      }
      if (PANEL_ROUTABLE_EVENTS.has(frame.event)) {
        liveFramesRef.current = [frame, ...liveFramesRef.current].slice(0, 300);
      }
      const identity = frame.identity?.trim();
      if (PANEL_ROUTABLE_EVENTS.has(frame.event) && identity && identity !== "_system") {
        appendFrame(identity, frame);
        updatePhaseForIdentity(identity, frame);
        const wasBusy = identityBusyRef.current[identity] === true;
        if (frame.event === "user_input" || frame.event === "interaction_started" || frame.event === "run_started") {
          identityBusyRef.current[identity] = true;
        } else if (frame.event === "interaction_complete" || frame.event === "interaction_failed" || frame.event === "run_completed" || frame.event === "run_failed" || frame.event === "message_delivery_failed") {
          identityBusyRef.current[identity] = false;
          if (wasBusy) maybeDrainHead(identity);
        }
      }
      forceRender();
      if (HISTORY_REFRESH_EVENTS.has(frame.event) && identity && identity !== "_system") {
        scheduleHistoryRefreshRef.current(identity);
      }
      if (REFRESH_TRIGGER_EVENTS.has(frame.event)) {
        scheduleExperienceRefreshRef.current();
      }
    });
    return () => {
      unsubscribe();
    };
  }, [baseUrl]);
  import_react25.default.useEffect(() => {
    return () => {
      for (const timer of Object.values(phaseTimerByKey.current)) window.clearTimeout(timer);
      for (const timer of Object.values(refreshTimersRef.current)) window.clearTimeout(timer);
      if (experienceTimerRef.current !== null) window.clearTimeout(experienceTimerRef.current);
    };
  }, []);
  function onSelectAgent(_block, _section, item) {
    const agent = agents.find((c) => c.member_id === item.id);
    if (agent) dock.openTarget(buildDockTarget(agent), "replace_focused");
  }
  async function submitMessageNow(panelId, target, text, handlingMode, attachments = []) {
    if (target.kind !== "agent-chat") return false;
    const panelKey = buildPanelConversationKey(panelId, target);
    const identity = target.identity || target.memberId;
    const optimisticObjectUrls = attachments.map((file) => URL.createObjectURL(file));
    const userEntry = createUserEntry(
      text,
      attachments.map((file, index) => ({
        src: optimisticObjectUrls[index] || "",
        mediaType: file.type || "application/octet-stream",
        alt: file.name
      }))
    );
    setSendingPanels((c) => new Set(c).add(panelKey));
    const log = getOrCreateLog(identity);
    optimisticUserByPanelKeyRef.current[panelKey] = {
      interactionId: "",
      entry: userEntry,
      sentAtMs: Date.now(),
      objectUrls: optimisticObjectUrls
    };
    commitPanelPhase(panelKey, "waiting");
    identityBusyRef.current[identity] = true;
    forceRender();
    try {
      const id = target.identity?.trim();
      if (attachments.length > 0 && id) {
        const result = await sendConsoleMultipart(
          baseUrl,
          id,
          text,
          attachments,
          `console:${panelId}`,
          createIdempotencyKey(),
          handlingMode
        );
        const optimisticUser = optimisticUserByPanelKeyRef.current[panelKey];
        if (optimisticUser) {
          optimisticUser.interactionId = result.interaction_id;
          const matched = log.events.some(
            (f) => (f.event === "interaction_started" || f.event === "user_input") && f.interactionId === result.interaction_id
          );
          if (matched) {
            optimisticUser.objectUrls?.forEach((url) => URL.revokeObjectURL(url));
            delete optimisticUserByPanelKeyRef.current[panelKey];
          }
        }
      } else if (attachments.length > 0) {
        const result = await sendMessageMultipart(baseUrl, target.memberId, text, attachments, handlingMode);
        const optimisticUser = optimisticUserByPanelKeyRef.current[panelKey];
        if (optimisticUser) {
          optimisticUser.interactionId = result.interaction_id || "";
          const matched = result.interaction_id ? log.events.some(
            (f) => f.event === "interaction_started" && f.interactionId === result.interaction_id
          ) : false;
          if (matched) {
            optimisticUser.objectUrls?.forEach((url) => URL.revokeObjectURL(url));
            delete optimisticUserByPanelKeyRef.current[panelKey];
          }
        }
      } else if (id) {
        const result = await sendConsole(
          baseUrl,
          id,
          text,
          `console:${panelId}`,
          createIdempotencyKey(),
          handlingMode
        );
        const optimisticUser = optimisticUserByPanelKeyRef.current[panelKey];
        if (optimisticUser) {
          optimisticUser.interactionId = result.interaction_id;
          const matched = log.events.some(
            (f) => (f.event === "interaction_started" || f.event === "user_input") && f.interactionId === result.interaction_id
          );
          if (matched) {
            optimisticUser.objectUrls?.forEach((url) => URL.revokeObjectURL(url));
            delete optimisticUserByPanelKeyRef.current[panelKey];
          }
        }
      } else {
        await sendMessage(baseUrl, target.memberId, text, handlingMode);
      }
      return true;
    } catch (submitError) {
      optimisticUserByPanelKeyRef.current[panelKey]?.objectUrls?.forEach((url) => URL.revokeObjectURL(url));
      delete optimisticUserByPanelKeyRef.current[panelKey];
      commitPanelPhase(panelKey, null);
      identityBusyRef.current[identity] = false;
      setError(errorMessage(submitError));
      forceRender();
      return false;
    } finally {
      setSendingPanels((c) => {
        const n = new Set(c);
        n.delete(panelKey);
        return n;
      });
    }
  }
  async function onSendMessage(panelId, target, attachments = []) {
    if (!target || target.kind !== "agent-chat") return false;
    const panelKey = buildPanelConversationKey(panelId, target);
    const identity = target.identity || target.memberId;
    const text = (draftByKey[panelKey] || "").trim();
    if (!text && attachments.length === 0) return false;
    const stack = getPendingStack(identity);
    const shouldQueue = isIdentityBusy(identity) || stack.length > 0;
    if (!shouldQueue || attachments.length > 0) {
      const sent = await submitMessageNow(panelId, target, text, "queue", attachments);
      if (sent) setDraftByKey((c) => ({ ...c, [panelKey]: "" }));
      return sent;
    }
    const newId = `pmsg-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 7)}`;
    setPendingStack(identity, (prev) => [
      ...prev,
      { id: newId, text, addedAt: Date.now(), status: "entering" }
    ]);
    setDraftByKey((c) => ({ ...c, [panelKey]: "" }));
    window.setTimeout(() => {
      setPendingStack(
        identity,
        (prev) => prev.map((it) => it.id === newId && it.status === "entering" ? { ...it, status: null } : it)
      );
    }, 240);
    return true;
  }
  const reducedMotion = typeof window !== "undefined" ? window.matchMedia?.("(prefers-reduced-motion: reduce)").matches ?? false : false;
  const animMs = (ms) => reducedMotion ? 0 : ms;
  function findChatTargetFor(identity) {
    for (const panel of dockRef.current.viewState.panels) {
      const t = panel.target;
      if (!t || t.kind !== "agent-chat") continue;
      if ((t.identity || t.memberId) === identity) {
        return { panelId: panel.id, target: t };
      }
    }
    return null;
  }
  function onStackSteer(identity, id) {
    setPendingStack(
      identity,
      (prev) => prev.map((it) => it.id === id ? { ...it, status: "promoting", editing: false } : it)
    );
    window.setTimeout(() => {
      const stack = getPendingStack(identity);
      const item = stack.find((it) => it.id === id);
      if (!item) return;
      setPendingStack(identity, (prev) => prev.filter((it) => it.id !== id));
      const target = findChatTargetFor(identity);
      if (target) {
        void submitMessageNow(target.panelId, target.target, item.text, "steer");
      }
    }, animMs(360));
  }
  function onStackTrash(identity, id) {
    setPendingStack(
      identity,
      (prev) => prev.map((it) => it.id === id ? { ...it, status: "trashing", editing: false } : it)
    );
    window.setTimeout(() => {
      setPendingStack(identity, (prev) => prev.filter((it) => it.id !== id));
    }, animMs(320));
  }
  function onStackEdit(identity, id) {
    setPendingStack(
      identity,
      (prev) => prev.map((it) => it.id === id ? { ...it, editing: true } : { ...it, editing: false })
    );
  }
  function onStackCommitEdit(identity, id, text) {
    const trimmed = text.trim();
    if (!trimmed) return;
    setPendingStack(
      identity,
      (prev) => prev.map((it) => it.id === id ? { ...it, text: trimmed, editing: false, addedAt: Date.now() } : it)
    );
  }
  function onStackCancelEdit(identity, id) {
    setPendingStack(
      identity,
      (prev) => prev.map((it) => it.id === id ? { ...it, editing: false } : it)
    );
  }
  function onStackReorder(identity, dragId, dropId, where) {
    setPendingStack(identity, (prev) => {
      const fromIdx = prev.findIndex((it) => it.id === dragId);
      const toIdx = prev.findIndex((it) => it.id === dropId);
      if (fromIdx === -1 || toIdx === -1) return prev;
      const next = prev.slice();
      const [moved] = next.splice(fromIdx, 1);
      let insertAt = next.findIndex((it) => it.id === dropId);
      if (where === "below") insertAt += 1;
      next.splice(insertAt, 0, moved);
      return next;
    });
  }
  function onStackClearAll(identity) {
    setPendingStack(
      identity,
      (prev) => prev.map((it) => ({ ...it, status: "trashing", editing: false }))
    );
    window.setTimeout(() => {
      setPendingStack(identity, () => []);
    }, animMs(320));
  }
  function onStackToggleExpand(identity, id) {
    setPendingStack(
      identity,
      (prev) => prev.map((it) => it.id === id ? { ...it, expanded: !it.expanded } : it)
    );
  }
  function maybeDrainHead(identity) {
    const stack = getPendingStack(identity);
    if (stack.length === 0) return;
    if (stack.some((it) => it.status === "draining" || it.status === "promoting")) return;
    const head = stack.find((it) => !it.status || it.status === "entering");
    if (!head) return;
    setPendingStack(
      identity,
      (prev) => prev.map((it) => it.id === head.id ? { ...it, status: "draining" } : it)
    );
    window.setTimeout(() => {
      setPendingStack(identity, (prev) => prev.filter((it) => it.id !== head.id));
      const target = findChatTargetFor(identity);
      if (target) {
        void submitMessageNow(target.panelId, target.target, head.text, "queue");
      }
    }, animMs(420));
  }
  async function onLifecycleAction(identity, method) {
    await callConsoleRpc(baseUrl, method, { identity });
    await loadExperience();
  }
  async function onGatingDecision(pendingId, decision) {
    await callConsoleRpc(baseUrl, "mobkit/gating/decide", {
      pending_id: pendingId,
      approver_id: DEFAULT_APPROVER_ID,
      decision,
      reason: `console_${decision}`
    });
    const [p, a] = await Promise.all([
      callConsoleRpc(baseUrl, "mobkit/gating/pending", {}),
      callConsoleRpc(baseUrl, "mobkit/gating/audit", { limit: 50 })
    ]);
    setGatingData({ pending: Array.isArray(p.pending) ? p.pending : [], audit: Array.isArray(a.entries) ? a.entries : [] });
  }
  const SIDEBAR_MIN = 180, SIDEBAR_MAX = 420;
  function handleSidebarResize(event) {
    event.preventDefault();
    const startX = event.clientX;
    const root = event.currentTarget.closest("[data-console-workbench]");
    if (!root) return;
    const startWidth = parseInt(getComputedStyle(root).getPropertyValue("--cc-workbench-sidebar-width") || "260", 10) || 260;
    const handle = event.currentTarget;
    if ("setPointerCapture" in handle) handle.setPointerCapture(event.pointerId);
    document.documentElement.setAttribute("data-cc-resizing", "true");
    function onPointerMove(e) {
      root.style.setProperty("--cc-workbench-sidebar-width", `${Math.min(SIDEBAR_MAX, Math.max(SIDEBAR_MIN, startWidth + (e.clientX - startX)))}px`);
    }
    function cleanup() {
      document.documentElement.removeAttribute("data-cc-resizing");
      window.removeEventListener("pointermove", onPointerMove);
      window.removeEventListener("pointerup", cleanup);
      window.removeEventListener("pointercancel", cleanup);
      if ("hasPointerCapture" in handle && handle.hasPointerCapture(event.pointerId)) handle.releasePointerCapture(event.pointerId);
    }
    window.addEventListener("pointermove", onPointerMove);
    window.addEventListener("pointerup", cleanup);
    window.addEventListener("pointercancel", cleanup);
  }
  const ACTIVITY_MIN = 200, ACTIVITY_MAX = 480;
  function handleActivityResize(event) {
    event.preventDefault();
    const startX = event.clientX;
    const root = event.currentTarget.closest("[data-console-workbench]");
    if (!root) return;
    const startWidth = parseInt(getComputedStyle(root).getPropertyValue("--cc-workbench-activity-width") || "280", 10) || 280;
    const handle = event.currentTarget;
    if ("setPointerCapture" in handle) handle.setPointerCapture(event.pointerId);
    document.documentElement.setAttribute("data-cc-resizing", "true");
    function onPointerMove(e) {
      root.style.setProperty("--cc-workbench-activity-width", `${Math.min(ACTIVITY_MAX, Math.max(ACTIVITY_MIN, startWidth - (e.clientX - startX)))}px`);
    }
    function cleanup() {
      document.documentElement.removeAttribute("data-cc-resizing");
      window.removeEventListener("pointermove", onPointerMove);
      window.removeEventListener("pointerup", cleanup);
      window.removeEventListener("pointercancel", cleanup);
      if ("hasPointerCapture" in handle && handle.hasPointerCapture(event.pointerId)) handle.releasePointerCapture(event.pointerId);
    }
    window.addEventListener("pointermove", onPointerMove);
    window.addEventListener("pointerup", cleanup);
    window.addEventListener("pointercancel", cleanup);
  }
  if (loading) return /* @__PURE__ */ (0, import_jsx_runtime33.jsx)("div", { "data-testid": "console-loading", children: "Loading console..." });
  if (error) return /* @__PURE__ */ (0, import_jsx_runtime33.jsx)("div", { "data-testid": "console-error", children: error });
  const focusedMemberId = dock.focusedTarget?.kind === "agent-chat" ? dock.focusedTarget.memberId : "";
  const sidebarVS = buildSidebarViewState({ agents, selectedMemberId: focusedMemberId, pinnedAgentIds });
  const activityVS = buildActivityRailViewState({
    agents,
    eventFrames: activityRef.current,
    filterPresets: experience?.activity_feed?.filter_presets,
    activePresetId: activeActivityPresetId
  });
  function renderChatPanel(panel) {
    const target = panel.target;
    if (!target || target.kind !== "agent-chat") return null;
    const panelKey = buildPanelConversationKey(panel.id, target);
    const identity = target.identity || target.memberId;
    const agent = agents.find((c) => c.member_id === target.memberId) || null;
    const sortedFrames = framesVisibleInPanel(getSortedFrames(identity), panel.id);
    const conversationEntries = mapFramesToTimelineEntries(agent, sortedFrames, {
      renderInteractionStartsAsUser: true,
      renderTextDeltas: true,
      blobBaseUrl: baseUrl
    });
    const optimisticUser = optimisticUserByPanelKeyRef.current[panelKey] ?? null;
    const optimisticEntry = optimisticUser ? optimisticUser.entry : null;
    const entries = sanitizeConversationEntries(sortConversationTimelineEntries([
      ...conversationEntries,
      ...optimisticEntry ? [optimisticEntry] : []
    ]));
    const conversation = buildConversationViewState({
      memberId: target.memberId,
      agentLabel: target.title,
      agent,
      entries
    });
    const draft = draftByKey[panelKey] || "";
    const staged = stagedAttachmentsByIdentity[identity] ?? [];
    const isSending = sendingPanels.has(panelKey);
    const phase = Object.prototype.hasOwnProperty.call(phaseRef.current, panelKey) ? phaseRef.current[panelKey] : agent?.response_phase ?? null;
    const canRespawn = agent?.affordances?.can_respawn === true;
    const canRetire = agent?.affordances?.can_retire === true;
    const quickPrompts = buildQuickPromptSuggestions(agent).map((s) => ({
      id: s.id,
      kind: "pill",
      label: s.label,
      iconName: s.iconName || "i-bolt"
    }));
    const footerLeftItems = [
      { id: "target", kind: "sub-pill", label: `To: ${target.title}`, iconName: "i-team" },
      { id: "identity", kind: "sub-pill", label: target.identity || target.memberId, iconName: "i-terminal" }
    ];
    const footerRightItems = [
      ...agent?.role ? [{ id: "role", kind: "sub-pill", label: agent.role }] : [],
      ...phase ? [{ id: "phase", kind: "sub-pill", label: phase, iconName: "i-bolt" }] : [],
      { id: "state", kind: "sub-pill", label: agent?.state || "unknown", iconName: "i-dot" }
    ];
    const stackItems = getPendingStack(identity);
    const agentBusy = isIdentityBusy(identity);
    const stackSlot = stackItems.length > 0 ? /* @__PURE__ */ (0, import_jsx_runtime33.jsx)(
      PendingStack,
      {
        items: stackItems,
        agentBusy,
        reducedMotion,
        onSteer: (itemId) => onStackSteer(identity, itemId),
        onTrash: (itemId) => onStackTrash(identity, itemId),
        onEdit: (itemId) => onStackEdit(identity, itemId),
        onCommitEdit: (itemId, t) => onStackCommitEdit(identity, itemId, t),
        onCancelEdit: (itemId) => onStackCancelEdit(identity, itemId),
        onReorder: (dragId, dropId, where) => onStackReorder(identity, dragId, dropId, where),
        onClearAll: () => onStackClearAll(identity),
        onToggleExpand: (itemId) => onStackToggleExpand(identity, itemId)
      }
    ) : null;
    return /* @__PURE__ */ (0, import_jsx_runtime33.jsx)(
      ChatPane,
      {
        agent,
        agentLabel: target.title || agent?.label || identity,
        identity,
        entries,
        phase,
        draft,
        sending: isSending,
        staged,
        onDraftChange: (v) => setDraftByKey((c) => ({ ...c, [panelKey]: v })),
        onStagedChange: (action) => setStagedAttachmentsForIdentity(identity, action),
        onSend: (attachments) => onSendMessage(panel.id, target, attachments),
        onInspect: () => {
          if (agent) dock.openTarget(buildInspectTarget(agent), "new_tab");
        },
        onRespawn: canRespawn ? () => void onLifecycleAction(identity, "mobkit/respawn") : void 0,
        onRetire: canRetire ? () => void onLifecycleAction(identity, "mobkit/retire") : void 0,
        stackSlot
      }
    );
  }
  function renderInspectPanel(target) {
    const inspect = inspectByIdentity[target.identity];
    const agent = agents.find((candidate) => candidate.identity === target.identity || candidate.member_id === target.identity);
    const canRespawn = agent?.affordances?.can_respawn === true;
    const canRetire = agent?.affordances?.can_retire === true;
    const canReset = experience?.runtime_capabilities?.can_retire_members === true;
    return /* @__PURE__ */ (0, import_jsx_runtime33.jsxs)("div", { className: "console-panel", "data-testid": `inspect-panel:${target.identity}`, children: [
      /* @__PURE__ */ (0, import_jsx_runtime33.jsxs)("div", { className: "console-panel__header", children: [
        /* @__PURE__ */ (0, import_jsx_runtime33.jsx)("h3", { children: target.identity }),
        /* @__PURE__ */ (0, import_jsx_runtime33.jsxs)("div", { className: "console-panel__actions", children: [
          canRespawn ? /* @__PURE__ */ (0, import_jsx_runtime33.jsx)("button", { "data-testid": `inspect-action:${target.identity}:respawn`, type: "button", onClick: () => void onLifecycleAction(target.identity, "mobkit/respawn"), children: "Respawn" }) : null,
          canReset ? /* @__PURE__ */ (0, import_jsx_runtime33.jsx)("button", { "data-testid": `inspect-action:${target.identity}:reset`, type: "button", onClick: () => void onLifecycleAction(target.identity, "mobkit/reset"), children: "Reset" }) : null,
          canRetire ? /* @__PURE__ */ (0, import_jsx_runtime33.jsx)("button", { "data-testid": `inspect-action:${target.identity}:retire`, type: "button", onClick: () => void onLifecycleAction(target.identity, "mobkit/retire"), children: "Retire" }) : null
        ] })
      ] }),
      !inspect ? /* @__PURE__ */ (0, import_jsx_runtime33.jsx)("p", { children: "Loading identity details\u2026" }) : /* @__PURE__ */ (0, import_jsx_runtime33.jsxs)("dl", { className: "console-panel__grid", children: [
        /* @__PURE__ */ (0, import_jsx_runtime33.jsx)("dt", { children: "State" }),
        /* @__PURE__ */ (0, import_jsx_runtime33.jsx)("dd", { children: inspect.state }),
        /* @__PURE__ */ (0, import_jsx_runtime33.jsx)("dt", { children: "Role" }),
        /* @__PURE__ */ (0, import_jsx_runtime33.jsx)("dd", { children: inspect.role || "n/a" }),
        /* @__PURE__ */ (0, import_jsx_runtime33.jsx)("dt", { children: "Addressability" }),
        /* @__PURE__ */ (0, import_jsx_runtime33.jsx)("dd", { children: inspect.addressability }),
        /* @__PURE__ */ (0, import_jsx_runtime33.jsx)("dt", { children: "Generation" }),
        /* @__PURE__ */ (0, import_jsx_runtime33.jsx)("dd", { children: inspect.continuity?.generation ?? "n/a" }),
        /* @__PURE__ */ (0, import_jsx_runtime33.jsx)("dt", { children: "Checkpoint" }),
        /* @__PURE__ */ (0, import_jsx_runtime33.jsx)("dd", { children: inspect.continuity?.checkpoint_version ?? "n/a" }),
        /* @__PURE__ */ (0, import_jsx_runtime33.jsx)("dt", { children: "Session" }),
        /* @__PURE__ */ (0, import_jsx_runtime33.jsx)("dd", { children: inspect.continuity?.session_id || "n/a" }),
        /* @__PURE__ */ (0, import_jsx_runtime33.jsx)("dt", { children: "Runtime" }),
        /* @__PURE__ */ (0, import_jsx_runtime33.jsx)("dd", { children: inspect.continuity?.agent_runtime_id || "n/a" }),
        /* @__PURE__ */ (0, import_jsx_runtime33.jsx)("dt", { children: "Lease Healthy" }),
        /* @__PURE__ */ (0, import_jsx_runtime33.jsx)("dd", { children: String(inspect.lease_healthy ?? inspect.lease?.healthy ?? false) }),
        /* @__PURE__ */ (0, import_jsx_runtime33.jsx)("dt", { children: "Peers" }),
        /* @__PURE__ */ (0, import_jsx_runtime33.jsx)("dd", { children: inspect.topology_peers?.join(", ") || "none" }),
        /* @__PURE__ */ (0, import_jsx_runtime33.jsx)("dt", { children: "Output Preview" }),
        /* @__PURE__ */ (0, import_jsx_runtime33.jsx)("dd", { children: inspect.output_preview || "n/a" })
      ] })
    ] });
  }
  function renderHealthPanel(identities) {
    return /* @__PURE__ */ (0, import_jsx_runtime33.jsx)("div", { className: "console-panel", "data-testid": "health-panel", children: /* @__PURE__ */ (0, import_jsx_runtime33.jsx)("ul", { className: "console-panel__list", children: identities.map((r2) => /* @__PURE__ */ (0, import_jsx_runtime33.jsxs)("li", { "data-testid": `health-identity:${r2.identity}`, children: [
      /* @__PURE__ */ (0, import_jsx_runtime33.jsx)("strong", { children: r2.display_name || r2.identity }),
      " \xB7 ",
      r2.state,
      " \xB7 ",
      r2.addressability
    ] }, r2.identity)) }) });
  }
  function handleInspectAgent(agent) {
    dock.openTarget(buildInspectTarget(agent), "new_tab");
  }
  const mobName = experience?.agent_sidebar?.title || "mob";
  const mobStatus = experience?.health_overview?.live_snapshot?.running === false ? "stopped" : "running";
  function toggleTheme() {
    const next = theme === "dark" ? "light" : "dark";
    setTheme(next);
    try {
      localStorage.setItem("mobkit-console-theme", next);
    } catch {
    }
  }
  function renderPanelBody(panel) {
    const target = panel.target;
    if (!target) return /* @__PURE__ */ (0, import_jsx_runtime33.jsx)("div", { className: "console-panel", children: "No panel target" });
    if (target.kind === "agent-chat") return renderChatPanel(panel);
    if (target.kind === "identity-inspect") return renderInspectPanel(target);
    if ((target.kind === "routing" || target.kind === "gating" || target.kind === "gates") && !hasMobControlSurface) {
      return /* @__PURE__ */ (0, import_jsx_runtime33.jsx)("div", { className: "console-panel", children: "This view requires a mob runtime control surface." });
    }
    if (target.kind === "routing") return /* @__PURE__ */ (0, import_jsx_runtime33.jsx)(RoutingPanel, { data: routingData });
    if (target.kind === "gating") return /* @__PURE__ */ (0, import_jsx_runtime33.jsx)(
      GatingInboxPanel,
      {
        pending: gatingData.pending,
        audit: gatingData.audit,
        onDecide: (pid, decision) => void onGatingDecision(pid, decision)
      }
    );
    if (target.kind === "topology") return /* @__PURE__ */ (0, import_jsx_runtime33.jsx)(
      TopologyPanel,
      {
        nodes: experience?.topology?.live_snapshot?.nodes || [],
        agents,
        activity: liveFramesRef.current
      }
    );
    if (target.kind === "health") return renderHealthPanel(experience?.health_overview?.live_snapshot?.identities || []);
    if (target.kind === "timeline") return /* @__PURE__ */ (0, import_jsx_runtime33.jsx)(TimelinePanel, { frames: activityRef.current });
    if (target.kind === "roster") return /* @__PURE__ */ (0, import_jsx_runtime33.jsx)(
      RosterPanel,
      {
        agents,
        onSelect: (a) => dock.openTarget(buildDockTarget(a), "replace_focused"),
        onInspect: handleInspectAgent,
        onLifecycle: (identity, method) => void onLifecycleAction(identity, method),
        canResetLifecycle: hasMobControlSurface
      }
    );
    if (target.kind === "gates") return /* @__PURE__ */ (0, import_jsx_runtime33.jsx)(GatesPanel, { audit: gatingData.audit });
    if (target.kind === "logs") return /* @__PURE__ */ (0, import_jsx_runtime33.jsx)(LogsPanel, { frames: activityRef.current });
    return /* @__PURE__ */ (0, import_jsx_runtime33.jsx)("div", { className: "console-panel", children: "Unsupported panel" });
  }
  return /* @__PURE__ */ (0, import_jsx_runtime33.jsxs)(
    "div",
    {
      className: "cc-theme-scope mobkit-shell",
      "data-cc-theme": theme,
      "data-cc-variant": variant,
      "data-testid": "meerkat-console",
      children: [
        /* @__PURE__ */ (0, import_jsx_runtime33.jsx)(SpriteSheet, {}),
        /* @__PURE__ */ (0, import_jsx_runtime33.jsx)(
          Topbar,
          {
            mobName,
            mobStatus,
            theme,
            onToggleTheme: toggleTheme,
            sidebarCollapsed,
            railCollapsed,
            onToggleSidebar: toggleSidebarCollapsed,
            onToggleRail: toggleRailCollapsed
          }
        ),
        /* @__PURE__ */ (0, import_jsx_runtime33.jsxs)(
          "div",
          {
            className: "shell",
            "data-sidebar-collapsed": sidebarCollapsed ? "true" : "false",
            "data-rail-collapsed": railCollapsed ? "true" : "false",
            children: [
              /* @__PURE__ */ (0, import_jsx_runtime33.jsx)(
                Sidebar,
                {
                  agents,
                  selectedMemberId: focusedMemberId,
                  recentActivity: activityRef.current,
                  collapsed: sidebarCollapsed,
                  visibleControls,
                  onSelect: (a) => dock.openTarget(buildDockTarget(a), "replace_focused"),
                  onInspect: (a) => dock.openTarget(buildInspectTarget(a), "replace_focused"),
                  onOpenControl: (kind) => {
                    if (!visibleControls.includes(kind)) return;
                    dock.openTarget(buildControlTarget(kind), "replace_focused");
                  }
                }
              ),
              /* @__PURE__ */ (0, import_jsx_runtime33.jsx)("div", { className: "pane-resizer", "aria-hidden": "true", "data-testid": "resize:sidebar", onPointerDown: handleSidebarResize }),
              /* @__PURE__ */ (0, import_jsx_runtime33.jsx)("div", { className: "main", children: /* @__PURE__ */ (0, import_jsx_runtime33.jsx)(
                MobKitDock,
                {
                  viewState: dock.viewState,
                  agents,
                  renderPanelBody,
                  visibleControls,
                  onSelectTab: (id) => dock.selectTab(id),
                  onCloseTab: (id) => dock.closeTab(id),
                  onCreateTab: () => dock.createTab(),
                  onFocusPanel: (id) => dock.focusPanel(id),
                  onSplitPanel: (id, dir) => dock.splitPanel(id, dir),
                  onClosePanel: (id) => dock.closePanel(id),
                  onResizeSplit: (id, ratio) => dock.resizeSplit(id, ratio),
                  onOpenTargetInPanel: (panelId, target) => {
                    dock.focusPanel(panelId);
                    dock.openTarget(target, "replace_focused");
                  }
                }
              ) }),
              /* @__PURE__ */ (0, import_jsx_runtime33.jsx)("div", { className: "pane-resizer pane-resizer--activity", "aria-hidden": "true", "data-testid": "resize:activity", onPointerDown: handleActivityResize }),
              /* @__PURE__ */ (0, import_jsx_runtime33.jsx)(
                SignalsRail,
                {
                  frames: activityRef.current,
                  collapsed: railCollapsed
                }
              )
            ]
          }
        )
      ]
    }
  );
}

// src/index.tsx
var import_jsx_runtime34 = require("react/jsx-runtime");
function createConsoleApp(target, options = {}) {
  if (!target) {
    throw new Error("target element is required");
  }
  const baseUrl = options.baseUrl || "";
  const root = (0, import_client.createRoot)(target);
  root.render(/* @__PURE__ */ (0, import_jsx_runtime34.jsx)(ConsoleApp, { baseUrl }));
  return {
    unmount() {
      root.unmount();
    }
  };
}
