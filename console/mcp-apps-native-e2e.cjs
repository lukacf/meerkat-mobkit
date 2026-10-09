#!/usr/bin/env node
"use strict";

// Real stock Console -> native session admission -> physical stdio MCP server.
// Uses the prebuilt native acceptance fixture and the current production UI.
const assert = require("node:assert/strict");
const fs = require("node:fs/promises");
const http = require("node:http");
const path = require("node:path");
const { spawn } = require("node:child_process");
const { build } = require("esbuild");
const { chromium } = require("playwright");
const { exampleBackendSpec } = require("./example-backend.cjs");
const { LOOPBACK_ANY_PORT, awaitFixtureReady } = require("./fixture-ready.cjs");
const { eventually, rpc } = require("./acceptance-runtime.cjs");
const { createSandboxServer } = require("./mcp-apps-sandbox.cjs");

async function listen(server) {
  await new Promise((resolve, reject) => { server.once("error", reject); server.listen(0, "127.0.0.1", resolve); });
  return `http://127.0.0.1:${server.address().port}`;
}
async function closeServer(server) {
  server?.closeAllConnections();
  if (server?.listening) await new Promise(resolve => server.close(resolve));
}
async function stop(child) {
  if (!child || child.exitCode !== null || child.signalCode !== null) return;
  child.kill("SIGTERM");
  await new Promise(resolve => {
    const timer = setTimeout(() => { child.kill("SIGKILL"); resolve(); }, 3_000);
    child.once("exit", () => { clearTimeout(timer); resolve(); });
  });
}
async function post(base, endpoint, body) {
  const response = await fetch(base + endpoint, {
    method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(body),
  });
  return { status: response.status, body: await response.json(), cache: response.headers.get("cache-control") };
}

