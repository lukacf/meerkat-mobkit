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

  try {
    await waitForHttpOk(`${baseUrl}/healthz`);

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
    const app = createConsoleApp(root, { baseUrl });

    // 1. Wait for the current design sidebar to populate with agent rows
    await waitFor(() => {
      return dom.window.document.querySelectorAll("[data-testid^=\"sidebar-agent:\"]").length >= 2;
    });

    const sidebarLabels = Array.from(
      dom.window.document.querySelectorAll("[data-testid^=\"sidebar-agent:\"]")
    ).map((row) => row.textContent.trim());
    assert(
      sidebarLabels.some((label) => label.includes("Billing")),
      `expected "Billing" in sidebar labels: ${JSON.stringify(sidebarLabels)}`
    );
    assert(
      sidebarLabels.some((label) => label.includes("Delivery")),
      `expected "Delivery" in sidebar labels: ${JSON.stringify(sidebarLabels)}`
    );

    // 2. Open a chat panel and verify the current conversation pane rendered
    dom.window.document.querySelector("[data-testid^=\"sidebar-agent:\"]").click();
    await waitFor(() => {
      return dom.window.document.querySelector("[data-testid^=\"chat-pane:\"]") !== null;
    });

    // 3. Verify activity rail rendered
    await waitFor(() => {
      return dom.window.document.querySelector("[data-testid=\"signals-rail\"]") !== null;
    });

    // 4. Verify chat composer is present
    const composer = dom.window.document.querySelector(".composer");
    assert(composer, "chat composer missing");

    // 5. Verify the workbench layout has all three columns
    const workbench = dom.window.document.querySelector("[data-console-workbench=\"root\"]");
    assert(workbench, "workbench layout missing");

    app.unmount();
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
