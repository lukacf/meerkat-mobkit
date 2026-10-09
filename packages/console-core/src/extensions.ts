import { toolCompletionFromFrame, type ToolCompletionOutcome } from "./tool-completion";
import type { HostWorkbenchTarget } from "./targets";
import type { ConversationIdentity, ConversationMessageEntry } from "./conversation";
import type { ConsoleExperience, ConsoleFrame } from "./runtime-types";

/** Data only. Transcript content never supplies executable code or module URLs. */
export interface ConsoleChatWidget {
  type: string;
  version: number;
  data: ConsoleJsonValue;
  fallback: string;
}

export type ConsoleJsonValue = null | boolean | number | string | ConsoleJsonValue[] | { [key: string]: ConsoleJsonValue };
export interface ConsoleConversationTarget { scopeKey: string; identity: string }
export interface ConsolePanelOpenOptions {
  instanceKey?: string;
  params?: ConsoleJsonValue;
  conversation?: ConsoleConversationTarget | null;
  followSelection?: boolean;
  intent?: "replace_focused" | "new_tab" | "split_right" | "split_down";
}
export interface ConsoleExtensionRequest {
  path: string;
  method?: "GET" | "POST" | "PUT" | "PATCH" | "DELETE";
  body?: ConsoleJsonValue;
}
export interface ConsoleExtensionScope {
  key: string;
  runtimeId?: string;
}
export type ConsoleExtensionService = (
  request: ConsoleExtensionRequest,
  scope: { authority: ConsoleExtensionScope; conversation: ConsoleConversationTarget | null; readOnly: boolean },
  signal: AbortSignal,
) => Promise<unknown>;

export interface ConsoleExtensionContext {
  baseUrl: string;
  readOnly: boolean;
  experience: ConsoleExperience | null;
  authority: ConsoleExtensionScope;
  selection: ConsoleConversationTarget | null;
  conversation: ConsoleConversationTarget | null;
  /** Supplied for panel mounts, never a runtime authorization claim. */
  panel?: { instanceKey: string; params: ConsoleJsonValue; focused: boolean };
  request: (request: ConsoleExtensionRequest, signal: AbortSignal, conversation?: ConsoleConversationTarget | null) => Promise<unknown>;
  openPanel: (id: string, options?: ConsolePanelOpenOptions | ConsolePanelOpenOptions["intent"]) => void;
}

export interface ConsoleWidgetContext extends ConsoleExtensionContext {
  widget: ConsoleChatWidget;
  identity: ConversationIdentity;
  entryId: string;
  toolStatus: ToolCompletionOutcome;
}

/** A mount owns only its container. Abort work and release resources on dispose. */
export type ConsoleExtensionMount<T> = (
  container: HTMLElement, context: T, signal: AbortSignal,
) => { update?: (context: T) => void; dispose: () => void } | void;

export interface ConsolePanelDefinition {
  id: string;
  title: string;
  mount: ConsoleExtensionMount<ConsoleExtensionContext>;
  /** Required when opening a panel with non-null parameters. Must be pure. */
  validateParams?: (params: ConsoleJsonValue) => boolean;
}

export interface ConsoleWidgetDefinition {
  type: string;
  version: number;
  mount: ConsoleExtensionMount<ConsoleWidgetContext>;
}

export interface ConsoleExtension {
  id: string;
  panels?: readonly ConsolePanelDefinition[];
  widgets?: readonly ConsoleWidgetDefinition[];
}

export interface ConsoleExtensionPanelTarget extends HostWorkbenchTarget<ConsoleExtensionPanelPayload> {
  kind: "extension/panel";
  payload: ConsoleExtensionPanelPayload;
}

export interface ConsoleExtensionPanelPayload {
  panelId: string;
  instanceKey: string;
  params: ConsoleJsonValue;
  conversation: ConsoleConversationTarget | null;
  followSelection: boolean;
}

export function consoleExtensionPanelTarget(panel: ConsolePanelDefinition, options: ConsolePanelOpenOptions = {}): ConsoleExtensionPanelTarget {
  const instanceKey = options.instanceKey ?? "default";
  if (typeof instanceKey !== "string" || !instanceKey.trim() || instanceKey.length > 512) throw new Error("Invalid panel instance key");
  // Round-trip only plain JSON selectors. Functions, cycles, nonfinite numbers and secrets do not belong in layout preferences.
  const params = options.params ?? null;
  assertConsoleJson(params);
  if (params !== null && (!panel.validateParams || !panel.validateParams(params))) throw new Error(`Invalid parameters for ${panel.id}`);
  const conversation = options.conversation ?? null;
  if (conversation && (typeof conversation.identity !== "string" || !conversation.identity.trim() || typeof conversation.scopeKey !== "string" || !conversation.scopeKey.trim())) throw new Error("Invalid conversation target");
  return { id: `extension:${panel.id}:${instanceKey}`, kind: "extension/panel", title: panel.title, payloadVersion: 1, provenance: "host",
    payload: { panelId: panel.id, instanceKey, params: JSON.parse(JSON.stringify(params)),
      conversation: conversation ? { ...conversation } : null, followSelection: options.followSelection === true } };
}

