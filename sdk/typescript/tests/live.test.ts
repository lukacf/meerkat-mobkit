import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { describe, it } from "node:test";

import {
  LIVE_EXECUTION_CLIENT_CONTEXT_V1,
  LIVE_EXECUTION_FUNCTION_BRIDGE_V1,
  LIVE_EXECUTION_IDENTITY_V1,
  activeLiveChannelHandleToWire,
  experimentalLiveGatewayConfigToWire,
  liveChannelHandleToWire,
  liveExecutionIdentityV1ToWire,
  liveOpenExecutionIdentityParams,
  liveExecutionModeCapability,
  parseActiveLiveChannelHandle,
  parseExperimentalLiveChannelStatus,
  parseLivePlaybackOwnerReadiness,
  parsePendingLiveChannelHandle,
  parseLiveAssistantOutputAddress,
  parseLiveChannelHandle,
  parseLivePlaybackCompleteResult,
  parseLiveReplacementRequired,
  supportsLiveExecutionIdentityV1,
  supportsLiveExecutionMode,
} from "../src/live.js";
import { parseCapabilitiesResult } from "../src/types.js";

const fixture = JSON.parse(
  readFileSync(
    new URL("../../../meerkat-mobkit/tests/fixtures/live_contracts_v1.json", import.meta.url),
    "utf8",
  ),
) as Record<string, unknown>;

describe("experimental live gateway registration", () => {
  it("serializes every authority-bearing field without defaults", () => {
    assert.deepEqual(
      experimentalLiveGatewayConfigToWire({
        principal: "user:luka",
        realm: "family",
        factoryKind: "openai-gpt-live",
        factoryVersion: "v1",
        gate0Qualification: "gate0-v1",
        authBinding: {
          realm: "family",
          binding: "chatgpt-oauth",
          profile: "luka",
        },
        voice: "marin",
        executionProfiles: [
          {
            profileId: "homecore.reachy.open-room.v1",
            sessionInstructions: "You are Reachy's voice embodiment.",
          },
        ],
      }),
      fixture.experimental_gateway_config,
    );
  });

  it("rejects an auth binding from another realm", () => {
    assert.throws(() =>
      experimentalLiveGatewayConfigToWire({
        principal: "user:luka",
        realm: "family",
        factoryKind: "openai-gpt-live",
        factoryVersion: "v1",
        gate0Qualification: "gate0-v1",
        authBinding: { realm: "other", binding: "chatgpt-oauth" },
        voice: "marin",
      }),
    );
  });

  it("rejects caller-owned instructions in gateway config", () => {
    assert.throws(() =>
      experimentalLiveGatewayConfigToWire({
        principal: "user:luka",
        realm: "family",
        factoryKind: "openai-gpt-live",
        factoryVersion: "v1",
        gate0Qualification: "gate0-v1",
        authBinding: { realm: "family", binding: "chatgpt-oauth" },
        voice: "marin",
        instructions: "caller-owned prompt",
      } as unknown as import("../src/live.js").ExperimentalLiveGatewayConfig),
    );
  });

  it("rejects malformed, reserved, duplicate, and authority-bearing profiles", () => {
    const base = {
      principal: "user:luka",
      realm: "family",
      factoryKind: "openai-gpt-live",
      factoryVersion: "v1",
      gate0Qualification: "gate0-v1",
      authBinding: { realm: "family", binding: "chatgpt-oauth" },
      voice: "marin",
    };
    for (const executionProfiles of [
      [{ profileId: " ", sessionInstructions: "voice" }],
      [{ profileId: "reachy", sessionInstructions: " " }],
      [
        {
          profileId: "openai.gpt-live-1-codex.client-context.v1",
          sessionInstructions: "override canonical policy",
        },
      ],
      [
        {
          profileId: "openai.gpt-live-1-codex.function-bridge.v1",
          sessionInstructions: "relabel FunctionBridge",
        },
      ],
      [
        { profileId: "reachy", sessionInstructions: "voice" },
        { profileId: " reachy ", sessionInstructions: "other" },
      ],
    ]) {
      assert.throws(() =>
        experimentalLiveGatewayConfigToWire({
          ...base,
          executionProfiles,
        } as never),
      );
    }
    assert.throws(() =>
      experimentalLiveGatewayConfigToWire({
        ...base,
        executionProfiles: {},
      } as never),
    );
    for (const field of [
      "mode",
      "model",
      "provider",
      "tools",
      "responses",
      "capabilities",
    ]) {
      assert.throws(() =>
        experimentalLiveGatewayConfigToWire({
          ...base,
          executionProfiles: [
            {
              profileId: "reachy",
              sessionInstructions: "voice",
              [field]: "forbidden",
            },
          ],
        } as never),
      );
    }
  });
});

