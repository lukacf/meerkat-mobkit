import assert from "node:assert/strict";
import test from "node:test";

import { MEMORY_TABS, __memoryTest } from "./MemoryPanel";
import type { MemoryDreamRun, MemoryPanelRecord, MemoryPanelRecordsResult } from "../types";
import type { ConsoleFrame } from "../types";

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
  memoryTabLabel,
  resolveMemoryTabAlias,
  buildRecordsQueryParams,
  hasActiveFilter,
  filtersEquivalent,
  buildRecordsListView,
  createMemoryRecordsPager,
  recordUtility,
  sortRecordsByUtility,
  utilityLine,
  formatBytes,
  latticeInvariants,
  runLatticeWalk,
  computeVerdictTiles,
  verdictStatusLabel,
  scopeOverviewRows,
  filterForScope,
  lineageLane,
  dreamRunsTouching,
  evidenceExcerptLines,
  identityOptions,
  knowledgeComposition,
  dedupeFramesById,
  countFramesBehind,
  memoryFramePivot,
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

// ── Phase-1: tab restructure ───────────────────────────────────────────────

test("tab union is holdings/records/knowledge/pipeline/dreams with labels", () => {
  assert.deepEqual(MEMORY_TABS, ["holdings", "records", "knowledge", "pipeline", "dreams"]);
  assert.equal(memoryTabLabel("holdings"), "Holdings");
  assert.equal(memoryTabLabel("pipeline"), "Pipeline");
});

test("quarantine tab alias redirects to pipeline for one release (open Q6)", () => {
  assert.equal(resolveMemoryTabAlias("quarantine"), "pipeline");
  assert.equal(resolveMemoryTabAlias("records"), "records");
  assert.equal(resolveMemoryTabAlias("bogus"), null);
});

// ── Phase-1: records filter bar param building ─────────────────────────────

test("filter bar builds the server's scope/identity/scope_key/status params", () => {
  assert.deepEqual(buildRecordsQueryParams({}), {});
  assert.deepEqual(buildRecordsQueryParams({ key: "identity:ada" }), { identity: "identity:ada" });
  assert.deepEqual(
    buildRecordsQueryParams({ scope: "identity", key: "identity:ada", status: "active" }),
    { identity: "identity:ada", status: "active" },
  );
  assert.deepEqual(buildRecordsQueryParams({ scope: "identity" }), { scope: "identity" });
  assert.deepEqual(
    buildRecordsQueryParams({ scope: "mob", key: "research" }),
    { scope: "mob", scope_key: "research" },
  );
  assert.deepEqual(buildRecordsQueryParams({ scope: "realm" }), { scope: "realm" });
  assert.deepEqual(
    buildRecordsQueryParams({ status: "quarantined" }, { cursor: "123:m-1", limit: 200 }),
    { status: "quarantined", limit: 200, cursor: "123:m-1" },
  );
  assert.equal(hasActiveFilter({}), false);
  assert.equal(hasActiveFilter({ key: "  " }), false);
  assert.equal(hasActiveFilter({ status: "active" }), true);
});

// ── Phase-1: recall utility mode ───────────────────────────────────────────

test("record utility flags dead weight and approximates bytes spent", () => {
  const dead = recordUtility({
    usage: { injected_count: 5, judged_useful_count: 0, explicit_recall_count: 0 },
    body_bytes: 1024,
  });
  assert.equal(dead.dead, true);
  assert.equal(dead.bytesSpent, 5 * 1024);
  assert.equal(dead.ratio, 0);

  const useful = recordUtility({
    usage: { injected_count: 4, judged_useful_count: 2 },
    body_bytes: 100,
  });
  assert.equal(useful.dead, false);
  assert.equal(useful.ratio, 0.5);

  const fresh = recordUtility({ usage: undefined, body_bytes: 100 });
  assert.equal(fresh.dead, false);
  assert.equal(fresh.ratio, null);
});

