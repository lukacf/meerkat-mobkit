#!/usr/bin/env node

const assert = require("node:assert/strict");
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

async function runBrowserProof() {
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

  let browser;
  try {
    await waitForHttpOk(`${baseUrl}/healthz`);
    await waitForHttpOk(`${baseUrl}/console`);

    browser = await launchBrowser();
    const page = await browser.newPage();
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
        label.includes("interaction_started") ||
        label.includes("text_delta") ||
        label.includes("subscribed")
      ),
      `expected event in pulse: ${JSON.stringify(pulseLabels)}`
    );

    process.stdout.write("browser e2e ok\n");
  } finally {
    if (browser) {
      await browser.close();
    }
    await stopBackend(backend);
  }
}

runBrowserProof().catch((error) => {
  process.stderr.write(`${error.stack || error.message}\n`);
  process.exit(1);
});
