#!/usr/bin/env node

const assert = require("node:assert/strict");
const fs = require("node:fs");
const http = require("node:http");
const net = require("node:net");
const path = require("node:path");
const { spawn, spawnSync } = require("node:child_process");
const { setTimeout: sleep } = require("node:timers/promises");
const { chromium } = require("playwright");

const repoRoot = path.resolve(__dirname, "..");

function waitForExit(child, timeoutMs = 5_000) {
  return new Promise((resolve) => {
    if (child.exitCode !== null) {
      resolve();
      return;
    }
    const timer = setTimeout(() => {
      if (child.exitCode === null) {
        child.kill("SIGKILL");
      }
      resolve();
    }, timeoutMs);
    child.once("exit", () => {
      clearTimeout(timer);
      resolve();
    });
  });
}

async function stopBackend(child) {
  if (child.exitCode !== null) {
    return;
  }
  child.kill("SIGTERM");
  await waitForExit(child);
}

function reservePort() {
  return new Promise((resolve, reject) => {
    const server = net.createServer();
    server.on("error", reject);
    server.listen(0, "127.0.0.1", () => {
      const address = server.address();
      if (!address || typeof address === "string") {
        server.close(() => reject(new Error("failed to reserve port")));
        return;
      }
      const { port } = address;
      server.close((error) => {
        if (error) {
          reject(error);
          return;
        }
        resolve(port);
      });
    });
  });
}

async function waitForHttpOk(url, timeoutMs = 120_000) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    try {
      const response = await fetch(url);
      if (response.ok) {
        return;
      }
    } catch (_) {
      // Backend is still starting.
    }
    await sleep(500);
  }
  throw new Error(`timed out waiting for ${url}`);
}

async function launchBrowser() {
  try {
    return await chromium.launch({ headless: true });
  } catch (launchError) {
    const npxCommand = process.platform === "win32" ? "npx.cmd" : "npx";
    const installResult = spawnSync(npxCommand, ["playwright", "install", "chromium"], {
      cwd: __dirname,
      stdio: "inherit",
    });
    if (installResult.status !== 0) {
      throw new Error(`playwright chromium install failed with status ${installResult.status}`);
    }
    return chromium.launch({ headless: true });
  }
}

async function gotoConsole(page, url) {
  await page.goto(url, { waitUntil: "domcontentloaded", timeout: 30_000 });
  await page.waitForSelector('[data-testid="meerkat-console"], .cc-workbench, .cc-conversation-pane', { timeout: 30_000 });
}

async function sidebarLabels(page) {
  return page.$$eval(
    '.agent[role="button"], .cc-sidebar-row',
    (rows) => rows.map((row) => (row.textContent || "").trim()),
  );
}

async function fillComposer(page, text) {
  const textarea = page
    .locator('[data-testid^="chat-composer:"]:visible, textarea:visible, .cc-composer__textarea:visible')
    .first();
  await textarea.fill(text);
}

async function clickSend(page) {
  const send = page.locator('button:has-text("Send"):visible, .cc-composer__send-btn:visible').first();
  await send.click();
}

async function openSidebarAgentChat(page, labelPattern) {
  const row = page
    .locator('.agent[role="button"], .cc-sidebar-row')
    .filter({ hasText: labelPattern })
    .first();
  await row.click();
  await page.waitForSelector('[data-testid^="chat-composer:"]', { timeout: 30_000 });
}