test("utility sort leads with dead weight, then ascending usefulness", () => {
  const mk = (id: string, injected: number, useful: number, bytes: number) =>
    record({
      id,
      scope: { scope: "realm", realm: "default" },
      usage: { injected_count: injected, judged_useful_count: useful },
      body_bytes: bytes,
    });
  const sorted = sortRecordsByUtility([
    mk("useful", 4, 4, 10),
    mk("dead-small", 3, 0, 10),
    mk("dead-big", 6, 0, 1000),
    mk("meh", 4, 1, 10),
  ]);
  assert.deepEqual(sorted.map((r) => r.id), ["dead-big", "dead-small", "meh", "useful"]);
  assert.match(utilityLine(sorted[0]), /inj 6 .* useful 0 .* ratio 0\.00/);
  assert.equal(formatBytes(6 * 1000), "5.9KB");
});

// ── Phase-1: lattice audit (invariants a + c) ──────────────────────────────

test("lattice invariants catch LLM-authored records above the ceiling", () => {
  const ok = record({
    id: "ok",
    scope: { scope: "realm", realm: "default" },
    trust: "agent_observed",
    provenance: { author: { author: "distiller", run_id: "d-1" } },
  });
  const violation = record({
    id: "bad",
    scope: { scope: "realm", realm: "default" },
    trust: "agent_verified",
    provenance: { author: { author: "agent", identity: "identity:ada" } },
  });
  const operatorRecord = record({
    id: "op",
    scope: { scope: "realm", realm: "default" },
    trust: "operator",
    provenance: { author: { author: "operator" } },
  });
  const result = latticeInvariants([ok, violation, operatorRecord], { complete: true });
  assert.deepEqual(result.llmCeilingViolations, ["bad"]);
});

test("lattice invariants catch supersede cycles; dangling only when complete", () => {
  const a = record({ id: "a", scope: { scope: "realm", realm: "default" }, supersedes: "b" });
  const b = record({ id: "b", scope: { scope: "realm", realm: "default" }, supersedes: "a" });
  const dangling = record({
    id: "c",
    scope: { scope: "realm", realm: "default" },
    supersedes: "missing",
  });
  const complete = latticeInvariants([a, b, dangling], { complete: true });
  assert.deepEqual(complete.chainViolations.sort(), ["a", "b", "c"]);
  const partial = latticeInvariants([dangling], { complete: false });
  assert.deepEqual(partial.chainViolations, []);
});

test("lattice walk pages with the keyset cursor and honors the cap", async () => {
  const pageA: MemoryPanelRecordsResult = {
    records: [record({ id: "r1", scope: { scope: "realm", realm: "default" } })],
    next_cursor: "cursor-2",
    realms: ["default"],
  };
  const pageB: MemoryPanelRecordsResult = {
    records: [record({ id: "r2", scope: { scope: "realm", realm: "default" } })],
    next_cursor: null,
    realms: ["default"],
  };
  const calls: Array<Record<string, unknown>> = [];
  const walk = await runLatticeWalk(
    async (params) => {
      calls.push(params);
      return params.cursor ? pageB : pageA;
    },
    { singleRealm: true },
  );
  assert.equal(calls.length, 2);
  assert.equal(calls[1].cursor, "cursor-2");
  assert.equal(walk?.checked, 2);
  assert.equal(walk?.complete, true);

  // Access denial propagates as null → the tile renders "no grant".
  const denied = await runLatticeWalk(async () => null, { singleRealm: true });
  assert.equal(denied, null);

  // Multi-realm listings cannot be walked past the first merged page.
  const partialCalls: Array<Record<string, unknown>> = [];
  const partial = await runLatticeWalk(
    async (params) => {
      partialCalls.push(params);
      return pageA;
    },
    { singleRealm: false },
  );
  assert.equal(partialCalls.length, 1);
  assert.equal(partial?.complete, false);
});

// ── Phase-1: verdict tiles ─────────────────────────────────────────────────

