import assert from "node:assert/strict";
import test from "node:test";
import * as React from "react";
import { flushSync } from "react-dom";
import { createRoot } from "react-dom/client";
import { JSDOM } from "jsdom";

import { ConsoleActivityRail } from "@console-components";
import { Sidebar } from "../panels/Sidebar";
import type { ConsoleAgent } from "../types";

test("ConsoleActivityRail wires roster panel actions to host callbacks", async () => {
  const dom = new JSDOM("<!doctype html><html><body><div id=\"root\"></div></body></html>");
  const previousWindow = globalThis.window;
  const previousDocument = globalThis.document;
  globalThis.window = dom.window as unknown as Window & typeof globalThis;
  globalThis.document = dom.window.document;

  const calls: Array<{ panelId: string; actionId: string }> = [];
  const rootElement = dom.window.document.getElementById("root");
  assert.ok(rootElement);
  const root = createRoot(rootElement);

  try {
    flushSync(() => {
      root.render(
        <ConsoleActivityRail
          Icon={({ name }) => <span aria-hidden="true" data-icon={name} />}
          viewState={{
            collapsed: false,
            panels: [
              {
                kind: "roster",
                id: "team",
                title: "Team",
                actions: [{ id: "refresh", label: "Refresh" }],
                groups: [],
              },
            ],
          }}
          onTogglePicker={() => undefined}
          onCollapse={() => undefined}
          onPanelAction={(panelId, actionId) => calls.push({ panelId, actionId })}
          renderSlotPreview={() => null}
        />,
      );
    });

    const button = dom.window.document.querySelector("[data-testid='activity-action:team:refresh']");
    assert.ok(button);
    button.dispatchEvent(new dom.window.MouseEvent("click", { bubbles: true }));

    assert.deepEqual(calls, [{ panelId: "team", actionId: "refresh" }]);
  } finally {
    root.unmount();
    globalThis.window = previousWindow;
    globalThis.document = previousDocument;
    dom.window.close();
  }
});

test("stock Sidebar keyboard reorder announces movement and retains focus", async () => {
  const dom = new JSDOM("<!doctype html><html><body><div id=\"root\"></div></body></html>", {
    url: "http://console.test",
  });
  const previousWindow = globalThis.window;
  const previousDocument = globalThis.document;
  globalThis.window = dom.window as unknown as Window & typeof globalThis;
  globalThis.document = dom.window.document;

  const agents: ConsoleAgent[] = [
    {
      agent_id: "agent-alpha",
      member_id: "agent-alpha",
      identity: "identity:alpha",
      label: "Agent Alpha",
      kind: "mob_agent",
      role: "worker",
      group: "Alpha",
    },
    {
      agent_id: "agent-beta",
      member_id: "agent-beta",
      identity: "identity:beta",
      label: "Agent Beta",
      kind: "mob_agent",
      role: "worker",
      group: "Beta",
    },
  ];
  const rootElement = dom.window.document.getElementById("root");
  assert.ok(rootElement);
  const root = createRoot(rootElement);

  try {
    flushSync(() => {
      root.render(
        <Sidebar
          agents={agents}
          selectedMemberId="agent-alpha"
          recentActivity={[]}
          collapsed={false}
          grouping={{ group_by: ["group"] }}
          storageNamespace="component-interaction"
          onSelect={() => undefined}
          onOpenControl={() => undefined}
        />,
      );
    });

    const alpha = dom.window.document.querySelector("[data-testid='sidebar-section-toggle:Alpha']") as HTMLButtonElement | null;
    assert.ok(alpha);
    alpha.focus();
    assert.equal(dom.window.document.activeElement, alpha);

    alpha.dispatchEvent(new dom.window.KeyboardEvent("keydown", {
      key: "ArrowDown",
      altKey: true,
      bubbles: true,
    }));
    await new Promise((resolve) => setTimeout(resolve, 0));

    const live = dom.window.document.querySelector("[data-testid='sidebar-reorder-live']");
    assert.equal(live?.textContent, "Moved section Alpha after Beta.");
    const focused = dom.window.document.activeElement as HTMLElement | null;
    assert.equal(focused?.getAttribute("data-testid"), "sidebar-section-toggle:Alpha");
    assert.deepEqual(
      JSON.parse(dom.window.localStorage.getItem("mobkit-console-sidebar-section-order:component-interaction") || "[]"),
      ["Beta", "Alpha"],
    );
  } finally {
    root.unmount();
    globalThis.window = previousWindow;
    globalThis.document = previousDocument;
    dom.window.close();
  }
});
