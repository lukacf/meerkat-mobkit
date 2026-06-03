import test from "node:test";
import assert from "node:assert/strict";

import {
  DEFAULT_CONSOLE_FETCH_TIMEOUT_MS,
  fetchJson,
  parseSseFrames,
  queryTimeline,
  sendConsole,
  sendConsoleMultipart,
  subscribeTimelineEvents,
  uploadConsoleBlobMultipart,
} from "./network";
import { mapFramesToTimelineEntries } from "./adapters";
import type { ConsoleFrame } from "../types";

test("fetchJson defaults console requests to a 60 second timeout with an abort reason", async () => {
  const originalFetch = globalThis.fetch;
  const originalSetTimeout = globalThis.setTimeout;
  const originalClearTimeout = globalThis.clearTimeout;
  let scheduledMs: number | undefined;
  let cleared = false;

  globalThis.setTimeout = ((handler: TimerHandler, timeout?: number) => {
    scheduledMs = timeout;
    return originalSetTimeout(handler, 0);
  }) as typeof setTimeout;
  globalThis.clearTimeout = ((handle?: number) => {
    cleared = true;
    return originalClearTimeout(handle);
  }) as typeof clearTimeout;
  globalThis.fetch = (async (_input, init) => {
    const signal = init?.signal;
    return new Promise<Response>((_resolve, reject) => {
      signal?.addEventListener("abort", () => reject(signal.reason));
    });
  }) as typeof fetch;

  try {
    await assert.rejects(
      fetchJson("http://127.0.0.1:7000", "/console/experience"),
      /console fetch timeout after 60 s/,
    );
    assert.equal(scheduledMs, DEFAULT_CONSOLE_FETCH_TIMEOUT_MS);
    assert.equal(cleared, true);
  } finally {
    globalThis.fetch = originalFetch;
    globalThis.setTimeout = originalSetTimeout;
    globalThis.clearTimeout = originalClearTimeout;
  }
});

test("console RPC requests use the configured timeout with an abort reason", async () => {
  const originalFetch = globalThis.fetch;
  const originalSetTimeout = globalThis.setTimeout;
  const originalClearTimeout = globalThis.clearTimeout;
  let scheduledMs: number | undefined;
  let cleared = false;

  globalThis.setTimeout = ((handler: TimerHandler, timeout?: number) => {
    scheduledMs = timeout;
    return originalSetTimeout(handler, 0);
  }) as typeof setTimeout;
  globalThis.clearTimeout = ((handle?: number) => {
    cleared = true;
    return originalClearTimeout(handle);
  }) as typeof clearTimeout;
  globalThis.fetch = (async (_input, init) => {
    const signal = init?.signal;
    return new Promise<Response>((_resolve, reject) => {
      signal?.addEventListener("abort", () => reject(signal.reason));
    });
  }) as typeof fetch;

  try {
    await assert.rejects(
      queryTimeline("http://127.0.0.1:7000", { identity: "agent-a" }, 10, 25),
      /console rpc timeout after 25 ms/,
    );
    assert.equal(scheduledMs, 25);
    assert.equal(cleared, true);
  } finally {
    globalThis.fetch = originalFetch;
    globalThis.setTimeout = originalSetTimeout;
    globalThis.clearTimeout = originalClearTimeout;
  }
});

test("console multipart requests use the configured timeout with an abort reason", async () => {
  const originalFetch = globalThis.fetch;
  const originalSetTimeout = globalThis.setTimeout;
  const originalClearTimeout = globalThis.clearTimeout;
  let scheduledMs: number | undefined;
  let cleared = false;

  globalThis.setTimeout = ((handler: TimerHandler, timeout?: number) => {
    scheduledMs = timeout;
    return originalSetTimeout(handler, 0);
  }) as typeof setTimeout;
  globalThis.clearTimeout = ((handle?: number) => {
    cleared = true;
    return originalClearTimeout(handle);
  }) as typeof clearTimeout;
  globalThis.fetch = (async (_input, init) => {
    const signal = init?.signal;
    return new Promise<Response>((_resolve, reject) => {
      signal?.addEventListener("abort", () => reject(signal.reason));
    });
  }) as typeof fetch;

  try {
    await assert.rejects(
      sendConsoleMultipart(
        "http://127.0.0.1:7000",
        "agent-a",
        "hello",
        [new File(["png"], "image.png", { type: "image/png" })],
        "test",
        "idem-timeout",
        "queue",
        25,
      ),
      /console multipart timeout after 25 ms/,
    );
    assert.equal(scheduledMs, 25);
    assert.equal(cleared, true);
  } finally {
    globalThis.fetch = originalFetch;
    globalThis.setTimeout = originalSetTimeout;
    globalThis.clearTimeout = originalClearTimeout;
  }
});

