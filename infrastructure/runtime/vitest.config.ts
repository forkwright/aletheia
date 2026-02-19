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
        statements: 80,
        branches: 78,
        functions: 90,
        lines: 80,
      },
      include: ["**/*.ts"],
      exclude: ["**/*.test.ts", "**/*.integration.test.ts", "entry.ts"],
    },
    reporters: ["default", "json"],
    outputFile: {
      json: "../test-results/results.json",
    },
    passWithNoTests: false,
    testTimeout: 10000,
    hookTimeout: 10000,
    pool: "forks",
    poolOptions: {
      forks: {
        // CI overrides via VITEST_MAX_FORKS env var; local default is conservative
        maxForks: parseInt(process.env["VITEST_MAX_FORKS"] ?? "2", 10),
      },
    },
  },
});
