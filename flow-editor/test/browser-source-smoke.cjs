#!/usr/bin/env node

const http = require("node:http");
const path = require("node:path");
const { spawn } = require("node:child_process");
const { chromium } = require("playwright");

const repoRoot = path.join(__dirname, "..", "..");
const defaultBinary = path.join(repoRoot, "target", "debug", "mobkit_flow_editor");
const binary = process.env.MOBKIT_FLOW_EDITOR_BIN || defaultBinary;
const port = Number(process.env.MOBKIT_FLOW_EDITOR_BROWSER_PORT || 4196);
const addr = `127.0.0.1:${port}`;
const baseUrl = process.env.MOBKIT_FLOW_EDITOR_BROWSER_URL || `http://${addr}`;
const shouldSpawn = !process.env.MOBKIT_FLOW_EDITOR_BROWSER_URL;

function waitForReady(url, child) {
  const deadline = Date.now() + 20_000;
  return new Promise((resolve, reject) => {
    const poll = () => {
      if (child && child.exitCode !== null) {
        reject(new Error(`mobkit_flow_editor exited before ready with status ${child.exitCode}`));
        return;
      }
      const req = http.request(url, { method: "GET" }, (res) => {
        res.resume();
        if (res.statusCode && res.statusCode >= 200 && res.statusCode < 500) {
          resolve();
          return;
        }
        retry();
      });
      req.on("error", retry);
      req.end();
    };
    const retry = () => {
      if (Date.now() > deadline) {
        reject(new Error(`timed out waiting for ${url}`));
        return;
      }
      setTimeout(poll, 200);
    };
    poll();
  });
}

async function main() {
  let server = null;
  if (shouldSpawn) {
    server = spawn(binary, ["--listen", addr], {
      cwd: repoRoot,
      stdio: ["ignore", "pipe", "pipe"],
    });
    server.stdout.on("data", (chunk) => process.stdout.write(chunk));
    server.stderr.on("data", (chunk) => process.stderr.write(chunk));
  }

  const browser = await chromium.launch({ headless: true });
  const page = await browser.newPage({ viewport: { width: 1280, height: 900 } });
  const consoleMessages = [];
  page.on("console", (message) => {
    if (message.type() === "error") consoleMessages.push(message.text());
  });
  page.on("pageerror", (error) => consoleMessages.push(error.message || String(error)));

  try {
    await waitForReady(`${baseUrl}/flow-editor`, server);
    await page.goto(`${baseUrl}/flow-editor?cache_bust=browser-source-smoke`, {
      waitUntil: "networkidle",
    });
    await page.getByText("api ready").waitFor({ timeout: 10_000 });

    await page.locator(".bld-toml-toggle").click();
    const inlineEditor = page.locator(".bld-toml");
    await inlineEditor.waitFor({ timeout: 10_000 });

    const sourceBox = inlineEditor.locator('[role="textbox"][aria-readonly="true"]');
    await sourceBox.waitFor({ timeout: 10_000 });
    await page.waitForFunction(() => {
      const source = document.querySelector('.bld-toml [role="textbox"][aria-readonly="true"]');
      return source?.innerText.includes("[profiles.") && source?.innerText.includes("[flows.");
    }, null, { timeout: 10_000 });
    const mobToml = await sourceBox.innerText();
    if (!mobToml.includes("[profiles.") || !mobToml.includes("[flows.")) {
      throw new Error(`inline mob.toml editor did not render MobKit source: ${mobToml.slice(0, 400)}`);
    }

    const definitionButton = inlineEditor.locator(".source-file-row", { hasText: "definition.json" });
    await definitionButton.click();
    await page.waitForFunction(() => {
      const source = document.querySelector('.bld-toml [role="textbox"][aria-readonly="true"]');
      return source?.innerText.includes('"id"') && source?.innerText.includes('"profiles"');
    }, null, { timeout: 10_000 });
    const definitionJson = await sourceBox.innerText();
    if (!definitionJson.includes('"id"') || !definitionJson.includes('"profiles"')) {
      throw new Error(`inline definition.json editor did not render exported source: ${definitionJson.slice(0, 400)}`);
    }

    const readonly = await sourceBox.getAttribute("aria-readonly");
    if (readonly !== "true") {
      throw new Error(`inline source editor must be read-only, got aria-readonly=${readonly}`);
    }

    await page.locator(".bld-toml-toggle").click();
    await inlineEditor.waitFor({ state: "hidden", timeout: 10_000 });

    await page.locator(".bld-toml-toggle").click();
    await inlineEditor.waitFor({ timeout: 10_000 });
    await inlineEditor.locator(".bld-toml__head .btn--ghost").click();
    await inlineEditor.waitFor({ state: "hidden", timeout: 10_000 });

    await page.locator("button.modetoggle__opt", { hasText: "Graph" }).click();
    const graphSource = page.locator(".node--source-file").first();
    await graphSource.waitFor({ state: "visible", timeout: 10_000 });
    const graphSourceClass = await graphSource.getAttribute("class");
    if ((graphSourceClass || "").split(/\s+/).includes("node")) {
      throw new Error(`graph source file affordance must not inherit graph node chrome: ${graphSourceClass}`);
    }
    await graphSource.click();
    const graphInlineEditor = page.locator(".bld-toml--graph");
    await graphInlineEditor.waitFor({ timeout: 10_000 });
    const graphSourceBox = graphInlineEditor.locator('[role="textbox"][aria-readonly="true"]');
    await page.waitForFunction(() => {
      const source = document.querySelector('.bld-toml--graph [role="textbox"][aria-readonly="true"]');
      return source?.innerText.includes("[profiles.") && source?.innerText.includes("[flows.");
    }, null, { timeout: 10_000 });
    const graphReadonly = await graphSourceBox.getAttribute("aria-readonly");
    if (graphReadonly !== "true") {
      throw new Error(`graph inline source editor must be read-only, got aria-readonly=${graphReadonly}`);
    }
    await graphInlineEditor.locator(".bld-toml__head .btn--ghost").click();
    await graphInlineEditor.waitFor({ state: "hidden", timeout: 10_000 });

    await page.locator("button.viewtab", { hasText: "AGENTS" }).click();
    const runtimeDetails = page.locator("details.agent-runtime").first();
    await runtimeDetails.waitFor({ state: "visible", timeout: 10_000 });
    const runtimeState = await runtimeDetails.evaluate((el) => {
      const body = el.querySelector(".agent-runtime__body");
      return {
        open: el.open,
        summary: el.querySelector("summary")?.textContent?.trim() || "",
        bodyDisplay: body ? getComputedStyle(body).display : "",
      };
    });
    if (runtimeState.open || runtimeState.summary !== "RUNTIME" || runtimeState.bodyDisplay !== "none") {
      throw new Error(`agent runtime section must render collapsed behind schema-backed title: ${JSON.stringify(runtimeState)}`);
    }

    if (consoleMessages.length) {
      throw new Error(`browser console errors:\n${consoleMessages.join("\n")}`);
    }
  } finally {
    await browser.close();
    if (server) {
      server.kill("SIGTERM");
    }
  }
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
