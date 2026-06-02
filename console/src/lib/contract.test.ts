import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import test from "node:test";

import {
  CONSOLE_CONTRACT_VERSION,
  CONSOLE_BLOB_PATH_PREFIX,
  CONSOLE_REST_PATHS,
  CONSOLE_RPC_METHODS,
  CONSOLE_RPC_PATHS,
  CONSOLE_TIMELINE_QUERY_MODES,
  CONSOLE_TIMELINE_REPLAY_UNAVAILABLE_CODE,
} from "./contract";

type ContractError = {
  status?: number;
  shape?: {
    error?: string;
    message?: string;
    reason?: string;
    requested_cursor?: string;
    latest_cursor?: string;
  };
  codes?: number[];
};

type ContractSchema = {
  contract_version: string;
  surfaces: {
    rest: Record<string, {
      method: string;
      path?: string;
      path_template?: string;
      path_prefix?: string;
      response?: { required_top_level_fields?: string[] };
      query_optional_fields?: string[];
      errors?: ContractError[];
    }>;
    rpc: {
      console_rpc_endpoint: { method: string; path: string; errors?: ContractError[] };
      console_multipart_rpc_endpoint: { method: string; path: string; errors?: ContractError[] };
      methods: Record<string, {
        success?: { mode_values?: string[] };
        errors?: ContractError[];
      }>;
    };
    sse: Record<string, {
      method?: string;
      path?: string;
      path_template?: string;
      errors?: ContractError[];
    } | string>;
  };
};

test("console contract constants stay synchronized with docs/rct contract v0.5.0", () => {
  const schema = JSON.parse(
    readFileSync(resolve(process.cwd(), "../docs/rct/console-rest-sse-contract-v0.5.0.json"), "utf8"),
  ) as ContractSchema;

  assert.equal(CONSOLE_CONTRACT_VERSION, schema.contract_version);
  assert.equal(CONSOLE_REST_PATHS.experience, schema.surfaces.rest.experience.path);
  assert.equal(CONSOLE_REST_PATHS.modules, schema.surfaces.rest.modules.path);
  assert.equal(CONSOLE_REST_PATHS.identities, schema.surfaces.rest.identities.path);
  assert.equal(CONSOLE_REST_PATHS.timeline, schema.surfaces.rest.timeline.path);
  assert.equal(CONSOLE_REST_PATHS.legacySend, schema.surfaces.rest.legacy_send.path);
  assert.equal(CONSOLE_BLOB_PATH_PREFIX, schema.surfaces.rest.blob.path_prefix);
  assert.equal(CONSOLE_REST_PATHS.timelineStream, (schema.surfaces.sse.timeline as { path: string }).path);
  assert.equal(
    CONSOLE_REST_PATHS.identityTimelineStreamTemplate,
    (schema.surfaces.sse.identity_timeline as { path_template: string }).path_template,
  );
  assert.equal(CONSOLE_RPC_PATHS.jsonRpc, schema.surfaces.rpc.console_rpc_endpoint.path);
  assert.equal(CONSOLE_RPC_PATHS.multipartJsonRpc, schema.surfaces.rpc.console_multipart_rpc_endpoint.path);
  for (const method of Object.values(CONSOLE_RPC_METHODS)) {
    assert.equal(
      method in schema.surfaces.rpc.methods,
      true,
      `expected docs/rct contract to document ${method}`,
    );
  }
  assert.deepEqual(
    [...CONSOLE_TIMELINE_QUERY_MODES],
    schema.surfaces.rpc.methods[CONSOLE_RPC_METHODS.queryTimeline]?.success?.mode_values,
  );
  assert.equal(
    schema.surfaces.rpc.methods[CONSOLE_RPC_METHODS.queryTimeline]?.errors
      ?.some((entry) => entry.codes?.includes(CONSOLE_TIMELINE_REPLAY_UNAVAILABLE_CODE)),
    true,
  );
});