describe("live execution identity v1", () => {
  it("serializes the shared host-profile fixture", () => {
    assert.deepEqual(
      liveExecutionIdentityV1ToWire({
        profileId: "homecore.reachy.open-room.v1",
      }),
      fixture.execution_identity,
    );
  });

  it("rejects unknown nested fields and legacy conflicts", () => {
    assert.throws(() =>
      liveExecutionIdentityV1ToWire({
        profileId: "homecore.reachy.open-room.v1",
        extra: true,
      } as never),
    );
    for (const [field, value] of [
      ["model", "legacy"],
      ["provider", "openai"],
      ["auth_binding", { realm: "family", binding: "other" }],
      ["self_hosted_server_id", "server"],
      ["provider_params", {}],
    ] as const) {
      assert.throws(() =>
        liveOpenExecutionIdentityParams(
          { profileId: "homecore.reachy.open-room.v1" },
          { [field]: value },
        ),
      );
    }
    for (const input of [
      { profileId: "  " },
      { profileId: "homecore.reachy.open-room.v1", model: "gpt-live-1-codex" },
      { profileId: "homecore.reachy.open-room.v1", provider: "openai" },
      { profileId: "homecore.reachy.open-room.v1", selfHostedServerId: "server" },
      { profileId: "homecore.reachy.open-room.v1", authBinding: { action: "clear" } },
    ]) {
      assert.throws(() => liveExecutionIdentityV1ToWire(input as never));
    }
  });

  it("treats absent experimental capability as unsupported", () => {
    const oldGateway = parseCapabilitiesResult({
      contract_version: "0.5.0",
      methods: [],
      loaded_modules: [],
    });
    assert.deepEqual(oldGateway.featureCapabilities, []);
    assert.equal(supportsLiveExecutionIdentityV1(oldGateway), false);
    assert.equal(
      supportsLiveExecutionIdentityV1({
        featureCapabilities: [LIVE_EXECUTION_IDENTITY_V1],
      }),
      true,
    );
  });
});

describe("LiveChannelHandle", () => {
  it("parses the shared canonical handle fixture", () => {
    const handle = parseLiveChannelHandle(fixture.channel_handle);
    assert.equal(handle.channelId, "live-ch-1");
    assert.equal(handle.targetIdentity, "identity:luka");
    assert.equal(handle.transport.transport, "websocket");
    assert.equal(handle.continuity.mode, "transcript_only");
    assert.equal(handle.capabilities.imageIn, true);
    assert.deepEqual(liveChannelHandleToWire(handle), fixture.channel_handle);
  });
});

