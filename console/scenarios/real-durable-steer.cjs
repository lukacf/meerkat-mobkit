"use strict";

const assert = require("node:assert/strict");
const fs = require("node:fs/promises");
const path = require("node:path");
const { randomUUID } = require("node:crypto");
const { chromium } = require("playwright");
const { startFixture, eventually, rpc } = require("../acceptance-runtime.cjs");

const identity = "router:main";
const evidenceDir = process.env.MOBKIT_BROWSER_EVIDENCE || path.join(__dirname, "../../output/playwright/console-acceptance");
const seedSource = "# Review context\n\n" + Array.from({ length: 18 }, (_, index) =>
  `### Evidence ${index + 1}\n\nThe release review includes the workgraph, agent communications and uploaded images. Keep this evidence available while a background review completes.\n\n`).join("");
const notice = "Background review complete: preserve A\u030A, \u00e5 and \ud83d\ude80 exactly; check the WorkGraph prerequisites before publishing.";
const finalSource = "## Background review incorporated\n\nThe current run checked the real WorkGraph ready set and received the durable review notice before its next model request. The instruction remains part of this conversation.\n\n| Check | Result |\n| --- | --- |\n| WorkGraph prerequisites | Reviewed through the runtime tool |\n| Background instruction | Kept once in the current run |\n| Operator draft | Preserved while the review completed |\n\nThe next step is to finish the release review. No publication was requested by this acceptance scenario.";
const draft = "My next instruction is still a draft. Keep A\u030A and \ud83d\ude80 intact.";
const textContent = content => typeof content === "string" ? content : (content || []).filter(block => block.type === "text").map(block => block.text).join("");
const assistantText = message => (message.blocks || []).filter(block => block.block_type === "text").map(block => block.data?.text ?? block.text ?? "").join("\n\n");
const liveEvent = frame => frame.source?.kind === "console_event";

function expectedNavigationCancellation(request) {
  if (!request.failure()?.errorText?.includes("ERR_ABORTED")) return false;
  if (new URL(request.url()).pathname.endsWith("/timeline/stream")) return true;
  try {
    const body = JSON.parse(request.postData() || "{}");
    return body.method === "mobkit/console/query_timeline" && body.params?.mode === "recent"
      && body.params?.limit === 200 && !body.params?.identity;
  } catch { return false; }
}

