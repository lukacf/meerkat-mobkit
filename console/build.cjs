#!/usr/bin/env node

const fs = require("node:fs/promises");
const path = require("node:path");
const { build } = require("esbuild");

const outDir = path.join(__dirname, "dist");
const embeddedOutDir = path.join(__dirname, "../meerkat-mobkit/console-dist");
const indexSourcePath = path.join(__dirname, "src/index.tsx");
const browserSourcePath = path.join(__dirname, "src/browser.tsx");
const libraryBundlePath = path.join(outDir, "index.cjs");
const appBundlePath = path.join(outDir, "console-app.js");
const htmlPath = path.join(outDir, "index.html");

// Resolve shared packages from local workspace
const alias = {
  "@console-core": path.resolve(__dirname, "../packages/console-core/src/index.ts"),
  "@console-components": path.resolve(__dirname, "../packages/console-components/src/index.ts"),
  "@console-components/styles": path.resolve(__dirname, "../packages/console-components/src/styles/index.ts"),
};

// Shared packages import clsx etc. — resolve from console/node_modules
const nodePaths = [path.resolve(__dirname, "node_modules")];

const html = `<!doctype html>
<html lang="en">
  <head>
    <meta charset="utf-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1" />
    <meta name="mobkit-base-url" content="" />
    <title>MobKit Console</title>
    <link rel="stylesheet" href="/console/assets/console-app.css" />
  </head>
  <body>
    <div id="root"></div>
    <script src="/console/assets/console-app.js" defer></script>
  </body>
</html>
`;

async function main() {
  await fs.mkdir(outDir, { recursive: true });
  await fs.mkdir(embeddedOutDir, { recursive: true });

  // Shared components use JSX automatic runtime (react/jsx-runtime)
  const jsxOptions = { jsx: "automatic" };

  // Library bundle (CJS) for JSDOM / smoke tests
  await build({
    entryPoints: [indexSourcePath],
    outfile: libraryBundlePath,
    bundle: true,
    format: "cjs",
    platform: "neutral",
    target: ["es2020"],
    external: ["react", "react-dom", "react-dom/client", "react/jsx-runtime", "react/jsx-dev-runtime"],
    alias,
    nodePaths,
    ...jsxOptions,
    minify: false,
  });

  // Browser app bundle (IIFE) served by the gateway
  await build({
    entryPoints: [browserSourcePath],
    outfile: appBundlePath,
    bundle: true,
    format: "iife",
    platform: "browser",
    target: ["es2020"],
    define: {
      "process.env.NODE_ENV": '"production"',
      NODE_ENV: '"production"',
    },
    alias,
    nodePaths,
    ...jsxOptions,
    keepNames: true,
    minify: true,
  });

  await fs.writeFile(htmlPath, html, "utf8");

  await Promise.all([
    fs.copyFile(appBundlePath, path.join(embeddedOutDir, "console-app.js")),
    fs.copyFile(path.join(outDir, "console-app.css"), path.join(embeddedOutDir, "console-app.css")),
    fs.copyFile(htmlPath, path.join(embeddedOutDir, "index.html")),
  ]);
}

main().catch((error) => {
  process.stderr.write(`${error.stack || error.message}\n`);
  process.exit(1);
});
