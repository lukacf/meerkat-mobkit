/**
 * TDD tests for Identity-First Continuity types, runtime APIs,
 * provider interfaces, builder extensions, and callback dispatcher routing.
 *
 * Covers REQ-46 through REQ-51.
 */

import { describe, it } from "node:test";
import assert from "node:assert/strict";

// ---------------------------------------------------------------------------
// REQ-46: DurableAgentSpec
// ---------------------------------------------------------------------------

describe("DurableAgentSpec (REQ-46)", () => {
  it("interface has all fields (camelCase)", async () => {
    const { parseDurableAgentSpec } = await import("../src/types.js");
    const spec = parseDurableAgentSpec({
      identity: "triage:main",
      profile: "triage",
      addressability: "addressable",
      display_name: "Triage Agent",
      labels: { tier: "1" },
      context: { key: "value" },
      additional_instructions: ["Be concise."],
    });
    assert.equal(spec.identity, "triage:main");
    assert.equal(spec.profile, "triage");
    assert.equal(spec.addressability, "addressable");
    assert.equal(spec.displayName, "Triage Agent");
    assert.deepEqual(spec.labels, { tier: "1" });
    assert.deepEqual(spec.context, { key: "value" });
    assert.deepEqual(spec.additionalInstructions, ["Be concise."]);
  });

  it("addressability defaults to 'addressable' when omitted", async () => {
    const { parseDurableAgentSpec } = await import("../src/types.js");
    const spec = parseDurableAgentSpec({
      identity: "gate:main",
      profile: "gate",
    });
    assert.equal(spec.addressability, "addressable");
  });

  it("optional fields default gracefully", async () => {
    const { parseDurableAgentSpec } = await import("../src/types.js");
    const spec = parseDurableAgentSpec({
      identity: "worker:1",
      profile: "worker",
    });
    assert.equal(spec.displayName, null);
    assert.deepEqual(spec.labels, {});
    assert.equal(spec.context, null);
    assert.deepEqual(spec.additionalInstructions, []);
  });

  it("serializes to wire format (snake_case)", async () => {
    const { durableAgentSpecToDict } = await import("../src/types.js");
    const wire = durableAgentSpecToDict({
      identity: "triage:main",
      profile: "triage",
      addressability: "internal_only",
      displayName: "Triage",
      labels: { tier: "1" },
      context: { k: "v" },
      additionalInstructions: ["Be concise."],
    });
    assert.equal(wire.identity, "triage:main");
    assert.equal(wire.profile, "triage");
    assert.equal(wire.addressability, "internal_only");
    assert.equal(wire.display_name, "Triage");
    assert.deepEqual(wire.labels, { tier: "1" });
    assert.deepEqual(wire.context, { k: "v" });
    assert.deepEqual(wire.additional_instructions, ["Be concise."]);
  });
});

// ---------------------------------------------------------------------------
// REQ-49: DispatchInput + DispatchContentBlock
// ---------------------------------------------------------------------------

describe("DispatchInput + DispatchContentBlock (REQ-49)", () => {
  it("text-only dispatch input round-trips", async () => {
    const { parseDispatchInput, dispatchInputToDict } = await import("../src/types.js");
    const input = parseDispatchInput({
      content: "Hello, agent!",
      origin: "connector",
      correlation_id: "corr-1",
      idempotency_key: "idem-1",
    });
    assert.equal(input.content, "Hello, agent!");
    assert.equal(input.origin, "connector");
    assert.equal(input.correlationId, "corr-1");
    assert.equal(input.idempotencyKey, "idem-1");

    const wire = dispatchInputToDict(input);
    assert.equal(wire.content, "Hello, agent!");
    assert.equal(wire.origin, "connector");
    assert.equal(wire.correlation_id, "corr-1");
    assert.equal(wire.idempotency_key, "idem-1");
  });

  it("multimodal dispatch with content blocks", async () => {
    const { parseDispatchInput, dispatchInputToDict } = await import("../src/types.js");
    const input = parseDispatchInput({
      content: [
        { type: "text", text: "Describe this." },
        { type: "image", media_type: "image/png", data: "base64data" },
      ],
      origin: "scheduler",
    });
    assert.ok(Array.isArray(input.content));
    const blocks = input.content as Array<{ type: string }>;
    assert.equal(blocks.length, 2);
    assert.equal(blocks[0].type, "text");
    assert.equal((blocks[0] as { text: string }).text, "Describe this.");
    assert.equal(blocks[1].type, "image");
    assert.equal((blocks[1] as { mediaType: string }).mediaType, "image/png");
    assert.equal((blocks[1] as { source: string }).source, "inline");

    const wire = dispatchInputToDict(input);
    const wireBlocks = wire.content as Array<Record<string, unknown>>;
    assert.equal(wireBlocks[1].media_type, "image/png");
    assert.equal(wireBlocks[1].source, "inline");
    assert.equal(wireBlocks[1].data, "base64data");
  });

  it("blob-backed image dispatch blocks round-trip", async () => {
    const { parseDispatchInput, dispatchInputToDict } = await import("../src/types.js");
    const blobId = `sha256:${"a".repeat(64)}`;
    const input = parseDispatchInput({
      content: [
        { type: "image", media_type: "image/png", source: "blob", blob_id: blobId },
      ],
      origin: "system",
    });
    assert.ok(Array.isArray(input.content));
    const blocks = input.content as Array<Record<string, unknown>>;
    assert.equal(blocks[0].type, "image");
    assert.equal(blocks[0].source, "blob");
    assert.equal(blocks[0].blobId, blobId);

    const wire = dispatchInputToDict(input);
    const wireBlocks = wire.content as Array<Record<string, unknown>>;
    assert.deepEqual(wireBlocks[0], {
      type: "image",
      media_type: "image/png",
      source: "blob",
      blob_id: blobId,
    });
  });

  it("all five origin values accepted", async () => {
    const { parseDispatchInput } = await import("../src/types.js");
    for (const origin of ["connector", "scheduler", "policy", "flow", "system"]) {
      const input = parseDispatchInput({ content: "test", origin });
      assert.equal(input.origin, origin);
    }
  });

  it("optional fields default to undefined", async () => {
    const { parseDispatchInput } = await import("../src/types.js");
    const input = parseDispatchInput({
      content: "test",
      origin: "system",
    });
    assert.equal(input.correlationId, undefined);
    assert.equal(input.idempotencyKey, undefined);
  });
});

