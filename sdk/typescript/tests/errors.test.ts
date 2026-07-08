import { describe, it } from "node:test";
import assert from "node:assert/strict";

import {
  MobKitError,
  TransportError,
  RpcError,
  CapabilityUnavailableError,
  ConsoleTimelineReplayUnavailableError,
  WorkGraphUnavailableError,
  WorkGraphConflictError,
  ContractMismatchError,
  LeaseLostError,
  NotConnectedError,
  MobkitRpcError,
} from "../dist/index.js";

// ---------------------------------------------------------------------------
// MobKitError (base)
// ---------------------------------------------------------------------------

describe("MobKitError", () => {
  it("is an instance of Error", () => {
    const err = new MobKitError("base error");
    assert.ok(err instanceof Error);
  });

  it("is an instance of MobKitError", () => {
    const err = new MobKitError("base error");
    assert.ok(err instanceof MobKitError);
  });

  it("has name set to MobKitError", () => {
    const err = new MobKitError("msg");
    assert.equal(err.name, "MobKitError");
  });

  it("has the correct message", () => {
    const err = new MobKitError("something went wrong");
    assert.equal(err.message, "something went wrong");
  });
});

// ---------------------------------------------------------------------------
// TransportError
// ---------------------------------------------------------------------------

describe("TransportError", () => {
  it("extends MobKitError", () => {
    const err = new TransportError("transport fail");
    assert.ok(err instanceof MobKitError);
  });

  it("is an instance of Error", () => {
    const err = new TransportError("transport fail");
    assert.ok(err instanceof Error);
  });

  it("has name set to TransportError", () => {
    const err = new TransportError("msg");
    assert.equal(err.name, "TransportError");
  });

  it("has the correct message", () => {
    const err = new TransportError("connection refused");
    assert.equal(err.message, "connection refused");
  });
});

// ---------------------------------------------------------------------------
// RpcError
// ---------------------------------------------------------------------------

describe("RpcError", () => {
  it("extends MobKitError", () => {
    const err = new RpcError(-32600, "invalid request", "req-1", "mob.status");
    assert.ok(err instanceof MobKitError);
  });

  it("is an instance of Error", () => {
    const err = new RpcError(-32600, "invalid request", "req-1", "mob.status");
    assert.ok(err instanceof Error);
  });

  it("has name set to RpcError", () => {
    const err = new RpcError(-32600, "msg", "req-1", "mob.status");
    assert.equal(err.name, "RpcError");
  });

  it("stores code", () => {
    const err = new RpcError(-32601, "not found", "req-2", "mob.spawn");
    assert.equal(err.code, -32601);
  });

  it("stores requestId", () => {
    const err = new RpcError(-32600, "bad", "req-42", "mob.status");
    assert.equal(err.requestId, "req-42");
  });

  it("stores method", () => {
    const err = new RpcError(-32600, "bad", "req-1", "mob.reconcile");
    assert.equal(err.method, "mob.reconcile");
  });

  it("has the correct message", () => {
    const err = new RpcError(-32600, "invalid params", "r", "m");
    assert.equal(err.message, "invalid params");
  });
});

// ---------------------------------------------------------------------------
// CapabilityUnavailableError
// ---------------------------------------------------------------------------

describe("CapabilityUnavailableError", () => {
  it("extends MobKitError", () => {
    const err = new CapabilityUnavailableError("no memory");
    assert.ok(err instanceof MobKitError);
  });

  it("is an instance of Error", () => {
    const err = new CapabilityUnavailableError("no memory");
    assert.ok(err instanceof Error);
  });

  it("has name set to CapabilityUnavailableError", () => {
    const err = new CapabilityUnavailableError("msg");
    assert.equal(err.name, "CapabilityUnavailableError");
  });
});

// ---------------------------------------------------------------------------
// LeaseLostError
// ---------------------------------------------------------------------------

describe("LeaseLostError", () => {
  it("uses the dedicated lease-lost code and is NOT a CapabilityUnavailableError", () => {
    // -32004 is CAPABILITY_UNAVAILABLE_CODE (permanent capability gap). A
    // transient/recoverable lease loss must use -32005 and surface as its own
    // type so callers do not give up on an identity that needs a re-acquire.
    const err = new LeaseLostError("lease lost: review:singleton", "rid", "mobkit/send");
    assert.ok(err instanceof MobKitError);
    assert.ok(err instanceof RpcError);
    assert.ok(!(err instanceof CapabilityUnavailableError));
    assert.equal(err.name, "LeaseLostError");
    assert.equal(err.code, -32005);
  });
});

