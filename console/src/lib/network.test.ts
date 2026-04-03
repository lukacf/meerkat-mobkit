import test from "node:test";
import assert from "node:assert/strict";

import { parseSseFrames, queryEvents, sendAddressedInteraction } from "./network";

test("queryEvents uses fallback events from no_event_log_configured envelopes", async () => {
  const originalFetch = globalThis.fetch;

  globalThis.fetch = (async () => new Response(JSON.stringify({
    jsonrpc: "2.0",
    id: "mobkit/query_events:1",
    result: {
      status: "no_event_log_configured",
      events: [
        {
          id: "evt-1",
          event: {
            kind: "agent",
            event_type: "text_delta",
            payload: {
              delta: "hello",
            },
          },
        },
      ],
    },
  }), {
    status: 200,
    headers: { "content-type": "application/json" },
  })) as typeof fetch;

  try {
    const frames = await queryEvents("http://127.0.0.1:7000", "member-1", 10);
    assert.equal(frames.length, 1);
    assert.equal(frames[0]?.id, "evt-1");
    assert.equal(frames[0]?.event, "text_delta");
    assert.deepEqual(frames[0]?.data, { delta: "hello" });
  } finally {
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
