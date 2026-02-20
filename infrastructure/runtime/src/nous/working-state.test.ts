import { describe, it, expect } from "vitest";
import { formatWorkingState } from "./working-state.js";
import type { WorkingState } from "../mneme/store.js";

describe("formatWorkingState", () => {
  it("formats a complete working state", () => {
    const state: WorkingState = {
      currentTask: "Reviewing PR #49 for recall performance fix",
      completedSteps: ["Read recall.ts diff", "Verified test coverage"],
      nextSteps: ["Merge PR", "Redeploy"],
      recentDecisions: ["Decision: vector-first search — because graph adds 1.8s latency"],
      openFiles: ["src/nous/recall.ts", "src/nous/recall.test.ts"],
      updatedAt: "2026-02-20T17:50:00Z",
    };

    const result = formatWorkingState(state);

    expect(result).toContain("## Working State");
    expect(result).toContain("Reviewing PR #49");
    expect(result).toContain("Read recall.ts diff");
    expect(result).toContain("Merge PR");
    expect(result).toContain("vector-first search");
    expect(result).toContain("src/nous/recall.ts");
  });

  it("omits empty sections", () => {
    const state: WorkingState = {
      currentTask: "Investigating timeout issue",
      completedSteps: [],
      nextSteps: [],
      recentDecisions: [],
      openFiles: [],
      updatedAt: "2026-02-20T17:50:00Z",
    };

    const result = formatWorkingState(state);

    expect(result).toContain("Investigating timeout issue");
    expect(result).not.toContain("Completed");
    expect(result).not.toContain("Next");
    expect(result).not.toContain("Decisions");
    expect(result).not.toContain("Files");
  });

  it("handles all fields populated", () => {
    const state: WorkingState = {
      currentTask: "Task",
      completedSteps: ["Step 1"],
      nextSteps: ["Next 1"],
      recentDecisions: ["Decision 1"],
      openFiles: ["file.ts"],
      updatedAt: "2026-02-20T17:50:00Z",
    };

    const result = formatWorkingState(state);

    expect(result).toContain("**Current task:**");
    expect(result).toContain("**Completed:**");
    expect(result).toContain("**Next:**");
    expect(result).toContain("**Decisions:**");
    expect(result).toContain("**Files:**");
  });
});