test("console HTTP errors expose bounded response previews", async () => {
  const originalFetch = globalThis.fetch;
  const body = `prefix-${"x".repeat(700)}-secret-tail`;
  globalThis.fetch = (async () => new Response(body, { status: 502 })) as typeof fetch;

  try {
    await assert.rejects(
      fetchJson("http://127.0.0.1:7000", "/console/experience", 60_000),
      (error: unknown) => {
        assert.ok(error instanceof Error);
        assert.match(error.message, /Request failed 502/);
        assert.match(error.message, /prefix-/);
        assert.doesNotMatch(error.message, /secret-tail/);
        return true;
      },
    );
  } finally {
    globalThis.fetch = originalFetch;
  }
});

test("timeline stream HTTP errors expose bounded response previews", async () => {
  const originalFetch = globalThis.fetch;
  const body = `prefix-${"x".repeat(700)}-secret-tail`;
  globalThis.fetch = (async () => new Response(body, { status: 502 })) as typeof fetch;
  const frames: ConsoleFrame[] = [];
  const unsubscribe = subscribeTimelineEvents(
    "http://127.0.0.1:7000",
    {},
    (frame) => frames.push(frame),
  );

  try {
    await new Promise((resolve) => setTimeout(resolve, 20));
    assert.equal(frames.length > 0, true);
    const message = String((frames[0]?.data as { message?: unknown } | undefined)?.message || "");
    assert.match(message, /interaction stream request failed 502/);
    assert.match(message, /prefix-/);
    assert.doesNotMatch(message, /secret-tail/);
  } finally {
    unsubscribe();
    globalThis.fetch = originalFetch;
  }
});

test("queryTimeline normalizes aggregate log frames", async () => {
  const originalFetch = globalThis.fetch;

  globalThis.fetch = (async () => new Response(JSON.stringify({
    jsonrpc: "2.0",
    id: "mobkit/console/query_timeline:1",
    result: {
      frames: [
        {
          id: "console-frame-1",
          cursor: "console:1",
          dedupe_key: "send:runtime:agent:origin:key",
          timestamp_ms: 10,
          runtime_key: "runtime-a",
          identity: "agent-a",
          conversation_id: "agent-a",
          session_id: "session-a",
          kind: "user_input",
          status: "delivered",
          frame_version: 2,
          updated_at_ms: 11,
          payload: { content: "hello" },
          source: { kind: "send" },
          interaction_id: "turn-1",
          turn_id: "turn-1",
          run_id: "run-1",
        },
      ],
      next_cursor: "console:1",
    },
  }), {
    status: 200,
    headers: { "content-type": "application/json" },
  })) as typeof fetch;

  try {
    const result = await queryTimeline("http://127.0.0.1:7000", { identity: "agent-a" }, 10);
    assert.equal(result.available, true);
    assert.equal(result.nextCursor, "console:1");
    assert.equal(result.frames[0]?.event, "user_input");
    assert.equal(result.frames[0]?.cursor, "console:1");
    assert.equal(result.frames[0]?.status, "delivered");
    assert.equal(result.frames[0]?.frameVersion, 2);
    assert.equal(result.frames[0]?.turnId, "turn-1");
    assert.deepEqual(result.frames[0]?.data, { content: "hello" });
  } finally {
    globalThis.fetch = originalFetch;
  }
});

