#!/usr/bin/env node

const fs = require("node:fs");
const http = require("node:http");
const os = require("node:os");
const path = require("node:path");
const { spawn } = require("node:child_process");
const { chromium } = require("playwright");

const { resolveFlowEditorBinary } = require("./flow-editor-binary.cjs");

const repoRoot = path.join(__dirname, "..", "..");
const binary = resolveFlowEditorBinary();
const port = Number(process.env.MOBKIT_FLOW_EDITOR_INTERACTIONS_PORT || 4197);
const addr = `127.0.0.1:${port}`;
const baseUrl = process.env.MOBKIT_FLOW_EDITOR_INTERACTIONS_URL || `http://${addr}`;
const shouldSpawn = !process.env.MOBKIT_FLOW_EDITOR_INTERACTIONS_URL;

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

async function chooseFirstRealAgentDefinition(page) {
  const addDefinition = page.locator(".agents-list__scroll select").first();
  const value = await addDefinition.locator("option").evaluateAll((options) => {
    const option = options.find((candidate) => candidate.value && !candidate.disabled);
    return option?.value || "";
  });
  if (!value) throw new Error("Agent Editor did not expose any real MobKit agent definitions");
  await addDefinition.selectOption(value);
}

async function chooseFirstGraphMember(page) {
  const implementer = page.locator(".add-menu__row", { hasText: "implementer" }).first();
  if (await implementer.count()) {
    await implementer.click();
    return;
  }
  const firstRow = page.locator(".add-menu__row").first();
  if (!(await firstRow.count())) throw new Error("Graph add menu did not expose any member rows");
  await firstRow.click();
}

async function main() {
  let server = null;
  let draftDir = null;
  if (shouldSpawn) {
    draftDir = fs.mkdtempSync(path.join(os.tmpdir(), "mobkit-flow-editor-interactions."));
    server = spawn(binary, ["--listen", addr], {
      cwd: repoRoot,
      env: {
        ...process.env,
        MOBKIT_FLOW_EDITOR_DRAFT_STORE: path.join(draftDir, "drafts.json"),
      },
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
    await page.goto(`${baseUrl}/flow-editor?cache_bust=browser-interactions-smoke`, {
      waitUntil: "domcontentloaded",
    });
    await page.getByText("api ready").waitFor({ timeout: 10_000 });

    await page.locator("button.modetoggle__opt", { hasText: "Graph" }).click();
    await page.locator(".canvas-host").waitFor({ state: "visible", timeout: 10_000 });
    const graphNodeCountBefore = await page.locator("[data-inst-id]").count();
    await page.locator(".cell:not(.is-occupied)").first().click();
    await page.locator(".add-menu").waitFor({ state: "visible", timeout: 10_000 });
    await chooseFirstGraphMember(page);
    await page.waitForFunction(
      (count) => document.querySelectorAll("[data-inst-id]").length > count,
      graphNodeCountBefore,
      { timeout: 10_000 },
    );
    const graphNodeCountAfter = await page.locator("[data-inst-id]").count();
    if (graphNodeCountAfter <= graphNodeCountBefore) {
      throw new Error(`Graph add menu did not create a MobKit graph node: before=${graphNodeCountBefore} after=${graphNodeCountAfter}`);
    }

    const movedNode = page.locator("[data-inst-id]").last();
    const beforeMove = await movedNode.boundingBox();
    const targetCell = await page.locator(".cell:not(.is-occupied)").last().boundingBox();
    if (!beforeMove || !targetCell) throw new Error("Graph node drag test could not measure node/cell bounds");
    await page.mouse.move(beforeMove.x + beforeMove.width / 2, beforeMove.y + beforeMove.height / 2);
    await page.mouse.down();
    await page.mouse.move(targetCell.x + targetCell.width / 2, targetCell.y + targetCell.height / 2, { steps: 20 });
    await page.mouse.up();
    await page.waitForTimeout(1_000);
    const afterMove = await movedNode.boundingBox();
    if (!afterMove || Math.abs(afterMove.x - beforeMove.x) < 100 || Math.abs(afterMove.y - beforeMove.y) < 40) {
      throw new Error(`Graph drag did not move through MobKit graph projection: before=${JSON.stringify(beforeMove)} after=${JSON.stringify(afterMove)}`);
    }

    const edgeCountBefore = await page.locator(".edge").count();
    const sourceNode = page.locator("[data-inst-id]").first();
    const targetNode = page.locator("[data-inst-id]").last();
    const sourcePort = sourceNode.locator(".port-out");
    const sourcePortBox = await sourcePort.boundingBox();
    const targetNodeBox = await targetNode.boundingBox();
    if (!sourcePortBox || !targetNodeBox) throw new Error("Graph connection test could not measure source port/target node bounds");
    await page.mouse.move(sourcePortBox.x + sourcePortBox.width / 2, sourcePortBox.y + sourcePortBox.height / 2);
    await page.mouse.down();
    await page.mouse.move(targetNodeBox.x + Math.min(12, targetNodeBox.width / 4), targetNodeBox.y + targetNodeBox.height / 2, { steps: 24 });
    await page.mouse.up();
    await page.waitForFunction(
      (count) => document.querySelectorAll(".edge").length > count,
      edgeCountBefore,
      { timeout: 10_000 },
    );
    const edgeCountAfter = await page.locator(".edge").count();
    if (edgeCountAfter <= edgeCountBefore) {
      throw new Error(`Graph port drag did not create a MobKit edge: before=${edgeCountBefore} after=${edgeCountAfter}`);
    }

    await page.locator("button.viewtab", { hasText: "AGENTS" }).click();
    await page.locator(".agents-view").waitFor({ state: "visible", timeout: 10_000 });
    const agentCountBefore = await page.locator(".agents-list__name").count();
    await chooseFirstRealAgentDefinition(page);
    await page.waitForFunction(
      (count) => document.querySelectorAll(".agents-list__name").length > count,
      agentCountBefore,
      { timeout: 10_000 },
    );
    await page.locator(".agents-list__scroll").first().locator(".agents-list__item").last().click();

    const titleInput = page.locator(".agent-editor__title-input");
    await titleInput.fill("Browser Interaction Agent");
    await page.waitForTimeout(1_000);
    const titleValue = await titleInput.inputValue();
    if (titleValue !== "Browser Interaction Agent") {
      throw new Error(`Agent Editor title did not reflect MobKit operation result: ${titleValue}`);
    }

    const toolSelect = page.locator("select").filter({ has: page.locator('option[value="image_generation"]') }).first();
    await toolSelect.selectOption("image_generation");
    await page.waitForFunction(() => {
      return Array.from(document.querySelectorAll(".tool-row .name")).some((row) => row.textContent.trim() === "image_generation");
    }, null, { timeout: 10_000 });

    await page.getByRole("button", { name: /INLINE/i }).click();
    await page.locator(".inline-skill input").fill("browser interaction");
    await page.locator(".inline-skill textarea").fill("Use this skill for browser interaction smoke testing.");
    await page.locator(".inline-skill .btn", { hasText: "ADD" }).click();
    await page.waitForFunction(() => {
      return Array.from(document.querySelectorAll(".skill-row__name")).some((row) => row.textContent.trim() === "mob.browser.interaction");
    }, null, { timeout: 10_000 });

    if (consoleMessages.length) {
      throw new Error(`browser console errors:\n${consoleMessages.join("\n")}`);
    }
  } finally {
    await browser.close();
    if (server) server.kill("SIGTERM");
  }
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