// ---------------------------------------------------------------------------
// REQ-49a: AgentBuildContext, AgentBuildDraft, ExternalToolDef, ManagedPeerEdge
// ---------------------------------------------------------------------------

describe("AgentBuildContext + AgentBuildDraft (REQ-49a)", () => {
  it("AgentBuildContext parses from wire", async () => {
    const { parseAgentBuildContext } = await import("../src/types.js");
    const ctx = parseAgentBuildContext({
      identity: "lead:main",
      active_peers: ["worker:1", "worker:2"],
      managed_edges: [{ a: "lead:main", b: "worker:1" }],
    });
    assert.equal(ctx.identity, "lead:main");
    assert.deepEqual(ctx.activePeers, ["worker:1", "worker:2"]);
    assert.equal(ctx.managedEdges.length, 1);
    assert.equal(ctx.managedEdges[0].a, "lead:main");
    assert.equal(ctx.managedEdges[0].b, "worker:1");
  });

  it("AgentBuildDraft parses and serializes", async () => {
    const { parseAgentBuildDraft, agentBuildDraftToDict } = await import("../src/types.js");
    const draft = parseAgentBuildDraft({
      model: "claude-sonnet-4-5",
      system_prompt: "You are helpful.",
      additional_instructions: ["Be brief."],
      labels: { role: "lead" },
      app_context: { custom: true },
      external_tools: [
        { name: "search", description: "Search the web", input_schema: { type: "object" } },
      ],
    });
    assert.equal(draft.model, "claude-sonnet-4-5");
    assert.equal(draft.systemPrompt, "You are helpful.");
    assert.deepEqual(draft.additionalInstructions, ["Be brief."]);
    assert.deepEqual(draft.labels, { role: "lead" });
    assert.deepEqual(draft.appContext, { custom: true });
    assert.equal(draft.externalTools.length, 1);
    assert.equal(draft.externalTools[0].name, "search");

    const wire = agentBuildDraftToDict(draft);
    assert.equal(wire.model, "claude-sonnet-4-5");
    assert.equal(wire.system_prompt, "You are helpful.");
    assert.deepEqual(wire.additional_instructions, ["Be brief."]);
    const wireTools = wire.external_tools as Array<Record<string, unknown>>;
    assert.equal(wireTools[0].name, "search");
    assert.deepEqual(wireTools[0].input_schema, { type: "object" });
  });

  it("AgentBuildDraft null fields default gracefully", async () => {
    const { parseAgentBuildDraft } = await import("../src/types.js");
    const draft = parseAgentBuildDraft({});
    assert.equal(draft.model, null);
    assert.equal(draft.systemPrompt, null);
    assert.deepEqual(draft.additionalInstructions, []);
    assert.deepEqual(draft.labels, {});
    assert.equal(draft.appContext, null);
    assert.deepEqual(draft.externalTools, []);
  });

  it("ExternalToolDef round-trips", async () => {
    const { parseExternalToolDef, externalToolDefToDict } = await import("../src/types.js");
    const tool = parseExternalToolDef({
      name: "fetch",
      description: "Fetch a URL",
      input_schema: { type: "object", properties: { url: { type: "string" } } },
    });
    assert.equal(tool.name, "fetch");
    assert.equal(tool.description, "Fetch a URL");
    assert.deepEqual(tool.inputSchema, { type: "object", properties: { url: { type: "string" } } });

    const wire = externalToolDefToDict(tool);
    assert.equal(wire.input_schema.type, "object");
  });

  it("ManagedPeerEdge round-trips", async () => {
    const { parseManagedPeerEdge, managedPeerEdgeToDict } = await import("../src/types.js");
    const edge = parseManagedPeerEdge({ a: "lead:main", b: "worker:1" });
    assert.equal(edge.a, "lead:main");
    assert.equal(edge.b, "worker:1");

    const wire = managedPeerEdgeToDict(edge);
    assert.equal(wire.a, "lead:main");
    assert.equal(wire.b, "worker:1");
  });
});