function startMockConsoleServer(port, options = {}) {
  const baseUrl = `http://127.0.0.1:${port}`;
  const html = fs.readFileSync(path.join(__dirname, "dist", "index.html"), "utf8");
  const js = fs.readFileSync(path.join(__dirname, "dist", "console-app.js"), "utf8");
  const css = fs.readFileSync(path.join(__dirname, "dist", "console-app.css"), "utf8");
  const tinyPng = Buffer.from(
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+/p9sAAAAASUVORK5CYII=",
    "base64",
  );
  const timelineFrames = Array.isArray(options.timelineFrames) ? options.timelineFrames : [];
  const includeImageAgent = options.includeImageAgent === true;
  const requests = [];

  const server = http.createServer((req, res) => {
    const method = req.method || "GET";
    const url = req.url || "/";
    const chunks = [];
    req.on("data", (chunk) => chunks.push(chunk));
    req.on("end", () => {
      const body = Buffer.concat(chunks).toString("utf8");
      requests.push({ method, url, body });

      if (method === "GET" && url === "/console") {
        res.writeHead(200, { "content-type": "text/html; charset=utf-8" });
        res.end(html);
        return;
      }
      if (method === "GET" && url.startsWith("/console/assets/console-app.js")) {
        res.writeHead(200, { "content-type": "application/javascript; charset=utf-8" });
        res.end(js);
        return;
      }
      if (method === "GET" && url.startsWith("/console/assets/console-app.css")) {
        res.writeHead(200, { "content-type": "text/css; charset=utf-8" });
        res.end(css);
        return;
      }
      if (method === "GET" && url.startsWith("/blobs/")) {
        res.writeHead(200, { "content-type": "image/png", "cache-control": "no-store" });
        res.end(tinyPng);
        return;
      }
      if (method === "GET" && url === "/console/modules") {
        res.writeHead(200, { "content-type": "application/json" });
        res.end(JSON.stringify({ modules: [] }));
        return;
      }
      if (method === "GET" && url === "/console/identities") {
        res.writeHead(200, { "content-type": "application/json" });
        res.end(JSON.stringify({
          rows: [
            {
              identity: "identity:luka",
              display_name: "Identity Luka",
              profile: "lead",
              state: "running",
              addressability: "addressable",
            },
            {
              identity: "legacy-router",
              display_name: "Legacy Router",
              profile: "router",
              state: "running",
              addressability: "addressable",
            },
            ...(includeImageAgent ? [{
              identity: "image-agent",
              display_name: "Image Agent",
              profile: "coordinator",
              state: "running",
              addressability: "addressable",
              model_capabilities: { image_input: true },
            }] : []),
          ],
        }));
        return;
      }
      if (method === "GET" && url === "/console/experience") {
        res.writeHead(200, { "content-type": "application/json" });
        res.end(JSON.stringify({
          contract_version: "0.3.0",
          agent_sidebar: {
            panel_id: "console.agent_sidebar",
            schema_version: "1",
            refresh: { mode: "pull", interval_ms: 5000 },
            live_snapshot: {
              agents: [
                {
                  agent_id: "identity:luka",
                  member_id: "identity:luka",
                  identity: "identity:luka",
                  label: "Identity Luka",
                  kind: "identity",
                  profile: "lead",
                  state: "running",
                  addressable: true,
                  affordances: { can_send_message: true },
                },
                {
                  agent_id: "legacy-router",
                  member_id: "legacy-router",
                  label: "Legacy Router",
                  kind: "module_agent",
                  profile: "router",
                  state: "running",
                  addressable: true,
                  affordances: { can_send_message: true },
                },
                ...(includeImageAgent ? [{
                  agent_id: "image-agent",
                  member_id: "image-agent",
                  identity: "image-agent",
                  label: "Image Agent",
                  kind: "identity",
                  profile: "coordinator",
                  state: "running",
                  addressable: true,
                  affordances: { can_send_message: true },
                  model_capabilities: { image_input: true },
                }] : []),
              ],
            },
          },
          identity_status: {
            panel_id: "console.identity_status",
            schema_version: "1",
            refresh: { mode: "poll", interval_ms: 5000 },
            rows: [
              {
                identity: "identity:luka",
                display_name: "Identity Luka",
                profile: "lead",
                state: "running",
                addressability: "addressable",
                labels: {},
              },
              ...(includeImageAgent ? [{
                identity: "image-agent",
                display_name: "Image Agent",
                profile: "coordinator",
                state: "running",
                addressability: "addressable",
                labels: {},
                model_capabilities: { image_input: true },
              }] : []),
            ],
          },
        }));
        return;
      }
      if (method === "POST" && url === "/console/rpc") {
        const payload = JSON.parse(body || "{}");
        const rpcId = payload.id || "rpc";
        if (payload.method === "mobkit/console/query_timeline") {
          const identity = payload.params?.identity;
          res.writeHead(200, { "content-type": "application/json" });
          res.end(JSON.stringify({
            jsonrpc: "2.0",
            id: rpcId,
            result: {
              frames: identity === "image-agent" ? timelineFrames : [],
              next_cursor: timelineFrames.length > 0 ? "console:image:3" : null,
            },
          }));
          return;
        }
        if (payload.method === "mobkit/console/inspect_identity") {
          res.writeHead(200, { "content-type": "application/json" });
          res.end(JSON.stringify({
            jsonrpc: "2.0",
            id: rpcId,
            result: { identity: payload.params.identity, peers: [] },
          }));
          return;
        }
        if (payload.method === "mobkit/console/send") {
          res.writeHead(200, { "content-type": "application/json" });
          res.end(JSON.stringify({
            jsonrpc: "2.0",
            id: rpcId,
            result: {
              accepted: true,
              interaction_id: `turn-${payload.params.identity || "unknown"}`,
              identity: payload.params.identity,
            },
          }));
          return;
        }
        if (payload.method === "mobkit/interact") {
          res.writeHead(200, { "content-type": "application/json" });
          res.end(JSON.stringify({
            jsonrpc: "2.0",
            id: rpcId,
            result: {
              interaction_id: "turn-identity-1",
              identity: payload.params.identity,
            },
          }));
          return;
        }
        if (payload.method === "mobkit/send_message") {
          res.writeHead(200, { "content-type": "application/json" });
          res.end(JSON.stringify({
            jsonrpc: "2.0",
            id: rpcId,
            result: {
              accepted: true,
              member_id: payload.params.member_id,
              session_id: "sess-legacy-1",
            },
          }));
          return;
        }
      }
      if (method === "POST" && url === "/console/rpc/multipart") {
        res.writeHead(200, { "content-type": "application/json" });
        res.end(JSON.stringify({
          jsonrpc: "2.0",
          id: "multipart-rpc",
          result: {
            accepted: true,
            interaction_id: "turn-image-upload-1",
            identity: "image-agent",
          },
        }));
        return;
      }
      if (method === "POST" && url === "/console/identity/stream") {
        res.writeHead(200, { "content-type": "text/event-stream" });
        res.end([
          "id: subscribed-identity-1",
          "event: subscribed",
          'data: {"event_id":"subscribed-identity-1","identity":"identity:luka","event_type":"subscribed","timestamp_ms":1,"data":{"stream":"identity"}}',
          "",
          "id: evt-identity-1",
          "event: interaction_complete",
          'data: {"event_id":"evt-identity-1","interaction_id":"turn-identity-1","identity":"identity:luka","event_type":"interaction_complete","timestamp_ms":2,"data":{"text":"identity done"}}',
          "",
        ].join("\n"));
        return;
      }
      if (method === "GET" && url.startsWith("/console/timeline/stream")) {
        res.writeHead(200, { "content-type": "text/event-stream" });
        res.end([
          "id: timeline-empty-1",
          "event: keep-alive",
          'data: {"frame_version":1,"id":"timeline-empty-1","kind":"keep-alive","timestamp_ms":1,"payload":{}}',
          "",
        ].join("\n"));
        return;
      }
      if (method === "POST" && url === "/interactions/stream") {
        res.writeHead(200, { "content-type": "text/event-stream" });
        res.end([
          "id: evt-legacy-1",
          "event: interaction_complete",
          'data: {"session_id":"sess-legacy-1","text":"legacy done"}',
          "",
        ].join("\n"));
        return;
      }

      res.writeHead(404, { "content-type": "text/plain; charset=utf-8" });
      res.end(`not found: ${method} ${url}`);
    });
  });

  return new Promise((resolve, reject) => {
    server.on("error", reject);
    server.listen(port, "127.0.0.1", () => {
      resolve({
        baseUrl,
        requests,
        close: () => new Promise((done, closeError) => {
          server.close((error) => error ? closeError(error) : done());
        }),
      });
    });
  });
}

