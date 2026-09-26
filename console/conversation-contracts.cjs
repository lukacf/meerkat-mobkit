#!/usr/bin/env node
"use strict";
const path = require("node:path");
const { spawnSync } = require("node:child_process");
const { build } = require("esbuild");
const entries = [
  "../packages/console-core/src/pending-approvals.test.ts",
  "../packages/console-core/src/tool-completion.test.ts",
  "src/lib/adapters-completion.test.ts",
  "src/lib/adapters-integrity.test.ts",
];
(async () => {
  const outdir = path.join(__dirname, ".tmp/conversation-contracts");
  const files = ["real-images", "real-reasoning", "real-routine-tools", "real-startup-lineage", "stream-parity"].map(name => path.join(__dirname, `scenarios/${name}.test.cjs`));
  for (const [index, entry] of entries.entries()) {
    const outfile = path.join(outdir, `${index}-${path.basename(entry, ".ts")}.mjs`);
    await build({ entryPoints: [path.join(__dirname, entry)], outfile, bundle: true, platform: "node", format: "esm",
      alias: { "@console-core": path.resolve(__dirname, "../packages/console-core/src/index.ts") }, external: ["react"] });
    files.push(outfile);
  }
  const result = spawnSync(process.execPath, ["--test", ...files], { stdio: "inherit" });
  process.exitCode = result.status ?? 1;
})().catch(error => { process.stderr.write(`${error.stack}\n`); process.exitCode = 1; });
