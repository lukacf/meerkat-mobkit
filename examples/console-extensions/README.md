# Stock console extensions

`extension.js` adds a Results workbench panel and a result-count chat widget.
It uses the DOM directly and needs no build step or framework dependency.
The widget opens a parameterized panel pinned to its original conversation.

To try it locally with fixture data:

```sh
npm ci --prefix console
npm --prefix console run build
node examples/console-extensions/preview.cjs
```

Open the printed URL. The fixture serves the actual stock console bundle and
loads `extension.js` through `console_config.extension_modules`. Its transcript
is synthetic and no agents are started. Click **Open results panel**, then
reload to verify that the dock layout persists.

In your application:

1. Serve `extension.js` as JavaScript at `/extensions/example.js` on the same
   origin as your console server, using your host or reverse proxy. The gateway
   does not serve arbitrary files from your workspace.
2. Add `extension_modules = ["/extensions/example.js"]` at the top level of
   `config/console.toml`. Restart the runtime and reload the console.
3. Have a tool return this object (or its JSON string):

```json
{
  "console_widget": {
    "type": "example/result-count",
    "version": 1,
    "data": { "count": 7 },
    "fallback": "Found seven matching records."
  }
}
```

For rich callbacks, prefer the canonical structured block:

```python
from meerkat_mobkit import console_widget_block, tool_content

result = tool_content(console_widget_block(
    "example/result-count", data={"count": 7},
    fallback="Found seven matching records.",
))
```

TypeScript has `consoleWidgetBlock` and `toolContent` with the same contract.
Combine that block with `text_block` / `image_block` as needed. The preview
uses the shared `crates/meerkat-mobkit/tests/fixtures/console-widget-rich-result.json` fixture,
qualified by SDK callback, Rust event/history projection, and console tests.
MCP-style results preserved by the integration can put `console_widget` inside
`structuredContent`. The JSON Schema is
`packages/console-core/schemas/console-widget.schema.json`.
Keep a useful `fallback`: it is shown when the plugin is missing, its version
is unsupported, or its mount fails, and is used for copying the transcript.
The ordinary tool row is retained as execution evidence.

For an embedded console, import the extension and pass
`createConsoleApp(root, { baseUrl, extensions: [extension] })`.
Hosts can also inject `extensionService`; plugins call
`context.request(request, signal)` with their pinned conversation context.
Both entry points use the same registrations. Full API documentation is in
`docs/guides/console.mdx`, under Custom panels and chat widgets.
