import path from "node:path";
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

export default defineConfig(({ command }) => ({
  root: "src",
  // Upstream loaded `code-inspector-plugin` in dev. It is a development
  // convenience for a large UI; with seven source files it is not worth the
  // dependency, and it broke `vite build` when the tree was trimmed.
  plugins: [react()],
  // Relative asset paths: the bundle is loaded from the filesystem via the
  // custom protocol, where absolute `/assets/...` would resolve to the
  // protocol root rather than the app directory.
  base: "./",
  build: {
    outDir: "../dist",
    emptyOutDir: true,
    // The UI is 7 files; a single chunk beats a request waterfall.
    chunkSizeWarningLimit: 400,
  },
  server: {
    port: 3000,
    strictPort: true,
  },
  resolve: {
    alias: {
      "@": path.resolve(__dirname, "./src"),
    },
  },
  clearScreen: false,
  envPrefix: ["VITE_", "TAURI_"],
}));