// ---------------------------------------------------------------------------
// REQ-49b: IdentityStatus, ContinuityHealth, DurabilityPolicy, LeaseInfo
// ---------------------------------------------------------------------------

describe("IdentityStatus + ContinuityHealth (REQ-49b)", () => {
  it("full IdentityStatus parses from wire", async () => {
    const { parseIdentityStatus } = await import("../src/types.js");
    const status = parseIdentityStatus({
      identity: "triage:main",
      state: "active",
      agent_runtime_id: "rt-abc",
      session_id: "sess-123",
      profile: "triage",
      addressability: "addressable",
      display_name: "Triage Lead",
      labels: { tier: "1" },
      generation: 2,
      checkpoint_version: 5,
      lease: {
        fencing_token: 42,
        ttl_remaining_ms: 30000,
        healthy: true,
      },
      continuity_health: {
        store_reachable: true,
        durability_policy: { kind: "sync_write_through" },
        last_checkpoint_version: 5,
      },
    });
    assert.equal(status.identity, "triage:main");
    assert.equal(status.lifecycleState, "active");
    assert.equal(status.agentRuntimeId, "rt-abc");
    assert.equal(status.sessionId, "sess-123");
    assert.equal(status.displayName, "Triage Lead");
    assert.equal(status.profile, "triage");
    assert.equal(status.addressability, "addressable");
    assert.deepEqual(status.labels, { tier: "1" });
    assert.equal(status.generation, 2);
    assert.equal(status.checkpointVersion, 5);
    assert.ok(status.lease !== null);
    assert.equal(status.lease!.fencingToken, 42);
    assert.equal(status.lease!.ttlRemainingMs, 30000);
    assert.equal(status.lease!.healthy, true);
    assert.ok(status.continuityHealth !== null);
    assert.equal(status.continuityHealth!.storeReachable, true);
    assert.equal(status.continuityHealth!.durabilityPolicy.kind, "syncWriteThrough");
    assert.equal(status.continuityHealth!.lastCheckpointVersion, 5);
  });

  it("DurabilityPolicy bufferedExport has maxLossWindowMs", async () => {
    const { parseIdentityStatus } = await import("../src/types.js");
    const status = parseIdentityStatus({
      identity: "x:1",
      state: "active",
      continuity_health: {
        store_reachable: true,
        durability_policy: {
          kind: "buffered_export",
          max_loss_window_ms: 5000,
        },
        last_checkpoint_version: null,
      },
    });
    assert.ok(status.continuityHealth !== null);
    assert.equal(status.continuityHealth!.durabilityPolicy.kind, "bufferedExport");
    assert.equal(status.continuityHealth!.durabilityPolicy.maxLossWindowMs, 5000);
  });

  it("null lease and continuity_health", async () => {
    const { parseIdentityStatus } = await import("../src/types.js");
    const status = parseIdentityStatus({
      identity: "x:1",
      state: "initializing",
    });
    assert.equal(status.lease, null);
    assert.equal(status.continuityHealth, null);
  });
});

// ---------------------------------------------------------------------------
// REQ-49c: Provider result types (discriminated unions)
// ---------------------------------------------------------------------------