async function runReferenceBrowserProof() {
  const port = await reservePort();
  const addr = `127.0.0.1:${port}`;
  const baseUrl = `http://${addr}`;
  const observedRequests = [];

  const backend = spawn(
    path.join(repoRoot, "scripts", "repo-cargo"),
    ["run", "-p", "meerkat-mobkit", "--example", "library_mode_reference"],
    {
      cwd: repoRoot,
      env: { ...process.env, MOBKIT_REF_ADDR: addr },
      stdio: ["ignore", "pipe", "pipe"],
    }
  );

  backend.stderr.on("data", (chunk) => {
    process.stderr.write(chunk);
  });

  let browser;
  try {
    await waitForHttpOk(`${baseUrl}/healthz`);
    await waitForHttpOk(`${baseUrl}/console`);

    browser = await launchBrowser();
    const page = await browser.newPage();
    page.on("request", (request) => {
      observedRequests.push({
        method: request.method(),
        url: request.url(),
        postData: request.postData() || "",
      });
    });
    await gotoConsole(page, `${baseUrl}/console`);

    // Wait for sidebar agent rows to appear
    await page.waitForSelector('.agent[role="button"], .cc-sidebar-row', { timeout: 30_000 });
    const labels = await sidebarLabels(page);
    assert(
      labels.some((label) => label.toLowerCase().includes("router")),
      `sidebar missing router: ${JSON.stringify(labels)}`
    );
    assert(
      labels.some((label) => label.toLowerCase().includes("delivery")),
      `sidebar missing delivery: ${JSON.stringify(labels)}`
    );

    // Verify dock and conversation pane rendered
    await page.waitForSelector(".pane, .cc-conversation-pane", { timeout: 10_000 });

    // Verify activity rail rendered
    await page.waitForSelector('[data-testid="signals-rail"], .cc-activity-rail', { timeout: 10_000 });

    // Send a message via the composer
    await openSidebarAgentChat(page, /router/i);
    await fillComposer(page, "browser proof message");
    await clickSend(page);
    await page.waitForTimeout(1_000);

    const usesConsoleSendLane = observedRequests.some(
      (request) =>
        request.url === `${baseUrl}/console/rpc` &&
        request.postData.includes('"method":"mobkit/console/send"'),
    ) && observedRequests.some(
      (request) =>
        request.method === "GET" && request.url.startsWith(`${baseUrl}/console/timeline/stream`),
    );

    assert(
      usesConsoleSendLane,
      `expected browser flow to use canonical console send/timeline lane; saw ${JSON.stringify(observedRequests, null, 2)}`,
    );

    process.stdout.write("browser e2e ok\n");
  } finally {
    if (browser) {
      await browser.close();
    }
    await stopBackend(backend);
  }
}

