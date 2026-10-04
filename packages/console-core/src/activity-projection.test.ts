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

for (const interactionSeen of [false, true]) {
 for (const historyFirst of [false, true]) {
  test(`saved exact ID pairs link run ownership without reserving work (interaction seen: ${interactionSeen}, history first: ${historyFirst})`, () => {
    const projection = new ConsoleActivityProjection();
    const live = { sourceKind: "console_event" as const };
    const association = frame("tool_call_requested", { runId: "active-run", interactionId: "active-input" });
    if (historyFirst) {
      projection.fold(association);
      assert.equal(projection.busy, false);
      assert.equal(projection.terminal, false);
    }
    if (interactionSeen) projection.fold(frame("interaction_started", { ...live, interactionId: "active-input" }));
    projection.fold(frame("run_started", { ...live, runId: "active-run" }));
    projection.fold(frame("run_started", { ...live, runId: "sibling-run", interactionId: "sibling-input" }));
    projection.fold(frame("text_delta", { ...live, runId: "sibling-run", interactionId: "sibling-input" }));
    if (!historyFirst) projection.fold(association);
    assert.equal(projection.busy, true, "linking cannot release either current owner");
    assert.equal(projection.phase, "generating", "saved association cannot replace the latest live phase");
    projection.fold(frame("interaction_complete", { ...live, interactionId: "active-input" }));
    assert.equal(projection.busy, true, "unrelated sibling remains open");
    projection.fold(frame("interaction_complete", { ...live, interactionId: "sibling-input" }));
    assert.equal(projection.busy, false, "the exact pair joins the run to its actual interaction terminal");
    assert.equal(projection.phase, null);
  });
 }
}

test("saved ID pairs cannot reassign a run or link conflicting contexts", () => {
  for (const extra of [{ runtimeKey: "other-runtime" }, { identity: "other-agent" }, { sessionId: "other-session" }, { interactionId: "wrong-input" }]) {
    const projection = new ConsoleActivityProjection();
    const live = { sourceKind: "console_event" as const };
    projection.fold(frame("run_started", { ...live, runId: "active-run", interactionId: "active-input" }));
    projection.fold(frame("tool_call_requested", { runId: "active-run", interactionId: "wrong-input", ...extra }));
    projection.fold(frame("interaction_complete", { ...live, interactionId: "wrong-input" }));
    assert.equal(projection.busy, true, "a historical pair cannot reassign a known run");
    projection.fold(frame("interaction_complete", { ...live, interactionId: "active-input" }));
    assert.equal(projection.busy, false);
  }
});

test("an exact interaction may own several runs without a historical activity reservation", () => {
  const projection = new ConsoleActivityProjection();
  const live = { sourceKind: "console_event" as const };
  for (const runId of ["run-1", "run-2"]) {
    projection.fold(frame("tool_call_requested", { runId, interactionId: "same-input" }));
    projection.fold(frame("run_started", { ...live, runId }));
  }
  projection.fold(frame("run_completed", { ...live, runId: "run-1" }));
  assert.equal(projection.busy, true, "the second run remains open");
  projection.fold(frame("interaction_complete", { ...live, interactionId: "same-input" }));
  assert.equal(projection.busy, false, "the actual interaction terminal settles its runs");
});

for (const historyFirst of [false, true]) {
  for (const completedRun of ["run-1", "run-2"]) {
    test(`an associated run terminal preserves the other run's live phase (history first: ${historyFirst}, completed: ${completedRun})`, () => {
      const projection = new ConsoleActivityProjection();
      const live = { sourceKind: "console_event" as const };
      const associations = ["run-1", "run-2"].map(runId => frame("tool_call_requested", { runId, interactionId: "same-input" }));
      if (historyFirst) for (const row of associations) projection.fold(row);
      for (const runId of ["run-1", "run-2"]) projection.fold(frame("run_started", { ...live, runId }));
      projection.fold(frame("text_delta", { ...live, runId: "run-1" }));
      projection.fold(frame("tool_execution_started", { ...live, runId: "run-2" }));
      if (!historyFirst) for (const row of associations) projection.fold(row);
      assert.equal(projection.phase, "tool-executing");
      projection.fold(frame("run_completed", { ...live, runId: completedRun }));
      assert.equal(projection.busy, true);
      assert.equal(projection.runOpen, true);
      assert.equal(projection.phase, completedRun === "run-1" ? "tool-executing" : "generating");
      assert.equal(projection.terminal, false);
      projection.fold(frame("interaction_complete", { ...live, interactionId: "same-input" }));
      assert.equal(projection.busy, false);
      assert.equal(projection.phase, null);
      assert.equal(projection.terminal, true);
    });
  }
}