describe("Provider result types (REQ-49c)", () => {
  it("ContinuityResolveState: uninitialized", async () => {
    const { parseContinuityResolveState } = await import("../src/types.js");
    const state = parseContinuityResolveState({ state: "uninitialized" });
    assert.equal(state.state, "uninitialized");
    assert.equal(state.record, undefined);
    assert.equal(state.failure, undefined);
  });

  it("ContinuityResolveState: ready with record", async () => {
    const { parseContinuityResolveState } = await import("../src/types.js");
    const state = parseContinuityResolveState({
      state: "ready",
      record: {
        identity: "lead:main",
        agent_runtime_id: "rt-1",
        session_id: "sess-1",
        generation: 0,
        checkpoint_version: 3,
      },
    });
    assert.equal(state.state, "ready");
    assert.ok(state.record);
    assert.equal(state.record!.identity, "lead:main");
    assert.equal(state.record!.agentRuntimeId, "rt-1");
    assert.equal(state.record!.sessionId, "sess-1");
    assert.equal(state.record!.generation, 0);
    assert.equal(state.record!.checkpointVersion, 3);
  });

  it("ContinuityResolveState: broken with failure", async () => {
    const { parseContinuityResolveState } = await import("../src/types.js");
    const state = parseContinuityResolveState({
      state: "broken",
      failure: {
        identity: "lead:main",
        kind: "snapshot_missing",
        detail: "no snapshot found",
      },
    });
    assert.equal(state.state, "broken");
    assert.ok(state.failure);
    assert.equal(state.failure!.identity, "lead:main");
    assert.equal(state.failure!.kind, "snapshot_missing");
    assert.equal(state.failure!.detail, "no snapshot found");
  });

  it("ContinuityRecord round-trip", async () => {
    const { parseContinuityRecord, continuityRecordToDict } = await import("../src/types.js");
    const record = parseContinuityRecord({
      identity: "worker:1",
      agent_runtime_id: "rt-w1",
      session_id: "sess-w1",
      generation: 1,
      checkpoint_version: 7,
    });
    assert.equal(record.identity, "worker:1");
    assert.equal(record.agentRuntimeId, "rt-w1");
    assert.equal(record.generation, 1);
    assert.equal(record.checkpointVersion, 7);

    const wire = continuityRecordToDict(record);
    assert.equal(wire.agent_runtime_id, "rt-w1");
    assert.equal(wire.session_id, "sess-w1");
  });

  it("ContinuityFailure with optional record", async () => {
    const { parseContinuityFailure } = await import("../src/types.js");
    const fail = parseContinuityFailure({
      identity: "x:1",
      kind: "snapshot_corrupted",
      detail: "checksum mismatch",
      record: {
        identity: "x:1",
        agent_runtime_id: "rt-x1",
        session_id: "sess-x1",
        generation: 0,
        checkpoint_version: 2,
      },
    });
    assert.equal(fail.kind, "snapshot_corrupted");
    assert.equal(fail.detail, "checksum mismatch");
    assert.ok(fail.record);
    assert.equal(fail.record!.agentRuntimeId, "rt-x1");
  });

  it("SessionSnapshot wraps Uint8Array", async () => {
    const { parseSessionSnapshot, sessionSnapshotToDict } = await import("../src/types.js");
    // Wire format: base64 string
    const snap = parseSessionSnapshot({ data: "SGVsbG8=" });
    assert.ok(snap.data instanceof Uint8Array);
    assert.equal(new TextDecoder().decode(snap.data), "Hello");

    const wire = sessionSnapshotToDict(snap);
    assert.equal(wire.data, "SGVsbG8=");
  });

  it("LeaseGrant round-trip", async () => {
    const { parseLeaseGrant, leaseGrantToDict } = await import("../src/types.js");
    const grant = parseLeaseGrant({
      identity: "lead:main",
      fencing_token: 99,
      ttl_ms: 30000,
    });
    assert.equal(grant.identity, "lead:main");
    assert.equal(grant.fencingToken, 99);
    assert.equal(grant.ttlMs, 30000);

    const wire = leaseGrantToDict(grant);
    assert.equal(wire.fencing_token, 99);
    assert.equal(wire.ttl_ms, 30000);
  });

  it("LeaseAcquireResult: acquired", async () => {
    const { parseLeaseAcquireResult } = await import("../src/types.js");
    const result = parseLeaseAcquireResult({
      status: "acquired",
      grant: { identity: "lead:main", fencing_token: 1, ttl_ms: 30000 },
    });
    assert.equal(result.status, "acquired");
    assert.ok(result.grant);
    assert.equal(result.grant!.fencingToken, 1);
  });

  it("LeaseAcquireResult: already_held", async () => {
    const { parseLeaseAcquireResult } = await import("../src/types.js");
    const result = parseLeaseAcquireResult({
      status: "already_held",
      holder: "other-runtime",
    });
    assert.equal(result.status, "alreadyHeld");
    assert.equal(result.holder, "other-runtime");
  });

  it("LeaseRenewResult: renewed", async () => {
    const { parseLeaseRenewResult } = await import("../src/types.js");
    const result = parseLeaseRenewResult({
      status: "renewed",
      grant: { identity: "x:1", fencing_token: 2, ttl_ms: 30000 },
    });
    assert.equal(result.status, "renewed");
    assert.ok(result.grant);
  });

  it("LeaseRenewResult: lost", async () => {
    const { parseLeaseRenewResult } = await import("../src/types.js");
    const result = parseLeaseRenewResult({ status: "lost" });
    assert.equal(result.status, "lost");
    assert.equal(result.grant, undefined);
  });
});

// ---------------------------------------------------------------------------
// REQ-47: Identity-first runtime APIs on MobKitRuntime
// ---------------------------------------------------------------------------