const emptyVerdictInputs = {
  records: [] as MemoryPanelRecord[],
  recordsDenied: false,
  dreams: [] as MemoryDreamRun[],
  dreamsDenied: false,
  lattice: null,
};

test("verdict strip mounts six tiles; missing surfaces are named", () => {
  const tiles = computeVerdictTiles(emptyVerdictInputs);
  assert.deepEqual(
    tiles.map((tile) => tile.id),
    ["echo-safety", "taint-wall", "lattice", "recall", "dreams", "store-floor"],
  );
  const byId = new Map(tiles.map((tile) => [tile.id, tile]));
  assert.equal(byId.get("echo-safety")?.status, "unverifiable");
  assert.match(byId.get("echo-safety")?.lines[0] || "", /panel\/injections/);
  assert.equal(byId.get("taint-wall")?.status, "unverifiable");
  assert.match(byId.get("taint-wall")?.lines[0] || "", /panel\/proposals/);
  assert.equal(byId.get("store-floor")?.status, "unverifiable");
  assert.match(byId.get("store-floor")?.lines[0] || "", /panel\/overview/);
  assert.equal(verdictStatusLabel("unverifiable"), "UNVERIFIABLE");
});

test("tiles whose evidence the principal cannot read render no-grant, never green", () => {
  const tiles = computeVerdictTiles({
    ...emptyVerdictInputs,
    recordsDenied: true,
    dreamsDenied: true,
  });
  const byId = new Map(tiles.map((tile) => [tile.id, tile]));
  assert.equal(byId.get("lattice")?.status, "no-grant");
  assert.equal(byId.get("recall")?.status, "no-grant");
  assert.equal(byId.get("dreams")?.status, "no-grant");
  assert.equal(byId.get("taint-wall")?.status, "no-grant");
  assert.equal(verdictStatusLabel("no-grant"), "NO GRANT");
});

test("lattice tile reports violations and the partial-walk header honestly", () => {
  const holding = computeVerdictTiles({
    ...emptyVerdictInputs,
    lattice: { checked: 3, complete: true, llmCeilingViolations: [], chainViolations: [] },
  }).find((tile) => tile.id === "lattice");
  assert.equal(holding?.status, "holding");
  assert.match(holding?.lines[1] || "", /checked 3\/3/);

  const capped = computeVerdictTiles({
    ...emptyVerdictInputs,
    lattice: { checked: 2000, complete: false, llmCeilingViolations: ["x"], chainViolations: [] },
  }).find((tile) => tile.id === "lattice");
  assert.equal(capped?.status, "violated");
  assert.match(capped?.lines[1] || "", /first 2000 — partial/);
});

test("recall and dreams tiles compute from loaded usage and dream recency", () => {
  const now = 4_000_000_000_000;
  const tiles = computeVerdictTiles({
    records: [
      record({
        id: "dead",
        scope: { scope: "realm", realm: "default" },
        usage: { injected_count: 9, judged_useful_count: 0 },
        body_bytes: 100,
      }),
      record({ id: "fine", scope: { scope: "realm", realm: "default" } }),
    ],
    recordsDenied: false,
    dreams: [
      {
        realm: "default",
        run_id: "run-88",
        last_op_at_ms: now - 2 * 60 * 60 * 1000,
        quarantined_ops: 1,
      },
    ],
    dreamsDenied: false,
    lattice: null,
    now,
  });
  const byId = new Map(tiles.map((tile) => [tile.id, tile]));
  assert.equal(byId.get("recall")?.status, "degraded");
  assert.match(byId.get("recall")?.lines[0] || "", /1 dead weight of 2/);
  assert.equal(byId.get("dreams")?.status, "holding");
  assert.match(byId.get("dreams")?.lines[0] || "", /last run 2h ago/);
  assert.match(byId.get("dreams")?.lines[1] || "", /1 quarantined/);
});

// ── Phase-1: holdings scope overview ───────────────────────────────────────

