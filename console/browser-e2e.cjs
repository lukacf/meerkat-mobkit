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
  const timelineStreamFrames = Array.isArray(options.timelineStreamFrames)
    ? options.timelineStreamFrames
    : [];
  const timelineFramesByIdentity =
    options.timelineFramesByIdentity && typeof options.timelineFramesByIdentity === "object"
      ? options.timelineFramesByIdentity
      : {};
  const timelineFramesAfterSendByIdentity =
    options.timelineFramesAfterSendByIdentity &&
    typeof options.timelineFramesAfterSendByIdentity === "object"
      ? options.timelineFramesAfterSendByIdentity
      : {};
  const timelineStreamFramesDuringSendByIdentity =
    options.timelineStreamFramesDuringSendByIdentity &&
    typeof options.timelineStreamFramesDuringSendByIdentity === "object"
      ? options.timelineStreamFramesDuringSendByIdentity
      : {};
  const streamClients = new Set();
  const includeImageAgent = options.includeImageAgent === true;
  const includeBusyWorker = options.includeBusyWorker === true;
  const includeToolOnlyWorker = options.includeToolOnlyWorker === true;
  const requests = [];

  function writeTimelineStreamFrame(res, frame, index) {
    res.write([
      `id: ${frame.id || `timeline-live-${index}`}`,
      `event: ${frame.kind || frame.event || "message"}`,
      `data: ${JSON.stringify({ type: "console_frame", frame })}`,
      "",
      "",
    ].join("\n"));
  }

  function emitDuringSendFrames(identity) {
    const frames =
      typeof identity === "string" && Array.isArray(timelineStreamFramesDuringSendByIdentity[identity])
        ? timelineStreamFramesDuringSendByIdentity[identity]
        : [];
    if (frames.length === 0 || streamClients.size === 0) return;
    let index = 0;
    for (const res of streamClients) {
      for (const frame of frames) {
        index += 1;
        writeTimelineStreamFrame(res, frame, `during-send-${index}`);
      }
    }
  }

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
            ...(includeBusyWorker ? [{
              identity: "person-worker-alpha",
              display_name: "Person Worker Alpha",
              profile: "person-worker",
              state: "running",
              addressability: "addressable",
            }] : []),
            ...(includeToolOnlyWorker ? [{
              identity: "tool-only-worker",
              display_name: "Tool Only Worker",
              profile: "investigation-worker",
              state: "running",
              addressability: "addressable",
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
                ...(includeToolOnlyWorker ? [{
                  agent_id: "tool-only-worker",
                  member_id: "tool-only-worker",
                  identity: "tool-only-worker",
                  label: "Tool Only Worker",
                  kind: "identity",
                  profile: "investigation-worker",
                  role: "investigation-worker",
                  group: "Workers",
                  state: "active",
                  addressable: true,
                  affordances: { can_send_message: true },
                }] : []),
                ...(includeBusyWorker ? [{
                  agent_id: "person-worker-alpha",
                  member_id: "person-worker-alpha",
                  identity: "person-worker-alpha",
                  label: "Person Worker Alpha",
                  kind: "identity",
                  profile: "person-worker",
                  role: "person-worker",
                  group: "Workers",
                  state: "active",
                  addressable: true,
                  response_phase: "tool-executing",
                  affordances: { can_send_message: true },
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
              ...(includeBusyWorker ? [{
                identity: "person-worker-alpha",
                display_name: "Person Worker Alpha",
                profile: "person-worker",
                state: "active",
                response_phase: "tool-executing",
                addressability: "addressable",
                labels: {},
              }] : []),
              ...(includeToolOnlyWorker ? [{
                identity: "tool-only-worker",
                display_name: "Tool Only Worker",
                profile: "investigation-worker",
                state: "active",
                addressability: "addressable",
                labels: {},
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
          const framesForIdentity =
            typeof identity === "string" && Array.isArray(timelineFramesByIdentity[identity])
              ? timelineFramesByIdentity[identity]
              : null;
          const frames = framesForIdentity || (identity === "image-agent" ? timelineFrames : []);
          res.writeHead(200, { "content-type": "application/json" });
          res.end(JSON.stringify({
            jsonrpc: "2.0",
            id: rpcId,
            result: {
              frames,
              next_cursor: frames.length > 0 ? frames[frames.length - 1].cursor || null : null,
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
          const identity = payload.params?.identity;
          if (
            typeof identity === "string" &&
            Array.isArray(timelineFramesAfterSendByIdentity[identity])
          ) {
            const existing = Array.isArray(timelineFramesByIdentity[identity])
              ? timelineFramesByIdentity[identity]
              : [];
            timelineFramesByIdentity[identity] = [
              ...existing,
              ...timelineFramesAfterSendByIdentity[identity],
            ];
          }
          emitDuringSendFrames(identity);
          const sendResponse = JSON.stringify({
            jsonrpc: "2.0",
            id: rpcId,
            result: {
              accepted: true,
              interaction_id: `turn-${identity || "unknown"}`,
              identity,
            },
          });
          const writeSendResponse = () => {
            res.writeHead(200, { "content-type": "application/json" });
            res.end(sendResponse);
          };
          if (options.consoleSendResponseDelayMs) {
            setTimeout(writeSendResponse, options.consoleSendResponseDelayMs);
          } else {
            writeSendResponse();
          }
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
        streamClients.add(res);
        res.on("close", () => {
          streamClients.delete(res);
        });
        if (Object.keys(timelineStreamFramesDuringSendByIdentity).length > 0) {
          res.write([
            "id: timeline-ready-1",
            "event: keep-alive",
            'data: {"frame_version":1,"id":"timeline-ready-1","kind":"keep-alive","timestamp_ms":1,"payload":{}}',
            "",
            "",
          ].join("\n"));
          return;
        }
        if (timelineStreamFrames.length === 0) {
          res.end([
            "id: timeline-empty-1",
            "event: keep-alive",
            'data: {"frame_version":1,"id":"timeline-empty-1","kind":"keep-alive","timestamp_ms":1,"payload":{}}',
            "",
          ].join("\n"));
          return;
        }
        let index = 0;
        const writeNext = () => {
          if (index >= timelineStreamFrames.length) {
            res.end();
            return;
          }
          const frame = timelineStreamFrames[index];
          index += 1;
          writeTimelineStreamFrame(res, frame, index);
          setTimeout(writeNext, frame.delay_ms || 50);
        };
        setTimeout(writeNext, options.timelineStreamInitialDelayMs || 150);
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

async function runBusyWorkerConsoleProof() {
  const port = await reservePort();
  const baseTs = Date.parse("2026-05-23T20:29:50.000Z");
  const server = await startMockConsoleServer(port, {
    includeBusyWorker: true,
    timelineFramesByIdentity: {
      "person-worker-alpha": [
        {
          id: "worker-tool-first-in-cursor-order",
          kind: "tool_call_requested",
          identity: "person-worker-alpha",
          timestamp_ms: baseTs + 1000,
          cursor: "console:worker:10",
          payload: {
            id: "call-king-search-1",
            name: "king_search",
            arguments: { query: "Andreas Holmen" },
          },
        },
        {
          id: "worker-run-started-backfilled-late",
          kind: "run_started",
          identity: "person-worker-alpha",
          timestamp_ms: baseTs,
          cursor: "console:worker:11",
          payload: {
            prompt: "Parent audit request: inspect Andreas Holmen for CTO Integration.",
          },
        },
        {
          id: "worker-tool-done-no-terminal-yet",
          kind: "tool_execution_completed",
          identity: "person-worker-alpha",
          timestamp_ms: baseTs + 2000,
          cursor: "console:worker:12",
          payload: {
            id: "call-king-search-1",
            name: "king_search",
            result: "ok",
          },
        },
      ],
    },
  });
  let browser;

  try {
    browser = await launchBrowser();
    const page = await browser.newPage();
    await gotoConsole(page, `${server.baseUrl}/console`);

    await page.waitForSelector('.agent[role="button"], .cc-sidebar-row', { timeout: 30_000 });
    await page.locator('.agent[role="button"], .cc-sidebar-row').filter({ hasText: "Person Worker Alpha" }).click();
    await page.waitForSelector('[data-testid="chat-pane:person-worker-alpha"]', { timeout: 30_000 });

    const pane = page.locator('[data-testid="chat-pane:person-worker-alpha"]');
    await pane.getByText("Parent audit request: inspect Andreas Holmen for CTO Integration.").waitFor({ timeout: 10_000 });
    await pane.getByText("king_search").first().waitFor({ timeout: 10_000 });
    await page.waitForSelector('[data-testid="chat-typing:person-worker-alpha"]', { timeout: 10_000 });

    const bodyText = await pane.innerText();
    const promptIndex = bodyText.indexOf("Parent audit request: inspect Andreas Holmen for CTO Integration.");
    const toolIndex = bodyText.indexOf("king_search");
    assert(promptIndex >= 0, `missing parent prompt in worker transcript: ${bodyText}`);
    assert(toolIndex >= 0, `missing tool card in worker transcript: ${bodyText}`);
    assert(
      promptIndex < toolIndex,
      `parent prompt must render before tool activity even when backfilled late: ${bodyText}`,
    );
    assert(bodyText.includes("working"), `busy worker must show working indicator: ${bodyText}`);

    await page.locator('[data-testid="chat-composer:person-worker-alpha"]').fill("Please prioritize this audit note.");
    await page.locator('[data-testid="chat-send:person-worker-alpha"]').click();
    await page.waitForSelector('[data-testid="pending-stack"]', { timeout: 10_000 });
    await pane.getByText("Please prioritize this audit note.").waitFor({ timeout: 10_000 });
    await page.waitForSelector('[data-testid^="pending-steer:"]', { timeout: 10_000 });
    await page.locator('[data-testid^="pending-steer:"]').first().click();
    await page.waitForTimeout(500);

    const steerRequest = server.requests.find((request) =>
      request.url === "/console/rpc" &&
      request.body.includes('"method":"mobkit/console/send"') &&
      request.body.includes('"identity":"person-worker-alpha"') &&
      request.body.includes('"handling_mode":"steer"') &&
      request.body.includes("Please prioritize this audit note.")
    );
    assert(
      steerRequest,
      `queued busy-worker draft was not promoted through canonical steer send; saw ${JSON.stringify(server.requests, null, 2)}`,
    );

    process.stdout.write("browser busy worker queue/steer ok\n");
  } finally {
    if (browser) {
      await browser.close();
    }
    await server.close();
  }
}

async function runToolOnlyWorkerBusyQueueProof() {
  const port = await reservePort();
  const baseTs = Date.parse("2026-05-23T20:50:00.000Z");
  const queuedText = "Queue me while the tool-only worker is busy.";
  const server = await startMockConsoleServer(port, {
    includeToolOnlyWorker: true,
    timelineFramesByIdentity: {
      "tool-only-worker": [
        {
          id: "tool-only-parent-handoff",
          kind: "user_input",
          identity: "tool-only-worker",
          timestamp_ms: baseTs,
          cursor: "console:tool-only:1",
          payload: {
            content: "Parent handoff: investigate Daily Candy build with Bazel.",
          },
        },
        {
          id: "tool-only-tool-call",
          kind: "tool_call_requested",
          identity: "tool-only-worker",
          timestamp_ms: baseTs + 1_000,
          cursor: "console:tool-only:2",
          payload: {
            id: "call-king-search-1",
            name: "king_search",
            arguments: { query: "Daily Candy build with Bazel" },
          },
        },
        {
          id: "tool-only-tool-done",
          kind: "tool_execution_completed",
          identity: "tool-only-worker",
          timestamp_ms: baseTs + 2_000,
          cursor: "console:tool-only:3",
          payload: {
            id: "call-king-search-1",
            name: "king_search",
            result: "ok",
          },
        },
        {
          id: "tool-only-reasoning",
          kind: "reasoning_delta",
          identity: "tool-only-worker",
          timestamp_ms: baseTs + 3_000,
          cursor: "console:tool-only:4",
          payload: {
            delta: "Reviewing search results before the next tool call.",
          },
        },
      ],
    },
  });
  let browser;

  try {
    browser = await launchBrowser();
    const page = await browser.newPage();
    await gotoConsole(page, `${server.baseUrl}/console`);

    await page.waitForSelector('.agent[role="button"], .cc-sidebar-row', { timeout: 30_000 });
    await page.locator('.agent[role="button"], .cc-sidebar-row').filter({ hasText: "Tool Only Worker" }).click();
    await page.waitForSelector('[data-testid="chat-pane:tool-only-worker"]', { timeout: 30_000 });

    const pane = page.locator('[data-testid="chat-pane:tool-only-worker"]');
    await pane.getByText("Parent handoff: investigate Daily Candy build with Bazel.").waitFor({ timeout: 10_000 });
    await pane.getByText("king_search").first().waitFor({ timeout: 10_000 });
    await page.waitForSelector('[data-testid="chat-typing:tool-only-worker"]', { timeout: 10_000 });

    const beforeSendText = await pane.innerText();
    assert(
      beforeSendText.includes("working"),
      `tool-only active turn must show working indicator without run_started/response_phase: ${beforeSendText}`,
    );

    await page.locator('[data-testid="chat-composer:tool-only-worker"]').fill(queuedText);
    await page.locator('[data-testid="chat-send:tool-only-worker"]').click();
    await page.waitForSelector('[data-testid="pending-stack"]', { timeout: 10_000 });
    await page.waitForSelector('[data-testid^="pending-steer:"]', { timeout: 10_000 });
    await pane.getByText(queuedText).waitFor({ timeout: 10_000 });

    const directSend = server.requests.find((request) =>
      request.url === "/console/rpc" &&
      request.body.includes('"method":"mobkit/console/send"') &&
      request.body.includes('"identity":"tool-only-worker"') &&
      request.body.includes(queuedText)
    );
    assert(
      !directSend,
      `busy tool-only worker accepted draft immediately instead of queueing it: ${JSON.stringify(server.requests, null, 2)}`,
    );

    await page.locator('[data-testid^="pending-steer:"]').first().click();
    await page.waitForTimeout(500);

    const steerRequest = server.requests.find((request) =>
      request.url === "/console/rpc" &&
      request.body.includes('"method":"mobkit/console/send"') &&
      request.body.includes('"identity":"tool-only-worker"') &&
      request.body.includes('"handling_mode":"steer"') &&
      request.body.includes(queuedText)
    );
    assert(
      steerRequest,
      `queued tool-only worker draft was not promoted through canonical steer send; saw ${JSON.stringify(server.requests, null, 2)}`,
    );

    process.stdout.write("browser tool-only worker busy queue ok\n");
  } finally {
    if (browser) {
      await browser.close();
    }
    await server.close();
  }
}

async function runToolOnlyWorkerTerminalClearsBusyProof() {
  const port = await reservePort();
  const baseTs = Date.parse("2026-05-23T20:55:00.000Z");
  const sendText = "This should send immediately after terminal turn_completed.";
  const server = await startMockConsoleServer(port, {
    includeToolOnlyWorker: true,
    timelineFramesByIdentity: {
      "tool-only-worker": [
        {
          id: "terminal-tool-only-parent-handoff",
          kind: "user_input",
          identity: "tool-only-worker",
          timestamp_ms: baseTs,
          cursor: "console:tool-only-terminal:1",
          payload: {
            content: "Parent handoff: run one short terminal tool-only check.",
          },
        },
        {
          id: "terminal-tool-only-tool-call",
          kind: "tool_call_requested",
          identity: "tool-only-worker",
          timestamp_ms: baseTs + 1_000,
          cursor: "console:tool-only-terminal:2",
          payload: {
            id: "call-king-search-2",
            name: "king_search",
            arguments: { query: "terminal tool-only check" },
          },
        },
        {
          id: "terminal-tool-only-tool-done",
          kind: "tool_execution_completed",
          identity: "tool-only-worker",
          timestamp_ms: baseTs + 2_000,
          cursor: "console:tool-only-terminal:3",
          payload: {
            id: "call-king-search-2",
            name: "king_search",
            result: "ok",
          },
        },
        {
          id: "terminal-tool-only-turn-completed",
          kind: "turn_completed",
          identity: "tool-only-worker",
          timestamp_ms: baseTs + 3_000,
          cursor: "console:tool-only-terminal:4",
          payload: {
            stop_reason: "end_turn",
          },
        },
      ],
    },
  });
  let browser;

  try {
    browser = await launchBrowser();
    const page = await browser.newPage();
    await gotoConsole(page, `${server.baseUrl}/console`);

    await page.waitForSelector('.agent[role="button"], .cc-sidebar-row', { timeout: 30_000 });
    await page.locator('.agent[role="button"], .cc-sidebar-row').filter({ hasText: "Tool Only Worker" }).click();
    await page.waitForSelector('[data-testid="chat-pane:tool-only-worker"]', { timeout: 30_000 });

    const pane = page.locator('[data-testid="chat-pane:tool-only-worker"]');
    await pane.getByText("Parent handoff: run one short terminal tool-only check.").waitFor({ timeout: 10_000 });
    await pane.getByText("king_search").first().waitFor({ timeout: 10_000 });
    await page.waitForTimeout(500);

    const terminalText = await pane.innerText();
    assert(
      !terminalText.includes("working"),
      `terminal tool-only turn must clear visible working indicator: ${terminalText}`,
    );
    assert.equal(
      await page.locator('[data-testid="chat-typing:tool-only-worker"]').count(),
      0,
      "terminal turn_completed must remove typing indicator",
    );

    await page.locator('[data-testid="chat-composer:tool-only-worker"]').fill(sendText);
    await page.locator('[data-testid="chat-send:tool-only-worker"]').click();
    await page.waitForTimeout(500);

    assert.equal(
      await page.locator('[data-testid="pending-stack"]').count(),
      0,
      "terminal turn_completed must clear hidden busy state so drafts do not queue",
    );

    const directSend = server.requests.find((request) =>
      request.url === "/console/rpc" &&
      request.body.includes('"method":"mobkit/console/send"') &&
      request.body.includes('"identity":"tool-only-worker"') &&
      request.body.includes(sendText)
    );
    assert(
      directSend,
      `idle terminal tool-only worker did not send directly; saw ${JSON.stringify(server.requests, null, 2)}`,
    );
    assert(
      !directSend.body.includes('"handling_mode":"steer"'),
      `idle terminal tool-only worker incorrectly sent as steer: ${directSend.body}`,
    );

    process.stdout.write("browser tool-only terminal clears busy ok\n");
  } finally {
    if (browser) {
      await browser.close();
    }
    await server.close();
  }
}

async function runNonCommsSystemNoticeDoesNotClearBusyProof() {
  const port = await reservePort();
  const baseTs = Date.parse("2026-05-23T20:57:00.000Z");
  const queuedText = "Queue me after runtime metadata while the worker is still busy.";
  const server = await startMockConsoleServer(port, {
    includeToolOnlyWorker: true,
    timelineFramesByIdentity: {
      "tool-only-worker": [
        {
          id: "notice-tool-only-parent-handoff",
          kind: "user_input",
          identity: "tool-only-worker",
          timestamp_ms: baseTs,
          cursor: "console:tool-only-notice:1",
          payload: {
            content: "Parent handoff: keep running after metadata notice.",
          },
        },
        {
          id: "notice-tool-only-tool-call",
          kind: "tool_call_requested",
          identity: "tool-only-worker",
          timestamp_ms: baseTs + 1_000,
          cursor: "console:tool-only-notice:2",
          payload: {
            id: "call-king-search-3",
            name: "king_search",
            arguments: { query: "runtime metadata notice check" },
          },
        },
        {
          id: "notice-tool-only-tool-done",
          kind: "tool_execution_completed",
          identity: "tool-only-worker",
          timestamp_ms: baseTs + 2_000,
          cursor: "console:tool-only-notice:3",
          payload: {
            id: "call-king-search-3",
            name: "king_search",
            result: "ok",
          },
        },
      ],
    },
    timelineStreamFrames: [
      {
        id: "notice-tool-only-runtime-notice",
        kind: "system_notice",
        identity: "tool-only-worker",
        timestamp_ms: baseTs + 3_000,
        cursor: "console:tool-only-notice:4",
        payload: {
          message: {
            role: "system_notice",
            kind: "generic",
            body: "Runtime recovered from transient stream lag",
            blocks: [{
              type: "runtime_notice",
              category: "stream",
              detail: "Runtime recovered from transient stream lag",
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
    await page.locator('.agent[role="button"], .cc-sidebar-row').filter({ hasText: "Tool Only Worker" }).click();
    await page.waitForSelector('[data-testid="chat-pane:tool-only-worker"]', { timeout: 30_000 });

    const pane = page.locator('[data-testid="chat-pane:tool-only-worker"]');
    await pane.getByText("Parent handoff: keep running after metadata notice.").waitFor({ timeout: 10_000 });
    await pane.getByText("king_search").first().waitFor({ timeout: 10_000 });
    await pane.getByText("Runtime recovered from transient stream lag").waitFor({ timeout: 10_000 });
    await page.waitForSelector('[data-testid="chat-typing:tool-only-worker"]', { timeout: 10_000 });

    const noticeText = await pane.innerText();
    assert(
      noticeText.includes("working"),
      `non-comms system_notice must not clear busy state during an active turn: ${noticeText}`,
    );

    await page.locator('[data-testid="chat-composer:tool-only-worker"]').fill(queuedText);
    await page.locator('[data-testid="chat-send:tool-only-worker"]').click();
    await page.waitForSelector('[data-testid="pending-stack"]', { timeout: 10_000 });
    await page.waitForSelector('[data-testid^="pending-steer:"]', { timeout: 10_000 });

    const directSend = server.requests.find((request) =>
      request.url === "/console/rpc" &&
      request.body.includes('"method":"mobkit/console/send"') &&
      request.body.includes('"identity":"tool-only-worker"') &&
      request.body.includes(queuedText)
    );
    assert(
      !directSend,
      `non-comms system_notice cleared hidden busy state and sent immediately: ${JSON.stringify(server.requests, null, 2)}`,
    );

    process.stdout.write("browser non-comms system_notice stays busy ok\n");
  } finally {
    if (browser) {
      await browser.close();
    }
    await server.close();
  }
}

async function runChatPaneAutoScrollProof() {
  const port = await reservePort();
  const baseTs = Date.parse("2026-05-23T20:40:00.000Z");
  const historyFrames = Array.from({ length: 48 }, (_, index) => ({
    id: `history-line-${index}`,
    kind: "interaction_complete",
    identity: "person-worker-alpha",
    interaction_id: `history-turn-${index}`,
    timestamp_ms: baseTs + index * 1_000,
    cursor: `console:auto:${index}`,
    payload: {
      text: `Historical worker line ${index + 1}: enough transcript content to require scrolling.`,
    },
  }));
  const growingFrame = {
    id: "growing-answer",
    kind: "interaction_complete",
    identity: "person-worker-alpha",
    interaction_id: "growing-answer-turn",
    timestamp_ms: baseTs + 55_000,
    cursor: "console:auto:growing",
    payload: {
      text: "Live auto-scroll proof begins.",
    },
  };
  const liveUpdates = [
    {
      id: "live-autoscroll-frame-update",
      kind: "frame_updated",
      identity: "person-worker-alpha",
      timestamp_ms: baseTs + 60_000,
      cursor: "console:auto:live:update",
      payload: {
        frame: {
          ...growingFrame,
          payload: {
            text:
              "Live auto-scroll proof begins. The chat pane should remain stuck to the bottom while this same assistant message grows. AUTO_SCROLL_FINAL_VISIBLE",
          },
        },
      },
    },
  ];
  const server = await startMockConsoleServer(port, {
    includeBusyWorker: true,
    timelineFramesByIdentity: {
      "person-worker-alpha": [...historyFrames, growingFrame],
    },
    timelineStreamFrames: liveUpdates,
    timelineStreamInitialDelayMs: 2_000,
  });
  let browser;

  try {
    browser = await launchBrowser();
    const page = await browser.newPage();
    await gotoConsole(page, `${server.baseUrl}/console`);

    await page.waitForSelector('.agent[role="button"], .cc-sidebar-row', { timeout: 30_000 });
    await page.locator('.agent[role="button"], .cc-sidebar-row').filter({ hasText: "Person Worker Alpha" }).click();
    const pane = page.locator('[data-testid="chat-pane:person-worker-alpha"]');
    await pane.getByText("Historical worker line 48").waitFor({ timeout: 10_000 });
    await pane.getByText("AUTO_SCROLL_FINAL_VISIBLE").waitFor({ timeout: 10_000 });
    const body = pane.locator(".conv__body");
    const scrollState = await body.evaluate((node) => ({
      scrollTop: node.scrollTop,
      clientHeight: node.clientHeight,
      scrollHeight: node.scrollHeight,
      text: node.textContent || "",
    }));
    const distanceFromBottom =
      scrollState.scrollHeight - scrollState.clientHeight - scrollState.scrollTop;
    assert(
      distanceFromBottom <= 4,
      `chat pane did not stay pinned to latest transcript content; distance=${distanceFromBottom} state=${JSON.stringify(scrollState)}`,
    );

    process.stdout.write("browser chat pane auto-scroll ok\n");
  } finally {
    if (browser) {
      await browser.close();
    }
    await server.close();
  }
}

async function runRunStartedClearsOptimisticPromptProof() {
  const port = await reservePort();
  const prompt = "ORDER_PROOF send this once and keep the transcript chronological.";
  const baseTs = Date.parse("2026-05-23T20:45:00.000Z");
  const server = await startMockConsoleServer(port, {
    // Reproduce the send/SSE race: run_started arrives on the live stream
    // while the console send RPC is still in flight, before the optimistic
    // entry has the interaction id from the response.
    consoleSendResponseDelayMs: 400,
    timelineStreamFramesDuringSendByIdentity: {
      "identity:luka": [
        {
          id: "order-proof-run-started",
          kind: "run_started",
          identity: "identity:luka",
          interaction_id: "turn-identity:luka",
          timestamp_ms: baseTs,
          cursor: "console:order:1",
          payload: { prompt },
        },
      ],
    },
    timelineFramesAfterSendByIdentity: {
      "identity:luka": [
        {
          id: "order-proof-complete",
          kind: "interaction_complete",
          identity: "identity:luka",
          interaction_id: "turn-identity:luka",
          timestamp_ms: baseTs + 2_000,
          cursor: "console:order:2",
          payload: { text: "ORDER_PROOF_FINAL visible after the prompt." },
        },
      ],
    },
  });
  let browser;

  try {
    browser = await launchBrowser();
    const page = await browser.newPage();
    await gotoConsole(page, `${server.baseUrl}/console`);

    await openSidebarAgentChat(page, "Identity Luka");
    await page.waitForSelector('[data-testid="chat-pane:identity:luka"]', { timeout: 30_000 });
    for (let attempt = 0; attempt < 40; attempt += 1) {
      if (server.requests.some((request) => request.url.startsWith("/console/timeline/stream"))) {
        break;
      }
      await sleep(50);
    }
    assert(
      server.requests.some((request) => request.url.startsWith("/console/timeline/stream")),
      `timeline stream was not connected before send: ${JSON.stringify(server.requests, null, 2)}`,
    );
    await fillComposer(page, prompt);
    await clickSend(page);

    const pane = page.locator('[data-testid="chat-pane:identity:luka"]');
    await pane.getByText("ORDER_PROOF_FINAL").waitFor({ timeout: 10_000 });
    await page.waitForTimeout(600);
    const bodyText = await pane.innerText();
    const promptMatches = bodyText.match(/ORDER_PROOF send this once/g) || [];
    assert.equal(
      promptMatches.length,
      1,
      `run_started should replace the optimistic prompt instead of leaving a duplicate tail prompt: ${bodyText}`,
    );
    const promptIndex = bodyText.indexOf(prompt);
    const finalIndex = bodyText.indexOf("ORDER_PROOF_FINAL");
    assert(promptIndex >= 0, `missing run_started prompt: ${bodyText}`);
    assert(finalIndex >= 0, `missing final response: ${bodyText}`);
    assert(
      promptIndex < finalIndex,
      `operator prompt must render before the final response: ${bodyText}`,
    );

    process.stdout.write("browser run_started optimistic cleanup ok\n");
  } finally {
    if (browser) {
      await browser.close();
    }
    await server.close();
  }
}

async function runUserInputEchoClearsOptimisticPromptProof() {
  const port = await reservePort();
  const prompt = "USER_INPUT_ECHO_PROOF should render once after the send race.";
  const baseTs = Date.parse("2026-05-23T21:20:00.000Z");
  const server = await startMockConsoleServer(port, {
    // Reproduce the send/SSE race for the canonical console user_input
    // echo. This is distinct from run_started: the echoed frame already has
    // an interaction id, but the optimistic entry may not yet have received
    // that id from the still-in-flight send RPC.
    consoleSendResponseDelayMs: 400,
    timelineStreamFramesDuringSendByIdentity: {
      "identity:luka": [
        {
          id: "user-input-echo-proof",
          kind: "user_input",
          identity: "identity:luka",
          interaction_id: "turn-identity:luka",
          timestamp_ms: baseTs,
          cursor: "console:user-input-echo:1",
          payload: {
            content: prompt,
            handling_mode: "queue",
            origin: "console:panel-race",
          },
          status: "delivered",
        },
      ],
    },
    timelineFramesAfterSendByIdentity: {
      "identity:luka": [
        {
          id: "user-input-echo-complete",
          kind: "interaction_complete",
          identity: "identity:luka",
          interaction_id: "turn-identity:luka",
          timestamp_ms: baseTs + 2_000,
          cursor: "console:user-input-echo:2",
          payload: { text: "USER_INPUT_ECHO_FINAL visible after one prompt." },
        },
      ],
    },
  });
  let browser;

  try {
    browser = await launchBrowser();
    const page = await browser.newPage();
    await gotoConsole(page, `${server.baseUrl}/console`);

    await openSidebarAgentChat(page, "Identity Luka");
    await page.waitForSelector('[data-testid="chat-pane:identity:luka"]', { timeout: 30_000 });
    for (let attempt = 0; attempt < 40; attempt += 1) {
      if (server.requests.some((request) => request.url.startsWith("/console/timeline/stream"))) {
        break;
      }
      await sleep(50);
    }
    assert(
      server.requests.some((request) => request.url.startsWith("/console/timeline/stream")),
      `timeline stream was not connected before send: ${JSON.stringify(server.requests, null, 2)}`,
    );
    await fillComposer(page, prompt);
    await clickSend(page);

    const pane = page.locator('[data-testid="chat-pane:identity:luka"]');
    await pane.getByText("USER_INPUT_ECHO_FINAL").waitFor({ timeout: 10_000 });
    await page.waitForTimeout(600);
    const bodyText = await pane.innerText();
    const promptMatches = bodyText.match(/USER_INPUT_ECHO_PROOF/g) || [];
    assert.equal(
      promptMatches.length,
      1,
      `user_input echo should replace the optimistic prompt instead of duplicating it: ${bodyText}`,
    );

    process.stdout.write("browser user_input optimistic cleanup ok\n");
  } finally {
    if (browser) {
      await browser.close();
    }
    await server.close();
  }
}

async function runLiveSystemNoticeAppearsInOpenChatProof() {
  const port = await reservePort();
  const baseTs = Date.parse("2026-05-23T21:10:00.000Z");
  const server = await startMockConsoleServer(port, {
    timelineFramesByIdentity: {
      "identity:luka": [
        {
          id: "live-peer-prior-tool-completed",
          kind: "tool_execution_completed",
          identity: "identity:luka",
          timestamp_ms: baseTs - 1_000,
          cursor: "console:peer-live:0",
          payload: {
            id: "call-send-message",
            name: "send_message",
            result: "sent",
          },
        },
      ],
    },
    timelineStreamFrames: [
      {
        id: "live-peer-system-notice",
        kind: "system_notice",
        identity: "identity:luka",
        timestamp_ms: baseTs,
        cursor: "console:peer-live:1",
        payload: {
          blocks: [{
            content: [{
              type: "text",
              text: "Peer message from ob3/investigation-worker/investigation-worker-live-proof:\nLIVE_PEER_NOTICE landed in the parent chat.",
            }],
          }],
        },
      },
    ],
  });
  let browser;

  try {
    browser = await launchBrowser();
    const page = await browser.newPage();
    await gotoConsole(page, `${server.baseUrl}/console`);

    await openSidebarAgentChat(page, "Identity Luka");
    const pane = page.locator('[data-testid="chat-pane:identity:luka"]');
    await pane.getByText("LIVE_PEER_NOTICE landed in the parent chat").waitFor({
      timeout: 10_000,
    });
    const bodyText = await pane.innerText();
    assert(
      bodyText.includes("Received from") || bodyText.includes("LIVE_PEER_NOTICE"),
      `live system_notice must route into the open chat pane, not only Signals: ${bodyText}`,
    );
    assert(
      !(await pane.locator('[data-testid="chat-typing:identity:luka"]').count()),
      `live system_notice should clear stale tool-completed busy state: ${bodyText}`,
    );
    assert(
      !bodyText.includes("working"),
      `live system_notice should not leave the parent pane visibly busy: ${bodyText}`,
    );

    process.stdout.write("browser live system_notice chat routing ok\n");
  } finally {
    if (browser) {
      await browser.close();
    }
    await server.close();
  }
}

async function runConsoleMountsWithoutCryptoRandomUuidProof() {
  const port = await reservePort();
  const server = await startMockConsoleServer(port);
  let browser;

  try {
    browser = await launchBrowser();
    const page = await browser.newPage();
    const errors = [];
    page.on("pageerror", (error) => errors.push(error.message || String(error)));
    await page.addInitScript(() => {
      const cryptoObject = globalThis.crypto;
      if (!cryptoObject) return;
      try {
        Object.defineProperty(cryptoObject, "randomUUID", {
          configurable: true,
          value: undefined,
        });
      } catch (_) {
        try {
          delete cryptoObject.randomUUID;
        } catch (_) {
          // Best effort: older browsers may expose a non-configurable property.
        }
      }
    });

    await gotoConsole(page, `${server.baseUrl}/console`);
    await page.locator('[data-testid="meerkat-console"]').waitFor({ timeout: 10_000 });
    const labels = await sidebarLabels(page);
    assert(
      labels.some((label) => label.includes("Identity Luka")),
      `console should mount and render agents without crypto.randomUUID; labels=${JSON.stringify(labels)}`,
    );
    assert.equal(
      errors.filter((message) => message.includes("randomUUID")).length,
      0,
      `console should not throw randomUUID page errors: ${JSON.stringify(errors)}`,
    );

    process.stdout.write("browser missing crypto.randomUUID mount ok\n");
  } finally {
    if (browser) {
      await browser.close();
    }
    await server.close();
  }
}

async function main() {
  const repoCargo = path.join(repoRoot, "scripts", "repo-cargo");
  if (fs.existsSync(repoCargo)) {
    await runReferenceBrowserProof();
  } else {
    process.stdout.write("browser reference proof skipped: MobKit workspace scripts unavailable in vendored package\n");
  }
  await runCanonicalSendBrowserProof();
  await runImageRenderingBrowserProof();
  await runComposerPasteAttachmentProof();
  await runBusyWorkerConsoleProof();
  await runToolOnlyWorkerBusyQueueProof();
  await runToolOnlyWorkerTerminalClearsBusyProof();
  await runNonCommsSystemNoticeDoesNotClearBusyProof();
  await runChatPaneAutoScrollProof();
  await runRunStartedClearsOptimisticPromptProof();
  await runUserInputEchoClearsOptimisticPromptProof();
  await runLiveSystemNoticeAppearsInOpenChatProof();
  await runConsoleMountsWithoutCryptoRandomUuidProof();
}

main().catch((error) => {
  process.stderr.write(`${error.stack || error.message}\n`);
  process.exit(1);
});
