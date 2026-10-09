import React from "react";
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { consoleWidgetEntryFromFrame, conversationEntryText, type ConsoleExtension, type ConsoleFrame } from "@console-core";
import { ConversationMessageView } from "../../../packages/console-components/src/conversation/conversation-message-view";
import { ConsoleExtensionsProvider } from "@console-components";
import { mapFramesToTimelineEntries as shared } from "../../../packages/console-core/src/adapters";
import { mapFramesToTimelineEntries as stock } from "./adapters";
import { ConsoleApp } from "../ConsoleApp";
import type { MobKitConsoleTransport } from "./headless";
import { CONSOLE_RPC_METHODS } from "./contract";

const identity = { id: "identity:demo", label: "Demo", role: "assistant" as const };
const widget = { type: "demo/result", version: 1, data: { count: 7 }, fallback: "Seven results" };
const completion: ConsoleFrame = { id: "done", event: "tool_result_received", identity: identity.id,
  interactionId: "turn-1", timestampMs: 1234, data: { id: "call-1", name: "lookup", result: { console_widget: widget } } };

describe("widget projection", () => {
  it("accepts structured and JSON tool results but never message prose or failures", () => {
    expect(consoleWidgetEntryFromFrame(completion, identity)?.widget).toEqual(widget);
    expect(consoleWidgetEntryFromFrame({ ...completion, data: { result: JSON.stringify({ structuredContent: { console_widget: widget } }) } }, identity)?.widget).toEqual(widget);
    for (const frame of [
      { ...completion, event: "assistant_message" },
      { ...completion, data: { result: { console_widget: { ...widget, version: 0 } } } },
      { ...completion, data: { result: { console_widget: { ...widget, fallback: "" } } } },
      { ...completion, data: { is_error: true, result: { console_widget: widget } } },
      { ...completion, data: { result: { isError: true, console_widget: widget } } },
    ]) expect(consoleWidgetEntryFromFrame(frame, identity)).toBeNull();
  });

  it.each([["stock", stock], ["shared", shared]] as const)("%s projection preserves order and deduplicates tool completion twins", (_name, project) => {
    const entries = project(null, [
      { id: "before", event: "text_complete", identity: identity.id, timestampMs: 1000, data: { content: "Before" } },
      completion, { ...completion, id: "twin", event: "tool_execution_completed" },
      { id: "after", event: "text_complete", identity: identity.id, timestampMs: 2000, data: { content: "After" } },
    ]);
    const widgets = entries.filter(entry => entry.kind === "message" && entry.widget);
    expect(widgets).toHaveLength(1);
    expect(conversationEntryText(widgets[0])).toBe(widget.fallback);
    expect(widgets[0].id).toBe(completion.id);
    expect(widgets[0].renderKey).toContain("call-1");
    expect(entries.map(conversationEntryText)).toEqual(["Before", widget.fallback, "After"]);
  });

  it.each([["stock", stock], ["shared", shared]] as const)("%s keeps equal widgets from distinct calls", (_name, project) => {
    const entries = project(null, [completion, {
      ...completion, id: "second", data: { id: "call-2", result: { console_widget: widget } },
    }]);
    expect(entries.filter(entry => entry.kind === "message" && entry.widget)).toHaveLength(2);
  });

  it("renders the same registered widget in reusable conversations", () => {
    const entry = consoleWidgetEntryFromFrame(completion, identity)!;
    render(<ConsoleExtensionsProvider value={{ context: { baseUrl: "", readOnly: true, experience: null, openPanel: () => {} },
      extensions: [{ id: "demo", widgets: [{ type: widget.type, version: 1, mount(element, context) { element.textContent = `Count ${(context.widget.data as { count: number }).count}`; } }] }] }}>
      <ConversationMessageView entry={entry} />
    </ConsoleExtensionsProvider>);
    expect(screen.getByText("Count 7")).toBeVisible();
  });
});

describe("stock console extensions", () => {
  beforeEach(() => window.localStorage.clear());
  it("opens, splits, restores custom panels and renders live widgets in chat", async () => {
    let live: (frame: ConsoleFrame) => void = () => {};
    const experience = { contract_version: "test", runtime_id: "extension-test", console_config: { layout: { initial_agent: identity.id } }, console_policy: {},
      agent_sidebar: { live_snapshot: { agents: [{ identity: identity.id, member_id: identity.id, agent_id: identity.id,
        label: "Demo", kind: "member", role: "worker", state: "idle", addressable: true, affordances: {} }] } },
      activity_feed: { filter_presets: [], active_preset_id: "all" } };
    const transport: MobKitConsoleTransport = {
      loadExperience: async () => experience as never, loadModules: async () => ({ modules: [] }) as never,
      capabilities: async () => ({ version: "test", methods: Object.values(CONSOLE_RPC_METHODS) }) as never,
      queryTimeline: async () => ({ frames: [], exhausted: true, available: true }),
      subscribeTimeline: (_input, onFrame) => { live = onFrame; return () => {}; },
      send: async () => ({}) as never, executeCommand: async () => ({ accepted: true, result: {} }) as never,
    };
    const dispose = vi.fn();
    const mount = vi.fn((element: HTMLElement) => { element.textContent = "Custom overview"; return { dispose }; });
    const extensions: ConsoleExtension[] = [{ id: "demo", panels: [{ id: "demo/overview", title: "Overview", mount }],
      widgets: [{ type: widget.type, version: 1, mount(element, context) {
        const button = document.createElement("button"); button.textContent = "Open results";
        button.onclick = () => context.openPanel("demo/overview", "split_right"); element.append(button);
      } }] }];
    const view = render(<ConsoleApp baseUrl="" transport={transport} extensions={extensions} />);
    await waitFor(() => expect(screen.getByTestId("nav-extension:demo/overview")).toBeVisible());
    await act(async () => { await new Promise(resolve => setTimeout(resolve, 100)); live(completion); });
    await waitFor(() => expect(screen.getByRole("button", { name: "Open results" })).toBeVisible());
    fireEvent.click(screen.getByRole("button", { name: "Open results" }));
    await waitFor(() => expect(screen.getByText("Custom overview")).toBeVisible());
    expect(view.container.querySelectorAll(".pane")).toHaveLength(2);
    expect(mount).toHaveBeenCalledTimes(1);
    await act(async () => { await new Promise(resolve => setTimeout(resolve, 100)); });
    view.unmount();
    expect(dispose).toHaveBeenCalledTimes(1);
    const restored = render(<ConsoleApp baseUrl="" transport={transport} extensions={extensions} />);
    await waitFor(() => expect(screen.getByText("Custom overview")).toBeVisible());
    expect(restored.container.querySelectorAll(".pane")).toHaveLength(2);
  });
});
