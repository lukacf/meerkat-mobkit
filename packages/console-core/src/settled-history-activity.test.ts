import assert from "node:assert/strict";
import test from "node:test";
import type { ConsoleFrame } from "./runtime-types";
import { settledHistoryActivity } from "./settled-history-activity";
import { inferResponsePhaseFromFrames, resolvePanelResponsePhase } from "./adapters";

function frame(event: string, cursor: string, extra: Partial<ConsoleFrame> = {}): ConsoleFrame {
  return { id: cursor, cursor, event, runtimeKey: "runtime", identity: "keeper", sessionId: "session",
    sourceKind: "session_history", data: {}, ...extra };
}
function snapshot(through = "console:39", cursor = "console:43", extra: Partial<ConsoleFrame> = {}): ConsoleFrame {
  return frame("assistant_history_snapshot", cursor, { data: { complete: true, session_id: "session",
    observed_through: through, assistant_message_ids: ["assistant-final"] }, ...extra });
}

test("recorded autonomous tool history is covered only after its settled observation", () => {
  const call = frame("tool_call_requested", "console:37");
  const result = frame("tool_execution_completed", "console:38");
  const answer = frame("text_complete", "console:39", { data: { message: { role: "block_assistant", stop_reason: "end_turn" } } });
  assert.equal(settledHistoryActivity([call, result, answer]).size, 0, "assistant text is not a lifecycle boundary");
  for (const rows of [[call, result, answer, snapshot()], [snapshot(), answer, result, call]]) {
    assert.deepEqual([...settledHistoryActivity(rows)].map(row => row.cursor).sort(), ["console:37", "console:38"]);
  }
});

test("a boundary cannot settle another runtime, identity or session", () => {
  const call = frame("tool_call_requested", "console:37");
  for (const extra of [{ runtimeKey: "other" }, { identity: "other" }, { sessionId: "other" },
    { runtimeKey: undefined }, { identity: undefined }, { sessionId: undefined }]) {
    assert.equal(settledHistoryActivity([call, snapshot("console:39", "console:43", extra)]).size, 0);
    assert.equal(settledHistoryActivity([{ ...call, ...extra }, snapshot()]).size, 0);
  }
});

test("live, newer and cursorless activity remains busy even with a settled history prefix", () => {
  const rows = [
    frame("tool_call_requested", "console:37", { sourceKind: "console_event" }),
    frame("tool_execution_completed", "console:40"),
    frame("reasoning_complete", "console:38", { cursor: undefined }),
    frame("run_started", "console:36"), frame("interaction_started", "console:35"),
    frame("user_input", "console:34", { status: "delivered" }),
  ];
  assert.equal(settledHistoryActivity([...rows, snapshot()]).size, 0);
});

test("partial, forged, inconsistent or invalid observations cannot clear activity", () => {
  const call = frame("tool_call_requested", "console:37");
  const valid = snapshot();
  for (const invalid of [
    { ...valid, sourceKind: "console_event" },
    { ...valid, data: { ...valid.data as object, complete: false } },
    { ...valid, data: { ...valid.data as object, session_id: "other" } },
    { ...valid, data: { ...valid.data as object, observed_through: "console:43" } },
    { ...valid, data: { ...valid.data as object, observed_through: "console:18446744073709551616" } },
    { ...valid, data: { ...valid.data as object, assistant_message_ids: ["duplicate", "duplicate"] } },
  ]) assert.equal(settledHistoryActivity([call, invalid]).size, 0);
});

test("conflicting latest observations fail closed regardless of arrival order", () => {
  const call = frame("tool_call_requested", "console:37");
  const good = snapshot();
  const conflict = snapshot("console:36");
  for (const rows of [[good, conflict], [conflict, good]]) {
    assert.equal(settledHistoryActivity([call, ...rows]).size, 0);
    assert.equal(settledHistoryActivity([call, ...rows, snapshot("console:44", "console:45")]).size, 1);
  }
  const otherIds = snapshot("console:39", "console:43", { data: { ...good.data as object, assistant_message_ids: [] } });
  assert.equal(settledHistoryActivity([call, good, otherIds]).size, 0);
});

test("latest observation wins and u64 cursor ordering does not round", () => {
  const call = frame("tool_call_requested", "console:9007199254740993");
  const older = snapshot("console:9007199254740992", "console:9007199254740994");
  const newer = snapshot("console:9007199254740993", "console:9007199254740995");
  assert.equal(settledHistoryActivity([call, older]).size, 0);
  for (const rows of [[older, newer], [newer, older]]) {
    assert.equal(settledHistoryActivity([call, ...rows]).size, 1);
  }
});

