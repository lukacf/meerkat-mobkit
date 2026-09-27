import { fireEvent, render } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { assistantPresentationEntries } from "../../../console-core/src/assistant-presentation";
import type { ConversationMessageEntry } from "../../../console-core/src/conversation";
import type { ConsoleFrame } from "../../../console-core/src/runtime-types";
import { buildConversationViewState, mapFramesToTimelineEntries as shared } from "../../../console-core/src/adapters";
import { mapFramesToTimelineEntries as stock } from "../../../../console/src/lib/adapters";
import { ChatPane } from "../../../../console/src/panels/ChatPane";
import { ConversationTranscript } from "./conversation-transcript";

const agent = { agent_id: "worker", member_id: "worker", identity: "worker", label: "Worker", kind: "mob_agent" };
const callId = "poll-empty";
const resultBytes = '{"events":[],"note":"A\u030a, å and 🚀"}';
const noop = () => {};
function frame(id: string, event: string, data: Record<string, unknown>, scope: Partial<ConsoleFrame> = {}): ConsoleFrame {
  return { id, event, data, runtimeKey: "runtime", identity: "worker", sessionId: "session", runId: "run",
    interactionId: "interaction", sourceKind: "console_event", timestampMs: 1000, ...scope };
}
function liveTools(ids: string[], scope: Partial<ConsoleFrame> = {}) {
  return ids.flatMap(id => [
    frame(`request-${id}`, "tool_call_requested", { id, name: "workgraph_events", args: { after_seq: 1 } }, scope),
    frame(`result-${id}`, "tool_execution_completed", { id, name: "workgraph_events", result: resultBytes, is_error: false }, scope),
  ]);
}
function savedTools(ids: string[], text = "", scope: Partial<ConsoleFrame> = {}) {
  return frame("canonical-tool-frame", "assistant_message", {
    assistant_message_id: "assistant-tool",
    message: { role: "block_assistant", blocks: [
      ...ids.map(id => ({ block_type: "tool_use", data: { id, name: "workgraph_events", args: { after_seq: 1 } } })),
      ...(text ? [{ block_type: "text", data: { text } }] : []),
    ], stop_reason: text ? "end_turn" : "tool_use" }, text, result: text,
  }, { sourceKind: "session_history", ...scope });
}
function toolNodes(container: HTMLElement) {
  const tool = container.querySelector<HTMLElement>(".cc-tool-call")!;
  expect(tool).toBeTruthy();
  const header = tool.querySelector<HTMLElement>(".cc-tool-call__header")!;
  const row = tool.closest<HTMLElement>("[data-conversation-row-id]")!;
  const turn = row.closest<HTMLElement>("[data-conversation-turn-id]")!;
  return { tool, header, row, turn };
}
function openResult(container: HTMLElement, expectedBytes = resultBytes) {
  const nodes = toolNodes(container);
  for (const header of container.querySelectorAll<HTMLElement>(".cc-tool-call__header")) {
    if (header.getAttribute("aria-expanded") !== "true") fireEvent.click(header);
  }
  const results = [...container.querySelectorAll<HTMLElement>(".cc-tool-call__section")]
    .filter(node => node.querySelector(".cc-tool-call__section-label")?.textContent === "Result")
    .map(node => node.querySelector("pre")!);
  expect(results.length).toBeGreaterThan(0);
  results.forEach(node => expect(node.textContent).toBe(expectedBytes));
  return { ...nodes, results };
}

