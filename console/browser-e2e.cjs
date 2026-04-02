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

function startMockConsoleServer(port) {
  const baseUrl = `http://127.0.0.1:${port}`;
  const html = fs.readFileSync(path.join(__dirname, "dist", "index.html"), "utf8");
  const js = fs.readFileSync(path.join(__dirname, "dist", "console-app.js"), "utf8");
  const css = fs.readFileSync(path.join(__dirname, "dist", "console-app.css"), "utf8");
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
      if (method === "GET" && url === "/console/assets/console-app.js") {
        res.writeHead(200, { "content-type": "application/javascript; charset=utf-8" });
        res.end(js);
        return;
      }
      if (method === "GET" && url === "/console/assets/console-app.css") {
        res.writeHead(200, { "content-type": "text/css; charset=utf-8" });
        res.end(css);
        return;
      }
      if (method === "GET" && url === "/console/modules") {
        res.writeHead(200, { "content-type": "application/json" });
        res.end(JSON.stringify({ modules: [] }));
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
            ],
          },
        }));
        return;
      }
      if (method === "POST" && url === "/console/rpc") {
        const payload = JSON.parse(body || "{}");
        const rpcId = payload.id || "rpc";
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
    await page.goto(`${baseUrl}/console`, { waitUntil: "networkidle" });

    // Wait for sidebar agent rows to appear
    await page.waitForSelector(".cc-sidebar-row", { timeout: 30_000 });
    const sidebarLabels = await page.$$eval(".cc-sidebar-row", (rows) =>
      rows.map((row) => row.textContent.trim())
    );
    assert(
      sidebarLabels.some((label) => label.includes("router")),
      `sidebar missing router: ${JSON.stringify(sidebarLabels)}`
    );
    assert(
      sidebarLabels.some((label) => label.includes("delivery")),
      `sidebar missing delivery: ${JSON.stringify(sidebarLabels)}`
    );

    // Verify dock and conversation pane rendered
    await page.waitForSelector(".cc-conversation-pane", { timeout: 10_000 });

    // Verify activity rail rendered
    await page.waitForSelector(".cc-activity-rail", { timeout: 10_000 });

    // Send a message via the composer
    await page.fill(".cc-composer__textarea", "browser proof message");
    await page.click(".cc-composer__send-btn");

    // Wait for activity pulse to show events
    await page.waitForFunction(
      () => document.querySelectorAll(".cc-activity-rail__pulse-row").length > 0,
      { timeout: 30_000 }
    );

    const pulseLabels = await page.$$eval(".cc-activity-rail__pulse-row", (items) =>
      items.map((item) => item.textContent || "")
    );
    assert(
      pulseLabels.some((label) =>
        label.includes("run_started") ||
        label.includes("turn_started") ||
        label.includes("text_delta") ||
        label.includes("run_completed") ||
        label.includes("interaction_started") ||
        label.includes("interaction_delta") ||
        label.includes("interaction_complete") ||
        label.includes("interaction_failed")
      ),
      `expected post-send activity event in pulse, not just subscription noise: ${JSON.stringify(pulseLabels)}`
    );

    const usesIdentityLane = observedRequests.some(
      (request) =>
        request.url === `${baseUrl}/console/rpc` &&
        request.postData.includes('"method":"mobkit/interact"'),
    ) && observedRequests.some(
      (request) =>
        request.method === "POST" && request.url === `${baseUrl}/console/identity/stream`,
    );
    const usesLegacyLane = observedRequests.some(
      (request) =>
        request.url === `${baseUrl}/console/rpc` &&
        request.postData.includes('"method":"mobkit/send_message"'),
    ) && observedRequests.some(
      (request) =>
        request.method === "POST" && request.url === `${baseUrl}/interactions/stream`,
    );

    assert(
      usesIdentityLane || usesLegacyLane,
      `expected browser flow to use one coherent send/stream lane; saw ${JSON.stringify(observedRequests, null, 2)}`,
    );

    process.stdout.write("browser e2e ok\n");
  } finally {
    if (browser) {
      await browser.close();
    }
    await stopBackend(backend);
  }
}

async function runMixedMigrationBrowserProof() {
  const port = await reservePort();
  const server = await startMockConsoleServer(port);
  let browser;

  try {
    browser = await launchBrowser();
    const page = await browser.newPage();
    await page.goto(`${server.baseUrl}/console`, { waitUntil: "networkidle" });

    await page.waitForSelector(".cc-sidebar-row", { timeout: 30_000 });
    const sidebarLabels = await page.$$eval(".cc-sidebar-row", (rows) =>
      rows.map((row) => (row.textContent || "").trim())
    );
    assert(
      sidebarLabels.some((label) => label.includes("Identity Luka")),
      `mock sidebar missing identity target: ${JSON.stringify(sidebarLabels)}`,
    );
    assert(
      sidebarLabels.some((label) => label.includes("Legacy Router")),
      `mock sidebar missing legacy target: ${JSON.stringify(sidebarLabels)}`,
    );

    await page.fill(".cc-composer__textarea", "identity proof message");
    await page.click(".cc-composer__send-btn");
    await page.waitForTimeout(100);

    await page.locator(".cc-sidebar-row").nth(1).click();
    await page.fill(".cc-composer__textarea", "legacy proof message");
    await page.click(".cc-composer__send-btn");
    await page.waitForTimeout(100);

    const sawIdentityLane = server.requests.some(
      (request) =>
        request.url === "/console/rpc" &&
        request.body.includes('"method":"mobkit/interact"') &&
        request.body.includes('"identity":"identity:luka"'),
    ) && server.requests.some(
      (request) => request.method === "POST" && request.url === "/console/identity/stream",
    );
    const sawLegacyLane = server.requests.some(
      (request) =>
        request.url === "/console/rpc" &&
        request.body.includes('"method":"mobkit/send_message"') &&
        request.body.includes('"member_id":"legacy-router"'),
    ) && server.requests.some(
      (request) => request.method === "POST" && request.url === "/interactions/stream",
    );

    assert(
      sawIdentityLane,
      `expected mixed migration proof to use identity-native lane for the identity target; saw ${JSON.stringify(server.requests, null, 2)}`,
    );
    assert(
      sawLegacyLane,
      `expected mixed migration proof to use legacy lane for the member target; saw ${JSON.stringify(server.requests, null, 2)}`,
    );

    process.stdout.write("browser mixed migration ok\n");
  } finally {
    if (browser) {
      await browser.close();
    }
    await server.close();
  }
}

async function main() {
  await runReferenceBrowserProof();
  await runMixedMigrationBrowserProof();
}

main().catch((error) => {
  process.stderr.write(`${error.stack || error.message}\n`);
  process.exit(1);
});
