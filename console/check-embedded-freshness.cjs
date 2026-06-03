#!/usr/bin/env node

const fs = require("node:fs");
const path = require("node:path");
const { spawnSync } = require("node:child_process");
const {
  DIST_GENERATED_FILES,
  EMBEDDED_GENERATED_FILES,
  EMBEDDED_SHARED_FILES,
} = require("./generated-assets.cjs");

const rootDir = path.resolve(__dirname, "..");
const distDir = path.join(__dirname, "dist");
const embeddedDir = path.join(rootDir, "meerkat-mobkit", "console-dist");
const rustConsolePath = path.join(rootDir, "meerkat-mobkit", "src", "http_console.rs");

function read(filePath) {
  try {
    return fs.readFileSync(filePath);
  } catch (error) {
    process.stderr.write(`missing generated console asset: ${filePath}\n`);
    process.stderr.write(`${error.message}\n`);
    process.exit(1);
  }
}

function assertDirectoryFiles(directory, expectedFiles, label) {
  const expected = new Set(expectedFiles);
  let actual;
  try {
    actual = fs.readdirSync(directory).filter((entry) => (
      fs.statSync(path.join(directory, entry)).isFile()
    ));
  } catch (error) {
    process.stderr.write(`missing generated console directory: ${directory}\n`);
    process.stderr.write(`${error.message}\n`);
    process.exit(1);
  }
  const unexpected = actual.filter((file) => !expected.has(file)).sort();
  const missing = expectedFiles.filter((file) => !actual.includes(file));
  if (unexpected.length > 0 || missing.length > 0) {
    process.stderr.write(
      `${label} generated asset set drifted; missing=[${missing.join(", ")}] unexpected=[${unexpected.join(", ")}]\n`,
    );
    process.exit(1);
  }
}

function assertRustEmbeddedAssets() {
  const source = read(rustConsolePath).toString("utf8");
  const actual = [...source.matchAll(/include_str!\("\.\.\/console-dist\/([^"]+)"\)/g)]
    .map((match) => match[1])
    .sort();
  const expected = [...EMBEDDED_GENERATED_FILES].sort();
  if (JSON.stringify(actual) !== JSON.stringify(expected)) {
    process.stderr.write(
      `Rust embedded console asset list drifted; expected=[${expected.join(", ")}] actual=[${actual.join(", ")}]\n`,
    );
    process.exit(1);
  }
}

function assertGitTracksFiles(files, label) {
  const result = spawnSync("git", ["ls-files", "--error-unmatch", "--", ...files], {
    cwd: rootDir,
    stdio: "ignore",
  });
  if (result.status !== 0) {
    process.stderr.write(
      `${label} contains generated assets that are not tracked by git; add the generated console assets before building release binaries\n`,
    );
    process.exit(result.status || 1);
  }
}

assertDirectoryFiles(distDir, DIST_GENERATED_FILES, "console/dist");
assertDirectoryFiles(embeddedDir, EMBEDDED_GENERATED_FILES, "meerkat-mobkit/console-dist");
assertRustEmbeddedAssets();
assertGitTracksFiles(
  DIST_GENERATED_FILES.map((file) => path.join("console", "dist", file)),
  "console/dist",
);
assertGitTracksFiles(
  EMBEDDED_GENERATED_FILES.map((file) => path.join("meerkat-mobkit", "console-dist", file)),
  "meerkat-mobkit/console-dist",
);

for (const file of EMBEDDED_SHARED_FILES) {
  const distPath = path.join(distDir, file);
  const embeddedPath = path.join(embeddedDir, file);
  if (!read(distPath).equals(read(embeddedPath))) {
    process.stderr.write(
      `embedded console asset is stale: ${path.relative(rootDir, embeddedPath)} does not match ${path.relative(rootDir, distPath)}\n`,
    );
    process.exit(1);
  }
}

const diff = spawnSync(
  "git",
  ["diff", "--quiet", "HEAD", "--", "console/dist", "meerkat-mobkit/console-dist"],
  { cwd: rootDir, stdio: "inherit" },
);
if (diff.status !== 0) {
  process.stderr.write(
    "console build left generated asset diffs; commit the refreshed console/dist and meerkat-mobkit/console-dist assets before building release binaries\n",
  );
  process.exit(diff.status || 1);
}

process.stdout.write("embedded console assets are fresh\n");
