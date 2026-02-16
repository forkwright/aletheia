import { defineConfig } from "vitest/config";

export default defineConfig({
  test: {
    root: "src",
    include: ["**/*.test.ts"],
    exclude: ["**/*.integration.test.ts"],
    coverage: {
      provider: "v8",
      reporter: ["text", "json", "lcov"],
      reportsDirectory: "../coverage",
      thresholds: {
        statements: 60,
        branches: 50,
        functions: 60,
        lines: 60,
      },
      include: ["**/*.ts"],
      exclude: ["**/*.test.ts", "**/*.integration.test.ts", "entry.ts"],
    },
    reporters: ["default", "json"],
    outputFile: {
      json: "../test-results/results.json",
    },
    passWithNoTests: true,
    testTimeout: 10000,
    hookTimeout: 10000,
    pool: "forks",
  },
});
