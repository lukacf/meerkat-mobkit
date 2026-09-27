"use strict";
const assert = require("node:assert/strict");

function assertScopeCancellation(value) {
  assert(["seed", "repair"].includes(value.phase));
  assert.notEqual(value.oldScope, value.replacementScope);
  assert(value.held.length > 0, "an actual successful old history response was held");
  const oldCursors = new Set();
  for (const held of value.held) {
    assert.equal(held.scope, value.oldScope);
    assert.equal(held.status, 200, "hold real successful owner history, not a fabricated fault");
    assert(held.heldAt <= value.switchedAt);
    assert(held.abortedAt >= value.switchedAt, "the scope switch cancels the old socket");
    assert(held.releasedAt >= held.abortedAt, "release happens after observed cancellation");
    const page = JSON.parse(held.response).result;
    assert(page?.frames?.some(frame => JSON.stringify(frame.payload).includes(value.oldMarker)), "held bytes contain actual old owner history");
    if (page.latest_cursor) oldCursors.add(page.latest_cursor);
  }
  assert(value.replacementFrames.some(frame => JSON.stringify(frame.payload).includes(value.replacementMarker)));
  assert(!value.replacementFrames.some(frame => JSON.stringify(frame.payload).includes(value.oldMarker)));
  assert(value.transcript.includes(value.replacementMarker), "replacement authorized history is visible");
  assert(!value.transcript.includes(value.oldMarker), "old history cannot enter the replacement");
  assert.equal(new Set(value.rows).size, value.rows.length, "replacement rows are not duplicated");
  assert.equal(value.draft, "", "old draft is cleared on authority change");
  assert.equal(value.contextCount, 0, "old quote snapshots are not visible in replacement scope");
  assert.equal(value.phaseAfter, "live");
  assert.deepEqual(value.errors, []);
  assert.deepEqual(value.modelRequestCounts.after, value.modelRequestCounts.before, "read-only scope switch does not dispatch model work");
  const streams = value.observations.filter(item => item.path.startsWith("/console/timeline/stream"));
  const replacement = streams.filter(item => item.scope === value.replacementScope);
  assert(replacement.some(item => item.status === 200), "new authority establishes an actual stream");
  assert(value.replacementCursor, "replacement owner supplies an accepted snapshot cursor");
  for (const stream of replacement) {
    assert(!oldCursors.has(stream.cursor), "old accepted cursor cannot transfer to replacement stream");
    assert.equal(stream.cursor, value.replacementCursor, "replacement stream resumes at its own real owner frontier");
  }
  assert(!streams.some(item => item.scope === value.oldScope && item.startedAt > value.switchedAt), "disposed authority does not open another stream");
  if (value.phase === "repair") {
    assert(streams.some(item => item.scope === value.oldScope && item.status === 409 && item.startedAt < value.switchedAt), "repair follows a genuine expired replay response");
  }
  for (const item of value.observations) {
    let body;
    try { body = JSON.parse(item.request || "{}"); } catch { continue; }
    assert(body.method !== "mobkit/console/send" && !item.path.endsWith("/send"), "scope switch cannot automatically send");
  }
}

module.exports = { assertScopeCancellation };