test("scope overview rows count statuses, bytes, and trust mix per scope", () => {
  const rows = scopeOverviewRows([
    record({
      id: "a1",
      scope: { scope: "identity", realm: "default", identity: "identity:ada" },
      status: { status: "active" },
      body_bytes: 100,
    }),
    record({
      id: "a2",
      scope: { scope: "identity", realm: "default", identity: "identity:ada" },
      status: { status: "quarantined", reason: "taint" },
      trust: "untrusted",
      body_bytes: 50,
    }),
    record({ id: "r1", scope: { scope: "realm", realm: "default" } }),
  ]);
  assert.equal(rows.length, 2);
  assert.equal(rows[0].label, "identity:ada");
  assert.equal(rows[0].active, 1);
  assert.equal(rows[0].quarantined, 1);
  assert.equal(rows[0].bytes, 150);
  assert.match(rows[0].trustMix, /1 observed/);
  assert.match(rows[0].trustMix, /1 untrusted/);
});

test("scope rows pivot into the records filter that reproduces them", () => {
  assert.deepEqual(
    filterForScope({ scope: "identity", realm: "default", identity: "identity:ada" }),
    { scope: "identity", key: "identity:ada" },
  );
  assert.deepEqual(
    filterForScope({ scope: "mob", realm: "default", mob: "research" }),
    { scope: "mob", key: "research" },
  );
  assert.deepEqual(filterForScope({ scope: "realm", realm: "default" }), { scope: "realm" });
});

// ── Phase-1: biography sections ────────────────────────────────────────────

test("lineage lane renders newest-first with the current record marked", () => {
  const chain = [
    record({ id: "oldest", scope: { scope: "realm", realm: "default" } }),
    record({ id: "middle", scope: { scope: "realm", realm: "default" } }),
    record({ id: "current", scope: { scope: "realm", realm: "default" } }),
  ];
  const lane = lineageLane(chain, "current");
  assert.deepEqual(lane.map((entry) => entry.record.id), ["current", "middle", "oldest"]);
  assert.deepEqual(lane.map((entry) => entry.current), [true, false, false]);
});

test("biography dreams section joins through the lossy memory_ids sample", () => {
  const dreams: MemoryDreamRun[] = [
    { realm: "default", run_id: "run-1", memory_ids: ["m-1", "m-2"] },
    { realm: "default", run_id: "run-2", memory_ids: ["m-3"] },
    { realm: "default", run_id: "run-3" },
  ];
  assert.deepEqual(dreamRunsTouching(dreams, "m-1").map((run) => run.run_id), ["run-1"]);
  assert.deepEqual(dreamRunsTouching(dreams, "m-9"), []);
});

test("evidence excerpts slice the message window and skip non-text entries", () => {
  const entries = [
    { kind: "message", id: "e0", identity: { id: "u", label: "You", role: "user" }, text: "zero" },
    { kind: "message", id: "e1", identity: { id: "a", label: "Ada", role: "assistant" }, text: "one" },
    { kind: "message", id: "e2", identity: { id: "a", label: "Ada", role: "assistant" }, text: "" },
    { kind: "message", id: "e3", identity: { id: "u", label: "You", role: "user" }, text: "three" },
  ] as never[];
  const all = evidenceExcerptLines(entries);
  assert.deepEqual(all.map((line) => line.text), ["zero", "one", "three"]);
  const windowed = evidenceExcerptLines(entries, [1, 3]);
  assert.deepEqual(windowed.map((line) => line.text), ["one", "three"]);
  assert.equal(windowed[0].speaker, "Ada");
});

// ── Phase-1: knowledge lens composition ────────────────────────────────────

