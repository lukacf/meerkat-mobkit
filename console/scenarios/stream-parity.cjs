"use strict";
const assert = require("node:assert/strict");
const fs = require("node:fs/promises");
const path = require("node:path");
const { randomUUID } = require("node:crypto");
const { startFixture, eventually, rpc } = require("../acceptance-runtime.cjs");

async function liveCapture(url) {
  const abort = new AbortController();
  const response = await fetch(url, { signal: abort.signal });
  assert.equal(response.status, 200);
  const events = [];
  let fault;
  const task = (async () => {
    const reader = response.body.getReader();
    const decoder = new TextDecoder();
    let buffer = "";
    while (true) {
      const { value, done } = await reader.read();
      buffer += decoder.decode(value, { stream: !done });
      let end;
      while ((end = buffer.indexOf("\n\n")) >= 0) {
        const block = buffer.slice(0, end); buffer = buffer.slice(end + 2);
        const data = block.split("\n").filter(line => line.startsWith("data:")).map(line => line.slice(5).trimStart()).join("\n");
        if (data) events.push(JSON.parse(data));
      }
      if (done) throw new Error("live stream closed before capture completed");
    }
  })().catch(error => { if (!abort.signal.aborted) fault = error; });
  return {
    events,
    check() { if (fault) throw fault; },
    async close() { abort.abort(); await task; },
  };
}

async function sharedOwnerStreamParity() {
  assert(process.env.MOBKIT_EXAMPLE_BIN_DIR, "This acceptance must use the coordinator's prebuilt runtime.");
  const fixture = await startFixture();
  const captures = [];
  const evidence = {};
  try {
    const source = Array.from({ length: 40 }, (_, i) => `Row ${String(i).padStart(3, "0")} preserves exact  whitespace and peer-${i} identifiers.\n`).join("");
    await fixture.control("model", { source, chunk_chars: 8, delay_ms: 3 });
    for (const base of [fixture.backendUrl, `${fixture.backendUrl}/mirror`, fixture.baseUrl]) {
      captures.push(await liveCapture(`${base}/console/timeline/stream?identity=router%3Amain`));
    }
    await eventually(() => captures.every(capture => {
      capture.check();
      return capture.events.some(event => event.type === "snapshot_complete");
    }), "all actual router snapshots complete before send");
    const sent = await rpc(fixture.baseUrl, "mobkit/console/send", {
      identity: "router:main", origin: "fixture:stream-parity", origin_kind: "operator",
      idempotency_key: randomUUID(), content: "Prove each actual router emits the complete source.",
    });
    assert(!sent.body.error, JSON.stringify(sent.body));
    const interactionId = sent.body.result.interaction_id;
    const relevant = capture => capture.events.flatMap(event => event.frame?.interaction_id === interactionId ? [event.frame] : []);
    // Observe live streams directly. No timeline query or model-request poll is
    // allowed to heal missing frames before both terminal events arrive.
    await eventually(() => captures.every(capture => {
      capture.check();
      return relevant(capture).some(frame => frame.kind === "interaction_complete");
    }), "every router receives live terminal completion", 12_000);
    const ownerResponse = await fetch(`${fixture.backendUrl}/console/timeline?identity=router%3Amain&mode=recent&limit=1000`);
    assert.equal(ownerResponse.status, 200);
    const owner = (await ownerResponse.json()).frames.filter(frame => frame.interaction_id === interactionId);
    const deltas = frames => frames.filter(frame => frame.kind === "text_delta").map(frame => frame.payload.delta).join("");
    assert.equal(deltas(owner), source, "canonical owner source");
    const ownerIds = owner.filter(frame => frame.source.kind !== "session_history").map(frame => frame.id).sort();
    for (const [index, capture] of captures.entries()) {
      const frames = relevant(capture);
      assert.equal(new Set(frames.map(frame => frame.id)).size, frames.length, `router ${index}: no duplicate canonical frames`);
      assert.equal(deltas(frames), source, `router ${index}: exact delta source`);
      assert.equal(frames.find(frame => frame.kind === "text_complete")?.payload.content, source);
      assert.equal(frames.find(frame => frame.kind === "interaction_complete")?.payload.result, source);
      assert.deepEqual(frames.filter(frame => frame.source.kind !== "session_history").map(frame => frame.id).sort(), ownerIds, `router ${index}: complete owner frame set`);
    }
    evidence.sourceLength = source.length;
    evidence.owner = owner;
    evidence.streams = captures.map(relevant);
  } finally {
    evidence.captures = captures.map(capture => capture.events);
    evidence.logs = fixture.logs();
    const dir = process.env.MOBKIT_BROWSER_EVIDENCE || path.join(__dirname, "../../output/playwright/console-acceptance");
    await fs.mkdir(dir, { recursive: true });
    await fs.writeFile(path.join(dir, "dual-router-stream-parity.json"), JSON.stringify(evidence, null, 2));
    await Promise.all(captures.map(capture => capture.close()));
    await fixture.close();
  }
}

module.exports = { apiScenarios: [
  { id: "api-dual-router-stream-parity", family: "transport", backend: "real", run: sharedOwnerStreamParity },
] };
