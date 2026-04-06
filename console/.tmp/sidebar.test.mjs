// ../packages/console-core/src/sidebar.test.ts
import assert from "node:assert/strict";
import test from "node:test";

// ../packages/console-core/src/control-plane.ts
function trimString(value) {
  if (typeof value !== "string") {
    return void 0;
  }
  const trimmed = value.trim();
  return trimmed || void 0;
}
function normalizeSidebarWatchFields(value) {
  const record = value && typeof value === "object" ? value : {};
  const normalized = {};
  if (typeof record.watched === "boolean") {
    normalized.watched = record.watched;
  }
  if (record.alertLevel === "elevated" || record.alertLevel === "critical" || record.alertLevel === null) {
    normalized.alertLevel = record.alertLevel;
  }
  if (typeof record.degraded === "boolean") {
    normalized.degraded = record.degraded;
  }
  const degradedReason = trimString(record.degradedReason);
  if (degradedReason) {
    normalized.degradedReason = degradedReason;
  }
  return normalized;
}

// ../packages/console-core/src/sidebar.ts
function normalizeMeta(meta) {
  return (meta || []).filter((item) => Boolean(item?.label));
}
function normalizeActions(actions) {
  return (actions || []).filter((action) => Boolean(action?.id && action?.label));
}
function normalizeItems(items) {
  return (items || []).filter((item) => Boolean(item?.id && item?.title)).map((item) => ({
    ...item,
    ...normalizeSidebarWatchFields(item),
    meta: normalizeMeta(item.meta),
    actions: normalizeActions(item.actions)
  }));
}
function normalizeSections(sections) {
  return (sections || []).filter((section) => Boolean(section?.id && typeof section?.title === "string")).map((section) => ({
    ...section,
    meta: normalizeMeta(section.meta),
    actions: normalizeActions(section.actions),
    items: normalizeItems(section.items)
  })).filter((section) => {
    if (section.items.length > 0) {
      return true;
    }
    return Boolean(
      section.title || section.subtitle || section.iconName || section.actions.length || section.meta.length
    );
  });
}
function normalizeConsoleSidebarViewState(viewState) {
  const blocks = (viewState?.blocks || []).filter((block) => Boolean(block?.id && block?.kind)).map((block) => ({
    ...block,
    meta: normalizeMeta(block.meta),
    actions: normalizeActions(block.actions),
    sections: normalizeSections(block.sections)
  })).filter((block) => {
    if (block.kind === "action_strip") {
      return block.actions.length > 0;
    }
    if (block.sections.length > 0) {
      return true;
    }
    return Boolean(block.title || block.meta.length || block.actions.length);
  });
  return { blocks };
}

// ../packages/console-core/src/sidebar.test.ts
test("normalizeConsoleSidebarViewState keeps valid action-strip and list blocks while filtering incomplete data", () => {
  const viewState = normalizeConsoleSidebarViewState({
    blocks: [
      {
        id: "primary",
        kind: "action_strip",
        actions: [
          { id: "new", label: "New thread", iconName: "i-new-thread" },
          { id: "", label: "" }
        ]
      },
      {
        id: "threads",
        kind: "list",
        title: "Threads",
        actions: [{ id: "filter", label: "Filter", iconName: "i-sliders" }],
        sections: [
          {
            id: "workspace",
            title: "workspace",
            actions: [{ id: "create", label: "New thread", iconName: "i-plus" }],
            items: [
              {
                id: "thread-1",
                title: "Sidebar extraction",
                actions: [{ id: "pin", label: "Pin thread", iconName: "i-pin" }]
              },
              {
                id: "",
                title: ""
              }
            ]
          },
          {
            id: "empty",
            title: "",
            items: []
          }
        ]
      }
    ]
  });
  const threadBlock = viewState.blocks[1];
  const firstSection = threadBlock?.sections?.[0];
  assert.equal(viewState.blocks.length, 2);
  assert.equal(viewState.blocks[0]?.actions?.length, 1);
  assert.equal(threadBlock?.sections?.length, 1);
  assert.equal(firstSection?.items?.length, 1);
});
test("normalizeConsoleSidebarViewState preserves watch fields and warning tones on sidebar items", () => {
  const viewState = normalizeConsoleSidebarViewState({
    blocks: [
      {
        id: "agents",
        kind: "list",
        sections: [
          {
            id: "ops",
            title: "Ops",
            items: [
              {
                id: "identity:luka",
                title: "Luka",
                watched: true,
                alertLevel: "critical",
                degraded: true,
                degradedReason: "peer_unreachable",
                meta: [{ id: "alert", label: "Critical", tone: "warning" }]
              }
            ]
          }
        ]
      }
    ]
  });
  const item = viewState.blocks[0]?.sections?.[0]?.items?.[0];
  assert.equal(item?.watched, true);
  assert.equal(item?.alertLevel, "critical");
  assert.equal(item?.degraded, true);
  assert.equal(item?.degradedReason, "peer_unreachable");
  assert.equal(item?.meta?.[0]?.tone, "warning");
});
