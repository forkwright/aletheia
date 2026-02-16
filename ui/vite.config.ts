import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";

export default defineConfig({
  plugins: [svelte()],
  base: "/ui/",
  server: {
    proxy: {
      "/api": "http://localhost:18789",
    },
  },
  build: {
    outDir: "dist",
    emptyOutDir: true,
    rollupOptions: {
      output: {
        manualChunks: {
          markdown: ["marked", "dompurify", "highlight.js/lib/core"],
          graph3d: ["three", "3d-force-graph"],
        },
      },
    },
  },
});
