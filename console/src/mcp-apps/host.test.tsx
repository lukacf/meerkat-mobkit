import React from 'react';
import { act, render, screen, waitFor } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import { ConsoleMcpAppView, ConsoleMcpAppsProvider, McpAppTransport, mcpSandboxUrl, type ConsoleMcpAppSession } from '../../../packages/console-components/src/mcp-apps';

const locator = { identity: 'member:one', sessionId: 'session:one', toolCallId: 'call:one' };
const session = (): ConsoleMcpAppSession => ({
  tool: { name: 'find', inputSchema: { type: 'object' }, _meta: { ui: { resourceUri: 'ui://example/result' } } },
  arguments: { query: 'test' },
  result: { content: [{ type: 'text', text: 'Seven results' }], structuredContent: { count: 7 }, _meta: { privateViewField: 'detail' } },
  readResource: vi.fn(async () => ({ contents: [{ uri: 'ui://example/result', mimeType: 'text/html;profile=mcp-app', text: '<!doctype html><html><body>App</body></html>' }] })),
  callTool: vi.fn(async () => ({ content: [{ type: 'text', text: 'Updated' }] })),
  dispose: vi.fn(),
});
function show(resolve, readOnly = false, authority = 'scope:one') {
  return <ConsoleMcpAppsProvider value={{ host: { sandboxProxyUrl: 'https://sandbox.test/sandbox.html', resolve }, readOnly, authority }}>
    <ConsoleMcpAppView locator={locator} fallback="Seven results" />
  </ConsoleMcpAppsProvider>;
}
function message(frame: HTMLIFrameElement, method: string, params: unknown, id?: number) {
  window.dispatchEvent(new MessageEvent('message', { source: frame.contentWindow, origin: 'https://sandbox.test', data: { jsonrpc: '2.0', method, params, ...(id === undefined ? {} : { id }) } }));
}
async function initialize(frame: HTMLIFrameElement, sent) {
  await act(async () => message(frame, 'ui/notifications/sandbox-proxy-ready', {}));
  await waitFor(() => expect(sent.mock.calls.some(([m]) => m.method === 'ui/notifications/sandbox-resource-ready')).toBe(true));
  await act(async () => message(frame, 'ui/initialize', { protocolVersion: '2026-01-26', appInfo: { name: 'Example', version: '1.0.0' }, appCapabilities: { availableDisplayModes: ['inline'] } }, 1));
  await waitFor(() => expect(sent.mock.calls.some(([m]) => m.id === 1 && m.result)).toBe(true));
  await act(async () => message(frame, 'ui/notifications/initialized', {}));
}