test("knowledge lens lists identities and composes the scope union", () => {
  const records = [
    record({ id: "a", scope: { scope: "identity", realm: "default", identity: "identity:ada" } }),
    record({ id: "b", scope: { scope: "identity", realm: "default", identity: "identity:bob" } }),
    record({ id: "m", scope: { scope: "mob", realm: "default", mob: "research" } }),
    record({ id: "r", scope: { scope: "realm", realm: "default" } }),
  ];
  assert.deepEqual(identityOptions(records), ["identity:ada", "identity:bob"]);
  const segments = knowledgeComposition(records, "identity:ada");
  assert.equal(segments[0].count, 1);
  assert.equal(segments[0].approximate, false);
  assert.equal(segments[1].count, 1); // all mob-scope rows, flagged approximate
  assert.equal(segments[1].approximate, true);
  assert.equal(segments[3].count, 1);
});

// ── Phase-1: live strip + pivots ───────────────────────────────────────────

function frame(overrides: Partial<ConsoleFrame> & Pick<ConsoleFrame, "id" | "event">): ConsoleFrame {
  return { data: {}, ...overrides } as ConsoleFrame;
}

test("live strip dedupes snapshot replays by frame id and counts frames behind", () => {
  const a = frame({ id: "f-1", event: "memory.dream.completed" });
  const replay = frame({ id: "f-1", event: "memory.dream.completed" });
  const b = frame({ id: "f-2", event: "memory.record.promoted" });
  const deduped = dedupeFramesById([a, replay, b]);
  assert.deepEqual(deduped.map((f) => f.id), ["f-1", "f-2"]);
  assert.equal(countFramesBehind(deduped, [a]), 1);
  assert.equal(countFramesBehind(deduped, deduped), 0);
});

test("memory frames pivot to the record their payload names", () => {
  assert.deepEqual(
    memoryFramePivot(
      frame({
        id: "f-1",
        event: "memory.record.promoted",
        data: { record_id: "m-7", realm: "homecore" },
      }),
    ),
    { recordId: "m-7", realm: "homecore" },
  );
  assert.equal(
    memoryFramePivot(frame({ id: "f-2", event: "memory.dream.started", data: {} })),
    null,
  );
  assert.equal(
    memoryFramePivot(frame({ id: "f-3", event: "interaction_complete", data: { record_id: "x" } })),
    null,
  );
});

// ── Fix round: e2e-surfaced defects (regression net) ──────────────────────

function pagerHarness() {
  const state: {
    paged: { records: MemoryPanelRecord[]; nextCursor: string | null } | null;
    loading: boolean;
    queries: Array<Record<string, unknown>>;
  } = { paged: null, loading: false, queries: [] };
  const pending: Array<(result: MemoryPanelRecordsResult | null) => void> = [];
  const pager = createMemoryRecordsPager({
    query: (params) => {
      state.queries.push(params);
      return new Promise((resolve) => pending.push(resolve));
    },
    setPaged: (paged) => {
      state.paged = paged;
    },
    setLoading: (loading) => {
      state.loading = loading;
    },
  });
  const page = (ids: string[], nextCursor: string | null): MemoryPanelRecordsResult => ({
    records: ids.map((id) => record({ id, scope: { scope: "realm", realm: "default" } })),
    next_cursor: nextCursor,
    realms: ["default"],
  });
  return { state, pending, pager, page };
}

test("bug 1: a stale filter response never clobbers a newer query (issue-order wins)", async () => {
  const { state, pending, pager, page } = pagerHarness();

  const broad = pager.applyFilter({ scope: "mob" }); // resolves LAST
  const narrow = pager.applyFilter({ scope: "mob", key: "research" });
  assert.equal(pending.length, 2);

  // Newer (narrow) resolves first and applies…
  pending[1](page(["narrow-1"], null));
  await narrow;
  assert.deepEqual(state.paged?.records.map((r) => r.id), ["narrow-1"]);

  // …then the older broad response arrives and must be dropped.
  pending[0](page(["broad-1", "broad-2"], "cursor-x"));
  await broad;
  assert.deepEqual(state.paged?.records.map((r) => r.id), ["narrow-1"]);
  assert.equal(state.paged?.nextCursor, null);
  assert.equal(state.loading, false);
});

