// Runs only on the dedicated sandbox origin. The inner view has an opaque origin.
const view = document.createElement('iframe');
view.title = 'MCP App';
view.setAttribute('sandbox', 'allow-scripts');
view.referrerPolicy = 'no-referrer';
let loaded = false;
window.addEventListener('message', event => {
  if (!event.data || event.data.jsonrpc !== '2.0') return;
  const method = event.data.method;
  const sandboxMessage = typeof method === 'string' && method.startsWith('ui/notifications/sandbox-');
  if (event.source === window.parent && event.origin === config.hostOrigin) {
    if (method === 'ui/notifications/sandbox-resource-ready' && !loaded) {
      const html = event.data.params?.html;
      if (typeof html !== 'string' || html.length > 5_000_000) return;
      loaded = true;
      // Parse inertly and put policy before any executable content. The outer
      // HTTP CSP is also inherited, so injected policy can only tighten it.
      const doc = new DOMParser().parseFromString(html, 'text/html');
      const policy = doc.createElement('meta');
      policy.httpEquiv = 'Content-Security-Policy';
      policy.content = config.innerCsp;
      doc.head.prepend(policy);
      view.srcdoc = '<!doctype html>' + doc.documentElement.outerHTML;
      document.body.append(view);
    } else if (!sandboxMessage && loaded) {
      view.contentWindow.postMessage(event.data, '*');
    }
  } else if (event.source === view.contentWindow && event.origin === 'null' && !sandboxMessage) {
    window.parent.postMessage(event.data, config.hostOrigin);
  }
});
window.parent.postMessage({ jsonrpc: '2.0', method: 'ui/notifications/sandbox-proxy-ready', params: {} }, config.hostOrigin);
