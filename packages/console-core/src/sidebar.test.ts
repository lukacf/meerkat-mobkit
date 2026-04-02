import { normalizeConsoleSidebarViewState } from "./sidebar";

describe("normalizeConsoleSidebarViewState", () => {
  test("keeps valid action-strip and list blocks while filtering incomplete data", () => {
    const viewState = normalizeConsoleSidebarViewState({
      blocks: [
        {
          id: "primary",
          kind: "action_strip",
          actions: [
            { id: "new", label: "New thread", iconName: "i-new-thread" },
            { id: "", label: "" },
          ],
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
                  actions: [{ id: "pin", label: "Pin thread", iconName: "i-pin" }],
                },
                {
                  id: "",
                  title: "",
                },
              ],
            },
            {
              id: "empty",
              title: "",
              items: [],
            },
          ],
        },
      ],
    });

    const threadBlock = viewState.blocks[1];
    const firstSection = threadBlock?.sections?.[0];

    expect(viewState.blocks).toHaveLength(2);
    expect(viewState.blocks[0]?.actions).toHaveLength(1);
    expect(threadBlock?.sections).toHaveLength(1);
    expect(firstSection?.items).toHaveLength(1);
  });
});
