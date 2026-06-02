#!/usr/bin/env node

const fs = require("node:fs");
const path = require("node:path");
const { spawnSync } = require("node:child_process");

const rootDir = path.resolve(__dirname, "..");
const distDir = path.join(__dirname, "dist");
const embeddedDir = path.join(rootDir, "meerkat-mobkit", "console-dist");
const generatedFiles = ["console-app.css", "console-app.js", "index.html"];

function read(filePath) {
  try {
    return fs.readFileSync(filePath);
  } catch (error) {
    process.stderr.write(`missing generated console asset: ${filePath}\n`);
    process.stderr.write(`${error.message}\n`);
    process.exit(1);
  }
}

for (const file of generatedFiles) {
  const distPath = path.join(distDir, file);
  const embeddedPath = path.join(embeddedDir, file);
  if (!read(distPath).equals(read(embeddedPath))) {
    process.stderr.write(
      `embedded console asset is stale: ${path.relative(rootDir, embeddedPath)} does not match ${path.relative(rootDir, distPath)}\n`,
    );
    process.exit(1);
  }
}

const requireGitClean =
  process.argv.includes("--git-clean") ||
  process.env.CI === "true" ||
  process.env.GITHUB_ACTIONS === "true";

if (requireGitClean) {
  const diff = spawnSync(
    "git",
    ["diff", "--quiet", "--", "console/dist", "meerkat-mobkit/console-dist"],
    { cwd: rootDir, stdio: "inherit" },
  );
  if (diff.status !== 0) {
    process.stderr.write(
      "console build left generated asset diffs; commit the refreshed console/dist and meerkat-mobkit/console-dist assets before building release binaries\n",
    );
    process.exit(diff.status || 1);
  }
}

process.stdout.write("embedded console assets are fresh\n");
