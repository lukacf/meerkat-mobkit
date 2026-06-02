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

type ContractSchema = {
  contract_version: string;
  surfaces: {
    rest: Record<string, { path: string; response?: { required_top_level_fields?: string[] }; query_optional_fields?: string[] }>;
    rpc: {
      console_rpc_endpoint: { path: string };
      methods: Record<string, {
        success?: { mode_values?: string[] };
        errors?: Array<{ codes?: number[] }>;
      }>;
    };
    sse: Record<string, { path?: string } | string>;
  };
};

test("console contract constants stay synchronized with docs/rct contract v0.5.0", () => {
  const schema = JSON.parse(
    readFileSync(resolve(process.cwd(), "../docs/rct/console-rest-sse-contract-v0.5.0.json"), "utf8"),
  ) as ContractSchema;

  assert.equal(CONSOLE_CONTRACT_VERSION, schema.contract_version);
  assert.equal(CONSOLE_REST_PATHS.experience, schema.surfaces.rest.experience.path);
  assert.equal(CONSOLE_REST_PATHS.modules, schema.surfaces.rest.modules.path);
  assert.equal(CONSOLE_REST_PATHS.timeline, schema.surfaces.rest.timeline.path);
  assert.equal(CONSOLE_BLOB_PATH_PREFIX, schema.surfaces.rest.blob.path_prefix);
  assert.equal(CONSOLE_REST_PATHS.timelineStream, (schema.surfaces.sse.timeline as { path: string }).path);
  assert.equal(CONSOLE_RPC_PATHS.jsonRpc, schema.surfaces.rpc.console_rpc_endpoint.path);
  assert.equal(CONSOLE_RPC_PATHS.multipartJsonRpc, schema.surfaces.rpc.console_multipart_rpc_endpoint.path);
  assert.equal(CONSOLE_RPC_METHODS.capabilities in schema.surfaces.rpc.methods, true);
  assert.equal(CONSOLE_RPC_METHODS.send in schema.surfaces.rpc.methods, true);
  assert.equal(CONSOLE_RPC_METHODS.queryTimeline in schema.surfaces.rpc.methods, true);
  assert.equal(CONSOLE_RPC_METHODS.blobUpload in schema.surfaces.rpc.methods, true);
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

test("console contract route and method names stay synchronized with Rust http console source", () => {
  const schema = JSON.parse(
    readFileSync(resolve(process.cwd(), "../docs/rct/console-rest-sse-contract-v0.5.0.json"), "utf8"),
  ) as ContractSchema;
  const rustSource = readFileSync(resolve(process.cwd(), "../meerkat-mobkit/src/http_console.rs"), "utf8");
  const expectedLiterals = [
    schema.surfaces.rest.experience.path,
    schema.surfaces.rest.modules.path,
    schema.surfaces.rest.timeline.path,
    schema.surfaces.rest.blob.path_template,
    (schema.surfaces.sse.timeline as { path: string }).path,
    schema.surfaces.rpc.console_rpc_endpoint.path,
    schema.surfaces.rpc.console_multipart_rpc_endpoint.path,
    ...Object.keys(schema.surfaces.rpc.methods),
  ];

  for (const literal of expectedLiterals) {
    assert.equal(
      rustSource.includes(literal),
      true,
      `expected http_console.rs to contain ${literal}`,
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
