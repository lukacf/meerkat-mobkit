"use strict";
const assert = require("node:assert/strict");
const fs = require("node:fs/promises");
const path = require("node:path");
const { chromium } = require("playwright");
const { randomUUID } = require("node:crypto");
const { startFixture, eventually, rpc } = require("../acceptance-runtime.cjs");

const evidence = process.env.MOBKIT_BROWSER_EVIDENCE || path.join(__dirname, "../../output/playwright/console-acceptance");
const prose = index => `## Investigation ${index}\n\nThe reviewer checks the **candidate workgraph** and records the exact evidence. Repeated phrase, repeated phrase. Unicode: A\u030A, \u00e5 and \ud83d\ude80.\n\n- [x] Source inspected\n- [ ] Peer review pending\n\n| Check | Result |\n| --- | --- |\n| Runtime | Healthy |\n| Delivery | Confirmed |\n\n\`\`\`ts\nconst check = ${index};\n\`\`\`\n\n[Reference](https://example.com) and ![remote tracking image](https://example.com/tracker.png).\n\n`;

function browserErrors(page) {
  const errors = [];
  const expected = [];
  let allowed = null;
  page.on("pageerror", error => errors.push(error.message));
  page.on("requestfailed", request => {
    const failure = { method: request.method(), url: request.url(), error: request.failure()?.errorText };
    if (allowed?.matches(request)) expected.push({ ...failure, reason: allowed.reason });
    else errors.push(`${failure.method} ${failure.url}: ${failure.error} ${request.postData() || ""}`);
  });
  return {
    errors, expected,
    async during(reason, action, matches = request => new URL(request.url()).pathname.endsWith("/timeline/stream")) {
      assert.equal(allowed, null, "failure allowances must not overlap");
      allowed = { reason, matches };
      try { return await action(); } finally { allowed = null; }
    },
  };
}

async function settle(page) {
  await page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))));
}

async function timeline(fixture) {
  const response = await fetch(`${fixture.baseUrl}/console/timeline?identity=router%3Amain&mode=recent&limit=1000`);
  assert.equal(response.status, 200);
  return response.json();
}

async function completed(fixture, source) {
  return eventually(async () => (await timeline(fixture)).frames?.find(frame =>
    frame.kind === "text_complete" && JSON.stringify(frame.payload).includes(source)), `completed text ${source.slice(0, 50)}`, 40_000);
}

async function anchorAt(viewport, row, offset = 100) {
  const id = await row.getAttribute("data-conversation-row-id");
  assert(id, "runtime transcript row has a stable id");
  await viewport.evaluate((node, { id, offset }) => {
    const row = [...node.querySelectorAll("[data-conversation-row-id]")].find(item => item.dataset.conversationRowId === id);
    node.scrollTop += row.getBoundingClientRect().top - node.getBoundingClientRect().top - offset;
    node.dispatchEvent(new Event("scroll"));
  }, { id, offset });
  await settle(viewport.page());
  return viewport.evaluate((node, id) => {
    const row = [...node.querySelectorAll("[data-conversation-row-id]")].find(item => item.dataset.conversationRowId === id);
    return { id, offset: row.getBoundingClientRect().top - node.getBoundingClientRect().top };
  }, id);
}

async function measureAnchor(viewport, anchor, action, label) {
  const before = await viewport.evaluate((node, id) => {
    const row = [...node.querySelectorAll("[data-conversation-row-id]")].find(item => item.dataset.conversationRowId === id);
    return { top: row.getBoundingClientRect().top - node.getBoundingClientRect().top, height: node.clientHeight, rows: node.querySelectorAll("[data-conversation-row-id]").length, text: row.textContent.slice(0, 150) };
  }, anchor.id);
  await action(); await settle(viewport.page());
  const after = await viewport.evaluate((node, id) => {
    const row = [...node.querySelectorAll("[data-conversation-row-id]")].find(item => item.dataset.conversationRowId === id);
    return { top: row?.getBoundingClientRect().top - node.getBoundingClientRect().top, height: node.clientHeight, rows: node.querySelectorAll("[data-conversation-row-id]").length,
      ...(row ? {} : { retainedRows: [...node.querySelectorAll("[data-conversation-row-id]")].slice(0, 12).map(item => ({ id: item.dataset.conversationRowId, text: item.textContent.slice(0, 150) })) }) };
  }, anchor.id);
  const drift = Math.abs(after.top - before.top);
  assert(Number.isFinite(drift) && drift <= 2, `${label} anchor drift: ${JSON.stringify({ before, after, drift })}`);
  return { label, anchor: anchor.id, before, after, drift };
}

function initializationCancellation(request) {
  if (!request.failure()?.errorText?.includes("ERR_ABORTED")) return false;
  if (new URL(request.url()).pathname.endsWith("/timeline/stream")) return true;
  try {
    const body = JSON.parse(request.postData() || "{}");
    return body.method === "mobkit/console/query_timeline" && body.params?.mode === "recent" && body.params?.limit === 200 && !body.params?.identity;
  } catch { return false; }
}

