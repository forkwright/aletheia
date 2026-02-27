import { describe, it, expect } from "vitest";
import { runGraphMaintenance } from "./graph-maintenance-cron.js";

describe("graph-maintenance-cron", () => {
  // This test verifies the module loads and exports correctly.
  // Integration tests would require Neo4j + Qdrant running, which are
  // validated in the sidecar's own test suite and manual QA script runs.
  it("exports runGraphMaintenance function", () => {
    expect(typeof runGraphMaintenance).toBe("function");
  });

  it("handles missing scripts directory gracefully", async () => {
    // Override cwd to a non-existent location
    const origCwd = process.cwd;
    process.cwd = () => "/nonexistent/path";

    try {
      await expect(runGraphMaintenance()).rejects.toThrow("Cannot find memory scripts directory");
    } finally {
      process.cwd = origCwd;
    }
  });
});
