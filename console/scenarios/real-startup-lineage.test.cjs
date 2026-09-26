"use strict";
const assert = require("node:assert/strict");
const { test } = require("node:test");
const { assertStartupLineage, assertStartupRendering, source } = require("./real-startup-lineage.cjs");

function runFrames(runId, interactionId) {
  const identity = { run_id: runId, ...(interactionId ? { interaction_id: interactionId } : {}) };
  const frame = (suffix, kind, payload, history = false) => ({
    id: `${runId}:${suffix}`, kind, run_id: runId, interaction_id: interactionId,
    runtime_key: "default", identity: "router:main", session_id: "session",
    source: { kind: history ? "session_history" : "console_event" },
    ...(!history ? { source_event_id: `${runId}:${suffix}` } : {}), payload: structuredClone(payload),
  });
  return [
    frame("start", "run_started", { identity }),
    frame("delta", "text_delta", { delta: source, identity }),
    frame("history", "interaction_complete", { result: source, message: { identity } }, true),
    frame("complete", "interaction_complete", { result: source, identity }),
  ];
}

test("startup lineage oracle requires exact live/history runtime owners for every equal reply", () => {
  const frames = [...runFrames("run-a"), ...runFrames("run-b", "interaction-b")];
  assert.equal(assertStartupLineage(frames).length, 2);
  const badHistory = structuredClone(frames);
  badHistory[2].payload.message.identity.run_id = "run-b";
  assert.throws(() => assertStartupLineage(badHistory), /persisted owner/);
  const missingRun = structuredClone(frames);
  delete missingRun[3].run_id;
  assert.throws(() => assertStartupLineage(missingRun), /canonical run/);
  assert.throws(() => assertStartupLineage(frames.filter(frame => frame.id !== "run-a:delta")), /exact source deltas/);
});

test("startup durability requires actual committed owner messages even when timeline counterparts are pruned", () => {
  const frames = [...runFrames("run-a"), ...runFrames("run-b", "interaction-b")].filter(frame => frame.source.kind !== "session_history");
  const messages = [
    { role: "block_assistant", identity: { run_id: "run-a" }, blocks: [{ block_type: "text", data: { text: source } }] },
    { role: "block_assistant", identity: { run_id: "run-b", interaction_id: "interaction-b" }, blocks: [{ block_type: "text", data: { text: source } }] },
  ];
  const page = { session_id: "session", offset: 0, has_more: false, message_count: 2, messages };
  assert.deepEqual(assertStartupLineage(frames, [page]).map(owner => [owner.runId, owner.historyOffset]), [["run-a", 0], ["run-b", 1]]);
  assert.throws(() => assertStartupLineage(frames), /one persisted reply/);
  assert.throws(() => assertStartupLineage(frames, [{ ...page, session_id: "other-session" }]), /actual durable session/);
  assert.throws(() => assertStartupLineage(frames, [{ ...page, has_more: true }]), /complete durable transcript/);
  assert.throws(() => assertStartupLineage(frames, [{ ...page, message_count: 3 }]), /all committed messages/);
  const foreign = structuredClone(page); foreign.messages[1].identity.run_id = "foreign-run";
  assert.throws(() => assertStartupLineage(frames, [foreign]), /exactly one actual committed reply/);
  const changed = structuredClone(page); changed.messages[1].blocks[0].data.text += " |";
  assert.throws(() => assertStartupLineage(frames, [changed]), /exactly one actual committed reply/);
  const wrongInteraction = structuredClone(page); wrongInteraction.messages[1].identity.interaction_id = "foreign-interaction";
  assert.throws(() => assertStartupLineage(frames, [wrongInteraction]), /actual persisted interaction owner/);
});

test("startup rendering oracle rejects partial duplicates, missing equal-run replies, and reused owners", () => {
  const owners = assertStartupLineage([...runFrames("run-a"), ...runFrames("run-b", "interaction-b")]);
  const rendered = { rowIds: ["row-a", "row-b"], quotes: [
    { id: "run-a:delta", source }, { id: "run-b:delta", source },
  ], tables: 2 };
  assert.deepEqual(assertStartupRendering(rendered, owners).sort(), ["run-a", "run-b"]);
  assert.throws(() => assertStartupRendering({ ...rendered, quotes: rendered.quotes.slice(0, 1) }, owners), /one complete/);
  assert.throws(() => assertStartupRendering({ ...rendered, quotes: [...rendered.quotes, { id: "tail", source: " |\n" }] }, owners), /partial/);
  assert.throws(() => assertStartupRendering({ ...rendered, quotes: [rendered.quotes[0], { ...rendered.quotes[0], id: "run-a:history" }] }, owners), /one rendered owner/);
  assert.throws(() => assertStartupRendering({ ...rendered, tables: 1 }, owners), /complete Markdown table/);
});
