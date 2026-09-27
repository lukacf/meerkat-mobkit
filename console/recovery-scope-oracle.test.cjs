"use strict";
const assert = require("node:assert/strict");
const { test } = require("node:test");
const { assertScopeCancellation } = require("./recovery-scope-oracle.cjs");

function validEvidence() {
  return {
    phase: "repair", oldScope: "scope-a", replacementScope: "scope-b", switchedAt: 30,
    oldMarker: "Private old owner record", replacementMarker: "Replacement owner record",
    held: [{ scope: "scope-a", status: 200, heldAt: 20, abortedAt: 31, releasedAt: 40,
      response: JSON.stringify({ result: { frames: [{ id: "old:frame", payload: { text: "Private old owner record" } }], latest_cursor: "old:cursor" } }) }],
    observations: [
      { scope: "scope-a", path: "/console/timeline/stream", status: 409, startedAt: 15 },
      { scope: "scope-b", path: "/console/rpc", method: "POST", startedAt: 32, status: 200,
        request: JSON.stringify({ method: "mobkit/console/query_timeline", params: { mode: "recent" } }) },
      { scope: "scope-b", path: "/console/timeline/stream", method: "GET", startedAt: 34, status: 200, cursor: "new:cursor" },
    ],
    replacementFrames: [{ id: "new:frame", payload: { text: "Replacement owner record" } }], replacementCursor: "new:cursor",
    transcript: "Replacement owner record", rows: ["new:frame"], draft: "", contextCount: 0,
    phaseAfter: "live", errors: [], modelRequestCounts: { before: [2, 2], after: [2, 2] },
  };
}

test("accepts actual old-socket cancellation with isolated replacement history", () => {
  assert.doesNotThrow(() => assertScopeCancellation(validEvidence()));
});

for (const [name, corrupt] of [
  ["uncancelled held query", value => { delete value.held[0].abortedAt; }],
  ["query cancelled before the scope switch", value => { value.held[0].abortedAt = 29; }],
  ["response released before cancellation", value => { value.held[0].releasedAt = 30; }],
  ["fabricated or failed history response", value => { value.held[0].status = 503; }],
  ["old transcript injected after replacement", value => { value.transcript += value.oldMarker; }],
  ["replacement history absent", value => { value.transcript = "Loading"; }],
  ["old cursor transferred to new stream", value => { value.observations[2].cursor = "old:cursor"; }],
  ["replacement stream skips its owner frontier", value => { value.observations[2].cursor = "invented:cursor"; }],
  ["old scope opens another subscription", value => { value.observations.push({ scope: "scope-a", path: "/console/timeline/stream", startedAt: 41 }); }],
  ["scope switch submits a message", value => { value.observations.push({ scope: "scope-b", path: "/console/rpc", method: "POST", startedAt: 35, request: JSON.stringify({ method: "mobkit/console/send" }) }); }],
  ["owner receives an unintended model request", value => { value.modelRequestCounts.after[1]++; }],
  ["old draft remains", value => { value.draft = "old draft"; }],
  ["old quote remains", value => { value.contextCount = 1; }],
  ["hidden replacement error", value => { value.errors = ["ERR_FAILED"]; }],
  ["replacement did not resume", value => { value.phaseAfter = "retrying"; }],
  ["repair was not caused by real expired replay", value => { value.observations[0].status = 200; }],
  ["duplicate replacement rows", value => { value.rows.push(value.rows[0]); }],
]) {
  test(`rejects ${name}`, () => {
    const evidence = validEvidence(); corrupt(evidence);
    assert.throws(() => assertScopeCancellation(evidence));
  });
}

test("initial seed needs no fabricated replay gap", () => {
  const evidence = validEvidence(); evidence.phase = "seed";
  evidence.observations = evidence.observations.slice(1);
  assert.doesNotThrow(() => assertScopeCancellation(evidence));
});
