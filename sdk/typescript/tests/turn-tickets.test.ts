/**
 * Per-admission completion for the TypeScript SDK.
 *
 * The defect: `sendAndWait` waited until the identity-wide completion cursor
 * passed the send's baseline, then returned the session's latest
 * `outputPreview`. A concurrent delivery (a peer message, a scheduled turn,
 * a fork completion wake) completing first satisfied the wait, and the caller
 * got someone else's output.
 *
 * `sendAndWait` / `dispatchAndWait` now send with `trackTurn` and wait on the
 * returned ticket through `mobkit/turn_result`, which reports THAT turn and
 * its own output.
 */

import { describe, it } from "node:test";
import assert from "node:assert/strict";

import { TurnFailedError, TurnUnknownError } from "../src/errors.js";
import {
  dispatchResultToDict,
  parseDispatchResult,
  parseSendResult,
  parseTurnResult,
  sendResultToDict,
} from "../src/types.js";

type TurnScript = Record<string, Record<string, unknown>[]>;

const PENDING = { state: "pending" };

function completed(output: string | null): Record<string, unknown> {
  return {
    state: "completed",
    output,
    output_available: true,
    output_truncated: false,
  };
}

function sent(
  ticket: string | null,
  extra: Record<string, unknown> = {},
): Record<string, unknown> {
  return {
    fencing_token: 3,
    completion_baseline: { epoch: 3, turns: 0 },
    ...(ticket !== null ? { turn: { ticket } } : {}),
    ...extra,
  };
}

function inspection(preview: string | null, turns: number) {
  return {
    output_preview: preview,
    completion_cursor: { epoch: 3, turns },
  };
}

/**
 * Runtime whose `_rpc` answers from a script: `sends` is consumed one per
 * send/dispatch, `turns` maps a ticket to the states `mobkit/turn_result`
 * walks through (one per poll, holding the last), and `inspections` models the
 * identity-wide cursor moving for someone else's turn.
 */
