/**
 * Turn-completion contract for the TypeScript SDK.
 *
 * The defect this mirrors: a consumer captured the previous turn's output text
 * as a baseline, sent again, and waited for the text to change. Two turns that
 * both answer exactly `ACK` are indistinguishable from no turn at all, so the
 * wait sleeps out its whole timeout. Completion is a cursor —
 * `{epoch, turns}` — never a text comparison.
 */

import { describe, it } from "node:test";
import assert from "node:assert/strict";

import type { CompletionCursor } from "../src/types.js";

type ScriptedInspection = {
  output_preview: string | null;
  completion_cursor: { epoch: number; turns: number } | null;
};

/**
 * Runtime whose `_rpc` answers from a script. `inspect_identity` walks one
 * entry per poll and holds the last, so "still running, then done" is
 * expressible without racing a clock.
 */
async function makeRuntime(script: {
  send?: Record<string, unknown>;
  dispatch?: Record<string, unknown>;
  inspections?: ScriptedInspection[];
}) {
  const { MobKitRuntime } = await import("../src/runtime.js");
  const calls: { method: string; params: Record<string, unknown> }[] = [];
  const rt = new MobKitRuntime({
    mobConfigPath: null,
    sessionBuilder: null,
    sessionStore: null,
    errorCallback: null,
    eventLog: null,
    consoleConfigPath: null,
    consoleRequireAppAuth: null,
    consoleReadOnly: null,
    consoleFetchTimeoutMs: null,
    gatingConfigPath: null,
    routingConfigPath: null,
    memoryConfig: null,
    authConfig: null,
    implicitDelegateIdleRetireSecs: undefined,
    maxSessions: null,
    gatewayBin: null,
    modules: [],
    persistentState: null,
    continuityStore: null,
    leaseProvider: null,
    scratchDir: null,
    rosterProvider: null,
    agentCustomizer: null,
    topologyProvider: null,
  });
  const inspections = script.inspections ?? [];
  let inspectIndex = 0;
  (rt as unknown as Record<string, unknown>)._rpc = async (
    method: string,
    params?: Record<string, unknown>,
  ) => {
    calls.push({ method, params: params ?? {} });
    if (method === "mobkit/send") return script.send ?? {};
    if (method === "mobkit/dispatch") return script.dispatch ?? {};
    if (method === "mobkit/inspect_identity") {
      const entry = inspections[Math.min(inspectIndex, inspections.length - 1)];
      inspectIndex += 1;
      return { identity: params?.identity ?? "x:1", is_final: false, ...entry };
    }
    return { accepted: true };
  };
  (rt as unknown as Record<string, unknown>)._running = true;
  const inspectCalls = () =>
    calls.filter((c) => c.method === "mobkit/inspect_identity").length;
  return { rt, calls, inspectCalls };
}

// ---------------------------------------------------------------------------
// The production regression
// ---------------------------------------------------------------------------

describe("identical consecutive output", () => {
  it("detects the second turn when both answer exactly ACK", async () => {
    const { rt, inspectCalls } = await makeRuntime({
      send: { fencing_token: 7, completion_baseline: { epoch: 7, turns: 1 } },
      inspections: [
        // Turn 2 in flight — the PREVIOUS turn's ACK is still visible.
        { output_preview: "ACK", completion_cursor: { epoch: 7, turns: 1 } },
        // Turn 2 committed. Same text, byte for byte.
        { output_preview: "ACK", completion_cursor: { epoch: 7, turns: 2 } },
      ],
    });

    const output = await rt.sendAndWait("triage:main", "ping", {
      timeoutMs: 5000,
      pollIntervalMs: 1,
    });

    assert.equal(output, "ACK");
    assert.equal(
      inspectCalls(),
      2,
      "the waiter must poll past the first (unchanged) inspection",
    );
  });

  it("dispatchAndWait threads its own baseline", async () => {
    const { rt, inspectCalls } = await makeRuntime({
      dispatch: {
        fencing_token: 4,
        durable: true,
        completion_baseline: { epoch: 4, turns: 5 },
      },
      inspections: [
        { output_preview: "ACK", completion_cursor: { epoch: 4, turns: 5 } },
        { output_preview: "ACK", completion_cursor: { epoch: 4, turns: 6 } },
      ],
    });

    const output = await rt.dispatchAndWait(
      "internal:main",
      { content: "go", origin: "system" },
      { timeoutMs: 5000, pollIntervalMs: 1 },
    );

    assert.equal(output, "ACK");
    assert.equal(inspectCalls(), 2);
  });
});

// ---------------------------------------------------------------------------
// Waiter semantics
// ---------------------------------------------------------------------------