for (const [surface, map] of [["stock", stock], ["shared", shared]] as const) {
  function view(frames: ConsoleFrame[]) {
    const entries = map(agent, frames, { textMode: "markdown" });
    return surface === "stock"
      ? <ChatPane agent={agent} agentLabel="Worker" identity="worker" entries={entries} phase={null}
          draft="Unsent draft" sending={false} staged={[]} onDraftChange={noop} onStagedChange={noop} onSend={() => false} />
      : <ConversationTranscript viewState={buildConversationViewState({ memberId: "worker", agentLabel: "Worker", entries })} />;
  }
  describe(`${surface} tool presentation identity`, () => {
    it("opens a fresh pending call and respects manual close until new outcome evidence", () => {
      const id = "held-read";
      const args = { path: "late-A\u030a-å-🚀.txt" };
      const pending = [frame("held-request", "tool_call_requested", { id, name: "read_file", args })];
      const result = render(view(pending));
      const original = toolNodes(result.container);
      expect(original.tool.classList.contains("cc-tool-call--pending")).toBe(true);
      expect(original.header.getAttribute("aria-expanded")).toBe("true");
      const sections = [...original.tool.querySelectorAll<HTMLElement>(".cc-tool-call__section")];
      expect(sections.map(node => node.querySelector(".cc-tool-call__section-label")?.textContent)).toEqual(["Input"]);
      expect(sections[0].querySelector("pre")?.textContent).toBe(JSON.stringify(args));
      fireEvent.click(original.header);
      expect(original.header.getAttribute("aria-expanded")).toBe("false");
      result.rerender(view([...pending]));
      expect(toolNodes(result.container).header).toBe(original.header);
      expect(original.header.getAttribute("aria-expanded")).toBe("false");
      const started = frame("held-started", "tool_execution_started", { id, name: "read_file", args });
      result.rerender(view([...pending, started]));
      expect(toolNodes(result.container).header).toBe(original.header);
      expect(original.header.getAttribute("aria-expanded")).toBe("false");
      expect(original.tool.querySelector(".cc-tool-call__body")).toBeNull();
      const canonical = frame("canonical-held-call", "assistant_message", {
        assistant_message_id: "assistant-held-call",
        message: { role: "block_assistant", blocks: [{ block_type: "tool_use", data: { id, name: "read_file", args } }], stop_reason: "tool_use" },
      }, { sourceKind: "session_history" });
      result.rerender(view([...pending, started, canonical]));
      const hydrated = toolNodes(result.container);
      expect(hydrated.row).toBe(original.row);
      expect(hydrated.tool).toBe(original.tool);
      expect(hydrated.header).toBe(original.header);
      expect(hydrated.header.getAttribute("aria-expanded")).toBe("true");
      expect(hydrated.tool.querySelector(".cc-tool-call__status")?.textContent).toContain("Completion unknown");
      const reopened = [...hydrated.tool.querySelectorAll<HTMLElement>(".cc-tool-call__section")];
      expect(reopened.map(node => node.querySelector(".cc-tool-call__section-label")?.textContent)).toEqual(["Input"]);
      expect(reopened[0].querySelector("pre")?.textContent).toBe(JSON.stringify(args));
      fireEvent.click(hydrated.header);
      expect(hydrated.header.getAttribute("aria-expanded")).toBe("false");
      result.rerender(view([...pending, started, { ...canonical }]));
      expect(toolNodes(result.container).header).toBe(original.header);
      expect(original.header.getAttribute("aria-expanded")).toBe("false");
      expect(original.tool.querySelector(".cc-tool-call__body")).toBeNull();
    });
    for (const variant of ["tool-only", "with-user", "with-answer", "bundled-answer", "two-tools", "two-owners"] as const) {
      it(`keeps exact opened tool DOM across canonical hydration: ${variant}`, () => {
        const ids = variant === "two-tools" || variant === "two-owners" ? [callId, "poll-again"] : [callId];
        const user = variant === "with-user" ? [frame("user", "user_input", { content: "Review the events." }, { sourceKind: "send" })] : [];
        const text = "The events are checked.";
        const answer = variant === "with-answer" || variant === "bundled-answer"
          ? [frame("answer", "text_delta", { assistant_message_id: variant === "bundled-answer" ? "assistant-tool" : "assistant-answer", delta: text })] : [];
        const live = [...user, ...liveTools(ids), ...answer];
        const result = render(view(live));
        const expectedBytes = ids.length > 1 && surface === "stock" ? JSON.stringify(JSON.parse(resultBytes), null, 2) : resultBytes;
        const original = openResult(result.container, expectedBytes);
        const rowId = original.row.dataset.conversationRowId;
        const canonical = variant === "two-owners" ? ids.map((id, index) => {
          const saved = savedTools([id]);
          saved.id = index ? "canonical-second-tool" : "canonical-tool-frame";
          (saved.data as Record<string, unknown>).assistant_message_id = `assistant-tool-${index}`;
          return saved;
        }) : [savedTools(ids, variant === "bundled-answer" ? text : "")];
        const projected = map(agent, [...live, ...canonical], { textMode: "markdown" });
        expect(projected.some(entry => entry.id === "canonical-tool-frame")).toBe(true);
        result.rerender(view([...live, ...canonical]));
        const current = toolNodes(result.container);
        expect(current.row.dataset.conversationRowId).toBe(rowId);
        expect(current.turn).toBe(original.turn);
        expect(current.row).toBe(original.row);
        expect(current.tool).toBe(original.tool);
        expect(current.header).toBe(original.header);
        expect(current.header.getAttribute("aria-expanded")).toBe("true");
        const results = [...result.container.querySelectorAll<HTMLElement>(".cc-tool-call__section")]
          .filter(node => node.querySelector(".cc-tool-call__section-label")?.textContent === "Result")
          .map(node => node.querySelector("pre")!);
        expect(results).toHaveLength(original.results.length);
        results.forEach((node, index) => { expect(node).toBe(original.results[index]); expect(node.textContent).toBe(expectedBytes); });
        const rowIds = [...result.container.querySelectorAll<HTMLElement>("[data-conversation-row-id]")].map(node => node.dataset.conversationRowId);
        expect(new Set(rowIds).size).toBe(rowIds.length);
      });
    }
    for (const field of ["runtimeKey", "identity", "sessionId", "runId"] as const) {
      it(`does not reuse disclosure state for the same call ID in another ${field}`, () => {
        const result = render(view(liveTools([callId])));
        const original = openResult(result.container);
        result.rerender(view(liveTools([callId], { [field]: `other-${field}` })));
        const current = toolNodes(result.container);
        expect(current.row.dataset.conversationRowId).not.toBe(original.row.dataset.conversationRowId);
        expect(current.tool).not.toBe(original.tool);
        expect(current.header.getAttribute("aria-expanded")).toBe("false");
      });
    }
  });
}