async function open(page, fixture, host, monitor) {
  if (monitor) return monitor.during("initial host authority initialization", () => open(page, fixture, host), initializationCancellation);
  await page.goto(fixture.baseUrl + (host === "shared" ? "/shared" : "/console"));
  if (host === "stock") {
    await page.locator('.agent[role="button"], .cc-sidebar-row').filter({ hasText: /router/i }).first().click();
  }
  await page.locator('[data-testid="console-transport-status"][data-phase="live"]').waitFor();
}

const transcript = (page, host) => page.locator(host === "shared" ? '[data-testid="shared-pane-0"] .cc-conversation-pane__scroll' : ".conv__body").first();
const conversationPane = (page, host) => page.locator(host === "shared" ? '[data-testid="shared-pane-0"]' : ".conv").first();

async function inspectJump(page, host, working) {
  const viewport = transcript(page, host);
  const jump = conversationPane(page, host).getByRole("button", { name: "Jump to latest", exact: true });
  await jump.waitFor();
  await eventually(() => jump.getAttribute("data-working").then(value => value === String(working)), `${host} jump follows owner working=${working}`);
  const result = await jump.evaluate(async button => {
    const activity = button.querySelector(".cc-conversation-jump-latest__activity");
    const animation = activity.getAnimations()[0];
    const before = animation?.currentTime;
    await new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    const rect = button.getBoundingClientRect();
    return {
      text: button.textContent.trim(), svgCount: button.querySelectorAll("svg").length,
      position: getComputedStyle(button).position, width: rect.width, height: rect.height,
      top: rect.top, bottom: rect.bottom, left: rect.left, right: rect.right,
      anchorHeight: button.parentElement.getBoundingClientRect().height,
      animations: activity.getAnimations().length, playState: animation?.playState,
      advanced: typeof before === "number" && animation.currentTime > before,
      stroke: getComputedStyle(activity).stroke,
    };
  });
  const bounds = await viewport.boundingBox();
  assert.equal(result.text, "", `${host} jump uses an icon without visible text`);
  assert(result.svgCount >= 1);
  assert.equal(result.position, "absolute");
  assert.equal(result.anchorHeight, 0, `${host} jump adds no normal-flow row`);
  assert(result.width >= 32 && result.width <= 44 && Math.abs(result.width - result.height) <= 1, `${host} jump remains a compact circle`);
  assert(result.left >= bounds.x && result.right <= bounds.x + bounds.width && result.top >= bounds.y && result.bottom <= bounds.y + bounds.height + 2,
    `${host} jump is inside the transcript above its footer: ${JSON.stringify({ result, bounds })}`);
  if (working) {
    assert(result.animations > 0 && result.playState === "running" && result.advanced, `${host} actual active perimeter animation advances`);
    await page.emulateMedia({ reducedMotion: "reduce" });
    const reduced = await jump.locator(".cc-conversation-jump-latest__activity").evaluate(node => ({
      animations: node.getAnimations().length, name: getComputedStyle(node).animationName,
      stroke: getComputedStyle(node).stroke,
    }));
    assert.equal(reduced.animations, 0, `${host} reduced motion stops the activity animation`);
    assert.equal(reduced.name, "none");
    assert.notEqual(reduced.stroke, "transparent");
    assert.notEqual(reduced.stroke, "rgba(0, 0, 0, 0)", `${host} reduced motion retains a static active accent`);
    result.reducedMotion = reduced;
    await page.emulateMedia({ reducedMotion: "no-preference" });
  } else {
    assert.equal(result.animations, 0, `${host} idle jump has no running animation`);
  }
  return result;
}

async function inspectQuoteAction(page, host) {
  const viewport = transcript(page, host);
  const quote = conversationPane(page, host).getByRole("button", { name: "Add to message", exact: true });
  assert.equal(await quote.count(), 0, `${host} has no permanent quote action`);
  const before = await viewport.evaluate(node => ({ height: node.clientHeight, scrollTop: node.scrollTop }));
  const selectedText = await viewport.evaluate(node => {
    const bounds = node.getBoundingClientRect();
    const paragraph = [...node.querySelectorAll("[data-quote-source] p")].find(item => {
      const rect = item.getBoundingClientRect();
      return item.textContent.trim() && rect.top >= bounds.top + 50 && rect.bottom <= bounds.bottom - 10;
    });
    if (!paragraph) throw new Error("Need an actual visible reply paragraph for selection");
    const range = document.createRange(); range.selectNodeContents(paragraph);
    const selection = window.getSelection(); selection.removeAllRanges(); selection.addRange(range);
    return selection.toString();
  });
  await quote.waitFor();
  const visible = await quote.evaluate(button => {
    const rect = button.getBoundingClientRect();
    return { text: button.textContent.trim(), position: getComputedStyle(button).position,
      anchorHeight: button.parentElement.getBoundingClientRect().height,
      top: rect.top, bottom: rect.bottom, left: rect.left, right: rect.right,
      svgCount: button.querySelectorAll("svg").length };
  });
  const bounds = await viewport.boundingBox();
  assert.equal(visible.text, "", `${host} quote action has no visible text`);
  assert.equal(visible.position, "absolute"); assert.equal(visible.anchorHeight, 0);
  assert.equal(visible.svgCount, 1);
  assert(visible.left >= bounds.x && visible.right <= bounds.x + bounds.width && visible.top >= bounds.y && visible.bottom <= bounds.y + bounds.height,
    `${host} quote floats beside the visible selection: ${JSON.stringify({ visible, bounds })}`);
  const after = await viewport.evaluate(node => ({ height: node.clientHeight, scrollTop: node.scrollTop }));
  assert.deepEqual(after, before, `${host} quote appearance does not consume transcript height or move its reading position`);
  await capture(page, `${host}-floating-quote-selection`);
  await page.keyboard.press("Escape");
  await quote.waitFor({ state: "detached" });
  assert.equal(await page.evaluate(() => window.getSelection()?.toString()), selectedText, "Escape dismisses the action without destroying selection");
  await page.evaluate(() => window.getSelection()?.removeAllRanges());
  await settle(page);
  assert.equal(await quote.count(), 0, `${host} quote stays absent after selection clears`);
  return { selectedText, visible, before, after };
}

