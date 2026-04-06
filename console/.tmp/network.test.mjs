var __getOwnPropNames = Object.getOwnPropertyNames;
var __esm = (fn, res) => function __init() {
  return fn && (res = (0, fn[__getOwnPropNames(fn)[0]])(fn = 0)), res;
};
var __commonJS = (cb, mod) => function __require() {
  return mod || (0, cb[__getOwnPropNames(cb)[0]])((mod = { exports: {} }).exports, mod), mod.exports;
};

// ../packages/console-core/src/control-plane.ts
function trimString(value) {
  if (typeof value !== "string") {
    return void 0;
  }
  const trimmed = value.trim();
  return trimmed || void 0;
}
function normalizeConsoleInteractionAccepted(value) {
  const record = value && typeof value === "object" ? value : null;
  if (!record) {
    return null;
  }
  const interactionId = trimString(record.interaction_id);
  const identity = trimString(record.identity);
  if (!interactionId || !identity) {
    return null;
  }
  return { interaction_id: interactionId, identity };
}
function normalizeReplayUnavailableError(value) {
  const record = value && typeof value === "object" ? value : null;
  if (!record || record.error !== "replay_unavailable") {
    return null;
  }
  const stream = record.stream === "identity" || record.stream === "all_events" ? record.stream : null;
  const requested = trimString(record.requested_last_event_id);
  const latest = trimString(record.latest_event_id);
  if (!stream || !requested || !latest) {
    return null;
  }
  return {
    error: "replay_unavailable",
    stream,
    requested_last_event_id: requested,
    latest_event_id: latest
  };
}
function normalizeConsoleInteractionRejectedError(value) {
  const record = value && typeof value === "object" ? value : null;
  if (!record) {
    return null;
  }
  const code = record.code;
  const message = trimString(record.message);
  if (code !== -32001 && code !== -32002 && code !== -32003 && code !== -32004 && code !== -32602 && code !== -32603) {
    return null;
  }
  if (!message) {
    return null;
  }
  return { code, message };
}
var init_control_plane = __esm({
  "../packages/console-core/src/control-plane.ts"() {
  }
});

// ../packages/console-core/src/rich-content.ts
var init_rich_content = __esm({
  "../packages/console-core/src/rich-content.ts"() {
  }
});

// ../packages/console-core/src/conversation.ts
var init_conversation = __esm({
  "../packages/console-core/src/conversation.ts"() {
    init_rich_content();
  }
});

// ../packages/console-core/src/dock.ts
var init_dock = __esm({
  "../packages/console-core/src/dock.ts"() {
  }
});

// ../packages/console-core/src/sidebar.ts
var init_sidebar = __esm({
  "../packages/console-core/src/sidebar.ts"() {
    init_control_plane();
  }
});

// ../packages/console-core/src/format.ts
var init_format = __esm({
  "../packages/console-core/src/format.ts"() {
  }
});

// ../packages/console-core/src/index.ts
var init_src = __esm({
  "../packages/console-core/src/index.ts"() {
    init_control_plane();
    init_conversation();
    init_dock();
    init_sidebar();
    init_rich_content();
    init_format();
  }
});

