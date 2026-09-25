"use strict";
const assert = require("node:assert/strict");
const fs = require("node:fs/promises");
const path = require("node:path");
const { chromium } = require("playwright");
const { startFixture, eventually } = require("../acceptance-runtime.cjs");

async function explicitLegacyImport() {
  assert(process.env.MOBKIT_EXAMPLE_BIN_DIR, "Use the coordinator's prebuilt fixture");
  const fixture = await startFixture();
  const browser = await chromium.launch({ headless: true });
  const page = await browser.newPage({ viewport: { width: 1600, height: 1000 } });
  const identity = "router:main";
  const text = "Imported once: review the WorkGraph and preserve this exact instruction.\n\nSecond paragraph.";
  const legacyKey = `mobkit-pending-stack:${identity}`;
  const legacyBytes = JSON.stringify([{ id: "legacy-proof", text, addedAt: 1790370000000 }]);
  const namespace = `${fixture.baseUrl}/acceptance-realm/operator-a`;
  const queueKey = `mobkit-send-attempts:v1:${encodeURIComponent(namespace)}:${encodeURIComponent(identity)}`;
  const dir = process.env.MOBKIT_BROWSER_EVIDENCE || path.join(__dirname, "../../output/playwright/console-acceptance");
  const errors = [];
  page.on("pageerror", error => errors.push(error.message));
  const sends = () => fixture.observations.filter(item => item.method === "POST" &&
    (item.path.endsWith("/send") || item.request.includes('"method":"mobkit/console/send"')));
  const requests = async () => (await (await fetch(fixture.backendUrl + "/__fixture/requests")).json());
  try {
    await page.goto(fixture.baseUrl + "/scoped");
    await page.evaluate(([key, bytes]) => localStorage.setItem(key, bytes), [legacyKey, legacyBytes]);
    await page.reload();
    await page.locator('.agent[role="button"], .cc-sidebar-row').filter({ hasText: /router/i }).first().click();
    await page.locator('[data-testid="console-transport-status"][data-phase="live"]').waitFor();
    const importButton = page.getByRole("button", { name: "Import legacy queue into this account", exact: true });
    await importButton.waitFor();
    assert.equal(sends().length, 0, "opening a scoped console cannot submit a legacy queue");
    assert.equal(await page.evaluate(key => localStorage.getItem(key), queueKey), null);
    assert(!(await requests()).some(request => JSON.stringify(request.messages).includes(text)));
    await fixture.control("model", { source: "Legacy import completed exactly once.", delay_ms: 0, chunk_chars: 32 });
    await importButton.click();
    await eventually(async () => {
      const response = await fetch(`${fixture.baseUrl}/console/timeline?identity=router%3Amain&mode=recent&limit=500`);
      assert.equal(response.status, 200);
      const frames = (await response.json()).frames;
      return frames.some(frame => frame.kind === "interaction_complete" && frame.payload.result === "Legacy import completed exactly once.");
    }, "explicit import reaches actual runtime completion");
    assert.equal(sends().length, 1, "one imported intent creates one POST");
    const submitted = JSON.parse(sends()[0].request);
    const envelope = submitted.params || submitted;
    assert.equal(envelope.content, text);
    assert.equal(envelope.identity, identity);
    await eventually(async () => !await importButton.count(), "one-time import action disappears");
    assert.equal(await page.evaluate(key => localStorage.getItem(key), legacyKey), legacyBytes, "original bytes are preserved");
    const saved = await page.evaluate(key => JSON.parse(localStorage.getItem(key)), queueKey);
    assert.equal(saved.legacyImported, true);
    await page.reload();
    await page.getByTestId(`chat-composer:${identity}`).waitFor();
    await page.locator('[data-testid="console-transport-status"][data-phase="live"]').waitFor();
    assert.equal(await importButton.count(), 0);
    assert.equal(sends().length, 1, "reload cannot resubmit the imported intent");
    assert.equal(await page.evaluate(key => localStorage.getItem(key), legacyKey), legacyBytes);
    assert.deepEqual(errors, []);
    await fs.mkdir(dir, { recursive: true });
    await page.screenshot({ path: path.join(dir, "legacy-import-after-reload.png"), fullPage: true });
    await fs.writeFile(path.join(dir, "legacy-import.json"), JSON.stringify({ saved, envelope, originalBytesPreserved: true, sends: sends(), errors }, null, 2));
  } catch (error) {
    await fs.mkdir(dir, { recursive: true });
    await page.screenshot({ path: path.join(dir, "legacy-import-failure.png"), fullPage: true }).catch(() => {});
    await fs.writeFile(path.join(dir, "legacy-import-failure.json"), JSON.stringify({ error: String(error), observations: fixture.observations, logs: fixture.logs(), errors }, null, 2));
    throw error;
  } finally { await browser.close(); await fixture.close(); }
}

module.exports = { scenarios: [{ id: "real-explicit-legacy-import", family: "real-send", backend: "real", run: explicitLegacyImport }] };
if (require.main === module) require("../scenario-registry.cjs").runScenarios(module.exports.scenarios)
  .catch(error => { process.stderr.write(`${error.stack}\n`); process.exitCode = 1; });
