#!/usr/bin/env node

const fs = require("node:fs/promises");
const path = require("node:path");
const { build } = require("esbuild");

const outDir = path.join(__dirname, "dist");
const indexSourcePath = path.join(__dirname, "src/index.tsx");
const browserSourcePath = path.join(__dirname, "src/browser.tsx");
const libraryBundlePath = path.join(outDir, "index.cjs");
const appBundlePath = path.join(outDir, "console-app.js");
const htmlPath = path.join(outDir, "index.html");

const html = `<!doctype html>
<html lang="en">
  <head>
    <meta charset="utf-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1" />
    <meta name="mobkit-base-url" content="" />
    <title>Meerkat Console</title>
    <style>
      body {
        margin: 0;
        font-family: "IBM Plex Sans", "Segoe UI", sans-serif;
        background: linear-gradient(180deg, #f7f7f3 0%, #ecefe9 100%);
        color: #123032;
      }

      #root {
        min-height: 100vh;
        padding: 16px;
      }

      [data-testid="meerkat-console"] {
        display: grid;
        gap: 12px;
        grid-template-columns: minmax(220px, 320px) minmax(320px, 1fr);
      }

      [data-testid="meerkat-console"] section {
        border: 1px solid #c4d2cc;
        border-radius: 10px;
        background: #ffffff;
        padding: 12px;
        box-shadow: 0 4px 12px rgba(18, 48, 50, 0.08);
      }

      [data-testid="chat-inspector"],
      [data-testid="topology-panel"],
      [data-testid="health-overview"] {
        grid-column: 1 / -1;
      }

      [data-testid="sidebar-list"],
      [data-testid="activity-feed"],
      [data-testid="chat-events"],
      [data-testid="topology-nodes"],
      [data-testid="health-loaded-modules"] {
        margin: 0;
        padding-left: 20px;
      }

      [data-testid="chat-form"] {
        display: grid;
        gap: 8px;
      }

      textarea,
      select,
      button {
        font: inherit;
      }

      textarea {
        min-height: 96px;
      }

      @media (max-width: 900px) {
        [data-testid="meerkat-console"] {
          grid-template-columns: 1fr;
        }
      }
    </style>
  </head>
  <body>
    <div id="root"></div>
    <script src="/console/assets/console-app.js" defer></script>
  </body>
</html>
`;

async function main() {
  await fs.mkdir(outDir, { recursive: true });
  await build({
    entryPoints: [indexSourcePath],
    outfile: libraryBundlePath,
    bundle: true,
    format: "cjs",
    platform: "neutral",
    target: ["es2020"],
    external: ["react", "react-dom", "react-dom/client"],
    minify: false,
  });
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
    keepNames: true,
    minify: true,
  });
  await fs.writeFile(htmlPath, html, "utf8");
}

main().catch((error) => {
  process.stderr.write(`${error.stack || error.message}\n`);
  process.exit(1);
});