async function main() {
  assert(process.env.MOBKIT_EXAMPLE_BIN_DIR, "Set MOBKIT_EXAMPLE_BIN_DIR to the prebuilt native acceptance fixture; this harness never starts Cargo.");
  const repoRoot = path.resolve(__dirname, "..");
  const evidenceRoot = process.env.MOBKIT_BROWSER_EVIDENCE || path.join(repoRoot, "output/playwright");
  await fs.mkdir(evidenceRoot, { recursive: true });
  const evidence = await fs.mkdtemp(path.join(evidenceRoot, "mcp-apps-native-"));
  const stateDir = path.join(evidence, "state");
  await fs.mkdir(stateDir);
  const logPath = path.join(evidence, "mcp-wire.jsonl");
  await fs.writeFile(logPath, "");
  const bundle = await build({ entryPoints: [path.join(__dirname, "src/mcp-apps/preview-app.ts")], bundle: true,
    write: false, format: "esm", platform: "browser", target: "es2022", nodePaths: [path.join(__dirname, "node_modules")] });
  const htmlPath = path.join(evidence, "app.html");
  await fs.writeFile(htmlPath, `<!doctype html><html><body><script type="module">${bundle.outputFiles[0].text.replaceAll("</script", "<\\/script")}</script></body></html>`);
  const assets = new Map(await Promise.all([
    ["/console", "index.html", "text/html"],
    ["/console/assets/console-app.js", "console-app.js", "text/javascript"],
    ["/console/assets/console-app.css", "console-app.css", "text/css"],
  ].map(async ([url, name, type]) => [url, { type, bytes: await fs.readFile(path.join(__dirname, "dist", name)) }])));
  const spec = exampleBackendSpec(repoRoot, "console_acceptance_fixture");
  let backendUrl, child, sandbox, browser, page, logs = "";
  const requests = [];
  const browserLogs = [];
  let streamEvidence = "";
  // Bind before the backend so the sandbox has an exact host origin without
  // closing and reserving a port. This proxy forwards all APIs to the native host.
  const proxy = http.createServer((req, res) => {
    const pathname = new URL(req.url, "http://localhost").pathname;
    const asset = assets.get(pathname === "/console/" ? "/console" : pathname);
    if (asset && req.method === "GET") { res.writeHead(200, { "content-type": asset.type }); res.end(asset.bytes); return; }
    if (!backendUrl) { res.writeHead(503); res.end(); return; }
    const target = new URL(req.url, backendUrl);
    const observed = { method: req.method, path: target.pathname, body: "", status: null };
    requests.push(observed);
    req.on("data", chunk => { if (observed.body.length < 65_536) observed.body += chunk; });
    const upstream = http.request(target, { method: req.method, headers: { ...req.headers, host: target.host } }, incoming => {
      observed.status = incoming.statusCode;
      if (target.pathname === "/console/timeline/stream") {
        incoming.on("data", chunk => { streamEvidence = (streamEvidence + chunk).slice(-1_000_000); });
      }
      res.writeHead(incoming.statusCode, incoming.headers);
      incoming.on("error", error => res.destroy(error));
      res.on("close", () => incoming.destroy());
      incoming.pipe(res);
    });
    upstream.on("error", () => { if (!res.headersSent) res.writeHead(502); res.end(); });
    req.pipe(upstream);
  });
  try {
    const baseUrl = await listen(proxy);
    sandbox = createSandboxServer({ allowedHostOrigins: [baseUrl] });
    const sandboxUrl = `${await listen(sandbox)}/sandbox.html`;
    child = spawn(spec.command, spec.args, { cwd: repoRoot, stdio: ["ignore", "pipe", "pipe"], env: {
      ...process.env, MOBKIT_FIXTURE_ADDR: LOOPBACK_ANY_PORT, MOBKIT_FIXTURE_MODE: "identity",
      MOBKIT_FIXTURE_LIVE_MODEL: "0", MOBKIT_FIXTURE_LIVE_IMAGES: "0", MOBKIT_FIXTURE_ROUTINE_TOOLS: "0",
      MOBKIT_FIXTURE_STATE: stateDir, MOBKIT_FIXTURE_NODE: process.execPath,
      MOBKIT_FIXTURE_MCP_APPS_SERVER: path.join(__dirname, "fixtures/mcp-apps-server.cjs"),
      MOBKIT_FIXTURE_APP_HTML: htmlPath, MOBKIT_FIXTURE_APP_LOG: logPath,
      MOBKIT_FIXTURE_APP_SANDBOX: sandboxUrl,
    } });
    const ready = awaitFixtureReady(child, { label: "native MCP Apps fixture", timeoutMs: 60_000 });
    for (const output of [child.stdout, child.stderr]) output.on("data", chunk => { logs = (logs + chunk).slice(-30_000); });
    ({ baseUrl: backendUrl } = await ready);
    await eventually(async () => {
      const value = await rpc(baseUrl, "mobkit/wait_ready", { timeout_ms: 1000 });
      return value.body.result?.timeout === false && value.body.result.ready?.length === 2;
    }, "native members ready", 60_000);
    // Keep the final assistant response streaming for over the host's 15-second
    // setup limit. A retained view must render without waiting for that turn lock.
    const configured = await post(baseUrl, "/__fixture/model", {
      source: "Ready.", delay_ms: 1000, chunk_chars: 1,
      scenario: { kind: "mcp_apps", run_id: "apps-native" },
    });
    assert.equal(configured.status, 200);
    browser = await chromium.launch({ headless: true });
    page = await browser.newPage({ viewport: { width: 1280, height: 900 } });
    page.on("console", message => browserLogs.push({ type: message.type(), text: message.text() }));
    page.on("pageerror", error => browserLogs.push({ type: "pageerror", text: error.message }));
    await page.goto(`${baseUrl}/console`, { waitUntil: "domcontentloaded" });
    await page.getByTestId("sidebar-agent:router:main").click();
    await page.getByTestId("chat-composer:router:main").fill("[fixture:apps-native] Show matching records.");
    const sentAt = Date.now();
    await page.locator('button:has-text("Send"):visible, .cc-composer__send-btn:visible').first().click();
    const app = page.frameLocator('iframe[title="Interactive tool result"]').frameLocator("iframe");
    await app.getByText("7 matching records", { exact: true }).waitFor({ timeout: 15_000 });
    const firstRenderMs = Date.now() - sentAt;
    assert(firstRenderMs < 15_000, "retained view must render while the final model turn is still running");
    assert.equal(await page.getByText("The MCP App is ready.", { exact: true }).count(), 0,
      "the app must be visible before the delayed final model response completes");
    await app.getByText("PRIVATE_APP_DETAIL", { exact: true }).waitFor();
    await page.locator('iframe[title="Interactive tool result"]').scrollIntoViewIfNeeded();
    await page.screenshot({ path: path.join(evidence, "stock-native-app-live.png"), fullPage: true });
    await app.getByRole("button", { name: "Refresh result" }).click();
    await app.getByText("8 matching records", { exact: true }).waitFor({ timeout: 30_000 });
    const locatorRequest = requests.find(request => request.path === "/console/mcp-apps/resolve" && request.status === 200);
    assert(locatorRequest, "stock host must resolve via the native gateway");
    const locator = JSON.parse(locatorRequest.body);
    assert.equal(locator.identity, "router:main");
    assert.equal(locator.toolCallId, "fixture-apps-native-display");
    const call = requests.find(request => request.path === "/console/mcp-apps/call-tool" && request.status === 200);
    assert(call, "button action must use native admission");
    assert.deepEqual(JSON.parse(call.body), { ...locator, name: "refresh", arguments: {} });
    await page.reload({ waitUntil: "domcontentloaded" });
    await app.getByText("7 matching records", { exact: true }).waitFor({ timeout: 60_000 });
    await app.getByText("PRIVATE_APP_DETAIL", { exact: true }).waitFor();
    const wire = (await fs.readFile(logPath, "utf8")).trim().split("\n").filter(Boolean).map(line => JSON.parse(line));
    const displays = wire.filter(row => row.method === "tools/call" && row.params.name === "display");
    const refreshes = wire.filter(row => row.method === "tools/call" && row.params.name === "refresh");
    assert.equal(displays.length, 1, "reload must not replay the original tool");
    assert.equal(refreshes.length, 1, "one button activation must dispatch exactly once");
    assert.equal(wire.filter(row => row.method === "resources/read").length, 1,
      "app actions and reload must reuse the original view instead of prefetching more HTML");
    assert.equal(displays[0].pid, refreshes[0].pid, "app action must use its original physical connection");
    const init = wire.find(row => row.method === "initialize");
    assert(init.params.capabilities.extensions["io.modelcontextprotocol/ui"].mimeTypes.includes("text/html;profile=mcp-app"));
    const model = await (await fetch(`${baseUrl}/__fixture/requests`)).json();
    assert(!JSON.stringify(model).includes("PRIVATE_APP_DETAIL"), "UI-only metadata must not enter model requests");
    assert(model.some(request => request.tools?.some(tool => tool.name === "display")), "launch tool must be exposed to the native model");
    assert(!model.some(request => request.tools?.some(tool => tool.name === "refresh")), "app-only tool must be absent from model discovery");
    const timeline = await (await fetch(`${baseUrl}/console/timeline?identity=router%3Amain&mode=recent&limit=500`)).json();
    assert(!JSON.stringify(timeline).includes("PRIVATE_APP_DETAIL"), "UI-only metadata must not enter the browser frame log");
    const storage = await page.evaluate(() => JSON.stringify({ local: { ...localStorage }, session: { ...sessionStorage } }));
    assert(!storage.includes("PRIVATE_APP_DETAIL"), "UI-only metadata must not enter browser storage");
    const denied = await post(baseUrl, "/console/mcp-apps/call-tool", { ...locator, identity: "missing:member", name: "refresh", arguments: {} });
    assert.equal(denied.status, 403);
    const forbidden = await post(baseUrl, "/console/mcp-apps/call-tool", { ...locator, name: "display", arguments: {} });
    assert.equal(forbidden.status, 403, "model-only tools must reject app invocation");
    const resolved = await post(baseUrl, "/console/mcp-apps/resolve", locator);
    assert.equal(resolved.status, 200);
    assert.equal(resolved.cache, "no-store");
    assert.equal(resolved.body.result._meta.viewDetail, "PRIVATE_APP_DETAIL");
    await page.screenshot({ path: path.join(evidence, "stock-native-app.png"), fullPage: true });
    await Promise.all([
      fs.writeFile(path.join(evidence, "fixture.log"), logs),
      fs.writeFile(path.join(evidence, "requests.json"), JSON.stringify(requests, null, 2)),
      fs.writeFile(path.join(evidence, "browser.json"), JSON.stringify(browserLogs, null, 2)),
      fs.writeFile(path.join(evidence, "model-requests.json"), JSON.stringify(model, null, 2)),
      fs.writeFile(path.join(evidence, "timeline-stream.txt"), streamEvidence),
      fs.writeFile(path.join(evidence, "timeline.json"), JSON.stringify(timeline, null, 2)),
    ]);
    await fs.writeFile(path.join(evidence, "summary.json"), JSON.stringify({
      passed: true, originalToolCalls: displays.length, appActions: refreshes.length, firstRenderMs,
      locator, sandboxUrl, evidence,
    }, null, 2));
    console.log(JSON.stringify({ passed: true, evidence, originalToolCalls: 1, appActions: 1, firstRenderMs }));
  } catch (error) {
    if (backendUrl) {
      await fetch(new URL("/__fixture/requests", backendUrl))
        .then(response => response.json())
        .then(model => fs.writeFile(path.join(evidence, "model-requests.json"), JSON.stringify(model, null, 2)))
        .catch(() => {});
      await fetch(new URL("/console/timeline?identity=router%3Amain&mode=recent&limit=500", backendUrl), { signal: AbortSignal.timeout(2_000) })
        .then(response => response.json())
        .then(timeline => fs.writeFile(path.join(evidence, "timeline.json"), JSON.stringify(timeline, null, 2)))
        .catch(() => {});
    }
    await fs.writeFile(path.join(evidence, "timeline-stream.txt"), streamEvidence);
    await fs.writeFile(path.join(evidence, "fixture.log"), logs);
    await fs.writeFile(path.join(evidence, "requests.json"), JSON.stringify(requests, null, 2));
    await fs.writeFile(path.join(evidence, "browser.json"), JSON.stringify(browserLogs, null, 2));
    if (page && !page.isClosed()) await fs.writeFile(path.join(evidence, "page.html"), await page.content());
    if (page && !page.isClosed()) await page.screenshot({ path: path.join(evidence, "failure.png"), fullPage: true }).catch(() => {});
    throw new Error(`${error.message}\nNative MCP Apps evidence: ${evidence}\n${logs}`);
  } finally {
    await browser?.close();
    await closeServer(proxy);
    await closeServer(sandbox);
    await stop(child);
  }
}

main().catch(error => { console.error(error); process.exitCode = 1; });
