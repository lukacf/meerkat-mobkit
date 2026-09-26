import React from "react";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ConsoleApp } from "../ConsoleApp";
import type { MobKitConsoleTransport } from "./headless";
import { createConsoleSendAttempt, beginConsoleSendAttempt } from "../../../packages/console-core/src/send-attempt";
import { consoleSendStorageKey, saveConsoleSendAttempts } from "./send-attempt-storage";
import { createConsoleContextRecord } from "../../../packages/console-core/src/context-record";

const identity = "identity:queue-agent";
function seed(twoPanes = false) {
  const target = { id: `chat:${identity}`, kind: "agent-chat", title: "Queue agent", identity, memberId: identity };
  window.localStorage.setItem("mobkit-console-dock-state:queue-test", JSON.stringify({
    tabs: [{ id: "tab-1", presetId: "single", layout: twoPanes ? { kind: "split", id: "split-1", direction: "horizontal", ratio: 0.5, first: { kind: "panel", panelId: "panel-1" }, second: { kind: "panel", panelId: "panel-2" } } : { kind: "panel", panelId: "panel-1" } }],
    panels: [{ id: "panel-1", mode: "console", target }, ...(twoPanes ? [{ id: "panel-2", mode: "console", target }] : [])],
    activeTabId: "tab-1", focusedPanelId: "panel-1",
  }));
}
function transport(send: MobKitConsoleTransport["send"]): MobKitConsoleTransport {
  return {
    loadExperience: async () => ({ runtime_id: "queue-test", console_config: {}, agent_sidebar: { live_snapshot: { agents: [{ identity, member_id: identity, agent_id: identity, label: "Queue agent", kind: "member", state: "running", addressable: true, affordances: { can_send_message: true }, model_capabilities: { image_input: false } }] } }, activity_feed: { filter_presets: [], active_preset_id: "all" } }) as never,
    loadModules: async () => ({ modules: [] }) as never,
    capabilities: async () => ({ version: "test", methods: ["mobkit/console/send", "mobkit/console/timeline"] }) as never,
    queryTimeline: async () => ({ frames: [], available: true }),
    subscribeTimeline: () => () => {}, send,
    executeCommand: async () => ({ result: {} }) as never,
    upload: async () => ({ blob_id: "blob" }) as never, blobUrl: (id) => `/blobs/${id}`,
  };
}
async function compose(text: string) {
  const textarea = await screen.findByTestId(`chat-composer:${identity}`) as HTMLTextAreaElement;
  await act(async () => {
    fireEvent.change(textarea, { target: { value: text } });
    fireEvent.keyDown(textarea, { key: "Enter" });
    await Promise.resolve();
  });
  return textarea;
}
beforeEach(() => {
  vi.stubGlobal("localStorage", window.localStorage);
  window.localStorage.clear(); seed();
  window.sessionStorage.clear();
  // One browser lock queue provides the same mutual exclusion boundary as Web Locks.
  let pending = Promise.resolve();
  Object.defineProperty(navigator, "locks", { configurable: true, value: { request: (_key: string, callback: () => unknown) => {
    const next = pending.then(callback); pending = next.then(() => {}, () => {}); return next;
  } } });
});
afterEach(async () => {
  await act(async () => { await new Promise((resolve) => window.setTimeout(resolve, 20)); });
  cleanup(); vi.restoreAllMocks(); vi.unstubAllGlobals();
});

describe("owner activity refresh", () => {
  it.each(["interaction_started", "run_started"])("refreshes Working from the owner before the fallback poll on %s", async (event) => {
    const fake = transport(vi.fn());
    const quiet = await fake.loadExperience();
    const quietAgents = quiet.agent_sidebar!.live_snapshot!.agents!;
    quietAgents[0].response_phase = null;
    let finishRefresh!: (value: typeof quiet) => void;
    fake.loadExperience = vi.fn()
      .mockResolvedValueOnce(quiet)
      .mockImplementation(() => new Promise(resolve => { finishRefresh = resolve; }));
    let receive: ((frame: never) => void) | undefined;
    fake.subscribeTimeline = (_input, next) => { receive = next; return () => {}; };
    render(<ConsoleApp baseUrl="" transport={fake} />);
    await screen.findByTestId(`sidebar-agent:${identity}`);
    await waitFor(() => expect(receive).toBeTypeOf("function"));
    expect(fake.loadExperience).toHaveBeenCalledTimes(1);
    fireEvent.click(screen.getByRole("button", { name: "Working", exact: true }));
    expect(screen.queryByTestId(`sidebar-agent:${identity}`)).toBeNull();
    await act(async () => {
      for (const id of ["text-run-start", "text-run-start-repeat"]) {
        receive?.({ id, event, identity, interactionId: "text-only-run", timestampMs: Date.now(), data: { content: "A text-only request", prompt: "A text-only request" } } as never);
      }
      receive?.({ id: "text-run-delta", event: "text_delta", identity, interactionId: "text-only-run", timestampMs: Date.now(), data: { delta: "Working on the answer" } } as never);
    });
    await waitFor(() => expect(fake.loadExperience).toHaveBeenCalledTimes(2), { timeout: 1_000 });
    // Live chat activity is only a freshness signal; the pending owner reply
    // still says quiet, so it must not fabricate a Working roster row.
    expect(screen.queryByTestId(`sidebar-agent:${identity}`)).toBeNull();
    const working = {
      ...quiet,
      agent_sidebar: { ...quiet.agent_sidebar, live_snapshot: {
        ...quiet.agent_sidebar?.live_snapshot,
        agents: quietAgents.map(agent => ({ ...agent, response_phase: "waiting" })),
      } },
    };
    await act(async () => { finishRefresh(working as typeof quiet); });
    await screen.findByTestId(`sidebar-agent:${identity}`);
    expect(fake.loadExperience).toHaveBeenCalledTimes(2);
  });
});

