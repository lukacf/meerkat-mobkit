import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import path from "node:path";

export default defineConfig({
  plugins: [react()],
  resolve: {
    alias: {
      "@console-core": path.resolve(__dirname, "../packages/console-core/src/index.ts"),
      "@console-components/styles": path.resolve(__dirname, "../packages/console-components/src/styles/index.ts"),
      "@console-components": path.resolve(__dirname, "../packages/console-components/src/index.ts"),
    },
  },
  optimizeDeps: {
    include: ["clsx", "react", "react-dom"],
  },
  server: {
    port: 5199,
    proxy: {
      "/console/experience": "http://127.0.0.1:63210",
      "/console/events": "http://127.0.0.1:63210",
      "/console/identity": "http://127.0.0.1:63210",
      "/console/modules": "http://127.0.0.1:63210",
      "/console/rpc": "http://127.0.0.1:63210",
      "/interactions": "http://127.0.0.1:63210",
      "/healthz": "http://127.0.0.1:63210",
    },
  },
});
