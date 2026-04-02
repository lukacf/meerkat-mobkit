# console-core

`console-core` owns the normalized, UI-facing model for the shared console surfaces.

## Stable domains

- `conversation`
- `dock`
- `sidebar`
- `activity`
- `rich-content`

## What stays here

- transcript/view-model types
- dock state, presets, reducer-style operations, and view-state builders
- sidebar schema normalization
- activity-rail view-model types
- rich-content parsing and structured block helpers

## What stays app-side

- Zustand or other app stores
- Electron/Desktop bridge code
- network or RPC calls
- panel-local session state such as drafts, provider/model selection, permissions, branches, and execution mode
- host-specific adapters from app contracts into shared models

## Consumer recipe

1. Define a host target union that extends `ConsoleDockTarget`.
2. Use `createConsoleDockState`, `openConsoleDockTarget`, `splitConsoleDockPanel`, and related helpers directly, or pair them with `useConsoleDockController` from `console-components`.
3. Resolve host transcript/sidebar/activity data into:
   - `ConversationViewState`
   - `ConsoleSidebarViewState`
   - `ConsoleActivityRailViewState`
4. Keep any panel-local state in a host-owned map keyed by `panelId`.

## Public entrypoint

Import from `@console-core`. The root barrel is curated and explicit; compatibility aliases should not be treated as stable package API.

The legacy Node runtime helpers remain available at `@console-core/runtime` for existing consumers that still rely on the package root helper bundle.
