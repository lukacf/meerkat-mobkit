/**
 * Deterministic lifecycle-policy tests for PersistentTransport.
 */

import { describe, it } from "node:test";
import assert from "node:assert/strict";
import type { ChildProcess } from "node:child_process";

import {
  PersistentTransport,
  PERSISTENT_TRANSPORT_SHUTDOWN_GRACE_MS,
  stopChildProcess,
} from "../dist/transport.js";

interface FakeChild {
  readonly child: ChildProcess;
  readonly signals: NodeJS.Signals[];
  stdinEnded: number;
}

function fakeChild(): FakeChild {
  const signals: NodeJS.Signals[] = [];
  const state = {
    stdinEnded: 0,
  };
  const child = {
    stdin: {
      end: () => {
        state.stdinEnded += 1;
      },
    },
    exitCode: null,
    signalCode: null,
    kill: (signal: NodeJS.Signals) => {
      signals.push(signal);
      return true;
    },
  } as unknown as ChildProcess;

  return {
    child,
    signals,
    get stdinEnded() {
      return state.stdinEnded;
    },
    set stdinEnded(value: number) {
      state.stdinEnded = value;
    },
  };
}

describe("persistent transport shutdown", () => {
  it("rejects new RPC admission while a stop is in flight", async () => {
    const transport = new PersistentTransport("unused-test-gateway");
    (transport as any)._stopping = new Promise<void>(() => {});

    await assert.rejects(
      transport.sendAsync({
        jsonrpc: "2.0",
        id: "during-stop",
        method: "mobkit/status",
        params: {},
      }),
      /persistent transport is stopping/,
    );
    assert.throws(
      () => transport.start(),
      /persistent transport is stopping/,
    );
  });

  it("allows the gateway a 60-second graceful drain without signaling it", async () => {
    const fake = fakeChild();
    const waits: number[] = [];

    await stopChildProcess(fake.child, async (_child, timeoutMs) => {
      waits.push(timeoutMs);
      return true;
    });

    assert.equal(PERSISTENT_TRANSPORT_SHUTDOWN_GRACE_MS, 60_000);
    assert.equal(fake.stdinEnded, 1);
    assert.deepEqual(waits, [60_000]);
    assert.deepEqual(fake.signals, []);
  });

  it("uses bounded SIGTERM then SIGKILL fallback when drain stalls", async () => {
    const fake = fakeChild();
    const waits: number[] = [];
    const outcomes = [false, false, true];

    await stopChildProcess(fake.child, async (_child, timeoutMs) => {
      waits.push(timeoutMs);
      return outcomes.shift() ?? true;
    });

    assert.equal(fake.stdinEnded, 1);
    assert.deepEqual(waits, [60_000, 5_000, 5_000]);
    assert.deepEqual(fake.signals, ["SIGTERM", "SIGKILL"]);
  });

  it("does not force-kill a gateway that exits after SIGTERM", async () => {
    const fake = fakeChild();
    const waits: number[] = [];
    const outcomes = [false, true];

    await stopChildProcess(fake.child, async (_child, timeoutMs) => {
      waits.push(timeoutMs);
      return outcomes.shift() ?? true;
    });

    assert.deepEqual(waits, [60_000, 5_000]);
    assert.deepEqual(fake.signals, ["SIGTERM"]);
  });
});
