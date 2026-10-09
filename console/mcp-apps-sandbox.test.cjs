const assert = require('node:assert/strict');
const { test } = require('node:test');
const { appCsp, createSandboxServer } = require('./mcp-apps-sandbox.cjs');
test('CSP defaults deny network and undeclared nested frames', () => {
  const policy = appCsp();
  assert.match(policy, /connect-src 'none'/);
  assert.match(policy, /frame-src 'none'/);
  assert.match(policy, /object-src 'none'/);
  assert.match(policy, /form-action 'none'/);
  assert.match(appCsp({ connectDomains: ['https://api.example.test'] }), /connect-src https:\/\/api.example.test/);
  for (const domain of ['*', 'https://example.test; script-src *', 'https://user@example.test', 'javascript:alert(1)', 'https://example.test/path']) {
    assert.throws(() => appCsp({ connectDomains: [domain] }));
  }
});
test('sandbox response applies policy in HTTP headers and rejects unapproved parents', async () => {
  const server = createSandboxServer({ allowedHostOrigins: ['https://console.example.test'] });
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  try {
    const root = `http://127.0.0.1:${server.address().port}/sandbox.html`;
    const valid = await fetch(root + '?hostOrigin=' + encodeURIComponent('https://console.example.test'));
    assert.equal(valid.status, 200);
    assert.match(valid.headers.get('content-security-policy'), /frame-ancestors https:\/\/console.example.test/);
    assert.match(valid.headers.get('content-security-policy'), /connect-src 'none'/);
    assert.equal(valid.headers.get('cache-control'), 'no-store');
    assert.equal((await fetch(root + '?hostOrigin=https://other.test')).status, 400);
    const bad = new URL(root); bad.searchParams.set('hostOrigin', 'https://console.example.test'); bad.searchParams.set('csp', JSON.stringify({ resourceDomains: ["https://x.test; default-src *"] }));
    assert.equal((await fetch(bad)).status, 400);
  } finally { await new Promise(resolve => server.close(resolve)); }
});