test("bug 1: clearing the filter invalidates an in-flight fetch", async () => {
  const { state, pending, pager, page } = pagerHarness();

  const filtered = pager.applyFilter({ status: "active" });
  await pager.applyFilter({}); // clear — synchronous reset, bumps the sequence
  assert.equal(state.paged, null);

  pending[0](page(["stale-1"], null));
  await filtered;
  assert.equal(state.paged, null, "stale filtered page must not resurrect after clear");
  assert.equal(state.loading, false);
});

test("bug 2: blur re-apply is a no-op unless the filter value actually changed", async () => {
  const { state, pending, pager, page } = pagerHarness();

  await Promise.all([
    pager.applyFilter({ scope: "mob", key: "research" }),
    (async () => {
      // resolve the initial apply
      while (pending.length === 0) await Promise.resolve();
      pending[0](page(["m-1"], "cursor-1"));
    })(),
  ]);
  assert.equal(state.queries.length, 1);

  // Unchanged (incl. whitespace-only difference) → no new query.
  await pager.applyFilterIfChanged({ scope: "mob", key: "research" });
  await pager.applyFilterIfChanged({ scope: "mob", key: " research " });
  assert.equal(state.queries.length, 1);

  // Real change → re-queries.
  const changed = pager.applyFilterIfChanged({ scope: "mob", key: "ops" });
  assert.equal(state.queries.length, 2);
  pending[1](page(["m-2"], null));
  await changed;
  assert.deepEqual(state.paged?.records.map((r) => r.id), ["m-2"]);

  assert.equal(filtersEquivalent({}, { key: "  " }), true);
  assert.equal(filtersEquivalent({ scope: "mob" }, { scope: "mob", key: "x" }), false);
});

test("bug 2: an unchanged blur during load-more cannot reset the appended page", async () => {
  const { state, pending, pager, page } = pagerHarness();
  const base = [record({ id: "base-1", scope: { scope: "realm", realm: "default" } })];

  const more = pager.loadMore({
    filter: {},
    paged: null,
    baseRecords: base,
    baseCursor: "cursor-1",
  });
  // Blur fired by the load-more click: filter is unchanged ({}), so this
  // must not issue a query or bump the sequence.
  await pager.applyFilterIfChanged({});
  assert.equal(state.queries.length, 1);

  pending[0](page(["more-1"], null));
  await more;
  assert.deepEqual(state.paged?.records.map((r) => r.id), ["base-1", "more-1"]);
});

test("bug 3: unfiltered load-more rows render — grouped view reads the accumulated page", () => {
  const base = [
    record({ id: "a1", scope: { scope: "identity", realm: "default", identity: "identity:ada" } }),
  ];
  const appended = [
    ...base,
    record({ id: "a2", scope: { scope: "identity", realm: "default", identity: "identity:ada" } }),
  ];

  const view = buildRecordsListView({
    records: base,
    paged: { records: appended, nextCursor: "cursor-2" },
    baseCursor: null,
    filter: {},
    sortMode: "recency",
  });
  assert.equal(view.mode, "grouped");
  assert.deepEqual(
    view.groups.flatMap((group) => group.records.map((r) => r.id)),
    ["a1", "a2"],
  );
  assert.equal(view.cursor, "cursor-2");

  // Without paged state the base page + cursor drive the view.
  const baseView = buildRecordsListView({
    records: base,
    paged: null,
    baseCursor: "cursor-1",
    filter: {},
    sortMode: "recency",
  });
  assert.equal(baseView.mode, "grouped");
  assert.equal(baseView.cursor, "cursor-1");

  // Filter or utility sort flips to the flat list over the same source.
  const flat = buildRecordsListView({
    records: base,
    paged: { records: appended, nextCursor: null },
    baseCursor: null,
    filter: { status: "active" },
    sortMode: "recency",
  });
  assert.equal(flat.mode, "flat");
  assert.deepEqual(flat.records.map((r) => r.id), ["a1", "a2"]);
});