// ---------------------------------------------------------------------------
// ConsoleTimelineReplayUnavailableError
// ---------------------------------------------------------------------------

describe("ConsoleTimelineReplayUnavailableError", () => {
  it("extends MobKitError and uses the dedicated console timeline code", () => {
    const data = {
      error: "replay_unavailable",
      stream: "timeline",
      requested_cursor: "console:500",
      latest_cursor: "console:42",
    };
    const err = new ConsoleTimelineReplayUnavailableError("replay unavailable", "rid", "mobkit/console/query_timeline", data);
    assert.ok(err instanceof MobKitError);
    assert.equal(err.name, "ConsoleTimelineReplayUnavailableError");
    assert.equal(err.code, -32013);
    assert.deepEqual(err.data, data);
  });
});

// ---------------------------------------------------------------------------
// WorkGraphUnavailableError / WorkGraphConflictError
// ---------------------------------------------------------------------------

describe("WorkGraphUnavailableError", () => {
  it("extends MobKitError and uses the dedicated workgraph-unavailable code", () => {
    const err = new WorkGraphUnavailableError(
      "workgraph service not configured",
      "rid",
      "mobkit/workgraph/snapshot",
      { kind: "workgraph_unavailable" },
    );
    assert.ok(err instanceof MobKitError);
    assert.ok(err instanceof RpcError);
    assert.equal(err.name, "WorkGraphUnavailableError");
    assert.equal(err.code, -32041);
    assert.deepEqual(err.data, { kind: "workgraph_unavailable" });
  });
});

describe("WorkGraphConflictError", () => {
  it("extends MobKitError and uses the dedicated CAS-conflict code", () => {
    const data = { kind: "workgraph_conflict", detail: "revision mismatch" };
    const err = new WorkGraphConflictError(
      "revision conflict",
      "rid",
      "mobkit/workgraph/update",
      data,
    );
    assert.ok(err instanceof MobKitError);
    assert.ok(err instanceof RpcError);
    assert.equal(err.name, "WorkGraphConflictError");
    assert.equal(err.code, -32042);
    assert.deepEqual(err.data, data);
  });
});

// ---------------------------------------------------------------------------
// ContractMismatchError
// ---------------------------------------------------------------------------

describe("ContractMismatchError", () => {
  it("extends MobKitError", () => {
    const err = new ContractMismatchError("version mismatch");
    assert.ok(err instanceof MobKitError);
  });

  it("is an instance of Error", () => {
    const err = new ContractMismatchError("version mismatch");
    assert.ok(err instanceof Error);
  });

  it("has name set to ContractMismatchError", () => {
    const err = new ContractMismatchError("msg");
    assert.equal(err.name, "ContractMismatchError");
  });
});

// ---------------------------------------------------------------------------
// NotConnectedError
// ---------------------------------------------------------------------------

describe("NotConnectedError", () => {
  it("extends MobKitError", () => {
    const err = new NotConnectedError("not connected");
    assert.ok(err instanceof MobKitError);
  });

  it("is an instance of Error", () => {
    const err = new NotConnectedError("not connected");
    assert.ok(err instanceof Error);
  });

  it("has name set to NotConnectedError", () => {
    const err = new NotConnectedError("msg");
    assert.equal(err.name, "NotConnectedError");
  });
});

// ---------------------------------------------------------------------------
// MobkitRpcError (backward-compat alias)
// ---------------------------------------------------------------------------

describe("MobkitRpcError (backward compat)", () => {
  it("is the same constructor as RpcError", () => {
    assert.equal(MobkitRpcError, RpcError);
  });

  it("instances are instanceof RpcError", () => {
    const err = new MobkitRpcError(-32600, "msg", "req-1", "mob.status");
    assert.ok(err instanceof RpcError);
  });

  it("instances are instanceof MobKitError", () => {
    const err = new MobkitRpcError(-32600, "msg", "req-1", "mob.status");
    assert.ok(err instanceof MobKitError);
  });
});