async function durableSteer(host) {
  assert(process.env.MOBKIT_EXAMPLE_BIN_DIR, "Use the coordinator's prebuilt durable-steer fixture; never invoke Cargo from this browser case.");
  const fixture = await startFixture();
  const browser = await chromium.launch({ headless: process.env.MOBKIT_HEADED !== "1" });
  const page = await browser.newPage({ viewport: { width: 1600, height: 1000 } });
  page.setDefaultTimeout(20_000);
  const result = { host, notice, finalSource, draft, errors: [], expectedCancellations: [], views: {} };
  const barrierId = `durable-${host}-${randomUUID().slice(0, 8)}`;
  const instruction = `Review this release while the background check runs. Exact turn marker ${barrierId}.`;
  let allowance = "navigation";
  let armed = false;
  const pane = () => host === "shared" ? page.getByTestId("shared-pane-0") : page.getByTestId(`chat-pane:${identity}`).first();
  const viewport = () => pane().locator(host === "shared" ? ".cc-conversation-pane__scroll" : ".conv__body");
  const composer = () => host === "shared" ? pane().getByRole("textbox", { name: "Message", exact: true }) : pane().getByTestId(`chat-composer:${identity}`);
  page.on("pageerror", error => result.errors.push(error.message));
  page.on("requestfailed", request => {
    const detail = { url: request.url(), error: request.failure()?.errorText };
    if ((allowance === "navigation" && expectedNavigationCancellation(request))
      || (allowance === "disconnect" && new URL(request.url()).pathname.endsWith("/timeline/stream"))) {
      result.expectedCancellations.push({ ...detail, reason: allowance });
    } else result.errors.push(detail);
  });
  page.on("response", response => {
    if (response.status() >= 400) result.errors.push({ url: response.url(), status: response.status() });
  });
  async function read(url) {
    const response = await fetch(url);
    assert.equal(response.status, 200, `${response.status}: ${await response.clone().text()}`);
    return response.json();
  }
  const frames = async () => (await read(`${fixture.baseUrl}/console/timeline?identity=${encodeURIComponent(identity)}&mode=recent&limit=1000`)).frames;
  const requests = () => read(`${fixture.backendUrl}/__fixture/requests`);
  const state = () => read(`${fixture.backendUrl}/__fixture/durable-steer?session_id=${encodeURIComponent(result.accepted.session_id)}&input_id=${encodeURIComponent(result.accepted.input_id)}`);
  const history = () => read(`${fixture.backendUrl}/__fixture/session-history?session_id=${encodeURIComponent(result.accepted.session_id)}`);
  async function send(content, key) {
    const response = await rpc(fixture.baseUrl, "mobkit/console/send", {
      identity, content, origin: "console:durable-steer-acceptance", origin_kind: "operator",
      handling_mode: "queue", idempotency_key: key,
    });
    assert.equal(response.status, 200);
    assert(!response.body.error, JSON.stringify(response.body));
    assert(response.body.result?.input_frame_id && response.body.result.interaction_id);
    return response.body.result;
  }
  async function open(reload = false) {
    allowance = "navigation";
    if (reload) await page.reload(); else await page.goto(fixture.baseUrl + (host === "shared" ? "/shared" : "/scoped"));
    if (host === "stock" && !await pane().count()) await page.locator('.agent[role="button"], .cc-sidebar-row').filter({ hasText: /router:main/ }).first().click();
    if (host === "shared") await page.getByRole("combobox", { name: "Agent", exact: true }).selectOption(identity);
    await page.locator('[data-testid="console-transport-status"][data-phase="live"]').waitFor();
    await composer().waitFor();
    allowance = null;
  }
  async function capture(label) {
    await fs.mkdir(evidenceDir, { recursive: true });
    await page.screenshot({ path: path.join(evidenceDir, `${host}-durable-steer-${label}.png`), fullPage: true });
  }
  const inputStillRunning = current => current.current_run_id === result.runStart.run_id
    && current.terminal_outcome === null && current.completion === null;
  async function inspectNotice(label, requireRunning = false) {
    const rendered = await eventually(async () => {
      const before = requireRunning ? await state() : null;
      // Return this failure out of the polling helper immediately. A late
      // history row must never satisfy the live-rendering assertion.
      if (before && !inputStillRunning(before)) return { endedBeforeLiveObservation: before };
      const view = await viewport().evaluate((root, expected) => {
        const rows = [...root.querySelectorAll("[data-conversation-row-id]")]
          .filter(row => row.textContent.includes(expected));
        const workGraphTools = [...root.querySelectorAll(".cc-tool-call")]
          .filter(tool => tool.querySelector(".cc-tool-call__name")?.textContent === "workgraph_ready")
          .map(tool => ({
            rowId: tool.closest("[data-conversation-row-id]")?.dataset.conversationRowId,
            status: tool.querySelector(".cc-tool-call__status")?.textContent,
            displayed: tool.getBoundingClientRect().height > 0,
            beforeNotice: Boolean(rows[0] && (tool.compareDocumentPosition(rows[0]) & Node.DOCUMENT_POSITION_FOLLOWING)),
          }));
        return {
          text: root.textContent,
          matches: root.textContent.split(expected).length - 1,
          rows: rows.map(row => ({
            id: row.dataset.conversationRowId,
            text: row.textContent,
            workFooters: row.querySelectorAll('.msg__worked, [aria-label="Copy work time"]').length,
          })),
          incomingPeers: [...root.querySelectorAll(".cc-tool-call--incoming .cc-tool-call__name")]
            .map(peer => ({ text: peer.textContent, title: peer.getAttribute("title"), displayed: peer.getBoundingClientRect().height > 0 })),
          workGraphTools,
        };
      }, notice);
      const after = requireRunning ? await state() : null;
      if (after && !inputStillRunning(after)) return { endedBeforeLiveObservation: after };
      assert.equal(view.matches, 1, "the durable instruction appears exactly once in the real transcript");
      assert(!view.text.includes("boundary_append_applied") && !view.text.includes('"input_id"') && !view.text.includes('"append_count"'),
        "the transcript renders the instruction without raw transport envelopes");
      assert.equal(view.rows.length, 1, "one stable rendered row owns the durable notice");
      assert.equal(view.rows[0].workFooters, 0, "the typed System notice has no assistant work-duration footer");
      assert.doesNotMatch(view.rows[0].text, /Worked for/);
      for (const peer of result.expectedPeers) {
        const matching = view.incomingPeers.filter(rendered => rendered.title === peer.id);
        assert.equal(matching.length, 1, "one incoming peer header retains the exact canonical peer identity in its title");
        assert.equal(matching[0].text, `Received from ${peer.label}`, "canonical display metadata supplies the readable peer label");
        assert.equal(matching[0].displayed, true);
      }
      if (host === "stock") {
        assert.equal(view.workGraphTools.length, 1, "the empty WorkGraph query remains visible as one tool result");
        assert.equal(view.workGraphTools[0].displayed, true);
        assert.match(view.workGraphTools[0].status || "", /Success/);
        assert.equal(view.workGraphTools[0].beforeNotice, true, "the durable notice follows the real WorkGraph result in the rendered transcript");
      }
      if (requireRunning) view.liveObservation = { before, after };
      return view;
    }, `${host} ${label}: one readable durable instruction`);
    assert(!rendered.endedBeforeLiveObservation,
      `the original run completed before its notice was rendered live: ${JSON.stringify(rendered.endedBeforeLiveObservation)}`);
    result.views[label] = rendered;
    return rendered;
  }
  async function assertDraftAndReading() {
    const current = await composer().evaluate(node => ({ value: node.value, start: node.selectionStart, end: node.selectionEnd, focused: document.activeElement === node }));
    assert.deepEqual(current, result.draftBefore, "streaming the notice preserves draft, selection and keyboard focus");
    const after = await viewport().getByRole("heading", { name: "Evidence 3", exact: true }).boundingBox();
    assert(after && Math.abs(after.y - result.readingBefore.y) <= 2, "background delivery preserves the exact reading anchor");
  }
  try {
    await fixture.control("model", { source: seedSource, delay_ms: 0, chunk_chars: 4096 });
    const seeded = await send("Read the release evidence before the background review.", `${barrierId}-seed`);
    await eventually(async () => (await frames()).some(frame => liveEvent(frame) && frame.kind === "interaction_complete" && frame.interaction_id === seeded.interaction_id), "seed review completes through the real runtime");
    await open();
    await viewport().getByRole("heading", { name: "Evidence 3", exact: true }).waitFor();
    // The streamed reply remains visibly active long enough to inspect the
    // boundary event before completion. Progress is synchronized by events.
    await fixture.control("model", { source: "Unexpected fallback response.", delay_ms: 80, chunk_chars: 8 });
    result.armed = await fixture.control("model-barrier", { action: "arm", plan: { id: barrierId, match_text: instruction, source: finalSource } });
    armed = true;
    result.started = await send(instruction, `${barrierId}-turn`);
    result.entered = await eventually(async () => {
      const current = await fixture.control("model-barrier", { action: "status", id: barrierId });
      return current.entered && !current.released ? current : null;
    }, "the intended model request reaches its explicit barrier");
    result.runStart = await eventually(async () => (await frames()).find(frame => liveEvent(frame) && frame.kind === "run_started" && frame.interaction_id === result.started.interaction_id), "the running turn has canonical run and session ownership");
    assert(result.runStart.run_id && result.runStart.session_id);
    const seededHistory = await read(`${fixture.backendUrl}/__fixture/session-history?session_id=${encodeURIComponent(result.runStart.session_id)}`);
    result.expectedPeers = seededHistory.messages.flatMap(message => (message.blocks || [])
      .filter(block => block.type === "comms" && block.direction === "incoming" && block.peer?.display_name === "console-acceptance/lead/mk--domain_cdelivery")
      .map(block => ({ id: block.peer.id, displayName: block.peer.display_name, label: "domain:delivery" })));
    assert.equal(result.expectedPeers.length, 1, "the real fixture provides one canonical incoming peer with display metadata");
    assert(result.expectedPeers[0].id);
    await composer().fill(draft);
    await composer().evaluate(node => node.setSelectionRange(3, 17));
    await viewport().getByRole("heading", { name: "Evidence 3", exact: true }).scrollIntoViewIfNeeded();
    result.draftBefore = await composer().evaluate(node => ({ value: node.value, start: node.selectionStart, end: node.selectionEnd, focused: document.activeElement === node }));
    assert.equal(result.draftBefore.focused, true);
    result.readingBefore = await viewport().getByRole("heading", { name: "Evidence 3", exact: true }).boundingBox();
    assert(result.readingBefore);
    await capture("waiting-with-draft");

    result.accepted = await fixture.control("durable-steer", { identity, session_id: result.runStart.session_id, content: notice });
    assert.equal(result.accepted.accepted, true);
    assert.equal(result.accepted.session_id, result.runStart.session_id);
    assert(result.accepted.input_id);
    result.admitted = await state();
    assert.equal(result.admitted.current_run_id, result.runStart.run_id, "durable admission preserves the currently running turn");
    assert.equal(result.admitted.terminal_outcome, null, "admission alone does not consume the durable input");
    result.released = await fixture.control("model-barrier", { action: "release", id: barrierId });
    result.applied = await eventually(async () => (await frames()).find(frame => liveEvent(frame) && frame.kind === "boundary_append_applied" && frame.payload?.input_id === result.accepted.input_id), "the actual runtime applies the exact durable input at its next boundary");
    assert.equal(result.applied.run_id, result.runStart.run_id);
    assert.equal(result.applied.payload.run_id, result.runStart.run_id);
    assert.equal(result.applied.payload.append_count, 1);
    assert.equal(textContent(result.applied.payload.content), notice);
    result.duringRun = await state();
    assert.equal(result.duringRun.current_run_id, result.runStart.run_id, "the append is visible before the same run completes");
    const duringView = await inspectNotice("during-run", true);
    await assertDraftAndReading();
    result.afterLiveReading = await state();
    assert(inputStillRunning(result.afterLiveReading), "live notice and preserved typing/reading were observed before the original run completed");
    await capture("notice-preserves-reading");

    result.completed = await eventually(async () => {
      const current = await state();
      return current.terminal_outcome?.outcome_type === "consumed" && current.completion
        && current.current_run_id === null && current.queue.length === 0 && current.steer_queue.length === 0 ? current : null;
    }, "the durable input and the original run converge without a follow-up", 30_000);
    assert.equal(result.completed.run_id, result.runStart.run_id, "the durable input is consumed by the original run");
    assert.equal(result.completed.completion.completion_type, "completed", "the retained input shares its run's successful result receipt");
    result.finalFrames = await frames();
    assert.equal(result.finalFrames.filter(frame => liveEvent(frame) && frame.kind === "boundary_append_applied" && frame.payload?.input_id === result.accepted.input_id).length, 1);
    const turnRequests = (await requests()).filter(request => request.messages.some(message => message.role === "user" && textContent(message.content) === instruction));
    assert.equal(turnRequests.length, 2, "only the blocked request and one post-tool request execute; no follow-up turn");
    assert.equal(turnRequests[0].messages.filter(message => message.role === "system_notice" && message.body === notice).length, 0);
    assert.equal(turnRequests[1].messages.filter(message => message.role === "system_notice" && message.body === notice).length, 1, "the next actual model request carries the durable notice once");
    assert(turnRequests[1].messages.some(message => message.role === "tool_results" && message.results.some(tool => tool.tool_use_id === `fixture-${barrierId}-peer-ready` && !tool.is_error)), "a real WorkGraph tool result establishes the cooperative boundary");
    result.turnRequests = turnRequests;
    result.history = await history();
    assert.equal(result.history.has_more, false);
    const positions = result.history.messages.flatMap((message, index) => message.role === "system_notice" && message.body === notice ? [index] : []);
    assert.equal(positions.length, 1, "the committed session has exactly one durable instruction");
    const at = positions[0];
    assert.equal(result.history.messages[at - 1].role, "tool_results");
    const answer = result.history.messages[at + 1];
    assert.equal(answer.role, "block_assistant");
    assert.equal(answer.identity.run_id, result.runStart.run_id);
    assert.equal(assistantText(answer), finalSource);
    await assertDraftAndReading();
    const completedView = await inspectNotice("completed");
    assert.deepEqual(completedView.rows, duringView.rows, "the exact live notice row and content survive the committed history reconciliation");
    await viewport().getByText(notice, { exact: true }).scrollIntoViewIfNeeded();
    await capture("completed");

    allowance = "disconnect";
    const streams = () => fixture.observations.filter(item => item.path.includes("/timeline/stream") && item.status === 200).length;
    const before = streams(); fixture.disconnectStreams();
    await eventually(() => streams() > before, "the browser reconnects to an actual successful stream");
    await page.locator('[data-testid="console-transport-status"][data-phase="live"]').waitFor();
    allowance = null;
    const reconnected = await inspectNotice("reconnected");
    assert.deepEqual(reconnected.rows, completedView.rows, "reconnect preserves the exact notice row");
    assert.equal(await composer().inputValue(), draft);
    await open(true);
    const reloaded = await inspectNotice("reloaded");
    assert.deepEqual(reloaded.rows, completedView.rows, "history reconciliation and reload preserve the exact notice row");
    assert.deepEqual((await history()).messages, result.history.messages, "reload does not alter committed history");
    // Stock owns draft persistence. The reusable test host keeps draft state
    // in React only, so reload is a transcript check there.
    if (host === "stock") assert.equal(await composer().inputValue(), draft, "reload restores the unsent operator draft");
    await viewport().getByText(notice, { exact: true }).scrollIntoViewIfNeeded();
    await capture("reloaded");
    await viewport().locator(".cc-tool-call--incoming .cc-tool-call__name")
      .filter({ hasText: "Received from domain:delivery" }).scrollIntoViewIfNeeded();
    await capture("peer-label");
    const apiFailures = fixture.observations.filter(item => item.status >= 400 || (item.response && (() => { try { return Boolean(JSON.parse(item.response).error); } catch { return false; } })()));
    assert.deepEqual(apiFailures, [], "no unexpected actual API failures");
    assert.deepEqual(result.errors, [], "no unexpected browser or network failures");
  } catch (error) {
    result.failure = error.stack || String(error);
    await capture("failure").catch(() => {});
    throw error;
  } finally {
    if (armed) await fixture.control("model-barrier", { action: "release", id: barrierId }).catch(() => {});
    result.requests = await requests().catch(() => []);
    result.frames = await frames().catch(() => []);
    if (result.accepted) {
      result.finalState = await state().catch(error => ({ error: String(error) }));
      result.finalHistory = await history().catch(error => ({ error: String(error) }));
    }
    result.observations = fixture.observations;
    result.logs = fixture.logs();
    await fs.mkdir(evidenceDir, { recursive: true });
    await fs.writeFile(path.join(evidenceDir, `${host}-durable-steer.json`), JSON.stringify(result, null, 2));
    await browser.close(); await fixture.close();
  }
}

const scenarios = ["stock", "shared"].map(host => ({ id: `real-${host}-durable-steer`, family: "real-runtime", backend: "real", run: () => durableSteer(host) }));
module.exports = { scenarios };
if (require.main === module) require("../scenario-registry.cjs").runScenarios(scenarios).catch(error => { process.stderr.write(`${error.stack || error}\n`); process.exitCode = 1; });