async function capture(page, name) {
  await fs.mkdir(evidence, { recursive: true });
  await page.screenshot({ path: path.join(evidence, `${name}.png`), fullPage: true });
}

async function sendApi(fixture, text, key = text) {
  const accepted = await rpc(fixture.baseUrl, "mobkit/console/send", {
    identity: "router:main", content: text, origin: "console:visual-acceptance",
    origin_kind: "operator", idempotency_key: key,
  });
  assert(accepted.body.result?.input_frame_id, JSON.stringify(accepted.body));
  return accepted.body.result;
}

async function presentation(host) {
  const fixture = await startFixture();
  const browser = await chromium.launch({ headless: true });
  const page = await browser.newPage({ viewport: { width: 1600, height: 1000 }, permissions: ["clipboard-read", "clipboard-write"] });
  const monitor = browserErrors(page);
  try {
    await fixture.control("model", { source: Array.from({ length: 12 }, (_, index) => prose(index)).join(""), delay_ms: 0, chunk_chars: 512 });
    const first = await sendApi(fixture, "Review the workgraph and prepare peer-review evidence.");
    await completed(fixture, "Investigation 11");
    await open(page, fixture, host, monitor);
    const viewport = transcript(page, host);
    await viewport.locator("table").first().waitFor();
    assert.equal(await viewport.locator('img[src*="example.com/tracker"]').count(), 0, "default Markdown cannot fetch external images");
    assert(await viewport.locator('a[href="https://example.com"]').count() > 0);
    await capture(page, `${host}-1600-initial`);

    // Reading intent must survive a new, long stream through the real runtime.
    await viewport.evaluate(node => { node.scrollTop = Math.min(350, node.scrollHeight / 3); node.dispatchEvent(new Event("scroll")); });
    const controls = { idle: await inspectJump(page, host, false), quote: await inspectQuoteAction(page, host) };
    const anchor = await viewport.evaluate(node => {
      const top = node.getBoundingClientRect().top;
      const row = [...node.querySelectorAll("[data-conversation-row-id]")].find(item => item.getBoundingClientRect().bottom > top + 20);
      return { id: row.dataset.conversationRowId, top: row.getBoundingClientRect().top - top };
    });
    const selectedText = "Preserve this exact selection: A\u030A, \u00e5 and \ud83d\ude80.";
    const streamSource = `${selectedText}\n\n${prose(99).repeat(8)}Stream finished successfully.\n`;
    await fixture.control("model", { source: streamSource, delay_ms: 16, chunk_chars: 8 });
    await sendApi(fixture, "Continue in the background while I inspect the earlier evidence.", "stream-anchor");
    const streamingDocument = viewport.locator('.cc-markdown-document[data-streaming="true"]').last();
    await streamingDocument.getByText(selectedText, { exact: true }).waitFor();
    controls.working = await inspectJump(page, host, true);
    await capture(page, `${host}-floating-jump-working`);
    await streamingDocument.getByText(selectedText, { exact: true }).evaluate(node => {
      const range = document.createRange(); range.selectNodeContents(node);
      const selection = window.getSelection(); selection.removeAllRanges(); selection.addRange(range);
      window.__presentationSelection = { node, document: node.closest(".cc-markdown-document") };
    });
    const geometry = await viewport.evaluate(async (node, anchor) => {
      let maxDrift = 0; let updates = 0;
      const row = () => [...node.querySelectorAll("[data-conversation-row-id]")].find(item => item.dataset.conversationRowId === anchor.id);
      const observer = new MutationObserver(() => { updates += 1; const item = row(); if (item) maxDrift = Math.max(maxDrift, Math.abs(item.getBoundingClientRect().top - node.getBoundingClientRect().top - anchor.top)); });
      observer.observe(node, { childList: true, subtree: true, characterData: true });
      const start = performance.now();
      while (updates < 100 && performance.now() - start < 20_000) await new Promise(requestAnimationFrame);
      observer.disconnect();
      return { maxDrift, updates };
    }, anchor);
    assert(geometry.updates >= 100, `must observe 100 real streamed updates: ${JSON.stringify(geometry)}`);
    assert(geometry.maxDrift <= 2, `${host} stream anchor drift: ${JSON.stringify(geometry)}`);
    await completed(fixture, "Stream finished successfully.");
    await eventually(() => streamingDocument.count().then(count => count === 0), "stream document completed");
    const selection = await page.evaluate(() => ({
      text: window.getSelection()?.toString(), connected: window.__presentationSelection.node.isConnected,
      sameDocument: window.__presentationSelection.node.closest(".cc-markdown-document") === window.__presentationSelection.document,
      streaming: window.__presentationSelection.document.dataset.streaming,
      completedSource: window.__presentationSelection.document.closest("[data-quote-source]")?.dataset.quoteSource,
    }));
    assert.equal(selection.text, selectedText, `${host} selection survives stream completion`);
    assert(selection.connected && selection.sameDocument, `${host} selected paragraph and document keep DOM identity`);
    assert.equal(selection.streaming, "false");
    assert.equal(selection.completedSource, streamSource, `${host} the selected document becomes the exact complete reply`);
    assert.equal(await viewport.locator(".cc-markdown-document").filter({ hasText: selectedText }).count(), 1, `${host} history/live reconciliation does not duplicate the reply`);
    geometry.selection = selection;
    controls.completed = await inspectJump(page, host, false);
    geometry.controls = controls;
    geometry.resize = await measureAnchor(viewport, anchor, () => page.setViewportSize({ width: 1440, height: 900 }), `${host} viewport resize while reading`);
    await capture(page, `${host}-1440-reading`);
    const completedDocument = viewport.locator(".cc-markdown-document").filter({ hasText: selectedText }).last();
    const copy = host === "stock"
      ? completedDocument.locator('xpath=ancestor::*[contains(concat(" ",normalize-space(@class)," ")," msg ")][1]').getByRole("button", { name: "Copy reply", exact: true })
      : completedDocument.locator('xpath=ancestor::*[contains(concat(" ",normalize-space(@class)," ")," cc-message-group ")][1]').getByRole("button", { name: "Copy response", exact: true });
    await page.evaluate(() => {
      window.__clipboardEvidence = { writes: [], clicks: [] };
      const original = navigator.clipboard.writeText.bind(navigator.clipboard);
      navigator.clipboard.writeText = async text => {
        const call = { length: text.length, tail: text.slice(-80) }; window.__clipboardEvidence.writes.push(call);
        try { const result = await original(text); call.ok = true; return result; } catch (error) { call.error = String(error); throw error; }
      };
      for (const type of ["pointerdown", "pointerup", "mousedown", "mouseup", "click"]) document.addEventListener(type, event => {
        const button = event.target.closest("button"); window.__clipboardEvidence.clicks.push({ type, button: button?.outerHTML, target: event.target.outerHTML?.slice(0, 300), x: event.clientX, y: event.clientY });
      }, { capture: true });
    });
    await copy.click();
    await eventually(async () => {
      const actual = await page.evaluate(() => navigator.clipboard.readText());
      assert.equal(actual, streamSource, `${host} exact Markdown source clipboard differs (actual ${actual.length}, expected ${streamSource.length})`);
      return true;
    }, `${host} exact Markdown source clipboard`);
    geometry.clipboardExact = true;
    const jumpAfterCopy = conversationPane(page, host).getByRole("button", { name: "Jump to latest", exact: true });
    if (await jumpAfterCopy.count()) await jumpAfterCopy.click();
    const endDistance = await viewport.evaluate(node => node.scrollHeight - node.scrollTop - node.clientHeight);
    assert(endDistance <= 48, `${host} copy target reveal or jump reaches the transcript end, remaining ${endDistance}px`);
    await page.setViewportSize({ width: 1024, height: 768 });
    await capture(page, `${host}-1024-containment`);
    if (host === "shared") {
      await page.getByRole("button", { name: "Toggle second pane" }).click();
      await page.setViewportSize({ width: 1600, height: 1000 });
      await capture(page, `${host}-two-panes`);
    }
    await monitor.during("explicit presentation reload", async () => {
      await page.reload();
      if (host === "stock" && !await page.locator(".conv__body").count()) await page.locator('.agent[role="button"], .cc-sidebar-row').filter({ hasText: /router/i }).first().click();
      await page.locator('[data-testid="console-transport-status"][data-phase="live"]').waitFor();
    }, initializationCancellation);
    await transcript(page, host).getByText("Review the workgraph and prepare peer-review evidence.", { exact: true }).waitFor();
    assert(first.input_frame_id);
    assert.deepEqual(monitor.errors, []);
    await fs.writeFile(path.join(evidence, `${host}-geometry.json`), JSON.stringify({ geometry, errors: monitor.errors, expectedFailures: monitor.expected, observations: fixture.observations }, null, 2));
  } catch (error) {
    await capture(page, `${host}-presentation-failure`).catch(() => {});
    await fs.writeFile(path.join(evidence, `${host}-presentation-failure.json`), JSON.stringify({ error: error.stack, html: await page.content(), clipboard: await page.evaluate(() => ({ ...window.__clipboardEvidence, focused: document.hasFocus() })), frames: (await timeline(fixture)).frames, observations: fixture.observations, logs: fixture.logs() }, null, 2));
    throw error;
  } finally { await browser.close(); await fixture.close(); }
}

