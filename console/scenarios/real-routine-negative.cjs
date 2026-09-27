"use strict";
const assert = require("node:assert/strict");
const fs = require("node:fs/promises");
const path = require("node:path");
const os = require("node:os");
const { randomUUID } = require("node:crypto");
const { chromium } = require("playwright");
const { startFixture, eventually, rpc, snapshot } = require("../acceptance-runtime.cjs");
const { browserFailureMonitor, navigation } = require("./browser-failure-monitor.cjs");
const evidence = process.env.MOBKIT_BROWSER_EVIDENCE || path.join(__dirname, "../../output/playwright/console-acceptance");
const actions = ["cancel_after_boundary", "interrupt"];
function stoppedTools(scenarioId) { return require("./real-routine-tools.cjs").expectedTools(scenarioId).slice(0, 4); }
function textContent(value) { return typeof value === "string" ? value : (value || []).filter(block => block.type === "text").map(block => block.text).join(""); }
function unknown(spec, tool) { return spec.action === "interrupt" && tool.step === "late"; }
function assertFrameOwner(frame, spec, requireSession = false) {
  assert.equal(frame.source?.kind, "console_event", "original runtime source");
  assert.equal(frame.source_event_id, frame.id, "original source event ID");
  assert.equal(frame.identity, "router:main", "canonical member");
  assert.equal(frame.interaction_id, spec.accepted.interaction_id, "canonical interaction");
  assert.equal(frame.run_id, spec.runId, "canonical run");
  if (requireSession || frame.session_id != null) assert.equal(frame.session_id, spec.accepted.session_id, "canonical session");
}
function isCancelled(frame, spec) {
  return frame.source?.kind === "console_event" && frame.kind === "interaction_failed"
    && frame.interaction_id === spec.accepted.interaction_id
    && (frame.payload?.reason?.kind === "cancelled" || frame.payload?.error_report?.class === "cancelled");
}
function assertStoppedFrames(frames, spec) {
  assert(actions.includes(spec.action));
  const start = frames.filter(frame => frame.source?.kind === "console_event" && frame.kind === "run_started" && frame.interaction_id === spec.accepted.interaction_id);
  assert.equal(start.length, 1, "one canonical start"); assertFrameOwner(start[0], spec, true);
  const terminal = frames.filter(frame => isCancelled(frame, spec));
  assert(terminal.length >= 1, "typed cancellation terminal, never cancelled prose");
  assert.equal(new Set(terminal.map(frame => frame.id)).size, terminal.length, "distinct original terminal event IDs");
  for (const frame of terminal) assertFrameOwner(frame, spec);
  const expected = stoppedTools(spec.scenarioId), ids = new Set(expected.map(tool => tool.id));
  const owned = frames.filter(frame => frame.source?.kind === "console_event" && ids.has(frame.payload?.tool_call_id));
  assert.equal(new Set(owned.map(frame => frame.id)).size, owned.length, "distinct original tool event IDs");
  assert(!frames.some(frame => frame.payload?.tool_call_id === `fixture-${spec.scenarioId}-ready`), "no later tool after the stopped boundary");
  assert(!frames.some(frame => frame.kind === "interaction_complete" && frame.interaction_id === spec.accepted.interaction_id), "stopped interaction never completes successfully");
  const tools = expected.map(tool => {
    const calls = owned.filter(frame => frame.kind === "tool_call_requested" && frame.payload.tool_call_id === tool.id);
    const results = owned.filter(frame => ["tool_result_received", "tool_execution_completed"].includes(frame.kind) && frame.payload.tool_call_id === tool.id);
    assert.equal(calls.length, 1, `${tool.step}: one call`); assertFrameOwner(calls[0], spec);
    assert.equal(calls[0].payload.name, tool.name); assert.deepEqual(calls[0].payload.args, tool.args, "exact arguments");
    if (unknown(spec, tool)) {
      assert.equal(results.length, 0, "interrupted dispatch has no authoritative result");
    } else {
      assert.equal(results.length, 1, `${tool.step}: one actual result`); assertFrameOwner(results[0], spec);
      assert.equal(textContent(results[0].payload.content ?? results[0].payload.result), tool.result, "exact result bytes");
      assert.equal(results[0].payload.is_error, tool.error, "actual result outcome");
      assert(frames.indexOf(results[0]) > frames.indexOf(calls[0]), "result follows its call");
      assert(frames.indexOf(results[0]) < frames.indexOf(terminal[0]), "result precedes the owner terminal");
    }
    return { id: tool.id, callId: calls[0].id, resultId: results[0]?.id ?? null, outcome: unknown(spec, tool) ? "unknown" : tool.error ? "error" : "success" };
  });
  return { startId: start[0].id, terminalIds: terminal.map(frame => frame.id), tools };
}
function assertProofRetained(previous, next) {
  assert.equal(next.startId, previous.startId, "exact original start identity");
  assert.deepEqual(next.tools, previous.tools, "exact tool source identities and outcomes");
  for (const id of previous.terminalIds) assert(next.terminalIds.includes(id), "original typed terminal identity retained");
}
function assertStoppedHistory(page, spec) {
  assert.equal(page.session_id, spec.accepted.session_id, "actual owner session");
  assert.equal(page.offset, 0, "complete owner history"); assert.equal(page.has_more, false, "complete owner history");
  assert.equal(page.messages.length, page.message_count, "complete owner history");
  const calls = page.messages.flatMap((message, index) => message.role === "block_assistant"
    ? message.blocks.filter(block => block.block_type === "tool_use").map(block => ({ index, identity: message.identity, call: block.data })) : []);
  const results = page.messages.flatMap((message, index) => message.role === "tool_results"
    ? message.results.map(result => ({ index, result })) : []);
  assert(!calls.some(item => item.call.id === `fixture-${spec.scenarioId}-ready`), "no later tool in durable history");
  return stoppedTools(spec.scenarioId).map(tool => {
    const ownCalls = calls.filter(item => item.call.id === tool.id), ownResults = results.filter(item => item.result.tool_use_id === tool.id);
    assert.equal(ownCalls.length, 1, `history ${tool.step}: one call`);
    const call = ownCalls[0]; assert.equal(call.identity?.interaction_id, spec.accepted.interaction_id, "canonical interaction");
    assert.equal(call.identity?.run_id, spec.runId, "canonical run");
    assert.equal(call.call.name, tool.name); assert.deepEqual(call.call.args, tool.args, "exact arguments");
    if (unknown(spec, tool)) assert.equal(ownResults.length, 0, "durable interrupted dispatch has no authoritative result");
    else {
      assert.equal(ownResults.length, 1, `history ${tool.step}: one actual result`);
      const result = ownResults[0]; assert(result.index > call.index, "result follows its own call");
      assert.equal(textContent(result.result.content), tool.result, "exact result bytes"); assert.equal(result.result.is_error, tool.error);
    }
    return { id: tool.id, callIndex: call.index, resultIndex: ownResults[0]?.index ?? null,
      outcome: unknown(spec, tool) ? "unknown" : tool.error ? "error" : "success" };
  });
}