// src/lib/network.ts
function unwrapConsoleEnvelope(eventName, data) {
  if (!data || typeof data !== "object") {
    return { data };
  }
  const record = data;
  if (typeof record.event_id === "string" && typeof record.event_type === "string" && typeof record.identity === "string" && "data" in record) {
    const envelope = record;
    return {
      id: envelope.event_id,
      event: envelope.event_type || eventName,
      identity: envelope.identity,
      interactionId: envelope.interaction_id,
      timestampMs: envelope.timestamp_ms,
      data: envelope.data
    };
  }
  return { data };
}
function parseSseFrames(rawText) {
  const blocks = rawText.split(/\n\n+/).map((part) => part.trim()).filter(Boolean);
  const frames = [];
  for (const block of blocks) {
    const lines = block.split("\n");
    let id = "";
    let event = "message";
    const dataLines = [];
    for (const line of lines) {
      if (line.startsWith("id:")) {
        id = line.slice(3).trim();
        continue;
      }
      if (line.startsWith("event:")) {
        event = line.slice(6).trim();
        continue;
      }
      if (line.startsWith("data:")) {
        dataLines.push(line.slice(5).trim());
      }
    }
    if (!id && dataLines.length === 0) {
      continue;
    }
    const rawData = dataLines.join("\n");
    let data = rawData;
    if (rawData) {
      try {
        data = JSON.parse(rawData);
      } catch (_) {
        data = rawData;
      }
    }
    const normalized = unwrapConsoleEnvelope(event, data);
    frames.push({
      id: normalized.id || id,
      event: normalized.event || event,
      identity: normalized.identity,
      interactionId: normalized.interactionId,
      timestampMs: normalized.timestampMs,
      data: normalized.data
    });
  }
  return frames;
}
async function rpc(baseUrl, method, params) {
  const response = await fetch(`${baseUrl}/console/rpc`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({
      jsonrpc: "2.0",
      id: `${method}:${Date.now()}`,
      method,
      params
    })
  });
  if (!response.ok) {
    const text = await response.text();
    throw new Error(`${method} request failed ${response.status}: ${text}`);
  }
  const result = await response.json();
  if (result.error) {
    const typedError = normalizeConsoleInteractionRejectedError(result.error);
    if (typedError) {
      const error = new Error(`${method} RPC error ${typedError.code}: ${typedError.message}`);
      error.rpcError = typedError;
      throw error;
    }
    throw new Error(`${method} RPC error: ${result.error.message || JSON.stringify(result.error)}`);
  }
  return result.result;
}
async function sendMessage(baseUrl, memberId, message) {
  return rpc(baseUrl, "mobkit/send_message", {
    member_id: memberId,
    message
  });
}
function matchesCorrelation(candidate, correlation, allowUnscoped = true) {
  if (!correlation?.sessionId && !correlation?.interactionId) {
    return true;
  }
  if (candidate === null || typeof candidate !== "object") {
    return allowUnscoped;
  }
  const record = candidate;
  const sessionId = record.session_id ?? record.sessionId;
  const interactionId = record.interaction_id ?? record.interactionId;
  const hasScopedField = sessionId !== void 0 || interactionId !== void 0;
  if (!hasScopedField) {
    return allowUnscoped;
  }
  if (correlation.sessionId && sessionId === correlation.sessionId) {
    return true;
  }
  if (correlation.interactionId && interactionId === correlation.interactionId) {
    return true;
  }
  return false;
}
async function drainInteractionResponse(response, correlation) {
  return streamFramesFromResponse(response, { correlation });
}
async function streamFramesFromResponse(response, options = {}) {
  const stopOnTerminal = options.stopOnTerminal ?? Boolean(options.correlation);
  if (!response.ok) {
    const text = await response.text();
    let parsed = null;
    try {
      parsed = JSON.parse(text);
    } catch {
      parsed = null;
    }
    const replayError = normalizeReplayUnavailableError(parsed);
    if (replayError) {
      const error = new Error(
        `interaction stream replay unavailable for ${replayError.stream}: ${replayError.requested_last_event_id} -> ${replayError.latest_event_id}`
      );
      error.replayError = replayError;
      throw error;
    }
    throw new Error(`interaction stream request failed ${response.status}: ${text}`);
  }
  if (!response.body || typeof response.body.getReader !== "function") {
    const frames2 = parseSseFrames(await response.text());
    for (const frame of frames2) {
      if (matchesCorrelation(frame, options.correlation, true)) {
        options.onFrame?.(frame);
      }
    }
    return !options.correlation ? frames2 : frames2.filter((frame) => matchesCorrelation(frame, options.correlation, true));
  }
  const reader = response.body.getReader();
  const decoder = new TextDecoder();
  let frameBuffer = "";
  const frames = [];
  try {
    while (true) {
      const { value, done } = await reader.read();
      if (done) {
        break;
      }
      const chunk = decoder.decode(value, { stream: true });
      frameBuffer += chunk;
      let sawTerminal = false;
      frameBuffer = flushSseBlocks(frameBuffer, (frame) => {
        if (matchesCorrelation(frame, options.correlation, true)) {
          frames.push(frame);
          options.onFrame?.(frame);
          if (stopOnTerminal && TERMINAL_SSE_EVENTS.has(frame.event || "")) {
            sawTerminal = true;
          }
        }
      });
      if (sawTerminal) {
        break;
      }
    }
    const finalChunk = decoder.decode();
    frameBuffer += finalChunk;
    frameBuffer = flushSseBlocks(frameBuffer, (frame) => {
      if (matchesCorrelation(frame, options.correlation, true)) {
        frames.push(frame);
        options.onFrame?.(frame);
      }
    });
    flushTrailingSseBlock(frameBuffer, (frame) => {
      if (matchesCorrelation(frame, options.correlation, true)) {
        frames.push(frame);
        options.onFrame?.(frame);
      }
    });
  } finally {
    try {
      await reader.cancel();
    } catch {
    }
  }
  return frames;
}
function flushSseBlocks(buffer, onFrame) {
  let searchIndex = 0;
  while (true) {
    const boundaryIndex = buffer.indexOf("\n\n", searchIndex);
    if (boundaryIndex === -1) {
      break;
    }
    const block = buffer.slice(0, boundaryIndex + 2);
    buffer = buffer.slice(boundaryIndex + 2);
    searchIndex = 0;
    for (const frame of parseSseFrames(block)) {
      onFrame(frame);
    }
  }
  return buffer;
}
function flushTrailingSseBlock(buffer, onFrame) {
  if (!buffer.trim()) {
    return;
  }
  for (const frame of parseSseFrames(`${buffer}

`)) {
    onFrame(frame);
  }
}
function persistedEventToFrame(raw, index) {
  const record = typeof raw === "object" && raw !== null ? raw : {};
  if (typeof record.event_id === "string" && typeof record.event_type === "string" && typeof record.identity === "string" && "data" in record) {
    return {
      id: String(record.event_id),
      event: String(record.event_type),
      identity: String(record.identity),
      ...typeof record.interaction_id === "string" ? { interactionId: String(record.interaction_id) } : {},
      ...typeof record.timestamp_ms === "number" ? { timestampMs: record.timestamp_ms } : {},
      data: record.data
    };
  }
  const event = typeof record.event === "object" && record.event !== null ? record.event : {};
  if (event.kind === "agent") {
    const payload = typeof event.payload === "object" && event.payload !== null ? event.payload : null;
    return {
      id: String(record.id ?? `event:${index}`),
      event: String(event.event_type ?? "agent_event"),
      ...typeof record.timestamp_ms === "number" ? { timestampMs: record.timestamp_ms } : {},
      data: payload ?? event
    };
  }
  if (event.kind === "module") {
    return {
      id: String(record.id ?? `event:${index}`),
      event: String(event.event_type ?? "module_event"),
      ...typeof record.timestamp_ms === "number" ? { timestampMs: record.timestamp_ms } : {},
      data: event.payload ?? event
    };
  }
  return {
    id: String(record.id ?? `event:${index}`),
    event: String(record.type ?? "event"),
    ...typeof record.timestamp_ms === "number" ? { timestampMs: record.timestamp_ms } : {},
    data: raw
  };
}
async function queryEvents(baseUrl, target, limit = 40) {
  const identity = target.identity?.trim();
  const memberId = target.memberId?.trim();
  const result = await rpc(baseUrl, "mobkit/query_events", {
    limit,
    ...identity ? { identity } : {},
    ...identity ? {} : memberId ? { member_id: memberId } : {}
  });
  let events = result;
  if (typeof result === "object" && result !== null) {
    const record = result;
    if (record.status === "no_event_log_configured") {
      events = Array.isArray(record.events) ? record.events : [];
    }
  }
  if (!Array.isArray(events)) {
    return [];
  }
  return events.filter((raw) => {
    if (typeof raw !== "object" || raw === null) return true;
    const ev = raw.event;
    if (typeof ev !== "object" || ev === null) return true;
    const eventRecord = ev;
    if (eventRecord.kind !== "agent") return true;
    return typeof eventRecord.payload === "object" && eventRecord.payload !== null;
  }).map((event, index) => persistedEventToFrame(event, index));
}
async function sendInteraction(baseUrl, memberId, message) {
  const streamAbort = new AbortController();
  const streamResponsePromise = fetch(`${baseUrl}/interactions/stream`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ member_id: memberId }),
    signal: streamAbort.signal
  });
  void streamResponsePromise.catch(() => {
  });
  let sendResult;
  try {
    sendResult = await sendMessage(baseUrl, memberId, message);
  } catch (err) {
    streamAbort.abort();
    throw err;
  }
  let frames;
  try {
    frames = await drainInteractionResponse(
      await streamResponsePromise,
      { sessionId: sendResult.session_id }
    );
  } catch {
    frames = [];
  }
  return { sendResult, frames };
}
async function sendInteract(baseUrl, identity, content, origin) {
  const accepted = await rpc(baseUrl, "mobkit/interact", {
    identity,
    content,
    origin
  });
  const normalized = normalizeConsoleInteractionAccepted(accepted);
  if (!normalized) {
    throw new Error("mobkit/interact returned an invalid acceptance payload");
  }
  return normalized;
}
async function sendAddressedInteraction(baseUrl, target, message, origin = "console") {
  if (target.addressingMode === "identity") {
    const identity = target.identity?.trim();
    if (!identity) {
      throw new Error("identity-addressed send requires target.identity");
    }
    const streamAbort = new AbortController();
    const streamResponsePromise = fetch(`${baseUrl}/console/identity/stream`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ identity }),
      signal: streamAbort.signal
    });
    void streamResponsePromise.catch(() => {
    });
    let sendResult;
    try {
      sendResult = await sendInteract(baseUrl, identity, message, origin);
    } catch (err) {
      streamAbort.abort();
      throw err;
    }
    let frames;
    try {
      frames = await drainInteractionResponse(
        await streamResponsePromise,
        { interactionId: sendResult.interaction_id }
      );
    } catch {
      frames = [];
    }
    return { sendResult, frames };
  }
  const memberId = target.memberId?.trim();
  if (!memberId) {
    throw new Error("member-addressed send requires target.memberId");
  }
  return sendInteraction(baseUrl, memberId, message);
}
function subscribeConsoleEvents(baseUrl, path, onFrame, options) {
  const controller = new AbortController();
  void (async () => {
    const response = await fetch(`${baseUrl}${path}`, {
      method: options?.method || "GET",
      headers: { "content-type": "application/json" },
      ...options?.body ? { body: JSON.stringify(options.body) } : {},
      signal: controller.signal
    });
    await streamFramesFromResponse(response, { onFrame, stopOnTerminal: false });
  })().catch(() => {
  });
  return () => controller.abort();
}
function subscribeIdentityEvents(baseUrl, identity, onFrame) {
  return subscribeConsoleEvents(baseUrl, "/console/identity/stream", onFrame, {
    method: "POST",
    body: { identity }
  });
}
var TERMINAL_SSE_EVENTS;
var init_network = __esm({
  "src/lib/network.ts"() {
    init_src();
    TERMINAL_SSE_EVENTS = /* @__PURE__ */ new Set([
      "interaction_complete",
      "run_completed",
      "interaction_failed",
      "run_failed"
    ]);
  }
});