test("queryTimeline parsed production frames suppress raw peer prompt when structured notice exists", async () => {
  const originalFetch = globalThis.fetch;

  globalThis.fetch = (async () => new Response(JSON.stringify({
    jsonrpc: "2.0",
    id: "mobkit/console/query_timeline:1",
    result: {
      frames: [
        {
          id: "raw-run-started",
          cursor: "console:1",
          timestamp_ms: Date.parse("2026-05-27T08:00:00.000Z"),
          identity: "planner",
          kind: "run_started",
          payload: {
            prompt:
              "Peer message\n"
              + "Peer message from fugue/issue_lead/LUC-642/issue_lead:\n"
              + "Focused RED-review replan is complete.\n"
              + "Peer message",
          },
          source: { kind: "live" },
        },
        {
          id: "structured-notice",
          cursor: "console:2",
          timestamp_ms: Date.parse("2026-05-27T08:00:01.000Z"),
          identity: "planner",
          kind: "system_notice",
          payload: {
            message: {
              role: "system_notice",
              kind: "comms",
              body: "Focused RED-review replan is complete.",
              blocks: [{
                type: "comms",
                kind: "message",
                direction: "incoming",
                peer: {
                  id: "fugue/issue_lead/LUC-642/issue_lead",
                  display_name: "fugue/issue_lead/LUC-642/issue_lead",
                },
                request_id: "request-1",
                content: [{
                  type: "text",
                  text: "Focused RED-review replan is complete.",
                }],
              }],
            },
          },
          source: { kind: "live" },
        },
      ],
      next_cursor: "console:2",
    },
  }))) as typeof fetch;

  try {
    const page = await queryTimeline("http://127.0.0.1:7000", { identity: "planner" });
    const entries = mapFramesToTimelineEntries(
      {
        agent_id: "planner",
        member_id: "planner",
        label: "Planner",
        kind: "identity",
      },
      page.frames,
      { renderInteractionStartsAsUser: true },
    );

    assert.equal(entries.length, 1);
    assert.equal(entries[0]?.identity.id, "comms");
    assert.equal(entries.some((entry) => (
      entry.identity.id === "user"
      && "text" in entry
      && entry.text.includes("Peer message from fugue/issue_lead")
    )), false);
  } finally {
    globalThis.fetch = originalFetch;
  }
});

test("queryTimeline normalizes replayable frame update markers", async () => {
  const originalFetch = globalThis.fetch;

  globalThis.fetch = (async () => new Response(JSON.stringify({
    jsonrpc: "2.0",
    id: "mobkit/console/query_timeline:1",
    result: {
      frames: [
        {
          id: "console-frame-update-1",
          cursor: "console:2",
          dedupe_key: "frame-update:console-frame-1:2",
          timestamp_ms: 12,
          runtime_key: "runtime-a",
          identity: "agent-a",
          conversation_id: "agent-a",
          session_id: "session-a",
          kind: "frame_updated",
          status: "delivered",
          frame_version: 1,
          payload: {
            frame: {
              id: "console-frame-1",
              cursor: "console:1",
              dedupe_key: "send:runtime:agent:origin:key",
              timestamp_ms: 10,
              runtime_key: "runtime-a",
              identity: "agent-a",
              kind: "user_input",
              status: "delivered",
              frame_version: 2,
              updated_at_ms: 12,
              payload: { content: "hello" },
              source: { kind: "send" },
              interaction_id: "turn-1",
            },
          },
          source: { kind: "synthetic" },
          interaction_id: "turn-1",
          parent_frame_id: "console-frame-1",
          caused_by_frame_id: "console-frame-1",
        },
      ],
      next_cursor: "console:2",
    },
  }), {
    status: 200,
    headers: { "content-type": "application/json" },
  })) as typeof fetch;

  try {
    const result = await queryTimeline("http://127.0.0.1:7000", { identity: "agent-a", after: "console:1" }, 10);
    assert.equal(result.frames[0]?.event, "frame_updated");
    assert.equal(result.frames[0]?.cursor, "console:2");
    const updated = (result.frames[0]?.data as { frame?: { status?: string; cursor?: string } }).frame;
    assert.equal(updated?.status, "delivered");
    assert.equal(updated?.cursor, "console:1");
  } finally {
    globalThis.fetch = originalFetch;
  }
});

test("queryTimeline exposes typed replay-unavailable RPC errors", async () => {
  const originalFetch = globalThis.fetch;

  globalThis.fetch = (async () => new Response(JSON.stringify({
    jsonrpc: "2.0",
    id: "mobkit/console/query_timeline:1",
    error: {
      code: -32013,
      message: "query_timeline failed: replay unavailable",
      data: {
        error: "replay_unavailable",
        stream: "timeline",
        requested_cursor: "console:500",
        latest_cursor: "console:42",
      },
    },
  }), {
    status: 200,
    headers: { "content-type": "application/json" },
  })) as typeof fetch;

  try {
    await assert.rejects(
      queryTimeline("http://127.0.0.1:7000", { after: "console:500" }, 10),
      (error: unknown) => {
        const replay = error as Error & {
          replayError?: { stream?: string; requested_last_event_id?: string; latest_event_id?: string };
          timelineReplayUnavailable?: boolean;
        };
        assert.equal(replay.timelineReplayUnavailable, true);
        assert.equal(replay.replayError?.stream, "timeline");
        assert.equal(replay.replayError?.requested_last_event_id, "console:500");
        assert.equal(replay.replayError?.latest_event_id, "console:42");
        return true;
      },
    );
  } finally {
    globalThis.fetch = originalFetch;
  }
});

