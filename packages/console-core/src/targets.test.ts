import assert from "node:assert/strict";
import test from "node:test";

import { normalizeConsoleDockState, type ConsoleDockState } from "./dock";
import {
  migrateConsoleWorkbenchTarget,
  type ConsoleWorkbenchTarget,
} from "./targets";

test("migrateConsoleWorkbenchTarget maps legacy agent-chat targets to namespaced identity chat", () => {
  const target = migrateConsoleWorkbenchTarget({
    id: "member-luka",
    kind: "agent-chat",
    title: "Luka",
    identity: "identity:luka",
    memberId: "member-luka",
  });

  assert.equal(target?.kind, "mobkit/identity-chat");
  assert.equal(target?.kind === "mobkit/identity-chat" && "identity" in target ? target.identity : null, "identity:luka");
  assert.equal(target?.kind === "mobkit/identity-chat" && "addressingMode" in target ? target.addressingMode : null, "identity");
});

test("migrateConsoleWorkbenchTarget maps legacy identity-inspect, routing, and gating targets", () => {
  assert.deepEqual(migrateConsoleWorkbenchTarget({
    id: "inspect:identity:luka",
    kind: "identity-inspect",
    title: "Luka Details",
    identity: "identity:luka",
  }), {
    id: "inspect:identity:luka",
    kind: "mobkit/identity-inspect",
    title: "Luka Details",
    subtitle: undefined,
    iconName: undefined,
    badgeLabel: undefined,
    identity: "identity:luka",
    memberId: undefined,
  });

  assert.equal(migrateConsoleWorkbenchTarget({ id: "routing", kind: "routing", title: "Routing" })?.kind, "mobkit/routing");
  assert.equal(migrateConsoleWorkbenchTarget({ id: "gating", kind: "gating", title: "Approvals" })?.kind, "mobkit/gating");
});

test("migrated legacy targets hydrate through dock normalization", () => {
  const state: ConsoleDockState<ConsoleWorkbenchTarget> = normalizeConsoleDockState({
    activeTabId: "one",
    focusedPanelId: "p-chat",
    panels: [
      {
        id: "p-chat",
        mode: "console",
        target: migrateConsoleWorkbenchTarget({
          id: "member-luka",
          kind: "agent-chat",
          title: "Luka",
          identity: "identity:luka",
          memberId: "member-luka",
        }),
      },
      {
        id: "p-inspect",
        mode: "console",
        target: migrateConsoleWorkbenchTarget({
          id: "inspect:identity:luka",
          kind: "identity-inspect",
          title: "Luka Details",
          identity: "identity:luka",
        }),
      },
      {
        id: "p-routing",
        mode: "console",
        target: migrateConsoleWorkbenchTarget({ id: "routing", kind: "routing", title: "Routing" }),
      },
      {
        id: "p-gating",
        mode: "console",
        target: migrateConsoleWorkbenchTarget({ id: "gating", kind: "gating", title: "Approvals" }),
      },
    ],
    tabs: [
      {
        id: "one",
        presetId: "grid",
        layout: {
          kind: "split",
          id: "root",
          direction: "horizontal",
          first: { kind: "panel", panelId: "p-chat" },
          second: {
            kind: "split",
            id: "right",
            direction: "vertical",
            first: { kind: "panel", panelId: "p-inspect" },
            second: {
              kind: "split",
              id: "bottom",
              direction: "horizontal",
              first: { kind: "panel", panelId: "p-routing" },
              second: { kind: "panel", panelId: "p-gating" },
            },
          },
        },
      },
    ],
  });

  assert.deepEqual(state.panels.map((panel) => panel.target?.kind), [
    "mobkit/identity-chat",
    "mobkit/identity-inspect",
    "mobkit/routing",
    "mobkit/gating",
  ]);
  assert.equal(state.focusedPanelId, "p-chat");
});

test("unknown host targets remain inertly persistable when namespaced", () => {
  const target = migrateConsoleWorkbenchTarget({
    id: "project:alpha",
    kind: "host/project",
    title: "Project Alpha",
    payloadVersion: 3,
    payload: { projectId: "alpha" },
  });

  assert.equal(target?.kind, "host/project");
  assert.equal(target?.kind === "host/project" ? target.provenance : null, "host");
  assert.equal(target?.kind === "host/project" ? target.payloadVersion : null, 3);
  assert.equal(migrateConsoleWorkbenchTarget({ id: "bad", kind: "project", title: "Bad" }), null);
});
