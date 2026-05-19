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
} from "./network";

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
  const seen: string[] = [];
  let unsubscribe = () => {};

  globalThis.fetch = (async (input) => {
    const url = String(input);
    calls.push(url);
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
    assert.equal(calls[1], "http://127.0.0.1:7000/console/timeline/stream?after=console%3A10");
    assert.deepEqual(seen, ["snapshot_complete", "text_complete"]);
  } finally {
    unsubscribe();
    globalThis.fetch = originalFetch;
  }
});
