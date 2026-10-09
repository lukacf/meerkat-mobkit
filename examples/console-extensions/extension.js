// A dependency-free ES module. A plugin may instead bundle React, Vue, or another UI.
// Mounts own their container. Runtime data and permissions come from context.
export default {
  id: "example-results",
  panels: [{
    id: "example/results",
    title: "Results",
    mount(container, context) {
      const heading = document.createElement("h2");
      heading.textContent = "Application results";
      const description = document.createElement("p");
      description.textContent = "This custom panel shares the stock console's tabs and split panes.";
      const status = document.createElement("p");
      const update = next => { status.textContent = next.readOnly ? "View only" : "Interactive console"; };
      update(context);
      container.append(heading, description, status);
      return { update, dispose() { container.replaceChildren(); } };
    },
  }],
  widgets: [{
    type: "example/result-count",
    version: 1,
    mount(container, context) {
      const title = document.createElement("strong");
      const summary = document.createElement("p");
      const button = document.createElement("button");
      title.textContent = "Search results";
      button.textContent = "Open results panel";
      button.style.cssText = "padding:8px 12px;border:1px solid currentColor;border-radius:6px;background:transparent;color:inherit;cursor:pointer";
      const update = next => {
        const count = next.widget.data?.count;
        summary.textContent = Number.isSafeInteger(count) && count >= 0
          ? `${count} matching records` : next.widget.fallback;
        button.onclick = () => next.openPanel("example/results", "split_right");
      };
      update(context);
      container.style.cssText = "padding:16px;border:1px solid currentColor;border-radius:8px";
      container.append(title, summary, button);
      return { update, dispose() { button.onclick = null; container.replaceChildren(); } };
    },
  }],
};
