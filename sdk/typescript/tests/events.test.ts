import { describe, it } from "node:test";
import assert from "node:assert/strict";

import {
  parseAgentEvent,
  parseMobEventFromSse,
  parseAgentEventFromSse,
  isTextDelta,
  isTextComplete,
  isRunCompleted,
  isRunFailed,
  isTurnCompleted,
  isToolCallRequested,
} from "../dist/index.js";

// ---------------------------------------------------------------------------
// parseAgentEvent — all 11 known event types + unknown
// ---------------------------------------------------------------------------

describe("parseAgentEvent", () => {
  it("parses run_started", () => {
    const event = parseAgentEvent({
      type: "run_started",
      session_id: "sess-1",
      prompt: "hello world",
    });
    assert.equal(event.type, "run_started");
    assert.equal((event as any).sessionId, "sess-1");
    assert.equal((event as any).prompt, "hello world");
  });

  it("parses run_completed", () => {
    const event = parseAgentEvent({
      type: "run_completed",
      session_id: "sess-2",
      result: "done",
    });
    assert.equal(event.type, "run_completed");
    assert.equal((event as any).sessionId, "sess-2");
    assert.equal((event as any).result, "done");
  });

  it("parses run_failed", () => {
    const event = parseAgentEvent({
      type: "run_failed",
      session_id: "sess-3",
      error: "boom",
    });
    assert.equal(event.type, "run_failed");
    assert.equal((event as any).sessionId, "sess-3");
    assert.equal((event as any).error, "boom");
  });

  it("parses turn_started", () => {
    const event = parseAgentEvent({
      type: "turn_started",
      turn_number: 5,
    });
    assert.equal(event.type, "turn_started");
    assert.equal((event as any).turnNumber, 5);
  });

  it("parses text_delta", () => {
    const event = parseAgentEvent({
      type: "text_delta",
      delta: "chunk",
    });
    assert.equal(event.type, "text_delta");
    assert.equal((event as any).delta, "chunk");
  });

  it("parses text_complete", () => {
    const event = parseAgentEvent({
      type: "text_complete",
      content: "full text",
    });
    assert.equal(event.type, "text_complete");
    assert.equal((event as any).content, "full text");
  });

  it("parses tool_call_requested", () => {
    const args = { key: "value" };
    const event = parseAgentEvent({
      type: "tool_call_requested",
      id: "tc-1",
      name: "search",
      args,
    });
    assert.equal(event.type, "tool_call_requested");
    assert.equal((event as any).id, "tc-1");
    assert.equal((event as any).name, "search");
    assert.deepEqual((event as any).args, args);
  });

  it("parses tool_result_received", () => {
    const event = parseAgentEvent({
      type: "tool_result_received",
      id: "tr-1",
      name: "search",
      is_error: true,
    });
    assert.equal(event.type, "tool_result_received");
    assert.equal((event as any).id, "tr-1");
    assert.equal((event as any).name, "search");
    assert.equal((event as any).isError, true);
  });

  it("parses tool_result_received with is_error false", () => {
    const event = parseAgentEvent({
      type: "tool_result_received",
      id: "tr-2",
      name: "calc",
      is_error: false,
    });
    assert.equal((event as any).isError, false);
  });

  it("parses turn_completed", () => {
    const event = parseAgentEvent({
      type: "turn_completed",
      stop_reason: "end_turn",
    });
    assert.equal(event.type, "turn_completed");
    assert.equal((event as any).stopReason, "end_turn");
  });

  it("parses tool_execution_started", () => {
    const event = parseAgentEvent({
      type: "tool_execution_started",
      id: "tex-1",
      name: "bash",
    });
    assert.equal(event.type, "tool_execution_started");
    assert.equal((event as any).id, "tex-1");
    assert.equal((event as any).name, "bash");
  });

  it("parses tool_execution_completed", () => {
    const event = parseAgentEvent({
      type: "tool_execution_completed",
      id: "tex-2",
      name: "bash",
      result: "ok",
      is_error: false,
      duration_ms: 120,
    });
    assert.equal(event.type, "tool_execution_completed");
    assert.equal((event as any).id, "tex-2");
    assert.equal((event as any).name, "bash");
    assert.equal((event as any).result, "ok");
    assert.equal((event as any).isError, false);
    assert.equal((event as any).durationMs, 120);
  });

  it("returns UnknownEvent for unrecognized type", () => {
    const raw = { type: "custom_event", foo: "bar" };
    const event = parseAgentEvent(raw);
    assert.equal(event.type, "custom_event");
    assert.deepEqual((event as any).data, raw);
  });

  it("handles missing fields with defaults", () => {
    const event = parseAgentEvent({ type: "run_started" });
    assert.equal(event.type, "run_started");
    assert.equal((event as any).sessionId, "");
    assert.equal((event as any).prompt, "");
  });

  it("defaults turn_number to 0 when missing", () => {
    const event = parseAgentEvent({ type: "turn_started" });
    assert.equal((event as any).turnNumber, 0);
  });

  it("defaults tool_execution_completed duration_ms to 0", () => {
    const event = parseAgentEvent({ type: "tool_execution_completed" });
    assert.equal((event as any).durationMs, 0);
  });

  it("defaults tool_call_requested args to null when missing", () => {
    const event = parseAgentEvent({ type: "tool_call_requested" });
    assert.equal((event as any).args, null);
  });

  it("treats missing type as empty string → unknown event", () => {
    const raw = { foo: "bar" };
    const event = parseAgentEvent(raw);
    assert.equal(event.type, "");
    assert.deepEqual((event as any).data, raw);
  });
});

// ---------------------------------------------------------------------------
// Type guards
// ---------------------------------------------------------------------------