describe("Identity-first runtime APIs (REQ-47)", () => {
  // We test that the methods exist and have the right signature by calling
  // them on a runtime with a mock transport. We stub _rpc to capture calls.

  async function makeRuntime() {
    const { MobKitRuntime } = await import("../src/runtime.js");
    const calls: { method: string; params: Record<string, unknown> }[] = [];
    const rt = new MobKitRuntime({
      mobConfigPath: null,
      sessionBuilder: null,
      sessionStore: null,
      discoveryCallback: null,
      preSpawnCallback: null,
      errorCallback: null,
      eventLog: null,
      gatingConfigPath: null,
      routingConfigPath: null,
      schedulingFiles: [],
      memoryConfig: null,
      authConfig: null,
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
    // Stub _rpc to capture calls and return plausible results
    (rt as unknown as Record<string, unknown>)._rpc = async (
      method: string,
      params?: Record<string, unknown>,
    ) => {
      calls.push({ method, params: params ?? {} });
      // Return type-appropriate stubs
      if (method === "mobkit/status_identity") {
        return {
          identity: params?.identity ?? "x:1",
          state: "active",
          agent_runtime_id: "rt-1",
          session_id: "sess-1",
          profile: "worker",
          addressability: "addressable",
          labels: {},
          generation: 0,
          checkpoint_version: 0,
          lease: null,
          continuity_health: null,
        };
      }
      return { accepted: true };
    };
    (rt as unknown as Record<string, unknown>)._running = true;
    return { rt, calls };
  }

  it("send(identity, string) calls RPC with content", async () => {
    const { rt, calls } = await makeRuntime();
    await rt.send("triage:main", "Hello!");
    assert.equal(calls.length, 1);
    assert.equal(calls[0].method, "mobkit/send");
    assert.equal(calls[0].params.identity, "triage:main");
    assert.equal(calls[0].params.content, "Hello!");
  });

  it("send(identity, content blocks) sends array", async () => {
    const { rt, calls } = await makeRuntime();
    await rt.send("triage:main", [{ type: "text", text: "Hello" }]);
    assert.equal(calls[0].params.identity, "triage:main");
    assert.ok(Array.isArray(calls[0].params.content));
  });

  it("send(identity, image block) sends strict image source shape", async () => {
    const { rt, calls } = await makeRuntime();
    await rt.send("triage:main", [
      { type: "image", mediaType: "image/png", data: "abc" },
    ]);
    assert.deepEqual(calls[0].params.content, [
      {
        type: "image",
        media_type: "image/png",
        source: "inline",
        data: "abc",
      },
    ]);
  });

  it("dispatch(identity, dispatchInput)", async () => {
    const { rt, calls } = await makeRuntime();
    await rt.dispatch("triage:main", {
      content: "Hello",
      origin: "connector",
    });
    assert.equal(calls[0].method, "mobkit/dispatch");
    assert.equal(calls[0].params.identity, "triage:main");
    assert.equal((calls[0].params.dispatch_input as Record<string, unknown>).origin, "connector");
  });

  it("agent(identity)", async () => {
    const { rt, calls } = await makeRuntime();
    const snap = await rt.agent("lead:main");
    assert.equal(calls[0].method, "mobkit/status_identity");
    assert.equal(calls[0].params.identity, "lead:main");
    assert.equal(snap.agentIdentity, "rt-1");
    assert.equal(snap.role, "worker");
  });

  it("subscribe(identity)", async () => {
    const { rt, calls } = await makeRuntime();
    await rt.subscribe("worker:1");
    assert.equal(calls[0].method, "mobkit/subscribe");
    assert.equal(calls[0].params.identity, "worker:1");
  });

  it("status(identity) returns parsed IdentityStatus", async () => {
    const { rt, calls } = await makeRuntime();
    const status = await rt.status("triage:main");
    assert.equal(calls[0].method, "mobkit/status_identity");
    assert.equal(status.identity, "triage:main");
    assert.equal(status.lifecycleState, "active");
  });

  it("respawn(identity)", async () => {
    const { rt, calls } = await makeRuntime();
    await rt.respawn("worker:1");
    assert.equal(calls[0].method, "mobkit/respawn");
    assert.equal(calls[0].params.identity, "worker:1");
  });

  it("retire(identity)", async () => {
    const { rt, calls } = await makeRuntime();
    await rt.retire("worker:1");
    assert.equal(calls[0].method, "mobkit/retire");
    assert.equal(calls[0].params.identity, "worker:1");
  });

  it("reset(identity)", async () => {
    const { rt, calls } = await makeRuntime();
    await rt.reset("worker:1");
    assert.equal(calls[0].method, "mobkit/reset");
    assert.equal(calls[0].params.identity, "worker:1");
  });

  it("deleteIdentity(identity)", async () => {
    const { rt, calls } = await makeRuntime();
    await rt.deleteIdentity("worker:1");
    assert.equal(calls[0].method, "mobkit/delete_identity");
    assert.equal(calls[0].params.identity, "worker:1");
  });
});

// ---------------------------------------------------------------------------
// REQ-48: Provider interfaces (structural tests)
// ---------------------------------------------------------------------------

describe("Provider interfaces (REQ-48)", () => {
  // Structural compile-time tests: prove the interfaces exist and have
  // the right shape by constructing mock implementations.

  it("ContinuityStore interface is implementable", async () => {
    const types = await import("../src/types.js");
    // Type-level test: construct an object satisfying the interface
    const store: import("../src/types.js").ContinuityStore = {
      async resolveMany(identities: string[]) {
        const result: Record<string, import("../src/types.js").ContinuityResolveState> = {};
        for (const id of identities) {
          result[id] = { state: "uninitialized" };
        }
        return result;
      },
      async loadSessionSnapshot(_sessionId: string) {
        return null;
      },
      async saveSessionSnapshot(_identity, _sessionId, _generation, _version, _fencingToken, _snapshot) {},
      async upsertContinuityRecord(_record, _fencingToken) {},
    };
    const result = await store.resolveMany(["x:1"]);
    assert.equal(result["x:1"].state, "uninitialized");
  });

  it("LeaseProvider interface is implementable", async () => {
    const store: import("../src/types.js").LeaseProvider = {
      async acquireLeases(_identities: string[], _runtimeInstance: string) {
        return {
          "x:1": { status: "acquired" as const, grant: { identity: "x:1", fencingToken: 1, ttlMs: 30000 } },
        };
      },
      async renewLeases(_grants) {
        return {};
      },
      async releaseLeases(_grants) {},
    };
    const result = await store.acquireLeases(["x:1"], "rt-1");
    assert.equal(result["x:1"].status, "acquired");
  });

  it("RosterProvider interface is implementable", async () => {
    const provider: import("../src/types.js").RosterProvider = {
      async roster(_context) {
        return [
          {
            identity: "triage:main",
            profile: "triage",
            addressability: "addressable",
            displayName: null,
            labels: {},
            context: null,
            additionalInstructions: [],
          },
        ];
      },
    };
    const specs = await provider.roster({});
    assert.equal(specs.length, 1);
    assert.equal(specs[0].identity, "triage:main");
  });

  it("AgentCustomizer interface is implementable", async () => {
    const customizer: import("../src/types.js").AgentCustomizer = {
      async customizeBuild(_context, _spec, draft) {
        draft.model = "claude-sonnet-4-5";
      },
    };
    const draft = {
      model: null as string | null,
      systemPrompt: null as string | null,
      additionalInstructions: [] as string[],
      labels: {} as Record<string, string>,
      appContext: null as unknown,
      externalTools: [] as import("../src/types.js").ExternalToolDef[],
    };
    await customizer.customizeBuild(
      { identity: "x:1", activePeers: [], managedEdges: [] },
      {
        identity: "x:1", profile: "worker", addressability: "addressable",
        displayName: null, labels: {}, context: null, additionalInstructions: [],
      },
      draft,
    );
    assert.equal(draft.model, "claude-sonnet-4-5");
  });

  it("TopologyProvider interface is implementable", async () => {
    const provider: import("../src/types.js").TopologyProvider = {
      async computeEdges(_targetIdentities, _context) {
        return [{ a: "lead:main", b: "worker:1" }];
      },
    };
    const edges = await provider.computeEdges(["lead:main", "worker:1"], {});
    assert.equal(edges.length, 1);
  });
});

// ---------------------------------------------------------------------------
// REQ-50: Builder continuityStore / leaseProvider / scratchDir
// ---------------------------------------------------------------------------

describe("Builder identity-first extensions (REQ-50)", () => {
  it("continuityStore() is chainable", async () => {
    const { MobKit } = await import("../src/builder.js");
    const builder = MobKit.builder();
    const store: import("../src/types.js").ContinuityStore = {
      async resolveMany() { return {}; },
      async loadSessionSnapshot() { return null; },
      async saveSessionSnapshot() {},
      async upsertContinuityRecord() {},
    };
    const result = builder.continuityStore(store);
    assert.equal(result, builder);
  });

  it("leaseProvider() is chainable", async () => {
    const { MobKit } = await import("../src/builder.js");
    const builder = MobKit.builder();
    const provider: import("../src/types.js").LeaseProvider = {
      async acquireLeases() { return {}; },
      async renewLeases() { return {}; },
      async releaseLeases() {},
    };
    const result = builder.leaseProvider(provider);
    assert.equal(result, builder);
  });

  it("scratchDir() is chainable", async () => {
    const { MobKit } = await import("../src/builder.js");
    const builder = MobKit.builder();
    const result = builder.scratchDir("/tmp/scratch");
    assert.equal(result, builder);
  });

  it("persistentState + continuityStore fails at build time", async () => {
    const { MobKit } = await import("../src/builder.js");
    const builder = MobKit.builder();
    builder.persistentState("/tmp/state");
    builder.continuityStore({
      async resolveMany() { return {}; },
      async loadSessionSnapshot() { return null; },
      async saveSessionSnapshot() {},
      async upsertContinuityRecord() {},
    });
    await assert.rejects(builder.build(), (err: Error) => {
      assert.ok(err.message.includes("mutually exclusive"));
      return true;
    });
  });

  it("persistentState + leaseProvider fails at build time", async () => {
    const { MobKit } = await import("../src/builder.js");
    const builder = MobKit.builder();
    builder.persistentState("/tmp/state");
    builder.leaseProvider({
      async acquireLeases() { return {}; },
      async renewLeases() { return {}; },
      async releaseLeases() {},
    });
    await assert.rejects(builder.build(), (err: Error) => {
      assert.ok(err.message.includes("mutually exclusive"));
      return true;
    });
  });

  it("stores config fields correctly", async () => {
    const { MobKit } = await import("../src/builder.js");
    const builder = MobKit.builder();
    const store = {
      async resolveMany() { return {}; },
      async loadSessionSnapshot() { return null; },
      async saveSessionSnapshot() {},
      async upsertContinuityRecord() {},
    };
    const lease = {
      async acquireLeases() { return {}; },
      async renewLeases() { return {}; },
      async releaseLeases() {},
    };
    builder.continuityStore(store).leaseProvider(lease).scratchDir("/tmp/scratch");
    assert.equal(builder._config.continuityStore, store);
    assert.equal(builder._config.leaseProvider, lease);
    assert.equal(builder._config.scratchDir, "/tmp/scratch");
  });

  it("runtime init params advertise identity-first provider callbacks", async () => {
    const { MobKitRuntime } = await import("../src/runtime.js");
    const rosterProvider = { async roster() { return []; } };
    const topologyProvider = { async computeEdges() { return []; } };
    const agentCustomizer = { async customizeBuild() {} };
    const rt = new MobKitRuntime({
      mobConfigPath: null,
      sessionBuilder: null,
      sessionStore: null,
      discoveryCallback: null,
      preSpawnCallback: null,
      errorCallback: null,
      eventLog: null,
      gatingConfigPath: null,
      routingConfigPath: null,
      schedulingFiles: [],
      memoryConfig: null,
      authConfig: null,
      gatewayBin: "/bin/rpc_gateway",
      modules: [],
      persistentState: "/tmp/state",
      continuityStore: null,
      leaseProvider: null,
      scratchDir: null,
      rosterProvider,
      agentCustomizer,
      topologyProvider,
    });

    const params = (rt as unknown as { _buildInitParams(): Record<string, unknown> })
      ._buildInitParams();
    assert.equal(params.has_roster_provider, true);
    assert.equal(params.has_topology_provider, true);
    assert.equal(params.has_agent_customizer, true);
  });
});

// ---------------------------------------------------------------------------
// REQ-51: CallbackDispatcher — provider routing
// ---------------------------------------------------------------------------

describe("CallbackDispatcher provider routing (REQ-51)", () => {
  it("routes callback/continuity_store/resolve_many to ContinuityStore", async () => {
    const { CallbackDispatcher } = await import("../src/agent-builder.js");
    const dispatcher = new CallbackDispatcher();
    const resolveCalls: string[][] = [];

    dispatcher.registerContinuityStore({
      async resolveMany(identities: string[]) {
        resolveCalls.push(identities);
        const result: Record<string, { state: string }> = {};
        for (const id of identities) {
          result[id] = { state: "uninitialized" };
        }
        return result;
      },
      async loadSessionSnapshot() { return null; },
      async saveSessionSnapshot() {},
      async upsertContinuityRecord() {},
    });

    const result = await dispatcher.handleCallback("callback/continuity_store/resolve_many", {
      identities: ["lead:main", "worker:1"],
    });
    assert.equal(resolveCalls.length, 1);
    assert.deepEqual(resolveCalls[0], ["lead:main", "worker:1"]);
    assert.ok(result !== null);
  });

  it("routes callback/continuity_store/load_session_snapshot", async () => {
    const { CallbackDispatcher } = await import("../src/agent-builder.js");
    const dispatcher = new CallbackDispatcher();
    let loadedSessionId: string | null = null;

    dispatcher.registerContinuityStore({
      async resolveMany() { return {}; },
      async loadSessionSnapshot(sessionId: string) {
        loadedSessionId = sessionId;
        return { data: new Uint8Array([72, 101, 108, 108, 111]) };
      },
      async saveSessionSnapshot() {},
      async upsertContinuityRecord() {},
    });

    const result = await dispatcher.handleCallback("callback/continuity_store/load_session_snapshot", {
      session_id: "sess-1",
    });
    assert.equal(loadedSessionId, "sess-1");
    assert.ok(result !== null);
  });

  it("routes callback/continuity_store/save_session_snapshot", async () => {
    const { CallbackDispatcher } = await import("../src/agent-builder.js");
    const dispatcher = new CallbackDispatcher();
    let savedArgs: Record<string, unknown> | null = null;

    dispatcher.registerContinuityStore({
      async resolveMany() { return {}; },
      async loadSessionSnapshot() { return null; },
      async saveSessionSnapshot(identity, sessionId, generation, version, fencingToken, snapshot) {
        savedArgs = { identity, sessionId, generation, version, fencingToken, hasData: snapshot.data.length > 0 };
      },
      async upsertContinuityRecord() {},
    });

    await dispatcher.handleCallback("callback/continuity_store/save_session_snapshot", {
      identity: "lead:main",
      session_id: "sess-1",
      generation: 0,
      checkpoint_version: 1,
      fencing_token: 42,
      snapshot: "SGVsbG8=", // base64 "Hello"
    });
    assert.ok(savedArgs !== null);
    assert.equal(savedArgs!.identity, "lead:main");
    assert.equal(savedArgs!.fencingToken, 42);
    assert.equal(savedArgs!.hasData, true);
  });

  it("routes callback/continuity_store/upsert_continuity_record", async () => {
    const { CallbackDispatcher } = await import("../src/agent-builder.js");
    const dispatcher = new CallbackDispatcher();
    let upsertedRecord: Record<string, unknown> | null = null;

    dispatcher.registerContinuityStore({
      async resolveMany() { return {}; },
      async loadSessionSnapshot() { return null; },
      async saveSessionSnapshot() {},
      async upsertContinuityRecord(record, fencingToken) {
        upsertedRecord = { ...record, fencingToken };
      },
    });

    await dispatcher.handleCallback("callback/continuity_store/upsert_continuity_record", {
      record: {
        identity: "lead:main",
        agent_runtime_id: "rt-1",
        session_id: "sess-1",
        generation: 0,
        checkpoint_version: 0,
      },
      fencing_token: 42,
    });
    assert.ok(upsertedRecord !== null);
    assert.equal(upsertedRecord!.identity, "lead:main");
    assert.equal(upsertedRecord!.fencingToken, 42);
  });

  it("routes callback/lease_provider/acquire_leases to LeaseProvider", async () => {
    const { CallbackDispatcher } = await import("../src/agent-builder.js");
    const dispatcher = new CallbackDispatcher();

    dispatcher.registerLeaseProvider({
      async acquireLeases(identities, runtimeInstance) {
        const result: Record<string, { status: string; grant?: Record<string, unknown> }> = {};
        for (const id of identities) {
          result[id] = { status: "acquired", grant: { identity: id, fencingToken: 1, ttlMs: 30000 } };
        }
        return result;
      },
      async renewLeases() { return {}; },
      async releaseLeases() {},
    });

    const result = await dispatcher.handleCallback("callback/lease_provider/acquire_leases", {
      identities: ["x:1"],
      runtime_instance: "rt-1",
    });
    assert.ok(result !== null);
  });

  it("routes callback/lease_provider/renew_leases to LeaseProvider", async () => {
    const { CallbackDispatcher } = await import("../src/agent-builder.js");
    const dispatcher = new CallbackDispatcher();

    dispatcher.registerLeaseProvider({
      async acquireLeases() { return {}; },
      async renewLeases(grants) {
        const result: Record<string, { status: string }> = {};
        for (const g of grants) {
          result[g.identity] = { status: "renewed" };
        }
        return result;
      },
      async releaseLeases() {},
    });

    const result = await dispatcher.handleCallback("callback/lease_provider/renew_leases", {
      grants: [{ identity: "x:1", fencing_token: 1, ttl_ms: 30000 }],
    });
    assert.ok(result !== null);
  });

  it("routes callback/lease_provider/release_leases to LeaseProvider", async () => {
    const { CallbackDispatcher } = await import("../src/agent-builder.js");
    const dispatcher = new CallbackDispatcher();
    let releasedCount = 0;

    dispatcher.registerLeaseProvider({
      async acquireLeases() { return {}; },
      async renewLeases() { return {}; },
      async releaseLeases(grants) { releasedCount = grants.length; },
    });

    await dispatcher.handleCallback("callback/lease_provider/release_leases", {
      grants: [{ identity: "x:1", fencing_token: 1, ttl_ms: 30000 }],
    });
    assert.equal(releasedCount, 1);
  });

  it("routes callback/roster_provider/roster to RosterProvider", async () => {
    const { CallbackDispatcher } = await import("../src/agent-builder.js");
    const dispatcher = new CallbackDispatcher();

    dispatcher.registerRosterProvider({
      async roster(_context) {
        return [
          {
            identity: "triage:main",
            profile: "triage",
            addressability: "addressable",
            displayName: null,
            labels: {},
            context: null,
            additionalInstructions: [],
          },
        ];
      },
    });

    const result = await dispatcher.handleCallback("callback/roster_provider/roster", {
      context: {},
    });
    const specs = result as Array<Record<string, unknown>>;
    assert.equal(specs.length, 1);
    assert.equal(specs[0].identity, "triage:main");
  });

  it("routes callback/topology_provider/compute_edges to TopologyProvider", async () => {
    const { CallbackDispatcher } = await import("../src/agent-builder.js");
    const dispatcher = new CallbackDispatcher();

    dispatcher.registerTopologyProvider({
      async computeEdges(targetIdentities, _context) {
        return [{ a: targetIdentities[0], b: targetIdentities[1] }];
      },
    });

    const result = await dispatcher.handleCallback("callback/topology_provider/compute_edges", {
      target_identities: ["lead:main", "worker:1"],
      context: {},
    });
    const edges = result as Array<Record<string, string>>;
    assert.equal(edges.length, 1);
    assert.equal(edges[0].a, "lead:main");
  });

  it("routes callback/agent_customizer/customize_build to AgentCustomizer", async () => {
    const { CallbackDispatcher } = await import("../src/agent-builder.js");
    const dispatcher = new CallbackDispatcher();

    dispatcher.registerAgentCustomizer({
      async customizeBuild(_context, _spec, draft) {
        draft.model = "claude-sonnet-4-5";
        draft.externalTools.push({
          name: "search",
          description: "Search",
          inputSchema: {},
        });
      },
    });

    const result = await dispatcher.handleCallback("callback/agent_customizer/customize_build", {
      context: { identity: "x:1", active_peers: [], managed_edges: [] },
      spec: { identity: "x:1", profile: "worker", addressability: "addressable" },
      draft: { model: null, system_prompt: null, additional_instructions: [], labels: {}, app_context: null, external_tools: [] },
    }) as Record<string, unknown>;
    assert.equal(result.model, "claude-sonnet-4-5");
    const tools = result.external_tools as Array<Record<string, unknown>>;
    assert.equal(tools.length, 1);
    assert.equal(tools[0].name, "search");
  });

  it("routes callback/agent_customizer/after_create to AgentCustomizer", async () => {
    const { CallbackDispatcher } = await import("../src/agent-builder.js");
    const dispatcher = new CallbackDispatcher();
    let afterCreateCalled = false;

    dispatcher.registerAgentCustomizer({
      async customizeBuild() {},
      async afterCreate(_identity, _agentRuntimeId, _sessionId) {
        afterCreateCalled = true;
      },
    });

    await dispatcher.handleCallback("callback/agent_customizer/after_create", {
      identity: "x:1",
      agent_runtime_id: "rt-1",
      session_id: "sess-1",
    });
    assert.equal(afterCreateCalled, true);
  });
});