test("console contract documents live REST, SSE, and send RPC error shapes", () => {
  const schema = JSON.parse(
    readFileSync(resolve(process.cwd(), "../docs/rct/console-rest-sse-contract-v0.5.0.json"), "utf8"),
  ) as ContractSchema;
  const sendCodes = schema.surfaces.rpc.methods[CONSOLE_RPC_METHODS.send]?.errors?.flatMap((entry) => entry.codes || []) || [];
  const listIdentityCodes = schema.surfaces.rpc.methods[CONSOLE_RPC_METHODS.listIdentities]?.errors?.flatMap((entry) => entry.codes || []) || [];
  const inspectIdentityCodes = schema.surfaces.rpc.methods[CONSOLE_RPC_METHODS.inspectIdentity]?.errors?.flatMap((entry) => entry.codes || []) || [];
  const queryTimelineCodes = schema.surfaces.rpc.methods[CONSOLE_RPC_METHODS.queryTimeline]?.errors?.flatMap((entry) => entry.codes || []) || [];
  const blobUploadErrors = schema.surfaces.rpc.methods[CONSOLE_RPC_METHODS.blobUpload]?.errors || [];
  const blobUploadCodes = blobUploadErrors.flatMap((entry) => entry.codes || []);
  const rpcEndpointErrors = schema.surfaces.rpc.console_rpc_endpoint.errors || [];
  const multipartEndpointErrors = schema.surfaces.rpc.console_multipart_rpc_endpoint.errors || [];

  assertRestReason(schema.surfaces.rest.experience.errors, 401, "unauthorized", "string");
  assertRestReason(schema.surfaces.rest.modules.errors, 401, "unauthorized", "string");
  assertRestError(schema.surfaces.rest.identities.errors, 500, "internal_error", "string");
  assertRestError(schema.surfaces.rest.timeline.errors, 401, "unauthorized", "console timeline requires a valid auth token");
  assertRestError(schema.surfaces.rest.timeline.errors, 404, "unavailable", "console aggregator unavailable");
  assertRestError(schema.surfaces.rest.timeline.errors, 409, "replay_unavailable");
  assertRestError(schema.surfaces.rest.blob.errors, 401, "unauthorized");
  assertRestError(schema.surfaces.rest.blob.errors, 400, "invalid_blob_id");
  assertRestError(schema.surfaces.rest.blob.errors, 404, "blob_store_unavailable");
  assertRestError(schema.surfaces.rest.blob.errors, 404, "blob_not_found");
  assertRestError(schema.surfaces.rest.blob.errors, 500, "string");
  assertRestError((schema.surfaces.sse.timeline as { errors?: ContractError[] }).errors, 404, "unavailable", "console aggregator unavailable");
  assert.deepEqual(
    [-32001, -32002, -32004, -32009, -32000, -32602].filter((code) => !sendCodes.includes(code)),
    [],
  );
  assert.equal(sendCodes.includes(-32003), false);
  assert.equal(sendCodes.includes(-32603), false);
  assert.deepEqual([-32004, -32000].filter((code) => !listIdentityCodes.includes(code)), []);
  assert.equal(listIdentityCodes.includes(-32603), false);
  assert.deepEqual([-32001, -32004, -32602, -32000].filter((code) => !inspectIdentityCodes.includes(code)), []);
  assert.equal(inspectIdentityCodes.includes(-32603), false);
  assert.deepEqual([-32004, -32013, -32602].filter((code) => !queryTimelineCodes.includes(code)), []);
  assert.equal(queryTimelineCodes.includes(-32003), false);
  assert.equal(queryTimelineCodes.includes(-32603), false);
  assert.deepEqual([-32600, -32602, -32000].filter((code) => !blobUploadCodes.includes(code)), []);
  for (const status of [401, 400, 200, 404, 500]) {
    assert.equal(blobUploadErrors.some((entry) => entry.status === status), true);
  }
  assertRpcEndpointError(rpcEndpointErrors, 200, -32600);
  assertRpcEndpointError(rpcEndpointErrors, 401, -32600);
  assertRpcEndpointError(multipartEndpointErrors, 401, -32600);
  assertRpcEndpointError(multipartEndpointErrors, 400, -32602);
  assertRpcEndpointError(multipartEndpointErrors, 200, -32600);
  assertRpcEndpointError(multipartEndpointErrors, 200, -32602);
});

