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

async function transcriptMessages(panel) {
  return panel.locator(".cc-conversation-transcript article").evaluateAll((nodes) =>
    nodes.map((node) => ({
      className: node.className,
      text: (node.textContent || "").trim().replace(/\s+/g, " "),
    })),
  );
}

function normalizePromptText(text) {
  return String(text || "").trim().replace(/\s+/g, " ");
}

async function main() {
  const baseUrl = process.argv[2];
  assert.ok(baseUrl, "baseUrl is required");
  const runTag = `smoke-${Date.now().toString(36)}`;
  const scenario = YAML.parse(
    fs.readFileSync(path.join(__dirname, "scenario.yaml"), "utf8"),
  );
  const prompts = scenario.smoke?.prompts || {};
  const toolSweepPrompt = normalizePromptText(`${prompts.tool_sweep || "Run a status sweep and use both tools before answering."} [${runTag}:tool]`);
  const merchantStatusPrompt = normalizePromptText(`${prompts.merchant_status || "Give a one-sentence merchant status update for the fictional incident."} [${runTag}:merchant]`);
  const alphaFollowUpPrompt = normalizePromptText(`${prompts.alpha_follow_up || "Panel alpha follow-up. Give one short sentence about rollback guardrails."} [${runTag}:alpha]`);
  const bravoFollowUpPrompt = normalizePromptText(`${prompts.bravo_follow_up || "Panel bravo follow-up. Give one short sentence about customer impact."} [${runTag}:bravo]`);

  const browser = await chromium.launch({ headless: true });
  const page = await browser.newPage({ viewport: { width: 1600, height: 1000 } });

  try {
    console.log("smoke:load");
    await page.goto(`${baseUrl}/console`, { waitUntil: "domcontentloaded" });
    await page.getByTestId("meerkat-console").waitFor({ state: "visible", timeout: 30000 });

    console.log("smoke:commander");
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
      toolSweepPrompt,
    );
    const phasePill = page.getByTestId(`composer-toolbar:${commanderPanelId}:footer-right:phase`);
    let seenPhases = new Set();
    try {
      await phasePill.waitFor({ state: "visible", timeout: 5000 });
      const seenPhasesPromise = collectSeenPhaseLabels(phasePill, 90000);
      await phasePill.waitFor({ state: "detached", timeout: 90000 });
      seenPhases = await seenPhasesPromise;
    } catch {
      seenPhases = new Set();
    }
    if (seenPhases.size > 0) {
      assert.ok(
        seenPhases.has("waiting") || seenPhases.has("tool-executing") || seenPhases.has("generating"),
        `expected visible response phase, saw: ${Array.from(seenPhases).join(", ") || "<none>"}`,
      );
    }
    await assertEventually(async () => {
      const transcript = await transcriptMessages(commanderPanel);
      return transcript.length >= 2;
    }, "commander transcript should have at least user + assistant", 90000, 250);
    const commanderTranscript = await transcriptMessages(commanderPanel);
    assert.ok(commanderTranscript.length >= 2, "commander transcript should have at least user + assistant");
    assert.equal(
      (commanderTranscript[0]?.text || "").trim(),
      toolSweepPrompt.trim(),
      "commander transcript should start with the latest user prompt",
    );
    await assertEventually(async () => {
      const currentTranscript = await transcriptMessages(commanderPanel);
      return currentTranscript.filter((entry) => entry.text.trim() === toolSweepPrompt).length === 1;
    }, "commander transcript should not duplicate the active user prompt", 60000, 250);
    assert.ok(
      commanderTranscript.every((entry) => entry.text.length > 0),
      "commander transcript must not contain blank message bubbles",
    );
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

    console.log("smoke:merchant");
    await selectSidebarItem(page, "Merchant Success");
    const merchantPanel = page.locator('[data-testid^="chat-panel:merchant-success:"]:visible').first();
    await merchantPanel.waitFor({ state: "visible" });
    const merchantBefore = await merchantPanel.innerText();
    await sendPanelMessage(
      merchantPanel,
      merchantStatusPrompt,
    );
    await waitForPanelChange(merchantPanel, merchantBefore);

    await page.getByTestId("activity-action:pulse:watched-only").click();
    await page.getByTestId("activity-action:pulse:watched-only").waitFor({ state: "visible" });
    await page.waitForTimeout(300);
    await page.getByTestId("activity-action:pulse:all").click();

    console.log("smoke:routing");
    await page.getByTestId("sidebar-action:open_routing").click();
    await page.getByTestId("routing-panel").waitFor();
    await page.getByTestId("routing-route:incident-statuspage").waitFor();

    console.log("smoke:gating");
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

    console.log("smoke:topology");
    await page.getByTestId("sidebar-action:open_topology").click();
    const topologyNode = page.getByTestId("topology-node:incident-commander");
    await topologyNode.waitFor();
    await waitForText(topologyNode, "payments-sre");

    console.log("smoke:health");
    await page.getByTestId("sidebar-action:open_health").click();
    await page.getByTestId("health-panel").waitFor();
    await page.getByTestId("health-identity:health-monitor").waitFor();

    console.log("smoke:inspect");
    await page.getByTestId("sidebar-item-action:incident-commander:inspect_identity").click({ force: true });
    await page.getByTestId("inspect-panel:incident-commander").waitFor();
    await waitForText(page.getByTestId("inspect-panel:incident-commander"), "addressable");

    console.log("smoke:split");
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
      alphaFollowUpPrompt,
    );
    await waitForPanelChange(panelAlpha, alphaBefore);
    await assertEventually(async () => {
      const alphaMessages = await transcriptMessages(panelAlpha);
      return alphaMessages.filter((entry) => entry.text.trim() === alphaFollowUpPrompt).length === 1;
    }, "alpha panel must not duplicate its own prompt", 60000, 250);
    assert.equal(
      await panelBravo.getByText(
        alphaFollowUpPrompt,
      ).count(),
      0,
      "bravo panel must not receive alpha user prompt",
    );

    await sendPanelMessage(
      panelBravo,
      bravoFollowUpPrompt,
    );
    await waitForPanelChange(panelBravo, bravoBefore);
    await assertEventually(async () => {
      const bravoMessages = await transcriptMessages(panelBravo);
      return bravoMessages.filter((entry) => entry.text.trim() === bravoFollowUpPrompt).length === 1;
    }, "bravo panel must not duplicate its own prompt", 60000, 250);
    assert.equal(
      await panelAlpha.getByText(bravoFollowUpPrompt).count(),
      0,
      "alpha panel must not receive bravo user prompt",
    );

    console.log("smoke:scribe");
    await selectSidebarItem(page, "Scribe");
    const scribePanel = page.locator('[data-testid^="chat-panel:scribe:"]:visible').first();
    await scribePanel.waitFor({ state: "visible" });
    await assertEventually(async () => {
      const messages = await transcriptMessages(scribePanel);
      return messages.some((entry) =>
        entry.text.includes("Peer request: incident_facts_timeline")
        || entry.text.includes("Peer request: request_summary")
        || entry.text.includes("Peer request:")
        || entry.text.includes("Peer message:")
        || entry.text.includes("Peer response:")
      );
    }, "scribe panel should show recent peer activity", 60000, 250);
  } finally {
    await browser.close();
  }

  console.log("incident browser smoke passed");
}

main().catch((error) => {
  console.error(error.stack || error.message);
  process.exit(1);
});
