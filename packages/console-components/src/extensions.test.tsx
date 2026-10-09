import { useConsoleExtensions } from "../../../console/src/lib/extensions";
import React from "react";
import { act, render, renderHook, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { consoleExtensionModuleUrl, consoleExtensionPanelTarget, migrateConsoleWorkbenchTarget, validateConsoleExtensions, type ConsoleExtension, type ConsoleExtensionContext } from "@console-core";
import { ConsoleExtensionSurface, ConsoleExtensionsProvider, ConsoleChatWidgetView } from "./extensions";

const context: ConsoleExtensionContext = { baseUrl: "http://localhost", readOnly: false, experience: null, openPanel: vi.fn() };
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