test("sendConsole posts idempotent console send RPC", async () => {
  const originalFetch = globalThis.fetch;
  let body = "";

  globalThis.fetch = (async (_input, init) => {
    body = typeof init?.body === "string" ? init.body : "";
    return new Response(JSON.stringify({
      jsonrpc: "2.0",
      id: "mobkit/console/send:1",
      result: {
        interaction_id: "turn-1",
        identity: "agent-a",
        input_frame_id: "console-frame-1",
        cursor: "console:1",
        status: "accepted",
      },
    }), {
      status: 200,
      headers: { "content-type": "application/json" },
    });
  }) as typeof fetch;

  try {
    const result = await sendConsole(
      "http://127.0.0.1:7000",
      "agent-a",
      "hello",
      "console:panel",
      "idem-1",
    );
    assert.match(body, /"method":"mobkit\/console\/send"/);
    assert.match(body, /"idempotency_key":"idem-1"/);
    assert.equal(result.input_frame_id, "console-frame-1");
    assert.equal(result.cursor, "console:1");
  } finally {
    globalThis.fetch = originalFetch;
  }
});

test("sendConsoleMultipart posts identity multipart sends with upload placeholders", async () => {
  const originalFetch = globalThis.fetch;
  let payload = "";
  let hasFilePart = false;

  globalThis.fetch = (async (_input, init) => {
    const form = init?.body as FormData;
    payload = String(form.get("payload") || "");
    hasFilePart = Boolean(form.get("file:upload-test-0"));
    return new Response(JSON.stringify({
      jsonrpc: "2.0",
      id: "mobkit/console/send:1",
      result: {
        interaction_id: "turn-1",
        identity: "agent-a",
        input_frame_id: "console-frame-1",
        cursor: "console:1",
        status: "accepted",
      },
    }), {
      status: 200,
      headers: { "content-type": "application/json" },
    });
  }) as typeof fetch;

  const originalNow = Date.now;
  try {
    Date.now = () => Number.parseInt("test", 36);
    const result = await sendConsoleMultipart(
      "http://127.0.0.1:7000",
      "agent-a",
      "describe this",
      [new File(["png"], "badge.png", { type: "image/png" })],
      "console:panel",
      "idem-1",
    );
    assert.match(payload, /"method":"mobkit\/console\/send"/);
    assert.match(payload, /"identity":"agent-a"/);
    assert.match(payload, /"idempotency_key":"idem-1"/);
    assert.match(payload, /"upload_id":"upload-test-0"/);
    assert.equal(hasFilePart, true);
    assert.equal(result.input_frame_id, "console-frame-1");
    assert.equal(result.cursor, "console:1");
  } finally {
    Date.now = originalNow;
    globalThis.fetch = originalFetch;
  }
});

test("sendConsoleMultipart preserves structured content before upload placeholders", async () => {
  const originalFetch = globalThis.fetch;
  let payload: Record<string, unknown> | null = null;

  globalThis.fetch = (async (_input, init) => {
    const form = init?.body as FormData;
    payload = JSON.parse(String(form.get("payload") || "{}")) as Record<string, unknown>;
    return new Response(JSON.stringify({
      jsonrpc: "2.0",
      id: "mobkit/console/send:1",
      result: {
        interaction_id: "turn-structured",
        identity: "agent-a",
        status: "accepted",
      },
    }), {
      status: 200,
      headers: { "content-type": "application/json" },
    });
  }) as typeof fetch;

  const originalNow = Date.now;
  try {
    Date.now = () => Number.parseInt("test", 36);
    await sendConsoleMultipart(
      "http://127.0.0.1:7000",
      "agent-a",
      [
        { type: "text", text: "Keep this block" },
        { type: "tool_context", id: "ctx-1" },
      ],
      [new File(["png"], "badge.png", { type: "image/png" })],
      "console:panel",
      "idem-structured",
    );

    const params = payload?.params as { content?: Array<Record<string, unknown>> } | undefined;
    assert.deepEqual(params?.content, [
      { type: "text", text: "Keep this block" },
      { type: "tool_context", id: "ctx-1" },
      {
        type: "image_upload",
        upload_id: "upload-test-0",
        media_type: "image/png",
        alt: "badge.png",
      },
    ]);
  } finally {
    Date.now = originalNow;
    globalThis.fetch = originalFetch;
  }
});

