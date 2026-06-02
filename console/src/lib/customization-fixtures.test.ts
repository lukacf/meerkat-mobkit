import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import test from "node:test";

const root = path.basename(process.cwd()) === "console"
  ? path.resolve(process.cwd(), "..")
  : process.cwd();

function fixture(name: string): Record<string, unknown> {
  return JSON.parse(fs.readFileSync(
    path.join(root, "console", "fixtures", name, "fixture.json"),
    "utf8",
  )) as Record<string, unknown>;
}

test("reference-wrapper fixture keeps host status separate from MobKit console protocol", () => {
  const wrapper = fixture("reference-wrapper");
  const consoleRoutes = wrapper.consoleRoutes as Record<string, string>;
  const hostRoutes = wrapper.hostRoutes as Record<string, Record<string, unknown>>;
  const policy = wrapper.wrapperPolicy as Record<string, boolean>;

  for (const route of [
    "/console",
    "/console/rpc",
    "/console/timeline",
    "/console/timeline/stream",
    "/blobs/{blob_id}",
  ]) {
    assert.equal(consoleRoutes[route], "mobkit-reference-console");
  }
  assert.equal(hostRoutes["/host/status"].provenance, "host-adapter");
  assert.equal(hostRoutes["/host/status"].consumedByMobKitHeadless, false);
  for (const [key, value] of Object.entries(policy)) {
    assert.equal(value, false, `${key} must stay forbidden`);
  }
});

test("custom-host-shell fixture maps host records to explicit MobKit targets through a non-HTTP adapter", () => {
  const custom = fixture("custom-host-shell");
  const transport = custom.rendererTransport as Record<string, unknown>;
  const records = custom.hostRecords as Array<Record<string, unknown>>;
  const navigation = custom.hostNavigation as Record<string, unknown>;
  const components = custom.mobkitComponents as string[];

  assert.equal(transport.kind, "host-adapter");
  assert.equal(transport.directBrowserRpc, false);
  assert.equal(transport.rawRuntimeCalls, false);
  assert.equal(transport.preservesTypedErrors, true);
  assert.equal(transport.preservesCursors, true);
  assert.equal(transport.preservesBlobs, true);
  assert.equal(navigation.orientation, "horizontal");
  assert.equal(navigation.bindsMobKitTargets, true);
  assert.deepEqual(components.sort(), [
    "ConsoleComposer",
    "ConsoleDock",
    "ConversationPane",
    "ConversationTranscript",
  ].sort());

  for (const record of records) {
    assert.match(String(record.kind), /^host\//);
    const target = record.mobkitTarget as Record<string, unknown>;
    assert.equal(target.kind, "mobkit/identity-chat");
    assert.match(String(target.identity), /^identity:/);
  }
});

test("configured-host-shell fixture keeps host mutations out of console.toml buttons and protects direct console routes", () => {
  const configured = fixture("configured-host-shell");
  const toolbar = configured.hostToolbarActions as Array<Record<string, unknown>>;
  const sidebarButtons = configured.sidebarButtons as Record<string, unknown>;
  const routeProtection = configured.directRouteProtectionWhenMobKitAuthDisabled as Record<string, string>;

  assert.equal(toolbar[0].provenance, "host-adapter");
  assert.equal(toolbar[0].requiresConfirmation, true);
  assert.match(String(toolbar[0].postRoute), /^\/host\/actions\//);
  assert.equal(sidebarButtons.mutationSemantics, false);
  assert.deepEqual(sidebarButtons.allowedFields, ["id", "label", "href", "target", "control", "icon_name"]);

  for (const route of [
    "/console",
    "/console/assets/console-app.js",
    "/console/rpc",
    "/console/rpc/multipart",
    "/console/timeline",
    "/console/timeline/stream",
    "/blobs/{blob_id}",
  ]) {
    assert.equal(routeProtection[route], "host-proxy");
  }
});