describe("waitForCompletion", () => {
  it("times out on a genuinely stalled turn", async () => {
    const { rt } = await makeRuntime({
      inspections: [
        { output_preview: "ACK", completion_cursor: { epoch: 3, turns: 1 } },
      ],
    });

    await assert.rejects(
      rt.waitForCompletion(
        "triage:main",
        { epoch: 3, turns: 1 },
        { timeoutMs: 50, pollIntervalMs: 1 },
      ),
      /did not complete a turn/,
    );
  });

  it("reports an incarnation change rather than guessing", async () => {
    const { rt } = await makeRuntime({
      inspections: [
        { output_preview: "ACK", completion_cursor: { epoch: 9, turns: 0 } },
      ],
    });

    await assert.rejects(
      rt.waitForCompletion(
        "triage:main",
        { epoch: 3, turns: 1 },
        { timeoutMs: 5000, pollIntervalMs: 1 },
      ),
      /superseded runtime incarnation/,
    );
  });

  it("fails loudly against a gateway with no cursor", async () => {
    const { rt } = await makeRuntime({
      send: { fencing_token: 7 },
      inspections: [{ output_preview: "ACK", completion_cursor: null }],
    });

    await assert.rejects(
      rt.sendAndWait("triage:main", "ping", {
        timeoutMs: 1000,
        pollIntervalMs: 1,
      }),
      /no completion_baseline/,
    );
  });

  it("a different identity's completion does not satisfy the wait", async () => {
    // The cursor is per-identity: this identity's cursor never moves, so the
    // wait must time out no matter what any other identity did.
    const { rt, calls } = await makeRuntime({
      inspections: [
        { output_preview: "ACK", completion_cursor: { epoch: 4, turns: 2 } },
      ],
    });

    await assert.rejects(
      rt.waitForCompletion(
        "triage:main",
        { epoch: 4, turns: 2 },
        { timeoutMs: 50, pollIntervalMs: 1 },
      ),
      /did not complete a turn/,
    );
    assert.ok(
      calls.every((c) => c.params.identity === "triage:main"),
      "the waiter must only ever poll its own identity",
    );
  });
});

// ---------------------------------------------------------------------------
// Cursor value semantics
// ---------------------------------------------------------------------------

describe("CompletionCursor", () => {
  it("classifies progress by cursor, not content", async () => {
    const { completionProgressSince } = await import("../src/types.js");
    const baseline: CompletionCursor = { epoch: 2, turns: 3 };

    assert.equal(completionProgressSince(baseline, baseline), "pending");
    assert.equal(
      completionProgressSince({ epoch: 2, turns: 4 }, baseline),
      "completed",
    );
    assert.equal(
      completionProgressSince({ epoch: 3, turns: 0 }, baseline),
      "incarnation_changed",
    );
  });

  it("round-trips", async () => {
    const { parseCompletionCursor, completionCursorToDict } = await import(
      "../src/types.js"
    );
    const cursor = parseCompletionCursor({ epoch: 12, turns: 34 });
    assert.deepEqual(cursor, { epoch: 12, turns: 34 });
    assert.deepEqual(completionCursorToDict(cursor), { epoch: 12, turns: 34 });
  });
});

// ---------------------------------------------------------------------------
// Wire mirrors: both directions, both fields
// ---------------------------------------------------------------------------

describe("model mirrors carry the cursor in both directions", () => {
  it("IdentityInspection", async () => {
    const { parseIdentityInspection, identityInspectionToDict } = await import(
      "../src/types.js"
    );
    const payload = {
      identity: "triage:main",
      output_preview: "ACK",
      is_final: false,
      peer_reachable_count: 0,
      completion_cursor: { epoch: 7, turns: 2 },
    };

    const parsed = parseIdentityInspection(payload);
    assert.deepEqual(parsed.completionCursor, { epoch: 7, turns: 2 });
    assert.deepEqual(identityInspectionToDict(parsed), payload);
    assert.deepEqual(
      parseIdentityInspection(identityInspectionToDict(parsed)),
      parsed,
    );
  });

  it("DispatchResult", async () => {
    const { parseDispatchResult, dispatchResultToDict } = await import(
      "../src/types.js"
    );
    const payload = {
      fencing_token: 4,
      durable: true,
      completion_baseline: { epoch: 4, turns: 5 },
    };

    const parsed = parseDispatchResult(payload);
    assert.deepEqual(parsed.completionBaseline, { epoch: 4, turns: 5 });
    assert.deepEqual(dispatchResultToDict(parsed), payload);
    assert.deepEqual(parseDispatchResult(dispatchResultToDict(parsed)), parsed);
  });

  it("SendResult", async () => {
    const { parseSendResult, sendResultToDict } = await import(
      "../src/types.js"
    );
    const payload = {
      fencing_token: 4,
      completion_baseline: { epoch: 4, turns: 5 },
    };

    const parsed = parseSendResult(payload);
    assert.deepEqual(parsed.completionBaseline, { epoch: 4, turns: 5 });
    assert.deepEqual(sendResultToDict(parsed), payload);
    assert.deepEqual(parseSendResult(sendResultToDict(parsed)), parsed);
  });

  it("payloads without the field still parse, and absence stays null", async () => {
    const { parseIdentityInspection, parseDispatchResult, parseSendResult } =
      await import("../src/types.js");

    const inspection = parseIdentityInspection({
      identity: "triage:main",
      output_preview: "ACK",
      is_final: true,
    });
    assert.equal(inspection.completionCursor, null);
    assert.equal(inspection.outputPreview, "ACK");
    assert.equal(inspection.isFinal, true);

    assert.equal(
      parseDispatchResult({ fencing_token: 2, durable: false })
        .completionBaseline,
      null,
    );
    assert.equal(parseSendResult({ fencing_token: 2 }).completionBaseline, null);
  });

  it("an explicit null cursor reads as absent, not as zero turns", async () => {
    const { parseIdentityInspection } = await import("../src/types.js");
    const inspection = parseIdentityInspection({
      identity: "live:alias",
      completion_cursor: null,
    });
    assert.equal(inspection.completionCursor, null);
  });
});
