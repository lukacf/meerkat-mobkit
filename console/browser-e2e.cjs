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

  let browser;
  try {
    await waitForHttpOk(`${baseUrl}/healthz`);
    await waitForHttpOk(`${baseUrl}/console`);

    browser = await launchBrowser();
    const page = await browser.newPage();
    await page.goto(`${baseUrl}/console`, { waitUntil: "networkidle" });

    await page.waitForSelector('[data-testid="sidebar-list"] button', {
      timeout: 30_000,
    });
    const sidebarLabels = await page.$$eval('[data-testid="sidebar-list"] button', (buttons) =>
      buttons.map((button) => button.textContent.trim())
    );
    assert(sidebarLabels.includes("router"), "sidebar missing router");
    assert(sidebarLabels.includes("delivery"), "sidebar missing delivery");

    await page.fill('textarea[name="message"]', "browser proof message");
    await page.click('[data-testid="chat-form"] button[type="submit"]');

    await page.waitForFunction(
      () =>
        Array.from(document.querySelectorAll('[data-testid="activity-feed"] li')).some((row) =>
          (row.textContent || "").includes("interaction_started")
        ),
      { timeout: 30_000 }
    );

    const activityRows = await page.$$eval('[data-testid="activity-feed"] li', (items) =>
      items.map((item) => item.textContent || "")
    );
    assert(
      activityRows.some((value) => value.includes("interaction_started")),
      "activity feed did not show interaction_started"
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
