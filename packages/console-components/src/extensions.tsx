import React from "react";
import {
  type ConsoleExtension, type ConsoleExtensionContext, type ConsoleExtensionMount,
  consoleExtensionPanelTarget, type ConsoleExtensionPanelTarget, type ConsoleConversationTarget, type ConsoleWidgetContext,
} from "@console-core";

type ExtensionState = { extensions: readonly ConsoleExtension[]; context: ConsoleExtensionContext };
const ExtensionContext = React.createContext<ExtensionState | null>(null);
export const ConsoleExtensionsProvider = ExtensionContext.Provider;

/** Contain lifecycle failures to one extension surface, including update/dispose. */
export function ConsoleExtensionSurface<T>({ mount, context, fallback }: {
  mount: ConsoleExtensionMount<T>; context: T; fallback: React.ReactNode;
}) {
  const container = React.useRef<HTMLDivElement>(null);
  const instance = React.useRef<ReturnType<ConsoleExtensionMount<T>>>(undefined);
  const abort = React.useRef<AbortController | null>(null);
  const [failed, setFailed] = React.useState(false);
  const latest = React.useRef(context);
  latest.current = context;
  const cleanup = React.useCallback(() => {
    abort.current?.abort();
    abort.current = null;
    const current = instance.current;
    instance.current = undefined;
    try { if (current) current.dispose(); } catch { /* Isolate plugin disposal failures. */ }
    container.current?.replaceChildren();
  }, []);
  React.useLayoutEffect(() => cleanup, [cleanup]);
  const start = React.useCallback((value: T) => {
    abort.current = new AbortController();
    instance.current = mount(container.current!, value, abort.current.signal);
  }, [mount]);
  React.useEffect(() => {
    setFailed(false);
    try { start(latest.current); } catch { cleanup(); setFailed(true); }
    return cleanup;
  }, [start, cleanup]);
  const previous = React.useRef(context);
  React.useEffect(() => {
    if (previous.current === context) return;
    previous.current = context;
    if (failed) return;
    try {
      if (instance.current && instance.current.update) instance.current.update(context);
      else { cleanup(); start(context); }
    } catch { cleanup(); setFailed(true); }
  }, [context, failed, cleanup, start]);
  return <><div ref={container} hidden={failed} className="console-extension-surface" />{failed ? fallback : null}</>;
}

function targetContext(base: ConsoleExtensionContext, conversation: ConsoleConversationTarget | null): ConsoleExtensionContext {
  return { ...base, conversation,
    request: (request, signal) => base.request(request, signal, conversation),
    openPanel: (id, options) => {
      const input = typeof options === "string" ? { intent: options } : options ?? {};
      base.openPanel(id, { ...input, conversation: input.conversation === undefined ? conversation : input.conversation });
    },
  };
}

export function ConsoleExtensionPanel({ target, focused }: { target: ConsoleExtensionPanelTarget; focused: boolean }) {
  const state = React.useContext(ExtensionContext);
  const payload = target.payload;
  const panel = state?.extensions.flatMap(extension => extension.panels ?? []).find(panel => panel.id === payload?.panelId);
  const context = React.useMemo(() => {
    if (!panel || !state || target.payloadVersion !== 1 || typeof payload.instanceKey !== "string"
      || typeof payload.followSelection !== "boolean" || !("params" in payload) || !("conversation" in payload)) return null;
    try { consoleExtensionPanelTarget(panel, payload); } catch { return null; }
    if (payload.conversation && payload.conversation.scopeKey !== state.context.authority.key) return null;
    const conversation = payload.followSelection ? state.context.selection : payload.conversation;
    return { ...targetContext(state.context, conversation), panel: { instanceKey: payload.instanceKey, params: payload.params, focused } };
  }, [panel, state?.context, payload, target.payloadVersion, focused]);
  const fallback = <div role="status">Custom panel unavailable: {panel?.title ?? payload?.panelId ?? target.id}</div>;
  // Replacing a target or following another conversation destroys private DOM
  // and aborts its work before mounting the replacement selection.
  const key = JSON.stringify([target.id, context?.authority.key, context?.conversation, context?.panel?.params]);
  return <div className="console-panel" data-extension-panel={payload?.panelId}>{panel && context
    ? <ConsoleExtensionSurface key={key} mount={panel.mount} context={context} fallback={fallback} />
    : fallback}</div>;
}

export function ConsoleChatWidgetView({ widget, identity, entryId, toolStatus = "unknown" }: Pick<ConsoleWidgetContext, "widget" | "identity" | "entryId"> & { toolStatus?: ConsoleWidgetContext["toolStatus"] }) {
  const state = React.useContext(ExtensionContext);
  const renderer = state?.extensions.flatMap(extension => extension.widgets ?? [])
    .find(renderer => renderer.type === widget.type && renderer.version === widget.version);
  const context = React.useMemo(() => state ? {
    ...targetContext(state.context, { scopeKey: state.context.authority.key, identity: identity.id }), widget, identity, entryId, toolStatus,
  } : null, [state?.context, widget, identity, entryId, toolStatus]);
  const fallback = <div className="cc-widget-fallback">{widget.fallback}</div>;
  return <div data-console-widget={widget.type}>{renderer && context
    ? <ConsoleExtensionSurface key={JSON.stringify([widget.type, widget.version, context.authority.key, context.conversation])} mount={renderer.mount} context={context} fallback={fallback} />
    : fallback}</div>;
}
