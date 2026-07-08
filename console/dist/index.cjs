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
  parseSseFrames: () => parseSseFrames2
});
module.exports = __toCommonJS(index_exports);
var import_client = require("react-dom/client");

// src/ConsoleApp.tsx
var import_react34 = __toESM(require("react"));

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

// ../packages/console-components/src/copy-button.tsx
var import_react = require("react");
var import_jsx_runtime2 = require("react/jsx-runtime");
function CopyButton({
  text,
  label,
  copiedLabel = "Copied",
  className,
  Icon: Icon3
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
  return /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(
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
      children: Icon3 ? /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Icon3, { name: copied ? "i-check" : "i-copy" }) : copied ? "Copied" : "Copy"
    }
  );
}

// ../packages/console-components/src/conversation/conversation-empty-state.tsx
var import_jsx_runtime3 = require("react/jsx-runtime");

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
  if (!record || record.error !== "replay_unavailable" && record.type !== "replay_unavailable") {
    return null;
  }
  const explicitStream = record.stream === "identity" || record.stream === "all_events" || record.stream === "timeline" ? record.stream : null;
  const requested = trimString(record.requested_last_event_id) || trimString(record.requested_cursor);
  const latest = trimString(record.latest_event_id) || trimString(record.latest_cursor);
  const stream = explicitStream || (requested?.startsWith("console:") || latest?.startsWith("console:") ? "timeline" : null);
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
var HIDDEN_PEER_DISPLAY_INTENTS = /* @__PURE__ */ new Set([
  "completed",
  "complete",
  "queued",
  "queue",
  "steer",
  "checksum_token",
  "peer"
]);
var UUID_RE = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;
var MACHINE_PEER_TOKEN_RE = /^peer[-_][a-z0-9][a-z0-9_-]*$/i;
var MACHINE_PEER_TOKEN_SUFFIX_RE = /\s+peer[-_][a-z0-9][a-z0-9_-]*$/i;
var EMBEDDED_MACHINE_PEER_TOKEN_RE = /\bpeer[-_][a-z0-9][a-z0-9_-]*\b/gi;
var EMBEDDED_PEER_ACK_TOKEN_RE = /\bACK_?FROM_?PEER_?peer[-_][a-z0-9][a-z0-9_-]*\b/gi;
var EMBEDDED_PEER_RESPONSE_TOKEN_RE = /\bpeer[-_]merge[-_][a-z0-9][a-z0-9_-]*\b/gi;
var LEGACY_INLINE_CODE_PLACEHOLDER_RE = /@@CODE\d+@@/g;
function normalizeProjectDisplayLabel(value) {
  const text = String(value || "").trim();
  if (!text) {
    return "";
  }
  const lower = text.toLowerCase();
  if (lower === "hsns" || lower === "hsns_clean") {
    return "HSNS";
  }
  if (lower === "homecore") {
    return "HomeCore";
  }
  return text.split(/[\s_-]+/u).filter(Boolean).map((part) => part.replace(/^[a-z]/u, (char) => char.toUpperCase())).join(" ");
}
function escapeHtml(value) {
  return String(value || "").replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;").replace(/'/g, "&#39;");
}
function safeConsoleHref(value) {
  const trimmed = String(value || "").trim();
  if (!trimmed) return null;
  if (/[\u0000-\u001f\u007f]/.test(trimmed)) return null;
  const lower = trimmed.toLowerCase();
  if (lower.startsWith("//")) return null;
  if (lower.startsWith("http://") || lower.startsWith("https://") || lower.startsWith("mailto:") || lower.startsWith("/") || lower.startsWith("./") || lower.startsWith("../") || lower.startsWith("#")) {
    return trimmed;
  }
  return null;
}
function renderConversationInlineMarkdown(text, options = {}) {
  const displayNormalization = options.displayNormalization !== false;
  const codeTokens = [];
  const tokenPrefix = "\uE000CCODE";
  const tokenSuffix = "\uE001";
  const source = displayNormalization ? normalizeConversationDisplayText(text || "") : String(text || "");
  const escaped = escapeHtml(source).replace(/`([^`]+)`/g, (_match, code) => {
    const index = codeTokens.push(`<code class="cc-rich-inline-code">${code}</code>`) - 1;
    return `${tokenPrefix}${index}${tokenSuffix}`;
  }).replace(/\*\*([^*\n]+)\*\*/g, "<strong>$1</strong>").replace(/(^|[^*])\*([^*\n]+)\*(?!\*)/g, "$1<em>$2</em>").replace(/(^|[^\w_])_([^_\n]+)_(?![\w_])/g, "$1<em>$2</em>").replace(/\[([^\]]+)\]\(([^)]+)\)/g, (_match, label, href) => {
    const safeHref = safeConsoleHref(href);
    return safeHref ? `<a href="${safeHref}" rel="noreferrer">${label}</a>` : label;
  }).replace(/\n/g, "<br />");
  return escaped.replace(new RegExp(`${tokenPrefix}(\\d+)${tokenSuffix}`, "g"), (_match, index) => codeTokens[Number(index)] || "");
}
function normalizeLegacyInlineCodePlaceholders(text) {
  const source = String(text || "");
  if (!LEGACY_INLINE_CODE_PLACEHOLDER_RE.test(source)) {
    return source;
  }
  LEGACY_INLINE_CODE_PLACEHOLDER_RE.lastIndex = 0;
  return source.split(/\n/u).map((line) => line.replace(/\s*@@CODE\d+@@\s*(?:[—–-]\s*)?/g, " ").replace(/\s*,\s*(?=,|and\b|or\b|[.;:!?]|$)/gi, " ").replace(/\s*\+\s*/g, " ").replace(/\s+([,.;:!?])/g, "$1").replace(/\s{2,}/g, " ").trim()).filter(Boolean).join("\n").replace(/\n{3,}/g, "\n\n").trim();
}
function normalizeEmbeddedMachinePeerTokens(text) {
  const source = String(text || "");
  if (!EMBEDDED_MACHINE_PEER_TOKEN_RE.test(source) && !EMBEDDED_PEER_ACK_TOKEN_RE.test(source)) {
    return source;
  }
  EMBEDDED_MACHINE_PEER_TOKEN_RE.lastIndex = 0;
  EMBEDDED_PEER_ACK_TOKEN_RE.lastIndex = 0;
  return source.split(/\n/u).map((line) => line.replace(EMBEDDED_PEER_ACK_TOKEN_RE, "acknowledgement").replace(EMBEDDED_PEER_RESPONSE_TOKEN_RE, "response token").replace(EMBEDDED_MACHINE_PEER_TOKEN_RE, " ").replace(/\bcontaining\s*([.;])/gi, "$1").replace(/^MobKit live peer smoke[.:]?\s*/i, "Peer check. ").replace(/\s+([,.;:!?])/g, "$1").replace(/:\s*([.;])/g, "$1").replace(/([.;:!?]){2,}/g, "$1").replace(/\s{2,}/g, " ").trim()).filter(Boolean).join("\n").replace(/\n{3,}/g, "\n\n").trim();
}
function normalizePeerSteeringPrompt(text) {
  const source = String(text || "").trim();
  if (!source) {
    return "";
  }
  if (/^After (?:the peer message|the request) is sent, stop\.?$/i.test(source)) {
    return "";
  }
  if (/^In the request blocks,\s*ask it to send_response\b/i.test(source)) {
    return "";
  }
  const splitSendRequest = source.match(/^Call peers, then send\b.*\bsend_request\b.*?\bto the peered\s+(.+?)\s+thread\b/i);
  if (splitSendRequest) {
    const projectLabel = normalizeProjectDisplayLabel(splitSendRequest[1]) || splitSendRequest[1].trim();
    return `Requested a peer response from ${projectLabel} thread.`;
  }
  const splitExactMessage = source.match(/^Send this exact message body to the peered\s+(.+?)\s+thread\b/i);
  if (splitExactMessage) {
    const projectLabel = normalizeProjectDisplayLabel(splitExactMessage[1]) || splitExactMessage[1].trim();
    if (/\bPlease reply with acknowledgement\b/i.test(source)) {
      return `Requested an acknowledgement from ${projectLabel} thread.`;
    }
    return `Sent a peer message to ${projectLabel} thread.`;
  }
  const standalonePeerInstruction = source.match(/^Use your MobKit peer tools only\b[\s\S]*?\bto the peered\s+(.+?)\s+thread\b/i);
  if (standalonePeerInstruction) {
    const projectLabel = normalizeProjectDisplayLabel(standalonePeerInstruction[1]) || standalonePeerInstruction[1].trim();
    const peerLabel = `${projectLabel} thread`;
    if (/\bsend_request\b/i.test(source) || /\bsend_response\b/i.test(source)) {
      return `Requested a peer response from ${peerLabel}.`;
    }
    if (/\bPlease reply with acknowledgement\b/i.test(source)) {
      return `Requested an acknowledgement from ${peerLabel}.`;
    }
    return `Sent a peer message to ${peerLabel}.`;
  }
  if (/^Use your MobKit peer tools only\b/i.test(source)) {
    return "";
  }
  const legacyTrustedPeerInstruction = /\bFind your trusted peer\b[\s\S]*?\bsend_message\b[\s\S]*?\bhandling_mode\b/i.test(source);
  const connectedMatch = source.match(
    /^Connected to\s+(.+?)\.\s+(?:Each thread keeps its own transcript and can message the other through MobKit|Each thread keeps its own transcript\. They can now message each other)\./is
  );
  if (connectedMatch && (/\bUse your MobKit peer tools only\b/i.test(source) || legacyTrustedPeerInstruction)) {
    const peerLabel = normalizeConversationDisplayLabel(connectedMatch[1]) || connectedMatch[1].trim();
    if (legacyTrustedPeerInstruction) {
      const action2 = /\bplease reply\b/i.test(source) ? `Requested a peer reply from ${peerLabel}.` : `Sent a peer message to ${peerLabel}.`;
      return [`Connected to ${peerLabel}.`, action2].join("\n");
    }
    if (/\bCall peers, then send\b.*\bsend_request\b/i.test(source) || /\bask it to send_response\b/i.test(source)) {
      return [`Connected to ${peerLabel}.`, `Requested a peer response from ${peerLabel}.`].join("\n");
    }
    if (!/\bSend this exact message body\b/i.test(source)) {
      return source;
    }
    const requestedAcknowledgement = /\bPlease reply with acknowledgement\b/i.test(source);
    const action = requestedAcknowledgement ? `Requested an acknowledgement from ${peerLabel}.` : `Sent a peer message to ${peerLabel}.`;
    return [`Connected to ${peerLabel}.`, action].join("\n");
  }
  if (legacyTrustedPeerInstruction) {
    return /\bplease reply\b/i.test(source) ? "Requested a peer reply." : "Sent a peer message.";
  }
  if (/^Call peers, then send_request\b/i.test(source) && /\bAsk the peer to send_response\b/i.test(source)) {
    return "Requested a peer response.";
  }
  return source;
}
function normalizeDisplayPunctuation(text) {
  return String(text || "").split(/\n/u).map((line) => line.replace(/\b(verified|received):\s*`?(?:response token|acknowledgement)`?\.?$/i, "$1.").replace(/:\s*\./g, ".").replace(/:\s*$/g, ".").replace(/\s+([,.;:!?])/g, "$1").replace(/([.;:!?]){2,}/g, "$1").trim()).filter((line) => line && !/^[\s"'“”‘’`´.,;:!?()[\]{}<>—–-]+$/u.test(line)).join("\n").trim();
}
function normalizeConversationDisplayText(text) {
  return normalizeDisplayPunctuation(
    normalizePeerSteeringPrompt(normalizeEmbeddedMachinePeerTokens(normalizeLegacyInlineCodePlaceholders(text)))
  );
}
function conversationRichPeerIntentForDisplay(intent, body) {
  const text = String(intent || "").trim();
  if (!text) {
    return void 0;
  }
  if (HIDDEN_PEER_DISPLAY_INTENTS.has(text.toLowerCase()) || UUID_RE.test(text) || MACHINE_PEER_TOKEN_RE.test(text)) {
    return void 0;
  }
  if (body && String(body).trim()) {
    return void 0;
  }
  return text;
}
function conversationRichPeerTargetForDisplay(target) {
  const text = normalizeConversationDisplayLabel(target);
  if (!text || UUID_RE.test(text)) {
    return "Peer";
  }
  return text;
}
function normalizeConversationDisplayLabel(label) {
  const text = String(label || "").trim().replace(/\s+/g, " ");
  if (!text || UUID_RE.test(text) || MACHINE_PEER_TOKEN_RE.test(text)) {
    return "";
  }
  const withoutToken = text.replace(MACHINE_PEER_TOKEN_SUFFIX_RE, "").trim();
  if (!withoutToken || UUID_RE.test(withoutToken) || MACHINE_PEER_TOKEN_RE.test(withoutToken)) {
    return "";
  }
  const livePeer = withoutToken.match(/^Peer\s+live\s+(.+)$/i);
  if (livePeer) {
    return `${normalizeProjectDisplayLabel(livePeer[1])} peer thread`;
  }
  return withoutToken.replace(/\bpeer\s+(?:source|target)\b/i, "peer thread").replace(/\brequest\s+source\b/i, "request thread").replace(/\bresponse\s+target\b/i, "response thread").replace(/\bmerged\s+request\b/i, "peer request").replace(/\bmerged\s+response\b/i, "peer response").trim();
}
function conversationRichPeerBodyForDisplay(body) {
  const raw = String(body || "").trim();
  if (!raw) {
    return void 0;
  }
  if (UUID_RE.test(raw) || MACHINE_PEER_TOKEN_RE.test(raw)) {
    return "Response sent.";
  }
  if (/^please\s+send_response\b.*\bresult\.token\b/i.test(raw)) {
    return "Response requested.";
  }
  if (/^please\s+reply\s+with\s+ACK_FROM_PEER_/i.test(raw)) {
    return "Acknowledgement requested.";
  }
  if (/^ACK_?FROM_?PEER_/i.test(raw)) {
    return "Acknowledgement sent.";
  }
  const text = normalizeConversationDisplayText(raw);
  return text || void 0;
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
        const peerBody = conversationRichPeerBodyForDisplay(block.peerBody);
        const images = (block.peerImages || []).map((image) => [image.alt || "image", image.blobId || image.src].filter(Boolean).join(" ")).filter(Boolean).join(" ");
        return [
          `${dir} ${conversationRichPeerTargetForDisplay(block.peerTarget)}`,
          conversationRichPeerIntentForDisplay(block.peerIntent, peerBody),
          peerBody,
          images,
          block.result
        ].filter(Boolean).join(": ").trim();
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
function parseStreamingConversationRichBlocks(content, options) {
  const source = String(content || "").trimEnd();
  if (!source.trim()) {
    return [];
  }
  const stableEnd = streamingStablePrefixLength(source);
  const stable = stableEnd > 0 ? source.slice(0, stableEnd).trim() : "";
  const tail = source.slice(stableEnd).trim();
  const blocks = stable ? parseConversationRichBlocks(stable, options) : [];
  if (tail) {
    const tailText = tail.replace(/\n{3,}/g, "\n\n");
    const visibleTail = (unclosedFenceStartIndex(tailText) !== null ? tailText : hideIncompleteInlineTail(tailText)).trim();
    if (visibleTail) {
      blocks.push({ type: "paragraph", text: visibleTail, streaming: true });
    }
  }
  return compactConversationBlocks(blocks);
}
function streamingStablePrefixLength(source) {
  const fenceStart = unclosedFenceStartIndex(source);
  const scanEnd = fenceStart ?? source.length;
  const scanSource = source.slice(0, scanEnd);
  let stableEnd = 0;
  const blankLineRe = /\n[ \t]*\n/gu;
  let match;
  while (match = blankLineRe.exec(scanSource)) {
    stableEnd = blankLineRe.lastIndex;
  }
  return stableEnd;
}
function hideIncompleteInlineTail(source) {
  const firstOpen = firstUnclosedInlineMarkerIndex(source);
  if (firstOpen === null) {
    return source;
  }
  return source.slice(0, firstOpen).replace(/\s+$/u, "");
}
function firstUnclosedInlineMarkerIndex(source) {
  return minNullable([
    unclosedDelimitedMarkerIndex(source, "`"),
    unclosedDelimitedMarkerIndex(source, "**"),
    unclosedDelimitedMarkerIndex(source, "*"),
    unclosedDelimitedMarkerIndex(source, "_"),
    unclosedLinkStartIndex(source)
  ]);
}
function minNullable(values) {
  return values.reduce((min, value) => {
    if (value === null) return min;
    return min === null || value < min ? value : min;
  }, null);
}
function unclosedDelimitedMarkerIndex(source, delimiter) {
  const positions = [];
  for (let index = 0; index < source.length; index++) {
    if (source[index - 1] === "\\") continue;
    if (delimiter === "**") {
      if (source.slice(index, index + 2) !== "**") continue;
      positions.push(index);
      index += 1;
      continue;
    }
    const char = source[index];
    if (char !== delimiter) continue;
    if (delimiter === "*" && (source[index - 1] === "*" || source[index + 1] === "*")) continue;
    if (delimiter === "_" && isAlphaNumeric(source[index - 1]) && isAlphaNumeric(source[index + 1])) continue;
    if ((delimiter === "*" || delimiter === "_") && isLineBulletMarker(source, index)) continue;
    positions.push(index);
  }
  return positions.length % 2 === 1 ? positions.at(-1) ?? null : null;
}
function unclosedLinkStartIndex(source) {
  const match = source.match(/\[[^\]\n]*\]\([^)\n]*$/u);
  return match?.index ?? null;
}
function isAlphaNumeric(value) {
  return Boolean(value && /[A-Za-z0-9]/u.test(value));
}
function isLineBulletMarker(source, index) {
  const before = source.slice(0, index);
  const linePrefix = before.slice(before.lastIndexOf("\n") + 1);
  return linePrefix.trim().length === 0 && /\s/u.test(source[index + 1] || "");
}
function unclosedFenceStartIndex(source) {
  const fenceRe = /^```/gmu;
  let match;
  let openStart = null;
  while (match = fenceRe.exec(source)) {
    openStart = openStart === null ? match.index : null;
  }
  return openStart;
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
function parseConversationRichBlocks(content, options) {
  const displayNormalization = options?.displayNormalization !== false;
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
    blocks.push(...parseConversationTextBlocks(before, displayNormalization));
    blocks.push({
      type: "code",
      language: (match[1] || "text").trim() || "text",
      body: match[2].replace(/\n+$/u, "")
    });
    lastIndex = fenceRe.lastIndex;
  }
  blocks.push(...parseConversationTextBlocks(source.slice(lastIndex), displayNormalization));
  return compactConversationBlocks(blocks);
}
function parseConversationTextBlocks(fragment, displayNormalization = true) {
  const source = (displayNormalization ? normalizeConversationDisplayText(String(fragment || "")) : String(fragment || "")).trim();
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
  if (entry.kind === "flow_run") {
    const rowLines = entry.rows.map((row) => `${row.label}: ${row.caption}`);
    return [entry.flowName, entry.objective || "", ...rowLines, entry.outcome || ""].filter(Boolean).join("\n");
  }
  if (entry.kind === "workgraph") {
    const itemLines = entry.items.map((item) => `${"  ".repeat(item.depth)}${item.title} \u2014 ${item.status.replace(/_/g, " ")}`);
    const attentionLines = entry.attention.map((row) => `${row.mode}: ${row.statusLabel}${row.targetLabel ? ` \u2192 ${row.targetLabel}` : ""}`);
    return [
      `${entry.title} (${entry.progress.completed}/${entry.progress.total})`,
      entry.objective || "",
      ...itemLines,
      ...attentionLines
    ].filter(Boolean).join("\n");
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
  const [firstTarget, ...remainingTargets] = suggestDockTargets({
    count: requestedCount,
    preferred: preferredTarget,
    excludedIds: [],
    suggestTargets
  });
  const [secondTarget, thirdTarget, suggestedFourthTarget] = remainingTargets.filter(
    (target) => Boolean(target)
  );
  const fourthTarget = suggestedFourthTarget || (presetId === "grid" && thirdTarget && preferredTarget && thirdTarget.id !== preferredTarget.id ? preferredTarget : null);
  const primary = createPanelState({
    target: preferredPanel ? preferredTarget ?? preferredPanel.target : firstTarget || null,
    sourcePanel: preferredPanel || null
  });
  const singlePanelState = () => ({
    presetId: "single",
    layout: panelNode(primary.id),
    panels: [primary],
    focusedPanelId: primary.id
  });
  if (presetId === "single") {
    return singlePanelState();
  }
  if (presetId === "two_columns") {
    if (!secondTarget) return singlePanelState();
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
    if (!secondTarget) return singlePanelState();
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
  if (!secondTarget && !thirdTarget && !fourthTarget) {
    return singlePanelState();
  }
  const rightTop = createPanelState({ target: secondTarget || null, sourcePanel: preferredPanel || primary });
  if (!thirdTarget) {
    return {
      presetId: "two_columns",
      layout: {
        kind: "split",
        id: createSplitId(),
        direction: "horizontal",
        ratio: 0.5,
        first: panelNode(primary.id),
        second: panelNode(rightTop.id)
      },
      panels: [primary, rightTop],
      focusedPanelId: primary.id
    };
  }
  const leftBottom = createPanelState({ target: thirdTarget, sourcePanel: preferredPanel || primary });
  if (!fourthTarget) {
    return {
      presetId,
      layout: {
        kind: "split",
        id: createSplitId(),
        direction: "horizontal",
        ratio: 0.5,
        first: panelNode(primary.id),
        second: {
          kind: "split",
          id: createSplitId(),
          direction: "vertical",
          ratio: 0.5,
          first: panelNode(rightTop.id),
          second: panelNode(leftBottom.id)
        }
      },
      panels: [primary, rightTop, leftBottom],
      focusedPanelId: primary.id
    };
  }
  const rightBottom = createPanelState({ target: fourthTarget, sourcePanel: preferredPanel || primary });
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
      presetId: initial.presetId,
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
      presetId: presetState.presetId,
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
          title: resolved.title || panel.target?.title || "Choose a target",
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

// ../packages/console-core/src/navigation.ts
function normalizeMeta(meta) {
  return (meta || []).filter((entry) => Boolean(entry?.label));
}
function normalizeActions(actions) {
  return (actions || []).filter((action) => Boolean(action?.id && action?.label));
}
function collectNavigationNodeIds(nodes) {
  const ids = [];
  for (const node of nodes) {
    ids.push(node.id);
    if (node.type === "group") {
      ids.push(...collectNavigationNodeIds(node.children));
    }
  }
  return ids;
}
function normalizeNode2(node, seen) {
  if (!node?.id || !node.label || seen.has(node.id)) {
    return null;
  }
  seen.add(node.id);
  const base = {
    ...node,
    meta: normalizeMeta(node.meta),
    actions: normalizeActions(node.actions)
  };
  if (node.type === "group") {
    return {
      ...base,
      type: "group",
      expanded: node.expanded !== false,
      children: (node.children || []).map((child) => normalizeNode2(child, seen)).filter(Boolean)
    };
  }
  if (node.type === "item") {
    return {
      ...base,
      type: "item",
      pinned: Boolean(node.pinned),
      unread: Boolean(node.unread)
    };
  }
  return null;
}
function findNavigationNode(nodes, id, parentId = null, path = []) {
  for (let index = 0; index < nodes.length; index += 1) {
    const node = nodes[index];
    const nodePath = [...path, index];
    if (node.id === id) {
      return { node, parentId, path: nodePath };
    }
    if (node.type === "group") {
      const child = findNavigationNode(node.children, id, node.id, nodePath);
      if (child) return child;
    }
  }
  return null;
}
function isDescendantPath(path, possibleDescendant) {
  return path.length < possibleDescendant.length && path.every((segment, index) => possibleDescendant[index] === segment);
}
function removeNodeAtPath(nodes, path) {
  if (path.length === 0) {
    return { nodes, removed: null };
  }
  const [head, ...tail] = path;
  if (head === void 0 || head < 0 || head >= nodes.length) {
    return { nodes, removed: null };
  }
  if (tail.length === 0) {
    const next2 = [...nodes];
    const [removed] = next2.splice(head, 1);
    return { nodes: next2, removed: removed || null };
  }
  const node = nodes[head];
  if (node.type !== "group") {
    return { nodes, removed: null };
  }
  const childResult = removeNodeAtPath(node.children, tail);
  const next = [...nodes];
  next[head] = { ...node, children: childResult.nodes };
  return { nodes: next, removed: childResult.removed };
}
function insertNode(nodes, targetId, position, nodeToInsert) {
  const next = [...nodes];
  for (let index = 0; index < next.length; index += 1) {
    const node = next[index];
    if (node.id === targetId) {
      if (position === "inside" && node.type === "group") {
        next[index] = { ...node, expanded: true, children: [...node.children, nodeToInsert] };
        return { nodes: next, inserted: true };
      }
      const offset = position === "after" ? 1 : 0;
      next.splice(index + offset, 0, nodeToInsert);
      return { nodes: next, inserted: true };
    }
    if (node.type === "group") {
      const childResult = insertNode(node.children, targetId, position, nodeToInsert);
      if (childResult.inserted) {
        next[index] = {
          ...node,
          children: childResult.nodes
        };
        return { nodes: next, inserted: true };
      }
    }
  }
  return { nodes, inserted: false };
}
function navigationMoveAnnouncement(moved, target, position) {
  if (position === "inside") {
    return `Moved ${moved.label} into ${target.label}.`;
  }
  return `Moved ${moved.label} ${position} ${target.label}.`;
}
function normalizeConsoleNavigationModel(model) {
  const seen = /* @__PURE__ */ new Set();
  const nodes = (model?.nodes || []).map((node) => normalizeNode2(node, seen)).filter(Boolean);
  const ids = collectNavigationNodeIds(nodes);
  const idSet = new Set(ids);
  const activeNodeId = model?.activeNodeId && idSet.has(model.activeNodeId) ? model.activeNodeId : void 0;
  const focusNodeId = model?.focusNodeId && idSet.has(model.focusNodeId) ? model.focusNodeId : activeNodeId;
  const orderedNodeIds = (model?.order?.orderedNodeIds || []).filter((id) => idSet.has(id));
  return {
    orientation: model?.orientation,
    activeNodeId,
    focusNodeId,
    nodes,
    order: { orderedNodeIds: orderedNodeIds.length ? orderedNodeIds : ids }
  };
}
function canMoveConsoleNavigationNode(model, input) {
  const normalized = normalizeConsoleNavigationModel(model);
  const moved = findNavigationNode(normalized.nodes, input.id);
  const target = findNavigationNode(normalized.nodes, input.targetId);
  if (!moved || !target || moved.node.disabled || target.node.disabled) {
    return false;
  }
  if (moved.node.id === target.node.id) {
    return false;
  }
  if (isDescendantPath(moved.path, target.path)) {
    return false;
  }
  if (input.position === "inside" && target.node.type !== "group") {
    return false;
  }
  if (input.scope === "siblings" && moved.parentId !== target.parentId) {
    return false;
  }
  return true;
}
function moveConsoleNavigationNode(model, input) {
  const normalized = normalizeConsoleNavigationModel(model);
  const moved = findNavigationNode(normalized.nodes, input.id);
  const target = findNavigationNode(normalized.nodes, input.targetId);
  if (!moved || !target || !canMoveConsoleNavigationNode(normalized, input)) {
    return {
      model: normalized,
      focusNodeId: normalized.focusNodeId || null,
      announcement: "Move unavailable."
    };
  }
  const removed = removeNodeAtPath(normalized.nodes, moved.path);
  if (!removed.removed) {
    return {
      model: normalized,
      focusNodeId: normalized.focusNodeId || null,
      announcement: "Move unavailable."
    };
  }
  const inserted = insertNode(removed.nodes, input.targetId, input.position, removed.removed);
  if (!inserted.inserted) {
    return {
      model: normalized,
      focusNodeId: normalized.focusNodeId || null,
      announcement: "Move unavailable."
    };
  }
  const nodes = inserted.nodes;
  const next = normalizeConsoleNavigationModel({
    ...normalized,
    focusNodeId: removed.removed.id,
    nodes,
    order: { orderedNodeIds: collectNavigationNodeIds(nodes) }
  });
  return {
    model: next,
    focusNodeId: removed.removed.id,
    announcement: navigationMoveAnnouncement(removed.removed, target.node, input.position)
  };
}
function applyConsoleNavigationReorderIntent(model, intent) {
  return moveConsoleNavigationNode(model, intent);
}

// ../packages/console-core/src/sidebar-preferences.ts
var SIDEBAR_PINS_STORAGE_PREFIX = "mobkit-console-sidebar-pins";
var SIDEBAR_SECTION_ORDER_STORAGE_PREFIX = "mobkit-console-sidebar-section-order";
var SIDEBAR_SUBGROUP_ORDER_STORAGE_PREFIX = "mobkit-console-sidebar-subgroup-order";
var SECTION_COLLAPSE_STORAGE_PREFIX = "mobkit-console-sidebar-sections";
var SUBGROUP_COLLAPSE_STORAGE_PREFIX = "mobkit-console-sidebar-subgroups";
var SIDEBAR_STORAGE_PREFIXES = [
  SIDEBAR_PINS_STORAGE_PREFIX,
  SIDEBAR_SECTION_ORDER_STORAGE_PREFIX,
  SIDEBAR_SUBGROUP_ORDER_STORAGE_PREFIX,
  SECTION_COLLAPSE_STORAGE_PREFIX,
  SUBGROUP_COLLAPSE_STORAGE_PREFIX
];
function sidebarStorageKey(prefix, namespace) {
  return `${prefix}:${namespace?.trim() || "default"}`;
}
function readSidebarStringSet(storage, key) {
  if (!storage) return null;
  try {
    const raw = storage.getItem(key);
    if (raw === null) return null;
    const parsed = JSON.parse(raw);
    if (!Array.isArray(parsed)) return /* @__PURE__ */ new Set();
    return new Set(parsed.filter((value) => typeof value === "string" && value.trim().length > 0));
  } catch {
    return null;
  }
}
function writeSidebarStringSet(storage, key, value) {
  if (!storage) return;
  try {
    const normalized = Array.from(value).map((item) => item.trim()).filter(Boolean).sort();
    storage.setItem(key, JSON.stringify(Array.from(new Set(normalized))));
  } catch {
  }
}
function readSidebarStringList(storage, key) {
  if (!storage) return null;
  try {
    const raw = storage.getItem(key);
    if (raw === null) return null;
    const parsed = JSON.parse(raw);
    if (!Array.isArray(parsed)) return [];
    const seen = /* @__PURE__ */ new Set();
    const out = [];
    for (const value of parsed) {
      if (typeof value !== "string") continue;
      const trimmed = value.trim();
      if (!trimmed || seen.has(trimmed)) continue;
      seen.add(trimmed);
      out.push(trimmed);
    }
    return out;
  } catch {
    return null;
  }
}
function writeSidebarStringList(storage, key, value) {
  if (!storage) return;
  try {
    const seen = /* @__PURE__ */ new Set();
    const normalized = value.map((item) => item.trim()).filter((item) => {
      if (!item || seen.has(item)) return false;
      seen.add(item);
      return true;
    });
    storage.setItem(key, JSON.stringify(normalized));
  } catch {
  }
}
function pruneStaleSidebarStorage(storage, scope, activeNamespace) {
  if (!storage) return;
  try {
    const scopePrefix = encodeURIComponent(scope.trim());
    const activeKeys = new Set(SIDEBAR_STORAGE_PREFIXES.map((prefix) => `${prefix}:${activeNamespace}`));
    const stale = [];
    for (let i = 0; i < storage.length; i += 1) {
      const key = storage.key(i);
      if (!key || activeKeys.has(key)) continue;
      if (SIDEBAR_STORAGE_PREFIXES.some((prefix) => key.startsWith(`${prefix}:${scopePrefix}:`))) {
        stale.push(key);
      }
    }
    for (const key of stale) storage.removeItem(key);
  } catch {
  }
}
function applyConsoleSidebarOrder(items, storedOrder) {
  const available = new Set(items);
  const seen = /* @__PURE__ */ new Set();
  const ordered = [];
  for (const item of storedOrder || []) {
    if (!available.has(item) || seen.has(item)) continue;
    ordered.push(item);
    seen.add(item);
  }
  for (const item of items) {
    if (seen.has(item)) continue;
    ordered.push(item);
    seen.add(item);
  }
  return ordered;
}

// ../packages/console-core/src/format.ts
function formatCount(value) {
  return new Intl.NumberFormat("en-US").format(Number(value) || 0);
}

// ../packages/console-core/src/adapters.ts
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
  "snapshot_complete",
  "snapshot_started",
  "run_failed",
  "keep-alive",
  "server_tool_content",
  "tool_config_changed",
  "tool_scope_changed"
]);
var ACTIVITY_HIDDEN_EVENTS = /* @__PURE__ */ new Set([
  ...HIDDEN_EVENTS,
  "text_delta",
  "tool_call_requested",
  "tool_call",
  "server_tool_content",
  "tool_execution_started",
  "tool_result_received",
  "tool_execution_completed"
]);

// ../packages/console-core/src/contract.ts
var CONSOLE_RPC_METHODS = {
  capabilities: "mobkit/capabilities",
  send: "mobkit/console/send",
  listIdentities: "mobkit/console/list_identities",
  inspectIdentity: "mobkit/console/inspect_identity",
  queryTimeline: "mobkit/console/query_timeline",
  blobUpload: "mobkit/blob/upload",
  retireIdentity: "mobkit/retire",
  respawnIdentity: "mobkit/respawn",
  resetIdentity: "mobkit/reset",
  routingRoutesList: "mobkit/routing/routes/list",
  deliveryHistory: "mobkit/delivery/history",
  gatingPending: "mobkit/gating/pending",
  gatingAudit: "mobkit/gating/audit",
  gatingDecide: "mobkit/gating/decide",
  accessStatus: "mobkit/access/status",
  accessGet: "mobkit/access/get",
  accessSet: "mobkit/access/set",
  accessEnable: "mobkit/access/enable",
  accessRuleUpsert: "mobkit/access/rules/upsert",
  accessRuleDelete: "mobkit/access/rules/delete",
  accessGroupSet: "mobkit/access/groups/set",
  accessGroupDelete: "mobkit/access/groups/delete",
  accessPreview: "mobkit/access/preview",
  memoryPanelRecords: "mobkit/memory/panel/records",
  memoryPanelRecord: "mobkit/memory/panel/record",
  memoryPanelQuarantine: "mobkit/memory/panel/quarantine",
  memoryPanelDreams: "mobkit/memory/panel/dreams",
  memoryPanelOverview: "mobkit/memory/panel/overview",
  memoryPanelProposals: "mobkit/memory/panel/proposals",
  memoryPanelInjections: "mobkit/memory/panel/injections",
  memoryPanelHarvests: "mobkit/memory/panel/harvests",
  memoryPanelDreamRuns: "mobkit/memory/panel/dream_runs",
  memoryPanelAuditVerdicts: "mobkit/memory/panel/audit_verdicts",
  workgraphSnapshot: "mobkit/workgraph/snapshot",
  workgraphEvents: "mobkit/workgraph/events",
  workgraphGoalStatus: "mobkit/workgraph/goal/status",
  workgraphClaim: "mobkit/workgraph/claim",
  workgraphRelease: "mobkit/workgraph/release",
  workgraphClose: "mobkit/workgraph/close",
  workgraphGoalConfirm: "mobkit/workgraph/goal/confirm",
  workgraphGoalRequestClose: "mobkit/workgraph/goal/request_close",
  workgraphAttentionPause: "mobkit/workgraph/attention/pause",
  workgraphAttentionResume: "mobkit/workgraph/attention/resume",
  workgraphAttentionReassign: "mobkit/workgraph/attention/reassign"
};

// ../packages/console-core/src/headless.ts
var CONSOLE_COMMAND_NAMES = {
  inspectIdentity: "inspectIdentity",
  retireIdentity: "retireIdentity",
  respawnIdentity: "respawnIdentity",
  resetIdentity: "resetIdentity",
  listRoutingRoutes: "listRoutingRoutes",
  listDeliveryHistory: "listDeliveryHistory",
  listGatingPending: "listGatingPending",
  listGatingAudit: "listGatingAudit",
  decideGating: "decideGating",
  accessStatus: "accessStatus",
  getAccessConfig: "getAccessConfig",
  setAccessConfig: "setAccessConfig",
  enableAccess: "enableAccess",
  upsertAccessRule: "upsertAccessRule",
  deleteAccessRule: "deleteAccessRule",
  setAccessGroup: "setAccessGroup",
  deleteAccessGroup: "deleteAccessGroup",
  previewAccess: "previewAccess",
  listMemoryRecords: "listMemoryRecords",
  getMemoryRecord: "getMemoryRecord",
  listMemoryQuarantine: "listMemoryQuarantine",
  listMemoryDreams: "listMemoryDreams",
  getMemoryOverview: "getMemoryOverview",
  listMemoryProposals: "listMemoryProposals",
  listMemoryInjections: "listMemoryInjections",
  listMemoryHarvests: "listMemoryHarvests",
  listMemoryDreamRuns: "listMemoryDreamRuns",
  listMemoryAuditVerdicts: "listMemoryAuditVerdicts",
  workgraphSnapshot: "workgraphSnapshot",
  workgraphEvents: "workgraphEvents",
  workgraphGoalStatus: "workgraphGoalStatus",
  workgraphClaim: "workgraphClaim",
  workgraphRelease: "workgraphRelease",
  workgraphClose: "workgraphClose",
  workgraphGoalConfirm: "workgraphGoalConfirm",
  workgraphGoalRequestClose: "workgraphGoalRequestClose",
  workgraphAttentionPause: "workgraphAttentionPause",
  workgraphAttentionResume: "workgraphAttentionResume",
  workgraphAttentionReassign: "workgraphAttentionReassign"
};
var CONSOLE_COMMAND_SPECS = {
  [CONSOLE_COMMAND_NAMES.inspectIdentity]: {
    method: CONSOLE_RPC_METHODS.inspectIdentity,
    targetKinds: /* @__PURE__ */ new Set([
      "mobkit/identity-chat",
      "mobkit/identity-inspect"
    ])
  },
  [CONSOLE_COMMAND_NAMES.retireIdentity]: {
    method: CONSOLE_RPC_METHODS.retireIdentity,
    targetKinds: /* @__PURE__ */ new Set([
      "mobkit/identity-chat",
      "mobkit/identity-inspect"
    ])
  },
  [CONSOLE_COMMAND_NAMES.respawnIdentity]: {
    method: CONSOLE_RPC_METHODS.respawnIdentity,
    targetKinds: /* @__PURE__ */ new Set([
      "mobkit/identity-chat",
      "mobkit/identity-inspect"
    ])
  },
  [CONSOLE_COMMAND_NAMES.resetIdentity]: {
    method: CONSOLE_RPC_METHODS.resetIdentity,
    targetKinds: /* @__PURE__ */ new Set([
      "mobkit/identity-chat",
      "mobkit/identity-inspect"
    ])
  },
  [CONSOLE_COMMAND_NAMES.listRoutingRoutes]: {
    method: CONSOLE_RPC_METHODS.routingRoutesList,
    targetKinds: /* @__PURE__ */ new Set(["mobkit/routing"])
  },
  [CONSOLE_COMMAND_NAMES.listDeliveryHistory]: {
    method: CONSOLE_RPC_METHODS.deliveryHistory,
    targetKinds: /* @__PURE__ */ new Set(["mobkit/routing"])
  },
  [CONSOLE_COMMAND_NAMES.listGatingPending]: {
    method: CONSOLE_RPC_METHODS.gatingPending,
    targetKinds: /* @__PURE__ */ new Set(["mobkit/gating"])
  },
  [CONSOLE_COMMAND_NAMES.listGatingAudit]: {
    method: CONSOLE_RPC_METHODS.gatingAudit,
    targetKinds: /* @__PURE__ */ new Set(["mobkit/gating"])
  },
  [CONSOLE_COMMAND_NAMES.decideGating]: {
    method: CONSOLE_RPC_METHODS.gatingDecide,
    targetKinds: /* @__PURE__ */ new Set(["mobkit/gating"])
  },
  [CONSOLE_COMMAND_NAMES.accessStatus]: {
    method: CONSOLE_RPC_METHODS.accessStatus,
    targetKinds: /* @__PURE__ */ new Set(["mobkit/access"])
  },
  [CONSOLE_COMMAND_NAMES.getAccessConfig]: {
    method: CONSOLE_RPC_METHODS.accessGet,
    targetKinds: /* @__PURE__ */ new Set(["mobkit/access"])
  },
  [CONSOLE_COMMAND_NAMES.setAccessConfig]: {
    method: CONSOLE_RPC_METHODS.accessSet,
    targetKinds: /* @__PURE__ */ new Set(["mobkit/access"])
  },
  [CONSOLE_COMMAND_NAMES.enableAccess]: {
    method: CONSOLE_RPC_METHODS.accessEnable,
    targetKinds: /* @__PURE__ */ new Set(["mobkit/access"])
  },
  [CONSOLE_COMMAND_NAMES.upsertAccessRule]: {
    method: CONSOLE_RPC_METHODS.accessRuleUpsert,
    targetKinds: /* @__PURE__ */ new Set(["mobkit/access"])
  },
  [CONSOLE_COMMAND_NAMES.deleteAccessRule]: {
    method: CONSOLE_RPC_METHODS.accessRuleDelete,
    targetKinds: /* @__PURE__ */ new Set(["mobkit/access"])
  },
  [CONSOLE_COMMAND_NAMES.setAccessGroup]: {
    method: CONSOLE_RPC_METHODS.accessGroupSet,
    targetKinds: /* @__PURE__ */ new Set(["mobkit/access"])
  },
  [CONSOLE_COMMAND_NAMES.deleteAccessGroup]: {
    method: CONSOLE_RPC_METHODS.accessGroupDelete,
    targetKinds: /* @__PURE__ */ new Set(["mobkit/access"])
  },
  [CONSOLE_COMMAND_NAMES.previewAccess]: {
    method: CONSOLE_RPC_METHODS.accessPreview,
    targetKinds: /* @__PURE__ */ new Set(["mobkit/access"])
  },
  [CONSOLE_COMMAND_NAMES.listMemoryRecords]: {
    method: CONSOLE_RPC_METHODS.memoryPanelRecords,
    targetKinds: /* @__PURE__ */ new Set(["mobkit/memory"])
  },
  [CONSOLE_COMMAND_NAMES.getMemoryRecord]: {
    method: CONSOLE_RPC_METHODS.memoryPanelRecord,
    targetKinds: /* @__PURE__ */ new Set(["mobkit/memory"])
  },
  [CONSOLE_COMMAND_NAMES.listMemoryQuarantine]: {
    method: CONSOLE_RPC_METHODS.memoryPanelQuarantine,
    targetKinds: /* @__PURE__ */ new Set(["mobkit/memory"])
  },
  [CONSOLE_COMMAND_NAMES.listMemoryDreams]: {
    method: CONSOLE_RPC_METHODS.memoryPanelDreams,
    targetKinds: /* @__PURE__ */ new Set(["mobkit/memory"])
  },
  [CONSOLE_COMMAND_NAMES.getMemoryOverview]: {
    method: CONSOLE_RPC_METHODS.memoryPanelOverview,
    targetKinds: /* @__PURE__ */ new Set(["mobkit/memory"])
  },
  [CONSOLE_COMMAND_NAMES.listMemoryProposals]: {
    method: CONSOLE_RPC_METHODS.memoryPanelProposals,
    targetKinds: /* @__PURE__ */ new Set(["mobkit/memory"])
  },
  [CONSOLE_COMMAND_NAMES.listMemoryInjections]: {
    method: CONSOLE_RPC_METHODS.memoryPanelInjections,
    targetKinds: /* @__PURE__ */ new Set(["mobkit/memory"])
  },
  [CONSOLE_COMMAND_NAMES.listMemoryHarvests]: {
    method: CONSOLE_RPC_METHODS.memoryPanelHarvests,
    targetKinds: /* @__PURE__ */ new Set(["mobkit/memory"])
  },
  [CONSOLE_COMMAND_NAMES.listMemoryDreamRuns]: {
    method: CONSOLE_RPC_METHODS.memoryPanelDreamRuns,
    targetKinds: /* @__PURE__ */ new Set(["mobkit/memory"])
  },
  [CONSOLE_COMMAND_NAMES.listMemoryAuditVerdicts]: {
    method: CONSOLE_RPC_METHODS.memoryPanelAuditVerdicts,
    targetKinds: /* @__PURE__ */ new Set(["mobkit/memory"])
  },
  [CONSOLE_COMMAND_NAMES.workgraphSnapshot]: {
    method: CONSOLE_RPC_METHODS.workgraphSnapshot,
    targetKinds: /* @__PURE__ */ new Set(["mobkit/workgraph"])
  },
  [CONSOLE_COMMAND_NAMES.workgraphEvents]: {
    method: CONSOLE_RPC_METHODS.workgraphEvents,
    targetKinds: /* @__PURE__ */ new Set(["mobkit/workgraph"])
  },
  [CONSOLE_COMMAND_NAMES.workgraphGoalStatus]: {
    method: CONSOLE_RPC_METHODS.workgraphGoalStatus,
    targetKinds: /* @__PURE__ */ new Set(["mobkit/workgraph"])
  },
  [CONSOLE_COMMAND_NAMES.workgraphClaim]: {
    method: CONSOLE_RPC_METHODS.workgraphClaim,
    targetKinds: /* @__PURE__ */ new Set(["mobkit/workgraph"])
  },
  [CONSOLE_COMMAND_NAMES.workgraphRelease]: {
    method: CONSOLE_RPC_METHODS.workgraphRelease,
    targetKinds: /* @__PURE__ */ new Set(["mobkit/workgraph"])
  },
  [CONSOLE_COMMAND_NAMES.workgraphClose]: {
    method: CONSOLE_RPC_METHODS.workgraphClose,
    targetKinds: /* @__PURE__ */ new Set(["mobkit/workgraph"])
  },
  [CONSOLE_COMMAND_NAMES.workgraphGoalConfirm]: {
    method: CONSOLE_RPC_METHODS.workgraphGoalConfirm,
    targetKinds: /* @__PURE__ */ new Set(["mobkit/workgraph"])
  },
  [CONSOLE_COMMAND_NAMES.workgraphGoalRequestClose]: {
    method: CONSOLE_RPC_METHODS.workgraphGoalRequestClose,
    targetKinds: /* @__PURE__ */ new Set(["mobkit/workgraph"])
  },
  [CONSOLE_COMMAND_NAMES.workgraphAttentionPause]: {
    method: CONSOLE_RPC_METHODS.workgraphAttentionPause,
    targetKinds: /* @__PURE__ */ new Set(["mobkit/workgraph"])
  },
  [CONSOLE_COMMAND_NAMES.workgraphAttentionResume]: {
    method: CONSOLE_RPC_METHODS.workgraphAttentionResume,
    targetKinds: /* @__PURE__ */ new Set(["mobkit/workgraph"])
  },
  [CONSOLE_COMMAND_NAMES.workgraphAttentionReassign]: {
    method: CONSOLE_RPC_METHODS.workgraphAttentionReassign,
    targetKinds: /* @__PURE__ */ new Set(["mobkit/workgraph"])
  }
};
var identityCommandMethods = /* @__PURE__ */ new Set([
  CONSOLE_RPC_METHODS.inspectIdentity,
  CONSOLE_RPC_METHODS.retireIdentity,
  CONSOLE_RPC_METHODS.respawnIdentity,
  CONSOLE_RPC_METHODS.resetIdentity
]);

// ../packages/console-core/src/targets.ts
var LEGACY_CONTROL_TARGETS = {
  topology: "mobkit/topology",
  health: "mobkit/activity",
  timeline: "mobkit/activity",
  roster: "mobkit/roster",
  routing: "mobkit/routing",
  gating: "mobkit/gating",
  gates: "mobkit/gating",
  access: "mobkit/access",
  memory: "mobkit/memory",
  workgraph: "mobkit/workgraph",
  logs: "mobkit/logs"
};
function migrateConsoleWorkbenchTarget(input) {
  if (!isRecord(input)) {
    return null;
  }
  const id = stringValue(input.id);
  const kind = stringValue(input.kind);
  const title = stringValue(input.title);
  if (!id || !kind || !title) {
    return null;
  }
  if (kind === "agent-chat") {
    const identity = stringValue(input.identity) || stringValue(input.memberId) || id;
    return {
      ...baseTarget(input, id, "mobkit/identity-chat", title),
      identity,
      memberId: stringValue(input.memberId),
      addressingMode: "identity"
    };
  }
  if (kind === "identity-inspect") {
    const identity = stringValue(input.identity) || stringValue(input.memberId) || id.replace(/^inspect:/, "");
    return {
      ...baseTarget(input, id, "mobkit/identity-inspect", title),
      identity,
      memberId: stringValue(input.memberId)
    };
  }
  const controlKind = LEGACY_CONTROL_TARGETS[kind];
  if (controlKind) {
    return baseTarget(input, id, controlKind, title);
  }
  if (kind.startsWith("mobkit/")) {
    return normalizeMobKitWorkbenchTarget(input, id, kind, title);
  }
  if (isNamespacedKind(kind) && kind !== "mobkit/unknown") {
    const payloadVersion = typeof input.payloadVersion === "number" && Number.isSafeInteger(input.payloadVersion) ? input.payloadVersion : 1;
    return {
      ...baseTarget(input, id, kind, title),
      payloadVersion,
      payload: input.payload,
      provenance: "host"
    };
  }
  return null;
}
function normalizeMobKitWorkbenchTarget(input, id, kind, title) {
  if (kind === "mobkit/identity-chat") {
    const identity = stringValue(input.identity) || stringValue(input.memberId);
    if (!identity) return null;
    return {
      ...baseTarget(input, id, kind, title),
      identity,
      memberId: stringValue(input.memberId),
      addressingMode: "identity"
    };
  }
  if (kind === "mobkit/identity-inspect") {
    const identity = stringValue(input.identity) || stringValue(input.memberId);
    if (!identity) return null;
    return {
      ...baseTarget(input, id, kind, title),
      identity,
      memberId: stringValue(input.memberId)
    };
  }
  if (Object.values(LEGACY_CONTROL_TARGETS).includes(kind)) {
    return baseTarget(input, id, kind, title);
  }
  return null;
}
function baseTarget(input, id, kind, title) {
  return {
    id,
    kind,
    title,
    subtitle: stringValue(input.subtitle),
    iconName: stringValue(input.iconName),
    badgeLabel: stringValue(input.badgeLabel)
  };
}
function isRecord(value) {
  return Boolean(value && typeof value === "object" && !Array.isArray(value));
}
function stringValue(value) {
  return typeof value === "string" && value.trim() ? value.trim() : void 0;
}
function isNamespacedKind(kind) {
  const [namespace, name, ...rest] = kind.split("/");
  return Boolean(namespace && name && rest.length === 0);
}

// ../packages/console-components/src/composer/console-composer.tsx
var import_jsx_runtime4 = require("react/jsx-runtime");

// ../packages/console-components/src/conversation/conversation-pane.tsx
var import_react6 = require("react");

// ../packages/console-components/src/conversation/conversation-message-view.tsx
var import_react5 = require("react");

// ../packages/console-components/src/conversation/conversation-rich-content.tsx
var import_react2 = require("react");

// ../packages/console-components/src/conversation/change-stat-pair.tsx
var import_jsx_runtime5 = require("react/jsx-runtime");
function ChangeStatPair({
  plus,
  minus,
  className
}) {
  return /* @__PURE__ */ (0, import_jsx_runtime5.jsxs)("span", { className: clsx_default("cc-change-stat", className), children: [
    /* @__PURE__ */ (0, import_jsx_runtime5.jsxs)("span", { className: "cc-change-stat__value is-plus", children: [
      "+",
      formatCount(plus)
    ] }),
    /* @__PURE__ */ (0, import_jsx_runtime5.jsxs)("span", { className: "cc-change-stat__value is-minus", children: [
      "-",
      formatCount(minus)
    ] })
  ] });
}

// ../packages/console-components/src/conversation/conversation-rich-content.tsx
var import_jsx_runtime6 = require("react/jsx-runtime");
function markdownHtml(text, displayNormalization = true) {
  return { __html: renderConversationInlineMarkdown(text, { displayNormalization }) };
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
function renderThinkingBlock(block, displayNormalization = true) {
  if (!block.label?.trim() && !block.text?.trim()) {
    return null;
  }
  const collapsedByDefault = Boolean(block.final && block.persisted);
  return /* @__PURE__ */ (0, import_jsx_runtime6.jsxs)(
    "details",
    {
      className: clsx_default(
        "cc-rich-thinking",
        block.final && "cc-rich-thinking--final",
        block.persisted && "cc-rich-thinking--persisted",
        collapsedByDefault && "cc-rich-thinking--collapsed"
      ),
      open: !collapsedByDefault,
      children: [
        /* @__PURE__ */ (0, import_jsx_runtime6.jsx)("summary", { className: "cc-rich-thinking__label", children: block.label }),
        /* @__PURE__ */ (0, import_jsx_runtime6.jsx)("p", { className: "cc-rich-paragraph cc-rich-thinking__body", dangerouslySetInnerHTML: markdownHtml(block.text, displayNormalization) })
      ]
    }
  );
}
function renderBlock(block, index, Icon3, displayNormalization = true) {
  if (block.type === "paragraph") {
    return /* @__PURE__ */ (0, import_jsx_runtime6.jsx)("p", { className: "cc-rich-paragraph", dangerouslySetInnerHTML: markdownHtml(block.text, displayNormalization) }, `paragraph-${index}`);
  }
  if (block.type === "heading") {
    return /* @__PURE__ */ (0, import_jsx_runtime6.jsx)(
      "h3",
      {
        className: `cc-rich-heading cc-rich-heading--${Number(block.level) || 2}`,
        dangerouslySetInnerHTML: markdownHtml(block.text, displayNormalization)
      },
      `heading-${index}`
    );
  }
  if (block.type === "code") {
    const codeBlock = block;
    return /* @__PURE__ */ (0, import_jsx_runtime6.jsxs)("section", { className: "cc-rich-code-card", children: [
      /* @__PURE__ */ (0, import_jsx_runtime6.jsxs)("div", { className: "cc-rich-code-card__header", children: [
        /* @__PURE__ */ (0, import_jsx_runtime6.jsx)("span", { className: "cc-rich-code-language", children: codeBlock.language || "text" }),
        /* @__PURE__ */ (0, import_jsx_runtime6.jsx)(
          CopyButton,
          {
            copiedLabel: "Copied code",
            Icon: Icon3,
            label: "Copy code",
            text: codeBlock.body
          }
        )
      ] }),
      /* @__PURE__ */ (0, import_jsx_runtime6.jsx)("pre", { className: "cc-rich-code-body", children: codeBlock.highlightedHtml ? /* @__PURE__ */ (0, import_jsx_runtime6.jsx)(
        "code",
        {
          className: `cc-rich-code-content language-${codeBlock.language || "text"}`,
          dangerouslySetInnerHTML: { __html: codeBlock.highlightedHtml }
        }
      ) : /* @__PURE__ */ (0, import_jsx_runtime6.jsx)("code", { className: `cc-rich-code-content language-${codeBlock.language || "text"}`, children: codeBlock.body }) })
    ] }, `code-${index}`);
  }
  if (block.type === "table") {
    return /* @__PURE__ */ (0, import_jsx_runtime6.jsx)("div", { className: "cc-rich-table-wrap", children: /* @__PURE__ */ (0, import_jsx_runtime6.jsxs)("table", { className: "cc-rich-table", children: [
      /* @__PURE__ */ (0, import_jsx_runtime6.jsx)("thead", { children: /* @__PURE__ */ (0, import_jsx_runtime6.jsx)("tr", { children: block.headers.map((header, cellIndex) => /* @__PURE__ */ (0, import_jsx_runtime6.jsx)(
        "th",
        {
          "data-align": alignmentAttr(block.alignments[cellIndex]),
          dangerouslySetInnerHTML: markdownHtml(header, displayNormalization)
        },
        `header-${cellIndex}`
      )) }) }),
      /* @__PURE__ */ (0, import_jsx_runtime6.jsx)("tbody", { children: block.rows.map((row, rowIndex) => /* @__PURE__ */ (0, import_jsx_runtime6.jsx)("tr", { children: block.headers.map((_header, cellIndex) => /* @__PURE__ */ (0, import_jsx_runtime6.jsx)(
        "td",
        {
          "data-align": alignmentAttr(block.alignments[cellIndex]),
          dangerouslySetInnerHTML: markdownHtml(row[cellIndex] || "", displayNormalization)
        },
        `cell-${rowIndex}-${cellIndex}`
      )) }, `row-${rowIndex}`)) })
    ] }) }, `table-${index}`);
  }
  if (block.type === "command") {
    return /* @__PURE__ */ (0, import_jsx_runtime6.jsxs)("div", { className: "cc-rich-command-stack", children: [
      /* @__PURE__ */ (0, import_jsx_runtime6.jsx)("div", { className: "cc-rich-command-caption", children: block.caption }),
      /* @__PURE__ */ (0, import_jsx_runtime6.jsxs)("div", { className: "cc-rich-command-card", children: [
        /* @__PURE__ */ (0, import_jsx_runtime6.jsxs)("div", { className: "cc-rich-command-card__header", children: [
          /* @__PURE__ */ (0, import_jsx_runtime6.jsx)("div", { className: "cc-rich-command-card__title", children: block.title }),
          /* @__PURE__ */ (0, import_jsx_runtime6.jsx)(
            CopyButton,
            {
              copiedLabel: "Copied command output",
              Icon: Icon3,
              label: "Copy command output",
              text: commandCopyText(block)
            }
          )
        ] }),
        /* @__PURE__ */ (0, import_jsx_runtime6.jsx)("pre", { className: "cc-rich-command-card__body", children: block.body }),
        block.output ? /* @__PURE__ */ (0, import_jsx_runtime6.jsx)("pre", { className: "cc-rich-command-card__output", children: block.output }) : null,
        block.footer ? /* @__PURE__ */ (0, import_jsx_runtime6.jsx)("div", { className: "cc-rich-command-card__footer", children: block.footer }) : null
      ] })
    ] }, `command-${index}`);
  }
  if (block.type === "file-change") {
    return /* @__PURE__ */ (0, import_jsx_runtime6.jsxs)("section", { className: "cc-rich-file-change", children: [
      /* @__PURE__ */ (0, import_jsx_runtime6.jsxs)("div", { className: "cc-rich-file-change__main", children: [
        /* @__PURE__ */ (0, import_jsx_runtime6.jsx)("span", { className: "cc-rich-file-change__verb", children: block.verb }),
        block.before ? /* @__PURE__ */ (0, import_jsx_runtime6.jsx)("span", { className: "cc-rich-file-change__context", dangerouslySetInnerHTML: markdownHtml(block.before, displayNormalization) }) : null,
        /* @__PURE__ */ (0, import_jsx_runtime6.jsx)("button", { className: "cc-rich-file-change__link", type: "button", children: block.name }),
        block.after ? /* @__PURE__ */ (0, import_jsx_runtime6.jsx)("span", { className: "cc-rich-file-change__context", dangerouslySetInnerHTML: markdownHtml(block.after, displayNormalization) }) : null
      ] }),
      /* @__PURE__ */ (0, import_jsx_runtime6.jsxs)("div", { className: "cc-rich-file-change__stats", children: [
        /* @__PURE__ */ (0, import_jsx_runtime6.jsx)(ChangeStatPair, { minus: block.minus, plus: block.plus }),
        /* @__PURE__ */ (0, import_jsx_runtime6.jsx)("span", { className: "cc-rich-file-change__dot" }),
        /* @__PURE__ */ (0, import_jsx_runtime6.jsx)(
          CopyButton,
          {
            copiedLabel: "Copied file change",
            Icon: Icon3,
            label: "Copy file change",
            text: fileChangeCopyText(block)
          }
        )
      ] })
    ] }, `file-change-${index}`);
  }
  if (block.type === "divider") {
    return /* @__PURE__ */ (0, import_jsx_runtime6.jsxs)("div", { className: "cc-rich-divider", children: [
      /* @__PURE__ */ (0, import_jsx_runtime6.jsx)("span", { className: "cc-rich-divider__line" }),
      /* @__PURE__ */ (0, import_jsx_runtime6.jsx)("span", { className: "cc-rich-divider__label", children: block.text }),
      /* @__PURE__ */ (0, import_jsx_runtime6.jsx)("span", { className: "cc-rich-divider__line" })
    ] }, `divider-${index}`);
  }
  if (block.type === "image") {
    const image = block;
    return /* @__PURE__ */ (0, import_jsx_runtime6.jsx)(
      "button",
      {
        className: "cc-rich-image-button",
        onClick: () => window.open(image.src, "_blank", "noopener,noreferrer"),
        type: "button",
        children: /* @__PURE__ */ (0, import_jsx_runtime6.jsx)(
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
    return /* @__PURE__ */ (0, import_jsx_runtime6.jsx)(ToolCallBlock, { block }, `tool-call-${index}`);
  }
  const thinking = renderThinkingBlock(block, displayNormalization);
  if (!thinking) {
    return null;
  }
  return /* @__PURE__ */ (0, import_jsx_runtime6.jsx)("div", { children: thinking }, `thinking-${index}`);
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
    const peerBody = conversationRichPeerBodyForDisplay(block.peerBody);
    const result = meaningfulPeerResult(block.result);
    return [
      `${dir} ${conversationRichPeerTargetForDisplay(block.peerTarget)}`,
      conversationRichPeerIntentForDisplay(block.peerIntent, peerBody),
      peerBody,
      result
    ].filter(Boolean).join(": ").trim();
  }
  const parts = [`$ ${block.name}`];
  if (block.arguments) parts.push(`Input: ${block.arguments}`);
  if (block.result) parts.push(`Result: ${block.result}`);
  return parts.join("\n").trim();
}
function parseObjectJson(text) {
  const trimmed = String(text || "").trim();
  if (!trimmed || !trimmed.startsWith("{") || !trimmed.endsWith("}")) {
    return null;
  }
  try {
    const parsed = JSON.parse(trimmed);
    return parsed && typeof parsed === "object" && !Array.isArray(parsed) ? parsed : null;
  } catch {
    return null;
  }
}
function textFromUnknown(value) {
  if (value == null) {
    return "";
  }
  if (typeof value === "string") {
    return normalizeConversationDisplayText(value).trim();
  }
  return normalizeConversationDisplayText(JSON.stringify(value, null, 2));
}
function meaningfulPeerResult(value) {
  const text = normalizeConversationDisplayText(String(value || "")).trim();
  if (!text || /^(completed|delivered|ok|success)$/i.test(text)) {
    return "";
  }
  return formatJsonIfPossible(text);
}
function peerDetailRows(block) {
  const args = parseObjectJson(block.arguments) || {};
  const peerBody = conversationRichPeerBodyForDisplay(block.peerBody);
  const peerIntent = conversationRichPeerIntentForDisplay(block.peerIntent, peerBody);
  const body = peerBody || textFromUnknown(args.body) || textFromUnknown(args.message) || textFromUnknown(args.content) || textFromUnknown(args.text);
  const params = textFromUnknown(args.params);
  const requestId = textFromUnknown(args.in_reply_to) || textFromUnknown(args.inReplyTo) || textFromUnknown(args.request_id) || textFromUnknown(args.requestId);
  const result = meaningfulPeerResult(block.result);
  const primaryLabel = block.name === "send_request" ? "Request" : block.name === "send_response" ? "Response" : "Message";
  return [
    body ? { label: primaryLabel, value: body } : null,
    peerIntent ? { label: "Intent", value: peerIntent } : null,
    params ? { label: "Params", value: params } : null,
    requestId ? { label: "Request ID", value: requestId } : null,
    result ? { label: "Result", value: result } : null
  ].filter(Boolean);
}
function CopyBtn({ text, label = "Copy" }) {
  const [copied, setCopied] = (0, import_react2.useState)(false);
  return /* @__PURE__ */ (0, import_jsx_runtime6.jsx)(
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
function onToolHeaderKeyDown(event, toggle) {
  if (event.key !== "Enter" && event.key !== " ") {
    return;
  }
  event.preventDefault();
  toggle();
}
function ToolCallBlock({ block }) {
  const [expanded, setExpanded] = (0, import_react2.useState)(false);
  const isPeer = PEER_TOOL_NAMES.has(block.name);
  const statusIcon = block.status === "success" ? "\u2713" : block.status === "error" ? "\u2717" : "\u22EF";
  const statusClass = `cc-tool-call--${block.status}`;
  if (isPeer || block.peerIncoming) {
    const target = conversationRichPeerTargetForDisplay(block.peerTarget);
    const peerBody = conversationRichPeerBodyForDisplay(block.peerBody);
    const peerIntent = conversationRichPeerIntentForDisplay(block.peerIntent, peerBody);
    const content = peerBody || peerIntent || "";
    const arrow = block.peerIncoming ? "\u2199" : "\u2197";
    const detailRows = peerDetailRows(block);
    return /* @__PURE__ */ (0, import_jsx_runtime6.jsxs)("section", { className: clsx_default("cc-tool-call cc-tool-call--peer", block.peerIncoming && "cc-tool-call--incoming", statusClass), children: [
      /* @__PURE__ */ (0, import_jsx_runtime6.jsxs)(
        "div",
        {
          className: "cc-tool-call__header",
          role: "button",
          tabIndex: 0,
          onClick: () => setExpanded((prev) => !prev),
          onKeyDown: (event) => onToolHeaderKeyDown(event, () => setExpanded((prev) => !prev)),
          "aria-expanded": expanded,
          children: [
            /* @__PURE__ */ (0, import_jsx_runtime6.jsx)("span", { className: "cc-tool-call__chevron", children: expanded ? "\u25BE" : "\u25B8" }),
            /* @__PURE__ */ (0, import_jsx_runtime6.jsx)("span", { className: "cc-tool-call__icon", children: arrow }),
            /* @__PURE__ */ (0, import_jsx_runtime6.jsx)("span", { className: "cc-tool-call__name", children: block.peerIncoming ? `Received from ${target}` : target }),
            peerIntent && /* @__PURE__ */ (0, import_jsx_runtime6.jsx)("span", { className: "cc-tool-call__peer-intent", children: peerIntent }),
            content && /* @__PURE__ */ (0, import_jsx_runtime6.jsx)("span", { className: "cc-tool-call__preview", children: content }),
            /* @__PURE__ */ (0, import_jsx_runtime6.jsx)("span", { className: "cc-tool-call__status", children: statusIcon }),
            /* @__PURE__ */ (0, import_jsx_runtime6.jsx)(CopyBtn, { text: toolBlockCopyText(block) })
          ]
        }
      ),
      block.peerImages && block.peerImages.length > 0 && /* @__PURE__ */ (0, import_jsx_runtime6.jsx)("div", { className: "cc-tool-call__attachments", children: block.peerImages.map((image, index) => /* @__PURE__ */ (0, import_jsx_runtime6.jsx)(
        "button",
        {
          className: "cc-tool-call__image-button",
          onClick: () => window.open(image.src, "_blank", "noopener,noreferrer"),
          type: "button",
          children: /* @__PURE__ */ (0, import_jsx_runtime6.jsx)(
            "img",
            {
              alt: image.alt || "",
              className: "cc-tool-call__image",
              height: image.height,
              loading: "lazy",
              src: image.src,
              width: image.width
            }
          )
        },
        `${image.blobId || image.imageId || image.src}-${index}`
      )) }),
      expanded && detailRows.length > 0 && /* @__PURE__ */ (0, import_jsx_runtime6.jsx)("div", { className: "cc-tool-call__body", children: detailRows.map((row) => /* @__PURE__ */ (0, import_jsx_runtime6.jsxs)("div", { className: "cc-tool-call__section", children: [
        /* @__PURE__ */ (0, import_jsx_runtime6.jsx)("div", { className: "cc-tool-call__section-label", children: row.label }),
        /* @__PURE__ */ (0, import_jsx_runtime6.jsx)("pre", { className: "cc-tool-call__pre", children: formatJsonIfPossible(row.value) })
      ] }, `${row.label}:${row.value}`)) })
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
  return /* @__PURE__ */ (0, import_jsx_runtime6.jsxs)("section", { className: clsx_default("cc-tool-call", statusClass), children: [
    /* @__PURE__ */ (0, import_jsx_runtime6.jsxs)(
      "div",
      {
        className: "cc-tool-call__header",
        role: "button",
        tabIndex: 0,
        onClick: () => setExpanded((prev) => !prev),
        onKeyDown: (event) => onToolHeaderKeyDown(event, () => setExpanded((prev) => !prev)),
        "aria-expanded": expanded,
        children: [
          /* @__PURE__ */ (0, import_jsx_runtime6.jsx)("span", { className: "cc-tool-call__chevron", children: expanded ? "\u25BE" : "\u25B8" }),
          /* @__PURE__ */ (0, import_jsx_runtime6.jsx)("span", { className: "cc-tool-call__icon", children: "\u2699" }),
          /* @__PURE__ */ (0, import_jsx_runtime6.jsx)("span", { className: "cc-tool-call__name", children: block.name }),
          argsPreview && /* @__PURE__ */ (0, import_jsx_runtime6.jsx)("span", { className: "cc-tool-call__preview", children: argsPreview }),
          /* @__PURE__ */ (0, import_jsx_runtime6.jsxs)("span", { className: "cc-tool-call__status", children: [
            statusIcon,
            " ",
            block.status === "pending" ? "Running" : block.status === "success" ? "Success" : "Failed"
          ] }),
          /* @__PURE__ */ (0, import_jsx_runtime6.jsx)(CopyBtn, { text: toolBlockCopyText(block) })
        ]
      }
    ),
    expanded && /* @__PURE__ */ (0, import_jsx_runtime6.jsxs)("div", { className: "cc-tool-call__body", children: [
      argsPreview && /* @__PURE__ */ (0, import_jsx_runtime6.jsxs)("div", { className: "cc-tool-call__section", children: [
        /* @__PURE__ */ (0, import_jsx_runtime6.jsx)("div", { className: "cc-tool-call__section-label", children: "Input" }),
        /* @__PURE__ */ (0, import_jsx_runtime6.jsx)("pre", { className: "cc-tool-call__pre", children: argsPreview })
      ] }),
      block.result && /* @__PURE__ */ (0, import_jsx_runtime6.jsxs)("div", { className: "cc-tool-call__section", children: [
        /* @__PURE__ */ (0, import_jsx_runtime6.jsx)("div", { className: "cc-tool-call__section-label", children: "Result" }),
        /* @__PURE__ */ (0, import_jsx_runtime6.jsx)("pre", { className: "cc-tool-call__pre", children: block.result })
      ] })
    ] })
  ] });
}
function ToolCallGroup({ blocks }) {
  const [expanded, setExpanded] = (0, import_react2.useState)(false);
  const allSuccess = blocks.every((b) => b.status === "success");
  const anyError = blocks.some((b) => b.status === "error");
  const statusIcon = anyError ? "\u2717" : allSuccess ? "\u2713" : "\u22EF";
  const statusLabel2 = anyError ? "Failed" : allSuccess ? "Success" : "Running";
  const statusClass = anyError ? "cc-tool-call--error" : allSuccess ? "cc-tool-call--success" : "cc-tool-call--pending";
  const name = blocks[0]?.name || "tool";
  return /* @__PURE__ */ (0, import_jsx_runtime6.jsxs)("section", { className: clsx_default("cc-tool-call cc-tool-call--group", statusClass), children: [
    /* @__PURE__ */ (0, import_jsx_runtime6.jsxs)(
      "div",
      {
        className: "cc-tool-call__header",
        role: "button",
        tabIndex: 0,
        onClick: () => setExpanded((prev) => !prev),
        onKeyDown: (event) => onToolHeaderKeyDown(event, () => setExpanded((prev) => !prev)),
        "aria-expanded": expanded,
        children: [
          /* @__PURE__ */ (0, import_jsx_runtime6.jsx)("span", { className: "cc-tool-call__chevron", children: expanded ? "\u25BE" : "\u25B8" }),
          /* @__PURE__ */ (0, import_jsx_runtime6.jsx)("span", { className: "cc-tool-call__icon", children: "\u2699" }),
          /* @__PURE__ */ (0, import_jsx_runtime6.jsx)("span", { className: "cc-tool-call__name", children: name }),
          /* @__PURE__ */ (0, import_jsx_runtime6.jsxs)("span", { className: "cc-tool-call__count", children: [
            "\xD7",
            blocks.length
          ] }),
          /* @__PURE__ */ (0, import_jsx_runtime6.jsxs)("span", { className: "cc-tool-call__status", children: [
            statusIcon,
            " ",
            statusLabel2
          ] }),
          /* @__PURE__ */ (0, import_jsx_runtime6.jsx)(CopyBtn, { text: blocks.map((b) => toolBlockCopyText(b)).join("\n") })
        ]
      }
    ),
    expanded && /* @__PURE__ */ (0, import_jsx_runtime6.jsx)("div", { className: "cc-tool-call__body", children: blocks.map((block, i) => {
      const args = block.arguments ? formatJsonIfPossible(block.arguments) : "";
      const result = block.result ? formatJsonIfPossible(block.result) : "";
      return /* @__PURE__ */ (0, import_jsx_runtime6.jsxs)("div", { className: "cc-tool-call__sub", children: [
        /* @__PURE__ */ (0, import_jsx_runtime6.jsxs)("div", { className: "cc-tool-call__sub-head", children: [
          /* @__PURE__ */ (0, import_jsx_runtime6.jsxs)("span", { className: "cc-tool-call__sub-index", children: [
            "#",
            i + 1
          ] }),
          /* @__PURE__ */ (0, import_jsx_runtime6.jsx)("span", { className: `cc-tool-call__peer-status cc-tool-call__peer-status--${block.status}`, children: block.status === "success" ? "\u2713" : block.status === "error" ? "\u2717" : "\u22EF" })
        ] }),
        args && /* @__PURE__ */ (0, import_jsx_runtime6.jsxs)("div", { className: "cc-tool-call__section", children: [
          /* @__PURE__ */ (0, import_jsx_runtime6.jsx)("div", { className: "cc-tool-call__section-label", children: "Input" }),
          /* @__PURE__ */ (0, import_jsx_runtime6.jsx)("pre", { className: "cc-tool-call__pre", children: args })
        ] }),
        result && /* @__PURE__ */ (0, import_jsx_runtime6.jsxs)("div", { className: "cc-tool-call__section", children: [
          /* @__PURE__ */ (0, import_jsx_runtime6.jsx)("div", { className: "cc-tool-call__section-label", children: "Result" }),
          /* @__PURE__ */ (0, import_jsx_runtime6.jsx)("pre", { className: "cc-tool-call__pre", children: result })
        ] })
      ] }, block.toolCallId || i);
    }) })
  ] });
}
function PeerToolGroup({ blocks }) {
  const [expanded, setExpanded] = (0, import_react2.useState)(false);
  const targets = Array.from(new Set(blocks.map((b) => conversationRichPeerTargetForDisplay(b.peerTarget))));
  const allSuccess = blocks.every((b) => b.status === "success");
  const anyError = blocks.some((b) => b.status === "error");
  const statusIcon = anyError ? "\u2717" : allSuccess ? "\u2713" : "\u22EF";
  const statusClass = anyError ? "cc-tool-call--error" : allSuccess ? "cc-tool-call--success" : "cc-tool-call--pending";
  const isIncoming = blocks[0]?.peerIncoming;
  const arrow = isIncoming ? "\u2199" : "\u2197";
  const label = isIncoming ? `Received from ${targets.join(", ")}` : `Sent to ${targets.join(", ")}`;
  return /* @__PURE__ */ (0, import_jsx_runtime6.jsxs)("section", { className: clsx_default("cc-tool-call cc-tool-call--peer-group", isIncoming && "cc-tool-call--incoming", statusClass), children: [
    /* @__PURE__ */ (0, import_jsx_runtime6.jsxs)(
      "div",
      {
        className: "cc-tool-call__header",
        role: "button",
        tabIndex: 0,
        onClick: () => setExpanded((prev) => !prev),
        onKeyDown: (event) => onToolHeaderKeyDown(event, () => setExpanded((prev) => !prev)),
        "aria-expanded": expanded,
        children: [
          /* @__PURE__ */ (0, import_jsx_runtime6.jsx)("span", { className: "cc-tool-call__chevron", children: expanded ? "\u25BE" : "\u25B8" }),
          /* @__PURE__ */ (0, import_jsx_runtime6.jsx)("span", { className: "cc-tool-call__icon", children: arrow }),
          /* @__PURE__ */ (0, import_jsx_runtime6.jsx)("span", { className: "cc-tool-call__name", children: label }),
          /* @__PURE__ */ (0, import_jsx_runtime6.jsx)("span", { className: "cc-tool-call__status", children: statusIcon }),
          /* @__PURE__ */ (0, import_jsx_runtime6.jsx)(CopyBtn, { text: blocks.map((b) => toolBlockCopyText(b)).join("\n") })
        ]
      }
    ),
    expanded && /* @__PURE__ */ (0, import_jsx_runtime6.jsx)("div", { className: "cc-tool-call__body", children: blocks.map((block, i) => {
      const peerBody = conversationRichPeerBodyForDisplay(block.peerBody);
      const peerIntent = conversationRichPeerIntentForDisplay(block.peerIntent, peerBody);
      return /* @__PURE__ */ (0, import_jsx_runtime6.jsxs)("div", { className: "cc-tool-call__peer-row", children: [
        /* @__PURE__ */ (0, import_jsx_runtime6.jsxs)("span", { className: "cc-tool-call__peer-target", children: [
          isIncoming ? "\u2190" : "\u2192",
          " ",
          conversationRichPeerTargetForDisplay(block.peerTarget)
        ] }),
        peerIntent ? /* @__PURE__ */ (0, import_jsx_runtime6.jsx)("span", { className: "cc-tool-call__peer-intent", children: peerIntent }) : null,
        peerBody && /* @__PURE__ */ (0, import_jsx_runtime6.jsx)("span", { className: "cc-tool-call__peer-body", children: peerBody }),
        /* @__PURE__ */ (0, import_jsx_runtime6.jsx)("span", { className: `cc-tool-call__peer-status cc-tool-call__peer-status--${block.status}`, children: block.status === "success" ? "\u2713" : block.status === "error" ? "\u2717" : "\u22EF" })
      ] }, block.toolCallId || i);
    }) })
  ] });
}
function ConversationRichContent({
  blocks,
  richStyle = "default",
  Icon: Icon3,
  displayNormalization = true
}) {
  if (blocks.length > 1 && blocks.every((b) => b.type === "tool-call")) {
    const tools = blocks;
    const firstName = tools[0].name;
    if (tools.every((b) => b.name === firstName)) {
      const allPeer = tools.every((b) => PEER_TOOL_NAMES.has(b.name) || b.peerIncoming);
      if (allPeer) {
        return /* @__PURE__ */ (0, import_jsx_runtime6.jsx)(PeerToolGroup, { blocks: tools });
      }
      return /* @__PURE__ */ (0, import_jsx_runtime6.jsx)(ToolCallGroup, { blocks: tools });
    }
  }
  const body = blocks.map((block, index) => renderBlock(block, index, Icon3, displayNormalization)).filter(Boolean);
  if (body.length === 0) {
    return null;
  }
  if (richStyle === "streaming") {
    return /* @__PURE__ */ (0, import_jsx_runtime6.jsx)("div", { className: "cc-rich-streaming", children: body });
  }
  return /* @__PURE__ */ (0, import_jsx_runtime6.jsx)(import_jsx_runtime6.Fragment, { children: body });
}

// ../packages/console-components/src/conversation/flow-run-card.tsx
var import_react3 = require("react");
var import_jsx_runtime7 = require("react/jsx-runtime");

// ../packages/console-components/src/conversation/summary-card.tsx
var import_jsx_runtime8 = require("react/jsx-runtime");

// ../packages/console-components/src/conversation/work-graph-card.tsx
var import_react4 = require("react");
var import_jsx_runtime9 = require("react/jsx-runtime");
var CARD_STATUS_LABEL = {
  active: "Active",
  blocked: "Blocked",
  completed: "Done",
  failed: "Failed",
  mixed: "Mixed"
};
var ITEM_STATUS_LABEL = {
  open: "Open",
  in_progress: "In progress",
  blocked: "Blocked",
  completed: "Done",
  cancelled: "Cancelled",
  failed: "Failed"
};
var expandedWorkGraphItems = /* @__PURE__ */ new Set();
var collapsedWorkGraphCards = /* @__PURE__ */ new Set();
function rememberFlag(registry, key, value) {
  if (value) registry.add(key);
  else registry.delete(key);
}
function itemStatusLabel(status) {
  return ITEM_STATUS_LABEL[status] || status.replace(/_/g, " ");
}
function formatDay(iso) {
  return typeof iso === "string" && iso.length >= 10 ? iso.slice(0, 10) : "";
}
function formatClock(iso) {
  if (!iso) return "";
  const date = new Date(iso);
  if (Number.isNaN(date.getTime())) return "";
  const hh = String(date.getHours()).padStart(2, "0");
  const mm = String(date.getMinutes()).padStart(2, "0");
  return `${hh}:${mm}`;
}
function itemRowHasDetail(row) {
  return Boolean(
    row.description || row.evidence && row.evidence.length > 0 || row.labels && row.labels.length > 0 || row.alsoUnder && row.alsoUnder.length > 0 || row.createdAt || row.updatedAt
  );
}
function ItemRow({
  row,
  actions
}) {
  const [expanded, setExpandedState] = (0, import_react4.useState)(() => expandedWorkGraphItems.has(row.itemId));
  const setExpanded = (update) => {
    setExpandedState((value) => {
      const next = update(value);
      rememberFlag(expandedWorkGraphItems, row.itemId, next);
      return next;
    });
  };
  const hasDetail = itemRowHasDetail(row);
  const terminal = row.status === "completed" || row.status === "cancelled" || row.status === "failed";
  const canClaim = Boolean(actions?.onClaim) && row.status === "open" && !row.ownerLabel;
  const canClose = Boolean(actions?.onClose) && !terminal && row.status !== "blocked";
  const dueDay = formatDay(row.dueAt);
  return /* @__PURE__ */ (0, import_jsx_runtime9.jsxs)(
    "li",
    {
      className: `cc-work-graph__item is-${row.status}${expanded ? " is-expanded" : ""}`,
      "data-workgraph-item": row.itemId,
      "data-item-status": row.status,
      "data-revision": row.revision,
      children: [
        /* @__PURE__ */ (0, import_jsx_runtime9.jsxs)("div", { className: "cc-work-graph__item-line", style: { paddingLeft: `${row.depth * 18}px` }, children: [
          /* @__PURE__ */ (0, import_jsx_runtime9.jsxs)(
            "button",
            {
              type: "button",
              className: "cc-work-graph__item-row",
              disabled: !hasDetail,
              "aria-expanded": hasDetail ? expanded : void 0,
              onClick: hasDetail ? () => setExpanded((value) => !value) : void 0,
              "data-testid": `workgraph-item:${row.itemId}`,
              children: [
                /* @__PURE__ */ (0, import_jsx_runtime9.jsx)("span", { className: `cc-work-graph__dot is-${row.status}`, "aria-hidden": "true" }),
                /* @__PURE__ */ (0, import_jsx_runtime9.jsx)("span", { className: "cc-work-graph__item-title", children: row.title }),
                row.priority && row.priority !== "medium" ? /* @__PURE__ */ (0, import_jsx_runtime9.jsx)("span", { className: `cc-work-graph__chip is-priority-${row.priority}`, children: row.priority }) : null,
                row.ownerLabel ? /* @__PURE__ */ (0, import_jsx_runtime9.jsx)("span", { className: "cc-work-graph__chip is-owner", title: `Owned by ${row.ownerLabel}`, children: row.ownerLabel }) : null,
                row.blocked ? /* @__PURE__ */ (0, import_jsx_runtime9.jsx)("span", { className: "cc-work-graph__chip is-blocked", children: "blocked" }) : null,
                dueDay ? /* @__PURE__ */ (0, import_jsx_runtime9.jsx)("span", { className: "cc-work-graph__chip is-due", title: "Due date", children: dueDay }) : null,
                hasDetail ? /* @__PURE__ */ (0, import_jsx_runtime9.jsx)("span", { className: "cc-work-graph__item-chevron", "aria-hidden": "true", children: expanded ? "\u25BE" : "\u25B8" }) : /* @__PURE__ */ (0, import_jsx_runtime9.jsx)("span", { className: "cc-work-graph__item-status", children: itemStatusLabel(row.status) })
              ]
            }
          ),
          canClaim ? /* @__PURE__ */ (0, import_jsx_runtime9.jsx)(
            "button",
            {
              type: "button",
              className: "cc-work-graph__action",
              title: `Claim ${row.title}`,
              "data-testid": `workgraph-action:${row.itemId}:claim`,
              onClick: (event) => {
                event.stopPropagation();
                actions?.onClaim?.({ itemId: row.itemId, revision: row.revision });
              },
              children: "Claim"
            }
          ) : null,
          canClose ? /* @__PURE__ */ (0, import_jsx_runtime9.jsx)(
            "button",
            {
              type: "button",
              className: "cc-work-graph__action",
              title: `Close ${row.title} as completed`,
              "data-testid": `workgraph-action:${row.itemId}:close`,
              onClick: (event) => {
                event.stopPropagation();
                actions?.onClose?.({ itemId: row.itemId, revision: row.revision });
              },
              children: "Done"
            }
          ) : null
        ] }),
        hasDetail && expanded ? /* @__PURE__ */ (0, import_jsx_runtime9.jsxs)("div", { className: "cc-work-graph__item-detail", style: { marginLeft: `${row.depth * 18 + 25}px` }, children: [
          row.description ? /* @__PURE__ */ (0, import_jsx_runtime9.jsx)("p", { className: "cc-work-graph__item-description", children: row.description }) : null,
          row.alsoUnder && row.alsoUnder.length > 0 ? /* @__PURE__ */ (0, import_jsx_runtime9.jsxs)(
            "p",
            {
              className: "cc-work-graph__item-also-under",
              "data-testid": `workgraph-item:${row.itemId}:also-under`,
              children: [
                "also under ",
                row.alsoUnder.join(", ")
              ]
            }
          ) : null,
          row.labels && row.labels.length > 0 ? /* @__PURE__ */ (0, import_jsx_runtime9.jsx)("div", { className: "cc-work-graph__item-labels", children: row.labels.map((label) => /* @__PURE__ */ (0, import_jsx_runtime9.jsx)("span", { className: "cc-work-graph__chip is-label", children: label }, label)) }) : null,
          row.evidence && row.evidence.length > 0 ? /* @__PURE__ */ (0, import_jsx_runtime9.jsx)("ul", { className: "cc-work-graph__evidence", children: row.evidence.map((line, index) => /* @__PURE__ */ (0, import_jsx_runtime9.jsx)("li", { children: line }, `${line}-${index}`)) }) : null,
          /* @__PURE__ */ (0, import_jsx_runtime9.jsxs)("div", { className: "cc-work-graph__item-meta", children: [
            /* @__PURE__ */ (0, import_jsx_runtime9.jsx)("span", { children: itemStatusLabel(row.status) }),
            typeof row.revision === "number" ? /* @__PURE__ */ (0, import_jsx_runtime9.jsxs)("span", { children: [
              "rev ",
              row.revision
            ] }) : null,
            row.updatedAt ? /* @__PURE__ */ (0, import_jsx_runtime9.jsxs)("span", { children: [
              "updated ",
              formatDay(row.updatedAt),
              " ",
              formatClock(row.updatedAt)
            ] }) : null
          ] })
        ] }) : null
      ]
    }
  );
}
function attentionIsPaused(row) {
  return row.statusLabel.startsWith("paused");
}
function attentionIsActive(row) {
  return row.statusLabel === "active";
}
function AttentionRow({
  row,
  goalRevision,
  actions
}) {
  const bindingInput = { bindingId: row.bindingId, revision: row.revision };
  const goalInput = { bindingId: row.bindingId, revision: goalRevision };
  const live = attentionIsActive(row) || attentionIsPaused(row);
  const buttons = [];
  if (actions?.onAttentionPause && attentionIsActive(row)) {
    buttons.push({
      key: "pause",
      label: "Pause",
      title: "Pause this attention binding",
      onClick: () => actions.onAttentionPause?.(bindingInput)
    });
  }
  if (actions?.onAttentionResume && attentionIsPaused(row)) {
    buttons.push({
      key: "resume",
      label: "Resume",
      title: "Resume this attention binding",
      onClick: () => actions.onAttentionResume?.(bindingInput)
    });
  }
  if (actions?.onGoalConfirm && live) {
    buttons.push({
      key: "confirm",
      label: "Confirm",
      title: "Confirm goal completion",
      onClick: () => actions.onGoalConfirm?.(goalInput)
    });
  }
  if (actions?.onGoalRequestClose && live) {
    buttons.push({
      key: "request-close",
      label: "Request close",
      title: "Request goal closure",
      onClick: () => actions.onGoalRequestClose?.(goalInput)
    });
  }
  if (actions?.onAttentionReassign && live && row.mode === "coordinate") {
    buttons.push({
      key: "reassign",
      label: "Reassign",
      title: "Reassign this attention binding",
      onClick: () => actions.onAttentionReassign?.(bindingInput)
    });
  }
  return /* @__PURE__ */ (0, import_jsx_runtime9.jsxs)(
    "li",
    {
      className: `cc-work-graph__attention-row${attentionIsPaused(row) ? " is-paused" : ""}`,
      "data-workgraph-binding": row.bindingId,
      children: [
        /* @__PURE__ */ (0, import_jsx_runtime9.jsx)("span", { className: `cc-work-graph__mode is-${row.mode}`, children: row.mode }),
        /* @__PURE__ */ (0, import_jsx_runtime9.jsx)("span", { className: "cc-work-graph__attention-status", children: row.statusLabel }),
        row.targetLabel ? /* @__PURE__ */ (0, import_jsx_runtime9.jsx)("span", { className: "cc-work-graph__attention-target", title: "Attention target", children: row.targetLabel }) : null,
        /* @__PURE__ */ (0, import_jsx_runtime9.jsx)("span", { className: "cc-work-graph__attention-spacer" }),
        buttons.map((button) => /* @__PURE__ */ (0, import_jsx_runtime9.jsx)(
          "button",
          {
            type: "button",
            className: "cc-work-graph__action",
            title: button.title,
            "data-testid": `workgraph-attention:${row.bindingId}:${button.key}`,
            onClick: (event) => {
              event.stopPropagation();
              button.onClick();
            },
            children: button.label
          },
          button.key
        ))
      ]
    }
  );
}
function WorkGraphCard({
  entry,
  Icon: Icon3,
  actions = null
}) {
  const uiStateKey = entry.uiStateKey || entry.id;
  const [collapsed, setCollapsedState] = (0, import_react4.useState)(() => collapsedWorkGraphCards.has(uiStateKey));
  const setCollapsed = (update) => {
    setCollapsedState((value) => {
      const next = update(value);
      rememberFlag(collapsedWorkGraphCards, uiStateKey, next);
      return next;
    });
  };
  const { completed, total } = entry.progress;
  const percent = total > 0 ? Math.round(completed / total * 100) : 0;
  const hasBody = entry.items.length > 0 || entry.attention.length > 0 || Boolean(entry.recentEvents && entry.recentEvents.length > 0);
  const revisionByItemId = new Map(entry.items.map((row) => [row.itemId, row.revision]));
  const goalRevisionFor = (row) => row.itemId != null && revisionByItemId.has(row.itemId) ? revisionByItemId.get(row.itemId) : revisionByItemId.get(entry.rootId);
  return /* @__PURE__ */ (0, import_jsx_runtime9.jsxs)(
    "section",
    {
      className: `cc-work-graph is-${entry.status}${collapsed ? " is-collapsed" : ""}`,
      "data-work-graph-card": "",
      "data-root-id": entry.rootId,
      "data-status": entry.status,
      "data-testid": `workgraph-card:${entry.rootId}`,
      children: [
        /* @__PURE__ */ (0, import_jsx_runtime9.jsxs)("header", { className: "cc-work-graph__header", children: [
          /* @__PURE__ */ (0, import_jsx_runtime9.jsx)("span", { className: "cc-work-graph__mark", "aria-hidden": "true", children: Icon3 ? /* @__PURE__ */ (0, import_jsx_runtime9.jsx)(Icon3, { name: "i-cube" }) : "\u25C8" }),
          /* @__PURE__ */ (0, import_jsx_runtime9.jsxs)("div", { className: "cc-work-graph__heading", children: [
            /* @__PURE__ */ (0, import_jsx_runtime9.jsx)("span", { className: "cc-work-graph__title", children: entry.title }),
            entry.objective ? /* @__PURE__ */ (0, import_jsx_runtime9.jsx)("span", { className: "cc-work-graph__objective", children: entry.objective }) : null
          ] }),
          total > 0 ? /* @__PURE__ */ (0, import_jsx_runtime9.jsxs)(
            "div",
            {
              className: "cc-work-graph__progress",
              role: "progressbar",
              "aria-valuemin": 0,
              "aria-valuemax": total,
              "aria-valuenow": completed,
              "aria-label": `${completed} of ${total} work items completed`,
              children: [
                /* @__PURE__ */ (0, import_jsx_runtime9.jsxs)("span", { className: "cc-work-graph__progress-count", children: [
                  completed,
                  "/",
                  total
                ] }),
                /* @__PURE__ */ (0, import_jsx_runtime9.jsx)("span", { className: "cc-work-graph__progress-track", children: /* @__PURE__ */ (0, import_jsx_runtime9.jsx)("span", { className: "cc-work-graph__progress-fill", style: { width: `${percent}%` } }) })
              ]
            }
          ) : null,
          /* @__PURE__ */ (0, import_jsx_runtime9.jsxs)("span", { className: `cc-work-graph__badge is-${entry.status}`, children: [
            entry.status === "active" ? /* @__PURE__ */ (0, import_jsx_runtime9.jsx)("span", { className: "cc-work-graph__pulse", "aria-hidden": "true" }) : null,
            CARD_STATUS_LABEL[entry.status]
          ] }),
          entry.lastActionFailed ? /* @__PURE__ */ (0, import_jsx_runtime9.jsx)(
            "span",
            {
              className: "cc-work-graph__last-failed",
              title: "The last WorkGraph action failed",
              "data-testid": `workgraph-card:${entry.rootId}:last-action-failed`,
              children: "\u2717"
            }
          ) : null,
          hasBody ? /* @__PURE__ */ (0, import_jsx_runtime9.jsx)(
            "button",
            {
              type: "button",
              className: "cc-work-graph__collapse",
              "aria-expanded": !collapsed,
              "aria-label": collapsed ? "Expand work graph" : "Collapse work graph",
              "data-testid": `workgraph-card:${entry.rootId}:toggle`,
              onClick: () => setCollapsed((value) => !value),
              children: collapsed ? "\u25B8" : "\u25BE"
            }
          ) : null
        ] }),
        !collapsed && entry.items.length > 0 ? /* @__PURE__ */ (0, import_jsx_runtime9.jsx)("ul", { className: "cc-work-graph__items", children: entry.items.map((row) => /* @__PURE__ */ (0, import_jsx_runtime9.jsx)(ItemRow, { row, actions }, row.itemId)) }) : null,
        !collapsed && entry.attention.length > 0 ? /* @__PURE__ */ (0, import_jsx_runtime9.jsx)("ul", { className: "cc-work-graph__attention", children: entry.attention.map((row) => /* @__PURE__ */ (0, import_jsx_runtime9.jsx)(
          AttentionRow,
          {
            row,
            goalRevision: goalRevisionFor(row),
            actions
          },
          row.bindingId
        )) }) : null,
        !collapsed && entry.recentEvents && entry.recentEvents.length > 0 ? /* @__PURE__ */ (0, import_jsx_runtime9.jsx)("div", { className: "cc-work-graph__events", children: entry.recentEvents.map((line, index) => /* @__PURE__ */ (0, import_jsx_runtime9.jsx)("div", { className: "cc-work-graph__event", children: line }, `${line}-${index}`)) }) : null
      ]
    }
  );
}

// ../packages/console-components/src/conversation/conversation-message-view.tsx
var import_jsx_runtime10 = require("react/jsx-runtime");

// ../packages/console-components/src/conversation/conversation-message-group.tsx
var import_jsx_runtime11 = require("react/jsx-runtime");

// ../packages/console-components/src/conversation/turn-diff-card.tsx
var import_jsx_runtime12 = require("react/jsx-runtime");

// ../packages/console-components/src/conversation/conversation-transcript.tsx
var import_jsx_runtime13 = require("react/jsx-runtime");

// ../packages/console-components/src/conversation/conversation-pane.tsx
var import_jsx_runtime14 = require("react/jsx-runtime");

// ../packages/console-components/src/conversation/console-conversation-panel.tsx
var import_jsx_runtime15 = require("react/jsx-runtime");

// ../packages/console-components/src/dock/console-dock.tsx
var import_react7 = require("react");
var import_jsx_runtime16 = require("react/jsx-runtime");

// ../packages/console-components/src/pending/console-pending-stack.tsx
var import_react8 = __toESM(require("react"));
var import_jsx_runtime17 = require("react/jsx-runtime");

// ../packages/console-components/src/dock/use-console-dock-controller.ts
var import_react9 = require("react");
function useConsoleDockController({
  initialTarget = null,
  initialPresetId = "single",
  createPanelState,
  suggestTargets,
  resolvePanelView,
  resolveTabView
}) {
  const panelCounterRef = (0, import_react9.useRef)(1);
  const splitCounterRef = (0, import_react9.useRef)(1);
  const tabCounterRef = (0, import_react9.useRef)(1);
  function nextPanelId() {
    return `panel-${panelCounterRef.current++}`;
  }
  function nextSplitId() {
    return `split-${splitCounterRef.current++}`;
  }
  function nextTabId() {
    return `tab-${tabCounterRef.current++}`;
  }
  const [state, setState] = (0, import_react9.useState)(() => createConsoleDockState({
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
  const viewState = (0, import_react9.useMemo)(() => buildConsoleDockViewState(state, {
    resolvePanelView,
    resolveTabView
  }), [resolvePanelView, resolveTabView, state]);
  const focusedPanel = (0, import_react9.useMemo)(
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
var import_react10 = require("react");
var import_jsx_runtime18 = require("react/jsx-runtime");

// ../packages/console-components/src/topology/topology-panel.tsx
var import_react14 = __toESM(require("react"));

// ../packages/console-components/src/topology/role-tree.tsx
var import_react12 = __toESM(require("react"));

// ../packages/console-components/src/topology/data.ts
var import_react11 = __toESM(require("react"));

// ../packages/console-components/src/topology/role-tree.tsx
var import_jsx_runtime19 = require("react/jsx-runtime");

// ../packages/console-components/src/topology/dense-graph-map.tsx
var import_react13 = __toESM(require("react"));
var import_jsx_runtime20 = require("react/jsx-runtime");
var GOLDEN_ANGLE = Math.PI * (3 - Math.sqrt(5));

// ../packages/console-components/src/topology/topology-panel.tsx
var import_jsx_runtime21 = require("react/jsx-runtime");

// ../packages/console-components/src/workbench/console-workbench.tsx
var import_jsx_runtime22 = require("react/jsx-runtime");

// ../packages/console-components/src/composer/pending-stack.tsx
var import_react15 = __toESM(require("react"));
var import_jsx_runtime23 = require("react/jsx-runtime");

// src/lib/agents.ts
function canonicalConsoleIdentity(identity, agents) {
  const normalized = identity?.trim() || "";
  if (!normalized) return "";
  for (const agent of agents) {
    const labelIdentity = typeof agent.labels?.agent_identity === "string" ? agent.labels.agent_identity.trim() : "";
    const aliases = [
      agent.identity,
      agent.member_id,
      agent.agent_id,
      labelIdentity
    ].filter((value) => Boolean(value?.trim())).map((value) => value.trim());
    if (!aliases.includes(normalized)) continue;
    return (agent.identity || agent.member_id || agent.agent_id || normalized).trim();
  }
  return normalized;
}
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
      const entryLabels = entry.labels && typeof entry.labels === "object" ? entry.labels : {};
      const durableAgentIdentity = typeof entryLabels.agent_identity === "string" ? entryLabels.agent_identity.trim() : "";
      const statusRow = identityStatusByIdentity.get(durableAgentIdentity) || identityStatusByIdentity.get(entryIdentity) || identityStatusByIdentity.get(entryMemberId) || normalizeIdentityStatusRow(entry);
      const watchFields = normalizeSidebarWatchFields(entry);
      const responsePhase = normalizeResponsePhase(entry.response_phase);
      const modelCapabilities = entry.model_capabilities !== void 0 ? normalizeModelCapabilities(entry) : normalizeModelCapabilities(identityStatusRows.find((row) => {
        const normalized = normalizeIdentityStatusRow(row);
        return normalized?.identity === statusRow?.identity;
      }));
      return {
        ...statusRow?.identity ? { identity: statusRow.identity } : durableAgentIdentity ? { identity: durableAgentIdentity } : entry.identity ? { identity: String(entry.identity) } : {},
        agent_id: String(entry.agent_id || statusRow?.identity || durableAgentIdentity || entry.identity || entry.member_id || ""),
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

// src/lib/id.ts
function randomUuidFromValues(cryptoSource) {
  if (typeof cryptoSource.getRandomValues !== "function") {
    return null;
  }
  try {
    const bytes = cryptoSource.getRandomValues(new Uint8Array(16));
    bytes[6] = bytes[6] & 15 | 64;
    bytes[8] = bytes[8] & 63 | 128;
    const hex = Array.from(bytes, (byte) => byte.toString(16).padStart(2, "0"));
    return [
      hex.slice(0, 4).join(""),
      hex.slice(4, 6).join(""),
      hex.slice(6, 8).join(""),
      hex.slice(8, 10).join(""),
      hex.slice(10, 16).join("")
    ].join("-");
  } catch {
    return null;
  }
}
function createConsoleId(prefix = "console", cryptoSource = typeof globalThis.crypto !== "undefined" ? globalThis.crypto : void 0) {
  if (cryptoSource && typeof cryptoSource.randomUUID === "function") {
    try {
      return `${prefix}-${cryptoSource.randomUUID()}`;
    } catch {
    }
  }
  const generated = cryptoSource ? randomUuidFromValues(cryptoSource) : null;
  if (generated) {
    return `${prefix}-${generated}`;
  }
  return `${prefix}-${Date.now().toString(36)}-${Math.random().toString(36).slice(2)}`;
}

// src/lib/adapters.ts
function buildPanelConversationKey2(panelId, target) {
  if (!target) {
    return `panel:${panelId}:none`;
  }
  if (target.kind !== "agent-chat") {
    return `panel:${panelId}:${target.kind}:${target.id}`;
  }
  const targetKey = target.identity || target.memberId || target.id;
  return `panel:${panelId}:${target.kind}:${targetKey}`;
}
function optimisticUserMessageForPanel2(optimisticByPanelKey, panelKey, identity) {
  const direct = optimisticByPanelKey[panelKey];
  if (direct) return direct;
  const identitySuffix = `:agent-chat:${identity}`;
  let latest = null;
  for (const [key, optimistic] of Object.entries(optimisticByPanelKey)) {
    if (!key.endsWith(identitySuffix)) continue;
    if (!latest || optimistic.sentAtMs > latest.sentAtMs) latest = optimistic;
  }
  return latest;
}
function buildDockTarget2(agent) {
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
function buildInspectTarget2(agent) {
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
function buildControlTarget2(kind) {
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
    case "access":
      return { id: "access", kind, title: "Access", subtitle: "Who can see and do what", iconName: "i-gear" };
    case "memory":
      return { id: "memory", kind, title: "Memory", subtitle: "Records, quarantine, dreams", iconName: "i-archive" };
    case "workgraph":
      return { id: "workgraph", kind, title: "WorkGraph", subtitle: "Goals, work items, and attention", iconName: "i-cube" };
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
function sidebarAgentPinId2(agent) {
  return agent.identity?.trim() || agent.labels?.agent_identity?.trim() || agent.member_id.trim();
}
function isAgentPinned2(agent, pinnedAgentIds) {
  if (!pinnedAgentIds) return false;
  return pinnedAgentIds.has(sidebarAgentPinId2(agent)) || pinnedAgentIds.has(agent.member_id);
}
function buildSidebarViewState2(args) {
  const { agents, selectedMemberId, pinnedAgentIds = /* @__PURE__ */ new Set(), sortMode = "group" } = args;
  const sorted = [...agents].sort((a, b) => {
    const aPinned = isAgentPinned2(a, pinnedAgentIds) ? 0 : 1;
    const bPinned = isAgentPinned2(b, pinnedAgentIds) ? 0 : 1;
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
      const isPinned = isAgentPinned2(agent, pinnedAgentIds);
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
function buildRoutingSectionView2(args) {
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
function memoryString(data, key) {
  const value = data[key];
  return typeof value === "string" && value.trim() ? value.trim() : "";
}
function memoryNumber(data, key) {
  const value = data[key];
  return typeof value === "number" && Number.isFinite(value) ? value : null;
}
function memoryScopeLabel(data) {
  const kind = memoryString(data, "scope_kind");
  const key = memoryString(data, "scope_key");
  if (kind && key) return `${kind}:${key}`;
  return kind || key || "";
}
function humanizeMemoryEvent(event) {
  const suffix = event.startsWith("memory.") ? event.slice("memory.".length) : event;
  const words = suffix.split(/[._]/).filter(Boolean);
  if (words.length === 0) return "Memory event";
  return `Memory ${words.join(" ")}`;
}
function describeMemoryTimelineEvent2(event, data) {
  switch (event) {
    case "memory.dream.started":
      return `Dream started${memoryString(data, "run_id") ? ` (${memoryString(data, "run_id")})` : ""}`;
    case "memory.dream.completed": {
      const ops = memoryNumber(data, "ops_committed");
      const detail = memoryString(data, "detail");
      const opsText = ops !== null ? `${ops} op${ops === 1 ? "" : "s"} committed` : "completed";
      return `Dream ${opsText}${detail ? ` \u2014 ${detail}` : ""}`;
    }
    case "memory.dream.skipped":
      return `Dream skipped${memoryString(data, "reason") ? ` \u2014 ${memoryString(data, "reason")}` : ""}`;
    case "memory.record.promoted": {
      const scope = memoryScopeLabel(data);
      const gated = data.gated === true ? " (gated)" : "";
      return `Record promoted${scope ? ` to ${scope}` : ""}${gated}`;
    }
    case "memory.quarantine.verdict": {
      const verdict = memoryString(data, "verdict") || "decided";
      const rationale = memoryString(data, "rationale");
      return `Quarantine verdict: ${verdict}${rationale ? ` \u2014 ${rationale}` : ""}`;
    }
    case "memory.quarantine.release_blocked": {
      const verdict = memoryString(data, "verdict");
      const action = verdict === "promote_pending_gate" ? "promotion" : verdict || "release";
      const record = memoryString(data, "record_id");
      const cls = memoryString(data, "class");
      return `Quarantine ${action} blocked${record ? ` for ${record}` : ""}${cls ? ` \u2014 matches secret pattern ${cls}` : ""}`;
    }
    case "memory.conflict.signal": {
      const entity = memoryString(data, "entity");
      const topic = memoryString(data, "topic");
      const subject = [entity, topic].filter(Boolean).join(" / ");
      const reason = memoryString(data, "reason");
      return `Conflict signal${subject ? ` on ${subject}` : ""}${reason ? ` \u2014 ${reason}` : ""}`;
    }
    case "memory.write.quarantined": {
      const author = memoryString(data, "author");
      const reason = memoryString(data, "reason");
      return `Write quarantined${author ? ` from ${author}` : ""}${reason ? ` \u2014 ${reason}` : ""}`;
    }
    case "memory.taint.transition": {
      const kind = memoryString(data, "kind") || "changed";
      const label = kind === "tainted" ? "Session tainted" : kind === "reset_boundary" ? "Reset boundary" : kind === "rotated_clean" ? "Rotated clean" : `Taint ${kind}`;
      const source = memoryString(data, "source");
      const session = memoryString(data, "session_key");
      const context = [session, source].filter(Boolean).join(" \xB7 ");
      return `${label}${context ? ` (${context})` : ""}`;
    }
    case "memory.budget.denied": {
      const stage = memoryString(data, "stage");
      const reason = memoryString(data, "reason");
      return `Budget denied${stage ? ` at ${stage}` : ""}${reason ? ` \u2014 ${reason}` : ""}`;
    }
    case "memory.promotion.pending_gate": {
      const scope = memoryScopeLabel(data);
      return `Promotion awaiting gate${scope ? ` for ${scope}` : ""}`;
    }
    case "memory.harvest.completed": {
      const promoted = memoryNumber(data, "promoted");
      const tombstoned = memoryNumber(data, "tombstoned");
      const parts = [];
      if (promoted !== null) parts.push(`${promoted} promoted`);
      if (tombstoned !== null) parts.push(`${tombstoned} tombstoned`);
      const identity = memoryString(data, "identity");
      return `Harvest completed${parts.length ? ` \u2014 ${parts.join(", ")}` : ""}${identity ? ` (${identity})` : ""}`;
    }
    case "memory.distill.timed_out": {
      const cause = memoryString(data, "cause");
      const session = memoryString(data, "session_key");
      return `Distill timed out${cause ? ` \u2014 ${cause}` : ""}${session ? ` (${session})` : ""}`;
    }
    case "memory.hygiene.proposed":
    case "memory.hygiene.applied":
    case "memory.hygiene.blocked":
    case "memory.hygiene.skipped": {
      const phase = event.slice("memory.hygiene.".length);
      const cause = memoryString(data, "cause");
      const ops = memoryNumber(data, "ops");
      const reason = memoryString(data, "reason");
      const detail = reason || (ops !== null ? `${ops} op${ops === 1 ? "" : "s"}` : "") || cause;
      return `Hygiene ${phase}${detail ? ` \u2014 ${detail}` : ""}`;
    }
    default: {
      const reason = memoryString(data, "reason") || memoryString(data, "detail") || memoryString(data, "cause") || memoryString(data, "verdict");
      return `${humanizeMemoryEvent(event)}${reason ? ` \u2014 ${reason}` : ""}`;
    }
  }
}
function isSteerDeliveryTerminalFrame(frame) {
  if (frame.event !== "interaction_complete") return false;
  if (!frame.data || typeof frame.data !== "object") return false;
  const record = frame.data;
  return record.reason === "steer_delivered";
}
function eventSortRank(event) {
  switch (event) {
    case "user_input":
    case "interaction_started":
    case "run_started":
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
    // Reasoning ("thinking") precedes the answer text it leads to; rank it before
    // text so equal-timestamp (or timestamp-less) reasoning frames don't sort
    // after the answer.
    case "reasoning_delta":
    case "reasoning_complete":
      return 38;
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
function isInteractionStartEvent(event) {
  return event === "user_input" || event === "interaction_started" || event === "run_started";
}
function cursorSeq(cursor) {
  if (!cursor) return null;
  const match = /^console:(\d+)$/.exec(cursor);
  if (!match) return null;
  const parsed = Number(match[1]);
  return Number.isFinite(parsed) ? parsed : null;
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
  const transcriptGroupTimestamp = (frame) => {
    const interactionId = frame.interactionId?.trim() || "";
    const ownTimestamp = typeof frame.timestampMs === "number" ? frame.timestampMs : Number.MAX_SAFE_INTEGER;
    if (!interactionId) return ownTimestamp;
    return interactionStartMs.get(interactionId) ?? ownTimestamp;
  };
  return frames.map((frame, index) => ({ frame, index })).sort((left, right) => {
    const leftInteraction = left.frame.interactionId?.trim() || "";
    const rightInteraction = right.frame.interactionId?.trim() || "";
    const leftGroupTs = transcriptGroupTimestamp(left.frame);
    const rightGroupTs = transcriptGroupTimestamp(right.frame);
    if (leftGroupTs !== rightGroupTs) {
      return leftGroupTs - rightGroupTs;
    }
    if (leftInteraction && rightInteraction && leftInteraction === rightInteraction) {
      const leftStarts = isInteractionStartEvent(left.frame.event);
      const rightStarts = isInteractionStartEvent(right.frame.event);
      if (leftStarts !== rightStarts) {
        return leftStarts ? -1 : 1;
      }
    }
    const leftTs = typeof left.frame.timestampMs === "number" ? left.frame.timestampMs : leftGroupTs;
    const rightTs = typeof right.frame.timestampMs === "number" ? right.frame.timestampMs : rightGroupTs;
    if (leftTs !== rightTs) {
      return leftTs - rightTs;
    }
    if (leftInteraction && rightInteraction && leftInteraction === rightInteraction) {
      const leftRank = eventSortRank(left.frame.event);
      const rightRank = eventSortRank(right.frame.event);
      if (leftRank !== rightRank) {
        return leftRank - rightRank;
      }
    }
    const leftCursor = cursorSeq(left.frame.cursor);
    const rightCursor = cursorSeq(right.frame.cursor);
    if (leftCursor !== null && rightCursor !== null && leftCursor !== rightCursor) {
      return leftCursor - rightCursor;
    }
    return left.index - right.index;
  }).map(({ frame }) => frame);
}
var HIDDEN_EVENTS2 = /* @__PURE__ */ new Set([
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
  "snapshot_complete",
  "snapshot_started",
  "run_failed",
  "keep-alive",
  "server_tool_content",
  "tool_config_changed",
  "tool_scope_changed"
]);
function appendDistinctText(parts, value) {
  const text = value.trim();
  if (!text) return;
  const comparable = normalizeComparableText(text);
  if (parts.some((part) => normalizeComparableText(part) === comparable)) return;
  parts.push(text);
}
function textFromReasoningValue(value) {
  if (typeof value === "string") return value.trim();
  if (Array.isArray(value)) {
    return value.map((item) => textFromReasoningValue(item)).filter(Boolean).join("\n\n").trim();
  }
  if (!value || typeof value !== "object") return "";
  const record = value;
  const parts = [];
  appendDistinctText(parts, textFromReasoningValue(record.summary));
  appendDistinctText(parts, textFromReasoningValue(record.text));
  appendDistinctText(parts, textFromReasoningValue(record.content));
  appendDistinctText(parts, textFromReasoningValue(record.delta));
  return parts.join("\n\n").trim();
}
function reasoningBlockText(block) {
  const data = block.data && typeof block.data === "object" ? block.data : block;
  const parts = [];
  appendDistinctText(parts, textFromReasoningValue(data.summary));
  appendDistinctText(parts, textFromReasoningValue(data.text));
  appendDistinctText(parts, textFromReasoningValue(data.content));
  appendDistinctText(parts, textFromReasoningValue(block.summary));
  appendDistinctText(parts, textFromReasoningValue(block.text));
  appendDistinctText(parts, textFromReasoningValue(block.content));
  return parts.join("\n\n").trim();
}
function reasoningFrameText(frame) {
  const data = frame.data && typeof frame.data === "object" ? frame.data : frame.data;
  if (frame.event === "reasoning_delta" && typeof data?.delta === "string") {
    return data.delta;
  }
  return textFromReasoningValue(data).trim();
}
var ACTIVITY_HIDDEN_EVENTS2 = /* @__PURE__ */ new Set([
  ...HIDDEN_EVENTS2,
  "text_delta",
  "tool_call_requested",
  "tool_call",
  "server_tool_content",
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
  if (frame.event === "server_tool_content") {
    const content = record?.content && typeof record.content === "object" ? record.content : null;
    const type = typeof content?.type === "string" ? content.type : "";
    const isAnnotationPayload = type === "message_annotations" || Array.isArray(content?.annotations);
    const id2 = isAnnotationPayload ? content?.item_id ?? record?.item_id ?? record?.tool_call_id : content?.item_id ?? content?.id ?? record?.item_id ?? record?.tool_call_id ?? record?.id;
    return typeof id2 === "string" && id2.trim() ? id2.trim() : null;
  }
  const id = record?.tool_call_id ?? record?.id;
  return typeof id === "string" && id.trim() ? id.trim() : null;
}
function parseToolName(frame) {
  const record = frame.data && typeof frame.data === "object" ? frame.data : null;
  if (frame.event === "server_tool_content") {
    const content = record?.content && typeof record.content === "object" ? record.content : null;
    const name = content?.name ?? record?.tool_name ?? record?.name;
    return typeof name === "string" && name.trim() ? name.trim() : "tool";
  }
  return typeof record?.name === "string" && record.name.trim() ? record.name : "tool";
}
function parseToolArguments(frame) {
  const record = frame.data && typeof frame.data === "object" ? frame.data : null;
  if (frame.event === "server_tool_content") {
    const content = record?.content && typeof record.content === "object" ? record.content : null;
    const action = content?.action && typeof content.action === "object" ? content.action : null;
    const queries = Array.isArray(action?.queries) ? action.queries.filter((query2) => typeof query2 === "string" && query2.trim().length > 0) : [];
    const query = queries.length > 0 ? queries.join("\n") : content?.query ?? content?.input ?? action?.query;
    return typeof query === "string" && query.trim() ? query.trim() : "";
  }
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
  for (const frame of frames) {
    if (frame.sourceKind === "session_history") continue;
    if (frame.event !== "tool_call_requested" && frame.event !== "tool_call" && frame.event !== "tool_execution_started" && frame.event !== "server_tool_content") {
      continue;
    }
    const toolCallId = parseToolCallId(frame);
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
  const resultText2 = typeof rawResult === "string" ? rawResult : rawResult && typeof rawResult === "object" ? JSON.stringify(rawResult) : "";
  if (!resultText2) return;
  try {
    const parsed = JSON.parse(resultText2);
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
function parseToolResult(frame) {
  const record = frame.data && typeof frame.data === "object" ? frame.data : null;
  const isError = Boolean(record?.is_error) || frame.event === "interaction_failed";
  let result = "";
  const toolName2 = typeof record?.name === "string" ? record.name : typeof record?.tool_name === "string" ? record.tool_name : void 0;
  if (typeof record?.result === "string") {
    const display = summarizeToolResultForDisplay(toolName2, record.result);
    if (display) {
      result = display;
    } else {
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
    }
  } else if (typeof record?.result === "object" && record.result !== null) {
    result = summarizeToolResultForDisplay(toolName2, record.result) || "";
    if (!result) {
      const clean = { ...record.result };
      delete clean.source_event_type;
      delete clean.type;
      result = JSON.stringify(clean, null, 2);
    }
  }
  if (!result && frame.event === "tool_result_received") {
    return { status: isError ? "error" : "success" };
  }
  return {
    ...result ? { result } : {},
    status: isError ? "error" : "success"
  };
}
function buildToolBlocks(frames, workGraphNamesByCallId) {
  const toolCalls = /* @__PURE__ */ new Map();
  const pendingResults = /* @__PURE__ */ new Map();
  const peerRegistry = buildPeerRegistry(frames);
  for (const frame of frames) {
    if (isWorkGraphToolFrame(frame, workGraphNamesByCallId)) continue;
    if (frame.event === "server_tool_content") {
      const toolCallId = parseToolCallId(frame);
      const parsed = serverToolContentSummary(frame);
      if (!toolCallId) {
        if (parsed && parsed.status !== "pending") {
          const name = parseToolName(frame);
          const targetName = name.replace(/_annotations$/, "");
          const existing2 = [...toolCalls.values()].reverse().find(
            (block) => !block.result && (block.name === targetName || block.name === name || name.startsWith(block.name))
          );
          if (existing2) {
            toolCalls.set(existing2.toolCallId, {
              ...existing2,
              ...parsed.result ? { result: parsed.result } : {},
              status: parsed.status
            });
          }
        }
        continue;
      }
      const existing = toolCalls.get(toolCallId);
      if (existing) {
        toolCalls.set(toolCallId, {
          ...existing,
          ...parsed?.result ? { result: parsed.result } : {},
          status: parsed?.status || existing.status
        });
        continue;
      }
      toolCalls.set(toolCallId, {
        type: "tool-call",
        toolCallId,
        name: parseToolName(frame),
        arguments: parseToolArguments(frame),
        ...parsed?.result ? { result: parsed.result } : {},
        status: parsed?.status || "pending"
      });
      continue;
    }
    if (frame.event === "tool_result_received" || frame.event === "tool_execution_completed") {
      const toolCallId = parseToolCallId(frame);
      const data = frame.data;
      if (data && (data.name === "peers" || data.tool_name === "peers")) {
        capturePeersResult(peerRegistry, data.result);
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
      const peerTarget2 = isPeerTool ? peerTargetFromArgs(argsRecord, peerRegistry) : void 0;
      const rawPeerIntent = isPeerTool && typeof argsRecord?.intent === "string" ? argsRecord.intent : void 0;
      const peerIntent = displayPeerIntent(rawPeerIntent);
      const peerBody = isPeerTool ? extractPeerBodyFromArgs(argsRecord) : void 0;
      toolCalls.set(toolCallId, {
        type: "tool-call",
        toolCallId,
        name,
        arguments: parseToolArguments(frame),
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
  for (const frame of frames) {
    if (frame.event !== "tool_result_received" && frame.event !== "tool_execution_completed") continue;
    const data = frame.data && typeof frame.data === "object" ? frame.data : null;
    if (!data || data.name !== "peers" && data.tool_name !== "peers") continue;
    capturePeersResult(peerRegistry, data.result);
  }
  return peerRegistry;
}
var WORKGRAPH_TOOL_NAMES = /* @__PURE__ */ new Set([
  "workgraph_create",
  "workgraph_get",
  "workgraph_list",
  "workgraph_ready",
  "workgraph_snapshot",
  "workgraph_events",
  "workgraph_claim",
  "workgraph_release",
  "workgraph_update",
  "workgraph_policy_escalate",
  "workgraph_block",
  "workgraph_close",
  "workgraph_link",
  "workgraph_add_evidence",
  "workgraph_attention_reassign"
]);
var WORKGRAPH_TOOL_EVENTS = /* @__PURE__ */ new Set([
  "tool_call_requested",
  "tool_call",
  "tool_execution_started",
  "tool_result_received",
  "tool_execution_completed"
]);
var WORKGRAPH_OPERATOR_RESULT_EVENT = "workgraph_operator_result";
var WORKGRAPH_LOCAL_SOURCE_KIND = "console-local";
function buildWorkGraphOperatorResultFrame(input) {
  return {
    id: input.frameId || createConsoleId("local-workgraph"),
    event: WORKGRAPH_OPERATOR_RESULT_EVENT,
    ...input.identity ? { identity: input.identity } : {},
    timestampMs: input.timestampMs ?? Date.now(),
    sourceKind: WORKGRAPH_LOCAL_SOURCE_KIND,
    data: {
      method: input.method,
      // The sent params double as routing args (id / binding_id) so failed
      // mutations still land on the right card.
      args: input.params,
      ...input.refresh ? { refresh: true } : {},
      ...input.errorMessage !== void 0 ? { is_error: true, result: input.errorMessage } : { result: input.result ?? null }
    }
  };
}
function workGraphOperatorDisplayName(method) {
  const raw = typeof method === "string" && method.trim() ? method.trim() : "workgraph";
  return raw.replace(/^mobkit\//, "").replace(/\//g, "_");
}
function workGraphToolNamesByCallId(frames) {
  const names = /* @__PURE__ */ new Map();
  for (const frame of frames) {
    if (!WORKGRAPH_TOOL_EVENTS.has(frame.event)) continue;
    const name = parseToolName(frame);
    if (!WORKGRAPH_TOOL_NAMES.has(name)) continue;
    const toolCallId = parseToolCallId(frame);
    if (toolCallId) names.set(toolCallId, name);
  }
  return names;
}
function isWorkGraphToolFrame(frame, namesByCallId) {
  if (frame.event === WORKGRAPH_OPERATOR_RESULT_EVENT) return true;
  if (!WORKGRAPH_TOOL_EVENTS.has(frame.event)) return false;
  if (WORKGRAPH_TOOL_NAMES.has(parseToolName(frame))) return true;
  if (!namesByCallId || namesByCallId.size === 0) return false;
  const toolCallId = parseToolCallId(frame);
  return toolCallId !== null && namesByCallId.has(toolCallId);
}
function workGraphToolNameOf(frame, namesByCallId) {
  const name = parseToolName(frame);
  if (WORKGRAPH_TOOL_NAMES.has(name)) return name;
  const toolCallId = parseToolCallId(frame);
  return toolCallId && namesByCallId?.get(toolCallId) || name;
}
function workGraphString(value) {
  return typeof value === "string" && value.trim() ? value.trim() : void 0;
}
function workGraphOwnerLabel(record) {
  const fromOwner = (value) => {
    if (!value || typeof value !== "object") return void 0;
    const owner = value;
    const display = workGraphString(owner.display_name);
    if (display) return display;
    const key = owner.key && typeof owner.key === "object" ? owner.key : null;
    return workGraphString(key?.id);
  };
  const direct = fromOwner(record.owner);
  if (direct) return direct;
  const claim = record.claim && typeof record.claim === "object" ? record.claim : null;
  return fromOwner(claim?.owner);
}
function workGraphEvidenceLines(record) {
  if (!Array.isArray(record.evidence_refs) || record.evidence_refs.length === 0) return void 0;
  const lines = record.evidence_refs.map((value) => {
    if (!value || typeof value !== "object") return "";
    const evidence = value;
    const label = workGraphString(evidence.label) || workGraphString(evidence.summary);
    const kind = workGraphString(evidence.kind);
    const id = workGraphString(evidence.id);
    if (label) return kind ? `${kind}: ${label}` : label;
    return [kind, id].filter(Boolean).join(" ");
  }).filter(Boolean);
  return lines.length > 0 ? lines : void 0;
}
function foldWorkGraphItem(state, value, frameIso) {
  if (!value || typeof value !== "object") return null;
  const record = value;
  const itemId = workGraphString(record.id);
  if (!itemId) return null;
  const revision = typeof record.revision === "number" ? record.revision : void 0;
  const existing = state.items.get(itemId);
  if (existing && existing.revision !== void 0 && (revision === void 0 || existing.revision > revision)) {
    if (frameIso) existing.lastEventAt = frameIso;
    return itemId;
  }
  state.items.set(itemId, {
    itemId,
    title: workGraphString(record.title) || itemId,
    status: workGraphString(record.status) || "open",
    priority: workGraphString(record.priority),
    ownerLabel: workGraphOwnerLabel(record),
    revision,
    dueAt: workGraphString(record.due_at),
    description: workGraphString(record.description),
    labels: Array.isArray(record.labels) ? record.labels.filter((label) => typeof label === "string") : void 0,
    evidence: workGraphEvidenceLines(record),
    createdAt: workGraphString(record.created_at),
    updatedAt: workGraphString(record.updated_at),
    lastEventAt: frameIso || existing?.lastEventAt
  });
  return itemId;
}
function workGraphBindingStatus(value) {
  const record = value && typeof value === "object" ? value : null;
  const state = workGraphString(record?.state) || "active";
  if (state === "paused") {
    const until = workGraphString(record?.until);
    return {
      label: until ? `paused until ${until.slice(0, 16).replace("T", " ")}` : "paused",
      active: false
    };
  }
  return { label: state, active: state === "active" };
}
function workGraphTargetLabel(value) {
  const record = value && typeof value === "object" ? value : null;
  if (!record) return void 0;
  const sessionId = workGraphString(record.session_id);
  if (sessionId) return sessionId;
  const ownerKey = record.owner_key && typeof record.owner_key === "object" ? record.owner_key : null;
  if (ownerKey) {
    const kind = workGraphString(ownerKey.kind);
    const id = workGraphString(ownerKey.id);
    return [kind, id].filter(Boolean).join(":") || void 0;
  }
  return void 0;
}
function foldWorkGraphBinding(state, value, frameIso) {
  if (!value || typeof value !== "object") return null;
  const record = value;
  const bindingId = workGraphString(record.binding_id);
  if (!bindingId) return null;
  const machineState = record.machine_state && typeof record.machine_state === "object" ? record.machine_state : null;
  const revision = typeof machineState?.revision === "number" ? machineState.revision : void 0;
  const existing = state.bindings.get(bindingId);
  if (existing && existing.revision !== void 0 && (revision === void 0 || existing.revision > revision)) {
    return bindingId;
  }
  const workRef = record.work_ref && typeof record.work_ref === "object" ? record.work_ref : null;
  const status = workGraphBindingStatus(record.status);
  state.bindings.set(bindingId, {
    bindingId,
    mode: workGraphString(record.mode) || "pursue",
    statusLabel: status.label,
    active: status.active,
    targetLabel: workGraphTargetLabel(record.target),
    revision,
    itemId: workGraphString(workRef?.item_id) || existing?.itemId,
    updatedAt: workGraphString(record.updated_at) || frameIso
  });
  return bindingId;
}
function foldWorkGraphEdge(state, value) {
  if (!value || typeof value !== "object") return;
  const record = value;
  if (workGraphString(record.kind) !== "parent") return;
  const child = workGraphString(record.from_id);
  const parent = workGraphString(record.to_id);
  if (!child || !parent || child === parent) return;
  const first = state.parents.get(child);
  if (first === void 0) {
    state.parents.set(child, parent);
    return;
  }
  if (first === parent) return;
  const extras = state.extraParents.get(child) || /* @__PURE__ */ new Set();
  extras.add(parent);
  state.extraParents.set(child, extras);
}
function foldWorkGraphEvent(state, value) {
  if (!value || typeof value !== "object") return;
  const record = value;
  const kind = workGraphString(record.kind);
  if (!kind) return;
  let dedupeKey = typeof record.seq === "number" ? `seq:${record.seq}` : "";
  if (!dedupeKey) {
    try {
      dedupeKey = `content:${JSON.stringify(record)}`;
    } catch {
      dedupeKey = "";
    }
  }
  if (dedupeKey) {
    if (state.seenEventKeys.has(dedupeKey)) return;
    state.seenEventKeys.add(dedupeKey);
  }
  const at = workGraphString(record.at);
  const clock = at ? `${at.slice(11, 16)}` : "";
  state.events.push({
    at,
    itemId: workGraphString(record.item_id),
    text: [kind.replace(/_/g, " "), clock].filter(Boolean).join(" \xB7 ")
  });
}
var WORKGRAPH_FAILURE_MESSAGE_LIMIT = 80;
function workGraphFailureLine(name, raw) {
  let message = "";
  if (typeof raw === "string") {
    message = raw.trim();
  } else if (raw && typeof raw === "object") {
    const record = raw;
    message = workGraphString(record.message) || workGraphString(record.detail) || workGraphString(record.error) || "";
    if (!message) {
      try {
        message = JSON.stringify(raw);
      } catch {
        message = "";
      }
    }
  }
  if (message.length > WORKGRAPH_FAILURE_MESSAGE_LIMIT) {
    message = `${message.slice(0, WORKGRAPH_FAILURE_MESSAGE_LIMIT - 1)}\u2026`;
  }
  return message ? `\u2717 ${name} failed: ${message}` : `\u2717 ${name} failed`;
}
function parseWorkGraphResult(frame) {
  const record = frame.data && typeof frame.data === "object" ? frame.data : null;
  if (!record || record.is_error === true) return null;
  const raw = record.result;
  if (raw && typeof raw === "object") return raw;
  if (typeof raw === "string") {
    const parsed = parseJsonPayload(raw);
    if (parsed && typeof parsed === "object" && !Array.isArray(parsed)) {
      return parsed;
    }
  }
  return null;
}
function resolveWorkGraphRoot(itemId, parents) {
  let current = itemId;
  const seen = /* @__PURE__ */ new Set();
  while (parents.has(current) && !seen.has(current)) {
    seen.add(current);
    current = parents.get(current);
  }
  return current;
}
function deriveWorkGraphStatus(items) {
  if (items.length === 0) return "active";
  let open = 0;
  let inProgress = 0;
  let blocked = 0;
  let completed = 0;
  let cancelled = 0;
  let failed = 0;
  for (const item of items) {
    if (item.status === "open") open += 1;
    else if (item.status === "in_progress") inProgress += 1;
    else if (item.status === "blocked") blocked += 1;
    else if (item.status === "completed") completed += 1;
    else if (item.status === "cancelled") cancelled += 1;
    else if (item.status === "failed") failed += 1;
    else open += 1;
  }
  const nonTerminal = open + inProgress + blocked;
  if (nonTerminal === 0) {
    if (failed > 0) return "failed";
    if (cancelled > 0) return "mixed";
    return "completed";
  }
  if (open + inProgress > 0) return "active";
  return "blocked";
}
function workGraphItemRows(rootId, memberIds, state) {
  const memberSet = new Set(memberIds);
  const childrenOf = /* @__PURE__ */ new Map();
  for (const id of memberIds) {
    const parent = state.parents.get(id);
    if (parent === void 0) continue;
    const children = childrenOf.get(parent) || [];
    children.push(id);
    childrenOf.set(parent, children);
  }
  const sortIds = (ids) => [...ids].sort((left, right) => {
    const leftItem = state.items.get(left);
    const rightItem = state.items.get(right);
    const leftKey = leftItem?.createdAt || "";
    const rightKey = rightItem?.createdAt || "";
    if (leftKey !== rightKey) return leftKey < rightKey ? -1 : 1;
    return left < right ? -1 : left === right ? 0 : 1;
  });
  const rows = [];
  const visited = /* @__PURE__ */ new Set();
  const visit = (itemId, depth) => {
    if (visited.has(itemId)) return;
    visited.add(itemId);
    const draft = state.items.get(itemId);
    let childDepth = depth;
    if (draft && memberSet.has(itemId)) {
      const extraParents = state.extraParents.get(itemId);
      rows.push({
        itemId: draft.itemId,
        title: draft.title,
        status: draft.status,
        priority: draft.priority ?? null,
        ownerLabel: draft.ownerLabel ?? null,
        ...draft.revision !== void 0 ? { revision: draft.revision } : {},
        depth,
        parentId: state.parents.get(itemId) ?? null,
        // Parents beyond the placement one (first-parent-wins), labeled by
        // title when the parent item was observed in this window.
        ...extraParents && extraParents.size > 0 ? { alsoUnder: [...extraParents].map((parent) => state.items.get(parent)?.title || parent) } : {},
        blocked: draft.status === "blocked",
        dueAt: draft.dueAt ?? null,
        lastEventAt: draft.lastEventAt ?? null,
        description: draft.description ?? null,
        ...draft.labels && draft.labels.length > 0 ? { labels: draft.labels } : {},
        ...draft.evidence && draft.evidence.length > 0 ? { evidence: draft.evidence } : {},
        createdAt: draft.createdAt ?? null,
        updatedAt: draft.updatedAt ?? null
      });
      childDepth = depth + 1;
    }
    for (const child of sortIds(childrenOf.get(itemId) || [])) {
      visit(child, childDepth);
    }
  };
  visit(rootId, 0);
  for (const id of sortIds(memberIds)) {
    visit(id, 0);
  }
  return rows;
}
function workGraphAttentionRows(bindings) {
  return [...bindings].sort((left, right) => {
    if (left.active !== right.active) return left.active ? -1 : 1;
    return left.bindingId < right.bindingId ? -1 : left.bindingId === right.bindingId ? 0 : 1;
  }).map((binding) => ({
    bindingId: binding.bindingId,
    mode: binding.mode,
    statusLabel: binding.statusLabel,
    targetLabel: binding.targetLabel ?? null,
    ...binding.revision !== void 0 ? { revision: binding.revision } : {},
    itemId: binding.itemId ?? null
  }));
}
var WORKGRAPH_RECENT_EVENT_LIMIT = 5;
function buildWorkGraphEntries(agent, frames, namesByCallId) {
  const state = {
    items: /* @__PURE__ */ new Map(),
    parents: /* @__PURE__ */ new Map(),
    extraParents: /* @__PURE__ */ new Map(),
    bindings: /* @__PURE__ */ new Map(),
    events: [],
    seenEventKeys: /* @__PURE__ */ new Set(),
    contributions: []
  };
  let sawWorkGraphFrame = false;
  const argIdsByCallId = /* @__PURE__ */ new Map();
  for (let index = 0; index < frames.length; index++) {
    const frame = frames[index];
    if (!isWorkGraphToolFrame(frame, namesByCallId)) continue;
    sawWorkGraphFrame = true;
    const frameIso = isoFromTimestampMs(frame.timestampMs);
    const contribution = {
      frameIndex: index,
      interactionId: frame.interactionId?.trim() || "",
      itemIds: [],
      bindingIds: [],
      outcome: void 0
    };
    const record = frame.data && typeof frame.data === "object" ? frame.data : null;
    const args = record?.args && typeof record.args === "object" ? record.args : null;
    const toolCallId = parseToolCallId(frame);
    let argItemId = workGraphString(args?.id);
    let argBindingId = workGraphString(args?.binding_id);
    if (args && toolCallId) {
      argIdsByCallId.set(toolCallId, { itemId: argItemId, bindingId: argBindingId });
    } else if (!args && toolCallId) {
      const paired = argIdsByCallId.get(toolCallId);
      argItemId = argItemId || paired?.itemId;
      argBindingId = argBindingId || paired?.bindingId;
    }
    if (argItemId) contribution.itemIds.push(argItemId);
    if (argBindingId) contribution.bindingIds.push(argBindingId);
    const isOperatorResult = frame.event === WORKGRAPH_OPERATOR_RESULT_EVENT;
    if (frame.event === "tool_result_received" || frame.event === "tool_execution_completed" || isOperatorResult) {
      const failed = record?.is_error === true;
      if (record?.refresh !== true) {
        contribution.outcome = failed ? "error" : "ok";
      }
      if (failed) {
        state.events.push({
          at: frameIso,
          itemId: argItemId || (argBindingId ? state.bindings.get(argBindingId)?.itemId : void 0),
          interactionId: contribution.interactionId,
          text: workGraphFailureLine(
            isOperatorResult ? workGraphOperatorDisplayName(record?.method) : workGraphToolNameOf(frame, namesByCallId),
            record?.result ?? record?.content
          )
        });
      }
      const result = parseWorkGraphResult(frame);
      if (result) {
        const foldItem = (value) => {
          const itemId = foldWorkGraphItem(state, value, frameIso);
          if (itemId) contribution.itemIds.push(itemId);
        };
        const foldBinding = (value) => {
          const bindingId = foldWorkGraphBinding(state, value, frameIso);
          if (bindingId) contribution.bindingIds.push(bindingId);
        };
        foldItem(result.item);
        if (Array.isArray(result.items)) result.items.forEach(foldItem);
        foldBinding(result.attention);
        foldBinding(result.previous);
        foldWorkGraphEdge(state, result.edge);
        if (Array.isArray(result.events)) {
          for (const event of result.events) foldWorkGraphEvent(state, event);
        }
        const snapshot = result.snapshot && typeof result.snapshot === "object" ? result.snapshot : null;
        if (snapshot) {
          if (Array.isArray(snapshot.items)) snapshot.items.forEach(foldItem);
          if (Array.isArray(snapshot.edges)) {
            for (const edge of snapshot.edges) foldWorkGraphEdge(state, edge);
          }
          if (Array.isArray(snapshot.attention)) snapshot.attention.forEach(foldBinding);
        }
      }
    }
    state.contributions.push(contribution);
  }
  const byAnchor = /* @__PURE__ */ new Map();
  if (!sawWorkGraphFrame) return byAnchor;
  const rootMembers = /* @__PURE__ */ new Map();
  for (const itemId of state.items.keys()) {
    const root = resolveWorkGraphRoot(itemId, state.parents);
    const members = rootMembers.get(root) || [];
    members.push(itemId);
    rootMembers.set(root, members);
  }
  const rootBindings = /* @__PURE__ */ new Map();
  for (const binding of state.bindings.values()) {
    if (!binding.itemId) continue;
    const root = resolveWorkGraphRoot(binding.itemId, state.parents);
    const bindings = rootBindings.get(root) || [];
    bindings.push(binding);
    rootBindings.set(root, bindings);
    if (!rootMembers.has(root)) rootMembers.set(root, []);
  }
  const rootForItem = (itemId) => resolveWorkGraphRoot(itemId, state.parents);
  const ownCardRoots = /* @__PURE__ */ new Set();
  for (const [root, members] of rootMembers) {
    const hasHierarchy = members.length > 1 || members.some((id) => id !== root);
    if (hasHierarchy || (rootBindings.get(root)?.length || 0) > 0) {
      ownCardRoots.add(root);
    }
  }
  const anchorByCard = /* @__PURE__ */ new Map();
  const lastOutcomeByCard = /* @__PURE__ */ new Map();
  const firstItemByCard = /* @__PURE__ */ new Map();
  const catchAllMembers = /* @__PURE__ */ new Map();
  const catchAllForItem = /* @__PURE__ */ new Map();
  const bindingRoot = (bindingId) => {
    const binding = state.bindings.get(bindingId);
    return binding?.itemId ? rootForItem(binding.itemId) : null;
  };
  for (const contribution of state.contributions) {
    const cardKeys = /* @__PURE__ */ new Set();
    const recordFirstItem = (cardKey, itemId) => {
      if (!firstItemByCard.has(cardKey)) firstItemByCard.set(cardKey, itemId);
    };
    for (const itemId of contribution.itemIds) {
      const root = rootForItem(itemId);
      if (ownCardRoots.has(root)) {
        const cardKey = `workgraph:${root}`;
        cardKeys.add(cardKey);
        recordFirstItem(cardKey, itemId);
      } else if (state.items.has(itemId) || state.items.has(root)) {
        const interactionKey = catchAllForItem.get(root) || `workgraph:interaction:${contribution.interactionId || "unscoped"}`;
        catchAllForItem.set(root, interactionKey);
        const members = catchAllMembers.get(interactionKey) || /* @__PURE__ */ new Set();
        members.add(root);
        catchAllMembers.set(interactionKey, members);
        cardKeys.add(interactionKey);
        recordFirstItem(interactionKey, itemId);
      }
    }
    for (const bindingId of contribution.bindingIds) {
      const root = bindingRoot(bindingId);
      if (root && ownCardRoots.has(root)) cardKeys.add(`workgraph:${root}`);
    }
    if (contribution.outcome === "error" && cardKeys.size === 0) {
      const interactionKey = `workgraph:interaction:${contribution.interactionId || "unscoped"}`;
      if (!catchAllMembers.has(interactionKey)) {
        catchAllMembers.set(interactionKey, /* @__PURE__ */ new Set());
      }
      cardKeys.add(interactionKey);
    }
    for (const key of cardKeys) {
      if (!anchorByCard.has(key)) {
        anchorByCard.set(key, {
          frameIndex: contribution.frameIndex,
          createdAt: isoFromTimestampMs(frames[contribution.frameIndex]?.timestampMs),
          interactionId: contribution.interactionId
        });
      }
      if (contribution.outcome) {
        lastOutcomeByCard.set(key, contribution.outcome);
      }
    }
  }
  const eventsForMembers = (memberSet, catchAllInteractionId) => {
    const matched = state.events.filter((event) => {
      if (event.itemId && (memberSet.has(rootForItem(event.itemId)) || memberSet.has(event.itemId))) {
        return true;
      }
      if (catchAllInteractionId === void 0 || event.interactionId !== catchAllInteractionId) {
        return false;
      }
      const routable = Boolean(
        event.itemId && (state.items.has(event.itemId) || state.items.has(rootForItem(event.itemId)))
      );
      return !routable;
    });
    if (matched.length === 0) return void 0;
    return matched.slice(-WORKGRAPH_RECENT_EVENT_LIMIT).map((event) => event.text);
  };
  const pushEntry = (entry, anchorIndex) => {
    const list = byAnchor.get(anchorIndex) || [];
    list.push(entry);
    byAnchor.set(anchorIndex, list);
  };
  const latestIso = (values) => {
    let latest;
    for (const value of values) {
      if (value && (!latest || value > latest)) latest = value;
    }
    return latest;
  };
  const uiStateKeyForCard = (entryId, anchorInteractionId) => `workgraph:interaction:${anchorInteractionId || "unscoped"}:${firstItemByCard.get(entryId) || "unrooted"}`;
  for (const root of ownCardRoots) {
    const entryId = `workgraph:${root}`;
    const anchor = anchorByCard.get(entryId);
    if (!anchor) continue;
    const items = workGraphItemRows(root, rootMembers.get(root) || [], state);
    const attention = workGraphAttentionRows(rootBindings.get(root) || []);
    const rootItem = state.items.get(root);
    const completed = items.filter((item) => item.status === "completed").length;
    const memberSet = /* @__PURE__ */ new Set([root, ...rootMembers.get(root) || []]);
    const recentEvents = eventsForMembers(memberSet);
    const lastUpdatedAt = latestIso(items.flatMap((item) => [item.updatedAt, item.lastEventAt]));
    const title = rootItem ? rootItem.title : "Goal from an earlier conversation";
    const objective = rootItem ? rootItem.description ?? null : `Goal \u2026${root.slice(-6)}`;
    pushEntry({
      kind: "workgraph",
      id: entryId,
      uiStateKey: uiStateKeyForCard(entryId, anchor.interactionId),
      identity: agentIdentity(agent),
      ...anchor.createdAt ? { createdAt: anchor.createdAt } : {},
      rootId: root,
      title,
      objective,
      status: deriveWorkGraphStatus(items),
      progress: { completed, total: items.length },
      items,
      attention,
      ...recentEvents ? { recentEvents } : {},
      ...lastOutcomeByCard.get(entryId) === "error" ? { lastActionFailed: true } : {},
      ...lastUpdatedAt ? { lastUpdatedAt } : {}
    }, anchor.frameIndex);
  }
  for (const [entryId, members] of catchAllMembers) {
    const anchor = anchorByCard.get(entryId);
    if (!anchor) continue;
    const memberIds = [...members].filter((id) => state.items.has(id));
    const recentEvents = eventsForMembers(members, anchor.interactionId);
    if (memberIds.length === 0 && !recentEvents) continue;
    const rows = memberIds.flatMap((id) => workGraphItemRows(id, [id], state));
    const completed = rows.filter((item) => item.status === "completed").length;
    const lastUpdatedAt = latestIso(rows.flatMap((item) => [item.updatedAt, item.lastEventAt]));
    pushEntry({
      kind: "workgraph",
      id: entryId,
      uiStateKey: uiStateKeyForCard(entryId, anchor.interactionId),
      identity: agentIdentity(agent),
      ...anchor.createdAt ? { createdAt: anchor.createdAt } : {},
      rootId: entryId.replace(/^workgraph:/, ""),
      title: rows.length === 1 ? rows[0].title : rows.length === 0 ? "WorkGraph activity" : "Work items",
      objective: rows.length === 1 ? rows[0].description ?? null : null,
      status: deriveWorkGraphStatus(rows),
      progress: { completed, total: rows.length },
      items: rows,
      attention: [],
      ...recentEvents ? { recentEvents } : {},
      ...lastOutcomeByCard.get(entryId) === "error" ? { lastActionFailed: true } : {},
      ...lastUpdatedAt ? { lastUpdatedAt } : {}
    }, anchor.frameIndex);
  }
  return byAnchor;
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
    if (isSteerDeliveryTerminalFrame(frame)) return null;
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
    if (streamedTextMatchesTerminal(streamedText, text)) {
      return null;
    }
    const blocks = parseConversationRichBlocks(text, { displayNormalization: false });
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
function terminalFrameVisibleText(frame) {
  if (isSteerDeliveryTerminalFrame(frame)) return "";
  if (frame.event === "text_complete") {
    const record = frame.data && typeof frame.data === "object" ? frame.data : null;
    if (typeof record?.content === "string") return record.content;
    if (typeof record?.text === "string") return record.text;
  }
  if (frame.event === "interaction_complete" || frame.event === "run_completed" || frame.event === "text_complete") {
    return summarizeFrameData(frame.data);
  }
  return "";
}
function liveAssistantTerminalTextSignatures(frames) {
  const signatures = /* @__PURE__ */ new Set();
  for (const frame of frames) {
    if (frame.sourceKind === "session_history") continue;
    const text = terminalFrameVisibleText(frame).trim();
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
function renderAssistantImageEntry(agent, frame, entryId, blobBaseUrl) {
  const data = frame.data && typeof frame.data === "object" ? frame.data : {};
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
function renderGeneratedImageToolResultEntries(agent, frame, entryId, blobBaseUrl) {
  const data = frame.data && typeof frame.data === "object" ? frame.data : {};
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
  return images.flatMap((image, index) => {
    if (!image || typeof image !== "object") return [];
    const imageFrame = {
      ...frame,
      data: { image }
    };
    const imageEntry = renderAssistantImageEntry(
      agent,
      imageFrame,
      `${entryId}:generated-image:${index}`,
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
function normalizeTextWithoutWhitespace(value) {
  return value.replace(/\s+/g, "");
}
function streamedTextMatchesTerminal(streamedText, terminalText) {
  const streamed = normalizeComparableText(streamedText);
  const terminal = normalizeComparableText(terminalText);
  if (!streamed || !terminal) return false;
  return streamed === terminal || normalizeTextWithoutWhitespace(streamed) === normalizeTextWithoutWhitespace(terminal);
}
function conversationEntryVisibleText(entry) {
  if (entry.kind !== "message") return "";
  if ("text" in entry && typeof entry.text === "string") return entry.text;
  if (!("blocks" in entry) || !Array.isArray(entry.blocks)) return "";
  return entry.blocks.map((block) => {
    if (!block || typeof block !== "object") return "";
    const record = block;
    if (record.type === "thinking") return "";
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
  for (let index = priorEntries.length - 1; index >= 0; index--) {
    const prior = priorEntries[index];
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
function userEntryDedupeKey(frame, entry) {
  const interactionId = frame.interactionId?.trim();
  if (interactionId) return `interaction:${interactionId}`;
  const signature = userEntryTextSignature(entry);
  if (frame.sourceKind === "session_history" && /^You are\b/i.test(signature)) {
    return `history-kickoff:${signature}`;
  }
  const occurrence = typeof frame.timestampMs === "number" ? `ts:${frame.timestampMs}` : frame.cursor ? `cursor:${frame.cursor}` : `frame:${frame.id}`;
  return signature ? `content:${occurrence}:${signature}` : "";
}
function userPromptDedupeKey(frame, entry) {
  return userEntryDedupeKey(frame, entry);
}
function renderRunStartedPromptEntries(frame, entryId, options = {}) {
  if (frame.event !== "run_started" || typeof frame.data !== "object" || frame.data === null) {
    return [];
  }
  const record = frame.data;
  const promptBlocks = contentToUserBlocks(record.prompt, options.blobBaseUrl);
  const prompt = extractPromptText(record.prompt).trim();
  if (!prompt) {
    return [];
  }
  if (isCommsLikeRunStartedPrompt(prompt) && runStartedPromptHasImagePlaceholder(frame)) {
    return [];
  }
  if (options.suppressStructuredCommsPrompt) {
    return [];
  }
  const createdAt = isoFromTimestampMs(frame.timestampMs);
  const entries = [];
  if (!options.suppressEmbeddedRpcPrompt) {
    if (promptBlocks.length > 0 && promptBlocks.some((block) => block.type === "image")) {
      entries.push({
        kind: "message",
        id: entryId,
        identity: USER_IDENTITY,
        variant: "rich",
        ...createdAt ? { createdAt } : {},
        blocks: promptBlocks
      });
      return entries;
    }
    const scrubbedPrompt = stripPeerTransportScaffold(prompt);
    if (!scrubbedPrompt) {
      return entries;
    }
    entries.push({
      kind: "message",
      id: entryId,
      identity: USER_IDENTITY,
      variant: "plain",
      ...createdAt ? { createdAt } : {},
      text: scrubbedPrompt
    });
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
    return parseConversationRichBlocks(content, { displayNormalization: false });
  }
  if (!Array.isArray(content)) {
    return [];
  }
  const blocks = [];
  for (const block of content) {
    if (typeof block === "string") {
      blocks.push(...parseConversationRichBlocks(block, { displayNormalization: false }));
      continue;
    }
    if (!block || typeof block !== "object") continue;
    const record = block;
    const type = typeof record.type === "string" ? record.type : "";
    if (type === "text") {
      const text = typeof record.text === "string" ? record.text : typeof record.content === "string" ? record.content : "";
      blocks.push(...parseConversationRichBlocks(text, { displayNormalization: false }));
      continue;
    }
    if (type === "image" || type === "image_ref") {
      const image = record.image && typeof record.image === "object" ? record.image : record;
      const blobRef = image.blob_ref && typeof image.blob_ref === "object" ? image.blob_ref : image.blobRef && typeof image.blobRef === "object" ? image.blobRef : null;
      const source = typeof image.source === "string" ? image.source : "";
      const blobId = typeof record.blob_id === "string" ? record.blob_id : typeof image.blob_id === "string" ? image.blob_id : typeof record.blobId === "string" ? record.blobId : typeof image.blobId === "string" ? image.blobId : typeof blobRef?.blob_id === "string" ? blobRef.blob_id : typeof blobRef?.blobId === "string" ? blobRef.blobId : "";
      const mediaType = typeof image.media_type === "string" ? image.media_type : typeof image.mediaType === "string" ? image.mediaType : typeof blobRef?.media_type === "string" ? blobRef.media_type : typeof blobRef?.mediaType === "string" ? blobRef.mediaType : "image/png";
      const inlineData = typeof image.data === "string" ? image.data : typeof image.base64 === "string" ? image.base64 : "";
      const directSrc = typeof image.src === "string" && image.src.trim() ? image.src.trim() : typeof image.url === "string" && image.url.trim() ? image.url.trim() : "";
      const src = blobId && (source === "blob" || !directSrc) ? buildBlobUrl(blobId, blobBaseUrl) : inlineData ? `data:${mediaType};base64,${inlineData}` : directSrc;
      if (!src) continue;
      const alt = typeof image.alt === "string" && image.alt.trim() ? image.alt.trim() : type === "image_ref" ? "referenced image" : "attached image";
      const width = typeof image.width === "number" ? image.width : void 0;
      const height = typeof image.height === "number" ? image.height : void 0;
      const imageId = typeof image.image_id === "string" ? image.image_id : void 0;
      blocks.push({
        type: "image",
        src,
        mediaType,
        alt,
        ...width !== void 0 ? { width } : {},
        ...height !== void 0 ? { height } : {},
        ...blobId ? { blobId } : {},
        ...imageId ? { imageId } : {}
      });
    }
  }
  return blocks;
}
function peerLastSegment(value) {
  return value.split("/").pop() || value;
}
function summarizePeersResult(result) {
  let parsed = typeof result === "string" ? parseJsonPayload(result) : result && typeof result === "object" ? result : null;
  if (typeof parsed === "string") {
    parsed = parseJsonPayload(parsed);
  }
  if (!parsed || typeof parsed !== "object") return null;
  const peers = parsed.peers;
  if (!Array.isArray(peers)) return null;
  const roleCounts = /* @__PURE__ */ new Map();
  const preview = [];
  for (const peer of peers) {
    if (!peer || typeof peer !== "object") continue;
    const record = peer;
    const rawName = typeof record.name === "string" && record.name.trim() ? record.name.trim() : typeof record.address?.endpoint === "string" ? String(record.address.endpoint).trim() : typeof record.peer_id === "string" ? record.peer_id.trim() : "";
    if (!rawName) continue;
    const parts = rawName.split("/").filter(Boolean);
    const role = parts.length >= 2 ? parts[parts.length - 2] : "peer";
    roleCounts.set(role, (roleCounts.get(role) || 0) + 1);
    if (preview.length < 8) preview.push(peerLastSegment(rawName));
  }
  const roles = [...roleCounts.entries()].sort((left, right) => right[1] - left[1] || left[0].localeCompare(right[0])).slice(0, 5).map(([role, count]) => `${role} ${count}`).join(", ");
  const lines = [`${peers.length} peers${roles ? ` \xB7 ${roles}` : ""}`];
  if (preview.length > 0) {
    lines.push(`First peers: ${preview.join(", ")}`);
  }
  return lines.join("\n");
}
function summarizeToolResultForDisplay(toolName2, result) {
  if (toolName2 === "peers") {
    const summary = summarizePeersResult(result);
    if (summary) return summary;
  }
  return null;
}
function formatServerToolAnnotations(annotations) {
  return annotations.map((annotation, index) => {
    const record = annotation && typeof annotation === "object" ? annotation : null;
    const title = typeof record?.title === "string" && record.title.trim() ? record.title.trim() : typeof record?.text === "string" && record.text.trim() ? record.text.trim() : `Source ${index + 1}`;
    const url = typeof record?.url === "string" && record.url.trim() ? record.url.trim() : "";
    return url ? `${index + 1}. ${title}
${url}` : `${index + 1}. ${title}`;
  }).join("\n\n").trim();
}
function serverToolContentSummary(frame) {
  const record = frame.data && typeof frame.data === "object" ? frame.data : null;
  const content = record?.content && typeof record.content === "object" ? record.content : null;
  const type = typeof content?.type === "string" ? content.type : typeof record?.type === "string" ? record.type : "";
  const status = typeof content?.status === "string" ? content.status : typeof record?.status === "string" ? record.status : "";
  if (type.includes(".failed") || type.includes(".error") || status === "failed" || status === "error") {
    return { status: "error" };
  }
  if (Array.isArray(content?.annotations)) {
    const result = formatServerToolAnnotations(content.annotations);
    return {
      status: "success",
      ...result ? { result } : {}
    };
  }
  if (type.includes(".completed") || type.includes(".done") || status === "completed" || status === "done" || status === "succeeded") {
    return { status: "success" };
  }
  if (type.includes(".in_progress") || type.includes(".searching") || type.includes(".started") || status === "in_progress" || status === "searching" || status === "queued" || type.includes("_call")) {
    return { status: "pending" };
  }
  return null;
}
function isActiveServerToolContentFrame(frame) {
  return serverToolContentSummary(frame)?.status === "pending";
}
function isTerminalServerToolContentFrame(frame) {
  const record = frame.data && typeof frame.data === "object" ? frame.data : null;
  const content = record?.content && typeof record.content === "object" ? record.content : null;
  const type = typeof content?.type === "string" ? content.type : "";
  if (type === "message_annotations" || Array.isArray(content?.annotations)) return false;
  const status = serverToolContentSummary(frame)?.status;
  return status === "success" || status === "error";
}
function toolResultTextFromContent(content) {
  if (typeof content === "string") return content;
  if (!Array.isArray(content)) return "";
  return content.map((block) => {
    if (typeof block === "string") return block;
    if (!block || typeof block !== "object") return "";
    const record = block;
    if (typeof record.text === "string") return record.text;
    if (typeof record.content === "string") return record.content;
    const data = record.data && typeof record.data === "object" ? record.data : null;
    if (typeof data?.text === "string") return data.text;
    if (typeof data?.content === "string") return data.content;
    return "";
  }).filter((value) => value.trim().length > 0).join("");
}
function historyToolResults(frames, workGraphNamesByCallId) {
  const results = /* @__PURE__ */ new Map();
  for (const frame of frames) {
    if (frame.sourceKind !== "session_history" || frame.event !== "tool_execution_completed" && frame.event !== "tool_result_received") {
      continue;
    }
    const data = frame.data && typeof frame.data === "object" ? frame.data : null;
    const historyToolName = typeof data?.name === "string" ? data.name : typeof data?.tool_name === "string" ? data.tool_name : "";
    if (WORKGRAPH_TOOL_NAMES.has(historyToolName)) continue;
    if (isWorkGraphToolFrame(frame, workGraphNamesByCallId)) continue;
    const toolCallId = typeof data?.tool_call_id === "string" && data.tool_call_id.trim() ? data.tool_call_id.trim() : typeof data?.id === "string" && data.id.trim() ? data.id.trim() : "";
    if (!toolCallId) continue;
    const rawResult = data?.result ?? data?.content;
    const result = rawResult !== void 0 ? summarizeToolResultForDisplay(void 0, rawResult) || toolResultTextFromContent(rawResult) : "";
    const status = data?.is_error === true || data?.status === "error" ? "error" : "success";
    results.set(toolCallId, {
      status,
      ...result.trim() ? { result } : {}
    });
  }
  return results;
}
function blockAssistantToolBlock(item, index, peerRegistry, toolResults) {
  const blockType = typeof item.block_type === "string" ? item.block_type : typeof item.type === "string" ? item.type : "";
  if (blockType !== "tool_use") return null;
  const data = item.data && typeof item.data === "object" ? item.data : item;
  const name = typeof data.name === "string" && data.name.trim() ? data.name.trim() : "tool";
  const id = typeof data.id === "string" && data.id.trim() ? data.id.trim() : `history-tool-${index + 1}`;
  const args = data.args !== void 0 ? data.args : data.arguments;
  const argsRecord = args && typeof args === "object" ? args : null;
  const argumentsText = args === void 0 ? "" : typeof args === "string" ? args : JSON.stringify(args);
  const isPeerTool = name === "send_request" || name === "send_message" || name === "send_response";
  const peerTarget2 = isPeerTool ? peerTargetFromArgs(argsRecord, peerRegistry) : void 0;
  const rawPeerIntent = isPeerTool && typeof argsRecord?.intent === "string" ? argsRecord.intent : void 0;
  const peerIntent = displayPeerIntent(rawPeerIntent);
  const peerBody = isPeerTool ? extractPeerBodyFromArgs(argsRecord) : void 0;
  const result = toolResults?.get(id);
  const displayResult = result?.result ? summarizeToolResultForDisplay(name, result.result) || result.result : void 0;
  return {
    type: "tool-call",
    toolCallId: id,
    name,
    arguments: argumentsText,
    ...displayResult ? { result: displayResult } : {},
    status: result?.status || "success",
    ...peerTarget2 ? { peerTarget: peerTarget2 } : {},
    ...peerIntent ? { peerIntent } : {},
    ...peerBody ? { peerBody } : {}
  };
}
function blockAssistantRichBlocks(blocks, peerRegistry, toolResults) {
  const reasoningBlocks = [];
  const actionAndTextBlocks = [];
  let hasNonTextBlock = false;
  let toolIndex = 0;
  for (const block of blocks) {
    if (!block || typeof block !== "object") continue;
    const item = block;
    const blockType = typeof item.block_type === "string" ? item.block_type : typeof item.type === "string" ? item.type : "";
    const data = item.data && typeof item.data === "object" ? item.data : {};
    if (blockType === "reasoning") {
      const text = reasoningBlockText(item);
      if (text) {
        hasNonTextBlock = true;
        reasoningBlocks.push({
          type: "thinking",
          label: "",
          text,
          final: true,
          persisted: true
        });
      }
      continue;
    }
    if (blockType === "tool_use") {
      const toolBlock = blockAssistantToolBlock(item, toolIndex, peerRegistry, toolResults);
      toolIndex += 1;
      if (toolBlock) {
        hasNonTextBlock = true;
        actionAndTextBlocks.push(toolBlock);
      }
      continue;
    }
    if (blockType === "text") {
      const text = typeof data.text === "string" ? data.text : typeof item.text === "string" ? item.text : "";
      if (text.trim()) actionAndTextBlocks.push(...parseConversationRichBlocks(text, { displayNormalization: false }));
    }
  }
  return hasNonTextBlock ? [...reasoningBlocks, ...actionAndTextBlocks] : [];
}
function textFromUnknown2(value) {
  return typeof value === "string" ? value.trim() : "";
}
function typedNoticeContentBlocks(content, blobBaseUrl) {
  return contentToUserBlocks(content, blobBaseUrl);
}
function typedNoticeBlockText(block) {
  const parts = [
    textFromUnknown2(block.summary),
    textFromUnknown2(block.body),
    textFromUnknown2(block.detail),
    textFromUnknown2(block.state),
    textFromUnknown2(block.status)
  ].filter(Boolean);
  return parts.join("\n");
}
function typedCommsStableBodyText(block) {
  const parts = [
    textFromUnknown2(block.summary),
    textFromUnknown2(block.body),
    textFromUnknown2(block.detail)
  ].filter(Boolean);
  return parts.join("\n");
}
function stripCommsIntentBodyPrefix(text, peerAliases = []) {
  const match = text.match(/^\s*\[COMMS\s+(?:MESSAGE|REQUEST|RESPONSE)\s+from\s+([^\]\n]+)\]\s*\n\s*Intent:\s*[^\n]*\n\s*Body:\s*([\s\S]+)$/i) || text.match(/^\s*Peer\s+(?:message|request|response)\s+from\s+(.+):\s*\n\s*Intent:\s*[^\n]*\n\s*Body:\s*([\s\S]+)$/i);
  if (match?.[1] && peerAliases.length > 0) {
    const peer = normalizePeerAlias(match[1]);
    if (!peerAliases.includes(peer)) return text.trim();
  }
  return (match?.[2] || text).trim();
}
function stripBareCommsIntentBodyPrefix(text) {
  const match = text.match(/^\s*Intent:\s*[^\n]*\n\s*Body:\s*([\s\S]+)$/i);
  return (match?.[1] || text).trim();
}
function isExternalEventOnlySystemNotice(message) {
  if (!message || typeof message !== "object") return false;
  const record = message;
  if (textFromUnknown2(record.kind) === "external_event") return true;
  const blocks = record.blocks;
  if (!Array.isArray(blocks)) return false;
  let sawExternalEventBlock = false;
  for (const block of blocks) {
    if (!block || typeof block !== "object") continue;
    const type = textFromUnknown2(block.type);
    if (!type) continue;
    if (type !== "external_event") return false;
    sawExternalEventBlock = true;
  }
  return sawExternalEventBlock;
}
function systemNoticeMessageRecord(frame) {
  if (frame.event !== "system_notice" || !frame.data || typeof frame.data !== "object") {
    return null;
  }
  const data = frame.data;
  if (data.message && typeof data.message === "object") {
    return data.message;
  }
  return data;
}
function commsNoticeMessageRecord(frame) {
  const systemNotice = systemNoticeMessageRecord(frame);
  if (systemNotice) return systemNotice;
  if (frame.sourceKind !== "session_history" || !frame.data || typeof frame.data !== "object") {
    return null;
  }
  if (frame.event !== "text_complete" && frame.event !== "interaction_complete" && frame.event !== "interaction_failed" && frame.event !== "run_failed") {
    return null;
  }
  const message = frame.data.message;
  if (!message || typeof message !== "object") return null;
  const record = message;
  return textFromUnknown2(record.role) === "system_notice" ? record : null;
}
function systemNoticeBlockRecords(record) {
  const blocks = record.blocks;
  if (!Array.isArray(blocks)) return [];
  return blocks.filter((block) => Boolean(block) && typeof block === "object");
}
function legacyPeerNoticeTextCandidates(record) {
  const candidates = [];
  const body = textFromUnknown2(record.body).trim();
  if (body) candidates.push(body);
  for (const block of systemNoticeBlockRecords(record)) {
    const blockText = typedNoticeBlockText(block).trim();
    if (blockText) candidates.push(blockText);
    const content = block.content;
    if (!Array.isArray(content)) continue;
    for (const item of content) {
      if (!item || typeof item !== "object") continue;
      const itemRecord = item;
      const itemText = textFromUnknown2(itemRecord.text).trim();
      if (itemText) candidates.push(itemText);
      const data = itemRecord.data;
      if (data && typeof data === "object") {
        const dataText = textFromUnknown2(data.text).trim();
        if (dataText) candidates.push(dataText);
      }
    }
  }
  return candidates;
}
function isLegacyPeerNoticeText(text) {
  return /^(Peer (?:message|request|response) from|\[COMMS (?:MESSAGE|REQUEST|RESPONSE)\b)/i.test(text.trim());
}
function isCommsLikeRunStartedPrompt(text) {
  const trimmed = text.trim();
  return /(^|\n)\s*Peer (?:message|request|response)(?:\s+from\b|$)/i.test(trimmed) || /(^|\n)\s*\[COMMS (?:MESSAGE|REQUEST|RESPONSE)\b/i.test(trimmed);
}
var PEER_ENVELOPE_LINE_RE = /^Peer\s+(?:message|request|response)\s+from\s+(.+):(.*)$/i;
var PEER_TRANSPORT_SCAFFOLD_START_RE = /Peer\s+request\s+from\s+peer_id\s/i;
var PEER_TRANSPORT_SCAFFOLD_SPAN_RE = /Peer\s+request\s+from\s+peer_id\s[\s\S]*?(?:Do not answer this request with send_message\.|Do not use send_message for this reply\.)/gi;
function stripPeerTransportScaffold(text) {
  if (!text || !PEER_TRANSPORT_SCAFFOLD_START_RE.test(text)) return text;
  let scrubbed = text.replace(PEER_TRANSPORT_SCAFFOLD_SPAN_RE, " ");
  const residualIndex = scrubbed.search(PEER_TRANSPORT_SCAFFOLD_START_RE);
  if (residualIndex >= 0) scrubbed = scrubbed.slice(0, residualIndex);
  return scrubbed.replace(/[ \t]+\n/g, "\n").replace(/\n{3,}/g, "\n\n").trim();
}
function escapeRegExp(value) {
  return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}
function stripPeerEnvelopeByAlias(text, peerAliases) {
  const normalized = text.replace(/\r/g, "\n").trim();
  const aliases = [...peerAliases].filter(Boolean).sort((a, b) => b.length - a.length);
  for (const alias of aliases) {
    const escaped = escapeRegExp(alias);
    const peerEnvelope = new RegExp(
      `^Peer\\s+(?:message|request|response)\\s+from\\s+${escaped}:(?:\\s+|$)`,
      "i"
    );
    const bracketedEnvelope = new RegExp(
      `^\\[COMMS\\s+(?:MESSAGE|REQUEST|RESPONSE)\\s+from\\s+${escaped}\\]\\s*`,
      "i"
    );
    const lines = normalized.split("\n").map((line) => line.trim());
    for (let index = 0; index < lines.length; index++) {
      const line = lines[index];
      if (!line || /^Peer (?:message|request|response)$/i.test(line)) continue;
      const peerMatch = line.match(peerEnvelope);
      const bracketMatch = line.match(bracketedEnvelope);
      if (!peerMatch && !bracketMatch) return null;
      const bodyOnEnvelopeLine = line.replace(peerEnvelope, "").replace(bracketedEnvelope, "").trim();
      const bodyLines = [
        ...bodyOnEnvelopeLine ? [bodyOnEnvelopeLine] : [],
        ...lines.slice(index + 1).filter((candidate) => !isPeerEnvelopeScaffoldLine(candidate, aliases, true))
      ];
      return bodyLines.join("\n").trim();
    }
  }
  return null;
}
function isPeerEnvelopeScaffoldLine(line, peerAliases = [], allowStandaloneScaffold = false) {
  if (!line) return true;
  if (allowStandaloneScaffold && /^Peer (?:message|request|response)$/i.test(line)) return true;
  if (peerAliases.length === 0 && /^\[COMMS (?:MESSAGE|REQUEST|RESPONSE) from [^\]]+\]$/i.test(line)) return true;
  for (const alias of peerAliases) {
    if (!alias) continue;
    const escaped = escapeRegExp(alias);
    if (new RegExp(`^Peer\\s+(?:message|request|response)\\s+from\\s+${escaped}:\\s*$`, "i").test(line)) {
      return true;
    }
    if (new RegExp(`^\\[COMMS\\s+(?:MESSAGE|REQUEST|RESPONSE)\\s+from\\s+${escaped}\\]\\s*$`, "i").test(line)) {
      return true;
    }
  }
  return false;
}
function isImagePlaceholderLine(line) {
  return /^\[image:\s*[^\]]+\]$/i.test(line.trim());
}
function normalizePeerEnvelopeText(text, peerAliases = []) {
  const allowGenericEnvelopeStrip = peerAliases.length === 0;
  const intentBodyStripped = stripCommsIntentBodyPrefix(text, peerAliases);
  const envelopeStripped = intentBodyStripped === text.trim() ? stripPeerEnvelopeByAlias(text, peerAliases) : null;
  const aliasStripped = envelopeStripped ?? (intentBodyStripped === text.trim() ? null : intentBodyStripped);
  let normalized = (envelopeStripped !== null ? stripBareCommsIntentBodyPrefix(envelopeStripped) : aliasStripped ?? text).replace(/\r/g, "\n").split("\n").map((line) => line.trim()).filter((line) => {
    if (isPeerEnvelopeScaffoldLine(
      line,
      peerAliases,
      allowGenericEnvelopeStrip || aliasStripped !== null
    )) return false;
    if (isImagePlaceholderLine(line)) return false;
    if (allowGenericEnvelopeStrip && PEER_ENVELOPE_LINE_RE.test(line) && !line.replace(PEER_ENVELOPE_LINE_RE, "$2").trim()) return false;
    return true;
  }).join("\n");
  if (allowGenericEnvelopeStrip) {
    normalized = normalized.replace(PEER_ENVELOPE_LINE_RE, "$2").replace(/^\[COMMS\s+(?:MESSAGE|REQUEST|RESPONSE)\s+from\s+[^\]]+\]\s*/i, "");
  }
  return normalized.replace(/\s+/g, " ").trim();
}
function normalizeStructuredCommsBodyText(text, peerAliases = []) {
  const trimmed = text.trim();
  if (trimmed) {
    const stripped = stripPeerEnvelopeByAlias(trimmed, peerAliases);
    if (stripped !== null) return stripped.replace(/\s+/g, " ").trim();
  }
  return trimmed.replace(/\r/g, "\n").split("\n").map((line) => line.trim()).filter(Boolean).join(" ");
}
function normalizePeerAlias(value) {
  return value.trim().toLowerCase();
}
function peerFromCommsText(text) {
  const trimmed = text.trim();
  const peerLine = trimmed.match(/(?:^|\n)\s*Peer\s+(?:message|request|response)\s+from\s+(.+):(?:[^\n]*)/i);
  if (peerLine?.[1]) return normalizePeerAlias(peerLine[1]);
  const bracketed = trimmed.match(/(?:^|\n)\s*\[COMMS\s+(?:MESSAGE|REQUEST|RESPONSE)\s+from\s+([^\]]+)\]/i);
  if (bracketed?.[1]) return normalizePeerAlias(bracketed[1]);
  return "";
}
var STRUCTURED_COMMS_PROMPT_MATCH_WINDOW_MS = 3e4;
function normalizedPeerAliases(...values) {
  const aliases = [];
  for (const value of values) {
    const alias = normalizePeerAlias(value);
    if (alias && !aliases.includes(alias)) aliases.push(alias);
  }
  return aliases;
}
function commsKindFromText(text) {
  const match = text.trim().match(/(?:^|\n)\s*Peer\s+(message|request|response)\s+from\s+/i) || text.trim().match(/(?:^|\n)\s*\[COMMS\s+(MESSAGE|REQUEST|RESPONSE)\s+from\s+/i);
  return match?.[1]?.toLowerCase() || "";
}
function systemNoticeCommsSignatures(frame) {
  const record = commsNoticeMessageRecord(frame);
  if (!record || isExternalEventOnlySystemNotice(record)) return [];
  const isCommsNotice = textFromUnknown2(record.kind) === "comms" || systemNoticeBlockRecords(record).some((block) => textFromUnknown2(block.type) === "comms") || canUseLegacyPeerNoticeText(record) && legacyPeerNoticeTextCandidates(record).some(isLegacyPeerNoticeText);
  if (!isCommsNotice) return [];
  const signatures = [];
  const seenSignatures = /* @__PURE__ */ new Set();
  const pushCandidate = (candidate, peerAliases = [], occurrenceId, kind, direction) => {
    const aliases = peerAliases.length ? peerAliases : normalizedPeerAliases(peerFromCommsText(candidate));
    const body2 = normalizePeerEnvelopeText(candidate, aliases);
    if (!body2) return;
    const candidateKind = kind || commsKindFromText(candidate);
    const candidateDirection = direction || (commsKindFromText(candidate) ? "incoming" : "");
    const key = [
      aliases.join("|"),
      body2,
      candidateKind,
      candidateDirection,
      occurrenceId || ""
    ].join("\0");
    if (seenSignatures.has(key)) return;
    seenSignatures.add(key);
    signatures.push({
      peer: aliases[0] || "",
      peerAliases: aliases,
      body: body2,
      kind: candidateKind,
      direction: candidateDirection,
      occurrenceId,
      timestampMs: frame.timestampMs,
      sourceKind: frame.sourceKind
    });
  };
  const noticeOccurrenceId = textFromUnknown2(record.request_id) || textFromUnknown2(record.correlation_id) || textFromUnknown2(record.id);
  const noticeBlocks = systemNoticeBlockRecords(record);
  const typedCommsBlocks = noticeBlocks.filter((block) => textFromUnknown2(block.type) === "comms");
  if (!typedCommsBlocks.length) {
    for (const candidate of legacyPeerNoticeTextCandidates(record)) {
      pushCandidate(candidate, [], noticeOccurrenceId);
    }
  }
  const body = textFromUnknown2(record.body);
  if (body && !typedCommsBlocks.length) pushCandidate(body, [], noticeOccurrenceId);
  for (let index = 0; index < typedCommsBlocks.length; index++) {
    const block = typedCommsBlocks[index];
    const peer = block.peer && typeof block.peer === "object" ? block.peer : {};
    const peerAliases = normalizedPeerAliases(
      textFromUnknown2(peer.display_name),
      textFromUnknown2(peer.id)
    );
    const blockOccurrenceId = textFromUnknown2(block.request_id) || textFromUnknown2(block.correlation_id) || textFromUnknown2(block.id) || (noticeOccurrenceId ? `${noticeOccurrenceId}:${index}` : `${index}`);
    const blockKind = textFromUnknown2(block.kind);
    const blockDirection = textFromUnknown2(block.direction);
    const contentText = typedNoticeContentBlocks(block.content).map((item) => item.type === "paragraph" ? item.text : "").filter(Boolean).join("\n");
    const stableBodyText = typedCommsStableBodyText(block);
    const candidateText = contentText || stableBodyText || body;
    if (candidateText) pushCandidate(candidateText, peerAliases, blockOccurrenceId, blockKind, blockDirection);
  }
  return signatures;
}
function structuredCommsNoticeTextSignatures(frames) {
  const signatures = [];
  const seen = /* @__PURE__ */ new Set();
  for (let index = 0; index < frames.length; index++) {
    const frame = frames[index];
    for (const signature of systemNoticeCommsSignatures(frame)) {
      const primaryPeerAlias = signature.peer || signature.peerAliases[0] || "";
      const key = [
        frame.id || `${frame.event}:${index}`,
        primaryPeerAlias,
        signature.kind || "",
        signature.direction || "",
        signature.occurrenceId || "",
        signature.body
      ].join("\0");
      if (seen.has(key)) continue;
      seen.add(key);
      signatures.push({
        ...signature,
        sourceIndex: index
      });
    }
  }
  return signatures;
}
function runStartedPromptMatchesStructuredCommsNotice(frame, signature) {
  if (frame.event !== "run_started" || typeof frame.data !== "object" || frame.data === null) {
    return false;
  }
  const prompt = extractPromptText(frame.data.prompt).trim();
  if (!prompt || !isCommsLikeRunStartedPrompt(prompt)) return false;
  const promptPeer = peerFromCommsText(prompt);
  const promptKind = commsKindFromText(prompt);
  const normalizedPrompt = normalizePeerEnvelopeText(prompt, signature.peerAliases);
  if (!normalizedPrompt) return false;
  if (promptKind && (!signature.kind || promptKind !== signature.kind)) return false;
  if (signature.direction === "outgoing") return false;
  const matchedByAlias = stripPeerEnvelopeByAlias(prompt, signature.peerAliases) !== null;
  if (promptPeer && signature.peerAliases.length === 0) return false;
  if (promptPeer && signature.peerAliases.length > 0 && !signature.peerAliases.includes(promptPeer) && !matchedByAlias) {
    return false;
  }
  if (typeof frame.timestampMs === "number" && typeof signature.timestampMs === "number" && Math.abs(frame.timestampMs - signature.timestampMs) > STRUCTURED_COMMS_PROMPT_MATCH_WINDOW_MS) {
    return false;
  }
  return Boolean(signature.body && normalizedPrompt === signature.body);
}
function runStartedPromptHasImagePlaceholder(frame) {
  if (frame.event !== "run_started" || typeof frame.data !== "object" || frame.data === null) {
    return false;
  }
  const prompt = extractPromptText(frame.data.prompt);
  return prompt.replace(/\r/g, "\n").split("\n").some(isImagePlaceholderLine);
}
function structuredCommsPromptSuppressionKeys(frames, structuredCommsSignatures) {
  const keys = /* @__PURE__ */ new Set();
  const consumed = /* @__PURE__ */ new Set();
  const consumedStructuredNotices = /* @__PURE__ */ new Set();
  for (const signature of structuredCommsSignatures) {
    const signatureKey = [
      signature.sourceIndex ?? "",
      signature.peerAliases.join("|"),
      signature.body,
      signature.kind || "",
      signature.direction || "",
      signature.occurrenceId || ""
    ].join("\0");
    if (consumedStructuredNotices.has(signatureKey)) continue;
    consumedStructuredNotices.add(signatureKey);
    let best = null;
    for (let index = 0; index < frames.length; index++) {
      const frame = frames[index];
      const key = `${frame.id || frame.event || "frame"}:${index}`;
      if (consumed.has(key)) continue;
      if (typeof signature.timestampMs === "number" && typeof frame.timestampMs === "number") {
        if (frame.timestampMs > signature.timestampMs && !runStartedPromptHasImagePlaceholder(frame)) {
          continue;
        }
        if (frame.timestampMs === signature.timestampMs && typeof signature.sourceIndex === "number" && index > signature.sourceIndex) continue;
      } else if (typeof signature.sourceIndex === "number" && index > signature.sourceIndex) {
        continue;
      }
      if (!runStartedPromptMatchesStructuredCommsNotice(frame, signature)) continue;
      const distance = typeof frame.timestampMs === "number" && typeof signature.timestampMs === "number" ? Math.abs(frame.timestampMs - signature.timestampMs) : Math.abs(index - (signature.sourceIndex ?? index));
      if (!best || distance < best.distance) {
        best = {
          key,
          distance
        };
      }
    }
    if (best) {
      keys.add(best.key);
      consumed.add(best.key);
    }
  }
  return keys;
}
function commsNoticeDedupeKeys(frame) {
  const signatures = systemNoticeCommsSignatures(frame);
  const keys = [];
  for (const signature of signatures) {
    if (!signature.body) continue;
    const key = [
      signature.peer || "unknown",
      signature.kind || "message",
      signature.direction || "incoming",
      signature.occurrenceId || "",
      signature.body
    ].join(":");
    if (!keys.includes(key)) keys.push(key);
  }
  return keys;
}
function commsNoticeDuplicateKey(key, frame, emitted) {
  const previous = emitted.get(key);
  if (!previous) return false;
  const sourceKind = frame.sourceKind || "live";
  const previousSourceKind = previous.sourceKind || "live";
  const mixedLiveHistory = sourceKind !== previousSourceKind && (sourceKind === "session_history" || previousSourceKind === "session_history");
  const closeInTime = typeof frame.timestampMs !== "number" || typeof previous.timestampMs !== "number" || Math.abs(frame.timestampMs - previous.timestampMs) <= 6e4;
  return mixedLiveHistory && closeInTime;
}
function markCommsNoticeDedupeKey(key, frame, emitted) {
  emitted.set(key, { sourceKind: frame.sourceKind, timestampMs: frame.timestampMs });
}
function commsNoticeDedupeKeysFromBlock(record, fallbackBody, index) {
  const type = textFromUnknown2(record.type);
  const keys = [];
  const pushKey = (candidate, peerAliases = [], occurrenceId, kind, direction) => {
    const aliases = peerAliases.length ? peerAliases : normalizedPeerAliases(peerFromCommsText(candidate));
    const body = normalizePeerEnvelopeText(candidate, aliases);
    if (!body) return;
    const candidateKind = kind || commsKindFromText(candidate) || "message";
    const candidateDirection = direction || (commsKindFromText(candidate) ? "incoming" : "incoming");
    const key = [
      aliases[0] || "unknown",
      candidateKind,
      candidateDirection,
      occurrenceId || "",
      body
    ].join(":");
    if (!keys.includes(key)) keys.push(key);
  };
  if (type === "comms") {
    const peer = record.peer && typeof record.peer === "object" ? record.peer : {};
    const peerAliases = normalizedPeerAliases(
      textFromUnknown2(peer.display_name),
      textFromUnknown2(peer.id)
    );
    const contentText = typedNoticeContentBlocks(record.content).map((item) => item.type === "paragraph" ? item.text : "").filter(Boolean).join("\n");
    const stableBodyText = typedCommsStableBodyText(record);
    const occurrenceId = textFromUnknown2(record.request_id) || textFromUnknown2(record.correlation_id) || textFromUnknown2(record.id) || `${index}`;
    pushKey(
      contentText || stableBodyText || fallbackBody,
      peerAliases,
      occurrenceId,
      textFromUnknown2(record.kind) || "message",
      textFromUnknown2(record.direction) || "incoming"
    );
    return keys;
  }
  if (type && type !== "text") return keys;
  const blockText = typedNoticeBlockText(record).trim();
  if (blockText && isLegacyPeerNoticeText(blockText)) {
    pushKey(blockText);
  }
  const content = record.content;
  if (Array.isArray(content)) {
    for (const item of content) {
      if (!item || typeof item !== "object") continue;
      const itemRecord = item;
      const itemText = textFromUnknown2(itemRecord.text).trim();
      if (itemText && isLegacyPeerNoticeText(itemText)) {
        pushKey(itemText);
      }
      const data = itemRecord.data;
      if (data && typeof data === "object") {
        const dataText = textFromUnknown2(data.text).trim();
        if (dataText && isLegacyPeerNoticeText(dataText)) {
          pushKey(dataText);
        }
      }
    }
  }
  return keys;
}
function consumeCommsNoticeBlockDedupeKeys(keys, consumeDuplicateCommsBlock) {
  if (keys.length === 0 || !consumeDuplicateCommsBlock) return false;
  let duplicateCount = 0;
  for (const key of keys) {
    if (consumeDuplicateCommsBlock(key)) duplicateCount += 1;
  }
  return duplicateCount === keys.length;
}
function shouldSuppressDuplicateCommsNotice(frame, emitted) {
  const keys = commsNoticeDedupeKeys(frame);
  if (keys.length === 0) return false;
  let duplicateCount = 0;
  for (const key of keys) {
    if (commsNoticeDuplicateKey(key, frame, emitted)) duplicateCount += 1;
  }
  if (duplicateCount === keys.length) {
    return true;
  }
  const record = systemNoticeMessageRecord(frame);
  const hasBlockLevelComms = record ? systemNoticeBlockRecords(record).some((block, index) => commsNoticeDedupeKeysFromBlock(block, textFromUnknown2(record.body), index).length > 0) : false;
  if (!hasBlockLevelComms) {
    for (const key of keys) {
      markCommsNoticeDedupeKey(key, frame, emitted);
    }
  }
  return false;
}
function structuredCommsBodyShouldPreserveLeadingEnvelope(body, peerAliases) {
  if (!body.match(/^\s*(?:Peer\s+(?:message|request|response)\s+from\s+.+:|\[COMMS\s+(?:MESSAGE|REQUEST|RESPONSE)\s+from\s+[^\]]+\])\s*\n/i)) {
    return false;
  }
  return peerAliases.some((alias) => alias && !alias.startsWith("implicit-"));
}
function canUseLegacyPeerNoticeText(record) {
  const kind = textFromUnknown2(record.kind);
  if (kind && kind !== "generic") return false;
  const blockTypes = systemNoticeBlockRecords(record).map((block) => textFromUnknown2(block.type)).filter(Boolean);
  return blockTypes.every((type) => type === "text");
}
function systemNoticeClearsBusyState2(frame) {
  const record = systemNoticeMessageRecord(frame);
  if (!record || isExternalEventOnlySystemNotice(record)) return false;
  if (textFromUnknown2(record.kind) === "comms") return true;
  const blocks = systemNoticeBlockRecords(record);
  if (blocks.some((block) => textFromUnknown2(block.type) === "comms")) return true;
  if (!canUseLegacyPeerNoticeText(record)) return false;
  return legacyPeerNoticeTextCandidates(record).some(isLegacyPeerNoticeText);
}
function typedSystemNoticeBlocksToRich(blocks, body, blobBaseUrl, sourceKind, consumeDuplicateCommsBlock) {
  const rich = [];
  const bodyText = textFromUnknown2(body);
  if (!Array.isArray(blocks)) {
    if (bodyText) rich.push({ type: "paragraph", text: bodyText });
    return rich;
  }
  let consumedDuplicateCommsBlock = false;
  for (let index = 0; index < blocks.length; index++) {
    const block = blocks[index];
    if (!block || typeof block !== "object") continue;
    const record = block;
    const type = textFromUnknown2(record.type);
    if (type === "comms") {
      const dedupeKeys = commsNoticeDedupeKeysFromBlock(record, bodyText, index);
      if (consumeCommsNoticeBlockDedupeKeys(dedupeKeys, consumeDuplicateCommsBlock)) {
        consumedDuplicateCommsBlock = true;
        continue;
      }
      const peer = record.peer && typeof record.peer === "object" ? record.peer : {};
      const peerLabel = peerLastSegment(textFromUnknown2(peer.display_name) || textFromUnknown2(peer.id) || "peer");
      const peerAliases = normalizedPeerAliases(
        textFromUnknown2(peer.display_name),
        textFromUnknown2(peer.id)
      );
      const kind = textFromUnknown2(record.kind) || "message";
      const direction = textFromUnknown2(record.direction);
      const intent = textFromUnknown2(record.intent);
      const requestId = textFromUnknown2(record.request_id) || `typed-comms:${peerLabel}:${kind}`;
      const contentBlocks2 = typedNoticeContentBlocks(record.content, blobBaseUrl);
      const contentText = contentBlocks2.map((item) => item.type === "paragraph" ? item.text : "").filter(Boolean).join("\n").trim();
      const peerImages = contentBlocks2.filter((item) => item.type === "image");
      const displayBodySource = stripPeerTransportScaffold(contentText) || stripPeerTransportScaffold(typedCommsStableBodyText(record)) || stripPeerTransportScaffold(bodyText);
      const preserveStructuredContentEnvelope = structuredCommsBodyShouldPreserveLeadingEnvelope(
        displayBodySource,
        peerAliases
      );
      const displayBody = normalizeStructuredCommsBodyText(
        displayBodySource,
        preserveStructuredContentEnvelope ? [] : peerAliases
      );
      rich.push({
        type: "tool-call",
        toolCallId: requestId,
        name: `peer_${kind}`,
        arguments: JSON.stringify(record.payload ?? {}, null, 2),
        status: "success",
        peerIncoming: direction !== "outgoing",
        peerTarget: peerLabel,
        ...intent ? { peerIntent: intent } : {},
        peerBody: displayBody || void 0,
        ...peerImages.length > 0 ? { peerImages } : {}
      });
      continue;
    }
    const legacyDedupeKeys = commsNoticeDedupeKeysFromBlock(record, bodyText, index);
    if (consumeCommsNoticeBlockDedupeKeys(legacyDedupeKeys, consumeDuplicateCommsBlock)) {
      consumedDuplicateCommsBlock = true;
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
    const contentBlocks = typedNoticeContentBlocks(record.content, blobBaseUrl);
    if (contentBlocks.length > 0) {
      rich.push(...contentBlocks);
      continue;
    }
    rich.push({ type: "divider", text: typedNoticeBlockText(record) || "Runtime metadata" });
  }
  if (rich.length === 0 && bodyText && !consumedDuplicateCommsBlock) {
    rich.push({ type: "paragraph", text: bodyText });
  }
  return rich;
}
function historyMessageText(message, peerRegistry, blobBaseUrl, toolResults, sourceKind, consumeDuplicateCommsBlock) {
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
      const blocks = typedSystemNoticeBlocksToRich(
        record.blocks,
        record.body,
        blobBaseUrl,
        sourceKind,
        consumeDuplicateCommsBlock
      );
      const duplicateCommsConsumed = Boolean(
        consumeDuplicateCommsBlock && blocks.length === 0 && systemNoticeBlockRecords(record).some((block, index) => commsNoticeDedupeKeysFromBlock(
          block,
          textFromUnknown2(record.body),
          index
        ).length > 0)
      );
      const text = duplicateCommsConsumed ? "" : typeof record.body === "string" ? record.body : blocks.map((block) => block.type === "paragraph" || block.type === "divider" ? block.text : "").filter(Boolean).join("\n");
      return { role: "meta", text, ...blocks.length > 0 ? { blocks } : {} };
    }
    case "assistant":
      return { role: "assistant", text: typeof record.content === "string" ? record.content : "" };
    case "block_assistant": {
      const blocks = Array.isArray(record.blocks) ? record.blocks : [];
      const richBlocks = blockAssistantRichBlocks(blocks, peerRegistry, toolResults);
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
      return { role: "assistant", text, ...richBlocks.length > 0 ? { blocks: richBlocks } : {} };
    }
    case "system":
      return { role: "system", text: typeof record.content === "string" ? record.content : "" };
    default:
      return { role: null, text: "" };
  }
}
function renderSessionHistoryTextCompleteEntry(agent, frame, entryId, options = {}) {
  if (frame.sourceKind !== "session_history") return null;
  const record = frame.data && typeof frame.data === "object" ? frame.data : {};
  const parsed = historyMessageText(
    record.message,
    options.peerRegistry,
    options.blobBaseUrl,
    options.toolResults,
    frame.sourceKind,
    options.consumeDuplicateCommsBlock
  );
  const text = parsed.text.trim();
  const parsedBlocks = Array.isArray(parsed.blocks) ? parsed.blocks : [];
  if (parsed.role === "meta") {
    const filteredParsedBlocks2 = options.consumeDuplicateToolBlock ? parsedBlocks.filter((block) => {
      if (block.type !== "tool-call") return true;
      return !options.consumeDuplicateToolBlock?.(block);
    }) : parsedBlocks;
    if (!text && filteredParsedBlocks2.length === 0) return null;
    const blocks2 = filteredParsedBlocks2.length > 0 ? filteredParsedBlocks2 : parseConversationRichBlocks(text, { displayNormalization: false });
    return {
      kind: "message",
      id: entryId,
      identity: COMMS_IDENTITY,
      variant: blocks2.length > 0 ? "rich" : "meta",
      createdAt: isoFromTimestampMs(frame.timestampMs),
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
  const blocks = filteredParsedBlocks.length > 0 ? filteredParsedBlocks : parseConversationRichBlocks(text, { displayNormalization: false });
  return {
    kind: "message",
    id: entryId,
    identity: agentIdentity(agent),
    variant: blocks.length > 0 ? "rich" : "plain",
    createdAt: isoFromTimestampMs(frame.timestampMs),
    ...blocks.length > 0 ? { blocks } : { text }
  };
}
function renderSystemNoticeEntry(frame, entryId, options = {}) {
  if (frame.event !== "system_notice") return null;
  const record = frame.data && typeof frame.data === "object" ? frame.data : {};
  const rawMessage = record.message && typeof record.message === "object" ? record.message : null;
  const message = rawMessage ? textFromUnknown2(rawMessage.role) ? rawMessage : { role: "system_notice", ...rawMessage } : {
    role: "system_notice",
    kind: record.kind,
    render_class: record.render_class,
    body: record.body,
    blocks: record.blocks
  };
  if (isExternalEventOnlySystemNotice(message)) return null;
  const parsed = historyMessageText(
    message,
    void 0,
    options.blobBaseUrl,
    void 0,
    frame.sourceKind,
    options.consumeDuplicateCommsBlock
  );
  if (parsed.role !== "meta") return null;
  const parsedBlocks = Array.isArray(parsed.blocks) ? parsed.blocks : [];
  const filteredParsedBlocks = options.consumeDuplicateToolBlock ? parsedBlocks.filter((block) => {
    if (block.type !== "tool-call") return true;
    return !options.consumeDuplicateToolBlock?.(block);
  }) : parsedBlocks;
  const text = parsed.text.trim();
  if (!text && filteredParsedBlocks.length === 0) return null;
  const blocks = filteredParsedBlocks.length > 0 ? filteredParsedBlocks : parseConversationRichBlocks(text, { displayNormalization: false });
  return {
    kind: "message",
    id: entryId,
    identity: COMMS_IDENTITY,
    variant: blocks.length > 0 ? "rich" : "meta",
    createdAt: isoFromTimestampMs(frame.timestampMs),
    ...blocks.length > 0 ? { blocks } : { text }
  };
}
function mapFramesToTimelineEntries2(agent, frames, options = {}) {
  const orderedFrames = options.renderInteractionStartsAsUser ? sortFramesForTranscript(frames) : frames;
  const entries = [];
  const workGraphNamesByCallId = workGraphToolNamesByCallId(orderedFrames);
  const toolBlocks = buildToolBlocks(orderedFrames, workGraphNamesByCallId);
  const workGraphEntriesByAnchor = buildWorkGraphEntries(agent, orderedFrames, workGraphNamesByCallId);
  const peerRegistry = buildPeerRegistry(orderedFrames);
  const sessionToolResults = historyToolResults(orderedFrames, workGraphNamesByCallId);
  const structuredCommsSignatures = structuredCommsNoticeTextSignatures(orderedFrames);
  const structuredCommsPromptSuppression = structuredCommsPromptSuppressionKeys(
    orderedFrames,
    structuredCommsSignatures
  );
  const emittedToolCalls = /* @__PURE__ */ new Set();
  const {
    liveToolCallIds,
    liveToolSignatureCounts
  } = liveToolDedupeState(orderedFrames, toolBlocks);
  const liveAssistantTerminalTexts = liveAssistantTerminalTextSignatures(orderedFrames);
  const emittedImages = /* @__PURE__ */ new Set();
  const emittedUserInputs = /* @__PURE__ */ new Set();
  const emittedCommsNotices = /* @__PURE__ */ new Map();
  let pendingText = "";
  let pendingId = "";
  let pendingCreatedAt;
  let pendingReasoningText = "";
  let pendingReasoningId = "";
  let pendingReasoningCreatedAt;
  let pendingReasoningInteractionId = "";
  const emittedReasoning = /* @__PURE__ */ new Map();
  let streamedInteractionText = "";
  let streamedInteractionId = "";
  function reasoningInteractionKey(interactionId) {
    return interactionId || "__unscoped__";
  }
  function reconcileEmittedReasoning(interactionId, text) {
    const normalized = normalizeComparableText(text);
    if (!normalized) return false;
    const previous = emittedReasoning.get(reasoningInteractionKey(interactionId)) || [];
    for (const candidate of previous) {
      if (candidate.normalized === normalized || candidate.normalized.includes(normalized)) {
        return true;
      }
      if (normalized.includes(candidate.normalized)) {
        if (!interactionId) {
          return true;
        }
        candidate.normalized = normalized;
        candidate.block.text = text;
        return true;
      }
    }
    return false;
  }
  function markEmittedReasoning(interactionId, text, block) {
    const normalized = normalizeComparableText(text);
    if (!normalized) return;
    const key = reasoningInteractionKey(interactionId);
    const previous = emittedReasoning.get(key) || [];
    if (!previous.some((candidate) => candidate.normalized === normalized)) {
      emittedReasoning.set(key, [...previous, { normalized, block }]);
    }
  }
  function flushPendingReasoning(final = false) {
    if (!pendingReasoningText.trim()) return;
    if (final && reconcileEmittedReasoning(pendingReasoningInteractionId, pendingReasoningText)) {
      pendingReasoningText = "";
      pendingReasoningId = "";
      pendingReasoningCreatedAt = void 0;
      pendingReasoningInteractionId = "";
      return;
    }
    const thinkingBlock = {
      type: "thinking",
      label: "",
      text: pendingReasoningText,
      ...final ? { final: true } : {}
    };
    entries.push({
      kind: "message",
      id: pendingReasoningId,
      identity: agentIdentity(agent),
      variant: "rich",
      ...pendingReasoningCreatedAt ? { createdAt: pendingReasoningCreatedAt } : {},
      blocks: [thinkingBlock]
    });
    if (final) {
      markEmittedReasoning(pendingReasoningInteractionId, pendingReasoningText, thinkingBlock);
    }
    pendingReasoningText = "";
    pendingReasoningId = "";
    pendingReasoningCreatedAt = void 0;
    pendingReasoningInteractionId = "";
  }
  function reconcilePendingReasoning(interactionId, text) {
    if (!pendingReasoningId || interactionId !== pendingReasoningInteractionId) return false;
    const normalizedPending = normalizeComparableText(pendingReasoningText);
    const normalizedText = normalizeComparableText(text);
    if (!normalizedPending || !normalizedText) return false;
    if (normalizedPending === normalizedText || normalizedPending.includes(normalizedText)) {
      return true;
    }
    if (normalizedText.includes(normalizedPending)) {
      pendingReasoningText = text;
      return true;
    }
    pendingReasoningText = `${pendingReasoningText.trimEnd()}

${text.trimStart()}`;
    return true;
  }
  function flushPendingText(final = true) {
    if (!pendingText) return;
    const blocks = final ? parseConversationRichBlocks(pendingText, { displayNormalization: false }) : parseStreamingConversationRichBlocks(pendingText, { displayNormalization: false });
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
    if (frame.event === "reasoning_delta") {
      const delta = reasoningFrameText(frame);
      if (!delta) continue;
      const frameInteractionId = frame.interactionId?.trim() || "";
      if (pendingReasoningId && frameInteractionId !== pendingReasoningInteractionId) {
        flushPendingReasoning(true);
      }
      if (!pendingReasoningId) {
        pendingReasoningId = entryId;
        pendingReasoningCreatedAt = isoFromTimestampMs(frame.timestampMs);
        pendingReasoningInteractionId = frameInteractionId;
      }
      pendingReasoningText += delta;
      continue;
    }
    if (frame.event === "reasoning_complete") {
      const frameInteractionId = frame.interactionId?.trim() || pendingReasoningInteractionId;
      if (pendingReasoningId && frameInteractionId !== pendingReasoningInteractionId) {
        flushPendingReasoning(true);
      }
      const text2 = reasoningFrameText(frame);
      if (text2) {
        if (reconcilePendingReasoning(frameInteractionId, text2)) {
          flushPendingReasoning(true);
          continue;
        }
        if (reconcileEmittedReasoning(frameInteractionId, text2)) {
          pendingReasoningText = "";
          pendingReasoningId = "";
          pendingReasoningCreatedAt = void 0;
          pendingReasoningInteractionId = "";
          continue;
        }
        pendingReasoningText = text2;
        if (!pendingReasoningId) {
          pendingReasoningId = entryId;
          pendingReasoningCreatedAt = isoFromTimestampMs(frame.timestampMs);
        }
        pendingReasoningInteractionId = frameInteractionId;
      }
      flushPendingReasoning(true);
      continue;
    }
    if (frame.event === "text_delta") {
      if (options.renderTextDeltas === false) {
        continue;
      }
      flushPendingReasoning(true);
      const frameInteractionId = frame.interactionId?.trim() || "";
      if (frameInteractionId !== streamedInteractionId) {
        streamedInteractionText = "";
        streamedInteractionId = frameInteractionId;
      }
      const delta = summarizeFrameData(frame.data);
      if (!pendingId) {
        pendingId = entryId;
        pendingCreatedAt = isoFromTimestampMs(frame.timestampMs);
      }
      pendingText += delta;
      streamedInteractionText += delta;
      continue;
    }
    if (frame.event === "assistant_image" || frame.event === "assistant_image_appended") {
      flushPendingReasoning(true);
      flushPendingText();
      const imageEntry = renderAssistantImageEntry(agent, frame, entryId, options.blobBaseUrl);
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
    if (isWorkGraphToolFrame(frame, workGraphNamesByCallId)) {
      const workGraphCards = workGraphEntriesByAnchor.get(i);
      if (workGraphCards && workGraphCards.length > 0) {
        flushPendingReasoning(true);
        flushPendingText();
        for (const card of workGraphCards) {
          entries.push(card);
        }
      }
      continue;
    }
    const toolCallId = parseToolCallId(frame);
    if (toolCallId && (frame.event === "tool_call_requested" || frame.event === "tool_call" || frame.event === "tool_execution_started" || frame.event === "server_tool_content") && !emittedToolCalls.has(toolCallId)) {
      flushPendingReasoning(true);
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
      flushPendingReasoning(true);
      const imageEntries = renderGeneratedImageToolResultEntries(
        agent,
        frame,
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
    if (options.renderInteractionStartsAsUser && (frame.event === "interaction_started" || frame.event === "user_input")) {
      flushPendingReasoning(true);
      flushPendingText();
      const frameInteractionId = frame.interactionId?.trim() || "";
      if (frameInteractionId !== streamedInteractionId) {
        streamedInteractionText = "";
        streamedInteractionId = frameInteractionId;
      }
      const userEntry = renderHistoryUserEntry(frame, entryId, options.blobBaseUrl);
      if (userEntry) {
        const userKey = userEntryDedupeKey(frame, userEntry);
        if (userKey && emittedUserInputs.has(userKey)) {
          continue;
        }
        if (userKey) emittedUserInputs.add(userKey);
        entries.push(userEntry);
      }
      continue;
    }
    if (frame.event === "run_started") {
      flushPendingReasoning(true);
      flushPendingText();
      const promptEntries = renderRunStartedPromptEntries(frame, entryId, {
        suppressEmbeddedRpcPrompt: options.suppressEmbeddedRunStartedPrompt === true,
        suppressStructuredCommsPrompt: structuredCommsPromptSuppression.has(entryId),
        blobBaseUrl: options.blobBaseUrl
      });
      if (promptEntries.length > 0) {
        for (const promptEntry of promptEntries) {
          const userKey = userPromptDedupeKey(frame, promptEntry);
          if (userKey && emittedUserInputs.has(userKey)) {
            continue;
          }
          if (userKey) emittedUserInputs.add(userKey);
          entries.push(promptEntry);
        }
        continue;
      }
    }
    if (frame.event === "system_notice") {
      flushPendingReasoning(true);
      flushPendingText();
      if (shouldSuppressDuplicateCommsNotice(frame, emittedCommsNotices)) {
        continue;
      }
      const noticeEntry = renderSystemNoticeEntry(frame, entryId, {
        blobBaseUrl: options.blobBaseUrl,
        consumeDuplicateCommsBlock: (key) => {
          if (commsNoticeDuplicateKey(key, frame, emittedCommsNotices)) {
            return true;
          }
          markCommsNoticeDedupeKey(key, frame, emittedCommsNotices);
          return false;
        },
        consumeDuplicateToolBlock: (block) => WORKGRAPH_TOOL_NAMES.has(block.name) || liveToolCallIds.has(block.toolCallId) || consumeToolSignatureCount(liveToolSignatureCounts, block)
      });
      if (noticeEntry) {
        entries.push(noticeEntry);
      }
      continue;
    }
    if (frame.event === "text_complete") {
      flushPendingReasoning(true);
      if (frame.sourceKind !== "session_history") {
        const text2 = terminalFrameVisibleText(frame).trim();
        if (text2 && pendingText && normalizeComparableText(pendingText) === normalizeComparableText(text2)) {
          continue;
        }
        const interactionId = frame.interactionId?.trim();
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
      const historyText = frame.sourceKind === "session_history" ? terminalFrameVisibleText(frame).trim() : "";
      if (historyText && liveAssistantTerminalTexts.has(normalizeComparableText(historyText))) {
        continue;
      }
      const historyEntry = renderSessionHistoryTextCompleteEntry(agent, frame, entryId, {
        peerRegistry,
        blobBaseUrl: options.blobBaseUrl,
        toolResults: sessionToolResults,
        consumeDuplicateCommsBlock: (key) => {
          if (commsNoticeDuplicateKey(key, frame, emittedCommsNotices)) {
            return true;
          }
          markCommsNoticeDedupeKey(key, frame, emittedCommsNotices);
          return false;
        },
        consumeDuplicateToolBlock: (block) => WORKGRAPH_TOOL_NAMES.has(block.name) || liveToolCallIds.has(block.toolCallId) || consumeToolSignatureCount(liveToolSignatureCounts, block)
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
    if (frame.event === "interaction_complete" || frame.event === "interaction_failed" || frame.event === "run_failed") {
      const streamedText = streamedInteractionText || pendingText;
      flushPendingReasoning(true);
      flushPendingText();
      streamedInteractionText = "";
      streamedInteractionId = "";
      if (frame.sourceKind === "session_history") {
        const historyText = terminalFrameVisibleText(frame).trim();
        if (historyText && liveAssistantTerminalTexts.has(normalizeComparableText(historyText))) {
          continue;
        }
        const historyEntry = renderSessionHistoryTextCompleteEntry(agent, frame, entryId, {
          peerRegistry,
          blobBaseUrl: options.blobBaseUrl,
          toolResults: sessionToolResults,
          consumeDuplicateCommsBlock: (key) => {
            if (commsNoticeDuplicateKey(key, frame, emittedCommsNotices)) {
              return true;
            }
            markCommsNoticeDedupeKey(key, frame, emittedCommsNotices);
            return false;
          },
          consumeDuplicateToolBlock: (block) => WORKGRAPH_TOOL_NAMES.has(block.name) || liveToolCallIds.has(block.toolCallId) || consumeToolSignatureCount(liveToolSignatureCounts, block)
        });
        if (historyEntry) {
          if (shouldSuppressRepeatedAssistantEntry(historyEntry, entries)) {
            continue;
          }
          entries.push(historyEntry);
        }
        continue;
      }
      const terminalEntry = renderTerminalEntry(agent, frame, entryId, streamedText);
      if (terminalEntry) {
        if (shouldSuppressRepeatedAssistantEntry(terminalEntry, entries)) {
          continue;
        }
        entries.push(terminalEntry);
      }
      continue;
    }
    if (HIDDEN_EVENTS2.has(frame.event)) {
      continue;
    }
    flushPendingReasoning(true);
    flushPendingText();
    const peerEntry = renderPeerEntry(frame, entryId);
    if (peerEntry) {
      entries.push(peerEntry);
      continue;
    }
    if (frame.event === "tool_call_requested" || frame.event === "tool_call" || frame.event === "tool_execution_started" || frame.event === "tool_result_received" || frame.event === "tool_execution_completed") {
      continue;
    }
    if (frame.event.startsWith("memory.")) {
      entries.push({
        kind: "message",
        id: entryId,
        identity: SYSTEM_IDENTITY,
        variant: "meta",
        createdAt: isoFromTimestampMs(frame.timestampMs),
        text: describeMemoryTimelineEvent2(
          frame.event,
          frame.data && typeof frame.data === "object" ? frame.data : {}
        )
      });
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
  flushPendingReasoning(false);
  flushPendingText(false);
  return entries;
}
function createUserEntry2(message, images = []) {
  if (images.length > 0) {
    const blocks = [
      ...parseConversationRichBlocks(message, { displayNormalization: false }),
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
function appendOptimisticConversationEntry2(entries, optimisticEntry) {
  return optimisticEntry ? [...entries, optimisticEntry] : entries;
}
function inferResponsePhaseFromFrames2(frames, fallback = null) {
  let phase = fallback;
  let interactionOpen = false;
  let runOpen = false;
  for (const frame of frames) {
    switch (frame.event) {
      case "user_input":
        if (isTerminalUserInputStatus(frame.status)) phase = null;
        else phase = "waiting";
        break;
      case "interaction_started":
        interactionOpen = true;
        phase = "waiting";
        break;
      case "run_started":
        runOpen = true;
        phase = "waiting";
        break;
      case "tool_call_requested":
      case "tool_call":
      case "tool_execution_started":
        phase = "tool-executing";
        break;
      case "server_tool_content":
        if (isActiveServerToolContentFrame(frame)) phase = "tool-executing";
        else if (isTerminalServerToolContentFrame(frame)) phase = "waiting";
        break;
      case "tool_result_received":
      case "tool_execution_completed":
        phase = "waiting";
        break;
      case "reasoning_delta":
        phase = "generating";
        break;
      case "reasoning_complete":
        phase = "waiting";
        break;
      case "text_delta":
        phase = "generating";
        break;
      case "text_complete":
        phase = interactionOpen || runOpen ? "waiting" : null;
        break;
      case "interaction_complete":
      case "interaction_failed":
        interactionOpen = false;
        runOpen = false;
        phase = null;
        break;
      case "run_completed":
      case "run_failed":
        runOpen = false;
        phase = interactionOpen ? "waiting" : null;
        break;
      case "system_notice":
        if (systemNoticeClearsBusyState2(frame)) phase = null;
        break;
      case "turn_completed": {
        const data = frame.data && typeof frame.data === "object" ? frame.data : {};
        const stopReason = data.stop_reason ?? data.stopReason;
        if (typeof stopReason === "string" ? stopReason !== "tool_use" : true) {
          phase = interactionOpen || runOpen ? "waiting" : null;
        }
        break;
      }
      default:
        break;
    }
  }
  return phase;
}
function isTerminalUserInputStatus(status) {
  return status === "completed" || status === "delivery_failed" || status === "failed";
}
function resolvePanelResponsePhase2(args) {
  if (args.hasLocalPhase) {
    return args.localPhase ?? null;
  }
  if (args.frames.length > 0) {
    const localPhase = inferResponsePhaseFromFrames2(args.frames, null);
    if (args.serverPhase && localPhase === null && !latestRoutableFrameIsTerminal(args.frames)) {
      return args.serverPhase;
    }
    return localPhase;
  }
  return args.serverPhase ?? null;
}
function latestRoutableFrameIsTerminal(frames) {
  for (let index = frames.length - 1; index >= 0; index -= 1) {
    const frame = frames[index];
    switch (frame.event) {
      case "user_input":
        return isTerminalUserInputStatus(frame.status);
      case "text_complete":
      case "run_completed":
      case "run_failed":
        return !hasOpenLifecycleBefore(frames, index);
      case "interaction_complete":
      case "interaction_failed":
      case "message_delivery_failed":
        return true;
      case "system_notice":
        return systemNoticeClearsBusyState2(frame);
      case "turn_completed": {
        const data = frame.data && typeof frame.data === "object" ? frame.data : {};
        const stopReason = data.stop_reason ?? data.stopReason;
        return typeof stopReason === "string" ? stopReason !== "tool_use" : true;
      }
      case "interaction_started":
      case "run_started":
      case "tool_call_requested":
      case "tool_call":
      case "tool_execution_started":
      case "tool_result_received":
      case "tool_execution_completed":
      case "reasoning_delta":
      case "reasoning_complete":
      case "text_delta":
        return false;
      default:
        break;
    }
  }
  return false;
}
function hasOpenLifecycleBefore(frames, beforeIndex) {
  let interactionOpen = false;
  let runOpen = false;
  for (let index = 0; index < beforeIndex; index += 1) {
    switch (frames[index].event) {
      case "interaction_started":
        interactionOpen = true;
        break;
      case "run_started":
        runOpen = true;
        break;
      case "interaction_complete":
      case "interaction_failed":
        interactionOpen = false;
        runOpen = false;
        break;
      case "run_completed":
      case "run_failed":
        runOpen = false;
        break;
      case "message_delivery_failed":
        interactionOpen = false;
        runOpen = false;
        break;
      default:
        break;
    }
  }
  return interactionOpen || runOpen;
}
function buildConversationViewState2(args) {
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
function buildActivityRailViewState2(args) {
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
    if (ACTIVITY_HIDDEN_EVENTS2.has(frame.event)) {
      return false;
    }
    if (frame.sourceKind === "session_history") {
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
function jsonRpcErrorCode(error) {
  const rpcError = error?.rpcError;
  return typeof rpcError?.code === "number" ? rpcError.code : null;
}

// src/lib/contract.ts
var CONSOLE_REST_PATHS2 = {
  experience: "/console/experience",
  modules: "/console/modules",
  identities: "/console/identities",
  timeline: "/console/timeline",
  timelineStream: "/console/timeline/stream",
  identityTimelineStreamTemplate: "/console/identity/{identity}/stream",
  legacySend: "/console/send"
};
var CONSOLE_RPC_PATHS2 = {
  jsonRpc: "/console/rpc",
  multipartJsonRpc: "/console/rpc/multipart"
};
var CONSOLE_RPC_METHODS2 = {
  capabilities: "mobkit/capabilities",
  send: "mobkit/console/send",
  listIdentities: "mobkit/console/list_identities",
  inspectIdentity: "mobkit/console/inspect_identity",
  queryTimeline: "mobkit/console/query_timeline",
  blobUpload: "mobkit/blob/upload",
  retireIdentity: "mobkit/retire",
  respawnIdentity: "mobkit/respawn",
  resetIdentity: "mobkit/reset",
  routingRoutesList: "mobkit/routing/routes/list",
  deliveryHistory: "mobkit/delivery/history",
  gatingPending: "mobkit/gating/pending",
  gatingAudit: "mobkit/gating/audit",
  gatingDecide: "mobkit/gating/decide",
  accessStatus: "mobkit/access/status",
  accessGet: "mobkit/access/get",
  accessSet: "mobkit/access/set",
  accessEnable: "mobkit/access/enable",
  accessRuleUpsert: "mobkit/access/rules/upsert",
  accessRuleDelete: "mobkit/access/rules/delete",
  accessGroupSet: "mobkit/access/groups/set",
  accessGroupDelete: "mobkit/access/groups/delete",
  accessPreview: "mobkit/access/preview",
  memoryPanelRecords: "mobkit/memory/panel/records",
  memoryPanelRecord: "mobkit/memory/panel/record",
  memoryPanelQuarantine: "mobkit/memory/panel/quarantine",
  memoryPanelDreams: "mobkit/memory/panel/dreams",
  memoryPanelOverview: "mobkit/memory/panel/overview",
  memoryPanelProposals: "mobkit/memory/panel/proposals",
  memoryPanelInjections: "mobkit/memory/panel/injections",
  memoryPanelHarvests: "mobkit/memory/panel/harvests",
  memoryPanelDreamRuns: "mobkit/memory/panel/dream_runs",
  memoryPanelAuditVerdicts: "mobkit/memory/panel/audit_verdicts",
  workgraphSnapshot: "mobkit/workgraph/snapshot",
  workgraphEvents: "mobkit/workgraph/events",
  workgraphGet: "mobkit/workgraph/get",
  workgraphGoalStatus: "mobkit/workgraph/goal/status",
  workgraphClaim: "mobkit/workgraph/claim",
  workgraphRelease: "mobkit/workgraph/release",
  workgraphClose: "mobkit/workgraph/close",
  workgraphGoalConfirm: "mobkit/workgraph/goal/confirm",
  workgraphGoalRequestClose: "mobkit/workgraph/goal/request_close",
  workgraphAttentionPause: "mobkit/workgraph/attention/pause",
  workgraphAttentionResume: "mobkit/workgraph/attention/resume",
  workgraphAttentionReassign: "mobkit/workgraph/attention/reassign"
};
var CONSOLE_BLOB_PATH_PREFIX2 = "/blobs/";
var CONSOLE_TIMELINE_REPLAY_UNAVAILABLE_CODE2 = -32013;

// src/lib/network.ts
function unwrapConsoleEnvelope(eventName, data) {
  if (!data || typeof data !== "object") {
    return { data };
  }
  const record = data;
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
      sourceKind: frame.sourceKind,
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
function parseSseFrames2(rawText) {
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
var DEFAULT_CONSOLE_FETCH_TIMEOUT_MS2 = 6e4;
var ERROR_BODY_PREVIEW_LIMIT = 500;
function formatTimeoutReason(timeoutMs) {
  if (timeoutMs % 1e3 === 0) {
    return `${timeoutMs / 1e3} s`;
  }
  return `${timeoutMs} ms`;
}
async function fetchWithConsoleTimeout(input, init, label, timeoutMs = DEFAULT_CONSOLE_FETCH_TIMEOUT_MS2) {
  const controller = new AbortController();
  const timeoutReason = `${label} timeout after ${formatTimeoutReason(timeoutMs)}`;
  const timer = globalThis.setTimeout(() => controller.abort(timeoutReason), timeoutMs);
  try {
    return await fetch(input, {
      ...init,
      signal: controller.signal
    });
  } catch (error) {
    if (controller.signal.aborted && typeof controller.signal.reason === "string") {
      throw new Error(controller.signal.reason);
    }
    throw error;
  } finally {
    globalThis.clearTimeout(timer);
  }
}
async function responseErrorPreview(response) {
  const text = await response.text();
  return responseTextErrorPreview(text);
}
function responseTextErrorPreview(text) {
  const trimmed = text.trim();
  if (!trimmed) {
    return "";
  }
  try {
    const parsed = JSON.parse(trimmed);
    if (parsed && typeof parsed === "object") {
      const record = parsed;
      const message = typeof record.message === "string" ? record.message : void 0;
      const error = record.error && typeof record.error === "object" ? record.error : null;
      const errorMessage2 = error && typeof error.message === "string" ? error.message : void 0;
      const errorCode = error && (typeof error.code === "string" || typeof error.code === "number") ? String(error.code) : void 0;
      const selected = [
        errorCode ? `code=${errorCode}` : "",
        errorMessage2 || message || ""
      ].filter(Boolean).join(" ");
      if (selected) {
        return selected;
      }
    }
  } catch {
  }
  return trimmed.length > ERROR_BODY_PREVIEW_LIMIT ? `${trimmed.slice(0, ERROR_BODY_PREVIEW_LIMIT)}...` : trimmed;
}
async function fetchJson2(baseUrl, path, timeoutMs = DEFAULT_CONSOLE_FETCH_TIMEOUT_MS2) {
  const response = await fetchWithConsoleTimeout(
    `${baseUrl}${path}`,
    {},
    "console fetch",
    timeoutMs
  );
  if (!response.ok) {
    const preview = await responseErrorPreview(response);
    throw new Error(`Request failed ${response.status} for ${path}${preview ? `: ${preview}` : ""}`);
  }
  return response.json();
}
async function rpc(baseUrl, method, params, timeoutMs = DEFAULT_CONSOLE_FETCH_TIMEOUT_MS2) {
  const response = await fetchWithConsoleTimeout(
    `${baseUrl}${CONSOLE_RPC_PATHS2.jsonRpc}`,
    {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({
        jsonrpc: "2.0",
        id: `${method}:${Date.now()}`,
        method,
        params
      })
    },
    "console rpc",
    timeoutMs
  );
  if (!response.ok) {
    const preview = await responseErrorPreview(response);
    throw new Error(`${method} request failed ${response.status}${preview ? `: ${preview}` : ""}`);
  }
  const result = await response.json();
  if (result.error) {
    const typedError = normalizeConsoleInteractionRejectedError(result.error);
    if (typedError) {
      const error2 = new Error(`${method} RPC error ${typedError.code}: ${typedError.message}`);
      error2.rpcError = typedError;
      throw error2;
    }
    const replayError = normalizeReplayUnavailableError(result.error.data);
    if (replayError || result.error.code === CONSOLE_TIMELINE_REPLAY_UNAVAILABLE_CODE2) {
      const error2 = new Error(
        `${method} RPC replay unavailable: ${result.error.message || JSON.stringify(result.error)}`
      );
      const annotated = error2;
      if (replayError) {
        annotated.replayError = replayError;
      }
      annotated.timelineReplayUnavailable = true;
      throw error2;
    }
    const error = new Error(`${method} RPC error: ${result.error.message || JSON.stringify(result.error)}`);
    error.rpcError = result.error;
    throw error;
  }
  return result.result;
}
async function sendConsoleMultipart2(baseUrl, identity, contentInput, attachments, origin, idempotencyKey, handlingMode = "queue", timeoutMs = DEFAULT_CONSOLE_FETCH_TIMEOUT_MS2) {
  const content = typeof contentInput === "string" ? contentInput.trim() ? [{ type: "text", text: contentInput }] : [] : [...contentInput];
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
    id: `${CONSOLE_RPC_METHODS2.send}:${Date.now()}`,
    method: CONSOLE_RPC_METHODS2.send,
    params: {
      identity,
      content,
      origin,
      idempotency_key: idempotencyKey,
      handling_mode: handlingMode
    }
  }));
  const response = await fetchWithConsoleTimeout(
    `${baseUrl}${CONSOLE_RPC_PATHS2.multipartJsonRpc}`,
    {
      method: "POST",
      body: form
    },
    "console multipart",
    timeoutMs
  );
  if (!response.ok) {
    const preview = await responseErrorPreview(response);
    throw new Error(`${CONSOLE_RPC_METHODS2.send} multipart failed ${response.status}${preview ? `: ${preview}` : ""}`);
  }
  const result = await response.json();
  if (result.error) {
    throw new Error(`${CONSOLE_RPC_METHODS2.send} RPC error: ${result.error.message || JSON.stringify(result.error)}`);
  }
  return normalizeConsoleTimelineAccepted(result.result, identity);
}
async function uploadConsoleBlobMultipart2(baseUrl, input, timeoutMs = DEFAULT_CONSOLE_FETCH_TIMEOUT_MS2) {
  const file = input.file;
  if (!file) {
    throw new Error(`${CONSOLE_RPC_METHODS2.blobUpload} requires a file`);
  }
  const mediaType = input.mediaType || file.type || "application/octet-stream";
  const uploadId = input.blobId?.trim() || `upload-${Date.now().toString(36)}-0`;
  const uploadFile = file.type === mediaType ? file : new File([file], file.name || "upload", { type: mediaType });
  const form = new FormData();
  form.append(`file:${uploadId}`, uploadFile, uploadFile.name || file.name || "upload");
  form.append("payload", JSON.stringify({
    jsonrpc: "2.0",
    id: `${CONSOLE_RPC_METHODS2.blobUpload}:${Date.now()}`,
    method: CONSOLE_RPC_METHODS2.blobUpload,
    params: {
      upload: {
        type: "image_upload",
        upload_id: uploadId,
        media_type: mediaType,
        alt: file.name || "upload"
      }
    }
  }));
  const response = await fetchWithConsoleTimeout(
    `${baseUrl}${CONSOLE_RPC_PATHS2.multipartJsonRpc}`,
    {
      method: "POST",
      body: form
    },
    "console multipart",
    timeoutMs
  );
  if (!response.ok) {
    const preview = await responseErrorPreview(response);
    throw new Error(`${CONSOLE_RPC_METHODS2.blobUpload} multipart failed ${response.status}${preview ? `: ${preview}` : ""}`);
  }
  const result = await response.json();
  if (result.error) {
    throw new Error(`${CONSOLE_RPC_METHODS2.blobUpload} RPC error: ${result.error.message || JSON.stringify(result.error)}`);
  }
  const record = result.result && typeof result.result === "object" ? result.result : {};
  const blobId = typeof record.blob_id === "string" ? record.blob_id : "";
  if (!blobId) {
    throw new Error(`${CONSOLE_RPC_METHODS2.blobUpload} returned an invalid blob payload`);
  }
  return {
    blob_id: blobId,
    url: typeof record.url === "string" ? record.url : void 0
  };
}
var TERMINAL_SSE_EVENTS = /* @__PURE__ */ new Set([
  "interaction_complete",
  "run_completed",
  "interaction_failed",
  "run_failed",
  "turn_completed"
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
function isTerminalTurnCompletedData(data) {
  const record = data && typeof data === "object" ? data : {};
  const stopReason = record.stop_reason ?? record.stopReason;
  return typeof stopReason === "string" ? stopReason !== "tool_use" : true;
}
function isTerminalSseFrame(frame) {
  if (!TERMINAL_SSE_EVENTS.has(frame.event || "")) return false;
  if (frame.event !== "turn_completed") return true;
  return isTerminalTurnCompletedData(frame.data);
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
    const preview = responseTextErrorPreview(text);
    throw new Error(`interaction stream request failed ${response.status}${preview ? `: ${preview}` : ""}`);
  }
  const replayUnavailableError = (frame) => {
    if (frame.event !== "replay_unavailable") {
      return null;
    }
    const replayError = normalizeReplayUnavailableError(frame.data);
    if (!replayError) {
      return new Error("timeline stream replay unavailable");
    }
    const error = new Error(
      `interaction stream replay unavailable for ${replayError.stream}: ${replayError.requested_last_event_id} -> ${replayError.latest_event_id}`
    );
    error.replayError = replayError;
    return error;
  };
  if (!response.body || typeof response.body.getReader !== "function") {
    const frames2 = parseSseFrames2(await response.text());
    for (const frame of frames2) {
      const replayError = replayUnavailableError(frame);
      if (replayError) {
        throw replayError;
      }
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
        const replayError = replayUnavailableError(frame);
        if (replayError) {
          throw replayError;
        }
        if (matchesCorrelation(frame, options.correlation, true)) {
          frames.push(frame);
          options.onFrame?.(frame);
          if (stopOnTerminal && isTerminalSseFrame(frame)) {
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
      const replayError = replayUnavailableError(frame);
      if (replayError) {
        throw replayError;
      }
      if (matchesCorrelation(frame, options.correlation, true)) {
        frames.push(frame);
        options.onFrame?.(frame);
      }
    });
    flushTrailingSseBlock(frameBuffer, (frame) => {
      const replayError = replayUnavailableError(frame);
      if (replayError) {
        throw replayError;
      }
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
    for (const frame of parseSseFrames2(block)) {
      onFrame(frame);
    }
  }
  return buffer;
}
function flushTrailingSseBlock(buffer, onFrame) {
  if (!buffer.trim()) {
    return;
  }
  for (const frame of parseSseFrames2(`${buffer}

`)) {
    onFrame(frame);
  }
}
async function queryTimeline2(baseUrl, target, limit = 400, timeoutMs = DEFAULT_CONSOLE_FETCH_TIMEOUT_MS2) {
  const result = await rpc(baseUrl, CONSOLE_RPC_METHODS2.queryTimeline, {
    limit,
    ...target.identity?.trim() ? { identity: target.identity.trim() } : {},
    ...target.conversationId?.trim() ? { conversation_id: target.conversationId.trim() } : {},
    ...target.after?.trim() ? { after: target.after.trim() } : {},
    ...target.before?.trim() ? { before: target.before.trim() } : {},
    ...target.mode ? { mode: target.mode } : {}
  }, timeoutMs);
  if (!result || typeof result !== "object") {
    return { frames: [], available: false };
  }
  const record = result;
  const rawFrames = Array.isArray(record.frames) ? record.frames : [];
  return {
    frames: rawFrames.map(timelineFrameToConsoleFrame),
    nextCursor: typeof record.next_cursor === "string" ? record.next_cursor : void 0,
    latestCursor: typeof record.latest_cursor === "string" ? record.latest_cursor : void 0,
    exhausted: record.exhausted === true,
    available: record.available !== false
  };
}
async function sendConsole2(baseUrl, identity, content, origin, idempotencyKey, handlingMode = "queue", timeoutMs = DEFAULT_CONSOLE_FETCH_TIMEOUT_MS2) {
  const accepted = await rpc(baseUrl, CONSOLE_RPC_METHODS2.send, {
    identity,
    content,
    origin,
    idempotency_key: idempotencyKey,
    handling_mode: handlingMode
  }, timeoutMs);
  if (!accepted || typeof accepted !== "object") {
    throw new Error(`${CONSOLE_RPC_METHODS2.send} returned an invalid acceptance payload`);
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
async function callConsoleRpc2(baseUrl, method, params = {}, timeoutMs = DEFAULT_CONSOLE_FETCH_TIMEOUT_MS2) {
  return rpc(baseUrl, method, params, timeoutMs);
}
function timelineStreamPath(target) {
  const params = new URLSearchParams();
  if (target.identity?.trim()) params.set("identity", target.identity.trim());
  if (target.conversationId?.trim()) params.set("conversation_id", target.conversationId.trim());
  return `${CONSOLE_REST_PATHS2.timelineStream}${params.size > 0 ? `?${params.toString()}` : ""}`;
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
function subscribeTimelineEvents2(baseUrl, target, onFrame) {
  let stopped = false;
  let controller = null;
  let after = target.after?.trim() || void 0;
  const fetchImpl = globalThis.fetch;
  void (async () => {
    let retryDelayMs = 250;
    while (!stopped) {
      controller = new AbortController();
      try {
        const headers = { "content-type": "application/json" };
        if (after) {
          headers["Last-Event-ID"] = after;
        }
        await streamFramesFromResponse(
          await fetchImpl(`${baseUrl}${timelineStreamPath(target)}`, {
            method: "GET",
            headers,
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
        const replayError = error.replayError;
        if (replayError?.latest_event_id) {
          after = replayError.latest_event_id;
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

// src/lib/headless.ts
var CONSOLE_COMMAND_NAMES2 = {
  inspectIdentity: "inspectIdentity",
  retireIdentity: "retireIdentity",
  respawnIdentity: "respawnIdentity",
  resetIdentity: "resetIdentity",
  listRoutingRoutes: "listRoutingRoutes",
  listDeliveryHistory: "listDeliveryHistory",
  listGatingPending: "listGatingPending",
  listGatingAudit: "listGatingAudit",
  decideGating: "decideGating",
  accessStatus: "accessStatus",
  getAccessConfig: "getAccessConfig",
  setAccessConfig: "setAccessConfig",
  enableAccess: "enableAccess",
  upsertAccessRule: "upsertAccessRule",
  deleteAccessRule: "deleteAccessRule",
  setAccessGroup: "setAccessGroup",
  deleteAccessGroup: "deleteAccessGroup",
  previewAccess: "previewAccess",
  listMemoryRecords: "listMemoryRecords",
  getMemoryRecord: "getMemoryRecord",
  listMemoryQuarantine: "listMemoryQuarantine",
  listMemoryDreams: "listMemoryDreams",
  getMemoryOverview: "getMemoryOverview",
  listMemoryProposals: "listMemoryProposals",
  listMemoryInjections: "listMemoryInjections",
  listMemoryHarvests: "listMemoryHarvests",
  listMemoryDreamRuns: "listMemoryDreamRuns",
  listMemoryAuditVerdicts: "listMemoryAuditVerdicts",
  workgraphSnapshot: "workgraphSnapshot",
  workgraphEvents: "workgraphEvents",
  workgraphGet: "workgraphGet",
  workgraphGoalStatus: "workgraphGoalStatus",
  workgraphClaim: "workgraphClaim",
  workgraphRelease: "workgraphRelease",
  workgraphClose: "workgraphClose",
  workgraphGoalConfirm: "workgraphGoalConfirm",
  workgraphGoalRequestClose: "workgraphGoalRequestClose",
  workgraphAttentionPause: "workgraphAttentionPause",
  workgraphAttentionResume: "workgraphAttentionResume",
  workgraphAttentionReassign: "workgraphAttentionReassign"
};
var LEGACY_INSPECT_IDENTITY_METHOD = "mobkit/inspect_identity";
var MIN_TIMELINE_DEDUP_KEYS = 1e3;
var CONSOLE_COMMAND_SPECS2 = {
  [CONSOLE_COMMAND_NAMES2.inspectIdentity]: {
    method: CONSOLE_RPC_METHODS2.inspectIdentity,
    targetKinds: /* @__PURE__ */ new Set([
      "mobkit/identity-chat",
      "mobkit/identity-inspect"
    ])
  },
  [CONSOLE_COMMAND_NAMES2.retireIdentity]: {
    method: CONSOLE_RPC_METHODS2.retireIdentity,
    targetKinds: /* @__PURE__ */ new Set([
      "mobkit/identity-chat",
      "mobkit/identity-inspect"
    ])
  },
  [CONSOLE_COMMAND_NAMES2.respawnIdentity]: {
    method: CONSOLE_RPC_METHODS2.respawnIdentity,
    targetKinds: /* @__PURE__ */ new Set([
      "mobkit/identity-chat",
      "mobkit/identity-inspect"
    ])
  },
  [CONSOLE_COMMAND_NAMES2.resetIdentity]: {
    method: CONSOLE_RPC_METHODS2.resetIdentity,
    targetKinds: /* @__PURE__ */ new Set([
      "mobkit/identity-chat",
      "mobkit/identity-inspect"
    ])
  },
  [CONSOLE_COMMAND_NAMES2.listRoutingRoutes]: {
    method: CONSOLE_RPC_METHODS2.routingRoutesList,
    targetKinds: /* @__PURE__ */ new Set(["mobkit/routing"])
  },
  [CONSOLE_COMMAND_NAMES2.listDeliveryHistory]: {
    method: CONSOLE_RPC_METHODS2.deliveryHistory,
    targetKinds: /* @__PURE__ */ new Set(["mobkit/routing"])
  },
  [CONSOLE_COMMAND_NAMES2.listGatingPending]: {
    method: CONSOLE_RPC_METHODS2.gatingPending,
    targetKinds: /* @__PURE__ */ new Set(["mobkit/gating"])
  },
  [CONSOLE_COMMAND_NAMES2.listGatingAudit]: {
    method: CONSOLE_RPC_METHODS2.gatingAudit,
    targetKinds: /* @__PURE__ */ new Set(["mobkit/gating"])
  },
  [CONSOLE_COMMAND_NAMES2.decideGating]: {
    method: CONSOLE_RPC_METHODS2.gatingDecide,
    targetKinds: /* @__PURE__ */ new Set(["mobkit/gating"])
  },
  [CONSOLE_COMMAND_NAMES2.accessStatus]: {
    method: CONSOLE_RPC_METHODS2.accessStatus,
    targetKinds: /* @__PURE__ */ new Set(["mobkit/access"])
  },
  [CONSOLE_COMMAND_NAMES2.getAccessConfig]: {
    method: CONSOLE_RPC_METHODS2.accessGet,
    targetKinds: /* @__PURE__ */ new Set(["mobkit/access"])
  },
  [CONSOLE_COMMAND_NAMES2.setAccessConfig]: {
    method: CONSOLE_RPC_METHODS2.accessSet,
    targetKinds: /* @__PURE__ */ new Set(["mobkit/access"])
  },
  [CONSOLE_COMMAND_NAMES2.enableAccess]: {
    method: CONSOLE_RPC_METHODS2.accessEnable,
    targetKinds: /* @__PURE__ */ new Set(["mobkit/access"])
  },
  [CONSOLE_COMMAND_NAMES2.upsertAccessRule]: {
    method: CONSOLE_RPC_METHODS2.accessRuleUpsert,
    targetKinds: /* @__PURE__ */ new Set(["mobkit/access"])
  },
  [CONSOLE_COMMAND_NAMES2.deleteAccessRule]: {
    method: CONSOLE_RPC_METHODS2.accessRuleDelete,
    targetKinds: /* @__PURE__ */ new Set(["mobkit/access"])
  },
  [CONSOLE_COMMAND_NAMES2.setAccessGroup]: {
    method: CONSOLE_RPC_METHODS2.accessGroupSet,
    targetKinds: /* @__PURE__ */ new Set(["mobkit/access"])
  },
  [CONSOLE_COMMAND_NAMES2.deleteAccessGroup]: {
    method: CONSOLE_RPC_METHODS2.accessGroupDelete,
    targetKinds: /* @__PURE__ */ new Set(["mobkit/access"])
  },
  [CONSOLE_COMMAND_NAMES2.previewAccess]: {
    method: CONSOLE_RPC_METHODS2.accessPreview,
    targetKinds: /* @__PURE__ */ new Set(["mobkit/access"])
  },
  [CONSOLE_COMMAND_NAMES2.listMemoryRecords]: {
    method: CONSOLE_RPC_METHODS2.memoryPanelRecords,
    targetKinds: /* @__PURE__ */ new Set(["mobkit/memory"])
  },
  [CONSOLE_COMMAND_NAMES2.getMemoryRecord]: {
    method: CONSOLE_RPC_METHODS2.memoryPanelRecord,
    targetKinds: /* @__PURE__ */ new Set(["mobkit/memory"])
  },
  [CONSOLE_COMMAND_NAMES2.listMemoryQuarantine]: {
    method: CONSOLE_RPC_METHODS2.memoryPanelQuarantine,
    targetKinds: /* @__PURE__ */ new Set(["mobkit/memory"])
  },
  [CONSOLE_COMMAND_NAMES2.listMemoryDreams]: {
    method: CONSOLE_RPC_METHODS2.memoryPanelDreams,
    targetKinds: /* @__PURE__ */ new Set(["mobkit/memory"])
  },
  [CONSOLE_COMMAND_NAMES2.getMemoryOverview]: {
    method: CONSOLE_RPC_METHODS2.memoryPanelOverview,
    targetKinds: /* @__PURE__ */ new Set(["mobkit/memory"])
  },
  [CONSOLE_COMMAND_NAMES2.listMemoryProposals]: {
    method: CONSOLE_RPC_METHODS2.memoryPanelProposals,
    targetKinds: /* @__PURE__ */ new Set(["mobkit/memory"])
  },
  [CONSOLE_COMMAND_NAMES2.listMemoryInjections]: {
    method: CONSOLE_RPC_METHODS2.memoryPanelInjections,
    targetKinds: /* @__PURE__ */ new Set(["mobkit/memory"])
  },
  [CONSOLE_COMMAND_NAMES2.listMemoryHarvests]: {
    method: CONSOLE_RPC_METHODS2.memoryPanelHarvests,
    targetKinds: /* @__PURE__ */ new Set(["mobkit/memory"])
  },
  [CONSOLE_COMMAND_NAMES2.listMemoryDreamRuns]: {
    method: CONSOLE_RPC_METHODS2.memoryPanelDreamRuns,
    targetKinds: /* @__PURE__ */ new Set(["mobkit/memory"])
  },
  [CONSOLE_COMMAND_NAMES2.listMemoryAuditVerdicts]: {
    method: CONSOLE_RPC_METHODS2.memoryPanelAuditVerdicts,
    targetKinds: /* @__PURE__ */ new Set(["mobkit/memory"])
  },
  [CONSOLE_COMMAND_NAMES2.workgraphSnapshot]: {
    method: CONSOLE_RPC_METHODS2.workgraphSnapshot,
    targetKinds: /* @__PURE__ */ new Set(["mobkit/workgraph"])
  },
  [CONSOLE_COMMAND_NAMES2.workgraphEvents]: {
    method: CONSOLE_RPC_METHODS2.workgraphEvents,
    targetKinds: /* @__PURE__ */ new Set(["mobkit/workgraph"])
  },
  [CONSOLE_COMMAND_NAMES2.workgraphGet]: {
    method: CONSOLE_RPC_METHODS2.workgraphGet,
    targetKinds: /* @__PURE__ */ new Set(["mobkit/workgraph"])
  },
  [CONSOLE_COMMAND_NAMES2.workgraphGoalStatus]: {
    method: CONSOLE_RPC_METHODS2.workgraphGoalStatus,
    targetKinds: /* @__PURE__ */ new Set(["mobkit/workgraph"])
  },
  [CONSOLE_COMMAND_NAMES2.workgraphClaim]: {
    method: CONSOLE_RPC_METHODS2.workgraphClaim,
    targetKinds: /* @__PURE__ */ new Set(["mobkit/workgraph"])
  },
  [CONSOLE_COMMAND_NAMES2.workgraphRelease]: {
    method: CONSOLE_RPC_METHODS2.workgraphRelease,
    targetKinds: /* @__PURE__ */ new Set(["mobkit/workgraph"])
  },
  [CONSOLE_COMMAND_NAMES2.workgraphClose]: {
    method: CONSOLE_RPC_METHODS2.workgraphClose,
    targetKinds: /* @__PURE__ */ new Set(["mobkit/workgraph"])
  },
  [CONSOLE_COMMAND_NAMES2.workgraphGoalConfirm]: {
    method: CONSOLE_RPC_METHODS2.workgraphGoalConfirm,
    targetKinds: /* @__PURE__ */ new Set(["mobkit/workgraph"])
  },
  [CONSOLE_COMMAND_NAMES2.workgraphGoalRequestClose]: {
    method: CONSOLE_RPC_METHODS2.workgraphGoalRequestClose,
    targetKinds: /* @__PURE__ */ new Set(["mobkit/workgraph"])
  },
  [CONSOLE_COMMAND_NAMES2.workgraphAttentionPause]: {
    method: CONSOLE_RPC_METHODS2.workgraphAttentionPause,
    targetKinds: /* @__PURE__ */ new Set(["mobkit/workgraph"])
  },
  [CONSOLE_COMMAND_NAMES2.workgraphAttentionResume]: {
    method: CONSOLE_RPC_METHODS2.workgraphAttentionResume,
    targetKinds: /* @__PURE__ */ new Set(["mobkit/workgraph"])
  },
  [CONSOLE_COMMAND_NAMES2.workgraphAttentionReassign]: {
    method: CONSOLE_RPC_METHODS2.workgraphAttentionReassign,
    targetKinds: /* @__PURE__ */ new Set(["mobkit/workgraph"])
  }
};
function consoleCommandMethod(command) {
  return CONSOLE_COMMAND_SPECS2[command].method;
}
function createHttpConsoleTransport2({
  baseUrl,
  fetchTimeoutMs
}) {
  const timeout = () => typeof fetchTimeoutMs === "function" ? fetchTimeoutMs() : fetchTimeoutMs;
  return {
    loadExperience: () => fetchJson2(baseUrl, CONSOLE_REST_PATHS2.experience, timeout()),
    loadModules: () => fetchJson2(baseUrl, CONSOLE_REST_PATHS2.modules, timeout()),
    capabilities: async () => normalizeCapabilities(
      await callConsoleRpc2(baseUrl, CONSOLE_RPC_METHODS2.capabilities, {}, timeout())
    ),
    queryTimeline: (input) => queryTimeline2(baseUrl, input, input.limit, timeout()),
    subscribeTimeline: (input, onFrame) => subscribeTimelineEvents2(baseUrl, input, onFrame),
    send: (input) => {
      const handlingMode = input.handlingMode ?? "queue";
      if (input.attachments?.length) {
        return sendConsoleMultipart2(
          baseUrl,
          input.identity,
          input.content,
          input.attachments,
          input.origin,
          input.idempotencyKey,
          handlingMode,
          timeout()
        );
      }
      return sendConsole2(
        baseUrl,
        input.identity,
        input.content,
        input.origin,
        input.idempotencyKey,
        handlingMode,
        timeout()
      );
    },
    executeCommand: async (input) => {
      const spec = commandSpec(input.command);
      const params = { ...input.params || {} };
      if (identityCommandMethods2.has(spec.method)) {
        const identity = stringValue2(params.identity) || identityForCommandTarget(input.target);
        if (!identity) {
          throw new Error(`${input.command} requires an identity-addressed target`);
        }
        params.identity = identity;
      }
      let result;
      try {
        result = await callConsoleRpc2(baseUrl, spec.method, params, timeout());
      } catch (error) {
        if (spec.method !== CONSOLE_RPC_METHODS2.inspectIdentity || !isJsonRpcMethodNotFoundError(error)) {
          throw error;
        }
        result = await callConsoleRpc2(baseUrl, LEGACY_INSPECT_IDENTITY_METHOD, params, timeout());
      }
      return {
        command: input.command,
        accepted: true,
        result
      };
    },
    upload: (input) => uploadConsoleBlobMultipart2(baseUrl, input, timeout()),
    blobUrl: (blobId) => `${baseUrl}${CONSOLE_BLOB_PATH_PREFIX2}${encodeURIComponent(blobId)}`
  };
}
function createMobKitConsoleController2({
  transport
}) {
  const facts = createFactFactory();
  return {
    transport,
    facts,
    timeline: createTimelineController(transport, facts),
    commands: createConsoleCommandSurface(transport, facts)
  };
}
function isJsonRpcMethodNotFoundError(error) {
  const rpcError = error?.rpcError;
  return rpcError?.code === -32601;
}
function createConsoleCommandSurface(transport, facts) {
  let cachedCapabilities = null;
  let capabilitiesRequest = null;
  const capabilities = async (force = false) => {
    if (force || !cachedCapabilities) {
      if (!capabilitiesRequest) {
        capabilitiesRequest = transport.capabilities().finally(() => {
          capabilitiesRequest = null;
        });
      }
      cachedCapabilities = await capabilitiesRequest;
    }
    return cachedCapabilities;
  };
  const requireFreshCapability = async (method) => {
    let currentCapabilities = await capabilities(true);
    if (!hasCapability(currentCapabilities, method)) {
      currentCapabilities = await capabilities(true);
    }
    requireCapability(currentCapabilities, method);
    return currentCapabilities;
  };
  return {
    async sendMessage(target, input) {
      const identity = identityForSendTarget(target);
      if (!identity) {
        throw new Error(`target ${target.kind} cannot send MobKit console messages`);
      }
      const currentCapabilities = await requireFreshCapability(CONSOLE_RPC_METHODS2.send);
      const optimistic = facts.optimistic({
        idempotencyKey: input.idempotencyKey,
        targetId: target.id
      }, input.idempotencyKey);
      const accepted = await transport.send({
        ...input,
        identity
      });
      return {
        optimistic,
        accepted: facts.mobkit(accepted, {
          routeOrMethod: CONSOLE_RPC_METHODS2.send,
          capabilityVersion: currentCapabilities.version,
          correlationId: input.idempotencyKey,
          cursor: accepted.cursor
        })
      };
    },
    async uploadBlob(input) {
      const currentCapabilities = await requireFreshCapability(CONSOLE_RPC_METHODS2.blobUpload);
      if (!transport.upload) {
        throw new Error(`transport does not implement ${CONSOLE_RPC_METHODS2.blobUpload}`);
      }
      const uploaded = await transport.upload(input);
      return facts.mobkit(uploaded, {
        routeOrMethod: CONSOLE_RPC_METHODS2.blobUpload,
        capabilityVersion: currentCapabilities.version
      });
    },
    async execute(input) {
      if (!isMobKitTarget(input.target)) {
        throw new Error(`host target ${input.target.kind} cannot execute MobKit commands`);
      }
      const spec = commandSpec(input.command);
      if (!spec.targetKinds.has(input.target.kind)) {
        throw new Error(`target ${input.target.kind} cannot execute command ${input.command}`);
      }
      await requireFreshCapability(spec.method);
      if (!transport.executeCommand) {
        throw new Error(`transport does not implement command ${input.command}`);
      }
      return transport.executeCommand(input);
    }
  };
}
function createTimelineController(transport, facts) {
  return {
    async query(input) {
      const page = await transport.queryTimeline(input);
      return facts.mobkit(page, {
        routeOrMethod: CONSOLE_RPC_METHODS2.queryTimeline,
        cursor: page.latestCursor || page.nextCursor
      });
    },
    async subscribeWithBackfill(input, onFrame) {
      const delivered = createBoundedTimelineDedupSet(input.limit);
      const deliver = (frame) => {
        const key = timelineDedupKey(frame);
        if (key && !delivered.add(key)) return;
        onFrame(facts.mobkit(frame, {
          routeOrMethod: CONSOLE_REST_PATHS2.timelineStream,
          cursor: frame.cursor
        }));
      };
      const seed = await transport.queryTimeline({
        ...input,
        mode: "recent"
      });
      seed.frames.forEach(deliver);
      const after = seed.latestCursor || seed.nextCursor || input.after;
      const unsubscribe = transport.subscribeTimeline({ ...input, after }, (frame) => {
        if (frame.event === "replay_unavailable") {
          void transport.queryTimeline({ ...input, mode: "recent" }).then((page) => {
            page.frames.forEach(deliver);
          });
          return;
        }
        deliver(frame);
      });
      return unsubscribe;
    }
  };
}
function createBoundedTimelineDedupSet(limit) {
  const max = Math.max(MIN_TIMELINE_DEDUP_KEYS, (limit || 400) * 4);
  const keys = /* @__PURE__ */ new Set();
  const order = [];
  return {
    add(key) {
      if (keys.has(key)) {
        return false;
      }
      keys.add(key);
      order.push(key);
      while (order.length > max) {
        const oldest = order.shift();
        if (oldest) {
          keys.delete(oldest);
        }
      }
      return true;
    }
  };
}
function timelineDedupKey(frame) {
  const id = frame.id?.trim();
  if (id) return `id:${id}`;
  const cursor = frame.cursor?.trim();
  if (cursor) return `cursor:${cursor}`;
  const timestamp = frame.timestampMs;
  if (typeof timestamp === "number") {
    return `timestamp:${frame.event || ""}:${frame.identity || ""}:${timestamp}:${stableDedupText(frame.data)}`;
  }
  return null;
}
function stableDedupText(value) {
  try {
    return JSON.stringify(value, (_key, nested) => {
      if (!nested || typeof nested !== "object" || Array.isArray(nested)) {
        return nested;
      }
      return Object.fromEntries(
        Object.entries(nested).sort(([left], [right]) => left.localeCompare(right))
      );
    });
  } catch {
    return String(value);
  }
}
function createFactFactory() {
  const wrap = (source, value, meta = {}) => ({
    value,
    provenance: {
      source,
      timestampMs: Date.now(),
      ...meta
    }
  });
  return {
    mobkit: (value, meta) => wrap("mobkit-protocol", value, meta),
    derived: (value, meta) => wrap("controller-derived", value, meta),
    optimistic: (value, correlationId) => wrap("optimistic", value, { correlationId }),
    host: (value) => wrap("host-adapter", value)
  };
}
function normalizeCapabilities(value) {
  const record = value && typeof value === "object" ? value : {};
  const methods = Array.isArray(record.methods) ? Array.from(new Set(record.methods.filter((method) => typeof method === "string" && method.trim().length > 0))) : [];
  return {
    methods,
    version: typeof record.version === "string" ? record.version : void 0,
    ...typeof record.read_only === "boolean" ? { readOnly: record.read_only } : {},
    runtime_capabilities: record.runtime_capabilities,
    method_capabilities: record.method_capabilities
  };
}
function requireCapability(capabilities, method) {
  if (!hasCapability(capabilities, method)) {
    throw new Error(`MobKit capability missing for ${method}`);
  }
}
function hasCapability(capabilities, method) {
  return capabilities.methods.includes(method);
}
function commandSpec(command) {
  if (!isConsoleCommandName(command)) {
    throw new Error(`unknown MobKit console command ${String(command)}`);
  }
  return CONSOLE_COMMAND_SPECS2[command];
}
function isConsoleCommandName(command) {
  return typeof command === "string" && command in CONSOLE_COMMAND_SPECS2;
}
function identityForSendTarget(target) {
  return target.kind === "mobkit/identity-chat" ? target.identity : null;
}
var identityCommandMethods2 = /* @__PURE__ */ new Set([
  CONSOLE_RPC_METHODS2.inspectIdentity,
  CONSOLE_RPC_METHODS2.retireIdentity,
  CONSOLE_RPC_METHODS2.respawnIdentity,
  CONSOLE_RPC_METHODS2.resetIdentity
]);
function identityForCommandTarget(target) {
  if (target.kind === "mobkit/identity-chat" || target.kind === "mobkit/identity-inspect") {
    return target.identity;
  }
  return null;
}
function stringValue2(value) {
  return typeof value === "string" && value.trim() ? value.trim() : null;
}
function isMobKitTarget(target) {
  return target.kind.startsWith("mobkit/");
}

// src/lib/workgraph-actions.ts
var WORKGRAPH_CONFLICT_CODE = -32042;
function workGraphConflictRefreshRequest(params) {
  const itemId = typeof params.id === "string" && params.id.trim() ? params.id : null;
  if (itemId) {
    return { command: CONSOLE_COMMAND_NAMES2.workgraphGet, params: { id: itemId } };
  }
  const bindingId = typeof params.binding_id === "string" && params.binding_id.trim() ? params.binding_id : null;
  if (bindingId) {
    return { command: CONSOLE_COMMAND_NAMES2.workgraphGoalStatus, params: { binding_id: bindingId } };
  }
  return null;
}
function asRecord(value) {
  return value && typeof value === "object" && !Array.isArray(value) ? value : null;
}
function revisionOfItem(result) {
  const item = asRecord(asRecord(result)?.item);
  return typeof item?.revision === "number" ? item.revision : void 0;
}
async function resolveWorkGraphItemRevision(run, itemId) {
  const result = await run(CONSOLE_COMMAND_NAMES2.workgraphGet, { id: itemId });
  const revision = revisionOfItem(result);
  if (revision === void 0) {
    throw new Error(`could not resolve the current revision of work item ${itemId}`);
  }
  return revision;
}
async function resolveWorkGraphGoalItemRevision(run, bindingId) {
  const result = await run(CONSOLE_COMMAND_NAMES2.workgraphGoalStatus, { binding_id: bindingId });
  const revision = revisionOfItem(result);
  if (revision === void 0) {
    throw new Error(`could not resolve the goal item revision for binding ${bindingId}`);
  }
  return revision;
}
async function resolveWorkGraphBindingRevision(run, bindingId) {
  const result = await run(CONSOLE_COMMAND_NAMES2.workgraphGoalStatus, { binding_id: bindingId });
  const machineState = asRecord(asRecord(asRecord(result)?.attention)?.machine_state);
  const revision = typeof machineState?.revision === "number" ? machineState.revision : void 0;
  if (revision === void 0) {
    throw new Error(`could not resolve the machine revision of attention binding ${bindingId}`);
  }
  return revision;
}
function workGraphClaimOwnerId(subject, fallback) {
  const trimmed = typeof subject === "string" ? subject.trim() : "";
  return trimmed || fallback;
}

// src/lib/pane-resize.ts
function findPaneResizeRoot(handle) {
  const workbenchRoot = handle.closest("[data-console-workbench]");
  if (workbenchRoot instanceof HTMLElement) return workbenchRoot;
  const shellRoot = handle.closest(".shell");
  return shellRoot instanceof HTMLElement ? shellRoot : null;
}

// src/lib/read-only-override.ts
var READ_ONLY_QUERY_KEYS = [
  "console_read_only",
  "mobkit_console_read_only",
  "view_only"
];
function parseBooleanFlag(value) {
  if (typeof value === "boolean") return value;
  if (typeof value !== "string") return null;
  switch (value.trim().toLowerCase()) {
    case "1":
    case "true":
    case "yes":
    case "on":
      return true;
    case "0":
    case "false":
    case "no":
    case "off":
      return false;
    default:
      return null;
  }
}
function browserSearch() {
  if (typeof window === "undefined") return "";
  return window.location.search;
}
function browserHostOverride() {
  if (typeof window === "undefined") return void 0;
  return window.__MOBKIT_CONSOLE_READ_ONLY__;
}
function resolveConsoleReadOnlyOverride(input = {}) {
  const search = input.search ?? browserSearch();
  const params = new URLSearchParams(search.startsWith("?") ? search.slice(1) : search);
  const hostOverride = parseBooleanFlag(input.hostOverride ?? browserHostOverride());
  if (hostOverride === true) return true;
  for (const key of READ_ONLY_QUERY_KEYS) {
    const parsed = parseBooleanFlag(params.get(key));
    if (parsed === true) return true;
  }
  return false;
}

// src/icon.tsx
var import_jsx_runtime24 = require("react/jsx-runtime");
function SpriteSheet() {
  return /* @__PURE__ */ (0, import_jsx_runtime24.jsxs)("svg", { className: "sprite-root", width: "0", height: "0", style: { position: "absolute" }, "aria-hidden": "true", children: [
    /* @__PURE__ */ (0, import_jsx_runtime24.jsx)("symbol", { id: "i-plus", viewBox: "0 0 24 24", children: /* @__PURE__ */ (0, import_jsx_runtime24.jsx)("path", { d: "M12 5v14M5 12h14" }) }),
    /* @__PURE__ */ (0, import_jsx_runtime24.jsxs)("symbol", { id: "i-compose", viewBox: "0 0 24 24", children: [
      /* @__PURE__ */ (0, import_jsx_runtime24.jsx)("path", { d: "m4 20 4.5-1 9.5-9.5-3.5-3.5L5 15.5 4 20z" }),
      /* @__PURE__ */ (0, import_jsx_runtime24.jsx)("path", { d: "m13.5 4.5 3.5 3.5" }),
      /* @__PURE__ */ (0, import_jsx_runtime24.jsx)("path", { d: "M9 19h11" })
    ] }),
    /* @__PURE__ */ (0, import_jsx_runtime24.jsxs)("symbol", { id: "i-new-thread", viewBox: "0 0 24 24", children: [
      /* @__PURE__ */ (0, import_jsx_runtime24.jsx)("rect", { x: "4", y: "4", width: "16", height: "16", rx: "3" }),
      /* @__PURE__ */ (0, import_jsx_runtime24.jsx)("path", { d: "m9 15 5.5-5.5 2 2L11 17H9v-2z" }),
      /* @__PURE__ */ (0, import_jsx_runtime24.jsx)("path", { d: "m13 9 2 2" })
    ] }),
    /* @__PURE__ */ (0, import_jsx_runtime24.jsx)("symbol", { id: "i-bolt", viewBox: "0 0 24 24", children: /* @__PURE__ */ (0, import_jsx_runtime24.jsx)("path", { d: "M13 2 6 13h5l-1 9 8-12h-5z" }) }),
    /* @__PURE__ */ (0, import_jsx_runtime24.jsxs)("symbol", { id: "i-sliders", viewBox: "0 0 24 24", children: [
      /* @__PURE__ */ (0, import_jsx_runtime24.jsx)("path", { d: "M4 6h16M4 12h16M4 18h16" }),
      /* @__PURE__ */ (0, import_jsx_runtime24.jsx)("circle", { cx: "8", cy: "12", r: "2" })
    ] }),
    /* @__PURE__ */ (0, import_jsx_runtime24.jsx)("symbol", { id: "i-folder", viewBox: "0 0 24 24", children: /* @__PURE__ */ (0, import_jsx_runtime24.jsx)("path", { d: "M3 6h7l2 2h9v10a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V6z" }) }),
    /* @__PURE__ */ (0, import_jsx_runtime24.jsx)("symbol", { id: "i-play", viewBox: "0 0 24 24", children: /* @__PURE__ */ (0, import_jsx_runtime24.jsx)("path", { d: "m9 7 9 5-9 5z" }) }),
    /* @__PURE__ */ (0, import_jsx_runtime24.jsx)("symbol", { id: "i-stop", viewBox: "0 0 24 24", children: /* @__PURE__ */ (0, import_jsx_runtime24.jsx)("path", { d: "M8 8h8v8H8z" }) }),
    /* @__PURE__ */ (0, import_jsx_runtime24.jsx)("symbol", { id: "i-chevron", viewBox: "0 0 24 24", children: /* @__PURE__ */ (0, import_jsx_runtime24.jsx)("path", { d: "m7 10 5 5 5-5" }) }),
    /* @__PURE__ */ (0, import_jsx_runtime24.jsxs)("symbol", { id: "i-terminal", viewBox: "0 0 24 24", children: [
      /* @__PURE__ */ (0, import_jsx_runtime24.jsx)("path", { d: "m4 6 7 6-7 6" }),
      /* @__PURE__ */ (0, import_jsx_runtime24.jsx)("path", { d: "M13 18h7" })
    ] }),
    /* @__PURE__ */ (0, import_jsx_runtime24.jsxs)("symbol", { id: "i-team", viewBox: "0 0 24 24", children: [
      /* @__PURE__ */ (0, import_jsx_runtime24.jsx)("circle", { cx: "9", cy: "9", r: "3" }),
      /* @__PURE__ */ (0, import_jsx_runtime24.jsx)("circle", { cx: "17", cy: "10", r: "2.5" }),
      /* @__PURE__ */ (0, import_jsx_runtime24.jsx)("path", { d: "M4 19a5 5 0 0 1 10 0" }),
      /* @__PURE__ */ (0, import_jsx_runtime24.jsx)("path", { d: "M13.5 19a4 4 0 0 1 7 0" })
    ] }),
    /* @__PURE__ */ (0, import_jsx_runtime24.jsxs)("symbol", { id: "i-branch", viewBox: "0 0 24 24", children: [
      /* @__PURE__ */ (0, import_jsx_runtime24.jsx)("path", { d: "M6 3v6a4 4 0 0 0 4 4h8" }),
      /* @__PURE__ */ (0, import_jsx_runtime24.jsx)("path", { d: "M14 7h4v4" }),
      /* @__PURE__ */ (0, import_jsx_runtime24.jsx)("circle", { cx: "6", cy: "3", r: "2" }),
      /* @__PURE__ */ (0, import_jsx_runtime24.jsx)("circle", { cx: "6", cy: "15", r: "2" }),
      /* @__PURE__ */ (0, import_jsx_runtime24.jsx)("circle", { cx: "18", cy: "13", r: "2" })
    ] }),
    /* @__PURE__ */ (0, import_jsx_runtime24.jsx)("symbol", { id: "i-shield", viewBox: "0 0 24 24", children: /* @__PURE__ */ (0, import_jsx_runtime24.jsx)("path", { d: "M12 3 4 6v6c0 5 3.5 8 8 9 4.5-1 8-4 8-9V6z" }) }),
    /* @__PURE__ */ (0, import_jsx_runtime24.jsx)("symbol", { id: "i-dot", viewBox: "0 0 24 24", children: /* @__PURE__ */ (0, import_jsx_runtime24.jsx)("circle", { cx: "12", cy: "12", r: "4" }) }),
    /* @__PURE__ */ (0, import_jsx_runtime24.jsxs)("symbol", { id: "i-clock", viewBox: "0 0 24 24", children: [
      /* @__PURE__ */ (0, import_jsx_runtime24.jsx)("circle", { cx: "12", cy: "12", r: "9" }),
      /* @__PURE__ */ (0, import_jsx_runtime24.jsx)("path", { d: "M12 7v6l4 2" })
    ] }),
    /* @__PURE__ */ (0, import_jsx_runtime24.jsxs)("symbol", { id: "i-cube", viewBox: "0 0 24 24", children: [
      /* @__PURE__ */ (0, import_jsx_runtime24.jsx)("path", { d: "m12 3 8 4.5v9L12 21l-8-4.5v-9z" }),
      /* @__PURE__ */ (0, import_jsx_runtime24.jsx)("path", { d: "m12 12 8-4.5" }),
      /* @__PURE__ */ (0, import_jsx_runtime24.jsx)("path", { d: "m12 12-8-4.5" })
    ] }),
    /* @__PURE__ */ (0, import_jsx_runtime24.jsxs)("symbol", { id: "i-sidebar-toggle", viewBox: "0 0 24 24", children: [
      /* @__PURE__ */ (0, import_jsx_runtime24.jsx)("rect", { x: "3", y: "5", width: "18", height: "14", rx: "2" }),
      /* @__PURE__ */ (0, import_jsx_runtime24.jsx)("path", { d: "M9 5v14" }),
      /* @__PURE__ */ (0, import_jsx_runtime24.jsx)("path", { d: "m14 12 3-3" }),
      /* @__PURE__ */ (0, import_jsx_runtime24.jsx)("path", { d: "m14 12 3 3" })
    ] }),
    /* @__PURE__ */ (0, import_jsx_runtime24.jsxs)("symbol", { id: "i-open", viewBox: "0 0 24 24", children: [
      /* @__PURE__ */ (0, import_jsx_runtime24.jsx)("path", { d: "M4 12V6a2 2 0 0 1 2-2h12" }),
      /* @__PURE__ */ (0, import_jsx_runtime24.jsx)("path", { d: "M20 4v6h-6" }),
      /* @__PURE__ */ (0, import_jsx_runtime24.jsx)("path", { d: "m20 4-9 9" }),
      /* @__PURE__ */ (0, import_jsx_runtime24.jsx)("path", { d: "M20 14v4a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2v-4" })
    ] }),
    /* @__PURE__ */ (0, import_jsx_runtime24.jsxs)("symbol", { id: "i-swap", viewBox: "0 0 24 24", children: [
      /* @__PURE__ */ (0, import_jsx_runtime24.jsx)("path", { d: "M15 7h6" }),
      /* @__PURE__ */ (0, import_jsx_runtime24.jsx)("path", { d: "m18 4 3 3-3 3" }),
      /* @__PURE__ */ (0, import_jsx_runtime24.jsx)("path", { d: "M9 17H3" }),
      /* @__PURE__ */ (0, import_jsx_runtime24.jsx)("path", { d: "m6 14-3 3 3 3" }),
      /* @__PURE__ */ (0, import_jsx_runtime24.jsx)("path", { d: "M21 7H9a4 4 0 0 0-4 4v6" })
    ] }),
    /* @__PURE__ */ (0, import_jsx_runtime24.jsxs)("symbol", { id: "i-copy", viewBox: "0 0 24 24", children: [
      /* @__PURE__ */ (0, import_jsx_runtime24.jsx)("rect", { x: "9", y: "9", width: "11", height: "11", rx: "2" }),
      /* @__PURE__ */ (0, import_jsx_runtime24.jsx)("rect", { x: "4", y: "4", width: "11", height: "11", rx: "2" })
    ] }),
    /* @__PURE__ */ (0, import_jsx_runtime24.jsx)("symbol", { id: "i-check", viewBox: "0 0 24 24", children: /* @__PURE__ */ (0, import_jsx_runtime24.jsx)("path", { d: "m5 12 4.2 4.2L19 6.5" }) }),
    /* @__PURE__ */ (0, import_jsx_runtime24.jsxs)("symbol", { id: "i-archive", viewBox: "0 0 24 24", children: [
      /* @__PURE__ */ (0, import_jsx_runtime24.jsx)("path", { d: "M4 7h16" }),
      /* @__PURE__ */ (0, import_jsx_runtime24.jsx)("path", { d: "M6 7v11a2 2 0 0 0 2 2h8a2 2 0 0 0 2-2V7" }),
      /* @__PURE__ */ (0, import_jsx_runtime24.jsx)("path", { d: "M9 11h6" }),
      /* @__PURE__ */ (0, import_jsx_runtime24.jsx)("path", { d: "M10 3h4l1 2H9l1-2z" })
    ] }),
    /* @__PURE__ */ (0, import_jsx_runtime24.jsxs)("symbol", { id: "i-square-plus", viewBox: "0 0 24 24", children: [
      /* @__PURE__ */ (0, import_jsx_runtime24.jsx)("rect", { x: "3", y: "3", width: "18", height: "18", rx: "3" }),
      /* @__PURE__ */ (0, import_jsx_runtime24.jsx)("path", { d: "M12 8v8M8 12h8" })
    ] }),
    /* @__PURE__ */ (0, import_jsx_runtime24.jsxs)("symbol", { id: "i-info", viewBox: "0 0 24 24", children: [
      /* @__PURE__ */ (0, import_jsx_runtime24.jsx)("circle", { cx: "12", cy: "12", r: "9" }),
      /* @__PURE__ */ (0, import_jsx_runtime24.jsx)("path", { d: "M12 10v6" }),
      /* @__PURE__ */ (0, import_jsx_runtime24.jsx)("path", { d: "M12 7h.01" })
    ] }),
    /* @__PURE__ */ (0, import_jsx_runtime24.jsxs)("symbol", { id: "i-refresh", viewBox: "0 0 24 24", children: [
      /* @__PURE__ */ (0, import_jsx_runtime24.jsx)("path", { d: "M21 12a9 9 0 0 1-15.4 6.4" }),
      /* @__PURE__ */ (0, import_jsx_runtime24.jsx)("path", { d: "M3 12A9 9 0 0 1 18.4 5.6" }),
      /* @__PURE__ */ (0, import_jsx_runtime24.jsx)("path", { d: "M3 16v-4h4" }),
      /* @__PURE__ */ (0, import_jsx_runtime24.jsx)("path", { d: "M21 8v4h-4" })
    ] }),
    /* @__PURE__ */ (0, import_jsx_runtime24.jsxs)("symbol", { id: "i-mic", viewBox: "0 0 24 24", children: [
      /* @__PURE__ */ (0, import_jsx_runtime24.jsx)("path", { d: "M12 3a3 3 0 0 1 3 3v6a3 3 0 0 1-6 0V6a3 3 0 0 1 3-3z" }),
      /* @__PURE__ */ (0, import_jsx_runtime24.jsx)("path", { d: "M19 11a7 7 0 0 1-14 0" }),
      /* @__PURE__ */ (0, import_jsx_runtime24.jsx)("path", { d: "M12 18v3" }),
      /* @__PURE__ */ (0, import_jsx_runtime24.jsx)("path", { d: "M8 21h8" })
    ] }),
    /* @__PURE__ */ (0, import_jsx_runtime24.jsxs)("symbol", { id: "i-ellipsis", viewBox: "0 0 24 24", children: [
      /* @__PURE__ */ (0, import_jsx_runtime24.jsx)("circle", { cx: "5", cy: "12", r: "2" }),
      /* @__PURE__ */ (0, import_jsx_runtime24.jsx)("circle", { cx: "12", cy: "12", r: "2" }),
      /* @__PURE__ */ (0, import_jsx_runtime24.jsx)("circle", { cx: "19", cy: "12", r: "2" })
    ] }),
    /* @__PURE__ */ (0, import_jsx_runtime24.jsxs)("symbol", { id: "i-gear", viewBox: "0 0 24 24", children: [
      /* @__PURE__ */ (0, import_jsx_runtime24.jsx)("path", { d: "M12 8a4 4 0 1 1 0 8 4 4 0 0 1 0-8z" }),
      /* @__PURE__ */ (0, import_jsx_runtime24.jsx)("path", { d: "M12 2v3M12 19v3M2 12h3M19 12h3M4.9 4.9l2.2 2.2M16.9 16.9l2.2 2.2M19.1 4.9l-2.2 2.2M7.1 16.9l-2.2 2.2" })
    ] }),
    /* @__PURE__ */ (0, import_jsx_runtime24.jsxs)("symbol", { id: "i-search", viewBox: "0 0 24 24", children: [
      /* @__PURE__ */ (0, import_jsx_runtime24.jsx)("circle", { cx: "11", cy: "11", r: "6" }),
      /* @__PURE__ */ (0, import_jsx_runtime24.jsx)("path", { d: "m20 20-4.35-4.35" })
    ] }),
    /* @__PURE__ */ (0, import_jsx_runtime24.jsxs)("symbol", { id: "i-pin", viewBox: "0 0 24 24", children: [
      /* @__PURE__ */ (0, import_jsx_runtime24.jsx)("path", { d: "m14 4 6 6" }),
      /* @__PURE__ */ (0, import_jsx_runtime24.jsx)("path", { d: "M11 7l6 6" }),
      /* @__PURE__ */ (0, import_jsx_runtime24.jsx)("path", { d: "m8 10 6 6" }),
      /* @__PURE__ */ (0, import_jsx_runtime24.jsx)("path", { d: "M6 12l6 6" }),
      /* @__PURE__ */ (0, import_jsx_runtime24.jsx)("path", { d: "m11 13-7 7" })
    ] }),
    /* @__PURE__ */ (0, import_jsx_runtime24.jsx)("symbol", { id: "i-star", viewBox: "0 0 24 24", children: /* @__PURE__ */ (0, import_jsx_runtime24.jsx)("path", { d: "m12 3 2.8 5.7 6.2.9-4.5 4.4 1.1 6.2L12 17.2 6.4 20.2l1.1-6.2L3 9.6l6.2-.9L12 3z" }) })
  ] });
}
function Icon({ name, className }) {
  return /* @__PURE__ */ (0, import_jsx_runtime24.jsx)("svg", { className, "aria-label": name, children: /* @__PURE__ */ (0, import_jsx_runtime24.jsx)("use", { href: `#${name}` }) });
}

// src/panels/TopologyPanel.tsx
var import_react19 = __toESM(require("react"));

// src/panels/topology/RoleTree.tsx
var import_react17 = __toESM(require("react"));

// src/panels/topology/data.ts
var import_react16 = __toESM(require("react"));
var PEER_TOOL_NAMES2 = /* @__PURE__ */ new Set(["send_request", "send_message", "send_response"]);
function frameData(frame) {
  return frame.data && typeof frame.data === "object" ? frame.data : null;
}
function toolName(data) {
  if (!data) return "";
  if (typeof data.name === "string") return data.name;
  if (typeof data.tool_name === "string") return data.tool_name;
  return "";
}
function toolCallsFromFrameData(data) {
  if (!data) return [];
  const calls = [];
  const directName = toolName(data);
  if (directName) {
    calls.push({
      id: textFromUnknown3(data.id),
      name: directName,
      args: data.args && typeof data.args === "object" ? data.args : null
    });
  }
  const message = data.message && typeof data.message === "object" ? data.message : null;
  const blocks = Array.isArray(message?.blocks) ? message.blocks : [];
  for (const block of blocks) {
    if (!block || typeof block !== "object") continue;
    const blockRecord = block;
    if (textFromUnknown3(blockRecord.block_type) !== "tool_use") continue;
    const toolData = blockRecord.data && typeof blockRecord.data === "object" ? blockRecord.data : null;
    const name = toolName(toolData);
    if (!name) continue;
    calls.push({
      id: textFromUnknown3(toolData?.id),
      name,
      args: toolData?.args && typeof toolData.args === "object" ? toolData.args : null
    });
  }
  return calls;
}
function resultText(value) {
  if (typeof value === "string") return value;
  if (!value || typeof value !== "object") return "";
  try {
    return JSON.stringify(value);
  } catch {
    return "";
  }
}
function peerLastSegment2(value) {
  return value.split("/").filter(Boolean).pop() || value;
}
function textFromUnknown3(value) {
  return typeof value === "string" ? value.trim() : "";
}
function capturePeerRegistry(peerRegistry, rawResult) {
  const raw = resultText(rawResult);
  if (!raw) return;
  try {
    const parsed = JSON.parse(raw);
    if (!Array.isArray(parsed.peers)) return;
    for (const peer of parsed.peers) {
      if (typeof peer.peer_id === "string" && typeof peer.name === "string") {
        peerRegistry.set(peer.peer_id, peer.name);
      }
    }
  } catch {
  }
}
function resolvePeerTarget(args, peerRegistry, graph) {
  const candidates = [];
  const peerId = typeof args?.peer_id === "string" ? args.peer_id.trim() : "";
  const registryName = peerId ? peerRegistry.get(peerId) : "";
  if (registryName) candidates.push(registryName, peerLastSegment2(registryName));
  for (const key of ["identity", "target_identity", "recipient", "to", "display_name"]) {
    const value = typeof args?.[key] === "string" ? args[key].trim() : "";
    if (value) candidates.push(value, peerLastSegment2(value));
  }
  if (peerId) candidates.push(peerId);
  for (const candidate of candidates) {
    if (graph.byId.has(candidate)) return candidate;
    const match = graph.agents.find(
      (agent) => agent.id === candidate || agent.label === candidate || agent.memberId === candidate
    );
    if (match) return match.id;
  }
  return null;
}
function resolveGraphIdentity(value, graph) {
  const raw = value.trim();
  if (!raw) return null;
  const candidates = [raw, peerLastSegment2(raw)];
  for (const candidate of candidates) {
    if (graph.byId.has(candidate)) return candidate;
    const match = graph.agents.find(
      (agent) => agent.id === candidate || agent.label === candidate || agent.memberId === candidate
    );
    if (match) return match.id;
  }
  return null;
}
function commsBlocksFromFrameData(data) {
  const candidates = [];
  if (data) {
    candidates.push(data);
    if (data.message && typeof data.message === "object") candidates.push(data.message);
  }
  const blocks = [];
  for (const candidate of candidates) {
    if (!candidate || typeof candidate !== "object") continue;
    const record = candidate;
    const recordKind = textFromUnknown3(record.kind);
    if (recordKind === "comms") blocks.push(record);
    if (!Array.isArray(record.blocks)) continue;
    for (const block of record.blocks) {
      if (!block || typeof block !== "object") continue;
      const blockRecord = block;
      if (textFromUnknown3(blockRecord.type) === "comms") blocks.push(blockRecord);
    }
  }
  return blocks;
}
function typedCommsPulseFromFrame(frame, data, graph) {
  if (frame.event !== "system_notice") return null;
  const receiver = frame.identity ? resolveGraphIdentity(frame.identity, graph) : null;
  if (!receiver) return null;
  const blocks = commsBlocksFromFrameData(data);
  for (const block of blocks) {
    const peer = block.peer && typeof block.peer === "object" ? block.peer : {};
    const peerIdentity = resolveGraphIdentity(
      textFromUnknown3(peer.display_name) || textFromUnknown3(peer.id),
      graph
    );
    if (!peerIdentity || peerIdentity === receiver) continue;
    const direction = textFromUnknown3(block.direction) || "incoming";
    const requestId = textFromUnknown3(block.request_id);
    if (direction === "outgoing") {
      return {
        id: requestId || `${frame.id || frame.timestampMs}-typed-comms`,
        from: receiver,
        to: peerIdentity,
        ts: frame.timestampMs || 0
      };
    }
    return {
      id: requestId || `${frame.id || frame.timestampMs}-typed-comms`,
      from: peerIdentity,
      to: receiver,
      ts: frame.timestampMs || 0
    };
  }
  return null;
}
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
function colourForRole2(role, roleIndex) {
  const idx = roleIndex[role] ?? 0;
  return ROLE_PALETTE[idx % ROLE_PALETTE.length];
}
function roleSortKey(role) {
  const idx = ROLE_ORDER_HINT.findIndex((hint) => role.toLowerCase().includes(hint));
  return idx === -1 ? ROLE_ORDER_HINT.length : idx;
}
function buildGraph2(nodes, agents) {
  const agentByIdentity = /* @__PURE__ */ new Map();
  for (const a of agents) {
    const candidates = [a.identity, a.member_id, a.agent_id].filter(Boolean);
    for (const id of candidates) {
      if (!agentByIdentity.has(id)) agentByIdentity.set(id, a);
    }
  }
  const source = nodes.length > 0 ? nodes : agents.map((a) => ({
    identity: a.identity || a.member_id,
    label: a.label,
    role: a.role,
    state: a.state,
    wired_to: a.wired_to,
    labels: a.labels,
    group: a.group,
    subgroup: a.subgroup
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
      labels,
      memberId: registry?.member_id,
      responsePhase: registry?.response_phase
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
  const groups = Array.from(new Set(list.map((a) => a.group))).sort((a, b) => {
    const ca = list.filter((agent) => agent.group === a).length;
    const cb = list.filter((agent) => agent.group === b).length;
    if (ca !== cb) return cb - ca;
    return a.localeCompare(b);
  });
  return { agents: list, byId, edges, degree, roles, groups };
}
function roleIndexFor2(roles) {
  const idx = {};
  roles.forEach((r2, i) => {
    idx[r2] = i;
  });
  return idx;
}
function useTopologyActivity2(frames, graph, options = {}) {
  const life = options.life ?? 1500;
  const [now, setNow] = import_react16.default.useState(() => Date.now());
  const ticking = import_react16.default.useRef(false);
  import_react16.default.useEffect(() => {
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
  return import_react16.default.useMemo(() => {
    return deriveTopologyActivity(frames, graph, now, life);
  }, [frames, graph, life, now]);
}
function deriveTopologyActivity(frames, graph, now, life = 1500) {
  const active = {};
  const pulses = [];
  const peerRegistry = /* @__PURE__ */ new Map();
  const busy = {};
  const calls = {};
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
    const data = frameData(frame);
    const name = toolName(data);
    if (name === "peers" && (frame.event === "tool_execution_completed" || frame.event === "tool_result_received")) {
      capturePeerRegistry(peerRegistry, data?.result);
    }
    const typedCommsPulse = typedCommsPulseFromFrame(frame, data, graph);
    if (typedCommsPulse && typedCommsPulse.ts) {
      pulses.push(typedCommsPulse);
      calls[typedCommsPulse.from] = Math.max(calls[typedCommsPulse.from] || 0, typedCommsPulse.ts);
      calls[typedCommsPulse.to] = Math.max(calls[typedCommsPulse.to] || 0, typedCommsPulse.ts);
    }
    const shouldReadPeerToolCalls = frame.event === "tool_call_requested" || frame.event === "tool_call" || frame.event === "tool_execution_started" || frame.event === "interaction_complete";
    if (shouldReadPeerToolCalls && identity && graph.byId.has(identity)) {
      for (const call of toolCallsFromFrameData(data).filter((candidate) => PEER_TOOL_NAMES2.has(candidate.name))) {
        const args = call.args;
        const recipient = resolvePeerTarget(args, peerRegistry, graph);
        if (recipient && recipient !== identity) {
          pulses.push({
            id: call.id || `${frame.id || ts}-${pulses.length}`,
            from: identity,
            to: recipient,
            ts
          });
          calls[identity] = Math.max(calls[identity] || 0, ts);
          calls[recipient] = Math.max(calls[recipient] || 0, ts);
        }
      }
    }
  }
  const cutoff = now - life;
  for (const [k, v] of Object.entries(active)) {
    if (v < cutoff) delete active[k];
  }
  for (const [k, v] of Object.entries(calls)) {
    if (v < cutoff) delete calls[k];
  }
  return { active, pulses: pulses.filter((p) => p.ts >= cutoff), busy, calls };
}
function edgeKey2(a, b) {
  return a < b ? `${a}|${b}` : `${b}|${a}`;
}

// src/panels/topology/RoleTree.tsx
var import_jsx_runtime25 = require("react/jsx-runtime");
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
function RoleTree2({
  nodes,
  agents,
  activity
}) {
  const graph = import_react17.default.useMemo(() => buildGraph2(nodes, agents), [nodes, agents]);
  const roleIndex = import_react17.default.useMemo(() => roleIndexFor2(graph.roles), [graph.roles]);
  const live = useTopologyActivity2(activity, graph, { life: 1500 });
  const grouped = import_react17.default.useMemo(() => {
    var _a;
    const g = {};
    for (const r2 of graph.roles) g[r2] = [];
    for (const a of graph.agents) (g[_a = a.role] || (g[_a] = [])).push(a);
    return g;
  }, [graph]);
  const [expanded, setExpanded] = import_react17.default.useState(() => {
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
  return /* @__PURE__ */ (0, import_jsx_runtime25.jsxs)("div", { className: "topo-roletree", children: [
    /* @__PURE__ */ (0, import_jsx_runtime25.jsx)("div", { className: "topo-roletree__row", children: /* @__PURE__ */ (0, import_jsx_runtime25.jsxs)(
      "button",
      {
        type: "button",
        className: `topo-roletree__mob ${rootHot ? "is-hot" : ""}${rootBusy ? " is-busy" : ""}`,
        onClick: () => toggle("__root"),
        children: [
          /* @__PURE__ */ (0, import_jsx_runtime25.jsx)(
            "span",
            {
              className: "topo-roletree__chevron",
              style: { transform: expanded.__root ? "rotate(90deg)" : "rotate(0)" },
              children: "\u25B8"
            }
          ),
          /* @__PURE__ */ (0, import_jsx_runtime25.jsx)("span", { className: "topo-roletree__dot", style: { background: "var(--ok)" } }),
          /* @__PURE__ */ (0, import_jsx_runtime25.jsx)("span", { className: "topo-roletree__label", children: "mob" }),
          /* @__PURE__ */ (0, import_jsx_runtime25.jsxs)("span", { className: "topo-roletree__count", children: [
            graph.agents.length,
            " agents \xB7 ",
            graph.roles.length,
            " roles"
          ] }),
          rootBusy && /* @__PURE__ */ (0, import_jsx_runtime25.jsx)("span", { className: "topo-roletree__busy", "aria-label": "agents working" })
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
      const colour = colourForRole2(role, roleIndex);
      return /* @__PURE__ */ (0, import_jsx_runtime25.jsxs)("div", { className: "topo-roletree__section", children: [
        /* @__PURE__ */ (0, import_jsx_runtime25.jsxs)(
          "button",
          {
            type: "button",
            className: `topo-roletree__role ${sectionHot ? "is-hot" : ""}${sectionBusy ? " is-busy" : ""}`,
            onClick: () => toggle(role),
            children: [
              /* @__PURE__ */ (0, import_jsx_runtime25.jsx)(
                "span",
                {
                  className: "topo-roletree__chevron",
                  style: { transform: isOpen ? "rotate(90deg)" : "rotate(0)" },
                  children: "\u25B8"
                }
              ),
              /* @__PURE__ */ (0, import_jsx_runtime25.jsx)("span", { className: "topo-roletree__dot", style: { background: colour } }),
              /* @__PURE__ */ (0, import_jsx_runtime25.jsx)("span", { className: "topo-roletree__label", children: role }),
              /* @__PURE__ */ (0, import_jsx_runtime25.jsx)("span", { className: "topo-roletree__count", children: list.length }),
              sectionBusy && /* @__PURE__ */ (0, import_jsx_runtime25.jsx)("span", { className: "topo-roletree__busy", "aria-label": `${sectionBusyCount} working`, children: /* @__PURE__ */ (0, import_jsx_runtime25.jsx)("span", { className: "topo-roletree__busy-count", children: sectionBusyCount }) })
            ]
          }
        ),
        isOpen && /* @__PURE__ */ (0, import_jsx_runtime25.jsx)("div", { className: "topo-roletree__pod", children: list.map((agent) => {
          const isHot = !!live.active[agent.id];
          const isBusy = !!live.busy[agent.id];
          return /* @__PURE__ */ (0, import_jsx_runtime25.jsxs)(
            "div",
            {
              className: `topo-roletree__agent ${isHot ? "is-hot" : ""}${isBusy ? " is-busy" : ""}`,
              "data-testid": `topology-node:${agent.id}`,
              title: `${agent.id}${agent.state ? " \xB7 " + agent.state : ""}${isBusy ? " \xB7 working" : ""}`,
              children: [
                /* @__PURE__ */ (0, import_jsx_runtime25.jsx)(
                  "span",
                  {
                    className: "topo-roletree__agent-dot",
                    style: { background: stateColour(agent.state) }
                  }
                ),
                /* @__PURE__ */ (0, import_jsx_runtime25.jsx)("span", { className: "topo-roletree__agent-label", children: agent.label || agent.id }),
                isBusy && /* @__PURE__ */ (0, import_jsx_runtime25.jsx)("span", { className: "topo-roletree__busy", "aria-label": "working" })
              ]
            },
            agent.id
          );
        }) })
      ] }, role);
    })
  ] });
}

// src/panels/topology/DenseGraphMap.tsx
var import_react18 = __toESM(require("react"));
var import_jsx_runtime26 = require("react/jsx-runtime");
var GOLDEN_ANGLE2 = Math.PI * (3 - Math.sqrt(5));
var EMPTY_ACTIVITY = { active: {}, busy: {}, calls: {}, pulses: [] };
var ACTIVITY_LIFE_MS = 8e3;
var SMALL_GRAPH_NODE_LIMIT = 16;
var SMALL_GRAPH_EDGE_LIMIT = 80;
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
function groupPalette(index) {
  const colours = [
    "hsl(188 74% 66%)",
    "hsl(156 68% 62%)",
    "hsl(260 54% 72%)",
    "hsl(35 70% 69%)",
    "hsl(214 76% 66%)",
    "hsl(335 58% 70%)"
  ];
  return colours[index % colours.length];
}
function withAlpha(colour, alpha) {
  if (colour.startsWith("hsl(") && !colour.includes("/")) {
    return colour.replace(")", ` / ${alpha})`);
  }
  return colour;
}
function fitLayout(nodes, groups, width, height) {
  if (nodes.length === 0) return;
  let minX = Infinity;
  let minY = Infinity;
  let maxX = -Infinity;
  let maxY = -Infinity;
  for (const node of nodes) {
    const x = node.x || 0;
    const y = node.y || 0;
    const outer = Math.max(14, node.radius + 12);
    minX = Math.min(minX, x - outer);
    minY = Math.min(minY, y - outer - 30);
    maxX = Math.max(maxX, x + outer);
    maxY = Math.max(maxY, y + outer + 34);
  }
  const padX = Math.max(54, Math.min(width, height) * 0.085);
  const padY = Math.max(62, Math.min(width, height) * 0.115);
  const graphW = Math.max(1, maxX - minX);
  const graphH = Math.max(1, maxY - minY);
  const scale = Math.min(
    (width - padX * 2) / graphW,
    (height - padY * 2) / graphH,
    1
  );
  const cx = (minX + maxX) / 2;
  const cy = (minY + maxY) / 2;
  for (const node of nodes) {
    node.x = ((node.x || 0) - cx) * scale + width / 2;
    node.y = ((node.y || 0) - cy) * scale + height / 2;
  }
  const grouped = /* @__PURE__ */ new Map();
  for (const node of nodes) {
    const entry = grouped.get(node.groupIndex) || { x: 0, y: 0, count: 0 };
    entry.x += node.x || 0;
    entry.y += node.y || 0;
    entry.count += 1;
    grouped.set(node.groupIndex, entry);
  }
  for (let index = 0; index < groups.length; index += 1) {
    const entry = grouped.get(index);
    if (!entry || entry.count === 0) continue;
    groups[index].x = entry.x / entry.count;
    groups[index].y = entry.y / entry.count;
  }
}
function isSmallGraph(graph) {
  return graph.agents.length <= SMALL_GRAPH_NODE_LIMIT && graph.edges.length <= SMALL_GRAPH_EDGE_LIMIT;
}
function buildLayout(graph, width, height) {
  const groupIndex = /* @__PURE__ */ new Map();
  graph.groups.forEach((group, index) => groupIndex.set(group, index));
  const cx = width / 2;
  const cy = height / 2;
  const groupCount = Math.max(1, graph.groups.length);
  const smallGraph = isSmallGraph(graph);
  const marginX = smallGraph ? Math.max(82, width * 0.12) : Math.max(155, width * 0.23);
  const marginY = smallGraph ? Math.max(76, height * 0.13) : Math.max(150, height * 0.3);
  const compactR = Math.max(72, Math.min(width, height) * 0.18);
  let explicitAnchors = [];
  if (smallGraph && groupCount === 1) {
    explicitAnchors = [{ x: cx, y: cy }];
  } else if (smallGraph && groupCount === 2) {
    explicitAnchors = [{ x: cx - compactR, y: cy }, { x: cx + compactR, y: cy }];
  } else if (smallGraph && groupCount === 3) {
    explicitAnchors = [
      { x: cx, y: cy - compactR * 0.72 },
      { x: cx + compactR * 0.86, y: cy + compactR * 0.56 },
      { x: cx - compactR * 0.86, y: cy + compactR * 0.56 }
    ];
  } else if (smallGraph && groupCount === 4) {
    explicitAnchors = [
      { x: cx - compactR * 0.72, y: cy - compactR * 0.62 },
      { x: cx + compactR * 0.72, y: cy - compactR * 0.62 },
      { x: cx - compactR * 0.72, y: cy + compactR * 0.62 },
      { x: cx + compactR * 0.72, y: cy + compactR * 0.62 }
    ];
  } else if (groupCount === 1) {
    explicitAnchors = [{ x: cx, y: cy }];
  } else if (groupCount === 2) {
    explicitAnchors = [{ x: marginX, y: cy }, { x: width - marginX, y: cy }];
  } else if (groupCount === 3) {
    explicitAnchors = [
      { x: cx, y: marginY },
      { x: width - marginX, y: height - marginY },
      { x: marginX, y: height - marginY }
    ];
  } else if (groupCount === 4) {
    explicitAnchors = [
      { x: marginX, y: marginY },
      { x: width - marginX, y: marginY },
      { x: marginX, y: height - marginY },
      { x: width - marginX, y: height - marginY }
    ];
  }
  const rx = smallGraph ? compactR : Math.max(180, width * 0.34);
  const ry = smallGraph ? compactR * 0.78 : Math.max(130, height * 0.31);
  const groups = graph.groups.map((name, index) => {
    const fallbackT = index / groupCount * Math.PI * 2 - Math.PI / 2;
    const anchor = explicitAnchors[index] || {
      x: cx + Math.cos(fallbackT) * rx,
      y: cy + Math.sin(fallbackT) * ry
    };
    return {
      name,
      x: anchor.x,
      y: anchor.y,
      count: graph.agents.filter((a) => a.group === name).length,
      colour: groupPalette(index)
    };
  });
  const groupedAgents = /* @__PURE__ */ new Map();
  for (const agent of graph.agents) {
    const gi = groupIndex.get(agent.group) ?? 0;
    const entry = groupedAgents.get(gi) || [];
    entry.push(agent);
    groupedAgents.set(gi, entry);
  }
  for (const entry of groupedAgents.values()) {
    entry.sort((a, b) => {
      const da = graph.degree[a.id] || 0;
      const db = graph.degree[b.id] || 0;
      if (da !== db) return db - da;
      return a.label.localeCompare(b.label);
    });
  }
  const nodes = [];
  if (smallGraph) {
    const ordered = graph.groups.flatMap((groupName) => {
      const gi = groupIndex.get(groupName) ?? 0;
      return (groupedAgents.get(gi) || []).map((agent) => ({ agent, groupIndex: gi }));
    });
    const count = Math.max(1, ordered.length);
    const ringX = Math.max(86, Math.min(width * 0.34, Math.min(width, height) * 0.34));
    const ringY = Math.max(76, Math.min(height * 0.31, Math.min(width, height) * 0.3));
    ordered.forEach(({ agent, groupIndex: gi }, index) => {
      const theta = count === 1 ? -Math.PI / 2 : -Math.PI / 2 + index / count * Math.PI * 2;
      const degree = graph.degree[agent.id] || 0;
      const emphasis = degree > 1 ? 1.03 : 1;
      nodes.push({
        id: agent.id,
        agent,
        groupIndex: gi,
        radius: nodeRadius(graph, agent.id),
        x: count === 1 ? cx : cx + Math.cos(theta) * ringX * emphasis,
        y: count === 1 ? cy : cy + Math.sin(theta) * ringY * emphasis
      });
    });
    fitLayout(nodes, groups, width, height);
    const byId2 = new Map(nodes.map((node) => [node.id, node]));
    const edgeById2 = /* @__PURE__ */ new Map();
    for (const edge of graph.edges) {
      if (!byId2.has(edge.from) || !byId2.has(edge.to)) continue;
      const from = edgeById2.get(edge.from) || [];
      from.push(edge);
      edgeById2.set(edge.from, from);
      const to = edgeById2.get(edge.to) || [];
      to.push(edge);
      edgeById2.set(edge.to, to);
    }
    return { nodes, byId: byId2, edgeById: edgeById2, groups, width, height };
  }
  for (const [gi, entry] of groupedAgents.entries()) {
    const anchor = groups[gi] || { x: cx, y: cy, count: entry.length };
    const count = Math.max(1, entry.length);
    const clusterRadius = smallGraph ? Math.min(Math.max(22, Math.sqrt(count) * 15), Math.min(width, height) * 0.08) : Math.min(
      Math.max(74, Math.sqrt(count) * 11.8),
      Math.min(width, height) * (groupCount <= 4 ? 0.175 : 0.13)
    );
    const twist = hash(anchor.name) % 1e3 / 1e3 * Math.PI * 2;
    entry.forEach((agent, index) => {
      const seed = hash(agent.id);
      const normalized = Math.sqrt((index + 0.45) / count);
      const theta = twist + index * GOLDEN_ANGLE2 + seed % 37 / 37 * 0.18;
      const radial = clusterRadius * normalized;
      const spiralBias = 0.86 + seed % 17 / 100;
      nodes.push({
        id: agent.id,
        agent,
        groupIndex: gi,
        radius: nodeRadius(graph, agent.id),
        x: anchor.x + Math.cos(theta) * radial * spiralBias,
        y: anchor.y + Math.sin(theta) * radial * (1.02 - seed % 11 / 120)
      });
    });
  }
  fitLayout(nodes, groups, width, height);
  const byId = new Map(nodes.map((node) => [node.id, node]));
  const edgeById = /* @__PURE__ */ new Map();
  for (const edge of graph.edges) {
    if (!byId.has(edge.from) || !byId.has(edge.to)) continue;
    const from = edgeById.get(edge.from) || [];
    from.push(edge);
    edgeById.set(edge.from, from);
    const to = edgeById.get(edge.to) || [];
    to.push(edge);
    edgeById.set(edge.to, to);
  }
  return { nodes, byId, edgeById, groups, width, height };
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
function drawCurve(ctx, a, b, bend) {
  const ax = a.x || 0;
  const ay = a.y || 0;
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
function curvePoint(a, b, bend, t) {
  const ax = a.x || 0;
  const ay = a.y || 0;
  const bx = b.x || 0;
  const by = b.y || 0;
  const mx = (ax + bx) / 2;
  const my = (ay + by) / 2;
  const dx = bx - ax;
  const dy = by - ay;
  const len = Math.hypot(dx, dy) || 1;
  const cx = mx + -dy / len * bend;
  const cy = my + dx / len * bend;
  const mt = 1 - t;
  return {
    x: mt * mt * ax + 2 * mt * t * cx + t * t * bx,
    y: mt * mt * ay + 2 * mt * t * cy + t * t * by
  };
}
function curveTangent(a, b, bend, t) {
  const ax = a.x || 0;
  const ay = a.y || 0;
  const bx = b.x || 0;
  const by = b.y || 0;
  const mx = (ax + bx) / 2;
  const my = (ay + by) / 2;
  const dx = bx - ax;
  const dy = by - ay;
  const len = Math.hypot(dx, dy) || 1;
  const cx = mx + -dy / len * bend;
  const cy = my + dx / len * bend;
  return {
    x: 2 * (1 - t) * (cx - ax) + 2 * t * (bx - cx),
    y: 2 * (1 - t) * (cy - ay) + 2 * t * (by - cy)
  };
}
function pulseBend(a, b) {
  const seed = hash(edgeKey2(a.id, b.id));
  if (a.groupIndex === b.groupIndex) return seed % 17 - 8;
  return (seed % 2 === 0 ? 1 : -1) * (22 + seed % 34);
}
function drawArrowHead(ctx, point, tangent, size) {
  const angle = Math.atan2(tangent.y, tangent.x);
  ctx.moveTo(point.x, point.y);
  ctx.lineTo(
    point.x - Math.cos(angle - Math.PI / 6) * size,
    point.y - Math.sin(angle - Math.PI / 6) * size
  );
  ctx.moveTo(point.x, point.y);
  ctx.lineTo(
    point.x - Math.cos(angle + Math.PI / 6) * size,
    point.y - Math.sin(angle + Math.PI / 6) * size
  );
}
function drawBundledCurve(ctx, a, b, groups) {
  if (a.groupIndex === b.groupIndex) {
    const seed = hash(edgeKey2(a.id, b.id));
    drawCurve(ctx, a, b, seed % 15 - 7);
    return;
  }
  const ax = a.x || 0;
  const ay = a.y || 0;
  const bx = b.x || 0;
  const by = b.y || 0;
  const ga = groups[a.groupIndex];
  const gb = groups[b.groupIndex];
  const c1x = ga ? ga.x + (gb.x - ga.x) * 0.36 : (ax + bx) / 2;
  const c1y = ga ? ga.y + (gb.y - ga.y) * 0.36 : (ay + by) / 2;
  const c2x = gb ? gb.x + (ga.x - gb.x) * 0.36 : (ax + bx) / 2;
  const c2y = gb ? gb.y + (ga.y - gb.y) * 0.36 : (ay + by) / 2;
  ctx.moveTo(ax, ay);
  ctx.bezierCurveTo(c1x, c1y, c2x, c2y, bx, by);
}
function nodeRadius(graph, id) {
  if (isSmallGraph(graph)) {
    return Math.min(14, 5.6 + Math.sqrt(graph.degree[id] || 0) * 1.55);
  }
  return Math.min(8.5, 2.1 + Math.sqrt(graph.degree[id] || 0) * 0.48);
}
function DenseGraphMap2({
  graph,
  edgeMode = "all",
  activity = EMPTY_ACTIVITY
}) {
  const wrapRef = import_react18.default.useRef(null);
  const canvasRef = import_react18.default.useRef(null);
  const staticRef = import_react18.default.useRef(null);
  const dragRef = import_react18.default.useRef(null);
  const [size, setSize] = import_react18.default.useState({ width: 900, height: 420 });
  const [viewport, setViewport] = import_react18.default.useState({ scale: 1, x: 0, y: 0 });
  const viewportRef = import_react18.default.useRef(viewport);
  const [hoverId, setHoverId] = import_react18.default.useState(null);
  const roleIndex = import_react18.default.useMemo(() => roleIndexFor2(graph.roles), [graph.roles]);
  const layoutFingerprint = import_react18.default.useMemo(
    () => [
      graph.agents.length,
      graph.groups.join("|"),
      graph.agents.map((a) => a.id).join("|")
    ].join("::"),
    [graph]
  );
  const drawFingerprint = import_react18.default.useMemo(
    () => `${layoutFingerprint}::edges=${graph.edges.length}::edgeMode=${edgeMode}`,
    [layoutFingerprint, graph.edges.length, edgeMode]
  );
  const layout = import_react18.default.useMemo(
    () => buildLayout(graph, size.width, size.height),
    // `graph` is rebuilt every console poll. The dense layout
    // should only rerun when graph shape changes, not when an equivalent
    // REST payload is normalized into fresh object identities.
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [layoutFingerprint, size.width, size.height]
  );
  import_react18.default.useEffect(() => {
    viewportRef.current = viewport;
  }, [viewport]);
  import_react18.default.useEffect(() => {
    dragRef.current = null;
    setHoverId(null);
    setViewport({ scale: 1, x: 0, y: 0 });
  }, [layoutFingerprint, size.width, size.height]);
  import_react18.default.useEffect(() => {
    const el = wrapRef.current;
    if (!el) return;
    const ro = new ResizeObserver((entries) => {
      const rect = entries[0]?.contentRect;
      if (!rect) return;
      setSize({
        width: Math.max(1, Math.floor(rect.width)),
        height: Math.max(1, Math.floor(rect.height))
      });
    });
    ro.observe(el);
    return () => ro.disconnect();
  }, []);
  const drawStatic = import_react18.default.useCallback(() => {
    const host = wrapRef.current;
    if (!host) return null;
    const dpr = Math.max(1, Math.min(2, window.devicePixelRatio || 1));
    const off = document.createElement("canvas");
    off.width = Math.floor(layout.width * dpr);
    off.height = Math.floor(layout.height * dpr);
    const ctx = off.getContext("2d");
    if (!ctx) return null;
    ctx.scale(dpr, dpr);
    ctx.clearRect(0, 0, layout.width, layout.height);
    const faint = cssVar(host, "--ink-faint", "rgba(148, 163, 184, 1)");
    const smallGraph = isSmallGraph(graph);
    const edgeAlpha = smallGraph ? 0.72 : graph.edges.length > 18e3 ? 0.105 : graph.edges.length > 6e3 ? 0.135 : 0.18;
    for (const group of layout.groups) {
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
    if (smallGraph) {
      ctx.lineCap = "round";
      ctx.lineWidth = 1.35;
      for (const edge of graph.edges) {
        const a = layout.byId.get(edge.from);
        const b = layout.byId.get(edge.to);
        if (!a || !b) continue;
        const sameGroup = a.groupIndex === b.groupIndex;
        const seed = hash(edgeKey2(a.id, b.id));
        ctx.strokeStyle = sameGroup ? layout.groups[a.groupIndex]?.colour || faint : faint;
        ctx.globalAlpha = sameGroup ? edgeAlpha * 0.92 : edgeAlpha * 0.68;
        ctx.beginPath();
        drawCurve(ctx, a, b, sameGroup ? seed % 13 - 6 : (seed % 2 === 0 ? 1 : -1) * (8 + seed % 14));
        ctx.stroke();
      }
    } else if (edgeMode === "all") {
      ctx.lineWidth = graph.edges.length > 12e3 ? 0.48 : 0.68;
      ctx.strokeStyle = faint;
      ctx.globalAlpha = edgeAlpha * 0.92;
      ctx.beginPath();
      for (const edge of graph.edges) {
        const a = layout.byId.get(edge.from);
        const b = layout.byId.get(edge.to);
        if (!a || !b) continue;
        if (a.agent.group === b.agent.group) continue;
        drawBundledCurve(ctx, a, b, layout.groups);
      }
      ctx.stroke();
      ctx.globalAlpha = Math.min(0.42, edgeAlpha * 1.8);
      for (let gi = 0; gi < layout.groups.length; gi += 1) {
        ctx.strokeStyle = layout.groups[gi]?.colour || faint;
        ctx.beginPath();
        for (const edge of graph.edges) {
          const a = layout.byId.get(edge.from);
          const b = layout.byId.get(edge.to);
          if (!a || !b || a.groupIndex !== gi || b.groupIndex !== gi) continue;
          drawBundledCurve(ctx, a, b, layout.groups);
        }
        ctx.stroke();
      }
    }
    for (const node of layout.nodes) {
      const r2 = nodeRadius(graph, node.id);
      const x = node.x || 0;
      const y = node.y || 0;
      ctx.globalAlpha = 0.86;
      ctx.fillStyle = layout.groups[node.groupIndex]?.colour || colourForRole2(node.agent.role, roleIndex);
      ctx.beginPath();
      ctx.arc(x, y, r2, 0, Math.PI * 2);
      ctx.fill();
    }
    ctx.globalAlpha = 1;
    return off;
  }, [drawFingerprint, layout, roleIndex]);
  import_react18.default.useEffect(() => {
    staticRef.current = drawStatic();
  }, [drawStatic]);
  import_react18.default.useEffect(() => {
    const el = wrapRef.current;
    if (!el) return;
    const onWheel = (event) => {
      event.preventDefault();
      const canvas = canvasRef.current;
      if (!canvas) return;
      const current = viewportRef.current;
      const rect = canvas.getBoundingClientRect();
      const mx = event.clientX - rect.left;
      const my = event.clientY - rect.top;
      const nextScale = Math.max(0.55, Math.min(4, current.scale * (event.deltaY < 0 ? 1.12 : 0.88)));
      const gx = (mx - current.x) / current.scale;
      const gy = (my - current.y) / current.scale;
      setViewport({
        scale: nextScale,
        x: mx - gx * nextScale,
        y: my - gy * nextScale
      });
    };
    el.addEventListener("wheel", onWheel, { passive: false });
    return () => el.removeEventListener("wheel", onWheel);
  }, []);
  const drawFrame = import_react18.default.useCallback(() => {
    const canvas = canvasRef.current;
    const host = wrapRef.current;
    const ctx = canvas?.getContext("2d");
    if (!canvas || !ctx || !host) return;
    const dpr = Math.max(1, Math.min(2, window.devicePixelRatio || 1));
    const targetW = Math.floor(layout.width * dpr);
    const targetH = Math.floor(layout.height * dpr);
    if (canvas.width !== targetW || canvas.height !== targetH) {
      canvas.width = targetW;
      canvas.height = targetH;
    }
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, layout.width, layout.height);
    const staticCanvas = staticRef.current || drawStatic();
    ctx.save();
    applyViewport(ctx, viewport);
    if (staticCanvas) ctx.drawImage(staticCanvas, 0, 0, layout.width, layout.height);
    const focus = cssVar(host, "--focus", "rgb(90, 160, 255)");
    const ink = cssVar(host, "--ink", "rgb(235, 238, 245)");
    const selected = layout.byId.get(hoverId || "");
    const ok = cssVar(host, "--ok", "rgb(72, 200, 150)");
    const warn = cssVar(host, "--warn", "rgb(245, 178, 76)");
    const now = Date.now();
    if (activity.pulses.length > 0) {
      ctx.lineCap = "round";
      for (const pulse of activity.pulses) {
        const a = layout.byId.get(pulse.from);
        const b = layout.byId.get(pulse.to);
        if (!a || !b) continue;
        const age = Math.max(0, Math.min(1, (now - pulse.ts) / ACTIVITY_LIFE_MS));
        const alpha = Math.max(0, 1 - age);
        const bend = pulseBend(a, b);
        const t = 0.1 + age * 0.78;
        const bead = curvePoint(a, b, bend, t);
        const tangent = curveTangent(a, b, bend, t);
        ctx.globalAlpha = 0.24 + alpha * 0.5;
        ctx.strokeStyle = warn;
        ctx.lineWidth = Math.max(1.1, 2.2 / Math.sqrt(viewport.scale));
        ctx.beginPath();
        drawCurve(ctx, a, b, bend);
        ctx.stroke();
        ctx.globalAlpha = 0.75 + alpha * 0.25;
        ctx.fillStyle = warn;
        ctx.shadowColor = warn;
        ctx.shadowBlur = 14 / Math.sqrt(viewport.scale);
        ctx.beginPath();
        ctx.arc(bead.x, bead.y, Math.max(3.2, 4.8 / Math.sqrt(viewport.scale)), 0, Math.PI * 2);
        ctx.fill();
        ctx.shadowBlur = 0;
        ctx.globalAlpha = 0.72 + alpha * 0.22;
        ctx.beginPath();
        drawArrowHead(ctx, bead, tangent, Math.max(5, 7 / Math.sqrt(viewport.scale)));
        ctx.stroke();
      }
    }
    for (const node of layout.nodes) {
      const activeTs = activity.active[node.id] || activity.calls[node.id] || 0;
      const isBusy = activity.busy[node.id] || node.agent.responsePhase != null;
      if (!activeTs && !isBusy) continue;
      const age = activeTs ? Math.max(0, Math.min(1, (now - activeTs) / ACTIVITY_LIFE_MS)) : 0;
      const alpha = activeTs ? Math.max(0.15, 1 - age) : 0.34;
      const x = node.x || 0;
      const y = node.y || 0;
      const r2 = nodeRadius(graph, node.id);
      if (activeTs) {
        ctx.globalAlpha = 0.22 * alpha;
        ctx.fillStyle = activity.calls[node.id] ? warn : ok;
        ctx.beginPath();
        ctx.arc(x, y, r2 + 9 + (1 - alpha) * 8, 0, Math.PI * 2);
        ctx.fill();
      }
      if (isBusy) {
        ctx.globalAlpha = 0.96;
        ctx.strokeStyle = warn;
        ctx.lineWidth = Math.max(1.4, 2.2 / Math.sqrt(viewport.scale));
        ctx.setLineDash([Math.max(3, 4 / viewport.scale), Math.max(3, 5 / viewport.scale)]);
        ctx.beginPath();
        ctx.arc(x, y, r2 + 5.8, 0, Math.PI * 2);
        ctx.stroke();
        ctx.setLineDash([]);
        ctx.globalAlpha = 0.7;
        ctx.strokeStyle = ok;
        ctx.lineWidth = Math.max(0.8, 1.1 / Math.sqrt(viewport.scale));
        ctx.beginPath();
        ctx.arc(x, y, r2 + 9.2, 0, Math.PI * 2);
        ctx.stroke();
      }
    }
    if (selected) {
      const selectedEdges = layout.edgeById.get(selected.id) || [];
      const peerSet = /* @__PURE__ */ new Set();
      for (const edge of selectedEdges) peerSet.add(edge.from === selected.id ? edge.to : edge.from);
      ctx.globalAlpha = edgeMode === "all" ? 0.52 : 0.78;
      ctx.lineWidth = Math.max(0.72, 1.08 / Math.sqrt(viewport.scale));
      ctx.strokeStyle = focus;
      ctx.beginPath();
      for (const edge of selectedEdges) {
        const peerId = edge.from === selected.id ? edge.to : edge.from;
        const peer = layout.byId.get(peerId);
        if (!peer) continue;
        const seed = hash(edgeKey2(selected.id, peer.id));
        const bend = selected.groupIndex === peer.groupIndex ? seed % 15 - 7 : (seed % 2 === 0 ? 1 : -1) * (18 + seed % 26);
        drawCurve(ctx, selected, peer, bend);
      }
      ctx.stroke();
      for (const peerId of peerSet) {
        const peer = layout.byId.get(peerId);
        if (!peer) continue;
        ctx.globalAlpha = 0.84;
        ctx.fillStyle = layout.groups[peer.groupIndex]?.colour || focus;
        ctx.strokeStyle = focus;
        ctx.lineWidth = Math.max(0.8, 1.1 / Math.sqrt(viewport.scale));
        ctx.beginPath();
        ctx.arc(peer.x || 0, peer.y || 0, Math.max(2.8, nodeRadius(graph, peer.id) + 0.9), 0, Math.PI * 2);
        ctx.fill();
        ctx.stroke();
      }
    }
    if (selected) {
      const r2 = nodeRadius(graph, selected.id) + 6.2 / Math.sqrt(viewport.scale);
      ctx.globalAlpha = 1;
      ctx.shadowColor = focus;
      ctx.shadowBlur = 18 / Math.sqrt(viewport.scale);
      ctx.fillStyle = layout.groups[selected.groupIndex]?.colour || ink;
      ctx.strokeStyle = focus;
      ctx.lineWidth = 3.1 / Math.sqrt(viewport.scale);
      ctx.beginPath();
      ctx.arc(selected.x || 0, selected.y || 0, r2, 0, Math.PI * 2);
      ctx.fill();
      ctx.stroke();
      ctx.shadowBlur = 0;
    }
    ctx.restore();
  }, [activity, drawStatic, drawFingerprint, hoverId, layout, viewport]);
  import_react18.default.useEffect(() => {
    drawFrame();
  }, [drawFrame]);
  const nearestId = import_react18.default.useCallback((clientX, clientY) => {
    const canvas = canvasRef.current;
    if (!canvas) return null;
    const pos = screenToGraph(clientX, clientY, canvas, viewport);
    let best = null;
    const threshold = Math.max(12, 18 / viewport.scale);
    for (const node of layout.nodes) {
      const dx = (node.x || 0) - pos.x;
      const dy = (node.y || 0) - pos.y;
      const d2 = dx * dx + dy * dy;
      if (d2 > threshold * threshold) continue;
      if (!best || d2 < best.d2) best = { id: node.id, d2 };
    }
    return best?.id || null;
  }, [layout, viewport]);
  const hover = hoverId ? graph.byId.get(hoverId) : null;
  const hoverBusy = hoverId ? activity.busy[hoverId] || layout.byId.get(hoverId)?.agent.responsePhase != null : false;
  const hoverCalls = hoverId ? activity.pulses.filter((pulse) => pulse.from === hoverId || pulse.to === hoverId).length : 0;
  const showNodeLabels = graph.agents.length <= 12;
  const showGroupLabels = !showNodeLabels;
  const labelCenter = { x: layout.width / 2, y: layout.height / 2 };
  return /* @__PURE__ */ (0, import_jsx_runtime26.jsxs)(
    "div",
    {
      ref: wrapRef,
      className: "topo-dense",
      "data-testid": "topology-dense-map",
      onPointerDown: (event) => {
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
        setHoverId(nearestId(event.clientX, event.clientY));
        event.currentTarget.releasePointerCapture(event.pointerId);
      },
      onPointerCancel: (event) => {
        dragRef.current = null;
        event.currentTarget.releasePointerCapture(event.pointerId);
      },
      onPointerLeave: () => setHoverId(null),
      children: [
        /* @__PURE__ */ (0, import_jsx_runtime26.jsx)("canvas", { ref: canvasRef, className: "topo-dense__canvas", "aria-label": "Dense topology graph" }),
        /* @__PURE__ */ (0, import_jsx_runtime26.jsxs)("div", { className: "topo-dense__labels", "aria-hidden": "true", children: [
          showGroupLabels && layout.groups.map((g) => /* @__PURE__ */ (0, import_jsx_runtime26.jsxs)(
            "div",
            {
              className: "topo-dense__group-label",
              style: {
                left: `${g.x * viewport.scale + viewport.x}px`,
                top: `${g.y * viewport.scale + viewport.y + 18}px`,
                borderColor: g.colour
              },
              children: [
                /* @__PURE__ */ (0, import_jsx_runtime26.jsx)("strong", { children: g.name }),
                /* @__PURE__ */ (0, import_jsx_runtime26.jsxs)("span", { children: [
                  g.count,
                  " agents"
                ] })
              ]
            },
            g.name
          )),
          showNodeLabels && layout.nodes.map((node) => {
            const x = node.x || 0;
            const y = node.y || 0;
            const angle = Math.atan2(y - labelCenter.y, x - labelCenter.x);
            const offset = node.radius + 34;
            return /* @__PURE__ */ (0, import_jsx_runtime26.jsx)(
              "div",
              {
                className: "topo-dense__node-label",
                style: {
                  left: `${(x + Math.cos(angle) * offset) * viewport.scale + viewport.x}px`,
                  top: `${(y + Math.sin(angle) * offset) * viewport.scale + viewport.y}px`,
                  borderColor: layout.groups[node.groupIndex]?.colour || colourForRole2(node.agent.role, roleIndex)
                },
                children: node.agent.label
              },
              node.id
            );
          })
        ] }),
        hover && /* @__PURE__ */ (0, import_jsx_runtime26.jsxs)(import_jsx_runtime26.Fragment, { children: [
          /* @__PURE__ */ (0, import_jsx_runtime26.jsxs)(
            "div",
            {
              className: "topo-dense__hover-label",
              style: {
                left: `${(layout.byId.get(hover.id)?.x || 0) * viewport.scale + viewport.x}px`,
                top: `${(layout.byId.get(hover.id)?.y || 0) * viewport.scale + viewport.y}px`
              },
              children: [
                /* @__PURE__ */ (0, import_jsx_runtime26.jsx)("strong", { children: hover.label }),
                /* @__PURE__ */ (0, import_jsx_runtime26.jsx)("span", { children: hoverBusy ? "working" : hover.role })
              ]
            }
          ),
          /* @__PURE__ */ (0, import_jsx_runtime26.jsxs)("div", { className: "topo-dense__inspector", children: [
            /* @__PURE__ */ (0, import_jsx_runtime26.jsx)("strong", { children: hover.label }),
            /* @__PURE__ */ (0, import_jsx_runtime26.jsx)("span", { children: hover.group }),
            /* @__PURE__ */ (0, import_jsx_runtime26.jsxs)("span", { children: [
              layout.edgeById.get(hover.id)?.length || 0,
              " peers"
            ] }),
            hoverBusy && /* @__PURE__ */ (0, import_jsx_runtime26.jsx)("span", { children: "working" }),
            hoverCalls > 0 && /* @__PURE__ */ (0, import_jsx_runtime26.jsxs)("span", { children: [
              hoverCalls,
              " live calls"
            ] })
          ] })
        ] })
      ]
    }
  );
}

// src/panels/TopologyPanel.tsx
var import_jsx_runtime27 = require("react/jsx-runtime");
var VIEW_STORAGE = "mobkit-console-topology-view";
var EDGE_STORAGE = "mobkit-console-topology-edges";
var VIEWS = [
  { id: "graph", label: "Graph", help: "Dense canvas graph with every node in one view" },
  { id: "roles", label: "Roles", help: "Flat mob \xB7 agents grouped by role" }
];
var EDGE_MODES = [
  { id: "all", label: "All", help: "Draw all graph edges persistently" },
  { id: "focus", label: "Focus", help: "Show only hovered-agent edges" }
];
function TopologyPanel2({
  nodes,
  agents,
  activity
}) {
  const [view, setView] = import_react19.default.useState(() => {
    try {
      const stored = localStorage.getItem(VIEW_STORAGE);
      if (stored === "summary") return "graph";
      if (stored === "force") return "graph";
      if (stored === "bullseye") return "graph";
      if (stored === "graph" || stored === "roles") return stored;
    } catch {
    }
    return "graph";
  });
  const pickView = (next) => {
    setView(next);
    try {
      localStorage.setItem(VIEW_STORAGE, next);
    } catch {
    }
  };
  const [edgeMode, setEdgeMode] = import_react19.default.useState(() => {
    try {
      const stored = localStorage.getItem(EDGE_STORAGE);
      if (stored === "all" || stored === "focus") return stored;
    } catch {
    }
    return "all";
  });
  const pickEdgeMode = (next) => {
    setEdgeMode(next);
    try {
      localStorage.setItem(EDGE_STORAGE, next);
    } catch {
    }
  };
  const graph = import_react19.default.useMemo(() => buildGraph2(nodes, agents), [nodes, agents]);
  const live = useTopologyActivity2(activity, graph, { life: 8e3 });
  const liveCount = Object.keys(live.active).length;
  const busyCount = Object.values(live.busy).filter(Boolean).length;
  return /* @__PURE__ */ (0, import_jsx_runtime27.jsxs)(
    "div",
    {
      className: "topo",
      "data-testid": "topology-panel",
      "data-activity-count": activity.length,
      "data-busy-count": busyCount,
      "data-live-count": liveCount,
      children: [
        /* @__PURE__ */ (0, import_jsx_runtime27.jsxs)("div", { className: "topo__head", children: [
          /* @__PURE__ */ (0, import_jsx_runtime27.jsx)("h2", { children: "Topology" }),
          /* @__PURE__ */ (0, import_jsx_runtime27.jsxs)("span", { className: "topo__head-meta", children: [
            graph.agents.length,
            " agents \xB7 ",
            graph.edges.length,
            " edges",
            busyCount > 0 ? ` \xB7 ${busyCount} working` : "",
            liveCount > 0 && busyCount === 0 ? ` \xB7 ${liveCount} live` : ""
          ] }),
          view === "graph" && /* @__PURE__ */ (0, import_jsx_runtime27.jsxs)("div", { className: "topo__viewbar topo__viewbar--labels", role: "group", "aria-label": "Edges", children: [
            /* @__PURE__ */ (0, import_jsx_runtime27.jsx)("span", { className: "topo__viewbar-tag", children: "Edges" }),
            EDGE_MODES.map((m) => /* @__PURE__ */ (0, import_jsx_runtime27.jsx)(
              "button",
              {
                type: "button",
                className: `topo__viewbtn ${edgeMode === m.id ? "is-active" : ""}`,
                onClick: () => pickEdgeMode(m.id),
                title: m.help,
                "data-testid": `topology-edges:${m.id}`,
                children: m.label
              },
              m.id
            ))
          ] }),
          /* @__PURE__ */ (0, import_jsx_runtime27.jsx)("div", { className: "topo__viewbar", children: VIEWS.map((v) => /* @__PURE__ */ (0, import_jsx_runtime27.jsx)(
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
        /* @__PURE__ */ (0, import_jsx_runtime27.jsxs)("div", { className: "topo__body", children: [
          view === "graph" && /* @__PURE__ */ (0, import_jsx_runtime27.jsx)(DenseGraphMap2, { graph, edgeMode, activity: live }),
          view === "roles" && /* @__PURE__ */ (0, import_jsx_runtime27.jsx)(
            RoleTree2,
            {
              nodes,
              agents,
              activity
            }
          )
        ] })
      ]
    }
  );
}

// src/panels/TimelinePanel.tsx
var import_react20 = __toESM(require("react"));
var import_jsx_runtime28 = require("react/jsx-runtime");
var INTERNAL_TIMELINE_EVENTS = /* @__PURE__ */ new Set([
  "keep-alive",
  "snapshot_complete",
  "snapshot_started",
  "subscribed"
]);
function classifyFrame(frame) {
  const ev = frame.event;
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
function summarizeFrame(frame) {
  const ev = frame.event;
  const data = frame.data || {};
  const shortInteraction = String(frame.interactionId || "").slice(0, 8);
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
  const entries = import_react20.default.useMemo(() => {
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
  return /* @__PURE__ */ (0, import_jsx_runtime28.jsxs)("div", { className: "tl", "data-testid": "timeline-panel", children: [
    /* @__PURE__ */ (0, import_jsx_runtime28.jsxs)("div", { className: "tl__head", children: [
      /* @__PURE__ */ (0, import_jsx_runtime28.jsx)("h2", { children: "Today" }),
      /* @__PURE__ */ (0, import_jsx_runtime28.jsxs)("p", { children: [
        "\xB7 ",
        entries.length,
        " events \xB7 ",
        dateLabel
      ] })
    ] }),
    /* @__PURE__ */ (0, import_jsx_runtime28.jsxs)("div", { className: "tl__body", children: [
      entries.length === 0 && /* @__PURE__ */ (0, import_jsx_runtime28.jsx)("div", { style: { gridColumn: "1 / -1", padding: "40px 0", color: "var(--ink-dim)", fontFamily: "var(--mono)", fontSize: 12, textAlign: "center" }, children: "No events yet today." }),
      entries.map((e, i) => /* @__PURE__ */ (0, import_jsx_runtime28.jsxs)("div", { className: "tl__row", "data-type": e.type, children: [
        /* @__PURE__ */ (0, import_jsx_runtime28.jsx)("div", { className: "tl__time", children: e.time }),
        /* @__PURE__ */ (0, import_jsx_runtime28.jsx)("div", { className: "tl__rail", children: /* @__PURE__ */ (0, import_jsx_runtime28.jsx)("span", { className: "tl__dot" }) }),
        /* @__PURE__ */ (0, import_jsx_runtime28.jsxs)("div", { className: "tl__card", children: [
          /* @__PURE__ */ (0, import_jsx_runtime28.jsxs)("div", { children: [
            /* @__PURE__ */ (0, import_jsx_runtime28.jsx)("span", { className: "tl__type", children: formatType(e.type) }),
            " ",
            /* @__PURE__ */ (0, import_jsx_runtime28.jsx)("span", { children: e.text })
          ] }),
          /* @__PURE__ */ (0, import_jsx_runtime28.jsx)("div", { className: "tl__who", children: e.who })
        ] })
      ] }, i))
    ] })
  ] });
}

// src/panels/GatingInboxPanel.tsx
var import_react21 = __toESM(require("react"));
var import_jsx_runtime29 = require("react/jsx-runtime");
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
function GatingInboxPanel({
  pending,
  audit,
  onDecide,
  readOnly = false
}) {
  const [tab, setTab] = import_react21.default.useState("pending");
  const [selectedId, setSelectedId] = import_react21.default.useState(null);
  const policies = import_react21.default.useMemo(() => derivePolicies(audit), [audit]);
  const autoApproved = audit.filter((e) => {
    const r2 = e;
    return String(r2.decision || "").toLowerCase() === "auto_approve" || String(r2.event_type || "").includes("auto");
  });
  const currentList = tab === "pending" ? pending : tab === "auto" ? autoApproved : audit;
  return /* @__PURE__ */ (0, import_jsx_runtime29.jsxs)("div", { className: "gating", "data-testid": "gating-panel", children: [
    /* @__PURE__ */ (0, import_jsx_runtime29.jsxs)("div", { className: "gating__head", children: [
      /* @__PURE__ */ (0, import_jsx_runtime29.jsx)("h2", { children: "Approvals" }),
      /* @__PURE__ */ (0, import_jsx_runtime29.jsxs)("p", { children: [
        "\xB7 ",
        pending.length,
        " pending \xB7 ",
        autoApproved.length,
        " auto-approved \xB7 ",
        policies.length,
        " policies"
      ] })
    ] }),
    /* @__PURE__ */ (0, import_jsx_runtime29.jsxs)("div", { className: "gating__tabs", children: [
      /* @__PURE__ */ (0, import_jsx_runtime29.jsxs)(
        "button",
        {
          className: `gating__tab ${tab === "pending" ? "is-active" : ""}`,
          onClick: () => setTab("pending"),
          "data-testid": "gating-tab:pending",
          children: [
            "Pending ",
            /* @__PURE__ */ (0, import_jsx_runtime29.jsx)("span", { className: "n", children: pending.length })
          ]
        }
      ),
      /* @__PURE__ */ (0, import_jsx_runtime29.jsxs)(
        "button",
        {
          className: `gating__tab ${tab === "auto" ? "is-active" : ""}`,
          onClick: () => setTab("auto"),
          "data-testid": "gating-tab:auto",
          children: [
            "Auto ",
            /* @__PURE__ */ (0, import_jsx_runtime29.jsx)("span", { className: "n", children: autoApproved.length })
          ]
        }
      ),
      /* @__PURE__ */ (0, import_jsx_runtime29.jsxs)(
        "button",
        {
          className: `gating__tab ${tab === "audit" ? "is-active" : ""}`,
          onClick: () => setTab("audit"),
          "data-testid": "gating-tab:audit",
          children: [
            "Audit ",
            /* @__PURE__ */ (0, import_jsx_runtime29.jsx)("span", { className: "n", children: audit.length })
          ]
        }
      ),
      /* @__PURE__ */ (0, import_jsx_runtime29.jsxs)(
        "button",
        {
          className: `gating__tab ${tab === "policies" ? "is-active" : ""}`,
          onClick: () => setTab("policies"),
          "data-testid": "gating-tab:policies",
          children: [
            "Policies ",
            /* @__PURE__ */ (0, import_jsx_runtime29.jsx)("span", { className: "n", children: policies.length })
          ]
        }
      )
    ] }),
    /* @__PURE__ */ (0, import_jsx_runtime29.jsx)("div", { className: "gating__list", children: tab === "policies" ? /* @__PURE__ */ (0, import_jsx_runtime29.jsxs)("div", { className: "gating__policies", children: [
      policies.length === 0 && /* @__PURE__ */ (0, import_jsx_runtime29.jsx)("div", { className: "gating__empty", children: "No gate policies inferred from recent audit." }),
      policies.map((policy) => /* @__PURE__ */ (0, import_jsx_runtime29.jsxs)("div", { className: "gpolicy", "data-state": policy.state, "data-testid": `gating-policy:${policy.id}`, children: [
        /* @__PURE__ */ (0, import_jsx_runtime29.jsxs)("div", { className: "gpolicy__head", children: [
          /* @__PURE__ */ (0, import_jsx_runtime29.jsx)("span", { className: "gpolicy__action", children: policy.action }),
          /* @__PURE__ */ (0, import_jsx_runtime29.jsx)("span", { className: `gpolicy__state gpolicy__state--${policy.state}`, children: policy.state })
        ] }),
        /* @__PURE__ */ (0, import_jsx_runtime29.jsxs)("div", { className: "gpolicy__meta", children: [
          "scope: ",
          policy.scope
        ] }),
        /* @__PURE__ */ (0, import_jsx_runtime29.jsx)("div", { className: "gpolicy__rule", children: policy.thresh }),
        /* @__PURE__ */ (0, import_jsx_runtime29.jsxs)("div", { className: "gpolicy__stats", children: [
          /* @__PURE__ */ (0, import_jsx_runtime29.jsxs)("span", { children: [
            /* @__PURE__ */ (0, import_jsx_runtime29.jsx)("b", { children: policy.approved }),
            " approved"
          ] }),
          /* @__PURE__ */ (0, import_jsx_runtime29.jsxs)("span", { children: [
            /* @__PURE__ */ (0, import_jsx_runtime29.jsx)("b", { children: policy.rejected }),
            " rejected"
          ] }),
          /* @__PURE__ */ (0, import_jsx_runtime29.jsxs)("span", { children: [
            /* @__PURE__ */ (0, import_jsx_runtime29.jsx)("b", { children: policy.escalated }),
            " escalated"
          ] })
        ] }),
        /* @__PURE__ */ (0, import_jsx_runtime29.jsx)("div", { className: "gpolicy__approvers", children: policy.approvers.length === 0 ? /* @__PURE__ */ (0, import_jsx_runtime29.jsx)("span", { className: "chip", children: "no approvers recorded" }) : policy.approvers.map((approver) => /* @__PURE__ */ (0, import_jsx_runtime29.jsx)("span", { className: "chip", children: approver }, approver)) })
      ] }, policy.id))
    ] }) : /* @__PURE__ */ (0, import_jsx_runtime29.jsxs)(import_jsx_runtime29.Fragment, { children: [
      currentList.length === 0 && /* @__PURE__ */ (0, import_jsx_runtime29.jsxs)("div", { className: "gating__empty", children: [
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
        const showActions = tab === "pending" && !readOnly;
        return /* @__PURE__ */ (0, import_jsx_runtime29.jsxs)(
          "div",
          {
            className: `gitem ${selected ? "is-selected" : ""}`,
            "data-risk": risk,
            "data-testid": `gating-pending:${pid}`,
            onClick: () => setSelectedId(pid),
            children: [
              /* @__PURE__ */ (0, import_jsx_runtime29.jsx)("span", { className: "gitem__risk" }),
              /* @__PURE__ */ (0, import_jsx_runtime29.jsx)("span", { className: "gitem__id", children: pid.slice(0, 8) }),
              /* @__PURE__ */ (0, import_jsx_runtime29.jsxs)("span", { children: [
                /* @__PURE__ */ (0, import_jsx_runtime29.jsx)("div", { className: "gitem__action", children: action }),
                payload && /* @__PURE__ */ (0, import_jsx_runtime29.jsx)("div", { className: "gitem__payload", children: payload }),
                agent && /* @__PURE__ */ (0, import_jsx_runtime29.jsx)("div", { className: "gitem__agent", children: agent })
              ] }),
              showActions ? /* @__PURE__ */ (0, import_jsx_runtime29.jsxs)("span", { className: "gitem__actions", children: [
                /* @__PURE__ */ (0, import_jsx_runtime29.jsx)(
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
                /* @__PURE__ */ (0, import_jsx_runtime29.jsx)(
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
                /* @__PURE__ */ (0, import_jsx_runtime29.jsx)(
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
              ] }) : /* @__PURE__ */ (0, import_jsx_runtime29.jsx)("span", { className: "gitem__actions" }),
              /* @__PURE__ */ (0, import_jsx_runtime29.jsxs)("span", { className: "gitem__waited", children: [
                "waited",
                /* @__PURE__ */ (0, import_jsx_runtime29.jsx)("br", {}),
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

// src/panels/AccessPanel.tsx
var import_react22 = __toESM(require("react"));
var import_jsx_runtime30 = require("react/jsx-runtime");
var DEFAULT_ACTIONS = [
  "agent.view",
  "agent.send",
  "agent.spawn",
  "agent.respawn",
  "agent.retire",
  "agent.reset",
  "gating.view",
  "gating.decide",
  "mob.observe",
  "runtime.admin",
  "access.admin"
];
function parseListInput(raw) {
  return raw.split(/[,\n]/).map((token) => token.trim()).filter((token) => token.length > 0);
}
function formatListInput(values) {
  return (values || []).join(", ");
}
function parseLabelSelectorInput(raw) {
  const labels = {};
  for (const token of parseListInput(raw)) {
    const eq = token.indexOf("=");
    if (eq <= 0) continue;
    const key = token.slice(0, eq).trim();
    const value = token.slice(eq + 1).trim();
    if (key) labels[key] = value;
  }
  return labels;
}
function formatLabelSelectorInput(labels) {
  return Object.entries(labels || {}).map(([key, value]) => `${key}=${value}`).join(", ");
}
function summarizeRuleSubjects(rule) {
  const parts = [];
  if (rule.groups?.length) parts.push(`groups: ${rule.groups.join(", ")}`);
  if (rule.subjects?.length) parts.push(rule.subjects.join(", "));
  return parts.length > 0 ? parts.join(" \xB7 ") : "everyone";
}
function summarizeRuleResources(rule) {
  const parts = [];
  if (rule.agents?.length) parts.push(`agents: ${rule.agents.join(", ")}`);
  if (rule.roles?.length) parts.push(`roles: ${rule.roles.join(", ")}`);
  const labels = formatLabelSelectorInput(rule.match_labels);
  if (labels) parts.push(`labels: ${labels}`);
  return parts.length > 0 ? parts.join(" \xB7 ") : "all agents";
}
function emptyRuleDraft() {
  return {
    id: "",
    description: "",
    effect: "allow",
    subjects: "",
    groups: "",
    actions: ["agent.view"],
    agents: "",
    roles: "",
    matchLabels: ""
  };
}
function draftFromRule(rule) {
  return {
    id: rule.id,
    description: rule.description || "",
    effect: rule.effect === "deny" ? "deny" : "allow",
    subjects: formatListInput(rule.subjects),
    groups: formatListInput(rule.groups),
    actions: [...rule.actions],
    agents: formatListInput(rule.agents),
    roles: formatListInput(rule.roles),
    matchLabels: formatLabelSelectorInput(rule.match_labels)
  };
}
function ruleFromDraft(draft) {
  const rule = {
    id: draft.id.trim(),
    effect: draft.effect,
    actions: draft.actions
  };
  const description = draft.description.trim();
  if (description) rule.description = description;
  const subjects = parseListInput(draft.subjects);
  if (subjects.length) rule.subjects = subjects;
  const groups = parseListInput(draft.groups);
  if (groups.length) rule.groups = groups;
  const agents = parseListInput(draft.agents);
  if (agents.length) rule.agents = agents;
  const roles = parseListInput(draft.roles);
  if (roles.length) rule.roles = roles;
  const labels = parseLabelSelectorInput(draft.matchLabels);
  if (Object.keys(labels).length) rule.match_labels = labels;
  return rule;
}
function AccessPanel({
  status,
  config,
  error,
  readOnly = false,
  agents,
  onRefresh,
  onSetEnabled,
  onSaveAdmins,
  onUpsertRule,
  onDeleteRule,
  onSaveGroup,
  onDeleteGroup,
  onPreview
}) {
  const [tab, setTab] = import_react22.default.useState("overview");
  const [ruleDraft, setRuleDraft] = import_react22.default.useState(null);
  const [adminsDraft, setAdminsDraft] = import_react22.default.useState(null);
  const [groupNameDraft, setGroupNameDraft] = import_react22.default.useState("");
  const [groupMembersDraft, setGroupMembersDraft] = import_react22.default.useState("");
  const [editingGroup, setEditingGroup] = import_react22.default.useState(null);
  const [previewSubject, setPreviewSubject] = import_react22.default.useState("");
  const [previewAction, setPreviewAction] = import_react22.default.useState("agent.view");
  const [previewIdentity, setPreviewIdentity] = import_react22.default.useState("");
  const [previewResult, setPreviewResult] = import_react22.default.useState(null);
  const actions = status?.actions?.length ? status.actions : DEFAULT_ACTIONS;
  const rules = config?.rules || [];
  const groups = Object.entries(config?.groups || {});
  const enabled = config?.enabled === true;
  const canEdit = !readOnly && Boolean(config);
  function startGroupEdit(name, members) {
    setEditingGroup(name);
    setGroupNameDraft(name);
    setGroupMembersDraft(formatListInput(members));
  }
  function submitGroup() {
    const name = groupNameDraft.trim();
    if (!name) return;
    onSaveGroup(name, { members: parseListInput(groupMembersDraft) });
    setEditingGroup(null);
    setGroupNameDraft("");
    setGroupMembersDraft("");
  }
  async function runPreview() {
    const subject = previewSubject.trim();
    if (!subject || !previewAction) return;
    const result = await onPreview(
      subject,
      previewAction,
      previewIdentity.trim() || void 0
    );
    setPreviewResult(result);
  }
  return /* @__PURE__ */ (0, import_jsx_runtime30.jsxs)("div", { className: "gating access-panel", "data-testid": "access-panel", children: [
    /* @__PURE__ */ (0, import_jsx_runtime30.jsxs)("div", { className: "gating__head", children: [
      /* @__PURE__ */ (0, import_jsx_runtime30.jsx)("h2", { children: "Access" }),
      /* @__PURE__ */ (0, import_jsx_runtime30.jsxs)("p", { children: [
        "\xB7 ",
        enabled ? "enforcing" : "not enforced",
        " \xB7 ",
        rules.length,
        " rules \xB7",
        " ",
        groups.length,
        " groups",
        status?.subject ? /* @__PURE__ */ (0, import_jsx_runtime30.jsxs)(import_jsx_runtime30.Fragment, { children: [
          " \xB7 you are ",
          status.subject
        ] }) : null
      ] })
    ] }),
    error ? /* @__PURE__ */ (0, import_jsx_runtime30.jsx)("div", { className: "gating__empty", "data-testid": "access-error", children: error }) : null,
    /* @__PURE__ */ (0, import_jsx_runtime30.jsxs)("div", { className: "gating__tabs", children: [
      ["overview", "groups", "rules", "preview"].map((candidate) => /* @__PURE__ */ (0, import_jsx_runtime30.jsxs)(
        "button",
        {
          className: `gating__tab ${tab === candidate ? "is-active" : ""}`,
          onClick: () => setTab(candidate),
          "data-testid": `access-tab:${candidate}`,
          children: [
            candidate === "overview" ? "Overview" : candidate === "groups" ? `Groups` : candidate === "rules" ? `Rules` : "Preview",
            candidate === "groups" ? /* @__PURE__ */ (0, import_jsx_runtime30.jsx)("span", { className: "n", children: groups.length }) : null,
            candidate === "rules" ? /* @__PURE__ */ (0, import_jsx_runtime30.jsx)("span", { className: "n", children: rules.length }) : null
          ]
        },
        candidate
      )),
      /* @__PURE__ */ (0, import_jsx_runtime30.jsx)("button", { className: "gating__tab", onClick: onRefresh, "data-testid": "access-refresh", children: "Refresh" })
    ] }),
    /* @__PURE__ */ (0, import_jsx_runtime30.jsxs)("div", { className: "gating__list access-panel__body", children: [
      tab === "overview" ? /* @__PURE__ */ (0, import_jsx_runtime30.jsxs)("div", { className: "gating__policies", children: [
        /* @__PURE__ */ (0, import_jsx_runtime30.jsxs)("div", { className: "gpolicy", "data-state": enabled ? "active" : "paused", children: [
          /* @__PURE__ */ (0, import_jsx_runtime30.jsxs)("div", { className: "gpolicy__head", children: [
            /* @__PURE__ */ (0, import_jsx_runtime30.jsx)("span", { className: "gpolicy__action", children: "Enforcement" }),
            /* @__PURE__ */ (0, import_jsx_runtime30.jsx)("span", { className: `gpolicy__state gpolicy__state--${enabled ? "active" : "paused"}`, children: enabled ? "enabled" : "disabled" })
          ] }),
          /* @__PURE__ */ (0, import_jsx_runtime30.jsx)("div", { className: "gpolicy__rule", children: enabled ? "Deny by default: every console caller only sees and operates what a rule (or admin standing) grants." : "Access control is configured but not enforced. Enabling requires at least one admin subject." }),
          canEdit ? /* @__PURE__ */ (0, import_jsx_runtime30.jsx)("div", { className: "gpolicy__stats", children: /* @__PURE__ */ (0, import_jsx_runtime30.jsx)(
            "button",
            {
              "data-testid": "access-toggle-enabled",
              onClick: () => onSetEnabled(!enabled),
              children: enabled ? "Disable enforcement" : "Enable enforcement"
            }
          ) }) : null
        ] }),
        /* @__PURE__ */ (0, import_jsx_runtime30.jsxs)("div", { className: "gpolicy", "data-state": "active", children: [
          /* @__PURE__ */ (0, import_jsx_runtime30.jsx)("div", { className: "gpolicy__head", children: /* @__PURE__ */ (0, import_jsx_runtime30.jsx)("span", { className: "gpolicy__action", children: "Admins" }) }),
          /* @__PURE__ */ (0, import_jsx_runtime30.jsx)("div", { className: "gpolicy__rule", children: "Admin subjects bypass every rule and manage this configuration." }),
          adminsDraft === null ? /* @__PURE__ */ (0, import_jsx_runtime30.jsxs)(import_jsx_runtime30.Fragment, { children: [
            /* @__PURE__ */ (0, import_jsx_runtime30.jsx)("div", { className: "gpolicy__approvers", children: (config?.admins || []).length === 0 ? /* @__PURE__ */ (0, import_jsx_runtime30.jsx)("span", { className: "chip", children: "no admins configured" }) : (config?.admins || []).map((admin) => /* @__PURE__ */ (0, import_jsx_runtime30.jsx)("span", { className: "chip", children: admin }, admin)) }),
            canEdit ? /* @__PURE__ */ (0, import_jsx_runtime30.jsx)("div", { className: "gpolicy__stats", children: /* @__PURE__ */ (0, import_jsx_runtime30.jsx)(
              "button",
              {
                "data-testid": "access-edit-admins",
                onClick: () => setAdminsDraft(formatListInput(config?.admins)),
                children: "Edit admins"
              }
            ) }) : null
          ] }) : /* @__PURE__ */ (0, import_jsx_runtime30.jsxs)("div", { className: "access-panel__form", children: [
            /* @__PURE__ */ (0, import_jsx_runtime30.jsxs)("label", { children: [
              "Admin subjects (comma separated)",
              /* @__PURE__ */ (0, import_jsx_runtime30.jsx)(
                "input",
                {
                  "data-testid": "access-admins-input",
                  value: adminsDraft,
                  onChange: (event) => setAdminsDraft(event.target.value),
                  placeholder: "root@example.com, ops-lead@example.com"
                }
              )
            ] }),
            /* @__PURE__ */ (0, import_jsx_runtime30.jsxs)("div", { className: "access-panel__form-actions", children: [
              /* @__PURE__ */ (0, import_jsx_runtime30.jsx)(
                "button",
                {
                  className: "approve",
                  "data-testid": "access-save-admins",
                  onClick: () => {
                    onSaveAdmins(parseListInput(adminsDraft));
                    setAdminsDraft(null);
                  },
                  children: "Save"
                }
              ),
              /* @__PURE__ */ (0, import_jsx_runtime30.jsx)("button", { onClick: () => setAdminsDraft(null), children: "Cancel" })
            ] })
          ] })
        ] })
      ] }) : null,
      tab === "groups" ? /* @__PURE__ */ (0, import_jsx_runtime30.jsxs)("div", { className: "gating__policies", children: [
        groups.length === 0 && editingGroup === null ? /* @__PURE__ */ (0, import_jsx_runtime30.jsx)("div", { className: "gating__empty", children: "No groups yet. Groups assign people to rules \u2014 create one, then reference it from a rule." }) : null,
        groups.map(
          ([name, group]) => editingGroup === name ? null : /* @__PURE__ */ (0, import_jsx_runtime30.jsxs)("div", { className: "gpolicy", "data-state": "active", "data-testid": `access-group:${name}`, children: [
            /* @__PURE__ */ (0, import_jsx_runtime30.jsx)("div", { className: "gpolicy__head", children: /* @__PURE__ */ (0, import_jsx_runtime30.jsx)("span", { className: "gpolicy__action", children: name }) }),
            group.description ? /* @__PURE__ */ (0, import_jsx_runtime30.jsx)("div", { className: "gpolicy__rule", children: group.description }) : null,
            /* @__PURE__ */ (0, import_jsx_runtime30.jsx)("div", { className: "gpolicy__approvers", children: (group.members || []).length === 0 ? /* @__PURE__ */ (0, import_jsx_runtime30.jsx)("span", { className: "chip", children: "no members" }) : (group.members || []).map((member) => /* @__PURE__ */ (0, import_jsx_runtime30.jsx)("span", { className: "chip", children: member }, member)) }),
            canEdit ? /* @__PURE__ */ (0, import_jsx_runtime30.jsxs)("div", { className: "gpolicy__stats", children: [
              /* @__PURE__ */ (0, import_jsx_runtime30.jsx)(
                "button",
                {
                  "data-testid": `access-group-edit:${name}`,
                  onClick: () => startGroupEdit(name, group.members),
                  children: "Edit members"
                }
              ),
              /* @__PURE__ */ (0, import_jsx_runtime30.jsx)(
                "button",
                {
                  className: "reject",
                  "data-testid": `access-group-delete:${name}`,
                  onClick: () => {
                    if (window.confirm(`Delete group "${name}"?`)) {
                      onDeleteGroup(name);
                    }
                  },
                  children: "Delete"
                }
              )
            ] }) : null
          ] }, name)
        ),
        canEdit ? /* @__PURE__ */ (0, import_jsx_runtime30.jsxs)("div", { className: "gpolicy", "data-state": "active", children: [
          /* @__PURE__ */ (0, import_jsx_runtime30.jsx)("div", { className: "gpolicy__head", children: /* @__PURE__ */ (0, import_jsx_runtime30.jsx)("span", { className: "gpolicy__action", children: editingGroup ? `Edit ${editingGroup}` : "New group" }) }),
          /* @__PURE__ */ (0, import_jsx_runtime30.jsxs)("div", { className: "access-panel__form", children: [
            /* @__PURE__ */ (0, import_jsx_runtime30.jsxs)("label", { children: [
              "Group name",
              /* @__PURE__ */ (0, import_jsx_runtime30.jsx)(
                "input",
                {
                  "data-testid": "access-group-name",
                  value: groupNameDraft,
                  onChange: (event) => setGroupNameDraft(event.target.value),
                  placeholder: "ops",
                  disabled: editingGroup !== null
                }
              )
            ] }),
            /* @__PURE__ */ (0, import_jsx_runtime30.jsxs)("label", { children: [
              "Members (comma separated subjects)",
              /* @__PURE__ */ (0, import_jsx_runtime30.jsx)(
                "input",
                {
                  "data-testid": "access-group-members",
                  value: groupMembersDraft,
                  onChange: (event) => setGroupMembersDraft(event.target.value),
                  placeholder: "alice@example.com, bob@example.com"
                }
              )
            ] }),
            /* @__PURE__ */ (0, import_jsx_runtime30.jsxs)("div", { className: "access-panel__form-actions", children: [
              /* @__PURE__ */ (0, import_jsx_runtime30.jsx)("button", { className: "approve", "data-testid": "access-group-save", onClick: submitGroup, children: editingGroup ? "Save members" : "Create group" }),
              editingGroup ? /* @__PURE__ */ (0, import_jsx_runtime30.jsx)("button", { onClick: () => {
                setEditingGroup(null);
                setGroupNameDraft("");
                setGroupMembersDraft("");
              }, children: "Cancel" }) : null
            ] })
          ] })
        ] }) : null
      ] }) : null,
      tab === "rules" ? /* @__PURE__ */ (0, import_jsx_runtime30.jsxs)("div", { className: "gating__policies", children: [
        rules.length === 0 && !ruleDraft ? /* @__PURE__ */ (0, import_jsx_runtime30.jsx)("div", { className: "gating__empty", children: "No rules. While enforcement is on, only admins can see or do anything until rules grant access." }) : null,
        rules.map(
          (rule) => ruleDraft && ruleDraft.id === rule.id ? null : /* @__PURE__ */ (0, import_jsx_runtime30.jsxs)(
            "div",
            {
              className: "gpolicy",
              "data-state": rule.effect === "deny" ? "paused" : "active",
              "data-testid": `access-rule:${rule.id}`,
              children: [
                /* @__PURE__ */ (0, import_jsx_runtime30.jsxs)("div", { className: "gpolicy__head", children: [
                  /* @__PURE__ */ (0, import_jsx_runtime30.jsx)("span", { className: "gpolicy__action", children: rule.id }),
                  /* @__PURE__ */ (0, import_jsx_runtime30.jsx)("span", { className: `gpolicy__state gpolicy__state--${rule.effect === "deny" ? "paused" : "active"}`, children: rule.effect === "deny" ? "deny" : "allow" })
                ] }),
                rule.description ? /* @__PURE__ */ (0, import_jsx_runtime30.jsx)("div", { className: "gpolicy__rule", children: rule.description }) : null,
                /* @__PURE__ */ (0, import_jsx_runtime30.jsxs)("div", { className: "gpolicy__meta", children: [
                  "who: ",
                  summarizeRuleSubjects(rule)
                ] }),
                /* @__PURE__ */ (0, import_jsx_runtime30.jsxs)("div", { className: "gpolicy__meta", children: [
                  "what: ",
                  rule.actions.join(", ")
                ] }),
                /* @__PURE__ */ (0, import_jsx_runtime30.jsxs)("div", { className: "gpolicy__meta", children: [
                  "on: ",
                  summarizeRuleResources(rule)
                ] }),
                canEdit ? /* @__PURE__ */ (0, import_jsx_runtime30.jsxs)("div", { className: "gpolicy__stats", children: [
                  /* @__PURE__ */ (0, import_jsx_runtime30.jsx)(
                    "button",
                    {
                      "data-testid": `access-rule-edit:${rule.id}`,
                      onClick: () => setRuleDraft(draftFromRule(rule)),
                      children: "Edit"
                    }
                  ),
                  /* @__PURE__ */ (0, import_jsx_runtime30.jsx)(
                    "button",
                    {
                      className: "reject",
                      "data-testid": `access-rule-delete:${rule.id}`,
                      onClick: () => {
                        if (window.confirm(`Delete rule "${rule.id}"? Access it grants (or denies) stops immediately.`)) {
                          onDeleteRule(rule.id);
                        }
                      },
                      children: "Delete"
                    }
                  )
                ] }) : null
              ]
            },
            rule.id
          )
        ),
        canEdit && !ruleDraft ? /* @__PURE__ */ (0, import_jsx_runtime30.jsx)("div", { className: "gpolicy__stats", children: /* @__PURE__ */ (0, import_jsx_runtime30.jsx)("button", { "data-testid": "access-rule-new", onClick: () => setRuleDraft(emptyRuleDraft()), children: "New rule" }) }) : null,
        canEdit && ruleDraft ? /* @__PURE__ */ (0, import_jsx_runtime30.jsxs)("div", { className: "gpolicy", "data-state": "active", "data-testid": "access-rule-editor", children: [
          /* @__PURE__ */ (0, import_jsx_runtime30.jsx)("div", { className: "gpolicy__head", children: /* @__PURE__ */ (0, import_jsx_runtime30.jsx)("span", { className: "gpolicy__action", children: rules.some((rule) => rule.id === ruleDraft.id) ? `Edit ${ruleDraft.id}` : "New rule" }) }),
          /* @__PURE__ */ (0, import_jsx_runtime30.jsxs)("div", { className: "access-panel__form", children: [
            /* @__PURE__ */ (0, import_jsx_runtime30.jsxs)("label", { children: [
              "Rule id",
              /* @__PURE__ */ (0, import_jsx_runtime30.jsx)(
                "input",
                {
                  "data-testid": "access-rule-id",
                  value: ruleDraft.id,
                  onChange: (event) => setRuleDraft({ ...ruleDraft, id: event.target.value }),
                  placeholder: "ops-view-all"
                }
              )
            ] }),
            /* @__PURE__ */ (0, import_jsx_runtime30.jsxs)("label", { children: [
              "Description",
              /* @__PURE__ */ (0, import_jsx_runtime30.jsx)(
                "input",
                {
                  value: ruleDraft.description,
                  onChange: (event) => setRuleDraft({ ...ruleDraft, description: event.target.value }),
                  placeholder: "Ops can see every agent"
                }
              )
            ] }),
            /* @__PURE__ */ (0, import_jsx_runtime30.jsxs)("label", { children: [
              "Effect",
              /* @__PURE__ */ (0, import_jsx_runtime30.jsxs)(
                "select",
                {
                  "data-testid": "access-rule-effect",
                  value: ruleDraft.effect,
                  onChange: (event) => setRuleDraft({ ...ruleDraft, effect: event.target.value === "deny" ? "deny" : "allow" }),
                  children: [
                    /* @__PURE__ */ (0, import_jsx_runtime30.jsx)("option", { value: "allow", children: "allow" }),
                    /* @__PURE__ */ (0, import_jsx_runtime30.jsx)("option", { value: "deny", children: "deny" })
                  ]
                }
              )
            ] }),
            /* @__PURE__ */ (0, import_jsx_runtime30.jsxs)("label", { children: [
              "Groups (comma separated; empty + empty subjects = everyone)",
              /* @__PURE__ */ (0, import_jsx_runtime30.jsx)(
                "input",
                {
                  "data-testid": "access-rule-groups",
                  value: ruleDraft.groups,
                  onChange: (event) => setRuleDraft({ ...ruleDraft, groups: event.target.value }),
                  placeholder: "ops"
                }
              )
            ] }),
            /* @__PURE__ */ (0, import_jsx_runtime30.jsxs)("label", { children: [
              "Subjects (comma separated emails)",
              /* @__PURE__ */ (0, import_jsx_runtime30.jsx)(
                "input",
                {
                  value: ruleDraft.subjects,
                  onChange: (event) => setRuleDraft({ ...ruleDraft, subjects: event.target.value }),
                  placeholder: "alice@example.com"
                }
              )
            ] }),
            /* @__PURE__ */ (0, import_jsx_runtime30.jsxs)("label", { children: [
              "Actions",
              /* @__PURE__ */ (0, import_jsx_runtime30.jsx)("div", { className: "access-panel__chips", children: actions.map((action) => {
                const selected = ruleDraft.actions.includes(action);
                return /* @__PURE__ */ (0, import_jsx_runtime30.jsx)(
                  "button",
                  {
                    className: `chip ${selected ? "is-active" : ""}`,
                    "data-selected": selected ? "true" : "false",
                    "data-testid": `access-rule-action:${action}`,
                    onClick: () => setRuleDraft({
                      ...ruleDraft,
                      actions: selected ? ruleDraft.actions.filter((candidate) => candidate !== action) : [...ruleDraft.actions, action]
                    }),
                    children: action
                  },
                  action
                );
              }) })
            ] }),
            /* @__PURE__ */ (0, import_jsx_runtime30.jsxs)("label", { children: [
              "Agents (comma separated identities; empty = all)",
              /* @__PURE__ */ (0, import_jsx_runtime30.jsx)(
                "input",
                {
                  "data-testid": "access-rule-agents",
                  value: ruleDraft.agents,
                  onChange: (event) => setRuleDraft({ ...ruleDraft, agents: event.target.value }),
                  placeholder: agents.slice(0, 2).map((agent) => agent.identity).join(", ") || "identity:ops-lead"
                }
              )
            ] }),
            /* @__PURE__ */ (0, import_jsx_runtime30.jsxs)("label", { children: [
              "Roles (comma separated; empty = all)",
              /* @__PURE__ */ (0, import_jsx_runtime30.jsx)(
                "input",
                {
                  value: ruleDraft.roles,
                  onChange: (event) => setRuleDraft({ ...ruleDraft, roles: event.target.value }),
                  placeholder: "analyst"
                }
              )
            ] }),
            /* @__PURE__ */ (0, import_jsx_runtime30.jsxs)("label", { children: [
              "Label selector (key=value, comma separated; empty = all)",
              /* @__PURE__ */ (0, import_jsx_runtime30.jsx)(
                "input",
                {
                  value: ruleDraft.matchLabels,
                  onChange: (event) => setRuleDraft({ ...ruleDraft, matchLabels: event.target.value }),
                  placeholder: "org=payments"
                }
              )
            ] }),
            /* @__PURE__ */ (0, import_jsx_runtime30.jsxs)("div", { className: "access-panel__form-actions", children: [
              /* @__PURE__ */ (0, import_jsx_runtime30.jsx)(
                "button",
                {
                  className: "approve",
                  "data-testid": "access-rule-save",
                  disabled: !ruleDraft.id.trim() || ruleDraft.actions.length === 0,
                  onClick: () => {
                    onUpsertRule(ruleFromDraft(ruleDraft));
                    setRuleDraft(null);
                  },
                  children: "Save rule"
                }
              ),
              /* @__PURE__ */ (0, import_jsx_runtime30.jsx)("button", { onClick: () => setRuleDraft(null), children: "Cancel" })
            ] })
          ] })
        ] }) : null
      ] }) : null,
      tab === "preview" ? /* @__PURE__ */ (0, import_jsx_runtime30.jsx)("div", { className: "gating__policies", children: /* @__PURE__ */ (0, import_jsx_runtime30.jsxs)("div", { className: "gpolicy", "data-state": "active", children: [
        /* @__PURE__ */ (0, import_jsx_runtime30.jsx)("div", { className: "gpolicy__head", children: /* @__PURE__ */ (0, import_jsx_runtime30.jsx)("span", { className: "gpolicy__action", children: "Check access as someone else" }) }),
        /* @__PURE__ */ (0, import_jsx_runtime30.jsxs)("div", { className: "access-panel__form", children: [
          /* @__PURE__ */ (0, import_jsx_runtime30.jsxs)("label", { children: [
            "Subject",
            /* @__PURE__ */ (0, import_jsx_runtime30.jsx)(
              "input",
              {
                "data-testid": "access-preview-subject",
                value: previewSubject,
                onChange: (event) => setPreviewSubject(event.target.value),
                placeholder: "alice@example.com"
              }
            )
          ] }),
          /* @__PURE__ */ (0, import_jsx_runtime30.jsxs)("label", { children: [
            "Action",
            /* @__PURE__ */ (0, import_jsx_runtime30.jsx)(
              "select",
              {
                "data-testid": "access-preview-action",
                value: previewAction,
                onChange: (event) => setPreviewAction(event.target.value),
                children: actions.map((action) => /* @__PURE__ */ (0, import_jsx_runtime30.jsx)("option", { value: action, children: action }, action))
              }
            )
          ] }),
          /* @__PURE__ */ (0, import_jsx_runtime30.jsxs)("label", { children: [
            "Agent (optional)",
            /* @__PURE__ */ (0, import_jsx_runtime30.jsxs)(
              "select",
              {
                "data-testid": "access-preview-agent",
                value: previewIdentity,
                onChange: (event) => setPreviewIdentity(event.target.value),
                children: [
                  /* @__PURE__ */ (0, import_jsx_runtime30.jsx)("option", { value: "", children: "\u2014" }),
                  agents.map((agent) => /* @__PURE__ */ (0, import_jsx_runtime30.jsx)("option", { value: agent.identity, children: agent.label || agent.identity }, agent.identity))
                ]
              }
            )
          ] }),
          /* @__PURE__ */ (0, import_jsx_runtime30.jsx)("div", { className: "access-panel__form-actions", children: /* @__PURE__ */ (0, import_jsx_runtime30.jsx)("button", { className: "approve", "data-testid": "access-preview-run", onClick: () => void runPreview(), children: "Evaluate" }) }),
          previewResult ? /* @__PURE__ */ (0, import_jsx_runtime30.jsxs)(
            "div",
            {
              className: "gpolicy__rule",
              "data-testid": "access-preview-result",
              "data-allowed": previewResult.allowed ? "true" : "false",
              children: [
                previewResult.allowed ? "ALLOWED" : "DENIED",
                previewResult.reason ? ` \u2014 ${previewResult.reason}` : "",
                previewResult.is_admin ? " (admin)" : "",
                previewResult.groups?.length ? ` \xB7 groups: ${previewResult.groups.join(", ")}` : ""
              ]
            }
          ) : null
        ] })
      ] }) }) : null
    ] })
  ] });
}

// src/panels/MemoryPanel.tsx
var import_react23 = __toESM(require("react"));
var import_jsx_runtime31 = require("react/jsx-runtime");
var MEMORY_TABS = [
  "holdings",
  "records",
  "knowledge",
  "pipeline",
  "dreams"
];
var TAB_LABEL = {
  holdings: "Holdings",
  records: "Records",
  knowledge: "Knowledge",
  pipeline: "Pipeline",
  dreams: "Dreams"
};
function memoryTabLabel(tab) {
  return TAB_LABEL[tab];
}
function resolveMemoryTabAlias(tab) {
  if (tab === "quarantine") return "pipeline";
  return MEMORY_TABS.includes(tab) ? tab : null;
}
function realmOfRecord(record) {
  return record.scope.realm;
}
function scopeGroupKey(scope) {
  switch (scope.scope) {
    case "identity":
      return `identity:${scope.realm}:${scope.identity}`;
    case "mob":
      return `mob:${scope.realm}:${scope.mob}`;
    case "operator":
      return `operator:${scope.realm}:${scope.operator}`;
    case "realm":
      return `realm:${scope.realm}`;
  }
}
function scopeGroupLabel(scope) {
  switch (scope.scope) {
    case "identity":
      return scope.identity;
    case "mob":
      return `Mob: ${scope.mob}`;
    case "operator":
      return `Operator: ${scope.operator}`;
    case "realm":
      return "Realm";
  }
}
function scopeGroupRank(scope) {
  switch (scope.scope) {
    case "identity":
      return 0;
    case "mob":
      return 1;
    case "operator":
      return 2;
    case "realm":
      return 3;
  }
}
function groupRecordsByScope(records) {
  const byKey = /* @__PURE__ */ new Map();
  for (const record of records) {
    const key = scopeGroupKey(record.scope);
    let group = byKey.get(key);
    if (!group) {
      group = {
        key,
        label: scopeGroupLabel(record.scope),
        scope: record.scope,
        records: []
      };
      byKey.set(key, group);
    }
    group.records.push(record);
  }
  return Array.from(byKey.values()).sort((a, b) => {
    const rankDelta = scopeGroupRank(a.scope) - scopeGroupRank(b.scope);
    if (rankDelta !== 0) return rankDelta;
    return a.label.localeCompare(b.label);
  });
}
var TRUST_LABEL = {
  untrusted: "untrusted",
  agent_observed: "observed",
  agent_verified: "verified",
  application: "application",
  operator: "operator"
};
var TRUST_RANK = {
  untrusted: 0,
  agent_observed: 1,
  agent_verified: 2,
  application: 3,
  operator: 4
};
function trustLabel(trust) {
  return trust && TRUST_LABEL[trust] || String(trust || "unknown");
}
function trustTone(trust) {
  switch (trust) {
    case "operator":
    case "application":
    case "agent_verified":
      return "positive";
    case "agent_observed":
      return "neutral";
    default:
      return "muted";
  }
}
function statusLabel(status) {
  if (!status) return "unknown";
  switch (status.status) {
    case "active":
      return "active";
    case "superseded":
      return status.by ? `superseded \u2192 ${status.by}` : "superseded";
    case "quarantined":
      return status.reason ? `quarantined: ${status.reason}` : "quarantined";
    case "tombstoned":
      return "tombstoned";
  }
}
function statusTone(status) {
  if (!status) return "muted";
  switch (status.status) {
    case "active":
      return "positive";
    case "quarantined":
      return "warning";
    default:
      return "muted";
  }
}
var RELATIVE_UNITS = [
  [365 * 24 * 60 * 60 * 1e3, "y"],
  [24 * 60 * 60 * 1e3, "d"],
  [60 * 60 * 1e3, "h"],
  [60 * 1e3, "m"],
  [1e3, "s"]
];
function relativeAge(atMs, now = Date.now()) {
  if (!atMs || atMs <= 0) return "\u2014";
  const diff = now - atMs;
  if (diff < 0) return "now";
  if (diff < 1e3) return "now";
  for (const [unitMs, suffix] of RELATIVE_UNITS) {
    if (diff >= unitMs) {
      return `${Math.floor(diff / unitMs)}${suffix} ago`;
    }
  }
  return "now";
}
function evidenceLabel(evidence) {
  const parts = [];
  if (evidence.session_id) parts.push(`session ${evidence.session_id}`);
  if (typeof evidence.generation === "number") parts.push(`gen ${evidence.generation}`);
  if (evidence.revision) parts.push(`rev ${evidence.revision}`);
  if (evidence.range && evidence.range.length === 2) {
    const [start, end] = evidence.range;
    parts.push(`msgs ${start}\u2013${end}`);
  }
  return parts.join(" \u2022 ") || "evidence";
}
function authorLine(author) {
  if (!author) return "unknown author";
  switch (author.author) {
    case "agent":
      return author.identity ? `agent ${author.identity}` : "agent";
    case "steward":
      return author.run_id ? `steward run ${author.run_id}` : "steward";
    case "distiller":
      return author.run_id ? `distiller run ${author.run_id}` : "distiller";
    case "operator":
      return "operator";
    case "application":
      return "application";
  }
}
function dreamOpKindsSummary(opKinds) {
  if (!opKinds) return "";
  const entries = Object.entries(opKinds).filter(([, count]) => typeof count === "number" && count > 0).sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0]));
  if (entries.length === 0) return "";
  return entries.map(([kind, count]) => `${count} ${kind}`).join(" \xB7 ");
}
function dreamTimeRange(run, now = Date.now()) {
  const first = run.first_op_at_ms;
  const last = run.last_op_at_ms;
  if (!first && !last) return "\u2014";
  if (first && last && first !== last) {
    return `${relativeAge(first, now)} \u2192 ${relativeAge(last, now)}`;
  }
  return relativeAge(last || first, now);
}
function injectionLine(injection, now = Date.now()) {
  const surface = injection.surface === "build" ? "build" : "turn";
  return `${surface} \u2022 ${injection.identity} \u2022 ${relativeAge(injection.at_ms, now)}`;
}
function buildRecordsQueryParams(filter, options = {}) {
  const params = {};
  const key = filter.key?.trim();
  if (filter.scope === "identity" || !filter.scope && key) {
    if (key) params.identity = key;
    else params.scope = "identity";
  } else if (filter.scope === "mob" || filter.scope === "operator") {
    params.scope = filter.scope;
    if (key) params.scope_key = key;
  } else if (filter.scope === "realm") {
    params.scope = "realm";
  }
  if (filter.status) params.status = filter.status;
  if (filter.realm?.trim()) params.realm = filter.realm.trim();
  if (options.limit) params.limit = options.limit;
  if (options.cursor) params.cursor = options.cursor;
  return params;
}
function hasActiveFilter(filter) {
  return Boolean(filter.scope || filter.key?.trim() || filter.status || filter.realm?.trim());
}
function filtersEquivalent(a, b) {
  return (a.scope || void 0) === (b.scope || void 0) && (a.key?.trim() || "") === (b.key?.trim() || "") && (a.status || void 0) === (b.status || void 0) && (a.realm?.trim() || "") === (b.realm?.trim() || "");
}
var CAPABILITY_MISS_PREFIX = "MobKit capability missing for ";
function memorySectionOutcome(error) {
  const code = jsonRpcErrorCode(error);
  if (code === -32030) return "denied";
  if (code === -32601) return "unavailable";
  if (error instanceof Error && error.message.startsWith(CAPABILITY_MISS_PREFIX)) {
    return "denied";
  }
  return "error";
}
function buildRecordsListView(args) {
  const listed = args.paged ? args.paged.records : args.records;
  const mode = hasActiveFilter(args.filter) || args.sortMode === "utility" ? "flat" : "grouped";
  return {
    mode,
    records: args.sortMode === "utility" ? sortRecordsByUtility(listed) : listed,
    groups: mode === "grouped" ? groupRecordsByScope(listed) : [],
    cursor: args.paged ? args.paged.nextCursor : args.baseCursor,
    denied: args.paged?.denied === true
  };
}
function createMemoryRecordsPager(deps) {
  let seqCounter = 0;
  let applied = {};
  const pager = {
    appliedFilter() {
      return applied;
    },
    async applyFilter(next) {
      applied = next;
      const seq = ++seqCounter;
      if (!hasActiveFilter(next)) {
        deps.setPaged(null);
        deps.setLoading(false);
        return;
      }
      deps.setLoading(true);
      try {
        const result = await deps.query(buildRecordsQueryParams(next));
        if (seq !== seqCounter) return;
        if (result === null) {
          deps.setPaged({ records: [], nextCursor: null, denied: true });
        } else {
          deps.setPaged({
            records: result.records || [],
            nextCursor: result.next_cursor ?? null
          });
        }
      } catch {
      } finally {
        if (seq === seqCounter) deps.setLoading(false);
      }
    },
    /// Blur-path re-apply: only when the value actually changed. A blur
    /// caused by clicking load-more must not re-issue the query and race
    /// the append.
    async applyFilterIfChanged(next) {
      if (filtersEquivalent(next, applied)) return;
      await pager.applyFilter(next);
    },
    async loadMore(current) {
      const cursor = current.paged ? current.paged.nextCursor : current.baseCursor;
      if (!cursor) return;
      const seq = ++seqCounter;
      deps.setLoading(true);
      try {
        const result = await deps.query(
          buildRecordsQueryParams(current.filter, { cursor })
        );
        if (seq !== seqCounter) return;
        const base = current.paged ? current.paged.records : current.baseRecords;
        if (result === null) {
          deps.setPaged({ records: base, nextCursor: null, denied: true });
        } else {
          deps.setPaged({
            records: [...base, ...result.records || []],
            nextCursor: result.next_cursor ?? null
          });
        }
      } catch {
      } finally {
        if (seq === seqCounter) deps.setLoading(false);
      }
    }
  };
  return pager;
}
var DEAD_INJECTION_THRESHOLD = 3;
function recordUtility(record) {
  const injected = record.usage?.injected_count ?? 0;
  const recalled = record.usage?.explicit_recall_count ?? 0;
  const useful = record.usage?.judged_useful_count ?? 0;
  return {
    injected,
    recalled,
    useful,
    ratio: injected > 0 ? useful / injected : null,
    bytesSpent: injected * (record.body_bytes ?? 0),
    dead: injected >= DEAD_INJECTION_THRESHOLD && useful === 0
  };
}
function sortRecordsByUtility(records) {
  return [...records].sort((a, b) => {
    const ua = recordUtility(a);
    const ub = recordUtility(b);
    if (ua.dead !== ub.dead) return ua.dead ? -1 : 1;
    const ra = ua.ratio ?? Number.POSITIVE_INFINITY;
    const rb = ub.ratio ?? Number.POSITIVE_INFINITY;
    if (ra !== rb) return ra - rb;
    return ub.bytesSpent - ua.bytesSpent;
  });
}
function utilityLine(record) {
  const u = recordUtility(record);
  const ratio = u.ratio === null ? "\u2014" : u.ratio.toFixed(2);
  return `inj ${u.injected} \xB7 recall ${u.recalled} \xB7 useful ${u.useful} \xB7 ratio ${ratio} \xB7 ~${formatBytes(u.bytesSpent)} spent`;
}
function formatBytes(bytes) {
  if (!Number.isFinite(bytes) || bytes <= 0) return "0B";
  if (bytes < 1024) return `${Math.round(bytes)}B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)}KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)}MB`;
}
var LATTICE_WALK_MAX_RECORDS = 2e3;
var LATTICE_WALK_PAGE_LIMIT = 200;
function latticeInvariants(records, options) {
  const llmCeilingViolations = [];
  const byId = new Map(records.map((record) => [record.id, record]));
  for (const record of records) {
    const author = record.provenance?.author?.author;
    const rank = record.trust ? TRUST_RANK[record.trust] : void 0;
    if ((author === "agent" || author === "distiller" || author === "steward") && typeof rank === "number" && rank > TRUST_RANK.agent_observed) {
      llmCeilingViolations.push({ id: record.id, realm: realmOfRecord(record) });
    }
  }
  const chainViolations = /* @__PURE__ */ new Map();
  for (const record of records) {
    const path = /* @__PURE__ */ new Set([record.id]);
    let cursor = record.supersedes;
    while (cursor) {
      if (path.has(cursor)) {
        chainViolations.set(record.id, { id: record.id, realm: realmOfRecord(record) });
        break;
      }
      const parent = byId.get(cursor);
      if (!parent) {
        if (options.complete) {
          chainViolations.set(record.id, { id: record.id, realm: realmOfRecord(record) });
        }
        break;
      }
      path.add(cursor);
      cursor = parent.supersedes;
    }
  }
  return {
    llmCeilingViolations,
    chainViolations: Array.from(chainViolations.values())
  };
}
function latticeFingerprint(records, realms, baseCursor) {
  const rows = records.map(
    (record) => `${record.id}:${record.supersedes || ""}:${record.trust}:${record.status?.status || ""}:${record.updated_at_ms || 0}`
  ).join("|");
  return `${realms.join(",")}#${baseCursor || ""}#${records.length}#${rows}`;
}
async function runLatticeWalk(fetchPage, options) {
  const max = options.maxRecords ?? LATTICE_WALK_MAX_RECORDS;
  const isCancelled = options.isCancelled || (() => false);
  const realmParams = options.realms.length > 1 ? options.realms : [void 0];
  const all = [];
  let exhaustedEverywhere = true;
  for (const realm of realmParams) {
    let cursor;
    for (; ; ) {
      if (isCancelled()) return null;
      if (all.length >= max) {
        exhaustedEverywhere = false;
        break;
      }
      const params = { limit: LATTICE_WALK_PAGE_LIMIT };
      if (realm) params.realm = realm;
      if (cursor) params.cursor = cursor;
      const page = await fetchPage(params);
      if (page === null) return null;
      all.push(...page.records || []);
      if (!page.next_cursor) break;
      cursor = page.next_cursor;
    }
    if (!exhaustedEverywhere) break;
  }
  const checked = all.slice(0, max);
  const complete = exhaustedEverywhere && all.length <= max;
  const invariants = latticeInvariants(checked, { complete });
  return {
    checked: checked.length,
    complete,
    ...invariants
  };
}
var VERDICT_EVIDENCE_MAX = 5;
var VERDICT_STATUS_LABEL = {
  holding: "HOLDING",
  degraded: "DEGRADED",
  violated: "VIOLATED",
  unverifiable: "UNVERIFIABLE",
  "no-grant": "NO GRANT"
};
function verdictStatusLabel(status) {
  return VERDICT_STATUS_LABEL[status];
}
function computeVerdictTiles(inputs) {
  const now = inputs.now ?? Date.now();
  const tiles = [];
  tiles.push({
    id: "echo-safety",
    label: "ECHO-SAFETY",
    status: "unverifiable",
    lines: ["needs mobkit/memory/panel/injections (surface 6)"],
    targetTab: "knowledge"
  });
  tiles.push({
    id: "taint-wall",
    label: "TAINT WALL",
    status: inputs.recordsDenied ? "no-grant" : "unverifiable",
    lines: inputs.recordsDenied ? ["records not readable by this principal"] : ["needs panel/proposals (surface 4)", "+ ever_quarantined field (surface 2)"],
    targetTab: "pipeline"
  });
  if (inputs.recordsDenied) {
    tiles.push({
      id: "lattice",
      label: "LATTICE",
      status: "no-grant",
      lines: ["records not readable by this principal"],
      targetTab: "records"
    });
  } else if (!inputs.lattice) {
    tiles.push({
      id: "lattice",
      label: "LATTICE",
      status: "unverifiable",
      lines: [inputs.latticeRunning ? "page-walk running\u2026" : "page-walk not run"],
      targetTab: "records"
    });
  } else {
    const walk = inputs.lattice;
    const violations = [...walk.llmCeilingViolations, ...walk.chainViolations];
    tiles.push({
      id: "lattice",
      label: "LATTICE",
      status: violations.length > 0 ? "violated" : "holding",
      lines: [
        violations.length > 0 ? `${violations.length} violation${violations.length === 1 ? "" : "s"}` : "0 violations",
        walk.complete ? `checked ${walk.checked}/${walk.checked}` : `checked first ${walk.checked} \u2014 partial (cap ${LATTICE_WALK_MAX_RECORDS})`,
        "invariant (b) needs ever_quarantined (surface 2)",
        ...inputs.latticeRunning ? ["re-checking\u2026"] : [],
        ...violations.length > VERDICT_EVIDENCE_MAX ? [`+${violations.length - VERDICT_EVIDENCE_MAX} more violations`] : []
      ],
      targetTab: "records",
      evidence: violations.slice(0, VERDICT_EVIDENCE_MAX)
    });
  }
  if (inputs.recordsDenied) {
    tiles.push({
      id: "recall",
      label: "RECALL",
      status: "no-grant",
      lines: ["records not readable by this principal"],
      targetTab: "records"
    });
  } else {
    const dead = inputs.records.filter((record) => recordUtility(record).dead);
    const deadBytes = dead.reduce((sum, record) => sum + recordUtility(record).bytesSpent, 0);
    tiles.push({
      id: "recall",
      label: "RECALL",
      status: dead.length > 0 ? "degraded" : "holding",
      lines: [
        `${dead.length} dead weight of ${inputs.records.length} loaded`,
        `~${formatBytes(deadBytes)} spent (approx)`
      ],
      targetTab: "records"
    });
  }
  if (inputs.dreamsDenied) {
    tiles.push({
      id: "dreams",
      label: "DREAMS",
      status: "no-grant",
      lines: ["dream audit not readable by this principal"],
      targetTab: "dreams"
    });
  } else if (inputs.dreams.length === 0) {
    tiles.push({
      id: "dreams",
      label: "DREAMS",
      status: "unverifiable",
      lines: ["no dream runs in the durable audit yet"],
      targetTab: "dreams"
    });
  } else {
    const lastOp = Math.max(
      ...inputs.dreams.map((run) => run.last_op_at_ms || run.first_op_at_ms || 0)
    );
    const quarantined = inputs.dreams.reduce((sum, run) => sum + (run.quarantined_ops || 0), 0);
    tiles.push({
      id: "dreams",
      label: "DREAMS",
      status: "holding",
      lines: [
        `last run ${relativeAge(lastOp, now)}`,
        quarantined > 0 ? `\u26A0 ${quarantined} quarantined ops` : "0 quarantined ops",
        "verdict sheet needs persisted DreamRun (surface 11)"
      ],
      targetTab: "dreams"
    });
  }
  if (inputs.overviewDenied) {
    tiles.push({
      id: "store-floor",
      label: "STORE FLOOR",
      status: "no-grant",
      lines: ["store overview not readable by this principal"],
      targetTab: "holdings"
    });
  } else if (!inputs.overview) {
    tiles.push({
      id: "store-floor",
      label: "STORE FLOOR",
      status: "unverifiable",
      lines: ["needs mobkit/memory/panel/overview (surface 1)"],
      targetTab: "holdings"
    });
  } else {
    const floor = storeFloorVerdict(inputs.overview.scopes);
    const floors = inputs.overview.floors;
    const floorLine = floors ? `floors ${floors.records ?? "?"} records / ${typeof floors.bytes === "number" ? formatBytes(floors.bytes) : "?"} per scope` : "floors unreported";
    tiles.push({
      id: "store-floor",
      label: "STORE FLOOR",
      status: floor.status === "ok" ? "holding" : "degraded",
      lines: floor.status === "ok" ? [
        `OK \u2014 no scope at floor pressure (${inputs.overview.scopes.length} scopes)`,
        floorLine
      ] : [
        `PRESSURE \u2014 ${floor.pressured.length} scope${floor.pressured.length === 1 ? "" : "s"} at floor`,
        floor.pressured.map((scope) => overviewScopeLabel(scope)).join(" \xB7 "),
        floorLine
      ],
      targetTab: "holdings"
    });
  }
  return tiles;
}
function scopeOverviewRows(records) {
  return groupRecordsByScope(records).map((group) => {
    const counts = { active: 0, quarantined: 0, superseded: 0, tombstoned: 0 };
    let bytes = 0;
    const trustCounts = /* @__PURE__ */ new Map();
    for (const record of group.records) {
      const status = record.status?.status;
      if (status && status in counts) counts[status] += 1;
      bytes += record.body_bytes ?? 0;
      const trust = trustLabel(record.trust);
      trustCounts.set(trust, (trustCounts.get(trust) || 0) + 1);
    }
    const trustMix = Array.from(trustCounts.entries()).sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0])).map(([trust, count]) => `${count} ${trust}`).join(" \xB7 ");
    return {
      key: group.key,
      label: group.label,
      scope: group.scope,
      ...counts,
      bytes,
      trustMix
    };
  });
}
function filterForScope(scope) {
  switch (scope.scope) {
    case "identity":
      return { scope: "identity", key: scope.identity };
    case "mob":
      return { scope: "mob", key: scope.mob };
    case "operator":
      return { scope: "operator", key: scope.operator };
    case "realm":
      return { scope: "realm" };
  }
}
function overviewScopeKey(scope) {
  if (scope.scope_kind === "realm") return `realm:${scope.realm}`;
  return `${scope.scope_kind}:${scope.realm}:${scope.scope_key}`;
}
function overviewScopeLabel(scope) {
  switch (scope.scope_kind) {
    case "identity":
      return scope.scope_key;
    case "mob":
      return `Mob: ${scope.scope_key}`;
    case "operator":
      return `Operator: ${scope.scope_key}`;
    case "realm":
      return "Realm";
    default:
      return `${scope.scope_kind}:${scope.scope_key}`;
  }
}
function filterForOverviewScope(scope) {
  if (scope.scope_kind === "realm") return { scope: "realm" };
  if (scope.scope_kind === "identity" || scope.scope_kind === "mob" || scope.scope_kind === "operator") {
    return { scope: scope.scope_kind, key: scope.scope_key };
  }
  return {};
}
function visibleOverviewScopes(scopes, denied) {
  return scopes.filter((scope) => {
    if (scope.scope_kind === "operator" && denied.operatorScopeDenied) return false;
    if (scope.scope_kind === "mob" && denied.mobScopeDenied) return false;
    return true;
  });
}
function sortOverviewScopes(scopes) {
  const rank = (kind) => {
    switch (kind) {
      case "identity":
        return 0;
      case "mob":
        return 1;
      case "operator":
        return 2;
      case "realm":
        return 3;
      default:
        return 4;
    }
  };
  return [...scopes].sort((a, b) => {
    const delta = rank(a.scope_kind) - rank(b.scope_kind);
    if (delta !== 0) return delta;
    return overviewScopeLabel(a).localeCompare(overviewScopeLabel(b));
  });
}
function storeFloorVerdict(scopes) {
  const pressured = scopes.filter((scope) => scope.floor_pressure === true);
  return { status: pressured.length > 0 ? "pressure" : "ok", pressured };
}
function annotateInjectionDups(entries) {
  const flags = new Array(entries.length).fill(false);
  const previousByIdentity = /* @__PURE__ */ new Map();
  for (let index = entries.length - 1; index >= 0; index -= 1) {
    const entry = entries[index];
    flags[index] = previousByIdentity.get(entry.identity) === entry.record_id;
    previousByIdentity.set(entry.identity, entry.record_id);
  }
  return entries.map((entry, index) => ({ entry, dup: flags[index] }));
}
function dreamRunsNewestFirst(runs) {
  return [...runs].sort(
    (a, b) => (b.completed_at_ms ?? b.started_at_ms ?? 0) - (a.completed_at_ms ?? a.started_at_ms ?? 0)
  );
}
function formatDurationMs(ms) {
  if (!Number.isFinite(ms) || ms < 0) return "\u2014";
  if (ms < 1e3) return `${Math.round(ms)}ms`;
  if (ms < 60 * 1e3) return `${(ms / 1e3).toFixed(1)}s`;
  const minutes = Math.floor(ms / (60 * 1e3));
  const seconds = Math.round(ms % (60 * 1e3) / 1e3);
  return `${minutes}m ${seconds}s`;
}
function dreamRunDuration(run) {
  if (!run.started_at_ms || !run.completed_at_ms) return "\u2014";
  return formatDurationMs(run.completed_at_ms - run.started_at_ms);
}
function normalizeDreamRunDetail(detail) {
  if (detail === void 0 || detail === null) {
    return { phases: [], verdicts: [], skips: [], raw: null };
  }
  if (typeof detail === "string") {
    return { phases: [], verdicts: [], skips: [], raw: detail };
  }
  const phases = [];
  for (const phase of detail.phases || []) {
    if (Array.isArray(phase) && phase.length >= 1) {
      phases.push([String(phase[0]), String(phase[1] ?? "")]);
    }
  }
  const verdicts = Object.entries(detail.verdicts || {}).filter(
    (candidate) => typeof candidate[1] === "number" && candidate[1] > 0
  );
  const skips = (detail.skips || []).map((skip) => String(skip));
  return { phases, verdicts, skips, raw: null };
}
function lineageLane(chain, currentId) {
  return [...chain].reverse().map((record) => ({
    record,
    current: record.id === currentId
  }));
}
function dreamRunsTouching(dreams, recordId) {
  return dreams.filter((run) => (run.memory_ids || []).includes(recordId));
}
function evidenceExcerptLines(entries, range, maxLines = 30) {
  let window2 = entries;
  if (range && range.length === 2) {
    const [start, end] = range;
    if (Number.isFinite(start) && Number.isFinite(end) && end >= start) {
      const from = Math.max(0, Math.min(start, entries.length));
      const to = Math.max(from, Math.min(end + 1, entries.length));
      const sliced = entries.slice(from, to);
      if (sliced.length > 0) window2 = sliced;
    }
  }
  const lines = [];
  for (const entry of window2) {
    if (entry.kind !== "message") continue;
    const text = (entry.text || entry.copyText || "").trim();
    if (!text) continue;
    lines.push({ id: entry.id, speaker: entry.identity.label, text });
    if (lines.length >= maxLines) break;
  }
  return lines;
}
function identityOptions(records) {
  const identities = /* @__PURE__ */ new Set();
  for (const record of records) {
    if (record.scope.scope === "identity") identities.add(record.scope.identity);
  }
  return Array.from(identities).sort((a, b) => a.localeCompare(b));
}
function knowledgeComposition(records, identity) {
  const count = (predicate) => records.filter(predicate).length;
  return [
    {
      label: `identity:${identity}`,
      count: count(
        (record) => record.scope.scope === "identity" && record.scope.identity === identity
      ),
      filter: { scope: "identity", key: identity },
      approximate: false
    },
    {
      label: "mob (all mobs)",
      count: count((record) => record.scope.scope === "mob"),
      filter: { scope: "mob" },
      approximate: true
    },
    {
      label: "operator",
      count: count((record) => record.scope.scope === "operator"),
      filter: { scope: "operator" },
      approximate: true
    },
    {
      label: "realm",
      count: count((record) => record.scope.scope === "realm"),
      filter: { scope: "realm" },
      approximate: true
    }
  ];
}
function dedupeFramesById(frames) {
  const seen = /* @__PURE__ */ new Set();
  const result = [];
  for (const frame of frames) {
    const key = frame.id || `${frame.event}:${frame.timestampMs || 0}`;
    if (seen.has(key)) continue;
    seen.add(key);
    result.push(frame);
  }
  return result;
}
function countFramesBehind(live, frozen) {
  const frozenIds = new Set(frozen.map((frame) => frame.id));
  return live.filter((frame) => !frozenIds.has(frame.id)).length;
}
function memoryFramePivot(frame) {
  if (!frame.event.startsWith("memory.")) return null;
  const data = frame.data && typeof frame.data === "object" ? frame.data : {};
  const recordId = typeof data.record_id === "string" && data.record_id.trim() ? data.record_id.trim() : null;
  if (!recordId) return null;
  const realm = typeof data.realm === "string" && data.realm.trim() ? data.realm.trim() : void 0;
  return { recordId, realm };
}
function Chip({ label, tone }) {
  return /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("span", { className: "chip memory-chip", "data-tone": tone || "neutral", children: label });
}
function SectionNote({
  children,
  testid
}) {
  return /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("div", { className: "memory-note", "data-testid": testid, children });
}
function RecordRow({
  record,
  utilityMode,
  onSelect
}) {
  const utility = utilityMode ? recordUtility(record) : null;
  return /* @__PURE__ */ (0, import_jsx_runtime31.jsxs)(
    "button",
    {
      type: "button",
      className: "memory-row",
      "data-testid": `memory-record:${record.id}`,
      onClick: onSelect,
      children: [
        /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("span", { className: "memory-row__title", children: record.title || record.id }),
        /* @__PURE__ */ (0, import_jsx_runtime31.jsxs)("span", { className: "memory-row__meta", children: [
          /* @__PURE__ */ (0, import_jsx_runtime31.jsx)(Chip, { label: record.kind }),
          /* @__PURE__ */ (0, import_jsx_runtime31.jsx)(Chip, { label: trustLabel(record.trust), tone: trustTone(record.trust) }),
          /* @__PURE__ */ (0, import_jsx_runtime31.jsx)(Chip, { label: statusLabel(record.status), tone: statusTone(record.status) }),
          utility?.dead ? /* @__PURE__ */ (0, import_jsx_runtime31.jsx)(Chip, { label: "DEAD", tone: "warning" }) : null,
          /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("span", { className: "memory-row__age", children: relativeAge(record.updated_at_ms) })
        ] }),
        utility ? /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("span", { className: "memory-row__meta memory-row__utility", children: utilityLine(record) }) : null
      ]
    }
  );
}
function evidenceKey(evidence, index) {
  return `${index}:${evidence.session_id || ""}:${evidence.generation ?? ""}`;
}
function BiographyView({
  detail,
  dreams,
  onBack,
  onSelectRecord,
  onLoadEvidence
}) {
  const { record, chain, injections } = detail;
  const provenance = record.provenance;
  const evidence = provenance?.evidence || [];
  const verification = provenance?.verification;
  const usage = record.usage;
  const lane = lineageLane(chain, record.id);
  const touchingRuns = dreamRunsTouching(dreams, record.id);
  const [evidenceState, setEvidenceState] = import_react23.default.useState(null);
  const evidenceSeqRef = import_react23.default.useRef(0);
  const recordIdentity = record.scope.scope === "identity" ? record.scope.identity : provenance?.author?.author === "agent" ? provenance.author.identity : void 0;
  async function openEvidence(ref, index) {
    if (!onLoadEvidence) return;
    const key = evidenceKey(ref, index);
    const seq = ++evidenceSeqRef.current;
    setEvidenceState({ key, status: "loading", lines: [] });
    const entries = await onLoadEvidence(recordIdentity, ref);
    if (seq !== evidenceSeqRef.current) return;
    const lines = entries ? evidenceExcerptLines(entries, ref.range) : [];
    setEvidenceState({
      key,
      status: entries === null ? "not-found" : lines.length > 0 ? "loaded" : "empty-range",
      lines
    });
  }
  return /* @__PURE__ */ (0, import_jsx_runtime31.jsxs)("div", { className: "memory-detail", "data-testid": "memory-detail", children: [
    /* @__PURE__ */ (0, import_jsx_runtime31.jsxs)("div", { className: "memory-detail__head", children: [
      /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("button", { type: "button", className: "memory-back", onClick: onBack, "data-testid": "memory-detail-back", children: "\u2190 Back" }),
      /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("h3", { children: record.title || record.id }),
      /* @__PURE__ */ (0, import_jsx_runtime31.jsxs)("span", { className: "memory-detail__chips", children: [
        /* @__PURE__ */ (0, import_jsx_runtime31.jsx)(Chip, { label: record.kind }),
        /* @__PURE__ */ (0, import_jsx_runtime31.jsx)(Chip, { label: trustLabel(record.trust), tone: trustTone(record.trust) }),
        /* @__PURE__ */ (0, import_jsx_runtime31.jsx)(Chip, { label: statusLabel(record.status), tone: statusTone(record.status) })
      ] }),
      /* @__PURE__ */ (0, import_jsx_runtime31.jsx)(
        CopyButton,
        {
          text: JSON.stringify(record, null, 2),
          label: "Copy record JSON",
          className: "memory-copy-json"
        }
      )
    ] }),
    record.description ? /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("p", { className: "memory-detail__description", children: record.description }) : null,
    /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("pre", { className: "memory-detail__body", "data-testid": "memory-detail-body", children: record.body }),
    record.tags && record.tags.length > 0 ? /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("div", { className: "memory-detail__tags", children: record.tags.map((tag) => /* @__PURE__ */ (0, import_jsx_runtime31.jsx)(Chip, { label: tag, tone: "muted" }, tag)) }) : null,
    /* @__PURE__ */ (0, import_jsx_runtime31.jsxs)("div", { className: "memory-detail__section", "data-testid": "memory-detail-born", children: [
      /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("span", { className: "memory-detail__label", children: "Born" }),
      /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("div", { className: "memory-detail__line", children: authorLine(provenance?.author) }),
      evidence.length > 0 ? /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("div", { className: "memory-evidence", children: evidence.map((ref, index) => /* @__PURE__ */ (0, import_jsx_runtime31.jsx)(
        "button",
        {
          type: "button",
          className: "memory-evidence__ref",
          "data-testid": `memory-evidence:${index}`,
          onClick: () => void openEvidence(ref, index),
          disabled: !onLoadEvidence,
          title: onLoadEvidence ? "Open transcript window" : void 0,
          children: evidenceLabel(ref)
        },
        `ev-${index}`
      )) }) : null,
      evidenceState ? evidenceState.status === "loading" ? /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("div", { className: "memory-detail__line", children: "Loading transcript\u2026" }) : evidenceState.status === "not-found" ? /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("div", { className: "memory-detail__line", "data-testid": "memory-evidence-degraded", children: "Session not found in the recent timeline window \u2014 evidence reference retained as label only." }) : evidenceState.status === "empty-range" ? /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("div", { className: "memory-detail__line", "data-testid": "memory-evidence-empty", children: "Session found, but no message entries in the evidence range \u2014 the window is approximate against the console timeline." }) : /* @__PURE__ */ (0, import_jsx_runtime31.jsxs)("div", { className: "memory-excerpt", "data-testid": "memory-evidence-excerpt", children: [
        /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("div", { className: "memory-detail__line memory-excerpt__note", children: "Approximate window against the console timeline (evidence indexes a session generation)." }),
        evidenceState.lines.map((line) => /* @__PURE__ */ (0, import_jsx_runtime31.jsxs)("div", { className: "memory-excerpt__line", children: [
          /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("span", { className: "memory-excerpt__speaker", children: line.speaker }),
          /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("span", { className: "memory-excerpt__text", children: line.text })
        ] }, line.id))
      ] }) : null,
      verification?.checked ? /* @__PURE__ */ (0, import_jsx_runtime31.jsxs)("div", { className: "memory-detail__line memory-detail__verification", children: [
        "verified: ",
        verification.checked
      ] }) : null
    ] }),
    lane.length > 0 ? /* @__PURE__ */ (0, import_jsx_runtime31.jsxs)("div", { className: "memory-detail__section", "data-testid": "memory-detail-lineage", children: [
      /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("span", { className: "memory-detail__label", children: "Lineage" }),
      /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("div", { className: "memory-chain", children: lane.map(({ record: entry, current }) => /* @__PURE__ */ (0, import_jsx_runtime31.jsxs)(
        "button",
        {
          type: "button",
          className: "memory-chain__row",
          "data-current": current ? "true" : void 0,
          "data-dimmed": current ? void 0 : "true",
          "data-testid": `memory-chain:${entry.id}`,
          onClick: () => {
            if (!current) onSelectRecord(realmOfRecord(entry), entry.id);
          },
          children: [
            /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("span", { className: "memory-chain__marker", children: current ? "\u25CF" : "\u25CB" }),
            /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("span", { className: "memory-chain__title", children: entry.title || entry.id }),
            /* @__PURE__ */ (0, import_jsx_runtime31.jsx)(Chip, { label: trustLabel(entry.trust), tone: trustTone(entry.trust) }),
            /* @__PURE__ */ (0, import_jsx_runtime31.jsx)(Chip, { label: statusLabel(entry.status), tone: statusTone(entry.status) })
          ]
        },
        entry.id
      )) })
    ] }) : null,
    /* @__PURE__ */ (0, import_jsx_runtime31.jsxs)("div", { className: "memory-detail__section", "data-testid": "memory-detail-life", children: [
      /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("span", { className: "memory-detail__label", children: "Life" }),
      usage ? /* @__PURE__ */ (0, import_jsx_runtime31.jsxs)("div", { className: "memory-detail__line", children: [
        "injected ",
        usage.injected_count ?? 0,
        " \xB7 recalled ",
        usage.explicit_recall_count ?? 0,
        " \xB7 judged useful ",
        usage.judged_useful_count ?? 0,
        usage.last_injected_at_ms ? ` \xB7 last injected ${relativeAge(usage.last_injected_at_ms)}` : ""
      ] }) : /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("div", { className: "memory-detail__line", children: "no usage recorded" }),
      injections.length > 0 ? injections.map((injection, index) => /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("div", { className: "memory-detail__line", children: injectionLine(injection) }, `inj-${index}`)) : /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("div", { className: "memory-detail__line", children: "no injections recorded for this record" })
    ] }),
    /* @__PURE__ */ (0, import_jsx_runtime31.jsxs)("div", { className: "memory-detail__section", "data-testid": "memory-detail-dreams", children: [
      /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("span", { className: "memory-detail__label", children: "Dreams" }),
      touchingRuns.length > 0 ? touchingRuns.map((run) => /* @__PURE__ */ (0, import_jsx_runtime31.jsxs)("div", { className: "memory-detail__line", children: [
        run.run_id,
        " \xB7 ",
        dreamTimeRange(run),
        run.quarantined_ops ? ` \xB7 \u26A0 ${run.quarantined_ops} quarantined` : ""
      ] }, run.run_id)) : /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("div", { className: "memory-detail__line", children: "no sampled dream runs reference this record (sample is \u226412 ids per run \u2014 exact history needs the record history[] surface)" })
    ] })
  ] });
}
function VerdictStrip({
  tiles,
  onOpen,
  onOpenRecord
}) {
  return /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("div", { className: "memory-tiles", "data-testid": "memory-verdict-strip", children: tiles.map((tile) => /* @__PURE__ */ (0, import_jsx_runtime31.jsxs)(
    "div",
    {
      className: "memory-tile",
      role: "button",
      tabIndex: 0,
      "data-status": tile.status,
      "data-testid": `memory-verdict:${tile.id}`,
      onClick: () => onOpen(tile),
      onKeyDown: (event) => {
        if (event.key === "Enter" || event.key === " ") onOpen(tile);
      },
      children: [
        /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("span", { className: "memory-tile__label", children: tile.label }),
        /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("span", { className: "memory-tile__status", "data-status": tile.status, children: verdictStatusLabel(tile.status) }),
        tile.lines.map((line, index) => /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("span", { className: "memory-tile__line", children: line }, `l-${index}`)),
        (tile.evidence || []).map((violation) => /* @__PURE__ */ (0, import_jsx_runtime31.jsx)(
          "button",
          {
            type: "button",
            className: "memory-tile__evidence",
            "data-testid": `memory-verdict-evidence:${tile.id}:${violation.id}`,
            onClick: (event) => {
              event.stopPropagation();
              onOpenRecord(violation.realm, violation.id);
            },
            children: violation.id
          },
          violation.id
        ))
      ]
    },
    tile.id
  )) });
}
function MemoryLiveStrip({
  frames,
  onPivot
}) {
  const deduped = import_react23.default.useMemo(() => dedupeFramesById(frames), [frames]);
  const [frozen, setFrozen] = import_react23.default.useState(null);
  const listRef = import_react23.default.useRef(null);
  const shown = frozen ?? deduped;
  const behind = frozen ? countFramesBehind(deduped, frozen) : 0;
  function handleScroll() {
    const el = listRef.current;
    if (!el) return;
    if (el.scrollTop > 4) {
      setFrozen((current) => current ?? deduped);
    } else if (frozen && behind === 0) {
      setFrozen(null);
    }
  }
  function jumpToLive() {
    setFrozen(null);
    listRef.current?.scrollTo?.({ top: 0 });
  }
  return /* @__PURE__ */ (0, import_jsx_runtime31.jsxs)("div", { className: "memory-group memory-live", "data-testid": "memory-live-strip", children: [
    /* @__PURE__ */ (0, import_jsx_runtime31.jsxs)("div", { className: "memory-group__label", children: [
      "Live memory events (in-memory ring \u2014 lossy)",
      behind > 0 ? /* @__PURE__ */ (0, import_jsx_runtime31.jsxs)(
        "button",
        {
          type: "button",
          className: "memory-live__jump",
          "data-testid": "memory-live-jump",
          onClick: jumpToLive,
          children: [
            behind,
            " behind \xB7 jump to live"
          ]
        }
      ) : null
    ] }),
    /* @__PURE__ */ (0, import_jsx_runtime31.jsxs)("div", { className: "memory-live__list", ref: listRef, onScroll: handleScroll, children: [
      shown.length === 0 ? /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("div", { className: "memory-detail__line", children: "No memory events in the ring." }) : shown.map((frame) => {
        const data = frame.data && typeof frame.data === "object" ? frame.data : {};
        const pivot = memoryFramePivot(frame);
        return /* @__PURE__ */ (0, import_jsx_runtime31.jsxs)("div", { className: "memory-live__row", "data-testid": `memory-live-row:${frame.id}`, children: [
          /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("span", { className: "memory-row__age", children: relativeAge(frame.timestampMs) }),
          /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("span", { className: "memory-live__text", children: describeMemoryTimelineEvent2(frame.event, data) }),
          pivot ? /* @__PURE__ */ (0, import_jsx_runtime31.jsx)(
            "button",
            {
              type: "button",
              className: "memory-live__pivot",
              "data-testid": `memory-live-pivot:${frame.id}`,
              onClick: () => onPivot(pivot.realm, pivot.recordId),
              children: "state here"
            }
          ) : null
        ] }, frame.id);
      }),
      /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("div", { className: "memory-live__seam", "data-testid": "memory-live-seam", children: "\u2014 ring history starts here \u2014" })
    ] })
  ] });
}
function MemoryPanel({
  records,
  realms,
  quarantineRecords,
  pendingPromotions,
  dreams,
  detail,
  detailLoading = false,
  canReviewQuarantine = false,
  unavailable = false,
  error,
  nextCursor = null,
  recordsDenied = false,
  dreamsDenied = false,
  operatorScopeDenied = false,
  mobScopeDenied = false,
  overview = null,
  overviewDenied = false,
  proposals = [],
  proposalsDenied = false,
  injections = [],
  injectionsDenied = false,
  harvests = [],
  harvestsDenied = false,
  dreamRuns = [],
  dreamRunsDenied = false,
  auditVerdicts = [],
  auditVerdictsDenied = false,
  liveFrames = [],
  onRefresh,
  onSelectRecord,
  onClearDetail,
  onQueryRecords,
  onLoadEvidence,
  onOpenGating
}) {
  const [tab, setTab] = import_react23.default.useState("holdings");
  const [filter, setFilter] = import_react23.default.useState({});
  const [sortMode, setSortMode] = import_react23.default.useState("recency");
  const [paged, setPaged] = import_react23.default.useState(null);
  const [pageLoading, setPageLoading] = import_react23.default.useState(false);
  const queryRecordsRef = import_react23.default.useRef(onQueryRecords);
  queryRecordsRef.current = onQueryRecords;
  const pagerRef = import_react23.default.useRef(null);
  if (!pagerRef.current) {
    pagerRef.current = createMemoryRecordsPager({
      query: (params) => queryRecordsRef.current ? queryRecordsRef.current(params) : Promise.resolve(null),
      setPaged,
      setLoading: setPageLoading
    });
  }
  const pager = pagerRef.current;
  const [lattice, setLattice] = import_react23.default.useState(null);
  const [latticeRunning, setLatticeRunning] = import_react23.default.useState(false);
  const [knowledgeIdentity, setKnowledgeIdentity] = import_react23.default.useState("");
  import_react23.default.useEffect(() => {
    if (detail) setTab("records");
  }, [detail]);
  const latticeRanForRef = import_react23.default.useRef(null);
  import_react23.default.useEffect(() => {
    if (tab !== "holdings" || !onQueryRecords || recordsDenied) return;
    const fingerprint = latticeFingerprint(records, realms, nextCursor);
    if (latticeRanForRef.current === fingerprint) return;
    latticeRanForRef.current = fingerprint;
    setLatticeRunning(true);
    let cancelled = false;
    void runLatticeWalk((params) => onQueryRecords(params), {
      realms,
      isCancelled: () => cancelled
    }).then((result) => {
      if (!cancelled) setLattice(result);
    }).catch(() => {
      if (!cancelled) setLattice(null);
    }).finally(() => {
      if (!cancelled) setLatticeRunning(false);
    });
    return () => {
      cancelled = true;
      if (latticeRanForRef.current === fingerprint) {
        latticeRanForRef.current = null;
      }
    };
  }, [tab, records, recordsDenied, realms, nextCursor]);
  const overviewRows = import_react23.default.useMemo(() => scopeOverviewRows(records), [records]);
  const overviewScopes = import_react23.default.useMemo(
    () => overview ? sortOverviewScopes(
      visibleOverviewScopes(overview.scopes || [], {
        operatorScopeDenied,
        mobScopeDenied
      })
    ) : null,
    [overview, operatorScopeDenied, mobScopeDenied]
  );
  const tiles = import_react23.default.useMemo(
    () => computeVerdictTiles({
      records,
      recordsDenied,
      dreams,
      dreamsDenied,
      lattice,
      latticeRunning,
      overview: overviewScopes && overview ? { scopes: overviewScopes, floors: overview.floors } : null,
      overviewDenied
    }),
    [
      records,
      recordsDenied,
      dreams,
      dreamsDenied,
      lattice,
      latticeRunning,
      overview,
      overviewScopes,
      overviewDenied
    ]
  );
  const annotatedInjections = import_react23.default.useMemo(
    () => annotateInjectionDups(injections),
    [injections]
  );
  const dreamSheets = import_react23.default.useMemo(() => dreamRunsNewestFirst(dreamRuns), [dreamRuns]);
  const [expandedRuns, setExpandedRuns] = import_react23.default.useState({});
  const identities = import_react23.default.useMemo(() => identityOptions(records), [records]);
  const selectedIdentity = knowledgeIdentity || identities[0] || "";
  const memoryFrames = import_react23.default.useMemo(
    () => liveFrames.filter((frame) => frame.event.startsWith("memory.")),
    [liveFrames]
  );
  function applyFilter(next) {
    setFilter(next);
    if (!onQueryRecords) return;
    void pager.applyFilter(next);
  }
  function loadMore() {
    if (!onQueryRecords) return;
    void pager.loadMore({ filter, paged, baseRecords: records, baseCursor: nextCursor });
  }
  function openRecordsFiltered(next) {
    setTab("records");
    onClearDetail();
    applyFilter(next);
  }
  function openTile(tile) {
    if (tile.id === "recall") setSortMode("utility");
    setTab(tile.targetTab);
  }
  if (unavailable) {
    return /* @__PURE__ */ (0, import_jsx_runtime31.jsxs)("div", { className: "gating memory-panel", "data-testid": "memory-panel", children: [
      /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("div", { className: "gating__head", children: /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("h2", { children: "Memory" }) }),
      /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("div", { className: "gating__empty", "data-testid": "memory-unavailable", children: "The memory panel is not configured on this runtime." })
    ] });
  }
  const listView = buildRecordsListView({
    records,
    paged,
    baseCursor: nextCursor,
    filter,
    sortMode
  });
  const quarantineCount = quarantineRecords.length + pendingPromotions.length;
  return /* @__PURE__ */ (0, import_jsx_runtime31.jsxs)("div", { className: "gating memory-panel", "data-testid": "memory-panel", children: [
    /* @__PURE__ */ (0, import_jsx_runtime31.jsxs)("div", { className: "gating__head", children: [
      /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("h2", { children: "Memory" }),
      /* @__PURE__ */ (0, import_jsx_runtime31.jsxs)("p", { children: [
        records.length,
        " records",
        realms.length > 1 ? ` \xB7 ${realms.length} realms` : ""
      ] })
    ] }),
    error ? /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("div", { className: "gating__empty", "data-testid": "memory-error", children: error }) : null,
    /* @__PURE__ */ (0, import_jsx_runtime31.jsxs)("div", { className: "gating__tabs", children: [
      MEMORY_TABS.map((candidate) => /* @__PURE__ */ (0, import_jsx_runtime31.jsxs)(
        "button",
        {
          className: `gating__tab ${tab === candidate ? "is-active" : ""}`,
          onClick: () => setTab(candidate),
          "data-testid": `memory-tab:${candidate}`,
          children: [
            memoryTabLabel(candidate),
            candidate === "pipeline" && quarantineCount > 0 ? /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("span", { className: "n", children: quarantineCount }) : null,
            candidate === "dreams" && dreams.length > 0 ? /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("span", { className: "n", children: dreams.length }) : null
          ]
        },
        candidate
      )),
      /* @__PURE__ */ (0, import_jsx_runtime31.jsx)(
        "button",
        {
          className: "gating__tab memory-tab-alias",
          onClick: () => setTab(resolveMemoryTabAlias("quarantine") || "pipeline"),
          "data-testid": "memory-tab:quarantine",
          "aria-hidden": "true",
          tabIndex: -1,
          children: "Quarantine"
        }
      ),
      /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("button", { className: "gating__tab", onClick: onRefresh, "data-testid": "memory-refresh", children: "Refresh" })
    ] }),
    /* @__PURE__ */ (0, import_jsx_runtime31.jsxs)("div", { className: "gating__list memory-panel__body", children: [
      tab === "holdings" ? /* @__PURE__ */ (0, import_jsx_runtime31.jsxs)("div", { className: "memory-groups", "data-testid": "memory-holdings", children: [
        /* @__PURE__ */ (0, import_jsx_runtime31.jsx)(
          VerdictStrip,
          {
            tiles,
            onOpen: openTile,
            onOpenRecord: (realm, memoryId) => onSelectRecord(realm, memoryId)
          }
        ),
        recordsDenied ? /* @__PURE__ */ (0, import_jsx_runtime31.jsx)(SectionNote, { testid: "memory-holdings-denied", children: "Records are not readable by this principal (access denied)." }) : /* @__PURE__ */ (0, import_jsx_runtime31.jsxs)("div", { className: "memory-group", children: [
          /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("div", { className: "memory-group__label", children: overviewScopes ? `Scopes \u2014 store totals (panel/overview)${overview?.floors ? ` \xB7 floors ${overview.floors.records ?? "?"} records / ${typeof overview.floors.bytes === "number" ? formatBytes(overview.floors.bytes) : "?"} per scope` : ""}` : `Scopes \u2014 counts over the ${records.length} loaded records (full totals need panel/overview)` }),
          overviewScopes ? overviewScopes.length === 0 && !operatorScopeDenied && !mobScopeDenied ? /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("div", { className: "gating__empty", children: "No memory records yet." }) : overviewScopes.map((scope) => {
            const key = overviewScopeKey(scope);
            return /* @__PURE__ */ (0, import_jsx_runtime31.jsxs)(
              "button",
              {
                type: "button",
                className: "memory-row memory-scope-row",
                "data-testid": `memory-holdings-scope:${key}`,
                onClick: () => openRecordsFiltered(filterForOverviewScope(scope)),
                children: [
                  /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("span", { className: "memory-row__title", children: overviewScopeLabel(scope) }),
                  /* @__PURE__ */ (0, import_jsx_runtime31.jsxs)("span", { className: "memory-row__meta", children: [
                    /* @__PURE__ */ (0, import_jsx_runtime31.jsx)(Chip, { label: `${scope.active ?? 0} active`, tone: "positive" }),
                    (scope.quarantined ?? 0) > 0 ? /* @__PURE__ */ (0, import_jsx_runtime31.jsx)(Chip, { label: `${scope.quarantined} quarantined`, tone: "warning" }) : null,
                    (scope.superseded ?? 0) > 0 ? /* @__PURE__ */ (0, import_jsx_runtime31.jsx)(Chip, { label: `${scope.superseded} superseded`, tone: "muted" }) : null,
                    (scope.tombstoned ?? 0) > 0 ? /* @__PURE__ */ (0, import_jsx_runtime31.jsx)(Chip, { label: `${scope.tombstoned} tombstoned`, tone: "muted" }) : null,
                    scope.floor_pressure ? /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("span", { "data-testid": `memory-holdings-floor:${key}`, children: /* @__PURE__ */ (0, import_jsx_runtime31.jsx)(Chip, { label: "FLOOR PRESSURE", tone: "warning" }) }) : null,
                    /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("span", { className: "memory-row__age", children: formatBytes(scope.body_bytes ?? 0) })
                  ] })
                ]
              },
              key
            );
          }) : overviewRows.length === 0 && !operatorScopeDenied && !mobScopeDenied ? /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("div", { className: "gating__empty", children: "No memory records yet." }) : overviewRows.map((row) => /* @__PURE__ */ (0, import_jsx_runtime31.jsxs)(
            "button",
            {
              type: "button",
              className: "memory-row memory-scope-row",
              "data-testid": `memory-holdings-scope:${row.key}`,
              onClick: () => openRecordsFiltered(filterForScope(row.scope)),
              children: [
                /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("span", { className: "memory-row__title", children: row.label }),
                /* @__PURE__ */ (0, import_jsx_runtime31.jsxs)("span", { className: "memory-row__meta", children: [
                  /* @__PURE__ */ (0, import_jsx_runtime31.jsx)(Chip, { label: `${row.active} active`, tone: "positive" }),
                  row.quarantined > 0 ? /* @__PURE__ */ (0, import_jsx_runtime31.jsx)(Chip, { label: `${row.quarantined} quarantined`, tone: "warning" }) : null,
                  row.superseded > 0 ? /* @__PURE__ */ (0, import_jsx_runtime31.jsx)(Chip, { label: `${row.superseded} superseded`, tone: "muted" }) : null,
                  row.tombstoned > 0 ? /* @__PURE__ */ (0, import_jsx_runtime31.jsx)(Chip, { label: `${row.tombstoned} tombstoned`, tone: "muted" }) : null,
                  /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("span", { className: "memory-row__age", children: formatBytes(row.bytes) })
                ] }),
                row.trustMix ? /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("span", { className: "memory-row__meta memory-row__reason", children: row.trustMix }) : null
              ]
            },
            row.key
          )),
          mobScopeDenied ? /* @__PURE__ */ (0, import_jsx_runtime31.jsxs)(
            "div",
            {
              className: "memory-row memory-row--static memory-scope-row",
              "data-testid": "memory-holdings-scope-denied:mob",
              children: [
                /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("span", { className: "memory-row__title", children: "Mob scopes" }),
                /* @__PURE__ */ (0, import_jsx_runtime31.jsxs)("span", { className: "memory-row__meta", children: [
                  /* @__PURE__ */ (0, import_jsx_runtime31.jsx)(Chip, { label: "no grant", tone: "warning" }),
                  /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("span", { className: "memory-row__reason", children: "requires mob.memory.read" })
                ] })
              ]
            }
          ) : null,
          operatorScopeDenied ? /* @__PURE__ */ (0, import_jsx_runtime31.jsxs)(
            "div",
            {
              className: "memory-row memory-row--static memory-scope-row",
              "data-testid": "memory-holdings-scope-denied:operator",
              children: [
                /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("span", { className: "memory-row__title", children: "Operator scope" }),
                /* @__PURE__ */ (0, import_jsx_runtime31.jsxs)("span", { className: "memory-row__meta", children: [
                  /* @__PURE__ */ (0, import_jsx_runtime31.jsx)(Chip, { label: "no grant", tone: "warning" }),
                  /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("span", { className: "memory-row__reason", children: "requires operator.memory.read" })
                ] })
              ]
            }
          ) : null
        ] }),
        /* @__PURE__ */ (0, import_jsx_runtime31.jsxs)("div", { className: "memory-group", children: [
          /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("div", { className: "memory-group__label", children: "In transit" }),
          /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("div", { className: "memory-detail__line", children: dreams.length > 0 ? `Last dream ${dreamTimeRange(dreams[0])} \xB7 ${dreams[0].ops ?? "\u2014"} ops` : dreamsDenied ? "Dream audit: no grant" : "No dream runs recorded yet" }),
          /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("div", { className: "memory-detail__line", children: canReviewQuarantine ? `Quarantine queue ${quarantineRecords.length} \xB7 pending gate ${pendingPromotions.length}` : "Quarantine queue: requires memory.quarantine.review" }),
          /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("div", { className: "memory-detail__line", children: proposalsDenied ? "Proposals: no grant" : `Proposals: ${proposals.length} pending${proposals.filter((proposal) => proposal.tainted).length > 0 ? ` \xB7 ${proposals.filter((proposal) => proposal.tainted).length} held (taint)` : ""}` }),
          /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("div", { className: "memory-detail__line", children: "Health (taint \xB7 budgets \xB7 cursors): needs mobkit/memory/panel/health (surface 8)" })
        ] }),
        /* @__PURE__ */ (0, import_jsx_runtime31.jsxs)("div", { className: "memory-group", "data-testid": "memory-harvests", children: [
          /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("div", { className: "memory-group__label", children: "Harvest queue \u2014 retired identities awaiting the exit-interview dream" }),
          harvestsDenied ? /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("div", { className: "memory-detail__line", children: "Harvest queue: no grant." }) : harvests.length === 0 ? /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("div", { className: "memory-detail__line", children: "No pending harvests." }) : harvests.map((harvest, index) => /* @__PURE__ */ (0, import_jsx_runtime31.jsxs)(
            "div",
            {
              className: "memory-row memory-row--static",
              "data-testid": `memory-harvest:${harvest.identity}`,
              children: [
                /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("span", { className: "memory-row__title", children: harvest.identity }),
                /* @__PURE__ */ (0, import_jsx_runtime31.jsxs)("span", { className: "memory-row__meta", children: [
                  harvest.cause ? /* @__PURE__ */ (0, import_jsx_runtime31.jsx)(Chip, { label: harvest.cause, tone: "muted" }) : null,
                  harvest.session_key ? /* @__PURE__ */ (0, import_jsx_runtime31.jsxs)("span", { className: "memory-row__reason", children: [
                    "session ",
                    harvest.session_key
                  ] }) : null,
                  /* @__PURE__ */ (0, import_jsx_runtime31.jsxs)("span", { className: "memory-row__age", children: [
                    "retired ",
                    relativeAge(harvest.retired_at_ms)
                  ] })
                ] })
              ]
            },
            `${harvest.realm}:${harvest.identity}:${index}`
          ))
        ] })
      ] }) : null,
      tab === "records" ? detail ? /* @__PURE__ */ (0, import_jsx_runtime31.jsx)(
        BiographyView,
        {
          detail,
          dreams,
          onBack: onClearDetail,
          onSelectRecord,
          onLoadEvidence
        }
      ) : detailLoading ? /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("div", { className: "gating__empty", children: "Loading record\u2026" }) : /* @__PURE__ */ (0, import_jsx_runtime31.jsxs)("div", { className: "memory-groups", children: [
        onQueryRecords ? /* @__PURE__ */ (0, import_jsx_runtime31.jsxs)("div", { className: "memory-filterbar", "data-testid": "memory-filter", children: [
          /* @__PURE__ */ (0, import_jsx_runtime31.jsxs)("label", { children: [
            "scope",
            /* @__PURE__ */ (0, import_jsx_runtime31.jsxs)(
              "select",
              {
                value: filter.scope || "",
                "data-testid": "memory-filter:scope",
                onChange: (event) => applyFilter({
                  ...filter,
                  scope: event.target.value || void 0
                }),
                children: [
                  /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("option", { value: "", children: "all" }),
                  /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("option", { value: "identity", children: "identity" }),
                  /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("option", { value: "mob", children: "mob" }),
                  /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("option", { value: "operator", children: "operator" }),
                  /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("option", { value: "realm", children: "realm" })
                ]
              }
            )
          ] }),
          /* @__PURE__ */ (0, import_jsx_runtime31.jsxs)("label", { children: [
            "identity / key",
            /* @__PURE__ */ (0, import_jsx_runtime31.jsx)(
              "input",
              {
                value: filter.key || "",
                "data-testid": "memory-filter-input",
                placeholder: "identity or scope key",
                onChange: (event) => setFilter({ ...filter, key: event.target.value }),
                onKeyDown: (event) => {
                  if (event.key === "Enter") applyFilter(filter);
                },
                onBlur: () => {
                  void pager.applyFilterIfChanged(filter);
                }
              }
            )
          ] }),
          /* @__PURE__ */ (0, import_jsx_runtime31.jsxs)("label", { children: [
            "status",
            /* @__PURE__ */ (0, import_jsx_runtime31.jsxs)(
              "select",
              {
                value: filter.status || "",
                "data-testid": "memory-filter:status",
                onChange: (event) => applyFilter({
                  ...filter,
                  status: event.target.value || void 0
                }),
                children: [
                  /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("option", { value: "", children: "all" }),
                  /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("option", { value: "active", children: "active" }),
                  /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("option", { value: "quarantined", children: "quarantined" }),
                  /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("option", { value: "superseded", children: "superseded" }),
                  /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("option", { value: "tombstoned", children: "tombstoned" })
                ]
              }
            )
          ] }),
          realms.length > 1 ? /* @__PURE__ */ (0, import_jsx_runtime31.jsxs)("label", { children: [
            "realm",
            /* @__PURE__ */ (0, import_jsx_runtime31.jsxs)(
              "select",
              {
                value: filter.realm || "",
                "data-testid": "memory-filter:realm",
                onChange: (event) => applyFilter({
                  ...filter,
                  realm: event.target.value || void 0
                }),
                children: [
                  /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("option", { value: "", children: "all (merged page)" }),
                  realms.map((realm) => /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("option", { value: realm, children: realm }, realm))
                ]
              }
            )
          ] }) : null,
          /* @__PURE__ */ (0, import_jsx_runtime31.jsxs)("label", { children: [
            "sort",
            /* @__PURE__ */ (0, import_jsx_runtime31.jsxs)(
              "select",
              {
                value: sortMode,
                "data-testid": "memory-sort",
                onChange: (event) => setSortMode(event.target.value === "utility" ? "utility" : "recency"),
                children: [
                  /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("option", { value: "recency", children: "recency" }),
                  /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("option", { value: "utility", children: "utility" })
                ]
              }
            )
          ] }),
          hasActiveFilter(filter) ? /* @__PURE__ */ (0, import_jsx_runtime31.jsx)(
            "button",
            {
              type: "button",
              className: "memory-back",
              "data-testid": "memory-filter-clear",
              onClick: () => applyFilter({}),
              children: "clear"
            }
          ) : null
        ] }) : null,
        sortMode === "utility" ? /* @__PURE__ */ (0, import_jsx_runtime31.jsxs)(SectionNote, { testid: "memory-utility-note", children: [
          "Utility mode \u2014 bytes-spent is approximated as injected_count \xD7 body_bytes until panel/injections lands. DEAD = injected \u2265 ",
          DEAD_INJECTION_THRESHOLD,
          ", never judged useful."
        ] }) : null,
        realms.length > 1 && !filter.realm?.trim() ? /* @__PURE__ */ (0, import_jsx_runtime31.jsx)(SectionNote, { testid: "memory-multi-realm-note", children: "Multi-realm view is a single merged page (keyset paging is per-realm) \u2014 pick a realm above to page through its records." }) : null,
        pageLoading ? /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("div", { className: "gating__empty", children: "Loading records\u2026" }) : null,
        !pageLoading && listView.records.length === 0 ? /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("div", { className: "gating__empty", "data-testid": "memory-records-empty", children: recordsDenied || listView.denied ? "Records: no grant." : "No memory records yet." }) : null,
        !pageLoading && listView.denied && listView.records.length > 0 ? /* @__PURE__ */ (0, import_jsx_runtime31.jsx)(SectionNote, { testid: "memory-records-denied-note", children: "Further pages: no grant \u2014 the continuation of this query was denied for this principal." }) : null,
        !pageLoading && listView.records.length > 0 ? listView.mode === "flat" ? /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("div", { className: "memory-group", children: listView.records.map((record) => /* @__PURE__ */ (0, import_jsx_runtime31.jsx)(
          RecordRow,
          {
            record,
            utilityMode: sortMode === "utility",
            onSelect: () => onSelectRecord(realmOfRecord(record), record.id)
          },
          record.id
        )) }) : listView.groups.map((group) => /* @__PURE__ */ (0, import_jsx_runtime31.jsxs)("div", { className: "memory-group", "data-testid": `memory-group:${group.key}`, children: [
          /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("div", { className: "memory-group__label", children: group.label }),
          group.records.map((record) => /* @__PURE__ */ (0, import_jsx_runtime31.jsx)(
            RecordRow,
            {
              record,
              onSelect: () => onSelectRecord(realmOfRecord(record), record.id)
            },
            record.id
          ))
        ] }, group.key)) : null,
        listView.cursor && onQueryRecords ? /* @__PURE__ */ (0, import_jsx_runtime31.jsx)(
          "button",
          {
            type: "button",
            className: "memory-back memory-load-more",
            "data-testid": "memory-load-more",
            disabled: pageLoading,
            onClick: loadMore,
            children: "load more"
          }
        ) : null
      ] }) : null,
      tab === "knowledge" ? /* @__PURE__ */ (0, import_jsx_runtime31.jsxs)("div", { className: "memory-groups", "data-testid": "memory-knowledge", children: [
        identities.length === 0 ? /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("div", { className: "gating__empty", children: recordsDenied ? "Records: no grant." : "No identity-scoped records loaded yet." }) : /* @__PURE__ */ (0, import_jsx_runtime31.jsxs)(import_jsx_runtime31.Fragment, { children: [
          /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("div", { className: "memory-filterbar", children: /* @__PURE__ */ (0, import_jsx_runtime31.jsxs)("label", { children: [
            "identity",
            /* @__PURE__ */ (0, import_jsx_runtime31.jsx)(
              "select",
              {
                value: selectedIdentity,
                "data-testid": "memory-knowledge-identity",
                onChange: (event) => setKnowledgeIdentity(event.target.value),
                children: identities.map((identity) => /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("option", { value: identity, children: identity }, identity))
              }
            )
          ] }) }),
          /* @__PURE__ */ (0, import_jsx_runtime31.jsxs)("div", { className: "memory-group", children: [
            /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("div", { className: "memory-group__label", children: "Composition (scope union over loaded records)" }),
            knowledgeComposition(records, selectedIdentity).map((segment) => /* @__PURE__ */ (0, import_jsx_runtime31.jsxs)(
              "button",
              {
                type: "button",
                className: "memory-row",
                "data-testid": `memory-knowledge-segment:${segment.label}`,
                onClick: () => openRecordsFiltered(segment.filter),
                children: [
                  /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("span", { className: "memory-row__title", children: segment.label }),
                  /* @__PURE__ */ (0, import_jsx_runtime31.jsxs)("span", { className: "memory-row__meta", children: [
                    /* @__PURE__ */ (0, import_jsx_runtime31.jsx)(Chip, { label: `${segment.count} records` }),
                    segment.approximate ? /* @__PURE__ */ (0, import_jsx_runtime31.jsxs)("span", { className: "memory-row__reason", children: [
                      "all ",
                      segment.label.split(" ")[0],
                      "-scope rows \u2014 membership resolution needs panel/context (surface 10)"
                    ] }) : null
                  ] })
                ]
              },
              segment.label
            ))
          ] })
        ] }),
        /* @__PURE__ */ (0, import_jsx_runtime31.jsx)(SectionNote, { testid: "memory-knowledge-as-injected", children: "AS-INJECTED is unverifiable in phase 1 \u2014 the composed injection block requires mobkit/memory/panel/context (surface 10)." }),
        /* @__PURE__ */ (0, import_jsx_runtime31.jsxs)("div", { className: "memory-group", "data-testid": "memory-knowledge-history", children: [
          /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("div", { className: "memory-group__label", children: "Injection history (durable ledger, newest first)" }),
          injectionsDenied ? /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("div", { className: "memory-detail__line", children: "Injection history: no grant." }) : annotatedInjections.length === 0 ? /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("div", { className: "memory-detail__line", children: "No injection-ledger rows yet." }) : annotatedInjections.map(({ entry, dup }, index) => /* @__PURE__ */ (0, import_jsx_runtime31.jsxs)(
            "div",
            {
              className: "memory-row memory-row--static",
              "data-testid": `memory-injection:${index}`,
              children: [
                /* @__PURE__ */ (0, import_jsx_runtime31.jsx)(
                  "button",
                  {
                    type: "button",
                    className: "memory-dream__record",
                    "data-testid": `memory-injection-record:${index}`,
                    onClick: () => onSelectRecord(entry.realm, entry.record_id),
                    children: entry.record_id
                  }
                ),
                /* @__PURE__ */ (0, import_jsx_runtime31.jsxs)("span", { className: "memory-row__meta", children: [
                  /* @__PURE__ */ (0, import_jsx_runtime31.jsx)(Chip, { label: entry.surface, tone: "muted" }),
                  /* @__PURE__ */ (0, import_jsx_runtime31.jsxs)("span", { className: "memory-row__reason", children: [
                    entry.identity,
                    entry.session_key ? ` \xB7 session ${entry.session_key}` : ""
                  ] }),
                  dup ? /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("span", { "data-testid": `memory-injection-dup:${index}`, children: /* @__PURE__ */ (0, import_jsx_runtime31.jsx)(Chip, { label: "DUP", tone: "warning" }) }) : null,
                  /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("span", { className: "memory-row__age", children: relativeAge(entry.at_ms) })
                ] })
              ]
            },
            `inj-${index}`
          )),
          /* @__PURE__ */ (0, import_jsx_runtime31.jsx)(SectionNote, { testid: "memory-knowledge-budget", children: "Session budget gauge requires panel/health (deferred to the distinct-affordance design)." })
        ] })
      ] }) : null,
      tab === "pipeline" ? /* @__PURE__ */ (0, import_jsx_runtime31.jsxs)("div", { className: "memory-quarantine", "data-testid": "memory-pipeline", children: [
        /* @__PURE__ */ (0, import_jsx_runtime31.jsxs)("div", { className: "memory-detail__line memory-pipeline__stages", "data-testid": "memory-pipeline-stages", children: [
          "PROPOSED (",
          proposalsDenied ? "no grant" : proposals.length,
          ") \u2500\u25B6 PENDING GATE (",
          pendingPromotions.length,
          ") \u2500\u25B6 COMMITTED \xB7 QUAR (",
          canReviewQuarantine ? quarantineRecords.length : "no grant",
          ")"
        ] }),
        /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("div", { className: "memory-note", "data-testid": "memory-quarantine-note", children: "Read-only. Verdicts are decided by the memory steward's dream and the gating flow \u2014 this queue cannot be actioned here." }),
        /* @__PURE__ */ (0, import_jsx_runtime31.jsxs)("div", { className: "memory-group", "data-testid": "memory-pipeline-proposals", children: [
          /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("div", { className: "memory-group__label", children: "Proposed \u2014 awaiting a dream verdict (taint captured at propose time)" }),
          proposalsDenied ? /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("div", { className: "memory-detail__line", children: "Proposals: no grant." }) : proposals.length === 0 ? /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("div", { className: "memory-detail__line", children: "No pending proposals." }) : proposals.map((proposal) => /* @__PURE__ */ (0, import_jsx_runtime31.jsxs)(
            "div",
            {
              className: "memory-row memory-row--static",
              "data-testid": `memory-proposal:${proposal.proposal_id}`,
              children: [
                /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("span", { className: "memory-row__title", children: proposal.title || proposal.proposal_id }),
                /* @__PURE__ */ (0, import_jsx_runtime31.jsxs)("span", { className: "memory-row__meta", children: [
                  proposal.kind ? /* @__PURE__ */ (0, import_jsx_runtime31.jsx)(Chip, { label: proposal.kind }) : null,
                  /* @__PURE__ */ (0, import_jsx_runtime31.jsx)(
                    Chip,
                    {
                      label: `\u2192 ${proposal.scope_kind}${proposal.scope_key ? `:${proposal.scope_key}` : ""}`,
                      tone: "muted"
                    }
                  ),
                  proposal.tainted ? /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("span", { "data-testid": `memory-proposal-taint:${proposal.proposal_id}`, children: /* @__PURE__ */ (0, import_jsx_runtime31.jsx)(Chip, { label: "tainted", tone: "warning" }) }) : null,
                  proposal.status ? /* @__PURE__ */ (0, import_jsx_runtime31.jsx)(
                    Chip,
                    {
                      label: proposal.status,
                      tone: proposal.status === "held" ? "warning" : "muted"
                    }
                  ) : null,
                  proposal.author ? /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("span", { className: "memory-row__reason", children: proposal.author }) : null,
                  /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("span", { className: "memory-row__age", children: relativeAge(proposal.created_at_ms) })
                ] })
              ]
            },
            `${proposal.realm}:${proposal.proposal_id}`
          ))
        ] }),
        canReviewQuarantine ? /* @__PURE__ */ (0, import_jsx_runtime31.jsxs)(import_jsx_runtime31.Fragment, { children: [
          quarantineRecords.length === 0 && pendingPromotions.length === 0 ? /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("div", { className: "gating__empty", children: "Quarantine queue is empty." }) : null,
          pendingPromotions.length > 0 ? /* @__PURE__ */ (0, import_jsx_runtime31.jsxs)("div", { className: "memory-group", children: [
            /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("div", { className: "memory-group__label", children: "Pending gated promotions" }),
            pendingPromotions.map((pending) => /* @__PURE__ */ (0, import_jsx_runtime31.jsxs)(
              "div",
              {
                className: "memory-row memory-row--static",
                "data-testid": `memory-pending:${pending.pending_id}`,
                children: [
                  /* @__PURE__ */ (0, import_jsx_runtime31.jsxs)("span", { className: "memory-row__title", children: [
                    pending.record_id,
                    " \u2192 ",
                    pending.scope_kind,
                    ":",
                    pending.scope_key
                  ] }),
                  /* @__PURE__ */ (0, import_jsx_runtime31.jsxs)("span", { className: "memory-row__meta", children: [
                    pending.rationale ? /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("span", { className: "memory-row__reason", children: pending.rationale }) : null,
                    pending.status ? /* @__PURE__ */ (0, import_jsx_runtime31.jsx)(Chip, { label: pending.status, tone: "muted" }) : null,
                    /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("span", { className: "memory-row__age", children: relativeAge(pending.created_at_ms) }),
                    onOpenGating ? /* @__PURE__ */ (0, import_jsx_runtime31.jsx)(
                      "button",
                      {
                        type: "button",
                        className: "memory-back",
                        "data-testid": `memory-pipeline-decide:${pending.pending_id}`,
                        onClick: onOpenGating,
                        children: "\u2192 decide in Gating inbox"
                      }
                    ) : null
                  ] })
                ]
              },
              pending.pending_id
            ))
          ] }) : null,
          quarantineRecords.length > 0 ? /* @__PURE__ */ (0, import_jsx_runtime31.jsxs)("div", { className: "memory-group", children: [
            /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("div", { className: "memory-group__label", children: "Quarantined records" }),
            quarantineRecords.map((record) => {
              const reason = record.status.status === "quarantined" ? record.status.reason : void 0;
              return /* @__PURE__ */ (0, import_jsx_runtime31.jsxs)(
                "button",
                {
                  type: "button",
                  className: "memory-row",
                  "data-testid": `memory-quarantine-record:${record.id}`,
                  onClick: () => onSelectRecord(realmOfRecord(record), record.id),
                  children: [
                    /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("span", { className: "memory-row__title", children: record.title || record.id }),
                    /* @__PURE__ */ (0, import_jsx_runtime31.jsxs)("span", { className: "memory-row__meta", children: [
                      reason ? /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("span", { className: "memory-row__reason", children: reason }) : null,
                      /* @__PURE__ */ (0, import_jsx_runtime31.jsx)(Chip, { label: trustLabel(record.trust), tone: trustTone(record.trust) }),
                      /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("span", { className: "memory-row__age", children: relativeAge(record.created_at_ms) })
                    ] })
                  ]
                },
                record.id
              );
            })
          ] }) : null
        ] }) : /* @__PURE__ */ (0, import_jsx_runtime31.jsx)(SectionNote, { testid: "memory-pipeline-no-grant", children: "Quarantine queue: no grant \u2014 rows require memory.quarantine.review." }),
        /* @__PURE__ */ (0, import_jsx_runtime31.jsxs)("div", { className: "memory-group", "data-testid": "memory-review-queue", children: [
          /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("div", { className: "memory-group__label", children: "Review queue \u2014 memories you might want to correct" }),
          auditVerdictsDenied ? /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("div", { className: "memory-detail__line", children: "Review queue: no grant." }) : auditVerdicts.length === 0 ? /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("div", { className: "memory-detail__line", children: "Review queue is empty." }) : auditVerdicts.map((verdict) => /* @__PURE__ */ (0, import_jsx_runtime31.jsxs)(
            "div",
            {
              className: "memory-row memory-row--static",
              "data-testid": `memory-review:${verdict.run_id}:${verdict.record_id}`,
              children: [
                /* @__PURE__ */ (0, import_jsx_runtime31.jsx)(
                  "button",
                  {
                    type: "button",
                    className: "memory-dream__record",
                    "data-testid": `memory-review-record:${verdict.run_id}:${verdict.record_id}`,
                    onClick: () => onSelectRecord(verdict.realm, verdict.record_id),
                    children: verdict.record_id
                  }
                ),
                /* @__PURE__ */ (0, import_jsx_runtime31.jsxs)("span", { className: "memory-row__meta", children: [
                  verdict.verdict ? /* @__PURE__ */ (0, import_jsx_runtime31.jsx)(Chip, { label: verdict.verdict, tone: "warning" }) : null,
                  verdict.rationale ? /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("span", { className: "memory-row__reason", children: verdict.rationale }) : null,
                  /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("span", { className: "memory-row__reason", children: verdict.run_id }),
                  /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("span", { className: "memory-row__age", children: relativeAge(verdict.created_at_ms) })
                ] })
              ]
            },
            `${verdict.realm}:${verdict.run_id}:${verdict.record_id}`
          )),
          /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("div", { className: "memory-note", children: "Read-only \u2014 the correction affordance ships with the write-path design." })
        ] }),
        /* @__PURE__ */ (0, import_jsx_runtime31.jsx)(
          MemoryLiveStrip,
          {
            frames: memoryFrames,
            onPivot: (realm, recordId) => void onSelectRecord(realm, recordId)
          }
        )
      ] }) : null,
      tab === "dreams" ? /* @__PURE__ */ (0, import_jsx_runtime31.jsxs)("div", { className: "memory-dreams", children: [
        /* @__PURE__ */ (0, import_jsx_runtime31.jsxs)("div", { className: "memory-group", "data-testid": "memory-dream-runs", children: [
          /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("div", { className: "memory-group__label", children: "Durable verdict sheets (dream_runs \u2014 survive restarts)" }),
          dreamRunsDenied ? /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("div", { className: "memory-detail__line", children: "Verdict sheets: no grant." }) : dreamSheets.length === 0 ? /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("div", { className: "memory-detail__line", children: "No persisted dream runs yet \u2014 runs before the dream_runs table land only in the audit reconstruction below." }) : dreamSheets.map((run) => {
            const expanded = expandedRuns[run.run_id] === true;
            const detail2 = normalizeDreamRunDetail(run.detail);
            return /* @__PURE__ */ (0, import_jsx_runtime31.jsxs)(
              "div",
              {
                className: "gpolicy memory-dream-run",
                "data-testid": `memory-dream-run:${run.run_id}`,
                children: [
                  /* @__PURE__ */ (0, import_jsx_runtime31.jsxs)(
                    "button",
                    {
                      type: "button",
                      className: "memory-row memory-dream-run__head",
                      "data-testid": `memory-dream-run-toggle:${run.run_id}`,
                      onClick: () => setExpandedRuns((current) => ({
                        ...current,
                        [run.run_id]: !expanded
                      })),
                      children: [
                        /* @__PURE__ */ (0, import_jsx_runtime31.jsxs)("span", { className: "memory-row__title", children: [
                          expanded ? "\u25BE" : "\u25B8",
                          " ",
                          run.run_id
                        ] }),
                        /* @__PURE__ */ (0, import_jsx_runtime31.jsxs)("span", { className: "memory-row__meta", children: [
                          run.partition ? /* @__PURE__ */ (0, import_jsx_runtime31.jsx)(Chip, { label: run.partition, tone: "muted" }) : null,
                          /* @__PURE__ */ (0, import_jsx_runtime31.jsxs)("span", { className: "memory-row__reason", children: [
                            dreamRunDuration(run),
                            " \xB7",
                            " ",
                            typeof run.ops_committed === "number" ? `${run.ops_committed} ops` : "\u2014 ops"
                          ] }),
                          /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("span", { className: "memory-row__age", children: relativeAge(run.completed_at_ms || run.started_at_ms) })
                        ] })
                      ]
                    }
                  ),
                  expanded ? /* @__PURE__ */ (0, import_jsx_runtime31.jsx)(
                    "div",
                    {
                      className: "memory-dream-run__detail",
                      "data-testid": `memory-dream-run-detail:${run.run_id}`,
                      children: detail2.raw !== null ? /* @__PURE__ */ (0, import_jsx_runtime31.jsxs)("div", { className: "memory-detail__line", children: [
                        "unparsed detail: ",
                        detail2.raw
                      ] }) : /* @__PURE__ */ (0, import_jsx_runtime31.jsxs)(import_jsx_runtime31.Fragment, { children: [
                        detail2.phases.length > 0 ? detail2.phases.map(([name, note], index) => /* @__PURE__ */ (0, import_jsx_runtime31.jsxs)("div", { className: "memory-detail__line", children: [
                          name,
                          note ? ` \u2014 ${note}` : ""
                        ] }, `ph-${index}`)) : /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("div", { className: "memory-detail__line", children: "no phases recorded" }),
                        /* @__PURE__ */ (0, import_jsx_runtime31.jsxs)("div", { className: "memory-detail__line", children: [
                          "verdicts:",
                          " ",
                          detail2.verdicts.length > 0 ? detail2.verdicts.map(([name, count]) => `${count} ${name}`).join(" \xB7 ") : "all counters zero"
                        ] }),
                        detail2.skips.map((skip, index) => /* @__PURE__ */ (0, import_jsx_runtime31.jsxs)(
                          "div",
                          {
                            className: "memory-detail__line memory-dream__rationale",
                            children: [
                              "skip: ",
                              skip
                            ]
                          },
                          `sk-${index}`
                        ))
                      ] })
                    }
                  ) : null
                ]
              },
              run.run_id
            );
          })
        ] }),
        /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("div", { className: "memory-group__label", children: "Reconstructed from audit rows" }),
        dreams.length === 0 ? /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("div", { className: "gating__empty", children: dreamsDenied ? "Dream audit: no grant." : "No dream runs recorded yet." }) : dreams.map((run) => {
          const summary = dreamOpKindsSummary(run.op_kinds);
          return /* @__PURE__ */ (0, import_jsx_runtime31.jsxs)("div", { className: "gpolicy memory-dream", "data-testid": `memory-dream:${run.run_id}`, children: [
            /* @__PURE__ */ (0, import_jsx_runtime31.jsxs)("div", { className: "gpolicy__head", children: [
              /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("span", { className: "gpolicy__action", children: run.run_id }),
              run.quarantined_ops && run.quarantined_ops > 0 ? /* @__PURE__ */ (0, import_jsx_runtime31.jsx)(Chip, { label: `${run.quarantined_ops} quarantined`, tone: "warning" }) : null
            ] }),
            /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("div", { className: "gpolicy__meta", children: dreamTimeRange(run) }),
            /* @__PURE__ */ (0, import_jsx_runtime31.jsxs)("div", { className: "gpolicy__meta", children: [
              typeof run.ops === "number" ? `${run.ops} ops` : "\u2014",
              summary ? ` \xB7 ${summary}` : ""
            ] }),
            (run.memory_ids || []).length > 0 ? /* @__PURE__ */ (0, import_jsx_runtime31.jsxs)("div", { className: "memory-dream__touched", children: [
              "touched:",
              (run.memory_ids || []).map((memoryId) => /* @__PURE__ */ (0, import_jsx_runtime31.jsx)(
                "button",
                {
                  type: "button",
                  className: "memory-dream__record",
                  "data-testid": `memory-dream-record:${run.run_id}:${memoryId}`,
                  onClick: () => onSelectRecord(run.realm, memoryId),
                  children: memoryId
                },
                memoryId
              ))
            ] }) : null,
            (run.rationales || []).map((rationale, index) => /* @__PURE__ */ (0, import_jsx_runtime31.jsx)("div", { className: "memory-dream__rationale", children: rationale }, `r-${index}`))
          ] }, run.run_id);
        })
      ] }) : null
    ] })
  ] });
}

// src/panels/RosterPanel.tsx
var import_react24 = __toESM(require("react"));
var import_jsx_runtime32 = require("react/jsx-runtime");
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
  const [q, setQ] = import_react24.default.useState("");
  const [role, setRole] = import_react24.default.useState("all");
  const [sel, setSel] = import_react24.default.useState(agents[0]?.member_id || "");
  import_react24.default.useEffect(() => {
    if (selectedMemberId) setSel(selectedMemberId);
  }, [selectedMemberId]);
  const rows = import_react24.default.useMemo(() => {
    return agents.filter((a) => {
      if (role !== "all" && roleOf(a) !== role) return false;
      if (!q) return true;
      const hay = `${a.label} ${a.member_id} ${a.identity || ""} ${a.role || ""} ${a.kind || ""}`.toLowerCase();
      return hay.includes(q.toLowerCase());
    });
  }, [agents, q, role]);
  const active = rows.find((r2) => r2.member_id === sel) || rows[0];
  const activeIdentity = active?.identity || active?.member_id || "";
  const activePeers = (active?.wired_to || []).map(displayPeer).filter(Boolean);
  return /* @__PURE__ */ (0, import_jsx_runtime32.jsxs)("div", { className: "view roster", "data-testid": "roster-panel", children: [
    /* @__PURE__ */ (0, import_jsx_runtime32.jsxs)("div", { className: "view__head", children: [
      /* @__PURE__ */ (0, import_jsx_runtime32.jsx)("h2", { children: "Roster" }),
      /* @__PURE__ */ (0, import_jsx_runtime32.jsxs)("span", { className: "view__sub", children: [
        rows.length,
        " of ",
        agents.length,
        " agents"
      ] }),
      /* @__PURE__ */ (0, import_jsx_runtime32.jsx)("span", { className: "view__spacer" }),
      /* @__PURE__ */ (0, import_jsx_runtime32.jsx)(
        "input",
        {
          className: "view__search",
          placeholder: "Filter agents, profiles, ids\u2026",
          value: q,
          onChange: (e) => setQ(e.target.value)
        }
      ),
      /* @__PURE__ */ (0, import_jsx_runtime32.jsx)("div", { className: "view__segs", children: ROLE_BUCKETS.map((r2) => /* @__PURE__ */ (0, import_jsx_runtime32.jsx)("button", { className: role === r2 ? "is-active" : "", onClick: () => setRole(r2), children: r2 }, r2)) })
    ] }),
    /* @__PURE__ */ (0, import_jsx_runtime32.jsxs)("div", { className: "roster__body", children: [
      /* @__PURE__ */ (0, import_jsx_runtime32.jsxs)("div", { className: "roster__table", children: [
        /* @__PURE__ */ (0, import_jsx_runtime32.jsxs)("div", { className: "roster__row roster__row--head", children: [
          /* @__PURE__ */ (0, import_jsx_runtime32.jsx)("span", { children: "Name" }),
          /* @__PURE__ */ (0, import_jsx_runtime32.jsx)("span", { children: "Role" }),
          /* @__PURE__ */ (0, import_jsx_runtime32.jsx)("span", { children: "State" }),
          /* @__PURE__ */ (0, import_jsx_runtime32.jsx)("span", { children: "Profile" }),
          /* @__PURE__ */ (0, import_jsx_runtime32.jsx)("span", { children: "Gen" }),
          /* @__PURE__ */ (0, import_jsx_runtime32.jsx)("span", { children: "Chk" }),
          /* @__PURE__ */ (0, import_jsx_runtime32.jsx)("span", { children: "Lease" })
        ] }),
        rows.map((r2) => {
          const isSel = active && r2.member_id === active.member_id;
          return /* @__PURE__ */ (0, import_jsx_runtime32.jsxs)(
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
                /* @__PURE__ */ (0, import_jsx_runtime32.jsxs)("span", { className: "roster__name", children: [
                  /* @__PURE__ */ (0, import_jsx_runtime32.jsx)("span", { className: "roster__dot" }),
                  /* @__PURE__ */ (0, import_jsx_runtime32.jsxs)("span", { children: [
                    /* @__PURE__ */ (0, import_jsx_runtime32.jsx)("div", { children: r2.label }),
                    /* @__PURE__ */ (0, import_jsx_runtime32.jsx)("div", { className: "roster__id", children: r2.identity || r2.member_id })
                  ] })
                ] }),
                /* @__PURE__ */ (0, import_jsx_runtime32.jsx)("span", { children: roleOf(r2) }),
                /* @__PURE__ */ (0, import_jsx_runtime32.jsx)("span", { className: "roster__state", children: stateLabel(r2.state) }),
                /* @__PURE__ */ (0, import_jsx_runtime32.jsx)("span", { className: "mono dim", children: r2.role || "\u2014" }),
                /* @__PURE__ */ (0, import_jsx_runtime32.jsx)("span", { className: "mono", children: r2.generation ?? "\u2014" }),
                /* @__PURE__ */ (0, import_jsx_runtime32.jsx)("span", { className: "mono", children: r2.checkpoint_version ?? "\u2014" }),
                /* @__PURE__ */ (0, import_jsx_runtime32.jsx)("span", { className: "mono dim", children: r2.lease_healthy === false ? "unhealthy" : "ok" })
              ]
            },
            r2.member_id
          );
        })
      ] }),
      /* @__PURE__ */ (0, import_jsx_runtime32.jsx)("aside", { className: "roster__detail", children: active && /* @__PURE__ */ (0, import_jsx_runtime32.jsxs)(import_jsx_runtime32.Fragment, { children: [
        /* @__PURE__ */ (0, import_jsx_runtime32.jsxs)("div", { className: "rd__head", children: [
          /* @__PURE__ */ (0, import_jsx_runtime32.jsx)("div", { className: "rd__title", children: active.label }),
          /* @__PURE__ */ (0, import_jsx_runtime32.jsx)("div", { className: "rd__id", children: active.identity || active.member_id }),
          /* @__PURE__ */ (0, import_jsx_runtime32.jsx)("div", { className: "rd__tags", children: [active.role, active.kind, roleOf(active)].filter(Boolean).map((t) => /* @__PURE__ */ (0, import_jsx_runtime32.jsx)("span", { className: "chip", children: String(t) }, String(t))) })
        ] }),
        /* @__PURE__ */ (0, import_jsx_runtime32.jsxs)("dl", { className: "rd__grid", children: [
          /* @__PURE__ */ (0, import_jsx_runtime32.jsx)("dt", { children: "Profile" }),
          /* @__PURE__ */ (0, import_jsx_runtime32.jsx)("dd", { children: active.role || "\u2014" }),
          /* @__PURE__ */ (0, import_jsx_runtime32.jsx)("dt", { children: "Kind" }),
          /* @__PURE__ */ (0, import_jsx_runtime32.jsx)("dd", { children: active.kind || "\u2014" }),
          /* @__PURE__ */ (0, import_jsx_runtime32.jsx)("dt", { children: "Role" }),
          /* @__PURE__ */ (0, import_jsx_runtime32.jsx)("dd", { children: roleOf(active) }),
          /* @__PURE__ */ (0, import_jsx_runtime32.jsx)("dt", { children: "State" }),
          /* @__PURE__ */ (0, import_jsx_runtime32.jsx)("dd", { children: /* @__PURE__ */ (0, import_jsx_runtime32.jsx)("span", { className: "roster__state", children: stateLabel(active.state) }) }),
          /* @__PURE__ */ (0, import_jsx_runtime32.jsx)("dt", { children: "Member" }),
          /* @__PURE__ */ (0, import_jsx_runtime32.jsx)("dd", { className: "mono", children: active.member_id }),
          /* @__PURE__ */ (0, import_jsx_runtime32.jsx)("dt", { children: "Identity" }),
          /* @__PURE__ */ (0, import_jsx_runtime32.jsx)("dd", { className: "mono", children: active.identity || "\u2014" }),
          /* @__PURE__ */ (0, import_jsx_runtime32.jsx)("dt", { children: "Session" }),
          /* @__PURE__ */ (0, import_jsx_runtime32.jsx)("dd", { className: "mono", children: active.session_id || "\u2014" }),
          /* @__PURE__ */ (0, import_jsx_runtime32.jsx)("dt", { children: "Generation" }),
          /* @__PURE__ */ (0, import_jsx_runtime32.jsx)("dd", { className: "mono", children: active.generation ?? "\u2014" }),
          /* @__PURE__ */ (0, import_jsx_runtime32.jsx)("dt", { children: "Checkpoint" }),
          /* @__PURE__ */ (0, import_jsx_runtime32.jsx)("dd", { className: "mono", children: active.checkpoint_version ?? "\u2014" }),
          /* @__PURE__ */ (0, import_jsx_runtime32.jsx)("dt", { children: "Lease" }),
          /* @__PURE__ */ (0, import_jsx_runtime32.jsx)("dd", { className: "mono", children: active.lease_healthy === false ? "unhealthy" : "ok" }),
          /* @__PURE__ */ (0, import_jsx_runtime32.jsx)("dt", { children: "Wired" }),
          /* @__PURE__ */ (0, import_jsx_runtime32.jsx)("dd", { children: activePeers.length > 0 ? /* @__PURE__ */ (0, import_jsx_runtime32.jsx)("span", { className: "rd__peers", children: activePeers.map((peer) => /* @__PURE__ */ (0, import_jsx_runtime32.jsx)("span", { className: "chip", children: peer }, peer)) }) : /* @__PURE__ */ (0, import_jsx_runtime32.jsx)("span", { className: "mono dim", children: "none" }) })
        ] }),
        /* @__PURE__ */ (0, import_jsx_runtime32.jsxs)("div", { className: "rd__actions", children: [
          actionVisibility?.inspect !== false ? /* @__PURE__ */ (0, import_jsx_runtime32.jsx)("button", { onClick: () => onDetails(active), children: actionLabels?.inspect || "Details" }) : null,
          actionVisibility?.chat !== false ? /* @__PURE__ */ (0, import_jsx_runtime32.jsx)("button", { onClick: () => onChat(active), children: actionLabels?.chat || "Open chat" }) : null,
          actionVisibility?.respawn !== false && active.affordances?.can_respawn ? /* @__PURE__ */ (0, import_jsx_runtime32.jsx)("button", { onClick: () => onLifecycle(activeIdentity, "mobkit/respawn"), children: actionLabels?.respawn || "Respawn" }) : null,
          actionVisibility?.reset !== false && canResetLifecycle ? /* @__PURE__ */ (0, import_jsx_runtime32.jsx)("button", { onClick: () => onLifecycle(activeIdentity, "mobkit/reset"), children: actionLabels?.reset || "Reset" }) : null,
          actionVisibility?.retire !== false && active.affordances?.can_retire ? /* @__PURE__ */ (0, import_jsx_runtime32.jsx)("button", { className: "danger", onClick: () => onLifecycle(activeIdentity, "mobkit/retire"), children: actionLabels?.retire || "Retire" }) : null
        ] })
      ] }) })
    ] })
  ] });
}

// src/panels/WorkGraphPanel.tsx
var import_react25 = __toESM(require("react"));
var import_jsx_runtime33 = require("react/jsx-runtime");
function buildWorkGraphPanelTree(items, edges) {
  const byId = /* @__PURE__ */ new Map();
  for (const item of items) {
    if (typeof item.id === "string" && item.id) byId.set(item.id, item);
  }
  const parentOf = /* @__PURE__ */ new Map();
  for (const edge of edges) {
    if (edge.kind !== "parent") continue;
    if (typeof edge.from_id === "string" && edge.from_id && typeof edge.to_id === "string" && edge.to_id && edge.from_id !== edge.to_id && !parentOf.has(edge.from_id)) {
      parentOf.set(edge.from_id, edge.to_id);
    }
  }
  const childrenOf = /* @__PURE__ */ new Map();
  const roots = [];
  for (const id of byId.keys()) {
    const parent = parentOf.get(id);
    if (parent && byId.has(parent)) {
      const children = childrenOf.get(parent) || [];
      children.push(id);
      childrenOf.set(parent, children);
    } else {
      roots.push(id);
    }
  }
  const sortIds = (ids) => [...ids].sort((left, right) => {
    const leftKey = byId.get(left)?.created_at || "";
    const rightKey = byId.get(right)?.created_at || "";
    if (leftKey !== rightKey) return leftKey < rightKey ? -1 : 1;
    return left < right ? -1 : left === right ? 0 : 1;
  });
  const rows = [];
  const visited = /* @__PURE__ */ new Set();
  const visit = (id, depth) => {
    if (visited.has(id)) return;
    visited.add(id);
    const item = byId.get(id);
    if (!item) return;
    rows.push({ item, itemId: id, depth });
    for (const child of sortIds(childrenOf.get(id) || [])) {
      visit(child, depth + 1);
    }
  };
  for (const root of sortIds(roots)) visit(root, 0);
  return rows;
}
function workGraphBindingStatusLabel(binding) {
  const state = binding.status?.state || "active";
  if (state === "paused") {
    const until = binding.status?.until;
    return until ? `paused until ${until.slice(0, 16).replace("T", " ")}` : "paused";
  }
  return state;
}
function workGraphBindingTargetLabel(binding) {
  const target = binding.target;
  if (!target) return "";
  if (typeof target.session_id === "string" && target.session_id) return target.session_id;
  const ownerKey = target.owner_key;
  if (ownerKey) {
    return [ownerKey.kind, ownerKey.id].filter(Boolean).join(":");
  }
  return "";
}
function workGraphEventLine(event) {
  const kind = typeof event.kind === "string" ? event.kind.replace(/_/g, " ") : "event";
  const at = typeof event.at === "string" && event.at.length >= 16 ? `${event.at.slice(0, 10)} ${event.at.slice(11, 16)}` : "";
  const item = typeof event.item_id === "string" && event.item_id ? event.item_id : "";
  return [at, kind, item].filter(Boolean).join(" \xB7 ");
}
function workGraphOwnerLabelOf(item) {
  return item.owner?.display_name || item.owner?.key?.id || item.claim?.owner?.display_name || item.claim?.owner?.key?.id || "";
}
function workGraphGoalRevisionOf(binding, items) {
  const itemId = binding.work_ref?.item_id;
  if (!itemId) return void 0;
  const item = items.find((candidate) => candidate.id === itemId);
  return typeof item?.revision === "number" ? item.revision : void 0;
}
function workGraphEventsParams(eventHighWaterMark, limit) {
  if (typeof eventHighWaterMark === "number" && Number.isFinite(eventHighWaterMark)) {
    return { limit, after_seq: Math.max(0, Math.floor(eventHighWaterMark) - limit) };
  }
  return { limit };
}
function workGraphEventsNewestFirst(events) {
  return [...events].reverse();
}
function createWorkGraphRefreshSequencer() {
  let latest = 0;
  return {
    begin() {
      latest += 1;
      const token = latest;
      return () => token === latest;
    }
  };
}
function statusDotClass(status) {
  return `workgraph__dot is-${status || "open"}`;
}
function ItemRow2({
  row,
  canManage,
  onClaim,
  onClose
}) {
  const { item, itemId, depth } = row;
  const status = item.status || "open";
  const revision = typeof item.revision === "number" ? item.revision : void 0;
  const terminal = status === "completed" || status === "cancelled" || status === "failed";
  const owner = workGraphOwnerLabelOf(item);
  return /* @__PURE__ */ (0, import_jsx_runtime33.jsxs)(
    "div",
    {
      className: "workgraph__item",
      "data-testid": `workgraph-panel-item:${itemId}`,
      style: { paddingLeft: `${depth * 16}px` },
      children: [
        /* @__PURE__ */ (0, import_jsx_runtime33.jsx)("span", { className: statusDotClass(status), "aria-hidden": "true" }),
        /* @__PURE__ */ (0, import_jsx_runtime33.jsx)("span", { className: "workgraph__item-title", title: item.description || item.title, children: item.title || itemId }),
        item.priority && item.priority !== "medium" ? /* @__PURE__ */ (0, import_jsx_runtime33.jsx)("span", { className: `workgraph__chip is-priority-${item.priority}`, children: item.priority }) : null,
        owner ? /* @__PURE__ */ (0, import_jsx_runtime33.jsx)("span", { className: "workgraph__chip", children: owner }) : null,
        /* @__PURE__ */ (0, import_jsx_runtime33.jsx)("span", { className: "workgraph__item-status", children: status.replace(/_/g, " ") }),
        canManage && onClaim && status === "open" && !owner ? /* @__PURE__ */ (0, import_jsx_runtime33.jsx)(
          "button",
          {
            type: "button",
            className: "workgraph__action",
            "data-testid": `workgraph-panel-action:${itemId}:claim`,
            onClick: () => onClaim({ itemId, revision }),
            children: "Claim"
          }
        ) : null,
        canManage && onClose && !terminal && status !== "blocked" ? /* @__PURE__ */ (0, import_jsx_runtime33.jsx)(
          "button",
          {
            type: "button",
            className: "workgraph__action",
            "data-testid": `workgraph-panel-action:${itemId}:close`,
            onClick: () => onClose({ itemId, revision }),
            children: "Done"
          }
        ) : null
      ]
    }
  );
}
function AttentionRow2({
  binding,
  goalRevision,
  canManage,
  onGoalConfirm,
  onGoalRequestClose,
  onAttentionPause,
  onAttentionResume,
  onAttentionReassign
}) {
  const [reassignOpen, setReassignOpen] = import_react25.default.useState(false);
  const [reassignIdentity, setReassignIdentity] = import_react25.default.useState("");
  const bindingId = binding.binding_id || "";
  const revision = binding.machine_state?.revision;
  const statusLabel2 = workGraphBindingStatusLabel(binding);
  const targetLabel = workGraphBindingTargetLabel(binding);
  const isActive = statusLabel2 === "active";
  const isPaused = statusLabel2.startsWith("paused");
  const live = isActive || isPaused;
  const canReassign = live && binding.mode === "coordinate";
  const bindingInput = { bindingId, revision };
  const goalInput = { bindingId, revision: goalRevision };
  if (!bindingId) return null;
  return /* @__PURE__ */ (0, import_jsx_runtime33.jsxs)("div", { className: "workgraph__binding", "data-testid": `workgraph-panel-binding:${bindingId}`, children: [
    /* @__PURE__ */ (0, import_jsx_runtime33.jsxs)("div", { className: "workgraph__binding-line", children: [
      /* @__PURE__ */ (0, import_jsx_runtime33.jsx)("span", { className: `workgraph__mode is-${binding.mode || "pursue"}`, children: binding.mode || "pursue" }),
      /* @__PURE__ */ (0, import_jsx_runtime33.jsx)("span", { className: "workgraph__binding-status", children: statusLabel2 }),
      targetLabel ? /* @__PURE__ */ (0, import_jsx_runtime33.jsx)("span", { className: "workgraph__binding-target", children: targetLabel }) : null,
      binding.work_ref?.item_id ? /* @__PURE__ */ (0, import_jsx_runtime33.jsx)("span", { className: "workgraph__chip", title: "Bound work item", children: binding.work_ref.item_id }) : null,
      /* @__PURE__ */ (0, import_jsx_runtime33.jsx)("span", { className: "workgraph__spacer" }),
      canManage && onAttentionPause && isActive ? /* @__PURE__ */ (0, import_jsx_runtime33.jsx)("button", { type: "button", className: "workgraph__action", onClick: () => onAttentionPause(bindingInput), children: "Pause" }) : null,
      canManage && onAttentionResume && isPaused ? /* @__PURE__ */ (0, import_jsx_runtime33.jsx)("button", { type: "button", className: "workgraph__action", onClick: () => onAttentionResume(bindingInput), children: "Resume" }) : null,
      canManage && onGoalConfirm && live ? /* @__PURE__ */ (0, import_jsx_runtime33.jsx)("button", { type: "button", className: "workgraph__action", onClick: () => onGoalConfirm(goalInput), children: "Confirm" }) : null,
      canManage && onGoalRequestClose && live ? /* @__PURE__ */ (0, import_jsx_runtime33.jsx)("button", { type: "button", className: "workgraph__action", onClick: () => onGoalRequestClose(goalInput), children: "Request close" }) : null,
      canManage && onAttentionReassign && canReassign ? /* @__PURE__ */ (0, import_jsx_runtime33.jsx)(
        "button",
        {
          type: "button",
          className: "workgraph__action",
          "aria-expanded": reassignOpen,
          onClick: () => setReassignOpen((value) => !value),
          children: "Reassign"
        }
      ) : null
    ] }),
    reassignOpen && canManage && onAttentionReassign && canReassign ? /* @__PURE__ */ (0, import_jsx_runtime33.jsxs)("div", { className: "workgraph__reassign", children: [
      /* @__PURE__ */ (0, import_jsx_runtime33.jsx)(
        "input",
        {
          placeholder: "Target agent identity\u2026",
          value: reassignIdentity,
          onChange: (event) => setReassignIdentity(event.target.value),
          "data-testid": `workgraph-panel-reassign-input:${bindingId}`
        }
      ),
      /* @__PURE__ */ (0, import_jsx_runtime33.jsx)(
        "button",
        {
          type: "button",
          className: "workgraph__action",
          disabled: !reassignIdentity.trim(),
          "data-testid": `workgraph-panel-reassign-submit:${bindingId}`,
          onClick: () => {
            onAttentionReassign({ ...bindingInput, identity: reassignIdentity.trim() });
            setReassignOpen(false);
            setReassignIdentity("");
          },
          children: "Reassign to identity"
        }
      )
    ] }) : null
  ] });
}
function WorkGraphPanel({
  data,
  canManage,
  onRefresh,
  onClaim,
  onClose,
  onGoalConfirm,
  onGoalRequestClose,
  onAttentionPause,
  onAttentionResume,
  onAttentionReassign
}) {
  const rows = import_react25.default.useMemo(
    () => buildWorkGraphPanelTree(data.items, data.edges),
    [data.items, data.edges]
  );
  if (data.unavailable) {
    return /* @__PURE__ */ (0, import_jsx_runtime33.jsx)("div", { className: "console-panel workgraph", "data-testid": "workgraph-panel", children: /* @__PURE__ */ (0, import_jsx_runtime33.jsx)("div", { className: "workgraph__empty", children: "WorkGraph is not configured on this runtime." }) });
  }
  return /* @__PURE__ */ (0, import_jsx_runtime33.jsxs)("div", { className: "console-panel workgraph", "data-testid": "workgraph-panel", children: [
    /* @__PURE__ */ (0, import_jsx_runtime33.jsxs)("div", { className: "workgraph__head", children: [
      /* @__PURE__ */ (0, import_jsx_runtime33.jsx)("h3", { children: "WorkGraph" }),
      data.capturedAt ? /* @__PURE__ */ (0, import_jsx_runtime33.jsxs)("span", { className: "workgraph__captured", children: [
        "as of ",
        data.capturedAt.slice(0, 19).replace("T", " ")
      ] }) : null,
      /* @__PURE__ */ (0, import_jsx_runtime33.jsx)("span", { className: "workgraph__spacer" }),
      /* @__PURE__ */ (0, import_jsx_runtime33.jsx)(
        "button",
        {
          type: "button",
          className: "workgraph__action",
          onClick: onRefresh,
          "data-testid": "workgraph-panel-refresh",
          children: "Refresh"
        }
      )
    ] }),
    data.error ? /* @__PURE__ */ (0, import_jsx_runtime33.jsx)("div", { className: "workgraph__error", role: "alert", children: data.error }) : null,
    data.denied ? /* @__PURE__ */ (0, import_jsx_runtime33.jsx)("div", { className: "workgraph__empty", "data-testid": "workgraph-panel-denied", children: "You do not have a grant to view WorkGraph state." }) : /* @__PURE__ */ (0, import_jsx_runtime33.jsxs)(import_jsx_runtime33.Fragment, { children: [
      /* @__PURE__ */ (0, import_jsx_runtime33.jsxs)("div", { className: "workgraph__section", children: [
        /* @__PURE__ */ (0, import_jsx_runtime33.jsx)("div", { className: "workgraph__sec-label", children: "Work items" }),
        rows.length === 0 ? /* @__PURE__ */ (0, import_jsx_runtime33.jsx)("div", { className: "workgraph__empty", children: "No work items." }) : rows.map((row) => /* @__PURE__ */ (0, import_jsx_runtime33.jsx)(
          ItemRow2,
          {
            row,
            canManage,
            onClaim,
            onClose
          },
          row.itemId
        ))
      ] }),
      /* @__PURE__ */ (0, import_jsx_runtime33.jsxs)("div", { className: "workgraph__section", children: [
        /* @__PURE__ */ (0, import_jsx_runtime33.jsx)("div", { className: "workgraph__sec-label", children: "Attention" }),
        data.attention.length === 0 ? /* @__PURE__ */ (0, import_jsx_runtime33.jsx)("div", { className: "workgraph__empty", children: "No attention bindings." }) : data.attention.map((binding, index) => /* @__PURE__ */ (0, import_jsx_runtime33.jsx)(
          AttentionRow2,
          {
            binding,
            goalRevision: workGraphGoalRevisionOf(binding, data.items),
            canManage,
            onGoalConfirm,
            onGoalRequestClose,
            onAttentionPause,
            onAttentionResume,
            onAttentionReassign
          },
          binding.binding_id || `binding-${index}`
        ))
      ] }),
      /* @__PURE__ */ (0, import_jsx_runtime33.jsxs)("div", { className: "workgraph__section", children: [
        /* @__PURE__ */ (0, import_jsx_runtime33.jsx)("div", { className: "workgraph__sec-label", children: "Recent events" }),
        data.events.length === 0 ? /* @__PURE__ */ (0, import_jsx_runtime33.jsx)("div", { className: "workgraph__empty", children: "No events." }) : /* @__PURE__ */ (0, import_jsx_runtime33.jsx)("div", { className: "workgraph__events", children: data.events.map((event, index) => /* @__PURE__ */ (0, import_jsx_runtime33.jsx)("div", { className: "workgraph__event", children: workGraphEventLine(event) }, `${event.seq ?? index}`)) })
      ] })
    ] })
  ] });
}

// src/panels/RoutingPanel.tsx
var import_react26 = __toESM(require("react"));
var import_jsx_runtime34 = require("react/jsx-runtime");
function RoutingPanel({ data }) {
  const routes = data.routes || [];
  const deliveries = data.deliveries || [];
  const [q, setQ] = import_react26.default.useState("");
  const [sel, setSel] = import_react26.default.useState(routes[0]?.route_key || "");
  const rows = import_react26.default.useMemo(() => {
    if (!q) return routes;
    const needle = q.toLowerCase();
    return routes.filter(
      (r2) => r2.route_key.toLowerCase().includes(needle) || r2.recipient.toLowerCase().includes(needle) || r2.sink.toLowerCase().includes(needle) || r2.target_module.toLowerCase().includes(needle)
    );
  }, [routes, q]);
  const active = rows.find((r2) => r2.route_key === sel) || rows[0];
  const recentDeliveries = deliveries.slice(0, 40);
  const trafficForRoute = (routeKey) => deliveries.filter((d) => d.route_id === routeKey).length;
  return /* @__PURE__ */ (0, import_jsx_runtime34.jsxs)("div", { className: "view routing", "data-testid": "routing-panel", children: [
    /* @__PURE__ */ (0, import_jsx_runtime34.jsxs)("div", { className: "view__head", children: [
      /* @__PURE__ */ (0, import_jsx_runtime34.jsx)("h2", { children: "Routing" }),
      /* @__PURE__ */ (0, import_jsx_runtime34.jsxs)("span", { className: "view__sub", children: [
        rows.length,
        " routes \xB7 ",
        deliveries.length,
        " deliveries (recent)"
      ] }),
      /* @__PURE__ */ (0, import_jsx_runtime34.jsx)("span", { className: "view__spacer" }),
      /* @__PURE__ */ (0, import_jsx_runtime34.jsx)(
        "input",
        {
          className: "view__search",
          placeholder: "Filter route, recipient, sink\u2026",
          value: q,
          onChange: (e) => setQ(e.target.value)
        }
      )
    ] }),
    /* @__PURE__ */ (0, import_jsx_runtime34.jsxs)("div", { className: "routing__body", children: [
      /* @__PURE__ */ (0, import_jsx_runtime34.jsxs)("div", { className: "routing__table", children: [
        /* @__PURE__ */ (0, import_jsx_runtime34.jsxs)("div", { className: "routing__row routing__row--head", children: [
          /* @__PURE__ */ (0, import_jsx_runtime34.jsx)("span", { children: "Route" }),
          /* @__PURE__ */ (0, import_jsx_runtime34.jsx)("span", { children: "Channel" }),
          /* @__PURE__ */ (0, import_jsx_runtime34.jsx)("span", { children: "Recipient" }),
          /* @__PURE__ */ (0, import_jsx_runtime34.jsx)("span", { children: "Sink" }),
          /* @__PURE__ */ (0, import_jsx_runtime34.jsx)("span", { children: "Module" }),
          /* @__PURE__ */ (0, import_jsx_runtime34.jsx)("span", { children: "24h" })
        ] }),
        rows.map((r2) => {
          const isSel = active && r2.route_key === active.route_key;
          return /* @__PURE__ */ (0, import_jsx_runtime34.jsxs)(
            "div",
            {
              className: `routing__row ${isSel ? "is-selected" : ""}`,
              onClick: () => setSel(r2.route_key),
              "data-testid": `routing-route:${r2.route_key}`,
              children: [
                /* @__PURE__ */ (0, import_jsx_runtime34.jsx)("span", { className: "routing__intent mono", children: r2.route_key }),
                /* @__PURE__ */ (0, import_jsx_runtime34.jsx)("span", { className: "mono dim", children: r2.channel || "\u2014" }),
                /* @__PURE__ */ (0, import_jsx_runtime34.jsx)("span", { className: "mono", children: r2.recipient }),
                /* @__PURE__ */ (0, import_jsx_runtime34.jsx)("span", { className: "dim", children: r2.sink }),
                /* @__PURE__ */ (0, import_jsx_runtime34.jsx)("span", { className: "mono dim", children: r2.target_module }),
                /* @__PURE__ */ (0, import_jsx_runtime34.jsx)("span", { className: "mono", children: trafficForRoute(r2.route_key) })
              ]
            },
            r2.route_key
          );
        }),
        rows.length === 0 && /* @__PURE__ */ (0, import_jsx_runtime34.jsx)("div", { style: { padding: 24, color: "var(--ink-dim)", fontFamily: "var(--mono)", fontSize: 12 }, children: "No routes configured." })
      ] }),
      /* @__PURE__ */ (0, import_jsx_runtime34.jsx)("aside", { className: "routing__flow", children: active && /* @__PURE__ */ (0, import_jsx_runtime34.jsxs)(import_jsx_runtime34.Fragment, { children: [
        /* @__PURE__ */ (0, import_jsx_runtime34.jsx)("div", { className: "rf__title", children: "Flow" }),
        /* @__PURE__ */ (0, import_jsx_runtime34.jsxs)("div", { className: "rf__diagram", children: [
          /* @__PURE__ */ (0, import_jsx_runtime34.jsxs)("div", { className: "rf__node rf__node--intent", children: [
            /* @__PURE__ */ (0, import_jsx_runtime34.jsx)("div", { className: "rf__lbl", children: "Route" }),
            /* @__PURE__ */ (0, import_jsx_runtime34.jsx)("div", { className: "rf__val mono", children: active.route_key })
          ] }),
          /* @__PURE__ */ (0, import_jsx_runtime34.jsx)("svg", { className: "rf__arrow", viewBox: "0 0 40 12", children: /* @__PURE__ */ (0, import_jsx_runtime34.jsx)("path", { d: "M0 6 H 34 M 28 2 L 34 6 L 28 10", stroke: "currentColor", fill: "none", strokeWidth: "1" }) }),
          /* @__PURE__ */ (0, import_jsx_runtime34.jsxs)("div", { className: "rf__node rf__node--handler", children: [
            /* @__PURE__ */ (0, import_jsx_runtime34.jsxs)("div", { className: "rf__lbl", children: [
              "via ",
              active.sink
            ] }),
            /* @__PURE__ */ (0, import_jsx_runtime34.jsx)("div", { className: "rf__val mono", children: active.recipient })
          ] }),
          /* @__PURE__ */ (0, import_jsx_runtime34.jsx)("svg", { className: "rf__arrow rf__arrow--drop", viewBox: "0 0 12 40", children: /* @__PURE__ */ (0, import_jsx_runtime34.jsx)("path", { d: "M6 0 V 34 M 2 28 L 6 34 L 10 28", stroke: "currentColor", fill: "none", strokeWidth: "1" }) }),
          /* @__PURE__ */ (0, import_jsx_runtime34.jsxs)("div", { className: "rf__node rf__node--gate", children: [
            /* @__PURE__ */ (0, import_jsx_runtime34.jsx)("div", { className: "rf__lbl", children: "Module" }),
            /* @__PURE__ */ (0, import_jsx_runtime34.jsx)("div", { className: "rf__val mono", children: active.target_module })
          ] })
        ] }),
        /* @__PURE__ */ (0, import_jsx_runtime34.jsxs)("div", { className: "rf__stats", children: [
          /* @__PURE__ */ (0, import_jsx_runtime34.jsxs)("div", { children: [
            /* @__PURE__ */ (0, import_jsx_runtime34.jsx)("dt", { children: "Retry max" }),
            /* @__PURE__ */ (0, import_jsx_runtime34.jsx)("dd", { children: active.retry_max ?? "\u2014" })
          ] }),
          /* @__PURE__ */ (0, import_jsx_runtime34.jsxs)("div", { children: [
            /* @__PURE__ */ (0, import_jsx_runtime34.jsx)("dt", { children: "Backoff" }),
            /* @__PURE__ */ (0, import_jsx_runtime34.jsx)("dd", { children: active.backoff_ms ? `${active.backoff_ms} ms` : "\u2014" })
          ] }),
          /* @__PURE__ */ (0, import_jsx_runtime34.jsxs)("div", { children: [
            /* @__PURE__ */ (0, import_jsx_runtime34.jsx)("dt", { children: "Rate limit" }),
            /* @__PURE__ */ (0, import_jsx_runtime34.jsx)("dd", { children: active.rate_limit_per_minute ? `${active.rate_limit_per_minute}/m` : "\u2014" })
          ] })
        ] }),
        /* @__PURE__ */ (0, import_jsx_runtime34.jsx)("div", { className: "rf__title", style: { marginTop: 12 }, children: "Recent deliveries" }),
        /* @__PURE__ */ (0, import_jsx_runtime34.jsxs)("div", { style: { display: "flex", flexDirection: "column", gap: 4, fontFamily: "var(--mono)", fontSize: 11, color: "var(--ink-muted)" }, children: [
          recentDeliveries.filter((d) => d.route_id === active.route_key).slice(0, 8).map((d) => /* @__PURE__ */ (0, import_jsx_runtime34.jsxs)("div", { "data-testid": `routing-delivery:${d.delivery_id}`, children: [
            /* @__PURE__ */ (0, import_jsx_runtime34.jsx)("span", { style: { color: d.status === "delivered" ? "var(--ok)" : d.status === "failed" ? "var(--crit)" : "var(--warn)" }, children: d.status }),
            " ",
            /* @__PURE__ */ (0, import_jsx_runtime34.jsxs)("span", { className: "dim", children: [
              "\xB7 ",
              d.delivery_id.slice(0, 8)
            ] }),
            " ",
            /* @__PURE__ */ (0, import_jsx_runtime34.jsxs)("span", { children: [
              "\u2192 ",
              d.recipient
            ] })
          ] }, d.delivery_id)),
          recentDeliveries.filter((d) => d.route_id === active.route_key).length === 0 && /* @__PURE__ */ (0, import_jsx_runtime34.jsx)("span", { className: "dim", children: "No recent deliveries." })
        ] })
      ] }) })
    ] })
  ] });
}

// src/panels/LogsPanel.tsx
var import_react27 = __toESM(require("react"));
var import_jsx_runtime35 = require("react/jsx-runtime");
var INTERNAL_LOG_EVENTS = /* @__PURE__ */ new Set([
  "keep-alive",
  "snapshot_complete",
  "snapshot_started",
  "subscribed"
]);
function isLogFrameVisible(frame) {
  if (INTERNAL_LOG_EVENTS.has(frame.event)) return false;
  return true;
}
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
var HIDDEN_HISTORY_BLOCK_TYPES = /* @__PURE__ */ new Set([
  "reasoning",
  "server_tool_content",
  "tool_call",
  "tool_result",
  "tool_results",
  "tool_use"
]);
function isRecord2(value) {
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
  if (!isRecord2(value)) return value;
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
  if (!isRecord2(value)) return null;
  for (const key of ["text", "body", "content", "result", "summary"]) {
    const child = value[key];
    if (typeof child === "string" && child.trim()) {
      return child.trim();
    }
  }
  const data = value.data;
  if (isRecord2(data)) {
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
function preferredLogSummary(frame, data) {
  if (frame.event === "user_input") {
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
function summarizeLogFrame(frame) {
  const sanitized = sanitizeLogFrameData(frame.data);
  const d = isRecord2(sanitized) ? sanitized : {};
  const preferred = preferredLogSummary(frame, d);
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
function formatFrameData(frame) {
  const data = sanitizeLogFrameData(frame.data ?? null);
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
  const [q, setQ] = import_react27.default.useState("");
  const [lvl, setLvl] = import_react27.default.useState("all");
  const [expanded, setExpanded] = import_react27.default.useState(/* @__PURE__ */ new Set());
  const toggle = (key) => setExpanded((prev) => {
    const next = new Set(prev);
    if (next.has(key)) next.delete(key);
    else next.add(key);
    return next;
  });
  const rows = import_react27.default.useMemo(() => {
    return frames.filter(isLogFrameVisible).map((f) => ({ f, level: levelFor(f) })).filter(({ f, level }) => {
      if (lvl !== "all" && level !== lvl) return false;
      if (!q) return true;
      const needle = q.toLowerCase();
      return f.event.toLowerCase().includes(needle) || (f.identity || "").toLowerCase().includes(needle);
    });
  }, [frames, q, lvl]);
  const counts = import_react27.default.useMemo(() => {
    const c = { info: 0, warn: 0, error: 0 };
    frames.filter(isLogFrameVisible).forEach((f) => {
      c[levelFor(f)]++;
    });
    return c;
  }, [frames]);
  const visibleTotal = counts.info + counts.warn + counts.error;
  return /* @__PURE__ */ (0, import_jsx_runtime35.jsxs)("div", { className: "view logs", "data-testid": "logs-panel", children: [
    /* @__PURE__ */ (0, import_jsx_runtime35.jsxs)("div", { className: "view__head", children: [
      /* @__PURE__ */ (0, import_jsx_runtime35.jsx)("h2", { children: "Logs" }),
      /* @__PURE__ */ (0, import_jsx_runtime35.jsxs)("span", { className: "view__sub", children: [
        rows.length,
        " of ",
        visibleTotal,
        " events \xB7 live"
      ] }),
      /* @__PURE__ */ (0, import_jsx_runtime35.jsx)("span", { className: "view__spacer" }),
      /* @__PURE__ */ (0, import_jsx_runtime35.jsx)(
        "input",
        {
          className: "view__search",
          placeholder: "Filter event, identity\u2026",
          value: q,
          onChange: (e) => setQ(e.target.value)
        }
      ),
      /* @__PURE__ */ (0, import_jsx_runtime35.jsxs)("div", { className: "view__segs", children: [
        /* @__PURE__ */ (0, import_jsx_runtime35.jsxs)("button", { className: lvl === "all" ? "is-active" : "", onClick: () => setLvl("all"), children: [
          "all ",
          /* @__PURE__ */ (0, import_jsx_runtime35.jsx)("span", { className: "n", children: visibleTotal })
        ] }),
        /* @__PURE__ */ (0, import_jsx_runtime35.jsxs)("button", { className: lvl === "info" ? "is-active" : "", onClick: () => setLvl("info"), children: [
          "info ",
          /* @__PURE__ */ (0, import_jsx_runtime35.jsx)("span", { className: "n", children: counts.info })
        ] }),
        /* @__PURE__ */ (0, import_jsx_runtime35.jsxs)("button", { className: `warn ${lvl === "warn" ? "is-active" : ""}`, onClick: () => setLvl("warn"), children: [
          "warn ",
          /* @__PURE__ */ (0, import_jsx_runtime35.jsx)("span", { className: "n", children: counts.warn })
        ] }),
        /* @__PURE__ */ (0, import_jsx_runtime35.jsxs)("button", { className: `bad ${lvl === "error" ? "is-active" : ""}`, onClick: () => setLvl("error"), children: [
          "err ",
          /* @__PURE__ */ (0, import_jsx_runtime35.jsx)("span", { className: "n", children: counts.error })
        ] })
      ] })
    ] }),
    /* @__PURE__ */ (0, import_jsx_runtime35.jsx)("div", { className: "logs__body", children: /* @__PURE__ */ (0, import_jsx_runtime35.jsxs)("div", { className: "logs__stream", children: [
      rows.map(({ f, level }, i) => {
        const key = f.id || `${f.event}:${f.timestampMs}:${i}`;
        const isOpen = expanded.has(key);
        const hasStructured = hasStructuredOutput(f);
        return /* @__PURE__ */ (0, import_jsx_runtime35.jsxs)(
          "div",
          {
            className: `logline logline--${level}${isOpen ? " is-open" : ""}`,
            "data-testid": `log-line:${f.id || i}`,
            children: [
              /* @__PURE__ */ (0, import_jsx_runtime35.jsxs)(
                "button",
                {
                  type: "button",
                  className: "logline__row",
                  onClick: () => toggle(key),
                  "aria-expanded": isOpen,
                  "data-testid": `log-line:${f.id || i}:toggle`,
                  children: [
                    /* @__PURE__ */ (0, import_jsx_runtime35.jsx)("span", { className: "logline__chevron", children: isOpen ? "\u25BE" : "\u25B8" }),
                    /* @__PURE__ */ (0, import_jsx_runtime35.jsx)("span", { className: "logline__t", children: formatTime2(f.timestampMs) }),
                    /* @__PURE__ */ (0, import_jsx_runtime35.jsx)("span", { className: `logline__lvl logline__lvl--${level}`, children: level.toUpperCase() }),
                    /* @__PURE__ */ (0, import_jsx_runtime35.jsx)("span", { className: "logline__src", children: f.identity || "_system" }),
                    /* @__PURE__ */ (0, import_jsx_runtime35.jsx)("span", { className: "logline__evt", children: f.event }),
                    /* @__PURE__ */ (0, import_jsx_runtime35.jsx)("span", { className: "logline__ctx dim", children: f.interactionId ? `int=${f.interactionId.slice(0, 8)}` : "" }),
                    /* @__PURE__ */ (0, import_jsx_runtime35.jsx)("span", { className: "logline__msg", children: summarizeLogFrame(f) }),
                    hasStructured && /* @__PURE__ */ (0, import_jsx_runtime35.jsx)("span", { className: "logline__badge", title: "Carries structured_output", children: "\u21B3 struct" })
                  ]
                }
              ),
              isOpen && /* @__PURE__ */ (0, import_jsx_runtime35.jsx)("pre", { className: "logline__detail", "data-testid": `log-line:${f.id || i}:detail`, children: formatFrameData(f) })
            ]
          },
          key
        );
      }),
      rows.length === 0 && /* @__PURE__ */ (0, import_jsx_runtime35.jsx)("div", { style: { padding: 24, color: "var(--ink-dim)", fontFamily: "var(--mono)", fontSize: 12 }, children: "No matching events." })
    ] }) })
  ] });
}

// src/panels/Topbar.tsx
var import_jsx_runtime36 = require("react/jsx-runtime");
function PanelGlyph({ side, open }) {
  const dividerLeft = side === "left";
  const cx = dividerLeft ? 16.5 : 7.5;
  const point = open ? dividerLeft ? 1 : -1 : dividerLeft ? -1 : 1;
  const x1 = cx + point * 1.6;
  const x2 = cx - point * 1.6;
  return /* @__PURE__ */ (0, import_jsx_runtime36.jsxs)(
    "svg",
    {
      viewBox: "0 0 24 24",
      "aria-hidden": "true",
      focusable: "false",
      children: [
        /* @__PURE__ */ (0, import_jsx_runtime36.jsx)("rect", { x: "3", y: "5", width: "18", height: "14", rx: "1.5" }),
        /* @__PURE__ */ (0, import_jsx_runtime36.jsx)("path", { d: dividerLeft ? "M9 5 L9 19" : "M15 5 L15 19" }),
        /* @__PURE__ */ (0, import_jsx_runtime36.jsx)("path", { d: `M${x1} 9.5 L${x2} 12 L${x1} 14.5` })
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
  return /* @__PURE__ */ (0, import_jsx_runtime36.jsxs)("div", { className: "mobkit-topbar", "data-testid": "mobkit-topbar", children: [
    /* @__PURE__ */ (0, import_jsx_runtime36.jsx)(
      "button",
      {
        type: "button",
        className: "mobkit-topbar__toggle mobkit-topbar__toggle--left",
        onClick: onToggleSidebar,
        "aria-pressed": !sidebarCollapsed,
        "aria-label": sidebarCollapsed ? "Expand sidebar" : "Collapse sidebar",
        title: sidebarCollapsed ? "Expand sidebar" : "Collapse sidebar",
        "data-testid": "sidebar-collapse-toggle",
        children: /* @__PURE__ */ (0, import_jsx_runtime36.jsx)(PanelGlyph, { side: "left", open: !sidebarCollapsed })
      }
    ),
    /* @__PURE__ */ (0, import_jsx_runtime36.jsxs)("div", { className: "mobkit-topbar__brand", children: [
      brandLogoUrl ? /* @__PURE__ */ (0, import_jsx_runtime36.jsx)("img", { className: "mobkit-topbar__brand-logo", src: brandLogoUrl, alt: brandLogoAlt || brandLabel }) : /* @__PURE__ */ (0, import_jsx_runtime36.jsx)("span", { className: "mobkit-topbar__brand-mark" }),
      /* @__PURE__ */ (0, import_jsx_runtime36.jsx)("span", { children: brandLabel })
    ] }),
    /* @__PURE__ */ (0, import_jsx_runtime36.jsxs)("div", { className: "mobkit-topbar__mob", children: [
      /* @__PURE__ */ (0, import_jsx_runtime36.jsx)("span", { className: "mobkit-topbar__mob-status", title: mobStatus }),
      /* @__PURE__ */ (0, import_jsx_runtime36.jsx)("span", { children: mobName }),
      /* @__PURE__ */ (0, import_jsx_runtime36.jsxs)("span", { className: "dim", children: [
        "\xB7 ",
        mobStatus
      ] })
    ] }),
    /* @__PURE__ */ (0, import_jsx_runtime36.jsxs)("div", { className: "mobkit-topbar__mob", children: [
      /* @__PURE__ */ (0, import_jsx_runtime36.jsx)("span", { children: "env:" }),
      /* @__PURE__ */ (0, import_jsx_runtime36.jsx)("span", { children: environment })
    ] }),
    /* @__PURE__ */ (0, import_jsx_runtime36.jsx)("div", { className: "mobkit-topbar__spacer" }),
    /* @__PURE__ */ (0, import_jsx_runtime36.jsx)("div", { className: "mobkit-topbar__util", children: /* @__PURE__ */ (0, import_jsx_runtime36.jsx)(
      "button",
      {
        type: "button",
        onClick: onToggleTheme,
        "data-testid": "theme-toggle",
        title: `Switch to ${theme === "dark" ? "light" : "dark"} mode`,
        children: theme === "dark" ? "\u263E dark" : "\u2600 light"
      }
    ) }),
    railVisible ? /* @__PURE__ */ (0, import_jsx_runtime36.jsx)(
      "button",
      {
        type: "button",
        className: "mobkit-topbar__toggle mobkit-topbar__toggle--right",
        onClick: onToggleRail,
        "aria-pressed": !railCollapsed,
        "aria-label": railCollapsed ? "Expand signals rail" : "Collapse signals rail",
        title: railCollapsed ? "Expand signals rail" : "Collapse signals rail",
        "data-testid": "signals-rail-collapse-toggle",
        children: /* @__PURE__ */ (0, import_jsx_runtime36.jsx)(PanelGlyph, { side: "right", open: !railCollapsed })
      }
    ) : null
  ] });
}

// src/panels/Tweaks.tsx
var import_react28 = __toESM(require("react"));
var VARIANT_STORAGE = "mobkit-console-variant";
function useConsoleVariant() {
  const [v, setV] = import_react28.default.useState(() => {
    try {
      const stored = localStorage.getItem(VARIANT_STORAGE);
      if (stored === "rams" || stored === "terminal" || stored === "graphite") return stored;
    } catch {
    }
    return "rams";
  });
  const set = import_react28.default.useCallback((next) => {
    setV(next);
    try {
      localStorage.setItem(VARIANT_STORAGE, next);
    } catch {
    }
  }, []);
  return [v, set];
}

// src/panels/Sidebar.tsx
var import_react29 = __toESM(require("react"));
var import_jsx_runtime37 = require("react/jsx-runtime");
var ALL_NAV = ["topology", "timeline", "gating", "roster", "routing", "logs", "health", "access", "memory", "workgraph"];
var NAV_LABEL = {
  topology: "Topology",
  timeline: "Today",
  gating: "Approvals",
  roster: "Roster",
  routing: "Routing",
  logs: "Logs",
  health: "Health",
  access: "Access",
  memory: "Memory",
  workgraph: "WorkGraph"
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
var SIDEBAR_ROW_HEIGHT = {
  section: 36,
  empty: 58,
  subgroup: 28,
  agent: 72
};
var SIDEBAR_OVERSCAN_PX = 360;
var PINNED_SECTION_NAME = "Pinned";
var PINNED_SECTION_KEY = "section:__mobkit_pinned";
function localSidebarStorage() {
  if (typeof window === "undefined") return null;
  try {
    return window.localStorage;
  } catch {
    return null;
  }
}
function isWorkerish(a) {
  const haystack = [a.label, a.identity, a.member_id, a.role].filter(Boolean).join(" ").toLowerCase();
  return haystack.includes("worker") || haystack.includes("delegate") || haystack.includes("helper");
}
function isCommanderLike(a) {
  if (isWorkerish(a)) return false;
  const haystack = [a.label, a.identity, a.member_id, a.role].filter(Boolean).join(" ").toLowerCase();
  return haystack.includes("commander") || haystack.includes("coordinator");
}
function agentKeys(a) {
  return [a?.identity, a?.member_id, a?.agent_id].filter((value) => Boolean(value)).map((value) => value.toLowerCase());
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
function isWiredTo(a, host) {
  if (!host) return false;
  const wiredTo = a.wired_to || [];
  return agentKeys(host).some(
    (key) => wiredTo.some((peer) => referenceMatchesAgentKey(peer, key))
  );
}
function isSpawnedDelegateLike(a, host) {
  if (!isWorkerish(a)) return false;
  if (isWiredTo(a, host)) return true;
  if (a.labels?.group?.trim() || a.labels?.console_group?.trim()) return false;
  const role = (a.role || "").toLowerCase();
  const group = (a.group || "").toLowerCase();
  return !group || group === role || group === "worker" || group === "delegate" || group.includes("helper");
}
function explicitHostId(a) {
  return a.labels?.delegate_host_identity || a.labels?.host_identity || a.labels?.parent_identity || null;
}
function findSpawnHost(a, agents, commander) {
  if (!isWorkerish(a)) return null;
  const explicitHost = explicitHostId(a);
  if (explicitHost) {
    const match = agents.find(
      (candidate) => candidate.member_id !== a.member_id && agentKeys(candidate).some((key) => referenceMatchesAgentKey(explicitHost, key))
    );
    if (match) return match;
  }
  const commanderHost = agents.find(
    (candidate) => candidate.member_id !== a.member_id && isCommanderLike(candidate) && isWiredTo(a, candidate)
  );
  if (commanderHost) return commanderHost;
  const wiredNonWorkerHost = agents.find(
    (candidate) => candidate.member_id !== a.member_id && !isWorkerish(candidate) && isWiredTo(a, candidate)
  );
  if (wiredNonWorkerHost) return wiredNonWorkerHost;
  const workerHost = agents.find(
    (candidate) => candidate.member_id !== a.member_id && isWorkerish(candidate) && isWiredTo(a, candidate)
  );
  if (workerHost) return workerHost;
  if (commander && commander.member_id !== a.member_id && isSpawnedDelegateLike(a, commander)) return commander;
  return null;
}
function sidebarPinnedFamilyPinIds(agent, agents) {
  const host = agents.find(isCommanderLike);
  const byId = new Map(agents.map((candidate) => [candidate.member_id, candidate]));
  const childrenById = /* @__PURE__ */ new Map();
  for (const candidate of agents) {
    const parent = findSpawnHost(candidate, agents, host || null);
    if (!parent) continue;
    if (!childrenById.has(parent.member_id)) childrenById.set(parent.member_id, []);
    childrenById.get(parent.member_id).push(candidate);
  }
  const ids = /* @__PURE__ */ new Set();
  const visit = (current) => {
    if (!current || ids.has(current.member_id)) return;
    ids.add(sidebarAgentPinId2(current));
    ids.add(current.member_id);
    for (const child of childrenById.get(current.member_id) || []) visit(byId.get(child.member_id));
  };
  visit(agent);
  return ids;
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
  const chain = [];
  let current = agent;
  const seen = /* @__PURE__ */ new Set();
  while (current && !seen.has(current.member_id)) {
    seen.add(current.member_id);
    chain.push(current);
    if (!parentById || !byId) break;
    const parentId = parentById.get(current.member_id);
    if (!parentId) break;
    current = byId.get(parentId);
  }
  const searchOrder = chain.length > 1 ? [...chain].reverse() : chain;
  for (const candidate of searchOrder) {
    const value = firstConfiguredValue(candidate, selectors);
    if (value) return value;
  }
  return config?.fallback_group?.trim() || "Agents";
}
function configuredAgentSubgroup(agent, config, parentById, byId) {
  const selectors = configuredSelectors(config, "subgroup_by");
  if (selectors.length === 0) return null;
  const chain = [];
  let current = agent;
  const seen = /* @__PURE__ */ new Set();
  while (current && !seen.has(current.member_id)) {
    seen.add(current.member_id);
    chain.push(current);
    if (!parentById || !byId) break;
    const parentId = parentById.get(current.member_id);
    if (!parentId) break;
    current = byId.get(parentId);
  }
  const searchOrder = chain.length > 1 ? [...chain].reverse() : chain;
  for (const candidate of searchOrder) {
    const value = firstConfiguredValue(candidate, selectors);
    if (value) return value;
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
function bucketForAgent(a, parentById, byId) {
  const seen = /* @__PURE__ */ new Set();
  let current = a;
  while (current) {
    if (seen.has(current.member_id)) break;
    seen.add(current.member_id);
    const parentId = parentById.get(current.member_id);
    if (!parentId) break;
    current = byId.get(parentId);
  }
  return bucketOf(current || a);
}
function depthForAgent(a, parentById) {
  const seen = /* @__PURE__ */ new Set();
  let depth = 0;
  let current = a.member_id;
  while (parentById.has(current) && !seen.has(current)) {
    seen.add(current);
    depth += 1;
    current = parentById.get(current);
  }
  return depth;
}
function compareRows(host, orderSubgroups = false) {
  return (a, b) => {
    if (orderSubgroups && a.subgroup !== b.subgroup) {
      if (!a.subgroup) return 1;
      if (!b.subgroup) return -1;
      return a.subgroup.localeCompare(b.subgroup);
    }
    if (host) {
      if (a.agent.member_id === host.member_id) return -1;
      if (b.agent.member_id === host.member_id) return 1;
    }
    if (a.childOfHost !== b.childOfHost) return a.childOfHost ? -1 : 1;
    return a.agent.label.localeCompare(b.agent.label);
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
function orderRowsPreorderByIndex(rows, orderIndex) {
  const byParent = /* @__PURE__ */ new Map();
  const rowById = new Map(rows.map((row) => [row.agent.member_id, row]));
  const roots = [];
  for (const row of rows) {
    const parentId = row.parentMemberId || void 0;
    if (parentId && rowById.has(parentId)) {
      if (!byParent.has(parentId)) byParent.set(parentId, []);
      byParent.get(parentId).push(row);
    } else {
      roots.push(row);
    }
  }
  const sortByExistingOrder = (a, b) => {
    return (orderIndex.get(a.agent.member_id) ?? Number.MAX_SAFE_INTEGER) - (orderIndex.get(b.agent.member_id) ?? Number.MAX_SAFE_INTEGER);
  };
  roots.sort(sortByExistingOrder);
  for (const children of byParent.values()) children.sort(sortByExistingOrder);
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
  const byId = new Map(filtered.map((a) => [a.member_id, a]));
  const parentById = /* @__PURE__ */ new Map();
  for (const a of filtered) {
    const parent = findSpawnHost(a, filtered, host || null);
    if (parent) parentById.set(a.member_id, parent.member_id);
  }
  for (const a of filtered) {
    const childOfHost = parentById.has(a.member_id);
    const configuredGroup = configuredAgentGroup(a, config, parentById, byId);
    const key = configuredGroup || bucketForAgent(a, parentById, byId);
    const subgroup = configuredAgentSubgroup(a, config, parentById, byId);
    if (!g.has(key)) g.set(key, []);
    g.get(key).push({
      agent: a,
      childOfHost,
      depth: depthForAgent(a, parentById),
      parentMemberId: parentById.get(a.member_id) || null,
      subgroup
    });
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
  const rank = new Map(order.map((name, index) => [name.toLowerCase(), index]));
  return names.sort((a, b) => {
    const ar = rank.get(a.toLowerCase()) ?? Number.MAX_SAFE_INTEGER;
    const br = rank.get(b.toLowerCase()) ?? Number.MAX_SAFE_INTEGER;
    if (ar !== br) return ar - br;
    return a.localeCompare(b);
  });
}
function sectionConfigFor(name, config) {
  const needle = name.toLowerCase();
  return (config?.sections || []).find((section) => section.name?.toLowerCase() === needle) || null;
}
function defaultCollapsedSections(config) {
  return new Set((config?.sections || []).filter((section) => section.collapsed === true).map((section) => section.name));
}
function collapsedSectionsForStorage(config, storageKey, storage = localSidebarStorage()) {
  return readSidebarStringSet(storage, storageKey) ?? defaultCollapsedSections(config);
}
function collapsedSubgroupsForStorage(storageKey, storage = localSidebarStorage()) {
  return readSidebarStringSet(storage, storageKey) ?? /* @__PURE__ */ new Set();
}
function sidebarSubgroupStorageId(bucket, subgroup) {
  return JSON.stringify([bucket, subgroup]);
}
function sidebarSubgroupStorageLabel(storageKey) {
  try {
    const parsed = JSON.parse(storageKey);
    if (Array.isArray(parsed) && typeof parsed[1] === "string" && parsed[1].trim()) {
      return parsed[1];
    }
  } catch {
  }
  return storageKey;
}
function reorderSidebarOrderWithNavigationModel(baseOrder, draggedId, target, where, inputSource) {
  const allowed = new Set(baseOrder);
  const result = applyConsoleNavigationReorderIntent({
    orientation: "vertical",
    nodes: baseOrder.map((id) => ({
      type: "item",
      id,
      label: id
    })),
    order: { orderedNodeIds: baseOrder }
  }, {
    id: draggedId,
    targetId: target,
    position: where,
    scope: "siblings",
    inputSource
  });
  return result.model.order.orderedNodeIds.filter((id) => allowed.has(id));
}
function collectPinnedRows(rows, pinnedAgentIds) {
  const pinned = /* @__PURE__ */ new Set();
  if (!pinnedAgentIds || pinnedAgentIds.size === 0) return pinned;
  const rowById = new Map(rows.map((row) => [row.agent.member_id, row]));
  const childrenById = /* @__PURE__ */ new Map();
  for (const row of rows) {
    if (!row.parentMemberId) continue;
    if (!childrenById.has(row.parentMemberId)) childrenById.set(row.parentMemberId, []);
    childrenById.get(row.parentMemberId).push(row);
  }
  const includeAncestors = (row) => {
    let current = row;
    const seen = /* @__PURE__ */ new Set();
    while (current && !seen.has(current.agent.member_id)) {
      seen.add(current.agent.member_id);
      pinned.add(current.agent.member_id);
      current = current.parentMemberId ? rowById.get(current.parentMemberId) : void 0;
    }
  };
  const includeDescendants = (row) => {
    for (const child of childrenById.get(row.agent.member_id) || []) {
      if (pinned.has(child.agent.member_id)) continue;
      pinned.add(child.agent.member_id);
      includeDescendants(child);
    }
  };
  for (const row of rows) {
    if (!isAgentPinned2(row.agent, pinnedAgentIds)) continue;
    includeAncestors(row);
    includeDescendants(row);
  }
  return pinned;
}
function orderRowsBySubgroupOrder(rows, bucket, subgroupOrder) {
  if (rows.length <= 1) return rows;
  const orderIndex = new Map(rows.map((row, index) => [row.agent.member_id, index]));
  const defaultSubgroups = rows.map((row) => row.subgroup).filter((value) => Boolean(value));
  const subgroupIds = applyConsoleSidebarOrder(
    Array.from(new Set(defaultSubgroups)).map((subgroup) => sidebarSubgroupStorageId(bucket, subgroup)),
    subgroupOrder
  );
  const subgroupRank = new Map(subgroupIds.map((id, index) => [id, index]));
  const byParent = /* @__PURE__ */ new Map();
  const rowById = new Map(rows.map((row) => [row.agent.member_id, row]));
  const roots = [];
  for (const row of rows) {
    const parentId = row.parentMemberId || void 0;
    if (parentId && rowById.has(parentId)) {
      if (!byParent.has(parentId)) byParent.set(parentId, []);
      byParent.get(parentId).push(row);
    } else {
      roots.push(row);
    }
  }
  const sortBySubgroup = (a, b) => {
    const ar = a.subgroup ? subgroupRank.get(sidebarSubgroupStorageId(bucket, a.subgroup)) ?? Number.MAX_SAFE_INTEGER : Number.MAX_SAFE_INTEGER;
    const br = b.subgroup ? subgroupRank.get(sidebarSubgroupStorageId(bucket, b.subgroup)) ?? Number.MAX_SAFE_INTEGER : Number.MAX_SAFE_INTEGER;
    if (ar !== br) return ar - br;
    return (orderIndex.get(a.agent.member_id) ?? 0) - (orderIndex.get(b.agent.member_id) ?? 0);
  };
  roots.sort(sortBySubgroup);
  for (const children of byParent.values()) children.sort(
    (a, b) => (orderIndex.get(a.agent.member_id) ?? 0) - (orderIndex.get(b.agent.member_id) ?? 0)
  );
  const ordered = [];
  const visit = (row) => {
    ordered.push(row);
    for (const child of byParent.get(row.agent.member_id) || []) visit(child);
  };
  for (const root of roots) visit(root);
  return ordered;
}
function sidebarFamilyPinIdsByMemberId(grouped) {
  const rows = Array.from(grouped.values()).flat();
  const rowById = new Map(rows.map((row) => [row.agent.member_id, row]));
  const childrenById = /* @__PURE__ */ new Map();
  for (const row of rows) {
    if (!row.parentMemberId) continue;
    if (!childrenById.has(row.parentMemberId)) childrenById.set(row.parentMemberId, []);
    childrenById.get(row.parentMemberId).push(row);
  }
  const familyById = /* @__PURE__ */ new Map();
  const visit = (row, ids) => {
    if (!row || ids.has(row.agent.member_id)) return;
    ids.add(sidebarAgentPinId2(row.agent));
    ids.add(row.agent.member_id);
    for (const child of childrenById.get(row.agent.member_id) || []) visit(rowById.get(child.agent.member_id), ids);
  };
  for (const row of rows) {
    const ids = /* @__PURE__ */ new Set();
    visit(row, ids);
    familyById.set(row.agent.member_id, ids);
  }
  return familyById;
}
function buildSidebarVirtualRows(args) {
  const rows = [];
  const orderedSections = applyConsoleSidebarOrder(args.sectionNames, args.sectionOrder);
  const baseRows = orderedSections.flatMap((bucket) => args.grouped.get(bucket) || []);
  const baseOrderIndex = new Map(baseRows.map((row, index) => [row.agent.member_id, index]));
  const pinnedRowIds = collectPinnedRows(baseRows, args.pinnedAgentIds);
  if (pinnedRowIds.size > 0) {
    const pinnedRows = orderRowsPreorderByIndex(
      baseRows.filter((row) => pinnedRowIds.has(row.agent.member_id)),
      baseOrderIndex
    );
    const collapsedPinned = args.searchActive ? false : args.collapsedSections.has(PINNED_SECTION_NAME);
    rows.push({
      kind: "section",
      key: PINNED_SECTION_KEY,
      bucket: PINNED_SECTION_NAME,
      count: pinnedRows.length,
      collapsed: collapsedPinned,
      pinned: true,
      reorderable: false
    });
    if (!collapsedPinned) {
      for (const row of pinnedRows) {
        rows.push({
          kind: "agent",
          key: `agent:${PINNED_SECTION_NAME}:${row.agent.member_id}`,
          bucket: PINNED_SECTION_NAME,
          row
        });
      }
    }
  }
  for (const bucket of orderedSections) {
    const list = (args.grouped.get(bucket) || []).filter((row) => !pinnedRowIds.has(row.agent.member_id));
    const sectionConfig = sectionConfigFor(bucket, args.grouping);
    if (list.length === 0 && !sectionConfig) continue;
    const collapsedSection = args.searchActive ? false : args.collapsedSections.has(bucket);
    rows.push({
      kind: "section",
      key: `section:${bucket}`,
      bucket,
      count: list.length,
      collapsed: collapsedSection,
      reorderable: true
    });
    if (collapsedSection) continue;
    if (list.length === 0) {
      rows.push({
        kind: "empty",
        key: `empty:${bucket}`,
        bucket,
        sectionConfig
      });
      continue;
    }
    const orderedList = orderRowsBySubgroupOrder(list, bucket, args.subgroupOrder);
    const subgroups = new Set(orderedList.map((row) => row.subgroup).filter((value) => Boolean(value)));
    const showSubgroups = configuredSelectors(args.grouping, "subgroup_by").length > 0 && subgroups.size > (args.grouping?.collapse_single_subgroup === false ? 0 : 1);
    const subgroupCounts = /* @__PURE__ */ new Map();
    for (const row of orderedList) {
      if (!row.subgroup) continue;
      subgroupCounts.set(row.subgroup, (subgroupCounts.get(row.subgroup) || 0) + 1);
    }
    let lastSubgroup = null;
    let currentSubgroupCollapsed = false;
    for (const row of orderedList) {
      if (showSubgroups && !row.subgroup) {
        lastSubgroup = null;
        currentSubgroupCollapsed = false;
      }
      if (showSubgroups && row.subgroup && row.subgroup !== lastSubgroup) {
        lastSubgroup = row.subgroup;
        const storageKey = sidebarSubgroupStorageId(bucket, row.subgroup);
        currentSubgroupCollapsed = args.searchActive ? false : args.collapsedSubgroups.has(storageKey);
        rows.push({
          kind: "subgroup",
          key: `subgroup:${bucket}:${row.subgroup}`,
          bucket,
          label: row.subgroup,
          count: subgroupCounts.get(row.subgroup) || 0,
          collapsed: currentSubgroupCollapsed,
          storageKey,
          reorderable: true
        });
      }
      if (currentSubgroupCollapsed) continue;
      rows.push({
        kind: "agent",
        key: `agent:${row.agent.member_id}`,
        bucket,
        row
      });
    }
  }
  return rows;
}
function sidebarNavigationLabel(row) {
  switch (row.kind) {
    case "section":
      return row.bucket;
    case "subgroup":
      return row.label;
    case "empty":
      return row.sectionConfig?.empty_title || row.sectionConfig?.empty_text || "No agents";
    case "agent":
      return row.row.agent.label;
  }
}
function sidebarNavigationNodeForRow(row) {
  const base = {
    id: row.key,
    label: sidebarNavigationLabel(row),
    target: row
  };
  if (row.kind === "section" || row.kind === "subgroup") {
    return {
      ...base,
      type: "group",
      expanded: !row.collapsed,
      children: []
    };
  }
  return {
    ...base,
    type: "item"
  };
}
function sidebarNavigationModelFromRows(rows) {
  const nodes = [];
  let currentSection = null;
  let currentSubgroup = null;
  for (const row of rows) {
    const node = sidebarNavigationNodeForRow(row);
    if (row.kind === "section") {
      nodes.push(node);
      currentSection = node;
      currentSubgroup = null;
      continue;
    }
    if (row.kind === "subgroup") {
      if (currentSection?.type === "group") {
        currentSection.children.push(node);
      } else {
        nodes.push(node);
      }
      currentSubgroup = node;
      continue;
    }
    const belongsToCurrentSubgroup = row.kind === "agent" && Boolean(row.row.subgroup) && currentSubgroup?.type === "group" && currentSubgroup.target?.kind === "subgroup" && currentSubgroup.target.bucket === row.bucket && currentSubgroup.target.label === row.row.subgroup;
    const parent = belongsToCurrentSubgroup ? currentSubgroup : currentSection?.type === "group" ? currentSection : null;
    if (parent) {
      parent.children.push(node);
    } else {
      nodes.push(node);
    }
  }
  return normalizeConsoleNavigationModel({
    orientation: "vertical",
    nodes,
    order: { orderedNodeIds: [] }
  });
}
function buildStockSidebarNavigationModel(args) {
  return sidebarNavigationModelFromRows(buildSidebarVirtualRows(args));
}
function sidebarNavigationRows(model) {
  const normalized = normalizeConsoleNavigationModel(model);
  const rows = [];
  const visit = (node) => {
    if (node.target) rows.push(node.target);
    if (node.type !== "group" || !node.expanded) return;
    for (const child of node.children) visit(child);
  };
  for (const node of normalized.nodes) visit(node);
  return rows;
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
function virtualRowHeight(row) {
  return SIDEBAR_ROW_HEIGHT[row.kind];
}
function sidebarDragPreviewRows(rows, item) {
  if (!item) return [];
  const start = rows.findIndex((row) => {
    if (item.kind === "section") return row.kind === "section" && row.bucket === item.id;
    return row.kind === "subgroup" && row.storageKey === item.id && row.bucket === item.bucket;
  });
  if (start < 0) return [];
  const out = [];
  for (let index = start; index < rows.length; index += 1) {
    const row = rows[index];
    if (index > start) {
      if (item.kind === "section" && row.kind === "section") break;
      if (item.kind === "subgroup" && (row.kind === "section" || row.kind === "subgroup")) break;
    }
    out.push(row);
  }
  return out;
}
function renderSidebarDragPreviewRows(rows) {
  return rows.map((row) => {
    if (row.kind === "section") {
      return /* @__PURE__ */ (0, import_jsx_runtime37.jsxs)(
        "div",
        {
          className: "sidebar__drag-preview-section",
          "data-pinned": row.pinned ? "true" : void 0,
          children: [
            /* @__PURE__ */ (0, import_jsx_runtime37.jsx)("span", { className: "sidebar__sec-label", children: row.bucket }),
            /* @__PURE__ */ (0, import_jsx_runtime37.jsx)("span", { className: "sidebar__sec-spacer" }),
            /* @__PURE__ */ (0, import_jsx_runtime37.jsx)("span", { className: "sidebar__sec-count", children: row.count })
          ]
        },
        `preview:${row.key}`
      );
    }
    if (row.kind === "subgroup") {
      return /* @__PURE__ */ (0, import_jsx_runtime37.jsxs)("div", { className: "sidebar__drag-preview-subgroup", children: [
        /* @__PURE__ */ (0, import_jsx_runtime37.jsx)("span", { children: row.label }),
        /* @__PURE__ */ (0, import_jsx_runtime37.jsx)("span", { className: "sidebar__sec-spacer" }),
        /* @__PURE__ */ (0, import_jsx_runtime37.jsx)("span", { className: "sidebar__sec-count", children: row.count })
      ] }, `preview:${row.key}`);
    }
    if (row.kind === "empty") {
      return /* @__PURE__ */ (0, import_jsx_runtime37.jsx)("div", { className: "sidebar__drag-preview-empty", children: row.sectionConfig?.empty_title || row.sectionConfig?.empty_text || "No agents" }, `preview:${row.key}`);
    }
    return /* @__PURE__ */ (0, import_jsx_runtime37.jsxs)(
      "div",
      {
        className: `sidebar__drag-preview-agent ${row.row.childOfHost ? "sidebar__drag-preview-agent--child" : ""}`,
        "data-depth": row.row.childOfHost ? String(Math.min(row.row.depth, 3)) : void 0,
        children: [
          /* @__PURE__ */ (0, import_jsx_runtime37.jsx)("span", { className: "agent__dot" }),
          /* @__PURE__ */ (0, import_jsx_runtime37.jsxs)("span", { className: "sidebar__drag-preview-agent-body", children: [
            /* @__PURE__ */ (0, import_jsx_runtime37.jsx)("span", { className: "agent__name", children: row.row.agent.label }),
            /* @__PURE__ */ (0, import_jsx_runtime37.jsx)("span", { className: "agent__id", children: row.row.agent.identity || row.row.agent.member_id })
          ] })
        ]
      },
      `preview:${row.key}`
    );
  });
}
function lowerBound(values, needle) {
  let lo = 0;
  let hi = values.length;
  while (lo < hi) {
    const mid = Math.floor((lo + hi) / 2);
    if (values[mid] < needle) lo = mid + 1;
    else hi = mid;
  }
  return lo;
}
function sidebarVirtualOffsets(rows) {
  const offsets = [];
  let total = 0;
  for (const row of rows) {
    offsets.push(total);
    total += virtualRowHeight(row);
  }
  return { offsets, total };
}
function sidebarVisibleRange(args) {
  if (args.rowCount === 0) return { start: 0, end: 0 };
  const startNeedle = Math.max(0, args.scrollTop - SIDEBAR_OVERSCAN_PX);
  const endNeedle = Math.min(args.total, args.scrollTop + Math.max(1, args.listHeight) + SIDEBAR_OVERSCAN_PX);
  const start = Math.max(0, lowerBound(args.offsets, startNeedle) - 1);
  const end = Math.min(args.rowCount, lowerBound(args.offsets, endNeedle) + 1);
  return { start, end };
}
function pendingOrderFocusMatchesRow(pending, row) {
  if (pending.kind === "section") {
    return row.kind === "section" && row.bucket === pending.id;
  }
  return row.kind === "subgroup" && row.storageKey === pending.id && row.bucket === pending.bucket;
}
function pendingOrderFocusMatchesElement(pending, element) {
  if (element.dataset.sidebarOrderKind !== pending.kind) return false;
  if (element.dataset.sidebarOrderId !== pending.id) return false;
  if (pending.kind === "subgroup" && element.dataset.sidebarOrderBucket !== pending.bucket) return false;
  return true;
}
function useMeasuredHeight() {
  const ref = import_react29.default.useRef(null);
  const [height, setHeight] = import_react29.default.useState(0);
  import_react29.default.useLayoutEffect(() => {
    const element = ref.current;
    if (!element) return void 0;
    const update = () => setHeight(element.clientHeight);
    update();
    if (typeof ResizeObserver === "undefined") {
      window.addEventListener("resize", update);
      return () => window.removeEventListener("resize", update);
    }
    const ro = new ResizeObserver((entries) => {
      const box = entries[0]?.contentRect;
      setHeight(box ? box.height : element.clientHeight);
    });
    ro.observe(element);
    return () => ro.disconnect();
  }, []);
  return [ref, height];
}
function renderAgentRow(row, selectedMemberId, recentActivity, grouping, pinnedAgentIds, onSelect, onTogglePinnedAgent, familyPinIds) {
  const { agent, childOfHost, depth } = row;
  const stateAttr = deriveStateAttr(agent);
  const pulse = pulseSamples(recentActivity, agent.identity || agent.member_id);
  const inbox = inboxCount(agent);
  const badges = configuredAgentBadges(agent, grouping);
  const pinned = isAgentPinned2(agent, pinnedAgentIds);
  return /* @__PURE__ */ (0, import_jsx_runtime37.jsxs)(
    "div",
    {
      className: `agent ${childOfHost ? "agent--child" : ""} ${agent.member_id === selectedMemberId ? "is-active" : ""}`,
      "data-state": stateAttr,
      "data-child-of-host": childOfHost ? "true" : void 0,
      "data-depth": childOfHost ? String(Math.min(depth, 3)) : void 0,
      "data-testid": `sidebar-agent:${agent.member_id}`,
      onClick: () => onSelect(agent),
      onKeyDown: (event) => {
        if (event.key === "Enter" || event.key === " ") {
          event.preventDefault();
          onSelect(agent);
        }
      },
      role: "button",
      tabIndex: 0,
      children: [
        /* @__PURE__ */ (0, import_jsx_runtime37.jsx)("span", { className: "agent__dot" }),
        /* @__PURE__ */ (0, import_jsx_runtime37.jsxs)("span", { className: "agent__body", children: [
          /* @__PURE__ */ (0, import_jsx_runtime37.jsx)("span", { className: "agent__name", children: agent.label }),
          /* @__PURE__ */ (0, import_jsx_runtime37.jsx)("span", { className: "agent__id", children: agent.identity || agent.member_id }),
          badges.length > 0 ? /* @__PURE__ */ (0, import_jsx_runtime37.jsx)("span", { className: "agent__badges", children: badges.map((badge) => /* @__PURE__ */ (0, import_jsx_runtime37.jsxs)(
            "span",
            {
              className: "agent__badge",
              "data-tone": badge.tone || "neutral",
              title: `${badge.label}: ${badge.value}`,
              children: [
                /* @__PURE__ */ (0, import_jsx_runtime37.jsx)("span", { children: badge.label }),
                /* @__PURE__ */ (0, import_jsx_runtime37.jsx)("strong", { children: badge.value })
              ]
            },
            badge.id
          )) }) : null
        ] }),
        /* @__PURE__ */ (0, import_jsx_runtime37.jsx)("span", { className: "agent__actions", children: onTogglePinnedAgent ? /* @__PURE__ */ (0, import_jsx_runtime37.jsx)(
          "button",
          {
            type: "button",
            className: "agent__pin",
            "data-active": pinned ? "true" : void 0,
            "aria-label": pinned ? `Unpin ${agent.label}` : `Pin ${agent.label}`,
            "aria-pressed": pinned,
            title: pinned ? "Unpin agent" : "Pin agent",
            "data-testid": `sidebar-agent-pin:${agent.member_id}`,
            onClick: (event) => {
              event.preventDefault();
              event.stopPropagation();
              onTogglePinnedAgent(agent, familyPinIds);
            },
            children: /* @__PURE__ */ (0, import_jsx_runtime37.jsx)(Icon, { name: "i-pin", className: "agent__pin-icon" })
          }
        ) : null }),
        /* @__PURE__ */ (0, import_jsx_runtime37.jsxs)("span", { className: "agent__meta", children: [
          /* @__PURE__ */ (0, import_jsx_runtime37.jsx)("span", { className: "agent__pulse", children: pulse.map((v, i) => /* @__PURE__ */ (0, import_jsx_runtime37.jsx)("span", { style: { height: `${Math.max(1, Math.min(12, v * 2 + 1))}px` } }, i)) }),
          inbox > 0 && /* @__PURE__ */ (0, import_jsx_runtime37.jsx)("span", { className: "agent__inbox", children: inbox })
        ] })
      ]
    }
  );
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
  storageNamespace,
  pinnedAgentIds,
  onSelect,
  onTogglePinnedAgent,
  onOpenControl
}) {
  const [q, setQ] = import_react29.default.useState("");
  const [draggingOrder, setDraggingOrder] = import_react29.default.useState(null);
  const [dragOverOrder, setDragOverOrder] = import_react29.default.useState(null);
  const [dragPreview, setDragPreview] = import_react29.default.useState(null);
  const [orderAnnouncement, setOrderAnnouncement] = import_react29.default.useState("");
  const pendingOrderFocusRef = import_react29.default.useRef(null);
  const draggingOrderRef = import_react29.default.useRef(null);
  const pointerDragRef = import_react29.default.useRef(null);
  const suppressOrderClickRef = import_react29.default.useRef(false);
  import_react29.default.useEffect(() => {
    draggingOrderRef.current = draggingOrder;
  }, [draggingOrder]);
  const navKinds = import_react29.default.useMemo(() => {
    const configured = visibleNavKinds();
    if (!visibleControls) return configured;
    const allowed = new Set(visibleControls);
    return configured.filter((kind) => allowed.has(kind));
  }, [visibleControls]);
  const filtered = import_react29.default.useMemo(() => {
    if (!q) return agents;
    const needle = q.toLowerCase();
    return agents.filter(
      (a) => a.label.toLowerCase().includes(needle) || (a.identity || "").toLowerCase().includes(needle) || (a.member_id || "").toLowerCase().includes(needle) || (a.role || "").toLowerCase().includes(needle)
    );
  }, [agents, q]);
  const grouped = import_react29.default.useMemo(() => {
    return groupSidebarAgents(filtered, grouping);
  }, [filtered, grouping]);
  const familyPinIdsByMemberId = import_react29.default.useMemo(() => sidebarFamilyPinIdsByMemberId(grouped), [grouped]);
  const sectionNames = import_react29.default.useMemo(() => orderedSectionNames(grouped, grouping), [grouped, grouping]);
  const defaultCollapsedKey = import_react29.default.useMemo(
    () => JSON.stringify((grouping?.sections || []).map((section) => [section.name, section.collapsed === true])),
    [grouping?.sections]
  );
  const sectionCollapseStorageKey = import_react29.default.useMemo(
    () => sidebarStorageKey(SECTION_COLLAPSE_STORAGE_PREFIX, storageNamespace),
    [storageNamespace]
  );
  const subgroupCollapseStorageKey = import_react29.default.useMemo(
    () => sidebarStorageKey(SUBGROUP_COLLAPSE_STORAGE_PREFIX, storageNamespace),
    [storageNamespace]
  );
  const sectionOrderStorageKey = import_react29.default.useMemo(
    () => sidebarStorageKey(SIDEBAR_SECTION_ORDER_STORAGE_PREFIX, storageNamespace),
    [storageNamespace]
  );
  const subgroupOrderStorageKey = import_react29.default.useMemo(
    () => sidebarStorageKey(SIDEBAR_SUBGROUP_ORDER_STORAGE_PREFIX, storageNamespace),
    [storageNamespace]
  );
  const [collapsedSections, setCollapsedSections] = import_react29.default.useState(() => {
    return collapsedSectionsForStorage(grouping, sectionCollapseStorageKey);
  });
  import_react29.default.useEffect(() => {
    setCollapsedSections(collapsedSectionsForStorage(grouping, sectionCollapseStorageKey));
  }, [defaultCollapsedKey, grouping, sectionCollapseStorageKey]);
  const [collapsedSubgroups, setCollapsedSubgroups] = import_react29.default.useState(() => {
    return collapsedSubgroupsForStorage(subgroupCollapseStorageKey);
  });
  import_react29.default.useEffect(() => {
    setCollapsedSubgroups(collapsedSubgroupsForStorage(subgroupCollapseStorageKey));
  }, [subgroupCollapseStorageKey]);
  const [sectionOrder, setSectionOrder] = import_react29.default.useState(() => {
    return readSidebarStringList(localSidebarStorage(), sectionOrderStorageKey) || [];
  });
  import_react29.default.useEffect(() => {
    setSectionOrder(readSidebarStringList(localSidebarStorage(), sectionOrderStorageKey) || []);
  }, [sectionOrderStorageKey]);
  const [subgroupOrder, setSubgroupOrder] = import_react29.default.useState(() => {
    return readSidebarStringList(localSidebarStorage(), subgroupOrderStorageKey) || [];
  });
  import_react29.default.useEffect(() => {
    setSubgroupOrder(readSidebarStringList(localSidebarStorage(), subgroupOrderStorageKey) || []);
  }, [subgroupOrderStorageKey]);
  const customSidebarButtons = import_react29.default.useMemo(
    () => (customButtons || []).filter((button) => button.id && button.label && (button.control || button.href)),
    [customButtons]
  );
  const completeSectionDrop = import_react29.default.useCallback((target, where, draggedId = draggingOrderRef.current?.id, inputSource = "pointer") => {
    if (!draggedId || draggedId === target) return;
    if (inputSource === "keyboard") {
      pendingOrderFocusRef.current = { kind: "section", id: draggedId };
    }
    setSectionOrder((current) => {
      const baseOrder = applyConsoleSidebarOrder(sectionNames, current);
      const next = reorderSidebarOrderWithNavigationModel(baseOrder, draggedId, target, where, inputSource);
      writeSidebarStringList(localSidebarStorage(), sectionOrderStorageKey, next);
      return next;
    });
    setOrderAnnouncement(`Moved section ${draggedId} ${where} ${target}.`);
  }, [sectionNames, sectionOrderStorageKey]);
  const subgroupIdsForBucket = import_react29.default.useCallback((bucket) => {
    const list = grouped.get(bucket) || [];
    const ids = list.map((row) => row.subgroup).filter((value) => Boolean(value)).map((subgroup) => sidebarSubgroupStorageId(bucket, subgroup));
    return Array.from(new Set(ids));
  }, [grouped]);
  const completeSubgroupDrop = import_react29.default.useCallback((target, bucket, where, draggedId = draggingOrderRef.current?.id, draggedBucket = draggingOrderRef.current?.bucket, inputSource = "pointer") => {
    if (!draggedId || draggedBucket !== bucket || draggedId === target) return;
    if (inputSource === "keyboard") {
      pendingOrderFocusRef.current = { kind: "subgroup", id: draggedId, bucket };
    }
    setSubgroupOrder((current) => {
      const bucketOrder = applyConsoleSidebarOrder(subgroupIdsForBucket(bucket), current);
      const nextBucketOrder = reorderSidebarOrderWithNavigationModel(bucketOrder, draggedId, target, where, inputSource);
      const nextBucketSet = new Set(nextBucketOrder);
      const next = [
        ...current.filter((id) => !nextBucketSet.has(id)),
        ...nextBucketOrder
      ];
      writeSidebarStringList(localSidebarStorage(), subgroupOrderStorageKey, next);
      return next;
    });
    setOrderAnnouncement(`Moved subgroup ${sidebarSubgroupStorageLabel(draggedId)} ${where} ${sidebarSubgroupStorageLabel(target)}.`);
  }, [subgroupIdsForBucket, subgroupOrderStorageKey]);
  const handleSectionOrderKeyDown = import_react29.default.useCallback((event, bucket) => {
    if (!event.altKey || event.key !== "ArrowUp" && event.key !== "ArrowDown") return;
    const ordered = applyConsoleSidebarOrder(sectionNames, sectionOrder);
    const index = ordered.indexOf(bucket);
    const target = event.key === "ArrowUp" ? ordered[index - 1] : ordered[index + 1];
    if (!target) return;
    event.preventDefault();
    completeSectionDrop(target, event.key === "ArrowUp" ? "before" : "after", bucket, "keyboard");
  }, [completeSectionDrop, sectionNames, sectionOrder]);
  const handleSubgroupOrderKeyDown = import_react29.default.useCallback((event, storageKey, bucket) => {
    if (!event.altKey || event.key !== "ArrowUp" && event.key !== "ArrowDown") return;
    const ordered = applyConsoleSidebarOrder(subgroupIdsForBucket(bucket), subgroupOrder);
    const index = ordered.indexOf(storageKey);
    const target = event.key === "ArrowUp" ? ordered[index - 1] : ordered[index + 1];
    if (!target) return;
    event.preventDefault();
    completeSubgroupDrop(target, bucket, event.key === "ArrowUp" ? "before" : "after", storageKey, bucket, "keyboard");
  }, [completeSubgroupDrop, subgroupIdsForBucket, subgroupOrder]);
  const beginPointerOrderDrag = import_react29.default.useCallback((event, item) => {
    if (event.button !== 0) return;
    event.preventDefault();
    pointerDragRef.current = {
      ...item,
      startX: event.clientX,
      startY: event.clientY,
      previewWidth: event.currentTarget.closest(".sidebar")?.getBoundingClientRect().width || event.currentTarget.getBoundingClientRect().width,
      moved: false,
      over: null
    };
    setDraggingOrder(item);
    draggingOrderRef.current = item;
  }, []);
  const movePointerOrderDrag = import_react29.default.useCallback((event) => {
    const drag = pointerDragRef.current;
    if (!drag) return;
    if (!drag.moved && Math.max(Math.abs(event.clientX - drag.startX), Math.abs(event.clientY - drag.startY)) < 4) return;
    drag.moved = true;
    event.preventDefault();
    setDragPreview({
      x: event.clientX,
      y: event.clientY,
      width: drag.previewWidth
    });
    const target = document.elementFromPoint(event.clientX, event.clientY)?.closest("[data-sidebar-order-kind]");
    if (!target) {
      drag.over = null;
      setDragOverOrder(null);
      return;
    }
    const kind = target.dataset.sidebarOrderKind;
    const id = target.dataset.sidebarOrderId;
    const bucket = target.dataset.sidebarOrderBucket;
    if (!kind || !id || kind !== drag.kind || id === drag.id || kind === "subgroup" && bucket !== drag.bucket) {
      drag.over = null;
      setDragOverOrder(null);
      return;
    }
    const rect = target.getBoundingClientRect();
    const where = event.clientY > rect.top + rect.height / 2 ? "after" : "before";
    drag.over = { id, bucket, where };
    setDragOverOrder({ kind, id, where });
  }, []);
  const finishPointerOrderDrag = import_react29.default.useCallback(() => {
    const drag = pointerDragRef.current;
    if (!drag) return;
    pointerDragRef.current = null;
    if (drag.moved && drag.over) {
      if (drag.kind === "section") {
        completeSectionDrop(drag.over.id, drag.over.where, drag.id, "pointer");
      } else if (drag.over.bucket) {
        completeSubgroupDrop(drag.over.id, drag.over.bucket, drag.over.where, drag.id, drag.bucket, "pointer");
      }
      suppressOrderClickRef.current = true;
      window.setTimeout(() => {
        suppressOrderClickRef.current = false;
      }, 0);
    }
    draggingOrderRef.current = null;
    setDraggingOrder(null);
    setDragOverOrder(null);
    setDragPreview(null);
  }, [completeSectionDrop, completeSubgroupDrop]);
  import_react29.default.useEffect(() => {
    if (!draggingOrder) return void 0;
    const onMove = (event) => movePointerOrderDrag(event);
    const onDone = () => finishPointerOrderDrag();
    window.addEventListener("pointermove", onMove);
    window.addEventListener("pointerup", onDone);
    window.addEventListener("pointercancel", onDone);
    return () => {
      window.removeEventListener("pointermove", onMove);
      window.removeEventListener("pointerup", onDone);
      window.removeEventListener("pointercancel", onDone);
    };
  }, [draggingOrder, finishPointerOrderDrag, movePointerOrderDrag]);
  const sidebarNavigationModel = import_react29.default.useMemo(() => {
    return buildStockSidebarNavigationModel({
      sectionNames,
      grouped,
      grouping,
      collapsedSections,
      collapsedSubgroups,
      pinnedAgentIds,
      sectionOrder,
      subgroupOrder,
      searchActive: Boolean(q)
    });
  }, [sectionNames, grouped, grouping, collapsedSections, collapsedSubgroups, pinnedAgentIds, sectionOrder, subgroupOrder, q]);
  const virtualRows = import_react29.default.useMemo(
    () => sidebarNavigationRows(sidebarNavigationModel),
    [sidebarNavigationModel]
  );
  const virtualOffsets = import_react29.default.useMemo(() => sidebarVirtualOffsets(virtualRows), [virtualRows]);
  const [listRef, listHeight] = useMeasuredHeight();
  const [scrollTop, setScrollTop] = import_react29.default.useState(0);
  import_react29.default.useEffect(() => {
    setScrollTop(0);
    if (listRef.current) listRef.current.scrollTop = 0;
  }, [q, grouping, listRef]);
  const visibleRange = import_react29.default.useMemo(() => sidebarVisibleRange({
    rowCount: virtualRows.length,
    offsets: virtualOffsets.offsets,
    total: virtualOffsets.total,
    scrollTop,
    listHeight
  }), [listHeight, scrollTop, virtualOffsets, virtualRows.length]);
  const visibleRows = import_react29.default.useMemo(
    () => virtualRows.slice(visibleRange.start, visibleRange.end),
    [virtualRows, visibleRange]
  );
  import_react29.default.useLayoutEffect(() => {
    const pending = pendingOrderFocusRef.current;
    const list = listRef.current;
    if (!pending || !list) return;
    const rowIndex = virtualRows.findIndex((row) => pendingOrderFocusMatchesRow(pending, row));
    if (rowIndex < 0) {
      pendingOrderFocusRef.current = null;
      return;
    }
    const rowTop = virtualOffsets.offsets[rowIndex] || 0;
    const rowBottom = rowTop + virtualRowHeight(virtualRows[rowIndex]);
    const currentTop = list.scrollTop;
    if (listHeight > 0) {
      const currentBottom = currentTop + listHeight;
      const nextTop = rowTop < currentTop ? rowTop : rowBottom > currentBottom ? Math.max(0, rowBottom - listHeight) : currentTop;
      if (nextTop !== currentTop) {
        list.scrollTop = nextTop;
        setScrollTop(nextTop);
        return;
      }
    }
    const restoreFocus = () => {
      const target = Array.from(list.querySelectorAll("[data-sidebar-order-kind]")).find((element) => pendingOrderFocusMatchesElement(pending, element));
      target?.focus();
      pendingOrderFocusRef.current = null;
    };
    if (typeof window !== "undefined" && typeof window.requestAnimationFrame === "function") {
      window.requestAnimationFrame(restoreFocus);
    } else {
      window.setTimeout(restoreFocus, 0);
    }
  }, [listHeight, listRef, scrollTop, virtualOffsets, virtualRows]);
  const dragPreviewRows = import_react29.default.useMemo(
    () => sidebarDragPreviewRows(virtualRows, draggingOrder),
    [virtualRows, draggingOrder]
  );
  if (collapsed) {
    return /* @__PURE__ */ (0, import_jsx_runtime37.jsx)(
      "aside",
      {
        className: "sidebar sidebar--collapsed",
        "data-collapsed": "true",
        "data-testid": "sidebar-root",
        children: /* @__PURE__ */ (0, import_jsx_runtime37.jsx)("i", { className: "sidebar__grip", "aria-hidden": "true" })
      }
    );
  }
  return /* @__PURE__ */ (0, import_jsx_runtime37.jsxs)("aside", { className: "sidebar", "data-testid": "sidebar-root", children: [
    /* @__PURE__ */ (0, import_jsx_runtime37.jsx)(
      "div",
      {
        "aria-live": "polite",
        "data-testid": "sidebar-reorder-live",
        style: {
          position: "absolute",
          width: 1,
          height: 1,
          padding: 0,
          margin: -1,
          overflow: "hidden",
          clip: "rect(0 0 0 0)",
          whiteSpace: "nowrap",
          border: 0
        },
        children: orderAnnouncement
      }
    ),
    /* @__PURE__ */ (0, import_jsx_runtime37.jsx)("div", { className: "sidebar__mast", children: /* @__PURE__ */ (0, import_jsx_runtime37.jsxs)("div", { children: [
      /* @__PURE__ */ (0, import_jsx_runtime37.jsx)("div", { className: "sidebar__mast-title", children: "Roster" }),
      /* @__PURE__ */ (0, import_jsx_runtime37.jsxs)("div", { className: "sidebar__mast-sub", children: [
        agents.length,
        " agents"
      ] })
    ] }) }),
    /* @__PURE__ */ (0, import_jsx_runtime37.jsx)("div", { className: "sidebar__search", children: /* @__PURE__ */ (0, import_jsx_runtime37.jsx)(
      "input",
      {
        placeholder: "Search roster...",
        value: q,
        onChange: (e) => setQ(e.target.value),
        "data-testid": "sidebar-search"
      }
    ) }),
    (navKinds.length > 0 || customSidebarButtons.length > 0) && /* @__PURE__ */ (0, import_jsx_runtime37.jsxs)("div", { className: "sidebar__section sidebar__section--nav", children: [
      /* @__PURE__ */ (0, import_jsx_runtime37.jsx)("div", { className: "sidebar__sec-head", children: /* @__PURE__ */ (0, import_jsx_runtime37.jsx)("span", { className: "sidebar__sec-label", children: "Workbench" }) }),
      /* @__PURE__ */ (0, import_jsx_runtime37.jsxs)("div", { className: "sidebar__navgrid", children: [
        navKinds.map((kind) => /* @__PURE__ */ (0, import_jsx_runtime37.jsx)(
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
            return /* @__PURE__ */ (0, import_jsx_runtime37.jsx)(
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
            const safeHref = safeConsoleHref(button.href);
            if (!safeHref) {
              return null;
            }
            const target = button.target || void 0;
            const rel = target === "_blank" ? "noopener noreferrer" : void 0;
            return /* @__PURE__ */ (0, import_jsx_runtime37.jsx)(
              "a",
              {
                className: "sidebar__navitem",
                href: safeHref,
                target,
                rel,
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
    /* @__PURE__ */ (0, import_jsx_runtime37.jsx)(
      "div",
      {
        className: "sidebar__virtual-list",
        ref: listRef,
        onScroll: (event) => setScrollTop(event.currentTarget.scrollTop),
        "data-testid": "sidebar-agent-list",
        children: /* @__PURE__ */ (0, import_jsx_runtime37.jsx)("div", { className: "sidebar__virtual-space", style: { height: `${virtualOffsets.total}px` }, children: visibleRows.map((row, index) => {
          const rowIndex = visibleRange.start + index;
          const top = virtualOffsets.offsets[rowIndex] || 0;
          const height = virtualRowHeight(row);
          return /* @__PURE__ */ (0, import_jsx_runtime37.jsx)(
            "div",
            {
              className: `sidebar__virtual-row sidebar__virtual-row--${row.kind}`,
              style: { transform: `translateY(${top}px)`, height: `${height}px` },
              children: row.kind === "section" ? /* @__PURE__ */ (0, import_jsx_runtime37.jsx)(
                "div",
                {
                  className: "sidebar__section",
                  "data-collapsed": row.collapsed ? "true" : void 0,
                  "data-pinned": row.pinned ? "true" : void 0,
                  "data-drag-over": dragOverOrder?.kind === "section" && dragOverOrder.id === row.bucket ? dragOverOrder.where : void 0,
                  children: /* @__PURE__ */ (0, import_jsx_runtime37.jsxs)(
                    "button",
                    {
                      type: "button",
                      className: `sidebar__sec-head sidebar__sec-head--button ${row.reorderable ? "sidebar__order-target" : ""}`,
                      "aria-expanded": !row.collapsed,
                      "data-sidebar-order-kind": row.reorderable ? "section" : void 0,
                      "data-sidebar-order-id": row.reorderable ? row.bucket : void 0,
                      "data-reorderable": row.reorderable ? "true" : void 0,
                      onPointerDown: row.reorderable ? (event) => beginPointerOrderDrag(event, { kind: "section", id: row.bucket }) : void 0,
                      onKeyDown: row.reorderable ? (event) => handleSectionOrderKeyDown(event, row.bucket) : void 0,
                      onClick: () => {
                        if (suppressOrderClickRef.current) return;
                        setCollapsedSections((current) => {
                          const next = new Set(current);
                          if (next.has(row.bucket)) next.delete(row.bucket);
                          else next.add(row.bucket);
                          writeSidebarStringSet(localSidebarStorage(), sectionCollapseStorageKey, next);
                          return next;
                        });
                      },
                      "data-testid": `sidebar-section-toggle:${row.bucket}`,
                      children: [
                        /* @__PURE__ */ (0, import_jsx_runtime37.jsx)("span", { className: "sidebar__sec-label", children: row.bucket }),
                        /* @__PURE__ */ (0, import_jsx_runtime37.jsx)("span", { className: "sidebar__sec-spacer" }),
                        /* @__PURE__ */ (0, import_jsx_runtime37.jsx)("span", { className: "sidebar__sec-count", children: row.count })
                      ]
                    }
                  )
                }
              ) : row.kind === "empty" ? /* @__PURE__ */ (0, import_jsx_runtime37.jsxs)("div", { className: "sidebar__empty", "data-testid": `sidebar-section-empty:${row.bucket}`, children: [
                row.sectionConfig?.empty_title ? /* @__PURE__ */ (0, import_jsx_runtime37.jsx)("span", { className: "sidebar__empty-title", children: row.sectionConfig.empty_title }) : null,
                /* @__PURE__ */ (0, import_jsx_runtime37.jsx)("span", { children: row.sectionConfig?.empty_text || "No agents in this section." })
              ] }) : row.kind === "subgroup" ? /* @__PURE__ */ (0, import_jsx_runtime37.jsxs)(
                "button",
                {
                  type: "button",
                  className: `sidebar__subgroup sidebar__subgroup--button ${row.reorderable ? "sidebar__order-target" : ""}`,
                  "data-collapsed": row.collapsed ? "true" : void 0,
                  "data-drag-over": dragOverOrder?.kind === "subgroup" && dragOverOrder.id === row.storageKey ? dragOverOrder.where : void 0,
                  "aria-expanded": !row.collapsed,
                  "data-sidebar-order-kind": row.reorderable ? "subgroup" : void 0,
                  "data-sidebar-order-id": row.reorderable ? row.storageKey : void 0,
                  "data-sidebar-order-bucket": row.reorderable ? row.bucket : void 0,
                  "data-reorderable": row.reorderable ? "true" : void 0,
                  "data-testid": `sidebar-subgroup-toggle:${row.bucket}:${row.label}`,
                  onPointerDown: row.reorderable ? (event) => beginPointerOrderDrag(event, { kind: "subgroup", id: row.storageKey, bucket: row.bucket }) : void 0,
                  onKeyDown: row.reorderable ? (event) => handleSubgroupOrderKeyDown(event, row.storageKey, row.bucket) : void 0,
                  onClick: () => {
                    if (suppressOrderClickRef.current) return;
                    setCollapsedSubgroups((current) => {
                      const next = new Set(current);
                      if (next.has(row.storageKey)) next.delete(row.storageKey);
                      else next.add(row.storageKey);
                      writeSidebarStringSet(localSidebarStorage(), subgroupCollapseStorageKey, next);
                      return next;
                    });
                  },
                  children: [
                    /* @__PURE__ */ (0, import_jsx_runtime37.jsx)("span", { children: row.label }),
                    /* @__PURE__ */ (0, import_jsx_runtime37.jsx)("span", { className: "sidebar__sec-spacer" }),
                    /* @__PURE__ */ (0, import_jsx_runtime37.jsx)("span", { className: "sidebar__sec-count", children: row.count })
                  ]
                }
              ) : renderAgentRow(
                row.row,
                selectedMemberId,
                recentActivity,
                grouping,
                pinnedAgentIds,
                onSelect,
                onTogglePinnedAgent,
                familyPinIdsByMemberId.get(row.row.agent.member_id)
              )
            },
            row.key
          );
        }) })
      }
    ),
    dragPreview && dragPreviewRows.length > 0 ? /* @__PURE__ */ (0, import_jsx_runtime37.jsx)(
      "div",
      {
        className: "sidebar__drag-preview",
        "data-testid": "sidebar-drag-preview",
        style: {
          width: `${Math.max(160, dragPreview.width)}px`,
          transform: `translate3d(${dragPreview.x + 12}px, ${dragPreview.y + 12}px, 0)`
        },
        "aria-hidden": "true",
        children: renderSidebarDragPreviewRows(dragPreviewRows)
      }
    ) : null
  ] });
}

// src/panels/SignalsRail.tsx
var import_react30 = __toESM(require("react"));
var import_jsx_runtime38 = require("react/jsx-runtime");
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
function sessionHistoryAssistantReply(frame, data) {
  if (frame.sourceKind !== "session_history") {
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
function agentFor(frame) {
  return frame.identity?.trim() || "_system";
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
    const content = stripPeerTransportScaffold(textFromValue(block.content));
    const detail = content || textFromValue(block.summary) || textFromValue(block.intent) || textFromValue(block.payload);
    if (detail) details.push(detail);
  }
  return {
    targets,
    detail: details.join(" "),
    incoming
  };
}
function blobKey(frame) {
  const data = recordOf(frame.data);
  const image = recordOf(data.image);
  const blobRef = recordOf(image.blob_ref ?? data.blob_ref);
  const blobId = typeof blobRef.blob_id === "string" ? blobRef.blob_id : typeof data.blob_id === "string" ? data.blob_id : "";
  const imageId = typeof image.image_id === "string" ? image.image_id : typeof data.image_id === "string" ? data.image_id : "";
  return blobId || imageId || frame.interactionId || frame.id;
}
function severityOf(frame) {
  const ev = frame.event;
  if (ev.includes("fail") || ev.includes("error") || ev.includes("crash")) return "critical";
  if (ev === "gating_decision" || ev.includes("warn") || ev.includes("degraded") || ev.includes("retired")) return "warning";
  return "info";
}
function signalFromFrame(frame) {
  const data = recordOf(frame.data);
  const severity = severityOf(frame);
  const base = {
    id: frame.id || `${frame.event}:${frame.timestampMs || 0}`,
    severity,
    agent: agentFor(frame),
    at: timeFor(frame.timestampMs),
    raw: frame
  };
  if (severity === "critical") {
    return {
      ...base,
      label: frame.event === "interaction_failed" ? "Agent turn failed" : frame.event.replace(/_/g, " "),
      detail: truncate(textFromValue(data.error ?? data.reason ?? data.message) || "Needs attention")
    };
  }
  switch (frame.event) {
    case "user_input":
    case "interaction_started": {
      const request = stripPeerTransportScaffold(
        textFromValue(data.content ?? data.text ?? data.prompt)
      );
      if (!request) return null;
      if (isScaffoldRequest(request)) return null;
      return {
        ...base,
        id: `user:${frame.id || frame.interactionId || frame.timestampMs || request}`,
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
        id: `comms:${frame.id || frame.interactionId || frame.timestampMs || peer}`,
        label: `${comms.incoming ? "Received from" : "Sent to"} ${peer}`,
        detail: truncate(comms.detail || "Peer comms")
      };
    }
    case "interaction_complete": {
      const reply = sessionHistoryAssistantReply(frame, data);
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
        id: `image:${blobKey(frame)}`,
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
        id: `peer:${frame.id || frame.interactionId || `${target}:${body}`}`,
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
      if (frame.event.startsWith("memory.")) {
        return memorySignal(frame, data, base);
      }
      return null;
  }
}
function memorySignal(frame, data, base) {
  const detail = truncate(describeMemoryTimelineEvent2(frame.event, data));
  const warning = (label) => ({ ...base, severity: "warning", label, detail });
  const info = (label) => ({ ...base, severity: "info", label, detail });
  switch (frame.event) {
    case "memory.write.quarantined":
      return warning("Memory write quarantined");
    case "memory.taint.transition":
      return data.kind === "tainted" ? warning("Session memory tainted") : null;
    case "memory.budget.denied":
      return warning("Memory budget denied");
    case "memory.hygiene.blocked":
      return warning("Memory hygiene blocked");
    case "memory.quarantine.release_blocked":
      return warning("Quarantine release blocked");
    case "memory.conflict.signal":
      return warning("Memory conflict");
    case "memory.dream.completed":
      return info("Memory dream completed");
    case "memory.record.promoted":
      return info("Memory record promoted");
    case "memory.harvest.completed":
      return info("Memory harvest completed");
    case "memory.quarantine.verdict":
      return info("Quarantine verdict");
    case "memory.promotion.pending_gate":
      return info("Promotion awaiting gate");
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
function strongerSeverity(a, b) {
  if (a === "critical" || b === "critical") return "critical";
  if (a === "warning" || b === "warning") return "warning";
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
  for (const frame of frames.slice(0, 260)) {
    const signal = signalFromFrame(frame);
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
  const presets = import_react30.default.useMemo(() => {
    const configured = (filterPresets || []).filter((preset) => preset.id && preset.label);
    return configured.length > 0 ? configured : DEFAULT_FILTER_PRESETS;
  }, [filterPresets]);
  const [filter, setFilter] = import_react30.default.useState(activePresetId || presets[0]?.id || "all");
  const [expandedGroups, setExpandedGroups] = import_react30.default.useState(() => /* @__PURE__ */ new Set());
  import_react30.default.useEffect(() => {
    if (activePresetId && presets.some((preset) => preset.id === activePresetId)) {
      setFilter(activePresetId);
    }
  }, [activePresetId, presets]);
  const groups = import_react30.default.useMemo(() => {
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
  const counts = import_react30.default.useMemo(() => {
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
    return /* @__PURE__ */ (0, import_jsx_runtime38.jsx)(
      "aside",
      {
        className: "rail rail--collapsed",
        "data-collapsed": "true",
        "data-testid": "signals-rail",
        children: /* @__PURE__ */ (0, import_jsx_runtime38.jsx)("i", { className: "rail__grip", "aria-hidden": "true" })
      }
    );
  }
  return /* @__PURE__ */ (0, import_jsx_runtime38.jsxs)("aside", { className: "rail", "data-testid": "signals-rail", children: [
    /* @__PURE__ */ (0, import_jsx_runtime38.jsxs)("div", { className: "rail__head", children: [
      /* @__PURE__ */ (0, import_jsx_runtime38.jsx)("span", { className: "rail__title", children: "Signals" }),
      /* @__PURE__ */ (0, import_jsx_runtime38.jsxs)("span", { className: "rail__sub", children: [
        recent15m,
        " in 15m"
      ] })
    ] }),
    /* @__PURE__ */ (0, import_jsx_runtime38.jsx)("div", { className: "rail__filters", children: presets.map((preset) => /* @__PURE__ */ (0, import_jsx_runtime38.jsxs)(
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
          /* @__PURE__ */ (0, import_jsx_runtime38.jsx)("span", { className: "rail__filter-count", children: counts.get(preset.id) || 0 })
        ]
      },
      preset.id
    )) }),
    /* @__PURE__ */ (0, import_jsx_runtime38.jsxs)("div", { className: "rail__list", children: [
      shown.length === 0 && /* @__PURE__ */ (0, import_jsx_runtime38.jsx)("div", { className: "rail__empty", children: emptyText || "No meaningful signals yet." }),
      shown.map((s) => {
        const expanded = expandedGroups.has(s.id);
        return /* @__PURE__ */ (0, import_jsx_runtime38.jsxs)(
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
              /* @__PURE__ */ (0, import_jsx_runtime38.jsx)("span", { className: "signal__bar" }),
              /* @__PURE__ */ (0, import_jsx_runtime38.jsxs)("span", { className: "signal__body", children: [
                /* @__PURE__ */ (0, import_jsx_runtime38.jsxs)("span", { className: "signal__label", children: [
                  s.items.length > 1 && /* @__PURE__ */ (0, import_jsx_runtime38.jsx)("span", { className: "signal__chevron", children: expanded ? "\u25BE" : "\u25B8" }),
                  s.title,
                  s.items.length > 1 && /* @__PURE__ */ (0, import_jsx_runtime38.jsx)("span", { className: "signal__count", children: s.items.length })
                ] }),
                /* @__PURE__ */ (0, import_jsx_runtime38.jsx)("span", { className: "signal__detail", children: s.detail }),
                /* @__PURE__ */ (0, import_jsx_runtime38.jsx)("span", { className: "signal__agent", children: s.agent }),
                s.items.length === 1 && s.items[0].raw.event.startsWith("memory.") && onSelect ? /* @__PURE__ */ (0, import_jsx_runtime38.jsx)(
                  "button",
                  {
                    type: "button",
                    className: "signal__memory-pivot",
                    "data-testid": "signal-memory-pivot",
                    onClick: (event) => {
                      event.stopPropagation();
                      onSelect(s.items[0].raw);
                    },
                    children: "state here"
                  }
                ) : null,
                s.items.length > 1 && expanded && /* @__PURE__ */ (0, import_jsx_runtime38.jsx)("span", { className: "signal__events", children: s.items.map((item) => /* @__PURE__ */ (0, import_jsx_runtime38.jsxs)(
                  "button",
                  {
                    className: "signal__event",
                    type: "button",
                    onClick: (event) => {
                      event.stopPropagation();
                      onSelect?.(item.raw);
                    },
                    children: [
                      /* @__PURE__ */ (0, import_jsx_runtime38.jsx)("span", { className: "signal__event-label", children: item.label }),
                      /* @__PURE__ */ (0, import_jsx_runtime38.jsx)("span", { className: "signal__event-detail", children: item.detail })
                    ]
                  },
                  item.id
                )) })
              ] }),
              /* @__PURE__ */ (0, import_jsx_runtime38.jsx)("span", { className: "signal__meta", children: /* @__PURE__ */ (0, import_jsx_runtime38.jsx)("span", { className: "signal__time", children: s.at }) })
            ]
          },
          s.id
        );
      })
    ] })
  ] });
}

// src/panels/ChatPane.tsx
var import_react31 = __toESM(require("react"));

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
function selectImageTransferFiles(directFiles, itemFiles) {
  return dedupeComposerImageFiles(directFiles.length > 0 ? directFiles : itemFiles);
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
var import_jsx_runtime39 = require("react/jsx-runtime");
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
function buildChatTurns(messages) {
  const turns = [];
  for (const message of messages) {
    const current = turns.at(-1);
    if (!current || message.kind === "user") {
      turns.push({
        id: `turn-${message.id}`,
        messages: [message]
      });
      continue;
    }
    current.messages.push(message);
  }
  return turns;
}
function chatTurnPreview(turn) {
  let title = "";
  let body = "";
  for (const message of turn.messages) {
    const text = msgCopyText(message);
    if (!text) {
      continue;
    }
    if (!title && message.kind === "user") {
      title = text;
      continue;
    }
    if (!body && message.kind !== "user") {
      body = text;
    }
  }
  if (!title) {
    title = msgCopyText(turn.messages[0]) || "Turn";
  }
  if (!body) {
    body = "No response yet.";
  }
  return { title, body };
}
function isScaffoldUserText(text) {
  const normalized = text.trimStart();
  return /^you have been spawned\b/i.test(normalized) || /^\[peer update\]/i.test(normalized);
}
function isScaffoldUserMessage(message) {
  return message.kind === "user" && isScaffoldUserText(msgCopyText(message));
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
  if (entry.kind === "workgraph") {
    return [{
      id: entry.id,
      kind: "workgraph",
      time: formatTime3(entry.createdAt),
      createdAt: entry.createdAt,
      // Copy/transcript surfaces read `text`; rendering goes through the card.
      text: conversationEntryText(entry),
      workGraphEntry: entry
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
function buildChatMessages(entries) {
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
      const canDedupeAdjacent = m.kind === "user" && last?.kind === "user" || m.kind === "agent" && last?.kind === "agent" && last.who === m.who;
      if (last && canDedupeAdjacent) {
        const lastSignature = textSignatureForMsg(last);
        const nextSignature = textSignatureForMsg(m);
        if (lastSignature && lastSignature === nextSignature) {
          continue;
        }
      }
      merged.push({ ...m });
    }
  }
  let pendingUserStartedAt = null;
  return merged.map((message) => {
    if (message.kind === "user") {
      pendingUserStartedAt = isScaffoldUserMessage(message) ? null : parseTimeMs(message.createdAt);
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
}
function collectImageTransferPayload(data) {
  const directFiles = Array.from(data.files).filter((file) => file.type.startsWith("image/"));
  const itemFiles = Array.from(data.items).filter((item) => item.kind === "file" && item.type.startsWith("image/")).map((item) => item.getAsFile()).filter((file) => Boolean(file));
  const textPayloads = [
    data.getData("text/html"),
    data.getData("text/uri-list"),
    data.getData("text/plain")
  ].filter(Boolean);
  return { files: selectImageTransferFiles(directFiles, itemFiles), textPayloads };
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
  const [copied, setCopied] = import_react31.default.useState(false);
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
  return /* @__PURE__ */ (0, import_jsx_runtime39.jsx)(
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
  readOnly = false,
  accessEnforcing = false,
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
  hasOlderHistory = false,
  loadingOlderHistory = false,
  isLoadingHistory = false,
  onLoadOlder,
  stackSlot,
  workGraphActions = null
}) {
  const bodyRef = import_react31.default.useRef(null);
  const preserveOlderHistoryScrollRef = import_react31.default.useRef(false);
  const olderHistoryScrollHeightRef = import_react31.default.useRef(0);
  const olderHistoryScrollTopRef = import_react31.default.useRef(0);
  const activeTurnFrameRef = import_react31.default.useRef(0);
  const [visibleTurnIndexes, setVisibleTurnIndexes] = import_react31.default.useState([]);
  const messages = import_react31.default.useMemo(() => {
    return buildChatMessages(entries);
  }, [entries]);
  const turns = import_react31.default.useMemo(() => buildChatTurns(messages), [messages]);
  const lastAgentMessageId = import_react31.default.useMemo(() => {
    for (let i = messages.length - 1; i >= 0; i -= 1) {
      if (messages[i].kind === "agent") return messages[i].id;
    }
    return null;
  }, [messages]);
  const scrollSignature = import_react31.default.useMemo(() => {
    const last = messages[messages.length - 1];
    const lastTextLength = last?.text?.length ?? 0;
    const lastBlockLength = last?.blocks ? JSON.stringify(last.blocks).length : last?.workGraphEntry ? JSON.stringify(last.workGraphEntry).length : 0;
    return [
      identity,
      messages.length,
      last?.id ?? "",
      lastTextLength,
      lastBlockLength,
      phase ?? ""
    ].join(":");
  }, [identity, messages, phase]);
  import_react31.default.useLayoutEffect(() => {
    if (preserveOlderHistoryScrollRef.current && bodyRef.current) {
      const node = bodyRef.current;
      const addedHeight = node.scrollHeight - olderHistoryScrollHeightRef.current;
      node.scrollTop = olderHistoryScrollTopRef.current + Math.max(0, addedHeight);
      node.scrollLeft = 0;
      preserveOlderHistoryScrollRef.current = false;
      return;
    }
    const resetTranscriptScroll = () => {
      if (bodyRef.current) {
        bodyRef.current.scrollTop = bodyRef.current.scrollHeight;
        bodyRef.current.scrollLeft = 0;
      }
    };
    resetTranscriptScroll();
    const firstFrame = window.requestAnimationFrame(resetTranscriptScroll);
    const secondFrame = window.requestAnimationFrame(resetTranscriptScroll);
    return () => {
      window.cancelAnimationFrame(firstFrame);
      window.cancelAnimationFrame(secondFrame);
    };
  }, [scrollSignature]);
  import_react31.default.useEffect(() => {
    if (!loadingOlderHistory && preserveOlderHistoryScrollRef.current) {
      preserveOlderHistoryScrollRef.current = false;
    }
  }, [loadingOlderHistory]);
  const updateActiveTurn = import_react31.default.useCallback(() => {
    activeTurnFrameRef.current = 0;
    const body = bodyRef.current;
    if (!body || turns.length <= 1) {
      setVisibleTurnIndexes([]);
      return;
    }
    const turnNodes = Array.from(
      body.querySelectorAll("[data-chat-turn-index]")
    );
    if (turnNodes.length === 0) {
      setVisibleTurnIndexes([]);
      return;
    }
    const bodyRect = body.getBoundingClientRect();
    const visibleTop = bodyRect.top;
    const visibleBottom = bodyRect.bottom;
    const targetY = bodyRect.top + Math.min(128, Math.max(48, bodyRect.height * 0.24));
    let nextIndex = 0;
    const nextVisibleIndexes = [];
    for (const turnNode of turnNodes) {
      const rawIndex = Number(turnNode.dataset.chatTurnIndex);
      if (!Number.isFinite(rawIndex)) {
        continue;
      }
      const turnRect = turnNode.getBoundingClientRect();
      if (turnRect.bottom >= visibleTop && turnRect.top <= visibleBottom) {
        nextVisibleIndexes.push(rawIndex);
      }
      if (turnRect.top <= targetY) {
        nextIndex = rawIndex;
      }
    }
    const nextIndexes = nextVisibleIndexes.length > 0 ? nextVisibleIndexes : [nextIndex];
    setVisibleTurnIndexes((current) => {
      if (current.length === nextIndexes.length && current.every((value, index) => value === nextIndexes[index])) {
        return current;
      }
      return nextIndexes;
    });
  }, [turns.length]);
  const scheduleActiveTurnUpdate = import_react31.default.useCallback(() => {
    if (activeTurnFrameRef.current) {
      return;
    }
    activeTurnFrameRef.current = window.requestAnimationFrame(updateActiveTurn);
  }, [updateActiveTurn]);
  import_react31.default.useEffect(() => {
    scheduleActiveTurnUpdate();
  }, [scheduleActiveTurnUpdate, scrollSignature]);
  import_react31.default.useEffect(() => {
    updateActiveTurn();
    window.addEventListener("resize", scheduleActiveTurnUpdate);
    return () => {
      if (activeTurnFrameRef.current) {
        window.cancelAnimationFrame(activeTurnFrameRef.current);
        activeTurnFrameRef.current = 0;
      }
      window.removeEventListener("resize", scheduleActiveTurnUpdate);
    };
  }, [scheduleActiveTurnUpdate, updateActiveTurn]);
  function scrollToTurn(turnIndex) {
    const turnNode = bodyRef.current?.querySelector(
      `[data-chat-turn-index="${turnIndex}"]`
    );
    turnNode?.scrollIntoView({ block: "start", behavior: "smooth" });
  }
  function requestOlderHistory() {
    if (bodyRef.current) {
      preserveOlderHistoryScrollRef.current = true;
      olderHistoryScrollHeightRef.current = bodyRef.current.scrollHeight;
      olderHistoryScrollTopRef.current = bodyRef.current.scrollTop;
    }
    onLoadOlder?.();
  }
  const transcriptText = import_react31.default.useMemo(() => transcriptCopyText(messages), [messages]);
  const initial = (agentLabel || "?").trim().charAt(0).toUpperCase() || "?";
  const state = (agent?.state || "unknown").toLowerCase();
  const canAttachImages = !readOnly && agent?.model_capabilities?.image_input === true;
  const sendWithheld = accessEnforcing && agent?.affordances?.can_send_message === false;
  const [dragActive, setDragActive] = import_react31.default.useState(false);
  const [attachmentError, setAttachmentError] = import_react31.default.useState(null);
  const resolvedDraftBlobRefs = import_react31.default.useRef("");
  const turnRail = turns.length > 1 ? /* @__PURE__ */ (0, import_jsx_runtime39.jsx)("nav", { className: "conv-turn-rail", "aria-label": "Conversation turns", children: /* @__PURE__ */ (0, import_jsx_runtime39.jsx)("ol", { className: "conv-turn-rail__list", children: turns.map((turn, turnIndex) => {
    const preview = chatTurnPreview(turn);
    const isVisibleTurn = visibleTurnIndexes.includes(turnIndex);
    return /* @__PURE__ */ (0, import_jsx_runtime39.jsxs)("li", { className: "conv-turn-rail__item", children: [
      /* @__PURE__ */ (0, import_jsx_runtime39.jsx)(
        "button",
        {
          "aria-current": isVisibleTurn ? "true" : void 0,
          "aria-label": `Jump to turn ${turnIndex + 1}: ${preview.title}`,
          className: `conv-turn-rail__button${isVisibleTurn ? " is-active" : ""}`,
          "data-testid": `chat-turn-rail:${identity}:${turnIndex}`,
          onClick: (event) => {
            scrollToTurn(turnIndex);
            if (event.detail > 0) {
              event.currentTarget.blur();
            }
          },
          type: "button",
          children: /* @__PURE__ */ (0, import_jsx_runtime39.jsx)("span", { className: "conv-turn-rail__tick", "aria-hidden": "true" })
        }
      ),
      /* @__PURE__ */ (0, import_jsx_runtime39.jsxs)("div", { className: "conv-turn-preview", role: "presentation", children: [
        /* @__PURE__ */ (0, import_jsx_runtime39.jsx)("div", { className: "conv-turn-preview__title", children: preview.title }),
        /* @__PURE__ */ (0, import_jsx_runtime39.jsx)("div", { className: "conv-turn-preview__body", children: preview.body })
      ] })
    ] }, turn.id);
  }) }) }) : null;
  function addFiles(fileList) {
    if (readOnly || !canAttachImages) return;
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
  import_react31.default.useEffect(() => {
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
    if (readOnly || sendWithheld) {
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
  return /* @__PURE__ */ (0, import_jsx_runtime39.jsxs)("div", { className: "conv", "data-testid": `chat-pane:${identity}`, children: [
    /* @__PURE__ */ (0, import_jsx_runtime39.jsxs)("div", { className: "conv__head", children: [
      /* @__PURE__ */ (0, import_jsx_runtime39.jsx)("div", { className: "conv__avatar", children: initial }),
      /* @__PURE__ */ (0, import_jsx_runtime39.jsxs)("div", { style: { minWidth: 0 }, children: [
        /* @__PURE__ */ (0, import_jsx_runtime39.jsx)("div", { className: "conv__title", children: agentLabel }),
        /* @__PURE__ */ (0, import_jsx_runtime39.jsxs)("div", { className: "conv__identity", children: [
          identity,
          agent?.role ? ` \xB7 ${agent.role}` : ""
        ] })
      ] }),
      /* @__PURE__ */ (0, import_jsx_runtime39.jsxs)("div", { className: "conv__actions", children: [
        onInspect ? /* @__PURE__ */ (0, import_jsx_runtime39.jsx)("button", { className: "conv__action", onClick: onInspect, "data-testid": "conv-action:details", children: inspectLabel }) : null,
        agent?.affordances?.can_respawn && onRespawn ? /* @__PURE__ */ (0, import_jsx_runtime39.jsx)("button", { className: "conv__action", onClick: onRespawn, "data-testid": "conv-action:respawn", children: respawnLabel }) : null,
        agent?.affordances?.can_retire && onRetire ? /* @__PURE__ */ (0, import_jsx_runtime39.jsx)("button", { className: "conv__action", onClick: onRetire, "data-testid": "conv-action:retire", children: retireLabel }) : null
      ] })
    ] }),
    /* @__PURE__ */ (0, import_jsx_runtime39.jsxs)(
      "div",
      {
        className: "conv__body",
        onScroll: (event) => {
          if (event.currentTarget.scrollLeft !== 0) {
            event.currentTarget.scrollLeft = 0;
          }
          scheduleActiveTurnUpdate();
          if (event.currentTarget.scrollTop <= 32 && hasOlderHistory && !loadingOlderHistory) {
            requestOlderHistory();
          }
        },
        ref: bodyRef,
        children: [
          /* @__PURE__ */ (0, import_jsx_runtime39.jsx)(
            CopyInlineButton,
            {
              className: "msg__copy--transcript",
              label: "Copy transcript",
              text: transcriptText
            }
          ),
          hasOlderHistory && /* @__PURE__ */ (0, import_jsx_runtime39.jsx)(
            "button",
            {
              className: "conv__history",
              disabled: loadingOlderHistory,
              onClick: requestOlderHistory,
              type: "button",
              children: loadingOlderHistory ? "Loading history" : "Load older history"
            }
          ),
          messages.length === 0 && isLoadingHistory && /* @__PURE__ */ (0, import_jsx_runtime39.jsxs)(
            "div",
            {
              className: "msg msg--origin",
              "data-testid": `chat-loading-history:${identity}`,
              "aria-live": "polite",
              "aria-busy": "true",
              children: [
                /* @__PURE__ */ (0, import_jsx_runtime39.jsx)("div", { className: "msg__time" }),
                /* @__PURE__ */ (0, import_jsx_runtime39.jsx)("div", { className: "msg__bubble", children: /* @__PURE__ */ (0, import_jsx_runtime39.jsxs)("span", { className: "msg__typing", children: [
                  /* @__PURE__ */ (0, import_jsx_runtime39.jsxs)("span", { className: "msg__typing-dots", "aria-hidden": "true", children: [
                    /* @__PURE__ */ (0, import_jsx_runtime39.jsx)("span", {}),
                    /* @__PURE__ */ (0, import_jsx_runtime39.jsx)("span", {}),
                    /* @__PURE__ */ (0, import_jsx_runtime39.jsx)("span", {})
                  ] }),
                  /* @__PURE__ */ (0, import_jsx_runtime39.jsx)("span", { className: "msg__typing-label", children: "Loading conversation\u2026" })
                ] }) })
              ]
            }
          ),
          messages.length === 0 && !isLoadingHistory && /* @__PURE__ */ (0, import_jsx_runtime39.jsxs)("div", { className: "msg msg--origin", children: [
            /* @__PURE__ */ (0, import_jsx_runtime39.jsx)("div", { className: "msg__time" }),
            /* @__PURE__ */ (0, import_jsx_runtime39.jsx)("div", { className: "msg__bubble", children: /* @__PURE__ */ (0, import_jsx_runtime39.jsxs)("span", { className: "msg__text", children: [
              "No messages yet. Say hello to ",
              agentLabel,
              "."
            ] }) })
          ] }),
          turns.map((turn, turnIndex) => /* @__PURE__ */ (0, import_jsx_runtime39.jsx)(
            "div",
            {
              "aria-label": `Turn ${turnIndex + 1}`,
              className: "conv-turn",
              "data-chat-turn-index": turnIndex,
              "data-testid": `chat-turn:${identity}:${turnIndex}`,
              children: turn.messages.map((m) => /* @__PURE__ */ (0, import_jsx_runtime39.jsxs)("div", { className: `msg msg--${m.kind}`, children: [
                /* @__PURE__ */ (0, import_jsx_runtime39.jsx)("div", { className: "msg__time", children: m.time }),
                /* @__PURE__ */ (0, import_jsx_runtime39.jsxs)("div", { className: "msg__bubble", children: [
                  (m.kind === "user" || m.kind === "agent") && /* @__PURE__ */ (0, import_jsx_runtime39.jsx)(CopyInlineButton, { label: `Copy ${m.kind === "user" ? "message" : "turn"}`, text: msgCopyText(m) }),
                  m.kind === "workgraph" && m.workGraphEntry ? /* @__PURE__ */ (0, import_jsx_runtime39.jsx)(WorkGraphCard, { entry: m.workGraphEntry, actions: workGraphActions }) : m.blocks && m.blocks.length > 0 ? /* @__PURE__ */ (0, import_jsx_runtime39.jsx)(ConversationRichContent, { blocks: m.blocks, displayNormalization: false }) : m.text && /* @__PURE__ */ (0, import_jsx_runtime39.jsx)("span", { className: "msg__text", children: m.text }),
                  m.workedFor && !(phase && m.id === lastAgentMessageId) && /* @__PURE__ */ (0, import_jsx_runtime39.jsxs)("div", { className: "msg__worked", children: [
                    /* @__PURE__ */ (0, import_jsx_runtime39.jsxs)("span", { children: [
                      "Worked for ",
                      m.workedFor
                    ] }),
                    /* @__PURE__ */ (0, import_jsx_runtime39.jsx)(
                      CopyInlineButton,
                      {
                        className: "msg__copy--inline",
                        label: "Copy work time",
                        text: m.workedForCopyText || `Worked for ${m.workedFor}`
                      }
                    )
                  ] })
                ] })
              ] }, m.id))
            },
            turn.id
          )),
          phase && /* @__PURE__ */ (0, import_jsx_runtime39.jsxs)(
            "div",
            {
              className: "msg msg--typing",
              "data-testid": `chat-typing:${identity}`,
              "aria-live": "polite",
              "aria-label": `${agentLabel} is ${phaseLabel(phase)}`,
              children: [
                /* @__PURE__ */ (0, import_jsx_runtime39.jsx)("div", { className: "msg__time" }),
                /* @__PURE__ */ (0, import_jsx_runtime39.jsx)("div", { className: "msg__bubble", children: /* @__PURE__ */ (0, import_jsx_runtime39.jsxs)("span", { className: "msg__typing", children: [
                  /* @__PURE__ */ (0, import_jsx_runtime39.jsxs)("span", { className: "msg__typing-dots", "aria-hidden": "true", children: [
                    /* @__PURE__ */ (0, import_jsx_runtime39.jsx)("span", {}),
                    /* @__PURE__ */ (0, import_jsx_runtime39.jsx)("span", {}),
                    /* @__PURE__ */ (0, import_jsx_runtime39.jsx)("span", {})
                  ] }),
                  /* @__PURE__ */ (0, import_jsx_runtime39.jsx)("span", { className: "msg__typing-label", children: phaseLabel(phase) })
                ] }) })
              ]
            }
          )
        ]
      }
    ),
    turnRail,
    stackSlot,
    /* @__PURE__ */ (0, import_jsx_runtime39.jsxs)("div", { className: "composer", children: [
      /* @__PURE__ */ (0, import_jsx_runtime39.jsxs)(
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
            staged.length > 0 && /* @__PURE__ */ (0, import_jsx_runtime39.jsx)("div", { className: "composer__attachments", children: staged.map((item) => /* @__PURE__ */ (0, import_jsx_runtime39.jsxs)("div", { className: "composer__attachment", children: [
              /* @__PURE__ */ (0, import_jsx_runtime39.jsx)("img", { alt: "", src: item.previewUrl }),
              /* @__PURE__ */ (0, import_jsx_runtime39.jsx)("button", { "aria-label": "Remove attachment", onClick: () => removeAttachment(item.id), type: "button", children: "\xD7" })
            ] }, item.id)) }),
            /* @__PURE__ */ (0, import_jsx_runtime39.jsx)(
              "textarea",
              {
                placeholder: readOnly ? "View-only console" : sendWithheld ? `You can view ${agentLabel} but not message it` : `Message ${agentLabel}\u2026`,
                value: draft,
                disabled: readOnly || sendWithheld,
                onChange: (e) => {
                  if (!readOnly && !sendWithheld) onDraftChange(e.target.value);
                },
                onKeyDown: (e) => {
                  if (e.key === "Enter" && !e.shiftKey) {
                    e.preventDefault();
                    submitComposer();
                  }
                },
                rows: 2,
                "data-testid": `chat-composer:${identity}`
              }
            ),
            /* @__PURE__ */ (0, import_jsx_runtime39.jsxs)("div", { className: "composer__row", children: [
              /* @__PURE__ */ (0, import_jsx_runtime39.jsx)("span", { className: "composer__chip mono", children: agent?.role || "agent" }),
              /* @__PURE__ */ (0, import_jsx_runtime39.jsx)("span", { className: "composer__spacer" }),
              /* @__PURE__ */ (0, import_jsx_runtime39.jsxs)(
                "button",
                {
                  className: "composer__send",
                  disabled: !draft.trim() && staged.length === 0 || readOnly || sendWithheld || staged.length > 0 && !canAttachImages || staged.length > 0 && sending,
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
      /* @__PURE__ */ (0, import_jsx_runtime39.jsxs)("div", { className: "composer__footer", children: [
        /* @__PURE__ */ (0, import_jsx_runtime39.jsxs)("span", { children: [
          "To: ",
          /* @__PURE__ */ (0, import_jsx_runtime39.jsx)("b", { style: { color: "var(--ink-muted)" }, children: agentLabel })
        ] }),
        /* @__PURE__ */ (0, import_jsx_runtime39.jsx)("span", { children: "\xB7" }),
        /* @__PURE__ */ (0, import_jsx_runtime39.jsx)("span", { className: "mono", children: identity }),
        agent?.role && /* @__PURE__ */ (0, import_jsx_runtime39.jsxs)(import_jsx_runtime39.Fragment, { children: [
          /* @__PURE__ */ (0, import_jsx_runtime39.jsx)("span", { children: "\xB7" }),
          /* @__PURE__ */ (0, import_jsx_runtime39.jsx)("span", { children: agent.role })
        ] }),
        /* @__PURE__ */ (0, import_jsx_runtime39.jsx)("span", { children: "\xB7" }),
        /* @__PURE__ */ (0, import_jsx_runtime39.jsx)("span", { className: "dot", style: {
          background: state === "active" || state === "running" ? "var(--ok)" : state.includes("degrade") ? "var(--warn)" : state === "retired" ? "var(--ink-faint)" : "var(--ink-dim)"
        } }),
        /* @__PURE__ */ (0, import_jsx_runtime39.jsx)("span", { children: state }),
        phase && /* @__PURE__ */ (0, import_jsx_runtime39.jsxs)(import_jsx_runtime39.Fragment, { children: [
          /* @__PURE__ */ (0, import_jsx_runtime39.jsx)("span", { children: "\xB7" }),
          /* @__PURE__ */ (0, import_jsx_runtime39.jsx)("span", { style: { color: "var(--accent)" }, children: phase })
        ] }),
        readOnly && /* @__PURE__ */ (0, import_jsx_runtime39.jsxs)(import_jsx_runtime39.Fragment, { children: [
          /* @__PURE__ */ (0, import_jsx_runtime39.jsx)("span", { children: "\xB7" }),
          /* @__PURE__ */ (0, import_jsx_runtime39.jsx)("span", { children: "view only" })
        ] }),
        !readOnly && sendWithheld && /* @__PURE__ */ (0, import_jsx_runtime39.jsxs)(import_jsx_runtime39.Fragment, { children: [
          /* @__PURE__ */ (0, import_jsx_runtime39.jsx)("span", { children: "\xB7" }),
          /* @__PURE__ */ (0, import_jsx_runtime39.jsx)("span", { children: "send not permitted" })
        ] }),
        !readOnly && !canAttachImages && /* @__PURE__ */ (0, import_jsx_runtime39.jsxs)(import_jsx_runtime39.Fragment, { children: [
          /* @__PURE__ */ (0, import_jsx_runtime39.jsx)("span", { children: "\xB7" }),
          /* @__PURE__ */ (0, import_jsx_runtime39.jsx)("span", { children: "model cannot see images" })
        ] }),
        attachmentError && /* @__PURE__ */ (0, import_jsx_runtime39.jsxs)(import_jsx_runtime39.Fragment, { children: [
          /* @__PURE__ */ (0, import_jsx_runtime39.jsx)("span", { children: "\xB7" }),
          /* @__PURE__ */ (0, import_jsx_runtime39.jsx)("span", { style: { color: "var(--bad)" }, children: attachmentError })
        ] })
      ] })
    ] })
  ] });
}

// src/panels/MobKitDock.tsx
var import_react32 = __toESM(require("react"));
var import_jsx_runtime40 = require("react/jsx-runtime");
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
  import_react32.default.useEffect(() => {
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
  return /* @__PURE__ */ (0, import_jsx_runtime40.jsxs)("div", { className: "mkdock", "data-testid": "mkdock", children: [
    /* @__PURE__ */ (0, import_jsx_runtime40.jsxs)("div", { className: "wstabs", children: [
      viewState.tabs.map((t) => {
        const isActive = t.id === activeTab?.id;
        const count = tabPanelCount(t.layout);
        return /* @__PURE__ */ (0, import_jsx_runtime40.jsxs)(
          "div",
          {
            className: `wstab ${isActive ? "is-active" : ""}`,
            onClick: () => onSelectTab(t.id),
            "data-testid": `wstab:${t.id}`,
            children: [
              /* @__PURE__ */ (0, import_jsx_runtime40.jsx)("span", { className: "wstab__mark" }),
              /* @__PURE__ */ (0, import_jsx_runtime40.jsx)("span", { className: "wstab__name", children: t.title || "untitled" }),
              count > 1 && /* @__PURE__ */ (0, import_jsx_runtime40.jsx)("span", { className: "wstab__count", children: count }),
              viewState.tabs.length > 1 && /* @__PURE__ */ (0, import_jsx_runtime40.jsx)(
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
      /* @__PURE__ */ (0, import_jsx_runtime40.jsx)(
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
    /* @__PURE__ */ (0, import_jsx_runtime40.jsx)("div", { className: "dock", children: activeTab && /* @__PURE__ */ (0, import_jsx_runtime40.jsx)(
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
    return /* @__PURE__ */ (0, import_jsx_runtime40.jsx)(PaneView, { panelId: node.panelId, ...props });
  }
  return /* @__PURE__ */ (0, import_jsx_runtime40.jsx)(SplitView, { node, ...props });
}
function SplitView(props) {
  const { node } = props;
  if (node.kind !== "split") return null;
  const ratio = typeof node.ratio === "number" ? Math.max(0.1, Math.min(0.9, node.ratio)) : 0.5;
  const direction = node.direction;
  const style = direction === "horizontal" ? { gridTemplateColumns: `${ratio * 100}% 6px ${(1 - ratio) * 100}%` } : { gridTemplateRows: `${ratio * 100}% 6px ${(1 - ratio) * 100}%` };
  const hostRef = import_react32.default.useRef(null);
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
  return /* @__PURE__ */ (0, import_jsx_runtime40.jsxs)(
    "div",
    {
      ref: hostRef,
      className: `split split--${direction === "horizontal" ? "h" : "v"}`,
      style,
      children: [
        /* @__PURE__ */ (0, import_jsx_runtime40.jsx)(DockLayout, { ...props, node: node.first }),
        /* @__PURE__ */ (0, import_jsx_runtime40.jsx)(
          "div",
          {
            className: `split__handle split__handle--${direction === "horizontal" ? "h" : "v"}`,
            onPointerDown: startDrag,
            "data-testid": `split-handle:${node.id}`
          }
        ),
        /* @__PURE__ */ (0, import_jsx_runtime40.jsx)(DockLayout, { ...props, node: node.second })
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
  const [menuOpen, setMenuOpen] = import_react32.default.useState(false);
  return /* @__PURE__ */ (0, import_jsx_runtime40.jsxs)(
    "div",
    {
      className: `pane ${isFocused ? "is-focused" : ""}`,
      onMouseDown: () => onFocusPanel(panelId),
      "data-testid": `pane:${panelId}`,
      children: [
        /* @__PURE__ */ (0, import_jsx_runtime40.jsxs)("div", { className: "pane__bar", children: [
          /* @__PURE__ */ (0, import_jsx_runtime40.jsxs)(
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
                /* @__PURE__ */ (0, import_jsx_runtime40.jsx)("span", { className: "pane__title-text", children: title }),
                /* @__PURE__ */ (0, import_jsx_runtime40.jsx)("span", { className: "pane__caret", children: "\u25BE" })
              ]
            }
          ),
          subId && /* @__PURE__ */ (0, import_jsx_runtime40.jsx)("span", { className: "pane__id", children: subId }),
          /* @__PURE__ */ (0, import_jsx_runtime40.jsx)("span", { className: "pane__spacer" }),
          /* @__PURE__ */ (0, import_jsx_runtime40.jsx)(
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
          /* @__PURE__ */ (0, import_jsx_runtime40.jsx)(
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
          /* @__PURE__ */ (0, import_jsx_runtime40.jsx)(
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
          menuOpen && /* @__PURE__ */ (0, import_jsx_runtime40.jsx)(
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
        /* @__PURE__ */ (0, import_jsx_runtime40.jsx)("div", { className: "pane__body", children: renderPanelBody({ id: panelId, target }) })
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
  return /* @__PURE__ */ (0, import_jsx_runtime40.jsxs)(import_jsx_runtime40.Fragment, { children: [
    /* @__PURE__ */ (0, import_jsx_runtime40.jsx)("div", { className: "pane-menu__scrim", onMouseDown: onClose }),
    /* @__PURE__ */ (0, import_jsx_runtime40.jsxs)("div", { className: "pane-menu", onMouseDown: (e) => e.stopPropagation(), children: [
      /* @__PURE__ */ (0, import_jsx_runtime40.jsx)("div", { className: "pane-menu__label", children: "Views" }),
      controls.map(([kind, label]) => /* @__PURE__ */ (0, import_jsx_runtime40.jsxs)(
        "button",
        {
          className: "pane-menu__item",
          onClick: () => onPick(buildControlTarget2(kind)),
          "data-testid": `pane-menu-view:${kind}`,
          children: [
            /* @__PURE__ */ (0, import_jsx_runtime40.jsx)("span", {}),
            /* @__PURE__ */ (0, import_jsx_runtime40.jsx)("span", { children: label }),
            /* @__PURE__ */ (0, import_jsx_runtime40.jsx)("span", { className: "pane-menu__id", children: "view" })
          ]
        },
        kind
      )),
      /* @__PURE__ */ (0, import_jsx_runtime40.jsx)("div", { className: "pane-menu__sep" }),
      /* @__PURE__ */ (0, import_jsx_runtime40.jsx)("div", { className: "pane-menu__label", children: "Agents" }),
      agents.slice(0, 14).map((a) => /* @__PURE__ */ (0, import_jsx_runtime40.jsxs)(
        "button",
        {
          className: "pane-menu__item",
          "data-state": (a.state || "").toLowerCase(),
          onClick: () => onPick(buildDockTarget2(a)),
          "data-testid": `pane-menu-agent:${a.member_id}`,
          children: [
            /* @__PURE__ */ (0, import_jsx_runtime40.jsx)("span", { className: "agent__dot" }),
            /* @__PURE__ */ (0, import_jsx_runtime40.jsx)("span", { children: a.label }),
            /* @__PURE__ */ (0, import_jsx_runtime40.jsx)("span", { className: "pane-menu__id", children: a.identity || a.member_id })
          ]
        },
        a.member_id
      ))
    ] })
  ] });
}

// src/panels/PendingStack.tsx
var import_react33 = __toESM(require("react"));
var import_jsx_runtime41 = require("react/jsx-runtime");
function StackHead({
  count,
  agentBusy,
  collapsed,
  onToggleCollapsed,
  onClear
}) {
  return /* @__PURE__ */ (0, import_jsx_runtime41.jsxs)("div", { className: "stack__head", children: [
    /* @__PURE__ */ (0, import_jsx_runtime41.jsx)(
      "button",
      {
        type: "button",
        className: "stack__head-btn",
        onClick: onToggleCollapsed,
        "aria-expanded": !collapsed,
        "aria-label": collapsed ? "Expand pending queue" : "Collapse pending queue",
        title: collapsed ? "Expand queue" : "Collapse queue",
        children: /* @__PURE__ */ (0, import_jsx_runtime41.jsx)("span", { className: "stack__head-chev", children: collapsed ? "\u25B8" : "\u25BE" })
      }
    ),
    /* @__PURE__ */ (0, import_jsx_runtime41.jsx)("span", { children: "Queue" }),
    /* @__PURE__ */ (0, import_jsx_runtime41.jsx)("span", { className: "stack__head-count", children: String(count).padStart(2, "0") }),
    !collapsed && count > 1 && /* @__PURE__ */ (0, import_jsx_runtime41.jsx)("span", { className: "stack__head-hint", children: "\xB7 drains top \u2192 bottom" }),
    /* @__PURE__ */ (0, import_jsx_runtime41.jsx)("span", { className: "stack__head-spacer" }),
    /* @__PURE__ */ (0, import_jsx_runtime41.jsxs)("span", { className: `stack__head-phase ${agentBusy ? "" : "is-idle"}`, children: [
      /* @__PURE__ */ (0, import_jsx_runtime41.jsx)("b", {}),
      agentBusy ? "Agent busy" : "Agent idle \xB7 draining"
    ] }),
    count > 0 && /* @__PURE__ */ (0, import_jsx_runtime41.jsx)(
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
  const taRef = import_react33.default.useRef(null);
  const [draft, setDraft] = import_react33.default.useState(item.text);
  import_react33.default.useEffect(() => {
    if (item.editing && taRef.current) {
      taRef.current.focus();
      const len = taRef.current.value.length;
      taRef.current.setSelectionRange(len, len);
      taRef.current.style.height = "auto";
      taRef.current.style.height = taRef.current.scrollHeight + "px";
    }
  }, [item.editing]);
  import_react33.default.useEffect(() => {
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
  return /* @__PURE__ */ (0, import_jsx_runtime41.jsxs)(
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
        /* @__PURE__ */ (0, import_jsx_runtime41.jsxs)("div", { className: "stk-item__lead", children: [
          /* @__PURE__ */ (0, import_jsx_runtime41.jsxs)("span", { className: "stk-item__grip", "aria-label": "Drag to reorder", title: "Drag to reorder", children: [
            /* @__PURE__ */ (0, import_jsx_runtime41.jsx)("span", {}),
            /* @__PURE__ */ (0, import_jsx_runtime41.jsx)("span", {}),
            /* @__PURE__ */ (0, import_jsx_runtime41.jsx)("span", {}),
            /* @__PURE__ */ (0, import_jsx_runtime41.jsx)("span", {}),
            /* @__PURE__ */ (0, import_jsx_runtime41.jsx)("span", {}),
            /* @__PURE__ */ (0, import_jsx_runtime41.jsx)("span", {})
          ] }),
          /* @__PURE__ */ (0, import_jsx_runtime41.jsx)("span", { className: "stk-item__queue-glyph", "aria-hidden": "true", children: "\u2935" })
        ] }),
        item.editing ? /* @__PURE__ */ (0, import_jsx_runtime41.jsxs)("div", { className: "stk-item__edit", children: [
          /* @__PURE__ */ (0, import_jsx_runtime41.jsx)(
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
          /* @__PURE__ */ (0, import_jsx_runtime41.jsxs)("div", { className: "stk-item__edit-row", children: [
            /* @__PURE__ */ (0, import_jsx_runtime41.jsxs)("span", { children: [
              /* @__PURE__ */ (0, import_jsx_runtime41.jsx)("span", { className: "stk-kbd", children: "Esc" }),
              " cancel"
            ] }),
            /* @__PURE__ */ (0, import_jsx_runtime41.jsxs)("span", { children: [
              /* @__PURE__ */ (0, import_jsx_runtime41.jsx)("span", { className: "stk-kbd", children: "\u21B5" }),
              " save \xB7 ",
              /* @__PURE__ */ (0, import_jsx_runtime41.jsx)("span", { className: "stk-kbd", children: "\u21E7\u21B5" }),
              " newline"
            ] }),
            /* @__PURE__ */ (0, import_jsx_runtime41.jsx)("span", { className: "stk-item__edit-spacer" }),
            /* @__PURE__ */ (0, import_jsx_runtime41.jsx)(
              "button",
              {
                type: "button",
                className: "stk-btn",
                onClick: () => onCancelEdit(item.id),
                children: "Cancel"
              }
            ),
            /* @__PURE__ */ (0, import_jsx_runtime41.jsx)(
              "button",
              {
                type: "button",
                className: "stk-btn stk-btn--save",
                onClick: () => onCommitEdit(item.id, draft),
                children: "Save"
              }
            )
          ] })
        ] }) : /* @__PURE__ */ (0, import_jsx_runtime41.jsxs)("div", { className: "stk-item__body", children: [
          /* @__PURE__ */ (0, import_jsx_runtime41.jsx)(
            "div",
            {
              className: `stk-item__text ${item.expanded ? "stk-item__text--expanded" : ""}`,
              onClick: longText ? () => onToggleExpand(item.id) : void 0,
              style: longText ? { cursor: "pointer" } : void 0,
              title: longText && !item.expanded ? item.text : void 0,
              children: item.text
            }
          ),
          /* @__PURE__ */ (0, import_jsx_runtime41.jsxs)("div", { className: "stk-item__meta", children: [
            isHead && /* @__PURE__ */ (0, import_jsx_runtime41.jsx)("span", { className: "stk-item__head-tag", children: "Next" }),
            /* @__PURE__ */ (0, import_jsx_runtime41.jsx)("span", { children: timeAgo(item.addedAt) }),
            item.status === "promoting" && /* @__PURE__ */ (0, import_jsx_runtime41.jsx)("span", { className: "stk-item__sending", children: "SENDING\u2026" })
          ] })
        ] }),
        !item.editing && /* @__PURE__ */ (0, import_jsx_runtime41.jsxs)("div", { className: "stk-item__actions", children: [
          /* @__PURE__ */ (0, import_jsx_runtime41.jsxs)(
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
                /* @__PURE__ */ (0, import_jsx_runtime41.jsx)("span", { className: "stk-btn__glyph", children: "\u21AA" }),
                " Steer"
              ]
            }
          ),
          /* @__PURE__ */ (0, import_jsx_runtime41.jsx)(
            "button",
            {
              type: "button",
              className: "stk-btn stk-btn--icon",
              onClick: () => onEdit(item.id),
              "aria-label": "Edit message",
              title: "Edit message",
              "data-testid": `pending-edit:${item.id}`,
              children: /* @__PURE__ */ (0, import_jsx_runtime41.jsx)("span", { className: "stk-btn__glyph", children: "\u270E" })
            }
          ),
          /* @__PURE__ */ (0, import_jsx_runtime41.jsx)(
            "button",
            {
              type: "button",
              className: "stk-btn stk-btn--icon stk-btn--trash",
              onClick: () => onTrash(item.id),
              "aria-label": "Remove from queue",
              title: "Remove from queue",
              "data-testid": `pending-trash:${item.id}`,
              children: /* @__PURE__ */ (0, import_jsx_runtime41.jsx)("span", { className: "stk-btn__glyph", children: "\xD7" })
            }
          )
        ] })
      ]
    }
  );
}
function PendingStack3({
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
  const [, setTick] = import_react33.default.useState(0);
  import_react33.default.useEffect(() => {
    const t = window.setInterval(() => setTick((n) => n + 1), 1e4);
    return () => window.clearInterval(t);
  }, []);
  const [dragId, setDragId] = import_react33.default.useState(null);
  const [dropTarget, setDropTarget] = import_react33.default.useState({ id: null, where: null });
  const [collapsed, setCollapsed] = import_react33.default.useState(false);
  const lastCount = import_react33.default.useRef(0);
  import_react33.default.useEffect(() => {
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
  return /* @__PURE__ */ (0, import_jsx_runtime41.jsxs)(
    "section",
    {
      className: `stack ${collapsed ? "is-collapsed" : ""} ${reducedMotion ? "reduced-motion" : ""}`,
      "aria-label": "Pending message queue",
      "data-testid": "pending-stack",
      children: [
        /* @__PURE__ */ (0, import_jsx_runtime41.jsx)(
          StackHead,
          {
            count: items.length,
            agentBusy,
            collapsed,
            onToggleCollapsed: () => setCollapsed((c) => !c),
            onClear: onClearAll
          }
        ),
        /* @__PURE__ */ (0, import_jsx_runtime41.jsx)("ol", { className: "stack__list", role: "list", children: items.map((item, i) => /* @__PURE__ */ (0, import_jsx_runtime41.jsx)(
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
var import_jsx_runtime42 = require("react/jsx-runtime");
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
function isWorkGraphSignalFrame(frame) {
  if (frame.event.startsWith("workgraph.")) return true;
  if (frame.event !== "tool_execution_completed" && frame.event !== "tool_result_received") {
    return false;
  }
  const data = frame.data && typeof frame.data === "object" ? frame.data : null;
  const name = typeof data?.name === "string" ? data.name : typeof data?.tool_name === "string" ? data.tool_name : "";
  return name.startsWith("workgraph_");
}
var DOCK_LAYOUT_STORAGE_PREFIX = "mobkit-console-dock-state";
function createIdempotencyKey() {
  return createConsoleId("console");
}
function dockLayoutStorageKey(baseUrl, experience) {
  const runtimeId = experience?.runtime_id?.trim();
  const title = experience?.console_config?.title?.trim();
  return `${DOCK_LAYOUT_STORAGE_PREFIX}:${runtimeId || title || baseUrl}`;
}
function stableHash(value) {
  let hash2 = 5381;
  for (let i = 0; i < value.length; i += 1) {
    hash2 = (hash2 << 5) + hash2 ^ value.charCodeAt(i);
  }
  return (hash2 >>> 0).toString(36);
}
function sidebarAgentListConfigIdentity(experience) {
  const agentList = experience?.console_config?.agent_list;
  if (!agentList) return "no-agent-list";
  const sections = (agentList.sections || []).map((section) => ({
    name: section.name,
    empty_title: section.empty_title,
    empty_text: section.empty_text
  }));
  return stableHash(JSON.stringify({
    group_by: agentList.group_by || [],
    subgroup_by: agentList.subgroup_by || [],
    section_order: agentList.section_order || [],
    fallback_group: agentList.fallback_group || "",
    fallback_subgroup: agentList.fallback_subgroup || "",
    collapse_single_subgroup: agentList.collapse_single_subgroup !== false,
    sections
  }));
}
function sidebarPreferencesScope(baseUrl, experience) {
  const runtimeId = experience?.runtime_id?.trim();
  const title = experience?.console_config?.title?.trim();
  return runtimeId || title || baseUrl;
}
function sidebarPreferencesNamespace(baseUrl, experience) {
  return [sidebarPreferencesScope(baseUrl, experience), sidebarAgentListConfigIdentity(experience)].map((part) => encodeURIComponent(part)).join(":");
}
function browserLocalStorage() {
  if (typeof window === "undefined") return null;
  try {
    return window.localStorage;
  } catch {
    return null;
  }
}
function cursorSeq2(cursor) {
  if (!cursor) return null;
  const match = /^console:(\d+)$/.exec(cursor);
  if (!match) return null;
  const parsed = Number(match[1]);
  return Number.isFinite(parsed) ? parsed : null;
}
function isTerminalTurnCompletedFrame(frame) {
  if (frame.event !== "turn_completed") return false;
  const data = frame.data && typeof frame.data === "object" ? frame.data : {};
  const stopReason = data.stop_reason ?? data.stopReason;
  return typeof stopReason === "string" ? stopReason !== "tool_use" : true;
}
function isActiveServerToolContentFrame2(frame) {
  if (frame.event !== "server_tool_content") return false;
  const record = frame.data && typeof frame.data === "object" ? frame.data : null;
  const content = record?.content && typeof record.content === "object" ? record.content : null;
  const type = typeof content?.type === "string" ? content.type : typeof record?.type === "string" ? record.type : "";
  if (type === "message_annotations" || Array.isArray(content?.annotations) || type.includes(".completed") || type.includes(".done") || type.includes(".failed") || type.includes(".error")) {
    return false;
  }
  return type.includes(".in_progress") || type.includes(".searching") || type.includes(".started") || type.includes("_call");
}
function isTerminalServerToolContentFrame2(frame) {
  if (frame.event !== "server_tool_content") return false;
  const record = frame.data && typeof frame.data === "object" ? frame.data : null;
  const content = record?.content && typeof record.content === "object" ? record.content : null;
  const type = typeof content?.type === "string" ? content.type : typeof record?.type === "string" ? record.type : "";
  const status = typeof content?.status === "string" ? content.status : typeof record?.status === "string" ? record.status : "";
  if (type === "message_annotations" || Array.isArray(content?.annotations)) {
    return false;
  }
  return type.includes(".completed") || type.includes(".done") || type.includes(".failed") || type.includes(".error") || status === "completed" || status === "done" || status === "succeeded" || status === "failed" || status === "error";
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
  "tool_execution_completed",
  "server_tool_content"
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
  "reasoning_delta",
  "reasoning_complete",
  "turn_completed",
  "tool_call_requested",
  "tool_call",
  "tool_result_received",
  "tool_execution_started",
  "tool_execution_completed",
  "server_tool_content",
  "run_started",
  "run_completed",
  "run_failed",
  "message_delivery_failed",
  "system_notice",
  "frame_updated"
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
  "snapshot_complete",
  "snapshot_started",
  "run_failed",
  "keep-alive",
  "tool_config_changed",
  "tool_scope_changed",
  "frame_updated",
  "text_delta",
  "tool_call",
  "tool_execution_started",
  "tool_result_received",
  "tool_execution_completed",
  "server_tool_content"
]);
function ConsoleApp({ baseUrl }) {
  const consoleFetchTimeoutMsRef = import_react34.default.useRef(DEFAULT_CONSOLE_FETCH_TIMEOUT_MS2);
  const consoleTransport = import_react34.default.useMemo(
    () => createHttpConsoleTransport2({
      baseUrl,
      fetchTimeoutMs: () => consoleFetchTimeoutMsRef.current
    }),
    [baseUrl]
  );
  const consoleController = import_react34.default.useMemo(
    () => createMobKitConsoleController2({ transport: consoleTransport }),
    [consoleTransport]
  );
  const [experience, setExperience] = import_react34.default.useState(
    null
  );
  const [agents, setAgents] = import_react34.default.useState([]);
  const [draftByKey, setDraftByKey] = import_react34.default.useState(
    {}
  );
  const [stagedAttachmentsByIdentity, setStagedAttachmentsByIdentity] = import_react34.default.useState({});
  const [sendingPanels, setSendingPanels] = import_react34.default.useState(
    /* @__PURE__ */ new Set()
  );
  const [pinnedAgentIds, setPinnedAgentIds] = import_react34.default.useState(
    /* @__PURE__ */ new Set()
  );
  const [inspectByIdentity, setInspectByIdentity] = import_react34.default.useState({});
  const [routingData, setRoutingData] = import_react34.default.useState({
    routes: [],
    deliveries: []
  });
  const [gatingData, setGatingData] = import_react34.default.useState({
    pending: [],
    audit: []
  });
  const [accessData, setAccessData] = import_react34.default.useState({
    status: null,
    config: null,
    error: null
  });
  const [memoryData, setMemoryData] = import_react34.default.useState({
    records: [],
    realms: [],
    quarantineRecords: [],
    pendingPromotions: [],
    dreams: [],
    detail: null,
    detailLoading: false,
    unavailable: false,
    error: null,
    nextCursor: null,
    recordsDenied: false,
    dreamsDenied: false,
    operatorScopeDenied: false,
    mobScopeDenied: false,
    overview: null,
    overviewDenied: false,
    proposals: [],
    proposalsDenied: false,
    injections: [],
    injectionsDenied: false,
    harvests: [],
    harvestsDenied: false,
    dreamRuns: [],
    dreamRunsDenied: false,
    auditVerdicts: [],
    auditVerdictsDenied: false
  });
  const [workGraphData, setWorkGraphData] = import_react34.default.useState({
    items: [],
    edges: [],
    attention: [],
    events: [],
    capturedAt: null,
    unavailable: false,
    denied: false,
    error: null
  });
  const [activeActivityPresetId, setActiveActivityPresetId] = import_react34.default.useState("");
  const [selectedRosterMemberId, setSelectedRosterMemberId] = import_react34.default.useState("");
  const [loading, setLoading] = import_react34.default.useState(true);
  const [loadingHistory, setLoadingHistory] = import_react34.default.useState({});
  const [error, setError] = import_react34.default.useState("");
  const [actionError, setActionError] = import_react34.default.useState("");
  const [theme, setTheme] = import_react34.default.useState(() => {
    try {
      return localStorage.getItem("mobkit-console-theme") || "light";
    } catch {
      return "light";
    }
  });
  const [variant, setVariant] = useConsoleVariant();
  const sidebarStorageScope = import_react34.default.useMemo(
    () => sidebarPreferencesScope(baseUrl, experience),
    [baseUrl, experience]
  );
  const sidebarStorageNamespace = import_react34.default.useMemo(
    () => sidebarPreferencesNamespace(baseUrl, experience),
    [baseUrl, experience]
  );
  const sidebarPinsStorageKey = import_react34.default.useMemo(
    () => sidebarStorageKey(SIDEBAR_PINS_STORAGE_PREFIX, sidebarStorageNamespace),
    [sidebarStorageNamespace]
  );
  import_react34.default.useEffect(() => {
    pruneStaleSidebarStorage(browserLocalStorage(), sidebarStorageScope, sidebarStorageNamespace);
  }, [sidebarStorageScope, sidebarStorageNamespace]);
  const [sidebarCollapsed, setSidebarCollapsed] = import_react34.default.useState(
    () => {
      try {
        return localStorage.getItem("mobkit-console-sidebar-collapsed") === "1";
      } catch {
        return false;
      }
    }
  );
  const toggleSidebarCollapsed = import_react34.default.useCallback(() => {
    setSidebarCollapsed((c) => {
      const next = !c;
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
  const [railCollapsed, setRailCollapsed] = import_react34.default.useState(() => {
    try {
      return localStorage.getItem("mobkit-console-rail-collapsed") === "1";
    } catch {
      return false;
    }
  });
  const toggleRailCollapsed = import_react34.default.useCallback(() => {
    setRailCollapsed((c) => {
      const next = !c;
      try {
        localStorage.setItem("mobkit-console-rail-collapsed", next ? "1" : "0");
      } catch {
      }
      return next;
    });
  }, []);
  const defaultPinnedAgentIdsKey = import_react34.default.useMemo(
    () => JSON.stringify(experience?.console_config?.agent_list?.default_pinned_agent_ids || []),
    [experience?.console_config?.agent_list?.default_pinned_agent_ids]
  );
  import_react34.default.useEffect(() => {
    const defaults = new Set(experience?.console_config?.agent_list?.default_pinned_agent_ids || []);
    const stored = readSidebarStringSet(
      browserLocalStorage(),
      sidebarPinsStorageKey
    );
    setPinnedAgentIds(stored ?? defaults);
  }, [defaultPinnedAgentIdsKey, experience?.console_config?.agent_list, sidebarPinsStorageKey]);
  const togglePinnedAgent = import_react34.default.useCallback((agent, renderedFamilyPinIds) => {
    const pinId = sidebarAgentPinId2(agent);
    setPinnedAgentIds((current) => {
      const next = new Set(current);
      const familyPinIds = renderedFamilyPinIds && renderedFamilyPinIds.size > 0 ? renderedFamilyPinIds : sidebarPinnedFamilyPinIds(agent, agents);
      const familyPinned = Array.from(familyPinIds).some((id) => next.has(id));
      if (next.has(pinId) || next.has(agent.member_id) || familyPinned) {
        for (const id of familyPinIds) next.delete(id);
      } else {
        next.add(pinId);
      }
      writeSidebarStringSet(
        browserLocalStorage(),
        sidebarPinsStorageKey,
        next
      );
      return next;
    });
  }, [agents, sidebarPinsStorageKey]);
  const [, setRenderTick] = import_react34.default.useState(0);
  const forceRender = import_react34.default.useCallback(() => setRenderTick((n) => n + 1), []);
  const stagedAttachmentsRef = import_react34.default.useRef(stagedAttachmentsByIdentity);
  import_react34.default.useEffect(() => {
    stagedAttachmentsRef.current = stagedAttachmentsByIdentity;
  }, [stagedAttachmentsByIdentity]);
  import_react34.default.useEffect(
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
  async function inspectIdentityViaHeadless(identity) {
    return executeHeadlessCommand(
      CONSOLE_COMMAND_NAMES2.inspectIdentity,
      identityWorkbenchTarget(identity, "inspect")
    );
  }
  function requireWorkbenchTarget(input) {
    const target = migrateConsoleWorkbenchTarget(input);
    if (!target) {
      throw new Error("invalid MobKit console target");
    }
    return target;
  }
  function identityWorkbenchTarget(identity, mode) {
    return requireWorkbenchTarget({
      id: mode === "inspect" ? `inspect:${identity}` : `chat:${identity}`,
      kind: mode === "inspect" ? "identity-inspect" : "agent-chat",
      title: identity,
      identity
    });
  }
  function controlWorkbenchTarget(kind) {
    return requireWorkbenchTarget(buildControlTarget2(kind));
  }
  async function executeHeadlessCommand(command, target, params) {
    return (await consoleController.commands.execute({
      command,
      target,
      params
    })).result;
  }
  const identityLogRef = import_react34.default.useRef({});
  const timelineFetchInFlightRef = import_react34.default.useRef(
    {}
  );
  const optimisticUserByPanelKeyRef = import_react34.default.useRef({});
  function getOrCreateLog(identity) {
    let log = identityLogRef.current[identity];
    if (!log) {
      log = {
        events: [],
        byKey: /* @__PURE__ */ new Map(),
        hasServerLog: null,
        olderHistoryExhausted: false,
        olderHistoryLoading: false
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
    return clearedPanelKeys.length > 0;
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
  function clearOptimisticUserByContent(identity, frame) {
    if (frame.event !== "interaction_started" && frame.event !== "user_input" && frame.event !== "run_started")
      return false;
    const record = frame.data && typeof frame.data === "object" ? frame.data : {};
    const contentValue = frame.event === "run_started" ? record.prompt : record.content;
    const content = typeof contentValue === "string" ? contentValue.trim() : "";
    if (!content) return false;
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
    return clearedPanelKeys.length > 0;
  }
  function clearOptimisticUserForFrame(identity, frame) {
    if ((frame.event === "interaction_started" || frame.event === "user_input" || frame.event === "run_started") && frame.interactionId && clearOptimisticUserByInteraction(frame.interactionId)) {
      return;
    }
    clearOptimisticUserByContent(identity, frame);
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
          log.events[existingIndex] = {
            ...log.events[existingIndex],
            ...updated
          };
          clearOptimisticUserForFrame(identity, updated);
          return true;
        }
      }
      return false;
    }
    const key = frameKey(frame);
    if (log.byKey.has(key)) return false;
    log.byKey.set(key, log.events.length);
    log.events.push(frame);
    clearOptimisticUserForFrame(identity, frame);
    return true;
  }
  function busyTransitionForFrame(frame) {
    if (frame.event === "user_input") {
      return isTerminalUserInputStatus2(frame.status) ? false : true;
    }
    if (frame.event === "interaction_started" || frame.event === "run_started" || frame.event === "reasoning_delta" || frame.event === "reasoning_complete" || frame.event === "tool_call_requested" || frame.event === "tool_call" || frame.event === "tool_execution_started" || frame.event === "server_tool_content" && isActiveServerToolContentFrame2(frame) || frame.event === "server_tool_content" && isTerminalServerToolContentFrame2(frame) || frame.event === "tool_result_received" || frame.event === "tool_execution_completed") {
      return true;
    }
    if (frame.event === "turn_completed" && isTerminalTurnCompletedFrame(frame) || frame.event === "interaction_complete" || frame.event === "interaction_failed" || frame.event === "run_completed" || frame.event === "run_failed" || frame.event === "system_notice" && systemNoticeClearsBusyState2(frame) || frame.event === "message_delivery_failed") {
      return false;
    }
    return null;
  }
  function isTerminalUserInputStatus2(status) {
    return status === "completed" || status === "delivery_failed" || status === "failed";
  }
  function busyTransitionSortRank(frame) {
    const transition = busyTransitionForFrame(frame);
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
  function updateBusyStateForFrame(identity, frame) {
    const lifecycle = identityLifecycleRef.current[identity] ?? {
      interactionOpen: false,
      runOpen: false
    };
    let sawLifecycle = true;
    switch (frame.event) {
      case "interaction_started":
        lifecycle.interactionOpen = true;
        break;
      case "run_started":
        lifecycle.runOpen = true;
        break;
      case "run_completed":
      case "run_failed":
        lifecycle.runOpen = false;
        break;
      case "interaction_complete":
      case "interaction_failed":
      case "message_delivery_failed":
        lifecycle.interactionOpen = false;
        lifecycle.runOpen = false;
        break;
      case "system_notice":
        if (systemNoticeClearsBusyState2(frame)) {
          lifecycle.interactionOpen = false;
          lifecycle.runOpen = false;
        } else {
          sawLifecycle = false;
        }
        break;
      default:
        sawLifecycle = false;
        break;
    }
    identityLifecycleRef.current[identity] = lifecycle;
    if (sawLifecycle) {
      applyBusyState(identity, lifecycle.interactionOpen || lifecycle.runOpen);
      return;
    }
    const transition = busyTransitionForFrame(frame);
    if (transition !== null) {
      applyBusyState(
        identity,
        transition || lifecycle.interactionOpen || lifecycle.runOpen
      );
    }
  }
  function recomputeBusyStateFromLog(identity) {
    const log = getOrCreateLog(identity);
    const lifecycleFrames = log.events.filter((frame) => busyTransitionForFrame(frame) !== null).sort((a, b) => {
      const timeDelta = (a.timestampMs || 0) - (b.timestampMs || 0);
      if (timeDelta !== 0) return timeDelta;
      const rankDelta = busyTransitionSortRank(a) - busyTransitionSortRank(b);
      if (rankDelta !== 0) return rankDelta;
      return (a.cursor || a.id || "").localeCompare(b.cursor || b.id || "");
    });
    const lifecycle = { interactionOpen: false, runOpen: false };
    let legacyBusy = false;
    for (const frame of lifecycleFrames) {
      switch (frame.event) {
        case "interaction_started":
          lifecycle.interactionOpen = true;
          break;
        case "run_started":
          lifecycle.runOpen = true;
          break;
        case "run_completed":
        case "run_failed":
          lifecycle.runOpen = false;
          break;
        case "interaction_complete":
        case "interaction_failed":
        case "message_delivery_failed":
          lifecycle.interactionOpen = false;
          lifecycle.runOpen = false;
          legacyBusy = false;
          break;
        case "system_notice":
          if (systemNoticeClearsBusyState2(frame)) {
            lifecycle.interactionOpen = false;
            lifecycle.runOpen = false;
            legacyBusy = false;
          }
          break;
        default: {
          const transition = busyTransitionForFrame(frame);
          if (transition !== null) legacyBusy = transition;
          break;
        }
      }
    }
    identityLifecycleRef.current[identity] = lifecycle;
    applyBusyState(
      identity,
      lifecycle.interactionOpen || lifecycle.runOpen || legacyBusy
    );
  }
  function reconcileServerLog(identity, frames, available) {
    const log = getOrCreateLog(identity);
    log.hasServerLog = available;
    let changed = false;
    for (const frame of frames) {
      if (!appendFrame(identity, frame)) continue;
      changed = true;
      if (updatePhaseForIdentity(identity, frame)) changed = true;
    }
    recomputeBusyStateFromLog(identity);
    if (recomputePhaseForIdentity(identity)) changed = true;
    return changed;
  }
  function newerCursor(a, b) {
    const aSeq = cursorSeq2(a);
    const bSeq = cursorSeq2(b);
    if (aSeq === null) return b || a;
    if (bSeq === null) return a || b;
    return bSeq > aSeq ? b : a;
  }
  function olderCursor(a, b) {
    const aSeq = cursorSeq2(a);
    const bSeq = cursorSeq2(b);
    if (aSeq === null) return b || a;
    if (bSeq === null) return a || b;
    return bSeq < aSeq ? b : a;
  }
  function noteIdentityTimelinePage(identity, page, target) {
    const log = getOrCreateLog(identity);
    const previousOldest = log.oldestTimelineCursor;
    const previousLatest = log.latestTimelineCursor;
    const previousExhausted = log.olderHistoryExhausted;
    const previousExhaustedAtCursor = log.olderHistoryExhaustedAtCursor;
    for (const frame of page.frames) {
      log.oldestTimelineCursor = olderCursor(log.oldestTimelineCursor, frame.cursor);
      log.latestTimelineCursor = newerCursor(log.latestTimelineCursor, frame.cursor);
    }
    if (target.mode === "recent") {
      log.latestTimelineCursor = newerCursor(log.latestTimelineCursor, page.latestCursor);
      if (target.before) {
        log.olderHistoryExhausted = page.exhausted === true;
        log.olderHistoryExhaustedAtCursor = page.exhausted === true ? log.oldestTimelineCursor : void 0;
      } else if (!log.olderHistoryExhaustedAtCursor) {
        log.olderHistoryExhausted = page.exhausted === true;
      }
    } else {
      log.latestTimelineCursor = newerCursor(
        log.latestTimelineCursor,
        page.nextCursor || page.latestCursor
      );
    }
    return previousOldest !== log.oldestTimelineCursor || previousLatest !== log.latestTimelineCursor || previousExhausted !== log.olderHistoryExhausted || previousExhaustedAtCursor !== log.olderHistoryExhaustedAtCursor;
  }
  function resetIdentityTimelineReplayMetadata(identity) {
    const log = getOrCreateLog(identity);
    const changed = log.events.length > 0 || log.byKey.size > 0 || log.oldestTimelineCursor !== void 0 || log.latestTimelineCursor !== void 0 || log.olderHistoryExhausted !== false || log.olderHistoryExhaustedAtCursor !== void 0;
    log.events = [];
    log.byKey.clear();
    log.oldestTimelineCursor = void 0;
    log.latestTimelineCursor = void 0;
    log.olderHistoryExhausted = false;
    log.olderHistoryExhaustedAtCursor = void 0;
    return changed;
  }
  async function queryIdentityTimelinePage(identity, target) {
    const pageFact = await consoleController.timeline.query(
      {
        identity,
        mode: target.mode,
        after: target.after,
        before: target.before,
        limit: target.limit ?? 200
      }
    );
    const page = pageFact.value;
    const metadataChanged = noteIdentityTimelinePage(identity, page, target);
    return { page, metadataChanged };
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
    setLoadingHistory(
      (current) => current[normalized] ? current : { ...current, [normalized]: true }
    );
    const request = (async () => {
      const { page } = await queryIdentityTimelinePage(normalized, {
        mode: "recent",
        limit: 200
      });
      reconcileServerLog(normalized, page.frames, page.available);
      if (options.clearPhase) clearPhaseForIdentity(normalized);
      forceRender();
    })().finally(() => {
      delete timelineFetchInFlightRef.current[normalized];
      setLoadingHistory((current) => {
        if (!current[normalized]) return current;
        const next = { ...current };
        delete next[normalized];
        return next;
      });
    });
    timelineFetchInFlightRef.current[normalized] = request;
    return request;
  }
  async function loadOlderIdentityTimeline(identity) {
    const normalized = identity.trim();
    if (!normalized) return;
    const log = getOrCreateLog(normalized);
    if (log.olderHistoryLoading || log.olderHistoryExhausted) return;
    log.olderHistoryLoading = true;
    forceRender();
    try {
      const { page } = await queryIdentityTimelinePage(normalized, {
        mode: "recent",
        before: log.oldestTimelineCursor,
        limit: 200
      });
      reconcileServerLog(normalized, page.frames, page.available);
    } catch {
    } finally {
      log.olderHistoryLoading = false;
      forceRender();
    }
  }
  function getSortedFrames(identity) {
    const log = identityLogRef.current[identity];
    if (!log) return [];
    return log.events.map((frame, index) => ({ frame, index })).sort((a, b) => {
      const ta = typeof a.frame.timestampMs === "number" ? a.frame.timestampMs : Number.MAX_SAFE_INTEGER;
      const tb = typeof b.frame.timestampMs === "number" ? b.frame.timestampMs : Number.MAX_SAFE_INTEGER;
      if (ta !== tb) return ta - tb;
      const ca = cursorSeq2(a.frame.cursor);
      const cb = cursorSeq2(b.frame.cursor);
      if (ca !== null && cb !== null && ca !== cb) return ca - cb;
      return a.index - b.index;
    }).map((entry) => entry.frame);
  }
  function framesVisibleInPanel(frames, panelId) {
    void panelId;
    return frames;
  }
  const activityRef = import_react34.default.useRef([]);
  const liveFramesRef = import_react34.default.useRef([]);
  const [liveFrames, setLiveFrames] = import_react34.default.useState([]);
  function commitLiveFrames(frames) {
    liveFramesRef.current = frames;
    setLiveFrames(frames);
  }
  const pendingStackRef = import_react34.default.useRef({});
  const PENDING_STACK_KEY_PREFIX = "mobkit-pending-stack:";
  const PENDING_DRAIN_CLAIM_TTL_MS = 15e3;
  const stackKeyFor = (identity) => `${PENDING_STACK_KEY_PREFIX}${identity}`;
  function loadPendingStack(identity, opts = {}) {
    try {
      const raw = localStorage.getItem(stackKeyFor(identity));
      if (!raw) return [];
      const parsed = JSON.parse(raw);
      if (!Array.isArray(parsed)) return [];
      const now = Date.now();
      return parsed.filter((it) => {
        if (!it || typeof it !== "object") return false;
        const r2 = it;
        return typeof r2.id === "string" && typeof r2.text === "string" && typeof r2.addedAt === "number";
      }).map((it) => {
        const r2 = it;
        const drainClaimedAt = typeof r2.drainClaimedAt === "number" ? r2.drainClaimedAt : void 0;
        const freshDrainClaim = opts.preserveFreshDraining === true && r2.status === "draining" && typeof r2.drainClaim === "string" && typeof drainClaimedAt === "number" && now - drainClaimedAt < PENDING_DRAIN_CLAIM_TTL_MS;
        return {
          id: it.id,
          text: it.text,
          addedAt: it.addedAt,
          status: freshDrainClaim ? "draining" : null,
          drainClaim: freshDrainClaim ? r2.drainClaim : void 0,
          drainClaimedAt: freshDrainClaim ? drainClaimedAt : void 0
        };
      });
    } catch {
      return [];
    }
  }
  function persistPendingStack(identity, items) {
    try {
      const clean = items.filter(
        (it) => it.status !== "trashing" && it.status !== "promoting"
      ).map((it) => ({
        id: it.id,
        text: it.text,
        addedAt: it.addedAt,
        ...it.status === "draining" ? {
          status: "draining",
          drainClaim: it.drainClaim,
          drainClaimedAt: it.drainClaimedAt
        } : {}
      }));
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
  import_react34.default.useEffect(() => {
    const onStorage = (e) => {
      if (!e.key || !e.key.startsWith(PENDING_STACK_KEY_PREFIX)) return;
      const identity = e.key.slice(PENDING_STACK_KEY_PREFIX.length);
      pendingStackRef.current[identity] = loadPendingStack(identity, {
        preserveFreshDraining: true
      });
      forceRender();
    };
    window.addEventListener("storage", onStorage);
    return () => window.removeEventListener("storage", onStorage);
  }, []);
  const identityBusyRef = import_react34.default.useRef({});
  const identityLifecycleRef = import_react34.default.useRef({});
  const isIdentityBusy = (identity) => identityBusyRef.current[identity] === true;
  const phaseRef = import_react34.default.useRef({});
  const phaseValueByKey = import_react34.default.useRef({});
  const phaseSinceByKey = import_react34.default.useRef({});
  const phaseTimerByKey = import_react34.default.useRef({});
  const refreshTimersRef = import_react34.default.useRef({});
  const experienceTimerRef = import_react34.default.useRef(null);
  const experienceLoadInFlightRef = import_react34.default.useRef(
    null
  );
  const agentsRef = import_react34.default.useRef([]);
  import_react34.default.useEffect(() => {
    agentsRef.current = agents;
  }, [agents]);
  const initialTargetOpened = import_react34.default.useRef(false);
  const dockLayoutHydrated = import_react34.default.useRef(false);
  const dockLayoutRestored = import_react34.default.useRef(false);
  const dockLayoutRestoring = import_react34.default.useRef(false);
  const dock = useConsoleDockController({
    createPanelState: ({ target }) => ({
      id: createConsoleId("panel"),
      target: target || null,
      mode: "console"
    })
  });
  const currentDockLayoutStorageKey = import_react34.default.useMemo(
    () => dockLayoutStorageKey(baseUrl, experience),
    [baseUrl, experience?.runtime_id, experience?.console_config?.title]
  );
  import_react34.default.useEffect(() => {
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
  import_react34.default.useEffect(() => {
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
    const timer = phaseTimerByKey.current[panelKey];
    if (timer !== void 0) {
      window.clearTimeout(timer);
      delete phaseTimerByKey.current[panelKey];
    }
  }
  function commitPanelPhase(panelKey, phase) {
    const previous = phaseValueByKey.current[panelKey] ?? null;
    clearPhaseTimer(panelKey);
    phaseValueByKey.current[panelKey] = phase;
    phaseSinceByKey.current[panelKey] = Date.now();
    phaseRef.current[panelKey] = phase;
    return previous !== phase;
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
  function updatePanelPhaseFromFrame(panelKey, frame, lifecycleBusy = false) {
    const currentPhase = phaseValueByKey.current[panelKey] ?? null;
    const elapsedMs = Date.now() - (phaseSinceByKey.current[panelKey] ?? 0);
    switch (frame.event) {
      case "user_input":
        if (isTerminalUserInputStatus2(frame.status)) return commitPanelPhase(panelKey, null);
        return commitPanelPhase(panelKey, "waiting");
      case "interaction_started":
        return commitPanelPhase(panelKey, "waiting");
      case "tool_call_requested":
      case "tool_call":
      case "tool_execution_started":
      case "server_tool_content":
        if (frame.event === "server_tool_content") {
          if (isTerminalServerToolContentFrame2(frame)) {
            return commitPanelPhase(panelKey, "waiting");
          }
          if (!isActiveServerToolContentFrame2(frame)) {
            return false;
          }
        }
        if (currentPhase === "waiting" && elapsedMs < 300) {
          schedulePanelPhase(panelKey, "tool-executing", 300 - elapsedMs);
          return true;
        }
        return commitPanelPhase(panelKey, "tool-executing");
      case "tool_result_received":
      case "tool_execution_completed":
        return commitPanelPhase(panelKey, "waiting");
      case "reasoning_delta":
        return commitPanelPhase(panelKey, "generating");
      case "reasoning_complete":
        return commitPanelPhase(panelKey, "waiting");
      case "text_delta": {
        if (currentPhase === "tool-executing") {
          const r2 = Math.max(0, 300 - elapsedMs);
          if (r2 > 0) {
            schedulePanelPhase(panelKey, "generating", r2);
            return true;
          }
        }
        if (currentPhase === "waiting" && elapsedMs < 300) {
          schedulePanelPhase(panelKey, "generating", 300 - elapsedMs);
          return true;
        }
        return commitPanelPhase(panelKey, "generating");
      }
      case "text_complete":
        return commitPanelPhase(panelKey, lifecycleBusy ? "waiting" : null);
      case "interaction_complete":
      case "interaction_failed":
        return commitPanelPhase(panelKey, null);
      case "run_completed":
      case "run_failed":
        return commitPanelPhase(panelKey, lifecycleBusy ? "waiting" : null);
      case "system_notice":
        if (systemNoticeClearsBusyState2(frame)) return commitPanelPhase(panelKey, null);
        return false;
      case "turn_completed":
        if (isTerminalTurnCompletedFrame(frame)) {
          return commitPanelPhase(panelKey, lifecycleBusy ? "waiting" : null);
        }
        return false;
      case "message_delivery_failed":
        return commitPanelPhase(panelKey, null);
      default:
        return false;
    }
  }
  const dockRef = import_react34.default.useRef(dock);
  dockRef.current = dock;
  function updatePhaseForIdentity(identity, frame) {
    let changed = false;
    const lifecycleBusy = isIdentityBusy(identity);
    for (const panel of dockRef.current.viewState.panels) {
      const target = panel.target;
      if (!target || target.kind !== "agent-chat") continue;
      if ((target.identity || target.memberId) !== identity) continue;
      if (updatePanelPhaseFromFrame(
        buildPanelConversationKey2(panel.id, target),
        frame,
        lifecycleBusy
      )) changed = true;
    }
    return changed;
  }
  function clearPhaseForIdentity(identity) {
    let changed = false;
    for (const panel of dockRef.current.viewState.panels) {
      const target = panel.target;
      if (!target || target.kind !== "agent-chat") continue;
      if ((target.identity || target.memberId) !== identity) continue;
      if (commitPanelPhase(buildPanelConversationKey2(panel.id, target), null)) {
        changed = true;
      }
    }
    return changed;
  }
  function commitPhaseForIdentity(identity, phase) {
    let changed = false;
    for (const panel of dockRef.current.viewState.panels) {
      const target = panel.target;
      if (!target || target.kind !== "agent-chat") continue;
      if ((target.identity || target.memberId) !== identity) continue;
      if (commitPanelPhase(buildPanelConversationKey2(panel.id, target), phase)) {
        changed = true;
      }
    }
    return changed;
  }
  function recomputePhaseForIdentity(identity) {
    const frames = getSortedFrames(identity).filter(
      (frame) => PANEL_ROUTABLE_EVENTS.has(frame.event)
    );
    const phase = inferResponsePhaseFromFrames2(frames, null);
    let changed = false;
    for (const panel of dockRef.current.viewState.panels) {
      const target = panel.target;
      if (!target || target.kind !== "agent-chat") continue;
      if ((target.identity || target.memberId) !== identity) continue;
      if (commitPanelPhase(buildPanelConversationKey2(panel.id, target), phase)) {
        changed = true;
      }
    }
    return changed;
  }
  const loadExperience = import_react34.default.useCallback(() => {
    if (experienceLoadInFlightRef.current) {
      return experienceLoadInFlightRef.current;
    }
    let request;
    request = (async () => {
      const [experienceJson, modulesJson] = await Promise.all([
        consoleTransport.loadExperience(),
        consoleTransport.loadModules?.() ?? Promise.resolve({ modules: [] })
      ]);
      const configuredTimeoutMs = experienceJson.console_policy?.fetch_timeout_ms;
      if (typeof configuredTimeoutMs === "number" && Number.isFinite(configuredTimeoutMs) && configuredTimeoutMs > 0) {
        consoleFetchTimeoutMsRef.current = configuredTimeoutMs;
      }
      const loadedModules = Array.isArray(modulesJson.modules) ? modulesJson.modules.map(String) : [];
      const nextAgents = normalizeAgents(experienceJson, loadedModules);
      setExperience(experienceJson);
      setAgents(nextAgents);
      setActiveActivityPresetId(
        (c) => c || experienceJson.console_config?.rail?.active_preset_id || experienceJson.activity_feed?.active_preset_id || "all"
      );
      return nextAgents;
    })().finally(() => {
      if (experienceLoadInFlightRef.current === request) {
        experienceLoadInFlightRef.current = null;
      }
    });
    experienceLoadInFlightRef.current = request;
    return request;
  }, [consoleTransport]);
  import_react34.default.useEffect(() => {
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
  import_react34.default.useEffect(() => {
    const timer = window.setInterval(() => {
      void loadExperience().catch(() => {
      });
    }, 15e3);
    return () => window.clearInterval(timer);
  }, [loadExperience]);
  import_react34.default.useEffect(() => {
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
  import_react34.default.useEffect(() => {
    const configured = experience?.console_config?.layout?.sidebar_collapsed;
    if (typeof configured !== "boolean") return;
    try {
      if (localStorage.getItem("mobkit-console-sidebar-collapsed") !== null)
        return;
    } catch {
    }
    setSidebarCollapsed(configured);
  }, [experience?.console_config?.layout?.sidebar_collapsed]);
  import_react34.default.useEffect(() => {
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
  const frontendReadOnly = import_react34.default.useMemo(() => resolveConsoleReadOnlyOverride(), []);
  const accessEnforcing = experience?.access?.enabled === true;
  const consoleReadOnly = frontendReadOnly || experience?.console_policy?.read_only === true || !accessEnforcing && experience?.runtime_capabilities?.can_send_messages === false;
  const consoleReadOnlyRef = import_react34.default.useRef(false);
  consoleReadOnlyRef.current = consoleReadOnly;
  const visibleControls = import_react34.default.useMemo(() => {
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
    if (configuredVisible.length > 0) {
      const extra = [];
      if (experience?.access?.can_administer === true) extra.push("access");
      if (experience?.memory?.can_read === true) extra.push("memory");
      if (experience?.workgraph?.available === true && experience?.workgraph?.can_view === true) {
        extra.push("workgraph");
      }
      return extra.length > 0 ? [...configuredVisible, ...extra] : configuredVisible;
    }
    const hidden = new Set(
      (sidebarConfig?.hidden_controls || []).map(normalizeNavKind).filter((kind) => Boolean(kind))
    );
    const controls = runtimeControls.filter((kind) => !hidden.has(kind));
    if (experience?.access?.can_administer === true) controls.push("access");
    if (experience?.memory?.can_read === true) controls.push("memory");
    if (experience?.workgraph?.available === true && experience?.workgraph?.can_view === true) {
      controls.push("workgraph");
    }
    return controls;
  }, [
    experience?.console_config?.sidebar,
    experience?.access?.can_administer,
    experience?.memory?.can_read,
    experience?.workgraph?.available,
    experience?.workgraph?.can_view,
    hasMobControlSurface
  ]);
  import_react34.default.useEffect(() => {
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
      target = buildControlTarget2(
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
      if (match) target = buildDockTarget2(match);
    }
    initialTargetOpened.current = true;
    if (!target) return;
    const preset = normalizeDockPreset(layoutConfig?.initial_preset);
    if (preset) dock.applyPreset(preset);
    dock.openTarget(target, "replace_focused");
  }, [agents, dock, experience, visibleControls]);
  import_react34.default.useEffect(() => {
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
      dock.openTarget(buildControlTarget2("roster"), "replace_focused");
    }
  }, [agents, dock.focusedTarget]);
  const refreshAccessData = import_react34.default.useCallback(async () => {
    const accessTarget = controlWorkbenchTarget("access");
    try {
      const status = await executeHeadlessCommand(
        CONSOLE_COMMAND_NAMES2.accessStatus,
        accessTarget
      ) || null;
      let config = null;
      if (status?.available && status?.can_administer) {
        const result = await executeHeadlessCommand(
          CONSOLE_COMMAND_NAMES2.getAccessConfig,
          accessTarget
        );
        config = result?.config || null;
      }
      setAccessData({ status, config, error: null });
    } catch (err) {
      setAccessData((current) => ({ ...current, error: errorMessage(err) }));
    }
  }, [baseUrl]);
  const refreshMemoryData = import_react34.default.useCallback(async () => {
    const memoryTarget = controlWorkbenchTarget("memory");
    try {
      let records = [];
      let realms = [];
      let nextCursor = null;
      let recordsDenied = false;
      try {
        const recordsResult = await executeHeadlessCommand(
          CONSOLE_COMMAND_NAMES2.listMemoryRecords,
          memoryTarget
        );
        records = recordsResult?.records || [];
        realms = recordsResult?.realms || [];
        nextCursor = recordsResult?.next_cursor ?? null;
      } catch (err) {
        if (memorySectionOutcome(err) !== "denied") throw err;
        recordsDenied = true;
      }
      let operatorScopeDenied = false;
      let mobScopeDenied = false;
      if (!recordsDenied) {
        const probeScope = async (scope) => {
          try {
            await executeHeadlessCommand(CONSOLE_COMMAND_NAMES2.listMemoryRecords, memoryTarget, {
              scope,
              limit: 1
            });
            return false;
          } catch (err) {
            if (memorySectionOutcome(err) === "denied") return true;
            throw err;
          }
        };
        [operatorScopeDenied, mobScopeDenied] = await Promise.all([
          probeScope("operator"),
          probeScope("mob")
        ]);
      }
      let quarantineRecords = [];
      let pendingPromotions = [];
      if (experience?.memory?.can_review_quarantine === true) {
        try {
          const quarantineResult = await executeHeadlessCommand(
            CONSOLE_COMMAND_NAMES2.listMemoryQuarantine,
            memoryTarget
          );
          quarantineRecords = quarantineResult?.records || [];
          pendingPromotions = quarantineResult?.pending_promotions || [];
        } catch (err) {
          if (jsonRpcErrorCode(err) !== -32030) throw err;
        }
      }
      let dreams = [];
      let dreamsDenied = false;
      try {
        const dreamsResult = await executeHeadlessCommand(
          CONSOLE_COMMAND_NAMES2.listMemoryDreams,
          memoryTarget
        );
        dreams = dreamsResult?.runs || [];
      } catch (err) {
        if (memorySectionOutcome(err) !== "denied") throw err;
        dreamsDenied = true;
      }
      const section = async (command, empty, pick) => {
        try {
          const result = await executeHeadlessCommand(command, memoryTarget);
          return { value: pick(result), denied: false };
        } catch (err) {
          if (memorySectionOutcome(err) !== "denied") throw err;
          return { value: empty, denied: true };
        }
      };
      const [overview, proposals, injections, harvests, dreamRuns, auditVerdicts] = await Promise.all([
        section(
          CONSOLE_COMMAND_NAMES2.getMemoryOverview,
          null,
          (result) => result ?? null
        ),
        section(
          CONSOLE_COMMAND_NAMES2.listMemoryProposals,
          [],
          (result) => result?.proposals || []
        ),
        section(
          CONSOLE_COMMAND_NAMES2.listMemoryInjections,
          [],
          (result) => result?.injections || []
        ),
        section(
          CONSOLE_COMMAND_NAMES2.listMemoryHarvests,
          [],
          (result) => result?.harvests || []
        ),
        section(
          CONSOLE_COMMAND_NAMES2.listMemoryDreamRuns,
          [],
          (result) => result?.runs || []
        ),
        section(
          CONSOLE_COMMAND_NAMES2.listMemoryAuditVerdicts,
          [],
          (result) => result?.verdicts || []
        )
      ]);
      setMemoryData((current) => ({
        ...current,
        records,
        realms,
        quarantineRecords,
        pendingPromotions,
        dreams,
        nextCursor,
        recordsDenied,
        dreamsDenied,
        operatorScopeDenied,
        mobScopeDenied,
        overview: overview.value,
        overviewDenied: overview.denied,
        proposals: proposals.value,
        proposalsDenied: proposals.denied,
        injections: injections.value,
        injectionsDenied: injections.denied,
        harvests: harvests.value,
        harvestsDenied: harvests.denied,
        dreamRuns: dreamRuns.value,
        dreamRunsDenied: dreamRuns.denied,
        auditVerdicts: auditVerdicts.value,
        auditVerdictsDenied: auditVerdicts.denied,
        unavailable: false,
        error: null
      }));
    } catch (err) {
      if (jsonRpcErrorCode(err) === -32601) {
        setMemoryData((current) => ({ ...current, unavailable: true, error: null }));
        return;
      }
      setMemoryData((current) => ({ ...current, error: errorMessage(err) }));
    }
  }, [baseUrl, experience?.memory?.can_review_quarantine]);
  const workGraphRefreshSequencerRef = import_react34.default.useRef(createWorkGraphRefreshSequencer());
  const refreshWorkGraphData = import_react34.default.useCallback(async () => {
    const workGraphTarget = controlWorkbenchTarget("workgraph");
    const isCurrent = workGraphRefreshSequencerRef.current.begin();
    try {
      let snapshot = null;
      let denied = false;
      try {
        snapshot = await executeHeadlessCommand(
          CONSOLE_COMMAND_NAMES2.workgraphSnapshot,
          workGraphTarget
        );
      } catch (err) {
        if (jsonRpcErrorCode(err) !== -32030) throw err;
        denied = true;
      }
      let events = [];
      if (!denied) {
        try {
          const eventsResult = await executeHeadlessCommand(
            CONSOLE_COMMAND_NAMES2.workgraphEvents,
            workGraphTarget,
            workGraphEventsParams(snapshot?.event_high_water_mark, 50)
          );
          events = workGraphEventsNewestFirst(eventsResult?.events || []);
        } catch (err) {
          if (jsonRpcErrorCode(err) !== -32030) throw err;
        }
      }
      if (!isCurrent()) return;
      setWorkGraphData({
        items: snapshot?.items || [],
        edges: snapshot?.edges || [],
        attention: snapshot?.attention || [],
        events,
        capturedAt: snapshot?.captured_at || null,
        unavailable: false,
        denied,
        error: null
      });
    } catch (err) {
      if (!isCurrent()) return;
      const code = jsonRpcErrorCode(err);
      const capabilityMissing = err instanceof Error && err.message.startsWith("MobKit capability missing");
      if (code === -32601 || code === -32041 || capabilityMissing) {
        setWorkGraphData((current) => ({ ...current, unavailable: true, error: null }));
        return;
      }
      setWorkGraphData((current) => ({ ...current, error: errorMessage(err) }));
    }
  }, [baseUrl]);
  const queryMemoryRecords = import_react34.default.useCallback(
    async (params) => {
      try {
        return await executeHeadlessCommand(
          CONSOLE_COMMAND_NAMES2.listMemoryRecords,
          controlWorkbenchTarget("memory"),
          params
        );
      } catch (err) {
        if (memorySectionOutcome(err) === "denied") return null;
        setMemoryData((current) => ({ ...current, error: errorMessage(err) }));
        throw err;
      }
    },
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [baseUrl]
  );
  const loadMemoryEvidence = import_react34.default.useCallback(
    async (identity, evidence) => {
      if (!evidence.session_id) return null;
      try {
        const pageFact = await consoleController.timeline.query({
          ...identity ? { identity } : {},
          mode: "recent",
          limit: 1e3
        });
        const page = pageFact.value;
        if (!page.available) return null;
        const frames = page.frames.filter(
          (frame) => frame.sessionId === evidence.session_id
        );
        if (frames.length === 0) return null;
        return mapFramesToTimelineEntries2(null, frames, {
          renderInteractionStartsAsUser: true
        });
      } catch {
        return null;
      }
    },
    [consoleController]
  );
  const loadMemoryRecordDetail = import_react34.default.useCallback(
    async (realm, memoryId) => {
      setMemoryData((current) => ({ ...current, detail: null, detailLoading: true, error: null }));
      try {
        const result = await executeHeadlessCommand(
          CONSOLE_COMMAND_NAMES2.getMemoryRecord,
          controlWorkbenchTarget("memory"),
          realm ? { realm, memory_id: memoryId } : { memory_id: memoryId }
        );
        const detail = result?.record ? {
          realm: result.realm,
          record: result.record,
          chain: result.chain || [],
          injections: result.injections || []
        } : null;
        setMemoryData((current) => ({ ...current, detail, detailLoading: false }));
      } catch (err) {
        setMemoryData((current) => ({
          ...current,
          detail: null,
          detailLoading: false,
          error: errorMessage(err)
        }));
      }
    },
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [baseUrl]
  );
  const runAccessMutation = import_react34.default.useCallback(
    async (command, params) => {
      try {
        await executeHeadlessCommand(command, controlWorkbenchTarget("access"), params);
        setAccessData((current) => ({ ...current, error: null }));
      } catch (err) {
        setAccessData((current) => ({ ...current, error: errorMessage(err) }));
      }
      await refreshAccessData();
      await loadExperience().catch(() => {
      });
    },
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [baseUrl, refreshAccessData, loadExperience]
  );
  const refreshPanelData = import_react34.default.useCallback(async () => {
    const openPanels = dock.viewState.panels.map((p) => p.target).filter(Boolean);
    const inspects = openPanels.filter(
      (t) => t.kind === "identity-inspect"
    );
    if (inspects.length) {
      const entries = await Promise.all(
        inspects.map(async (t) => {
          const r2 = await inspectIdentityViaHeadless(t.identity);
          return [t.identity, normalizeConsoleInspectResult(r2)];
        })
      );
      setInspectByIdentity((c) => ({ ...c, ...Object.fromEntries(entries) }));
    }
    if (hasMobControlSurface && openPanels.some((t) => t.kind === "routing")) {
      const routingTarget = controlWorkbenchTarget("routing");
      const [routes, history] = await Promise.all([
        executeHeadlessCommand(CONSOLE_COMMAND_NAMES2.listRoutingRoutes, routingTarget),
        executeHeadlessCommand(CONSOLE_COMMAND_NAMES2.listDeliveryHistory, routingTarget)
      ]);
      setRoutingData(
        buildRoutingSectionView2({
          routesResponse: routes,
          historyResponse: history
        })
      );
    }
    if (openPanels.some((t) => t.kind === "access")) {
      await refreshAccessData();
    }
    if (openPanels.some((t) => t.kind === "memory")) {
      await refreshMemoryData();
    }
    if (openPanels.some((t) => t.kind === "workgraph")) {
      await refreshWorkGraphData();
    }
    if (hasMobControlSurface && openPanels.some((t) => t.kind === "gating" || t.kind === "gates")) {
      const gatingTarget = controlWorkbenchTarget("gating");
      const [p, a] = await Promise.all([
        executeHeadlessCommand(CONSOLE_COMMAND_NAMES2.listGatingPending, gatingTarget),
        executeHeadlessCommand(CONSOLE_COMMAND_NAMES2.listGatingAudit, gatingTarget, { limit: 50 })
      ]);
      const pending = p && typeof p === "object" ? p : {};
      const audit = a && typeof a === "object" ? a : {};
      setGatingData({
        pending: Array.isArray(pending.pending) ? pending.pending : [],
        audit: Array.isArray(audit.entries) ? audit.entries : []
      });
    }
  }, [baseUrl, dock.viewState.panels, hasMobControlSurface, refreshAccessData, refreshMemoryData, refreshWorkGraphData]);
  import_react34.default.useEffect(() => {
    void refreshPanelData().catch(() => {
    });
  }, [dock.viewState.panels, refreshPanelData]);
  const scheduleExperienceRefresh = import_react34.default.useCallback(() => {
    if (experienceTimerRef.current !== null) return;
    experienceTimerRef.current = window.setTimeout(async () => {
      experienceTimerRef.current = null;
      await loadExperience().catch(() => {
      });
      await refreshPanelData().catch(() => {
      });
    }, 150);
  }, [loadExperience, refreshPanelData]);
  const scheduleHistoryRefresh = import_react34.default.useCallback(
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
  import_react34.default.useEffect(() => {
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
  import_react34.default.useEffect(() => {
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
          const sinceCursor = log.latestTimelineCursor && !(log.olderHistoryExhausted === true && !log.olderHistoryExhaustedAtCursor) ? log.latestTimelineCursor : void 0;
          const { page, metadataChanged } = await queryIdentityTimelinePage(identity, {
            mode: sinceCursor ? "since" : "recent",
            after: sinceCursor,
            limit: sinceCursor ? 1e3 : 200
          });
          if (reconcileServerLog(identity, page.frames, page.available) || metadataChanged) {
            changed = true;
          }
        } catch (error2) {
          const replay = error2;
          if (replay.timelineReplayUnavailable || replay.replayError?.stream === "timeline") {
            if (resetIdentityTimelineReplayMetadata(identity)) {
              changed = true;
            }
            try {
              const { page, metadataChanged } = await queryIdentityTimelinePage(identity, {
                mode: "recent",
                limit: 200
              });
              if (reconcileServerLog(identity, page.frames, page.available) || metadataChanged) {
                changed = true;
              }
            } catch {
            }
            continue;
          }
        }
      }
      if (changed) forceRender();
    };
    const timer = window.setInterval(() => {
      void refreshOpenChatPanels();
    }, 2e3);
    void refreshOpenChatPanels();
    return () => window.clearInterval(timer);
  }, [baseUrl, dock.viewState.panels, forceRender]);
  const scheduleHistoryRefreshRef = import_react34.default.useRef(scheduleHistoryRefresh);
  scheduleHistoryRefreshRef.current = scheduleHistoryRefresh;
  const scheduleExperienceRefreshRef = import_react34.default.useRef(scheduleExperienceRefresh);
  scheduleExperienceRefreshRef.current = scheduleExperienceRefresh;
  const refreshMemoryDataRef = import_react34.default.useRef(refreshMemoryData);
  refreshMemoryDataRef.current = refreshMemoryData;
  const memoryPanelDockedRef = import_react34.default.useRef(false);
  memoryPanelDockedRef.current = dock.viewState.panels.some(
    (panel) => panel.target?.kind === "memory"
  );
  const memoryRefreshTimerRef = import_react34.default.useRef(null);
  const refreshWorkGraphDataRef = import_react34.default.useRef(refreshWorkGraphData);
  refreshWorkGraphDataRef.current = refreshWorkGraphData;
  const workGraphPanelDockedRef = import_react34.default.useRef(false);
  workGraphPanelDockedRef.current = dock.viewState.panels.some(
    (panel) => panel.target?.kind === "workgraph"
  );
  const workGraphRefreshTimerRef = import_react34.default.useRef(null);
  import_react34.default.useEffect(() => {
    const handleLiveFrame = (incomingFrame) => {
      const canonicalIdentity = canonicalConsoleIdentity(
        incomingFrame.identity,
        agentsRef.current
      );
      const frame = canonicalIdentity && canonicalIdentity !== incomingFrame.identity ? { ...incomingFrame, identity: canonicalIdentity } : incomingFrame;
      if (!ACTIVITY_SKIP_EVENTS.has(frame.event)) {
        activityRef.current = [frame, ...activityRef.current].slice(0, 200);
      }
      if (PANEL_ROUTABLE_EVENTS.has(frame.event)) {
        commitLiveFrames([frame, ...liveFramesRef.current].slice(0, 300));
      }
      const identity = canonicalIdentity || frame.identity?.trim();
      if (PANEL_ROUTABLE_EVENTS.has(frame.event) && identity && identity !== "_system") {
        appendFrame(identity, frame);
        updateBusyStateForFrame(identity, frame);
        updatePhaseForIdentity(identity, frame);
      }
      forceRender();
      if ((HISTORY_REFRESH_EVENTS.has(frame.event) || isTerminalTurnCompletedFrame(frame)) && identity && identity !== "_system") {
        scheduleHistoryRefreshRef.current(identity);
      }
      if (REFRESH_TRIGGER_EVENTS.has(frame.event)) {
        scheduleExperienceRefreshRef.current();
      }
      if (frame.event.startsWith("memory.") && memoryPanelDockedRef.current && memoryRefreshTimerRef.current === null) {
        memoryRefreshTimerRef.current = window.setTimeout(() => {
          memoryRefreshTimerRef.current = null;
          void refreshMemoryDataRef.current().catch(() => {
          });
        }, 250);
      }
      if (isWorkGraphSignalFrame(frame) && workGraphPanelDockedRef.current && workGraphRefreshTimerRef.current === null) {
        workGraphRefreshTimerRef.current = window.setTimeout(() => {
          workGraphRefreshTimerRef.current = null;
          void refreshWorkGraphDataRef.current().catch(() => {
          });
        }, 250);
      }
    };
    let stopped = false;
    let unsubscribe = null;
    void consoleController.timeline.subscribeWithBackfill({ limit: 200 }, (frame) => {
      if (!stopped) handleLiveFrame(frame.value);
    }).then((nextUnsubscribe) => {
      if (stopped) {
        nextUnsubscribe();
      } else {
        unsubscribe = nextUnsubscribe;
      }
    }).catch(() => {
      if (!stopped) unsubscribe = consoleTransport.subscribeTimeline({}, handleLiveFrame);
    });
    return () => {
      stopped = true;
      unsubscribe?.();
    };
  }, [consoleController, consoleTransport]);
  import_react34.default.useEffect(() => {
    return () => {
      for (const timer of Object.values(phaseTimerByKey.current))
        window.clearTimeout(timer);
      for (const timer of Object.values(refreshTimersRef.current))
        window.clearTimeout(timer);
      if (experienceTimerRef.current !== null)
        window.clearTimeout(experienceTimerRef.current);
      if (memoryRefreshTimerRef.current !== null)
        window.clearTimeout(memoryRefreshTimerRef.current);
      if (workGraphRefreshTimerRef.current !== null)
        window.clearTimeout(workGraphRefreshTimerRef.current);
    };
  }, []);
  function openAgentChat(agent, intent = "replace_focused") {
    const target = buildDockTarget2(agent);
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
    const agent = agents.find((c) => c.member_id === item.id);
    if (agent) openAgentChat(agent);
  }
  async function submitMessageNow(panelId, target, text, handlingMode, attachments = []) {
    if (target.kind !== "agent-chat") return false;
    if (consoleReadOnlyRef.current) return false;
    const panelKey = buildPanelConversationKey2(panelId, target);
    const identity = target.identity || target.memberId;
    const optimisticObjectUrls = attachments.map(
      (file) => URL.createObjectURL(file)
    );
    const userEntry = createUserEntry2(
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
    commitPhaseForIdentity(identity, "waiting");
    identityBusyRef.current[identity] = true;
    commitLiveFrames([{
      id: `optimistic-topology:${identity}:${Date.now()}`,
      event: "interaction_started",
      identity,
      interactionId: "",
      timestampMs: Date.now(),
      data: {
        origin: `console:${panelId}`,
        handling_mode: handlingMode
      }
    }, ...liveFramesRef.current].slice(0, 300));
    forceRender();
    try {
      const workbenchTarget = migrateConsoleWorkbenchTarget(target);
      if (!workbenchTarget) {
        throw new Error("console send requires an identity-addressed target");
      }
      const result = (await consoleController.commands.sendMessage(
        workbenchTarget,
        {
          content: text,
          origin: `console:${panelId}`,
          idempotencyKey: createIdempotencyKey(),
          handlingMode,
          attachments
        }
      )).accepted.value;
      const optimisticUser = optimisticUserByPanelKeyRef.current[panelKey];
      if (optimisticUser) {
        optimisticUser.interactionId = result.interaction_id;
        const matched = log.events.some(
          (f) => (f.event === "interaction_started" || f.event === "user_input" || f.event === "run_started") && f.interactionId === result.interaction_id
        );
        if (matched) {
          optimisticUser.objectUrls?.forEach(
            (url) => URL.revokeObjectURL(url)
          );
          delete optimisticUserByPanelKeyRef.current[panelKey];
        }
      }
      setActionError("");
      return true;
    } catch (submitError) {
      optimisticUserByPanelKeyRef.current[panelKey]?.objectUrls?.forEach(
        (url) => URL.revokeObjectURL(url)
      );
      delete optimisticUserByPanelKeyRef.current[panelKey];
      commitPanelPhase(panelKey, null);
      identityBusyRef.current[identity] = false;
      setActionError(errorMessage(submitError));
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
    if (consoleReadOnly) return false;
    const panelKey = buildPanelConversationKey2(panelId, target);
    const identity = target.identity || target.memberId;
    const rawDraft = draftByKey[panelKey] || "";
    const text = rawDraft.trim();
    if (!text && attachments.length === 0) return false;
    const stack = getPendingStack(identity);
    const visiblePhase = phaseValueByKey.current[panelKey] ?? phaseRef.current[panelKey] ?? null;
    const agentPhase = agentsRef.current.find(
      (candidate) => [candidate.identity, candidate.member_id, candidate.agent_id].includes(
        identity
      )
    )?.response_phase ?? null;
    const shouldQueue = isIdentityBusy(identity) || visiblePhase !== null || agentPhase !== null || stack.length > 0;
    const clearSubmittedDraft = () => {
      setDraftByKey((current) => {
        if ((current[panelKey] || "") !== rawDraft) return current;
        return { ...current, [panelKey]: "" };
      });
    };
    const restoreSubmittedDraftIfEmpty = () => {
      setDraftByKey((current) => {
        if ((current[panelKey] || "") !== "") return current;
        return { ...current, [panelKey]: rawDraft };
      });
    };
    if (!shouldQueue || attachments.length > 0) {
      if (attachments.length === 0) {
        clearSubmittedDraft();
      }
      const sent = await submitMessageNow(
        panelId,
        target,
        text,
        "queue",
        attachments
      );
      if (sent) {
        clearSubmittedDraft();
      } else if (attachments.length === 0) {
        restoreSubmittedDraftIfEmpty();
      }
      return sent;
    }
    const newId = `pmsg-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 7)}`;
    setPendingStack(identity, (prev) => [
      ...prev,
      { id: newId, text, addedAt: Date.now(), status: "entering" }
    ]);
    clearSubmittedDraft();
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
  const pendingDrainOwnerRef = import_react34.default.useRef(
    `tab-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 8)}`
  );
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
    if (consoleReadOnlyRef.current) return;
    setPendingStack(
      identity,
      (prev) => prev.map(
        (it) => it.id === id ? { ...it, status: "promoting", editing: false } : it
      )
    );
    window.setTimeout(() => {
      if (consoleReadOnlyRef.current) return;
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
    if (consoleReadOnlyRef.current) return;
    const stack = getPendingStack(identity);
    if (stack.length === 0) return;
    const target = findChatTargetFor(identity);
    if (!target) return;
    if (stack.some((it) => it.status === "draining" || it.status === "promoting"))
      return;
    const head = stack.find((it) => !it.status || it.status === "entering");
    if (!head) return;
    const drainClaim = `${pendingDrainOwnerRef.current}:${head.id}:${Date.now().toString(36)}`;
    const drainClaimedAt = Date.now();
    setPendingStack(
      identity,
      (prev) => prev.map(
        (it) => it.id === head.id ? { ...it, status: "draining", drainClaim, drainClaimedAt } : it
      )
    );
    window.setTimeout(() => {
      if (consoleReadOnlyRef.current) return;
      const persistedHead = loadPendingStack(identity, {
        preserveFreshDraining: true
      }).find((it) => it.id === head.id);
      if (persistedHead?.drainClaim !== drainClaim) return;
      const target2 = findChatTargetFor(identity);
      if (!target2) {
        setPendingStack(
          identity,
          (prev) => prev.map(
            (it) => it.id === head.id && it.drainClaim === drainClaim ? { ...it, status: null, drainClaim: void 0 } : it
          )
        );
        return;
      }
      setPendingStack(
        identity,
        (prev) => prev.filter(
          (it) => it.id !== head.id || it.drainClaim !== drainClaim
        )
      );
      void submitMessageNow(
        target2.panelId,
        target2.target,
        head.text,
        "queue"
      );
    }, animMs(420));
  }
  async function onLifecycleAction(identity, method) {
    if (consoleReadOnly) return;
    const command = method === "mobkit/retire" ? CONSOLE_COMMAND_NAMES2.retireIdentity : method === "mobkit/respawn" ? CONSOLE_COMMAND_NAMES2.respawnIdentity : CONSOLE_COMMAND_NAMES2.resetIdentity;
    try {
      await executeHeadlessCommand(command, identityWorkbenchTarget(identity, "chat"), {
        identity
      });
      setActionError("");
    } catch (lifecycleError) {
      setActionError(errorMessage(lifecycleError));
      return;
    }
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
      dock.openTarget(buildControlTarget2("roster"), "replace_focused");
    }
  }
  async function onGatingDecision(pendingId, decision) {
    if (consoleReadOnly) return;
    const gatingTarget = controlWorkbenchTarget("gating");
    await executeHeadlessCommand(CONSOLE_COMMAND_NAMES2.decideGating, gatingTarget, {
      pending_id: pendingId,
      approver_id: DEFAULT_APPROVER_ID,
      decision,
      reason: `console_${decision}`
    });
    const [p, a] = await Promise.all([
      executeHeadlessCommand(CONSOLE_COMMAND_NAMES2.listGatingPending, gatingTarget),
      executeHeadlessCommand(CONSOLE_COMMAND_NAMES2.listGatingAudit, gatingTarget, { limit: 50 })
    ]);
    const pending = p && typeof p === "object" ? p : {};
    const audit = a && typeof a === "object" ? a : {};
    setGatingData({
      pending: Array.isArray(pending.pending) ? pending.pending : [],
      audit: Array.isArray(audit.entries) ? audit.entries : []
    });
  }
  const canManageWorkGraph = experience?.workgraph?.can_manage === true && !consoleReadOnly;
  const runWorkGraphCommand = import_react34.default.useCallback(
    async (command, params, cardIdentity) => {
      if (consoleReadOnlyRef.current) return;
      const echoResultToCard = (result, failureMessage) => {
        if (!cardIdentity) return;
        appendFrame(
          cardIdentity,
          buildWorkGraphOperatorResultFrame({
            method: consoleCommandMethod(command),
            params,
            ...failureMessage !== void 0 ? { errorMessage: failureMessage } : { result },
            identity: cardIdentity
          })
        );
        forceRender();
      };
      try {
        const result = await executeHeadlessCommand(command, controlWorkbenchTarget("workgraph"), params);
        setActionError("");
        echoResultToCard(result);
      } catch (err) {
        const message = errorMessage(err);
        setActionError(message);
        echoResultToCard(void 0, message);
        if (cardIdentity && jsonRpcErrorCode(err) === WORKGRAPH_CONFLICT_CODE) {
          const refresh = workGraphConflictRefreshRequest(params);
          if (refresh) {
            try {
              const fresh = await executeHeadlessCommand(
                refresh.command,
                controlWorkbenchTarget("workgraph"),
                refresh.params
              );
              appendFrame(
                cardIdentity,
                buildWorkGraphOperatorResultFrame({
                  method: consoleCommandMethod(refresh.command),
                  params: refresh.params,
                  result: fresh,
                  identity: cardIdentity,
                  refresh: true
                })
              );
              forceRender();
            } catch {
            }
          }
        }
      }
      if (workGraphPanelDockedRef.current) {
        await refreshWorkGraphData().catch(() => {
        });
      }
    },
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [baseUrl, refreshWorkGraphData]
  );
  const runWorkGraphQuery = import_react34.default.useCallback(
    (command, params) => executeHeadlessCommand(command, controlWorkbenchTarget("workgraph"), params),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [baseUrl]
  );
  const makeWorkGraphOperatorHandlers = import_react34.default.useCallback(
    (cardIdentity) => {
      const dispatch = (resolveRevision, send) => {
        void (async () => {
          let expectedRevision;
          try {
            expectedRevision = await resolveRevision();
          } catch (err) {
            setActionError(errorMessage(err));
            return;
          }
          await send(expectedRevision);
        })();
      };
      const revisionOr = (revision, resolve) => revision !== void 0 ? () => Promise.resolve(revision) : resolve;
      return {
        onClaim: ({ itemId, revision }) => dispatch(
          revisionOr(revision, () => resolveWorkGraphItemRevision(runWorkGraphQuery, itemId)),
          (expectedRevision) => runWorkGraphCommand(CONSOLE_COMMAND_NAMES2.workgraphClaim, {
            id: itemId,
            expected_revision: expectedRevision,
            owner: {
              kind: "principal",
              id: workGraphClaimOwnerId(experience?.access?.subject, DEFAULT_APPROVER_ID)
            }
          }, cardIdentity)
        ),
        onClose: ({ itemId, revision }) => dispatch(
          revisionOr(revision, () => resolveWorkGraphItemRevision(runWorkGraphQuery, itemId)),
          (expectedRevision) => runWorkGraphCommand(CONSOLE_COMMAND_NAMES2.workgraphClose, {
            id: itemId,
            expected_revision: expectedRevision
          }, cardIdentity)
        ),
        onGoalConfirm: ({ bindingId, revision }) => dispatch(
          revisionOr(revision, () => resolveWorkGraphGoalItemRevision(runWorkGraphQuery, bindingId)),
          (expectedRevision) => runWorkGraphCommand(CONSOLE_COMMAND_NAMES2.workgraphGoalConfirm, {
            binding_id: bindingId,
            expected_revision: expectedRevision
          }, cardIdentity)
        ),
        onGoalRequestClose: ({ bindingId, revision }) => dispatch(
          revisionOr(revision, () => resolveWorkGraphGoalItemRevision(runWorkGraphQuery, bindingId)),
          (expectedRevision) => runWorkGraphCommand(CONSOLE_COMMAND_NAMES2.workgraphGoalRequestClose, {
            binding_id: bindingId,
            expected_revision: expectedRevision
          }, cardIdentity)
        ),
        onAttentionPause: ({ bindingId, revision }) => dispatch(
          revisionOr(revision, () => resolveWorkGraphBindingRevision(runWorkGraphQuery, bindingId)),
          (expectedRevision) => runWorkGraphCommand(CONSOLE_COMMAND_NAMES2.workgraphAttentionPause, {
            binding_id: bindingId,
            expected_revision: expectedRevision
          }, cardIdentity)
        ),
        onAttentionResume: ({ bindingId, revision }) => dispatch(
          revisionOr(revision, () => resolveWorkGraphBindingRevision(runWorkGraphQuery, bindingId)),
          (expectedRevision) => runWorkGraphCommand(CONSOLE_COMMAND_NAMES2.workgraphAttentionResume, {
            binding_id: bindingId,
            expected_revision: expectedRevision
          }, cardIdentity)
        ),
        onAttentionReassign: ({ bindingId, revision, identity }) => dispatch(
          revisionOr(revision, () => resolveWorkGraphBindingRevision(runWorkGraphQuery, bindingId)),
          (expectedRevision) => runWorkGraphCommand(CONSOLE_COMMAND_NAMES2.workgraphAttentionReassign, {
            binding_id: bindingId,
            expected_revision: expectedRevision,
            target: { kind: "identity", identity }
          }, cardIdentity)
        )
      };
    },
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [runWorkGraphCommand, runWorkGraphQuery, experience?.access?.subject]
  );
  const workGraphCardActions = import_react34.default.useCallback(
    (cardIdentity) => {
      if (!canManageWorkGraph) return void 0;
      const { onAttentionReassign: _panelOnly, ...cardHandlers } = makeWorkGraphOperatorHandlers(cardIdentity);
      return cardHandlers;
    },
    [canManageWorkGraph, makeWorkGraphOperatorHandlers]
  );
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
    return /* @__PURE__ */ (0, import_jsx_runtime42.jsxs)(
      "div",
      {
        "data-testid": "console-loading",
        "aria-live": "polite",
        "aria-busy": "true",
        style: {
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
          gap: "0.6rem",
          minHeight: "100vh"
        },
        children: [
          /* @__PURE__ */ (0, import_jsx_runtime42.jsxs)("span", { className: "msg__typing-dots", "aria-hidden": "true", children: [
            /* @__PURE__ */ (0, import_jsx_runtime42.jsx)("span", {}),
            /* @__PURE__ */ (0, import_jsx_runtime42.jsx)("span", {}),
            /* @__PURE__ */ (0, import_jsx_runtime42.jsx)("span", {})
          ] }),
          /* @__PURE__ */ (0, import_jsx_runtime42.jsx)("span", { children: "Loading console\u2026" })
        ]
      }
    );
  if (error) return /* @__PURE__ */ (0, import_jsx_runtime42.jsx)("div", { "data-testid": "console-error", children: error });
  const focusedMemberId = dock.focusedTarget?.kind === "agent-chat" ? dock.focusedTarget.memberId : selectedRosterMemberId;
  const sidebarVS = buildSidebarViewState2({
    agents,
    selectedMemberId: focusedMemberId,
    pinnedAgentIds
  });
  const activityVS = buildActivityRailViewState2({
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
    const panelKey = buildPanelConversationKey2(panel.id, target);
    const identity = target.identity || target.memberId;
    const agent = agents.find((c) => c.member_id === target.memberId) || null;
    const sortedFrames = framesVisibleInPanel(
      getSortedFrames(identity),
      panel.id
    );
    const conversationEntries = mapFramesToTimelineEntries2(
      agent,
      sortedFrames,
      {
        renderInteractionStartsAsUser: true,
        renderTextDeltas: true,
        blobBaseUrl: baseUrl
      }
    );
    const optimisticUser = optimisticUserMessageForPanel2(
      optimisticUserByPanelKeyRef.current,
      panelKey,
      identity
    );
    const optimisticEntry = optimisticUser ? optimisticUser.entry : null;
    const entries = sanitizeConversationEntries(
      appendOptimisticConversationEntry2(conversationEntries, optimisticEntry)
    );
    const conversation = buildConversationViewState2({
      memberId: target.memberId,
      agentLabel: target.title,
      agent,
      entries
    });
    const draft = draftByKey[panelKey] || "";
    const staged = stagedAttachmentsByIdentity[identity] ?? [];
    const identityLog = getOrCreateLog(identity);
    const isSending = sendingPanels.has(panelKey);
    const hasLocalPhase = Object.prototype.hasOwnProperty.call(
      phaseRef.current,
      panelKey
    );
    const honorLocalPhase = hasLocalPhase && (isSending || optimisticEntry !== null);
    const phase = resolvePanelResponsePhase2({
      frames: sortedFrames.filter((frame) => PANEL_ROUTABLE_EVENTS.has(frame.event)),
      localPhase: honorLocalPhase ? phaseRef.current[panelKey] ?? null : null,
      hasLocalPhase: honorLocalPhase,
      serverPhase: agent?.response_phase ?? null
    });
    const canRespawn = !consoleReadOnly && configuredActionVisibility.respawn && agent?.affordances?.can_respawn === true;
    const canRetire = !consoleReadOnly && configuredActionVisibility.retire && agent?.affordances?.can_retire === true;
    const stackItems = getPendingStack(identity);
    const agentBusy = isIdentityBusy(identity);
    const stackSlot = stackItems.length > 0 ? /* @__PURE__ */ (0, import_jsx_runtime42.jsx)(
      PendingStack3,
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
    return /* @__PURE__ */ (0, import_jsx_runtime42.jsx)(
      ChatPane,
      {
        agent,
        agentLabel: target.title || agent?.label || identity,
        identity,
        entries,
        phase,
        isLoadingHistory: Boolean(loadingHistory[identity]),
        draft,
        sending: isSending,
        readOnly: consoleReadOnly,
        accessEnforcing,
        staged,
        onDraftChange: (v) => setDraftByKey((c) => ({ ...c, [panelKey]: v })),
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
        hasOlderHistory: identityLog.hasServerLog === true && Boolean(identityLog.oldestTimelineCursor) && identityLog.olderHistoryExhausted !== true,
        loadingOlderHistory: identityLog.olderHistoryLoading === true,
        onLoadOlder: () => void loadOlderIdentityTimeline(identity),
        stackSlot,
        workGraphActions: workGraphCardActions(identity)
      }
    );
  }
  function renderInspectPanel(target) {
    const inspect = inspectByIdentity[target.identity];
    const agent = agents.find(
      (candidate) => candidate.identity === target.identity || candidate.member_id === target.identity
    );
    const canRespawn = !consoleReadOnly && configuredActionVisibility.respawn && agent?.affordances?.can_respawn === true;
    const canRetire = !consoleReadOnly && configuredActionVisibility.retire && agent?.affordances?.can_retire === true;
    const canReset = !consoleReadOnly && configuredActionVisibility.reset && experience?.runtime_capabilities?.can_retire_members === true;
    return /* @__PURE__ */ (0, import_jsx_runtime42.jsxs)(
      "div",
      {
        className: "console-panel",
        "data-testid": `inspect-panel:${target.identity}`,
        children: [
          /* @__PURE__ */ (0, import_jsx_runtime42.jsxs)("div", { className: "console-panel__header", children: [
            /* @__PURE__ */ (0, import_jsx_runtime42.jsx)("h3", { children: target.identity }),
            /* @__PURE__ */ (0, import_jsx_runtime42.jsxs)("div", { className: "console-panel__actions", children: [
              canRespawn ? /* @__PURE__ */ (0, import_jsx_runtime42.jsx)(
                "button",
                {
                  "data-testid": `inspect-action:${target.identity}:respawn`,
                  type: "button",
                  onClick: () => void onLifecycleAction(target.identity, "mobkit/respawn"),
                  children: configuredActionLabels.respawn
                }
              ) : null,
              canReset ? /* @__PURE__ */ (0, import_jsx_runtime42.jsx)(
                "button",
                {
                  "data-testid": `inspect-action:${target.identity}:reset`,
                  type: "button",
                  onClick: () => void onLifecycleAction(target.identity, "mobkit/reset"),
                  children: configuredActionLabels.reset
                }
              ) : null,
              canRetire ? /* @__PURE__ */ (0, import_jsx_runtime42.jsx)(
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
          !inspect ? /* @__PURE__ */ (0, import_jsx_runtime42.jsx)("p", { children: "Loading identity details\u2026" }) : /* @__PURE__ */ (0, import_jsx_runtime42.jsxs)("dl", { className: "console-panel__grid", children: [
            /* @__PURE__ */ (0, import_jsx_runtime42.jsx)("dt", { children: "State" }),
            /* @__PURE__ */ (0, import_jsx_runtime42.jsx)("dd", { children: inspect.state }),
            /* @__PURE__ */ (0, import_jsx_runtime42.jsx)("dt", { children: "Role" }),
            /* @__PURE__ */ (0, import_jsx_runtime42.jsx)("dd", { children: inspect.role || "n/a" }),
            /* @__PURE__ */ (0, import_jsx_runtime42.jsx)("dt", { children: "Addressability" }),
            /* @__PURE__ */ (0, import_jsx_runtime42.jsx)("dd", { children: inspect.addressability }),
            /* @__PURE__ */ (0, import_jsx_runtime42.jsx)("dt", { children: "Generation" }),
            /* @__PURE__ */ (0, import_jsx_runtime42.jsx)("dd", { children: inspect.continuity?.generation ?? "n/a" }),
            /* @__PURE__ */ (0, import_jsx_runtime42.jsx)("dt", { children: "Checkpoint" }),
            /* @__PURE__ */ (0, import_jsx_runtime42.jsx)("dd", { children: inspect.continuity?.checkpoint_version ?? "n/a" }),
            /* @__PURE__ */ (0, import_jsx_runtime42.jsx)("dt", { children: "Session" }),
            /* @__PURE__ */ (0, import_jsx_runtime42.jsx)("dd", { children: inspect.continuity?.session_id || "n/a" }),
            /* @__PURE__ */ (0, import_jsx_runtime42.jsx)("dt", { children: "Runtime" }),
            /* @__PURE__ */ (0, import_jsx_runtime42.jsx)("dd", { children: inspect.continuity?.agent_runtime_id || "n/a" }),
            /* @__PURE__ */ (0, import_jsx_runtime42.jsx)("dt", { children: "Lease Healthy" }),
            /* @__PURE__ */ (0, import_jsx_runtime42.jsx)("dd", { children: String(inspect.lease_healthy ?? inspect.lease?.healthy ?? false) }),
            /* @__PURE__ */ (0, import_jsx_runtime42.jsx)("dt", { children: "Peers" }),
            /* @__PURE__ */ (0, import_jsx_runtime42.jsx)("dd", { children: inspect.topology_peers?.join(", ") || "none" }),
            /* @__PURE__ */ (0, import_jsx_runtime42.jsx)("dt", { children: "Output Preview" }),
            /* @__PURE__ */ (0, import_jsx_runtime42.jsx)("dd", { children: inspect.output_preview || "n/a" })
          ] })
        ]
      }
    );
  }
  function renderHealthPanel(identities) {
    return /* @__PURE__ */ (0, import_jsx_runtime42.jsx)("div", { className: "console-panel", "data-testid": "health-panel", children: /* @__PURE__ */ (0, import_jsx_runtime42.jsx)("ul", { className: "console-panel__list", children: identities.map((r2) => /* @__PURE__ */ (0, import_jsx_runtime42.jsxs)("li", { "data-testid": `health-identity:${r2.identity}`, children: [
      /* @__PURE__ */ (0, import_jsx_runtime42.jsx)("strong", { children: r2.display_name || r2.identity }),
      " \xB7 ",
      r2.state,
      " \xB7",
      " ",
      r2.addressability
    ] }, r2.identity)) }) });
  }
  async function refreshInspectIdentity(identity) {
    const r2 = await inspectIdentityViaHeadless(identity);
    setInspectByIdentity((current) => ({
      ...current,
      [identity]: normalizeConsoleInspectResult(r2)
    }));
  }
  function handleShowRosterDetails(agent) {
    setSelectedRosterMemberId(agent.member_id);
    const target = buildInspectTarget2(agent);
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
    if (!target) return /* @__PURE__ */ (0, import_jsx_runtime42.jsx)("div", { className: "console-panel", children: "No panel target" });
    if (target.kind === "agent-chat") return renderChatPanel(panel);
    if (target.kind === "identity-inspect") {
      return renderInspectPanel(target);
    }
    if ((target.kind === "routing" || target.kind === "gating" || target.kind === "gates" || target.kind === "workgraph") && !hasMobControlSurface) {
      return /* @__PURE__ */ (0, import_jsx_runtime42.jsx)("div", { className: "console-panel", children: "This view requires a mob runtime control surface." });
    }
    if (target.kind === "routing") return /* @__PURE__ */ (0, import_jsx_runtime42.jsx)(RoutingPanel, { data: routingData });
    if (target.kind === "gating")
      return /* @__PURE__ */ (0, import_jsx_runtime42.jsx)(
        GatingInboxPanel,
        {
          pending: gatingData.pending,
          audit: gatingData.audit,
          onDecide: (pid, decision) => void onGatingDecision(pid, decision),
          readOnly: consoleReadOnly
        }
      );
    if (target.kind === "topology")
      return /* @__PURE__ */ (0, import_jsx_runtime42.jsx)(
        TopologyPanel2,
        {
          nodes: experience?.topology?.live_snapshot?.nodes || [],
          agents,
          activity: liveFrames
        }
      );
    if (target.kind === "health")
      return renderHealthPanel(
        experience?.health_overview?.live_snapshot?.identities || []
      );
    if (target.kind === "timeline")
      return /* @__PURE__ */ (0, import_jsx_runtime42.jsx)(TimelinePanel, { frames: activityRef.current });
    if (target.kind === "roster")
      return /* @__PURE__ */ (0, import_jsx_runtime42.jsx)(
        RosterPanel,
        {
          agents,
          selectedMemberId: selectedRosterMemberId,
          onSelect: (a) => setSelectedRosterMemberId(a.member_id),
          onChat: (a) => openAgentChat(a),
          onDetails: (a) => handleShowRosterDetails(a),
          onLifecycle: (identity, method) => void onLifecycleAction(identity, method),
          canResetLifecycle: !consoleReadOnly && hasMobControlSurface,
          actionLabels: configuredActionLabels,
          actionVisibility: {
            ...configuredActionVisibility,
            respawn: !consoleReadOnly && configuredActionVisibility.respawn,
            retire: !consoleReadOnly && configuredActionVisibility.retire,
            reset: !consoleReadOnly && configuredActionVisibility.reset
          }
        }
      );
    if (target.kind === "gates")
      return /* @__PURE__ */ (0, import_jsx_runtime42.jsx)(
        GatingInboxPanel,
        {
          pending: gatingData.pending,
          audit: gatingData.audit,
          onDecide: (pid, decision) => void onGatingDecision(pid, decision),
          readOnly: consoleReadOnly
        }
      );
    if (target.kind === "logs")
      return /* @__PURE__ */ (0, import_jsx_runtime42.jsx)(LogsPanel, { frames: activityRef.current });
    if (target.kind === "access")
      return /* @__PURE__ */ (0, import_jsx_runtime42.jsx)(
        AccessPanel,
        {
          status: accessData.status,
          config: accessData.config,
          error: accessData.error,
          readOnly: frontendReadOnly || experience?.console_policy?.read_only === true,
          agents: agents.map((agent) => ({
            identity: agent.identity || agent.member_id,
            label: agent.label
          })),
          onRefresh: () => void refreshAccessData(),
          onSetEnabled: (enabled) => void runAccessMutation(CONSOLE_COMMAND_NAMES2.enableAccess, { enabled }),
          onSaveAdmins: (admins) => {
            const config = {
              ...accessData.config || {},
              admins
            };
            void runAccessMutation(CONSOLE_COMMAND_NAMES2.setAccessConfig, { config });
          },
          onUpsertRule: (rule) => void runAccessMutation(CONSOLE_COMMAND_NAMES2.upsertAccessRule, { rule }),
          onDeleteRule: (id) => void runAccessMutation(CONSOLE_COMMAND_NAMES2.deleteAccessRule, { id }),
          onSaveGroup: (name, group) => void runAccessMutation(CONSOLE_COMMAND_NAMES2.setAccessGroup, { name, group }),
          onDeleteGroup: (name) => void runAccessMutation(CONSOLE_COMMAND_NAMES2.deleteAccessGroup, { name }),
          onPreview: async (subject, action, identity) => {
            try {
              return await executeHeadlessCommand(
                CONSOLE_COMMAND_NAMES2.previewAccess,
                controlWorkbenchTarget("access"),
                identity ? { subject, action, identity } : { subject, action }
              ) || null;
            } catch (err) {
              setAccessData((current) => ({ ...current, error: errorMessage(err) }));
              return null;
            }
          }
        }
      );
    if (target.kind === "memory")
      return /* @__PURE__ */ (0, import_jsx_runtime42.jsx)(
        MemoryPanel,
        {
          records: memoryData.records,
          realms: memoryData.realms,
          quarantineRecords: memoryData.quarantineRecords,
          pendingPromotions: memoryData.pendingPromotions,
          dreams: memoryData.dreams,
          detail: memoryData.detail,
          detailLoading: memoryData.detailLoading,
          canReviewQuarantine: experience?.memory?.can_review_quarantine === true,
          unavailable: memoryData.unavailable,
          error: memoryData.error,
          nextCursor: memoryData.nextCursor,
          recordsDenied: memoryData.recordsDenied,
          dreamsDenied: memoryData.dreamsDenied,
          operatorScopeDenied: memoryData.operatorScopeDenied,
          mobScopeDenied: memoryData.mobScopeDenied,
          overview: memoryData.overview,
          overviewDenied: memoryData.overviewDenied,
          proposals: memoryData.proposals,
          proposalsDenied: memoryData.proposalsDenied,
          injections: memoryData.injections,
          injectionsDenied: memoryData.injectionsDenied,
          harvests: memoryData.harvests,
          harvestsDenied: memoryData.harvestsDenied,
          dreamRuns: memoryData.dreamRuns,
          dreamRunsDenied: memoryData.dreamRunsDenied,
          auditVerdicts: memoryData.auditVerdicts,
          auditVerdictsDenied: memoryData.auditVerdictsDenied,
          liveFrames: activityRef.current,
          onRefresh: () => void refreshMemoryData(),
          onSelectRecord: (realm, memoryId) => void loadMemoryRecordDetail(realm, memoryId),
          onClearDetail: () => setMemoryData((current) => ({ ...current, detail: null, detailLoading: false })),
          onQueryRecords: queryMemoryRecords,
          onLoadEvidence: loadMemoryEvidence,
          onOpenGating: (
            // Only offered where the nav itself offers gating — on runtimes
            // without a mob control surface (or with gating hidden) the
            // target would land on a dead-end placeholder.
            visibleControls.includes("gating") ? () => dock.openTarget(buildControlTarget2("gating"), "replace_focused") : void 0
          )
        }
      );
    if (target.kind === "workgraph") {
      const workGraphPanelHandlers = makeWorkGraphOperatorHandlers();
      return /* @__PURE__ */ (0, import_jsx_runtime42.jsx)(
        WorkGraphPanel,
        {
          data: workGraphData,
          canManage: canManageWorkGraph,
          onRefresh: () => void refreshWorkGraphData(),
          onClaim: workGraphPanelHandlers.onClaim,
          onClose: workGraphPanelHandlers.onClose,
          onGoalConfirm: workGraphPanelHandlers.onGoalConfirm,
          onGoalRequestClose: workGraphPanelHandlers.onGoalRequestClose,
          onAttentionPause: workGraphPanelHandlers.onAttentionPause,
          onAttentionResume: workGraphPanelHandlers.onAttentionResume,
          onAttentionReassign: workGraphPanelHandlers.onAttentionReassign
        }
      );
    }
    return /* @__PURE__ */ (0, import_jsx_runtime42.jsx)("div", { className: "console-panel", children: "Unsupported panel" });
  }
  return /* @__PURE__ */ (0, import_jsx_runtime42.jsxs)(
    "div",
    {
      className: "cc-theme-scope mobkit-shell",
      "data-cc-theme": theme,
      "data-cc-variant": variant,
      "data-testid": "meerkat-console",
      children: [
        /* @__PURE__ */ (0, import_jsx_runtime42.jsx)(SpriteSheet, {}),
        actionError && /* @__PURE__ */ (0, import_jsx_runtime42.jsxs)("div", { className: "mobkit-action-error", "data-testid": "console-action-error", role: "alert", children: [
          /* @__PURE__ */ (0, import_jsx_runtime42.jsx)("span", { children: actionError }),
          /* @__PURE__ */ (0, import_jsx_runtime42.jsx)(
            "button",
            {
              "aria-label": "Dismiss error",
              "data-testid": "console-action-error-dismiss",
              onClick: () => setActionError(""),
              type: "button",
              children: "\xD7"
            }
          )
        ] }),
        /* @__PURE__ */ (0, import_jsx_runtime42.jsx)(
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
        /* @__PURE__ */ (0, import_jsx_runtime42.jsxs)(
          "div",
          {
            className: "shell",
            "data-console-workbench": "root",
            "data-sidebar-collapsed": sidebarCollapsed ? "true" : "false",
            "data-rail-collapsed": railCollapsed ? "true" : "false",
            children: [
              /* @__PURE__ */ (0, import_jsx_runtime42.jsx)(
                Sidebar,
                {
                  agents,
                  selectedMemberId: focusedMemberId,
                  recentActivity: activityRef.current,
                  collapsed: sidebarCollapsed,
                  visibleControls,
                  customButtons: experience?.console_config?.sidebar?.buttons,
                  grouping: experience?.console_config?.agent_list,
                  storageNamespace: sidebarStorageNamespace,
                  pinnedAgentIds,
                  onSelect: (a) => openAgentChat(a),
                  onTogglePinnedAgent: togglePinnedAgent,
                  onOpenControl: (kind) => {
                    dock.openTarget(buildControlTarget2(kind), "replace_focused");
                  }
                }
              ),
              /* @__PURE__ */ (0, import_jsx_runtime42.jsx)(
                "div",
                {
                  className: "pane-resizer",
                  "aria-hidden": "true",
                  "data-testid": "resize:sidebar",
                  onPointerDown: handleSidebarResize
                }
              ),
              /* @__PURE__ */ (0, import_jsx_runtime42.jsx)("div", { className: "main", children: /* @__PURE__ */ (0, import_jsx_runtime42.jsx)(
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
              railVisible ? /* @__PURE__ */ (0, import_jsx_runtime42.jsxs)(import_jsx_runtime42.Fragment, { children: [
                /* @__PURE__ */ (0, import_jsx_runtime42.jsx)(
                  "div",
                  {
                    className: "pane-resizer pane-resizer--activity",
                    "aria-hidden": "true",
                    "data-testid": "resize:activity",
                    onPointerDown: handleActivityResize
                  }
                ),
                /* @__PURE__ */ (0, import_jsx_runtime42.jsx)(
                  SignalsRail,
                  {
                    frames: activityRef.current,
                    collapsed: railCollapsed,
                    filterPresets: railConfig?.filter_presets,
                    activePresetId: activeActivityPresetId || railConfig?.active_preset_id,
                    emptyText: railConfig?.empty_text,
                    watchedIdentities,
                    onPresetChange: setActiveActivityPresetId,
                    onSelect: (
                      // "State here" pivot: a live memory signal opens the Memory
                      // panel, and lands on the record's Biography when the frame
                      // names one. Offered only when the server-projected
                      // experience grants memory.can_read — the affordance must
                      // never outrun the nav gate.
                      experience?.memory?.can_read === true ? (frame) => {
                        if (!frame.event.startsWith("memory.")) return;
                        dock.openTarget(buildControlTarget2("memory"), "replace_focused");
                        const pivot = memoryFramePivot(frame);
                        if (pivot) void loadMemoryRecordDetail(pivot.realm, pivot.recordId);
                      } : void 0
                    )
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
var import_jsx_runtime43 = require("react/jsx-runtime");
function createConsoleApp(target, options = {}) {
  if (!target) {
    throw new Error("target element is required");
  }
  const baseUrl = options.baseUrl || "";
  const root = (0, import_client.createRoot)(target);
  root.render(/* @__PURE__ */ (0, import_jsx_runtime43.jsx)(ConsoleApp, { baseUrl }));
  return {
    unmount() {
      root.unmount();
    }
  };
}
