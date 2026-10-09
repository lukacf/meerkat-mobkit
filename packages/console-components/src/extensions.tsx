import React from "react";
import {
  type ConsoleExtension, type ConsoleExtensionContext, type ConsoleExtensionMount,
  type ConsoleWidgetContext,
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

export function ConsoleExtensionPanel({ id }: { id: string }) {
  const state = React.useContext(ExtensionContext);
  const panel = state?.extensions.flatMap(extension => extension.panels ?? []).find(panel => panel.id === id);
  const fallback = <div role="status">Custom panel unavailable: {panel?.title ?? id}</div>;
  return <div className="console-panel" data-extension-panel={id}>{panel && state
    ? <ConsoleExtensionSurface key={id} mount={panel.mount} context={state.context} fallback={fallback} />
    : fallback}</div>;
}

export function ConsoleChatWidgetView({ widget, identity, entryId }: Pick<ConsoleWidgetContext, "widget" | "identity" | "entryId">) {
  const state = React.useContext(ExtensionContext);
  const renderer = state?.extensions.flatMap(extension => extension.widgets ?? [])
    .find(renderer => renderer.type === widget.type && renderer.version === widget.version);
  const context = React.useMemo(() => state ? { ...state.context, widget, identity, entryId } : null,
    [state?.context, widget, identity, entryId]);
  const fallback = <div className="cc-widget-fallback">{widget.fallback}</div>;
  return <div data-console-widget={widget.type}>{renderer && context
    ? <ConsoleExtensionSurface key={`${widget.type}:${widget.version}`} mount={renderer.mount} context={context} fallback={fallback} />
    : fallback}</div>;
}