async function recovery(host) {
  const fixture = await startFixture();
  const browser = await chromium.launch({ headless: true });
  const page = await browser.newPage({ viewport: { width: 1600, height: 1000 } });
  const monitor = browserErrors(page);
  try {
    await fixture.control("model", { source: prose(1), delay_ms: 0, chunk_chars: 500 });
    await sendApi(fixture, "Keep this history through recovery.");
    await completed(fixture, "Investigation 1");
    await open(page, fixture, host, monitor);
    await transcript(page, host).getByText("Keep this history through recovery.", { exact: true }).waitFor();
    const baselineRecent = () => fixture.observations.filter(item => item.request.includes('"mode":"recent"')).length;
    const before = baselineRecent();
    await monitor.during("deliberate SSE disconnect and one 503", async () => {
      fixture.rejectNextStreams(1); fixture.disconnectStreams();
      await page.locator('[data-testid="console-transport-status"][data-phase="retrying"]').waitFor();
      await capture(page, `${host}-reconnecting`);
      await page.locator('[data-testid="console-transport-status"][data-phase="live"]').waitFor();
    });
    assert.equal(baselineRecent(), before, "503/EOF does not require a history repair");
    for (let gap = 0; gap < 2; gap += 1) {
      const beforeGaps = fixture.observations.filter(item => item.path.includes("timeline/stream") && item.status === 409).length;
      await monitor.during(`deliberate replay expiry ${gap + 1}`, async () => {
        await fixture.control("fault", { fault: "expired" }); fixture.disconnectStreams();
        await eventually(() => fixture.observations.filter(item => item.path.includes("timeline/stream") && item.status === 409).length > beforeGaps, `${host} real replay gap`);
        await fixture.control("fault", { fault: "none" });
        await page.locator('[data-testid="console-transport-status"][data-phase="live"]').waitFor();
      });
    }
    await transcript(page, host).getByText("Keep this history through recovery.", { exact: true }).waitFor();
    assert.deepEqual(monitor.errors, []);
    await capture(page, `${host}-recovered`);
    await fs.writeFile(path.join(evidence, `${host}-recovery.json`), JSON.stringify({ errors: monitor.errors, expectedFailures: monitor.expected, observations: fixture.observations }, null, 2));
  } catch (error) {
    await capture(page, `${host}-recovery-failure`).catch(() => {});
    await fs.writeFile(path.join(evidence, `${host}-recovery-failure.json`), JSON.stringify({ error: error.stack, html: await page.content(), observations: fixture.observations, errors: monitor.errors, expectedFailures: monitor.expected }, null, 2));
    throw error;
  } finally { await browser.close(); await fixture.close(); }
}