describe("stock durable queue integration", () => {
  it.each(["namespace", "transport", "baseUrl"] as const)("clears the entire authorized console immediately when %s changes", async (change) => {
    const send = vi.fn(async (input) => ({ interaction_id: "unused", identity: input.identity }));
    const old = transport(send);
    old.queryTimeline = async () => ({ available: true, frames: [{ id: "private-frame", event: "interaction_complete", identity, interactionId: "private", timestampMs: 1, data: { text: "Previous principal private transcript" } }] });
    let oldReceive: ((frame: never) => void) | undefined;
    old.subscribeTimeline = (_input, receive) => { oldReceive = receive; return () => {}; };
    const view = render(<ConsoleApp baseUrl="old-url" storageNamespace="principal-one" transport={old} />);
    await screen.findAllByText("Previous principal private transcript");
    fireEvent.change(await screen.findByTestId(`chat-composer:${identity}`), { target: { value: "Previous private draft" } });
    const next = change === "transport" ? transport(send) : old;
    let finish!: (value: never) => void;
    next.loadExperience = vi.fn(() => new Promise(resolve => { finish = resolve; }));
    view.rerender(<ConsoleApp baseUrl={change === "baseUrl" ? "new-url" : "old-url"} storageNamespace={change === "namespace" ? "principal-two" : "principal-one"} transport={next} />);
    expect(screen.queryAllByText("Previous principal private transcript")).toHaveLength(0);
    expect(screen.queryByTestId(`sidebar-agent:${identity}`)).toBeNull();
    expect(screen.queryByDisplayValue("Previous private draft")).toBeNull();
    await act(async () => { oldReceive?.({ id: "late-old-frame", event: "interaction_complete", identity, interactionId: "late-private", timestampMs: 2, data: { text: "Late previous authority transcript" } } as never); });
    expect(screen.queryByText("Late previous authority transcript")).toBeNull();
    await waitFor(() => expect(next.loadExperience).toHaveBeenCalledTimes(1));
    await act(async () => { finish({ runtime_id: "queue-test", console_config: {}, agent_sidebar: { live_snapshot: { agents: [] } }, activity_feed: { filter_presets: [], active_preset_id: "all" } } as never); });
    await waitFor(() => expect(screen.queryByTestId("console-loading")).toBeNull());
    expect(screen.queryByTestId(`sidebar-agent:${identity}`)).toBeNull();
    expect(screen.queryByTestId(`chat-composer:${identity}`)).toBeNull();
    expect(send).not.toHaveBeenCalled();
  });

  it("ignores an old experience promise after a same-URL transport replacement", async () => {
    const old = transport(vi.fn()); let resolveOld!: (value: never) => void;
    old.loadExperience = () => new Promise(resolve => { resolveOld = resolve; });
    const view = render(<ConsoleApp baseUrl="" transport={old} storageNamespace="scope" />);
    const next = transport(vi.fn());
    const nextExperience = await next.loadExperience();
    next.loadExperience = async () => ({ ...nextExperience, runtime_id: "new-runtime", agent_sidebar: { live_snapshot: { agents: [] } } }) as never;
    view.rerender(<ConsoleApp baseUrl="" transport={next} storageNamespace="scope" />);
    await waitFor(() => expect(screen.queryByTestId("console-loading")).toBeNull());
    await act(async () => { resolveOld(nextExperience as never); });
    expect(screen.queryByTestId(`sidebar-agent:${identity}`)).toBeNull();
    expect(screen.queryByTestId(`chat-composer:${identity}`)).toBeNull();
  });

  it("clears pending approval data before loading a new principal", async () => {
    const fake = transport(vi.fn());
    fake.capabilities = async () => ({ methods: ["mobkit/gating/pending", "mobkit/gating/decide", "mobkit/console/send"] });
    fake.executeCommand = async () => ({ result: { pending: [{ pending_id: "private-gate", action_id: "private-action", action: "Private principal approval", origin: { identity }, risk_tier: "r3" }] } }) as never;
    const view = render(<ConsoleApp baseUrl="" transport={fake} storageNamespace="old-principal" />);
    await screen.findAllByText("Private principal approval");
    fake.loadExperience = () => new Promise(() => {});
    view.rerender(<ConsoleApp baseUrl="" transport={fake} storageNamespace="new-principal" />);
    expect(screen.queryAllByText("Private principal approval")).toHaveLength(0);
    expect(screen.queryByRole("button", { name: "Approve" })).toBeNull();
  });

  it("cannot dispatch a persisted intent from a lock callback after unmount", async () => {
    const scope = "scope";
    saveConsoleSendAttempts(window.localStorage, scope, identity, [createConsoleSendAttempt({ id: "unmount-lock", scope, destination: identity, origin: "console:old", idempotencyKey: "unmount-key", text: "old intent", now: 1 })]);
    const callbacks: Array<() => void> = [];
    Object.defineProperty(navigator, "locks", { configurable: true, value: { request: (_key: string, callback: () => unknown) => new Promise(resolve => callbacks.push(() => resolve(callback()))) } });
    const send = vi.fn(async input => ({ interaction_id: "unexpected", identity: input.identity }));
    const view = render(<ConsoleApp baseUrl="" storageNamespace={scope} transport={transport(send)} />);
    await waitFor(() => expect(callbacks.length).toBeGreaterThan(0));
    view.unmount();
    await act(async () => { callbacks.splice(0).forEach(callback => callback()); });
    expect(send).not.toHaveBeenCalled();
    expect(JSON.parse(window.localStorage.getItem(consoleSendStorageKey(scope, identity))!).attempts[0].state).toBe("draft");
  });

  it("cannot dispatch after an in-flight capability check outlives the console", async () => {
    const send = vi.fn(async input => ({ interaction_id: "unexpected", identity: input.identity }));
    const fake = transport(send); let finish!: (value: never) => void;
    fake.capabilities = vi.fn(() => new Promise(resolve => { finish = resolve; }));
    const view = render(<ConsoleApp baseUrl="" transport={fake} storageNamespace="scope" />);
    await compose("waiting for capability");
    await waitFor(() => expect(fake.capabilities).toHaveBeenCalled());
    view.unmount();
    await act(async () => { finish({ version: "test", methods: ["mobkit/console/send"] } as never); });
    expect(send).not.toHaveBeenCalled();
  });

  it("loads and sends through the current lifetime in StrictMode", async () => {
    const send = vi.fn(async input => ({ interaction_id: "strict-accepted", identity: input.identity }));
    render(<React.StrictMode><ConsoleApp baseUrl="" transport={transport(send)} storageNamespace="scope" /></React.StrictMode>);
    await compose("strict lifetime");
    await waitFor(() => expect(send).toHaveBeenCalledTimes(1));
  });

  it("persists before sending and removes only after canonical acceptance", async () => {
    let finish!: (value: never) => void;
    const send = vi.fn(async () => new Promise<never>((resolve) => { finish = resolve; }));
    render(<ConsoleApp baseUrl="" storageNamespace="runtime/realm/principal" transport={transport(send)} />);
    await compose("  byte exact\n🌳  ");
    await waitFor(() => expect(send).toHaveBeenCalledTimes(1));
    const saved = JSON.parse(window.localStorage.getItem(consoleSendStorageKey("runtime/realm/principal", identity))!);
    expect(saved.attempts[0].state).toBe("attempting");
    expect(JSON.parse(saved.attempts[0].envelopeJson).content).toBe("  byte exact\n🌳  ");
    expect(screen.getByTestId("pending-stack")).toBeTruthy();
    await act(async () => { finish({ interaction_id: "accepted", identity, input_frame_id: "canonical-frame" } as never); });
    await waitFor(() => expect(screen.queryByTestId("pending-stack")).toBeNull());
  });
  it("retains a lost response through reload without another dispatch", async () => {
    const send = vi.fn(async () => { throw new Error("response lost"); });
    const props = { baseUrl: "", storageNamespace: "runtime/realm/principal", transport: transport(send) };
    const view = render(<ConsoleApp {...props} />); await compose("saved unknown");
    await waitFor(() => expect(screen.getByText(/Acceptance unknown/)).toBeTruthy());
    expect(send).toHaveBeenCalledTimes(1); view.unmount();
    render(<ConsoleApp {...props} />);
    await waitFor(() => expect(screen.getByText(/Acceptance unknown/)).toBeTruthy());
    expect(send).toHaveBeenCalledTimes(1);
    expect(screen.getByTestId(/^pending-steer:/)).toBeDisabled();
    expect(screen.getByTestId(/^pending-edit:/)).toBeDisabled();
  });
  it("keeps composer visible when persistence fails before queue or dispatch", async () => {
    const send = vi.fn(async () => ({ interaction_id: "wrong", identity }));
    render(<ConsoleApp baseUrl="" storageNamespace="runtime/realm/principal" transport={transport(send)} />);
    await screen.findByTestId(`chat-composer:${identity}`);
    const original = Storage.prototype.setItem;
    vi.spyOn(Storage.prototype, "setItem").mockImplementation(function (key, value) {
      if (key.startsWith("mobkit-send-attempts:")) throw new Error("quota exhausted");
      original.call(this, key, value);
    });
    const textarea = await compose("preserve in composer");
    await waitFor(() => expect(screen.getAllByText(/quota exhausted/).length).toBeGreaterThan(0));
    await waitFor(() => expect(textarea.value).toBe("preserve in composer"));
    expect(send).not.toHaveBeenCalled();
    expect(screen.queryByTestId("pending-stack")).toBeNull();
  });
  it("clears visible drafts on authenticated scope change", async () => {
    const send = vi.fn(async () => ({ interaction_id: "i", identity })); const fake = transport(send);
    const view = render(<ConsoleApp baseUrl="" storageNamespace="principal-one" transport={fake} />);
    const textarea = await screen.findByTestId(`chat-composer:${identity}`);
    fireEvent.change(textarea, { target: { value: "private draft" } });
    view.rerender(<ConsoleApp baseUrl="" storageNamespace="principal-two" transport={fake} />);
    await waitFor(() => expect((screen.getByTestId(`chat-composer:${identity}`) as HTMLTextAreaElement).value).toBe(""));
    expect(send).not.toHaveBeenCalled();
  });
  it("freezes selected context separately from instruction and does not send on selection", async () => {
    const send = vi.fn(async (input) => ({ interaction_id: "with-context", identity: input.identity, input_frame_id: "submitted" }));
    const fake = transport(send);
    fake.queryTimeline = async () => ({ available: true, frames: [
      { id: "source-frame", event: "interaction_complete", identity, interactionId: "source-interaction", timestampMs: Date.now(), data: { text: "This is exact selected context." } },
    ] });
    render(<ConsoleApp baseUrl="" storageNamespace="runtime/realm/principal" transport={fake} />);
    const source = await waitFor(() => {
      const node = document.querySelector("[data-quote-message-id]");
      if (!node) throw new Error("Transcript quote source not yet rendered");
      return node;
    });
    const range = document.createRange(); range.selectNodeContents(source);
    const selection = window.getSelection()!; selection.removeAllRanges(); selection.addRange(range); fireEvent(document, new Event("selectionchange"));
    fireEvent.click(screen.getByRole("button", { name: "Add to message" }));
    expect(send).not.toHaveBeenCalled();
    expect(screen.getByRole("region", { name: "Quoted context for Queue agent" })).toBeTruthy();
    await compose("Explain the quote");
    await waitFor(() => expect(send).toHaveBeenCalledTimes(1));
    const content = send.mock.calls[0][0].content;
    expect(Array.isArray(content)).toBe(true);
    expect(content[0]).toEqual({ type: "text", text: "Explain the quote" });
    expect(content[1].text).toContain("This is exact selected context.");
    const context = JSON.parse(content[1].text.split("\n")[2]);
    expect(context.messageId).toBe(source.getAttribute("data-quote-message-id"));
    expect(context.sourceRange).toBeUndefined();
  });
  it("keeps a queued steer attempt visible on failure with its original frozen mode", async () => {
    const send = vi.fn(async () => { throw new Error("steer response lost"); });
    const fake = transport(send);
    fake.queryTimeline = async () => ({ available: true, frames: [
      { id: "busy-frame", event: "interaction_started", identity, interactionId: "already-busy", timestampMs: Date.now(), data: { content: "existing work" } },
    ] });
    render(<ConsoleApp baseUrl="" storageNamespace="runtime/realm/principal" transport={fake} />);
    await compose("interrupt with this");
    await waitFor(() => expect(screen.getByTestId("pending-stack")).toBeTruthy());
    expect(send).not.toHaveBeenCalled();
    fireEvent.click(screen.getByTestId(/^pending-steer:/));
    await waitFor(() => expect(screen.getByText(/Acceptance unknown/)).toBeTruthy());
    expect(send).toHaveBeenCalledTimes(1);
    const saved = JSON.parse(window.localStorage.getItem(consoleSendStorageKey("runtime/realm/principal", identity))!);
    expect(JSON.parse(saved.attempts[0].envelopeJson).handling_mode).toBe("steer");
    expect(saved.attempts[0].state).toBe("outcome-unknown");
  });

  it("resumes a persisted unattempted draft after loading idle history", async () => {
    const attempt = createConsoleSendAttempt({ id: "queued-before-reload", scope: "runtime/realm/principal", destination: identity, origin: "console:old-pane", idempotencyKey: "original-key", text: "resume this draft", now: 1 });
    saveConsoleSendAttempts(window.localStorage, attempt.scope, identity, [attempt]);
    const send = vi.fn(async (input) => ({ interaction_id: "resumed", identity: input.identity }));
    render(<ConsoleApp baseUrl="" storageNamespace={attempt.scope} transport={transport(send)} />);
    await waitFor(() => expect(send).toHaveBeenCalledTimes(1));
    expect(send.mock.calls[0][0]).toMatchObject({ origin: "console:old-pane", idempotencyKey: "original-key", content: "resume this draft" });
  });
  it("never replays a stale attempted record after its browser lease expires", async () => {
    const attempt = beginConsoleSendAttempt(createConsoleSendAttempt({ id: "in-flight", scope: "runtime/realm/principal", destination: identity, origin: "console:old-pane", idempotencyKey: "attempted-key", text: "may be accepted", now: 1 }), { owner: "closed-tab", now: 2, handlingMode: "queue" });
    saveConsoleSendAttempts(window.localStorage, attempt.scope, identity, [attempt]);
    const send = vi.fn(async (input) => ({ interaction_id: "duplicate", identity: input.identity }));
    render(<ConsoleApp baseUrl="" storageNamespace={attempt.scope} transport={transport(send)} />);
    await waitFor(() => expect(screen.getByText(/Acceptance unknown/)).toBeTruthy());
    expect(send).not.toHaveBeenCalled();
  });

  it("resumes an unattempted draft once storage recovers on window focus", async () => {
    const scope = "runtime/realm/principal";
    const attempt = createConsoleSendAttempt({ id: "waiting-storage", scope, destination: identity, origin: "console:old", idempotencyKey: "recover-key", text: "after recovery", now: 1 });
    saveConsoleSendAttempts(window.localStorage, scope, identity, [attempt]);
    let quota = true;
    const original = Storage.prototype.setItem;
    vi.spyOn(Storage.prototype, "setItem").mockImplementation(function (key, value) {
      if (quota && key.startsWith("mobkit-send-attempts:")) throw new Error("frozen-write quota");
      original.call(this, key, value);
    });
    const send = vi.fn(async (input) => ({ interaction_id: "recovered", identity: input.identity }));
    render(<ConsoleApp baseUrl="" storageNamespace={scope} transport={transport(send)} />);
    await waitFor(() => expect(screen.getAllByText(/frozen-write quota/).length).toBeGreaterThan(0));
    expect(send).not.toHaveBeenCalled();
    quota = false; fireEvent(window, new Event("focus"));
    await waitFor(() => expect(send).toHaveBeenCalledTimes(1));
    expect(send.mock.calls[0][0].idempotencyKey).toBe("recover-key");
    await waitFor(() => expect(screen.queryByTestId("pending-stack")).toBeNull());
  });

  it("reopens a queued target after it becomes idle while its pane is closed", async () => {
    const send = vi.fn(async (input) => ({ interaction_id: "reopened", identity: input.identity }));
    const fake = transport(send); let receive: ((frame: never) => void) | undefined;
    fake.queryTimeline = async () => ({ available: true, frames: [{ id: "busy", event: "interaction_started", identity, interactionId: "busy", timestampMs: 1, data: { content: "working" } }] });
    fake.subscribeTimeline = (_input, onFrame) => { receive = onFrame; return () => {}; };
    render(<ConsoleApp baseUrl="" storageNamespace="runtime/realm/principal" transport={fake} />);
    await compose("queued while busy");
    await screen.findByTestId("pending-stack");
    fireEvent.click(screen.getByTestId("pane-close:panel-1"));
    await act(async () => { receive?.({ id: "idle", event: "interaction_complete", identity, interactionId: "busy", timestampMs: 2, data: { text: "finished" } } as never); });
    expect(send).not.toHaveBeenCalled();
    const row = await waitFor(() => {
      const node = document.querySelector('.agent[role="button"], .cc-sidebar-row');
      if (!node) throw new Error("Missing agent sidebar row"); return node;
    });
    fireEvent.click(row);
    await waitFor(() => expect(send).toHaveBeenCalledTimes(1));
  });

  it("isolates two pane drafts and quotes through submission and reload", async () => {
    seed(true);
    const send = vi.fn(async (input) => ({ interaction_id: "only-a", identity: input.identity }));
    const fake = transport(send);
    fake.queryTimeline = async () => ({ available: true, frames: [{ id: "quote", event: "interaction_complete", identity, interactionId: "q", timestampMs: 1, data: { text: "Quoted source" } }] });
    const props = { baseUrl: "", storageNamespace: "runtime/realm/principal", transport: fake };
    const view = render(<ConsoleApp {...props} />);
    await waitFor(() => expect(screen.getAllByTestId(`chat-composer:${identity}`)).toHaveLength(2));
    for (const panelId of ["panel-1", "panel-2"]) {
      const pane = screen.getByTestId(`pane:${panelId}`);
      const source = await waitFor(() => { const node = pane.querySelector("[data-quote-message-id]"); if (!node) throw new Error("No quote source"); return node; });
      const range = document.createRange(); range.selectNodeContents(source);
      window.getSelection()!.removeAllRanges(); window.getSelection()!.addRange(range); fireEvent(document, new Event("selectionchange"));
      fireEvent.click([...pane.querySelectorAll("button")].find((button) => button.getAttribute("aria-label") === "Add to message")!);
      fireEvent.change(pane.querySelector("textarea")!, { target: { value: `Unsent ${panelId}` } });
    }
    await act(async () => { await new Promise((resolve) => setTimeout(resolve, 160)); });
    fireEvent.keyDown(screen.getByTestId("pane:panel-1").querySelector("textarea")!, { key: "Enter" });
    await waitFor(() => expect(send).toHaveBeenCalledTimes(1));
    view.unmount(); render(<ConsoleApp {...props} />);
    await waitFor(() => expect((screen.getByTestId("pane:panel-2").querySelector("textarea") as HTMLTextAreaElement).value).toBe("Unsent panel-2"));
    expect(screen.getByTestId("pane:panel-2").querySelector(".cc-context-chip blockquote")?.textContent).toBe("Quoted source");
  });

  it("isolates independently opened tab drafts while preserving each reload identity", async () => {
    const send = vi.fn(async (input) => ({ interaction_id: "tab-a-send", identity: input.identity }));
    const props = { baseUrl: "", storageNamespace: "runtime/realm/principal", transport: transport(send) };
    window.sessionStorage.setItem("mobkit-composer-tab:v1", "browser-a");
    let view = render(<ConsoleApp {...props} />);
    fireEvent.change(await screen.findByTestId(`chat-composer:${identity}`), { target: { value: "Draft browser A" } });
    await act(async () => { await new Promise(resolve => setTimeout(resolve, 160)); }); view.unmount();
    window.sessionStorage.setItem("mobkit-composer-tab:v1", "browser-b");
    view = render(<ConsoleApp {...props} />);
    expect((await screen.findByTestId(`chat-composer:${identity}`) as HTMLTextAreaElement).value).toBe("");
    fireEvent.change(screen.getByTestId(`chat-composer:${identity}`), { target: { value: "Draft browser B" } });
    await act(async () => { await new Promise(resolve => setTimeout(resolve, 160)); }); view.unmount();
    window.sessionStorage.setItem("mobkit-composer-tab:v1", "browser-a");
    view = render(<ConsoleApp {...props} />);
    await waitFor(() => expect((screen.getByTestId(`chat-composer:${identity}`) as HTMLTextAreaElement).value).toBe("Draft browser A"));
    fireEvent.keyDown(screen.getByTestId(`chat-composer:${identity}`), { key: "Enter" });
    await waitFor(() => expect(send).toHaveBeenCalledTimes(1)); view.unmount();
    window.sessionStorage.setItem("mobkit-composer-tab:v1", "browser-b"); render(<ConsoleApp {...props} />);
    await waitFor(() => expect((screen.getByTestId(`chat-composer:${identity}`) as HTMLTextAreaElement).value).toBe("Draft browser B"));
    expect([...Array(window.sessionStorage.length)].map((_, index) => window.sessionStorage.key(index)).filter(key => key?.startsWith("mobkit-composer-draft:v2:"))).toHaveLength(2);
  });

  it("isolates transient intent when a replacement transport uses the same URL", async () => {
    const oldSend = vi.fn(async (input) => ({ interaction_id: "old", identity: input.identity }));
    const oldTransport = transport(oldSend);
    oldTransport.queryTimeline = async () => ({ available: true, frames: [{ id: "busy", event: "interaction_started", identity, interactionId: "busy", timestampMs: 1, data: { content: "working" } }] });
    const view = render(<ConsoleApp baseUrl="" transport={oldTransport} />);
    await compose("belongs to old controller"); await screen.findByTestId("pending-stack");
    fireEvent.change(screen.getByTestId(`chat-composer:${identity}`), { target: { value: "private unsent draft" } });
    const newSend = vi.fn(async (input) => ({ interaction_id: "new", identity: input.identity }));
    view.rerender(<ConsoleApp baseUrl="" transport={transport(newSend)} />);
    await waitFor(() => expect(screen.queryByTestId("pending-stack")).toBeNull());
    expect((await screen.findByTestId(`chat-composer:${identity}`) as HTMLTextAreaElement).value).toBe("");
    expect(oldSend).not.toHaveBeenCalled(); expect(newSend).not.toHaveBeenCalled();
  });

  it("does not dispatch a queued lock callback through a replacement controller", async () => {
    const scope = "runtime/realm/principal";
    const attempt = createConsoleSendAttempt({ id: "lock-wait", scope, destination: identity, origin: "console:old", idempotencyKey: "locked", text: "old intent", now: 1 });
    saveConsoleSendAttempts(window.localStorage, scope, identity, [attempt]);
    const callbacks: Array<() => void> = [];
    Object.defineProperty(navigator, "locks", { configurable: true, value: { request: (_key: string, callback: () => unknown) => new Promise(resolve => callbacks.push(() => resolve(callback()))) } });
    const oldSend = vi.fn(async (input) => ({ interaction_id: "old", identity: input.identity }));
    const view = render(<ConsoleApp baseUrl="" storageNamespace={scope} transport={transport(oldSend)} />);
    await waitFor(() => expect(callbacks.length).toBeGreaterThan(0));
    const newSend = vi.fn(async (input) => ({ interaction_id: "new", identity: input.identity }));
    view.rerender(<ConsoleApp baseUrl="" storageNamespace={scope} transport={transport(newSend)} />);
    await act(async () => { callbacks.shift()!(); });
    expect(oldSend).not.toHaveBeenCalled(); expect(newSend).not.toHaveBeenCalled();
    const saved = JSON.parse(window.localStorage.getItem(consoleSendStorageKey(scope, identity))!).attempts[0];
    expect(saved.state).toBe("draft");
  });

  it("does not turn a failed capability lookup into a definite refusal", async () => {
    const send = vi.fn(async (input) => ({ interaction_id: "unexpected", identity: input.identity }));
    const fake = transport(send); fake.capabilities = async () => { throw new Error("capability connection lost"); };
    render(<ConsoleApp baseUrl="" storageNamespace="runtime/realm/principal" transport={fake} />);
    await compose("uncertain capability request");
    await screen.findByText(/Acceptance unknown/);
    expect(screen.queryByRole("button", { name: "Retry same attempt" })).toBeNull();
    expect(send).not.toHaveBeenCalled();
  });

  it("proves capability refusal before dispatch and retries the same frozen envelope explicitly", async () => {
    const send = vi.fn(async (input) => ({ interaction_id: "restored-capability", identity: input.identity }));
    const fake = transport(send); let canSend = false;
    fake.capabilities = async () => ({ version: "test", methods: ["mobkit/console/timeline", ...(canSend ? ["mobkit/console/send"] : [])] }) as never;
    render(<ConsoleApp baseUrl="" storageNamespace="runtime/realm/principal" transport={fake} />);
    await compose("retry after capability restoration");
    await screen.findByRole("button", { name: "Retry same attempt" });
    expect(send).not.toHaveBeenCalled();
    const saved = JSON.parse(window.localStorage.getItem(consoleSendStorageKey("runtime/realm/principal", identity))!).attempts[0];
    expect(saved.state).toBe("definitely-rejected");
    canSend = true; fireEvent.click(screen.getByRole("button", { name: "Retry same attempt" }));
    await waitFor(() => expect(send).toHaveBeenCalledTimes(1));
    expect(send.mock.calls[0][0]).toMatchObject({ content: saved.text, idempotencyKey: saved.idempotencyKey, origin: saved.origin, handlingMode: "queue" });
  });

  it("preserves a multi-delta message reference without inventing a frame-relative source range", async () => {
    const send = vi.fn(async (input) => ({ interaction_id: "q", identity: input.identity }));
    const fake = transport(send);
    fake.queryTimeline = async () => ({ available: true, frames: [
      { id: "d", event: "interaction_complete", identity, interactionId: "stream", timestampMs: 1, data: { text: "Unrelated" } },
      { id: "d:1", event: "text_delta", identity, interactionId: "source", timestampMs: 2, data: { delta: "Hello " } },
      { id: "d:2", event: "text_delta", identity, interactionId: "source", timestampMs: 3, data: { delta: "world" } },
      { id: "d:3", event: "interaction_complete", identity, interactionId: "source", timestampMs: 4, data: { text: "Hello world" } },
    ] });
    render(<ConsoleApp baseUrl="" storageNamespace="runtime/realm/principal" transport={fake} />);
    const source = await waitFor(() => { const node = [...document.querySelectorAll("[data-quote-message-id]")].find(item => item.textContent?.includes("Hello world")); if (!node) throw new Error("No assembled source"); return node; });
    const node = [...source.querySelectorAll("p")].find(item => item.textContent === "Hello world")!.firstChild!;
    const range = document.createRange(); range.setStart(node, 6); range.setEnd(node, 11);
    window.getSelection()!.removeAllRanges(); window.getSelection()!.addRange(range); fireEvent(document, new Event("selectionchange"));
    fireEvent.click(screen.getByRole("button", { name: "Add to message" }));
    await compose("Explain this fragment");
    await waitFor(() => expect(send).toHaveBeenCalledTimes(1));
    const record = JSON.parse(send.mock.calls[0][0].content[1].text.split("\n")[2]);
    expect(record.quote).toBe("world");
    expect(record.messageId).toBe(source.getAttribute("data-quote-message-id"));
    expect(record.messageId).toBe("d:1");
    expect(record.sourceRange).toBeUndefined();
  });

  it.each([
    ["published different text", "Newer unsent intent", 450],
    ["published identical text", "First submitted intent", 450],
    ["unpublished different text", "Newer unsent intent", 0],
    ["unpublished identical text", "First submitted intent", 0],
  ] as const)("preserves %s while the previous enqueue waits for a Web Lock", async (_case, nextText, delay) => {
    const callbacks: Array<() => void> = [];
    Object.defineProperty(navigator, "locks", { configurable: true, value: { request: (_key: string, callback: () => unknown) => new Promise(resolve => callbacks.push(() => resolve(callback()))) } });
    const send = vi.fn(async input => ({ interaction_id: "accepted", identity: input.identity }));
    render(<ConsoleApp baseUrl="" storageNamespace="runtime/realm/principal" transport={transport(send)} />);
    const textarea = await compose("First submitted intent");
    await waitFor(() => expect(callbacks.length).toBeGreaterThan(0));
    expect(textarea.value).toBe("");
    fireEvent.change(textarea, { target: { value: nextText } });
    if (delay) await act(async () => { await new Promise(resolve => setTimeout(resolve, delay)); });
    await act(async () => { callbacks.shift()!(); await Promise.resolve(); });
    await waitFor(() => expect(textarea.value).toBe(nextText));
    const documents = [...Array(window.sessionStorage.length)].map((_, index) => window.sessionStorage.key(index)!).filter(key => key.startsWith("mobkit-composer-draft:v2:")).map(key => JSON.parse(window.sessionStorage.getItem(key)!));
    expect(documents.some(draft => draft.text === nextText)).toBe(true);
  });

  it("removes only submitted quotes when a newer quote is added during the enqueue lock wait", async () => {
    const callbacks: Array<() => void> = [];
    Object.defineProperty(navigator, "locks", { configurable: true, value: { request: (_key: string, callback: () => unknown) => new Promise(resolve => callbacks.push(() => resolve(callback()))) } });
    const fake = transport(vi.fn(async input => ({ interaction_id: "accepted", identity: input.identity })));
    fake.queryTimeline = async () => ({ available: true, frames: [{ id: "quote-race", event: "interaction_complete", identity, interactionId: "source", timestampMs: 1, data: { text: "Original quote. Newer quote." } }] });
    render(<ConsoleApp baseUrl="" storageNamespace="runtime/realm/principal" transport={fake} />);
    const paragraph = await waitFor(() => { const element = [...document.querySelectorAll("[data-quote-message-id] p")].find(item => item.textContent === "Original quote. Newer quote."); if (!element) throw new Error("No source"); return element; });
    const select = (text: string) => {
      const node = paragraph.firstChild!; const start = node.textContent!.indexOf(text);
      const range = document.createRange(); range.setStart(node, start); range.setEnd(node, start + text.length);
      window.getSelection()!.removeAllRanges(); window.getSelection()!.addRange(range); fireEvent(document, new Event("selectionchange"));
      fireEvent.click(screen.getByRole("button", { name: "Add to message" }));
    };
    select("Original quote.");
    const textarea = await compose("First instruction");
    await waitFor(() => expect(callbacks.length).toBeGreaterThan(0));
    select("Newer quote.");
    fireEvent.change(textarea, { target: { value: "Newer instruction" } });
    await act(async () => { callbacks.shift()!(); await Promise.resolve(); });
    await waitFor(() => expect([...document.querySelectorAll(".cc-context-chip blockquote")].map(node => node.textContent)).toEqual(["Newer quote."]));
    expect(textarea.value).toBe("Newer instruction");
    const documents = [...Array(window.sessionStorage.length)].map((_, index) => window.sessionStorage.key(index)!).filter(key => key.startsWith("mobkit-composer-draft:v2:")).map(key => JSON.parse(window.sessionStorage.getItem(key)!));
    expect(documents).toHaveLength(1);
    expect(documents[0].text).toBe("Newer instruction");
    expect(documents[0].contexts.map((context: { quote: string }) => context.quote)).toEqual(["Newer quote."]);
    const queued = JSON.parse(window.localStorage.getItem(consoleSendStorageKey("runtime/realm/principal", identity))!).attempts[0];
    expect(queued.contexts.map((context: { quote: string }) => context.quote)).toEqual(["Original quote."]);
  });

  it.each(["Newer attachment instruction", "Original attachment instruction"])("preserves new attachments and draft %s while an earlier attachment send awaits acceptance", async (nextText) => {
    vi.stubGlobal("URL", class extends URL {
      static createObjectURL = vi.fn(() => `blob:preview-${Math.random()}`);
      static revokeObjectURL = vi.fn();
    });
    let finish!: (value: never) => void;
    const send = vi.fn((_input: Parameters<MobKitConsoleTransport["send"]>[0]) => new Promise<never>(resolve => { finish = resolve; }));
    const fake = transport(send); const experience = await fake.loadExperience();
    fake.loadExperience = async () => ({ ...experience, agent_sidebar: { live_snapshot: { agents: [{ identity, member_id: identity, agent_id: identity, label: "Queue agent", kind: "member", state: "running", addressable: true, affordances: { can_send_message: true }, model_capabilities: { image_input: true } }] } } }) as never;
    render(<ConsoleApp baseUrl="" storageNamespace="runtime/realm/principal" transport={fake} />);
    const textarea = await screen.findByTestId(`chat-composer:${identity}`) as HTMLTextAreaElement;
    const first = new File([new Uint8Array([1, 2, 3])], "first.png", { type: "image/png" });
    const second = new File([new Uint8Array([4, 5, 6])], "second.png", { type: "image/png" });
    const drop = (file: File) => fireEvent.drop(document.querySelector(".composer__shell")!, { dataTransfer: { files: [file], items: [], types: ["Files"], getData: () => "" } });
    drop(first); await waitFor(() => expect(screen.getAllByRole("button", { name: "Remove attachment" })).toHaveLength(1));
    await compose("Original attachment instruction");
    await waitFor(() => expect(send).toHaveBeenCalledTimes(1));
    expect(send.mock.calls[0][0].attachments).toEqual([first]);
    fireEvent.change(textarea, { target: { value: "" } });
    fireEvent.change(textarea, { target: { value: nextText } });
    drop(second); await waitFor(() => expect(screen.getAllByRole("button", { name: "Remove attachment" })).toHaveLength(2));
    await act(async () => { finish({ interaction_id: "accepted-image", identity, input_frame_id: "image-frame" } as never); });
    await waitFor(() => expect(screen.getAllByRole("button", { name: "Remove attachment" })).toHaveLength(1));
    expect(textarea.value).toBe(nextText);
    expect(send).toHaveBeenCalledTimes(1);
    const documents = [...Array(window.sessionStorage.length)].map((_, index) => window.sessionStorage.key(index)!).filter(key => key.startsWith("mobkit-composer-draft:v2:")).map(key => JSON.parse(window.sessionStorage.getItem(key)!));
    expect(documents.some(draft => draft.text === nextText)).toBe(true);
  });

});

