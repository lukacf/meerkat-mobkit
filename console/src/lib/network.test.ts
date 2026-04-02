import test from "node:test";
import assert from "node:assert/strict";

import { queryEvents } from "./network";

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
