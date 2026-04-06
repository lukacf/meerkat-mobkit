var __getOwnPropNames = Object.getOwnPropertyNames;
var __esm = (fn, res) => function __init() {
  return fn && (res = (0, fn[__getOwnPropNames(fn)[0]])(fn = 0)), res;
};
var __commonJS = (cb, mod) => function __require() {
  return mod || (0, cb[__getOwnPropNames(cb)[0]])((mod = { exports: {} }).exports, mod), mod.exports;
};

// ../packages/console-core/src/control-plane.ts
function trimString(value) {
  if (typeof value !== "string") {
    return void 0;
  }
  const trimmed = value.trim();
  return trimmed || void 0;
}
function stringRecord(value) {
  if (!value || typeof value !== "object") {
    return {};
  }
  return Object.fromEntries(
    Object.entries(value).map(([key, raw]) => {
      const normalizedKey = trimString(key);
      const normalizedValue = trimString(raw);
      return normalizedKey && normalizedValue ? [normalizedKey, normalizedValue] : null;
    }).filter((entry) => Boolean(entry))
  );
}
function normalizeResponsePhase(value) {
  switch (value) {
    case "waiting":
    case "tool-executing":
    case "generating":
      return value;
    case null:
    case void 0:
      return null;
    default:
      return null;
  }
}
function normalizeSidebarWatchFields(value) {
  const record = value && typeof value === "object" ? value : {};
  const normalized = {};
  if (typeof record.watched === "boolean") {
    normalized.watched = record.watched;
  }
  if (record.alertLevel === "elevated" || record.alertLevel === "critical" || record.alertLevel === null) {
    normalized.alertLevel = record.alertLevel;
  }
  if (typeof record.degraded === "boolean") {
    normalized.degraded = record.degraded;
  }
  const degradedReason = trimString(record.degradedReason);
  if (degradedReason) {
    normalized.degradedReason = degradedReason;
  }
  return normalized;
}
function normalizeIdentityStatusRow(value) {
  const record = value && typeof value === "object" ? value : null;
  if (!record) {
    return null;
  }
  const identity = trimString(record.identity);
  const state = trimString(record.state);
  if (!identity || !state) {
    return null;
  }
  const addressability = record.addressability === "internal_only" ? "internal_only" : record.addressability === "addressable" ? "addressable" : null;
  if (!addressability) {
    return null;
  }
  return {
    identity,
    state,
    addressability,
    labels: stringRecord(record.labels),
    ...trimString(record.display_name) ? { display_name: trimString(record.display_name) } : {},
    ...trimString(record.profile) ? { profile: trimString(record.profile) } : {},
    ...typeof record.generation === "number" && Number.isFinite(record.generation) ? { generation: record.generation } : {},
    ...typeof record.checkpoint_version === "number" && Number.isFinite(record.checkpoint_version) ? { checkpoint_version: record.checkpoint_version } : {},
    ...typeof record.lease_healthy === "boolean" ? { lease_healthy: record.lease_healthy } : {}
  };
}
var init_control_plane = __esm({
  "../packages/console-core/src/control-plane.ts"() {
  }
});

// ../packages/console-core/src/rich-content.ts
var init_rich_content = __esm({
  "../packages/console-core/src/rich-content.ts"() {
  }
});

// ../packages/console-core/src/conversation.ts
var init_conversation = __esm({
  "../packages/console-core/src/conversation.ts"() {
    init_rich_content();
  }
});

// ../packages/console-core/src/dock.ts
var init_dock = __esm({
  "../packages/console-core/src/dock.ts"() {
  }
});

// ../packages/console-core/src/sidebar.ts
var init_sidebar = __esm({
  "../packages/console-core/src/sidebar.ts"() {
    init_control_plane();
  }
});

// ../packages/console-core/src/format.ts
var init_format = __esm({
  "../packages/console-core/src/format.ts"() {
  }
});

// ../packages/console-core/src/index.ts
var init_src = __esm({
  "../packages/console-core/src/index.ts"() {
    init_control_plane();
    init_conversation();
    init_dock();
    init_sidebar();
    init_rich_content();
    init_format();
  }
});

