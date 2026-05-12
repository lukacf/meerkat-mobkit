import test from "node:test";
import assert from "node:assert/strict";

import {
  parseSseFrames,
  queryEvents,
  queryTimeline,
  sendAddressedInteraction,
  sendConsole,
  sendConsoleMultipart,
  subscribeIdentityEvents,
  subscribeTimelineEvents,
} from "./network";

test("queryEvents uses fallback events from no_event_log_configured envelopes", async () => {
  const originalFetch = globalThis.fetch;

  globalThis.fetch = (async () => new Response(JSON.stringify({
    jsonrpc: "2.0",
    id: "mobkit/query_events:1",
    result: {
      status: "no_event_log_configured",
      events: [
        {
          event_id: "evt-1",
          identity: "identity:luka",
          interaction_id: "turn-1",
          event_type: "interaction_started",
          timestamp_ms: 1,
          data: {
            content: "hello",
          },
        },
      ],
    },
  }), {
    status: 200,
    headers: { "content-type": "application/json" },
  })) as typeof fetch;

  try {
    const result = await queryEvents("http://127.0.0.1:7000", { identity: "identity:luka" }, 10);
    assert.equal(result.available, false, "no_event_log_configured must signal available=false");
    assert.equal(result.frames.length, 1);
    assert.equal(result.frames[0]?.id, "evt-1");
    assert.equal(result.frames[0]?.event, "interaction_started");
    assert.equal(result.frames[0]?.identity, "identity:luka");
    assert.equal(result.frames[0]?.interactionId, "turn-1");
    assert.deepEqual(result.frames[0]?.data, { content: "hello" });
  } finally {
    globalThis.fetch = originalFetch;
  }
});

