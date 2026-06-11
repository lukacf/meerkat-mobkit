// Package boundary contracts for the extracted Flow Editor workspace
// packages. The extraction slices policed these properties piecemeal with
// regexes over the moving controller.js residue; this test replaces that
// with static scans over the finished package trees, mirroring the
// console's component-boundary.test.ts pattern (forbidden-pattern lists,
// a source walker, and a collected-violations deep-equal).
//
// What each boundary means:
// - @flow-editor-core is the headless controller plane. It must stay
//   framework-free and window-free so the projection suite can load it in
//   Node and embedders can construct the facade themselves. The only
//   `window.` tokens allowed live in the facade/bridge files, where the
//   comments document the shell's window contract; the modules themselves
//   never touch the global.
// - @flow-editor-components is the React view layer. Views render props
//   and call the window.MobKitFlowController facade at render time; they
//   must never open their own network path (fetch/callRpc) or import Node
//   built-ins, so the package stays a pure browser-bundle input.
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");

const root = path.join(__dirname, "..", "..");
const coreSrc = path.join(root, "packages", "flow-editor-core", "src");
const componentsSrc = path.join(root, "packages", "flow-editor-components", "src");

function sourceFiles(dir) {
  return fs
    .readdirSync(dir, { withFileTypes: true })
    .sort((a, b) => a.name.localeCompare(b.name))
    .flatMap((entry) => {
      const full = path.join(dir, entry.name);
      if (entry.isDirectory()) return sourceFiles(full);
      return /\.(ts|tsx)$/.test(entry.name) ? [full] : [];
    });
}

function scan(files, rules) {
  const violations = [];
  for (const filePath of files) {
    const source = fs.readFileSync(filePath, "utf8");
    const relPath = path.relative(root, filePath).split(path.sep).join("/");
    for (const rule of rules) {
      if ((rule.allow || []).includes(relPath)) continue;
      if (rule.pattern.test(source)) {
        violations.push(`${relPath} matched ${rule.pattern} — ${rule.why}`);
      }
    }
  }
  return violations;
}

// The facade and the barrel are the bridge files: their header comments
// document the window.MobKitFlowCore / window.MobKitFlowController shell
// contract. Code-level window access stays forbidden everywhere — these two
// files are themselves window-free at runtime (the ~3-line shell bootstrap
// in app.tsx owns the window assignment).
const coreWindowAllowlist = [
  "packages/flow-editor-core/src/index.ts",
  "packages/flow-editor-core/src/controller-facade.ts",
];

const coreRules = [
  {
    pattern: /window\./,
    allow: coreWindowAllowlist,
    why: "core modules must be window-free; only the facade/bridge files may mention the shell's window contract",
  },
  {
    // Match real React usage (member access, imports) without tripping on
    // prose like "no React." in module headers.
    pattern: /\bReact\.[A-Za-z$_]|from\s+["']react|require\(["']react|^import\s+React\b/m,
    why: "the controller plane is framework-free; React lives in @flow-editor-components and the shell",
  },
  {
    // Core modules pass mobpack flow documents around as locals named
    // `document`, so this pins DOM Document API member access specifically
    // rather than the bare identifier.
    pattern:
      /\bdocument\.(getElementById|querySelector(All)?|createElement(NS)?|createTextNode|body|head|documentElement|activeElement|addEventListener|removeEventListener|dispatchEvent|getElementsBy\w+|getSelection|execCommand|title|cookie|location|defaultView|fonts|hidden|visibilityState)\b/,
    why: "core modules must not touch the DOM document",
  },
  {
    pattern: /\brequire\s*\(/,
    why: "core modules are ESM bundle inputs; no CommonJS require",
  },
  {
    pattern: /from\s+["']node:/,
    why: "core modules must not import Node built-ins; the bundle is platform-neutral",
  },
];

const componentsRules = [
  {
    pattern: /\bfetch\s*\(/,
    why: "views go through props or the window.MobKitFlowController facade; networking lives in @flow-editor-core rpc/client",
  },
  {
    pattern: /\bcallRpc\b/,
    why: "views must not reach the RPC client directly; the facade is the runtime contract",
  },
  {
    pattern: /\brequire\s*\(/,
    why: "components are ESM browser-bundle inputs; no CommonJS require",
  },
  {
    pattern: /from\s+["']node:/,
    why: "components must not import Node built-ins",
  },
];

const coreFiles = sourceFiles(coreSrc);
const componentFiles = sourceFiles(componentsSrc);

// Anchor the walker: if these landmarks vanish the scan is reading the
// wrong tree, not proving a clean boundary.
assert(
  coreFiles.some((file) => file.endsWith(path.join("src", "controller-facade.ts"))),
  "core walk must include controller-facade.ts",
);
assert(
  componentFiles.some((file) => file.endsWith(path.join("graph", "graph.tsx"))),
  "components walk must include graph/graph.tsx",
);

assert.deepEqual(
  scan(coreFiles, coreRules),
  [],
  "@flow-editor-core boundary violations",
);
assert.deepEqual(
  scan(componentFiles, componentsRules),
  [],
  "@flow-editor-components boundary violations",
);

console.log(
  `package-boundaries OK (${coreFiles.length} core modules, ${componentFiles.length} component modules)`,
);
