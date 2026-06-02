import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import test from "node:test";

const root = path.basename(process.cwd()) === "console"
  ? path.resolve(process.cwd(), "..")
  : process.cwd();
const componentsSrc = path.join(root, "packages", "console-components", "src");
const componentIndex = path.join(componentsSrc, "index.ts");

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