function assertConsoleJson(value: unknown, ancestors = new Set<unknown>()): asserts value is ConsoleJsonValue {
  if (value === null || typeof value === "string" || typeof value === "boolean" || (typeof value === "number" && Number.isFinite(value))) return;
  if (typeof value !== "object" || ancestors.has(value) || (!Array.isArray(value) && Object.getPrototypeOf(value) !== Object.prototype)) {
    throw new Error("Panel parameters must be plain JSON");
  }
  ancestors.add(value);
  for (const item of Object.values(value)) assertConsoleJson(item, ancestors);
  ancestors.delete(value);
}

const namespaced = (value: unknown): value is string => typeof value === "string"
  && /^[a-zA-Z0-9][a-zA-Z0-9._-]*\/[a-zA-Z0-9][a-zA-Z0-9._-]*$/.test(value)
  && !value.startsWith("mobkit/");
const record = (value: unknown): Record<string, unknown> | null =>
  value && typeof value === "object" && !Array.isArray(value) ? value as Record<string, unknown> : null;

/** Validate a complete set before publishing it; duplicate registrations are errors. */
export function validateConsoleExtensions(extensions: readonly ConsoleExtension[]): void {
  const ids = new Set<string>(), panels = new Set<string>(), widgets = new Set<string>();
  for (const extension of extensions) {
    if (!extension || typeof extension.id !== "string" || !extension.id.trim() || ids.has(extension.id)) {
      throw new Error("Console extension IDs must be nonempty and unique");
    }
    ids.add(extension.id);
    for (const panel of extension.panels ?? []) {
      if (!namespaced(panel.id) || panels.has(panel.id) || !panel.title?.trim() || typeof panel.mount !== "function"
        || (panel.validateParams !== undefined && typeof panel.validateParams !== "function")) {
        throw new Error(`Invalid or duplicate console panel: ${panel.id}`);
      }
      panels.add(panel.id);
    }
    for (const widget of extension.widgets ?? []) {
      const key = JSON.stringify([widget.type, widget.version]);
      if (!namespaced(widget.type) || !Number.isSafeInteger(widget.version) || widget.version < 1
        || widgets.has(key) || typeof widget.mount !== "function") {
        throw new Error(`Invalid or duplicate console widget: ${widget.type}`);
      }
      widgets.add(key);
    }
  }
}

/** Only runtime-configured modules on the console server's origin may execute. */
export function consoleExtensionModuleUrl(path: string, baseUrl: string): string {
  const base = new URL(baseUrl || "/", globalThis.location?.href ?? "http://localhost/");
  const url = new URL(path, `${base.href.replace(/\/$/, "")}/`);
  if (!path.trim() || !["http:", "https:"].includes(url.protocol) || url.origin !== base.origin
    || url.username || url.password || url.hash) throw new Error("Console extension modules must use same-origin HTTP URLs");
  return url.href;
}

export function parseConsoleChatWidget(value: unknown): ConsoleChatWidget | null {
  const raw = record(value);
  if (!raw || !namespaced(raw.type) || !Number.isSafeInteger(raw.version) || (raw.version as number) < 1
    || typeof raw.fallback !== "string" || !raw.fallback.trim()) return null;
  try { assertConsoleJson(raw.data); } catch { return null; }
  return { type: raw.type, version: raw.version as number, data: raw.data, fallback: raw.fallback };
}

/** Project explicit tool-result metadata, never prose, into a replayable widget. */
export function consoleWidgetEntryFromFrame(frame: ConsoleFrame, identity: ConversationIdentity): ConversationMessageEntry | null {
  if (frame.event !== "tool_result_received" && frame.event !== "tool_execution_completed") return null;
  const envelope = record(frame.data);
  if (!envelope) return null;
  // Canonical structured blocks preserve the metadata as JSON through history.
  // Legacy text carriers must contain an exact envelope, never a prose substring.
  const candidates: unknown[] = [];
  for (const block of Array.isArray(envelope.content) ? envelope.content : []) {
    const item = record(block);
    if (item?.type === "structured") candidates.push(item.data);
    else if (item?.type === "text") candidates.push(item.text);
  }
  candidates.push(envelope.result);
  let widget: ConsoleChatWidget | null = null;
  for (let candidate of candidates) {
    if (typeof candidate === "string") {
      try { candidate = JSON.parse(candidate); } catch { continue; }
    }
    const payload = record(candidate);
    if (!payload) continue;
    widget = parseConsoleChatWidget(record(payload.structuredContent)?.console_widget ?? payload.console_widget);
    if (widget) break;
  }
  if (!widget) return null;
  const rawCallId = envelope.tool_call_id ?? envelope.id ?? envelope.call_id;
  const callId = typeof rawCallId === "string" && rawCallId.trim() ? rawCallId.trim() : frame.id;
  const toolStatus = toolCompletionFromFrame(frame, callId).outcome;
  const sourceIdentity = frame.identity?.trim() ? { ...identity, id: frame.identity } : identity;
  const renderKey = `widget:${JSON.stringify([frame.runtimeKey, frame.identity, callId, widget.type])}`;
  return { kind: "message", variant: "plain", id: frame.id, renderKey, identity: sourceIdentity, interactionId: frame.interactionId,
    createdAt: Number.isFinite(frame.timestampMs) && Math.abs(frame.timestampMs!) <= 8.64e15 ? new Date(frame.timestampMs!).toISOString() : undefined,
    text: widget.fallback, widget, widgetToolStatus: toolStatus };
}