// src/lib/network.test.ts
import test from "node:test";
import assert from "node:assert/strict";
var require_network_test = __commonJS({
  "src/lib/network.test.ts"() {
    init_network();
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
                content: "hello"
              }
            }
          ]
        }
      }), {
        status: 200,
        headers: { "content-type": "application/json" }
      }));
      try {
        const frames = await queryEvents("http://127.0.0.1:7000", { identity: "identity:luka" }, 10);
        assert.equal(frames.length, 1);
        assert.equal(frames[0]?.id, "evt-1");
        assert.equal(frames[0]?.event, "interaction_started");
        assert.equal(frames[0]?.identity, "identity:luka");
        assert.equal(frames[0]?.interactionId, "turn-1");
        assert.deepEqual(frames[0]?.data, { content: "hello" });
      } finally {
        globalThis.fetch = originalFetch;
      }
    });
    test("sendAddressedInteraction uses identity-native RPC and stream when target is identity-addressed", async () => {
      const originalFetch = globalThis.fetch;
      const calls = [];
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
            ""
          ].join("\n"), {
            status: 200,
            headers: { "content-type": "text/event-stream" }
          });
        }
        if (url.endsWith("/console/rpc")) {
          return new Response(JSON.stringify({
            jsonrpc: "2.0",
            id: "mobkit/interact:1",
            result: {
              interaction_id: "turn-1",
              identity: "identity:luka"
            }
          }), {
            status: 200,
            headers: { "content-type": "application/json" }
          });
        }
        throw new Error(`unexpected fetch: ${url}`);
      });
      try {
        const result = await sendAddressedInteraction(
          "http://127.0.0.1:7000",
          { addressingMode: "identity", identity: "identity:luka", memberId: "member-luka" },
          "hello",
          "console:panel-1"
        );
        assert.equal(calls.length, 2);
        assert.equal(calls[0]?.url, "http://127.0.0.1:7000/console/identity/stream");
        assert.equal(calls[1]?.url, "http://127.0.0.1:7000/console/rpc");
        assert.match(calls[1]?.body || "", /"method":"mobkit\/interact"/);
        assert.match(calls[1]?.body || "", /"identity":"identity:luka"/);
        assert.equal(result.sendResult.interaction_id, "turn-1");
        assert.equal(result.frames.some((frame) => frame.event === "interaction_complete"), true);
      } finally {
        globalThis.fetch = originalFetch;
      }
    });
    test("sendAddressedInteraction falls back to member transport for member-addressed targets", async () => {
      const originalFetch = globalThis.fetch;
      const calls = [];
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
            ""
          ].join("\n"), {
            status: 200,
            headers: { "content-type": "text/event-stream" }
          });
        }
        if (url.endsWith("/console/rpc")) {
          return new Response(JSON.stringify({
            jsonrpc: "2.0",
            id: "mobkit/send_message:1",
            result: {
              accepted: true,
              member_id: "member-luka",
              session_id: "sess-1"
            }
          }), {
            status: 200,
            headers: { "content-type": "application/json" }
          });
        }
        throw new Error(`unexpected fetch: ${url}`);
      });
      try {
        const result = await sendAddressedInteraction(
          "http://127.0.0.1:7000",
          { addressingMode: "member", memberId: "member-luka" },
          "hello"
        );
        assert.equal(calls.length, 2);
        assert.equal(calls[0]?.url, "http://127.0.0.1:7000/interactions/stream");
        assert.match(calls[1]?.body || "", /"method":"mobkit\/send_message"/);
        assert.equal(result.sendResult.session_id, "sess-1");
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
        ""
      ].join("\n"));
      assert.equal(frames.length, 1);
      assert.equal(frames[0]?.id, "evt-1");
      assert.equal(frames[0]?.event, "text_delta");
      assert.deepEqual(frames[0]?.data, { delta: "done" });
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
            ""
          ].join("\n"), {
            status: 200,
            headers: { "content-type": "text/event-stream" }
          });
        }
        if (url.endsWith("/console/rpc")) {
          return new Response(JSON.stringify({
            jsonrpc: "2.0",
            id: "mobkit/interact:1",
            result: {
              interaction_id: "turn-1",
              identity: "identity:luka"
            }
          }), {
            status: 200,
            headers: { "content-type": "application/json" }
          });
        }
        throw new Error(`unexpected fetch: ${url} (${String(init?.method || "GET")})`);
      });
      try {
        const result = await sendAddressedInteraction(
          "http://127.0.0.1:7000",
          { addressingMode: "identity", identity: "identity:luka", memberId: "member-luka" },
          "hello",
          "console:panel-1"
        );
        assert.deepEqual(
          result.frames.map((frame) => frame.id),
          ["evt-2", "evt-3"]
        );
        assert.deepEqual(result.frames[0]?.data, { delta: "right panel" });
      } finally {
        globalThis.fetch = originalFetch;
      }
    });
    test("subscribeIdentityEvents keeps consuming frames after terminal events", async () => {
      const originalFetch = globalThis.fetch;
      const seen = [];
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
          ""
        ].join("\n"), {
          status: 200,
          headers: { "content-type": "text/event-stream" }
        });
      });
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
  }
});
export default require_network_test();