describe("type guards", () => {
  it("isTextDelta returns true for text_delta", () => {
    const event = parseAgentEvent({ type: "text_delta", delta: "x" });
    assert.equal(isTextDelta(event), true);
  });

  it("isTextDelta returns false for other types", () => {
    const event = parseAgentEvent({ type: "run_completed", session_id: "s", result: "r" });
    assert.equal(isTextDelta(event), false);
  });

  it("isTextComplete returns true for text_complete", () => {
    const event = parseAgentEvent({ type: "text_complete", content: "c" });
    assert.equal(isTextComplete(event), true);
  });

  it("isTextComplete returns false for other types", () => {
    const event = parseAgentEvent({ type: "text_delta", delta: "x" });
    assert.equal(isTextComplete(event), false);
  });

  it("isRunCompleted returns true for run_completed", () => {
    const event = parseAgentEvent({ type: "run_completed", session_id: "s", result: "r" });
    assert.equal(isRunCompleted(event), true);
  });

  it("isRunCompleted returns false for run_started", () => {
    const event = parseAgentEvent({ type: "run_started", session_id: "s", prompt: "p" });
    assert.equal(isRunCompleted(event), false);
  });

  it("isRunFailed returns true for run_failed", () => {
    const event = parseAgentEvent({ type: "run_failed", session_id: "s", error: "e" });
    assert.equal(isRunFailed(event), true);
  });

  it("isRunFailed returns false for run_completed", () => {
    const event = parseAgentEvent({ type: "run_completed", session_id: "s", result: "r" });
    assert.equal(isRunFailed(event), false);
  });

  it("isTurnCompleted returns true for turn_completed", () => {
    const event = parseAgentEvent({ type: "turn_completed", stop_reason: "end" });
    assert.equal(isTurnCompleted(event), true);
  });

  it("isTurnCompleted returns false for turn_started", () => {
    const event = parseAgentEvent({ type: "turn_started", turn_number: 1 });
    assert.equal(isTurnCompleted(event), false);
  });

  it("isToolCallRequested returns true for tool_call_requested", () => {
    const event = parseAgentEvent({ type: "tool_call_requested", id: "1", name: "t", args: {} });
    assert.equal(isToolCallRequested(event), true);
  });

  it("isToolCallRequested returns false for tool_result_received", () => {
    const event = parseAgentEvent({ type: "tool_result_received", id: "1", name: "t", is_error: false });
    assert.equal(isToolCallRequested(event), false);
  });
});

// ---------------------------------------------------------------------------
// parseMobEventFromSse
// ---------------------------------------------------------------------------

describe("parseMobEventFromSse", () => {
  it("parses an attributed mob event from SSE", () => {
    const sse = {
      id: null,
      event: "message",
      data: JSON.stringify({
        member_id: "agent-1",
        timestamp_ms: 1700000000000,
        payload: { type: "text_delta", delta: "hello" },
      }),
    };
    const envelope = parseMobEventFromSse(sse);
    assert.equal(envelope.memberId, "agent-1");
    assert.equal(envelope.timestampMs, 1700000000000);
    assert.equal(envelope.event.type, "text_delta");
    assert.equal((envelope.event as any).delta, "hello");
  });

  it("falls back to source when member_id is absent", () => {
    const sse = {
      id: null,
      event: "message",
      data: JSON.stringify({
        source: "agent-2",
        payload: { type: "run_completed", session_id: "s", result: "ok" },
      }),
    };
    const envelope = parseMobEventFromSse(sse);
    assert.equal(envelope.memberId, "agent-2");
  });

  it("uses raw as payload if payload is not an object", () => {
    const sse = {
      id: null,
      event: "message",
      data: JSON.stringify({
        member_id: "a-3",
        type: "text_delta",
        delta: "hi",
      }),
    };
    const envelope = parseMobEventFromSse(sse);
    assert.equal(envelope.event.type, "text_delta");
    assert.equal((envelope.event as any).delta, "hi");
  });

  it("handles invalid JSON gracefully", () => {
    const sse = { id: null, event: "message", data: "not json" };
    const envelope = parseMobEventFromSse(sse);
    assert.equal(envelope.memberId, "");
    assert.equal(envelope.timestampMs, 0);
    // The event is an unknown event since raw is {}
    assert.equal(envelope.event.type, "");
  });

  it("defaults timestampMs to 0 when absent", () => {
    const sse = {
      id: null,
      event: "message",
      data: JSON.stringify({
        member_id: "a-4",
        payload: { type: "turn_started", turn_number: 1 },
      }),
    };
    const envelope = parseMobEventFromSse(sse);
    assert.equal(envelope.timestampMs, 0);
  });
});

// ---------------------------------------------------------------------------
// parseAgentEventFromSse
// ---------------------------------------------------------------------------

describe("parseAgentEventFromSse", () => {
  it("parses a per-agent SSE event", () => {
    const sse = {
      id: "evt-1",
      event: "text_delta",
      data: JSON.stringify({ type: "text_delta", delta: "chunk" }),
    };
    const result = parseAgentEventFromSse(sse);
    assert.equal(result.eventType, "text_delta");
    assert.equal(result.event.type, "text_delta");
    assert.equal((result.event as any).delta, "chunk");
  });

  it("preserves SSE event field as eventType", () => {
    const sse = {
      id: null,
      event: "custom_sse_event",
      data: JSON.stringify({ type: "run_started", session_id: "s1", prompt: "p" }),
    };
    const result = parseAgentEventFromSse(sse);
    assert.equal(result.eventType, "custom_sse_event");
    assert.equal(result.event.type, "run_started");
  });

  it("handles invalid JSON gracefully", () => {
    const sse = { id: null, event: "message", data: "broken" };
    const result = parseAgentEventFromSse(sse);
    assert.equal(result.eventType, "message");
    assert.equal(result.event.type, "");
  });
});
