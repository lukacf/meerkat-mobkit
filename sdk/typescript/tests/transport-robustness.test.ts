/**
 * Transport robustness (#550, part 1), one test per defect:
 * - a stdout line that is not a JSON object is skipped, not thrown out of the
 *   readline listener;
 * - a reader exit fails every waiter with the typed TransportReaderFailedError
 *   and fails later requests at once;
 * - a callback result with a non-finite number answers a typed error instead
 *   of silently becoming `null`;
 * - a callback request with no registered handler is answered at once.
 */

import { describe, it } from "node:test";
import assert from "node:assert/strict";

import { PersistentTransport } from "../dist/transport.js";
import { TransportError, TransportReaderFailedError } from "../dist/index.js";

function withWrites(transport: PersistentTransport): Array<Record<string, unknown>> {
  const writes: Array<Record<string, unknown>> = [];
  (transport as any)._writeLine = (value: Record<string, unknown>) => {
    writes.push(value);
  };
  return writes;
}

describe("persistent transport robustness", () => {
  it("skips stdout lines that are not JSON objects and keeps delivering responses", () => {
    const transport = new PersistentTransport("unused-test-gateway");
    let delivered: unknown = null;
    (transport as any)._pending.set("r1", {
      resolve: (value: unknown) => {
        delivered = value;
      },
      reject: () => assert.fail("must not reject"),
    });
    for (const line of ["not json", "5", "[1, 2]", "null", "\"text\""]) {
      assert.doesNotThrow(() => (transport as any)._handleLine(line));
    }
    (transport as any)._handleLine('{"jsonrpc":"2.0","id":"r1","result":{"ok":true}}');
    assert.deepEqual(delivered, { jsonrpc: "2.0", id: "r1", result: { ok: true } });
  });

  it("fails every waiter with a typed error when the reader stops, and later requests at once", async () => {
    const transport = new PersistentTransport("unused-test-gateway");
    withWrites(transport);
    (transport as any)._ensureRunning = () => {};
    const waiting = (transport as any)._sendAsyncWithTimeout(
      { jsonrpc: "2.0", id: "w1", method: "mobkit/status" },
      30_000,
    );

    (transport as any)._onReaderClosed("the gateway closed its stdout");

    await assert.rejects(waiting, (error: unknown) => {
      assert.ok(error instanceof TransportReaderFailedError);
      assert.ok(error instanceof TransportError);
      assert.match((error as TransportReaderFailedError).reason, /closed its stdout/);
      return true;
    });
    await assert.rejects(
      (transport as any)._sendAsyncWithTimeout(
        { jsonrpc: "2.0", id: "w2", method: "mobkit/status" },
        30_000,
      ),
      TransportReaderFailedError,
    );
  });

  for (const value of [Number.NaN, Number.POSITIVE_INFINITY, Number.NEGATIVE_INFINITY]) {
    it(`answers a typed error for a callback result containing ${value}`, async () => {
      const transport = new PersistentTransport("unused-test-gateway");
      const writes = withWrites(transport);
      transport.setCallbackHandler(async () => ({ roster: [{ weight: value }] }));

      (transport as any)._handleCallback({
        jsonrpc: "2.0",
        id: "cb-7",
        method: "callback/roster_provider/roster",
        params: {},
      });
      await new Promise<void>((resolve) => setImmediate(resolve));

      assert.equal(writes.length, 1);
      assert.equal(writes[0].id, "cb-7");
      assert.equal("result" in writes[0], false);
      const error = writes[0].error as Record<string, unknown>;
      assert.deepEqual(error.data, { kind: "non_finite_result" });
      assert.match(String(error.message), /NaN or Infinity/);
    });
  }

  it("answers a callback request with no registered handler at once", () => {
    const transport = new PersistentTransport("unused-test-gateway");
    const writes = withWrites(transport);

    (transport as any)._handleCallback({
      jsonrpc: "2.0",
      id: "cb-3",
      method: "callback/build_agent",
      params: {},
    });

    assert.equal(writes.length, 1);
    assert.equal(writes[0].id, "cb-3");
    assert.deepEqual((writes[0].error as Record<string, unknown>).data, {
      kind: "callback_handler_unavailable",
      method: "callback/build_agent",
    });
  });

  it("writes nothing for a notification with no registered handler", () => {
    const transport = new PersistentTransport("unused-test-gateway");
    const writes = withWrites(transport);

    (transport as any)._handleCallback({ jsonrpc: "2.0", method: "notification/x", params: {} });

    assert.equal(writes.length, 0);
  });
});
