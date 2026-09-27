const assert = require("node:assert/strict");
const { test } = require("node:test");
const { selectScenarios } = require("./scenario-registry.cjs");
const path = require("node:path");
const { execFileSync } = require("node:child_process");
const scenarios = [
  { id: "runtime", family: "runtime", backend: "real" },
  { id: "scroll", family: "presentation", backend: "mock" },
  { id: "markdown", family: "presentation", backend: "mock" },
  { id: "recovery", family: "runtime", backend: "real" },
];
test("complete deterministic shards cover every scenario exactly once including real backends", () => {
  const a = selectScenarios(scenarios, ["--shard=1/2"]).selected;
  const b = selectScenarios(scenarios, ["--shard=2/2"]).selected;
  assert.equal(new Set([...a, ...b].map((s) => s.id)).size, scenarios.length);
  assert.deepEqual(a.map((s) => s.id), ["runtime", "markdown"]);
  assert.deepEqual(b.map((s) => s.id), ["scroll", "recovery"]);
  assert.deepEqual(selectScenarios(scenarios, []).selected, scenarios);
});
test("typos, empty selections, duplicate IDs and invalid shards cannot produce green empty runs", () => {
  for (const args of [["--scenario=missing"], ["--family=absent"], ["--shard=3/2"], ["--shard=5/6"], ["--scenario=scroll", "--family=runtime"], ["--typo"]]) {
    assert.throws(() => selectScenarios(scenarios, args));
  }
  assert.throws(() => selectScenarios([...scenarios, scenarios[0]], []));
});

test("shipping runners discover every required real backend scenario and shard the complete set", () => {
  const browser = require("./browser-e2e.cjs").scenarios;
  const api = JSON.parse(execFileSync(process.execPath, [path.join(__dirname, "api-e2e.cjs"), "--list"], { encoding: "utf8" }));
  const browserModules = [
    require("./scenarios/real-conversation.cjs").scenarios,
    require("./scenarios/real-markdown-url-policy.cjs").scenarios,
    require("./scenarios/real-reasoning.cjs").scenarios,
    require("./scenarios/real-startup-lineage.cjs").scenarios,
    require("./scenarios/real-routine-tools.cjs").browserScenarios,
    require("./scenarios/approval-lifecycle.cjs").browserScenarios,
    require("./scenarios/real-workgraph.cjs").browserScenarios,
    require("./scenarios/real-images.cjs").browserScenarios,
    require("./scenarios/real-send-context.cjs").scenarios,
    require("./scenarios/real-durable-steer.cjs").scenarios,
    require("./scenarios/real-tab-isolation.cjs").scenarios,
    require("./scenarios/real-legacy-import.cjs").scenarios,
    require("./scenarios/real-sidebar-activity.cjs").scenarios,
  ];
  const requiredBrowser = browserModules.flat();
  assert(requiredBrowser.length >= 25, "real backend acceptance families must remain registered");
  for (const scenario of requiredBrowser) {
    assert.equal(browser.filter(item => item.id === scenario.id && item.backend === "real").length, 1, scenario.id);
  }
  for (const id of ["api-query-faults", "api-member-ingress", "api-identity-ingress", "api-member-send-restart", "api-identity-send-restart",
    ...require("./scenarios/approval-lifecycle.cjs").apiScenarios.map(item => item.id),
    ...require("./scenarios/real-workgraph.cjs").apiScenarios.map(item => item.id),
    ...require("./scenarios/real-routine-tools.cjs").apiScenarios.map(item => item.id),
    ...require("./scenarios/stream-parity.cjs").apiScenarios.map(item => item.id),
    ...require("./scenarios/real-correlation-overlap.cjs").apiScenarios.map(item => item.id)]) {
    assert.equal(api.filter(item => item.id === id && item.backend === "real").length, 1, id);
  }
  for (const runner of [browser, api]) {
    const shards = [1, 2, 3].flatMap(index => selectScenarios(runner, [`--shard=${index}/3`]).selected);
    assert.deepEqual(shards.map(item => item.id).sort(), runner.map(item => item.id).sort());
  }
});

test("default browser coverage includes activity filters against real runtime phases", () => {
  const runner = require("./browser-e2e.cjs").scenarios;
  const matches = selectScenarios(runner, []).selected.filter(item => item.id === "real-stock-sidebar-activity");
  assert.equal(matches.length, 1);
  assert.equal(matches[0].backend, "real");
  assert.equal(typeof matches[0].run, "function");
});