test("console contract route and method names stay synchronized with Rust http console source", () => {
  const schema = JSON.parse(
    readFileSync(resolve(process.cwd(), "../docs/rct/console-rest-sse-contract-v0.5.0.json"), "utf8"),
  ) as ContractSchema;
  const rustSource = readFileSync(resolve(process.cwd(), "../meerkat-mobkit/src/http_console.rs"), "utf8");
  const registeredRoutes = parseAxumRoutes(rustSource);
  const contractedRoutes = contractRoutes(schema);
  const frontendRoutes = new Set([
    "GET /",
    "GET /favicon.ico",
    "GET /console",
    "GET /console/",
    "GET /console/assets/console-app.js",
    "GET /console/assets/console-app.css",
  ]);

  for (const route of contractedRoutes) {
    assert.equal(
      registeredRoutes.has(route),
      true,
      `expected http_console.rs to register ${route}`,
    );
  }

  for (const route of registeredRoutes) {
    const path = route.split(" ")[1] || "";
    if (!path.startsWith("/console") && !path.startsWith("/blobs")) {
      continue;
    }
    assert.equal(
      contractedRoutes.has(route) || frontendRoutes.has(route),
      true,
      `registered route ${route} must be contracted or explicitly frontend-only`,
    );
  }

  for (const method of Object.keys(schema.surfaces.rpc.methods)) {
    assert.match(
      rustSource,
      new RegExp(JSON.stringify(method).replace(/[.*+?^${}()|[\]\\]/g, "\\$&")),
      `expected http_console.rs to contain RPC method ${method}`,
    );
  }
});

test("console experience contract names required top-level projection fields", () => {
  const schema = JSON.parse(
    readFileSync(resolve(process.cwd(), "../docs/rct/console-rest-sse-contract-v0.5.0.json"), "utf8"),
  ) as ContractSchema;
  const required = schema.surfaces.rest.experience.response?.required_top_level_fields || [];

  assert.deepEqual(required, [
    "contract_version",
    "runtime_capabilities",
    "base_panel",
    "agent_sidebar",
    "activity_feed",
    "chat_inspector",
    "topology",
    "health_overview",
    "flows",
    "session_history",
  ]);
});

function parseAxumRoutes(source: string): Set<string> {
  const routes = new Set<string>();
  const routePattern = /\.route\(\s*"([^"]+)"\s*,\s*(get|post)\s*\(/gs;
  for (const match of source.matchAll(routePattern)) {
    routes.add(`${match[2]!.toUpperCase()} ${match[1]}`);
  }
  return routes;
}

function contractRoutes(schema: ContractSchema): Set<string> {
  const routes = new Set<string>();
  for (const surface of Object.values(schema.surfaces.rest)) {
    const path = surface.path || surface.path_template;
    if (path && surface.method) {
      routes.add(`${surface.method.toUpperCase()} ${path}`);
    }
  }
  routes.add(`${schema.surfaces.rpc.console_rpc_endpoint.method.toUpperCase()} ${schema.surfaces.rpc.console_rpc_endpoint.path}`);
  routes.add(`${schema.surfaces.rpc.console_multipart_rpc_endpoint.method.toUpperCase()} ${schema.surfaces.rpc.console_multipart_rpc_endpoint.path}`);
  for (const surface of Object.values(schema.surfaces.sse)) {
    if (typeof surface === "string") {
      continue;
    }
    const path = surface.path || surface.path_template;
    if (path && surface.method) {
      routes.add(`${surface.method.toUpperCase()} ${path}`);
    }
  }
  return routes;
}

function assertRestError(
  errors: ContractError[] | undefined,
  status: number,
  error: string,
  message?: string,
) {
  assert.equal(
    (errors || []).some((entry) => (
      entry.status === status
        && errorMatches(entry.shape?.error, error)
        && (message === undefined || entry.shape?.message === message)
    )),
    true,
    `expected ${status} ${error}${message ? ` ${message}` : ""}`,
  );
}

function assertRestReason(
  errors: ContractError[] | undefined,
  status: number,
  error: string,
  reason: string,
) {
  assert.equal(
    (errors || []).some((entry) => (
      entry.status === status
        && errorMatches(entry.shape?.error, error)
        && entry.shape?.reason === reason
    )),
    true,
    `expected ${status} ${error} reason ${reason}`,
  );
}

function errorMatches(actual: string | undefined, expected: string): boolean {
  if (expected === "string") {
    return actual === "string";
  }
  return (actual || "").split("|").includes(expected);
}

function assertRpcEndpointError(errors: ContractError[], status: number, code: number) {
  assert.equal(
    errors.some((entry) => entry.status === status && (entry.codes || []).includes(code)),
    true,
    `expected endpoint error ${status} ${code}`,
  );
}
