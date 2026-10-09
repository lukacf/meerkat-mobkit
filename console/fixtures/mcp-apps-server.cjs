// A real stdio MCP server for native Console acceptance. No host projection mocks.
const fs = require('node:fs');
const readline = require('node:readline');
const uri = 'ui://fixture/results.html';
let count = 7;
const log = value => fs.appendFileSync(process.env.MOBKIT_FIXTURE_APP_LOG, JSON.stringify({ pid: process.pid, ...value }) + '\n');
const result = () => ({
  content: [{ type: 'text', text: JSON.stringify({ count }) }],
  structuredContent: { count },
  _meta: { viewDetail: 'PRIVATE_APP_DETAIL' },
});
const lines = readline.createInterface({ input: process.stdin });
lines.on('line', line => {
  const request = JSON.parse(line);
  if (request.id === undefined) return;
  log({ method: request.method, params: request.params });
  let value, error;
  switch (request.method) {
    case 'initialize':
      value = { protocolVersion: request.params.protocolVersion, serverInfo: { name: 'native-apps-fixture', version: '1' }, capabilities: { tools: {}, resources: {} } }; break;
    case 'ping': value = {}; break;
    case 'tools/list':
      value = { tools: [
        { name: 'display', description: 'Display matching records', inputSchema: { type: 'object', properties: { query: { type: 'string' } } }, _meta: { ui: { resourceUri: uri, visibility: ['model'] } } },
        { name: 'refresh', description: 'Refresh records from the app', inputSchema: { type: 'object' }, _meta: { ui: { resourceUri: uri, visibility: ['app'] } } },
      ] }; break;
    case 'resources/list': value = { resources: [] }; break;
    case 'resources/read':
      if (request.params.uri === uri) value = { contents: [{ uri, mimeType: 'text/html;profile=mcp-app', text: fs.readFileSync(process.env.MOBKIT_FIXTURE_APP_HTML, 'utf8') }] };
      else error = { code: -32602, message: 'Unknown resource' };
      break;
    case 'tools/call':
      if (request.params.name === 'refresh') { count++; value = result(); }
      else if (request.params.name === 'display') value = result();
      else error = { code: -32602, message: 'Unknown tool' };
      break;
    default: error = { code: -32601, message: 'Unknown method' };
  }
  process.stdout.write(JSON.stringify({ jsonrpc: '2.0', id: request.id, ...(error ? { error } : { result: value }) }) + '\n');
});
