"use strict";
const assert = require("node:assert/strict");
const { test } = require("node:test");
const { assertStoppedFrames, assertStoppedHistory, assertProofRetained, stoppedTools } = require("./real-routine-negative.cjs");

const expected = { scenarioId: "negative-unit", action: "interrupt", runId: "run-a",
  accepted: { session_id: "session-a", interaction_id: "interaction-a" } };
function frames(action = "interrupt") {
  const owner = { identity: "router:main", session_id: "session-a", interaction_id: "interaction-a", run_id: "run-a" };
  let seq = 0;
  return [{ kind: "run_started", payload: {} }, ...stoppedTools(expected.scenarioId).flatMap(tool => [
    { kind: "tool_call_requested", payload: { tool_call_id: tool.id, name: tool.name, args: tool.args } },
    ...(tool.step === "late" && action === "interrupt" ? [] : [{ kind: "tool_result_received",
      payload: { tool_call_id: tool.id, content: [{ type: "text", text: tool.result }], is_error: tool.error } }]),
  ]), { kind: "interaction_failed", payload: { reason: { kind: "cancelled" } } }]
    .map(frame => { const id = `event-${++seq}`; return { ...frame, ...owner, id, source_event_id: id, source: { kind: "console_event" } }; });
}
function history(action = "interrupt") {
  const identity = { interaction_id: "interaction-a", run_id: "run-a" };
  const messages = stoppedTools(expected.scenarioId).flatMap(tool => [
    { role: "block_assistant", identity, blocks: [{ block_type: "tool_use", data: { id: tool.id, name: tool.name, args: tool.args } }] },
    ...(tool.step === "late" && action === "interrupt" ? [] : [{ role: "tool_results", results: [{ tool_use_id: tool.id, content: [{ type: "text", text: tool.result }], is_error: tool.error }] }]),
  ]);
  return { session_id: "session-a", offset: 0, message_count: messages.length, has_more: false, messages };
}

test("stopped-run oracle distinguishes real cooperative result from absent interrupted completion", () => {
  for (const action of ["interrupt", "cancel_after_boundary"]) {
    const spec = { ...expected, action };
    const proof = assertStoppedFrames(frames(action), spec);
    assert.equal(proof.tools.at(-1).outcome, action === "interrupt" ? "unknown" : "success");
    assert.equal(assertStoppedHistory(history(action), spec).at(-1).outcome, action === "interrupt" ? "unknown" : "success");
    const wrong = frames(action); wrong.at(-1).payload.reason = { kind: "abandoned", detail: "cancelled" };
    assert.throws(() => assertStoppedFrames(wrong, spec), /typed cancellation/);
    const foreign = frames(action); foreign.at(-1).run_id = "other-run";
    assert.throws(() => assertStoppedFrames(foreign, spec), /canonical run/);
  }
});

test("unknown requires absence of authoritative completion, never a result containing unknown prose", () => {
  const bad = frames();
  const tool = stoppedTools(expected.scenarioId).at(-1);
  bad.splice(-1, 0, { ...bad[1], id: "invented", source_event_id: "invented", kind: "tool_result_received",
    payload: { tool_call_id: tool.id, is_error: false, content: [{ type: "text", text: "Cancelled, completion unknown" }] } });
  assert.throws(() => assertStoppedFrames(bad, expected), /no authoritative result/);
  const durable = history(); durable.messages.push({ role: "tool_results", results: [{ tool_use_id: tool.id, is_error: false, content: [] }] }); durable.message_count++;
  assert.throws(() => assertStoppedHistory(durable, expected), /no authoritative result/);
});

test("run cancellation accepts typed owner error reports while preserving distinct directed terminals", () => {
  const value = frames("cancel_after_boundary");
  const terminal = value.at(-1);
  value.splice(-1, 0, { ...terminal, id: "run-terminal", source_event_id: "run-terminal",
    payload: { source_event_type: "run_failed", error_report: { class: "cancelled", message: "Stopped" } } });
  assert.deepEqual(assertStoppedFrames(value, { ...expected, action: "cancel_after_boundary" }).terminalIds, ["run-terminal", terminal.id]);
  const untyped = frames(); untyped.at(-1).payload = { error_report: { class: "internal", message: "cancelled" } };
  assert.throws(() => assertStoppedFrames(untyped, expected), /typed cancellation/);
});

test("later typed cancellation carrier extends proof without losing any original terminal or tool identity", () => {
  const original = assertStoppedFrames(frames(), expected);
  const laterFrames = frames();
  laterFrames.push({ ...laterFrames.at(-1), id: "later-terminal", source_event_id: "later-terminal",
    payload: { error_report: { class: "cancelled", message: "Stopped" } } });
  const later = assertStoppedFrames(laterFrames, expected);
  assertProofRetained(original, later);
  assert.throws(() => assertProofRetained(original, { ...later, terminalIds: ["later-terminal"] }), /original typed terminal/);
  const rewired = structuredClone(later); rewired.tools[0].callId = "rewritten";
  assert.throws(() => assertProofRetained(original, rewired), /exact tool source identities/);
});

test("stop proof rejects missing or duplicate calls, mutated bytes, later work, and false ownership", () => {
  const missing = frames().filter(frame => frame.payload.tool_call_id !== stoppedTools(expected.scenarioId).at(-1).id);
  assert.throws(() => assertStoppedFrames(missing, expected), /one call/);
  const duplicate = frames(); duplicate.push({ ...duplicate[1], id: "duplicate", source_event_id: "duplicate" });
  assert.throws(() => assertStoppedFrames(duplicate, expected), /one call/);
  const altered = frames(); altered[2].payload.content[0].text = altered[2].payload.content[0].text.trim();
  assert.throws(() => assertStoppedFrames(altered, expected), /exact result/);
  const continued = frames(); continued.push({ ...continued[1], id: "after", source_event_id: "after", payload: { tool_call_id: `fixture-${expected.scenarioId}-ready` } });
  assert.throws(() => assertStoppedFrames(continued, expected), /no later tool/);
  const rewired = history(); rewired.messages[0].identity = { interaction_id: "foreign", run_id: expected.runId };
  assert.throws(() => assertStoppedHistory(rewired, expected), /canonical interaction/);
  const lost = history(); lost.messages.pop(); lost.message_count--;
  assert.throws(() => assertStoppedHistory(lost, expected), /one call/);
  assert.throws(() => assertStoppedHistory({ ...history(), has_more: true }, expected), /complete owner history/);
});
