"use strict";
const assert = require("node:assert/strict");
const fs = require("node:fs/promises");
const path = require("node:path");
const { chromium } = require("playwright");
const { startFixture, eventually } = require("../acceptance-runtime.cjs");
const evidence = path.join(__dirname, "../../output/playwright/console-acceptance");

async function clonedTabDrafts() {
  const fixture = await startFixture();
  const browser = await chromium.launch({ headless: true });
  const context = await browser.newContext({ viewport: { width: 1600, height: 1000 } });
  const first = await context.newPage();
  const errors = [];
  context.on("page", page => page.on("pageerror", error => errors.push(error.message)));
  first.on("pageerror", error => errors.push(error.message));
  let clone;
  const editor = page => page.getByTestId("chat-composer:router:main").first();
  async function waitSaved(page, text) {
    await eventually(() => page.evaluate(value => [localStorage, sessionStorage].some(storage =>
      Object.keys(storage).some(key => key.startsWith("mobkit-composer-draft:v2:") && JSON.parse(storage.getItem(key)).text === value)), text), "draft persisted");
  }
  try {
    await first.goto(fixture.baseUrl + "/scoped");
    await first.locator('.agent[role="button"], .cc-sidebar-row').filter({ hasText: /router/i }).first().click();
    await editor(first).fill("Original tab draft");
    await waitSaved(first, "Original tab draft");
    const opened = first.waitForEvent("popup");
    await first.evaluate(() => window.open(location.href, "_blank"));
    clone = await opened;
    await editor(clone).waitFor();
    // A real same-origin opener copies sessionStorage, including the tab ID.
    assert.equal(await first.evaluate(() => sessionStorage.getItem("mobkit-composer-tab:v1")),
      await clone.evaluate(() => sessionStorage.getItem("mobkit-composer-tab:v1")));
    assert.equal(await editor(clone).inputValue(), "Original tab draft");
    await editor(clone).fill("Independent copied tab draft");
    await waitSaved(clone, "Independent copied tab draft");
    await fixture.control("model", { source: "Original tab send completed.", delay_ms: 0, chunk_chars: 32 });
    await editor(first).press("Enter");
    await eventually(async () => (await (await fetch(fixture.backendUrl + "/__fixture/requests")).json())
      .some(request => JSON.stringify(request.messages).includes("Original tab draft")), "original tab real model ingress");
    await clone.reload();
    await editor(clone).waitFor();
    assert.equal(await editor(clone).inputValue(), "Independent copied tab draft", "sending in the original cannot erase a copied tab draft");
    await first.reload();
    await editor(first).waitFor();
    assert.equal(await editor(first).inputValue(), "");
    const requests = await (await fetch(fixture.backendUrl + "/__fixture/requests")).json();
    assert(!requests.some(request => JSON.stringify(request.messages).includes("Independent copied tab draft")));
    assert.deepEqual(errors, []);
    await fs.mkdir(evidence, { recursive: true });
    await clone.screenshot({ path: path.join(evidence, "cloned-tab-preserved-draft.png"), fullPage: true });
    await fs.writeFile(path.join(evidence, "cloned-tab-evidence.json"), JSON.stringify({ clonedStorage: true, originalSubmitted: true, copiedDraftPreserved: true, errors }, null, 2));
  } catch (error) {
    await fs.mkdir(evidence, { recursive: true });
    await (clone || first).screenshot({ path: path.join(evidence, "cloned-tab-failure.png"), fullPage: true }).catch(() => {});
    throw error;
  } finally { await browser.close(); await fixture.close(); }
}

module.exports = { scenarios: [{ id: "real-cloned-tab-drafts", family: "real-send", backend: "real", run: clonedTabDrafts }] };
if (require.main === module) require("../scenario-registry.cjs").runScenarios(module.exports.scenarios)
  .catch(error => { process.stderr.write(`${error.stack}\n`); process.exitCode = 1; });
