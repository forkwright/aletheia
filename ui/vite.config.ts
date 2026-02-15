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
    chunkSizeWarningLimit: 400,
    rollupOptions: {
      output: {
        manualChunks: {
          hljs: ["highlight.js"],
          markdown: ["marked", "dompurify"],
        },
      },
    },
  },
});
