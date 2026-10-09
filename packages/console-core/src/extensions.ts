import type { HostWorkbenchTarget } from "./targets";
import type { ConversationIdentity, ConversationMessageEntry } from "./conversation";
import type { ConsoleExperience, ConsoleFrame } from "./runtime-types";

/** Data only. Transcript content never supplies executable code or module URLs. */
export interface ConsoleChatWidget {
  type: string;
  version: number;
  data: unknown;
  fallback: string;
}

export interface ConsoleExtensionContext {
  baseUrl: string;
  readOnly: boolean;
  experience: ConsoleExperience | null;
  openPanel: (id: string, intent?: "replace_focused" | "new_tab" | "split_right" | "split_down") => void;
}

export interface ConsoleWidgetContext extends ConsoleExtensionContext {
  widget: ConsoleChatWidget;
  identity: ConversationIdentity;
  entryId: string;
}

/** A mount owns only its container. Abort work and release resources on dispose. */
export type ConsoleExtensionMount<T> = (
  container: HTMLElement, context: T, signal: AbortSignal,
) => { update?: (context: T) => void; dispose: () => void } | void;

export interface ConsolePanelDefinition {
  id: string;
  title: string;
  mount: ConsoleExtensionMount<ConsoleExtensionContext>;
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

export interface ConsoleExtensionPanelTarget extends HostWorkbenchTarget<{ panelId: string }> {
  kind: "extension/panel";
  payload: { panelId: string };
}

export function consoleExtensionPanelTarget(panel: ConsolePanelDefinition): ConsoleExtensionPanelTarget {
  return { id: `extension:${panel.id}`, kind: "extension/panel", title: panel.title, payloadVersion: 1, provenance: "host", payload: { panelId: panel.id } };
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
      if (!namespaced(panel.id) || panels.has(panel.id) || !panel.title?.trim() || typeof panel.mount !== "function") {
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
  return { type: raw.type, version: raw.version as number, data: raw.data, fallback: raw.fallback };
}

/** Project explicit tool-result metadata, never prose, into a replayable widget. */
export function consoleWidgetEntryFromFrame(frame: ConsoleFrame, identity: ConversationIdentity): ConversationMessageEntry | null {
  if (frame.event !== "tool_result_received" && frame.event !== "tool_execution_completed") return null;
  const envelope = record(frame.data);
  if (!envelope || envelope.is_error === true || envelope.isError === true || envelope.success === false) return null;
  let result = envelope.result;
  if (typeof result === "string") {
    try { result = JSON.parse(result); } catch { return null; }
  }
  const payload = record(result);
  if (!payload || payload.isError === true || payload.is_error === true) return null;
  const widget = parseConsoleChatWidget(record(payload.structuredContent)?.console_widget ?? payload.console_widget);
  if (!widget) return null;
  const rawCallId = envelope.tool_call_id ?? envelope.id ?? envelope.call_id;
  const callId = typeof rawCallId === "string" && rawCallId.trim() ? rawCallId.trim() : frame.id;
  const renderKey = `widget:${JSON.stringify([frame.runtimeKey, frame.identity, callId, widget.type])}`;
  return { kind: "message", variant: "plain", id: frame.id, renderKey, identity, interactionId: frame.interactionId,
    createdAt: Number.isFinite(frame.timestampMs) && Math.abs(frame.timestampMs!) <= 8.64e15 ? new Date(frame.timestampMs!).toISOString() : undefined,
    text: widget.fallback, widget };
}