describe("experimental live phase handles", () => {
  it("keeps pending custody, readiness, and active authority distinct", () => {
    const pending = parsePendingLiveChannelHandle(fixture.pending_channel_handle);
    const active = parseActiveLiveChannelHandle(fixture.active_channel_handle);
    const readiness = parseLivePlaybackOwnerReadiness(
      fixture.playback_owner_readiness,
    );
    assert.equal(pending.executionMode, "client_context");
    assert.equal(active.channelId, pending.channelId);
    assert.equal(readiness.channelId, pending.channelId);
    assert.notEqual(active.activationReceipt, pending.pendingReceipt);
    assert.deepEqual(
      activeLiveChannelHandleToWire(active),
      fixture.active_channel_handle,
    );
    assert.deepEqual(
      parseExperimentalLiveChannelStatus({
        phase: "active",
        handle: fixture.active_channel_handle,
      }),
      { phase: "active", handle: active },
    );
    assert.deepEqual(
      parseExperimentalLiveChannelStatus(fixture.revoked_status),
      { phase: "revoked" },
    );
    assert.deepEqual(
      parseExperimentalLiveChannelStatus(fixture.closed_status),
      { phase: "closed" },
    );
  });

  it("rejects provider-native mode and configuration leakage", () => {
    assert.throws(() =>
      parseActiveLiveChannelHandle({
        ...(fixture.active_channel_handle as object),
        execution_mode: "responses",
      }),
    );
    assert.throws(() =>
      parseActiveLiveChannelHandle({
        ...(fixture.active_channel_handle as object),
        responses_model: "gpt-5.5",
      }),
    );
    assert.throws(() =>
      parsePendingLiveChannelHandle({
        ...(fixture.pending_channel_handle as object),
        tools: [],
      }),
    );
    assert.throws(() =>
      parseActiveLiveChannelHandle({
        ...(fixture.active_channel_handle as object),
        activation_receipt: "  ",
      }),
    );
  });

  it("advertises provider-neutral modes independently", () => {
    const capabilities = {
      featureCapabilities: [
        LIVE_EXECUTION_IDENTITY_V1,
        LIVE_EXECUTION_FUNCTION_BRIDGE_V1,
      ],
    };
    assert.equal(supportsLiveExecutionMode(capabilities, "function_bridge"), true);
    assert.equal(supportsLiveExecutionMode(capabilities, "client_context"), false);
    assert.equal(
      liveExecutionModeCapability("client_context"),
      LIVE_EXECUTION_CLIENT_CONTEXT_V1,
    );
    assert.throws(() => liveExecutionModeCapability("responses" as never));
  });
});

describe("LiveReplacementRequired", () => {
  it("parses a fresh bootstrap and fences stale channel-shaped input", () => {
    const wire = {
      required: true,
      reason: "delegation_result",
      replacement: fixture.channel_handle,
      canonical_seed_cursor: 17,
    };
    const parsed = parseLiveReplacementRequired(wire);
    assert.equal(parsed.required, true);
    if (parsed.required) {
      assert.equal(parsed.replacement.channelId, "live-ch-1");
      assert.equal(parsed.canonicalSeedCursor, 17);
    }
    assert.deepEqual(parseLiveReplacementRequired({ required: false }), {
      required: false,
    });
    assert.throws(() =>
      parseLiveReplacementRequired({
        required: false,
        channel_id: "stale-old-channel",
      }),
    );
  });
});

describe("LivePlaybackCompleteResult", () => {
  it("accepts only the exact completed terminal", () => {
    assert.deepEqual(parseLivePlaybackCompleteResult({ status: "completed" }), {
      status: "completed",
    });
    assert.throws(() =>
      parseLivePlaybackCompleteResult({
        status: "completed",
        interaction_id: "caller-minted",
      }),
    );
  });
});

describe("LiveAssistantOutputAddress", () => {
  it("accepts only the opaque channel-scoped address", () => {
    const wire = {
      channel_id: "live-ch-1",
      output_id: "opaque-output-1",
      content_index: 0,
    };
    assert.deepEqual(parseLiveAssistantOutputAddress(wire), {
      channelId: "live-ch-1",
      outputId: "opaque-output-1",
      contentIndex: 0,
    });
    assert.throws(() =>
      parseLiveAssistantOutputAddress({ ...wire, item_id: "provider-item" }),
    );
    assert.throws(() =>
      parseLiveAssistantOutputAddress({ ...wire, content_index: -1 }),
    );
  });
});