async function uploadDiagram(fixture, bytes, instruction) {
  const uploadId = `presentation-${randomUUID()}`;
  const form = new FormData();
  form.append(`file:${uploadId}`, new Blob([bytes], { type: "image/png" }), "presentation-diagram.png");
  form.append("payload", JSON.stringify({ jsonrpc: "2.0", id: uploadId, method: "mobkit/console/send", params: {
    identity: "router:main", origin: "console:presentation-geometry", origin_kind: "operator", idempotency_key: uploadId,
    content: [{ type: "text", text: instruction }, { type: "image_upload", upload_id: uploadId, media_type: "image/png", alt: "Presentation dependency diagram" }],
  } }));
  const response = await fetch(`${fixture.baseUrl}/console/rpc/multipart`, { method: "POST", body: form });
  assert.equal(response.status, 200); const payload = await response.json();
  assert(payload.result?.input_frame_id, JSON.stringify(payload));
  const imageFrame = await eventually(async () => (await timeline(fixture)).frames.find(frame => frame.kind === "user_input"
    && JSON.stringify(frame.payload).includes(instruction)), "real uploaded image input");
  const image = imageFrame.payload.content.find(block => block.type === "image");
  assert(image?.blob_id, JSON.stringify(imageFrame));
  return { inputFrameId: imageFrame.id, blobId: image.blob_id };
}