test("queryEvents reports available=true for normal event-log envelopes", async () => {
  const originalFetch = globalThis.fetch;

  globalThis.fetch = (async () => new Response(JSON.stringify({
    jsonrpc: "2.0",
    id: "mobkit/query_events:1",
    result: {
      events: [
        {
          event_id: "evt-2",
          identity: "identity:luka",
          interaction_id: "turn-1",
          event_type: "text_complete",
          timestamp_ms: 5,
          data: { text: "hi" },
        },
      ],
    },
  }), {
    status: 200,
    headers: { "content-type": "application/json" },
  })) as typeof fetch;

  try {
    const result = await queryEvents("http://127.0.0.1:7000", { identity: "identity:luka" }, 10);
    assert.equal(result.available, true);
    assert.equal(result.frames.length, 1);
    assert.equal(result.frames[0]?.id, "evt-2");
  } finally {
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

test("sendAddressedInteraction uses identity-native RPC and stream when target is identity-addressed", async () => {
  const originalFetch = globalThis.fetch;
  const calls: Array<{ url: string; method: string; body: string }> = [];

  globalThis.fetch = (async (input, init) => {
    const url = String(input);
    const method = String(init?.method || "GET");
    const body = typeof init?.body === "string" ? init.body : "";
    calls.push({ url, method, body });

    if (url.endsWith("/console/identity/stream")) {
      return new Response([
        "id: console-stream-identity-1",
        "event: subscribed",
        'data: {"event_id":"console-stream-identity-1","identity":"identity:luka","event_type":"subscribed","timestamp_ms":1,"data":{"stream":"identity"}}',
        "",
        "id: evt-1",
        "event: interaction_complete",
        'data: {"event_id":"evt-1","interaction_id":"turn-1","identity":"identity:luka","event_type":"interaction_complete","timestamp_ms":2,"data":{"text":"done"}}',
        "",
      ].join("\n"), {
        status: 200,
        headers: { "content-type": "text/event-stream" },
      });
    }

    if (url.endsWith("/console/rpc")) {
      return new Response(JSON.stringify({
        jsonrpc: "2.0",
        id: "mobkit/interact:1",
        result: {
          interaction_id: "turn-1",
          identity: "identity:luka",
        },
      }), {
        status: 200,
        headers: { "content-type": "application/json" },
      });
    }

    throw new Error(`unexpected fetch: ${url}`);
  }) as typeof fetch;

  try {
    const result = await sendAddressedInteraction(
      "http://127.0.0.1:7000",
      { addressingMode: "identity", identity: "identity:luka", memberId: "member-luka" },
      "hello",
      "console:panel-1",
    );
    assert.equal(calls.length, 2);
    assert.equal(calls[0]?.url, "http://127.0.0.1:7000/console/identity/stream");
    assert.equal(calls[1]?.url, "http://127.0.0.1:7000/console/rpc");
    assert.match(calls[1]?.body || "", /"method":"mobkit\/interact"/);
    assert.match(calls[1]?.body || "", /"identity":"identity:luka"/);
    assert.equal((result.sendResult as { interaction_id?: string }).interaction_id, "turn-1");
    assert.equal(result.frames.some((frame) => frame.event === "interaction_complete"), true);
  } finally {
    globalThis.fetch = originalFetch;
  }
});

test("sendAddressedInteraction falls back to member transport for member-addressed targets", async () => {
  const originalFetch = globalThis.fetch;
  const calls: Array<{ url: string; method: string; body: string }> = [];

  globalThis.fetch = (async (input, init) => {
    const url = String(input);
    const method = String(init?.method || "GET");
    const body = typeof init?.body === "string" ? init.body : "";
    calls.push({ url, method, body });

    if (url.endsWith("/interactions/stream")) {
      return new Response([
        "id: evt-1",
        "event: interaction_complete",
        'data: {"session_id":"sess-1","text":"done"}',
        "",
      ].join("\n"), {
        status: 200,
        headers: { "content-type": "text/event-stream" },
      });
    }

    if (url.endsWith("/console/rpc")) {
      return new Response(JSON.stringify({
        jsonrpc: "2.0",
        id: "mobkit/send_message:1",
        result: {
          accepted: true,
          member_id: "member-luka",
          session_id: "sess-1",
        },
      }), {
        status: 200,
        headers: { "content-type": "application/json" },
      });
    }

    throw new Error(`unexpected fetch: ${url}`);
  }) as typeof fetch;

  try {
    const result = await sendAddressedInteraction(
      "http://127.0.0.1:7000",
      { addressingMode: "member", memberId: "member-luka" },
      "hello",
    );
    assert.equal(calls.length, 2);
    assert.equal(calls[0]?.url, "http://127.0.0.1:7000/interactions/stream");
    assert.match(calls[1]?.body || "", /"method":"mobkit\/send_message"/);
    assert.equal((result.sendResult as { session_id?: string }).session_id, "sess-1");
    assert.equal(result.frames.some((frame) => frame.event === "interaction_complete"), true);
  } finally {
    globalThis.fetch = originalFetch;
  }
});

test("parseSseFrames unwraps identity-stream envelopes to their nested payloads", () => {
  const frames = parseSseFrames([
    "id: evt-1",
    "event: text_delta",
    'data: {"event_id":"evt-1","interaction_id":"turn-1","identity":"identity:luka","event_type":"text_delta","timestamp_ms":2,"data":{"delta":"done"}}',
    "",
  ].join("\n"));

  assert.equal(frames.length, 1);
  assert.equal(frames[0]?.id, "evt-1");
  assert.equal(frames[0]?.event, "text_delta");
  assert.deepEqual(frames[0]?.data, { delta: "done" });
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

test("sendAddressedInteraction filters identity-stream frames by envelope interaction id", async () => {
  const originalFetch = globalThis.fetch;

  globalThis.fetch = (async (input, init) => {
    const url = String(input);

    if (url.endsWith("/console/identity/stream")) {
      return new Response([
        "id: evt-1",
        "event: text_delta",
        'data: {"event_id":"evt-1","interaction_id":"turn-2","identity":"identity:luka","event_type":"text_delta","timestamp_ms":1,"data":{"delta":"wrong panel"}}',
        "",
        "id: evt-2",
        "event: text_delta",
        'data: {"event_id":"evt-2","interaction_id":"turn-1","identity":"identity:luka","event_type":"text_delta","timestamp_ms":2,"data":{"delta":"right panel"}}',
        "",
        "id: evt-3",
        "event: interaction_complete",
        'data: {"event_id":"evt-3","interaction_id":"turn-1","identity":"identity:luka","event_type":"interaction_complete","timestamp_ms":3,"data":{"text":"done"}}',
        "",
      ].join("\n"), {
        status: 200,
        headers: { "content-type": "text/event-stream" },
      });
    }

    if (url.endsWith("/console/rpc")) {
      return new Response(JSON.stringify({
        jsonrpc: "2.0",
        id: "mobkit/interact:1",
        result: {
          interaction_id: "turn-1",
          identity: "identity:luka",
        },
      }), {
        status: 200,
        headers: { "content-type": "application/json" },
      });
    }

    throw new Error(`unexpected fetch: ${url} (${String(init?.method || "GET")})`);
  }) as typeof fetch;

  try {
    const result = await sendAddressedInteraction(
      "http://127.0.0.1:7000",
      { addressingMode: "identity", identity: "identity:luka", memberId: "member-luka" },
      "hello",
      "console:panel-1",
    );
    assert.deepEqual(
      result.frames.map((frame) => frame.id),
      ["evt-2", "evt-3"],
    );
    assert.deepEqual(result.frames[0]?.data, { delta: "right panel" });
  } finally {
    globalThis.fetch = originalFetch;
  }
});

test("subscribeIdentityEvents keeps consuming frames after terminal events", async () => {
  const originalFetch = globalThis.fetch;
  const seen: string[] = [];

  globalThis.fetch = (async (input) => {
    const url = String(input);
    if (!url.endsWith("/console/identity/stream")) {
      throw new Error(`unexpected fetch: ${url}`);
    }
    return new Response([
      "id: evt-1",
      "event: interaction_complete",
      'data: {"event_id":"evt-1","identity":"identity:luka","event_type":"interaction_complete","timestamp_ms":1,"data":{"text":"first"}}',
      "",
      "id: evt-2",
      "event: text_delta",
      'data: {"event_id":"evt-2","identity":"identity:luka","event_type":"text_delta","timestamp_ms":2,"data":{"delta":"second"}}',
      "",
    ].join("\n"), {
      status: 200,
      headers: { "content-type": "text/event-stream" },
    });
  }) as typeof fetch;

  try {
    const unsubscribe = subscribeIdentityEvents("http://127.0.0.1:7000", "identity:luka", (frame) => {
      seen.push(frame.id || "");
    });
    await new Promise((resolve) => setTimeout(resolve, 25));
    unsubscribe();
    assert.deepEqual(seen, ["evt-1", "evt-2"]);
  } finally {
    globalThis.fetch = originalFetch;
  }
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
