#!/usr/bin/env node

const assert = require("node:assert/strict");
const net = require("node:net");
const path = require("node:path");
const { spawn } = require("node:child_process");
const { setTimeout: sleep } = require("node:timers/promises");
const { JSDOM } = require("jsdom");

const { createConsoleApp } = require("./index.cjs");

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

async function waitFor(check, timeoutMs = 20_000, intervalMs = 50) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    if (check()) {
      return;
    }
    await sleep(intervalMs);
  }
  throw new Error("timed out waiting for condition");
}

async function runSmoke() {
  const port = await reservePort();
  const addr = `127.0.0.1:${port}`;
  const baseUrl = `http://${addr}`;

  const backend = spawn(
    "cargo",
    ["run", "-p", "meerkat-mobkit-core", "--example", "library_mode_reference"],
    {
      cwd: repoRoot,
      env: { ...process.env, MOBKIT_REF_ADDR: addr },
      stdio: ["ignore", "pipe", "pipe"],
    }
  );

  backend.stderr.on("data", (chunk) => {
    process.stderr.write(chunk);
  });

  try {
    await waitForHttpOk(`${baseUrl}/healthz`);
    const experienceResponse = await fetch(`${baseUrl}/console/experience`);
    assert(experienceResponse.ok, "console experience endpoint unavailable");
    const experienceJson = await experienceResponse.json();
    const expectedNodeCount =
      experienceJson?.topology?.live_snapshot?.node_count;
    const expectedLoadedModuleCount =
      experienceJson?.health_overview?.live_snapshot?.loaded_module_count;
    assert.equal(
      typeof expectedNodeCount,
      "number",
      "topology live snapshot node_count missing"
    );
    assert.equal(
      typeof expectedLoadedModuleCount,
      "number",
      "health live snapshot loaded_module_count missing"
    );

    const dom = new JSDOM(
      "<!doctype html><html><body><div id=\"root\"></div></body></html>",
      {
        url: baseUrl,
        pretendToBeVisual: true,
      }
    );

    global.window = dom.window;
    global.document = dom.window.document;
    global.navigator = dom.window.navigator;
    global.HTMLElement = dom.window.HTMLElement;
    global.Event = dom.window.Event;
    global.CustomEvent = dom.window.CustomEvent;
    global.Node = dom.window.Node;
    global.Text = dom.window.Text;

    const root = dom.window.document.getElementById("root");
    createConsoleApp(root, { baseUrl });

    await waitFor(() => {
      return dom.window.document.querySelectorAll("[data-testid=\"sidebar-list\"] button").length >= 2;
    });

    const sidebarLabels = Array.from(
      dom.window.document.querySelectorAll("[data-testid=\"sidebar-list\"] button")
    ).map((button) => button.textContent.trim());
    assert(sidebarLabels.includes("router"));
    assert(sidebarLabels.includes("delivery"));

    await waitFor(() => {
      const nodeCount = dom.window.document.querySelector("[data-testid=\"topology-node-count\"]");
      const moduleCount = dom.window.document.querySelector(
        "[data-testid=\"health-loaded-module-count\"]"
      );
      return Boolean(nodeCount && moduleCount);
    });

    const topologyCountText =
      dom.window.document.querySelector("[data-testid=\"topology-node-count\"]")?.textContent || "";
    const loadedModuleCountText =
      dom.window.document.querySelector("[data-testid=\"health-loaded-module-count\"]")
        ?.textContent || "";
    assert(
      topologyCountText.includes(String(expectedNodeCount)),
      `expected topology node count ${expectedNodeCount} in "${topologyCountText}"`
    );
    assert(
      loadedModuleCountText.includes(String(expectedLoadedModuleCount)),
      `expected loaded module count ${expectedLoadedModuleCount} in "${loadedModuleCountText}"`
    );

    const messageField = dom.window.document.querySelector("textarea[name=\"message\"]");
    const form = dom.window.document.querySelector("[data-testid=\"chat-form\"]");
    assert(messageField, "message textarea missing");
    assert(form, "chat form missing");

    messageField.value = "smoke message";
    messageField.dispatchEvent(new dom.window.Event("input", { bubbles: true }));
    messageField.dispatchEvent(new dom.window.Event("change", { bubbles: true }));
    form.dispatchEvent(
      new dom.window.Event("submit", { bubbles: true, cancelable: true })
    );

    await waitFor(() => {
      return dom.window.document.querySelectorAll("[data-testid=\"activity-feed\"] li").length > 0;
    }, 30_000, 100);

    const activityRows = Array.from(
      dom.window.document.querySelectorAll("[data-testid=\"activity-feed\"] li")
    ).map((row) => row.textContent || "");
    assert(
      activityRows.some((value) => value.includes("interaction_started")),
      "activity feed did not receive interaction_started event"
    );

    const inspectorRows = Array.from(
      dom.window.document.querySelectorAll("[data-testid=\"chat-events\"] li")
    ).map((row) => row.textContent || "");
    assert(
      inspectorRows.some((value) => value.includes("interaction_started")),
      "chat inspector did not render interaction_started event"
    );

    dom.window.close();
    process.stdout.write("smoke ok\n");
  } finally {
    await stopBackend(backend);
  }
}

runSmoke().catch((error) => {
  process.stderr.write(`${error.stack || error.message}\n`);
  process.exit(1);
});