test("a settled tool-only history stops projecting work without inventing a run terminal", () => {
  const call = frame("tool_call_requested", "console:37");
  const result = frame("tool_execution_completed", "console:38");
  assert.equal(inferResponsePhaseFromFrames([call, result]), "waiting");
  assert.equal(resolvePanelResponsePhase({ frames: [call, result], serverPhase: "generating" }), "waiting");
  for (const frames of [[call, result, snapshot()], [snapshot(), call, result], [result, snapshot(), call]]) {
    assert.equal(inferResponsePhaseFromFrames(frames), null);
    assert.equal(inferResponsePhaseFromFrames(frames, "tool-executing"), "tool-executing",
      "unversioned fallback may describe work newer than the history observation");
    assert.equal(resolvePanelResponsePhase({ frames, serverPhase: "generating" }), "generating",
      "a snapshot cannot clear unversioned server activity without a retained terminal");
    assert.equal(resolvePanelResponsePhase({ frames, serverPhase: null }), null);
    assert.equal(frames.some(row => row.event === "interaction_complete" || row.event === "run_completed"), false);
  }
});

test("late covered tool history cannot reopen a settled assistant phase", () => {
  const call = frame("tool_call_requested", "console:37");
  const result = frame("tool_execution_completed", "console:38");
  const answer = frame("text_complete", "console:39", { data: { message: { role: "block_assistant", stop_reason: "end_turn" } } });
  for (const frames of [[answer, snapshot(), call, result], [snapshot(), result, answer, call]]) {
    assert.equal(inferResponsePhaseFromFrames(frames), null);
    assert.equal(resolvePanelResponsePhase({ frames, serverPhase: "waiting" }), null);
  }
});

test("settled history preserves explicit active runs and interactions before or after the observation", () => {
  const call = frame("tool_call_requested", "console:37");
  for (const event of ["run_started", "interaction_started"]) {
    for (const cursor of ["console:36", "console:44"]) {
      const active = frame(event, cursor, { sourceKind: "console_event", runId: "active-run", interactionId: "active-interaction" });
      for (const frames of [[active, call, snapshot()], [call, snapshot(), active], [snapshot(), active, call]]) {
        assert.equal(inferResponsePhaseFromFrames(frames), "waiting");
        assert.equal(resolvePanelResponsePhase({ frames, serverPhase: "generating" }), "waiting");
      }
    }
  }
});

test("uncovered activity retains its phase despite a settled history prefix", () => {
  const call = frame("tool_call_requested", "console:37");
  for (const activity of [
    frame("tool_call_requested", "console:44", { sourceKind: "console_event" }),
    frame("tool_call_requested", "console:44"),
    frame("tool_call_requested", "console:37", { sourceKind: "console_event" }),
    frame("tool_call_requested", "console:37", { sessionId: "other-session" }),
    frame("tool_call_requested", "console:37", { runtimeKey: "other-runtime" }),
    frame("tool_call_requested", "console:37", { identity: "other-identity" }),
    frame("tool_call_requested", "console:37", { cursor: undefined }),
  ]) {
    for (const frames of [[activity, call, snapshot()], [call, snapshot(), activity]]) {
      assert.equal(inferResponsePhaseFromFrames(frames), "tool-executing");
      assert.equal(resolvePanelResponsePhase({ frames, serverPhase: "waiting" }), "tool-executing");
    }
  }
});

test("absent, partial and differently scoped snapshots cannot override fallback phases", () => {
  const call = frame("tool_call_requested", "console:37");
  for (const boundary of [snapshot("console:36"), snapshot("console:39", "console:43", { sessionId: "other" }),
    snapshot("console:39", "console:43", { data: { complete: false } })]) {
    assert.equal(inferResponsePhaseFromFrames([call, boundary], "generating"), "tool-executing");
    assert.equal(resolvePanelResponsePhase({ frames: [call, boundary], serverPhase: "waiting" }), "tool-executing");
  }
  assert.equal(inferResponsePhaseFromFrames([snapshot()], "generating"), "generating");
  assert.equal(resolvePanelResponsePhase({ frames: [snapshot()], serverPhase: "generating" }), "generating",
    "a snapshot without corresponding historical activity is not a lifecycle terminal");
  assert.equal(resolvePanelResponsePhase({ frames: [call, snapshot()], hasLocalPhase: true, localPhase: "waiting" }), "waiting",
    "a locally owned active phase remains explicit authority");
});