async function runCanonicalSendBrowserProof() {
  const port = await reservePort();
  const server = await startMockConsoleServer(port);
  let browser;

  try {
    browser = await launchBrowser();
    const page = await browser.newPage();
    await gotoConsole(page, `${server.baseUrl}/console`);

    await page.waitForSelector('.agent[role="button"], .cc-sidebar-row', { timeout: 30_000 });
    const labels = await sidebarLabels(page);
    assert(
      labels.some((label) => label.includes("Identity Luka")),
      `mock sidebar missing identity target: ${JSON.stringify(labels)}`,
    );
    assert(
      labels.some((label) => label.includes("Legacy Router")),
      `mock sidebar missing legacy target: ${JSON.stringify(labels)}`,
    );

    await openSidebarAgentChat(page, /Identity Luka/i);
    await fillComposer(page, "identity proof message");
    await clickSend(page);
    await page.waitForTimeout(100);

    await openSidebarAgentChat(page, /Legacy Router/i);
    await fillComposer(page, "legacy proof message");
    await clickSend(page);
    await page.waitForTimeout(100);

    const sawIdentityLane = server.requests.some(
      (request) =>
        request.url === "/console/rpc" &&
        request.body.includes('"method":"mobkit/console/send"') &&
        request.body.includes('"identity":"identity:luka"'),
    );
    const sawMemberLane = server.requests.some(
      (request) =>
        request.url === "/console/rpc" &&
        request.body.includes('"method":"mobkit/console/send"') &&
        request.body.includes('"identity":"legacy-router"'),
    );

    assert(
      sawIdentityLane,
      `expected mixed migration proof to use identity-native lane for the identity target; saw ${JSON.stringify(server.requests, null, 2)}`,
    );
    assert(
      sawMemberLane,
      `expected browser proof to use canonical console send lane for the member target; saw ${JSON.stringify(server.requests, null, 2)}`,
    );

    process.stdout.write("browser canonical send ok\n");
  } finally {
    if (browser) {
      await browser.close();
    }
    await server.close();
  }
}

