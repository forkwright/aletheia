// Distillation hooks tests
import { describe, it, expect, vi } from "vitest";
import { flushToMemory, type MemoryFlushTarget } from "./hooks.js";

function makeTarget(addResult = { added: 5, errors: 0 }): MemoryFlushTarget {
  return {
    addMemories: vi.fn().mockResolvedValue(addResult),
  };
}

describe("flushToMemory", () => {
  it("returns zeros for empty extraction", async () => {
    const target = makeTarget();
    const result = await flushToMemory(target, "syn", {
      facts: [], decisions: [], openItems: [], keyEntities: [], contradictions: [],
    });
    expect(result).toEqual({ flushed: 0, errors: 0 });
    expect(target.addMemories).not.toHaveBeenCalled();
  });

  it("combines facts and prefixed decisions", async () => {
    const target = makeTarget({ added: 3, errors: 0 });
    const result = await flushToMemory(target, "syn", {
      facts: ["fact1", "fact2"],
      decisions: ["decision1"],
      openItems: [], keyEntities: [], contradictions: [],
    });
    expect(target.addMemories).toHaveBeenCalledWith("syn", [
      "fact1", "fact2", "Decision: decision1",
    ]);
    expect(result.flushed).toBe(3);
  });

  it("retries on failure", async () => {
    const target = makeTarget();
    (target.addMemories as ReturnType<typeof vi.fn>)
      .mockRejectedValueOnce(new Error("network"))
      .mockResolvedValueOnce({ added: 2, errors: 0 });

    const result = await flushToMemory(target, "syn", {
      facts: ["a", "b"], decisions: [],
      openItems: [], keyEntities: [], contradictions: [],
    }, 3);
    expect(result.flushed).toBe(2);
    expect(target.addMemories).toHaveBeenCalledTimes(2);
  });

  it("returns error count after exhausting retries", async () => {
    const target = makeTarget();
    (target.addMemories as ReturnType<typeof vi.fn>)
      .mockRejectedValue(new Error("down"));

    const result = await flushToMemory(target, "syn", {
      facts: ["a", "b"], decisions: ["c"],
      openItems: [], keyEntities: [], contradictions: [],
    }, 1);
    expect(result.errors).toBe(3);
    expect(result.flushed).toBe(0);
  });
});