// src/lib/agents.ts
function normalizeAgents(experience, modules) {
  const identityStatusRows = Array.isArray(experience?.identity_status?.rows) ? experience.identity_status.rows : [];
  const normalizedIdentityStatusRows = identityStatusRows.map((entry) => normalizeIdentityStatusRow(entry)).filter((entry) => entry !== null);
  const identityStatusByIdentity = new Map(
    normalizedIdentityStatusRows.map((row) => [row.identity, row])
  );
  const snapshotAgents = experience?.agent_sidebar?.live_snapshot?.agents;
  if (Array.isArray(snapshotAgents) && snapshotAgents.length > 0) {
    return snapshotAgents.map((entry) => {
      const entryIdentity = typeof entry.identity === "string" ? entry.identity.trim() : "";
      const entryMemberId = typeof entry.member_id === "string" ? entry.member_id.trim() : "";
      const statusRow = identityStatusByIdentity.get(entryIdentity) || identityStatusByIdentity.get(entryMemberId) || normalizeIdentityStatusRow(entry);
      const watchFields = normalizeSidebarWatchFields(entry);
      const responsePhase = normalizeResponsePhase(entry.response_phase);
      return {
        ...statusRow?.identity ? { identity: statusRow.identity } : entry.identity ? { identity: String(entry.identity) } : {},
        agent_id: String(entry.agent_id || statusRow?.identity || entry.identity || entry.member_id || ""),
        member_id: String(entry.member_id || statusRow?.identity || entry.identity || entry.agent_id || ""),
        ...typeof entry.session_id === "string" && entry.session_id.trim() ? { session_id: entry.session_id.trim() } : {},
        label: String(entry.label || statusRow?.display_name || entry.display_name || statusRow?.identity || entry.identity || entry.member_id || entry.agent_id || "unknown"),
        kind: String(entry.kind || statusRow?.profile || entry.profile || "module_agent"),
        ...statusRow?.profile !== void 0 ? { profile: statusRow.profile } : entry.profile !== void 0 ? { profile: String(entry.profile) } : {},
        ...statusRow?.state !== void 0 ? { state: statusRow.state } : entry.state !== void 0 ? { state: String(entry.state) } : {},
        ...statusRow?.addressability ? { addressability: statusRow.addressability } : {},
        ...statusRow?.generation !== void 0 ? { generation: statusRow.generation } : {},
        ...statusRow?.checkpoint_version !== void 0 ? { checkpoint_version: statusRow.checkpoint_version } : {},
        ...statusRow?.lease_healthy !== void 0 ? { lease_healthy: statusRow.lease_healthy } : {},
        ...responsePhase !== null && { response_phase: responsePhase },
        ...entry.wired_to !== void 0 && { wired_to: entry.wired_to },
        ...statusRow?.labels && Object.keys(statusRow.labels).length > 0 ? { labels: statusRow.labels } : entry.labels !== void 0 ? { labels: entry.labels } : {},
        ...entry.group !== void 0 && { group: String(entry.group) },
        ...entry.addressable !== void 0 ? { addressable: Boolean(entry.addressable) } : statusRow?.addressability ? { addressable: statusRow.addressability === "addressable" } : {},
        ...entry.affordances !== void 0 && { affordances: entry.affordances },
        ...watchFields
      };
    });
  }
  if (Array.isArray(identityStatusRows) && identityStatusRows.length > 0) {
    return identityStatusRows.map((entry) => {
      const statusRow = normalizeIdentityStatusRow(entry);
      const identity = statusRow?.identity || "";
      return {
        identity,
        agent_id: String(identity),
        member_id: identity ? `identity-only:${identity}` : "",
        ...typeof statusRow?.session_id === "string" && statusRow.session_id.trim() ? { session_id: statusRow.session_id.trim() } : {},
        label: String(statusRow?.display_name || identity || "unknown"),
        kind: String(statusRow?.profile || "identity"),
        ...statusRow?.profile !== void 0 ? { profile: statusRow.profile } : {},
        ...statusRow?.state !== void 0 ? { state: statusRow.state } : {},
        ...statusRow?.addressability ? { addressability: statusRow.addressability } : {},
        ...statusRow?.generation !== void 0 ? { generation: statusRow.generation } : {},
        ...statusRow?.checkpoint_version !== void 0 ? { checkpoint_version: statusRow.checkpoint_version } : {},
        ...statusRow?.lease_healthy !== void 0 ? { lease_healthy: statusRow.lease_healthy } : {},
        ...statusRow?.labels && Object.keys(statusRow.labels).length > 0 ? { labels: statusRow.labels } : {},
        addressable: false,
        affordances: { can_send_message: false }
      };
    });
  }
  if (Array.isArray(modules) && modules.length > 0) {
    return modules.map((moduleId) => ({
      agent_id: String(moduleId),
      member_id: String(moduleId),
      label: String(moduleId),
      kind: "module_agent"
    }));
  }
  return [];
}
var init_agents = __esm({
  "src/lib/agents.ts"() {
    init_src();
  }
});