async function runImageRenderingBrowserProof() {
  const port = await reservePort();
  const baseTs = Date.parse("2026-05-12T05:45:00.000Z");
  const server = await startMockConsoleServer(port, {
    includeImageAgent: true,
    timelineFrames: [
      {
        id: "img-user",
        kind: "user_input",
        identity: "image-agent",
        timestamp_ms: baseTs,
        cursor: "console:image:1",
        payload: {
          content: [
            { type: "text", text: "Operator attached image:" },
            {
              type: "image_ref",
              source: "blob",
              blob_id: "sha256:user-image",
              media_type: "image/png",
              alt: "operator forwarded image",
            },
          ],
        },
      },
      {
        id: "img-generated",
        kind: "assistant_image",
        identity: "image-agent",
        timestamp_ms: baseTs + 1000,
        cursor: "console:image:2",
        payload: {
          blob_id: "sha256:generated-image",
          media_type: "image/png",
          width: 1,
          height: 1,
        },
      },
      {
        id: "img-peer",
        kind: "interaction_complete",
        identity: "image-agent",
        timestamp_ms: baseTs + 2000,
        cursor: "console:image:3",
        source: { kind: "session_history" },
        payload: {
          message: {
            role: "system_notice",
            blocks: [{
              type: "comms",
              kind: "message",
              peer: { display_name: "incident-command-center/scribe/scribe" },
              request_id: "peer-img-1",
              content: [
                { type: "text", text: "Forwarded generated image." },
                {
                  type: "image_ref",
                  source: "blob",
                  blob_id: "sha256:peer-image",
                  media_type: "image/png",
                  alt: "peer forwarded generated image",
                },
              ],
            }],
          },
        },
      },
    ],
  });
  let browser;

  try {
    browser = await launchBrowser();
    const page = await browser.newPage();
    await gotoConsole(page, `${server.baseUrl}/console`);

    await page.waitForSelector('.agent[role="button"], .cc-sidebar-row', { timeout: 30_000 });
    await page.locator('.agent[role="button"], .cc-sidebar-row').filter({ hasText: "Image Agent" }).click();
    await page.waitForSelector('img.cc-rich-image', { timeout: 30_000 });
    await page.waitForFunction(() => document.querySelectorAll("img.cc-rich-image").length >= 3);

    const imageSources = await page.$$eval(
      "img.cc-rich-image",
      (images) => images.map((image) => image.getAttribute("src") || ""),
    );
    assert.equal(imageSources.length, 3);
    assert(
      imageSources.some((src) => src.endsWith("/blobs/sha256%3Auser-image")),
      `missing user image ref: ${JSON.stringify(imageSources)}`,
    );
    assert(
      imageSources.some((src) => src.endsWith("/blobs/sha256%3Agenerated-image")),
      `missing generated assistant image: ${JSON.stringify(imageSources)}`,
    );
    assert(
      imageSources.some((src) => src.endsWith("/blobs/sha256%3Apeer-image")),
      `missing peer forwarded image ref: ${JSON.stringify(imageSources)}`,
    );

    const bodyText = await page.locator("body").innerText();
    assert(bodyText.includes("Operator attached image:"), "missing user image prompt text");
    assert(bodyText.includes("Forwarded generated image."), "missing peer image comms text");
    assert(
      bodyText.includes("Received from scribe") || bodyText.includes("Received from incident-command-center/scribe/scribe"),
      "missing peer comms row",
    );
    assert(!bodyText.includes("image_ref"), "raw image_ref leaked into visible transcript");
    assert(!bodyText.includes("blob_id"), "raw blob_id leaked into visible transcript");

    process.stdout.write("browser image rendering ok\n");
  } finally {
    if (browser) {
      await browser.close();
    }
    await server.close();
  }
}

async function runComposerPasteAttachmentProof() {
  const port = await reservePort();
  const server = await startMockConsoleServer(port, { includeImageAgent: true });
  let browser;

  try {
    browser = await launchBrowser();
    const page = await browser.newPage();
    await gotoConsole(page, `${server.baseUrl}/console`);

    await page.waitForSelector('.agent[role="button"], .cc-sidebar-row', { timeout: 30_000 });
    await page.locator('.agent[role="button"], .cc-sidebar-row').filter({ hasText: "Image Agent" }).click();
    await page.waitForSelector('[data-testid="chat-composer:image-agent"]', { timeout: 30_000 });

    await page.evaluate(() => {
      const textarea = document.querySelector('[data-testid="chat-composer:image-agent"]');
      if (!textarea) throw new Error("missing image-agent composer");
      const bytes = Uint8Array.from([137, 80, 78, 71, 13, 10, 26, 10]);
      const file = new File([bytes], "pasted-badge.png", { type: "image/png" });
      const data = new DataTransfer();
      data.items.add(file);
      textarea.dispatchEvent(new ClipboardEvent("paste", {
        bubbles: true,
        cancelable: true,
        clipboardData: data,
      }));
    });

    await page.waitForSelector(".composer__attachment img", { timeout: 10_000 });
    const attachmentCount = await page.locator(".composer__attachment img").count();
    assert.equal(attachmentCount, 1);
    await fillComposer(page, "Describe the pasted badge.");
    await clickSend(page);
    await page.waitForTimeout(500);

    const multipartRequest = server.requests.find((request) => request.url === "/console/rpc/multipart");
    assert(multipartRequest, `expected image paste send to use multipart RPC; saw ${JSON.stringify(server.requests, null, 2)}`);
    assert(
      multipartRequest.body.includes("image_upload") && multipartRequest.body.includes("pasted-badge.png"),
      `multipart request missing image upload metadata: ${multipartRequest.body}`,
    );

    process.stdout.write("browser composer image paste ok\n");
  } finally {
    if (browser) {
      await browser.close();
    }
    await server.close();
  }
}

async function main() {
  await runReferenceBrowserProof();
  await runCanonicalSendBrowserProof();
  await runImageRenderingBrowserProof();
  await runComposerPasteAttachmentProof();
}

main().catch((error) => {
  process.stderr.write(`${error.stack || error.message}\n`);
  process.exit(1);
});
