/**
 * Tests for MobKitRuntime and MobHandle with a mock RPC transport.
 */

import { describe, it, beforeEach } from "node:test";
import assert from "node:assert/strict";

import { MobKitRuntime, MobHandle, ToolCaller } from "../dist/index.js";

// ---------------------------------------------------------------------------
// Mock RPC helper
// ---------------------------------------------------------------------------

interface RpcCall {
  method: string;
  params: Record<string, unknown> | undefined;
}

function createMockRuntime(): {
  rt: MobKitRuntime;
  handle: MobHandle;
  calls: RpcCall[];
  setResponse: (fn: (method: string, params?: Record<string, unknown>) => unknown) => void;
} {
  const config = {
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
  };

  const rt = new MobKitRuntime(config);
  // Mark as running so MobHandle methods don't reject
  (rt as any)._running = true;

  const calls: RpcCall[] = [];
  let responseFn: (method: string, params?: Record<string, unknown>) => unknown = () => ({});

  (rt as any)._rpc = async (method: string, params?: Record<string, unknown>) => {
    calls.push({ method, params });
    return responseFn(method, params);
  };

  const handle = rt.mobHandle();

  return {
    rt,
    handle,
    calls,
    setResponse: (fn) => { responseFn = fn; },
  };
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

describe("MobKitRuntime", () => {
  it("mobHandle() returns a MobHandle", () => {
    const { handle } = createMockRuntime();
    assert.ok(handle instanceof MobHandle);
  });

  it("isRunning reflects _running state", () => {
    const { rt } = createMockRuntime();
    assert.equal(rt.isRunning, true);
  });

  it("shutdown sets isRunning to false", async () => {
    const { rt } = createMockRuntime();
    await rt.shutdown();
    assert.equal(rt.isRunning, false);
  });

  it("rustHttpBaseUrl getter and setter", () => {
    const { rt } = createMockRuntime();
    assert.equal(rt.rustHttpBaseUrl, null);
    rt.setRustHttpBase("http://127.0.0.1:8081");
    assert.equal(rt.rustHttpBaseUrl, "http://127.0.0.1:8081");
  });
});

describe("MobHandle.status()", () => {
  it("sends mobkit/status and parses the result", async () => {
    const { handle, calls, setResponse } = createMockRuntime();
    setResponse(() => ({
      contract_version: "0.2.0",
      running: true,
      loaded_modules: ["mod-a", "mod-b"],
    }));

    const result = await handle.status();
    assert.equal(calls.length, 1);
    assert.equal(calls[0].method, "mobkit/status");
    assert.equal(result.contractVersion, "0.2.0");
    assert.equal(result.running, true);
    assert.deepEqual(result.loadedModules, ["mod-a", "mod-b"]);
  });
});

describe("MobHandle.capabilities()", () => {
  it("sends mobkit/capabilities and parses the result", async () => {
    const { handle, calls, setResponse } = createMockRuntime();
    setResponse(() => ({
      contract_version: "0.2.0",
      methods: ["mobkit/status", "mobkit/capabilities"],
      loaded_modules: ["mod-a"],
    }));

    const result = await handle.capabilities();
    assert.equal(calls[0].method, "mobkit/capabilities");
    assert.equal(result.contractVersion, "0.2.0");
    assert.deepEqual(result.methods, ["mobkit/status", "mobkit/capabilities"]);
    assert.deepEqual(result.loadedModules, ["mod-a"]);
  });
});

describe("MobHandle.spawn()", () => {
  it("sends mobkit/spawn_member with discovery spec", async () => {
    const { handle, calls, setResponse } = createMockRuntime();
    setResponse(() => ({
      accepted: true,
      module_id: "mod-x",
      meerkat_id: "m-1",
      profile: "assistant",
    }));

    const result = await handle.spawn({
      profile: "assistant",
      meerkatId: "m-1",
      labels: { role: "helper" },
    });

    assert.equal(calls[0].method, "mobkit/spawn_member");
    assert.equal(calls[0].params!.profile, "assistant");
    assert.equal(calls[0].params!.meerkat_id, "m-1");
    assert.deepEqual(calls[0].params!.labels, { role: "helper" });
    assert.equal(result.accepted, true);
    assert.equal(result.moduleId, "mod-x");
    assert.equal(result.meerkatId, "m-1");
    assert.equal(result.profile, "assistant");
  });
});

describe("MobHandle.spawnMember()", () => {
  it("sends mobkit/spawn_member with module_id", async () => {
    const { handle, calls, setResponse } = createMockRuntime();
    setResponse(() => ({
      accepted: true,
      module_id: "mod-y",
      meerkat_id: null,
      profile: null,
    }));

    const result = await handle.spawnMember("mod-y");
    assert.equal(calls[0].method, "mobkit/spawn_member");
    assert.deepEqual(calls[0].params, { module_id: "mod-y" });
    assert.equal(result.accepted, true);
    assert.equal(result.moduleId, "mod-y");
    assert.equal(result.meerkatId, null);
    assert.equal(result.profile, null);
  });
});

describe("MobHandle.reconcile()", () => {
  it("sends mobkit/reconcile with modules array", async () => {
    const { handle, calls, setResponse } = createMockRuntime();
    setResponse(() => ({
      accepted: true,
      reconciled_modules: ["mod-a", "mod-b"],
      added: 2,
    }));

    const result = await handle.reconcile(["mod-a", "mod-b"]);
    assert.equal(calls[0].method, "mobkit/reconcile");
    assert.deepEqual(calls[0].params, { modules: ["mod-a", "mod-b"] });
    assert.equal(result.accepted, true);
    assert.deepEqual(result.reconciledModules, ["mod-a", "mod-b"]);
    assert.equal(result.added, 2);
  });
});

describe("MobHandle.subscribeEvents()", () => {
  it("sends mobkit/events/subscribe with scope", async () => {
    const { handle, calls, setResponse } = createMockRuntime();
    setResponse(() => ({
      scope: "mob",
      replay_from_event_id: null,
      keep_alive: { interval_ms: 30000, event: "ping" },
      keep_alive_comment: "keep-alive",
      event_frames: ["frame-1"],
      events: [
        {
          event_id: "e-1",
          source: "system",
          timestamp_ms: 1000,
          event: { type: "test" },
        },
      ],
    }));

    const result = await handle.subscribeEvents("mob", "evt-0", "agent-1");
    assert.equal(calls[0].method, "mobkit/events/subscribe");
    assert.equal(calls[0].params!.scope, "mob");
    assert.equal(calls[0].params!.last_event_id, "evt-0");
    assert.equal(calls[0].params!.agent_id, "agent-1");
    assert.equal(result.scope, "mob");
    assert.equal(result.replayFromEventId, null);
    assert.equal(result.keepAlive.intervalMs, 30000);
    assert.equal(result.keepAlive.event, "ping");
    assert.equal(result.events.length, 1);
    assert.equal(result.events[0].eventId, "e-1");
  });

  it("defaults scope to mob with no optional params", async () => {
    const { handle, calls, setResponse } = createMockRuntime();
    setResponse(() => ({
      scope: "mob",
      replay_from_event_id: null,
      keep_alive: { interval_ms: 30000, event: "ping" },
      keep_alive_comment: "",
      event_frames: [],
      events: [],
    }));

    await handle.subscribeEvents();
    assert.deepEqual(calls[0].params, { scope: "mob" });
  });
});

describe("MobHandle.send()", () => {
  it("sends mobkit/send_message and parses result", async () => {
    const { handle, calls, setResponse } = createMockRuntime();
    setResponse(() => ({
      accepted: true,
      member_id: "m-1",
      session_id: "sess-1",
    }));

    const result = await handle.send("m-1", "Hello!");
    assert.equal(calls[0].method, "mobkit/send_message");
    assert.deepEqual(calls[0].params, { member_id: "m-1", message: "Hello!" });
    assert.equal(result.accepted, true);
    assert.equal(result.memberId, "m-1");
    assert.equal(result.sessionId, "sess-1");
  });
});

describe("MobHandle.sendMessage()", () => {
  it("is an alias for send()", async () => {
    const { handle, calls, setResponse } = createMockRuntime();
    setResponse(() => ({
      accepted: true,
      member_id: "m-2",
      session_id: "sess-2",
    }));

    const result = await handle.sendMessage("m-2", "Hi");
    assert.equal(calls[0].method, "mobkit/send_message");
    assert.deepEqual(calls[0].params, { member_id: "m-2", message: "Hi" });
    assert.equal(result.accepted, true);
    assert.equal(result.memberId, "m-2");
  });
});

describe("MobHandle.ensureMember()", () => {
  it("sends mobkit/ensure_member with all options", async () => {
    const { handle, calls, setResponse } = createMockRuntime();
    setResponse(() => ({
      meerkat_id: "m-1",
      profile: "assistant",
      state: "active",
      wired_to: ["m-2"],
      labels: { role: "helper" },
    }));

    const result = await handle.ensureMember("m-1", "assistant", {
      labels: { role: "helper" },
      context: { foo: "bar" },
      resumeSessionId: "sess-old",
      additionalInstructions: ["Be nice"],
    });

    assert.equal(calls[0].method, "mobkit/ensure_member");
    assert.equal(calls[0].params!.profile, "assistant");
    assert.equal(calls[0].params!.meerkat_id, "m-1");
    assert.deepEqual(calls[0].params!.labels, { role: "helper" });
    assert.deepEqual(calls[0].params!.context, { foo: "bar" });
    assert.equal(calls[0].params!.resume_session_id, "sess-old");
    assert.deepEqual(calls[0].params!.additional_instructions, ["Be nice"]);
    assert.equal(result.meerkatId, "m-1");
    assert.equal(result.profile, "assistant");
    assert.equal(result.state, "active");
    assert.deepEqual(result.wiredTo, ["m-2"]);
    assert.deepEqual(result.labels, { role: "helper" });
  });

  it("sends without optional options", async () => {
    const { handle, calls, setResponse } = createMockRuntime();
    setResponse(() => ({
      meerkat_id: "m-1",
      profile: "assistant",
      state: "active",
      wired_to: [],
      labels: {},
    }));

    await handle.ensureMember("m-1", "assistant");
    assert.equal(calls[0].params!.profile, "assistant");
    assert.equal(calls[0].params!.meerkat_id, "m-1");
    assert.equal(calls[0].params!.labels, undefined);
    assert.equal(calls[0].params!.context, undefined);
  });
});

describe("MobHandle.findMembers()", () => {
  it("sends mobkit/find_members and returns array", async () => {
    const { handle, calls, setResponse } = createMockRuntime();
    setResponse(() => [
      { meerkat_id: "m-1", profile: "a", state: "active", wired_to: [], labels: {} },
      { meerkat_id: "m-2", profile: "b", state: "active", wired_to: [], labels: {} },
    ]);

    const result = await handle.findMembers("role", "helper");
    assert.equal(calls[0].method, "mobkit/find_members");
    assert.deepEqual(calls[0].params, { label_key: "role", label_value: "helper" });
    assert.equal(result.length, 2);
    assert.equal(result[0].meerkatId, "m-1");
    assert.equal(result[1].meerkatId, "m-2");
  });

  it("returns empty array when response is not an array", async () => {
    const { handle, setResponse } = createMockRuntime();
    setResponse(() => null);

    const result = await handle.findMembers("role", "x");
    assert.deepEqual(result, []);
  });
});

describe("MobHandle.listMembers()", () => {
  it("sends mobkit/list_members and returns array", async () => {
    const { handle, calls, setResponse } = createMockRuntime();
    setResponse(() => [
      { meerkat_id: "m-1", profile: "a", state: "active", wired_to: [], labels: {} },
    ]);

    const result = await handle.listMembers();
    assert.equal(calls[0].method, "mobkit/list_members");
    assert.equal(result.length, 1);
    assert.equal(result[0].meerkatId, "m-1");
  });

  it("returns empty array when response is not an array", async () => {
    const { handle, setResponse } = createMockRuntime();
    setResponse(() => "unexpected");

    const result = await handle.listMembers();
    assert.deepEqual(result, []);
  });
});

describe("MobHandle.getMember()", () => {
  it("sends mobkit/get_member and parses snapshot", async () => {
    const { handle, calls, setResponse } = createMockRuntime();
    setResponse(() => ({
      meerkat_id: "m-1",
      profile: "assistant",
      state: "active",
      wired_to: ["m-2"],
      labels: { team: "alpha" },
    }));

    const result = await handle.getMember("m-1");
    assert.equal(calls[0].method, "mobkit/get_member");
    assert.deepEqual(calls[0].params, { member_id: "m-1" });
    assert.equal(result.meerkatId, "m-1");
    assert.equal(result.profile, "assistant");
    assert.deepEqual(result.labels, { team: "alpha" });
  });
});

describe("MobHandle.retireMember()", () => {
  it("sends mobkit/retire_member", async () => {
    const { handle, calls, setResponse } = createMockRuntime();
    setResponse(() => ({}));

    await handle.retireMember("m-1");
    assert.equal(calls[0].method, "mobkit/retire_member");
    assert.deepEqual(calls[0].params, { member_id: "m-1" });
  });
});

describe("MobHandle.respawnMember()", () => {
  it("sends mobkit/respawn_member", async () => {
    const { handle, calls, setResponse } = createMockRuntime();
    setResponse(() => ({}));

    await handle.respawnMember("m-1");
    assert.equal(calls[0].method, "mobkit/respawn_member");
    assert.deepEqual(calls[0].params, { member_id: "m-1" });
  });
});

describe("MobHandle.resolveRouting()", () => {
  it("sends mobkit/routing/resolve with recipient", async () => {
    const { handle, calls, setResponse } = createMockRuntime();
    setResponse(() => ({
      recipient: "user@example.com",
      route: { sink: "email", target_module: "mailer" },
    }));

    const result = await handle.resolveRouting("user@example.com", { hint: "email" });
    assert.equal(calls[0].method, "mobkit/routing/resolve");
    assert.equal(calls[0].params!.recipient, "user@example.com");
    assert.equal(calls[0].params!.hint, "email");
    assert.equal(result.recipient, "user@example.com");
    assert.deepEqual(result.route, { sink: "email", target_module: "mailer" });
  });

  it("works without options", async () => {
    const { handle, calls, setResponse } = createMockRuntime();
    setResponse(() => ({ recipient: "r", route: {} }));

    await handle.resolveRouting("r");
    assert.deepEqual(calls[0].params, { recipient: "r" });
  });
});

describe("MobHandle.listRoutes()", () => {
  it("sends mobkit/routing/routes/list and parses routes", async () => {
    const { handle, calls, setResponse } = createMockRuntime();
    setResponse(() => ({
      routes: [
        {
          route_key: "rk-1",
          recipient: "user@ex.com",
          channel: "email",
          sink: "smtp",
          target_module: "mailer",
        },
      ],
    }));

    const result = await handle.listRoutes();
    assert.equal(calls[0].method, "mobkit/routing/routes/list");
    assert.equal(result.length, 1);
    assert.equal(result[0].routeKey, "rk-1");
    assert.equal(result[0].recipient, "user@ex.com");
    assert.equal(result[0].channel, "email");
    assert.equal(result[0].sink, "smtp");
    assert.equal(result[0].targetModule, "mailer");
  });
});

describe("MobHandle.addRoute()", () => {
  it("sends mobkit/routing/routes/add with all params", async () => {
    const { handle, calls, setResponse } = createMockRuntime();
    setResponse(() => ({
      route: {
        route_key: "rk-new",
        recipient: "alice",
        channel: "slack",
        sink: "webhook",
        target_module: "notifier",
      },
    }));

    const result = await handle.addRoute("rk-new", "alice", "webhook", "notifier", "slack");
    assert.equal(calls[0].method, "mobkit/routing/routes/add");
    assert.equal(calls[0].params!.route_key, "rk-new");
    assert.equal(calls[0].params!.recipient, "alice");
    assert.equal(calls[0].params!.sink, "webhook");
    assert.equal(calls[0].params!.target_module, "notifier");
    assert.equal(calls[0].params!.channel, "slack");
    assert.equal(result.routeKey, "rk-new");
    assert.equal(result.channel, "slack");
  });

  it("omits channel when not provided", async () => {
    const { handle, calls, setResponse } = createMockRuntime();
    setResponse(() => ({
      route: { route_key: "rk", recipient: "r", channel: null, sink: "s", target_module: "tm" },
    }));

    await handle.addRoute("rk", "r", "s", "tm");
    assert.equal(calls[0].params!.channel, undefined);
  });
});

describe("MobHandle.deleteRoute()", () => {
  it("sends mobkit/routing/routes/delete", async () => {
    const { handle, calls, setResponse } = createMockRuntime();
    setResponse(() => ({
      deleted: {
        route_key: "rk-del",
        recipient: "bob",
        channel: null,
        sink: "email",
        target_module: "mailer",
      },
    }));

    const result = await handle.deleteRoute("rk-del");
    assert.equal(calls[0].method, "mobkit/routing/routes/delete");
    assert.deepEqual(calls[0].params, { route_key: "rk-del" });
    assert.equal(result.routeKey, "rk-del");
    assert.equal(result.channel, null);
  });
});

describe("MobHandle.sendDelivery()", () => {
  it("sends mobkit/delivery/send with options", async () => {
    const { handle, calls, setResponse } = createMockRuntime();
    setResponse(() => ({
      delivered: true,
      delivery_id: "dlv-1",
    }));

    const result = await handle.sendDelivery({ recipient: "alice", payload: "hi" });
    assert.equal(calls[0].method, "mobkit/delivery/send");
    assert.deepEqual(calls[0].params, { recipient: "alice", payload: "hi" });
    assert.equal(result.delivered, true);
    assert.equal(result.deliveryId, "dlv-1");
  });
});

describe("MobHandle.deliveryHistory()", () => {
  it("sends mobkit/delivery/history with defaults", async () => {
    const { handle, calls, setResponse } = createMockRuntime();
    setResponse(() => ({
      deliveries: [{ id: "dlv-1" }],
    }));

    const result = await handle.deliveryHistory();
    assert.equal(calls[0].method, "mobkit/delivery/history");
    assert.deepEqual(calls[0].params, { limit: 20 });
    assert.equal(result.deliveries.length, 1);
  });

  it("sends with recipient, sink, and custom limit", async () => {
    const { handle, calls, setResponse } = createMockRuntime();
    setResponse(() => ({ deliveries: [] }));

    await handle.deliveryHistory("alice", "email", 5);
    assert.equal(calls[0].params!.recipient, "alice");
    assert.equal(calls[0].params!.sink, "email");
    assert.equal(calls[0].params!.limit, 5);
  });
});

describe("MobHandle.memoryQuery()", () => {
  it("sends mobkit/memory/query", async () => {
    const { handle, calls, setResponse } = createMockRuntime();
    setResponse(() => ({
      results: [{ entity: "foo", topic: "bar" }],
    }));

    const result = await handle.memoryQuery("search term", { store: "main" });
    assert.equal(calls[0].method, "mobkit/memory/query");
    assert.equal(calls[0].params!.query, "search term");
    assert.equal(calls[0].params!.store, "main");
    assert.equal(result.results.length, 1);
  });
});

describe("MobHandle.memoryStores()", () => {
  it("sends mobkit/memory/stores and parses stores", async () => {
    const { handle, calls, setResponse } = createMockRuntime();
    setResponse(() => ({
      stores: [
        { store: "main", record_count: 42 },
        { store: "archive", record_count: 100 },
      ],
    }));

    const result = await handle.memoryStores();
    assert.equal(calls[0].method, "mobkit/memory/stores");
    assert.equal(result.length, 2);
    assert.equal(result[0].store, "main");
    assert.equal(result[0].recordCount, 42);
    assert.equal(result[1].store, "archive");
    assert.equal(result[1].recordCount, 100);
  });
});

describe("MobHandle.memoryIndex()", () => {
  it("sends mobkit/memory/index", async () => {
    const { handle, calls, setResponse } = createMockRuntime();
    setResponse(() => ({
      entity: "user-1",
      topic: "preferences",
      store: "main",
      assertion_id: "assert-1",
    }));

    const result = await handle.memoryIndex("user-1", "preferences", "main", { extra: true });
    assert.equal(calls[0].method, "mobkit/memory/index");
    assert.equal(calls[0].params!.entity, "user-1");
    assert.equal(calls[0].params!.topic, "preferences");
    assert.equal(calls[0].params!.store, "main");
    assert.equal(calls[0].params!.extra, true);
    assert.equal(result.entity, "user-1");
    assert.equal(result.topic, "preferences");
    assert.equal(result.store, "main");
    assert.equal(result.assertionId, "assert-1");
  });
});

describe("MobHandle.callTool()", () => {
  it("sends mobkit/call_tool with args", async () => {
    const { handle, calls, setResponse } = createMockRuntime();
    setResponse(() => ({
      module_id: "google-workspace",
      tool: "gmail_search",
      result: { messages: ["msg-1"] },
    }));

    const result = await handle.callTool("google-workspace", "gmail_search", { query: "is:unread" });
    assert.equal(calls[0].method, "mobkit/call_tool");
    assert.equal(calls[0].params!.module_id, "google-workspace");
    assert.equal(calls[0].params!.tool, "gmail_search");
    assert.deepEqual(calls[0].params!.arguments, { query: "is:unread" });
    assert.equal(result.moduleId, "google-workspace");
    assert.equal(result.tool, "gmail_search");
    assert.deepEqual(result.result, { messages: ["msg-1"] });
  });

  it("omits arguments when not provided", async () => {
    const { handle, calls, setResponse } = createMockRuntime();
    setResponse(() => ({ module_id: "m", tool: "t", result: null }));

    await handle.callTool("m", "t");
    assert.equal(calls[0].params!.arguments, undefined);
  });
});

describe("MobHandle.toolCaller()", () => {
  it("returns a ToolCaller that calls the right module", async () => {
    const { handle, calls, setResponse } = createMockRuntime();
    setResponse(() => ({
      module_id: "ws",
      tool: "search",
      result: { items: [1, 2] },
    }));

    const caller = handle.toolCaller("ws");
    assert.ok(caller instanceof ToolCaller);

    const result = await caller.call("search", { q: "test" });
    assert.equal(calls[0].method, "mobkit/call_tool");
    assert.equal(calls[0].params!.module_id, "ws");
    assert.equal(calls[0].params!.tool, "search");
    assert.deepEqual(calls[0].params!.arguments, { q: "test" });
    // ToolCaller.call returns result.result, not the full CallToolResult
    assert.deepEqual(result, { items: [1, 2] });
  });
});

describe("MobHandle.gatingEvaluate()", () => {
  it("sends mobkit/gating/evaluate", async () => {
    const { handle, calls, setResponse } = createMockRuntime();
    setResponse(() => ({
      action_id: "act-1",
      action: "delete_account",
      actor_id: "user-1",
      risk_tier: "high",
      outcome: "pending",
      pending_id: "pend-1",
    }));

    const result = await handle.gatingEvaluate("delete_account", "user-1", { context: "admin" });
    assert.equal(calls[0].method, "mobkit/gating/evaluate");
    assert.equal(calls[0].params!.action, "delete_account");
    assert.equal(calls[0].params!.actor_id, "user-1");
    assert.equal(calls[0].params!.context, "admin");
    assert.equal(result.actionId, "act-1");
    assert.equal(result.action, "delete_account");
    assert.equal(result.actorId, "user-1");
    assert.equal(result.riskTier, "high");
    assert.equal(result.outcome, "pending");
    assert.equal(result.pendingId, "pend-1");
  });
});

describe("MobHandle.gatingPending()", () => {
  it("sends mobkit/gating/pending and parses entries", async () => {
    const { handle, calls, setResponse } = createMockRuntime();
    setResponse(() => ({
      pending: [
        {
          pending_id: "p-1",
          action_id: "a-1",
          action: "delete",
          actor_id: "u-1",
          risk_tier: "high",
          created_at_ms: 1000,
        },
      ],
    }));

    const result = await handle.gatingPending();
    assert.equal(calls[0].method, "mobkit/gating/pending");
    assert.equal(result.length, 1);
    assert.equal(result[0].pendingId, "p-1");
    assert.equal(result[0].actionId, "a-1");
    assert.equal(result[0].action, "delete");
    assert.equal(result[0].actorId, "u-1");
    assert.equal(result[0].riskTier, "high");
    assert.equal(result[0].createdAtMs, 1000);
  });
});

describe("MobHandle.gatingDecide()", () => {
  it("sends mobkit/gating/decide", async () => {
    const { handle, calls, setResponse } = createMockRuntime();
    setResponse(() => ({
      pending_id: "p-1",
      action_id: "a-1",
      decision: "approved",
    }));

    const result = await handle.gatingDecide("p-1", "approved", "admin-1", { note: "looks good" });
    assert.equal(calls[0].method, "mobkit/gating/decide");
    assert.equal(calls[0].params!.pending_id, "p-1");
    assert.equal(calls[0].params!.decision, "approved");
    assert.equal(calls[0].params!.approver_id, "admin-1");
    assert.equal(calls[0].params!.note, "looks good");
    assert.equal(result.pendingId, "p-1");
    assert.equal(result.actionId, "a-1");
    assert.equal(result.decision, "approved");
  });
});

describe("MobHandle.gatingAudit()", () => {
  it("sends mobkit/gating/audit with default limit", async () => {
    const { handle, calls, setResponse } = createMockRuntime();
    setResponse(() => ({
      entries: [
        {
          audit_id: "aud-1",
          timestamp_ms: 2000,
          event_type: "decision",
          action_id: "a-1",
          actor_id: "u-1",
          risk_tier: "medium",
          outcome: "approved",
        },
      ],
    }));

    const result = await handle.gatingAudit();
    assert.equal(calls[0].method, "mobkit/gating/audit");
    assert.deepEqual(calls[0].params, { limit: 100 });
    assert.equal(result.length, 1);
    assert.equal(result[0].auditId, "aud-1");
    assert.equal(result[0].timestampMs, 2000);
    assert.equal(result[0].eventType, "decision");
    assert.equal(result[0].riskTier, "medium");
  });

  it("sends with custom limit", async () => {
    const { handle, calls, setResponse } = createMockRuntime();
    setResponse(() => ({ entries: [] }));

    await handle.gatingAudit(10);
    assert.deepEqual(calls[0].params, { limit: 10 });
  });
});

describe("MobHandle.rediscover()", () => {
  it("returns RediscoverReport on normal response", async () => {
    const { handle, calls, setResponse } = createMockRuntime();
    setResponse(() => ({
      spawned: ["mod-a"],
      edges: {
        desired_edges: [],
        wired_edges: [{ from: "a", to: "b" }],
        unwired_edges: [],
        retained_edges: [],
        preexisting_edges: [],
        skipped_missing_members: [],
        pruned_stale_managed_edges: [],
        failures: [],
      },
    }));

    const result = await handle.rediscover();
    assert.equal(calls[0].method, "mobkit/rediscover");
    assert.notEqual(result, null);
    assert.deepEqual(result!.spawned, ["mod-a"]);
    assert.equal(result!.edges.wiredEdges.length, 1);
    assert.equal(result!.edges.isComplete, true);
  });

  it("returns null when status response", async () => {
    const { handle, setResponse } = createMockRuntime();
    setResponse(() => ({ status: "no_discovery_configured" }));

    const result = await handle.rediscover();
    assert.equal(result, null);
  });
});

describe("MobHandle.reconcileEdges()", () => {
  it("sends mobkit/reconcile_edges and parses report", async () => {
    const { handle, calls, setResponse } = createMockRuntime();
    setResponse(() => ({
      desired_edges: [{ from: "a", to: "b" }],
      wired_edges: [{ from: "a", to: "b" }],
      unwired_edges: [],
      retained_edges: [],
      preexisting_edges: [],
      skipped_missing_members: [],
      pruned_stale_managed_edges: [],
      failures: [],
    }));

    const result = await handle.reconcileEdges();
    assert.equal(calls[0].method, "mobkit/reconcile_edges");
    assert.equal(result.desiredEdges.length, 1);
    assert.equal(result.wiredEdges.length, 1);
    assert.equal(result.isComplete, true);
  });

  it("isComplete is false when there are failures", async () => {
    const { handle, setResponse } = createMockRuntime();
    setResponse(() => ({
      desired_edges: [],
      wired_edges: [],
      unwired_edges: [],
      retained_edges: [],
      preexisting_edges: [],
      skipped_missing_members: [],
      pruned_stale_managed_edges: [],
      failures: [{ error: "some error" }],
    }));

    const result = await handle.reconcileEdges();
    assert.equal(result.isComplete, false);
  });
});

describe("MobHandle.queryEvents()", () => {
  it("sends mobkit/query_events and returns parsed events", async () => {
    const { handle, calls, setResponse } = createMockRuntime();
    setResponse(() => [
      {
        id: "evt-1",
        seq: 1,
        timestamp_ms: 5000,
        member_id: "m-1",
        event: { Agent: { agent_id: "a-1", event_type: "run_started" } },
      },
      {
        id: "evt-2",
        seq: 2,
        timestamp_ms: 6000,
        member_id: null,
        event: { Module: { module: "mod-x", event_type: "loaded", payload: {} } },
      },
    ]);

    const result = await handle.queryEvents({ sinceMs: 1000, limit: 50 });
    assert.equal(calls[0].method, "mobkit/query_events");
    assert.equal(calls[0].params!.since_ms, 1000);
    assert.equal(calls[0].params!.limit, 50);
    assert.equal(result.length, 2);
    assert.equal(result[0].id, "evt-1");
    assert.equal(result[0].seq, 1);
    assert.equal(result[0].memberId, "m-1");
    assert.equal(result[0].event.kind, "agent");
    assert.equal(result[1].event.kind, "module");
  });

  it("returns empty array when no query is passed", async () => {
    const { handle, calls, setResponse } = createMockRuntime();
    setResponse(() => []);

    const result = await handle.queryEvents();
    assert.equal(calls[0].method, "mobkit/query_events");
    assert.deepEqual(calls[0].params, {});
    assert.deepEqual(result, []);
  });

  it("returns empty array when no_event_log_configured", async () => {
    const { handle, setResponse } = createMockRuntime();
    setResponse(() => ({ status: "no_event_log_configured" }));

    const result = await handle.queryEvents({ limit: 10 });
    assert.deepEqual(result, []);
  });

  it("returns empty array when response is not an array and not status", async () => {
    const { handle, setResponse } = createMockRuntime();
    setResponse(() => ({ something: "else" }));

    const result = await handle.queryEvents();
    assert.deepEqual(result, []);
  });
});
