import assert from "node:assert/strict";
import test from "node:test";

import { __memoryTest } from "./MemoryPanel";
import type { MemoryPanelRecord } from "../types";

const {
  scopeGroupKey,
  scopeGroupLabel,
  groupRecordsByScope,
  trustLabel,
  statusLabel,
  relativeAge,
  evidenceLabel,
  authorLine,
  dreamOpKindsSummary,
  dreamTimeRange,
  injectionLine,
} = __memoryTest;

function record(overrides: Partial<MemoryPanelRecord> & Pick<MemoryPanelRecord, "id" | "scope">): MemoryPanelRecord {
  return {
    kind: "fact",
    title: overrides.id,
    trust: "agent_observed",
    status: { status: "active" },
    ...overrides,
  } as MemoryPanelRecord;
}

test("scope group keys separate identities but collapse mob/operator/realm", () => {
  assert.equal(
    scopeGroupKey({ scope: "identity", realm: "default", identity: "identity:luka" }),
    "identity:default:identity:luka",
  );
  assert.equal(scopeGroupKey({ scope: "mob", realm: "default", mob: "main" }), "mob:default:main");
  assert.equal(scopeGroupKey({ scope: "operator", realm: "default", operator: "op-1" }), "operator:default:op-1");
  assert.equal(scopeGroupKey({ scope: "realm", realm: "default" }), "realm:default");
});

test("scope group labels are human readable", () => {
  assert.equal(scopeGroupLabel({ scope: "identity", realm: "default", identity: "identity:luka" }), "identity:luka");
  assert.equal(scopeGroupLabel({ scope: "mob", realm: "default", mob: "main" }), "Mob: main");
  assert.equal(scopeGroupLabel({ scope: "operator", realm: "default", operator: "op-1" }), "Operator: op-1");
  assert.equal(scopeGroupLabel({ scope: "realm", realm: "default" }), "Realm");
});

test("records group by scope with identity groups first, then mob/operator/realm", () => {
  const groups = groupRecordsByScope([
    record({ id: "r-realm", scope: { scope: "realm", realm: "default" } }),
    record({ id: "l1", scope: { scope: "identity", realm: "default", identity: "identity:luka" } }),
    record({ id: "m1", scope: { scope: "mob", realm: "default", mob: "main" } }),
    record({ id: "l2", scope: { scope: "identity", realm: "default", identity: "identity:luka" } }),
    record({ id: "o1", scope: { scope: "operator", realm: "default", operator: "op-1" } }),
  ]);

  assert.deepEqual(
    groups.map((g) => g.label),
    ["identity:luka", "Mob: main", "Operator: op-1", "Realm"],
  );
  // Both luka records land in the same identity group.
  assert.equal(groups[0].records.length, 2);
  assert.deepEqual(groups[0].records.map((r) => r.id), ["l1", "l2"]);
});

test("trust and status labels are readable", () => {
  assert.equal(trustLabel("agent_verified"), "verified");
  assert.equal(trustLabel("operator"), "operator");
  assert.equal(statusLabel({ status: "active" }), "active");
  assert.equal(statusLabel({ status: "superseded", by: "mem-9" }), "superseded → mem-9");
  assert.equal(statusLabel({ status: "quarantined", reason: "conflict" }), "quarantined: conflict");
  assert.equal(statusLabel({ status: "tombstoned" }), "tombstoned");
});

test("relative age formats coarse buckets and handles missing timestamps", () => {
  const now = 1_000_000_000_000;
  assert.equal(relativeAge(undefined, now), "—");
  assert.equal(relativeAge(0, now), "—");
  assert.equal(relativeAge(now - 3 * 24 * 60 * 60 * 1000, now), "3d ago");
  assert.equal(relativeAge(now - 5 * 60 * 1000, now), "5m ago");
  assert.equal(relativeAge(now - 500, now), "now");
});

test("evidence labels render session/gen/msg ranges without transcript fetch", () => {
  assert.equal(
    evidenceLabel({ session_id: "s", generation: 2, range: [1, 5] }),
    "session s • gen 2 • msgs 1–5",
  );
  assert.equal(evidenceLabel({ revision: "r7" }), "rev r7");
  assert.equal(evidenceLabel({}), "evidence");
});

test("author lines describe every author variant", () => {
  assert.equal(authorLine({ author: "agent", identity: "identity:x" }), "agent identity:x");
  assert.equal(authorLine({ author: "steward", run_id: "run-1" }), "steward run run-1");
  assert.equal(authorLine({ author: "distiller", run_id: "run-2" }), "distiller run run-2");
  assert.equal(authorLine({ author: "operator" }), "operator");
  assert.equal(authorLine({ author: "application" }), "application");
  assert.equal(authorLine(undefined), "unknown author");
});

test("dream op-kind summary sorts by count and skips zeroes", () => {
  assert.equal(dreamOpKindsSummary({ create: 3, tombstone: 1, noop: 0 }), "3 create · 1 tombstone");
  assert.equal(dreamOpKindsSummary({}), "");
  assert.equal(dreamOpKindsSummary(undefined), "");
});

test("dream time range collapses identical endpoints", () => {
  const now = 2_000_000_000_000;
  const first = now - 2 * 60 * 60 * 1000;
  const last = now - 30 * 60 * 1000;
  assert.equal(dreamTimeRange({ first_op_at_ms: first, last_op_at_ms: last }, now), "2h ago → 30m ago");
  assert.equal(dreamTimeRange({ first_op_at_ms: last, last_op_at_ms: last }, now), "30m ago");
  assert.equal(dreamTimeRange({}, now), "—");
});

test("injection lines combine surface, identity, and relative time", () => {
  const now = 3_000_000_000_000;
  assert.equal(
    injectionLine({ record_id: "m", identity: "identity:luka", surface: "build", at_ms: now - 60 * 60 * 1000 }, now),
    "build • identity:luka • 1h ago",
  );
  assert.equal(
    injectionLine({ record_id: "m", identity: "identity:luka", surface: "turn", at_ms: now - 5 * 1000 }, now),
    "turn • identity:luka • 5s ago",
  );
});