describe("durable queued quote editing", () => {
  function prepared() {
    const scope = "quote-edit-scope";
    const context = createConsoleContextRecord({ id: "quote-edit-one", sourceScope: scope, sourceIdentity: identity, messageId: "source-message", label: "Review source", quote: "original", sourceText: "before original after" });
    const other = { ...context, id: "quote-edit-two", quote: "another", sourceRange: undefined };
    const attempt = createConsoleSendAttempt({ id: "quote-edit-draft", scope, destination: identity, origin: "console:test", idempotencyKey: "quote-edit-key", text: "Compare quoted evidence", contexts: [context, other], now: 1 });
    saveConsoleSendAttempts(window.localStorage, scope, identity, [attempt]);
    const send = vi.fn(async () => { throw new Error("response lost after actual attempt"); });
    const fake = transport(send);
    fake.queryTimeline = async () => ({ available: true, frames: [{ id: "busy", event: "interaction_started", identity, interactionId: "busy", timestampMs: 1, data: { content: "existing work" } }] });
    return { scope, attempt, send, fake, read: () => JSON.parse(window.localStorage.getItem(consoleSendStorageKey(scope, identity))!).attempts[0] };
  }
  it("persists an exact quote edit under the queue lock, then freezes those bytes for the attempted send", async () => {
    const fixture = prepared();
    render(<ConsoleApp baseUrl="" storageNamespace={fixture.scope} transport={fixture.fake} />);
    await screen.findByTestId("pending-item:quote-edit-draft");
    fireEvent.click(screen.getByText("Compare quoted evidence"));
    fireEvent.click((await screen.findAllByRole("button", { name: "Edit quote from Review source" }))[0]);
    const exact = "  Edited A\u030A 🚀\nsecond line  ";
    const editor = screen.getByRole("textbox", { name: "Quote from Review source" });
    fireEvent.change(editor, { target: { value: exact } });
    fireEvent.keyDown(editor, { key: "Backspace", ctrlKey: true });
    expect(fixture.read().contexts[0].quote).toBe("original");
    fireEvent.click(screen.getByRole("button", { name: "Save quote" }));
    await waitFor(() => expect(fixture.read().contexts[0].quote).toBe(exact));
    expect(fixture.read().contexts.map((context: { id: string }) => context.id)).toEqual(["quote-edit-one", "quote-edit-two"]);
    expect(fixture.read().contexts[0].sourceRange).toBeUndefined();
    expect(fixture.read().contexts[1]).toEqual(fixture.attempt.contexts[1]);
    expect(fixture.send).not.toHaveBeenCalled();
    fireEvent.click(screen.getByTestId("pending-steer:quote-edit-draft"));
    await waitFor(() => expect(fixture.read().state).toBe("outcome-unknown"));
    const frozen = fixture.read().envelopeJson;
    expect(JSON.parse(JSON.parse(frozen).content[1].text.split("\n")[2]).quote).toBe(exact);
    expect(fixture.send).toHaveBeenCalledTimes(1);
    expect(screen.queryByRole("button", { name: "Edit quote from Review source" })).toBeNull();
    expect(fixture.read().envelopeJson).toBe(frozen);
  });
  it("rejects saving a quote if another tab has already attempted that queued message", async () => {
    const fixture = prepared();
    render(<ConsoleApp baseUrl="" storageNamespace={fixture.scope} transport={fixture.fake} />);
    await screen.findByTestId("pending-item:quote-edit-draft");
    fireEvent.click(screen.getByText("Compare quoted evidence"));
    fireEvent.click((await screen.findAllByRole("button", { name: "Edit quote from Review source" }))[0]);
    fireEvent.change(screen.getByRole("textbox", { name: "Quote from Review source" }), { target: { value: "stale local edit" } });
    const attempted = beginConsoleSendAttempt(fixture.attempt, { owner: "other-tab", now: Date.now(), handlingMode: "queue" });
    saveConsoleSendAttempts(window.localStorage, fixture.scope, identity, [attempted], [fixture.attempt]);
    fireEvent.click(screen.getByRole("button", { name: "Save quote" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("no longer an editable queued draft");
    expect(fixture.read().envelopeJson).toBe(attempted.envelopeJson);
    expect(fixture.read().contexts[0].quote).toBe("original");
    expect(fixture.send).not.toHaveBeenCalled();
  });
});