test("settling an associated run preserves the order of remaining interaction and sibling phases", () => {
  const projection = new ConsoleActivityProjection();
  const live = { sourceKind: "console_event" as const };
  projection.fold(frame("run_started", { ...live, runId: "run-1" }));
  projection.fold(frame("run_started", { ...live, runId: "run-2" }));
  projection.fold(frame("tool_execution_started", { ...live, interactionId: "same-input" }));
  projection.fold(frame("text_delta", { ...live, runId: "sibling-run", interactionId: "sibling-input" }));
  projection.fold(frame("text_delta", { ...live, runId: "run-2" }));
  for (const runId of ["run-1", "run-2"]) projection.fold(frame("tool_call_requested", { runId, interactionId: "same-input" }));
  projection.fold(frame("run_completed", { ...live, runId: "run-2" }));
  assert.equal(projection.phase, "generating", "a terminal cannot make an older contribution newer than the sibling");
  projection.fold(frame("interaction_complete", { ...live, interactionId: "sibling-input" }));
  assert.equal(projection.phase, "tool-executing", "the interaction's own newer auxiliary phase survives its run terminal");
  assert.equal(projection.busy, true);
  projection.fold(frame("run_completed", { ...live, runId: "run-1" }));
  assert.equal(projection.busy, true, "run terminals cannot release interaction-owned auxiliary work");
  projection.fold(frame("interaction_complete", { ...live, interactionId: "same-input" }));
  assert.equal(projection.busy, false);
  assert.equal(projection.phase, null);
});

test("saved exact ID pairs preserve supplied, live, and terminal phase evidence", () => {
  for (const event of auxiliaryEvents) {
    const saved = frame(event, { runId: "saved-run", interactionId: "saved-input" });
    const projection = new ConsoleActivityProjection("generating");
    projection.fold(saved);
    assert.equal(projection.phase, "generating", "association metadata cannot clear the supplied phase");
    assert.equal(projection.busy, false);
    assert.equal(projection.terminal, false);
    assert.equal(inferResponsePhaseFromFrames([saved], "tool-executing"), "tool-executing");
    projection.fold(frame("text_delta", { sourceKind: "console_event", runId: "live-run" }));
    projection.fold(saved);
    assert.equal(projection.phase, "generating", "an inactive pair cannot perturb a live contribution");
    projection.fold(frame("interaction_complete", { sourceKind: "console_event", runId: "live-run", interactionId: "live-input" }));
    projection.fold(saved);
    assert.equal(projection.phase, null, "the original fallback cannot return after a terminal");
    assert.equal(projection.terminal, true);
    assert.equal(projection.busy, false);
  }
});

test("a historical owner merge cannot resurrect a phase behind newer terminal evidence", () => {
  const projection = new ConsoleActivityProjection();
  const live = { sourceKind: "console_event" as const };
  projection.fold(frame("run_started", { ...live, runId: "run-1", interactionId: "same-input" }));
  projection.fold(frame("text_delta", { ...live, runId: "run-1", interactionId: "same-input" }));
  projection.fold(frame("user_input", { ...live, interactionId: "same-input", status: "completed" }));
  projection.fold(frame("tool_execution_started", { ...live, runId: "run-2" }));
  projection.fold(frame("text_complete", { ...live, runId: "run-2" }));
  assert.equal(projection.phase, null);
  assert.equal(projection.terminal, true);
  projection.fold(frame("tool_call_requested", { runId: "run-2", interactionId: "same-input" }));
  assert.equal(projection.phase, null, "association metadata must respect newer null phase evidence");
  assert.equal(projection.terminal, true);
  assert.equal(projection.busy, true, "the observed run remains open despite its cleared display phase");
});

for (const historyFirst of [false, true]) {
  test(`an observed sibling run keeps its phase without a retained run start (history first: ${historyFirst})`, () => {
    const projection = new ConsoleActivityProjection();
    const live = { sourceKind: "console_event" as const };
    const associations = ["run-1", "run-2"].map(runId => frame("tool_call_requested", { runId, interactionId: "same-input" }));
    if (historyFirst) for (const row of associations) projection.fold(row);
    projection.fold(frame("run_started", { ...live, runId: "run-1" }));
    projection.fold(frame("text_delta", { ...live, runId: "run-2" }));
    if (!historyFirst) for (const row of associations) projection.fold(row);
    projection.fold(frame("run_completed", { ...live, runId: "run-1" }));
    assert.equal(projection.phase, "generating", "the other observed run still supplies its live phase");
    assert.equal(projection.terminal, false);
    assert.equal(projection.runOpen, false, "a text observation cannot manufacture its missing run start");
    assert.equal(projection.busy, false, "phase-only evidence cannot manufacture busy ownership");
    projection.fold(frame("run_completed", { ...live, runId: "run-2" }));
    assert.equal(projection.phase, null);
    assert.equal(projection.terminal, true);
    assert.equal(projection.busy, false);
  });
}

for (const historyFirst of [false, true]) {
  test(`associated text-only runs retain independent phases when neither start is present (history first: ${historyFirst})`, () => {
    const projection = new ConsoleActivityProjection();
    const live = { sourceKind: "console_event" as const };
    const associations = ["run-2", "run-1"].map(runId => frame("tool_call_requested", { runId, interactionId: "same-input" }));
    if (historyFirst) for (const row of associations) projection.fold(row);
    projection.fold(frame("text_delta", { ...live, runId: "run-2" }));
    projection.fold(frame("text_delta", { ...live, runId: "run-1" }));
    if (!historyFirst) for (const row of associations) projection.fold(row);
    assert.equal(projection.runOpen, false);
    assert.equal(projection.busy, false);
    projection.fold(frame("run_completed", { ...live, runId: "run-1" }));
    assert.equal(projection.phase, "generating", "the other run's observed text remains current");
    assert.equal(projection.terminal, false);
    assert.equal(projection.runOpen, false);
    assert.equal(projection.busy, false);
    projection.fold(frame("run_completed", { ...live, runId: "run-2" }));
    assert.equal(projection.phase, null);
    assert.equal(projection.terminal, true);
    assert.equal(projection.busy, false);
  });
}

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
