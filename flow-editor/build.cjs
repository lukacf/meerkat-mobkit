#!/usr/bin/env node

const crypto = require("node:crypto");
const fs = require("node:fs/promises");
const os = require("node:os");
const path = require("node:path");
const { build, transform } = require("esbuild");

const root = __dirname;
const srcDir = path.join(root, "src");
const defaultOutDir = path.join(root, "dist");
const defaultRustOutDir = path.join(root, "../meerkat-mobkit/flow-editor-dist");
const tmpDir = path.join(root, ".tmp");
const corePackageEntry = path.join(root, "../packages/flow-editor-core/src/index.ts");
const componentsPackageEntry = path.join(root, "../packages/flow-editor-components/src/index.ts");

// "@flow-editor-core" is a virtual entry: the headless controller package is
// bundled as an IIFE assigned to window.MobKitFlowCore, and the controller.js
// bootstrap builds window.MobKitFlowController from its
// createMobKitFlowController facade. The emitted artifact set and names
// never change.
//
// "@flow-editor-components" is the second virtual entry: the React view
// package is bundled as an IIFE and a shell adapter footer assigns the same
// window.* component globals the replaced legacy .jsx files assigned, so
// app.jsx and the remaining legacy files keep resolving components through
// window unchanged.
const CORE_SCRIPT = "@flow-editor-core";
const COMPONENTS_SCRIPT = "@flow-editor-components";
const scriptOrder = [
  "data.js",
  CORE_SCRIPT,
  "controller.js",
  COMPONENTS_SCRIPT,
  "agents.jsx",
  "builder.jsx",
  "app.jsx",
];

// The window globals the legacy files assigned (tweaks-panel.jsx's
// Object.assign(window, ...) footer; the window.X = X footers of
// overlays.jsx, inspector.jsx, and graph.jsx); grows per slice as each
// .jsx file moves into @flow-editor-components.
const componentsWindowGlobals = [
  "useTweaks", "TweaksPanel", "TweakSection", "TweakRow",
  "TweakSlider", "TweakToggle", "TweakRadio", "TweakSelect",
  "TweakText", "TweakNumber", "TweakColor", "TweakButton",
  "DeployPlanTrace", "ValidateSheet", "SourceDrawer", "InlineSourceEditor",
  "Inspector", "AddNodeMenu",
  "useStudioState", "GraphEditor",
];

async function buildCoreIife() {
  const result = await build({
    absWorkingDir: root,
    entryPoints: [corePackageEntry],
    bundle: true,
    format: "iife",
    globalName: "MobKitFlowCore",
    platform: "neutral",
    target: ["es2020"],
    write: false,
    nodePaths: [path.join(root, "node_modules")],
  });
  const code = result.outputFiles[0].text;
  return `${code}\nwindow.MobKitFlowCore = MobKitFlowCore;\n`;
}

async function buildComponentsIife() {
  const result = await build({
    absWorkingDir: root,
    entryPoints: [componentsPackageEntry],
    bundle: true,
    format: "iife",
    globalName: "MobKitFlowComponents",
    platform: "neutral",
    target: ["es2020"],
    write: false,
    jsxFactory: "React.createElement",
    jsxFragment: "React.Fragment",
    alias: {
      "@flow-editor-core": corePackageEntry,
      "@flow-editor-components": componentsPackageEntry,
    },
    nodePaths: [path.join(root, "node_modules")],
  });
  const code = result.outputFiles[0].text;
  const assignments = componentsWindowGlobals
    .map((name) => `  ${name}: MobKitFlowComponents.${name},`)
    .join("\n");
  return `${code}\nObject.assign(window, {\n${assignments}\n});\n`;
}

function hash(bytes) {
  return crypto.createHash("sha256").update(bytes).digest("hex").slice(0, 12);
}

async function cleanDir(dir) {
  await fs.rm(dir, { force: true, recursive: true });
  await fs.mkdir(dir, { recursive: true });
}

async function compileClassicScript(file) {
  const source = await fs.readFile(path.join(srcDir, file), "utf8");
  if (file.endsWith(".js")) return source;
  const appGlobals = file === "app.jsx"
    ? `const { useStudioState, GraphEditor, Inspector, AddNodeMenu, DeployPlanTrace, ValidateSheet, SourceDrawer, InlineSourceEditor, useTweaks, TweaksPanel, TweakSection, TweakRadio, TweakSelect, TweakText, TweakNumber, AgentsView, BuilderView } = window;\n`
    : "";
  const result = await transform(source, {
    loader: "jsx",
    target: "es2020",
    jsxFactory: "React.createElement",
    jsxFragment: "React.Fragment",
  });
  if (file === "app.jsx") {
    return `${appGlobals}${result.code}`;
  }
  return `{\n${result.code}\n}`;
}

