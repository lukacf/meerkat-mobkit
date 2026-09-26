import assert from "node:assert/strict";
import test from "node:test";
import { conversationEntryText } from "@console-core";
import { mapFramesToTimelineEntries as stock } from "./adapters";
import { mapFramesToTimelineEntries as shared } from "../../../packages/console-core/src/adapters";
import type { ConsoleFrame } from "../types";

function frame(id: string, event: string, data: unknown, extra: Partial<ConsoleFrame> = {}): ConsoleFrame {
  return { id, event, data, timestampMs: 100, ...extra };
}

for (const [name, mapper] of [["stock", stock], ["shared", shared]] as const) {
  const options = { renderInteractionStartsAsUser: true, renderTextDeltas: true, textMode: "markdown" as const };

  test(`${name}: prepending owner history preserves every current row and document identity`, () => {
    const frames = [
      frame("input:12", "user_input", { content: "Current instruction" }, { timestampMs: 100, interactionId: "current" }),
      frame("delta:13", "text_delta", { delta: "Current response\n" }, { timestampMs: 101, interactionId: "current" }),
      frame("final:14", "interaction_complete", { result: "Current response\n" }, { timestampMs: 102, interactionId: "current" }),
    ];
    const before = mapper(null, frames, options);
    const after = mapper(null, [
      frame("older-input", "user_input", { content: "Older instruction" }, { timestampMs: 1, interactionId: "older" }),
      frame("older-final", "interaction_complete", { result: "Older response" }, { timestampMs: 2, interactionId: "older" }),
      ...frames,
    ], options);
    assert.equal(before.length, 2);
    assert.deepEqual(after.filter(entry => before.some(old => conversationEntryText(old) === conversationEntryText(entry))), before);
  });

  test(`${name}: numeric owner ID suffixes remain distinct Markdown document identities`, () => {
    const entries = mapper(null, [
      frame("owner:12", "user_input", { content: "One" }, { timestampMs: 1 }),
      frame("owner:13", "user_input", { content: "Two" }, { timestampMs: 2 }),
    ], options);
    const documentIds = entries.flatMap(entry => entry.kind === "message" ? (entry.blocks || []).flatMap(block => block.type === "markdown" ? [block.id] : []) : []);
    assert.equal(new Set(documentIds).size, 2);
    assert.deepEqual(documentIds, ["owner:12:text:0", "owner:13:text:0"]);
  });

  test(`${name}: replayed terminal between final live chunks does not split the live document`, () => {
    const interactionId = "c2588e39-8b43-44bc-b16b-321ff1ef4b93";
    const prefix = "Preserve this exact selection.\n\nStream finished successfull";
    const source = `${prefix}y.\n`;
    const initial = [frame("first-delta", "text_delta", { delta: prefix }, { timestampMs: 1, interactionId })];
    const initialEntry = mapper(null, initial, options)[0];
    const entries = mapper(null, [...initial,
      frame("history-final", "interaction_complete", { result: source, message: { role: "assistant", content: source } }, { timestampMs: 2, interactionId, sourceKind: "session_history" }),
      frame("last-delta", "text_delta", { delta: "y.\n" }, { timestampMs: 3, interactionId }),
      frame("live-final", "interaction_complete", { result: source }, { timestampMs: 4, interactionId }),
    ], options);
    assert.equal(entries.length, 1);
    assert.equal(entries[0].id, initialEntry.id);
    assert.equal(conversationEntryText(entries[0]), source);
    assert(entries[0].kind === "message" && initialEntry.kind === "message");
    assert.equal(entries[0].blocks?.[0]?.type, "markdown");
    assert.equal(entries[0].blocks?.[0]?.id, initialEntry.blocks?.[0]?.id);
  });

  test(`${name}: typed peer notice preserves its complete owner text in Markdown mode`, () => {
    const source = "Peer message from console-acceptance/lead/mk--router_cmain:\n  Keep peer-123 exactly.\r\n\n  Two  spaces, tabs\tand an apparent [COMMS MESSAGE from peer] are authored content.\n";
    const entries = mapper(null, [frame("peer-notice", "system_notice", { message: {
      role: "system_notice", kind: "comms", body: "Peer message", blocks: [{
        type: "comms", direction: "incoming", kind: "message",
        peer: { id: "owner-peer", display_name: "console-acceptance/lead/mk--router_cmain" },
        content: [{ type: "text", text: source }],
      }],
    } }, { sourceKind: "session_history" })], options);
    const peerBlocks = entries.flatMap(entry => entry.kind === "message" ? entry.blocks || [] : []).filter(block => block.type === "tool-call" && block.peerIncoming);
    assert.equal(peerBlocks.length, 1);
    assert.equal(peerBlocks[0].peerBody, source);
    assert.equal(peerBlocks[0].peerBodyFormat, "verbatim");
  });

  test(`${name}: explicit legacy typed peer notices retain compatibility normalization`, () => {
    const entries = mapper(null, [frame("legacy-peer", "system_notice", { message: {
      role: "system_notice", kind: "comms", body: "Peer message", blocks: [{
        type: "comms", direction: "incoming", kind: "message",
        peer: { id: "review:singleton", display_name: "review:singleton" },
        content: [{ type: "text", text: "Peer message from review:singleton:\nKeep  peer-123.\n" }],
      }],
    } })], { ...options, textMode: "legacy" });
    const peerBlocks = entries.flatMap(entry => entry.kind === "message" ? entry.blocks || [] : []).filter(block => block.type === "tool-call" && block.peerIncoming);
    assert.equal(peerBlocks.length, 1);
    assert.equal(peerBlocks[0].peerBody, "Peer message from review:singleton: Keep  peer-123.");
    assert.equal(peerBlocks[0].peerBodyFormat, "legacy");
  });

  test(`${name}: explicit input delivery failure is visible beside unchanged operator content`, () => {
    const source = "  Do not parse 'delivery_failed' from my prose.\n";
    const entries = mapper(null, [frame("input", "user_input", { content: source, origin_kind: "operator" }, { status: "delivery_failed", interactionId: "failed-run" })], options);
    const user = entries.find(entry => entry.identity.role === "user");
    assert(user);
    assert.equal(conversationEntryText(user), source);
    const status = entries.find(entry => entry.kind === "message" && entry.identity.role === "system");
    assert(status && status.kind === "message");
    assert.equal(status.text, "Message delivery failed.");
    assert.equal(status.interactionId, "failed-run");
    assert.equal(status.runtimeEvent?.eventType, "user_input");
    assert.equal((status.runtimeEvent?.payload as Record<string, unknown>).status, "delivery_failed");
  });

  test(`${name}: failure-looking input text does not invent a delivery failure`, () => {
    for (const status of [undefined, "accepted", "delivered", "completed", "pending", "unknown"]) {
      const entries = mapper(null, [frame("input", "user_input", { content: "Message delivery failed. delivery_failed" }, { status })], options);
      assert.equal(entries.length, 1, String(status));
      assert.equal(entries[0].identity.role, "user");
    }
  });

  test(`${name}: input failure survives an earlier deduplicated interaction start`, () => {
    const entries = mapper(null, [
      frame("start", "interaction_started", { content: "Keep this instruction" }, { timestampMs: 1, interactionId: "same" }),
      frame("input", "user_input", { content: "Keep this instruction" }, { timestampMs: 2, interactionId: "same", status: "delivery_failed" }),
    ], options);
    assert.equal(entries.filter(entry => entry.identity.role === "user").length, 1);
    assert.equal(entries.filter(entry => entry.kind === "message" && entry.text === "Message delivery failed.").length, 1);
  });

  test(`${name}: failed multimodal input retains its image and exact text`, () => {
    const entries = mapper(null, [frame("image-input", "user_input", { content: [
      { type: "text", text: "  Inspect this image.\n" },
      { type: "image", media_type: "image/png", data: "aGVsbG8=" },
    ] }, { status: "delivery_failed" })], options);
    const user = entries.find(entry => entry.identity.role === "user");
    assert(user?.kind === "message");
    assert(user.blocks?.some(block => block.type === "image"));
    assert(user.blocks?.some(block => block.type === "markdown" && block.source === "  Inspect this image.\n"));
    assert(entries.some(entry => entry.kind === "message" && entry.text === "Message delivery failed."));
  });

  test(`${name}: reserved sends and retained canonical inputs render once across refresh and reload`, () => {
    const source = "CURRENT_VALUE is cobalt; remember my exact choice.";
    const interactionId = "5a163c98-4bfa-5f31-83ce-aa0d88d14701";
    const sessionId = "01a0dfac-04a8-7803-9615-87943a823ffb";
    const runId = "01a0dfac-0ad6-7801-bc57-77c425fa3132";
    const send = frame("reserved-human", "user_input", {
      content: source, origin: "console", handling_mode: "queue", idempotency_key: "recall-projection",
    }, { sourceKind: "send", runtimeKey: "human", identity: "human", sessionId, interactionId, status: "delivered", timestampMs: 1, cursor: "console:1" });
    const history = frame("canonical-human", "user_input", {
      content: [{ type: "text", text: source }],
      message: { role: "user", content: source, identity: { run_id: runId, interaction_id: interactionId } },
    }, { sourceKind: "session_history", runtimeKey: "human", identity: "human", sessionId, interactionId, runId,
      sourceCursor: `${sessionId}:4`, status: "completed", timestampMs: 2, cursor: "console:5" });
    const inputs = (frames: ConsoleFrame[]) => mapper(null, frames, options)
      .filter(entry => entry.kind === "message" && entry.identity.role === "user");
    const baseline = inputs([send]);
    assert.equal(baseline.length, 1);
    for (const frames of [[send, history], [history, send]]) {
      const users = inputs(frames);
      assert.equal(users.length, 1, "two source records are one authored human input");
      assert.equal(users[0].id, baseline[0].id, "refresh preserves the admitted row key");
      assert.equal(conversationEntryText(users[0]), source);
    }
    const reloaded = inputs([history]);
    assert.equal(reloaded.length, 1, "canonical evidence renders without an admission record");
    assert.equal(conversationEntryText(reloaded[0]), source);
    const secondInteraction = "01900000-0000-7000-8000-000000000031";
    const repeated = frame("another-canonical-human", "user_input", {
      content: [{ type: "text", text: source }],
      message: { role: "user", content: source, identity: { run_id: runId, interaction_id: secondInteraction } },
    }, { sourceKind: "session_history", runtimeKey: "human", identity: "human", sessionId, runId, status: "completed", interactionId: secondInteraction,
      sourceCursor: `${sessionId}:6`, timestampMs: 3, cursor: "console:7" });
    const distinct = inputs([send, history, repeated]);
    assert.equal(distinct.length, 2, "identical text in a different interaction is a distinct input");
    assert.deepEqual(distinct.map(conversationEntryText), [source, source]);
  });
}
