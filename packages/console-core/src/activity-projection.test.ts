import assert from "node:assert/strict";
import test from "node:test";
import type { ConsoleFrame } from "./runtime-types";
import { ConsoleActivityProjection, inferResponsePhaseFromFrames, resolvePanelResponsePhase } from "./adapters";

const scope = { runtimeKey: "runtime", identity: "keeper", sessionId: "session" };
function frame(event: string, extra: Partial<ConsoleFrame> = {}): ConsoleFrame {
  return { ...scope, id: event, cursor: "console:40", event, sourceKind: "session_history", data: {}, ...extra };
}
const snapshot = frame("assistant_history_snapshot", { cursor: "console:43", data: {
  complete: true, session_id: "session", observed_through: "console:39", assistant_message_ids: ["assistant-final"],
} });
const auxiliaryEvents = ["reasoning_delta", "reasoning_complete", "tool_call_requested", "tool_call",
  "tool_execution_started", "tool_result_received", "tool_execution_completed", "server_tool_content"];

test("saved auxiliary observations do not reserve current work, regardless of snapshot coverage", () => {
  for (const event of auxiliaryEvents) {
    for (const extra of [{}, { cursor: "console:38" }, { cursor: undefined }, { runId: "old-run", interactionId: "old-input" }]) {
      const saved = frame(event, { data: { content: { type: "response.web_search_call.in_progress" } }, ...extra });
      for (const rows of [[saved], [snapshot, saved], [saved, snapshot]]) {
        const projection = new ConsoleActivityProjection();
        for (const row of rows) projection.fold(row);
        assert.equal(projection.busy, false, `${event} is transcript evidence, not a run reservation`);
        assert.equal(projection.phase, null);
        assert.equal(projection.terminal, false, "history does not manufacture a terminal either");
      }
    }
  }
});

test("saved auxiliary observations preserve current owner and release only on its terminal", () => {
  const start = frame("run_started", { sourceKind: "console_event", runId: "active-run", interactionId: "active-input" });
  const terminal = frame("interaction_complete", { sourceKind: "console_event", runId: "active-run", interactionId: "active-input" });
  for (const event of auxiliaryEvents) {
    for (const extra of [{}, { runId: "active-run", interactionId: "active-input" }, { sessionId: "other" }]) {
      const saved = frame(event, extra);
      for (const prefix of [[saved, start, snapshot], [start, snapshot, saved]]) {
        const projection = new ConsoleActivityProjection();
        for (const row of prefix) projection.fold(row);
        assert.equal(projection.busy, true);
        assert.equal(projection.runOpen, true);
        assert.equal(projection.phase, "waiting");
        projection.fold(terminal);
        projection.fold(saved);
        assert.equal(projection.busy, false, "late history cannot reopen the settled owner");
        assert.equal(projection.phase, null);
        assert.equal(projection.terminal, true);
      }
    }
  }
});

test("explicit lifecycle observations remain active even when read from history", () => {
  for (const event of ["run_started", "interaction_started"]) {
    const start = frame(event, { runId: "active-run", interactionId: "active-input" });
    const projection = new ConsoleActivityProjection();
    projection.fold(start);
    projection.fold(snapshot);
    projection.fold(frame("tool_execution_completed"));
    assert.equal(projection.busy, true);
    projection.fold(frame("interaction_complete", { runId: "other-run", interactionId: "other-input" }));
    assert.equal(projection.busy, true, "a sibling terminal cannot settle the active owner");
    projection.fold(frame("interaction_complete", { runId: "active-run", interactionId: "active-input" }));
    assert.equal(projection.busy, false);
  }
});

test("live tool-only work remains busy through text completion", () => {
  for (const sourceKind of ["console_event", undefined]) {
    const current = { sourceKind, runId: "active-run", interactionId: "active-input" };
    const projection = new ConsoleActivityProjection();
    projection.fold(frame("tool_execution_completed", current));
    projection.fold(snapshot);
    projection.fold(frame("tool_execution_completed"));
    assert.equal(projection.busy, true);
    projection.fold(frame("text_complete", current));
    assert.equal(projection.busy, true);
    projection.fold(frame("turn_completed", { ...current, data: { stop_reason: "end_turn" } }));
    assert.equal(projection.busy, false);
  }
});

test("history alone cannot override supplied current phases", () => {
  for (const event of auxiliaryEvents) {
    const saved = frame(event);
    for (const rows of [[saved], [saved, snapshot], [snapshot, saved]]) {
      assert.equal(inferResponsePhaseFromFrames(rows), null);
      assert.equal(inferResponsePhaseFromFrames(rows, "generating"), "generating");
      assert.equal(resolvePanelResponsePhase({ frames: rows, serverPhase: "waiting" }), "waiting");
      assert.equal(resolvePanelResponsePhase({ frames: rows, hasLocalPhase: true, localPhase: "tool-executing" }), "tool-executing");
    }
  }
});

test("late saved activity cannot override an explicit terminal phase", () => {
  const terminal = frame("interaction_complete", { sourceKind: "console_event", runId: "current-run", interactionId: "current-input" });
  for (const rows of [[terminal, frame("tool_execution_completed")], [frame("tool_execution_completed"), terminal]]) {
    assert.equal(inferResponsePhaseFromFrames(rows, "generating"), null);
    assert.equal(resolvePanelResponsePhase({ frames: rows, serverPhase: "waiting" }), null);
  }
});
