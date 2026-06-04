# console-components

`console-components` owns the reusable React surfaces for the shared console UI.

## Workspace-stable exports

This package is currently private to the MobKit console workspace. The exports
below are the curated internal surface used by local console code and proving
fixtures; they are not an npm-public API promise.

Components:

- `ConversationEmptyState`
- `ConsoleConversationPanel`
- `ConversationPane`
- `ConversationTranscript`
- `ConsoleActivityRail`
- `ConsoleComposer`
- `ConsoleDock`
- `ConsolePendingStack`
- `ConsoleSidebar`
- `TopologyPanel`
- `ConsoleWorkbench`

Hooks:

- `useConsoleDockController`

Types:

- `ConversationEmptyStateProps`
- `ConsoleConversationPanelPhase`
- `ConsoleConversationPanelProps`
- `ConversationPaneProps`
- `ConversationTranscriptProps`
- `ConsoleActivityRailProps`
- `ConsoleComposerProps`
- `ConsoleDockController`
- `ConsoleDockProps`
- `ConsolePendingDropWhere`
- `ConsolePendingItem`
- `ConsolePendingStackProps`
- `ConsoleSidebarActionButtonScope`
- `ConsoleSidebarItemTrailingRenderArgs`
- `ConsoleSidebarProps`
- `ConsoleSidebarSectionContainerRenderArgs`
- `ConsoleSidebarSectionHeaderRenderArgs`
- `ConsoleTopologyNode`
- `ConsoleWorkbenchProps`
- `IconRenderer`
- `UseConsoleDockControllerOptions`

## Shared stylesheet

```ts
import "@console-components/styles";
```

The shared stylesheet is organized by domain:

- `tokens.css`
- `themes.css`
- `conversation.css`
- `conversation-panel.css`
- `workbench.css`
- `dock.css`
- `sidebar.css`
- `activity.css`
- `composer.css`
- `pending-stack.css`
- `topology.css`

## Host token contract

Shared styles now speak only `--cc-*` tokens. Set them on a single host root or wrapper that contains the shared surfaces.

Theme activation is wrapper-driven too:

- Set `data-cc-theme="light"` on the wrapper to force light mode in a package-owned way.
- Set `data-cc-theme="dark"` on a nested wrapper to force a local dark island inside a light host.
- Hosts with their own theme system should mirror the resolved theme onto `data-cc-theme`.

The most useful overrides are:

- `--cc-window-scale`
- `--cc-content-width`
- `--cc-bubble-max-width`
- `--cc-composer-width`
- `--cc-workbench-sidebar-width`
- `--cc-workbench-activity-width`
- `--cc-sidebar-safe-top`
- `--cc-sidebar-pad-left`
- `--cc-sidebar-pad-right`
- `--cc-member-accent`
- `--cc-member-accent-soft`
- `--cc-member-surface`

## Integration model

- Shared components render normalized props only.
- Shared components do not import app stores, Electron APIs, or network code.
- `useConsoleDockController` owns dock layout, tabs, splits, focus, and target placement only.
- Hosts keep panel-local session state outside the shared hook.
- External hosts must not depend on the private `@console-components` package
  name until MobKit makes an explicit public-package or SDK-subpath decision.

## Minimal recipe

1. Import the shared stylesheet.
2. Build a host target union on top of `ConsoleDockTarget`.
3. Use `useConsoleDockController` with a host `createPanelState` callback.
4. Render:
   - `ConsoleSidebar`
   - `ConsoleDock`
   - `ConsoleActivityRail`
   - `ConsoleComposer`
   - `ConsolePendingStack`
   - `TopologyPanel`
   - `ConsoleWorkbench`
5. Resolve each dock target into timeline entries plus host callbacks and render
   `ConsoleConversationPanel`.