describe('standard MCP Apps host', () => {
  it('replays the original input and full result after the real SDK handshake, then proxies actions once', async () => {
    const binding = session();
    const resolve = vi.fn(async () => binding);
    const view = render(show(resolve));
    const frame = screen.getByTitle('Interactive tool result') as HTMLIFrameElement;
    await waitFor(() => expect(frame.src).toContain('sandbox.test'));
    const sent = vi.spyOn(frame.contentWindow!, 'postMessage');
    await initialize(frame, sent);
    await waitFor(() => expect(sent.mock.calls.some(([m]) => m.method === 'ui/notifications/tool-result')).toBe(true));
    const notifications = sent.mock.calls.map(([m]) => m).filter(m => m.method?.startsWith('ui/notifications/tool-'));
    expect(notifications.map(m => m.method)).toEqual(['ui/notifications/tool-input', 'ui/notifications/tool-result']);
    expect(notifications[1].params).toEqual(binding.result);
    expect(resolve).toHaveBeenCalledWith(locator, expect.any(AbortSignal));
    expect(binding.callTool).not.toHaveBeenCalled();
    await act(async () => message(frame, 'tools/call', { name: 'refresh', arguments: { page: 2 } }, 2));
    await waitFor(() => expect(sent.mock.calls.find(([m]) => m.id === 2)?.[0]).toMatchObject({ result: { content: [{ type: 'text', text: 'Updated' }] } }));
    expect(binding.callTool).toHaveBeenCalledTimes(1);
    expect(binding.callTool).toHaveBeenCalledWith('refresh', { page: 2 }, expect.any(AbortSignal));
    view.unmount();
    expect(binding.dispose).toHaveBeenCalledTimes(1);
  });

  it('keeps text fallback for missing UI resources without invoking a tool', async () => {
    const binding = session(); binding.readResource = vi.fn(async () => ({ contents: [{ uri: 'ui://example/result', mimeType: 'text/plain', text: 'Wrong type' }] }));
    render(show(async () => binding));
    await waitFor(() => expect(screen.getByRole('status')).toHaveTextContent('Seven results'));
    await waitFor(() => expect(binding.dispose).toHaveBeenCalledTimes(1));
    expect(screen.getByTitle('Interactive tool result')).not.toBeVisible();
    expect(binding.callTool).not.toHaveBeenCalled();
  });

  it('rejects app actions when the console is view-only', async () => {
    const binding = session();
    render(show(async () => binding, true));
    const frame = screen.getByTitle('Interactive tool result') as HTMLIFrameElement;
    await waitFor(() => expect(frame.src).toContain('sandbox.test'));
    const sent = vi.spyOn(frame.contentWindow!, 'postMessage');
    await initialize(frame, sent);
    await act(async () => message(frame, 'tools/call', { name: 'refresh', arguments: {} }, 2));
    await waitFor(() => expect(sent.mock.calls.some(([m]) => m.id === 2 && m.error)).toBe(true));
    expect(binding.callTool).not.toHaveBeenCalled();
  });

  it('disposes late resolved bindings after unmount', async () => {
    let finish: (value: ConsoleMcpAppSession) => void;
    const resolve = vi.fn(() => new Promise<ConsoleMcpAppSession>(done => { finish = done; }));
    const view = render(show(resolve));
    view.unmount();
    const binding = session();
    await act(async () => finish(binding));
    expect(binding.dispose).toHaveBeenCalledTimes(1);
    expect(binding.readResource).not.toHaveBeenCalled();
    expect(resolve.mock.calls[0][1].aborted).toBe(true);
  });

  it('aborts an in-flight widget action on unmount and never replays it', async () => {
    const binding = session();
    let finish: (value: any) => void;
    let actionSignal: AbortSignal;
    binding.callTool = vi.fn((_name, _args, signal) => {
      actionSignal = signal;
      return new Promise(done => { finish = done; });
    });
    const view = render(show(async () => binding));
    const frame = screen.getByTitle('Interactive tool result') as HTMLIFrameElement;
    await waitFor(() => expect(frame.src).toContain('sandbox.test'));
    const sent = vi.spyOn(frame.contentWindow!, 'postMessage');
    await initialize(frame, sent);
    await act(async () => message(frame, 'tools/call', { name: 'refresh', arguments: {} }, 2));
    await waitFor(() => expect(binding.callTool).toHaveBeenCalledTimes(1));
    view.unmount();
    expect(actionSignal!.aborted).toBe(true);
    await act(async () => finish!({ content: [{ type: 'text', text: 'Late private value' }] }));
    expect(binding.callTool).toHaveBeenCalledTimes(1);
    expect(sent.mock.calls.some(([m]) => m.id === 2 && m.result)).toBe(false);
  });

  it('requires an isolated HTTP sandbox and checks both source and origin', async () => {
    expect(() => mcpSandboxUrl('https://console.test/sandbox', 'https://console.test')).toThrow(/separate/);
    expect(() => mcpSandboxUrl('data:text/html,hi', 'https://console.test')).toThrow();
    const target = document.createElement('iframe'); document.body.append(target);
    const transport = new McpAppTransport(target.contentWindow!, 'https://sandbox.test');
    transport.onmessage = vi.fn(); await transport.start();
    const data = { jsonrpc: '2.0', method: 'ping', id: 1 };
    for (const event of [
      { source: window, origin: 'https://sandbox.test' },
      { source: target.contentWindow, origin: 'https://other.test' },
    ]) window.dispatchEvent(new MessageEvent('message', { ...event, data }));
    expect(transport.onmessage).not.toHaveBeenCalled();
    window.dispatchEvent(new MessageEvent('message', { source: target.contentWindow, origin: 'https://sandbox.test', data }));
    expect(transport.onmessage).toHaveBeenCalledTimes(1);
    await transport.close(); target.remove();
  });
});
