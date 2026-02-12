import { defineConfig } from "vitest/config";

export default defineConfig({
  test: {
    root: "src",
    include: ["**/*.integration.test.ts"],
    testTimeout: 30000,
    reporters: ["default", "json"],
    outputFile: {
      json: "../test-results/integration-results.json",
    },
    pool: "forks",
  },
});
