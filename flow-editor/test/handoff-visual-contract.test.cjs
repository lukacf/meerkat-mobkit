const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const { spawnSync } = require("node:child_process");

const root = path.join(__dirname, "..");
const zipPath = process.env.MOBKIT_EDITOR_HANDOFF_ZIP ||
  "/Users/luka/Downloads/Meerkat-MobKit Editor-handoff.zip";
const zipRoot = "meerkat-mobkit-editor/project";

function fromZip(entry) {
  const result = spawnSync("unzip", ["-p", zipPath, `${zipRoot}/${entry}`], { encoding: "utf8" });
  if (result.status !== 0) {
    throw new Error(`failed to read ${entry} from handoff zip: ${result.stderr || result.stdout}`);
  }
  return result.stdout;
}

function local(entry) {
  return fs.readFileSync(path.join(root, "src", entry), "utf8");
}

function diff(entry) {
  const result = spawnSync("diff", [
    "-u",
    "-",
    path.join(root, "src", entry),
  ], {
    encoding: "utf8",
    input: fromZip(entry),
  });
  return result.stdout;
}

if (!fs.existsSync(zipPath)) {
  process.stdout.write(`handoff zip not found at ${zipPath}; visual contract skipped\n`);
  process.exit(0);
}

assert.equal(local("tokens.css"), fromZip("tokens.css"), "design tokens must match the handoff exactly");

function allowedCssDelta(sign, line) {
  if (line.startsWith("---") || line.startsWith("+++")) return true;
  if (line.startsWith("@@")) return true;
  const text = line.slice(1);
  const shared = [
    /Source drawer/,
    /YAML drawer/,
    /source-drawer/,
    /yaml/,
    /toml-/,
    /y-key/,
    /y-comment/,
    /subagent/,
    /sub-mob/,
    /mob\.toml drawer/,
    /grid-template-columns: auto auto auto auto/,
    /minmax\(0, 1fr\)/,
    /min-width: 0/,
    /overflow: hidden/,
    /text-overflow: ellipsis/,
    /white-space: nowrap/,
    /\.crumbs \.crumb\.is-current/,
    /color: var\(--ink\)/,
  ];
  if (shared.some((pattern) => pattern.test(text))) return true;
  if (sign === "+") {
    return [
      /tool-row--invalid/,
      /branch-cond-row/,
      /inline-skill/,
      /skill-chip em/,
      /skill-chip\.is-invalid/,
      /node--source-file/,
      /bld-toml--graph/,
      /grid-template-columns/,
      /max-width: 120px/,
    ].some((pattern) => pattern.test(text));
  }
  return false;
}

const styleDiff = diff("styles.css")
  .split("\n")
  .filter((line) => line.startsWith("+") || line.startsWith("-") || line.startsWith("@@"));

let allowedAddedBlock = false;
const unexpected = styleDiff.filter((line) => {
  const sign = line[0];
  if (line.startsWith("@@")) {
    allowedAddedBlock = false;
    return !allowedCssDelta(sign, line);
  }
  if (line.startsWith("+++") || line.startsWith("---")) return false;
  if (sign === "+" && allowedAddedBlock) {
    if (line.includes("}")) allowedAddedBlock = false;
    return false;
  }
  if (sign === "+" && /(\.inline-skill|\+\.skill-chip em|\.crumbs \.crumb\.is-current|\.node--source-file|\.bld-toml--graph)/.test(line)) {
    if (!line.includes("}")) allowedAddedBlock = true;
    return false;
  }
  if (sign !== "+" && sign !== "-") return !allowedCssDelta(sign, line);
  return !allowedCssDelta(sign, line);
});

assert.deepEqual(unexpected, [], "styles.css drifted from the handoff outside the allowed MobKit-state additions");