async function layoutMutations(host) {
  const fixture = await startFixture();
  const browser = await chromium.launch({ headless: true });
  const page = await browser.newPage({ viewport: { width: 1600, height: 1000 }, permissions: ["clipboard-read", "clipboard-write"] });
  const monitor = browserErrors(page);
  const result = { host, checks: [], errors: monitor.errors, expectedFailures: monitor.expected };
  let releaseImage;
  try {
    // Emit real WorkGraph tool calls so the disclosure contains owner records.
    const peer = (await rpc(fixture.baseUrl, "mobkit/cross_mob/peer_info", { member_id: "router:main" })).body.result;
    const parts = peer?.comms_name?.split("/");
    assert(parts?.length === 3 && parts[0] === peer.mob_id, JSON.stringify(peer));
    const runId = `layout-${randomUUID().slice(0, 8)}`;
    await fixture.control("model", { source: "Acknowledged.", chunk_chars: 4096, scenario: {
      kind: "workgraph", run_id: runId, owner_id: `mob/${peer.mob_id}/agent/${parts[2]}`,
    } });
    await sendApi(fixture, `[fixture:${runId}] Complete the two release prerequisites and inspect their real tool results.`);
    await completed(fixture, "WorkGraph scenario complete");

    const diagramPage = await browser.newPage({ viewport: { width: 640, height: 360 } });
    await diagramPage.setContent('<body style="margin:0;background:#f5f0e7;font:24px system-ui;color:#213547"><div style="padding:40px"><h2>Release review</h2><p>Source and image review feed the publication task.</p><div style="display:flex;gap:25px"><span style="padding:20px;background:#c5d8ee">Source</span><span style="padding:20px;background:#cbe0c2">Image</span><span style="padding:20px;background:#fff">Publish</span></div></div></body>');
    const bytes = await diagramPage.screenshot(); await diagramPage.close();
    await fixture.control("model", { source: "The uploaded diagram is available for the release review.", delay_ms: 0, chunk_chars: 4096 });
    result.image = await uploadDiagram(fixture, bytes, "Keep the dependency image above the reading anchor.");
    await completed(fixture, "The uploaded diagram is available");
    const anchorText = "Reading anchor: inspect this retained decision while earlier content changes.";
    await fixture.control("model", { source: prose(201).repeat(8), delay_ms: 0, chunk_chars: 4096 });
    await sendApi(fixture, anchorText);
    await completed(fixture, "Investigation 201");

    // Delay only the actual blob's network response. Do not fabricate a frame or image.
    const imageGate = new Promise(resolve => { releaseImage = resolve; });
    const imageUrl = `${fixture.baseUrl}/blobs/${encodeURIComponent(result.image.blobId)}`;
    let requestedImage = false;
    await page.route(imageUrl, async route => { requestedImage = true; await imageGate; await route.continue(); });
    await open(page, fixture, host, monitor);
    const viewport = transcript(page, host);
    const anchorRow = viewport.locator("[data-conversation-row-id]").filter({ hasText: anchorText }).last();
    await anchorRow.waitFor();
    const delayedImage = viewport.locator(`img[src*="${encodeURIComponent(result.image.blobId)}"]`).first();
    await delayedImage.waitFor({ state: "attached" });
    await delayedImage.scrollIntoViewIfNeeded();
    await eventually(() => requestedImage, "browser requests actual delayed blob");
    assert.equal(await delayedImage.evaluate(node => node.complete), false, "blob remains pending until release");
    const anchor = await anchorAt(viewport, anchorRow, -10);
    result.checks.push(await measureAnchor(viewport, anchor, async () => {
      releaseImage();
      await eventually(() => delayedImage.evaluate(node => node.complete && node.naturalWidth === 640), "delayed real image decodes");
    }, `${host} delayed blob load`));

    // Expanding a genuine completed tool above the reader cannot move the anchor.
    const closedHeader = viewport.locator('[data-testid^="workgraph-item:"][aria-expanded="false"], .cc-tool-call__header[role="button"][aria-expanded="false"]').first();
    assert(await closedHeader.count(), "real completed WorkGraph tool has a disclosure");
    const toolHeaderId = await closedHeader.getAttribute("data-testid");
    const headerIndex = toolHeaderId ? -1 : await closedHeader.evaluate(node => [...node.closest('[aria-label="Conversation transcript"]').querySelectorAll('.cc-tool-call__header[role="button"]')].indexOf(node));
    const toolHeader = toolHeaderId ? viewport.getByTestId(toolHeaderId) : viewport.locator('.cc-tool-call__header[role="button"]').nth(headerIndex);
    const toolTitle = await toolHeader.innerText();
    result.checks.push(await measureAnchor(viewport, anchor, async () => {
      // Activate the disclosure without Playwright scrolling the earlier control into view.
      await toolHeader.dispatchEvent("click");
      await eventually(async () => await toolHeader.getAttribute("aria-expanded") === "true", "tool disclosure expands");
    }, `${host} earlier completed tool disclosure`));
    result.disclosure = toolTitle;

    // Stock grows its composer through the supported image staging flow. Shared
    // host exposes a native textarea resize grip, exercised with a real drag.
    if (host === "stock") {
      const composer = page.getByTestId("chat-composer:router:main");
      result.checks.push(await measureAnchor(viewport, anchor, async () => {
        await composer.evaluate((node, data) => {
          const transfer = new DataTransfer(); transfer.items.add(new File([Uint8Array.from(data)], "staged-diagram.png", { type: "image/png" }));
          node.dispatchEvent(new DragEvent("drop", { bubbles: true, cancelable: true, dataTransfer: transfer }));
        }, Array.from(bytes));
        await page.locator(".composer__attachment img").waitFor();
      }, "stock composer attachment growth"));
    } else {
      const composer = page.getByRole("textbox", { name: "Message", exact: true });
      const box = await composer.boundingBox();
      result.checks.push(await measureAnchor(viewport, anchor, async () => {
        await page.mouse.move(box.x + box.width - 4, box.y + box.height - 4); await page.mouse.down();
        await page.mouse.move(box.x + box.width - 4, box.y + box.height + 90, { steps: 8 }); await page.mouse.up();
        assert((await composer.boundingBox()).height > box.height + 40, "native shared composer actually grows");
      }, "shared native composer resize"));
    }
    assert(result.checks.at(-1).after.height < result.checks.at(-1).before.height, "composer growth reduces actual transcript height");
    await capture(page, `${host}-layout-1600`);
    result.checks.push(await measureAnchor(viewport, anchor, () => page.setViewportSize({ width: 1440, height: 900 }), `${host} width and height resize`));
    await capture(page, `${host}-layout-1440`);
    result.checks.push(await measureAnchor(viewport, anchor, () => page.setViewportSize({ width: 1024, height: 768 }), `${host} compact viewport resize`));
    await capture(page, `${host}-layout-1024`);
    if (host === "shared") {
      await page.setViewportSize({ width: 1600, height: 1000 }); await settle(page);
      result.checks.push(await measureAnchor(viewport, anchor, () => page.getByRole("button", { name: "Toggle second pane", exact: true }).click(), "shared split-pane width change"));
      await capture(page, "shared-layout-split");
      assert.equal(await page.locator('[data-testid^="shared-pane-"]').count(), 2);
    }
    assert.deepEqual(monitor.errors, []);
  } catch (error) {
    result.failure = error.stack || String(error); await capture(page, `${host}-layout-failure`).catch(() => {}); throw error;
  } finally {
    releaseImage?.();
    result.html = result.failure ? await page.content() : undefined;
    result.failedRequests = fixture.observations.filter(item => item.status !== 200);
    await fs.mkdir(evidence, { recursive: true });
    await fs.writeFile(path.join(evidence, `${host}-layout-geometry.json`), JSON.stringify(result, null, 2));
    await browser.close(); await fixture.close();
  }
}

