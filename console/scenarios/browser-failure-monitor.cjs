"use strict";
const assert = require("node:assert/strict");

const pageMonitors = new WeakMap();
const aborted = "net::ERR_ABORTED";

// Observe the context so popup requests are included from their first request.
// Expected failures name exact Playwright Request objects, never URL wildcards.
function browserFailureMonitor(context, { origin, initializationPrefixes = [""] }) {
  const authority = new URL(origin).origin;
  assert(initializationPrefixes.length && initializationPrefixes.every(prefix => prefix === "" || /^\/[a-zA-Z0-9_-]+(?:\/[a-zA-Z0-9_-]+)*$/.test(prefix)), "initialization prefixes must be explicit paths");
  const configuredPrefixes = new Set(initializationPrefixes);
  function actionPrefixes(initialize) {
    const selected = initialize === true ? initializationPrefixes : initialize || [];
    assert(Array.isArray(selected) && selected.every(prefix => configuredPrefixes.has(prefix)), "action must select a configured initialization prefix");
    return selected;
  }
  const errors = [], expected = [], failures = [];
  const active = new Map(), allowances = new Map(), actions = new Map(), pages = new Set();
  let popupAction = null;
  const requestPage = request => { try { return request.frame().page(); } catch { return null; } };
  function initializationKind(request, prefixes) {
    const url = new URL(request.url());
    if (url.origin !== authority) return null;
    const prefix = prefixes.find(item => url.pathname === `${item}/console/timeline/stream` || url.pathname === `${item}/console/rpc`);
    if (prefix === undefined) return null;
    if (request.method() === "GET" && url.pathname === `${prefix}/console/timeline/stream` && !url.search) return `${prefix}:stream`;
    if (request.method() !== "POST" || url.pathname !== `${prefix}/console/rpc`) return null;
    try {
      const body = JSON.parse(request.postData() || "{}");
      return body.method === "mobkit/console/query_timeline" && body.params?.mode === "recent"
        && body.params?.limit === 200 && !Object.hasOwn(body.params, "identity")
        && !Object.hasOwn(body.params, "before") && !Object.hasOwn(body.params, "after") ? `${prefix}:seed` : null;
    } catch { return null; }
  }
  function bind(request, reason, error, action, required = false) {
    assert(!allowances.has(request), "a request cannot have overlapping failure allowances");
    allowances.set(request, { reason, error, action, required });
  }
  function observeRequest(request) {
    const page = requestPage(request);
    // Popup subresource requests can precede the context's page event. Bind
    // only a real Request-owned page; frame-unavailable requests stay unknown.
    if (page) attachPage(page);
    active.set(request, page);
    const action = actions.get(page);
    if (!action?.prefixes.length) return;
    const kind = initializationKind(request, action.prefixes);
    if (!kind || action.initial.has(kind)) return;
    action.initial.add(kind);
    bind(request, `${action.reason}: authority initialization`, aborted, action);
  }
  function observeFinished(request) {
    active.delete(request);
    if (!allowances.get(request)?.required) allowances.delete(request);
  }
  function observeFailure(request) {
    const allowance = allowances.get(request);
    const failure = { method: request.method(), url: request.url(), postData: request.postData(), error: request.failure()?.errorText };
    const accepted = Boolean(allowance && failure.error === allowance.error);
    failures.push({ ...failure, expected: accepted, ...(accepted ? { reason: allowance.reason } : {}) });
    if (accepted) expected.push({ ...failure, reason: allowance.reason });
    else errors.push(`${failure.method} ${failure.url}: ${failure.error} ${failure.postData || ""}`);
    active.delete(request); allowances.delete(request);
  }
  function attachPage(page) {
    if (pages.has(page)) return;
    pages.add(page); pageMonitors.set(page, monitor);
    if (popupAction && !popupAction.page) {
      popupAction.page = page; actions.set(page, popupAction);
    }
  }
  // Context errors include exceptions before a popup page event. Listening to
  // this one owner event also avoids counting the mirrored pageerror twice.
  const observeWebError = webError => errors.push(webError.error().message);
  function finishAction(action) {
    for (const [request, allowance] of allowances) if (allowance.action === action) allowances.delete(request);
    for (const [page, value] of actions) if (value === action) actions.delete(page);
  }
  const monitor = {
    errors, expected, failures,
    expectFailure(request, reason, error) {
      assert(reason && error, "deliberate request failure requires an action reason and exact error");
      assert(active.has(request), "deliberate failure must identify an observed in-flight request");
      bind(request, reason, error, null, true);
    },
    async during(page, reason, action, { priorRequests = () => true, initialize = false } = {}) {
      assert(!actions.has(page), "failure action scopes must not overlap on a page");
      const scope = { reason, prefixes: actionPrefixes(initialize), initial: new Set() };
      actions.set(page, scope);
      try {
        for (const [request, owner] of active) if (owner === page && priorRequests(request)) {
          bind(request, `${reason}: prior document request`, aborted, scope);
        }
        return await action();
      } finally { finishAction(scope); }
    },
    navigation(page, reason, action) {
      return monitor.during(page, reason, action, { initialize: true });
    },
    async popup(reason, action) {
      assert.equal(popupAction, null, "popup action scopes must not overlap");
      const scope = { reason, prefixes: actionPrefixes(true), initial: new Set(), page: null };
      popupAction = scope;
      try { return await action(); } finally { popupAction = null; finishAction(scope); }
    },
    assertClean() {
      assert.deepEqual(errors, [], "no unexpected browser exceptions or failed requests");
      const missing = [...allowances].filter(([, allowance]) => allowance.required).map(([request, allowance]) => ({
        method: request.method(), url: request.url(), reason: allowance.reason,
      }));
      assert.deepEqual(missing, [], "expected request failure was not observed");
    },
    stop() {
      context.off("request", observeRequest); context.off("requestfinished", observeFinished);
      context.off("requestfailed", observeFailure); context.off("page", attachPage);
      context.off("weberror", observeWebError);
      for (const page of pages) pageMonitors.delete(page);
      pages.clear();
    },
  };
  for (const page of context.pages()) attachPage(page);
  context.on("page", attachPage); context.on("request", observeRequest);
  context.on("weberror", observeWebError);
  context.on("requestfinished", observeFinished); context.on("requestfailed", observeFailure);
  return monitor;
}

function navigation(page, reason, action) {
  const monitor = pageMonitors.get(page);
  assert(monitor, "navigation must have an attached browser failure monitor");
  return monitor.navigation(page, reason, action);
}

module.exports = { browserFailureMonitor, navigation };
