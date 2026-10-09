# Stock console extensions

`extension.js` adds a Results workbench panel and a result-count chat widget.
It uses the DOM directly and needs no build step or framework dependency.

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

MCP-style results can put `console_widget` inside `structuredContent`.
Keep a useful `fallback`: it is shown when the plugin is missing, its version
is unsupported, or its mount fails, and is used for copying the transcript.
The ordinary tool row is retained as execution evidence.

For an embedded console, import the extension and pass
`createConsoleApp(root, { baseUrl, extensions: [extension] })`.
Both entry points use the same registrations. Full API documentation is in
`docs/guides/console.mdx`, under Custom panels and chat widgets.
