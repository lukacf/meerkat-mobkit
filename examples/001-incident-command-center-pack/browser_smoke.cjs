#!/usr/bin/env node

const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const { chromium } = require("playwright");
const YAML = require("yaml");

async function waitForText(scope, text, timeout = 60000) {
  await scope.getByText(text, { exact: false }).waitFor({ state: "visible", timeout });
}

async function dragHandle(page, locator, deltaX) {
  const box = await locator.boundingBox();
  assert.ok(box, "expected resize handle bounding box");
  await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
  await page.mouse.down();
  await page.mouse.move(box.x + box.width / 2 + deltaX, box.y + box.height / 2, { steps: 12 });
  await page.mouse.up();
}

async function sendPanelMessage(panel, text) {
  const textarea = panel.locator("textarea");
  const submit = panel.locator('[id^="composer-submit:"]');
  await textarea.waitFor({ state: "visible", timeout: 10000 });
  await textarea.fill(text);
  await assertEventually(async () => !(await submit.isDisabled()), "expected composer submit button to enable");
  await submit.click({ force: true });
}

async function panelId(panel) {
  const id = await panel.getAttribute("data-panel-id");
  assert.ok(id, "expected panel id");
  return id;
}

async function currentPendingId(page) {
  const locator = page.locator('[data-testid^="gating-pending:"]').first();
  await locator.waitFor({ state: "visible", timeout: 10000 });
  const value = await locator.getAttribute("data-testid");
  assert.ok(value, "expected gating pending id");
  return value.slice("gating-pending:".length);
}

async function selectSidebarItem(page, label) {
  const row = page
    .locator('[data-console-workbench-part="launcher"] [data-console-sidebar-part="row"]')
    .filter({ hasText: label })
    .first();
  await row.waitFor({ state: "visible", timeout: 10000 });
  await row.click();
}

async function waitForSidebarItem(page, label) {
  await page
    .locator('[data-console-workbench-part="launcher"] [data-console-sidebar-part="row"]')
    .filter({ hasText: label })
    .first()
    .waitFor({ state: "visible", timeout: 60000 });
}

async function assertEventually(check, message, timeout = 10000, interval = 100) {
  const deadline = Date.now() + timeout;
  while (Date.now() < deadline) {
    if (await check()) {
      return;
    }
    await new Promise((resolve) => setTimeout(resolve, interval));
  }
  assert.fail(message);
}

async function waitForPanelChange(panel, previousText, timeout = 90000) {
  await assertEventually(async () => (await panel.innerText()) !== previousText, "expected panel text to change", timeout);
}

async function collectSeenPhaseLabels(locator, timeout = 90000) {
  const deadline = Date.now() + timeout;
  const seen = new Set();
  while (Date.now() < deadline) {
    try {
      const text = (await locator.textContent({ timeout: 250 }))?.trim();
      if (text) {
        seen.add(text);
      }
    } catch {
      if (seen.size > 0) {
        break;
      }
    }
    await new Promise((resolve) => setTimeout(resolve, 150));
  }
  return seen;
}

