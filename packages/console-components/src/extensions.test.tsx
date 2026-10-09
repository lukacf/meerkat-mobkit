import { bindConsoleExtensionService, createConsoleExtensionService, useConsoleExtensions } from "../../../console/src/lib/extensions";
import React from "react";
import { act, fireEvent, render, renderHook, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { consoleExtensionModuleUrl, consoleExtensionPanelTarget, migrateConsoleWorkbenchTarget, validateConsoleExtensions, type ConsoleExtension, type ConsoleExtensionContext } from "@console-core";
import { ConsoleExtensionSurface, ConsoleExtensionsProvider, ConsoleChatWidgetView, ConsoleExtensionPanel } from "./extensions";

const context: ConsoleExtensionContext = { baseUrl: "http://localhost", readOnly: false, experience: null, authority: { key: "scope:one" }, selection: { scopeKey: "scope:one", identity: "agent:b" }, conversation: null, request: vi.fn(), openPanel: vi.fn() };
const plugin: ConsoleExtension = { id: "demo", panels: [{ id: "demo/panel", title: "Demo", mount: () => {} }] };

describe("console extension lifecycle", () => {
  it("updates without remounting, then aborts and disposes on unmount", () => {
    const dispose = vi.fn(), update = vi.fn();
    let signal: AbortSignal;
    const mount = vi.fn((element, _value, lifetime) => {
      signal = lifetime; element.textContent = "Mounted"; return { update, dispose };
    });
    const view = render(<ConsoleExtensionSurface mount={mount} context={context} fallback="Unavailable" />);
    expect(screen.getByText("Mounted")).toBeVisible();
    const next = { ...context, readOnly: true };
    view.rerender(<ConsoleExtensionSurface mount={mount} context={next} fallback="Unavailable" />);
    expect(mount).toHaveBeenCalledTimes(1);
    expect(update).toHaveBeenCalledWith(next);
    view.unmount();
    expect(signal!.aborted).toBe(true);
    expect(dispose).toHaveBeenCalledTimes(1);
  });

  it("cleans up and renders fallback for mount and update exceptions", () => {
    const broken = (element: HTMLElement) => { element.textContent = "Partial"; throw Error("broken"); };
    const view = render(<ConsoleExtensionSurface mount={broken} context={context} fallback="Unavailable" />);
    expect(screen.getByText("Unavailable")).toBeVisible();
    expect(screen.queryByText("Partial")).toBeNull();
    const dispose = vi.fn(() => { throw Error("bad cleanup"); });
    const mount = (element: HTMLElement) => {
      element.textContent = "Good";
      return { update: () => { throw Error("bad update"); }, dispose };
    };
    view.rerender(<ConsoleExtensionSurface mount={mount} context={context} fallback="Unavailable" />);
    expect(screen.getByText("Good")).toBeVisible();
    view.rerender(<ConsoleExtensionSurface mount={mount} context={{ ...context, readOnly: true }} fallback="Unavailable" />);
    expect(screen.getByText("Unavailable")).toBeVisible();
    expect(dispose).toHaveBeenCalledTimes(1);
  });

  it("shows literal fallback text for absent and unsupported widget renderers", () => {
    const widget = { type: "demo/result", version: 2, fallback: "<script>literal</script>", data: {} };
    const identity = { id: "agent", label: "Agent", role: "assistant" as const };
    const mount = vi.fn();
    const view = render(<ConsoleExtensionsProvider value={{ context, extensions: [{ id: "demo", widgets: [{ type: widget.type, version: 1, mount }] }] }}>
      <ConsoleChatWidgetView widget={widget} identity={identity} entryId="entry" />
    </ConsoleExtensionsProvider>);
    expect(screen.getByText(widget.fallback)).toBeVisible();
    expect(view.container.querySelector("script")).toBeNull();
    expect(mount).not.toHaveBeenCalled();
  });
});

describe("console extension registration", () => {
  it("validates serializable panel selectors before persisting independent instances", () => {
    const panel = { ...plugin.panels![0], validateParams: (value: unknown) => !!value && typeof value === "object" && "recordId" in value };
    const a = consoleExtensionPanelTarget(panel, { instanceKey: "a", params: { recordId: "one" } });
    const b = consoleExtensionPanelTarget(panel, { instanceKey: "b", params: { recordId: "two" } });
    expect(a.id).not.toBe(b.id);
    expect(migrateConsoleWorkbenchTarget(JSON.parse(JSON.stringify(a)))).toEqual(a);
    expect(() => consoleExtensionPanelTarget(panel, { params: { wrong: true } })).toThrow(/parameters/);
    expect(() => consoleExtensionPanelTarget(plugin.panels![0], { params: { recordId: "one" } })).toThrow(/parameters/);
    expect(() => consoleExtensionPanelTarget(panel, { params: { recordId: Infinity } })).toThrow(/JSON/);
    const cyclic: Record<string, any> = {}; cyclic.self = cyclic;
    expect(() => consoleExtensionPanelTarget(panel, { params: cyclic })).toThrow(/JSON/);
  });

  it("rejects collisions and reserved stock names", () => {
    expect(() => validateConsoleExtensions([plugin, plugin])).toThrow(/unique/);
    expect(() => validateConsoleExtensions([plugin, { ...plugin, id: "other" }])).toThrow(/duplicate/);
    expect(() => validateConsoleExtensions([{ id: "bad", panels: [{ ...plugin.panels![0], id: "mobkit/workgraph" }] }])).toThrow();
    expect(() => validateConsoleExtensions([{ id: "bad", widgets: [{ type: "demo/widget", version: 0, mount: () => {} }] }])).toThrow();
  });

  it("preserves panel identity through the shared host-target migration", () => {
    const target = consoleExtensionPanelTarget(plugin.panels![0]);
    expect(migrateConsoleWorkbenchTarget(target)).toMatchObject(target);
  });

  it("accepts same-origin URLs and refuses executable or foreign URLs", () => {
    expect(consoleExtensionModuleUrl("./plugin.js", "https://example.test/runtime")).toBe("https://example.test/runtime/plugin.js");
    for (const url of ["javascript:alert(1)", "data:text/javascript,bad", "https://other.test/plugin.js", "//other.test/plugin.js", "https://user@example.test/plugin.js", "", "/plugin.js#x"]) {
      expect(() => consoleExtensionModuleUrl(url, "https://example.test")).toThrow();
    }
  });

  it("isolates module failures and validates all registrations before exposing them", async () => {
    const load = vi.fn(async (url: string) => {
      if (url.endsWith("bad.js")) throw Error("load failed");
      return { default: plugin };
    });
    const { result } = renderHook(() => useConsoleExtensions(undefined, ["/ok.js", "/bad.js", "/duplicate.js"], "http://localhost", load));
    await waitFor(() => expect(result.current.errors).toHaveLength(2));
    expect(result.current.extensions).toEqual([plugin]);
  });

  it("ignores stale module completion after changing runtime scope", async () => {
    let resolveOld: (value: { default: ConsoleExtension }) => void;
    const load = vi.fn((url: string) => url.includes("old.test")
      ? new Promise<{ default: ConsoleExtension }>(resolve => { resolveOld = resolve; })
      : Promise.resolve({ default: { id: "new" } }));
    const { result, rerender } = renderHook(({ base }) => useConsoleExtensions(undefined, ["/plugin.js"], base, load), { initialProps: { base: "http://old.test" } });
    rerender({ base: "http://new.test" });
    await waitFor(() => expect(result.current.extensions[0]?.id).toBe("new"));
    await act(async () => resolveOld!({ default: plugin }));
    expect(result.current.extensions[0]?.id).toBe("new");
  });
});

describe("extension context ownership", () => {
  const original = { scopeKey: "scope:one", identity: "agent:a" };
  it("keeps old widget navigation and service calls bound to its source conversation", () => {
    const request = vi.fn(), openPanel = vi.fn();
    const widget = { type: "demo/result", version: 1, data: {}, fallback: "Result" };
    const extensions: ConsoleExtension[] = [{ id: "demo", widgets: [{ type: widget.type, version: 1, mount(element, value, signal) {
      const button = document.createElement("button"); button.textContent = "Inspect original";
      button.onclick = () => {
        value.openPanel("demo/panel", { instanceKey: value.identity.id, intent: "split_right" });
        void value.request({ path: "/inspect" }, signal);
      };
      element.append(button);
    } }] }];
    render(<ConsoleExtensionsProvider value={{ extensions, context: { ...context, request, openPanel } }}>
      <ConsoleChatWidgetView widget={widget} identity={{ id: original.identity, label: "A", role: "assistant" }} entryId="old" />
    </ConsoleExtensionsProvider>);
    fireEvent.click(screen.getByText("Inspect original"));
    expect(openPanel).toHaveBeenCalledWith("demo/panel", { instanceKey: original.identity, intent: "split_right", conversation: original });
    expect(request).toHaveBeenCalledWith({ path: "/inspect" }, expect.any(AbortSignal), original);
  });

  it("pins panels, remounts follow-selection panels, and clears DOM on authority changes", () => {
    const signals: AbortSignal[] = [];
    const mount = vi.fn((element, value, signal) => {
      signals.push(signal); element.textContent = value.conversation.identity;
      return { update: vi.fn(), dispose: vi.fn() };
    });
    const panel = { ...plugin.panels![0], mount };
    const extensions = [{ id: "demo", panels: [panel] }];
    const target = consoleExtensionPanelTarget(panel, { conversation: original });
    const show = (ctx: ConsoleExtensionContext, followSelection = false) => <ConsoleExtensionsProvider value={{ extensions, context: ctx }}>
      <ConsoleExtensionPanel target={{ ...target, payload: { ...target.payload, followSelection } }} focused />
    </ConsoleExtensionsProvider>;
    const view = render(show(context));
    expect(screen.getByText("agent:a")).toBeVisible();
    view.rerender(show({ ...context, selection: { ...original, identity: "agent:c" } }));
    expect(mount).toHaveBeenCalledTimes(1);
    view.rerender(show(context, true));
    expect(signals[0].aborted).toBe(true);
    expect(screen.getByText("agent:b")).toBeVisible();
    view.rerender(show({ ...context, authority: { key: "scope:two" } }, true));
    expect(signals[1].aborted).toBe(true);
    expect(screen.queryByText("agent:b")).toBeNull();
    expect(screen.getByRole("status")).toHaveTextContent("unavailable");
  });
});

describe("extension host services", () => {
  const scope = { authority: { key: "scope:one" }, conversation: { scopeKey: "scope:one", identity: "agent:a" }, readOnly: false };
  it("blocks writes in read-only mode even with an injected service", async () => {
    const injected = vi.fn();
    const service = bindConsoleExtensionService(injected, new AbortController().signal);
    await expect(service({ path: "/action", method: "POST" }, { ...scope, readOnly: true }, new AbortController().signal)).rejects.toThrow(/view only/);
    expect(injected).not.toHaveBeenCalled();
  });

  it.each(["surface", "authority"])("aborts %s requests and discards late results without replay", async lifetime => {
    const surface = new AbortController(), authority = new AbortController();
    let resolve: (value: unknown) => void;
    const injected = vi.fn(() => new Promise(done => { resolve = done; }));
    const request = bindConsoleExtensionService(injected, authority.signal)({ path: "/data" }, scope, surface.signal);
    const rejected = expect(request).rejects.toMatchObject({ name: "AbortError" });
    (lifetime === "surface" ? surface : authority).abort();
    expect(injected.mock.calls[0][2].aborted).toBe(true);
    resolve!("private result");
    await rejected;
    expect(injected).toHaveBeenCalledTimes(1);
    expect(injected.mock.calls[0][1]).toEqual(scope);
  });

  it("uses same-origin cookies, refuses redirects, and does not retry HTTP failures", async () => {
    const fetch = vi.fn(async () => ({ ok: false, status: 403 }));
    vi.stubGlobal("fetch", fetch);
    try {
      const service = createConsoleExtensionService("http://localhost");
      await expect(service({ path: "https://other.test/data" }, scope, new AbortController().signal)).rejects.toThrow(/same-origin/);
      expect(fetch).not.toHaveBeenCalled();
      await expect(service({ path: "/data" }, scope, new AbortController().signal)).rejects.toThrow(/403/);
      expect(fetch).toHaveBeenCalledTimes(1);
      expect(fetch.mock.calls[0][1]).toMatchObject({ credentials: "same-origin", redirect: "error" });
    } finally { vi.unstubAllGlobals(); }
  });
});
