# MCP Apps host integration

This is independent of `customPanels` and `panel_modules`. App authors use
standard MCP tool `_meta.ui.resourceUri`, `ui://` resources, the
`text/html;profile=mcp-app` MIME type and standard MCP Apps communication.
No Meta manifest, custom result block, or renderer registration is required.

## Implemented frontend

`ConsoleMcpAppView` uses the official `@modelcontextprotocol/ext-apps` AppBridge.
`ConsoleMcpAppsHost` is a host integration adapter, not an app-facing contract.
A `resolve` call opens a view for the exact identity/session/tool-call locator,
returning its original standard Tool, arguments and CallToolResult. Resource
reads and app actions use closures bound to that same runtime registration.
An optional resource listing supplies standard UI metadata when resources/read
omits it. No result body is written to browser storage.

The host delivers input and the full result after initialization, supports
resource reads and optional tool calls, bounds view size, and rejects actions in
view-only mode. It does not retry calls. Missing resources, invalid MIME types,
initialization failures and unavailable bindings retain the provided fallback.
Closing a view aborts its requests and disposes its subscriptions. It must not
close the shared MCP connection.

`ConsoleMcpAppsProvider` is available to reusable conversation hosts.
`ConsoleApp` / `createConsoleApp` accept a separate `mcpAppsHost` option. The
conversation projection carries an `mcpApp` locator on a message entry. It must
come from authenticated runtime observation of an actual UI-enabled tool call,
never from arbitrary tool text or model-generated JSON.

## Native integration still required

Automatic discovery and durable invocation projection in the stock gateway are
not wired yet. The coordinator's native MCP owner is rewriting per-registration
observation and has requested no parallel edits to that owner until its commit
lands. A frontend fixture is not evidence of native admission, durable replay,
or application authorization.

The native adapter must:

- Resolve the exact originating member and registration through existing MCP
  observation/composer ownership. Recheck viewer access on every open/read/action.
- Preserve standard tool metadata and full CallToolResult, including `_meta`, in
  the host channel and durable session history. Keep UI-only data out of LLM input.
- Restore an existing invocation without executing its original tool again.
- Read resources through the existing authenticated MCP connection, preserving
  cancellation, generation/retirement checks and in-flight accounting.
- Admit app actions through the member's existing runtime policy and approval
  path, with standard app visibility and same-registration restrictions.
- Exclude app-only tools from model discovery. Do not broaden the model catalog
  to make app actions pass its current visibility precheck.
- Advertise the standard UI capability only when those operations are supported.

Do not construct another browser-owned MCP connection, bypass the native action
path, or treat client locators as authorization. The coordinator has the exact
native seam request and will route it after the current ownership commit.

## Sandbox deployment

Serve `console/mcp-apps-sandbox.cjs` on a dedicated origin, separate from Console
and its authentication cookies. It exposes only the sandbox proxy; do not serve
application APIs or sensitive files on that origin. Configure its exact allowed
Console origin. The UI resource's declared domains become HTTP CSP headers;
network and nested frames are denied by default. The inner view has an opaque
origin. Camera, microphone, geolocation, downloads, popups and form submission
are unavailable in this host profile. External-link host requests are unsupported.

For a local proxy:

```sh
MCP_APPS_HOST_ORIGIN=http://127.0.0.1:5000 MCP_APPS_SANDBOX_PORT=5001 node console/mcp-apps-sandbox.cjs
```

For the deterministic protocol/rendering fixture:

```sh
node console/mcp-apps-preview.cjs
```

The fixture uses in-memory host data and standard App/AppBridge messages. It
intentionally does not simulate the native MCP registration owner or claim its
policy path is verified. `/counts` reports resolve, resource-read and action
counts; the original tool result remains fixed across view reopening.