test("uploadConsoleBlobMultipart posts blob upload multipart RPC with one file", async () => {
  const originalFetch = globalThis.fetch;
  let payload: Record<string, unknown> | null = null;
  let hasFilePart = false;

  globalThis.fetch = (async (_input, init) => {
    const form = init?.body as FormData;
    payload = JSON.parse(String(form.get("payload") || "{}")) as Record<string, unknown>;
    hasFilePart = Boolean(form.get("file:upload-test-0"));
    return new Response(JSON.stringify({
      jsonrpc: "2.0",
      id: "mobkit/blob/upload:1",
      result: {
        blob_id: "blob-1",
        media_type: "image/png",
        size: 3,
      },
    }), {
      status: 200,
      headers: { "content-type": "application/json" },
    });
  }) as typeof fetch;

  const originalNow = Date.now;
  try {
    Date.now = () => Number.parseInt("test", 36);
    const result = await uploadConsoleBlobMultipart("http://127.0.0.1:7000", {
      file: new File(["png"], "badge.png", { type: "image/png" }),
    });
    assert.equal(hasFilePart, true);
    assert.equal(payload?.method, "mobkit/blob/upload");
    assert.deepEqual((payload?.params as { upload?: Record<string, unknown> }).upload, {
      type: "image_upload",
      upload_id: "upload-test-0",
      media_type: "image/png",
      alt: "badge.png",
    });
    assert.deepEqual(result, { blob_id: "blob-1", url: undefined });
  } finally {
    Date.now = originalNow;
    globalThis.fetch = originalFetch;
  }
});

test("parseSseFrames unwraps aggregate timeline frames", () => {
  const frames = parseSseFrames([
    "id: console:4",
    "event: console_frame",
    'data: {"type":"console_frame","frame":{"id":"console-frame-4","cursor":"console:4","dedupe_key":"event-4","timestamp_ms":4,"runtime_key":"runtime-a","identity":"agent-a","kind":"user_input","status":"accepted","frame_version":1,"payload":{"content":"hello"},"source":{"kind":"session_history"},"interaction_id":"turn-4"}}',
    "",
  ].join("\n"));

  assert.equal(frames.length, 1);
  assert.equal(frames[0]?.id, "console-frame-4");
  assert.equal(frames[0]?.event, "user_input");
  assert.equal(frames[0]?.cursor, "console:4");
  assert.equal(frames[0]?.frameVersion, 1);
  assert.equal(frames[0]?.sourceKind, "session_history");
  assert.deepEqual(frames[0]?.data, { content: "hello" });
});

test("parseSseFrames unwraps aggregate frame update events", () => {
  const frames = parseSseFrames([
    "id: console:5",
    "event: frame_updated",
    'data: {"type":"console_frame","frame":{"id":"console-frame-update-5","cursor":"console:5","dedupe_key":"frame-update:console-frame-4:2","timestamp_ms":5,"runtime_key":"runtime-a","identity":"agent-a","kind":"frame_updated","status":"delivered","frame_version":1,"payload":{"frame":{"id":"console-frame-4","cursor":"console:4","dedupe_key":"event-4","timestamp_ms":4,"runtime_key":"runtime-a","identity":"agent-a","kind":"user_input","status":"delivered","frame_version":2,"updated_at_ms":5,"payload":{"content":"hello"},"source":{"kind":"send"},"interaction_id":"turn-4"}},"source":{"kind":"synthetic"},"interaction_id":"turn-4","parent_frame_id":"console-frame-4"}}',
    "",
  ].join("\n"));

  assert.equal(frames.length, 1);
  assert.equal(frames[0]?.id, "console-frame-update-5");
  assert.equal(frames[0]?.event, "frame_updated");
  assert.equal(frames[0]?.cursor, "console:5");
  const updated = (frames[0]?.data as { frame?: { id?: string; status?: string } }).frame;
  assert.equal(updated?.id, "console-frame-4");
  assert.equal(updated?.status, "delivered");
});