describe("typed tool display authority", () => {
  function projected(source: ConsoleFrame, toolCallId = callId) {
    const entry: ConversationMessageEntry = { kind: "message", id: source.id, identity: { role: "assistant", label: "Worker" },
      variant: "rich", blocks: [{ type: "tool-call", toolCallId, name: "workgraph_events", arguments: "{}", status: "success" }] };
    return assistantPresentationEntries([entry], new Map(), new Map([[source.id, source]]))[0];
  }
  for (const field of ["runtimeKey", "identity", "sessionId", "runId"] as const) {
    for (const value of [undefined, " "]) {
      it(`keeps the source fallback when ${field} is ${value === undefined ? "absent" : "blank"}`, () => {
        const source = frame("original-source", "tool_call_requested", { id: callId }, { [field]: value });
        expect(projected(source).renderKey).toBeUndefined();
        expect(projected(source).id).toBe("original-source");
      });
    }
  }
  it("does not authorize an invented canonical tool ID from another live request", () => {
    const saved = savedTools([]);
    (saved.data as { message: { blocks: unknown[] } }).message.blocks = [{ block_type: "tool_use", data: { name: "workgraph_events" } }];
    const live = frame("live", "tool_call_requested", { id: "history-tool-1" });
    const entry: ConversationMessageEntry = { kind: "message", id: saved.id, identity: { role: "assistant", label: "Worker" },
      variant: "rich", blocks: [{ type: "tool-call", toolCallId: "history-tool-1", name: "workgraph_events", arguments: "{}", status: "success" }] };
    const [result] = assistantPresentationEntries([entry], new Map(), new Map([[saved.id, saved], [live.id, live]]));
    expect(result.renderKey).toBeUndefined();
    expect(result.id).toBe(saved.id);
  });
  for (const event of ["runtime_notice", "tool_call_requested"]) {
    it(`does not borrow a typed call for an unowned ${event} source`, () => {
      const source = frame("synthetic-source", event, { tool_call_id: "", id: callId });
      const live = frame("actual-call", "tool_call_requested", { id: callId });
      const entry: ConversationMessageEntry = { kind: "message", id: source.id, identity: { role: "assistant", label: "Worker" },
        variant: "rich", blocks: [{ type: "tool-call", toolCallId: callId, name: "workgraph_events", arguments: "{}", status: "success" }] };
      const [result] = assistantPresentationEntries([entry], new Map(), new Map([[source.id, source], [live.id, live]]));
      expect(result.renderKey).toBeUndefined();
      expect(result.id).toBe(source.id);
    });
  }
  it("keeps ambiguous canonical owners distinct instead of sharing a tool key", () => {
    const sources = [savedTools([callId]), savedTools([callId])];
    sources[1].id = "canonical-other-owner";
    (sources[1].data as Record<string, unknown>).assistant_message_id = "assistant-other-tool";
    const entries: ConversationMessageEntry[] = sources.map(source => ({ kind: "message", id: source.id,
      identity: { role: "assistant", label: "Worker" }, variant: "rich", blocks: [{ type: "tool-call",
        toolCallId: callId, name: "workgraph_events", arguments: "{}", status: "success" }] }));
    const rows = assistantPresentationEntries(entries, new Map(sources.map(source => [source.id, `assistant:${source.id}`])),
      new Map(sources.map(source => [source.id, source])));
    expect(new Set(rows.map(row => row.renderKey)).size).toBe(2);
    rows.forEach((row, index) => { expect(row.id).toBe(sources[index].id); expect(row.renderKey).not.toMatch(/^tool:/); });
  });
  it("preserves opaque scope and call ID bytes", () => {
    const source = frame("original-source", "tool_call_requested", { id: " call " }, { runId: " run " });
    const padded = projected(source, " call ");
    const plain = projected(frame("other-source", "tool_call_requested", { id: "call" }), "call");
    expect(padded.renderKey).toContain('" run "');
    expect(padded.renderKey).toContain('" call "');
    expect(padded.renderKey).not.toBe(plain.renderKey);
    expect(padded.id).toBe(source.id);
  });
});
