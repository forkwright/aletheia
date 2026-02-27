// Distillation hooks tests
import { afterEach, describe, expect, it, vi } from "vitest";
import { checkEvolutionBeforeFlush, flushToMemory, type MemoryFlushTarget } from "./hooks.js";

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
    }, 3, "test-session");
    expect(result).toEqual({ flushed: 0, errors: 0 });
    expect(target.addMemories).not.toHaveBeenCalled();
  });

  it("combines facts and prefixed decisions", async () => {
    const target = makeTarget({ added: 3, errors: 0 });
    const result = await flushToMemory(target, "syn", {
      facts: ["fact1", "fact2"],
      decisions: ["decision1"],
      openItems: [], keyEntities: [], contradictions: [],
    }, 3, "test-session");
    expect(target.addMemories).toHaveBeenCalledWith("syn", [
      "fact1", "fact2", "Decision: decision1",
    ], "test-session");
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
    }, 3, "test-session");
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
    }, 1, "test-session");
    expect(result.errors).toBe(3);
    expect(result.flushed).toBe(0);
  });

  it("runs evolution pre-check when sidecarUrl provided and filters evolved memories", async () => {
    const fetchSpy = vi.spyOn(globalThis, "fetch")
      .mockResolvedValueOnce(new Response(JSON.stringify({ action: "evolved" }), { status: 200 }))
      .mockResolvedValueOnce(new Response(JSON.stringify({ action: "add_new" }), { status: 200 }));

    const target = makeTarget({ added: 1, errors: 0 });
    const result = await flushToMemory(target, "syn", {
      facts: ["evolved-fact", "new-fact"], decisions: [],
      openItems: [], keyEntities: [], contradictions: [],
    }, { sidecarUrl: "http://localhost:8230" }, "test-session");

    // Only the non-evolved fact should reach addMemories
    expect(target.addMemories).toHaveBeenCalledWith("syn", ["new-fact"], "test-session");
    expect(result.flushed).toBe(1);
    fetchSpy.mockRestore();
  });

  it("returns zero flushed when all memories are evolved", async () => {
    // Create a fresh Response per call — body streams can only be consumed once
    const fetchSpy = vi.spyOn(globalThis, "fetch")
      .mockImplementation(async () =>
        new Response(JSON.stringify({ action: "evolved" }), { status: 200 }),
      );

    const target = makeTarget({ added: 0, errors: 0 });
    const result = await flushToMemory(target, "syn", {
      facts: ["fact1", "fact2"], decisions: [],
      openItems: [], keyEntities: [], contradictions: [],
    }, { sidecarUrl: "http://localhost:8230" }, "test-session");

    expect(target.addMemories).not.toHaveBeenCalled();
    expect(result).toEqual({ flushed: 0, errors: 0 });
    fetchSpy.mockRestore();
  });

  it("keeps memory for add_batch when evolution check fails", async () => {
    const fetchSpy = vi.spyOn(globalThis, "fetch")
      .mockRejectedValue(new Error("sidecar down"));

    const target = makeTarget({ added: 1, errors: 0 });
    await flushToMemory(target, "syn", {
      facts: ["some-fact"], decisions: [],
      openItems: [], keyEntities: [], contradictions: [],
    }, { sidecarUrl: "http://localhost:8230" }, "test-session");

    // Fail-open: fact kept for normal add_batch path
    expect(target.addMemories).toHaveBeenCalledWith("syn", ["some-fact"], "test-session");
    fetchSpy.mockRestore();
  });
});

describe("checkEvolutionBeforeFlush", () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("returns evolved action memories filtered out", async () => {
    const fetchSpy = vi.spyOn(globalThis, "fetch")
      .mockResolvedValueOnce(new Response(JSON.stringify({ action: "evolved" }), { status: 200 }))
      .mockResolvedValueOnce(new Response(JSON.stringify({ action: "add_new" }), { status: 200 }));

    const result = await checkEvolutionBeforeFlush(
      ["memory-a", "memory-b"],
      "http://localhost:8230",
      "syn",
    );

    expect(result).toEqual(["memory-b"]);
    expect(fetchSpy).toHaveBeenCalledTimes(2);
  });

  it("returns all memories when none are evolved", async () => {
    // Use mockImplementation to create fresh Response per call (body streams single-use)
    vi.spyOn(globalThis, "fetch").mockImplementation(async () =>
      new Response(JSON.stringify({ action: "add_new" }), { status: 200 }),
    );

    const result = await checkEvolutionBeforeFlush(
      ["memory-a", "memory-b"],
      "http://localhost:8230",
      "syn",
    );

    expect(result).toEqual(["memory-a", "memory-b"]);
  });

  it("keeps memories when sidecar returns non-200", async () => {
    vi.spyOn(globalThis, "fetch").mockImplementation(async () =>
      new Response("", { status: 503 }),
    );

    const result = await checkEvolutionBeforeFlush(
      ["memory-a"],
      "http://localhost:8230",
      "syn",
    );

    expect(result).toEqual(["memory-a"]);
  });

  it("keeps memories on fetch error — fail-open", async () => {
    vi.spyOn(globalThis, "fetch")
      .mockRejectedValue(new Error("connection refused"));

    const result = await checkEvolutionBeforeFlush(
      ["memory-a"],
      "http://localhost:8230",
      "syn",
    );

    expect(result).toEqual(["memory-a"]);
  });
});