test("subscribeTimelineEvents reconnects with the latest aggregate cursor", async () => {
  const originalFetch = globalThis.fetch;
  const calls: string[] = [];
  const lastEventIds: Array<string | undefined> = [];
  const seen: string[] = [];
  let unsubscribe = () => {};

  globalThis.fetch = (async (input, init) => {
    const url = String(input);
    calls.push(url);
    lastEventIds.push((init?.headers as Record<string, string> | undefined)?.["Last-Event-ID"]);
    if (calls.length === 1) {
      return new Response([
        "id: console:10",
        "event: snapshot_complete",
        'data: {"type":"snapshot_complete","cursor":"console:10"}',
        "",
      ].join("\n"), {
        status: 200,
        headers: { "content-type": "text/event-stream" },
      });
    }
    return new Response([
      "id: console:11",
      "event: console_frame",
      'data: {"type":"console_frame","frame":{"id":"console-frame-11","cursor":"console:11","dedupe_key":"event-11","timestamp_ms":11,"runtime_key":"runtime-a","identity":"agent-a","kind":"text_complete","status":"completed","frame_version":1,"payload":{"text":"after reconnect"},"source":{"kind":"console_event"}}}',
      "",
    ].join("\n"), {
      status: 200,
      headers: { "content-type": "text/event-stream" },
    });
  }) as typeof fetch;

  try {
    unsubscribe = subscribeTimelineEvents("http://127.0.0.1:7000", {}, (frame) => {
      seen.push(frame.event);
      if (frame.cursor === "console:11") {
        unsubscribe();
      }
    });
    await new Promise((resolve) => setTimeout(resolve, 400));
    assert.equal(calls[0], "http://127.0.0.1:7000/console/timeline/stream");
    assert.equal(calls[1], "http://127.0.0.1:7000/console/timeline/stream");
    assert.deepEqual(lastEventIds, [undefined, "console:10"]);
    assert.deepEqual(seen, ["snapshot_complete", "text_complete"]);
  } finally {
    unsubscribe();
    globalThis.fetch = originalFetch;
  }
});

test("subscribeTimelineEvents recovers from stale timeline cursors", async () => {
  const originalFetch = globalThis.fetch;
  const calls: string[] = [];
  const lastEventIds: Array<string | undefined> = [];
  const seen: string[] = [];
  let unsubscribe = () => {};

  globalThis.fetch = (async (input, init) => {
    const url = String(input);
    calls.push(url);
    lastEventIds.push((init?.headers as Record<string, string> | undefined)?.["Last-Event-ID"]);
    if (calls.length === 1) {
      return new Response(JSON.stringify({
        error: "replay_unavailable",
        requested_cursor: "bad",
        latest_cursor: "console:99",
      }), {
        status: 409,
        headers: { "content-type": "application/json" },
      });
    }
    return new Response([
      "id: console:100",
      "event: console_frame",
      'data: {"type":"console_frame","frame":{"id":"console-frame-100","cursor":"console:100","dedupe_key":"event-100","timestamp_ms":100,"runtime_key":"runtime-a","identity":"agent-a","kind":"text_complete","status":"completed","frame_version":1,"payload":{"text":"after stale cursor recovery"},"source":{"kind":"console_event"}}}',
      "",
    ].join("\n"), {
      status: 200,
      headers: { "content-type": "text/event-stream" },
    });
  }) as typeof fetch;

  try {
    unsubscribe = subscribeTimelineEvents(
      "http://127.0.0.1:7000",
      { after: "bad" },
      (frame) => {
        seen.push(frame.event);
        if (frame.cursor === "console:100") {
          unsubscribe();
        }
      },
    );
    await new Promise((resolve) => setTimeout(resolve, 500));
    assert.equal(calls[0], "http://127.0.0.1:7000/console/timeline/stream");
    assert.equal(calls[1], "http://127.0.0.1:7000/console/timeline/stream");
    assert.deepEqual(lastEventIds, ["bad", "console:99"]);
    assert.deepEqual(seen, ["replay_unavailable", "text_complete"]);
  } finally {
    unsubscribe();
    globalThis.fetch = originalFetch;
  }
});

