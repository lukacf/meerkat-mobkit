import path from "node:path";
import { createRequire } from "node:module";
import { fileURLToPath } from "node:url";

import { defineConfig } from "vitest/config";

const consoleRoot = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(consoleRoot, "..");
const require = createRequire(import.meta.url);
const consoleDependencies = [
  "@testing-library/react",
  "clsx",
  "d3-force",
  "react",
  "react/jsx-runtime",
  "react-dom",
  "react-dom/client",
  "react-dom/test-utils",
];

export default defineConfig({
  root: repoRoot,
  server: {
    fs: { allow: [repoRoot, consoleRoot] },
  },
  resolve: {
    alias: [
      {
        find: /^@console-core$/,
        replacement: path.resolve(consoleRoot, "../packages/console-core/src/index.ts"),
      },
      {
        find: /^@console-core\/runtime-types$/,
        replacement: path.resolve(
          consoleRoot,
          "../packages/console-core/src/runtime-types.ts",
        ),
      },
      {
        find: /^@console-components$/,
        replacement: path.resolve(
          consoleRoot,
          "../packages/console-components/src/index.ts",
        ),
      },
      ...consoleDependencies.map((dependency) => ({
        find: new RegExp(`^${dependency.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}$`),
        replacement: require.resolve(dependency),
      })),
    ],
  },
  test: {
    environment: "jsdom",
    globals: true,
    include: [
      "packages/console-components/src/topology/data.test.ts",
      "packages/console-components/src/topology/dense-graph-map.test.ts",
      "packages/console-components/src/topology/connection-picker.test.tsx",
      "packages/console-components/src/topology/topology-panel.test.tsx",
    ],
    restoreMocks: true,
    setupFiles: [path.resolve(consoleRoot, "vitest.setup.ts")],
  },
});
