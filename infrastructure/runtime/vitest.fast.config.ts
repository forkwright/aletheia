// Fast test config — excludes heavy test files for local dev and agent use
// Use: npm run test:fast
// CI runs full suite via npm run test:coverage
import { defineConfig } from "vitest/config";

export default defineConfig({
  test: {
    root: "src",
    include: ["**/*.test.ts"],
    exclude: [
      "**/*.integration.test.ts",
      // Heavy tests: full-suite variants, manager (real pipeline), store (real SQLite)
      "**/*-full.test.ts",
      "**/manager.test.ts",
      "**/manager-streaming.test.ts",
      "**/store.test.ts",
      "**/server-stream.test.ts",
    ],
    reporters: ["dot"],
    passWithNoTests: false,
    testTimeout: 5000,
    hookTimeout: 5000,
    pool: "forks",
    poolOptions: {
      forks: {
        maxForks: parseInt(process.env["VITEST_MAX_FORKS"] ?? "2", 10),
      },
    },
  },
});