test("subscribeTimelineEvents recovers from in-stream timeline replay gaps", async () => {
  const originalFetch = globalThis.fetch;
  const calls: string[] = [];
  const lastEventIds: Array<string | undefined> = [];
  const seen: string[] = [];
  let unsubscribe = () => {};

  globalThis.fetch = (async (input, init) => {
    const url = String(input);
    calls.push(url);
    lastEventIds.push((init?.headers as Record<string, string> | undefined)?.["Last-Event-ID"]);
    if (calls.length === 1) {
      return new Response([
        "event: replay_unavailable",
        'data: {"type":"replay_unavailable","requested_cursor":"lagged:4","latest_cursor":"console:99"}',
        "",
      ].join("\n"), {
        status: 200,
        headers: { "content-type": "text/event-stream" },
      });
    }
    return new Response([
      "id: console:100",
      "event: console_frame",
      'data: {"type":"console_frame","frame":{"id":"console-frame-100","cursor":"console:100","dedupe_key":"event-100","timestamp_ms":100,"runtime_key":"runtime-a","identity":"agent-a","kind":"text_complete","status":"completed","frame_version":1,"payload":{"text":"after stream replay gap"},"source":{"kind":"console_event"}}}',
      "",
    ].join("\n"), {
      status: 200,
      headers: { "content-type": "text/event-stream" },
    });
  }) as typeof fetch;

  try {
    unsubscribe = subscribeTimelineEvents(
      "http://127.0.0.1:7000",
      {},
      (frame) => {
        seen.push(frame.event);
        if (frame.cursor === "console:100") {
          unsubscribe();
        }
      },
    );
    await new Promise((resolve) => setTimeout(resolve, 500));
    assert.equal(calls[0], "http://127.0.0.1:7000/console/timeline/stream");
    assert.equal(calls[1], "http://127.0.0.1:7000/console/timeline/stream");
    assert.deepEqual(lastEventIds, [undefined, "console:99"]);
    assert.deepEqual(seen, ["replay_unavailable", "text_complete"]);
  } finally {
    unsubscribe();
    globalThis.fetch = originalFetch;
  }
});

test("subscribeTimelineEvents reconnects from the last delivered cursor after stream end", async () => {
  const originalFetch = globalThis.fetch;
  const calls: string[] = [];
  const lastEventIds: Array<string | undefined> = [];
  const seen: string[] = [];
  let unsubscribe = () => {};

  globalThis.fetch = (async (input, init) => {
    const url = String(input);
    calls.push(url);
    lastEventIds.push((init?.headers as Record<string, string> | undefined)?.["Last-Event-ID"]);
    if (calls.length === 1) {
      return new Response([
        "id: console:41",
        "event: console_frame",
        'data: {"type":"console_frame","frame":{"id":"console-frame-41","cursor":"console:41","dedupe_key":"event-41","timestamp_ms":41,"runtime_key":"runtime-a","identity":"agent-a","kind":"text_complete","status":"completed","frame_version":1,"payload":{"text":"before lag"},"source":{"kind":"console_event"}}}',
        "",
      ].join("\n"), {
        status: 200,
        headers: { "content-type": "text/event-stream" },
      });
    }
    return new Response([
      "id: console:42",
      "event: console_frame",
      'data: {"type":"console_frame","frame":{"id":"console-frame-42","cursor":"console:42","dedupe_key":"event-42","timestamp_ms":42,"runtime_key":"runtime-a","identity":"agent-a","kind":"text_complete","status":"completed","frame_version":1,"payload":{"text":"replayed after reconnect"},"source":{"kind":"console_event"}}}',
      "",
    ].join("\n"), {
      status: 200,
      headers: { "content-type": "text/event-stream" },
    });
  }) as typeof fetch;

  try {
    unsubscribe = subscribeTimelineEvents(
      "http://127.0.0.1:7000",
      {},
      (frame) => {
        seen.push(`${frame.event}:${frame.cursor ?? ""}`);
        if (frame.cursor === "console:42") {
          unsubscribe();
        }
      },
    );
    await new Promise((resolve) => setTimeout(resolve, 500));
    assert.equal(calls[0], "http://127.0.0.1:7000/console/timeline/stream");
    assert.equal(calls[1], "http://127.0.0.1:7000/console/timeline/stream");
    assert.deepEqual(lastEventIds, [undefined, "console:41"]);
    assert.deepEqual(seen, ["text_complete:console:41", "text_complete:console:42"]);
  } finally {
    unsubscribe();
    globalThis.fetch = originalFetch;
  }
});