async function stoppedScenario(host, action) {
  assert(process.env.MOBKIT_EXAMPLE_BIN_DIR, "Use the coordinator's prebuilt fixture; this scenario must not invoke Cargo.");
  const stateDir = await fs.mkdtemp(path.join(os.tmpdir(), `mobkit-routine-${action}-`));
  const result = { host: host || "api", action, scenarioId: `routine-${action}-${randomUUID().slice(0, 8)}`, errors: [] };
  let fixture, browser, page, live, monitor;
  const viewport = () => page.locator(host === "stock" ? ".conv__body" : '[data-testid="shared-pane-0"] .cc-conversation-pane__scroll').first();
  const read = async url => { const response = await fetch(url); assert.equal(response.status, 200, await response.clone().text()); return response.json(); };
  const timeline = async () => (await read(`${fixture.baseUrl}/console/timeline?identity=router%3Amain&mode=recent&limit=1000`)).frames;
  const history = () => read(`${fixture.backendUrl}/__fixture/session-history?session_id=${encodeURIComponent(result.accepted.session_id)}`);
  const runControl = (next, runId = result.runId) => fixture.control("routine-run", { identity: "router:main", session_id: result.accepted.session_id, run_id: runId, action: next });
  const shot = async label => { await fs.mkdir(evidence, { recursive: true }); await page.screenshot({ path: path.join(evidence, `${host}-real-routine-${action}-${label}.png`), fullPage: true }); };
  async function open(reload = false) {
    await navigation(page, `${reload ? "reload" : "open"} ${host} routine ${action} host`, async () => {
      if (reload) await page.reload(); else await page.goto(fixture.baseUrl + (host === "stock" ? "/console" : "/shared"));
      if (host === "stock" && !await page.locator(".conv__body").count()) await page.locator('.agent[role="button"], .cc-sidebar-row').filter({ hasText: /router/i }).first().click();
      await page.locator('[data-testid="console-transport-status"][data-phase="live"]').waitFor(); await viewport().waitFor();
    });
  }
  async function inspectTool(label, state) {
    const noResult = state !== "success";
    const late = stoppedTools(result.scenarioId).at(-1);
    const section = viewport().locator("section.cc-tool-call").filter({ hasText: "late-review.txt" }).last();
    await section.waitFor();
    assert.equal(await section.evaluate(node => Boolean(node.closest(".cc-completed-tools"))), false, "held/stopped call remains outside successful folding");
    assert.equal(await section.locator(".cc-tool-call__header").first().getAttribute("aria-expanded"), "true", "unresolved or failed group stays disclosed");
    const detail = section.locator(".cc-tool-call__sub").filter({ hasText: '"path": "late-review.txt"' });
    const body = await detail.count() ? detail : section;
    const input = body.locator(".cc-tool-call__section").filter({ has: page.getByText("Input", { exact: true }) });
    assert.deepEqual(JSON.parse(await input.locator("pre").textContent()), late.args, "exact held call arguments");
    const output = body.locator(".cc-tool-call__section").filter({ has: page.getByText("Result", { exact: true }) });
    if (noResult) {
      assert.equal(await output.count(), 0, `${state} completion has no result body`);
      if (await detail.count()) assert(await detail.locator(".cc-tool-call__peer-status--pending").count(), "exact held child has no success status");
      else {
        assert(await section.evaluate(node => node.classList.contains("cc-tool-call--pending")), "exact incomplete call has no success status");
        if (state === "unknown") assert.match(await section.locator(".cc-tool-call__status").textContent(), /Completion unknown/);
      }
    } else assert.equal(await output.locator("pre").textContent(), late.result, "boundary cancellation retains real completed result");
    await section.getByRole("button", { name: "Copy", exact: true }).click();
    const copied = await page.evaluate(() => navigator.clipboard.readText());
    const expectedLate = `$ ${late.name}\nInput: ${JSON.stringify(late.args)}${noResult ? "" : `\nResult: ${late.result}`}`;
    const grouped = await detail.count() > 0;
    const missing = stoppedTools(result.scenarioId).find(tool => tool.step === "missing");
    const expectedCopy = grouped ? `$ ${missing.name}\nInput: ${JSON.stringify(missing.args)}\nResult: ${missing.result}\n${expectedLate}` : expectedLate;
    assert.equal(copied, expectedCopy, "clipboard preserves each exact source, with no invented cancellation/result text");
    assert.equal(await viewport().locator("details.cc-completed-tools").count(), 1, "only the actual initial successes fold");
    await shot(label); return { label, state, grouped, copied };
  }
  try {
    fixture = await startFixture({ mode: "identity", routineTools: true, stateDir });
    if (host) {
      browser = await chromium.launch({ headless: true });
      const context = await browser.newContext({ viewport: { width: 1600, height: 1000 }, permissions: ["clipboard-read", "clipboard-write"] });
      monitor = browserFailureMonitor(context, { origin: fixture.baseUrl });
      result.errors = monitor.errors; result.expectedFailures = monitor.expected; result.requestFailures = monitor.failures;
      page = await context.newPage();
      page.on("response", response => { if (response.status() >= 400) result.errors.push({ url: response.url(), status: response.status() }); });
      await open();
    }
    const baseline = await read(`${fixture.baseUrl}/console/timeline?identity=router%3Amain&mode=recent&limit=1000`);
    live = await require("./real-routine-tools.cjs").capture(`${fixture.backendUrl}/console/timeline/stream?identity=router%3Amain&after=${encodeURIComponent(baseline.latest_cursor)}`);
    await eventually(() => { live.check(); return live.events.some(event => event.type === "snapshot_complete"); }, "subscriber connected before real tool dispatch");
    await fixture.control("model", { source: "Acknowledged.", delay_ms: 0, chunk_chars: 256, scenario: { kind: "routine", run_id: result.scenarioId } });
    const sent = await rpc(fixture.baseUrl, "mobkit/console/send", { identity: "router:main", content: `[fixture:${result.scenarioId}] Inspect the release files, then hold the delayed review for cancellation.`,
      origin: "console:routine-negative", origin_kind: "operator", handling_mode: "queue", idempotency_key: randomUUID() });
    assert.equal(sent.status, 200); assert(sent.body.result?.interaction_id, JSON.stringify(sent.body)); result.accepted = sent.body.result;
    await eventually(async () => (await fixture.control("routine-tools", { action: "status" })).entered, "actual file dispatcher is held");
    const started = await eventually(() => live.frames().find(frame => frame.kind === "run_started" && frame.interaction_id === result.accepted.interaction_id), "exact accepted run started");
    result.runId = started.run_id; assert(result.runId && result.accepted.session_id);
    const lateId = stoppedTools(result.scenarioId).at(-1).id;
    await eventually(() => live.frames().some(frame => frame.kind === "tool_call_requested" && frame.payload.tool_call_id === lateId), "actual held call reached SSE");
    assert(!live.frames().some(frame => frame.kind === "tool_result_received" && frame.payload.tool_call_id === lateId));
    result.before = await runControl("status"); assert.equal(result.before.current_run_id, result.runId);
    result.stale = await runControl(action, randomUUID()); assert.equal(result.stale.accepted, false, "stale exact-run control cannot interrupt current work");
    assert.equal(result.stale.current_run_id, result.runId);
    if (host) result.pendingView = await inspectTool("held", "held");
    result.control = await runControl(action); assert.equal(result.control.accepted, true, "actual owner admitted exact-run control");
    if (action === "cancel_after_boundary") {
      result.waiting = await fixture.control("routine-tools", { action: "status" });
      assert.equal(result.waiting.completed, false); assert.equal(result.waiting.dropped, false);
      assert.equal((await runControl("status")).current_run_id, result.runId, "cooperative cancel waits for the held tool boundary");
      assert(!live.frames().some(frame => isCancelled(frame, result)), "cancel admission is not terminality");
      await fixture.control("routine-tools", { action: "release" });
    }
    await eventually(() => { live.check(); return live.frames().some(frame => isCancelled(frame, result)); }, "actual typed interaction cancellation reaches live SSE", 30_000);
    result.settled = await eventually(async () => { const current = await runControl("status"); return current.current_run_id === null ? current : null; }, "owner unbinds the stopped run");
    result.dispatch = await fixture.control("routine-tools", { action: "status" });
    assert.equal(result.dispatch.completed, action === "cancel_after_boundary");
    assert.equal(result.dispatch.dropped, action === "interrupt");
    result.live = assertStoppedFrames(live.frames(), result);
    // Releasing the old wait handle after interruption must not resurrect work.
    if (action === "interrupt") await fixture.control("routine-tools", { action: "release" });
    result.ownerHistory = await eventually(async () => { const page = await history(); assertStoppedHistory(page, result); return page; }, "complete durable owner history retains the exact stopped call and true result absence");
    result.history = assertStoppedHistory(result.ownerHistory, result);
    result.ownerFrames = await timeline(); result.ownerProof = assertStoppedFrames(result.ownerFrames, result);
    assertProofRetained(result.live, result.ownerProof);
    result.replayFrames = (await snapshot(fixture.backendUrl, `?identity=router%3Amain&after=${encodeURIComponent(baseline.latest_cursor)}`)).flatMap(item => item.data.frame ? [item.data.frame] : []);
    result.replayProof = assertStoppedFrames(result.replayFrames, result); assertProofRetained(result.ownerProof, result.replayProof);
    result.postTerminalRetry = await runControl(action); assert.equal(result.postTerminalRetry.accepted, false, "terminal exact-run control is a no-op");
    if (host) {
      const state = action === "interrupt" ? "unknown" : "success";
      result.stoppedView = await inspectTool("stopped", state);
      await open(true); result.reloadedView = await inspectTool("reloaded", state);
      assert.deepEqual(assertStoppedHistory(await history(), result), result.history, "durable owner proof survives browser reload");
      result.reloadProof = assertStoppedFrames(await timeline(), result); assertProofRetained(result.replayProof, result.reloadProof);
      monitor.assertClean();
    }
  } catch (error) {
    result.failure = error.stack || String(error); if (page) await shot("failure").catch(() => {}); throw error;
  } finally {
    try {
      result.stream = live?.events; result.observations = fixture?.observations; result.logs = fixture?.logs();
      await fs.mkdir(evidence, { recursive: true }); await fs.writeFile(path.join(evidence, `${host || "api"}-real-routine-${action}.json`), JSON.stringify(result, null, 2));
      if (!result.failure) monitor?.assertClean();
    } finally {
      monitor?.stop();
      const cleanup = await Promise.allSettled([live?.close(), browser?.close(), fixture?.close()]);
      await fs.rm(stateDir, { recursive: true, force: true });
      const rejected = cleanup.find(item => item.status === "rejected");
      if (rejected && !result.failure) throw rejected.reason;
    }
  }
}
const apiScenarios = actions.map(action => ({ id: `api-routine-${action.replaceAll("_", "-")}`, family: "routine-tools", backend: "real", run: () => stoppedScenario(null, action) }));
const browserScenarios = ["stock", "shared"].flatMap(host => actions.map(action => ({ id: `real-${host}-routine-${action.replaceAll("_", "-")}`, family: "real-presentation", backend: "real", run: () => stoppedScenario(host, action) })));
module.exports = { apiScenarios, browserScenarios, assertStoppedFrames, assertStoppedHistory, assertProofRetained, stoppedTools };