function renderHtml({ vendorVersion, appVersion, cssVersion }) {
  return `<!doctype html>
<html lang="en">
  <head>
    <meta charset="utf-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1" />
    <meta name="mobkit-base-url" content="" />
    <title>MobKit · Flow Editor</title>
    <link rel="stylesheet" href="/flow-editor/assets/flow-editor.css?v=${cssVersion}" />
  </head>
  <body>
    <div id="root"></div>
    <script src="/flow-editor/assets/react-globals.js?v=${vendorVersion}" defer></script>
    <script src="/flow-editor/assets/flow-editor.js?v=${appVersion}" defer></script>
  </body>
</html>
`;
}

async function buildAssets({ outDir, rustOutDir }) {
  await cleanDir(outDir);
  await fs.mkdir(rustOutDir, { recursive: true });

  const vendorPath = path.join(outDir, "react-globals.js");
  await build({
    absWorkingDir: root,
    stdin: {
      contents: `
        import * as React from "react";
        import { createRoot } from "react-dom/client";
        window.React = React;
        window.ReactDOM = { createRoot };
      `,
      resolveDir: root,
      sourcefile: "react-globals-entry.js",
      loader: "js",
    },
    outfile: vendorPath,
    bundle: true,
    format: "iife",
    platform: "browser",
    target: ["es2020"],
    define: {
      "process.env.NODE_ENV": '"production"',
    },
    minify: true,
  });

  const coreIife = await buildCoreIife();
  const componentsIife = await buildComponentsIife();
  const appParts = [];
  for (const file of scriptOrder) {
    appParts.push(`\n/* ${file} */\n`);
    if (file === CORE_SCRIPT) {
      appParts.push(coreIife);
    } else if (file === COMPONENTS_SCRIPT) {
      appParts.push(componentsIife);
    } else {
      appParts.push(await compileClassicScript(file));
    }
  }
  const appJs = appParts.join("\n");
  await fs.writeFile(path.join(outDir, "flow-editor.js"), appJs, "utf8");

  const [tokensCss, stylesCss] = await Promise.all([
    fs.readFile(path.join(srcDir, "tokens.css"), "utf8"),
    fs.readFile(path.join(srcDir, "styles.css"), "utf8"),
  ]);
  const css = `${tokensCss}\n${stylesCss}`;
  await fs.writeFile(path.join(outDir, "flow-editor.css"), css, "utf8");

  const [vendorBytes, appBytes, cssBytes] = await Promise.all([
    fs.readFile(vendorPath),
    fs.readFile(path.join(outDir, "flow-editor.js")),
    fs.readFile(path.join(outDir, "flow-editor.css")),
  ]);
  await fs.writeFile(
    path.join(outDir, "index.html"),
    renderHtml({
      vendorVersion: hash(vendorBytes),
      appVersion: hash(appBytes),
      cssVersion: hash(cssBytes),
    }),
    "utf8",
  );

  await Promise.all(
    ["index.html", "react-globals.js", "flow-editor.js", "flow-editor.css"].map((file) =>
      fs.copyFile(path.join(outDir, file), path.join(rustOutDir, file)),
    ),
  );
}

async function assertFresh(generatedDir, expectedDir) {
  const files = ["index.html", "react-globals.js", "flow-editor.js", "flow-editor.css"];
  const mismatches = [];
  for (const file of files) {
    const [generated, expected] = await Promise.all([
      fs.readFile(path.join(generatedDir, file)),
      fs.readFile(path.join(expectedDir, file)).catch(() => null),
    ]);
    if (!expected || !generated.equals(expected)) mismatches.push(file);
  }
  if (mismatches.length) {
    throw new Error(`flow-editor-dist is stale; rebuild with npm --prefix flow-editor run build (${mismatches.join(", ")})`);
  }
}

// The projection suite loads the controller through this Node-requirable
// bundle: the core package (window.MobKitFlowCore) followed by the
// controller.js bootstrap — the same linkage the browser artifact uses, in
// bare Node with a window stub.
async function buildTestBundle() {
  await fs.mkdir(tmpDir, { recursive: true });
  const coreIife = await buildCoreIife();
  const controller = await compileClassicScript("controller.js");
  const bundle = `/* generated by build.cjs --test-bundle; do not edit */\n${coreIife}\n${controller}`;
  await fs.writeFile(path.join(tmpDir, "controller-under-test.cjs"), bundle, "utf8");
}

async function main() {
  if (process.argv.includes("--test-bundle")) {
    await buildTestBundle();
    return;
  }
  const checkOnly = process.argv.includes("--check");
  if (!checkOnly) {
    await buildAssets({ outDir: defaultOutDir, rustOutDir: defaultRustOutDir });
    return;
  }
  const tempRoot = await fs.mkdtemp(path.join(os.tmpdir(), "mobkit-flow-editor-build-"));
  try {
    const generatedDist = path.join(tempRoot, "dist");
    const generatedRustDist = path.join(tempRoot, "flow-editor-dist");
    await buildAssets({ outDir: generatedDist, rustOutDir: generatedRustDist });
    await assertFresh(generatedRustDist, defaultRustOutDir);
  } finally {
    await fs.rm(tempRoot, { force: true, recursive: true });
  }
}

main().catch((error) => {
  process.stderr.write(`${error.stack || error.message}\n`);
  process.exit(1);
});