// src/lib/agents.test.ts
import assert from "node:assert/strict";
import test from "node:test";
var require_agents_test = __commonJS({
  "src/lib/agents.test.ts"() {
    init_agents();
    test("normalizeAgents preserves identity-status summary fields without invention", () => {
      const agents = normalizeAgents(
        {
          agent_sidebar: {
            live_snapshot: {
              agents: [
                {
                  identity: " identity:luka ",
                  display_name: " Luka ",
                  profile: " operator ",
                  state: " running ",
                  addressability: "addressable",
                  generation: 4,
                  checkpoint_version: 8,
                  lease_healthy: true,
                  labels: { team: " console " }
                }
              ]
            }
          }
        },
        []
      );
      assert.equal(agents.length, 1);
      const agent = agents[0];
      assert.equal(agent?.identity, "identity:luka");
      assert.equal(agent?.member_id, "identity:luka");
      assert.equal(agent?.label, "Luka");
      assert.equal(agent?.profile, "operator");
      assert.equal(agent?.state, "running");
      assert.equal(agent?.addressability, "addressable");
      assert.equal(agent?.generation, 4);
      assert.equal(agent?.checkpoint_version, 8);
      assert.equal(agent?.lease_healthy, true);
      assert.deepEqual(agent?.labels, { team: "console" });
    });
    test("normalizeAgents falls back to identity_status rows when sidebar snapshot is absent", () => {
      const agents = normalizeAgents(
        {
          identity_status: {
            schema_version: "1",
            refresh: { mode: "poll", interval_ms: 5e3 },
            rows: [
              {
                identity: " identity:luka ",
                display_name: " Luka ",
                profile: " operator ",
                state: " running ",
                addressability: "addressable",
                labels: { team: " console " },
                generation: 4,
                checkpoint_version: 8,
                lease_healthy: true
              }
            ]
          }
        },
        []
      );
      assert.equal(agents.length, 1);
      const agent = agents[0];
      assert.equal(agent?.identity, "identity:luka");
      assert.equal(agent?.member_id, "identity-only:identity:luka");
      assert.equal(agent?.label, "Luka");
      assert.equal(agent?.profile, "operator");
      assert.equal(agent?.state, "running");
      assert.equal(agent?.addressability, "addressable");
      assert.equal(agent?.addressable, false);
      assert.equal(agent?.generation, 4);
      assert.equal(agent?.checkpoint_version, 8);
      assert.equal(agent?.lease_healthy, true);
      assert.deepEqual(agent?.labels, { team: "console" });
      assert.deepEqual(agent?.affordances, { can_send_message: false });
    });
    test("normalizeAgents enriches sidebar snapshot rows with identity_status when both surfaces exist", () => {
      const agents = normalizeAgents(
        {
          agent_sidebar: {
            live_snapshot: {
              agents: [
                {
                  agent_id: "domain:billing",
                  member_id: "domain:billing",
                  label: "Billing",
                  kind: "module_agent",
                  state: "running",
                  addressable: true
                }
              ]
            }
          },
          identity_status: {
            schema_version: "1",
            refresh: { mode: "poll", interval_ms: 5e3 },
            rows: [
              {
                identity: "domain:billing",
                display_name: "Billing",
                profile: "operator",
                state: "running",
                addressability: "addressable",
                labels: { team: "finance" },
                generation: 2,
                checkpoint_version: 3,
                lease_healthy: true
              }
            ]
          }
        },
        []
      );
      assert.equal(agents.length, 1);
      const agent = agents[0];
      assert.equal(agent?.identity, "domain:billing");
      assert.equal(agent?.member_id, "domain:billing");
      assert.equal(agent?.addressability, "addressable");
      assert.equal(agent?.addressable, true);
      assert.equal(agent?.generation, 2);
      assert.equal(agent?.checkpoint_version, 3);
      assert.equal(agent?.lease_healthy, true);
      assert.deepEqual(agent?.labels, { team: "finance" });
    });
  }
});
export default require_agents_test();
