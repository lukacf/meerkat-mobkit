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
var import_react26 = __toESM(require("react"));

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
  const existingRanges = selection ? Array.from({ length: selection.rangeCount }, (_value, index2) => selection.getRangeAt(index2)) : [];
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
    const index2 = codeTokens.push(`<code class="cc-rich-inline-code">${code}</code>`) - 1;
    return `@@CODE_${index2}@@`;
  }).replace(/\*\*([^*\n]+)\*\*/g, "<strong>$1</strong>").replace(/(^|[^A-Za-z0-9_*])\*([^*\n]+)\*(?![A-Za-z0-9_*])/g, "$1<em>$2</em>").replace(/(^|[^A-Za-z0-9_])_([^_\n]+)_(?![A-Za-z0-9_])/g, "$1<em>$2</em>").replace(/\[([^\]]+)\]\(([^)]+)\)/g, '<a href="$2">$1</a>').replace(/\n/g, "<br />");
  return escaped.replace(/@@CODE_(\d+)@@/g, (_match, index2) => codeTokens[Number(index2)] || "");
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
  const rows = lines.slice(2).map((line) => splitMarkdownTableRow(line)).filter((cells) => cells.length > 0 && cells.some((cell) => cell.length > 0)).map((cells) => headers.map((_header, index2) => cells[index2] || ""));
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
function renderBlock(block, index2, Icon2) {
  if (block.type === "paragraph") {
    return /* @__PURE__ */ (0, import_jsx_runtime5.jsx)("p", { className: "cc-rich-paragraph", dangerouslySetInnerHTML: markdownHtml(block.text) }, `paragraph-${index2}`);
  }
  if (block.type === "heading") {
    return /* @__PURE__ */ (0, import_jsx_runtime5.jsx)(
      "h3",
      {
        className: `cc-rich-heading cc-rich-heading--${Number(block.level) || 2}`,
        dangerouslySetInnerHTML: markdownHtml(block.text)
      },
      `heading-${index2}`
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
    ] }, `code-${index2}`);
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
    ] }) }, `table-${index2}`);
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
    ] }, `command-${index2}`);
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
    ] }, `file-change-${index2}`);
  }
  if (block.type === "divider") {
    return /* @__PURE__ */ (0, import_jsx_runtime5.jsxs)("div", { className: "cc-rich-divider", children: [
      /* @__PURE__ */ (0, import_jsx_runtime5.jsx)("span", { className: "cc-rich-divider__line" }),
      /* @__PURE__ */ (0, import_jsx_runtime5.jsx)("span", { className: "cc-rich-divider__label", children: block.text }),
      /* @__PURE__ */ (0, import_jsx_runtime5.jsx)("span", { className: "cc-rich-divider__line" })
    ] }, `divider-${index2}`);
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
      `image-${index2}`
    );
  }
  if (block.type === "tool-call") {
    return /* @__PURE__ */ (0, import_jsx_runtime5.jsx)(ToolCallBlock, { block }, `tool-call-${index2}`);
  }
  const thinking = renderThinkingBlock(block);
  if (!thinking) {
    return null;
  }
  return /* @__PURE__ */ (0, import_jsx_runtime5.jsx)("div", { children: thinking }, `thinking-${index2}`);
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
          className: clsx_default("cc-tool-call__header", block.peerIncoming && "cc-tool-call__header--incoming-peer"),
          type: "button",
          onClick: () => setExpanded((prev) => !prev),
          "aria-expanded": expanded,
          children: [
            /* @__PURE__ */ (0, import_jsx_runtime5.jsx)("span", { className: "cc-tool-call__chevron", children: expanded ? "\u25BE" : "\u25B8" }),
            /* @__PURE__ */ (0, import_jsx_runtime5.jsx)("span", { className: "cc-tool-call__icon", children: arrow }),
            block.peerIncoming ? /* @__PURE__ */ (0, import_jsx_runtime5.jsxs)("span", { className: "cc-tool-call__peer-summary", children: [
              /* @__PURE__ */ (0, import_jsx_runtime5.jsxs)("span", { className: "cc-tool-call__name", children: [
                "Received from ",
                target
              ] }),
              content && /* @__PURE__ */ (0, import_jsx_runtime5.jsx)("span", { className: "cc-tool-call__preview", children: content })
            ] }) : /* @__PURE__ */ (0, import_jsx_runtime5.jsxs)(import_jsx_runtime5.Fragment, { children: [
              /* @__PURE__ */ (0, import_jsx_runtime5.jsxs)("span", { className: "cc-tool-call__name", children: [
                block.name,
                " \u2192 ",
                target
              ] }),
              block.peerIntent && /* @__PURE__ */ (0, import_jsx_runtime5.jsx)("span", { className: "cc-tool-call__peer-intent", children: block.peerIntent }),
              content && /* @__PURE__ */ (0, import_jsx_runtime5.jsx)("span", { className: "cc-tool-call__preview", children: content })
            ] }),
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
  const targets = Array.from(new Set(blocks.map((b) => b.peerTarget || "peer")));
  const allSuccess = blocks.every((b) => b.status === "success");
  const anyError = blocks.some((b) => b.status === "error");
  const statusIcon = anyError ? "\u2717" : allSuccess ? "\u2713" : "\u22EF";
  const statusClass = anyError ? "cc-tool-call--error" : allSuccess ? "cc-tool-call--success" : "cc-tool-call--pending";
  const isIncoming = blocks[0]?.peerIncoming;
  const arrow = isIncoming ? "\u2199" : "\u2197";
  const label = isIncoming ? `Received from ${targets.join(", ")}` : blocks.length === 1 ? `${blocks[0]?.name || "peer"} \u2192 ${targets[0] || "peer"}` : `Sent to ${targets.join(", ")}`;
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
      /* @__PURE__ */ (0, import_jsx_runtime5.jsx)("span", { className: "cc-tool-call__peer-intent", children: block.name }),
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
  const body = blocks.map((block, index2) => renderBlock(block, index2, Icon2)).filter(Boolean);
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
  function dispatch2(action) {
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
    dispatch: dispatch2,
    createTab: () => dispatch2({ type: "create_tab" }),
    selectTab: (tabId) => dispatch2({ type: "select_tab", tabId }),
    closeTab: (tabId) => dispatch2({ type: "close_tab", tabId }),
    focusPanel: (panelId) => dispatch2({ type: "focus_panel", panelId }),
    closePanel: (panelId) => dispatch2({ type: "close_panel", panelId }),
    splitPanel: (panelId, direction) => dispatch2({ type: "split_panel", panelId, direction }),
    resizeSplit: (splitId, ratio) => dispatch2({ type: "resize_split", splitId, ratio }),
    applyPreset: (presetId) => dispatch2({ type: "apply_preset", presetId }),
    openTarget: (target, intent) => dispatch2({ type: "open_target", target, intent }),
    setPanelTarget: (panelId, target) => dispatch2({ type: "set_panel_target", panelId, target }),
    setPanelMode: (panelId, mode) => dispatch2({ type: "set_panel_mode", panelId, mode })
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
    const agents = snapshotAgents.map((entry) => {
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
        ...entry.subgroup !== void 0 && { subgroup: String(entry.subgroup) },
        ...entry.addressable !== void 0 ? { addressable: Boolean(entry.addressable) } : statusRow?.addressability ? { addressable: statusRow.addressability === "addressable" } : {},
        ...entry.affordances !== void 0 && { affordances: entry.affordances },
        model_capabilities: modelCapabilities,
        ...watchFields
      };
    });
    const seen = new Set(
      agents.flatMap((agent) => [agent.identity, agent.member_id, agent.agent_id]).filter((value) => Boolean(value)).map((value) => value.toLowerCase())
    );
    for (const statusRow of normalizedIdentityStatusRows) {
      if (seen.has(statusRow.identity.toLowerCase())) continue;
      const addressable = statusRow.addressability === "addressable";
      agents.push({
        identity: statusRow.identity,
        agent_id: statusRow.identity,
        member_id: statusRow.identity,
        label: String(statusRow.display_name || statusRow.identity),
        kind: String(statusRow.role || "identity"),
        ...statusRow.role !== void 0 ? { role: statusRow.role } : {},
        state: statusRow.state,
        addressability: statusRow.addressability,
        ...statusRow.generation !== void 0 ? { generation: statusRow.generation } : {},
        ...statusRow.checkpoint_version !== void 0 ? { checkpoint_version: statusRow.checkpoint_version } : {},
        ...statusRow.lease_healthy !== void 0 ? { lease_healthy: statusRow.lease_healthy } : {},
        ...statusRow.labels && Object.keys(statusRow.labels).length > 0 ? { labels: statusRow.labels } : {},
        ...statusRow.labels?.group ? { group: statusRow.labels.group } : {},
        ...statusRow.labels?.console_subgroup ? { subgroup: statusRow.labels.console_subgroup } : statusRow.labels?.org ? { subgroup: statusRow.labels.org } : {},
        addressable,
        affordances: { can_send_message: addressable },
        model_capabilities: { image_input: false }
      });
      seen.add(statusRow.identity.toLowerCase());
    }
    return agents;
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
  const targetKey = target.identity || target.memberId || target.id;
  return `panel:${panelId}:${target.kind}:${targetKey}`;
}
function buildDockTarget(agent) {
  const subtitle = [agent.role, agent.kind].filter(Boolean).join(" \xB7 ") || void 0;
  const identity = typeof agent.identity === "string" && agent.identity.trim() ? agent.identity.trim() : agent.member_id;
  return {
    id: agent.member_id,
    kind: "agent-chat",
    addressingMode: "identity",
    memberId: agent.member_id,
    identity,
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
    title: `${agent.label} Details`,
    subtitle: agent.identity || agent.member_id,
    iconName: "i-terminal"
  };
}
function buildControlTarget(kind) {
  switch (kind) {
    case "routing":
      return { id: "routing", kind, title: "Routing", subtitle: "Routes and delivery history", iconName: "i-swap" };
    case "gating":
      return { id: "gating", kind, title: "Approvals", subtitle: "Pending approvals, audit, and policies", iconName: "i-bolt" };
    case "topology":
      return { id: "topology", kind, title: "Topology", subtitle: "Identity connectivity", iconName: "i-team" };
    case "health":
      return { id: "health", kind, title: "Health", subtitle: "Runtime and identity health", iconName: "i-gear" };
    case "timeline":
      return { id: "timeline", kind, title: "Today", subtitle: "Chronological events", iconName: "i-clock" };
    case "roster":
      return { id: "roster", kind, title: "Roster", subtitle: "All agents", iconName: "i-team" };
    case "gates":
      return { id: "gating", kind: "gating", title: "Approvals", subtitle: "Pending approvals, audit, and policies", iconName: "i-bolt" };
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
  const sorted = [...agents].sort((a2, b) => {
    const aPinned = pinnedAgentIds.has(a2.member_id) ? 0 : 1;
    const bPinned = pinnedAgentIds.has(b.member_id) ? 0 : 1;
    if (aPinned !== bPinned) return aPinned - bPinned;
    if (sortMode === "alpha") return a2.label.localeCompare(b.label);
    if (sortMode === "status") {
      const stateOrder = (s) => s === "running" ? 0 : s === "active" ? 1 : 2;
      const diff = stateOrder(a2.state) - stateOrder(b.state);
      if (diff !== 0) return diff;
    }
    return a2.label.localeCompare(b.label);
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
            label: "Open roster details",
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
var COMMS_IDENTITY = {
  id: "comms",
  label: "",
  role: "system",
  showLabel: false
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
    case "assistant_image_appended":
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
  for (const frame2 of frames) {
    const interactionId = frame2.interactionId?.trim();
    const timestampMs = typeof frame2.timestampMs === "number" ? frame2.timestampMs : Number.MAX_SAFE_INTEGER;
    if (!interactionId) continue;
    const current = interactionStartMs.get(interactionId);
    if (current === void 0 || timestampMs < current) {
      interactionStartMs.set(interactionId, timestampMs);
    }
  }
  return frames.map((frame2, index2) => ({ frame: frame2, index: index2 })).sort((left, right) => {
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
  }).map(({ frame: frame2 }) => frame2);
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
function parseToolCallId(frame2) {
  const record = frame2.data && typeof frame2.data === "object" ? frame2.data : null;
  const id = record?.tool_call_id ?? record?.id;
  return typeof id === "string" && id.trim() ? id.trim() : null;
}
function parseToolName(frame2) {
  const record = frame2.data && typeof frame2.data === "object" ? frame2.data : null;
  return typeof record?.name === "string" && record.name.trim() ? record.name : "tool";
}
function parseToolArguments(frame2) {
  const record = frame2.data && typeof frame2.data === "object" ? frame2.data : null;
  if (typeof record?.arguments === "string" && record.arguments.trim()) {
    return record.arguments;
  }
  if ("args" in (record || {}) && record?.args !== void 0) {
    return JSON.stringify(record.args);
  }
  return JSON.stringify(record || {});
}
function normalizeToolArgumentsForSignature(argumentsText) {
  const trimmed = (argumentsText || "").trim();
  if (!trimmed) return "";
  try {
    return JSON.stringify(JSON.parse(trimmed));
  } catch {
    return trimmed.replace(/\s+/g, " ");
  }
}
function toolBlockSignature(block) {
  return `${block.name}\0${normalizeToolArgumentsForSignature(block.arguments)}`;
}
function addToolSignatureCount(counts, block) {
  const key = toolBlockSignature(block);
  counts.set(key, (counts.get(key) || 0) + 1);
}
function consumeToolSignatureCount(counts, block) {
  const key = toolBlockSignature(block);
  const count = counts.get(key) || 0;
  if (count <= 0) return false;
  if (count === 1) counts.delete(key);
  else counts.set(key, count - 1);
  return true;
}
function liveToolDedupeState(frames, toolBlocks) {
  const liveToolCallIds = /* @__PURE__ */ new Set();
  const liveToolSignatureCounts = /* @__PURE__ */ new Map();
  for (const frame2 of frames) {
    if (frame2.sourceKind === "session_history") continue;
    if (frame2.event !== "tool_call_requested" && frame2.event !== "tool_call" && frame2.event !== "tool_execution_started") {
      continue;
    }
    const toolCallId = parseToolCallId(frame2);
    if (!toolCallId || liveToolCallIds.has(toolCallId)) continue;
    const block = toolBlocks.get(toolCallId);
    if (!block) continue;
    liveToolCallIds.add(toolCallId);
    addToolSignatureCount(liveToolSignatureCounts, block);
  }
  return { liveToolCallIds, liveToolSignatureCounts };
}
var TECHNICAL_PEER_INTENTS = /* @__PURE__ */ new Set(["checksum_token"]);
var PEER_PAYLOAD_TEXT_KEYS = [
  "message",
  "body",
  "text",
  "summary",
  "reply",
  "content",
  "subject",
  "question",
  "prompt",
  "description",
  "request",
  "request_subject",
  "token",
  "status_line"
];
function isTechnicalPeerIntent(intent) {
  return Boolean(intent && TECHNICAL_PEER_INTENTS.has(intent.trim()));
}
function displayPeerIntent(intent) {
  if (!intent) return void 0;
  const trimmed = intent.trim();
  if (!trimmed || isTechnicalPeerIntent(trimmed)) return void 0;
  return trimmed;
}
function parseJsonPayload(value) {
  try {
    return JSON.parse(value);
  } catch {
    return null;
  }
}
function summarizePeerPayload(value) {
  if (typeof value === "string") {
    const trimmed = value.trim();
    if (!trimmed) return void 0;
    if (trimmed.startsWith("{") && trimmed.endsWith("}") || trimmed.startsWith("[") && trimmed.endsWith("]") || trimmed.startsWith('"') && trimmed.endsWith('"')) {
      const parsed = parseJsonPayload(trimmed);
      if (parsed !== null) {
        return summarizePeerPayload(parsed) || trimmed;
      }
    }
    return trimmed.replace(/^["']|["']$/g, "");
  }
  if (Array.isArray(value)) {
    const parts = value.map((item) => summarizePeerPayload(item)).filter((item) => Boolean(item));
    return parts.length ? parts.join(" ") : void 0;
  }
  if (value && typeof value === "object") {
    const record = value;
    const type = typeof record.type === "string" ? record.type : "";
    if (type === "image" || type === "image_ref" || type === "image_upload") {
      return typeof record.alt === "string" && record.alt.trim() ? record.alt.trim() : type === "image_ref" ? "referenced image" : "attached image";
    }
    for (const key of PEER_PAYLOAD_TEXT_KEYS) {
      const summary = summarizePeerPayload(record[key]);
      if (summary) return summary;
    }
    return JSON.stringify(record);
  }
  return void 0;
}
function extractPeerBodyFromArgs(argsRecord) {
  if (!argsRecord) return void 0;
  const directBody = summarizePeerPayload(argsRecord.body);
  if (directBody) return directBody;
  const paramsBody = summarizePeerPayload(argsRecord.params);
  if (paramsBody) return paramsBody;
  const resultBody = summarizePeerPayload(argsRecord.result);
  if (resultBody) return resultBody;
  return void 0;
}
function capturePeersResult(peerRegistry, rawResult) {
  const resultText = typeof rawResult === "string" ? rawResult : rawResult && typeof rawResult === "object" ? JSON.stringify(rawResult) : "";
  if (!resultText) return;
  try {
    const parsed = JSON.parse(resultText);
    if (!Array.isArray(parsed.peers)) return;
    for (const peer of parsed.peers) {
      if (typeof peer.peer_id === "string" && typeof peer.name === "string") {
        peerRegistry.set(peer.peer_id, peer.name);
      }
    }
  } catch {
  }
}
function peerTargetFromArgs(argsRecord, peerRegistry) {
  const peerId = typeof argsRecord?.peer_id === "string" ? argsRecord.peer_id.trim() : "";
  const registryName = peerId ? peerRegistry?.get(peerId) : void 0;
  return registryName ? peerLastSegment(registryName) : typeof argsRecord?.display_name === "string" && argsRecord.display_name.trim() ? peerLastSegment(argsRecord.display_name.trim()) : typeof argsRecord?.to === "string" && argsRecord.to.trim() ? peerLastSegment(argsRecord.to.trim()) : peerId ? peerId.slice(0, 8) : void 0;
}
function parseToolResult(frame2) {
  const record = frame2.data && typeof frame2.data === "object" ? frame2.data : null;
  const isError = Boolean(record?.is_error) || frame2.event === "interaction_failed";
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
  if (!result && frame2.event === "tool_result_received") {
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
  const peerRegistry = buildPeerRegistry(frames);
  for (const frame2 of frames) {
    if (frame2.event === "tool_result_received" || frame2.event === "tool_execution_completed") {
      const toolCallId = parseToolCallId(frame2);
      const data = frame2.data;
      if (data && (data.name === "peers" || data.tool_name === "peers")) {
        capturePeersResult(peerRegistry, data.result);
      }
      if (!toolCallId) continue;
      const parsed = parseToolResult(frame2);
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
    if (frame2.event === "tool_call_requested" || frame2.event === "tool_call" || frame2.event === "tool_execution_started") {
      const toolCallId = parseToolCallId(frame2);
      if (!toolCallId || toolCalls.has(toolCallId)) continue;
      const pending = pendingResults.get(toolCallId);
      const name = parseToolName(frame2);
      const args = frame2.data && typeof frame2.data === "object" ? frame2.data.args : null;
      const argsRecord = args && typeof args === "object" ? args : null;
      const isPeerTool = name === "send_request" || name === "send_message" || name === "send_response";
      const peerTarget2 = isPeerTool ? peerTargetFromArgs(argsRecord, peerRegistry) : void 0;
      const rawPeerIntent = isPeerTool && typeof argsRecord?.intent === "string" ? argsRecord.intent : void 0;
      const peerIntent = displayPeerIntent(rawPeerIntent);
      const peerBody = isPeerTool ? extractPeerBodyFromArgs(argsRecord) : void 0;
      toolCalls.set(toolCallId, {
        type: "tool-call",
        toolCallId,
        name,
        arguments: parseToolArguments(frame2),
        ...pending?.result ? { result: pending.result } : {},
        status: pending?.status || "pending",
        ...peerTarget2 ? { peerTarget: peerTarget2 } : {},
        ...peerIntent ? { peerIntent } : {},
        ...peerBody ? { peerBody } : {}
      });
    }
  }
  return toolCalls;
}
function buildPeerRegistry(frames) {
  const peerRegistry = /* @__PURE__ */ new Map();
  for (const frame2 of frames) {
    if (frame2.event !== "tool_result_received" && frame2.event !== "tool_execution_completed") continue;
    const data = frame2.data && typeof frame2.data === "object" ? frame2.data : null;
    if (!data || data.name !== "peers" && data.tool_name !== "peers") continue;
    capturePeersResult(peerRegistry, data.result);
  }
  return peerRegistry;
}
function parsePeerSummary(text) {
  const match = text.match(/Peer\s+(response|request|message):\s*(.+?)(?:\s*Status:\s|$)/s);
  if (!match) return null;
  const [, verb, body] = match;
  let summary = body.trim();
  try {
    const parsed = JSON.parse(summary);
    if (typeof parsed === "object" && parsed !== null) {
      if (typeof parsed.summary === "string") summary = parsed.summary;
      else if (typeof parsed.text === "string") summary = parsed.text;
      else if (typeof parsed.body === "string") summary = parsed.body;
      else if (typeof parsed.message === "string") summary = parsed.message;
    }
  } catch {
    summary = summary.replace(/^["']|["']$/g, "");
  }
  return { verb, summary };
}
function renderPeerEntry(frame2, entryId) {
  const rawText = summarizeFrameData(frame2.data);
  if (!rawText) return null;
  const peer = parsePeerSummary(rawText);
  if (!peer) return null;
  return {
    kind: "message",
    id: entryId,
    identity: SYSTEM_IDENTITY,
    variant: "meta",
    createdAt: isoFromTimestampMs(frame2.timestampMs),
    text: `\u21A9 ${peer.verb}: ${peer.summary}`
  };
}
function renderTerminalEntry(agent, frame2, entryId, streamedText = "") {
  if (frame2.event === "interaction_complete") {
    const text = summarizeFrameData(frame2.data).trim();
    if (!text) return null;
    const peer = parsePeerSummary(text);
    if (peer) {
      return {
        kind: "message",
        id: entryId,
        identity: SYSTEM_IDENTITY,
        variant: "meta",
        createdAt: isoFromTimestampMs(frame2.timestampMs),
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
      createdAt: isoFromTimestampMs(frame2.timestampMs),
      ...blocks.length > 0 ? { blocks } : { text }
    };
  }
  if (frame2.event === "interaction_failed" || frame2.event === "run_failed") {
    const text = `${frame2.event}: ${summarizeFrameData(frame2.data)}`.trim();
    if (!text || text === `${frame2.event}:`) return null;
    return {
      kind: "message",
      id: entryId,
      identity: SYSTEM_IDENTITY,
      variant: "meta",
      createdAt: isoFromTimestampMs(frame2.timestampMs),
      text
    };
  }
  return null;
}
function terminalFrameVisibleText(frame2) {
  if (frame2.event === "text_complete") {
    const record = frame2.data && typeof frame2.data === "object" ? frame2.data : null;
    if (typeof record?.content === "string") return record.content;
    if (typeof record?.text === "string") return record.text;
  }
  if (frame2.event === "interaction_complete" || frame2.event === "run_completed" || frame2.event === "text_complete") {
    return summarizeFrameData(frame2.data);
  }
  return "";
}
function liveAssistantTerminalTextSignatures(frames) {
  const signatures = /* @__PURE__ */ new Set();
  for (const frame2 of frames) {
    if (frame2.sourceKind === "session_history") continue;
    const text = terminalFrameVisibleText(frame2).trim();
    if (!text) continue;
    signatures.add(normalizeComparableText(text));
  }
  return signatures;
}
function buildBlobUrl(blobId, baseUrl) {
  const path = `/blobs/${encodeURIComponent(blobId)}`;
  const base = baseUrl?.trim();
  if (!base) return path;
  return `${base.replace(/\/+$/, "")}${path}`;
}
function renderAssistantImageEntry(agent, frame2, entryId, blobBaseUrl) {
  const data = frame2.data && typeof frame2.data === "object" ? frame2.data : {};
  const image = data.image && typeof data.image === "object" ? data.image : data;
  const blobRef = image.blob_ref && typeof image.blob_ref === "object" ? image.blob_ref : null;
  const blobId = typeof image.blob_id === "string" ? image.blob_id : typeof blobRef?.blob_id === "string" ? blobRef.blob_id : "";
  if (!blobId) return null;
  const mediaType = typeof image.media_type === "string" ? image.media_type : typeof blobRef?.media_type === "string" ? blobRef.media_type : "image/png";
  const width = typeof image.width === "number" ? image.width : void 0;
  const height = typeof image.height === "number" ? image.height : void 0;
  const imageId = typeof image.image_id === "string" ? image.image_id : void 0;
  return {
    kind: "message",
    id: entryId,
    identity: agentIdentity(agent),
    variant: "rich",
    createdAt: isoFromTimestampMs(frame2.timestampMs),
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
function renderGeneratedImageToolResultEntries(agent, frame2, entryId, blobBaseUrl) {
  const data = frame2.data && typeof frame2.data === "object" ? frame2.data : {};
  const name = typeof data.name === "string" ? data.name : typeof data.tool_name === "string" ? data.tool_name : "";
  if (name !== "generate_image") return [];
  let result = data.result;
  if (typeof result === "string") {
    try {
      result = JSON.parse(result);
    } catch {
      return [];
    }
  }
  if (!result || typeof result !== "object") return [];
  const images = result.images;
  if (!Array.isArray(images)) return [];
  return images.flatMap((image, index2) => {
    if (!image || typeof image !== "object") return [];
    const imageFrame = {
      ...frame2,
      data: { image }
    };
    const imageEntry = renderAssistantImageEntry(
      agent,
      imageFrame,
      `${entryId}:generated-image:${index2}`,
      blobBaseUrl
    );
    return imageEntry ? [imageEntry] : [];
  });
}
function imageEntryKey(entry) {
  if (entry.kind !== "message" || entry.variant !== "rich" || !("blocks" in entry)) {
    return null;
  }
  const block = entry.blocks?.[0];
  if (!block || block.type !== "image") return null;
  if (typeof block.blobId === "string" && block.blobId.trim()) {
    return `blob:${block.blobId.trim()}`;
  }
  if (typeof block.imageId === "string" && block.imageId.trim()) {
    return `image:${block.imageId.trim()}`;
  }
  if (typeof block.src === "string" && block.src.trim()) {
    return `src:${block.src.trim()}`;
  }
  return null;
}
function normalizeComparableText(value) {
  return value.replace(/\s+/g, " ").trim();
}
function conversationEntryVisibleText(entry) {
  if (entry.kind !== "message") return "";
  if ("text" in entry && typeof entry.text === "string") return entry.text;
  if (!("blocks" in entry) || !Array.isArray(entry.blocks)) return "";
  return entry.blocks.map((block) => {
    if (!block || typeof block !== "object") return "";
    const record = block;
    if (typeof record.text === "string") return record.text;
    if (typeof record.peerBody === "string") return record.peerBody;
    return "";
  }).filter(Boolean).join("\n");
}
function shouldSuppressRepeatedAssistantEntry(entry, priorEntries) {
  if (entry.kind !== "message") return false;
  if (entry.identity.id === USER_IDENTITY.id || entry.identity.id === COMMS_IDENTITY.id || entry.identity.id === SYSTEM_IDENTITY.id) {
    return false;
  }
  const signature = normalizeComparableText(conversationEntryVisibleText(entry));
  if (!signature) return false;
  const entryTs = Date.parse(String(entry.createdAt || ""));
  for (let index2 = priorEntries.length - 1; index2 >= 0; index2--) {
    const prior = priorEntries[index2];
    if (prior.kind !== "message") continue;
    if (prior.identity.id === USER_IDENTITY.id) {
      const userText = normalizeComparableText(conversationEntryVisibleText(prior));
      if (userText) return false;
      continue;
    }
    if (prior.identity.id !== entry.identity.id) continue;
    const priorSignature = normalizeComparableText(conversationEntryVisibleText(prior));
    if (priorSignature !== signature) continue;
    const priorTs = Date.parse(String(prior.createdAt || ""));
    if (Number.isFinite(entryTs) && Number.isFinite(priorTs) && Math.abs(entryTs - priorTs) > 15e3) {
      return false;
    }
    return true;
  }
  return false;
}
function buildQuickPromptSuggestions(agent) {
  const labels = agent?.labels ?? {};
  const suggestions = [];
  for (let index2 = 1; index2 <= 4; index2++) {
    const label = labels[`console_prompt_${index2}_label`]?.trim();
    const value = labels[`console_prompt_${index2}_value`]?.trim();
    if (!label || !value) continue;
    suggestions.push({
      id: `prompt-${index2}`,
      label,
      value,
      iconName: "i-bolt"
    });
  }
  return suggestions;
}
function renderHistoryUserEntry(frame2, entryId, blobBaseUrl) {
  if (frame2.event !== "interaction_started" && frame2.event !== "user_input") {
    return null;
  }
  if (typeof frame2.data !== "object" || frame2.data === null) {
    return null;
  }
  const record = frame2.data;
  const content = record.content;
  if (Array.isArray(content)) {
    const blocks = contentToUserBlocks(content, blobBaseUrl);
    if (blocks.length === 0) return null;
    return {
      kind: "message",
      id: entryId,
      identity: USER_IDENTITY,
      variant: "rich",
      createdAt: isoFromTimestampMs(frame2.timestampMs),
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
    createdAt: isoFromTimestampMs(frame2.timestampMs),
    text
  };
}
function userEntryTextSignature(entry) {
  if (entry.kind !== "message") return "";
  if ("text" in entry && typeof entry.text === "string") {
    return entry.text.replace(/\s+/g, " ").trim();
  }
  if ("blocks" in entry && Array.isArray(entry.blocks)) {
    return JSON.stringify(entry.blocks);
  }
  return "";
}
function userEntryDedupeKey(frame2, entry) {
  const interactionId = frame2.interactionId?.trim();
  if (interactionId) return `interaction:${interactionId}`;
  const signature = userEntryTextSignature(entry);
  if (frame2.sourceKind === "session_history" && /^You are\b/i.test(signature)) {
    return `history-kickoff:${signature}`;
  }
  const timestamp = typeof frame2.timestampMs === "number" ? frame2.timestampMs : "";
  return signature ? `content:${timestamp}:${signature}` : "";
}
function renderRunStartedPromptEntries(frame2, entryId, options = {}) {
  if (frame2.event !== "run_started" || typeof frame2.data !== "object" || frame2.data === null) {
    return [];
  }
  const record = frame2.data;
  const prompt = extractPromptText(record.prompt).trim();
  if (!prompt) {
    return [];
  }
  const createdAt = isoFromTimestampMs(frame2.timestampMs);
  const entries = [];
  void options;
  void createdAt;
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
function extractPromptText(prompt) {
  if (typeof prompt === "string") return prompt;
  if (!Array.isArray(prompt)) return "";
  return prompt.map((block) => {
    if (typeof block === "string") return block;
    if (!block || typeof block !== "object") return "";
    const record = block;
    if (typeof record.text === "string") return record.text;
    if (typeof record.content === "string") return record.content;
    return "";
  }).filter((value) => value.trim().length > 0).join("\n");
}
function contentToUserBlocks(content, blobBaseUrl) {
  if (typeof content === "string") {
    return parseConversationRichBlocks(content);
  }
  if (!Array.isArray(content)) {
    return [];
  }
  const blocks = [];
  for (const block of content) {
    if (typeof block === "string") {
      blocks.push(...parseConversationRichBlocks(block));
      continue;
    }
    if (!block || typeof block !== "object") continue;
    const record = block;
    const type = typeof record.type === "string" ? record.type : "";
    if (type === "text") {
      const text = typeof record.text === "string" ? record.text : typeof record.content === "string" ? record.content : "";
      blocks.push(...parseConversationRichBlocks(text));
      continue;
    }
    if (type === "image" || type === "image_ref") {
      const source = typeof record.source === "string" ? record.source : "";
      const blobId = typeof record.blob_id === "string" ? record.blob_id : typeof record.blobId === "string" ? record.blobId : "";
      const mediaType = typeof record.media_type === "string" ? record.media_type : typeof record.mediaType === "string" ? record.mediaType : "image/png";
      const inlineData = typeof record.data === "string" ? record.data : typeof record.base64 === "string" ? record.base64 : "";
      const src = source === "blob" && blobId ? buildBlobUrl(blobId, blobBaseUrl) : inlineData ? `data:${mediaType};base64,${inlineData}` : "";
      if (!src) continue;
      const alt = typeof record.alt === "string" && record.alt.trim() ? record.alt.trim() : type === "image_ref" ? "referenced image" : "attached image";
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
function peerLastSegment(value) {
  return value.split("/").pop() || value;
}
function blockAssistantToolBlocks(blocks, peerRegistry) {
  const toolBlocks = [];
  for (const block of blocks) {
    if (!block || typeof block !== "object") continue;
    const item = block;
    const blockType = typeof item.block_type === "string" ? item.block_type : typeof item.type === "string" ? item.type : "";
    if (blockType !== "tool_use") continue;
    const data = item.data && typeof item.data === "object" ? item.data : item;
    const name = typeof data.name === "string" && data.name.trim() ? data.name.trim() : "tool";
    const id = typeof data.id === "string" && data.id.trim() ? data.id.trim() : `history-tool-${toolBlocks.length + 1}`;
    const args = data.args !== void 0 ? data.args : data.arguments;
    const argsRecord = args && typeof args === "object" ? args : null;
    const argumentsText = args === void 0 ? "" : typeof args === "string" ? args : JSON.stringify(args);
    const isPeerTool = name === "send_request" || name === "send_message" || name === "send_response";
    const peerTarget2 = isPeerTool ? peerTargetFromArgs(argsRecord, peerRegistry) : void 0;
    const rawPeerIntent = isPeerTool && typeof argsRecord?.intent === "string" ? argsRecord.intent : void 0;
    const peerIntent = displayPeerIntent(rawPeerIntent);
    const peerBody = isPeerTool ? extractPeerBodyFromArgs(argsRecord) : void 0;
    toolBlocks.push({
      type: "tool-call",
      toolCallId: id,
      name,
      arguments: argumentsText,
      status: "success",
      ...peerTarget2 ? { peerTarget: peerTarget2 } : {},
      ...peerIntent ? { peerIntent } : {},
      ...peerBody ? { peerBody } : {}
    });
  }
  return toolBlocks;
}
function textFromUnknown(value) {
  return typeof value === "string" ? value.trim() : "";
}
function typedNoticeContentBlocks(content, blobBaseUrl) {
  return contentToUserBlocks(content, blobBaseUrl);
}
function typedNoticeBlockText(block) {
  const parts = [
    textFromUnknown(block.summary),
    textFromUnknown(block.body),
    textFromUnknown(block.detail),
    textFromUnknown(block.state),
    textFromUnknown(block.status)
  ].filter(Boolean);
  return parts.join("\n");
}
function isExternalEventOnlySystemNotice(message) {
  if (!message || typeof message !== "object") return false;
  const record = message;
  if (textFromUnknown(record.kind) === "external_event") return true;
  const blocks = record.blocks;
  if (!Array.isArray(blocks)) return false;
  let sawExternalEventBlock = false;
  for (const block of blocks) {
    if (!block || typeof block !== "object") continue;
    const type = textFromUnknown(block.type);
    if (!type) continue;
    if (type !== "external_event") return false;
    sawExternalEventBlock = true;
  }
  return sawExternalEventBlock;
}
function typedSystemNoticeBlocksToRich(blocks, body, blobBaseUrl) {
  const rich = [];
  const bodyText = textFromUnknown(body);
  if (!Array.isArray(blocks)) {
    if (bodyText) rich.push({ type: "paragraph", text: bodyText });
    return rich;
  }
  for (const block of blocks) {
    if (!block || typeof block !== "object") continue;
    const record = block;
    const type = textFromUnknown(record.type);
    if (type === "comms") {
      const peer = record.peer && typeof record.peer === "object" ? record.peer : {};
      const peerLabel = peerLastSegment(textFromUnknown(peer.display_name) || textFromUnknown(peer.id) || "peer");
      const kind = textFromUnknown(record.kind) || "message";
      const intent = textFromUnknown(record.intent);
      const requestId = textFromUnknown(record.request_id) || `typed-comms:${peerLabel}:${kind}`;
      const contentBlocks = typedNoticeContentBlocks(record.content, blobBaseUrl);
      const contentText = contentBlocks.map((item) => item.type === "paragraph" ? item.text : "").filter(Boolean).join("\n").trim();
      const displayBody = (contentText || typedNoticeBlockText(record)).replace(/^Peer\s+(?:message|request|response)\s+from\s+[^\n:]+:\s*/i, "").trim();
      rich.push({
        type: "tool-call",
        toolCallId: requestId,
        name: `peer_${kind}`,
        arguments: JSON.stringify(record.payload ?? {}, null, 2),
        status: "success",
        peerIncoming: true,
        peerTarget: peerLabel,
        ...intent ? { peerIntent: intent } : {},
        peerBody: displayBody || void 0
      });
      rich.push(...contentBlocks.filter((item) => item.type !== "paragraph"));
      continue;
    }
    if (type === "external_event") {
      continue;
    }
    if (type === "tool_config" || type === "mcp") {
      const payload = record.payload && typeof record.payload === "object" ? record.payload : record;
      const label = type === "mcp" ? "MCP" : "Tool config";
      const text = bodyText || typedNoticeBlockText(payload) || typedNoticeBlockText(record) || label;
      rich.push({ type: "divider", text });
      continue;
    }
    if (type === "background_job" || type === "auth" || type === "runtime_notice") {
      const text = typedNoticeBlockText(record) || type.replace(/_/g, " ");
      rich.push({ type: "paragraph", text });
      continue;
    }
    rich.push({ type: "divider", text: typedNoticeBlockText(record) || "Runtime metadata" });
  }
  if (rich.length === 0 && bodyText) rich.push({ type: "paragraph", text: bodyText });
  return rich;
}
function historyMessageText(message, peerRegistry, blobBaseUrl) {
  if (!message || typeof message !== "object") {
    return { role: null, text: "" };
  }
  const record = message;
  const role = typeof record.role === "string" ? record.role : null;
  switch (role) {
    case "user": {
      const text = extractTextFromContentBlocks(record.content);
      return { role: "user", text };
    }
    case "system_notice": {
      const blocks = typedSystemNoticeBlocksToRich(record.blocks, record.body, blobBaseUrl);
      const text = typeof record.body === "string" ? record.body : blocks.map((block) => block.type === "paragraph" || block.type === "divider" ? block.text : "").filter(Boolean).join("\n");
      return { role: "meta", text, ...blocks.length > 0 ? { blocks } : {} };
    }
    case "assistant":
      return { role: "assistant", text: typeof record.content === "string" ? record.content : "" };
    case "block_assistant": {
      const blocks = Array.isArray(record.blocks) ? record.blocks : [];
      const toolBlocks = blockAssistantToolBlocks(blocks, peerRegistry);
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
      }).filter((value) => value.trim().length > 0).join("\n\n");
      return { role: "assistant", text, ...toolBlocks.length > 0 ? { blocks: toolBlocks } : {} };
    }
    case "system":
      return { role: "system", text: typeof record.content === "string" ? record.content : "" };
    default:
      return { role: null, text: "" };
  }
}
function renderSessionHistoryTextCompleteEntry(agent, frame2, entryId, options = {}) {
  if (frame2.sourceKind !== "session_history") return null;
  const record = frame2.data && typeof frame2.data === "object" ? frame2.data : {};
  const parsed = historyMessageText(record.message, options.peerRegistry, options.blobBaseUrl);
  const text = parsed.text.trim();
  const parsedBlocks = Array.isArray(parsed.blocks) ? parsed.blocks : [];
  if (parsed.role === "meta") {
    const filteredParsedBlocks2 = options.consumeDuplicateToolBlock ? parsedBlocks.filter((block) => {
      if (block.type !== "tool-call") return true;
      return !options.consumeDuplicateToolBlock?.(block);
    }) : parsedBlocks;
    if (!text && filteredParsedBlocks2.length === 0) return null;
    const blocks2 = filteredParsedBlocks2.length > 0 ? filteredParsedBlocks2 : parseConversationRichBlocks(text);
    return {
      kind: "message",
      id: entryId,
      identity: COMMS_IDENTITY,
      variant: blocks2.length > 0 ? "rich" : "meta",
      createdAt: isoFromTimestampMs(frame2.timestampMs),
      ...blocks2.length > 0 ? { blocks: blocks2 } : { text }
    };
  }
  if (parsed.role !== "assistant" || !text && parsedBlocks.length === 0) return null;
  if (/^I have acknowledged the addition of the following peers:/i.test(text)) {
    return null;
  }
  const filteredParsedBlocks = options.consumeDuplicateToolBlock ? parsedBlocks.filter((block) => {
    if (block.type !== "tool-call") return true;
    return !options.consumeDuplicateToolBlock?.(block);
  }) : parsedBlocks;
  if (!text && filteredParsedBlocks.length === 0) return null;
  const blocks = filteredParsedBlocks.length > 0 ? filteredParsedBlocks : parseConversationRichBlocks(text);
  return {
    kind: "message",
    id: entryId,
    identity: agentIdentity(agent),
    variant: blocks.length > 0 ? "rich" : "plain",
    createdAt: isoFromTimestampMs(frame2.timestampMs),
    ...blocks.length > 0 ? { blocks } : { text }
  };
}
function renderSystemNoticeEntry(frame2, entryId, options = {}) {
  if (frame2.event !== "system_notice") return null;
  const record = frame2.data && typeof frame2.data === "object" ? frame2.data : {};
  const message = record.message && typeof record.message === "object" ? record.message : {
    role: "system_notice",
    kind: record.kind,
    render_class: record.render_class,
    body: record.body,
    blocks: record.blocks
  };
  if (isExternalEventOnlySystemNotice(message)) return null;
  const parsed = historyMessageText(message, void 0, options.blobBaseUrl);
  if (parsed.role !== "meta") return null;
  const parsedBlocks = Array.isArray(parsed.blocks) ? parsed.blocks : [];
  const filteredParsedBlocks = options.consumeDuplicateToolBlock ? parsedBlocks.filter((block) => {
    if (block.type !== "tool-call") return true;
    return !options.consumeDuplicateToolBlock?.(block);
  }) : parsedBlocks;
  const text = parsed.text.trim();
  if (!text && filteredParsedBlocks.length === 0) return null;
  const blocks = filteredParsedBlocks.length > 0 ? filteredParsedBlocks : parseConversationRichBlocks(text);
  return {
    kind: "message",
    id: entryId,
    identity: COMMS_IDENTITY,
    variant: blocks.length > 0 ? "rich" : "meta",
    createdAt: isoFromTimestampMs(frame2.timestampMs),
    ...blocks.length > 0 ? { blocks } : { text }
  };
}
function mapFramesToTimelineEntries(agent, frames, options = {}) {
  const orderedFrames = options.renderInteractionStartsAsUser ? sortFramesForTranscript(frames) : frames;
  const entries = [];
  const toolBlocks = buildToolBlocks(orderedFrames);
  const peerRegistry = buildPeerRegistry(orderedFrames);
  const emittedToolCalls = /* @__PURE__ */ new Set();
  const {
    liveToolCallIds,
    liveToolSignatureCounts
  } = liveToolDedupeState(orderedFrames, toolBlocks);
  const liveAssistantTerminalTexts = liveAssistantTerminalTextSignatures(orderedFrames);
  const emittedImages = /* @__PURE__ */ new Set();
  const emittedUserInputs = /* @__PURE__ */ new Set();
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
    const frame2 = orderedFrames[i];
    const entryId = `${frame2.id || frame2.event || "frame"}:${i}`;
    if (frame2.event === "text_delta") {
      if (options.renderTextDeltas === false) {
        continue;
      }
      if (!pendingId) {
        pendingId = entryId;
        pendingCreatedAt = isoFromTimestampMs(frame2.timestampMs);
      }
      pendingText += summarizeFrameData(frame2.data);
      continue;
    }
    if (frame2.event === "assistant_image" || frame2.event === "assistant_image_appended") {
      flushPendingText();
      const imageEntry = renderAssistantImageEntry(agent, frame2, entryId, options.blobBaseUrl);
      if (imageEntry) {
        const key = imageEntryKey(imageEntry);
        if (key && emittedImages.has(key)) {
          continue;
        }
        if (key) emittedImages.add(key);
        entries.push(imageEntry);
      }
      continue;
    }
    const toolCallId = parseToolCallId(frame2);
    if (toolCallId && (frame2.event === "tool_call_requested" || frame2.event === "tool_call" || frame2.event === "tool_execution_started") && !emittedToolCalls.has(toolCallId)) {
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
            createdAt: isoFromTimestampMs(frame2.timestampMs),
            blocks: [block]
          });
        }
        emittedToolCalls.add(toolCallId);
      }
      continue;
    }
    if (frame2.event === "tool_result_received" || frame2.event === "tool_execution_completed") {
      const imageEntries = renderGeneratedImageToolResultEntries(
        agent,
        frame2,
        entryId,
        options.blobBaseUrl
      );
      for (const imageEntry of imageEntries) {
        const key = imageEntryKey(imageEntry);
        if (key && emittedImages.has(key)) continue;
        if (key) emittedImages.add(key);
        entries.push(imageEntry);
      }
      continue;
    }
    if (options.renderInteractionStartsAsUser && (frame2.event === "interaction_started" || frame2.event === "user_input")) {
      flushPendingText();
      const userEntry = renderHistoryUserEntry(frame2, entryId, options.blobBaseUrl);
      if (userEntry) {
        const userKey = userEntryDedupeKey(frame2, userEntry);
        if (userKey && emittedUserInputs.has(userKey)) {
          continue;
        }
        if (userKey) emittedUserInputs.add(userKey);
        entries.push(userEntry);
      }
      continue;
    }
    if (frame2.event === "run_started") {
      flushPendingText();
      const promptEntries = renderRunStartedPromptEntries(frame2, entryId, {
        suppressEmbeddedRpcPrompt: options.renderInteractionStartsAsUser === true || options.suppressEmbeddedRunStartedPrompt === true
      });
      if (promptEntries.length > 0) {
        entries.push(...promptEntries);
        continue;
      }
    }
    if (frame2.event === "system_notice") {
      flushPendingText();
      const noticeEntry = renderSystemNoticeEntry(frame2, entryId, {
        blobBaseUrl: options.blobBaseUrl,
        consumeDuplicateToolBlock: (block) => liveToolCallIds.has(block.toolCallId) || consumeToolSignatureCount(liveToolSignatureCounts, block)
      });
      if (noticeEntry) {
        entries.push(noticeEntry);
      }
      continue;
    }
    if (frame2.event === "text_complete") {
      if (frame2.sourceKind !== "session_history") {
        const text2 = terminalFrameVisibleText(frame2).trim();
        const interactionId = frame2.interactionId?.trim();
        const duplicateTerminalFollows = text2 && orderedFrames.slice(i + 1).some((later) => {
          if (later.event !== "interaction_complete" && later.event !== "run_completed") {
            return false;
          }
          if (interactionId && later.interactionId?.trim() !== interactionId) {
            return false;
          }
          return normalizeComparableText(terminalFrameVisibleText(later)) === normalizeComparableText(text2);
        });
        if (duplicateTerminalFollows) {
          continue;
        }
      }
      const historyText = frame2.sourceKind === "session_history" ? terminalFrameVisibleText(frame2).trim() : "";
      if (historyText && liveAssistantTerminalTexts.has(normalizeComparableText(historyText))) {
        continue;
      }
      const historyEntry = renderSessionHistoryTextCompleteEntry(agent, frame2, entryId, {
        peerRegistry,
        blobBaseUrl: options.blobBaseUrl,
        consumeDuplicateToolBlock: (block) => liveToolCallIds.has(block.toolCallId) || consumeToolSignatureCount(liveToolSignatureCounts, block)
      });
      if (historyEntry) {
        flushPendingText();
        if (shouldSuppressRepeatedAssistantEntry(historyEntry, entries)) {
          continue;
        }
        entries.push(historyEntry);
      }
      continue;
    }
    if (frame2.event === "interaction_complete" || frame2.event === "interaction_failed" || frame2.event === "run_failed") {
      const streamedText = pendingText;
      flushPendingText();
      if (frame2.sourceKind === "session_history") {
        const historyText = terminalFrameVisibleText(frame2).trim();
        if (historyText && liveAssistantTerminalTexts.has(normalizeComparableText(historyText))) {
          continue;
        }
        const historyEntry = renderSessionHistoryTextCompleteEntry(agent, frame2, entryId, {
          peerRegistry,
          blobBaseUrl: options.blobBaseUrl,
          consumeDuplicateToolBlock: (block) => liveToolCallIds.has(block.toolCallId) || consumeToolSignatureCount(liveToolSignatureCounts, block)
        });
        if (historyEntry) {
          if (shouldSuppressRepeatedAssistantEntry(historyEntry, entries)) {
            continue;
          }
          entries.push(historyEntry);
        }
        continue;
      }
      const terminalEntry = renderTerminalEntry(agent, frame2, entryId, streamedText);
      if (terminalEntry) {
        if (shouldSuppressRepeatedAssistantEntry(terminalEntry, entries)) {
          continue;
        }
        entries.push(terminalEntry);
      }
      continue;
    }
    if (HIDDEN_EVENTS.has(frame2.event)) {
      continue;
    }
    flushPendingText();
    const peerEntry = renderPeerEntry(frame2, entryId);
    if (peerEntry) {
      entries.push(peerEntry);
      continue;
    }
    if (frame2.event === "tool_call_requested" || frame2.event === "tool_call" || frame2.event === "tool_execution_started" || frame2.event === "tool_result_received" || frame2.event === "tool_execution_completed") {
      continue;
    }
    const text = `${frame2.event}: ${summarizeFrameData(frame2.data)}`.trim();
    entries.push({
      kind: "message",
      id: entryId,
      identity: SYSTEM_IDENTITY,
      variant: "meta",
      createdAt: isoFromTimestampMs(frame2.timestampMs),
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
  return entries.map((entry, index2) => ({ entry, index: index2 })).sort((left, right) => {
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
  const filteredFrames = args.eventFrames.filter((frame2) => {
    if (ACTIVITY_HIDDEN_EVENTS.has(frame2.event)) {
      return false;
    }
    if (frame2.sourceKind === "session_history") {
      return false;
    }
    const frameIdentity = frame2.identity?.trim();
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
    if (activePreset.eventTypeFilter?.length && !activePreset.eventTypeFilter.includes(frame2.event)) {
      return false;
    }
    return true;
  });
  const pulseItems = filteredFrames.slice(0, 200).map((frame2, index2) => {
    const frameIdentity = frame2.identity?.trim();
    const agent = frameIdentity ? agentByIdentity.get(frameIdentity) : null;
    const ts = typeof frame2.timestampMs === "number" ? new Date(frame2.timestampMs).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", second: "2-digit" }) : "";
    return {
      id: `event:${frame2.id || index2}`,
      title: agent?.label || frameIdentity || frame2.event || "event",
      line: summarizeFrameData(frame2.data).slice(0, 120) || frame2.event,
      meta: `${frame2.event}${ts ? ` \xB7 ${ts}` : ""}`,
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
  if (typeof record.type === "string" && "frame" in record) {
    const frame2 = timelineFrameToConsoleFrame(record.frame);
    const isUpdateEnvelope = eventName === "frame_updated";
    return {
      id: frame2.id,
      event: isUpdateEnvelope ? "frame_updated" : frame2.event,
      identity: frame2.identity,
      interactionId: frame2.interactionId,
      timestampMs: frame2.timestampMs,
      cursor: frame2.cursor,
      runtimeKey: frame2.runtimeKey,
      sessionId: frame2.sessionId,
      status: frame2.status,
      sourceKind: frame2.sourceKind,
      frameVersion: frame2.frameVersion,
      updatedAtMs: frame2.updatedAtMs,
      turnId: frame2.turnId,
      runId: frame2.runId,
      data: isUpdateEnvelope ? frame2.event === "frame_updated" ? frame2.data : { frame: frame2 } : frame2.data
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
  const source = record.source && typeof record.source === "object" ? record.source : null;
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
      sourceKind: source && typeof source.kind === "string" ? source.kind : updated.sourceKind,
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
    sourceKind: source && typeof source.kind === "string" ? source.kind : void 0,
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
      sourceKind: normalized.sourceKind,
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
async function sendConsoleMultipart(baseUrl, identity, message, attachments, origin, idempotencyKey, handlingMode = "queue") {
  const content = [];
  if (message.trim()) {
    content.push({ type: "text", text: message });
  }
  const form = new FormData();
  attachments.forEach((file, index2) => {
    const uploadId = `upload-${Date.now().toString(36)}-${index2}`;
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
    for (const frame2 of frames2) {
      if (matchesCorrelation(frame2, options.correlation, true)) {
        options.onFrame?.(frame2);
      }
    }
    return !options.correlation ? frames2 : frames2.filter((frame2) => matchesCorrelation(frame2, options.correlation, true));
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
      frameBuffer = flushSseBlocks(frameBuffer, (frame2) => {
        if (matchesCorrelation(frame2, options.correlation, true)) {
          frames.push(frame2);
          options.onFrame?.(frame2);
          if (stopOnTerminal && TERMINAL_SSE_EVENTS.has(frame2.event || "")) {
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
    frameBuffer = flushSseBlocks(frameBuffer, (frame2) => {
      if (matchesCorrelation(frame2, options.correlation, true)) {
        frames.push(frame2);
        options.onFrame?.(frame2);
      }
    });
    flushTrailingSseBlock(frameBuffer, (frame2) => {
      if (matchesCorrelation(frame2, options.correlation, true)) {
        frames.push(frame2);
        options.onFrame?.(frame2);
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
    for (const frame2 of parseSseFrames(block)) {
      onFrame(frame2);
    }
  }
  return buffer;
}
function flushTrailingSseBlock(buffer, onFrame) {
  if (!buffer.trim()) {
    return;
  }
  for (const frame2 of parseSseFrames(`${buffer}

`)) {
    onFrame(frame2);
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
function cursorFromTimelineFrame(frame2) {
  const cursor = frame2.cursor?.trim();
  if (cursor) return cursor;
  if (frame2.event === "snapshot_complete") {
    const id = frame2.id?.trim();
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
            onFrame: (frame2) => {
              const nextCursor = cursorFromTimelineFrame(frame2);
              if (nextCursor) {
                after = nextCursor;
              }
              onFrame(frame2);
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

// src/lib/pane-resize.ts
function findPaneResizeRoot(handle) {
  const workbenchRoot = handle.closest("[data-console-workbench]");
  if (workbenchRoot instanceof HTMLElement) return workbenchRoot;
  const shellRoot = handle.closest(".shell");
  return shellRoot instanceof HTMLElement ? shellRoot : null;
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
var import_react14 = __toESM(require("react"));

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
  const agentByIdentity = /* @__PURE__ */ new Map();
  for (const a2 of agents) {
    const candidates = [a2.identity, a2.member_id, a2.agent_id].filter(Boolean);
    for (const id of candidates) {
      if (!agentByIdentity.has(id)) agentByIdentity.set(id, a2);
    }
  }
  const source = nodes.length > 0 ? nodes : agents.map((a2) => ({
    identity: a2.identity || a2.member_id,
    label: a2.label,
    role: a2.role,
    state: a2.state,
    wired_to: a2.wired_to,
    labels: a2.labels,
    group: a2.group,
    subgroup: a2.subgroup
  }));
  const byId = /* @__PURE__ */ new Map();
  const list = [];
  for (const n of source) {
    const id = (n.identity || n.label || "").trim();
    if (!id || byId.has(id)) continue;
    const registry = agentByIdentity.get(id);
    const labels = {
      ...registry?.labels || {},
      ...n.labels || {}
    };
    const group = (n.group || registry?.group || labels.console_group || labels.group || labels.swarm_mob || n.role || registry?.role || "Agents").trim();
    const agent = {
      id,
      label: (n.label || registry?.label || labels.display_name || id).trim(),
      role: (n.role || registry?.role || labels.role || "agent").trim(),
      state: (n.state || registry?.state || "").toLowerCase(),
      wiredTo: (n.wired_to || []).map((s) => s.trim()).filter(Boolean),
      group,
      subgroup: n.subgroup || registry?.subgroup || labels.shard || void 0,
      labels
    };
    byId.set(id, agent);
    list.push(agent);
  }
  const seen = /* @__PURE__ */ new Set();
  const edges = [];
  for (const a2 of list) {
    for (const t of a2.wiredTo) {
      if (!byId.has(t) || t === a2.id) continue;
      const key = a2.id < t ? `${a2.id}|${t}` : `${t}|${a2.id}`;
      if (seen.has(key)) continue;
      seen.add(key);
      edges.push({ from: a2.id, to: t });
    }
  }
  const degree = {};
  for (const e of edges) {
    degree[e.from] = (degree[e.from] || 0) + 1;
    degree[e.to] = (degree[e.to] || 0) + 1;
  }
  const roles = Array.from(new Set(list.map((a2) => a2.role))).sort((a2, b) => {
    const ra = roleSortKey(a2);
    const rb = roleSortKey(b);
    if (ra !== rb) return ra - rb;
    return a2.localeCompare(b);
  });
  const groups = Array.from(new Set(list.map((a2) => a2.group))).sort((a2, b) => {
    const ca = list.filter((agent) => agent.group === a2).length;
    const cb = list.filter((agent) => agent.group === b).length;
    if (ca !== cb) return cb - ca;
    return a2.localeCompare(b);
  });
  return { agents: list, byId, edges, degree, roles, groups };
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
  const [now2, setNow] = import_react7.default.useState(() => Date.now());
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
    for (const frame2 of ordered) {
      const ts = frame2.timestampMs || 0;
      if (!ts) continue;
      const identity = frame2.identity?.trim();
      if (identity && graph.byId.has(identity)) {
        if ((active[identity] || 0) < ts) active[identity] = ts;
        if (frame2.event === "interaction_started" || frame2.event === "run_started") {
          busy[identity] = true;
        } else if (frame2.event === "interaction_complete" || frame2.event === "interaction_failed" || frame2.event === "run_completed" || frame2.event === "run_failed" || frame2.event === "run_canceled") {
          busy[identity] = false;
        }
      }
      const data = frame2.data;
      const name = data && typeof data.name === "string" ? data.name : "";
      if (name === "peers" && (frame2.event === "tool_execution_completed" || frame2.event === "tool_result_received")) {
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
      if (PEER_TOOL_NAMES2.has(name) && (frame2.event === "tool_call_requested" || frame2.event === "tool_call" || frame2.event === "tool_execution_started") && identity && graph.byId.has(identity)) {
        const args = data && typeof data.args === "object" ? data.args : null;
        const peerId = typeof args?.peer_id === "string" ? args.peer_id : null;
        const recipient = peerId ? peerRegistry.get(peerId) : null;
        if (recipient && graph.byId.has(recipient) && recipient !== identity) {
          pulses.push({
            id: typeof data?.id === "string" ? data.id : `${frame2.id || ts}-${pulses.length}`,
            from: identity,
            to: recipient,
            ts
          });
        }
      }
    }
    const cutoff = now2 - life;
    for (const [k, v] of Object.entries(active)) {
      if (v < cutoff) delete active[k];
    }
    const live = pulses.filter((p) => p.ts >= cutoff);
    return { active, pulses: live, busy };
  }, [frames, graph, life, now2]);
}
function edgeKey(a2, b) {
  return a2 < b ? `${a2}|${b}` : `${b}|${a2}`;
}
function graphStats(graph) {
  const nodeCount = graph.agents.length;
  const edgeCount = graph.edges.length;
  const possibleEdges = nodeCount > 1 ? nodeCount * (nodeCount - 1) / 2 : 0;
  const degrees = graph.agents.map((a2) => graph.degree[a2.id] || 0);
  const minDegree = degrees.length ? Math.min(...degrees) : 0;
  const maxDegree = degrees.length ? Math.max(...degrees) : 0;
  const isolatedCount = degrees.filter((d) => d === 0).length;
  return {
    nodeCount,
    edgeCount,
    possibleEdges,
    density: possibleEdges > 0 ? edgeCount / possibleEdges : 0,
    minDegree,
    maxDegree,
    avgDegree: nodeCount > 0 ? edgeCount * 2 / nodeCount : 0,
    isolatedCount
  };
}
function groupSummaries(graph) {
  const byGroup = /* @__PURE__ */ new Map();
  for (const group of graph.groups) {
    byGroup.set(group, { group, count: 0, internalEdges: 0, externalEdges: 0 });
  }
  for (const agent of graph.agents) {
    const summary = byGroup.get(agent.group);
    if (summary) summary.count++;
  }
  for (const edge of graph.edges) {
    const from = graph.byId.get(edge.from);
    const to = graph.byId.get(edge.to);
    if (!from || !to) continue;
    if (from.group === to.group) {
      const summary = byGroup.get(from.group);
      if (summary) summary.internalEdges++;
    } else {
      const a2 = byGroup.get(from.group);
      const b = byGroup.get(to.group);
      if (a2) a2.externalEdges++;
      if (b) b.externalEdges++;
    }
  }
  return Array.from(byGroup.values()).sort((a2, b) => {
    if (a2.count !== b.count) return b.count - a2.count;
    return a2.group.localeCompare(b.group);
  });
}
function groupMatrix(graph, maxGroups = 8) {
  const allowed = new Set(groupSummaries(graph).slice(0, maxGroups).map((g) => g.group));
  const keyFor = (group) => allowed.has(group) ? group : "Other";
  const counts = /* @__PURE__ */ new Map();
  for (const edge of graph.edges) {
    const from = graph.byId.get(edge.from);
    const to = graph.byId.get(edge.to);
    if (!from || !to) continue;
    const a2 = keyFor(from.group);
    const b = keyFor(to.group);
    const key = a2 <= b ? `${a2}|${b}` : `${b}|${a2}`;
    counts.set(key, (counts.get(key) || 0) + 1);
  }
  return Array.from(counts.entries()).map(([key, edges]) => {
    const [from, to] = key.split("|");
    return { from, to, edges };
  }).sort((a2, b) => {
    if (a2.from !== b.from) return a2.from.localeCompare(b.from);
    return a2.to.localeCompare(b.to);
  });
}
function sampleEdges(edges, limit) {
  if (edges.length <= limit) return edges;
  if (limit <= 0) return [];
  const step = edges.length / limit;
  const sampled = [];
  let cursor = 0;
  while (sampled.length < limit && Math.floor(cursor) < edges.length) {
    sampled.push(edges[Math.floor(cursor)]);
    cursor += step;
  }
  return sampled;
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
  const visualEdges = import_react9.default.useMemo(() => sampleEdges(graph.edges, 1500), [graph.edges]);
  const labelMode = resolveLabelMode(graph.agents.length, labelsMode);
  const [hoverId, setHoverId] = import_react9.default.useState(null);
  const zoom = useZoomPan(width, height);
  const simRef = import_react9.default.useRef(null);
  const [, setTick] = import_react9.default.useState(0);
  const fingerprint = import_react9.default.useMemo(
    () => `${graph.agents.map((a2) => a2.id).join(",")}|${graph.edges.length}|${width}x${height}`,
    [graph, width, height]
  );
  const showLabelsInSim = labelMode === "on";
  const labelWidthCache = import_react9.default.useMemo(() => {
    const m2 = /* @__PURE__ */ new Map();
    for (const a2 of graph.agents) {
      const chars = (a2.label || a2.id).length;
      m2.set(a2.id, Math.min(140, Math.max(40, chars * 6.5)));
    }
    return m2;
  }, [graph.agents]);
  import_react9.default.useEffect(() => {
    const N = graph.agents.length;
    if (N === 0) {
      simRef.current = null;
      return;
    }
    const seeded = graph.agents.map((a2, i) => {
      const t = i / Math.max(1, N) * Math.PI * 2;
      return {
        id: a2.id,
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
      for (const e of visualEdges) {
        const a2 = sim2.byId.get(e.from);
        const b = sim2.byId.get(e.to);
        if (!a2 || !b) continue;
        const dx = b.x - a2.x;
        const dy = b.y - a2.y;
        const d = Math.sqrt(dx * dx + dy * dy) || 0.01;
        const f = 0.025 * (d - scale.idealEdgeLen);
        const ux = dx / d;
        const uy = dy / d;
        a2.vx += ux * f;
        a2.vy += uy * f;
        b.vx -= ux * f;
        b.vy -= uy * f;
      }
      if (showLabelsInSim && sim2.alpha > 0.12) {
        for (const a2 of sim2.nodes) {
          const labelW = labelWidthCache.get(a2.id) ?? 60;
          const halfW = labelW / 2;
          const labelTop = a2.y + scale.nodeMax + 4;
          const labelBot = labelTop + 14;
          for (const b of sim2.nodes) {
            if (a2 === b) continue;
            const overlapX = b.x > a2.x - halfW - scale.nodeMax && b.x < a2.x + halfW + scale.nodeMax;
            const overlapY = b.y > labelTop - scale.nodeMax && b.y < labelBot + scale.nodeMax;
            if (!overlapX || !overlapY) continue;
            const dx = (b.x - a2.x) * 0.04;
            const dy = (b.y - (labelTop + labelBot) / 2) * 0.18;
            b.vx += dx;
            b.vy += dy;
            a2.vx -= dx * 0.5;
            a2.vy -= dy * 0.5;
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
  }, [fingerprint, visualEdges]);
  const sim = simRef.current;
  const hotEdges = import_react9.default.useMemo(() => {
    const set2 = /* @__PURE__ */ new Set();
    for (const p of liveActivity.pulses) set2.add(edgeKey(p.from, p.to));
    return set2;
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
          /* @__PURE__ */ (0, import_jsx_runtime17.jsx)("g", { children: visualEdges.map((e, i) => {
            const a2 = sim.byId.get(e.from);
            const b = sim.byId.get(e.to);
            if (!a2 || !b) return null;
            const hot = hotEdges.has(edgeKey(e.from, e.to));
            return /* @__PURE__ */ (0, import_jsx_runtime17.jsx)(
              "line",
              {
                x1: a2.x,
                y1: a2.y,
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
            const a2 = sim.byId.get(p.from);
            const b = sim.byId.get(p.to);
            if (!a2 || !b) return null;
            const age = (Date.now() - p.ts) / 900;
            if (age > 1) return null;
            const x3 = a2.x + (b.x - a2.x) * age;
            const y3 = a2.y + (b.y - a2.y) * age;
            return /* @__PURE__ */ (0, import_jsx_runtime17.jsx)(
              "circle",
              {
                cx: x3,
                cy: y3,
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
function NodeLabelPill({ x: x3, y: y3, text, sub }) {
  const W2 = 180;
  const H2 = sub ? 32 : 18;
  return /* @__PURE__ */ (0, import_jsx_runtime17.jsx)(
    "foreignObject",
    {
      x: x3 - W2 / 2,
      y: y3,
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
    (a2, b) => (graph.degree[b.id] || 0) - (graph.degree[a2.id] || 0)
  );
  const maxDeg = sorted.length ? graph.degree[sorted[0].id] || 1 : 1;
  const buckets = Array.from({ length: RINGS }, () => []);
  for (const a2 of sorted) {
    const d = graph.degree[a2.id] || 0;
    const t = d / Math.max(1, maxDeg);
    const ringIdx = Math.min(RINGS - 1, Math.floor((1 - Math.pow(t, 0.6)) * RINGS));
    buckets[ringIdx].push(a2);
  }
  const minR = Math.min(width, height) * 0.1;
  const maxR = Math.min(width, height) * 0.44;
  const ringR = (i) => minR + i / Math.max(1, RINGS - 1) * (maxR - minR);
  const pos = {};
  buckets.forEach((list, ri) => {
    list.sort((a2, b) => a2.role.localeCompare(b.role) || a2.id.localeCompare(b.id));
    const r2 = ringR(ri);
    list.forEach((a2, i) => {
      const offset = ri / RINGS * (Math.PI / 6);
      const t = i / Math.max(1, list.length) * Math.PI * 2 - Math.PI / 2 + offset;
      pos[a2.id] = { x: cx + Math.cos(t) * r2, y: cy + Math.sin(t) * r2, ringIdx: ri };
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
  const visualEdges = import_react10.default.useMemo(() => sampleEdges(graph.edges, 1500), [graph.edges]);
  const labelMode = resolveLabelMode2(graph.agents.length, labelsMode);
  const [hoverId, setHoverId] = import_react10.default.useState(null);
  const zoom = useZoomPan(width, height);
  const { pos, ringR, cx, cy, buckets } = import_react10.default.useMemo(
    () => layout(graph, width, height),
    [graph, width, height]
  );
  const hotEdges = import_react10.default.useMemo(() => {
    const set2 = /* @__PURE__ */ new Set();
    for (const p of live.pulses) set2.add(edgeKey(p.from, p.to));
    return set2;
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
          /* @__PURE__ */ (0, import_jsx_runtime18.jsx)("g", { children: visualEdges.map((e, i) => {
            const a2 = pos[e.from];
            const b = pos[e.to];
            if (!a2 || !b) return null;
            const hot = hotEdges.has(edgeKey(e.from, e.to));
            return /* @__PURE__ */ (0, import_jsx_runtime18.jsx)(
              "line",
              {
                x1: a2.x,
                y1: a2.y,
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
            const a2 = pos[p.from];
            const b = pos[p.to];
            if (!a2 || !b) return null;
            const age = (Date.now() - p.ts) / 1100;
            if (age > 1) return null;
            const x3 = a2.x + (b.x - a2.x) * age;
            const y3 = a2.y + (b.y - a2.y) * age;
            return /* @__PURE__ */ (0, import_jsx_runtime18.jsx)(
              "circle",
              {
                cx: x3,
                cy: y3,
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
function BullseyeLabelPill({ x: x3, y: y3, text, sub }) {
  const W2 = 180;
  const H2 = sub ? 32 : 18;
  return /* @__PURE__ */ (0, import_jsx_runtime18.jsx)(
    "foreignObject",
    {
      x: x3 - W2 / 2,
      y: y3,
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
    for (const a2 of graph.agents) (g[_a = a2.role] || (g[_a] = [])).push(a2);
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
  const rootHot = graph.agents.some((a2) => live.active[a2.id]);
  const rootBusy = graph.agents.some((a2) => live.busy[a2.id]);
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
      const sectionHot = list.some((a2) => live.active[a2.id]);
      const sectionBusy = list.some((a2) => live.busy[a2.id]);
      const sectionBusyCount = list.filter((a2) => live.busy[a2.id]).length;
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

// src/panels/topology/LargeGraphSummary.tsx
var import_react13 = __toESM(require("react"));

// src/panels/topology/DenseGraphMap.tsx
var import_react12 = __toESM(require("react"));

// node_modules/d3-quadtree/src/add.js
function add_default(d) {
  const x3 = +this._x.call(null, d), y3 = +this._y.call(null, d);
  return add(this.cover(x3, y3), x3, y3, d);
}
function add(tree, x3, y3, d) {
  if (isNaN(x3) || isNaN(y3)) return tree;
  var parent, node = tree._root, leaf = { data: d }, x0 = tree._x0, y0 = tree._y0, x1 = tree._x1, y1 = tree._y1, xm, ym, xp, yp, right, bottom, i, j;
  if (!node) return tree._root = leaf, tree;
  while (node.length) {
    if (right = x3 >= (xm = (x0 + x1) / 2)) x0 = xm;
    else x1 = xm;
    if (bottom = y3 >= (ym = (y0 + y1) / 2)) y0 = ym;
    else y1 = ym;
    if (parent = node, !(node = node[i = bottom << 1 | right])) return parent[i] = leaf, tree;
  }
  xp = +tree._x.call(null, node.data);
  yp = +tree._y.call(null, node.data);
  if (x3 === xp && y3 === yp) return leaf.next = node, parent ? parent[i] = leaf : tree._root = leaf, tree;
  do {
    parent = parent ? parent[i] = new Array(4) : tree._root = new Array(4);
    if (right = x3 >= (xm = (x0 + x1) / 2)) x0 = xm;
    else x1 = xm;
    if (bottom = y3 >= (ym = (y0 + y1) / 2)) y0 = ym;
    else y1 = ym;
  } while ((i = bottom << 1 | right) === (j = (yp >= ym) << 1 | xp >= xm));
  return parent[j] = node, parent[i] = leaf, tree;
}
function addAll(data) {
  var d, i, n = data.length, x3, y3, xz = new Array(n), yz = new Array(n), x0 = Infinity, y0 = Infinity, x1 = -Infinity, y1 = -Infinity;
  for (i = 0; i < n; ++i) {
    if (isNaN(x3 = +this._x.call(null, d = data[i])) || isNaN(y3 = +this._y.call(null, d))) continue;
    xz[i] = x3;
    yz[i] = y3;
    if (x3 < x0) x0 = x3;
    if (x3 > x1) x1 = x3;
    if (y3 < y0) y0 = y3;
    if (y3 > y1) y1 = y3;
  }
  if (x0 > x1 || y0 > y1) return this;
  this.cover(x0, y0).cover(x1, y1);
  for (i = 0; i < n; ++i) {
    add(this, xz[i], yz[i], data[i]);
  }
  return this;
}

// node_modules/d3-quadtree/src/cover.js
function cover_default(x3, y3) {
  if (isNaN(x3 = +x3) || isNaN(y3 = +y3)) return this;
  var x0 = this._x0, y0 = this._y0, x1 = this._x1, y1 = this._y1;
  if (isNaN(x0)) {
    x1 = (x0 = Math.floor(x3)) + 1;
    y1 = (y0 = Math.floor(y3)) + 1;
  } else {
    var z = x1 - x0 || 1, node = this._root, parent, i;
    while (x0 > x3 || x3 >= x1 || y0 > y3 || y3 >= y1) {
      i = (y3 < y0) << 1 | x3 < x0;
      parent = new Array(4), parent[i] = node, node = parent, z *= 2;
      switch (i) {
        case 0:
          x1 = x0 + z, y1 = y0 + z;
          break;
        case 1:
          x0 = x1 - z, y1 = y0 + z;
          break;
        case 2:
          x1 = x0 + z, y0 = y1 - z;
          break;
        case 3:
          x0 = x1 - z, y0 = y1 - z;
          break;
      }
    }
    if (this._root && this._root.length) this._root = node;
  }
  this._x0 = x0;
  this._y0 = y0;
  this._x1 = x1;
  this._y1 = y1;
  return this;
}

// node_modules/d3-quadtree/src/data.js
function data_default() {
  var data = [];
  this.visit(function(node) {
    if (!node.length) do
      data.push(node.data);
    while (node = node.next);
  });
  return data;
}

// node_modules/d3-quadtree/src/extent.js
function extent_default(_) {
  return arguments.length ? this.cover(+_[0][0], +_[0][1]).cover(+_[1][0], +_[1][1]) : isNaN(this._x0) ? void 0 : [[this._x0, this._y0], [this._x1, this._y1]];
}

// node_modules/d3-quadtree/src/quad.js
function quad_default(node, x0, y0, x1, y1) {
  this.node = node;
  this.x0 = x0;
  this.y0 = y0;
  this.x1 = x1;
  this.y1 = y1;
}

// node_modules/d3-quadtree/src/find.js
function find_default(x3, y3, radius) {
  var data, x0 = this._x0, y0 = this._y0, x1, y1, x22, y22, x32 = this._x1, y32 = this._y1, quads = [], node = this._root, q, i;
  if (node) quads.push(new quad_default(node, x0, y0, x32, y32));
  if (radius == null) radius = Infinity;
  else {
    x0 = x3 - radius, y0 = y3 - radius;
    x32 = x3 + radius, y32 = y3 + radius;
    radius *= radius;
  }
  while (q = quads.pop()) {
    if (!(node = q.node) || (x1 = q.x0) > x32 || (y1 = q.y0) > y32 || (x22 = q.x1) < x0 || (y22 = q.y1) < y0) continue;
    if (node.length) {
      var xm = (x1 + x22) / 2, ym = (y1 + y22) / 2;
      quads.push(
        new quad_default(node[3], xm, ym, x22, y22),
        new quad_default(node[2], x1, ym, xm, y22),
        new quad_default(node[1], xm, y1, x22, ym),
        new quad_default(node[0], x1, y1, xm, ym)
      );
      if (i = (y3 >= ym) << 1 | x3 >= xm) {
        q = quads[quads.length - 1];
        quads[quads.length - 1] = quads[quads.length - 1 - i];
        quads[quads.length - 1 - i] = q;
      }
    } else {
      var dx = x3 - +this._x.call(null, node.data), dy = y3 - +this._y.call(null, node.data), d2 = dx * dx + dy * dy;
      if (d2 < radius) {
        var d = Math.sqrt(radius = d2);
        x0 = x3 - d, y0 = y3 - d;
        x32 = x3 + d, y32 = y3 + d;
        data = node.data;
      }
    }
  }
  return data;
}

// node_modules/d3-quadtree/src/remove.js
function remove_default(d) {
  if (isNaN(x3 = +this._x.call(null, d)) || isNaN(y3 = +this._y.call(null, d))) return this;
  var parent, node = this._root, retainer, previous, next, x0 = this._x0, y0 = this._y0, x1 = this._x1, y1 = this._y1, x3, y3, xm, ym, right, bottom, i, j;
  if (!node) return this;
  if (node.length) while (true) {
    if (right = x3 >= (xm = (x0 + x1) / 2)) x0 = xm;
    else x1 = xm;
    if (bottom = y3 >= (ym = (y0 + y1) / 2)) y0 = ym;
    else y1 = ym;
    if (!(parent = node, node = node[i = bottom << 1 | right])) return this;
    if (!node.length) break;
    if (parent[i + 1 & 3] || parent[i + 2 & 3] || parent[i + 3 & 3]) retainer = parent, j = i;
  }
  while (node.data !== d) if (!(previous = node, node = node.next)) return this;
  if (next = node.next) delete node.next;
  if (previous) return next ? previous.next = next : delete previous.next, this;
  if (!parent) return this._root = next, this;
  next ? parent[i] = next : delete parent[i];
  if ((node = parent[0] || parent[1] || parent[2] || parent[3]) && node === (parent[3] || parent[2] || parent[1] || parent[0]) && !node.length) {
    if (retainer) retainer[j] = node;
    else this._root = node;
  }
  return this;
}
function removeAll(data) {
  for (var i = 0, n = data.length; i < n; ++i) this.remove(data[i]);
  return this;
}

// node_modules/d3-quadtree/src/root.js
function root_default() {
  return this._root;
}

// node_modules/d3-quadtree/src/size.js
function size_default() {
  var size = 0;
  this.visit(function(node) {
    if (!node.length) do
      ++size;
    while (node = node.next);
  });
  return size;
}

// node_modules/d3-quadtree/src/visit.js
function visit_default(callback) {
  var quads = [], q, node = this._root, child, x0, y0, x1, y1;
  if (node) quads.push(new quad_default(node, this._x0, this._y0, this._x1, this._y1));
  while (q = quads.pop()) {
    if (!callback(node = q.node, x0 = q.x0, y0 = q.y0, x1 = q.x1, y1 = q.y1) && node.length) {
      var xm = (x0 + x1) / 2, ym = (y0 + y1) / 2;
      if (child = node[3]) quads.push(new quad_default(child, xm, ym, x1, y1));
      if (child = node[2]) quads.push(new quad_default(child, x0, ym, xm, y1));
      if (child = node[1]) quads.push(new quad_default(child, xm, y0, x1, ym));
      if (child = node[0]) quads.push(new quad_default(child, x0, y0, xm, ym));
    }
  }
  return this;
}

// node_modules/d3-quadtree/src/visitAfter.js
function visitAfter_default(callback) {
  var quads = [], next = [], q;
  if (this._root) quads.push(new quad_default(this._root, this._x0, this._y0, this._x1, this._y1));
  while (q = quads.pop()) {
    var node = q.node;
    if (node.length) {
      var child, x0 = q.x0, y0 = q.y0, x1 = q.x1, y1 = q.y1, xm = (x0 + x1) / 2, ym = (y0 + y1) / 2;
      if (child = node[0]) quads.push(new quad_default(child, x0, y0, xm, ym));
      if (child = node[1]) quads.push(new quad_default(child, xm, y0, x1, ym));
      if (child = node[2]) quads.push(new quad_default(child, x0, ym, xm, y1));
      if (child = node[3]) quads.push(new quad_default(child, xm, ym, x1, y1));
    }
    next.push(q);
  }
  while (q = next.pop()) {
    callback(q.node, q.x0, q.y0, q.x1, q.y1);
  }
  return this;
}

// node_modules/d3-quadtree/src/x.js
function defaultX(d) {
  return d[0];
}
function x_default(_) {
  return arguments.length ? (this._x = _, this) : this._x;
}

// node_modules/d3-quadtree/src/y.js
function defaultY(d) {
  return d[1];
}
function y_default(_) {
  return arguments.length ? (this._y = _, this) : this._y;
}

// node_modules/d3-quadtree/src/quadtree.js
function quadtree(nodes, x3, y3) {
  var tree = new Quadtree(x3 == null ? defaultX : x3, y3 == null ? defaultY : y3, NaN, NaN, NaN, NaN);
  return nodes == null ? tree : tree.addAll(nodes);
}
function Quadtree(x3, y3, x0, y0, x1, y1) {
  this._x = x3;
  this._y = y3;
  this._x0 = x0;
  this._y0 = y0;
  this._x1 = x1;
  this._y1 = y1;
  this._root = void 0;
}
function leaf_copy(leaf) {
  var copy = { data: leaf.data }, next = copy;
  while (leaf = leaf.next) next = next.next = { data: leaf.data };
  return copy;
}
var treeProto = quadtree.prototype = Quadtree.prototype;
treeProto.copy = function() {
  var copy = new Quadtree(this._x, this._y, this._x0, this._y0, this._x1, this._y1), node = this._root, nodes, child;
  if (!node) return copy;
  if (!node.length) return copy._root = leaf_copy(node), copy;
  nodes = [{ source: node, target: copy._root = new Array(4) }];
  while (node = nodes.pop()) {
    for (var i = 0; i < 4; ++i) {
      if (child = node.source[i]) {
        if (child.length) nodes.push({ source: child, target: node.target[i] = new Array(4) });
        else node.target[i] = leaf_copy(child);
      }
    }
  }
  return copy;
};
treeProto.add = add_default;
treeProto.addAll = addAll;
treeProto.cover = cover_default;
treeProto.data = data_default;
treeProto.extent = extent_default;
treeProto.find = find_default;
treeProto.remove = remove_default;
treeProto.removeAll = removeAll;
treeProto.root = root_default;
treeProto.size = size_default;
treeProto.visit = visit_default;
treeProto.visitAfter = visitAfter_default;
treeProto.x = x_default;
treeProto.y = y_default;

// node_modules/d3-force/src/constant.js
function constant_default(x3) {
  return function() {
    return x3;
  };
}

// node_modules/d3-force/src/jiggle.js
function jiggle_default(random) {
  return (random() - 0.5) * 1e-6;
}

// node_modules/d3-force/src/collide.js
function x(d) {
  return d.x + d.vx;
}
function y(d) {
  return d.y + d.vy;
}
function collide_default(radius) {
  var nodes, radii, random, strength = 1, iterations = 1;
  if (typeof radius !== "function") radius = constant_default(radius == null ? 1 : +radius);
  function force() {
    var i, n = nodes.length, tree, node, xi, yi, ri, ri2;
    for (var k = 0; k < iterations; ++k) {
      tree = quadtree(nodes, x, y).visitAfter(prepare);
      for (i = 0; i < n; ++i) {
        node = nodes[i];
        ri = radii[node.index], ri2 = ri * ri;
        xi = node.x + node.vx;
        yi = node.y + node.vy;
        tree.visit(apply);
      }
    }
    function apply(quad, x0, y0, x1, y1) {
      var data = quad.data, rj = quad.r, r2 = ri + rj;
      if (data) {
        if (data.index > node.index) {
          var x3 = xi - data.x - data.vx, y3 = yi - data.y - data.vy, l = x3 * x3 + y3 * y3;
          if (l < r2 * r2) {
            if (x3 === 0) x3 = jiggle_default(random), l += x3 * x3;
            if (y3 === 0) y3 = jiggle_default(random), l += y3 * y3;
            l = (r2 - (l = Math.sqrt(l))) / l * strength;
            node.vx += (x3 *= l) * (r2 = (rj *= rj) / (ri2 + rj));
            node.vy += (y3 *= l) * r2;
            data.vx -= x3 * (r2 = 1 - r2);
            data.vy -= y3 * r2;
          }
        }
        return;
      }
      return x0 > xi + r2 || x1 < xi - r2 || y0 > yi + r2 || y1 < yi - r2;
    }
  }
  function prepare(quad) {
    if (quad.data) return quad.r = radii[quad.data.index];
    for (var i = quad.r = 0; i < 4; ++i) {
      if (quad[i] && quad[i].r > quad.r) {
        quad.r = quad[i].r;
      }
    }
  }
  function initialize() {
    if (!nodes) return;
    var i, n = nodes.length, node;
    radii = new Array(n);
    for (i = 0; i < n; ++i) node = nodes[i], radii[node.index] = +radius(node, i, nodes);
  }
  force.initialize = function(_nodes, _random) {
    nodes = _nodes;
    random = _random;
    initialize();
  };
  force.iterations = function(_) {
    return arguments.length ? (iterations = +_, force) : iterations;
  };
  force.strength = function(_) {
    return arguments.length ? (strength = +_, force) : strength;
  };
  force.radius = function(_) {
    return arguments.length ? (radius = typeof _ === "function" ? _ : constant_default(+_), initialize(), force) : radius;
  };
  return force;
}

// node_modules/d3-force/src/link.js
function index(d) {
  return d.index;
}
function find(nodeById, nodeId) {
  var node = nodeById.get(nodeId);
  if (!node) throw new Error("node not found: " + nodeId);
  return node;
}
function link_default(links) {
  var id = index, strength = defaultStrength, strengths, distance = constant_default(30), distances, nodes, count, bias, random, iterations = 1;
  if (links == null) links = [];
  function defaultStrength(link) {
    return 1 / Math.min(count[link.source.index], count[link.target.index]);
  }
  function force(alpha) {
    for (var k = 0, n = links.length; k < iterations; ++k) {
      for (var i = 0, link, source, target, x3, y3, l, b; i < n; ++i) {
        link = links[i], source = link.source, target = link.target;
        x3 = target.x + target.vx - source.x - source.vx || jiggle_default(random);
        y3 = target.y + target.vy - source.y - source.vy || jiggle_default(random);
        l = Math.sqrt(x3 * x3 + y3 * y3);
        l = (l - distances[i]) / l * alpha * strengths[i];
        x3 *= l, y3 *= l;
        target.vx -= x3 * (b = bias[i]);
        target.vy -= y3 * b;
        source.vx += x3 * (b = 1 - b);
        source.vy += y3 * b;
      }
    }
  }
  function initialize() {
    if (!nodes) return;
    var i, n = nodes.length, m2 = links.length, nodeById = new Map(nodes.map((d, i2) => [id(d, i2, nodes), d])), link;
    for (i = 0, count = new Array(n); i < m2; ++i) {
      link = links[i], link.index = i;
      if (typeof link.source !== "object") link.source = find(nodeById, link.source);
      if (typeof link.target !== "object") link.target = find(nodeById, link.target);
      count[link.source.index] = (count[link.source.index] || 0) + 1;
      count[link.target.index] = (count[link.target.index] || 0) + 1;
    }
    for (i = 0, bias = new Array(m2); i < m2; ++i) {
      link = links[i], bias[i] = count[link.source.index] / (count[link.source.index] + count[link.target.index]);
    }
    strengths = new Array(m2), initializeStrength();
    distances = new Array(m2), initializeDistance();
  }
  function initializeStrength() {
    if (!nodes) return;
    for (var i = 0, n = links.length; i < n; ++i) {
      strengths[i] = +strength(links[i], i, links);
    }
  }
  function initializeDistance() {
    if (!nodes) return;
    for (var i = 0, n = links.length; i < n; ++i) {
      distances[i] = +distance(links[i], i, links);
    }
  }
  force.initialize = function(_nodes, _random) {
    nodes = _nodes;
    random = _random;
    initialize();
  };
  force.links = function(_) {
    return arguments.length ? (links = _, initialize(), force) : links;
  };
  force.id = function(_) {
    return arguments.length ? (id = _, force) : id;
  };
  force.iterations = function(_) {
    return arguments.length ? (iterations = +_, force) : iterations;
  };
  force.strength = function(_) {
    return arguments.length ? (strength = typeof _ === "function" ? _ : constant_default(+_), initializeStrength(), force) : strength;
  };
  force.distance = function(_) {
    return arguments.length ? (distance = typeof _ === "function" ? _ : constant_default(+_), initializeDistance(), force) : distance;
  };
  return force;
}

// node_modules/d3-dispatch/src/dispatch.js
var noop = { value: () => {
} };
function dispatch() {
  for (var i = 0, n = arguments.length, _ = {}, t; i < n; ++i) {
    if (!(t = arguments[i] + "") || t in _ || /[\s.]/.test(t)) throw new Error("illegal type: " + t);
    _[t] = [];
  }
  return new Dispatch(_);
}
function Dispatch(_) {
  this._ = _;
}
function parseTypenames(typenames, types) {
  return typenames.trim().split(/^|\s+/).map(function(t) {
    var name = "", i = t.indexOf(".");
    if (i >= 0) name = t.slice(i + 1), t = t.slice(0, i);
    if (t && !types.hasOwnProperty(t)) throw new Error("unknown type: " + t);
    return { type: t, name };
  });
}
Dispatch.prototype = dispatch.prototype = {
  constructor: Dispatch,
  on: function(typename, callback) {
    var _ = this._, T = parseTypenames(typename + "", _), t, i = -1, n = T.length;
    if (arguments.length < 2) {
      while (++i < n) if ((t = (typename = T[i]).type) && (t = get(_[t], typename.name))) return t;
      return;
    }
    if (callback != null && typeof callback !== "function") throw new Error("invalid callback: " + callback);
    while (++i < n) {
      if (t = (typename = T[i]).type) _[t] = set(_[t], typename.name, callback);
      else if (callback == null) for (t in _) _[t] = set(_[t], typename.name, null);
    }
    return this;
  },
  copy: function() {
    var copy = {}, _ = this._;
    for (var t in _) copy[t] = _[t].slice();
    return new Dispatch(copy);
  },
  call: function(type, that) {
    if ((n = arguments.length - 2) > 0) for (var args = new Array(n), i = 0, n, t; i < n; ++i) args[i] = arguments[i + 2];
    if (!this._.hasOwnProperty(type)) throw new Error("unknown type: " + type);
    for (t = this._[type], i = 0, n = t.length; i < n; ++i) t[i].value.apply(that, args);
  },
  apply: function(type, that, args) {
    if (!this._.hasOwnProperty(type)) throw new Error("unknown type: " + type);
    for (var t = this._[type], i = 0, n = t.length; i < n; ++i) t[i].value.apply(that, args);
  }
};
function get(type, name) {
  for (var i = 0, n = type.length, c2; i < n; ++i) {
    if ((c2 = type[i]).name === name) {
      return c2.value;
    }
  }
}
function set(type, name, callback) {
  for (var i = 0, n = type.length; i < n; ++i) {
    if (type[i].name === name) {
      type[i] = noop, type = type.slice(0, i).concat(type.slice(i + 1));
      break;
    }
  }
  if (callback != null) type.push({ name, value: callback });
  return type;
}
var dispatch_default = dispatch;

// node_modules/d3-timer/src/timer.js
var frame = 0;
var timeout = 0;
var interval = 0;
var pokeDelay = 1e3;
var taskHead;
var taskTail;
var clockLast = 0;
var clockNow = 0;
var clockSkew = 0;
var clock = typeof performance === "object" && performance.now ? performance : Date;
var setFrame = typeof window === "object" && window.requestAnimationFrame ? window.requestAnimationFrame.bind(window) : function(f) {
  setTimeout(f, 17);
};
function now() {
  return clockNow || (setFrame(clearNow), clockNow = clock.now() + clockSkew);
}
function clearNow() {
  clockNow = 0;
}
function Timer() {
  this._call = this._time = this._next = null;
}
Timer.prototype = timer.prototype = {
  constructor: Timer,
  restart: function(callback, delay, time) {
    if (typeof callback !== "function") throw new TypeError("callback is not a function");
    time = (time == null ? now() : +time) + (delay == null ? 0 : +delay);
    if (!this._next && taskTail !== this) {
      if (taskTail) taskTail._next = this;
      else taskHead = this;
      taskTail = this;
    }
    this._call = callback;
    this._time = time;
    sleep2();
  },
  stop: function() {
    if (this._call) {
      this._call = null;
      this._time = Infinity;
      sleep2();
    }
  }
};
function timer(callback, delay, time) {
  var t = new Timer();
  t.restart(callback, delay, time);
  return t;
}
function timerFlush() {
  now();
  ++frame;
  var t = taskHead, e;
  while (t) {
    if ((e = clockNow - t._time) >= 0) t._call.call(void 0, e);
    t = t._next;
  }
  --frame;
}
function wake() {
  clockNow = (clockLast = clock.now()) + clockSkew;
  frame = timeout = 0;
  try {
    timerFlush();
  } finally {
    frame = 0;
    nap();
    clockNow = 0;
  }
}
function poke() {
  var now2 = clock.now(), delay = now2 - clockLast;
  if (delay > pokeDelay) clockSkew -= delay, clockLast = now2;
}
function nap() {
  var t0, t1 = taskHead, t2, time = Infinity;
  while (t1) {
    if (t1._call) {
      if (time > t1._time) time = t1._time;
      t0 = t1, t1 = t1._next;
    } else {
      t2 = t1._next, t1._next = null;
      t1 = t0 ? t0._next = t2 : taskHead = t2;
    }
  }
  taskTail = t0;
  sleep2(time);
}
function sleep2(time) {
  if (frame) return;
  if (timeout) timeout = clearTimeout(timeout);
  var delay = time - clockNow;
  if (delay > 24) {
    if (time < Infinity) timeout = setTimeout(wake, time - clock.now() - clockSkew);
    if (interval) interval = clearInterval(interval);
  } else {
    if (!interval) clockLast = clock.now(), interval = setInterval(poke, pokeDelay);
    frame = 1, setFrame(wake);
  }
}

// node_modules/d3-force/src/lcg.js
var a = 1664525;
var c = 1013904223;
var m = 4294967296;
function lcg_default() {
  let s = 1;
  return () => (s = (a * s + c) % m) / m;
}

// node_modules/d3-force/src/simulation.js
function x2(d) {
  return d.x;
}
function y2(d) {
  return d.y;
}
var initialRadius = 10;
var initialAngle = Math.PI * (3 - Math.sqrt(5));
function simulation_default(nodes) {
  var simulation, alpha = 1, alphaMin = 1e-3, alphaDecay = 1 - Math.pow(alphaMin, 1 / 300), alphaTarget = 0, velocityDecay = 0.6, forces = /* @__PURE__ */ new Map(), stepper = timer(step), event = dispatch_default("tick", "end"), random = lcg_default();
  if (nodes == null) nodes = [];
  function step() {
    tick();
    event.call("tick", simulation);
    if (alpha < alphaMin) {
      stepper.stop();
      event.call("end", simulation);
    }
  }
  function tick(iterations) {
    var i, n = nodes.length, node;
    if (iterations === void 0) iterations = 1;
    for (var k = 0; k < iterations; ++k) {
      alpha += (alphaTarget - alpha) * alphaDecay;
      forces.forEach(function(force) {
        force(alpha);
      });
      for (i = 0; i < n; ++i) {
        node = nodes[i];
        if (node.fx == null) node.x += node.vx *= velocityDecay;
        else node.x = node.fx, node.vx = 0;
        if (node.fy == null) node.y += node.vy *= velocityDecay;
        else node.y = node.fy, node.vy = 0;
      }
    }
    return simulation;
  }
  function initializeNodes() {
    for (var i = 0, n = nodes.length, node; i < n; ++i) {
      node = nodes[i], node.index = i;
      if (node.fx != null) node.x = node.fx;
      if (node.fy != null) node.y = node.fy;
      if (isNaN(node.x) || isNaN(node.y)) {
        var radius = initialRadius * Math.sqrt(0.5 + i), angle = i * initialAngle;
        node.x = radius * Math.cos(angle);
        node.y = radius * Math.sin(angle);
      }
      if (isNaN(node.vx) || isNaN(node.vy)) {
        node.vx = node.vy = 0;
      }
    }
  }
  function initializeForce(force) {
    if (force.initialize) force.initialize(nodes, random);
    return force;
  }
  initializeNodes();
  return simulation = {
    tick,
    restart: function() {
      return stepper.restart(step), simulation;
    },
    stop: function() {
      return stepper.stop(), simulation;
    },
    nodes: function(_) {
      return arguments.length ? (nodes = _, initializeNodes(), forces.forEach(initializeForce), simulation) : nodes;
    },
    alpha: function(_) {
      return arguments.length ? (alpha = +_, simulation) : alpha;
    },
    alphaMin: function(_) {
      return arguments.length ? (alphaMin = +_, simulation) : alphaMin;
    },
    alphaDecay: function(_) {
      return arguments.length ? (alphaDecay = +_, simulation) : +alphaDecay;
    },
    alphaTarget: function(_) {
      return arguments.length ? (alphaTarget = +_, simulation) : alphaTarget;
    },
    velocityDecay: function(_) {
      return arguments.length ? (velocityDecay = 1 - _, simulation) : 1 - velocityDecay;
    },
    randomSource: function(_) {
      return arguments.length ? (random = _, forces.forEach(initializeForce), simulation) : random;
    },
    force: function(name, _) {
      return arguments.length > 1 ? (_ == null ? forces.delete(name) : forces.set(name, initializeForce(_)), simulation) : forces.get(name);
    },
    find: function(x3, y3, radius) {
      var i = 0, n = nodes.length, dx, dy, d2, node, closest;
      if (radius == null) radius = Infinity;
      else radius *= radius;
      for (i = 0; i < n; ++i) {
        node = nodes[i];
        dx = x3 - node.x;
        dy = y3 - node.y;
        d2 = dx * dx + dy * dy;
        if (d2 < radius) closest = node, radius = d2;
      }
      return closest;
    },
    on: function(name, _) {
      return arguments.length > 1 ? (event.on(name, _), simulation) : event.on(name);
    }
  };
}

// node_modules/d3-force/src/manyBody.js
function manyBody_default() {
  var nodes, node, random, alpha, strength = constant_default(-30), strengths, distanceMin2 = 1, distanceMax2 = Infinity, theta2 = 0.81;
  function force(_) {
    var i, n = nodes.length, tree = quadtree(nodes, x2, y2).visitAfter(accumulate);
    for (alpha = _, i = 0; i < n; ++i) node = nodes[i], tree.visit(apply);
  }
  function initialize() {
    if (!nodes) return;
    var i, n = nodes.length, node2;
    strengths = new Array(n);
    for (i = 0; i < n; ++i) node2 = nodes[i], strengths[node2.index] = +strength(node2, i, nodes);
  }
  function accumulate(quad) {
    var strength2 = 0, q, c2, weight = 0, x3, y3, i;
    if (quad.length) {
      for (x3 = y3 = i = 0; i < 4; ++i) {
        if ((q = quad[i]) && (c2 = Math.abs(q.value))) {
          strength2 += q.value, weight += c2, x3 += c2 * q.x, y3 += c2 * q.y;
        }
      }
      quad.x = x3 / weight;
      quad.y = y3 / weight;
    } else {
      q = quad;
      q.x = q.data.x;
      q.y = q.data.y;
      do
        strength2 += strengths[q.data.index];
      while (q = q.next);
    }
    quad.value = strength2;
  }
  function apply(quad, x1, _, x22) {
    if (!quad.value) return true;
    var x3 = quad.x - node.x, y3 = quad.y - node.y, w = x22 - x1, l = x3 * x3 + y3 * y3;
    if (w * w / theta2 < l) {
      if (l < distanceMax2) {
        if (x3 === 0) x3 = jiggle_default(random), l += x3 * x3;
        if (y3 === 0) y3 = jiggle_default(random), l += y3 * y3;
        if (l < distanceMin2) l = Math.sqrt(distanceMin2 * l);
        node.vx += x3 * quad.value * alpha / l;
        node.vy += y3 * quad.value * alpha / l;
      }
      return true;
    } else if (quad.length || l >= distanceMax2) return;
    if (quad.data !== node || quad.next) {
      if (x3 === 0) x3 = jiggle_default(random), l += x3 * x3;
      if (y3 === 0) y3 = jiggle_default(random), l += y3 * y3;
      if (l < distanceMin2) l = Math.sqrt(distanceMin2 * l);
    }
    do
      if (quad.data !== node) {
        w = strengths[quad.data.index] * alpha / l;
        node.vx += x3 * w;
        node.vy += y3 * w;
      }
    while (quad = quad.next);
  }
  force.initialize = function(_nodes, _random) {
    nodes = _nodes;
    random = _random;
    initialize();
  };
  force.strength = function(_) {
    return arguments.length ? (strength = typeof _ === "function" ? _ : constant_default(+_), initialize(), force) : strength;
  };
  force.distanceMin = function(_) {
    return arguments.length ? (distanceMin2 = _ * _, force) : Math.sqrt(distanceMin2);
  };
  force.distanceMax = function(_) {
    return arguments.length ? (distanceMax2 = _ * _, force) : Math.sqrt(distanceMax2);
  };
  force.theta = function(_) {
    return arguments.length ? (theta2 = _ * _, force) : Math.sqrt(theta2);
  };
  return force;
}

// node_modules/d3-force/src/x.js
function x_default2(x3) {
  var strength = constant_default(0.1), nodes, strengths, xz;
  if (typeof x3 !== "function") x3 = constant_default(x3 == null ? 0 : +x3);
  function force(alpha) {
    for (var i = 0, n = nodes.length, node; i < n; ++i) {
      node = nodes[i], node.vx += (xz[i] - node.x) * strengths[i] * alpha;
    }
  }
  function initialize() {
    if (!nodes) return;
    var i, n = nodes.length;
    strengths = new Array(n);
    xz = new Array(n);
    for (i = 0; i < n; ++i) {
      strengths[i] = isNaN(xz[i] = +x3(nodes[i], i, nodes)) ? 0 : +strength(nodes[i], i, nodes);
    }
  }
  force.initialize = function(_) {
    nodes = _;
    initialize();
  };
  force.strength = function(_) {
    return arguments.length ? (strength = typeof _ === "function" ? _ : constant_default(+_), initialize(), force) : strength;
  };
  force.x = function(_) {
    return arguments.length ? (x3 = typeof _ === "function" ? _ : constant_default(+_), initialize(), force) : x3;
  };
  return force;
}

// node_modules/d3-force/src/y.js
function y_default2(y3) {
  var strength = constant_default(0.1), nodes, strengths, yz;
  if (typeof y3 !== "function") y3 = constant_default(y3 == null ? 0 : +y3);
  function force(alpha) {
    for (var i = 0, n = nodes.length, node; i < n; ++i) {
      node = nodes[i], node.vy += (yz[i] - node.y) * strengths[i] * alpha;
    }
  }
  function initialize() {
    if (!nodes) return;
    var i, n = nodes.length;
    strengths = new Array(n);
    yz = new Array(n);
    for (i = 0; i < n; ++i) {
      strengths[i] = isNaN(yz[i] = +y3(nodes[i], i, nodes)) ? 0 : +strength(nodes[i], i, nodes);
    }
  }
  force.initialize = function(_) {
    nodes = _;
    initialize();
  };
  force.strength = function(_) {
    return arguments.length ? (strength = typeof _ === "function" ? _ : constant_default(+_), initialize(), force) : strength;
  };
  force.y = function(_) {
    return arguments.length ? (y3 = typeof _ === "function" ? _ : constant_default(+_), initialize(), force) : y3;
  };
  return force;
}

// src/panels/topology/DenseGraphMap.tsx
var import_jsx_runtime20 = require("react/jsx-runtime");
var LAYOUT_EDGE_LIMIT = 3e3;
var LABEL_LIMIT = 26;
function hash(value) {
  let h = 2166136261;
  for (let i = 0; i < value.length; i++) {
    h ^= value.charCodeAt(i);
    h = Math.imul(h, 16777619);
  }
  return h >>> 0;
}
function cssVar(el, name, fallback) {
  const value = getComputedStyle(el).getPropertyValue(name).trim();
  return value || fallback;
}
function groupPalette(index2) {
  const colours = [
    "hsl(188 74% 66%)",
    "hsl(156 68% 62%)",
    "hsl(260 54% 72%)",
    "hsl(35 70% 69%)",
    "hsl(214 76% 66%)",
    "hsl(335 58% 70%)"
  ];
  return colours[index2 % colours.length];
}
function withAlpha(colour, alpha) {
  if (colour.startsWith("hsl(") && !colour.includes("/")) {
    return colour.replace(")", ` / ${alpha})`);
  }
  return colour;
}
function buildLayout(graph, width, height) {
  const groupIndex = /* @__PURE__ */ new Map();
  graph.groups.forEach((group, index2) => groupIndex.set(group, index2));
  const cx = width / 2;
  const cy = height / 2;
  const rx = Math.max(160, width * 0.32);
  const ry = Math.max(110, height * 0.28);
  const groups = graph.groups.map((name, index2) => {
    const t = index2 / Math.max(1, graph.groups.length) * Math.PI * 2 - Math.PI / 2;
    return {
      name,
      x: cx + Math.cos(t) * rx,
      y: cy + Math.sin(t) * ry,
      count: graph.agents.filter((a2) => a2.group === name).length,
      colour: groupPalette(index2)
    };
  });
  const nodes = graph.agents.map((agent) => {
    const gi = groupIndex.get(agent.group) ?? 0;
    const anchor = groups[gi] || { x: cx, y: cy };
    const seed = hash(agent.id);
    const angle = seed % 1e3 / 1e3 * Math.PI * 2;
    const radius = 22 + seed % 90;
    return {
      id: agent.id,
      agent,
      groupIndex: gi,
      x: anchor.x + Math.cos(angle) * radius,
      y: anchor.y + Math.sin(angle) * radius
    };
  });
  const byId = new Map(nodes.map((node) => [node.id, node]));
  const layoutEdges = sampleEdges(graph.edges, LAYOUT_EDGE_LIMIT);
  const links = layoutEdges.map((edge) => ({
    source: edge.from,
    target: edge.to
  }));
  const simulation = simulation_default(nodes).force("link", link_default(links).id((d) => d.id).distance(34).strength(0.035)).force("charge", manyBody_default().strength(-42).distanceMax(180)).force("collide", collide_default().radius((d) => Math.min(9, 2.8 + Math.sqrt(graph.degree[d.id] || 0) * 0.55)).strength(0.42)).force("x", x_default2((d) => groups[d.groupIndex]?.x ?? cx).strength(0.052)).force("y", y_default2((d) => groups[d.groupIndex]?.y ?? cy).strength(0.052)).stop();
  const ticks = graph.agents.length > 450 ? 120 : 160;
  for (let i = 0; i < ticks; i++) simulation.tick();
  simulation.stop();
  return { nodes, byId, links, groups, width, height };
}
function screenToGraph(clientX, clientY, canvas, viewport) {
  const rect = canvas.getBoundingClientRect();
  return {
    x: (clientX - rect.left - viewport.x) / viewport.scale,
    y: (clientY - rect.top - viewport.y) / viewport.scale
  };
}
function applyViewport(ctx, viewport) {
  ctx.translate(viewport.x, viewport.y);
  ctx.scale(viewport.scale, viewport.scale);
}
function drawCurve(ctx, a2, b, bend) {
  const ax = a2.x || 0;
  const ay = a2.y || 0;
  const bx = b.x || 0;
  const by = b.y || 0;
  const mx = (ax + bx) / 2;
  const my = (ay + by) / 2;
  const dx = bx - ax;
  const dy = by - ay;
  const len = Math.hypot(dx, dy) || 1;
  const nx = -dy / len;
  const ny = dx / len;
  ctx.moveTo(ax, ay);
  ctx.quadraticCurveTo(mx + nx * bend, my + ny * bend, bx, by);
}
function nodeRadius(graph, id) {
  return Math.min(8.5, 2.1 + Math.sqrt(graph.degree[id] || 0) * 0.48);
}
function DenseGraphMap({
  graph,
  live,
  selectedId,
  onSelect
}) {
  const wrapRef = import_react12.default.useRef(null);
  const canvasRef = import_react12.default.useRef(null);
  const staticRef = import_react12.default.useRef(null);
  const dragRef = import_react12.default.useRef(null);
  const liveRef = import_react12.default.useRef(live);
  const [size, setSize] = import_react12.default.useState({ width: 900, height: 420 });
  const [viewport, setViewport] = import_react12.default.useState({ scale: 1, x: 0, y: 0 });
  const [hoverId, setHoverId] = import_react12.default.useState(null);
  const roleIndex = import_react12.default.useMemo(() => roleIndexFor(graph.roles), [graph.roles]);
  const graphFingerprint = import_react12.default.useMemo(
    () => [
      graph.agents.length,
      graph.edges.length,
      graph.groups.join("|"),
      graph.agents.map((a2) => a2.id).join("|")
    ].join("::"),
    [graph]
  );
  const layout2 = import_react12.default.useMemo(
    () => buildLayout(graph, size.width, size.height),
    // `graph` is rebuilt every console poll. The expensive force layout
    // should only rerun when graph shape changes, not when an equivalent
    // REST payload is normalized into fresh object identities.
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [graphFingerprint, size.width, size.height]
  );
  import_react12.default.useEffect(() => {
    liveRef.current = live;
  }, [live]);
  import_react12.default.useEffect(() => {
    const el = wrapRef.current;
    if (!el) return;
    const ro = new ResizeObserver((entries) => {
      const rect = entries[0]?.contentRect;
      if (!rect) return;
      setSize({
        width: Math.max(420, Math.floor(rect.width)),
        height: Math.max(320, Math.floor(rect.height))
      });
    });
    ro.observe(el);
    return () => ro.disconnect();
  }, []);
  const drawStatic = import_react12.default.useCallback(() => {
    const host = wrapRef.current;
    if (!host) return null;
    const dpr = Math.max(1, Math.min(2, window.devicePixelRatio || 1));
    const off = document.createElement("canvas");
    off.width = Math.floor(layout2.width * dpr);
    off.height = Math.floor(layout2.height * dpr);
    const ctx = off.getContext("2d");
    if (!ctx) return null;
    ctx.scale(dpr, dpr);
    ctx.clearRect(0, 0, layout2.width, layout2.height);
    const faint = cssVar(host, "--ink-faint", "rgba(148, 163, 184, 1)");
    const inkMuted = cssVar(host, "--ink-muted", "rgba(180, 190, 205, 1)");
    const edgeAlpha = graph.edges.length > 18e3 ? 0.03 : graph.edges.length > 6e3 ? 0.048 : 0.075;
    for (const group of layout2.groups) {
      const grad = ctx.createRadialGradient(group.x, group.y, 10, group.x, group.y, Math.max(110, group.count * 2.1));
      grad.addColorStop(0, group.colour);
      grad.addColorStop(0.34, withAlpha(group.colour, 0.34));
      grad.addColorStop(1, "transparent");
      ctx.globalAlpha = 0.16;
      ctx.fillStyle = grad;
      ctx.beginPath();
      ctx.arc(group.x, group.y, Math.max(110, group.count * 2.1), 0, Math.PI * 2);
      ctx.fill();
    }
    ctx.lineWidth = graph.edges.length > 12e3 ? 0.42 : 0.58;
    ctx.strokeStyle = faint;
    ctx.globalAlpha = edgeAlpha;
    ctx.beginPath();
    for (const edge of graph.edges) {
      const a2 = layout2.byId.get(edge.from);
      const b = layout2.byId.get(edge.to);
      if (!a2 || !b) continue;
      const seed = hash(edgeKey(edge.from, edge.to));
      const sameGroup = a2.agent.group === b.agent.group;
      const bend = sameGroup ? seed % 13 - 6 : (seed % 2 === 0 ? 1 : -1) * (18 + seed % 42);
      drawCurve(ctx, a2, b, bend);
    }
    ctx.stroke();
    const labelNodes = layout2.nodes.slice().sort((a2, b) => (graph.degree[b.id] || 0) - (graph.degree[a2.id] || 0) || a2.id.localeCompare(b.id)).slice(0, LABEL_LIMIT);
    const labelSet = new Set(labelNodes.map((n) => n.id));
    for (const node of layout2.nodes) {
      const r2 = nodeRadius(graph, node.id);
      const x3 = node.x || 0;
      const y3 = node.y || 0;
      ctx.globalAlpha = labelSet.has(node.id) ? 0.97 : 0.78;
      ctx.fillStyle = colourForRole(node.agent.role, roleIndex);
      ctx.beginPath();
      ctx.arc(x3, y3, r2, 0, Math.PI * 2);
      ctx.fill();
      if (labelSet.has(node.id)) {
        ctx.globalAlpha = 0.26;
        ctx.strokeStyle = colourForRole(node.agent.role, roleIndex);
        ctx.lineWidth = 1.2;
        ctx.beginPath();
        ctx.arc(x3, y3, r2 + 5, 0, Math.PI * 2);
        ctx.stroke();
      }
    }
    ctx.font = "600 12px Inter, system-ui, sans-serif";
    ctx.textAlign = "center";
    ctx.textBaseline = "middle";
    for (const node of labelNodes) {
      const x3 = node.x || 0;
      const y3 = (node.y || 0) - nodeRadius(graph, node.id) - 11;
      const text = node.agent.label.replace(/\s+(seat|sub-agent)\s+/i, " ");
      const metrics = ctx.measureText(text);
      ctx.globalAlpha = 0.76;
      ctx.fillStyle = "rgba(0,0,0,0.42)";
      ctx.fillRect(x3 - metrics.width / 2 - 5, y3 - 8, metrics.width + 10, 16);
      ctx.globalAlpha = 0.96;
      ctx.fillStyle = inkMuted;
      ctx.fillText(text, x3, y3);
    }
    ctx.globalAlpha = 1;
    return off;
  }, [graphFingerprint, layout2, roleIndex]);
  import_react12.default.useEffect(() => {
    staticRef.current = drawStatic();
  }, [drawStatic]);
  import_react12.default.useEffect(() => {
    let raf = 0;
    let stopped = false;
    const draw = () => {
      if (stopped) return;
      const canvas = canvasRef.current;
      const host = wrapRef.current;
      const ctx = canvas?.getContext("2d");
      if (!canvas || !ctx || !host) return;
      const dpr = Math.max(1, Math.min(2, window.devicePixelRatio || 1));
      const targetW = Math.floor(layout2.width * dpr);
      const targetH = Math.floor(layout2.height * dpr);
      if (canvas.width !== targetW || canvas.height !== targetH) {
        canvas.width = targetW;
        canvas.height = targetH;
        canvas.style.width = `${layout2.width}px`;
        canvas.style.height = `${layout2.height}px`;
      }
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
      ctx.clearRect(0, 0, layout2.width, layout2.height);
      const staticCanvas = staticRef.current || drawStatic();
      ctx.save();
      applyViewport(ctx, viewport);
      if (staticCanvas) ctx.drawImage(staticCanvas, 0, 0, layout2.width, layout2.height);
      const focus = cssVar(host, "--focus", "rgb(90, 160, 255)");
      const ok = cssVar(host, "--ok", "rgb(70, 200, 130)");
      const warn = cssVar(host, "--warn", "rgb(245, 170, 70)");
      const ink = cssVar(host, "--ink", "rgb(235, 238, 245)");
      const currentLive = liveRef.current;
      const selected2 = layout2.byId.get(hoverId || selectedId || "");
      const active = new Set(Object.keys(currentLive.active));
      const busy = new Set(Object.entries(currentLive.busy).filter(([, v]) => v).map(([k]) => k));
      if (selected2) {
        const peerSet = new Set(selected2.agent.wiredTo);
        ctx.globalAlpha = 0.88;
        ctx.lineWidth = 1.25 / Math.sqrt(viewport.scale);
        ctx.strokeStyle = focus;
        ctx.beginPath();
        for (const peerId of peerSet) {
          const peer = layout2.byId.get(peerId);
          if (!peer) continue;
          drawCurve(ctx, selected2, peer, selected2.agent.group === peer.agent.group ? 8 : 28);
        }
        ctx.stroke();
        for (const peerId of peerSet) {
          const peer = layout2.byId.get(peerId);
          if (!peer) continue;
          ctx.globalAlpha = 0.98;
          ctx.fillStyle = focus;
          ctx.beginPath();
          ctx.arc(peer.x || 0, peer.y || 0, 3.1 / Math.sqrt(viewport.scale), 0, Math.PI * 2);
          ctx.fill();
        }
      }
      for (const id of active) {
        const node = layout2.byId.get(id);
        if (!node) continue;
        ctx.globalAlpha = 0.28;
        ctx.strokeStyle = ok;
        ctx.lineWidth = 2 / Math.sqrt(viewport.scale);
        ctx.beginPath();
        ctx.arc(node.x || 0, node.y || 0, 11 / Math.sqrt(viewport.scale), 0, Math.PI * 2);
        ctx.stroke();
      }
      for (const id of busy) {
        const node = layout2.byId.get(id);
        if (!node) continue;
        const phase = Date.now() / 820 % (Math.PI * 2);
        ctx.globalAlpha = 0.9;
        ctx.strokeStyle = warn;
        ctx.lineWidth = 2.1 / Math.sqrt(viewport.scale);
        ctx.beginPath();
        ctx.arc(node.x || 0, node.y || 0, 14 / Math.sqrt(viewport.scale), phase, phase + Math.PI * 1.35);
        ctx.stroke();
      }
      if (selected2) {
        const r2 = nodeRadius(graph, selected2.id) + 3.6 / Math.sqrt(viewport.scale);
        ctx.globalAlpha = 1;
        ctx.fillStyle = ink;
        ctx.strokeStyle = focus;
        ctx.lineWidth = 2.4 / Math.sqrt(viewport.scale);
        ctx.beginPath();
        ctx.arc(selected2.x || 0, selected2.y || 0, r2, 0, Math.PI * 2);
        ctx.fill();
        ctx.stroke();
      }
      ctx.restore();
      raf = requestAnimationFrame(draw);
    };
    raf = requestAnimationFrame(draw);
    return () => {
      stopped = true;
      cancelAnimationFrame(raf);
    };
  }, [drawStatic, graphFingerprint, hoverId, layout2, selectedId, viewport]);
  const nearestId = import_react12.default.useCallback((clientX, clientY) => {
    const canvas = canvasRef.current;
    if (!canvas) return null;
    const pos = screenToGraph(clientX, clientY, canvas, viewport);
    let best = null;
    const threshold = Math.max(12, 18 / viewport.scale);
    for (const node of layout2.nodes) {
      const dx = (node.x || 0) - pos.x;
      const dy = (node.y || 0) - pos.y;
      const d2 = dx * dx + dy * dy;
      if (d2 > threshold * threshold) continue;
      if (!best || d2 < best.d2) best = { id: node.id, d2 };
    }
    return best?.id || null;
  }, [layout2, viewport]);
  const hover = hoverId ? graph.byId.get(hoverId) : null;
  const selected = selectedId ? graph.byId.get(selectedId) : null;
  return /* @__PURE__ */ (0, import_jsx_runtime20.jsxs)(
    "div",
    {
      ref: wrapRef,
      className: "topo-dense",
      "data-testid": "topology-dense-map",
      onPointerDown: (event) => {
        const id = nearestId(event.clientX, event.clientY);
        if (id) onSelect(id);
        dragRef.current = { x: event.clientX, y: event.clientY, viewport };
        event.currentTarget.setPointerCapture(event.pointerId);
      },
      onPointerMove: (event) => {
        if (dragRef.current) {
          const dx = event.clientX - dragRef.current.x;
          const dy = event.clientY - dragRef.current.y;
          setViewport({
            ...dragRef.current.viewport,
            x: dragRef.current.viewport.x + dx,
            y: dragRef.current.viewport.y + dy
          });
          return;
        }
        setHoverId(nearestId(event.clientX, event.clientY));
      },
      onPointerUp: (event) => {
        dragRef.current = null;
        event.currentTarget.releasePointerCapture(event.pointerId);
      },
      onPointerCancel: (event) => {
        dragRef.current = null;
        event.currentTarget.releasePointerCapture(event.pointerId);
      },
      onPointerLeave: () => setHoverId(null),
      onWheel: (event) => {
        event.preventDefault();
        const canvas = canvasRef.current;
        if (!canvas) return;
        const rect = canvas.getBoundingClientRect();
        const mx = event.clientX - rect.left;
        const my = event.clientY - rect.top;
        const nextScale = Math.max(0.55, Math.min(4, viewport.scale * (event.deltaY < 0 ? 1.12 : 0.88)));
        const gx = (mx - viewport.x) / viewport.scale;
        const gy = (my - viewport.y) / viewport.scale;
        setViewport({
          scale: nextScale,
          x: mx - gx * nextScale,
          y: my - gy * nextScale
        });
      },
      children: [
        /* @__PURE__ */ (0, import_jsx_runtime20.jsx)("canvas", { ref: canvasRef, className: "topo-dense__canvas", "aria-label": "Dense topology force graph" }),
        /* @__PURE__ */ (0, import_jsx_runtime20.jsxs)("div", { className: "topo-dense__toolbar", children: [
          /* @__PURE__ */ (0, import_jsx_runtime20.jsx)("button", { type: "button", onClick: () => setViewport((v) => ({ ...v, scale: Math.min(4, v.scale * 1.22) })), children: "+" }),
          /* @__PURE__ */ (0, import_jsx_runtime20.jsx)("button", { type: "button", onClick: () => setViewport((v) => ({ ...v, scale: Math.max(0.55, v.scale / 1.22) })), children: "-" }),
          /* @__PURE__ */ (0, import_jsx_runtime20.jsx)("button", { type: "button", onClick: () => setViewport({ scale: 1, x: 0, y: 0 }), children: "Reset" })
        ] }),
        /* @__PURE__ */ (0, import_jsx_runtime20.jsx)("div", { className: "topo-dense__labels", "aria-hidden": "true", children: layout2.groups.map((g) => /* @__PURE__ */ (0, import_jsx_runtime20.jsxs)(
          "div",
          {
            className: "topo-dense__group-label",
            style: {
              left: `${g.x * viewport.scale + viewport.x}px`,
              top: `${g.y * viewport.scale + viewport.y + 86}px`,
              borderColor: g.colour
            },
            children: [
              /* @__PURE__ */ (0, import_jsx_runtime20.jsx)("strong", { children: g.name }),
              /* @__PURE__ */ (0, import_jsx_runtime20.jsxs)("span", { children: [
                g.count,
                " agents"
              ] })
            ]
          },
          g.name
        )) }),
        (hover || selected) && /* @__PURE__ */ (0, import_jsx_runtime20.jsxs)("div", { className: "topo-dense__inspector", children: [
          /* @__PURE__ */ (0, import_jsx_runtime20.jsx)("strong", { children: (hover || selected)?.label }),
          /* @__PURE__ */ (0, import_jsx_runtime20.jsx)("span", { children: (hover || selected)?.group }),
          /* @__PURE__ */ (0, import_jsx_runtime20.jsxs)("span", { children: [
            (hover || selected)?.wiredTo.length || 0,
            " peers"
          ] })
        ] })
      ]
    }
  );
}

// src/panels/topology/LargeGraphSummary.tsx
var import_jsx_runtime21 = require("react/jsx-runtime");
var EDGE_SAMPLE_LIMIT = 1500;
function fmt(value) {
  return new Intl.NumberFormat(void 0, { maximumFractionDigits: 1 }).format(value);
}
function percent(value) {
  return `${fmt(value * 100)}%`;
}
function agentRank(graph) {
  return graph.agents.slice().sort((a2, b) => {
    const degreeDelta = (graph.degree[b.id] || 0) - (graph.degree[a2.id] || 0);
    if (degreeDelta !== 0) return degreeDelta;
    const busyDelta = Number(!!a2.labels.parent_identity) - Number(!!b.labels.parent_identity);
    if (busyDelta !== 0) return busyDelta;
    return a2.id.localeCompare(b.id);
  });
}
function LargeGraphSummary({
  graph,
  live
}) {
  const stats = import_react13.default.useMemo(() => graphStats(graph), [graph]);
  const groups = import_react13.default.useMemo(() => groupSummaries(graph), [graph]);
  const matrix = import_react13.default.useMemo(() => groupMatrix(graph, 8), [graph]);
  const ranked = import_react13.default.useMemo(() => agentRank(graph), [graph]);
  const [query, setQuery] = import_react13.default.useState("");
  const [selectedId, setSelectedId] = import_react13.default.useState(() => ranked[0]?.id || "");
  import_react13.default.useEffect(() => {
    if (!selectedId || !graph.byId.has(selectedId)) {
      setSelectedId(ranked[0]?.id || "");
    }
  }, [graph, ranked, selectedId]);
  const matches = import_react13.default.useMemo(() => {
    const q = query.trim().toLowerCase();
    const source = q ? ranked.filter(
      (a2) => a2.id.toLowerCase().includes(q) || a2.label.toLowerCase().includes(q) || a2.group.toLowerCase().includes(q) || a2.role.toLowerCase().includes(q)
    ) : ranked;
    return source.slice(0, 80);
  }, [query, ranked]);
  const selected = graph.byId.get(selectedId) || ranked[0];
  const peers = selected ? selected.wiredTo.map((id) => graph.byId.get(id)).filter((a2) => !!a2).sort((a2, b) => a2.group.localeCompare(b.group) || a2.id.localeCompare(b.id)) : [];
  const activeCount = Object.keys(live.active).length;
  const busyCount = Object.values(live.busy).filter(Boolean).length;
  const visiblePeerPreview = peers.slice(0, 150);
  return /* @__PURE__ */ (0, import_jsx_runtime21.jsxs)("div", { className: "topo-summary", "data-testid": "topology-summary", children: [
    /* @__PURE__ */ (0, import_jsx_runtime21.jsx)(
      DenseGraphMap,
      {
        graph,
        live,
        selectedId: selected?.id,
        onSelect: setSelectedId
      }
    ),
    /* @__PURE__ */ (0, import_jsx_runtime21.jsxs)("div", { className: "topo-summary__stats", "aria-label": "Topology scale", children: [
      /* @__PURE__ */ (0, import_jsx_runtime21.jsx)(ScaleStat, { label: "Agents", value: String(stats.nodeCount) }),
      /* @__PURE__ */ (0, import_jsx_runtime21.jsx)(ScaleStat, { label: "Edges", value: String(stats.edgeCount), sub: `${percent(stats.density)} density` }),
      /* @__PURE__ */ (0, import_jsx_runtime21.jsx)(ScaleStat, { label: "Degree", value: `${fmt(stats.avgDegree)} avg`, sub: `${stats.minDegree}-${stats.maxDegree}` }),
      /* @__PURE__ */ (0, import_jsx_runtime21.jsx)(ScaleStat, { label: "Live", value: String(activeCount), sub: busyCount > 0 ? `${busyCount} working` : "idle" }),
      stats.edgeCount > EDGE_SAMPLE_LIMIT && /* @__PURE__ */ (0, import_jsx_runtime21.jsx)(ScaleStat, { label: "Graph views", value: `${EDGE_SAMPLE_LIMIT} edges`, sub: "sampled" })
    ] }),
    /* @__PURE__ */ (0, import_jsx_runtime21.jsxs)("div", { className: "topo-summary__grid", children: [
      /* @__PURE__ */ (0, import_jsx_runtime21.jsxs)("section", { className: "topo-summary__section topo-summary__section--groups", "aria-label": "Mob groups", children: [
        /* @__PURE__ */ (0, import_jsx_runtime21.jsxs)("div", { className: "topo-summary__section-head", children: [
          /* @__PURE__ */ (0, import_jsx_runtime21.jsx)("h3", { children: "Mobs" }),
          /* @__PURE__ */ (0, import_jsx_runtime21.jsxs)("span", { children: [
            groups.length,
            " groups"
          ] })
        ] }),
        /* @__PURE__ */ (0, import_jsx_runtime21.jsx)("div", { className: "topo-summary__group-list", children: groups.map((g) => /* @__PURE__ */ (0, import_jsx_runtime21.jsxs)("div", { className: "topo-summary__group-row", children: [
          /* @__PURE__ */ (0, import_jsx_runtime21.jsx)("span", { className: "topo-summary__group-name", title: g.group, children: g.group }),
          /* @__PURE__ */ (0, import_jsx_runtime21.jsx)("span", { children: g.count }),
          /* @__PURE__ */ (0, import_jsx_runtime21.jsxs)("span", { children: [
            g.internalEdges,
            " internal"
          ] }),
          /* @__PURE__ */ (0, import_jsx_runtime21.jsxs)("span", { children: [
            g.externalEdges,
            " cross"
          ] })
        ] }, g.group)) })
      ] }),
      /* @__PURE__ */ (0, import_jsx_runtime21.jsxs)("section", { className: "topo-summary__section topo-summary__section--matrix", "aria-label": "Group edge matrix", children: [
        /* @__PURE__ */ (0, import_jsx_runtime21.jsxs)("div", { className: "topo-summary__section-head", children: [
          /* @__PURE__ */ (0, import_jsx_runtime21.jsx)("h3", { children: "Edge Matrix" }),
          /* @__PURE__ */ (0, import_jsx_runtime21.jsxs)("span", { children: [
            matrix.length,
            " populated cells"
          ] })
        ] }),
        /* @__PURE__ */ (0, import_jsx_runtime21.jsx)("div", { className: "topo-summary__matrix", children: matrix.map((cell) => /* @__PURE__ */ (0, import_jsx_runtime21.jsxs)("div", { className: "topo-summary__matrix-row", children: [
          /* @__PURE__ */ (0, import_jsx_runtime21.jsx)("span", { title: cell.from, children: cell.from }),
          /* @__PURE__ */ (0, import_jsx_runtime21.jsx)("span", { title: cell.to, children: cell.to }),
          /* @__PURE__ */ (0, import_jsx_runtime21.jsx)("strong", { children: cell.edges })
        ] }, `${cell.from}:${cell.to}`)) })
      ] }),
      /* @__PURE__ */ (0, import_jsx_runtime21.jsxs)("section", { className: "topo-summary__section topo-summary__section--agent", "aria-label": "Selected agent ego network", children: [
        /* @__PURE__ */ (0, import_jsx_runtime21.jsxs)("div", { className: "topo-summary__section-head", children: [
          /* @__PURE__ */ (0, import_jsx_runtime21.jsx)("h3", { children: "Ego Network" }),
          /* @__PURE__ */ (0, import_jsx_runtime21.jsx)("span", { children: selected ? `${peers.length} peers` : "none" })
        ] }),
        /* @__PURE__ */ (0, import_jsx_runtime21.jsxs)("div", { className: "topo-summary__agent-tools", children: [
          /* @__PURE__ */ (0, import_jsx_runtime21.jsx)(
            "input",
            {
              className: "topo-summary__search",
              type: "search",
              value: query,
              onChange: (event) => setQuery(event.target.value),
              placeholder: "Filter agents",
              "aria-label": "Filter topology agents"
            }
          ),
          /* @__PURE__ */ (0, import_jsx_runtime21.jsx)(
            "select",
            {
              className: "topo-summary__select",
              value: selected?.id || "",
              onChange: (event) => setSelectedId(event.target.value),
              "aria-label": "Select topology agent",
              children: matches.map((a2) => /* @__PURE__ */ (0, import_jsx_runtime21.jsx)("option", { value: a2.id, children: a2.label }, a2.id))
            }
          )
        ] }),
        selected && /* @__PURE__ */ (0, import_jsx_runtime21.jsxs)("div", { className: "topo-summary__agent-card", "data-testid": `topology-ego:${selected.id}`, children: [
          /* @__PURE__ */ (0, import_jsx_runtime21.jsxs)("div", { children: [
            /* @__PURE__ */ (0, import_jsx_runtime21.jsx)("strong", { children: selected.label }),
            /* @__PURE__ */ (0, import_jsx_runtime21.jsx)("span", { children: selected.id })
          ] }),
          /* @__PURE__ */ (0, import_jsx_runtime21.jsxs)("div", { children: [
            /* @__PURE__ */ (0, import_jsx_runtime21.jsx)("span", { children: selected.group }),
            /* @__PURE__ */ (0, import_jsx_runtime21.jsx)("span", { children: selected.role }),
            /* @__PURE__ */ (0, import_jsx_runtime21.jsx)("span", { children: selected.state || "unknown" })
          ] })
        ] }),
        /* @__PURE__ */ (0, import_jsx_runtime21.jsx)("div", { className: "topo-summary__peer-list", children: visiblePeerPreview.map((peer) => /* @__PURE__ */ (0, import_jsx_runtime21.jsxs)(
          "button",
          {
            type: "button",
            className: "topo-summary__peer",
            onClick: () => setSelectedId(peer.id),
            title: `${peer.label} - ${peer.group}`,
            children: [
              /* @__PURE__ */ (0, import_jsx_runtime21.jsx)("span", { children: peer.label }),
              /* @__PURE__ */ (0, import_jsx_runtime21.jsx)("small", { children: peer.group })
            ]
          },
          peer.id
        )) })
      ] })
    ] })
  ] });
}
function ScaleStat({
  label,
  value,
  sub
}) {
  return /* @__PURE__ */ (0, import_jsx_runtime21.jsxs)("div", { className: "topo-summary__stat", children: [
    /* @__PURE__ */ (0, import_jsx_runtime21.jsx)("span", { children: label }),
    /* @__PURE__ */ (0, import_jsx_runtime21.jsx)("strong", { children: value }),
    sub && /* @__PURE__ */ (0, import_jsx_runtime21.jsx)("small", { children: sub })
  ] });
}

// src/panels/TopologyPanel.tsx
var import_jsx_runtime22 = require("react/jsx-runtime");
var VIEW_STORAGE = "mobkit-console-topology-view";
var LABELS_STORAGE = "mobkit-console-topology-labels";
var VIEWS = [
  { id: "summary", label: "Summary", help: "Aggregate scale, groups, and selected ego network" },
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
  const [view, setView] = import_react14.default.useState(() => {
    try {
      const stored = localStorage.getItem(VIEW_STORAGE);
      if (stored === "summary" || stored === "force" || stored === "bullseye" || stored === "roles") return stored;
    } catch {
    }
    return "summary";
  });
  const [userPickedView, setUserPickedView] = import_react14.default.useState(false);
  const pickView = (next) => {
    setUserPickedView(true);
    setView(next);
    try {
      localStorage.setItem(VIEW_STORAGE, next);
    } catch {
    }
  };
  const [labelsMode, setLabelsMode] = import_react14.default.useState(() => {
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
  const graph = import_react14.default.useMemo(() => buildGraph(nodes, agents), [nodes, agents]);
  const live = useTopologyActivity(activity, graph, { life: 1500 });
  const roleIndex = import_react14.default.useMemo(() => roleIndexFor(graph.roles), [graph.roles]);
  const denseGraph = graph.agents.length >= 150 || graph.edges.length >= 3e3;
  import_react14.default.useEffect(() => {
    if (!denseGraph || userPickedView || view === "summary") return;
    setView("summary");
    try {
      localStorage.setItem(VIEW_STORAGE, "summary");
    } catch {
    }
  }, [denseGraph, userPickedView, view]);
  const liveCount = Object.keys(live.active).length;
  const busyCount = Object.values(live.busy).filter(Boolean).length;
  return /* @__PURE__ */ (0, import_jsx_runtime22.jsxs)("div", { className: "topo", "data-testid": "topology-panel", children: [
    /* @__PURE__ */ (0, import_jsx_runtime22.jsxs)("div", { className: "topo__head", children: [
      /* @__PURE__ */ (0, import_jsx_runtime22.jsx)("h2", { children: "Topology" }),
      /* @__PURE__ */ (0, import_jsx_runtime22.jsxs)("span", { className: "topo__head-meta", children: [
        graph.agents.length,
        " agents \xB7 ",
        graph.edges.length,
        " edges",
        busyCount > 0 ? ` \xB7 ${busyCount} working` : "",
        liveCount > 0 && busyCount === 0 ? ` \xB7 ${liveCount} live` : ""
      ] }),
      view !== "roles" && view !== "summary" && /* @__PURE__ */ (0, import_jsx_runtime22.jsxs)("div", { className: "topo__viewbar topo__viewbar--labels", role: "group", "aria-label": "Labels", children: [
        /* @__PURE__ */ (0, import_jsx_runtime22.jsx)("span", { className: "topo__viewbar-tag", children: "Labels" }),
        LABEL_MODES.map((m2) => /* @__PURE__ */ (0, import_jsx_runtime22.jsx)(
          "button",
          {
            type: "button",
            className: `topo__viewbtn ${labelsMode === m2.id ? "is-active" : ""}`,
            onClick: () => pickLabelsMode(m2.id),
            title: m2.help,
            "data-testid": `topology-labels:${m2.id}`,
            children: m2.label
          },
          m2.id
        ))
      ] }),
      /* @__PURE__ */ (0, import_jsx_runtime22.jsx)("div", { className: "topo__viewbar", children: VIEWS.map((v) => /* @__PURE__ */ (0, import_jsx_runtime22.jsx)(
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
    /* @__PURE__ */ (0, import_jsx_runtime22.jsxs)("div", { className: "topo__body", children: [
      view === "summary" && /* @__PURE__ */ (0, import_jsx_runtime22.jsx)(
        LargeGraphSummary,
        {
          graph,
          live
        }
      ),
      view === "force" && /* @__PURE__ */ (0, import_jsx_runtime22.jsx)(
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
      view === "bullseye" && /* @__PURE__ */ (0, import_jsx_runtime22.jsx)(
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
      view === "roles" && /* @__PURE__ */ (0, import_jsx_runtime22.jsx)(
        RoleTree,
        {
          nodes,
          agents,
          activity
        }
      )
    ] }),
    view !== "roles" && view !== "summary" && graph.roles.length > 0 && /* @__PURE__ */ (0, import_jsx_runtime22.jsx)("div", { className: "topo__legend", children: graph.roles.map((role) => {
      const count = graph.agents.filter((a2) => a2.role === role).length;
      return /* @__PURE__ */ (0, import_jsx_runtime22.jsxs)("div", { className: "topo__legend-item", children: [
        /* @__PURE__ */ (0, import_jsx_runtime22.jsx)(
          "span",
          {
            className: "topo__legend-dot",
            style: { background: colourForRole(role, roleIndex) }
          }
        ),
        role,
        /* @__PURE__ */ (0, import_jsx_runtime22.jsx)("span", { className: "topo__legend-count", children: count })
      ] }, role);
    }) })
  ] });
}

// src/panels/TimelinePanel.tsx
var import_react15 = __toESM(require("react"));
var import_jsx_runtime23 = require("react/jsx-runtime");
var INTERNAL_TIMELINE_EVENTS = /* @__PURE__ */ new Set([
  "keep-alive",
  "snapshot_complete",
  "snapshot_started",
  "subscribed"
]);
function classifyFrame(frame2) {
  const ev = frame2.event;
  if (ev === "gating_decision" || ev.startsWith("gate_")) return "gate";
  if (ev === "run_failed" || ev === "interaction_failed") return "warn";
  if (ev === "route_changed" || ev === "topology_updated") return "topology";
  if (ev === "member_ready" || ev === "member_retired" || ev === "state_changed") return "lifecycle";
  if (ev === "interaction_complete" || ev === "interaction_started") return "interaction";
  return "dispatch";
}
function formatType(type) {
  switch (type) {
    case "gate":
      return "Gate";
    case "warn":
      return "Warning";
    case "topology":
      return "Topology";
    case "lifecycle":
      return "Lifecycle";
    case "interaction":
      return "Interaction";
    default:
      return "Dispatch";
  }
}
function formatTime(tsMs) {
  if (!tsMs) return "\u2014";
  const d = new Date(tsMs);
  const hh = String(d.getHours()).padStart(2, "0");
  const mm = String(d.getMinutes()).padStart(2, "0");
  return `${hh}:${mm}`;
}
function summarizeFrame(frame2) {
  const ev = frame2.event;
  const data = frame2.data || {};
  const shortInteraction = String(frame2.interactionId || "").slice(0, 8);
  switch (ev) {
    case "interaction_complete":
      return shortInteraction ? `Completed ${shortInteraction}` : "Completed";
    case "interaction_failed":
      return `Failed: ${String(data.error || data.reason || "error")}`;
    case "interaction_started":
      return shortInteraction ? `Started ${shortInteraction}` : "Started";
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
  const entries = import_react15.default.useMemo(() => {
    const todayMs = (() => {
      const d = /* @__PURE__ */ new Date();
      d.setHours(0, 0, 0, 0);
      return d.getTime();
    })();
    return frames.filter((f) => !INTERNAL_TIMELINE_EVENTS.has(f.event)).filter((f) => (f.timestampMs || 0) >= todayMs).slice(0, 80).map((f) => ({
      time: formatTime(f.timestampMs),
      type: classifyFrame(f),
      text: summarizeFrame(f),
      who: f.identity || "_system"
    }));
  }, [frames]);
  const today = /* @__PURE__ */ new Date();
  const dateLabel = today.toLocaleDateString(void 0, { month: "short", day: "numeric", year: "numeric" });
  return /* @__PURE__ */ (0, import_jsx_runtime23.jsxs)("div", { className: "tl", "data-testid": "timeline-panel", children: [
    /* @__PURE__ */ (0, import_jsx_runtime23.jsxs)("div", { className: "tl__head", children: [
      /* @__PURE__ */ (0, import_jsx_runtime23.jsx)("h2", { children: "Today" }),
      /* @__PURE__ */ (0, import_jsx_runtime23.jsxs)("p", { children: [
        "\xB7 ",
        entries.length,
        " events \xB7 ",
        dateLabel
      ] })
    ] }),
    /* @__PURE__ */ (0, import_jsx_runtime23.jsxs)("div", { className: "tl__body", children: [
      entries.length === 0 && /* @__PURE__ */ (0, import_jsx_runtime23.jsx)("div", { style: { gridColumn: "1 / -1", padding: "40px 0", color: "var(--ink-dim)", fontFamily: "var(--mono)", fontSize: 12, textAlign: "center" }, children: "No events yet today." }),
      entries.map((e, i) => /* @__PURE__ */ (0, import_jsx_runtime23.jsxs)("div", { className: "tl__row", "data-type": e.type, children: [
        /* @__PURE__ */ (0, import_jsx_runtime23.jsx)("div", { className: "tl__time", children: e.time }),
        /* @__PURE__ */ (0, import_jsx_runtime23.jsx)("div", { className: "tl__rail", children: /* @__PURE__ */ (0, import_jsx_runtime23.jsx)("span", { className: "tl__dot" }) }),
        /* @__PURE__ */ (0, import_jsx_runtime23.jsxs)("div", { className: "tl__card", children: [
          /* @__PURE__ */ (0, import_jsx_runtime23.jsxs)("div", { children: [
            /* @__PURE__ */ (0, import_jsx_runtime23.jsx)("span", { className: "tl__type", children: formatType(e.type) }),
            " ",
            /* @__PURE__ */ (0, import_jsx_runtime23.jsx)("span", { children: e.text })
          ] }),
          /* @__PURE__ */ (0, import_jsx_runtime23.jsx)("div", { className: "tl__who", children: e.who })
        ] })
      ] }, i))
    ] })
  ] });
}

// src/panels/GatingInboxPanel.tsx
var import_react16 = __toESM(require("react"));
var import_jsx_runtime24 = require("react/jsx-runtime");
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
    approved: s.approved,
    rejected: s.rejected,
    escalated: s.escalated
  }));
}
function GatingInboxPanel({ pending, audit, onDecide }) {
  const [tab, setTab] = import_react16.default.useState("pending");
  const [selectedId, setSelectedId] = import_react16.default.useState(null);
  const policies = import_react16.default.useMemo(() => derivePolicies(audit), [audit]);
  const autoApproved = audit.filter((e) => {
    const r2 = e;
    return String(r2.decision || "").toLowerCase() === "auto_approve" || String(r2.event_type || "").includes("auto");
  });
  const currentList = tab === "pending" ? pending : tab === "auto" ? autoApproved : audit;
  return /* @__PURE__ */ (0, import_jsx_runtime24.jsxs)("div", { className: "gating", "data-testid": "gating-panel", children: [
    /* @__PURE__ */ (0, import_jsx_runtime24.jsxs)("div", { className: "gating__head", children: [
      /* @__PURE__ */ (0, import_jsx_runtime24.jsx)("h2", { children: "Approvals" }),
      /* @__PURE__ */ (0, import_jsx_runtime24.jsxs)("p", { children: [
        "\xB7 ",
        pending.length,
        " pending \xB7 ",
        autoApproved.length,
        " auto-approved \xB7 ",
        policies.length,
        " policies"
      ] })
    ] }),
    /* @__PURE__ */ (0, import_jsx_runtime24.jsxs)("div", { className: "gating__tabs", children: [
      /* @__PURE__ */ (0, import_jsx_runtime24.jsxs)(
        "button",
        {
          className: `gating__tab ${tab === "pending" ? "is-active" : ""}`,
          onClick: () => setTab("pending"),
          "data-testid": "gating-tab:pending",
          children: [
            "Pending ",
            /* @__PURE__ */ (0, import_jsx_runtime24.jsx)("span", { className: "n", children: pending.length })
          ]
        }
      ),
      /* @__PURE__ */ (0, import_jsx_runtime24.jsxs)(
        "button",
        {
          className: `gating__tab ${tab === "auto" ? "is-active" : ""}`,
          onClick: () => setTab("auto"),
          "data-testid": "gating-tab:auto",
          children: [
            "Auto ",
            /* @__PURE__ */ (0, import_jsx_runtime24.jsx)("span", { className: "n", children: autoApproved.length })
          ]
        }
      ),
      /* @__PURE__ */ (0, import_jsx_runtime24.jsxs)(
        "button",
        {
          className: `gating__tab ${tab === "audit" ? "is-active" : ""}`,
          onClick: () => setTab("audit"),
          "data-testid": "gating-tab:audit",
          children: [
            "Audit ",
            /* @__PURE__ */ (0, import_jsx_runtime24.jsx)("span", { className: "n", children: audit.length })
          ]
        }
      ),
      /* @__PURE__ */ (0, import_jsx_runtime24.jsxs)(
        "button",
        {
          className: `gating__tab ${tab === "policies" ? "is-active" : ""}`,
          onClick: () => setTab("policies"),
          "data-testid": "gating-tab:policies",
          children: [
            "Policies ",
            /* @__PURE__ */ (0, import_jsx_runtime24.jsx)("span", { className: "n", children: policies.length })
          ]
        }
      )
    ] }),
    /* @__PURE__ */ (0, import_jsx_runtime24.jsx)("div", { className: "gating__list", children: tab === "policies" ? /* @__PURE__ */ (0, import_jsx_runtime24.jsxs)("div", { className: "gating__policies", children: [
      policies.length === 0 && /* @__PURE__ */ (0, import_jsx_runtime24.jsx)("div", { className: "gating__empty", children: "No gate policies inferred from recent audit." }),
      policies.map((policy) => /* @__PURE__ */ (0, import_jsx_runtime24.jsxs)("div", { className: "gpolicy", "data-state": policy.state, "data-testid": `gating-policy:${policy.id}`, children: [
        /* @__PURE__ */ (0, import_jsx_runtime24.jsxs)("div", { className: "gpolicy__head", children: [
          /* @__PURE__ */ (0, import_jsx_runtime24.jsx)("span", { className: "gpolicy__action", children: policy.action }),
          /* @__PURE__ */ (0, import_jsx_runtime24.jsx)("span", { className: `gpolicy__state gpolicy__state--${policy.state}`, children: policy.state })
        ] }),
        /* @__PURE__ */ (0, import_jsx_runtime24.jsxs)("div", { className: "gpolicy__meta", children: [
          "scope: ",
          policy.scope
        ] }),
        /* @__PURE__ */ (0, import_jsx_runtime24.jsx)("div", { className: "gpolicy__rule", children: policy.thresh }),
        /* @__PURE__ */ (0, import_jsx_runtime24.jsxs)("div", { className: "gpolicy__stats", children: [
          /* @__PURE__ */ (0, import_jsx_runtime24.jsxs)("span", { children: [
            /* @__PURE__ */ (0, import_jsx_runtime24.jsx)("b", { children: policy.approved }),
            " approved"
          ] }),
          /* @__PURE__ */ (0, import_jsx_runtime24.jsxs)("span", { children: [
            /* @__PURE__ */ (0, import_jsx_runtime24.jsx)("b", { children: policy.rejected }),
            " rejected"
          ] }),
          /* @__PURE__ */ (0, import_jsx_runtime24.jsxs)("span", { children: [
            /* @__PURE__ */ (0, import_jsx_runtime24.jsx)("b", { children: policy.escalated }),
            " escalated"
          ] })
        ] }),
        /* @__PURE__ */ (0, import_jsx_runtime24.jsx)("div", { className: "gpolicy__approvers", children: policy.approvers.length === 0 ? /* @__PURE__ */ (0, import_jsx_runtime24.jsx)("span", { className: "chip", children: "no approvers recorded" }) : policy.approvers.map((approver) => /* @__PURE__ */ (0, import_jsx_runtime24.jsx)("span", { className: "chip", children: approver }, approver)) })
      ] }, policy.id))
    ] }) : /* @__PURE__ */ (0, import_jsx_runtime24.jsxs)(import_jsx_runtime24.Fragment, { children: [
      currentList.length === 0 && /* @__PURE__ */ (0, import_jsx_runtime24.jsxs)("div", { className: "gating__empty", children: [
        "No ",
        tab,
        " items."
      ] }),
      currentList.map((entry, index2) => {
        const r2 = entry;
        const pid = String(r2.pending_id || r2.audit_id || `item-${index2}`);
        const action = String(r2.action_id || r2.event_type || "unknown action");
        const agent = String(r2.agent || r2.identity || r2.actor || "");
        const waited = formatWaited(r2);
        const risk = getRisk(r2);
        const payload = payloadSummary(r2);
        const selected = selectedId === pid;
        const showActions = tab === "pending";
        return /* @__PURE__ */ (0, import_jsx_runtime24.jsxs)(
          "div",
          {
            className: `gitem ${selected ? "is-selected" : ""}`,
            "data-risk": risk,
            "data-testid": `gating-pending:${pid}`,
            onClick: () => setSelectedId(pid),
            children: [
              /* @__PURE__ */ (0, import_jsx_runtime24.jsx)("span", { className: "gitem__risk" }),
              /* @__PURE__ */ (0, import_jsx_runtime24.jsx)("span", { className: "gitem__id", children: pid.slice(0, 8) }),
              /* @__PURE__ */ (0, import_jsx_runtime24.jsxs)("span", { children: [
                /* @__PURE__ */ (0, import_jsx_runtime24.jsx)("div", { className: "gitem__action", children: action }),
                payload && /* @__PURE__ */ (0, import_jsx_runtime24.jsx)("div", { className: "gitem__payload", children: payload }),
                agent && /* @__PURE__ */ (0, import_jsx_runtime24.jsx)("div", { className: "gitem__agent", children: agent })
              ] }),
              showActions ? /* @__PURE__ */ (0, import_jsx_runtime24.jsxs)("span", { className: "gitem__actions", children: [
                /* @__PURE__ */ (0, import_jsx_runtime24.jsx)(
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
                /* @__PURE__ */ (0, import_jsx_runtime24.jsx)(
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
                /* @__PURE__ */ (0, import_jsx_runtime24.jsx)(
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
              ] }) : /* @__PURE__ */ (0, import_jsx_runtime24.jsx)("span", { className: "gitem__actions" }),
              /* @__PURE__ */ (0, import_jsx_runtime24.jsxs)("span", { className: "gitem__waited", children: [
                "waited",
                /* @__PURE__ */ (0, import_jsx_runtime24.jsx)("br", {}),
                waited
              ] })
            ]
          },
          pid
        );
      })
    ] }) })
  ] });
}

// src/panels/RosterPanel.tsx
var import_react17 = __toESM(require("react"));
var import_jsx_runtime25 = require("react/jsx-runtime");
var ROLE_BUCKETS = ["all", "personal", "coordinator", "domain", "internal"];
function roleOf(a2) {
  const p = (a2.role || a2.kind || "").toLowerCase();
  const g = (a2.group || "").toLowerCase();
  if (p.includes("personal") || g.includes("personal")) return "personal";
  if (p.includes("coord") || p.includes("triage") || p.includes("router")) return "coordinator";
  if (p.includes("monitor") || p.includes("scribe") || p.includes("gate")) return "internal";
  return "domain";
}
function stateLabel(state) {
  return (state || "unknown").toLowerCase();
}
function displayPeer(peer) {
  if (typeof peer === "string") return peer.split("/").pop() || peer;
  if (peer && typeof peer === "object") {
    const record = peer;
    const value = record.label ?? record.display_name ?? record.name ?? record.identity ?? record.member_id ?? record.id;
    if (typeof value === "string") return value.split("/").pop() || value;
  }
  return "";
}
function RosterPanel({
  agents,
  selectedMemberId,
  onSelect,
  onChat,
  onDetails,
  onLifecycle,
  canResetLifecycle = false,
  actionLabels,
  actionVisibility
}) {
  const [q, setQ] = import_react17.default.useState("");
  const [role, setRole] = import_react17.default.useState("all");
  const [sel, setSel] = import_react17.default.useState(agents[0]?.member_id || "");
  import_react17.default.useEffect(() => {
    if (selectedMemberId) setSel(selectedMemberId);
  }, [selectedMemberId]);
  const rows = import_react17.default.useMemo(() => {
    return agents.filter((a2) => {
      if (role !== "all" && roleOf(a2) !== role) return false;
      if (!q) return true;
      const hay = `${a2.label} ${a2.member_id} ${a2.identity || ""} ${a2.role || ""} ${a2.kind || ""}`.toLowerCase();
      return hay.includes(q.toLowerCase());
    });
  }, [agents, q, role]);
  const active = rows.find((r2) => r2.member_id === sel) || rows[0];
  const activeIdentity = active?.identity || active?.member_id || "";
  const activePeers = (active?.wired_to || []).map(displayPeer).filter(Boolean);
  return /* @__PURE__ */ (0, import_jsx_runtime25.jsxs)("div", { className: "view roster", "data-testid": "roster-panel", children: [
    /* @__PURE__ */ (0, import_jsx_runtime25.jsxs)("div", { className: "view__head", children: [
      /* @__PURE__ */ (0, import_jsx_runtime25.jsx)("h2", { children: "Roster" }),
      /* @__PURE__ */ (0, import_jsx_runtime25.jsxs)("span", { className: "view__sub", children: [
        rows.length,
        " of ",
        agents.length,
        " agents"
      ] }),
      /* @__PURE__ */ (0, import_jsx_runtime25.jsx)("span", { className: "view__spacer" }),
      /* @__PURE__ */ (0, import_jsx_runtime25.jsx)(
        "input",
        {
          className: "view__search",
          placeholder: "Filter agents, profiles, ids\u2026",
          value: q,
          onChange: (e) => setQ(e.target.value)
        }
      ),
      /* @__PURE__ */ (0, import_jsx_runtime25.jsx)("div", { className: "view__segs", children: ROLE_BUCKETS.map((r2) => /* @__PURE__ */ (0, import_jsx_runtime25.jsx)("button", { className: role === r2 ? "is-active" : "", onClick: () => setRole(r2), children: r2 }, r2)) })
    ] }),
    /* @__PURE__ */ (0, import_jsx_runtime25.jsxs)("div", { className: "roster__body", children: [
      /* @__PURE__ */ (0, import_jsx_runtime25.jsxs)("div", { className: "roster__table", children: [
        /* @__PURE__ */ (0, import_jsx_runtime25.jsxs)("div", { className: "roster__row roster__row--head", children: [
          /* @__PURE__ */ (0, import_jsx_runtime25.jsx)("span", { children: "Name" }),
          /* @__PURE__ */ (0, import_jsx_runtime25.jsx)("span", { children: "Role" }),
          /* @__PURE__ */ (0, import_jsx_runtime25.jsx)("span", { children: "State" }),
          /* @__PURE__ */ (0, import_jsx_runtime25.jsx)("span", { children: "Profile" }),
          /* @__PURE__ */ (0, import_jsx_runtime25.jsx)("span", { children: "Gen" }),
          /* @__PURE__ */ (0, import_jsx_runtime25.jsx)("span", { children: "Chk" }),
          /* @__PURE__ */ (0, import_jsx_runtime25.jsx)("span", { children: "Lease" })
        ] }),
        rows.map((r2) => {
          const isSel = active && r2.member_id === active.member_id;
          return /* @__PURE__ */ (0, import_jsx_runtime25.jsxs)(
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
                /* @__PURE__ */ (0, import_jsx_runtime25.jsxs)("span", { className: "roster__name", children: [
                  /* @__PURE__ */ (0, import_jsx_runtime25.jsx)("span", { className: "roster__dot" }),
                  /* @__PURE__ */ (0, import_jsx_runtime25.jsxs)("span", { children: [
                    /* @__PURE__ */ (0, import_jsx_runtime25.jsx)("div", { children: r2.label }),
                    /* @__PURE__ */ (0, import_jsx_runtime25.jsx)("div", { className: "roster__id", children: r2.identity || r2.member_id })
                  ] })
                ] }),
                /* @__PURE__ */ (0, import_jsx_runtime25.jsx)("span", { children: roleOf(r2) }),
                /* @__PURE__ */ (0, import_jsx_runtime25.jsx)("span", { className: "roster__state", children: stateLabel(r2.state) }),
                /* @__PURE__ */ (0, import_jsx_runtime25.jsx)("span", { className: "mono dim", children: r2.role || "\u2014" }),
                /* @__PURE__ */ (0, import_jsx_runtime25.jsx)("span", { className: "mono", children: r2.generation ?? "\u2014" }),
                /* @__PURE__ */ (0, import_jsx_runtime25.jsx)("span", { className: "mono", children: r2.checkpoint_version ?? "\u2014" }),
                /* @__PURE__ */ (0, import_jsx_runtime25.jsx)("span", { className: "mono dim", children: r2.lease_healthy === false ? "unhealthy" : "ok" })
              ]
            },
            r2.member_id
          );
        })
      ] }),
      /* @__PURE__ */ (0, import_jsx_runtime25.jsx)("aside", { className: "roster__detail", children: active && /* @__PURE__ */ (0, import_jsx_runtime25.jsxs)(import_jsx_runtime25.Fragment, { children: [
        /* @__PURE__ */ (0, import_jsx_runtime25.jsxs)("div", { className: "rd__head", children: [
          /* @__PURE__ */ (0, import_jsx_runtime25.jsx)("div", { className: "rd__title", children: active.label }),
          /* @__PURE__ */ (0, import_jsx_runtime25.jsx)("div", { className: "rd__id", children: active.identity || active.member_id }),
          /* @__PURE__ */ (0, import_jsx_runtime25.jsx)("div", { className: "rd__tags", children: [active.role, active.kind, roleOf(active)].filter(Boolean).map((t) => /* @__PURE__ */ (0, import_jsx_runtime25.jsx)("span", { className: "chip", children: String(t) }, String(t))) })
        ] }),
        /* @__PURE__ */ (0, import_jsx_runtime25.jsxs)("dl", { className: "rd__grid", children: [
          /* @__PURE__ */ (0, import_jsx_runtime25.jsx)("dt", { children: "Profile" }),
          /* @__PURE__ */ (0, import_jsx_runtime25.jsx)("dd", { children: active.role || "\u2014" }),
          /* @__PURE__ */ (0, import_jsx_runtime25.jsx)("dt", { children: "Kind" }),
          /* @__PURE__ */ (0, import_jsx_runtime25.jsx)("dd", { children: active.kind || "\u2014" }),
          /* @__PURE__ */ (0, import_jsx_runtime25.jsx)("dt", { children: "Role" }),
          /* @__PURE__ */ (0, import_jsx_runtime25.jsx)("dd", { children: roleOf(active) }),
          /* @__PURE__ */ (0, import_jsx_runtime25.jsx)("dt", { children: "State" }),
          /* @__PURE__ */ (0, import_jsx_runtime25.jsx)("dd", { children: /* @__PURE__ */ (0, import_jsx_runtime25.jsx)("span", { className: "roster__state", children: stateLabel(active.state) }) }),
          /* @__PURE__ */ (0, import_jsx_runtime25.jsx)("dt", { children: "Member" }),
          /* @__PURE__ */ (0, import_jsx_runtime25.jsx)("dd", { className: "mono", children: active.member_id }),
          /* @__PURE__ */ (0, import_jsx_runtime25.jsx)("dt", { children: "Identity" }),
          /* @__PURE__ */ (0, import_jsx_runtime25.jsx)("dd", { className: "mono", children: active.identity || "\u2014" }),
          /* @__PURE__ */ (0, import_jsx_runtime25.jsx)("dt", { children: "Session" }),
          /* @__PURE__ */ (0, import_jsx_runtime25.jsx)("dd", { className: "mono", children: active.session_id || "\u2014" }),
          /* @__PURE__ */ (0, import_jsx_runtime25.jsx)("dt", { children: "Generation" }),
          /* @__PURE__ */ (0, import_jsx_runtime25.jsx)("dd", { className: "mono", children: active.generation ?? "\u2014" }),
          /* @__PURE__ */ (0, import_jsx_runtime25.jsx)("dt", { children: "Checkpoint" }),
          /* @__PURE__ */ (0, import_jsx_runtime25.jsx)("dd", { className: "mono", children: active.checkpoint_version ?? "\u2014" }),
          /* @__PURE__ */ (0, import_jsx_runtime25.jsx)("dt", { children: "Lease" }),
          /* @__PURE__ */ (0, import_jsx_runtime25.jsx)("dd", { className: "mono", children: active.lease_healthy === false ? "unhealthy" : "ok" }),
          /* @__PURE__ */ (0, import_jsx_runtime25.jsx)("dt", { children: "Wired" }),
          /* @__PURE__ */ (0, import_jsx_runtime25.jsx)("dd", { children: activePeers.length > 0 ? /* @__PURE__ */ (0, import_jsx_runtime25.jsx)("span", { className: "rd__peers", children: activePeers.map((peer) => /* @__PURE__ */ (0, import_jsx_runtime25.jsx)("span", { className: "chip", children: peer }, peer)) }) : /* @__PURE__ */ (0, import_jsx_runtime25.jsx)("span", { className: "mono dim", children: "none" }) })
        ] }),
        /* @__PURE__ */ (0, import_jsx_runtime25.jsxs)("div", { className: "rd__actions", children: [
          actionVisibility?.inspect !== false ? /* @__PURE__ */ (0, import_jsx_runtime25.jsx)("button", { onClick: () => onDetails(active), children: actionLabels?.inspect || "Details" }) : null,
          actionVisibility?.chat !== false ? /* @__PURE__ */ (0, import_jsx_runtime25.jsx)("button", { onClick: () => onChat(active), children: actionLabels?.chat || "Open chat" }) : null,
          actionVisibility?.respawn !== false && active.affordances?.can_respawn ? /* @__PURE__ */ (0, import_jsx_runtime25.jsx)("button", { onClick: () => onLifecycle(activeIdentity, "mobkit/respawn"), children: actionLabels?.respawn || "Respawn" }) : null,
          actionVisibility?.reset !== false && canResetLifecycle ? /* @__PURE__ */ (0, import_jsx_runtime25.jsx)("button", { onClick: () => onLifecycle(activeIdentity, "mobkit/reset"), children: actionLabels?.reset || "Reset" }) : null,
          actionVisibility?.retire !== false && active.affordances?.can_retire ? /* @__PURE__ */ (0, import_jsx_runtime25.jsx)("button", { className: "danger", onClick: () => onLifecycle(activeIdentity, "mobkit/retire"), children: actionLabels?.retire || "Retire" }) : null
        ] })
      ] }) })
    ] })
  ] });
}

// src/panels/RoutingPanel.tsx
var import_react18 = __toESM(require("react"));
var import_jsx_runtime26 = require("react/jsx-runtime");
function RoutingPanel({ data }) {
  const routes = data.routes || [];
  const deliveries = data.deliveries || [];
  const [q, setQ] = import_react18.default.useState("");
  const [sel, setSel] = import_react18.default.useState(routes[0]?.route_key || "");
  const rows = import_react18.default.useMemo(() => {
    if (!q) return routes;
    const needle = q.toLowerCase();
    return routes.filter(
      (r2) => r2.route_key.toLowerCase().includes(needle) || r2.recipient.toLowerCase().includes(needle) || r2.sink.toLowerCase().includes(needle) || r2.target_module.toLowerCase().includes(needle)
    );
  }, [routes, q]);
  const active = rows.find((r2) => r2.route_key === sel) || rows[0];
  const recentDeliveries = deliveries.slice(0, 40);
  const trafficForRoute = (routeKey) => deliveries.filter((d) => d.route_id === routeKey).length;
  return /* @__PURE__ */ (0, import_jsx_runtime26.jsxs)("div", { className: "view routing", "data-testid": "routing-panel", children: [
    /* @__PURE__ */ (0, import_jsx_runtime26.jsxs)("div", { className: "view__head", children: [
      /* @__PURE__ */ (0, import_jsx_runtime26.jsx)("h2", { children: "Routing" }),
      /* @__PURE__ */ (0, import_jsx_runtime26.jsxs)("span", { className: "view__sub", children: [
        rows.length,
        " routes \xB7 ",
        deliveries.length,
        " deliveries (recent)"
      ] }),
      /* @__PURE__ */ (0, import_jsx_runtime26.jsx)("span", { className: "view__spacer" }),
      /* @__PURE__ */ (0, import_jsx_runtime26.jsx)(
        "input",
        {
          className: "view__search",
          placeholder: "Filter route, recipient, sink\u2026",
          value: q,
          onChange: (e) => setQ(e.target.value)
        }
      )
    ] }),
    /* @__PURE__ */ (0, import_jsx_runtime26.jsxs)("div", { className: "routing__body", children: [
      /* @__PURE__ */ (0, import_jsx_runtime26.jsxs)("div", { className: "routing__table", children: [
        /* @__PURE__ */ (0, import_jsx_runtime26.jsxs)("div", { className: "routing__row routing__row--head", children: [
          /* @__PURE__ */ (0, import_jsx_runtime26.jsx)("span", { children: "Route" }),
          /* @__PURE__ */ (0, import_jsx_runtime26.jsx)("span", { children: "Channel" }),
          /* @__PURE__ */ (0, import_jsx_runtime26.jsx)("span", { children: "Recipient" }),
          /* @__PURE__ */ (0, import_jsx_runtime26.jsx)("span", { children: "Sink" }),
          /* @__PURE__ */ (0, import_jsx_runtime26.jsx)("span", { children: "Module" }),
          /* @__PURE__ */ (0, import_jsx_runtime26.jsx)("span", { children: "24h" })
        ] }),
        rows.map((r2) => {
          const isSel = active && r2.route_key === active.route_key;
          return /* @__PURE__ */ (0, import_jsx_runtime26.jsxs)(
            "div",
            {
              className: `routing__row ${isSel ? "is-selected" : ""}`,
              onClick: () => setSel(r2.route_key),
              "data-testid": `routing-route:${r2.route_key}`,
              children: [
                /* @__PURE__ */ (0, import_jsx_runtime26.jsx)("span", { className: "routing__intent mono", children: r2.route_key }),
                /* @__PURE__ */ (0, import_jsx_runtime26.jsx)("span", { className: "mono dim", children: r2.channel || "\u2014" }),
                /* @__PURE__ */ (0, import_jsx_runtime26.jsx)("span", { className: "mono", children: r2.recipient }),
                /* @__PURE__ */ (0, import_jsx_runtime26.jsx)("span", { className: "dim", children: r2.sink }),
                /* @__PURE__ */ (0, import_jsx_runtime26.jsx)("span", { className: "mono dim", children: r2.target_module }),
                /* @__PURE__ */ (0, import_jsx_runtime26.jsx)("span", { className: "mono", children: trafficForRoute(r2.route_key) })
              ]
            },
            r2.route_key
          );
        }),
        rows.length === 0 && /* @__PURE__ */ (0, import_jsx_runtime26.jsx)("div", { style: { padding: 24, color: "var(--ink-dim)", fontFamily: "var(--mono)", fontSize: 12 }, children: "No routes configured." })
      ] }),
      /* @__PURE__ */ (0, import_jsx_runtime26.jsx)("aside", { className: "routing__flow", children: active && /* @__PURE__ */ (0, import_jsx_runtime26.jsxs)(import_jsx_runtime26.Fragment, { children: [
        /* @__PURE__ */ (0, import_jsx_runtime26.jsx)("div", { className: "rf__title", children: "Flow" }),
        /* @__PURE__ */ (0, import_jsx_runtime26.jsxs)("div", { className: "rf__diagram", children: [
          /* @__PURE__ */ (0, import_jsx_runtime26.jsxs)("div", { className: "rf__node rf__node--intent", children: [
            /* @__PURE__ */ (0, import_jsx_runtime26.jsx)("div", { className: "rf__lbl", children: "Route" }),
            /* @__PURE__ */ (0, import_jsx_runtime26.jsx)("div", { className: "rf__val mono", children: active.route_key })
          ] }),
          /* @__PURE__ */ (0, import_jsx_runtime26.jsx)("svg", { className: "rf__arrow", viewBox: "0 0 40 12", children: /* @__PURE__ */ (0, import_jsx_runtime26.jsx)("path", { d: "M0 6 H 34 M 28 2 L 34 6 L 28 10", stroke: "currentColor", fill: "none", strokeWidth: "1" }) }),
          /* @__PURE__ */ (0, import_jsx_runtime26.jsxs)("div", { className: "rf__node rf__node--handler", children: [
            /* @__PURE__ */ (0, import_jsx_runtime26.jsxs)("div", { className: "rf__lbl", children: [
              "via ",
              active.sink
            ] }),
            /* @__PURE__ */ (0, import_jsx_runtime26.jsx)("div", { className: "rf__val mono", children: active.recipient })
          ] }),
          /* @__PURE__ */ (0, import_jsx_runtime26.jsx)("svg", { className: "rf__arrow rf__arrow--drop", viewBox: "0 0 12 40", children: /* @__PURE__ */ (0, import_jsx_runtime26.jsx)("path", { d: "M6 0 V 34 M 2 28 L 6 34 L 10 28", stroke: "currentColor", fill: "none", strokeWidth: "1" }) }),
          /* @__PURE__ */ (0, import_jsx_runtime26.jsxs)("div", { className: "rf__node rf__node--gate", children: [
            /* @__PURE__ */ (0, import_jsx_runtime26.jsx)("div", { className: "rf__lbl", children: "Module" }),
            /* @__PURE__ */ (0, import_jsx_runtime26.jsx)("div", { className: "rf__val mono", children: active.target_module })
          ] })
        ] }),
        /* @__PURE__ */ (0, import_jsx_runtime26.jsxs)("div", { className: "rf__stats", children: [
          /* @__PURE__ */ (0, import_jsx_runtime26.jsxs)("div", { children: [
            /* @__PURE__ */ (0, import_jsx_runtime26.jsx)("dt", { children: "Retry max" }),
            /* @__PURE__ */ (0, import_jsx_runtime26.jsx)("dd", { children: active.retry_max ?? "\u2014" })
          ] }),
          /* @__PURE__ */ (0, import_jsx_runtime26.jsxs)("div", { children: [
            /* @__PURE__ */ (0, import_jsx_runtime26.jsx)("dt", { children: "Backoff" }),
            /* @__PURE__ */ (0, import_jsx_runtime26.jsx)("dd", { children: active.backoff_ms ? `${active.backoff_ms} ms` : "\u2014" })
          ] }),
          /* @__PURE__ */ (0, import_jsx_runtime26.jsxs)("div", { children: [
            /* @__PURE__ */ (0, import_jsx_runtime26.jsx)("dt", { children: "Rate limit" }),
            /* @__PURE__ */ (0, import_jsx_runtime26.jsx)("dd", { children: active.rate_limit_per_minute ? `${active.rate_limit_per_minute}/m` : "\u2014" })
          ] })
        ] }),
        /* @__PURE__ */ (0, import_jsx_runtime26.jsx)("div", { className: "rf__title", style: { marginTop: 12 }, children: "Recent deliveries" }),
        /* @__PURE__ */ (0, import_jsx_runtime26.jsxs)("div", { style: { display: "flex", flexDirection: "column", gap: 4, fontFamily: "var(--mono)", fontSize: 11, color: "var(--ink-muted)" }, children: [
          recentDeliveries.filter((d) => d.route_id === active.route_key).slice(0, 8).map((d) => /* @__PURE__ */ (0, import_jsx_runtime26.jsxs)("div", { "data-testid": `routing-delivery:${d.delivery_id}`, children: [
            /* @__PURE__ */ (0, import_jsx_runtime26.jsx)("span", { style: { color: d.status === "delivered" ? "var(--ok)" : d.status === "failed" ? "var(--crit)" : "var(--warn)" }, children: d.status }),
            " ",
            /* @__PURE__ */ (0, import_jsx_runtime26.jsxs)("span", { className: "dim", children: [
              "\xB7 ",
              d.delivery_id.slice(0, 8)
            ] }),
            " ",
            /* @__PURE__ */ (0, import_jsx_runtime26.jsxs)("span", { children: [
              "\u2192 ",
              d.recipient
            ] })
          ] }, d.delivery_id)),
          recentDeliveries.filter((d) => d.route_id === active.route_key).length === 0 && /* @__PURE__ */ (0, import_jsx_runtime26.jsx)("span", { className: "dim", children: "No recent deliveries." })
        ] })
      ] }) })
    ] })
  ] });
}

// src/panels/LogsPanel.tsx
var import_react19 = __toESM(require("react"));
var import_jsx_runtime27 = require("react/jsx-runtime");
var INTERNAL_LOG_EVENTS = /* @__PURE__ */ new Set([
  "keep-alive",
  "snapshot_complete",
  "snapshot_started",
  "subscribed"
]);
function isLogFrameVisible(frame2) {
  if (INTERNAL_LOG_EVENTS.has(frame2.event)) return false;
  return true;
}
function levelFor(frame2) {
  const ev = frame2.event;
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
var HIDDEN_HISTORY_BLOCK_TYPES = /* @__PURE__ */ new Set([
  "reasoning",
  "server_tool_content",
  "tool_call",
  "tool_result",
  "tool_results",
  "tool_use"
]);
function isRecord(value) {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}
function historyBlockType(record) {
  const raw = record.block_type ?? record.type;
  return typeof raw === "string" ? raw : void 0;
}
function sanitizeLogValue(value) {
  if (Array.isArray(value)) {
    return value.map(sanitizeLogValue).filter((item) => item !== void 0);
  }
  if (!isRecord(value)) return value;
  const blockType = historyBlockType(value);
  if (blockType && HIDDEN_HISTORY_BLOCK_TYPES.has(blockType)) {
    return void 0;
  }
  const clean = {};
  for (const [key, child] of Object.entries(value)) {
    const sanitized = sanitizeLogValue(child);
    if (sanitized !== void 0) clean[key] = sanitized;
  }
  return clean;
}
function sanitizeLogFrameData(data) {
  return sanitizeLogValue(data) ?? null;
}
function textFromContentBlock(value) {
  if (typeof value === "string") {
    const text = value.trim();
    return text ? text : null;
  }
  if (!isRecord(value)) return null;
  for (const key of ["text", "body", "content", "result", "summary"]) {
    const child = value[key];
    if (typeof child === "string" && child.trim()) {
      return child.trim();
    }
  }
  const data = value.data;
  if (isRecord(data)) {
    return textFromContentBlock(data);
  }
  return null;
}
function textFromContent(value) {
  if (Array.isArray(value)) {
    const text = value.map(textFromContentBlock).filter((part) => Boolean(part)).join(" ").replace(/\s+/g, " ").trim();
    return text ? text : null;
  }
  return textFromContentBlock(value);
}
function preferredLogSummary(frame2, data) {
  if (frame2.event === "user_input") {
    const text = textFromContent(data.content ?? data.input ?? data.prompt);
    return text ? `input=${text.slice(0, 120)}` : null;
  }
  for (const key of ["result", "text", "summary", "body", "message_text"]) {
    const value = data[key];
    if (typeof value === "string" && value.trim()) {
      return `${key}=${value.trim().slice(0, 120)}`;
    }
  }
  const contentText = textFromContent(data.content);
  return contentText ? `content=${contentText.slice(0, 120)}` : null;
}
function summarizeLogFrame(frame2) {
  const sanitized = sanitizeLogFrameData(frame2.data);
  const d = isRecord(sanitized) ? sanitized : {};
  const preferred = preferredLogSummary(frame2, d);
  if (preferred) return preferred;
  const bits = [];
  for (const [k, v] of Object.entries(d).filter(([key]) => key !== "message").slice(0, 4)) {
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
function formatFrameData(frame2) {
  const data = sanitizeLogFrameData(frame2.data ?? null);
  if (data === null || data === void 0) return "(no data)";
  try {
    const out = JSON.stringify(data, null, 2);
    if (out.length > 1e4) return out.slice(0, 1e4) + "\n\u2026 (truncated)";
    return out;
  } catch {
    return String(data);
  }
}
function hasStructuredOutput(frame2) {
  const d = frame2.data;
  if (!d || typeof d !== "object") return false;
  return d.structured_output != null;
}
function LogsPanel({ frames }) {
  const [q, setQ] = import_react19.default.useState("");
  const [lvl, setLvl] = import_react19.default.useState("all");
  const [expanded, setExpanded] = import_react19.default.useState(/* @__PURE__ */ new Set());
  const toggle = (key) => setExpanded((prev) => {
    const next = new Set(prev);
    if (next.has(key)) next.delete(key);
    else next.add(key);
    return next;
  });
  const rows = import_react19.default.useMemo(() => {
    return frames.filter(isLogFrameVisible).map((f) => ({ f, level: levelFor(f) })).filter(({ f, level }) => {
      if (lvl !== "all" && level !== lvl) return false;
      if (!q) return true;
      const needle = q.toLowerCase();
      return f.event.toLowerCase().includes(needle) || (f.identity || "").toLowerCase().includes(needle);
    });
  }, [frames, q, lvl]);
  const counts = import_react19.default.useMemo(() => {
    const c2 = { info: 0, warn: 0, error: 0 };
    frames.filter(isLogFrameVisible).forEach((f) => {
      c2[levelFor(f)]++;
    });
    return c2;
  }, [frames]);
  const visibleTotal = counts.info + counts.warn + counts.error;
  return /* @__PURE__ */ (0, import_jsx_runtime27.jsxs)("div", { className: "view logs", "data-testid": "logs-panel", children: [
    /* @__PURE__ */ (0, import_jsx_runtime27.jsxs)("div", { className: "view__head", children: [
      /* @__PURE__ */ (0, import_jsx_runtime27.jsx)("h2", { children: "Logs" }),
      /* @__PURE__ */ (0, import_jsx_runtime27.jsxs)("span", { className: "view__sub", children: [
        rows.length,
        " of ",
        visibleTotal,
        " events \xB7 live"
      ] }),
      /* @__PURE__ */ (0, import_jsx_runtime27.jsx)("span", { className: "view__spacer" }),
      /* @__PURE__ */ (0, import_jsx_runtime27.jsx)(
        "input",
        {
          className: "view__search",
          placeholder: "Filter event, identity\u2026",
          value: q,
          onChange: (e) => setQ(e.target.value)
        }
      ),
      /* @__PURE__ */ (0, import_jsx_runtime27.jsxs)("div", { className: "view__segs", children: [
        /* @__PURE__ */ (0, import_jsx_runtime27.jsxs)("button", { className: lvl === "all" ? "is-active" : "", onClick: () => setLvl("all"), children: [
          "all ",
          /* @__PURE__ */ (0, import_jsx_runtime27.jsx)("span", { className: "n", children: visibleTotal })
        ] }),
        /* @__PURE__ */ (0, import_jsx_runtime27.jsxs)("button", { className: lvl === "info" ? "is-active" : "", onClick: () => setLvl("info"), children: [
          "info ",
          /* @__PURE__ */ (0, import_jsx_runtime27.jsx)("span", { className: "n", children: counts.info })
        ] }),
        /* @__PURE__ */ (0, import_jsx_runtime27.jsxs)("button", { className: `warn ${lvl === "warn" ? "is-active" : ""}`, onClick: () => setLvl("warn"), children: [
          "warn ",
          /* @__PURE__ */ (0, import_jsx_runtime27.jsx)("span", { className: "n", children: counts.warn })
        ] }),
        /* @__PURE__ */ (0, import_jsx_runtime27.jsxs)("button", { className: `bad ${lvl === "error" ? "is-active" : ""}`, onClick: () => setLvl("error"), children: [
          "err ",
          /* @__PURE__ */ (0, import_jsx_runtime27.jsx)("span", { className: "n", children: counts.error })
        ] })
      ] })
    ] }),
    /* @__PURE__ */ (0, import_jsx_runtime27.jsx)("div", { className: "logs__body", children: /* @__PURE__ */ (0, import_jsx_runtime27.jsxs)("div", { className: "logs__stream", children: [
      rows.map(({ f, level }, i) => {
        const key = f.id || `${f.event}:${f.timestampMs}:${i}`;
        const isOpen = expanded.has(key);
        const hasStructured = hasStructuredOutput(f);
        return /* @__PURE__ */ (0, import_jsx_runtime27.jsxs)(
          "div",
          {
            className: `logline logline--${level}${isOpen ? " is-open" : ""}`,
            "data-testid": `log-line:${f.id || i}`,
            children: [
              /* @__PURE__ */ (0, import_jsx_runtime27.jsxs)(
                "button",
                {
                  type: "button",
                  className: "logline__row",
                  onClick: () => toggle(key),
                  "aria-expanded": isOpen,
                  "data-testid": `log-line:${f.id || i}:toggle`,
                  children: [
                    /* @__PURE__ */ (0, import_jsx_runtime27.jsx)("span", { className: "logline__chevron", children: isOpen ? "\u25BE" : "\u25B8" }),
                    /* @__PURE__ */ (0, import_jsx_runtime27.jsx)("span", { className: "logline__t", children: formatTime2(f.timestampMs) }),
                    /* @__PURE__ */ (0, import_jsx_runtime27.jsx)("span", { className: `logline__lvl logline__lvl--${level}`, children: level.toUpperCase() }),
                    /* @__PURE__ */ (0, import_jsx_runtime27.jsx)("span", { className: "logline__src", children: f.identity || "_system" }),
                    /* @__PURE__ */ (0, import_jsx_runtime27.jsx)("span", { className: "logline__evt", children: f.event }),
                    /* @__PURE__ */ (0, import_jsx_runtime27.jsx)("span", { className: "logline__ctx dim", children: f.interactionId ? `int=${f.interactionId.slice(0, 8)}` : "" }),
                    /* @__PURE__ */ (0, import_jsx_runtime27.jsx)("span", { className: "logline__msg", children: summarizeLogFrame(f) }),
                    hasStructured && /* @__PURE__ */ (0, import_jsx_runtime27.jsx)("span", { className: "logline__badge", title: "Carries structured_output", children: "\u21B3 struct" })
                  ]
                }
              ),
              isOpen && /* @__PURE__ */ (0, import_jsx_runtime27.jsx)("pre", { className: "logline__detail", "data-testid": `log-line:${f.id || i}:detail`, children: formatFrameData(f) })
            ]
          },
          key
        );
      }),
      rows.length === 0 && /* @__PURE__ */ (0, import_jsx_runtime27.jsx)("div", { style: { padding: 24, color: "var(--ink-dim)", fontFamily: "var(--mono)", fontSize: 12 }, children: "No matching events." })
    ] }) })
  ] });
}

// src/panels/Topbar.tsx
var import_jsx_runtime28 = require("react/jsx-runtime");
function PanelGlyph({ side, open }) {
  const dividerLeft = side === "left";
  const cx = dividerLeft ? 16.5 : 7.5;
  const point = open ? dividerLeft ? 1 : -1 : dividerLeft ? -1 : 1;
  const x1 = cx + point * 1.6;
  const x22 = cx - point * 1.6;
  return /* @__PURE__ */ (0, import_jsx_runtime28.jsxs)(
    "svg",
    {
      viewBox: "0 0 24 24",
      "aria-hidden": "true",
      focusable: "false",
      children: [
        /* @__PURE__ */ (0, import_jsx_runtime28.jsx)("rect", { x: "3", y: "5", width: "18", height: "14", rx: "1.5" }),
        /* @__PURE__ */ (0, import_jsx_runtime28.jsx)("path", { d: dividerLeft ? "M9 5 L9 19" : "M15 5 L15 19" }),
        /* @__PURE__ */ (0, import_jsx_runtime28.jsx)("path", { d: `M${x1} 9.5 L${x22} 12 L${x1} 14.5` })
      ]
    }
  );
}
function Topbar({
  mobName,
  brandLabel = "MobKit",
  brandLogoUrl,
  brandLogoAlt,
  mobStatus = "idle",
  environment = "dev",
  theme,
  onToggleTheme,
  sidebarCollapsed,
  railCollapsed,
  railVisible = true,
  onToggleSidebar,
  onToggleRail
}) {
  return /* @__PURE__ */ (0, import_jsx_runtime28.jsxs)("div", { className: "mobkit-topbar", "data-testid": "mobkit-topbar", children: [
    /* @__PURE__ */ (0, import_jsx_runtime28.jsx)(
      "button",
      {
        type: "button",
        className: "mobkit-topbar__toggle mobkit-topbar__toggle--left",
        onClick: onToggleSidebar,
        "aria-pressed": !sidebarCollapsed,
        "aria-label": sidebarCollapsed ? "Expand sidebar" : "Collapse sidebar",
        title: sidebarCollapsed ? "Expand sidebar" : "Collapse sidebar",
        "data-testid": "sidebar-collapse-toggle",
        children: /* @__PURE__ */ (0, import_jsx_runtime28.jsx)(PanelGlyph, { side: "left", open: !sidebarCollapsed })
      }
    ),
    /* @__PURE__ */ (0, import_jsx_runtime28.jsxs)("div", { className: "mobkit-topbar__brand", children: [
      brandLogoUrl ? /* @__PURE__ */ (0, import_jsx_runtime28.jsx)("img", { className: "mobkit-topbar__brand-logo", src: brandLogoUrl, alt: brandLogoAlt || brandLabel }) : /* @__PURE__ */ (0, import_jsx_runtime28.jsx)("span", { className: "mobkit-topbar__brand-mark" }),
      /* @__PURE__ */ (0, import_jsx_runtime28.jsx)("span", { children: brandLabel })
    ] }),
    /* @__PURE__ */ (0, import_jsx_runtime28.jsxs)("div", { className: "mobkit-topbar__mob", children: [
      /* @__PURE__ */ (0, import_jsx_runtime28.jsx)("span", { className: "mobkit-topbar__mob-status", title: mobStatus }),
      /* @__PURE__ */ (0, import_jsx_runtime28.jsx)("span", { children: mobName }),
      /* @__PURE__ */ (0, import_jsx_runtime28.jsxs)("span", { className: "dim", children: [
        "\xB7 ",
        mobStatus
      ] })
    ] }),
    /* @__PURE__ */ (0, import_jsx_runtime28.jsxs)("div", { className: "mobkit-topbar__mob", children: [
      /* @__PURE__ */ (0, import_jsx_runtime28.jsx)("span", { children: "env:" }),
      /* @__PURE__ */ (0, import_jsx_runtime28.jsx)("span", { children: environment })
    ] }),
    /* @__PURE__ */ (0, import_jsx_runtime28.jsx)("div", { className: "mobkit-topbar__spacer" }),
    /* @__PURE__ */ (0, import_jsx_runtime28.jsx)("div", { className: "mobkit-topbar__util", children: /* @__PURE__ */ (0, import_jsx_runtime28.jsx)(
      "button",
      {
        type: "button",
        onClick: onToggleTheme,
        "data-testid": "theme-toggle",
        title: `Switch to ${theme === "dark" ? "light" : "dark"} mode`,
        children: theme === "dark" ? "\u2600 light" : "\u263E dark"
      }
    ) }),
    railVisible ? /* @__PURE__ */ (0, import_jsx_runtime28.jsx)(
      "button",
      {
        type: "button",
        className: "mobkit-topbar__toggle mobkit-topbar__toggle--right",
        onClick: onToggleRail,
        "aria-pressed": !railCollapsed,
        "aria-label": railCollapsed ? "Expand signals rail" : "Collapse signals rail",
        title: railCollapsed ? "Expand signals rail" : "Collapse signals rail",
        "data-testid": "signals-rail-collapse-toggle",
        children: /* @__PURE__ */ (0, import_jsx_runtime28.jsx)(PanelGlyph, { side: "right", open: !railCollapsed })
      }
    ) : null
  ] });
}

// src/panels/Tweaks.tsx
var import_react20 = __toESM(require("react"));
var VARIANT_STORAGE = "mobkit-console-variant";
function useConsoleVariant() {
  const [v, setV] = import_react20.default.useState(() => {
    try {
      const stored = localStorage.getItem(VARIANT_STORAGE);
      if (stored === "rams" || stored === "terminal" || stored === "graphite") return stored;
    } catch {
    }
    return "rams";
  });
  const set2 = import_react20.default.useCallback((next) => {
    setV(next);
    try {
      localStorage.setItem(VARIANT_STORAGE, next);
    } catch {
    }
  }, []);
  return [v, set2];
}

// src/panels/Sidebar.tsx
var import_react21 = __toESM(require("react"));
var import_jsx_runtime29 = require("react/jsx-runtime");
var ALL_NAV = ["topology", "timeline", "gating", "roster", "routing", "logs", "health"];
var NAV_LABEL = {
  topology: "Topology",
  timeline: "Today",
  gating: "Approvals",
  roster: "Roster",
  routing: "Routing",
  logs: "Logs",
  health: "Health"
};
function normalizeNavKind(value) {
  return typeof value === "string" && ALL_NAV.includes(value) ? value : null;
}
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
function isWorkerish(a2) {
  const haystack = [a2.label, a2.identity, a2.member_id, a2.role].filter(Boolean).join(" ").toLowerCase();
  return haystack.includes("worker") || haystack.includes("delegate") || haystack.includes("helper");
}
function isCommanderLike(a2) {
  if (isWorkerish(a2)) return false;
  const haystack = [a2.label, a2.identity, a2.member_id, a2.role].filter(Boolean).join(" ").toLowerCase();
  return haystack.includes("commander") || haystack.includes("coordinator");
}
function agentKeys(a2) {
  return [a2?.identity, a2?.member_id, a2?.agent_id].filter((value) => Boolean(value)).map((value) => value.toLowerCase());
}
function referenceMatchesAgentKey(reference, key) {
  const normalizedReference = reference.toLowerCase();
  const normalizedKey = key.toLowerCase();
  if (normalizedReference === normalizedKey) return true;
  const compactReference = normalizedReference.replace(/[^a-z0-9]+/g, "");
  const compactKey = normalizedKey.replace(/[^a-z0-9]+/g, "");
  if (compactKey && compactReference === compactKey) return true;
  const tokens = normalizedReference.split(/[/:#\s]+/).filter(Boolean);
  if (tokens.includes(normalizedKey)) return true;
  if (!compactKey) return false;
  for (let start = 0; start < tokens.length; start++) {
    let compactSlice = "";
    for (let end = start; end < tokens.length; end++) {
      compactSlice += tokens[end].replace(/[^a-z0-9]+/g, "");
      if (compactSlice === compactKey) return true;
      if (compactSlice.length > compactKey.length) break;
    }
  }
  return false;
}
function isWiredTo(a2, host) {
  if (!host) return false;
  const wiredTo = a2.wired_to || [];
  return agentKeys(host).some(
    (key) => wiredTo.some((peer) => referenceMatchesAgentKey(peer, key))
  );
}
function isSpawnedDelegateLike(a2, host) {
  if (!isWorkerish(a2)) return false;
  if (isWiredTo(a2, host)) return true;
  const role = (a2.role || "").toLowerCase();
  const group = (a2.group || "").toLowerCase();
  return !group || group === role || group === "worker" || group === "delegate" || group.includes("helper");
}
function explicitHostId(a2) {
  return a2.labels?.delegate_host_identity || a2.labels?.host_identity || a2.labels?.parent_identity || null;
}
function findSpawnHost(a2, agents, commander) {
  if (!isWorkerish(a2)) return null;
  const explicitHost = explicitHostId(a2);
  if (explicitHost) {
    const match = agents.find(
      (candidate) => candidate.member_id !== a2.member_id && agentKeys(candidate).some((key) => referenceMatchesAgentKey(explicitHost, key))
    );
    if (match) return match;
  }
  const commanderHost = agents.find(
    (candidate) => candidate.member_id !== a2.member_id && isCommanderLike(candidate) && isWiredTo(a2, candidate)
  );
  if (commanderHost) return commanderHost;
  const workerHost = agents.find(
    (candidate) => candidate.member_id !== a2.member_id && isWorkerish(candidate) && isWiredTo(a2, candidate)
  );
  if (workerHost) return workerHost;
  if (commander && commander.member_id !== a2.member_id && isSpawnedDelegateLike(a2, commander)) return commander;
  return null;
}
function bucketOf(a2) {
  const g = (a2.group || "").toLowerCase();
  const p = (a2.role || a2.kind || "").toLowerCase();
  if (g.includes("coordinator") || p.includes("coord") || p.includes("triage") || p.includes("router") || p.includes("commander")) return "Coordinators";
  if (g.includes("personal") || p.includes("personal") || p.includes("identity") || p.includes("lead")) return "Personal";
  if (g.includes("internal") || p.includes("gate") || p.includes("monitor") || p.includes("scribe")) return "Internal";
  if (g.includes("domain") || g.includes("responder") || g.includes("communication") || g.includes("specialist")) return "Domains";
  return "Domains";
}
var SECTION_ORDER = ["Personal", "Coordinators", "Domains", "Internal", "Other"];
function configuredSelectors(config, key) {
  return (config?.[key] || []).map((value) => value.trim()).filter(Boolean);
}
function configuredFieldValue(agent, selector) {
  const normalized = selector.trim();
  if (!normalized) return null;
  if (normalized.startsWith("labels.")) {
    const key = normalized.slice("labels.".length);
    return agent.labels?.[key]?.trim() || null;
  }
  if (normalized.startsWith("label:")) {
    const key = normalized.slice("label:".length);
    return agent.labels?.[key]?.trim() || null;
  }
  switch (normalized) {
    case "group":
      return agent.group?.trim() || null;
    case "subgroup":
      return agent.subgroup?.trim() || null;
    case "role":
      return agent.role?.trim() || null;
    case "kind":
      return agent.kind?.trim() || null;
    case "identity":
      return agent.identity?.trim() || null;
    case "member_id":
      return agent.member_id?.trim() || null;
    case "agent_id":
      return agent.agent_id?.trim() || null;
    default:
      return agent.labels?.[normalized]?.trim() || null;
  }
}
function firstConfiguredValue(agent, selectors) {
  for (const selector of selectors) {
    const value = configuredFieldValue(agent, selector);
    if (value) return value;
  }
  return null;
}
function configuredAgentGroup(agent, config, parentById, byId) {
  const selectors = configuredSelectors(config, "group_by");
  if (selectors.length === 0) return null;
  let current = agent;
  const seen = /* @__PURE__ */ new Set();
  while (current) {
    const value = firstConfiguredValue(current, selectors);
    if (value) return value;
    if (!parentById || !byId || seen.has(current.member_id)) break;
    seen.add(current.member_id);
    const parentId = parentById.get(current.member_id);
    if (!parentId) break;
    current = byId.get(parentId);
  }
  return config?.fallback_group?.trim() || "Agents";
}
function configuredAgentSubgroup(agent, config, parentById, byId) {
  const selectors = configuredSelectors(config, "subgroup_by");
  if (selectors.length === 0) return null;
  let current = agent;
  const seen = /* @__PURE__ */ new Set();
  while (current) {
    const value = firstConfiguredValue(current, selectors);
    if (value) return value;
    if (!parentById || !byId || seen.has(current.member_id)) break;
    seen.add(current.member_id);
    const parentId = parentById.get(current.member_id);
    if (!parentId) break;
    current = byId.get(parentId);
  }
  return config?.fallback_subgroup?.trim() || null;
}
function configuredAgentBadges(agent, config) {
  return (config?.badges || []).map((badge) => {
    const value = configuredFieldValue(agent, badge.field || "");
    if (!badge.id || !badge.label || !value) return null;
    return {
      id: badge.id,
      label: badge.label,
      value,
      tone: badge.tone
    };
  }).filter((badge) => Boolean(badge));
}
function bucketForAgent(a2, parentById, byId) {
  const seen = /* @__PURE__ */ new Set();
  let current = a2;
  while (current) {
    if (seen.has(current.member_id)) break;
    seen.add(current.member_id);
    const parentId = parentById.get(current.member_id);
    if (!parentId) break;
    current = byId.get(parentId);
  }
  return bucketOf(current || a2);
}
function depthForAgent(a2, parentById) {
  const seen = /* @__PURE__ */ new Set();
  let depth = 0;
  let current = a2.member_id;
  while (parentById.has(current) && !seen.has(current)) {
    seen.add(current);
    depth += 1;
    current = parentById.get(current);
  }
  return depth;
}
function compareRows(host, orderSubgroups = false) {
  return (a2, b) => {
    if (orderSubgroups && a2.subgroup !== b.subgroup) {
      if (!a2.subgroup) return 1;
      if (!b.subgroup) return -1;
      return a2.subgroup.localeCompare(b.subgroup);
    }
    if (host) {
      if (a2.agent.member_id === host.member_id) return -1;
      if (b.agent.member_id === host.member_id) return 1;
    }
    if (a2.childOfHost !== b.childOfHost) return a2.childOfHost ? -1 : 1;
    return a2.agent.label.localeCompare(b.agent.label);
  };
}
function orderRowsPreorder(rows, parentById, host, orderSubgroups = false) {
  const byParent = /* @__PURE__ */ new Map();
  const rowById = new Map(rows.map((row) => [row.agent.member_id, row]));
  const roots = [];
  for (const row of rows) {
    const parentId = parentById.get(row.agent.member_id);
    if (parentId && rowById.has(parentId)) {
      if (!byParent.has(parentId)) byParent.set(parentId, []);
      byParent.get(parentId).push(row);
    } else {
      roots.push(row);
    }
  }
  const sortRows = compareRows(host, orderSubgroups);
  roots.sort(sortRows);
  for (const children of byParent.values()) children.sort(sortRows);
  const ordered = [];
  const visit = (row) => {
    ordered.push(row);
    for (const child of byParent.get(row.agent.member_id) || []) visit(child);
  };
  for (const root of roots) visit(root);
  return ordered;
}
function groupSidebarAgents(filtered, config) {
  const g = /* @__PURE__ */ new Map();
  const host = filtered.find(isCommanderLike);
  const byId = new Map(filtered.map((a2) => [a2.member_id, a2]));
  const parentById = /* @__PURE__ */ new Map();
  for (const a2 of filtered) {
    const parent = findSpawnHost(a2, filtered, host || null);
    if (parent) parentById.set(a2.member_id, parent.member_id);
  }
  for (const a2 of filtered) {
    const childOfHost = parentById.has(a2.member_id);
    const configuredGroup = configuredAgentGroup(a2, config, parentById, byId);
    const key = configuredGroup || bucketForAgent(a2, parentById, byId);
    const subgroup = configuredAgentSubgroup(a2, config, parentById, byId);
    if (!g.has(key)) g.set(key, []);
    g.get(key).push({ agent: a2, childOfHost, depth: depthForAgent(a2, parentById), subgroup });
  }
  for (const [key, rows] of g.entries()) {
    g.set(key, orderRowsPreorder(rows, parentById, host || null, configuredSelectors(config, "subgroup_by").length > 0));
  }
  return g;
}
function orderedSectionNames(grouped, config) {
  const names = Array.from(/* @__PURE__ */ new Set([
    ...Array.from(grouped.keys()),
    ...(config?.sections || []).map((section) => section.name).filter(Boolean)
  ]));
  const configuredOrder = (config?.section_order || []).map((value) => value.trim()).filter(Boolean);
  const order = configuredOrder.length > 0 ? configuredOrder : SECTION_ORDER;
  const rank = new Map(order.map((name, index2) => [name.toLowerCase(), index2]));
  return names.sort((a2, b) => {
    const ar = rank.get(a2.toLowerCase()) ?? Number.MAX_SAFE_INTEGER;
    const br = rank.get(b.toLowerCase()) ?? Number.MAX_SAFE_INTEGER;
    if (ar !== br) return ar - br;
    return a2.localeCompare(b);
  });
}
function sectionConfigFor(name, config) {
  const needle = name.toLowerCase();
  return (config?.sections || []).find((section) => section.name?.toLowerCase() === needle) || null;
}
function deriveStateAttr(agent) {
  const state = (agent.state || "").toLowerCase();
  if (state === "retired" || state === "retiring" || state === "stopped") return "retired";
  const degraded = agent.labels?.console_degraded === "true" || state.includes("degrade") || agent.lease_healthy === false;
  if (degraded) return "degraded";
  return "active";
}
function pulseSamples(activity, identity) {
  const bucket = new Array(10).fill(0);
  const now2 = Date.now();
  const window2 = 15 * 60 * 1e3;
  for (const f of activity) {
    if (!f.timestampMs || (f.identity || "") !== identity) continue;
    const age = now2 - f.timestampMs;
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
  customButtons,
  grouping,
  onSelect,
  onOpenControl
}) {
  const [q, setQ] = import_react21.default.useState("");
  const navKinds = import_react21.default.useMemo(() => {
    const configured = visibleNavKinds();
    if (!visibleControls) return configured;
    const allowed = new Set(visibleControls);
    return configured.filter((kind) => allowed.has(kind));
  }, [visibleControls]);
  const filtered = import_react21.default.useMemo(() => {
    if (!q) return agents;
    const needle = q.toLowerCase();
    return agents.filter(
      (a2) => a2.label.toLowerCase().includes(needle) || (a2.identity || "").toLowerCase().includes(needle) || (a2.member_id || "").toLowerCase().includes(needle) || (a2.role || "").toLowerCase().includes(needle)
    );
  }, [agents, q]);
  const grouped = import_react21.default.useMemo(() => {
    return groupSidebarAgents(filtered, grouping);
  }, [filtered, grouping]);
  const sectionNames = import_react21.default.useMemo(() => orderedSectionNames(grouped, grouping), [grouped, grouping]);
  const defaultCollapsedKey = import_react21.default.useMemo(
    () => JSON.stringify((grouping?.sections || []).map((section) => [section.name, section.collapsed === true])),
    [grouping?.sections]
  );
  const [collapsedSections, setCollapsedSections] = import_react21.default.useState(() => {
    return new Set((grouping?.sections || []).filter((section) => section.collapsed === true).map((section) => section.name));
  });
  import_react21.default.useEffect(() => {
    setCollapsedSections(new Set((grouping?.sections || []).filter((section) => section.collapsed === true).map((section) => section.name)));
  }, [defaultCollapsedKey]);
  const customSidebarButtons = import_react21.default.useMemo(
    () => (customButtons || []).filter((button) => button.id && button.label && (button.control || button.href)),
    [customButtons]
  );
  if (collapsed) {
    return /* @__PURE__ */ (0, import_jsx_runtime29.jsx)(
      "aside",
      {
        className: "sidebar sidebar--collapsed",
        "data-collapsed": "true",
        "data-testid": "sidebar-root",
        children: /* @__PURE__ */ (0, import_jsx_runtime29.jsx)("i", { className: "sidebar__grip", "aria-hidden": "true" })
      }
    );
  }
  return /* @__PURE__ */ (0, import_jsx_runtime29.jsxs)("aside", { className: "sidebar", "data-testid": "sidebar-root", children: [
    /* @__PURE__ */ (0, import_jsx_runtime29.jsx)("div", { className: "sidebar__mast", children: /* @__PURE__ */ (0, import_jsx_runtime29.jsxs)("div", { children: [
      /* @__PURE__ */ (0, import_jsx_runtime29.jsx)("div", { className: "sidebar__mast-title", children: "Roster" }),
      /* @__PURE__ */ (0, import_jsx_runtime29.jsxs)("div", { className: "sidebar__mast-sub", children: [
        agents.length,
        " agents"
      ] })
    ] }) }),
    /* @__PURE__ */ (0, import_jsx_runtime29.jsx)("div", { className: "sidebar__search", children: /* @__PURE__ */ (0, import_jsx_runtime29.jsx)(
      "input",
      {
        placeholder: "Search roster...",
        value: q,
        onChange: (e) => setQ(e.target.value),
        "data-testid": "sidebar-search"
      }
    ) }),
    (navKinds.length > 0 || customSidebarButtons.length > 0) && /* @__PURE__ */ (0, import_jsx_runtime29.jsxs)("div", { className: "sidebar__section sidebar__section--nav", children: [
      /* @__PURE__ */ (0, import_jsx_runtime29.jsx)("div", { className: "sidebar__sec-head", children: /* @__PURE__ */ (0, import_jsx_runtime29.jsx)("span", { className: "sidebar__sec-label", children: "Workbench" }) }),
      /* @__PURE__ */ (0, import_jsx_runtime29.jsxs)("div", { className: "sidebar__navgrid", children: [
        navKinds.map((kind) => /* @__PURE__ */ (0, import_jsx_runtime29.jsx)(
          "button",
          {
            className: "sidebar__navitem",
            onClick: () => onOpenControl(kind),
            "data-testid": `nav:${kind}`,
            children: NAV_LABEL[kind]
          },
          kind
        )),
        customSidebarButtons.map((button) => {
          const control = normalizeNavKind(button.control);
          if (control) {
            return /* @__PURE__ */ (0, import_jsx_runtime29.jsx)(
              "button",
              {
                className: "sidebar__navitem",
                onClick: () => onOpenControl(control),
                "data-testid": `nav-custom:${button.id}`,
                title: button.label,
                children: button.label
              },
              button.id
            );
          }
          if (button.href) {
            return /* @__PURE__ */ (0, import_jsx_runtime29.jsx)(
              "a",
              {
                className: "sidebar__navitem",
                href: button.href,
                target: button.target || void 0,
                rel: button.target === "_blank" ? "noreferrer" : void 0,
                "data-testid": `nav-custom:${button.id}`,
                title: button.label,
                children: button.label
              },
              button.id
            );
          }
          return null;
        })
      ] })
    ] }),
    sectionNames.map((bucket) => {
      const list = grouped.get(bucket) || [];
      const sectionConfig = sectionConfigFor(bucket, grouping);
      if (list.length === 0 && !sectionConfig) return null;
      const subgroups = new Set(list.map((row) => row.subgroup).filter((value) => Boolean(value)));
      const showSubgroups = configuredSelectors(grouping, "subgroup_by").length > 0 && subgroups.size > (grouping?.collapse_single_subgroup === false ? 0 : 1);
      let lastSubgroup = null;
      const collapsedSection = collapsedSections.has(bucket);
      return /* @__PURE__ */ (0, import_jsx_runtime29.jsxs)("div", { className: "sidebar__section", "data-collapsed": collapsedSection ? "true" : void 0, children: [
        /* @__PURE__ */ (0, import_jsx_runtime29.jsxs)(
          "button",
          {
            type: "button",
            className: "sidebar__sec-head sidebar__sec-head--button",
            onClick: () => {
              setCollapsedSections((current) => {
                const next = new Set(current);
                if (next.has(bucket)) next.delete(bucket);
                else next.add(bucket);
                return next;
              });
            },
            "data-testid": `sidebar-section-toggle:${bucket}`,
            children: [
              /* @__PURE__ */ (0, import_jsx_runtime29.jsx)("span", { className: "sidebar__sec-label", children: bucket }),
              /* @__PURE__ */ (0, import_jsx_runtime29.jsx)("span", { className: "sidebar__sec-spacer" }),
              /* @__PURE__ */ (0, import_jsx_runtime29.jsx)("span", { className: "sidebar__sec-count", children: list.length })
            ]
          }
        ),
        list.length === 0 && !collapsedSection ? /* @__PURE__ */ (0, import_jsx_runtime29.jsxs)("div", { className: "sidebar__empty", "data-testid": `sidebar-section-empty:${bucket}`, children: [
          sectionConfig?.empty_title ? /* @__PURE__ */ (0, import_jsx_runtime29.jsx)("span", { className: "sidebar__empty-title", children: sectionConfig.empty_title }) : null,
          /* @__PURE__ */ (0, import_jsx_runtime29.jsx)("span", { children: sectionConfig?.empty_text || "No agents in this section." })
        ] }) : null,
        !collapsedSection && list.map(({ agent, childOfHost, depth, subgroup }) => {
          const stateAttr = deriveStateAttr(agent);
          const pulse = pulseSamples(recentActivity, agent.identity || agent.member_id);
          const inbox = inboxCount(agent);
          const badges = configuredAgentBadges(agent, grouping);
          const subgroupHeader = showSubgroups && subgroup && subgroup !== lastSubgroup ? (() => {
            lastSubgroup = subgroup;
            return /* @__PURE__ */ (0, import_jsx_runtime29.jsx)("div", { className: "sidebar__subgroup", children: /* @__PURE__ */ (0, import_jsx_runtime29.jsx)("span", { children: subgroup }) }, `${bucket}:${subgroup}`);
          })() : null;
          return /* @__PURE__ */ (0, import_jsx_runtime29.jsxs)(import_react21.default.Fragment, { children: [
            subgroupHeader,
            /* @__PURE__ */ (0, import_jsx_runtime29.jsxs)(
              "div",
              {
                className: `agent ${childOfHost ? "agent--child" : ""} ${agent.member_id === selectedMemberId ? "is-active" : ""}`,
                "data-state": stateAttr,
                "data-child-of-host": childOfHost ? "true" : void 0,
                "data-depth": childOfHost ? String(Math.min(depth, 3)) : void 0,
                "data-testid": `sidebar-agent:${agent.member_id}`,
                onClick: () => onSelect(agent),
                role: "button",
                tabIndex: 0,
                children: [
                  /* @__PURE__ */ (0, import_jsx_runtime29.jsx)("span", { className: "agent__dot" }),
                  /* @__PURE__ */ (0, import_jsx_runtime29.jsxs)("span", { className: "agent__body", children: [
                    /* @__PURE__ */ (0, import_jsx_runtime29.jsx)("span", { className: "agent__name", children: agent.label }),
                    /* @__PURE__ */ (0, import_jsx_runtime29.jsx)("span", { className: "agent__id", children: agent.identity || agent.member_id }),
                    badges.length > 0 ? /* @__PURE__ */ (0, import_jsx_runtime29.jsx)("span", { className: "agent__badges", children: badges.map((badge) => /* @__PURE__ */ (0, import_jsx_runtime29.jsxs)(
                      "span",
                      {
                        className: "agent__badge",
                        "data-tone": badge.tone || "neutral",
                        title: `${badge.label}: ${badge.value}`,
                        children: [
                          /* @__PURE__ */ (0, import_jsx_runtime29.jsx)("span", { children: badge.label }),
                          /* @__PURE__ */ (0, import_jsx_runtime29.jsx)("strong", { children: badge.value })
                        ]
                      },
                      badge.id
                    )) }) : null
                  ] }),
                  /* @__PURE__ */ (0, import_jsx_runtime29.jsxs)("span", { className: "agent__meta", children: [
                    /* @__PURE__ */ (0, import_jsx_runtime29.jsx)("span", { className: "agent__pulse", children: pulse.map((v, i) => /* @__PURE__ */ (0, import_jsx_runtime29.jsx)("span", { style: { height: `${Math.max(1, Math.min(12, v * 2 + 1))}px` } }, i)) }),
                    inbox > 0 && /* @__PURE__ */ (0, import_jsx_runtime29.jsx)("span", { className: "agent__inbox", children: inbox })
                  ] })
                ]
              }
            )
          ] }, agent.member_id);
        })
      ] }, bucket);
    })
  ] });
}

// src/panels/SignalsRail.tsx
var import_react22 = __toESM(require("react"));
var import_jsx_runtime30 = require("react/jsx-runtime");
var DEFAULT_FILTER_PRESETS = [
  { id: "all", label: "All" },
  { id: "warning", label: "Attn", alertLevels: ["warning", "critical"] },
  { id: "critical", label: "Crit", alertLevels: ["critical"] }
];
var PEER_TOOLS = /* @__PURE__ */ new Set(["send_request", "send_message", "send_response"]);
var LOW_VALUE_REPLIES = /* @__PURE__ */ new Set(["done", "ok", "okay", "acknowledged"]);
var LOW_VALUE_REPLY_PATTERNS = [
  /^acknowledged[.!]?\s+(i[’']?m\s+)?(online|acting as|ready|scribe|incident commander)/i,
  /^acknowledged\b[\s\S]{0,60}\bonline\b/i,
  /^[\w-]+\s+online\b/i,
  /\bonline\.?\s+ready\b/i,
  /\b(is|am)\s+online\s+(as|for)\b/i,
  /\bwill\s+(coordinate|maintain|focus|act|draft)\b/i
];
function recordOf(value) {
  return value && typeof value === "object" ? value : {};
}
function textFromValue(value) {
  if (typeof value === "string") {
    const trimmed = value.trim();
    if (!trimmed) return "";
    try {
      return textFromValue(JSON.parse(trimmed)) || trimmed;
    } catch {
      return trimmed;
    }
  }
  if (Array.isArray(value)) {
    return value.map(textFromValue).filter(Boolean).join(" ").trim();
  }
  if (value && typeof value === "object") {
    const record = value;
    const direct = record.summary ?? record.message ?? record.text ?? record.body ?? record.reply ?? record.result ?? record.content ?? record.subject ?? record.request_subject ?? record.prompt ?? record.description ?? record.token;
    const text = textFromValue(direct);
    if (text) return text;
  }
  return "";
}
function truncate(value, max = 110) {
  const normalized = value.replace(/\*\*([^*]+)\*\*/g, "$1").replace(/`([^`]+)`/g, "$1").replace(/\s+/g, " ").trim();
  if (normalized.length <= max) return normalized;
  return `${normalized.slice(0, Math.max(0, max - 1)).trimEnd()}...`;
}
function displayName(value) {
  if (!value || value === "_system") return "System";
  return value.split(/[-_\s]+/).filter(Boolean).map((part) => `${part.slice(0, 1).toUpperCase()}${part.slice(1)}`).join(" ");
}
function isMeaningfulReply(value) {
  const normalized = value.trim().replace(/[.!]+$/g, "").toLowerCase();
  if (!normalized || LOW_VALUE_REPLIES.has(normalized)) return false;
  return !LOW_VALUE_REPLY_PATTERNS.some((pattern) => pattern.test(value.trim()));
}
function lastSegment(value) {
  return value.split("/").pop() || value;
}
function sessionHistoryAssistantReply(frame2, data) {
  if (frame2.sourceKind !== "session_history") {
    return textFromValue(data.result ?? data.text ?? data.content);
  }
  const message = recordOf(data.message);
  const role = typeof message.role === "string" ? message.role : "";
  if (role !== "block_assistant") {
    return textFromValue(data.result ?? data.text ?? data.content);
  }
  const blocks = Array.isArray(message.blocks) ? message.blocks : [];
  const text = blocks.map((block) => {
    const record = recordOf(block);
    const blockType = typeof record.block_type === "string" ? record.block_type : typeof record.type === "string" ? record.type : "";
    if (blockType !== "text") return "";
    const blockData = recordOf(record.data);
    return textFromValue(blockData.text ?? record.text);
  }).filter(Boolean).join(" ").trim();
  return text;
}
function agentFor(frame2) {
  return frame2.identity?.trim() || "_system";
}
function peerTarget(args) {
  if (typeof args.display_name === "string" && args.display_name.trim()) {
    return lastSegment(args.display_name.trim());
  }
  if (typeof args.to === "string" && args.to.trim()) {
    return lastSegment(args.to.trim());
  }
  return "peer";
}
function isScaffoldRequest(value) {
  return /^You have been spawned as\b/i.test(value.trim());
}
function typedSystemNoticeSignal(data) {
  const blocks = Array.isArray(data.blocks) ? data.blocks : [];
  const comms = blocks.map(recordOf).filter((block) => block.type === "comms");
  if (comms.length === 0) return null;
  const targets = [];
  const details = [];
  let incoming = true;
  for (const block of comms) {
    const peer = recordOf(block.peer);
    const peerLabel = textFromValue(peer.display_name) || textFromValue(peer.id) || "peer";
    targets.push(lastSegment(peerLabel));
    if (block.direction === "outgoing") incoming = false;
    const content = textFromValue(block.content);
    const detail = content || textFromValue(block.summary) || textFromValue(block.intent) || textFromValue(block.payload);
    if (detail) details.push(detail);
  }
  return {
    targets,
    detail: details.join(" "),
    incoming
  };
}
function blobKey(frame2) {
  const data = recordOf(frame2.data);
  const image = recordOf(data.image);
  const blobRef = recordOf(image.blob_ref ?? data.blob_ref);
  const blobId = typeof blobRef.blob_id === "string" ? blobRef.blob_id : typeof data.blob_id === "string" ? data.blob_id : "";
  const imageId = typeof image.image_id === "string" ? image.image_id : typeof data.image_id === "string" ? data.image_id : "";
  return blobId || imageId || frame2.interactionId || frame2.id;
}
function severityOf(frame2) {
  const ev = frame2.event;
  if (ev.includes("fail") || ev.includes("error") || ev.includes("crash")) return "critical";
  if (ev === "gating_decision" || ev.includes("warn") || ev.includes("degraded") || ev.includes("retired")) return "warning";
  return "info";
}
function signalFromFrame(frame2) {
  const data = recordOf(frame2.data);
  const severity = severityOf(frame2);
  const base = {
    id: frame2.id || `${frame2.event}:${frame2.timestampMs || 0}`,
    severity,
    agent: agentFor(frame2),
    at: timeFor(frame2.timestampMs),
    raw: frame2
  };
  if (severity === "critical") {
    return {
      ...base,
      label: frame2.event === "interaction_failed" ? "Agent turn failed" : frame2.event.replace(/_/g, " "),
      detail: truncate(textFromValue(data.error ?? data.reason ?? data.message) || "Needs attention")
    };
  }
  switch (frame2.event) {
    case "user_input":
    case "interaction_started": {
      const request = textFromValue(data.content ?? data.text ?? data.prompt);
      if (!request) return null;
      if (isScaffoldRequest(request)) return null;
      return {
        ...base,
        id: `user:${frame2.id || frame2.interactionId || frame2.timestampMs || request}`,
        label: `You asked ${displayName(base.agent)}`,
        detail: truncate(request)
      };
    }
    case "system_notice": {
      const comms = typedSystemNoticeSignal(data);
      if (!comms) return null;
      const peer = comms.targets.map(displayName).join(", ");
      return {
        ...base,
        id: `comms:${frame2.id || frame2.interactionId || frame2.timestampMs || peer}`,
        label: `${comms.incoming ? "Received from" : "Sent to"} ${peer}`,
        detail: truncate(comms.detail || "Peer comms")
      };
    }
    case "interaction_complete": {
      const reply = sessionHistoryAssistantReply(frame2, data);
      if (!isMeaningfulReply(reply)) return null;
      return {
        ...base,
        label: `${displayName(base.agent)} replied`,
        detail: truncate(reply)
      };
    }
    case "assistant_image":
    case "assistant_image_appended": {
      return {
        ...base,
        id: `image:${blobKey(frame2)}`,
        label: `${displayName(base.agent)} generated image`,
        detail: textFromValue(data.prompt ?? recordOf(data.image).prompt ?? recordOf(data.image).alt) || "Generated image attached"
      };
    }
    case "tool_call_requested": {
      const name = typeof data.name === "string" ? data.name : "";
      if (!PEER_TOOLS.has(name)) return null;
      const args = recordOf(data.args);
      const target = peerTarget(args);
      const body = textFromValue(args.body ?? args.params ?? args.result) || textFromValue(args.intent);
      const verb = name === "send_request" ? "asked" : name === "send_response" ? "replied to" : "sent to";
      return {
        ...base,
        id: `peer:${frame2.id || frame2.interactionId || `${target}:${body}`}`,
        label: `${displayName(base.agent)} ${verb} ${displayName(target)}`,
        detail: truncate(body || "Peer comms")
      };
    }
    case "gating_decision":
      return {
        ...base,
        label: `Gate ${String(data.decision || "decision")}`,
        detail: truncate(textFromValue(data.reason) || "Gating decision recorded")
      };
    case "member_retired":
      return { ...base, label: "Member retired", detail: truncate(textFromValue(data.reason) || "Lifecycle change") };
    case "state_changed":
      return { ...base, label: `State -> ${String(data.state || data.new_state || "changed")}`, detail: base.agent };
    case "route_changed":
      return { ...base, label: "Route changed", detail: truncate(textFromValue(data.reason) || "Routing updated") };
    default:
      return null;
  }
}
function groupKeyFor(signal) {
  const interactionId = signal.raw.interactionId?.trim();
  if (interactionId) return `interaction:${interactionId}`;
  return `single:${signal.id}`;
}
function semanticSignalKey(signal) {
  const canonical = (value) => value.replace(/\+\d+\s+-\d+\b/g, "").replace(/[\u2018\u2019]/g, "'").replace(/[\u201c\u201d]/g, '"').replace(/\s+/g, " ").replace(/[.!?\s]+$/g, "").trim().toLowerCase();
  return [
    canonical(signal.agent),
    canonical(signal.label),
    canonical(signal.detail)
  ].join("\0");
}
function strongerSeverity(a2, b) {
  if (a2 === "critical" || b === "critical") return "critical";
  if (a2 === "warning" || b === "warning") return "warning";
  return "info";
}
function groupSignals(signals) {
  const groups = [];
  const byId = /* @__PURE__ */ new Map();
  for (const signal of signals) {
    const key = groupKeyFor(signal);
    const existing = byId.get(key);
    if (existing) {
      const seenItemKeys = new Set(existing.items.map(semanticSignalKey));
      if (!seenItemKeys.has(semanticSignalKey(signal))) {
        existing.items.push(signal);
      }
      existing.severity = strongerSeverity(existing.severity, signal.severity);
      existing.title = titleForGroup(existing.items);
      existing.detail = detailForGroup(existing.items);
      existing.agent = existing.items[0]?.agent || signal.agent;
      existing.at = existing.items[0]?.at || signal.at;
      continue;
    }
    const group = {
      id: key,
      severity: signal.severity,
      title: titleForGroup([signal]),
      detail: detailForGroup([signal]),
      agent: signal.agent,
      at: signal.at,
      items: [signal]
    };
    byId.set(key, group);
    groups.push(group);
  }
  return groups;
}
function titleForGroup(items) {
  if (items.length === 1) return items[0].label;
  const hasUser = items.some((item) => item.raw.event === "user_input" || item.raw.event === "interaction_started");
  const peerCount = items.filter((item) => item.raw.event === "tool_call_requested").length;
  const replyCount = items.filter((item) => item.raw.event === "interaction_complete").length;
  if (hasUser && (peerCount > 0 || replyCount > 0)) return "Turn activity";
  if (peerCount > 1) return "Peer conversation";
  return `${items.length} related events`;
}
function detailForGroup(items) {
  if (items.length === 1) return items[0].detail;
  const newestReply = items.find((item) => item.raw.event === "interaction_complete");
  const userRequest = items.find((item) => item.raw.event === "user_input" || item.raw.event === "interaction_started");
  return newestReply?.detail || userRequest?.detail || items[0]?.detail || "";
}
function timeFor(tsMs) {
  if (!tsMs) return "--";
  const diff = Date.now() - tsMs;
  if (diff < 6e4) return `${Math.max(1, Math.floor(diff / 1e3))}s`;
  if (diff < 36e5) return `${Math.floor(diff / 6e4)}m`;
  return `${Math.floor(diff / 36e5)}h`;
}
function buildSignalGroupsForTest(frames) {
  const seen = /* @__PURE__ */ new Set();
  const seenSemantic = /* @__PURE__ */ new Set();
  const next = [];
  for (const frame2 of frames.slice(0, 260)) {
    const signal = signalFromFrame(frame2);
    if (!signal) continue;
    if (seen.has(signal.id)) continue;
    seen.add(signal.id);
    const semanticKey = semanticSignalKey(signal);
    if (seenSemantic.has(semanticKey)) continue;
    seenSemantic.add(semanticKey);
    next.push(signal);
    if (next.length >= 80) break;
  }
  return groupSignals(next);
}
function SignalsRail({
  frames,
  collapsed,
  filterPresets,
  activePresetId,
  emptyText,
  watchedIdentities,
  onPresetChange,
  onSelect
}) {
  const presets = import_react22.default.useMemo(() => {
    const configured = (filterPresets || []).filter((preset) => preset.id && preset.label);
    return configured.length > 0 ? configured : DEFAULT_FILTER_PRESETS;
  }, [filterPresets]);
  const [filter, setFilter] = import_react22.default.useState(activePresetId || presets[0]?.id || "all");
  const [expandedGroups, setExpandedGroups] = import_react22.default.useState(() => /* @__PURE__ */ new Set());
  import_react22.default.useEffect(() => {
    if (activePresetId && presets.some((preset) => preset.id === activePresetId)) {
      setFilter(activePresetId);
    }
  }, [activePresetId, presets]);
  const groups = import_react22.default.useMemo(() => {
    return buildSignalGroupsForTest(frames);
  }, [frames]);
  function groupMatchesPreset(group, preset) {
    if (preset.watchedOnly) {
      const watched = watchedIdentities || /* @__PURE__ */ new Set();
      const isWatched = group.items.some((item) => {
        const identity = item.raw.identity || "";
        return identity && watched.has(identity);
      });
      if (!isWatched) return false;
    }
    const alertLevels = new Set((preset.alertLevels || []).map((level) => level.toLowerCase()));
    if (alertLevels.size > 0 && !alertLevels.has(group.severity)) return false;
    return true;
  }
  const activePreset = presets.find((preset) => preset.id === filter) || presets[0] || DEFAULT_FILTER_PRESETS[0];
  const counts = import_react22.default.useMemo(() => {
    return new Map(presets.map((preset) => [
      preset.id,
      groups.filter((group) => groupMatchesPreset(group, preset)).length
    ]));
  }, [groups, presets, watchedIdentities]);
  const shown = groups.filter((group) => groupMatchesPreset(group, activePreset));
  const recent15m = groups.filter((s) => Date.now() - (s.items[0]?.raw.timestampMs || 0) < 15 * 60 * 1e3).length;
  function toggleGroup(group) {
    if (group.items.length <= 1) {
      onSelect?.(group.items[0].raw);
      return;
    }
    setExpandedGroups((prev) => {
      const next = new Set(prev);
      if (next.has(group.id)) next.delete(group.id);
      else next.add(group.id);
      return next;
    });
  }
  if (collapsed) {
    return /* @__PURE__ */ (0, import_jsx_runtime30.jsx)(
      "aside",
      {
        className: "rail rail--collapsed",
        "data-collapsed": "true",
        "data-testid": "signals-rail",
        children: /* @__PURE__ */ (0, import_jsx_runtime30.jsx)("i", { className: "rail__grip", "aria-hidden": "true" })
      }
    );
  }
  return /* @__PURE__ */ (0, import_jsx_runtime30.jsxs)("aside", { className: "rail", "data-testid": "signals-rail", children: [
    /* @__PURE__ */ (0, import_jsx_runtime30.jsxs)("div", { className: "rail__head", children: [
      /* @__PURE__ */ (0, import_jsx_runtime30.jsx)("span", { className: "rail__title", children: "Signals" }),
      /* @__PURE__ */ (0, import_jsx_runtime30.jsxs)("span", { className: "rail__sub", children: [
        recent15m,
        " in 15m"
      ] })
    ] }),
    /* @__PURE__ */ (0, import_jsx_runtime30.jsx)("div", { className: "rail__filters", children: presets.map((preset) => /* @__PURE__ */ (0, import_jsx_runtime30.jsxs)(
      "button",
      {
        className: `rail__filter ${filter === preset.id ? "is-active" : ""}`,
        onClick: () => {
          setFilter(preset.id);
          onPresetChange?.(preset.id);
        },
        "data-testid": `signals-filter:${preset.id}`,
        children: [
          preset.label,
          " ",
          /* @__PURE__ */ (0, import_jsx_runtime30.jsx)("span", { className: "rail__filter-count", children: counts.get(preset.id) || 0 })
        ]
      },
      preset.id
    )) }),
    /* @__PURE__ */ (0, import_jsx_runtime30.jsxs)("div", { className: "rail__list", children: [
      shown.length === 0 && /* @__PURE__ */ (0, import_jsx_runtime30.jsx)("div", { className: "rail__empty", children: emptyText || "No meaningful signals yet." }),
      shown.map((s) => {
        const expanded = expandedGroups.has(s.id);
        return /* @__PURE__ */ (0, import_jsx_runtime30.jsxs)(
          "div",
          {
            className: "signal",
            "data-sev": s.severity,
            "data-testid": `signal:${s.id}`,
            "data-expanded": expanded ? "true" : "false",
            onClick: () => toggleGroup(s),
            role: "button",
            tabIndex: 0,
            children: [
              /* @__PURE__ */ (0, import_jsx_runtime30.jsx)("span", { className: "signal__bar" }),
              /* @__PURE__ */ (0, import_jsx_runtime30.jsxs)("span", { className: "signal__body", children: [
                /* @__PURE__ */ (0, import_jsx_runtime30.jsxs)("span", { className: "signal__label", children: [
                  s.items.length > 1 && /* @__PURE__ */ (0, import_jsx_runtime30.jsx)("span", { className: "signal__chevron", children: expanded ? "\u25BE" : "\u25B8" }),
                  s.title,
                  s.items.length > 1 && /* @__PURE__ */ (0, import_jsx_runtime30.jsx)("span", { className: "signal__count", children: s.items.length })
                ] }),
                /* @__PURE__ */ (0, import_jsx_runtime30.jsx)("span", { className: "signal__detail", children: s.detail }),
                /* @__PURE__ */ (0, import_jsx_runtime30.jsx)("span", { className: "signal__agent", children: s.agent }),
                s.items.length > 1 && expanded && /* @__PURE__ */ (0, import_jsx_runtime30.jsx)("span", { className: "signal__events", children: s.items.map((item) => /* @__PURE__ */ (0, import_jsx_runtime30.jsxs)(
                  "button",
                  {
                    className: "signal__event",
                    type: "button",
                    onClick: (event) => {
                      event.stopPropagation();
                      onSelect?.(item.raw);
                    },
                    children: [
                      /* @__PURE__ */ (0, import_jsx_runtime30.jsx)("span", { className: "signal__event-label", children: item.label }),
                      /* @__PURE__ */ (0, import_jsx_runtime30.jsx)("span", { className: "signal__event-detail", children: item.detail })
                    ]
                  },
                  item.id
                )) })
              ] }),
              /* @__PURE__ */ (0, import_jsx_runtime30.jsx)("span", { className: "signal__meta", children: /* @__PURE__ */ (0, import_jsx_runtime30.jsx)("span", { className: "signal__time", children: s.at }) })
            ]
          },
          s.id
        );
      })
    ] })
  ] });
}

// src/panels/ChatPane.tsx
var import_react23 = __toESM(require("react"));

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
var import_jsx_runtime31 = require("react/jsx-runtime");
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
function parseTimeMs(iso) {
  if (!iso) return null;
  const ms = Date.parse(iso);
  return Number.isFinite(ms) ? ms : null;
}
function formatWorkedDuration(ms) {
  const totalSeconds = Math.max(0, Math.round(ms / 1e3));
  if (totalSeconds < 1) return "under 1s";
  if (totalSeconds < 60) return `${totalSeconds}s`;
  const totalMinutes = Math.floor(totalSeconds / 60);
  const seconds = totalSeconds % 60;
  if (totalMinutes < 60) {
    return seconds ? `${totalMinutes}m ${seconds}s` : `${totalMinutes}m`;
  }
  const hours = Math.floor(totalMinutes / 60);
  const minutes = totalMinutes % 60;
  return minutes ? `${hours}h ${minutes}m` : `${hours}h`;
}
function msgCopyText(message) {
  if (message.text) return message.text.trim();
  return conversationRichBlocksToText(message.blocks).trim();
}
function msgHasTextualPayload(message) {
  if (message.text?.trim()) return true;
  return Boolean(message.blocks?.some((block) => block.type === "paragraph" || block.type === "heading" || block.type === "divider" || block.type === "code" || block.type === "command"));
}
function transcriptCopyText(messages) {
  return messages.map((message) => {
    const text = msgCopyText(message);
    if (!text) return "";
    const label = message.kind === "user" ? "You" : message.kind === "agent" ? message.who || "Agent" : message.kind.toUpperCase();
    const time = message.time ? `[${message.time}] ` : "";
    const worked = message.workedFor ? `
Worked for ${message.workedFor}` : "";
    return `${time}${label}: ${text}${worked}`.trim();
  }).filter(Boolean).join("\n\n");
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
      createdAt: entry.createdAt,
      text: `${entry.title} (+${entry.plus}/-${entry.minus})`
    }];
  }
  if (entry.variant === "meta") {
    return [{
      id: entry.id,
      kind: "origin",
      time: formatTime3(entry.createdAt),
      createdAt: entry.createdAt,
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
        createdAt: entry.createdAt,
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
      createdAt: entry.createdAt,
      who: isUser ? void 0 : label,
      text: ""
    }];
  }
  return [{
    id: entry.id,
    kind: isUser ? "user" : "agent",
    time,
    createdAt: entry.createdAt,
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
function CopyInlineButton({
  text,
  label,
  className = ""
}) {
  const [copied, setCopied] = import_react23.default.useState(false);
  const disabled = !text.trim();
  async function copy() {
    if (disabled) return;
    try {
      await navigator.clipboard.writeText(text);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1400);
    } catch {
    }
  }
  return /* @__PURE__ */ (0, import_jsx_runtime31.jsx)(
    "button",
    {
      "aria-label": copied ? "Copied" : label,
      className: `msg__copy ${className}`,
      "data-copied": copied ? "true" : void 0,
      disabled,
      onClick: (event) => {
        event.stopPropagation();
        void copy();
      },
      title: copied ? "Copied" : label,
      type: "button",
      children: copied ? "\u2713" : "\u2398"
    }
  );
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
  inspectLabel = "Details",
  respawnLabel = "Respawn",
  retireLabel = "Retire",
  sendLabel = "Send",
  stackSlot
}) {
  const bodyRef = import_react23.default.useRef(null);
  import_react23.default.useEffect(() => {
    const resetTranscriptScroll = () => {
      if (bodyRef.current) {
        bodyRef.current.scrollTop = bodyRef.current.scrollHeight;
        bodyRef.current.scrollLeft = 0;
      }
    };
    resetTranscriptScroll();
    const frame2 = window.requestAnimationFrame(resetTranscriptScroll);
    return () => window.cancelAnimationFrame(frame2);
  }, [entries.length, phase]);
  const messages = import_react23.default.useMemo(() => {
    const flat = entries.flatMap(flattenEntry);
    const merged = [];
    for (const m2 of flat) {
      const last = merged[merged.length - 1];
      const lastBlocks = last?.blocks;
      const mBlocks = m2.blocks;
      const sameName = !!(last && last.kind === "tool" && m2.kind === "tool" && Array.isArray(lastBlocks) && lastBlocks.length > 0 && Array.isArray(mBlocks) && mBlocks.length > 0 && lastBlocks.every((b) => b.type === "tool-call") && mBlocks.every((b) => b.type === "tool-call") && lastBlocks[0].type === "tool-call" && mBlocks[0].type === "tool-call" && lastBlocks.every((b) => b.type === "tool-call" && b.name === mBlocks[0].name) && mBlocks.every((b) => b.type === "tool-call" && b.name === mBlocks[0].name));
      const peerCompatible = !sameName ? false : !mBlocks[0].peerTarget ? true : Boolean(lastBlocks[0].peerIncoming) === Boolean(mBlocks[0].peerIncoming);
      if (sameName && peerCompatible && last && lastBlocks && mBlocks) {
        last.blocks = [...lastBlocks, ...mBlocks];
        last.id = `${last.id}+${m2.id}`;
      } else {
        const canDedupeAdjacent = m2.kind === "user" && last?.kind === "user" || m2.kind === "agent" && last?.kind === "agent" && last.who === m2.who;
        if (last && canDedupeAdjacent) {
          const lastSignature = textSignatureForMsg(last);
          const nextSignature = textSignatureForMsg(m2);
          if (lastSignature && lastSignature === nextSignature) {
            continue;
          }
        }
        merged.push({ ...m2 });
      }
    }
    let pendingUserStartedAt = null;
    return merged.map((message) => {
      if (message.kind === "user") {
        pendingUserStartedAt = parseTimeMs(message.createdAt);
        return message;
      }
      if (message.kind !== "agent" || !msgHasTextualPayload(message)) {
        return message;
      }
      const finishedAt = parseTimeMs(message.createdAt);
      if (pendingUserStartedAt === null || finishedAt === null || finishedAt < pendingUserStartedAt) {
        return message;
      }
      const workedFor = formatWorkedDuration(finishedAt - pendingUserStartedAt);
      pendingUserStartedAt = null;
      return {
        ...message,
        workedFor,
        workedForCopyText: `Worked for ${workedFor}`
      };
    });
  }, [entries]);
  const transcriptText = import_react23.default.useMemo(() => transcriptCopyText(messages), [messages]);
  const initial = (agentLabel || "?").trim().charAt(0).toUpperCase() || "?";
  const state = (agent?.state || "unknown").toLowerCase();
  const canAttachImages = agent?.model_capabilities?.image_input === true;
  const [dragActive, setDragActive] = import_react23.default.useState(false);
  const [attachmentError, setAttachmentError] = import_react23.default.useState(null);
  const resolvedDraftBlobRefs = import_react23.default.useRef("");
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
  import_react23.default.useEffect(() => {
    if (!canAttachImages) return;
    const refs = consoleBlobReferencesFromText(draft);
    if (refs.length === 0) {
      resolvedDraftBlobRefs.current = "";
      return;
    }
    const signature = refs.map((ref) => ref.href).join("\n");
    if (signature === resolvedDraftBlobRefs.current) return;
    let cancelled = false;
    const timer2 = window.setTimeout(() => {
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
      window.clearTimeout(timer2);
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
        onDraftChange("");
        staged.forEach((item) => URL.revokeObjectURL(item.previewUrl));
        onStagedChange([]);
        setAttachmentError(null);
      }
    } catch {
      setAttachmentError("send failed; images retained");
    }
  }
  return /* @__PURE__ */ (0, import_jsx_runtime31.jsxs)("div", { className: "conv", "data-testid": `chat-pane:${identity}`, children: [
    /* @__PURE__ */ (0, import_jsx_runtime31.jsxs)("div", { className: "conv__head", children: [
      /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("div", { className: "conv__avatar", children: initial }),
      /* @__PURE__ */ (0, import_jsx_runtime31.jsxs)("div", { style: { minWidth: 0 }, children: [
        /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("div", { className: "conv__title", children: agentLabel }),
        /* @__PURE__ */ (0, import_jsx_runtime31.jsxs)("div", { className: "conv__identity", children: [
          identity,
          agent?.role ? ` \xB7 ${agent.role}` : ""
        ] })
      ] }),
      /* @__PURE__ */ (0, import_jsx_runtime31.jsxs)("div", { className: "conv__actions", children: [
        onInspect ? /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("button", { className: "conv__action", onClick: onInspect, "data-testid": "conv-action:details", children: inspectLabel }) : null,
        agent?.affordances?.can_respawn && onRespawn ? /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("button", { className: "conv__action", onClick: onRespawn, "data-testid": "conv-action:respawn", children: respawnLabel }) : null,
        agent?.affordances?.can_retire && onRetire ? /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("button", { className: "conv__action", onClick: onRetire, "data-testid": "conv-action:retire", children: retireLabel }) : null
      ] })
    ] }),
    /* @__PURE__ */ (0, import_jsx_runtime31.jsxs)(
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
          /* @__PURE__ */ (0, import_jsx_runtime31.jsx)(
            CopyInlineButton,
            {
              className: "msg__copy--transcript",
              label: "Copy transcript",
              text: transcriptText
            }
          ),
          messages.length === 0 && /* @__PURE__ */ (0, import_jsx_runtime31.jsxs)("div", { className: "msg msg--origin", children: [
            /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("div", { className: "msg__time" }),
            /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("div", { className: "msg__bubble", children: /* @__PURE__ */ (0, import_jsx_runtime31.jsxs)("span", { className: "msg__text", children: [
              "No messages yet. Say hello to ",
              agentLabel,
              "."
            ] }) })
          ] }),
          messages.map((m2) => /* @__PURE__ */ (0, import_jsx_runtime31.jsxs)("div", { className: `msg msg--${m2.kind}`, children: [
            /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("div", { className: "msg__time", children: m2.time }),
            /* @__PURE__ */ (0, import_jsx_runtime31.jsxs)("div", { className: "msg__bubble", children: [
              (m2.kind === "user" || m2.kind === "agent") && /* @__PURE__ */ (0, import_jsx_runtime31.jsx)(CopyInlineButton, { label: `Copy ${m2.kind === "user" ? "message" : "turn"}`, text: msgCopyText(m2) }),
              m2.blocks && m2.blocks.length > 0 ? /* @__PURE__ */ (0, import_jsx_runtime31.jsx)(ConversationRichContent, { blocks: m2.blocks }) : m2.text && /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("span", { className: "msg__text", children: m2.text }),
              m2.workedFor && /* @__PURE__ */ (0, import_jsx_runtime31.jsxs)("div", { className: "msg__worked", children: [
                /* @__PURE__ */ (0, import_jsx_runtime31.jsxs)("span", { children: [
                  "Worked for ",
                  m2.workedFor
                ] }),
                /* @__PURE__ */ (0, import_jsx_runtime31.jsx)(
                  CopyInlineButton,
                  {
                    className: "msg__copy--inline",
                    label: "Copy work time",
                    text: m2.workedForCopyText || `Worked for ${m2.workedFor}`
                  }
                )
              ] })
            ] })
          ] }, m2.id)),
          phase && /* @__PURE__ */ (0, import_jsx_runtime31.jsxs)(
            "div",
            {
              className: "msg msg--typing",
              "data-testid": `chat-typing:${identity}`,
              "aria-live": "polite",
              "aria-label": `${agentLabel} is ${phaseLabel(phase)}`,
              children: [
                /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("div", { className: "msg__time" }),
                /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("div", { className: "msg__bubble", children: /* @__PURE__ */ (0, import_jsx_runtime31.jsxs)("span", { className: "msg__typing", children: [
                  /* @__PURE__ */ (0, import_jsx_runtime31.jsxs)("span", { className: "msg__typing-dots", "aria-hidden": "true", children: [
                    /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("span", {}),
                    /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("span", {}),
                    /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("span", {})
                  ] }),
                  /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("span", { className: "msg__typing-label", children: phaseLabel(phase) })
                ] }) })
              ]
            }
          )
        ]
      }
    ),
    stackSlot,
    /* @__PURE__ */ (0, import_jsx_runtime31.jsxs)("div", { className: "composer", children: [
      /* @__PURE__ */ (0, import_jsx_runtime31.jsxs)(
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
            staged.length > 0 && /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("div", { className: "composer__attachments", children: staged.map((item) => /* @__PURE__ */ (0, import_jsx_runtime31.jsxs)("div", { className: "composer__attachment", children: [
              /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("img", { alt: "", src: item.previewUrl }),
              /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("button", { "aria-label": "Remove attachment", onClick: () => removeAttachment(item.id), type: "button", children: "\xD7" })
            ] }, item.id)) }),
            /* @__PURE__ */ (0, import_jsx_runtime31.jsx)(
              "textarea",
              {
                placeholder: `Message ${agentLabel}\u2026`,
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
            /* @__PURE__ */ (0, import_jsx_runtime31.jsxs)("div", { className: "composer__row", children: [
              /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("span", { className: "composer__chip mono", children: agent?.role || "agent" }),
              /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("span", { className: "composer__spacer" }),
              /* @__PURE__ */ (0, import_jsx_runtime31.jsxs)(
                "button",
                {
                  className: "composer__send",
                  disabled: !draft.trim() && staged.length === 0 || staged.length > 0 && !canAttachImages || sending,
                  onClick: submitComposer,
                  "data-testid": `chat-send:${identity}`,
                  children: [
                    sendLabel,
                    "  \u23CE"
                  ]
                }
              )
            ] })
          ]
        }
      ),
      /* @__PURE__ */ (0, import_jsx_runtime31.jsxs)("div", { className: "composer__footer", children: [
        /* @__PURE__ */ (0, import_jsx_runtime31.jsxs)("span", { children: [
          "To: ",
          /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("b", { style: { color: "var(--ink-muted)" }, children: agentLabel })
        ] }),
        /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("span", { children: "\xB7" }),
        /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("span", { className: "mono", children: identity }),
        agent?.role && /* @__PURE__ */ (0, import_jsx_runtime31.jsxs)(import_jsx_runtime31.Fragment, { children: [
          /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("span", { children: "\xB7" }),
          /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("span", { children: agent.role })
        ] }),
        /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("span", { children: "\xB7" }),
        /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("span", { className: "dot", style: {
          background: state === "active" || state === "running" ? "var(--ok)" : state.includes("degrade") ? "var(--warn)" : state === "retired" ? "var(--ink-faint)" : "var(--ink-dim)"
        } }),
        /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("span", { children: state }),
        phase && /* @__PURE__ */ (0, import_jsx_runtime31.jsxs)(import_jsx_runtime31.Fragment, { children: [
          /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("span", { children: "\xB7" }),
          /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("span", { style: { color: "var(--accent)" }, children: phase })
        ] }),
        !canAttachImages && /* @__PURE__ */ (0, import_jsx_runtime31.jsxs)(import_jsx_runtime31.Fragment, { children: [
          /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("span", { children: "\xB7" }),
          /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("span", { children: "model cannot see images" })
        ] }),
        attachmentError && /* @__PURE__ */ (0, import_jsx_runtime31.jsxs)(import_jsx_runtime31.Fragment, { children: [
          /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("span", { children: "\xB7" }),
          /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("span", { style: { color: "var(--bad)" }, children: attachmentError })
        ] })
      ] })
    ] })
  ] });
}

// src/panels/MobKitDock.tsx
var import_react24 = __toESM(require("react"));
var import_jsx_runtime32 = require("react/jsx-runtime");
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
  import_react24.default.useEffect(() => {
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
  return /* @__PURE__ */ (0, import_jsx_runtime32.jsxs)("div", { className: "mkdock", "data-testid": "mkdock", children: [
    /* @__PURE__ */ (0, import_jsx_runtime32.jsxs)("div", { className: "wstabs", children: [
      viewState.tabs.map((t) => {
        const isActive = t.id === activeTab?.id;
        const count = tabPanelCount(t.layout);
        return /* @__PURE__ */ (0, import_jsx_runtime32.jsxs)(
          "div",
          {
            className: `wstab ${isActive ? "is-active" : ""}`,
            onClick: () => onSelectTab(t.id),
            "data-testid": `wstab:${t.id}`,
            children: [
              /* @__PURE__ */ (0, import_jsx_runtime32.jsx)("span", { className: "wstab__mark" }),
              /* @__PURE__ */ (0, import_jsx_runtime32.jsx)("span", { className: "wstab__name", children: t.title || "untitled" }),
              count > 1 && /* @__PURE__ */ (0, import_jsx_runtime32.jsx)("span", { className: "wstab__count", children: count }),
              viewState.tabs.length > 1 && /* @__PURE__ */ (0, import_jsx_runtime32.jsx)(
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
      /* @__PURE__ */ (0, import_jsx_runtime32.jsx)(
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
    /* @__PURE__ */ (0, import_jsx_runtime32.jsx)("div", { className: "dock", children: activeTab && /* @__PURE__ */ (0, import_jsx_runtime32.jsx)(
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
    return /* @__PURE__ */ (0, import_jsx_runtime32.jsx)(PaneView, { panelId: node.panelId, ...props });
  }
  return /* @__PURE__ */ (0, import_jsx_runtime32.jsx)(SplitView, { node, ...props });
}
function SplitView(props) {
  const { node } = props;
  if (node.kind !== "split") return null;
  const ratio = typeof node.ratio === "number" ? Math.max(0.1, Math.min(0.9, node.ratio)) : 0.5;
  const direction = node.direction;
  const style = direction === "horizontal" ? { gridTemplateColumns: `${ratio * 100}% 6px ${(1 - ratio) * 100}%` } : { gridTemplateRows: `${ratio * 100}% 6px ${(1 - ratio) * 100}%` };
  const hostRef = import_react24.default.useRef(null);
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
  return /* @__PURE__ */ (0, import_jsx_runtime32.jsxs)(
    "div",
    {
      ref: hostRef,
      className: `split split--${direction === "horizontal" ? "h" : "v"}`,
      style,
      children: [
        /* @__PURE__ */ (0, import_jsx_runtime32.jsx)(DockLayout, { ...props, node: node.first }),
        /* @__PURE__ */ (0, import_jsx_runtime32.jsx)(
          "div",
          {
            className: `split__handle split__handle--${direction === "horizontal" ? "h" : "v"}`,
            onPointerDown: startDrag,
            "data-testid": `split-handle:${node.id}`
          }
        ),
        /* @__PURE__ */ (0, import_jsx_runtime32.jsx)(DockLayout, { ...props, node: node.second })
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
  const [menuOpen, setMenuOpen] = import_react24.default.useState(false);
  return /* @__PURE__ */ (0, import_jsx_runtime32.jsxs)(
    "div",
    {
      className: `pane ${isFocused ? "is-focused" : ""}`,
      onMouseDown: () => onFocusPanel(panelId),
      "data-testid": `pane:${panelId}`,
      children: [
        /* @__PURE__ */ (0, import_jsx_runtime32.jsxs)("div", { className: "pane__bar", children: [
          /* @__PURE__ */ (0, import_jsx_runtime32.jsxs)(
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
                /* @__PURE__ */ (0, import_jsx_runtime32.jsx)("span", { className: "pane__title-text", children: title }),
                /* @__PURE__ */ (0, import_jsx_runtime32.jsx)("span", { className: "pane__caret", children: "\u25BE" })
              ]
            }
          ),
          subId && /* @__PURE__ */ (0, import_jsx_runtime32.jsx)("span", { className: "pane__id", children: subId }),
          /* @__PURE__ */ (0, import_jsx_runtime32.jsx)("span", { className: "pane__spacer" }),
          /* @__PURE__ */ (0, import_jsx_runtime32.jsx)(
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
          /* @__PURE__ */ (0, import_jsx_runtime32.jsx)(
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
          /* @__PURE__ */ (0, import_jsx_runtime32.jsx)(
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
          menuOpen && /* @__PURE__ */ (0, import_jsx_runtime32.jsx)(
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
        /* @__PURE__ */ (0, import_jsx_runtime32.jsx)("div", { className: "pane__body", children: renderPanelBody({ id: panelId, target }) })
      ]
    }
  );
}
function PaneMenu({ agents, visibleControls, onClose, onPick }) {
  const controls = [
    ["topology", "Topology"],
    ["timeline", "Today"],
    ["gating", "Approvals"],
    ["roster", "Roster"],
    ["routing", "Routing"],
    ["logs", "Logs"],
    ["health", "Health"]
  ].filter(([kind]) => !visibleControls || visibleControls.includes(kind));
  return /* @__PURE__ */ (0, import_jsx_runtime32.jsxs)(import_jsx_runtime32.Fragment, { children: [
    /* @__PURE__ */ (0, import_jsx_runtime32.jsx)("div", { className: "pane-menu__scrim", onMouseDown: onClose }),
    /* @__PURE__ */ (0, import_jsx_runtime32.jsxs)("div", { className: "pane-menu", onMouseDown: (e) => e.stopPropagation(), children: [
      /* @__PURE__ */ (0, import_jsx_runtime32.jsx)("div", { className: "pane-menu__label", children: "Views" }),
      controls.map(([kind, label]) => /* @__PURE__ */ (0, import_jsx_runtime32.jsxs)(
        "button",
        {
          className: "pane-menu__item",
          onClick: () => onPick(buildControlTarget(kind)),
          "data-testid": `pane-menu-view:${kind}`,
          children: [
            /* @__PURE__ */ (0, import_jsx_runtime32.jsx)("span", {}),
            /* @__PURE__ */ (0, import_jsx_runtime32.jsx)("span", { children: label }),
            /* @__PURE__ */ (0, import_jsx_runtime32.jsx)("span", { className: "pane-menu__id", children: "view" })
          ]
        },
        kind
      )),
      /* @__PURE__ */ (0, import_jsx_runtime32.jsx)("div", { className: "pane-menu__sep" }),
      /* @__PURE__ */ (0, import_jsx_runtime32.jsx)("div", { className: "pane-menu__label", children: "Agents" }),
      agents.slice(0, 14).map((a2) => /* @__PURE__ */ (0, import_jsx_runtime32.jsxs)(
        "button",
        {
          className: "pane-menu__item",
          "data-state": (a2.state || "").toLowerCase(),
          onClick: () => onPick(buildDockTarget(a2)),
          "data-testid": `pane-menu-agent:${a2.member_id}`,
          children: [
            /* @__PURE__ */ (0, import_jsx_runtime32.jsx)("span", { className: "agent__dot" }),
            /* @__PURE__ */ (0, import_jsx_runtime32.jsx)("span", { children: a2.label }),
            /* @__PURE__ */ (0, import_jsx_runtime32.jsx)("span", { className: "pane-menu__id", children: a2.identity || a2.member_id })
          ]
        },
        a2.member_id
      ))
    ] })
  ] });
}

// src/panels/PendingStack.tsx
var import_react25 = __toESM(require("react"));
var import_jsx_runtime33 = require("react/jsx-runtime");
function StackHead({
  count,
  agentBusy,
  collapsed,
  onToggleCollapsed,
  onClear
}) {
  return /* @__PURE__ */ (0, import_jsx_runtime33.jsxs)("div", { className: "stack__head", children: [
    /* @__PURE__ */ (0, import_jsx_runtime33.jsx)(
      "button",
      {
        type: "button",
        className: "stack__head-btn",
        onClick: onToggleCollapsed,
        "aria-expanded": !collapsed,
        "aria-label": collapsed ? "Expand pending queue" : "Collapse pending queue",
        title: collapsed ? "Expand queue" : "Collapse queue",
        children: /* @__PURE__ */ (0, import_jsx_runtime33.jsx)("span", { className: "stack__head-chev", children: collapsed ? "\u25B8" : "\u25BE" })
      }
    ),
    /* @__PURE__ */ (0, import_jsx_runtime33.jsx)("span", { children: "Queue" }),
    /* @__PURE__ */ (0, import_jsx_runtime33.jsx)("span", { className: "stack__head-count", children: String(count).padStart(2, "0") }),
    !collapsed && count > 1 && /* @__PURE__ */ (0, import_jsx_runtime33.jsx)("span", { className: "stack__head-hint", children: "\xB7 drains top \u2192 bottom" }),
    /* @__PURE__ */ (0, import_jsx_runtime33.jsx)("span", { className: "stack__head-spacer" }),
    /* @__PURE__ */ (0, import_jsx_runtime33.jsxs)("span", { className: `stack__head-phase ${agentBusy ? "" : "is-idle"}`, children: [
      /* @__PURE__ */ (0, import_jsx_runtime33.jsx)("b", {}),
      agentBusy ? "Agent busy" : "Agent idle \xB7 draining"
    ] }),
    count > 0 && /* @__PURE__ */ (0, import_jsx_runtime33.jsx)(
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
  const m2 = Math.floor(s / 60);
  if (m2 < 60) return `${m2}m`;
  return `${Math.floor(m2 / 60)}h`;
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
  const taRef = import_react25.default.useRef(null);
  const [draft, setDraft] = import_react25.default.useState(item.text);
  import_react25.default.useEffect(() => {
    if (item.editing && taRef.current) {
      taRef.current.focus();
      const len = taRef.current.value.length;
      taRef.current.setSelectionRange(len, len);
      taRef.current.style.height = "auto";
      taRef.current.style.height = taRef.current.scrollHeight + "px";
    }
  }, [item.editing]);
  import_react25.default.useEffect(() => {
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
  return /* @__PURE__ */ (0, import_jsx_runtime33.jsxs)(
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
        /* @__PURE__ */ (0, import_jsx_runtime33.jsxs)("div", { className: "stk-item__lead", children: [
          /* @__PURE__ */ (0, import_jsx_runtime33.jsxs)("span", { className: "stk-item__grip", "aria-label": "Drag to reorder", title: "Drag to reorder", children: [
            /* @__PURE__ */ (0, import_jsx_runtime33.jsx)("span", {}),
            /* @__PURE__ */ (0, import_jsx_runtime33.jsx)("span", {}),
            /* @__PURE__ */ (0, import_jsx_runtime33.jsx)("span", {}),
            /* @__PURE__ */ (0, import_jsx_runtime33.jsx)("span", {}),
            /* @__PURE__ */ (0, import_jsx_runtime33.jsx)("span", {}),
            /* @__PURE__ */ (0, import_jsx_runtime33.jsx)("span", {})
          ] }),
          /* @__PURE__ */ (0, import_jsx_runtime33.jsx)("span", { className: "stk-item__queue-glyph", "aria-hidden": "true", children: "\u2935" })
        ] }),
        item.editing ? /* @__PURE__ */ (0, import_jsx_runtime33.jsxs)("div", { className: "stk-item__edit", children: [
          /* @__PURE__ */ (0, import_jsx_runtime33.jsx)(
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
          /* @__PURE__ */ (0, import_jsx_runtime33.jsxs)("div", { className: "stk-item__edit-row", children: [
            /* @__PURE__ */ (0, import_jsx_runtime33.jsxs)("span", { children: [
              /* @__PURE__ */ (0, import_jsx_runtime33.jsx)("span", { className: "stk-kbd", children: "Esc" }),
              " cancel"
            ] }),
            /* @__PURE__ */ (0, import_jsx_runtime33.jsxs)("span", { children: [
              /* @__PURE__ */ (0, import_jsx_runtime33.jsx)("span", { className: "stk-kbd", children: "\u21B5" }),
              " save \xB7 ",
              /* @__PURE__ */ (0, import_jsx_runtime33.jsx)("span", { className: "stk-kbd", children: "\u21E7\u21B5" }),
              " newline"
            ] }),
            /* @__PURE__ */ (0, import_jsx_runtime33.jsx)("span", { className: "stk-item__edit-spacer" }),
            /* @__PURE__ */ (0, import_jsx_runtime33.jsx)(
              "button",
              {
                type: "button",
                className: "stk-btn",
                onClick: () => onCancelEdit(item.id),
                children: "Cancel"
              }
            ),
            /* @__PURE__ */ (0, import_jsx_runtime33.jsx)(
              "button",
              {
                type: "button",
                className: "stk-btn stk-btn--save",
                onClick: () => onCommitEdit(item.id, draft),
                children: "Save"
              }
            )
          ] })
        ] }) : /* @__PURE__ */ (0, import_jsx_runtime33.jsxs)("div", { className: "stk-item__body", children: [
          /* @__PURE__ */ (0, import_jsx_runtime33.jsx)(
            "div",
            {
              className: `stk-item__text ${item.expanded ? "stk-item__text--expanded" : ""}`,
              onClick: longText ? () => onToggleExpand(item.id) : void 0,
              style: longText ? { cursor: "pointer" } : void 0,
              title: longText && !item.expanded ? item.text : void 0,
              children: item.text
            }
          ),
          /* @__PURE__ */ (0, import_jsx_runtime33.jsxs)("div", { className: "stk-item__meta", children: [
            isHead && /* @__PURE__ */ (0, import_jsx_runtime33.jsx)("span", { className: "stk-item__head-tag", children: "Next" }),
            /* @__PURE__ */ (0, import_jsx_runtime33.jsx)("span", { children: timeAgo(item.addedAt) }),
            item.status === "promoting" && /* @__PURE__ */ (0, import_jsx_runtime33.jsx)("span", { className: "stk-item__sending", children: "SENDING\u2026" })
          ] })
        ] }),
        !item.editing && /* @__PURE__ */ (0, import_jsx_runtime33.jsxs)("div", { className: "stk-item__actions", children: [
          /* @__PURE__ */ (0, import_jsx_runtime33.jsxs)(
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
                /* @__PURE__ */ (0, import_jsx_runtime33.jsx)("span", { className: "stk-btn__glyph", children: "\u21AA" }),
                " Steer"
              ]
            }
          ),
          /* @__PURE__ */ (0, import_jsx_runtime33.jsx)(
            "button",
            {
              type: "button",
              className: "stk-btn stk-btn--icon",
              onClick: () => onEdit(item.id),
              "aria-label": "Edit message",
              title: "Edit message",
              "data-testid": `pending-edit:${item.id}`,
              children: /* @__PURE__ */ (0, import_jsx_runtime33.jsx)("span", { className: "stk-btn__glyph", children: "\u270E" })
            }
          ),
          /* @__PURE__ */ (0, import_jsx_runtime33.jsx)(
            "button",
            {
              type: "button",
              className: "stk-btn stk-btn--icon stk-btn--trash",
              onClick: () => onTrash(item.id),
              "aria-label": "Remove from queue",
              title: "Remove from queue",
              "data-testid": `pending-trash:${item.id}`,
              children: /* @__PURE__ */ (0, import_jsx_runtime33.jsx)("span", { className: "stk-btn__glyph", children: "\xD7" })
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
  const [, setTick] = import_react25.default.useState(0);
  import_react25.default.useEffect(() => {
    const t = window.setInterval(() => setTick((n) => n + 1), 1e4);
    return () => window.clearInterval(t);
  }, []);
  const [dragId, setDragId] = import_react25.default.useState(null);
  const [dropTarget, setDropTarget] = import_react25.default.useState({ id: null, where: null });
  const [collapsed, setCollapsed] = import_react25.default.useState(false);
  const lastCount = import_react25.default.useRef(0);
  import_react25.default.useEffect(() => {
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
  return /* @__PURE__ */ (0, import_jsx_runtime33.jsxs)(
    "section",
    {
      className: `stack ${collapsed ? "is-collapsed" : ""} ${reducedMotion ? "reduced-motion" : ""}`,
      "aria-label": "Pending message queue",
      "data-testid": "pending-stack",
      children: [
        /* @__PURE__ */ (0, import_jsx_runtime33.jsx)(
          StackHead,
          {
            count: items.length,
            agentBusy,
            collapsed,
            onToggleCollapsed: () => setCollapsed((c2) => !c2),
            onClear: onClearAll
          }
        ),
        /* @__PURE__ */ (0, import_jsx_runtime33.jsx)("ol", { className: "stack__list", role: "list", children: items.map((item, i) => /* @__PURE__ */ (0, import_jsx_runtime33.jsx)(
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
var import_jsx_runtime34 = require("react/jsx-runtime");
function normalizeConsoleTheme(value) {
  return value === "dark" || value === "light" ? value : null;
}
function normalizeConsoleVariant(value) {
  return value === "rams" || value === "terminal" || value === "graphite" ? value : null;
}
function normalizeDockPreset(value) {
  return value === "single" || value === "two_columns" || value === "two_rows" || value === "grid" ? value : null;
}
function actionLabel(actions, key, fallback) {
  const value = actions?.[key];
  return typeof value === "string" && value.trim() ? value.trim() : fallback;
}
function actionVisible(actions, key) {
  return actions?.[key] !== false;
}
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
  if (record.type === "image" && (typeof record.src === "string" || typeof record.blobId === "string"))
    return true;
  if (Array.isArray(record.headers) && record.headers.some((v) => String(v || "").trim().length > 0))
    return true;
  if (Array.isArray(record.rows) && record.rows.some(
    (row) => Array.isArray(row) && row.some((v) => String(v || "").trim().length > 0)
  ))
    return true;
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
var DOCK_LAYOUT_STORAGE_PREFIX = "mobkit-console-dock-state";
function createIdempotencyKey() {
  try {
    if (typeof crypto !== "undefined" && typeof crypto.randomUUID === "function") {
      return crypto.randomUUID();
    }
  } catch {
  }
  return `console-${Date.now().toString(36)}-${Math.random().toString(36).slice(2)}`;
}
function dockLayoutStorageKey(baseUrl, experience) {
  const runtimeId = experience?.runtime_id?.trim();
  const title = experience?.console_config?.title?.trim();
  return `${DOCK_LAYOUT_STORAGE_PREFIX}:${runtimeId || title || baseUrl}`;
}
function cursorSeq(cursor) {
  if (!cursor) return null;
  const match = /^console:(\d+)$/.exec(cursor);
  if (!match) return null;
  const parsed = Number(match[1]);
  return Number.isFinite(parsed) ? parsed : null;
}
function isEndTurnFrame(frame2) {
  if (frame2.event !== "turn_completed") return false;
  const data = frame2.data && typeof frame2.data === "object" ? frame2.data : {};
  return data.stop_reason === "end_turn";
}
var REFRESH_TRIGGER_EVENTS = /* @__PURE__ */ new Set([
  "interaction_complete",
  "interaction_failed",
  "state_changed",
  "member_ready",
  "member_retired",
  "topology_updated",
  "gating_decision",
  "route_changed",
  "tool_call_requested",
  "tool_call",
  "tool_result_received",
  "tool_execution_started",
  "tool_execution_completed"
]);
var PANEL_ROUTABLE_EVENTS = /* @__PURE__ */ new Set([
  "user_input",
  "interaction_started",
  "interaction_complete",
  "interaction_failed",
  "assistant_image",
  "assistant_image_appended",
  "text_delta",
  "text_complete",
  "turn_completed",
  "tool_call_requested",
  "tool_call",
  "tool_result_received",
  "tool_execution_started",
  "tool_execution_completed",
  "run_started",
  "run_completed",
  "run_failed",
  "message_delivery_failed"
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
  "run_failed",
  "keep-alive",
  "tool_config_changed",
  "tool_scope_changed",
  "frame_updated",
  "text_delta",
  "tool_call",
  "tool_execution_started",
  "tool_result_received",
  "tool_execution_completed"
]);
function ConsoleApp({ baseUrl }) {
  const [experience, setExperience] = import_react26.default.useState(
    null
  );
  const [agents, setAgents] = import_react26.default.useState([]);
  const [draftByKey, setDraftByKey] = import_react26.default.useState(
    {}
  );
  const [stagedAttachmentsByIdentity, setStagedAttachmentsByIdentity] = import_react26.default.useState({});
  const [sendingPanels, setSendingPanels] = import_react26.default.useState(
    /* @__PURE__ */ new Set()
  );
  const [pinnedAgentIds, setPinnedAgentIds] = import_react26.default.useState(
    /* @__PURE__ */ new Set()
  );
  const [inspectByIdentity, setInspectByIdentity] = import_react26.default.useState({});
  const [routingData, setRoutingData] = import_react26.default.useState({
    routes: [],
    deliveries: []
  });
  const [gatingData, setGatingData] = import_react26.default.useState({
    pending: [],
    audit: []
  });
  const [activeActivityPresetId, setActiveActivityPresetId] = import_react26.default.useState("");
  const [selectedRosterMemberId, setSelectedRosterMemberId] = import_react26.default.useState("");
  const [loading, setLoading] = import_react26.default.useState(true);
  const [error, setError] = import_react26.default.useState("");
  const [theme, setTheme] = import_react26.default.useState(() => {
    try {
      return localStorage.getItem("mobkit-console-theme") || "light";
    } catch {
      return "light";
    }
  });
  const [variant, setVariant] = useConsoleVariant();
  const [sidebarCollapsed, setSidebarCollapsed] = import_react26.default.useState(
    () => {
      try {
        return localStorage.getItem("mobkit-console-sidebar-collapsed") === "1";
      } catch {
        return false;
      }
    }
  );
  const toggleSidebarCollapsed = import_react26.default.useCallback(() => {
    setSidebarCollapsed((c2) => {
      const next = !c2;
      try {
        localStorage.setItem(
          "mobkit-console-sidebar-collapsed",
          next ? "1" : "0"
        );
      } catch {
      }
      return next;
    });
  }, []);
  const [railCollapsed, setRailCollapsed] = import_react26.default.useState(() => {
    try {
      return localStorage.getItem("mobkit-console-rail-collapsed") === "1";
    } catch {
      return false;
    }
  });
  const toggleRailCollapsed = import_react26.default.useCallback(() => {
    setRailCollapsed((c2) => {
      const next = !c2;
      try {
        localStorage.setItem("mobkit-console-rail-collapsed", next ? "1" : "0");
      } catch {
      }
      return next;
    });
  }, []);
  const [, setRenderTick] = import_react26.default.useState(0);
  const forceRender = import_react26.default.useCallback(() => setRenderTick((n) => n + 1), []);
  const stagedAttachmentsRef = import_react26.default.useRef(stagedAttachmentsByIdentity);
  import_react26.default.useEffect(() => {
    stagedAttachmentsRef.current = stagedAttachmentsByIdentity;
  }, [stagedAttachmentsByIdentity]);
  import_react26.default.useEffect(
    () => () => {
      for (const items of Object.values(stagedAttachmentsRef.current)) {
        items.forEach((item) => URL.revokeObjectURL(item.previewUrl));
      }
    },
    []
  );
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
  const identityLogRef = import_react26.default.useRef({});
  const timelineFetchInFlightRef = import_react26.default.useRef(
    {}
  );
  const optimisticUserByPanelKeyRef = import_react26.default.useRef({});
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
    const clearedPanelKeys = [];
    for (const [panelKey, optimistic] of Object.entries(
      optimisticUserByPanelKeyRef.current
    )) {
      if (optimistic.interactionId !== interactionId) continue;
      optimistic.objectUrls?.forEach((url) => URL.revokeObjectURL(url));
      delete optimisticUserByPanelKeyRef.current[panelKey];
      clearedPanelKeys.push(panelKey);
    }
    if (clearedPanelKeys.length > 0) {
      setSendingPanels((current) => {
        const next = new Set(current);
        for (const panelKey of clearedPanelKeys) next.delete(panelKey);
        return next;
      });
    }
  }
  function clearSendingPanelsForIdentity(identity) {
    if (!identity.trim()) return;
    setSendingPanels((current) => {
      let changed = false;
      const next = new Set(current);
      const suffix = `:agent-chat:${identity}`;
      for (const panelKey of current) {
        if (panelKey.endsWith(suffix)) {
          next.delete(panelKey);
          changed = true;
        }
      }
      return changed ? next : current;
    });
  }
  function clearOptimisticUserByContent(identity, frame2) {
    if (frame2.event !== "interaction_started" && frame2.event !== "user_input")
      return;
    const record = frame2.data && typeof frame2.data === "object" ? frame2.data : {};
    const content = typeof record.content === "string" ? record.content.trim() : "";
    if (!content) return;
    const clearedPanelKeys = [];
    for (const [panelKey, optimistic] of Object.entries(
      optimisticUserByPanelKeyRef.current
    )) {
      if (!panelKey.endsWith(`:agent-chat:${identity}`)) continue;
      if (optimistic.interactionId) continue;
      if (!("text" in optimistic.entry) || typeof optimistic.entry.text !== "string")
        continue;
      if (optimistic.entry.text.trim() !== content) continue;
      optimistic.objectUrls?.forEach((url) => URL.revokeObjectURL(url));
      delete optimisticUserByPanelKeyRef.current[panelKey];
      clearedPanelKeys.push(panelKey);
    }
    if (clearedPanelKeys.length > 0) {
      setSendingPanels((current) => {
        const next = new Set(current);
        for (const panelKey of clearedPanelKeys) next.delete(panelKey);
        return next;
      });
    }
  }
  function frameKey(frame2) {
    if (frame2.id) return frame2.id;
    if (frame2.cursor) return frame2.cursor;
    return `${frame2.event}:${frame2.identity || ""}:${frame2.interactionId || ""}:${frame2.timestampMs || 0}`;
  }
  function appendFrame(identity, frame2) {
    const log = getOrCreateLog(identity);
    if (frame2.event === "frame_updated" && frame2.data && typeof frame2.data === "object") {
      const updated = frame2.data.frame;
      if (updated && updated.id) {
        const existingIndex = log.byKey.get(updated.id);
        if (existingIndex !== void 0 && log.events[existingIndex]) {
          const existingVersion = log.events[existingIndex].frameVersion ?? 0;
          const updatedVersion = updated.frameVersion ?? existingVersion;
          if (updatedVersion < existingVersion) return false;
          log.events[existingIndex] = {
            ...log.events[existingIndex],
            ...updated
          };
          return true;
        }
      }
      return false;
    }
    const key = frameKey(frame2);
    if (log.byKey.has(key)) return false;
    log.byKey.set(key, log.events.length);
    log.events.push(frame2);
    if ((frame2.event === "interaction_started" || frame2.event === "user_input") && frame2.interactionId) {
      clearOptimisticUserByInteraction(frame2.interactionId);
    } else {
      clearOptimisticUserByContent(identity, frame2);
    }
    return true;
  }
  function busyTransitionForFrame(frame2) {
    if (frame2.event === "user_input" || frame2.event === "interaction_started" || frame2.event === "run_started") {
      return true;
    }
    if (frame2.event === "interaction_complete" || frame2.event === "interaction_failed" || frame2.event === "run_completed" || frame2.event === "run_failed" || frame2.event === "message_delivery_failed" || isEndTurnFrame(frame2)) {
      return false;
    }
    return null;
  }
  function busyTransitionSortRank(frame2) {
    const transition = busyTransitionForFrame(frame2);
    return transition === false ? 1 : 0;
  }
  function applyBusyState(identity, nextBusy) {
    const wasBusy = identityBusyRef.current[identity] === true;
    identityBusyRef.current[identity] = nextBusy;
    if (wasBusy && !nextBusy) {
      clearSendingPanelsForIdentity(identity);
      maybeDrainHead(identity);
    }
  }
  function updateBusyStateForFrame(identity, frame2) {
    const transition = busyTransitionForFrame(frame2);
    if (transition !== null) {
      applyBusyState(identity, transition);
    }
  }
  function recomputeBusyStateFromLog(identity) {
    const log = getOrCreateLog(identity);
    const lifecycleFrames = log.events.filter((frame2) => busyTransitionForFrame(frame2) !== null).sort((a2, b) => {
      const timeDelta = (a2.timestampMs || 0) - (b.timestampMs || 0);
      if (timeDelta !== 0) return timeDelta;
      const rankDelta = busyTransitionSortRank(a2) - busyTransitionSortRank(b);
      if (rankDelta !== 0) return rankDelta;
      return (a2.cursor || a2.id || "").localeCompare(b.cursor || b.id || "");
    });
    let nextBusy = false;
    for (const frame2 of lifecycleFrames) {
      const transition = busyTransitionForFrame(frame2);
      if (transition !== null) nextBusy = transition;
    }
    applyBusyState(identity, nextBusy);
  }
  function reconcileServerLog(identity, frames, available) {
    const log = getOrCreateLog(identity);
    log.hasServerLog = available;
    for (const frame2 of frames) {
      if (!appendFrame(identity, frame2)) continue;
      updatePhaseForIdentity(identity, frame2);
    }
    recomputeBusyStateFromLog(identity);
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
      if (!next || next === after) break;
      after = next;
    }
    return { frames, available };
  }
  function refreshIdentityTimelineNow(identity, options = {}) {
    const normalized = identity.trim();
    if (!normalized) return Promise.resolve();
    const inFlight = timelineFetchInFlightRef.current[normalized];
    if (inFlight) {
      return inFlight.then(() => {
        if (options.clearPhase) {
          clearPhaseForIdentity(normalized);
          forceRender();
        }
      });
    }
    const request = (async () => {
      const { frames, available } = await queryIdentityTimeline(normalized);
      reconcileServerLog(normalized, frames, available);
      if (options.clearPhase) clearPhaseForIdentity(normalized);
      forceRender();
    })().finally(() => {
      delete timelineFetchInFlightRef.current[normalized];
    });
    timelineFetchInFlightRef.current[normalized] = request;
    return request;
  }
  function getSortedFrames(identity) {
    const log = identityLogRef.current[identity];
    if (!log) return [];
    return log.events.map((frame2, index2) => ({ frame: frame2, index: index2 })).sort((a2, b) => {
      const ca = cursorSeq(a2.frame.cursor);
      const cb = cursorSeq(b.frame.cursor);
      if (ca !== null && cb !== null && ca !== cb) return ca - cb;
      const ta = typeof a2.frame.timestampMs === "number" ? a2.frame.timestampMs : Number.MAX_SAFE_INTEGER;
      const tb = typeof b.frame.timestampMs === "number" ? b.frame.timestampMs : Number.MAX_SAFE_INTEGER;
      if (ta !== tb) return ta - tb;
      return a2.index - b.index;
    }).map((entry) => entry.frame);
  }
  function framesVisibleInPanel(frames, panelId) {
    void panelId;
    return frames;
  }
  const activityRef = import_react26.default.useRef([]);
  const liveFramesRef = import_react26.default.useRef([]);
  const pendingStackRef = import_react26.default.useRef({});
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
      const clean = items.filter(
        (it) => it.status !== "trashing" && it.status !== "draining" && it.status !== "promoting"
      ).map((it) => ({ id: it.id, text: it.text, addedAt: it.addedAt }));
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
  import_react26.default.useEffect(() => {
    const onStorage = (e) => {
      if (!e.key || !e.key.startsWith(PENDING_STACK_KEY_PREFIX)) return;
      const identity = e.key.slice(PENDING_STACK_KEY_PREFIX.length);
      pendingStackRef.current[identity] = loadPendingStack(identity);
      forceRender();
    };
    window.addEventListener("storage", onStorage);
    return () => window.removeEventListener("storage", onStorage);
  }, []);
  const identityBusyRef = import_react26.default.useRef({});
  const isIdentityBusy = (identity) => identityBusyRef.current[identity] === true;
  const phaseRef = import_react26.default.useRef({});
  const phaseValueByKey = import_react26.default.useRef({});
  const phaseSinceByKey = import_react26.default.useRef({});
  const phaseTimerByKey = import_react26.default.useRef({});
  const refreshTimersRef = import_react26.default.useRef({});
  const experienceTimerRef = import_react26.default.useRef(null);
  const agentsRef = import_react26.default.useRef([]);
  import_react26.default.useEffect(() => {
    agentsRef.current = agents;
  }, [agents]);
  const initialTargetOpened = import_react26.default.useRef(false);
  const dockLayoutHydrated = import_react26.default.useRef(false);
  const dockLayoutRestored = import_react26.default.useRef(false);
  const dockLayoutRestoring = import_react26.default.useRef(false);
  const dock = useConsoleDockController({
    createPanelState: ({ target }) => ({
      id: `panel-${crypto.randomUUID()}`,
      target: target || null,
      mode: "console"
    })
  });
  const currentDockLayoutStorageKey = import_react26.default.useMemo(
    () => dockLayoutStorageKey(baseUrl, experience),
    [baseUrl, experience?.runtime_id, experience?.console_config?.title]
  );
  import_react26.default.useEffect(() => {
    if (!experience || dockLayoutHydrated.current) return;
    dockLayoutHydrated.current = true;
    try {
      const raw = localStorage.getItem(currentDockLayoutStorageKey);
      if (!raw) return;
      const parsed = JSON.parse(raw);
      const restored = normalizeConsoleDockState(parsed);
      if (restored.tabs.length === 0 || restored.panels.length === 0) return;
      dockLayoutRestored.current = true;
      dockLayoutRestoring.current = true;
      dock.setState(restored);
    } catch {
    }
  }, [currentDockLayoutStorageKey, experience]);
  import_react26.default.useEffect(() => {
    if (!experience || !dockLayoutHydrated.current) return;
    if (dockLayoutRestoring.current) {
      dockLayoutRestoring.current = false;
      return;
    }
    try {
      localStorage.setItem(
        currentDockLayoutStorageKey,
        JSON.stringify(dock.state)
      );
    } catch {
    }
  }, [currentDockLayoutStorageKey, dock.state, experience]);
  function clearPhaseTimer(panelKey) {
    const timer2 = phaseTimerByKey.current[panelKey];
    if (timer2 !== void 0) {
      window.clearTimeout(timer2);
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
  function updatePanelPhaseFromFrame(panelKey, frame2) {
    const currentPhase = phaseValueByKey.current[panelKey] ?? null;
    const elapsedMs = Date.now() - (phaseSinceByKey.current[panelKey] ?? 0);
    switch (frame2.event) {
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
      case "text_complete":
      case "interaction_complete":
      case "interaction_failed":
      case "run_completed":
      case "run_failed":
        commitPanelPhase(panelKey, null);
        break;
      case "turn_completed":
        if (isEndTurnFrame(frame2)) commitPanelPhase(panelKey, null);
        break;
      case "message_delivery_failed":
        commitPanelPhase(panelKey, null);
        break;
      default:
        break;
    }
  }
  const dockRef = import_react26.default.useRef(dock);
  dockRef.current = dock;
  function updatePhaseForIdentity(identity, frame2) {
    for (const panel of dockRef.current.viewState.panels) {
      const target = panel.target;
      if (!target || target.kind !== "agent-chat") continue;
      if ((target.identity || target.memberId) !== identity) continue;
      updatePanelPhaseFromFrame(
        buildPanelConversationKey(panel.id, target),
        frame2
      );
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
  const loadExperience = import_react26.default.useCallback(async () => {
    const [experienceJson, modulesJson] = await Promise.all([
      fetchJson(baseUrl, "/console/experience"),
      fetchJson(baseUrl, "/console/modules")
    ]);
    const loadedModules = Array.isArray(modulesJson.modules) ? modulesJson.modules.map(String) : [];
    const nextAgents = normalizeAgents(experienceJson, loadedModules);
    setExperience(experienceJson);
    setAgents(nextAgents);
    setActiveActivityPresetId(
      (c2) => c2 || experienceJson.console_config?.rail?.active_preset_id || experienceJson.activity_feed?.active_preset_id || "all"
    );
    return nextAgents;
  }, [baseUrl]);
  import_react26.default.useEffect(() => {
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
  import_react26.default.useEffect(() => {
    const timer2 = window.setInterval(() => {
      void loadExperience().catch(() => {
      });
    }, 1e3);
    return () => window.clearInterval(timer2);
  }, [loadExperience]);
  import_react26.default.useEffect(() => {
    const appearance = experience?.console_config?.appearance;
    if (!appearance) return;
    const configuredTheme = normalizeConsoleTheme(appearance.default_theme);
    if (configuredTheme) {
      try {
        if (!localStorage.getItem("mobkit-console-theme"))
          setTheme(configuredTheme);
      } catch {
        setTheme(configuredTheme);
      }
    }
    const configuredVariant = normalizeConsoleVariant(
      appearance.default_variant
    );
    if (configuredVariant) {
      try {
        if (!localStorage.getItem("mobkit-console-variant"))
          setVariant(configuredVariant);
      } catch {
        setVariant(configuredVariant);
      }
    }
  }, [experience?.console_config?.appearance, setVariant]);
  import_react26.default.useEffect(() => {
    const configured = experience?.console_config?.layout?.sidebar_collapsed;
    if (typeof configured !== "boolean") return;
    try {
      if (localStorage.getItem("mobkit-console-sidebar-collapsed") !== null)
        return;
    } catch {
    }
    setSidebarCollapsed(configured);
  }, [experience?.console_config?.layout?.sidebar_collapsed]);
  import_react26.default.useEffect(() => {
    const configured = experience?.console_config?.rail?.collapsed;
    if (typeof configured !== "boolean") return;
    try {
      if (localStorage.getItem("mobkit-console-rail-collapsed") !== null)
        return;
    } catch {
    }
    setRailCollapsed(configured);
  }, [experience?.console_config?.rail?.collapsed]);
  const hasMobControlSurface = experience?.runtime_id !== "console-aggregator";
  const visibleControls = import_react26.default.useMemo(() => {
    const runtimeControls = hasMobControlSurface ? [
      "topology",
      "timeline",
      "gating",
      "roster",
      "routing",
      "logs",
      "health"
    ] : ["topology", "timeline", "roster", "logs", "health"];
    const sidebarConfig = experience?.console_config?.sidebar;
    const allowedByRuntime = new Set(runtimeControls);
    const configuredVisible = (sidebarConfig?.visible_controls || []).map(normalizeNavKind).filter(
      (kind) => Boolean(kind) && allowedByRuntime.has(kind)
    );
    if (configuredVisible.length > 0) return configuredVisible;
    const hidden = new Set(
      (sidebarConfig?.hidden_controls || []).map(normalizeNavKind).filter((kind) => Boolean(kind))
    );
    return runtimeControls.filter((kind) => !hidden.has(kind));
  }, [experience?.console_config?.sidebar, hasMobControlSurface]);
  import_react26.default.useEffect(() => {
    if (initialTargetOpened.current || dock.focusedTarget || !experience)
      return;
    if (!dockLayoutHydrated.current) return;
    if (dockLayoutRestored.current) {
      initialTargetOpened.current = true;
      return;
    }
    const layoutConfig = experience.console_config?.layout;
    let target = null;
    const configuredControl = normalizeNavKind(layoutConfig?.initial_control);
    if (configuredControl && visibleControls.includes(configuredControl)) {
      target = buildControlTarget(
        configuredControl
      );
    }
    const configuredAgent = layoutConfig?.initial_agent?.trim().toLowerCase();
    if (!target && configuredAgent) {
      const match = agents.find((agent) => {
        return [
          agent.identity,
          agent.member_id,
          agent.agent_id,
          agent.label
        ].some((value) => value?.toLowerCase() === configuredAgent);
      });
      if (match) target = buildDockTarget(match);
    }
    initialTargetOpened.current = true;
    if (!target) return;
    const preset = normalizeDockPreset(layoutConfig?.initial_preset);
    if (preset) dock.applyPreset(preset);
    dock.openTarget(target, "replace_focused");
  }, [agents, dock, experience, visibleControls]);
  import_react26.default.useEffect(() => {
    const target = dock.focusedTarget;
    if (!target || target.kind !== "agent-chat" || agents.length === 0) return;
    const identity = target.identity || target.memberId;
    if (agents.some(
      (agent) => agent.identity === identity || agent.member_id === identity
    ))
      return;
    const fallback = agents.find(
      (agent) => agent.addressable || agent.affordances?.can_send_message
    ) || agents[0];
    if (fallback) {
      openAgentChat(fallback, "replace_focused");
    } else {
      dock.openTarget(buildControlTarget("roster"), "replace_focused");
    }
  }, [agents, dock.focusedTarget]);
  const refreshPanelData = import_react26.default.useCallback(async () => {
    const openPanels = dock.viewState.panels.map((p) => p.target).filter(Boolean);
    const inspects = openPanels.filter(
      (t) => t.kind === "identity-inspect"
    );
    if (inspects.length) {
      const entries = await Promise.all(
        inspects.map(async (t) => {
          const r2 = await callConsoleRpc(
            baseUrl,
            "mobkit/console/inspect_identity",
            { identity: t.identity }
          ).catch(
            () => callConsoleRpc(baseUrl, "mobkit/inspect_identity", {
              identity: t.identity
            })
          );
          return [t.identity, normalizeConsoleInspectResult(r2)];
        })
      );
      setInspectByIdentity((c2) => ({ ...c2, ...Object.fromEntries(entries) }));
    }
    if (hasMobControlSurface && openPanels.some((t) => t.kind === "routing")) {
      const [routes, history] = await Promise.all([
        callConsoleRpc(baseUrl, "mobkit/routing/routes/list", {}),
        callConsoleRpc(baseUrl, "mobkit/delivery/history", {})
      ]);
      setRoutingData(
        buildRoutingSectionView({
          routesResponse: routes,
          historyResponse: history
        })
      );
    }
    if (hasMobControlSurface && openPanels.some((t) => t.kind === "gating" || t.kind === "gates")) {
      const [p, a2] = await Promise.all([
        callConsoleRpc(
          baseUrl,
          "mobkit/gating/pending",
          {}
        ),
        callConsoleRpc(
          baseUrl,
          "mobkit/gating/audit",
          { limit: 50 }
        )
      ]);
      setGatingData({
        pending: Array.isArray(p.pending) ? p.pending : [],
        audit: Array.isArray(a2.entries) ? a2.entries : []
      });
    }
  }, [baseUrl, dock.viewState.panels, hasMobControlSurface]);
  import_react26.default.useEffect(() => {
    void refreshPanelData().catch(() => {
    });
  }, [dock.viewState.panels, refreshPanelData]);
  const scheduleExperienceRefresh = import_react26.default.useCallback(() => {
    if (experienceTimerRef.current !== null) return;
    experienceTimerRef.current = window.setTimeout(async () => {
      experienceTimerRef.current = null;
      await loadExperience().catch(() => {
      });
      await refreshPanelData().catch(() => {
      });
    }, 150);
  }, [loadExperience, refreshPanelData]);
  const scheduleHistoryRefresh = import_react26.default.useCallback(
    (identity) => {
      clearTimeout(refreshTimersRef.current[identity]);
      refreshTimersRef.current[identity] = window.setTimeout(async () => {
        const log = getOrCreateLog(identity);
        if (log.hasServerLog === false) {
          clearPhaseForIdentity(identity);
          forceRender();
          return;
        }
        try {
          await refreshIdentityTimelineNow(identity, { clearPhase: true });
        } catch {
        }
      }, 200);
    },
    [baseUrl, forceRender]
  );
  import_react26.default.useEffect(() => {
    for (const panel of dock.viewState.panels) {
      const target = panel.target;
      if (!target || target.kind !== "agent-chat") continue;
      const identity = target.identity || target.memberId;
      const log = getOrCreateLog(identity);
      if (log.hasServerLog !== null) continue;
      void refreshIdentityTimelineNow(identity).catch(() => {
      });
    }
  }, [baseUrl, dock.viewState.panels, forceRender]);
  import_react26.default.useEffect(() => {
    const refreshOpenChatPanels = async () => {
      const identities = /* @__PURE__ */ new Set();
      for (const panel of dock.viewState.panels) {
        const target = panel.target;
        if (!target || target.kind !== "agent-chat") continue;
        identities.add(target.identity || target.memberId);
      }
      if (identities.size === 0) return;
      let changed = false;
      for (const identity of identities) {
        const log = getOrCreateLog(identity);
        if (log.hasServerLog === false) continue;
        try {
          const { frames, available } = await queryIdentityTimeline(identity);
          const before = log.events.length;
          reconcileServerLog(identity, frames, available);
          if (log.events.length !== before) changed = true;
        } catch {
        }
      }
      if (changed) forceRender();
    };
    const timer2 = window.setInterval(() => {
      void refreshOpenChatPanels();
    }, 2e3);
    void refreshOpenChatPanels();
    return () => window.clearInterval(timer2);
  }, [baseUrl, dock.viewState.panels, forceRender]);
  const scheduleHistoryRefreshRef = import_react26.default.useRef(scheduleHistoryRefresh);
  scheduleHistoryRefreshRef.current = scheduleHistoryRefresh;
  const scheduleExperienceRefreshRef = import_react26.default.useRef(scheduleExperienceRefresh);
  scheduleExperienceRefreshRef.current = scheduleExperienceRefresh;
  import_react26.default.useEffect(() => {
    void queryTimeline(baseUrl, {}, 200).then(({ frames }) => {
      const seen = /* @__PURE__ */ new Set();
      const filtered = [];
      for (const frame2 of frames) {
        if (ACTIVITY_SKIP_EVENTS.has(frame2.event)) continue;
        const key = frame2.id || `${frame2.event}:${frame2.timestampMs || 0}`;
        if (seen.has(key)) continue;
        seen.add(key);
        filtered.push(frame2);
      }
      activityRef.current = filtered.slice(-200).reverse();
      forceRender();
    }).catch(() => {
    });
    const unsubscribe = subscribeTimelineEvents(baseUrl, {}, (frame2) => {
      if (!ACTIVITY_SKIP_EVENTS.has(frame2.event)) {
        activityRef.current = [frame2, ...activityRef.current].slice(0, 200);
      }
      if (PANEL_ROUTABLE_EVENTS.has(frame2.event)) {
        liveFramesRef.current = [frame2, ...liveFramesRef.current].slice(0, 300);
      }
      const identity = frame2.identity?.trim();
      if (PANEL_ROUTABLE_EVENTS.has(frame2.event) && identity && identity !== "_system") {
        appendFrame(identity, frame2);
        updatePhaseForIdentity(identity, frame2);
        updateBusyStateForFrame(identity, frame2);
      }
      forceRender();
      if ((HISTORY_REFRESH_EVENTS.has(frame2.event) || isEndTurnFrame(frame2)) && identity && identity !== "_system") {
        scheduleHistoryRefreshRef.current(identity);
      }
      if (REFRESH_TRIGGER_EVENTS.has(frame2.event) || frame2.event !== "keep-alive") {
        scheduleExperienceRefreshRef.current();
      }
    });
    return () => {
      unsubscribe();
    };
  }, [baseUrl]);
  import_react26.default.useEffect(() => {
    return () => {
      for (const timer2 of Object.values(phaseTimerByKey.current))
        window.clearTimeout(timer2);
      for (const timer2 of Object.values(refreshTimersRef.current))
        window.clearTimeout(timer2);
      if (experienceTimerRef.current !== null)
        window.clearTimeout(experienceTimerRef.current);
    };
  }, []);
  function openAgentChat(agent, intent = "replace_focused") {
    const target = buildDockTarget(agent);
    void refreshIdentityTimelineNow(target.identity || target.memberId).catch(
      () => {
      }
    );
    dock.openTarget(target, intent);
  }
  function openDockTarget(target, intent = "replace_focused") {
    if (target.kind === "agent-chat") {
      void refreshIdentityTimelineNow(target.identity || target.memberId).catch(
        () => {
        }
      );
    }
    dock.openTarget(target, intent);
  }
  function onSelectAgent(_block, _section, item) {
    const agent = agents.find((c2) => c2.member_id === item.id);
    if (agent) openAgentChat(agent);
  }
  async function submitMessageNow(panelId, target, text, handlingMode, attachments = []) {
    if (target.kind !== "agent-chat") return false;
    const panelKey = buildPanelConversationKey(panelId, target);
    const identity = target.identity || target.memberId;
    const optimisticObjectUrls = attachments.map(
      (file) => URL.createObjectURL(file)
    );
    const userEntry = createUserEntry(
      text,
      attachments.map((file, index2) => ({
        src: optimisticObjectUrls[index2] || "",
        mediaType: file.type || "application/octet-stream",
        alt: file.name
      }))
    );
    setSendingPanels((c2) => new Set(c2).add(panelKey));
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
            optimisticUser.objectUrls?.forEach(
              (url) => URL.revokeObjectURL(url)
            );
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
            optimisticUser.objectUrls?.forEach(
              (url) => URL.revokeObjectURL(url)
            );
            delete optimisticUserByPanelKeyRef.current[panelKey];
          }
        }
      } else {
        throw new Error("console send requires an identity-addressed target");
      }
      return true;
    } catch (submitError) {
      optimisticUserByPanelKeyRef.current[panelKey]?.objectUrls?.forEach(
        (url) => URL.revokeObjectURL(url)
      );
      delete optimisticUserByPanelKeyRef.current[panelKey];
      commitPanelPhase(panelKey, null);
      identityBusyRef.current[identity] = false;
      setError(errorMessage(submitError));
      forceRender();
      return false;
    } finally {
      setSendingPanels((c2) => {
        const n = new Set(c2);
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
      const sent = await submitMessageNow(
        panelId,
        target,
        text,
        "queue",
        attachments
      );
      if (sent) setDraftByKey((c2) => ({ ...c2, [panelKey]: "" }));
      return sent;
    }
    const newId = `pmsg-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 7)}`;
    setPendingStack(identity, (prev) => [
      ...prev,
      { id: newId, text, addedAt: Date.now(), status: "entering" }
    ]);
    setDraftByKey((c2) => ({ ...c2, [panelKey]: "" }));
    window.setTimeout(() => {
      setPendingStack(
        identity,
        (prev) => prev.map(
          (it) => it.id === newId && it.status === "entering" ? { ...it, status: null } : it
        )
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
      (prev) => prev.map(
        (it) => it.id === id ? { ...it, status: "promoting", editing: false } : it
      )
    );
    window.setTimeout(() => {
      const stack = getPendingStack(identity);
      const item = stack.find((it) => it.id === id);
      if (!item) return;
      setPendingStack(identity, (prev) => prev.filter((it) => it.id !== id));
      const target = findChatTargetFor(identity);
      if (target) {
        void submitMessageNow(
          target.panelId,
          target.target,
          item.text,
          "steer"
        );
      }
    }, animMs(360));
  }
  function onStackTrash(identity, id) {
    setPendingStack(
      identity,
      (prev) => prev.map(
        (it) => it.id === id ? { ...it, status: "trashing", editing: false } : it
      )
    );
    window.setTimeout(() => {
      setPendingStack(identity, (prev) => prev.filter((it) => it.id !== id));
    }, animMs(320));
  }
  function onStackEdit(identity, id) {
    setPendingStack(
      identity,
      (prev) => prev.map(
        (it) => it.id === id ? { ...it, editing: true } : { ...it, editing: false }
      )
    );
  }
  function onStackCommitEdit(identity, id, text) {
    const trimmed = text.trim();
    if (!trimmed) return;
    setPendingStack(
      identity,
      (prev) => prev.map(
        (it) => it.id === id ? { ...it, text: trimmed, editing: false, addedAt: Date.now() } : it
      )
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
    if (stack.some((it) => it.status === "draining" || it.status === "promoting"))
      return;
    const head = stack.find((it) => !it.status || it.status === "entering");
    if (!head) return;
    setPendingStack(
      identity,
      (prev) => prev.map(
        (it) => it.id === head.id ? { ...it, status: "draining" } : it
      )
    );
    window.setTimeout(() => {
      setPendingStack(
        identity,
        (prev) => prev.filter((it) => it.id !== head.id)
      );
      const target = findChatTargetFor(identity);
      if (target) {
        void submitMessageNow(
          target.panelId,
          target.target,
          head.text,
          "queue"
        );
      }
    }, animMs(420));
  }
  async function onLifecycleAction(identity, method) {
    await callConsoleRpc(baseUrl, method, { identity });
    const nextAgents = await loadExperience();
    if (method !== "mobkit/retire") return;
    if (nextAgents.some(
      (agent) => agent.identity === identity || agent.member_id === identity
    ))
      return;
    const fallback = nextAgents.find(
      (agent) => agent.addressable || agent.affordances?.can_send_message
    ) || nextAgents[0];
    if (fallback) {
      openAgentChat(fallback, "replace_focused");
    } else {
      dock.openTarget(buildControlTarget("roster"), "replace_focused");
    }
  }
  async function onGatingDecision(pendingId, decision) {
    await callConsoleRpc(baseUrl, "mobkit/gating/decide", {
      pending_id: pendingId,
      approver_id: DEFAULT_APPROVER_ID,
      decision,
      reason: `console_${decision}`
    });
    const [p, a2] = await Promise.all([
      callConsoleRpc(
        baseUrl,
        "mobkit/gating/pending",
        {}
      ),
      callConsoleRpc(baseUrl, "mobkit/gating/audit", {
        limit: 50
      })
    ]);
    setGatingData({
      pending: Array.isArray(p.pending) ? p.pending : [],
      audit: Array.isArray(a2.entries) ? a2.entries : []
    });
  }
  const SIDEBAR_MIN = 180, SIDEBAR_MAX = 420;
  function handleSidebarResize(event) {
    event.preventDefault();
    const startX = event.clientX;
    const root = findPaneResizeRoot(event.currentTarget);
    if (!root) return;
    const startWidth = parseInt(
      getComputedStyle(root).getPropertyValue(
        "--cc-workbench-sidebar-width"
      ) || "260",
      10
    ) || 260;
    const handle = event.currentTarget;
    if ("setPointerCapture" in handle)
      handle.setPointerCapture(event.pointerId);
    document.documentElement.setAttribute("data-cc-resizing", "true");
    function onPointerMove(e) {
      root.style.setProperty(
        "--cc-workbench-sidebar-width",
        `${Math.min(SIDEBAR_MAX, Math.max(SIDEBAR_MIN, startWidth + (e.clientX - startX)))}px`
      );
    }
    function cleanup() {
      document.documentElement.removeAttribute("data-cc-resizing");
      window.removeEventListener("pointermove", onPointerMove);
      window.removeEventListener("pointerup", cleanup);
      window.removeEventListener("pointercancel", cleanup);
      if ("hasPointerCapture" in handle && handle.hasPointerCapture(event.pointerId))
        handle.releasePointerCapture(event.pointerId);
    }
    window.addEventListener("pointermove", onPointerMove);
    window.addEventListener("pointerup", cleanup);
    window.addEventListener("pointercancel", cleanup);
  }
  const ACTIVITY_MIN = 200, ACTIVITY_MAX = 480;
  function handleActivityResize(event) {
    event.preventDefault();
    const startX = event.clientX;
    const root = findPaneResizeRoot(event.currentTarget);
    if (!root) return;
    const startWidth = parseInt(
      getComputedStyle(root).getPropertyValue(
        "--cc-workbench-activity-width"
      ) || "280",
      10
    ) || 280;
    const handle = event.currentTarget;
    if ("setPointerCapture" in handle)
      handle.setPointerCapture(event.pointerId);
    document.documentElement.setAttribute("data-cc-resizing", "true");
    function onPointerMove(e) {
      root.style.setProperty(
        "--cc-workbench-activity-width",
        `${Math.min(ACTIVITY_MAX, Math.max(ACTIVITY_MIN, startWidth - (e.clientX - startX)))}px`
      );
    }
    function cleanup() {
      document.documentElement.removeAttribute("data-cc-resizing");
      window.removeEventListener("pointermove", onPointerMove);
      window.removeEventListener("pointerup", cleanup);
      window.removeEventListener("pointercancel", cleanup);
      if ("hasPointerCapture" in handle && handle.hasPointerCapture(event.pointerId))
        handle.releasePointerCapture(event.pointerId);
    }
    window.addEventListener("pointermove", onPointerMove);
    window.addEventListener("pointerup", cleanup);
    window.addEventListener("pointercancel", cleanup);
  }
  if (loading)
    return /* @__PURE__ */ (0, import_jsx_runtime34.jsx)("div", { "data-testid": "console-loading", children: "Loading console..." });
  if (error) return /* @__PURE__ */ (0, import_jsx_runtime34.jsx)("div", { "data-testid": "console-error", children: error });
  const focusedMemberId = dock.focusedTarget?.kind === "agent-chat" ? dock.focusedTarget.memberId : selectedRosterMemberId;
  const sidebarVS = buildSidebarViewState({
    agents,
    selectedMemberId: focusedMemberId,
    pinnedAgentIds
  });
  const activityVS = buildActivityRailViewState({
    agents,
    eventFrames: activityRef.current,
    filterPresets: experience?.console_config?.rail?.filter_presets || experience?.activity_feed?.filter_presets,
    activePresetId: activeActivityPresetId || experience?.console_config?.rail?.active_preset_id || "all"
  });
  const actionConfig = experience?.console_config?.actions;
  const configuredActionLabels = {
    inspect: actionLabel(actionConfig, "inspect_label", "Details"),
    chat: actionLabel(actionConfig, "chat_label", "Open chat"),
    send: actionLabel(actionConfig, "send_label", "Send"),
    respawn: actionLabel(actionConfig, "respawn_label", "Respawn"),
    retire: actionLabel(actionConfig, "retire_label", "Retire"),
    reset: actionLabel(actionConfig, "reset_label", "Reset")
  };
  const configuredActionVisibility = {
    inspect: actionVisible(actionConfig, "show_inspect"),
    chat: actionVisible(actionConfig, "show_chat"),
    respawn: actionVisible(actionConfig, "show_respawn"),
    retire: actionVisible(actionConfig, "show_retire"),
    reset: actionVisible(actionConfig, "show_reset")
  };
  function renderChatPanel(panel) {
    const target = panel.target;
    if (!target || target.kind !== "agent-chat") return null;
    const panelKey = buildPanelConversationKey(panel.id, target);
    const identity = target.identity || target.memberId;
    const agent = agents.find((c2) => c2.member_id === target.memberId) || null;
    const sortedFrames = framesVisibleInPanel(
      getSortedFrames(identity),
      panel.id
    );
    const conversationEntries = mapFramesToTimelineEntries(
      agent,
      sortedFrames,
      {
        renderInteractionStartsAsUser: true,
        renderTextDeltas: true,
        blobBaseUrl: baseUrl
      }
    );
    const optimisticUser = optimisticUserByPanelKeyRef.current[panelKey] ?? null;
    const optimisticEntry = optimisticUser ? optimisticUser.entry : null;
    const entries = sanitizeConversationEntries(
      sortConversationTimelineEntries([
        ...conversationEntries,
        ...optimisticEntry ? [optimisticEntry] : []
      ])
    );
    const conversation = buildConversationViewState({
      memberId: target.memberId,
      agentLabel: target.title,
      agent,
      entries
    });
    const draft = draftByKey[panelKey] || "";
    const staged = stagedAttachmentsByIdentity[identity] ?? [];
    const isSending = sendingPanels.has(panelKey);
    const phase = Object.prototype.hasOwnProperty.call(
      phaseRef.current,
      panelKey
    ) ? phaseRef.current[panelKey] : agent?.response_phase ?? null;
    const canRespawn = configuredActionVisibility.respawn && agent?.affordances?.can_respawn === true;
    const canRetire = configuredActionVisibility.retire && agent?.affordances?.can_retire === true;
    const stackItems = getPendingStack(identity);
    const agentBusy = isIdentityBusy(identity);
    const stackSlot = stackItems.length > 0 ? /* @__PURE__ */ (0, import_jsx_runtime34.jsx)(
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
    return /* @__PURE__ */ (0, import_jsx_runtime34.jsx)(
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
        onDraftChange: (v) => setDraftByKey((c2) => ({ ...c2, [panelKey]: v })),
        onStagedChange: (action) => setStagedAttachmentsForIdentity(identity, action),
        onSend: (attachments) => onSendMessage(panel.id, target, attachments),
        onInspect: configuredActionVisibility.inspect ? () => {
          if (agent) handleShowRosterDetails(agent);
        } : void 0,
        onRespawn: canRespawn ? () => void onLifecycleAction(identity, "mobkit/respawn") : void 0,
        onRetire: canRetire ? () => void onLifecycleAction(identity, "mobkit/retire") : void 0,
        inspectLabel: configuredActionLabels.inspect,
        respawnLabel: configuredActionLabels.respawn,
        retireLabel: configuredActionLabels.retire,
        sendLabel: configuredActionLabels.send,
        stackSlot
      }
    );
  }
  function renderInspectPanel(target) {
    const inspect = inspectByIdentity[target.identity];
    const agent = agents.find(
      (candidate) => candidate.identity === target.identity || candidate.member_id === target.identity
    );
    const canRespawn = configuredActionVisibility.respawn && agent?.affordances?.can_respawn === true;
    const canRetire = configuredActionVisibility.retire && agent?.affordances?.can_retire === true;
    const canReset = configuredActionVisibility.reset && experience?.runtime_capabilities?.can_retire_members === true;
    return /* @__PURE__ */ (0, import_jsx_runtime34.jsxs)(
      "div",
      {
        className: "console-panel",
        "data-testid": `inspect-panel:${target.identity}`,
        children: [
          /* @__PURE__ */ (0, import_jsx_runtime34.jsxs)("div", { className: "console-panel__header", children: [
            /* @__PURE__ */ (0, import_jsx_runtime34.jsx)("h3", { children: target.identity }),
            /* @__PURE__ */ (0, import_jsx_runtime34.jsxs)("div", { className: "console-panel__actions", children: [
              canRespawn ? /* @__PURE__ */ (0, import_jsx_runtime34.jsx)(
                "button",
                {
                  "data-testid": `inspect-action:${target.identity}:respawn`,
                  type: "button",
                  onClick: () => void onLifecycleAction(target.identity, "mobkit/respawn"),
                  children: configuredActionLabels.respawn
                }
              ) : null,
              canReset ? /* @__PURE__ */ (0, import_jsx_runtime34.jsx)(
                "button",
                {
                  "data-testid": `inspect-action:${target.identity}:reset`,
                  type: "button",
                  onClick: () => void onLifecycleAction(target.identity, "mobkit/reset"),
                  children: configuredActionLabels.reset
                }
              ) : null,
              canRetire ? /* @__PURE__ */ (0, import_jsx_runtime34.jsx)(
                "button",
                {
                  "data-testid": `inspect-action:${target.identity}:retire`,
                  type: "button",
                  onClick: () => void onLifecycleAction(target.identity, "mobkit/retire"),
                  children: configuredActionLabels.retire
                }
              ) : null
            ] })
          ] }),
          !inspect ? /* @__PURE__ */ (0, import_jsx_runtime34.jsx)("p", { children: "Loading identity details\u2026" }) : /* @__PURE__ */ (0, import_jsx_runtime34.jsxs)("dl", { className: "console-panel__grid", children: [
            /* @__PURE__ */ (0, import_jsx_runtime34.jsx)("dt", { children: "State" }),
            /* @__PURE__ */ (0, import_jsx_runtime34.jsx)("dd", { children: inspect.state }),
            /* @__PURE__ */ (0, import_jsx_runtime34.jsx)("dt", { children: "Role" }),
            /* @__PURE__ */ (0, import_jsx_runtime34.jsx)("dd", { children: inspect.role || "n/a" }),
            /* @__PURE__ */ (0, import_jsx_runtime34.jsx)("dt", { children: "Addressability" }),
            /* @__PURE__ */ (0, import_jsx_runtime34.jsx)("dd", { children: inspect.addressability }),
            /* @__PURE__ */ (0, import_jsx_runtime34.jsx)("dt", { children: "Generation" }),
            /* @__PURE__ */ (0, import_jsx_runtime34.jsx)("dd", { children: inspect.continuity?.generation ?? "n/a" }),
            /* @__PURE__ */ (0, import_jsx_runtime34.jsx)("dt", { children: "Checkpoint" }),
            /* @__PURE__ */ (0, import_jsx_runtime34.jsx)("dd", { children: inspect.continuity?.checkpoint_version ?? "n/a" }),
            /* @__PURE__ */ (0, import_jsx_runtime34.jsx)("dt", { children: "Session" }),
            /* @__PURE__ */ (0, import_jsx_runtime34.jsx)("dd", { children: inspect.continuity?.session_id || "n/a" }),
            /* @__PURE__ */ (0, import_jsx_runtime34.jsx)("dt", { children: "Runtime" }),
            /* @__PURE__ */ (0, import_jsx_runtime34.jsx)("dd", { children: inspect.continuity?.agent_runtime_id || "n/a" }),
            /* @__PURE__ */ (0, import_jsx_runtime34.jsx)("dt", { children: "Lease Healthy" }),
            /* @__PURE__ */ (0, import_jsx_runtime34.jsx)("dd", { children: String(inspect.lease_healthy ?? inspect.lease?.healthy ?? false) }),
            /* @__PURE__ */ (0, import_jsx_runtime34.jsx)("dt", { children: "Peers" }),
            /* @__PURE__ */ (0, import_jsx_runtime34.jsx)("dd", { children: inspect.topology_peers?.join(", ") || "none" }),
            /* @__PURE__ */ (0, import_jsx_runtime34.jsx)("dt", { children: "Output Preview" }),
            /* @__PURE__ */ (0, import_jsx_runtime34.jsx)("dd", { children: inspect.output_preview || "n/a" })
          ] })
        ]
      }
    );
  }
  function renderHealthPanel(identities) {
    return /* @__PURE__ */ (0, import_jsx_runtime34.jsx)("div", { className: "console-panel", "data-testid": "health-panel", children: /* @__PURE__ */ (0, import_jsx_runtime34.jsx)("ul", { className: "console-panel__list", children: identities.map((r2) => /* @__PURE__ */ (0, import_jsx_runtime34.jsxs)("li", { "data-testid": `health-identity:${r2.identity}`, children: [
      /* @__PURE__ */ (0, import_jsx_runtime34.jsx)("strong", { children: r2.display_name || r2.identity }),
      " \xB7 ",
      r2.state,
      " \xB7",
      " ",
      r2.addressability
    ] }, r2.identity)) }) });
  }
  async function refreshInspectIdentity(identity) {
    const r2 = await callConsoleRpc(
      baseUrl,
      "mobkit/console/inspect_identity",
      { identity }
    ).catch(
      () => callConsoleRpc(baseUrl, "mobkit/inspect_identity", { identity })
    );
    setInspectByIdentity((current) => ({
      ...current,
      [identity]: normalizeConsoleInspectResult(r2)
    }));
  }
  function handleShowRosterDetails(agent) {
    setSelectedRosterMemberId(agent.member_id);
    const target = buildInspectTarget(agent);
    dock.openTarget(target, "replace_focused");
    void refreshInspectIdentity(target.identity).catch(() => {
    });
  }
  const mobName = experience?.console_config?.title || experience?.agent_sidebar?.title || "mob";
  const brand = experience?.console_config?.brand;
  const environmentLabel = experience?.console_config?.environment?.label || "dev";
  const railConfig = experience?.console_config?.rail;
  const railVisible = railConfig?.visible !== false;
  const watchedIdentities = new Set(
    agents.filter((agent) => agent.watched).map((agent) => agent.identity || agent.member_id).filter((value) => Boolean(value))
  );
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
    if (!target) return /* @__PURE__ */ (0, import_jsx_runtime34.jsx)("div", { className: "console-panel", children: "No panel target" });
    if (target.kind === "agent-chat") return renderChatPanel(panel);
    if (target.kind === "identity-inspect") {
      return renderInspectPanel(target);
    }
    if ((target.kind === "routing" || target.kind === "gating" || target.kind === "gates") && !hasMobControlSurface) {
      return /* @__PURE__ */ (0, import_jsx_runtime34.jsx)("div", { className: "console-panel", children: "This view requires a mob runtime control surface." });
    }
    if (target.kind === "routing") return /* @__PURE__ */ (0, import_jsx_runtime34.jsx)(RoutingPanel, { data: routingData });
    if (target.kind === "gating")
      return /* @__PURE__ */ (0, import_jsx_runtime34.jsx)(
        GatingInboxPanel,
        {
          pending: gatingData.pending,
          audit: gatingData.audit,
          onDecide: (pid, decision) => void onGatingDecision(pid, decision)
        }
      );
    if (target.kind === "topology")
      return /* @__PURE__ */ (0, import_jsx_runtime34.jsx)(
        TopologyPanel,
        {
          nodes: experience?.topology?.live_snapshot?.nodes || [],
          agents,
          activity: liveFramesRef.current
        }
      );
    if (target.kind === "health")
      return renderHealthPanel(
        experience?.health_overview?.live_snapshot?.identities || []
      );
    if (target.kind === "timeline")
      return /* @__PURE__ */ (0, import_jsx_runtime34.jsx)(TimelinePanel, { frames: activityRef.current });
    if (target.kind === "roster")
      return /* @__PURE__ */ (0, import_jsx_runtime34.jsx)(
        RosterPanel,
        {
          agents,
          selectedMemberId: selectedRosterMemberId,
          onSelect: (a2) => setSelectedRosterMemberId(a2.member_id),
          onChat: (a2) => openAgentChat(a2),
          onDetails: (a2) => handleShowRosterDetails(a2),
          onLifecycle: (identity, method) => void onLifecycleAction(identity, method),
          canResetLifecycle: hasMobControlSurface,
          actionLabels: configuredActionLabels,
          actionVisibility: configuredActionVisibility
        }
      );
    if (target.kind === "gates")
      return /* @__PURE__ */ (0, import_jsx_runtime34.jsx)(
        GatingInboxPanel,
        {
          pending: gatingData.pending,
          audit: gatingData.audit,
          onDecide: (pid, decision) => void onGatingDecision(pid, decision)
        }
      );
    if (target.kind === "logs")
      return /* @__PURE__ */ (0, import_jsx_runtime34.jsx)(LogsPanel, { frames: activityRef.current });
    return /* @__PURE__ */ (0, import_jsx_runtime34.jsx)("div", { className: "console-panel", children: "Unsupported panel" });
  }
  return /* @__PURE__ */ (0, import_jsx_runtime34.jsxs)(
    "div",
    {
      className: "cc-theme-scope mobkit-shell",
      "data-cc-theme": theme,
      "data-cc-variant": variant,
      "data-testid": "meerkat-console",
      children: [
        /* @__PURE__ */ (0, import_jsx_runtime34.jsx)(SpriteSheet, {}),
        /* @__PURE__ */ (0, import_jsx_runtime34.jsx)(
          Topbar,
          {
            mobName,
            brandLabel: brand?.label,
            brandLogoUrl: brand?.logo_url,
            brandLogoAlt: brand?.logo_alt,
            mobStatus,
            environment: environmentLabel,
            theme,
            onToggleTheme: toggleTheme,
            sidebarCollapsed,
            railCollapsed,
            railVisible,
            onToggleSidebar: toggleSidebarCollapsed,
            onToggleRail: toggleRailCollapsed
          }
        ),
        /* @__PURE__ */ (0, import_jsx_runtime34.jsxs)(
          "div",
          {
            className: "shell",
            "data-console-workbench": "root",
            "data-sidebar-collapsed": sidebarCollapsed ? "true" : "false",
            "data-rail-collapsed": railCollapsed ? "true" : "false",
            children: [
              /* @__PURE__ */ (0, import_jsx_runtime34.jsx)(
                Sidebar,
                {
                  agents,
                  selectedMemberId: focusedMemberId,
                  recentActivity: activityRef.current,
                  collapsed: sidebarCollapsed,
                  visibleControls,
                  customButtons: experience?.console_config?.sidebar?.buttons,
                  grouping: experience?.console_config?.agent_list,
                  onSelect: (a2) => openAgentChat(a2),
                  onOpenControl: (kind) => {
                    dock.openTarget(buildControlTarget(kind), "replace_focused");
                  }
                }
              ),
              /* @__PURE__ */ (0, import_jsx_runtime34.jsx)(
                "div",
                {
                  className: "pane-resizer",
                  "aria-hidden": "true",
                  "data-testid": "resize:sidebar",
                  onPointerDown: handleSidebarResize
                }
              ),
              /* @__PURE__ */ (0, import_jsx_runtime34.jsx)("div", { className: "main", children: /* @__PURE__ */ (0, import_jsx_runtime34.jsx)(
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
                    openDockTarget(target);
                  }
                }
              ) }),
              railVisible ? /* @__PURE__ */ (0, import_jsx_runtime34.jsxs)(import_jsx_runtime34.Fragment, { children: [
                /* @__PURE__ */ (0, import_jsx_runtime34.jsx)(
                  "div",
                  {
                    className: "pane-resizer pane-resizer--activity",
                    "aria-hidden": "true",
                    "data-testid": "resize:activity",
                    onPointerDown: handleActivityResize
                  }
                ),
                /* @__PURE__ */ (0, import_jsx_runtime34.jsx)(
                  SignalsRail,
                  {
                    frames: activityRef.current,
                    collapsed: railCollapsed,
                    filterPresets: railConfig?.filter_presets,
                    activePresetId: activeActivityPresetId || railConfig?.active_preset_id,
                    emptyText: railConfig?.empty_text,
                    watchedIdentities,
                    onPresetChange: setActiveActivityPresetId
                  }
                )
              ] }) : null
            ]
          }
        )
      ]
    }
  );
}

// src/index.tsx
var import_jsx_runtime35 = require("react/jsx-runtime");
function createConsoleApp(target, options = {}) {
  if (!target) {
    throw new Error("target element is required");
  }
  const baseUrl = options.baseUrl || "";
  const root = (0, import_client.createRoot)(target);
  root.render(/* @__PURE__ */ (0, import_jsx_runtime35.jsx)(ConsoleApp, { baseUrl }));
  return {
    unmount() {
      root.unmount();
    }
  };
}
