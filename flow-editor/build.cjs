#!/usr/bin/env node

const crypto = require("node:crypto");
const fs = require("node:fs/promises");
const os = require("node:os");
const path = require("node:path");
let esbuild;
try {
  esbuild = require("esbuild");
} catch {
  esbuild = require("../console/node_modules/esbuild");
}
const { build, transform } = esbuild;

const root = __dirname;
const srcDir = path.join(root, "src");
const defaultOutDir = path.join(root, "dist");
const defaultRustOutDir = path.join(root, "../meerkat-mobkit/flow-editor-dist");

const scriptOrder = [
  "data.js",
  "controller.js",
  "tweaks-panel.jsx",
  "graph.jsx",
  "inspector.jsx",
  "overlays.jsx",
  "agents.jsx",
  "builder.jsx",
  "app.jsx",
];

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

  const appParts = [];
  for (const file of scriptOrder) {
    appParts.push(`\n/* ${file} */\n`);
    appParts.push(await compileClassicScript(file));
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

async function main() {
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
