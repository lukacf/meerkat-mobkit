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
  it("negotiates and validates the advertised shutdown horizon", async () => {
    const transport = new PersistentTransport("unused-test-gateway");
    (transport as any)._sendAsyncWithTimeout = async () => ({
      jsonrpc: "2.0",
      id: "init-horizon",
      result: {
        stdio_shutdown_handshake: true,
        stdio_shutdown_horizon_ms: 321_000,
      },
    });

    await transport.sendAsync({
      jsonrpc: "2.0",
      id: "init-horizon",
      method: "mobkit/init",
      params: {},
    });

    assert.equal((transport as any)._supportsShutdownHandshake, true);
    assert.equal((transport as any)._shutdownHorizonMs, 321_000);

    for (const invalid of [undefined, true, 0, -1, 1.5, "310000"]) {
      (transport as any)._sendAsyncWithTimeout = async () => ({
        jsonrpc: "2.0",
        id: "init-invalid-horizon",
        result: {
          stdio_shutdown_handshake: true,
          stdio_shutdown_horizon_ms: invalid,
        },
      });
      await transport.sendAsync({
        jsonrpc: "2.0",
        id: "init-invalid-horizon",
        method: "mobkit/init",
        params: {},
      });
      assert.equal(
        (transport as any)._shutdownHorizonMs,
        PERSISTENT_TRANSPORT_SHUTDOWN_GRACE_MS,
      );
    }
  });

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

  it("honors the negotiated horizon and keeps stdin open for a gated callback", async () => {
    const fake = fakeChild();
    const transport = new PersistentTransport("unused-test-gateway");
    (transport as any)._process = fake.child;
    (transport as any)._supportsShutdownHandshake = true;
    (transport as any)._shutdownHorizonMs = 321_000;

    let handshakeObserved = false;
    let releaseHandshake!: () => void;
    let markHandshakeEntered!: () => void;
    const handshakeEntered = new Promise<void>((resolve) => {
      markHandshakeEntered = resolve;
    });
    const handshakeGate = new Promise<void>((resolve) => {
      releaseHandshake = resolve;
    });
    (transport as any)._sendAsyncWithTimeout = async (
      request: Record<string, unknown>,
      timeoutMs: number,
      expectedChild: ChildProcess,
    ) => {
      assert.equal(fake.stdinEnded, 0);
      assert.equal(request.method, "mobkit/shutdown");
      assert.match(String(request.id), /^mobkit-shutdown-/);
      assert.equal(timeoutMs, 321_000);
      assert.equal(expectedChild, fake.child);
      handshakeObserved = true;
      markHandshakeEntered();
      await handshakeGate;
      (fake.child as any).exitCode = 0;
      return { jsonrpc: "2.0", id: request.id, result: { shutdown: true } };
    };

    const stopping = transport.stop();
    await handshakeEntered;
    assert.equal(fake.stdinEnded, 0);
    releaseHandshake();
    await stopping;

    assert.equal(handshakeObserved, true);
    assert.equal(fake.stdinEnded, 1);
    assert.equal((transport as any)._process, null);
  });

  it("keeps older gateways on the EOF shutdown protocol", async () => {
    const fake = fakeChild();
    const transport = new PersistentTransport("unused-test-gateway");
    (transport as any)._process = fake.child;
    (fake.child as any).exitCode = 0;

    let handshakeAttempts = 0;
    (transport as any)._sendAsyncWithTimeout = async () => {
      handshakeAttempts += 1;
    };

    await transport.stop();

    assert.equal(handshakeAttempts, 0);
    assert.equal(fake.stdinEnded, 1);
  });

  it("propagates incomplete runtime cleanup only after reaping the gateway", async () => {
    const fake = fakeChild();
    const transport = new PersistentTransport("unused-test-gateway");
    (transport as any)._process = fake.child;
    (transport as any)._supportsShutdownHandshake = true;
    (transport as any)._shutdownHorizonMs = 321_000;
    (transport as any)._sendAsyncWithTimeout = async () => {
      (fake.child as any).exitCode = 0;
      return {
        jsonrpc: "2.0",
        id: "shutdown-failed",
        result: {
          shutdown: false,
          runtime_cleanup_completed: false,
        },
      };
    };

    await assert.rejects(
      transport.stop(),
      /gateway shutdown failed after bounded cleanup/,
    );

    assert.equal(fake.stdinEnded, 1);
    assert.equal((transport as any)._process, null);
  });

  it("allows the gateway its full negotiated-safe drain without signaling it", async () => {
    const fake = fakeChild();
    const waits: number[] = [];

    await stopChildProcess(fake.child, async (_child, timeoutMs) => {
      waits.push(timeoutMs);
      return true;
    });

    assert.equal(PERSISTENT_TRANSPORT_SHUTDOWN_GRACE_MS, 310_000);
    assert.equal(fake.stdinEnded, 1);
    assert.deepEqual(waits, [310_000]);
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
    assert.deepEqual(waits, [310_000, 5_000, 5_000]);
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

    assert.deepEqual(waits, [310_000, 5_000]);
    assert.deepEqual(fake.signals, ["SIGTERM"]);
  });
});
