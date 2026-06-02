import assert from "node:assert/strict";
import test from "node:test";

import {
  applyConsoleNavigationReorderIntent,
  canMoveConsoleNavigationNode,
  consoleNavigationFromSidebarViewState,
  consoleNavigationToSidebarViewState,
  moveConsoleNavigationNode,
  normalizeConsoleNavigationModel,
  pinConsoleNavigationNode,
  selectConsoleNavigationNode,
  toggleConsoleNavigationGroup,
  type ConsoleNavigationModel,
} from "./navigation";
import { normalizeConsoleSidebarViewState } from "./sidebar";

test("ConsoleNavigationModel normalizes invalid nodes and preserves layout-neutral order", () => {
  const model = normalizeConsoleNavigationModel({
    orientation: "horizontal",
    activeNodeId: "thread-alpha",
    nodes: [
      {
        type: "group",
        id: "threads",
        label: "Threads",
        expanded: true,
        children: [
          { type: "item", id: "thread-alpha", label: "Alpha" },
          { type: "item", id: "", label: "" },
        ],
      },
    ],
    order: { orderedNodeIds: [] },
  });

  assert.equal(model.orientation, "horizontal");
  assert.equal(model.activeNodeId, "thread-alpha");
  assert.deepEqual(model.order.orderedNodeIds, ["threads", "thread-alpha"]);
});

test("navigation operations select, toggle, and pin without depending on sidebar layout", () => {
  const model = baseNavigationModel();

  const selected = selectConsoleNavigationNode(model, "thread-beta");
  assert.equal(selected.activeNodeId, "thread-beta");
  assert.equal(selected.focusNodeId, "thread-beta");

  const collapsed = toggleConsoleNavigationGroup(selected, "threads");
  const group = collapsed.nodes[0];
  assert.equal(group?.type, "group");
  assert.equal(group?.type === "group" ? group.expanded : true, false);

  const pinned = pinConsoleNavigationNode(collapsed, "thread-beta", true);
  const item = pinned.nodes[0]?.type === "group" ? pinned.nodes[0].children[1] : null;
  assert.equal(item?.type, "item");
  assert.equal(item?.type === "item" ? item.pinned : false, true);
});

test("navigation move validates disabled, descendant, inside, and sibling constraints", () => {
  const model = baseNavigationModel();

  assert.equal(canMoveConsoleNavigationNode(model, {
    id: "thread-alpha",
    targetId: "thread-beta",
    position: "after",
    scope: "siblings",
  }), true);
  assert.equal(canMoveConsoleNavigationNode(model, {
    id: "threads",
    targetId: "thread-alpha",
    position: "after",
  }), false);
  assert.equal(canMoveConsoleNavigationNode(model, {
    id: "thread-alpha",
    targetId: "thread-beta",
    position: "inside",
  }), false);

  const moved = moveConsoleNavigationNode(model, {
    id: "thread-alpha",
    targetId: "thread-beta",
    position: "after",
    scope: "siblings",
  });

  const children = moved.model.nodes[0]?.type === "group" ? moved.model.nodes[0].children : [];
  assert.deepEqual(children.map((child) => child.id), ["thread-beta", "thread-alpha"]);
  assert.equal(moved.focusNodeId, "thread-alpha");
  assert.equal(moved.announcement, "Moved Alpha after Beta.");
});

test("keyboard and pointer reorder intents use the same navigation move operation", () => {
  const model = baseNavigationModel();
  const keyboard = applyConsoleNavigationReorderIntent(model, {
    inputSource: "keyboard",
    id: "thread-alpha",
    targetId: "thread-beta",
    position: "after",
    scope: "siblings",
  });
  const pointer = applyConsoleNavigationReorderIntent(model, {
    inputSource: "pointer",
    id: "thread-alpha",
    targetId: "thread-beta",
    position: "after",
    scope: "siblings",
  });

  assert.deepEqual(
    keyboard.model.order.orderedNodeIds,
    pointer.model.order.orderedNodeIds,
  );
  assert.equal(keyboard.focusNodeId, pointer.focusNodeId);
  assert.equal(keyboard.announcement, pointer.announcement);
});

test("sidebar compatibility adapters round-trip existing ConsoleSidebarViewState", () => {
  const sidebar = {
    blocks: [
      {
        id: "primary",
        kind: "action_strip" as const,
        actions: [{ id: "new", label: "New thread", iconName: "i-plus" }],
      },
      {
        id: "agents",
        kind: "list" as const,
        title: "Agents",
        meta: [{ id: "count", label: "2" }],
        sections: [
          {
            id: "coordinators",
            title: "Coordinators",
            iconName: "i-bolt",
            actions: [{ id: "create", label: "Create", iconName: "i-plus" }],
            items: [
              {
                id: "identity:luka",
                title: "Luka",
                subtitle: "identity:luka",
                selected: true,
                pinned: true,
                meta: [{ id: "state", label: "Running", tone: "accent" as const }],
                actions: [{ id: "inspect", label: "Inspect", iconName: "i-terminal" }],
              },
            ],
          },
        ],
      },
    ],
  };

  const navigation = consoleNavigationFromSidebarViewState(sidebar);
  assert.equal(navigation.nodes.length, 2);
  assert.equal(navigation.nodes[1]?.type, "group");
  assert.equal(navigation.nodes[1]?.label, "Coordinators");

  assert.deepEqual(
    consoleNavigationToSidebarViewState(navigation),
    normalizeConsoleSidebarViewState(sidebar),
  );
});

function baseNavigationModel(): ConsoleNavigationModel {
  return normalizeConsoleNavigationModel({
    nodes: [
      {
        type: "group",
        id: "threads",
        label: "Threads",
        expanded: true,
        children: [
          { type: "item", id: "thread-alpha", label: "Alpha" },
          { type: "item", id: "thread-beta", label: "Beta" },
        ],
      },
      {
        type: "group",
        id: "disabled",
        label: "Disabled",
        disabled: true,
        expanded: true,
        children: [],
      },
    ],
    order: { orderedNodeIds: [] },
  });
}