async function olderHistory(host) {
  const fixture = await startFixture();
  const browser = await chromium.launch({ headless: true });
  const page = await browser.newPage({ viewport: { width: 1600, height: 1000 } });
  const monitor = browserErrors(page);
  const result = { host, turns: 0, pages: [], prepends: [], errors: monitor.errors, expectedFailures: monitor.expected };
  try {
    // Cross the owner's 1000-frame raw scan window as well as each host's
    // 200-frame recent page, then prove every actual turn remains reachable.
    const expected = [];
    for (let index = 0; index < 127; index += 1) {
      const input = `Review history checkpoint ${index}.`;
      const source = `History checkpoint ${index}: source, peer receipt and release decision retained.`;
      await fixture.control("model", { source, delay_ms: 0, chunk_chars: 4096 });
      const accepted = await sendApi(fixture, input);
      const complete = await completed(fixture, source);
      expected.push({ input, source, inputFrameId: accepted.input_frame_id, completeFrameId: complete.id });
      result.turns = index + 1;
    }
    const history = await timeline(fixture);
    assert(history.frames.length >= 1000 && Number(history.latest_cursor.split(":").at(-1)) > 1000,
      "actual owner history crosses both the recent-page and raw-scan boundaries");
    await open(page, fixture, host, monitor);
    const viewport = transcript(page, host);
    const ownerPages = () => fixture.observations.flatMap(item => {
      if (!item.request || !item.response) return [];
      try {
        const request = JSON.parse(item.request);
        if (request.method !== "mobkit/console/query_timeline" || request.params?.identity !== "router:main"
          || request.params?.mode !== "recent" || request.params?.limit !== 200) return [];
        return [{ observation: item, request, response: JSON.parse(item.response) }];
      } catch { return []; }
    });
    const seed = ownerPages().filter(item => !item.request.params.before).at(-1);
    assert(seed && seed.observation.status === 200 && seed.response.result?.frames.length > 0,
      "host seeds the actual authorized recent page");
    const ownerFrames = new Map(seed.response.result.frames.map(frame => [frame.id, frame]));
    const recordPage = (item, precedingBoundary) => {
      assert.equal(item.observation.status, 200);
      assert(Array.isArray(item.response.result?.frames), "older history request succeeds");
      const frames = item.response.result.frames;
      const before = item.request.params.before;
      const beforeSeq = Number(before.split(":").at(-1));
      assert(Number.isFinite(beforeSeq), "history request uses an owner cursor");
      assert.equal(before, precedingBoundary, "next page starts at the oldest retained owner cursor");
      assert(frames.every(frame => Number(frame.cursor.split(":").at(-1)) < beforeSeq),
        "every older frame is strictly before the requested owner boundary");
      for (const frame of frames) {
        assert(!ownerFrames.has(frame.id), `older pages do not repeat owner frame ${frame.id}`);
        ownerFrames.set(frame.id, frame);
      }
      result.pages.push({ request: item.request.params, exhausted: item.response.result.exhausted === true,
        frames: frames.map(frame => ({ id: frame.id, cursor: frame.cursor, kind: frame.kind })) });
      return frames[0]?.cursor ?? before;
    };
    const anchorRow = viewport.locator("[data-conversation-row-id]").nth(2);
    const anchor = await anchorAt(viewport, anchorRow, -10);
    let boundary = seed.response.result.frames[0].cursor;
    let exhausted = seed.response.result.exhausted === true;
    for (let index = 0; !exhausted && index < 16; index += 1) {
      const beforePages = ownerPages().filter(item => item.request.params.before).length;
      result.prepends.push(await measureAnchor(viewport, anchor, async () => {
        // Stock may first need to reveal its already-loaded virtual window.
        const reveal = viewport.getByRole("button", { name: "Show earlier messages", exact: true });
        if (await reveal.count()) await reveal.dispatchEvent("click");
        const older = page.getByRole("button", { name: "Load older history", exact: true }).first();
        await older.waitFor({ state: "attached", timeout: 5000 });
        await eventually(() => older.isEnabled(), "history action leaves its previous loading state");
        await older.dispatchEvent("click");
        await eventually(() => ownerPages().filter(item => item.request.params.before).length > beforePages,
          "actual owner older-history request completes");
        await eventually(async () => !(await page.getByRole("button", { name: "Loading history", exact: true }).count()),
          "host applies the completed older page");
      }, `${host} actual history prepend ${index + 1}`));
      const newPages = ownerPages().filter(item => item.request.params.before).slice(beforePages);
      assert(newPages.length > 0);
      for (const item of newPages) {
        boundary = recordPage(item, boundary);
        exhausted = item.response.result.exhausted === true;
      }
    }
    assert(exhausted, "repeated older-page loads reach the owner's history beginning");
    assert(result.pages.length > 1, "acceptance crosses multiple actual owner pages");
    const finalReveal = viewport.getByRole("button", { name: "Show earlier messages", exact: true });
    if (await finalReveal.count()) {
      result.prepends.push(await measureAnchor(viewport, anchor, () => finalReveal.dispatchEvent("click"),
        `${host} reveal retained first messages`));
    }
    await settle(page);
    const quotedSources = await viewport.locator("[data-quote-source]").evaluateAll(nodes => nodes.map(node => node.getAttribute("data-quote-source")));
    for (let index = 0; index < expected.length; index += 1) {
      const turn = expected[index];
      assert.equal(ownerFrames.get(turn.inputFrameId)?.payload.content, turn.input,
        `${host} all older pages retain exact operator checkpoint ${index}`);
      assert(JSON.stringify(ownerFrames.get(turn.completeFrameId)?.payload).includes(turn.source),
        `${host} all older pages retain exact actual reply checkpoint ${index}`);
      assert.equal(quotedSources.filter(source => source === turn.input).length, 1,
        `${host} operator checkpoint ${index} is reachable exactly once in the actual transcript`);
      assert.equal(quotedSources.filter(source => source === turn.source).length, 1,
        `${host} reply checkpoint ${index} is reachable exactly once in the actual transcript`);
    }
    const rowIds = await viewport.locator("[data-conversation-row-id]").evaluateAll(nodes => nodes.map(node => node.dataset.conversationRowId));
    assert.equal(new Set(rowIds).size, rowIds.length, "retained transcript has no duplicate canonical rows");
    result.completeHistory = { operatorMessages: expected.length, modelReplies: expected.length,
      ownerFrames: ownerFrames.size, renderedRows: rowIds.length, pages: result.pages.length };
    assert(result.prepends.at(-1).after.rows >= result.prepends[0].before.rows + 10,
      "multiple actual older messages prepend");
    await capture(page, `${host}-real-history-prepend`);
    // Also inspect the recovered beginning, instead of only the retained recent anchor.
    await viewport.evaluate(node => { node.scrollTop = 0; node.dispatchEvent(new Event("scroll")); });
    await settle(page);
    await capture(page, `${host}-real-history-complete-start`);
    assert.deepEqual(monitor.errors, []);
  } catch (error) {
    result.failure = error.stack || String(error); await capture(page, `${host}-history-prepend-failure`).catch(() => {}); throw error;
  } finally {
    await fs.mkdir(evidence, { recursive: true });
    await fs.writeFile(path.join(evidence, `${host}-history-prepend.json`), JSON.stringify(result, null, 2));
    await browser.close(); await fixture.close();
  }
}

const scenarios = [
  ...["stock", "shared"].map(host => ({ id: `real-${host}-presentation`, family: "real-presentation", backend: "real", run: () => presentation(host) })),
  ...["stock", "shared"].map(host => ({ id: `real-${host}-layout-mutations`, family: "real-presentation", backend: "real", run: () => layoutMutations(host) })),
  ...["stock", "shared"].map(host => ({ id: `real-${host}-history-prepend`, family: "real-presentation", backend: "real", run: () => olderHistory(host) })),
  ...["stock", "shared"].map(host => ({ id: `real-${host}-recovery`, family: "real-transport", backend: "real", run: () => recovery(host) })),
];
module.exports = { scenarios };

if (require.main === module) {
  assert(process.env.MOBKIT_EXAMPLE_BIN_DIR, "Use the coordinator prebuilt fixture; this lane must not run Cargo.");
  require("../scenario-registry.cjs").runScenarios(scenarios).catch(error => { process.stderr.write(`${error.stack || error}\n`); process.exitCode = 1; });
}