async function makeRuntime(script: {
  sends?: Record<string, unknown>[];
  turns?: TurnScript;
  inspections?: ReturnType<typeof inspection>[];
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
  const sends = [...(script.sends ?? [])];
  const turns = script.turns ?? {};
  const polls = new Map<string, number>();
  const inspections = script.inspections ?? [];
  let inspectIndex = 0;
  (rt as unknown as Record<string, unknown>)._rpc = async (
    method: string,
    params?: Record<string, unknown>,
  ) => {
    await Promise.resolve();
    const p = params ?? {};
    calls.push({ method, params: p });
    if (method === "mobkit/send" || method === "mobkit/dispatch") {
      return sends.shift() ?? {};
    }
    if (method === "mobkit/turn_result") {
      const ticket = String(p.ticket);
      const states = turns[ticket] ?? [{ state: "unknown" }];
      const n = polls.get(ticket) ?? 0;
      polls.set(ticket, n + 1);
      return {
        identity: p.identity,
        ticket,
        ...states[Math.min(n, states.length - 1)],
      };
    }
    if (method === "mobkit/inspect_identity") {
      const entry = inspections[Math.min(inspectIndex, inspections.length - 1)];
      inspectIndex += 1;
      return { identity: p.identity, is_final: false, ...entry };
    }
    return {};
  };
  (rt as unknown as Record<string, unknown>)._running = true;
  const paramsOf = (method: string) =>
    calls.filter((c) => c.method === method).map((c) => c.params);
  return { rt, calls, paramsOf };
}

const FAST = { timeoutMs: 5_000, pollIntervalMs: 1 };

async function withWarnings<T>(
  run: () => Promise<T>,
): Promise<{ value: T; warnings: string[] }> {
  const warnings: string[] = [];
  const original = console.warn;
  console.warn = (...args: unknown[]) => {
    warnings.push(args.map(String).join(" "));
  };
  try {
    return { value: await run(), warnings };
  } finally {
    console.warn = original;
  }
}

describe("sendAndWait waits for its own turn", () => {
  it("is not satisfied by a foreign completion", async () => {
    const { rt, paramsOf } = await makeRuntime({
      sends: [sent("ticket-a")],
      turns: { "ticket-a": [PENDING, PENDING, completed("A's reply")] },
      // The identity-wide cursor already moved for a foreign turn.
      inspections: [inspection("foreign reply", 1)],
    });

    const output = await rt.sendAndWait("keeper", "alpha", FAST);

    assert.equal(output, "A's reply");
    assert.equal(paramsOf("mobkit/send")[0]?.track_turn, true);
    assert.deepEqual(
      paramsOf("mobkit/inspect_identity"),
      [],
      "a ticketed wait must not read the identity-wide cursor",
    );
    assert.equal(paramsOf("mobkit/turn_result").length, 3);
  });

  it("gives concurrent sends each their own output", async () => {
    const { rt } = await makeRuntime({
      sends: [sent("ticket-a"), sent("ticket-b")],
      turns: {
        "ticket-a": [PENDING, PENDING, PENDING, completed("A's reply")],
        "ticket-b": [PENDING, completed("B's reply")],
      },
    });

    const outputs = await Promise.all([
      rt.sendAndWait("keeper", "alpha", FAST),
      rt.sendAndWait("keeper", "beta", FAST),
    ]);

    assert.deepEqual(outputs, ["A's reply", "B's reply"]);
  });

  it("control: the identity-wide cursor wait returns the foreign preview", async () => {
    const { rt } = await makeRuntime({
      inspections: [inspection("foreign reply", 1)],
    });

    const output = await rt.waitForCompletion(
      "keeper",
      { epoch: 3, turns: 0 },
      FAST,
    );

    assert.equal(output, "foreign reply");
  });

  it("dispatchAndWait waits on its ticket", async () => {
    const { rt, paramsOf } = await makeRuntime({
      sends: [sent("ticket-d", { durable: true })],
      turns: { "ticket-d": [completed("dispatched reply")] },
    });

    const output = await rt.dispatchAndWait(
      "keeper",
      { content: "work", origin: "system" },
      FAST,
    );

    assert.equal(output, "dispatched reply");
    assert.equal(paramsOf("mobkit/dispatch")[0]?.track_turn, true);
  });
});

describe("waitForTurn", () => {
  it("throws TurnFailedError for a failed turn", async () => {
    const { rt } = await makeRuntime({
      turns: { "ticket-a": [{ state: "failed", error: "the model refused" }] },
    });

    await assert.rejects(rt.waitForTurn("keeper", "ticket-a", FAST), (error) => {
      assert.ok(error instanceof TurnFailedError);
      assert.equal(error.reason, "the model refused");
      return true;
    });
  });

  it("throws TurnUnknownError and never guesses", async () => {
    const { rt } = await makeRuntime({
      turns: { "ticket-a": [{ state: "unknown" }] },
    });

    await assert.rejects(
      rt.waitForTurn("keeper", "ticket-a", FAST),
      TurnUnknownError,
    );
  });

  it("times out on a pending turn", async () => {
    const { rt } = await makeRuntime({ turns: { "ticket-a": [PENDING] } });

    await assert.rejects(
      rt.waitForTurn("keeper", "ticket-a", { timeoutMs: 20, pollIntervalMs: 1 }),
      /did not complete/,
    );
  });

  it("resolves null for a turn that committed no text", async () => {
    const { rt } = await makeRuntime({ turns: { "ticket-a": [completed(null)] } });

    assert.equal(await rt.waitForTurn("keeper", "ticket-a", FAST), null);
  });
});

describe("explicit fallback", () => {
  it("falls back to the cursor with a warning on an old gateway", async () => {
    const { rt, paramsOf } = await makeRuntime({
      sends: [sent(null)],
      inspections: [inspection("latest reply", 1)],
    });

    const { value, warnings } = await withWarnings(() =>
      rt.sendAndWait("keeper", "alpha", FAST),
    );

    assert.equal(value, "latest reply");
    assert.equal(warnings.length, 1);
    assert.match(warnings[0] ?? "", /predates turn tickets/);
    assert.deepEqual(paramsOf("mobkit/turn_result"), []);
  });

  it("names why a turn could not be tracked", async () => {
    const { rt } = await makeRuntime({
      sends: [sent(null, { turn: null, turn_unavailable: "remotely hosted member" })],
      inspections: [inspection("latest reply", 1)],
    });

    const { warnings } = await withWarnings(() =>
      rt.sendAndWait("keeper", "alpha", FAST),
    );

    assert.match(warnings[0] ?? "", /remotely hosted member/);
  });

  it("a plain send does not request tracking", async () => {
    const { rt, paramsOf } = await makeRuntime({ sends: [sent(null)] });

    const result = await rt.send("keeper", "alpha");

    assert.equal(paramsOf("mobkit/send")[0]?.track_turn, undefined);
    assert.equal(result.turnTicket, null);
  });
});

describe("models", () => {
  it("carry the ticket and round-trip", () => {
    const send = parseSendResult(sent("ticket-a"));
    assert.equal(send.turnTicket, "ticket-a");
    assert.deepEqual(parseSendResult(sendResultToDict(send)), send);
    const unavailable = parseDispatchResult(
      sent(null, { durable: false, turn: null, turn_unavailable: "no bridge" }),
    );
    assert.equal(unavailable.turnTicket, null);
    assert.equal(unavailable.turnUnavailable, "no bridge");
    assert.deepEqual(
      parseDispatchResult(dispatchResultToDict(unavailable)),
      unavailable,
    );
  });

  it("parse every turn state", () => {
    const done = parseTurnResult({
      identity: "keeper",
      ticket: "t",
      ...completed("hi"),
      completion_cursor: { epoch: 3, turns: 4 },
    });
    assert.equal(done.state, "completed");
    assert.equal(done.output, "hi");
    assert.deepEqual(done.completionCursor, { epoch: 3, turns: 4 });
    const failed = parseTurnResult({ state: "failed", error: "boom" });
    assert.equal(failed.state, "failed");
    assert.equal(failed.error, "boom");
    assert.equal(parseTurnResult({ state: "surprise" }).state, "unknown");
  });
});