async function main() {
  const baseUrl = process.argv[2];
  assert.ok(baseUrl, "baseUrl is required");
  const scenario = YAML.parse(
    fs.readFileSync(path.join(__dirname, "scenario.yaml"), "utf8"),
  );
  const prompts = scenario.smoke?.prompts || {};

  const browser = await chromium.launch({ headless: true });
  const page = await browser.newPage({ viewport: { width: 1600, height: 1000 } });

  try {
    await page.goto(`${baseUrl}/console`, { waitUntil: "domcontentloaded" });
    await page.getByTestId("meerkat-console").waitFor({ state: "visible", timeout: 30000 });

    await waitForSidebarItem(page, "Incident Commander");
    await waitForSidebarItem(page, "Payments SRE");
    await waitForSidebarItem(page, "Approval Gate");
    await selectSidebarItem(page, "Incident Commander");

    const sidebarHandle = page.getByTestId("resize:sidebar");
    const activityHandle = page.getByTestId("resize:activity");
    const sidebarBefore = await page.locator('[data-console-workbench-part="launcher"]').boundingBox();
    const activityBefore = await page.locator('[data-console-workbench-part="activity"]').boundingBox();
    await dragHandle(page, sidebarHandle, 80);
    await dragHandle(page, activityHandle, -60);
    const sidebarAfter = await page.locator('[data-console-workbench-part="launcher"]').boundingBox();
    const activityAfter = await page.locator('[data-console-workbench-part="activity"]').boundingBox();
    assert.ok(sidebarBefore && sidebarAfter && sidebarAfter.width !== sidebarBefore.width, "sidebar should resize");
    assert.ok(activityBefore && activityAfter && activityAfter.width !== activityBefore.width, "activity rail should resize");

    let commanderPanel = page.locator('[data-testid^="chat-panel:incident-commander:"]:visible').first();
    await commanderPanel.waitFor({ state: "visible" });
    const commanderPanelId = await panelId(commanderPanel);

    const commanderBefore = await commanderPanel.innerText();
    await sendPanelMessage(
      commanderPanel,
      prompts.tool_sweep || "Run a status sweep and use both tools before answering.",
    );
    const phasePill = page.getByTestId(`composer-toolbar:${commanderPanelId}:footer-right:phase`);
    await phasePill.waitFor({ state: "visible" });
    const seenPhasesPromise = collectSeenPhaseLabels(phasePill, 90000);
    await phasePill.waitFor({ state: "detached", timeout: 90000 });
    const seenPhases = await seenPhasesPromise;
    assert.ok(seenPhases.has("waiting"), "expected waiting phase");
    await waitForPanelChange(commanderPanel, commanderBefore);
    await assertEventually(
      async () => {
        const sreCount = await page
          .locator('[data-testid^="activity-item:pulse:"]')
          .filter({ hasText: "Payments SRE" })
          .count();
        const commsCount = await page
          .locator('[data-testid^="activity-item:pulse:"]')
          .filter({ hasText: "Merchant Comms" })
          .count();
        const scribeCount = await page
          .locator('[data-testid^="activity-item:pulse:"]')
          .filter({ hasText: "Scribe" })
          .count();
        return sreCount > 0 || commsCount > 0 || scribeCount > 0;
      },
      "expected cross-agent activity after commander coordination",
      90000,
      250,
    );

    await selectSidebarItem(page, "Merchant Success");
    const merchantPanel = page.locator('[data-testid^="chat-panel:merchant-success:"]:visible').first();
    await merchantPanel.waitFor({ state: "visible" });
    const merchantBefore = await merchantPanel.innerText();
    await sendPanelMessage(
      merchantPanel,
      prompts.merchant_status || "Give a one-sentence merchant status update for the fictional incident.",
    );
    await waitForPanelChange(merchantPanel, merchantBefore);

    await page.getByTestId("activity-action:pulse:watched-only").click();
    await page.getByTestId("activity-action:pulse:watched-only").waitFor({ state: "visible" });
    await page.waitForTimeout(300);
    await page.getByTestId("activity-action:pulse:all").click();

    await page.getByTestId("sidebar-action:open_routing").click();
    await page.getByTestId("routing-panel").waitFor();
    await page.getByTestId("routing-route:incident-statuspage").waitFor();

    await page.getByTestId("sidebar-action:open_gating").click();
    await page.getByTestId("gating-panel").waitFor();
    const originalPendingId = await currentPendingId(page);
    await page.getByTestId(`gating-action:${originalPendingId}:escalate`).click();
    await page.waitForFunction(
      (previousId) => {
        const next = document.querySelector('[data-testid^="gating-pending:"]');
        return next && next.getAttribute("data-testid") !== `gating-pending:${previousId}`;
      },
      originalPendingId,
    );
    const successorPendingId = await currentPendingId(page);
    assert.notEqual(successorPendingId, originalPendingId, "escalation should create successor pending entry");
    await page.getByTestId(`gating-action:${successorPendingId}:approve`).click();
    await page.waitForFunction(() => document.querySelectorAll('[data-testid^="gating-pending:"]').length === 0);

    await page.getByTestId("sidebar-action:open_topology").click();
    const topologyNode = page.getByTestId("topology-node:incident-commander");
    await topologyNode.waitFor();
    await waitForText(topologyNode, "payments-sre");

    await page.getByTestId("sidebar-action:open_health").click();
    await page.getByTestId("health-panel").waitFor();
    await page.getByTestId("health-identity:health-monitor").waitFor();

    await page.getByTestId("sidebar-item-action:incident-commander:inspect_identity").click({ force: true });
    await page.getByTestId("inspect-panel:incident-commander").waitFor();
    await waitForText(page.getByTestId("inspect-panel:incident-commander"), "addressable");

    await selectSidebarItem(page, "Incident Commander");
    commanderPanel = page.locator('[data-testid^="chat-panel:incident-commander:"]:visible').first();
    await commanderPanel.waitFor({ state: "visible" });
    const activeCommanderPanelId = await panelId(commanderPanel);
    await page.getByTestId(`dock-split:${activeCommanderPanelId}:right`).click();
    const commanderPanels = page.locator('[data-testid^="chat-panel:incident-commander:"]:visible');
    await page.waitForFunction(() => document.querySelectorAll('[data-testid^="chat-panel:incident-commander:"]').length >= 2);

    const [alphaId, bravoId] = await Promise.all([
      panelId(commanderPanels.nth(0)),
      panelId(commanderPanels.nth(1)),
    ]);
    const panelAlpha = page.locator(`[data-panel-id="${alphaId}"][data-testid^="chat-panel:incident-commander:"]`);
    const panelBravo = page.locator(`[data-panel-id="${bravoId}"][data-testid^="chat-panel:incident-commander:"]`);
    assert.notEqual(alphaId, bravoId, "split should create a second panel");

    const alphaBefore = await panelAlpha.innerText();
    const bravoBefore = await panelBravo.innerText();

    await sendPanelMessage(
      panelAlpha,
      prompts.alpha_follow_up || "Panel alpha follow-up. Give one short sentence about rollback guardrails.",
    );
    await waitForPanelChange(panelAlpha, alphaBefore);
    assert.equal(
      await panelBravo.getByText(
        prompts.alpha_follow_up || "Panel alpha follow-up. Give one short sentence about rollback guardrails.",
      ).count(),
      0,
      "bravo panel must not receive alpha user prompt",
    );

    await sendPanelMessage(
      panelBravo,
      prompts.bravo_follow_up || "Panel bravo follow-up. Give one short sentence about customer impact.",
    );
    await waitForPanelChange(panelBravo, bravoBefore);
    assert.equal(
      await panelAlpha.getByText(prompts.bravo_follow_up || "Panel bravo follow-up. Give one short sentence about customer impact.").count(),
      0,
      "alpha panel must not receive bravo user prompt",
    );
  } finally {
    await browser.close();
  }

  console.log("incident browser smoke passed");
}

main().catch((error) => {
  console.error(error.stack || error.message);
  process.exit(1);
});
