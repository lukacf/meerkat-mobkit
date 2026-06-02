import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import test from "node:test";

const root = path.basename(process.cwd()) === "console"
  ? path.resolve(process.cwd(), "..")
  : process.cwd();
const componentsSrc = path.join(root, "packages", "console-components", "src");
const coreActivitySource = path.join(root, "packages", "console-core", "src", "activity.ts");
const componentIndex = path.join(componentsSrc, "index.ts");
const stockConsoleAppSource = path.join(root, "console", "src", "ConsoleApp.tsx");

const forbiddenImportPatterns = [
  /from\s+["'][^"']*console\/src\//,
  /from\s+["'][^"']*ConsoleApp/,
  /from\s+["'][^"']*headless/,
  /from\s+["'][^"']*network/,
  /from\s+["']electron["']/,
  /createMobKitConsoleController/,
  /useMobKitConsoleController/,
];

function sourceFiles(dir: string): string[] {
  const entries = fs.readdirSync(dir, { withFileTypes: true });
  return entries.flatMap((entry) => {
    const fullPath = path.join(dir, entry.name);
    if (entry.isDirectory()) return sourceFiles(fullPath);
    return /\.(ts|tsx)$/.test(entry.name) ? [fullPath] : [];
  });
}

test("console-components stay model/callback renderers without app, network, or headless imports", () => {
  const violations: string[] = [];

  for (const filePath of sourceFiles(componentsSrc)) {
    if (filePath.endsWith(".test.ts") || filePath.endsWith(".test.tsx")) {
      continue;
    }
    const source = fs.readFileSync(filePath, "utf8");
    for (const pattern of forbiddenImportPatterns) {
      if (pattern.test(source)) {
        violations.push(`${path.relative(root, filePath)} matched ${pattern}`);
      }
    }
  }

  assert.deepEqual(violations, []);
});

test("console-components root barrel exposes reusable composition surfaces", () => {
  const source = fs.readFileSync(componentIndex, "utf8");
  for (const symbol of [
    "ConsoleActivityRail",
    "ConsoleComposer",
    "ConsoleDock",
    "ConsoleSidebar",
    "ConsoleWorkbench",
    "ConversationPane",
    "ConversationTranscript",
    "useConsoleDockController",
  ]) {
    assert.match(source, new RegExp(`\\b${symbol}\\b`));
  }
});

test("activity rail roster actions are declared in the shared core model", () => {
  const source = fs.readFileSync(coreActivitySource, "utf8");
  const rosterPanel = source.match(/export interface ConsoleActivityRosterPanel \{(?<body>[\s\S]*?)\n\}/)?.groups?.body || "";

  assert.match(rosterPanel, /\bactions\?:\s*ConsoleActivityAction\[\]/);
});

test("stock console runtime path is backed by the headless controller", () => {
  const source = fs.readFileSync(stockConsoleAppSource, "utf8");

  assert.match(source, /\bcreateHttpConsoleTransport\b/);
  assert.match(source, /\bcreateMobKitConsoleController\b/);
  assert.match(source, /\bconsoleController\.timeline\.query\b/);
  assert.match(source, /\bconsoleController\.timeline\.subscribeWithBackfill\b/);
  assert.match(source, /\bconsoleController\.commands\.sendMessage\b/);
  assert.match(source, /\bconsoleController\.commands\.execute\b/);
  for (const rawHelper of [
    "fetchJson",
    "queryTimeline",
    "sendConsole",
    "sendConsoleMultipart",
    "subscribeTimelineEvents",
    "callConsoleRpc",
  ]) {
    assert.doesNotMatch(source, new RegExp(`\\b${rawHelper}\\b`));
  }
});
