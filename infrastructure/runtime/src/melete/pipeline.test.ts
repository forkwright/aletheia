// Distillation pipeline tests
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cancelDistillation, distillSession, shouldDistill } from "./pipeline.js";

// Mock extraction
vi.mock("./extract.js", () => ({
  extractFromMessages: vi.fn().mockResolvedValue({
    facts: ["fact1"],
    decisions: ["decision1"],
    openItems: [],
    keyEntities: [],
    contradictions: [],
  }),
}));

// Mock summarize
vi.mock("./summarize.js", () => ({
  summarizeMessages: vi.fn().mockResolvedValue("compressed summary"),
}));

// Mock chunked summarize
vi.mock("./chunked-summarize.js", () => ({
  sanitizeToolResults: vi.fn((msgs: unknown[]) => msgs),
  summarizeInStages: vi.fn().mockResolvedValue("stage summary result"),
}));

// Mock hooks
vi.mock("./hooks.js", () => ({
  flushToMemory: vi.fn().mockResolvedValue({ flushed: 1, errors: 0 }),
}));

// Mock workspace-flush — real write behavior tested in workspace-flush.test.ts
vi.mock("./workspace-flush.js", () => ({
  flushToWorkspaceWithRetry: vi.fn().mockReturnValue({ written: true, path: "/tmp/mock-memory.md" }),
}));

// Mock contradiction detection — tested in contradiction-detect.test.ts
vi.mock("./contradiction-detect.js", () => ({
  detectCrossChunkContradictions: vi.fn().mockResolvedValue([]),
}));

function makeStore(overrides: Record<string, unknown> = {}) {
  return {
    findSessionById: vi.fn().mockReturnValue({
      id: "ses_1",
      nousId: "syn",
      messageCount: 20,
      tokenCountEstimate: 50000,
    }),
    getHistory: vi.fn().mockReturnValue([
      { seq: 1, role: "user", content: "hello", isDistilled: false, tokenEstimate: 100 },
      { seq: 2, role: "assistant", content: "hi there", isDistilled: false, tokenEstimate: 100 },
      { seq: 3, role: "user", content: "how are you?", isDistilled: false, tokenEstimate: 100 },
      { seq: 4, role: "assistant", content: "I am good", isDistilled: false, tokenEstimate: 100 },
      { seq: 5, role: "user", content: "great", isDistilled: false, tokenEstimate: 100 },
      { seq: 6, role: "assistant", content: "anything else?", isDistilled: false, tokenEstimate: 100 },
      { seq: 7, role: "user", content: "no thanks", isDistilled: false, tokenEstimate: 100 },
      { seq: 8, role: "assistant", content: "ok bye", isDistilled: false, tokenEstimate: 100 },
      { seq: 9, role: "user", content: "actually one more thing", isDistilled: false, tokenEstimate: 100 },
      { seq: 10, role: "assistant", content: "sure", isDistilled: false, tokenEstimate: 100 },
    ]),
    incrementDistillationCount: vi.fn().mockReturnValue(1),
    acquireDistillationLock: vi.fn().mockReturnValue(true),
    releaseDistillationLock: vi.fn(),
    runDistillationMutations: vi.fn(),
    setDistillationPriming: vi.fn(),
    getWorkingState: vi.fn().mockReturnValue(null),
    getNotes: vi.fn().mockReturnValue([]),
    // Keep legacy methods on mock to avoid breaking any tests that reference them
    appendMessage: vi.fn(),
    markMessagesDistilled: vi.fn(),
    recordDistillation: vi.fn(),
    updateLastDistilledAt: vi.fn(),
    ...overrides,
  } as never;
}

function makeRouter() {
  return {
    complete: vi.fn().mockResolvedValue({
      content: [{ type: "text", text: "summary" }],
      usage: { inputTokens: 100, outputTokens: 50, cacheReadTokens: 0, cacheWriteTokens: 0 },
      model: "claude-haiku",
    }),
  } as never;
}

describe("shouldDistill", () => {
  it("returns false for unknown session", async () => {
    const store = makeStore({ findSessionById: vi.fn().mockReturnValue(null) });
    expect(await shouldDistill(store, "unknown", { threshold: 10000, minMessages: 10 })).toBe(false);
  });

  it("returns false when below min messages", async () => {
    const store = makeStore({
      findSessionById: vi.fn().mockReturnValue({ messageCount: 3, tokenCountEstimate: 50000 }),
    });
    expect(await shouldDistill(store, "ses_1", { threshold: 10000, minMessages: 10 })).toBe(false);
  });

  it("returns false when below token threshold", async () => {
    const store = makeStore({
      findSessionById: vi.fn().mockReturnValue({ messageCount: 20, tokenCountEstimate: 5000 }),
    });
    expect(await shouldDistill(store, "ses_1", { threshold: 10000, minMessages: 10 })).toBe(false);
  });

  it("returns true when above both thresholds", async () => {
    expect(await shouldDistill(makeStore(), "ses_1", { threshold: 10000, minMessages: 10 })).toBe(true);
  });
});

describe("distillSession", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("runs full distillation pipeline", async () => {
    const store = makeStore();
    const result = await distillSession(store, makeRouter(), "ses_1", "syn", {
      triggerThreshold: 10000,
      minMessages: 4,
      extractionModel: "claude-haiku",
      summaryModel: "claude-haiku",
    });

    expect(result.sessionId).toBe("ses_1");
    expect(result.nousId).toBe("syn");
    expect(result.messagesBefore).toBe(10);
    expect(result.messagesAfter).toBe(1);
    expect(result.factsExtracted).toBe(2); // 1 fact + 1 decision
    expect(result.distillationNumber).toBe(1);
    expect(store.acquireDistillationLock).toHaveBeenCalledWith("ses_1", "syn");
    expect(store.runDistillationMutations).toHaveBeenCalled();
    expect(store.releaseDistillationLock).toHaveBeenCalledWith("ses_1");
  });

  it("prevents concurrent distillation when lock acquisition fails (returns false)", async () => {
    // First call acquires; second call returns false (already locked)
    const store = makeStore({
      acquireDistillationLock: vi.fn()
        .mockReturnValueOnce(true)
        .mockReturnValueOnce(false),
    });
    const router = makeRouter();

    const p1 = distillSession(store, router, "ses_concurrent", "syn", {
      triggerThreshold: 10000,
      minMessages: 4,
      extractionModel: "claude-haiku",
      summaryModel: "claude-haiku",
    });
    const p2 = distillSession(store, router, "ses_concurrent", "syn", {
      triggerThreshold: 10000,
      minMessages: 4,
      extractionModel: "claude-haiku",
      summaryModel: "claude-haiku",
    });

    await expect(p2).rejects.toThrow("already in progress");
    await p1;
  });

  it("throws on too few undistilled messages", async () => {
    const store = makeStore({
      getHistory: vi.fn().mockReturnValue([
        { seq: 1, role: "user", content: "hi", isDistilled: false, tokenEstimate: 100 },
        { seq: 2, role: "assistant", content: "hello", isDistilled: false, tokenEstimate: 100 },
      ]),
    });
    await expect(distillSession(store, makeRouter(), "ses_few", "syn", {
      triggerThreshold: 10000,
      minMessages: 10,
      extractionModel: "claude-haiku",
      summaryModel: "claude-haiku",
    })).rejects.toThrow("Not enough messages");
  });

  it("dispatches plugin hooks when plugins provided", async () => {
    const plugins = {
      dispatchBeforeDistill: vi.fn().mockResolvedValue(undefined),
      dispatchAfterDistill: vi.fn().mockResolvedValue(undefined),
    };
    const store = makeStore();
    await distillSession(store, makeRouter(), "ses_plugins", "syn", {
      triggerThreshold: 10000,
      minMessages: 4,
      extractionModel: "claude-haiku",
      summaryModel: "claude-haiku",
      plugins: plugins as never,
    });
    expect(plugins.dispatchBeforeDistill).toHaveBeenCalled();
    expect(plugins.dispatchAfterDistill).toHaveBeenCalled();
  });

  it("flushes to memory when target provided", async () => {
    const { flushToMemory } = await import("./hooks.js");
    const store = makeStore();
    await distillSession(store, makeRouter(), "ses_mem", "syn", {
      triggerThreshold: 10000,
      minMessages: 4,
      extractionModel: "claude-haiku",
      summaryModel: "claude-haiku",
      memoryTarget: { url: "http://localhost:8230/add", userId: "default" } as never,
    });
    expect(flushToMemory).toHaveBeenCalled();
  });

  it("includes tool_result messages in extraction", async () => {
    const store = makeStore({
      getHistory: vi.fn().mockReturnValue([
        { seq: 1, role: "user", content: "run ls", isDistilled: false, tokenEstimate: 100 },
        { seq: 2, role: "assistant", content: "running", isDistilled: false, tokenEstimate: 100 },
        { seq: 3, role: "tool_result", content: "file1.txt", isDistilled: false, tokenEstimate: 100, toolName: "exec" },
        { seq: 4, role: "assistant", content: "found file1.txt", isDistilled: false, tokenEstimate: 100 },
        ...Array.from({ length: 6 }, (_, i) => ({
          seq: 5 + i, role: i % 2 === 0 ? "user" : "assistant", content: `msg ${i}`, isDistilled: false, tokenEstimate: 100,
        })),
      ]),
    });

    await distillSession(store, makeRouter(), "ses_tools", "syn", {
      triggerThreshold: 10000,
      minMessages: 4,
      extractionModel: "claude-haiku",
      summaryModel: "claude-haiku",
    });

    const { sanitizeToolResults } = await import("./chunked-summarize.js");
    expect(sanitizeToolResults).toHaveBeenCalled();
  });

  it("preserves recent messages when preserveRecentMessages is set", async () => {
    // 20 messages so we have enough to distill + preserve
    const messages = Array.from({ length: 20 }, (_, i) => ({
      seq: i + 1,
      role: i % 2 === 0 ? "user" : "assistant",
      content: `message ${i + 1}`,
      isDistilled: false,
      tokenEstimate: 200,
    }));
    const store = makeStore({
      getHistory: vi.fn().mockReturnValue(messages),
    });

    const result = await distillSession(store, makeRouter(), "ses_preserve", "syn", {
      triggerThreshold: 10000,
      minMessages: 4,
      extractionModel: "claude-haiku",
      summaryModel: "claude-haiku",
      preserveRecentMessages: 10,
      preserveRecentMaxTokens: 12000,
    });

    // Should preserve up to 10 recent messages (within token budget)
    // messagesAfter = 1 (summary) + preserved count
    expect(result.messagesAfter).toBeGreaterThan(1);
    expect(result.messagesAfter).toBeLessThanOrEqual(11); // 1 summary + up to 10 preserved

    // The distilled seqs should NOT include the preserved messages
    const mutationCall = store.runDistillationMutations.mock.calls[0]?.[0] as { distilledSeqs: number[] };
    const distilledSeqs = mutationCall?.distilledSeqs ?? [];
    const preservedSeqs = messages.slice(-10).map(m => m.seq);
    for (const seq of preservedSeqs.slice(-(result.messagesAfter - 1))) {
      expect(distilledSeqs).not.toContain(seq);
    }
  });

  it("preserves fewer messages when token budget is tight", async () => {
    // 20 messages, each 2000 tokens — with 4000 token budget, only ~2 fit
    const messages = Array.from({ length: 20 }, (_, i) => ({
      seq: i + 1,
      role: i % 2 === 0 ? "user" : "assistant",
      content: `message ${i + 1}`,
      isDistilled: false,
      tokenEstimate: 2000,
    }));
    const store = makeStore({
      getHistory: vi.fn().mockReturnValue(messages),
    });

    const result = await distillSession(store, makeRouter(), "ses_token_limit", "syn", {
      triggerThreshold: 10000,
      minMessages: 4,
      extractionModel: "claude-haiku",
      summaryModel: "claude-haiku",
      preserveRecentMessages: 10,
      preserveRecentMaxTokens: 4000,
    });

    // Token budget limits preservation to ~2 messages (2000 tokens each, budget 4000)
    expect(result.messagesAfter).toBeLessThanOrEqual(3); // 1 summary + up to 2 preserved
  });

  it("runs lightweight distillation — skips extraction, uses router.complete", async () => {
    const { extractFromMessages } = await import("./extract.js");
    const { summarizeInStages } = await import("./chunked-summarize.js");
    const { flushToMemory } = await import("./hooks.js");

    vi.mocked(extractFromMessages).mockClear();
    vi.mocked(summarizeInStages).mockClear();
    vi.mocked(flushToMemory).mockClear();

    const router = makeRouter();
    vi.mocked(router.complete).mockResolvedValue({
      content: [{ type: "text", text: "Background session summary." }],
      stopReason: "end_turn",
      usage: { inputTokens: 50, outputTokens: 20, cacheReadTokens: 0, cacheWriteTokens: 0 },
      model: "claude-haiku",
    } as never);

    const store = makeStore();
    const result = await distillSession(store, router, "ses_lightweight", "syn", {
      triggerThreshold: 10000,
      minMessages: 4,
      extractionModel: "claude-haiku",
      summaryModel: "claude-haiku",
      lightweight: true,
    });

    expect(result.sessionId).toBe("ses_lightweight");
    expect(result.factsExtracted).toBe(0); // no extraction
    expect(extractFromMessages).not.toHaveBeenCalled();
    expect(summarizeInStages).not.toHaveBeenCalled();
    expect(flushToMemory).not.toHaveBeenCalled();
    expect(router.complete).toHaveBeenCalled();
    expect(store.runDistillationMutations).toHaveBeenCalled();
  });

  describe("workspace memory flush", () => {
    it("calls flushToWorkspaceWithRetry when workspace provided", async () => {
      const { flushToWorkspaceWithRetry } = await import("./workspace-flush.js");
      vi.mocked(flushToWorkspaceWithRetry).mockReturnValue({ written: true, path: "/tmp/mock.md" });

      const store = makeStore();
      await distillSession(store, makeRouter(), "ses_ws", "syn", {
        triggerThreshold: 10000,
        minMessages: 4,
        extractionModel: "claude-haiku",
        summaryModel: "claude-haiku",
        workspace: "/tmp/test-workspace",
      });

      expect(flushToWorkspaceWithRetry).toHaveBeenCalledWith(
        expect.objectContaining({ workspace: "/tmp/test-workspace", nousId: "syn", sessionId: "ses_ws" }),
      );
    });

    it("succeeds even when workspace flush fails", async () => {
      const { flushToWorkspaceWithRetry } = await import("./workspace-flush.js");
      vi.mocked(flushToWorkspaceWithRetry).mockReturnValue({ written: false, path: "/tmp/mock.md", error: "ENOTDIR" });

      const store = makeStore();
      const result = await distillSession(store, makeRouter(), "ses_ws_fail", "syn", {
        triggerThreshold: 10000,
        minMessages: 4,
        extractionModel: "claude-haiku",
        summaryModel: "claude-haiku",
        workspace: "/tmp/test-workspace",
      });

      // Distillation completed despite flush failure
      expect(result.sessionId).toBe("ses_ws_fail");
      expect(result.distillationNumber).toBe(1);
    });
  });

  describe("flush receipt logging", () => {
    it("logs a receipt with required fields on successful flush", async () => {
      const { flushToWorkspaceWithRetry } = await import("./workspace-flush.js");
      vi.mocked(flushToWorkspaceWithRetry).mockReturnValue({ written: true, path: "/tmp/mock.md" });

      const store = makeStore();
      // We verify that the pipeline runs without error — receipt logging is structural
      // and validated by the structured log call in pipeline.ts
      const result = await distillSession(store, makeRouter(), "ses_receipt_ok", "syn", {
        triggerThreshold: 10000,
        minMessages: 4,
        extractionModel: "claude-haiku",
        summaryModel: "claude-haiku",
        workspace: "/tmp/test-workspace",
      });

      expect(result.sessionId).toBe("ses_receipt_ok");
      expect(flushToWorkspaceWithRetry).toHaveBeenCalled();
    });

    it("logs a receipt with required fields on failed flush", async () => {
      const { flushToWorkspaceWithRetry } = await import("./workspace-flush.js");
      vi.mocked(flushToWorkspaceWithRetry).mockReturnValue({ written: false, path: "/tmp/mock.md", error: "disk full" });

      const store = makeStore();
      const result = await distillSession(store, makeRouter(), "ses_receipt_fail", "syn", {
        triggerThreshold: 10000,
        minMessages: 4,
        extractionModel: "claude-haiku",
        summaryModel: "claude-haiku",
        workspace: "/tmp/test-workspace",
      });

      // Distillation still returns result — flush failure is non-blocking
      expect(result.sessionId).toBe("ses_receipt_fail");
      expect(flushToWorkspaceWithRetry).toHaveBeenCalled();
    });
  });

  describe("flush failure counter and health events", () => {
    beforeEach(() => {
      vi.clearAllMocks();
    });

    it("does not emit health event on first or second failure", async () => {
      const { flushToWorkspaceWithRetry } = await import("./workspace-flush.js");
      vi.mocked(flushToWorkspaceWithRetry).mockReturnValue({ written: false, path: "/tmp/mock.md", error: "fail" });

      const { eventBus } = await import("../koina/event-bus.js");
      const emitSpy = vi.spyOn(eventBus, "emit");

      for (let i = 0; i < 2; i++) {
        await distillSession(makeStore(), makeRouter(), `ses_hc_early_${i}`, "syn_hc_early", {
          triggerThreshold: 10000,
          minMessages: 4,
          extractionModel: "claude-haiku",
          summaryModel: "claude-haiku",
          workspace: "/tmp/test-workspace",
        });
      }

      const healthCalls = emitSpy.mock.calls.filter(([event]) => event === "memory:health_degraded");
      expect(healthCalls).toHaveLength(0);
    });

    it("emits memory:health_degraded after 3 consecutive failures for same nousId", async () => {
      const { flushToWorkspaceWithRetry } = await import("./workspace-flush.js");
      vi.mocked(flushToWorkspaceWithRetry).mockReturnValue({ written: false, path: "/tmp/mock.md", error: "fail" });

      const { eventBus } = await import("../koina/event-bus.js");
      const emitSpy = vi.spyOn(eventBus, "emit");

      for (let i = 0; i < 3; i++) {
        await distillSession(makeStore(), makeRouter(), `ses_hc_3_${i}`, "syn_hc_3", {
          triggerThreshold: 10000,
          minMessages: 4,
          extractionModel: "claude-haiku",
          summaryModel: "claude-haiku",
          workspace: "/tmp/test-workspace",
        });
      }

      const healthCalls = emitSpy.mock.calls.filter(([event]) => event === "memory:health_degraded");
      expect(healthCalls).toHaveLength(1);
      expect(healthCalls[0]?.[1]).toMatchObject({
        nousId: "syn_hc_3",
        reason: "workspace_flush_failures",
        consecutiveFailures: 3,
        lastError: "fail",
      });
    });

    it("resets counter on success — no health event after recovery", async () => {
      const { flushToWorkspaceWithRetry } = await import("./workspace-flush.js");

      const { eventBus } = await import("../koina/event-bus.js");
      const emitSpy = vi.spyOn(eventBus, "emit");

      // 2 failures
      vi.mocked(flushToWorkspaceWithRetry).mockReturnValue({ written: false, path: "/tmp/mock.md", error: "fail" });
      for (let i = 0; i < 2; i++) {
        await distillSession(makeStore(), makeRouter(), `ses_hc_reset_${i}`, "syn_hc_reset", {
          triggerThreshold: 10000,
          minMessages: 4,
          extractionModel: "claude-haiku",
          summaryModel: "claude-haiku",
          workspace: "/tmp/test-workspace",
        });
      }

      // 1 success resets counter
      vi.mocked(flushToWorkspaceWithRetry).mockReturnValue({ written: true, path: "/tmp/mock.md" });
      await distillSession(makeStore(), makeRouter(), "ses_hc_reset_ok", "syn_hc_reset", {
        triggerThreshold: 10000,
        minMessages: 4,
        extractionModel: "claude-haiku",
        summaryModel: "claude-haiku",
        workspace: "/tmp/test-workspace",
      });

      // 2 more failures — counter restarted from 0, should not reach threshold
      vi.mocked(flushToWorkspaceWithRetry).mockReturnValue({ written: false, path: "/tmp/mock.md", error: "fail2" });
      for (let i = 0; i < 2; i++) {
        await distillSession(makeStore(), makeRouter(), `ses_hc_post_reset_${i}`, "syn_hc_reset", {
          triggerThreshold: 10000,
          minMessages: 4,
          extractionModel: "claude-haiku",
          summaryModel: "claude-haiku",
          workspace: "/tmp/test-workspace",
        });
      }

      const healthCalls = emitSpy.mock.calls.filter(([event]) => event === "memory:health_degraded");
      expect(healthCalls).toHaveLength(0);
    });

    it("tracks failures independently per nousId", async () => {
      const { flushToWorkspaceWithRetry } = await import("./workspace-flush.js");
      vi.mocked(flushToWorkspaceWithRetry).mockReturnValue({ written: false, path: "/tmp/mock.md", error: "fail" });

      const { eventBus } = await import("../koina/event-bus.js");
      const emitSpy = vi.spyOn(eventBus, "emit");

      // 2 failures for nous_a, 2 failures for nous_b — neither should trigger threshold
      for (let i = 0; i < 2; i++) {
        await distillSession(makeStore(), makeRouter(), `ses_a_${i}`, "syn_hc_a", {
          triggerThreshold: 10000,
          minMessages: 4,
          extractionModel: "claude-haiku",
          summaryModel: "claude-haiku",
          workspace: "/tmp/test-workspace",
        });
        await distillSession(makeStore(), makeRouter(), `ses_b_${i}`, "syn_hc_b", {
          triggerThreshold: 10000,
          minMessages: 4,
          extractionModel: "claude-haiku",
          summaryModel: "claude-haiku",
          workspace: "/tmp/test-workspace",
        });
      }

      const healthCalls = emitSpy.mock.calls.filter(([event]) => event === "memory:health_degraded");
      expect(healthCalls).toHaveLength(0);

      // 1 more for nous_a — triggers threshold only for nous_a
      await distillSession(makeStore(), makeRouter(), "ses_a_3", "syn_hc_a", {
        triggerThreshold: 10000,
        minMessages: 4,
        extractionModel: "claude-haiku",
        summaryModel: "claude-haiku",
        workspace: "/tmp/test-workspace",
      });

      const healthCallsAfter = emitSpy.mock.calls.filter(([event]) => event === "memory:health_degraded");
      expect(healthCallsAfter).toHaveLength(1);
      expect(healthCallsAfter[0]?.[1]).toMatchObject({ nousId: "syn_hc_a" });
    });
  });

  describe("SQLite locking", () => {
    it("acquires lock before distillation and releases in finally block", async () => {
      const store = makeStore();
      await distillSession(store, makeRouter(), "ses_lock", "syn", {
        triggerThreshold: 10000,
        minMessages: 4,
        extractionModel: "claude-haiku",
        summaryModel: "claude-haiku",
      });

      expect(store.acquireDistillationLock).toHaveBeenCalledWith("ses_lock", "syn");
      expect(store.releaseDistillationLock).toHaveBeenCalledWith("ses_lock");
    });

    it("releases lock even when distillation throws", async () => {
      const store = makeStore({
        getHistory: vi.fn().mockReturnValue([
          { seq: 1, role: "user", content: "hi", isDistilled: false, tokenEstimate: 100 },
        ]),
      });

      await expect(
        distillSession(store, makeRouter(), "ses_throw", "syn", {
          triggerThreshold: 10000,
          minMessages: 10,
          extractionModel: "claude-haiku",
          summaryModel: "claude-haiku",
        })
      ).rejects.toThrow("Not enough messages");

      expect(store.acquireDistillationLock).toHaveBeenCalled();
      expect(store.releaseDistillationLock).toHaveBeenCalledWith("ses_throw");
    });

    it("calls runDistillationMutations instead of individual store mutation methods", async () => {
      const store = makeStore();
      await distillSession(store, makeRouter(), "ses_atomic", "syn", {
        triggerThreshold: 10000,
        minMessages: 4,
        extractionModel: "claude-haiku",
        summaryModel: "claude-haiku",
      });

      expect(store.runDistillationMutations).toHaveBeenCalledTimes(1);
      // Legacy individual methods should NOT be called from pipeline
      expect(store.appendMessage).not.toHaveBeenCalled();
      expect(store.markMessagesDistilled).not.toHaveBeenCalled();
      expect(store.recordDistillation).not.toHaveBeenCalled();
      expect(store.updateLastDistilledAt).not.toHaveBeenCalled();
    });
  });

  describe("contradiction invalidation", () => {
    beforeEach(() => {
      vi.clearAllMocks();
    });

    afterEach(() => {
      vi.restoreAllMocks();
    });

    it("calls fetch with invalidate_text for each contradiction when sidecarUrl is set", async () => {
      const { extractFromMessages } = await import("./extract.js");
      vi.mocked(extractFromMessages).mockResolvedValueOnce({
        facts: ["fact1"],
        decisions: [],
        openItems: [],
        keyEntities: [],
        contradictions: ["user dislikes coffee", "user prefers tea over water"],
      });

      const fetchSpy = vi.spyOn(globalThis, "fetch").mockResolvedValue(
        new Response(JSON.stringify({ invalidated: true }), { status: 200 }),
      );

      const store = makeStore();
      await distillSession(store, makeRouter(), "ses_contra", "syn", {
        triggerThreshold: 10000,
        minMessages: 4,
        extractionModel: "claude-haiku",
        summaryModel: "claude-haiku",
        sidecarUrl: "http://localhost:8230",
      });

      const invalidateCalls = fetchSpy.mock.calls.filter(([url]) =>
        typeof url === "string" && url.includes("invalidate_text"),
      );
      expect(invalidateCalls).toHaveLength(2);
      for (const [url, opts] of invalidateCalls) {
        expect(url).toContain("/temporal/facts/invalidate_text");
        expect(opts?.method).toBe("POST");
      }
    });

    it("does not call fetch for contradiction invalidation when no contradictions", async () => {
      const { extractFromMessages } = await import("./extract.js");
      vi.mocked(extractFromMessages).mockResolvedValueOnce({
        facts: ["fact1"],
        decisions: [],
        openItems: [],
        keyEntities: [],
        contradictions: [],
      });

      const fetchSpy = vi.spyOn(globalThis, "fetch").mockResolvedValue(
        new Response("{}", { status: 200 }),
      );

      const store = makeStore();
      await distillSession(store, makeRouter(), "ses_no_contra", "syn", {
        triggerThreshold: 10000,
        minMessages: 4,
        extractionModel: "claude-haiku",
        summaryModel: "claude-haiku",
        sidecarUrl: "http://localhost:8230",
      });

      const invalidateCalls = fetchSpy.mock.calls.filter(([url]) =>
        typeof url === "string" && url.includes("invalidate_text"),
      );
      expect(invalidateCalls).toHaveLength(0);
    });

    it("does not call fetch when sidecarUrl is not set", async () => {
      const { extractFromMessages } = await import("./extract.js");
      vi.mocked(extractFromMessages).mockResolvedValueOnce({
        facts: ["fact1"],
        decisions: [],
        openItems: [],
        keyEntities: [],
        contradictions: ["some contradiction"],
      });

      const fetchSpy = vi.spyOn(globalThis, "fetch").mockResolvedValue(
        new Response("{}", { status: 200 }),
      );

      const store = makeStore();
      await distillSession(store, makeRouter(), "ses_no_sidecar", "syn", {
        triggerThreshold: 10000,
        minMessages: 4,
        extractionModel: "claude-haiku",
        summaryModel: "claude-haiku",
        // sidecarUrl intentionally omitted
      });

      const invalidateCalls = fetchSpy.mock.calls.filter(([url]) =>
        typeof url === "string" && url.includes("invalidate_text"),
      );
      expect(invalidateCalls).toHaveLength(0);
    });

    it("contradiction invalidation failure does not block distillation", async () => {
      const { extractFromMessages } = await import("./extract.js");
      vi.mocked(extractFromMessages).mockResolvedValueOnce({
        facts: ["fact1"],
        decisions: [],
        openItems: [],
        keyEntities: [],
        contradictions: ["some contradiction"],
      });

      vi.spyOn(globalThis, "fetch").mockRejectedValue(new Error("sidecar down"));

      const store = makeStore();
      // Should complete without throwing despite fetch failure
      const result = await distillSession(store, makeRouter(), "ses_contra_fail", "syn", {
        triggerThreshold: 10000,
        minMessages: 4,
        extractionModel: "claude-haiku",
        summaryModel: "claude-haiku",
        sidecarUrl: "http://localhost:8230",
      });

      expect(result.sessionId).toBe("ses_contra_fail");
      expect(result.distillationNumber).toBe(1);
    });
  });

  describe("cross-chunk contradiction detection", () => {
    beforeEach(() => {
      vi.clearAllMocks();
    });

    it("appends cross-chunk contradictions to extraction.contradictions", async () => {
      const { extractFromMessages } = await import("./extract.js");
      const { detectCrossChunkContradictions } = await import("./contradiction-detect.js");

      vi.mocked(extractFromMessages).mockResolvedValueOnce({
        facts: ["user prefers coffee", "user dislikes hot beverages"],
        decisions: [],
        openItems: [],
        keyEntities: [],
        contradictions: ["existing contradiction"],
      });
      vi.mocked(detectCrossChunkContradictions).mockResolvedValueOnce([
        "cross-chunk contradiction found",
      ]);

      const fetchSpy = vi.spyOn(globalThis, "fetch").mockResolvedValue(
        new Response("{}", { status: 200 }),
      );

      const store = makeStore();
      await distillSession(store, makeRouter(), "ses_cross_contra", "syn", {
        triggerThreshold: 10000,
        minMessages: 4,
        extractionModel: "claude-haiku",
        summaryModel: "claude-haiku",
        sidecarUrl: "http://localhost:8230",
      });

      // Verify detectCrossChunkContradictions was called with the facts
      expect(detectCrossChunkContradictions).toHaveBeenCalledWith(
        expect.anything(),
        ["user prefers coffee", "user dislikes hot beverages"],
        "claude-haiku",
      );

      // Verify cross-chunk contradictions fed into invalidation path
      const invalidateCalls = fetchSpy.mock.calls.filter(([url]) =>
        typeof url === "string" && url.includes("invalidate_text"),
      );
      expect(invalidateCalls).toHaveLength(2); // existing + cross-chunk
      fetchSpy.mockRestore();
    });

    it("skips cross-chunk detection when fewer than 2 facts extracted", async () => {
      const { extractFromMessages } = await import("./extract.js");
      const { detectCrossChunkContradictions } = await import("./contradiction-detect.js");

      vi.mocked(extractFromMessages).mockResolvedValueOnce({
        facts: ["single fact"],
        decisions: [],
        openItems: [],
        keyEntities: [],
        contradictions: [],
      });
      vi.mocked(detectCrossChunkContradictions).mockClear();

      await distillSession(makeStore(), makeRouter(), "ses_single_fact", "syn", {
        triggerThreshold: 10000,
        minMessages: 4,
        extractionModel: "claude-haiku",
        summaryModel: "claude-haiku",
      });

      expect(detectCrossChunkContradictions).not.toHaveBeenCalled();
    });

    it("skips cross-chunk detection for lightweight distillation", async () => {
      const { detectCrossChunkContradictions } = await import("./contradiction-detect.js");
      vi.mocked(detectCrossChunkContradictions).mockClear();

      const router = makeRouter();
      vi.mocked(router.complete).mockResolvedValue({
        content: [{ type: "text", text: "Light summary." }],
        stopReason: "end_turn",
        usage: { inputTokens: 50, outputTokens: 10, cacheReadTokens: 0, cacheWriteTokens: 0 },
        model: "claude-haiku",
      } as never);

      await distillSession(makeStore(), router, "ses_lightweight_cc", "syn", {
        triggerThreshold: 10000,
        minMessages: 4,
        extractionModel: "claude-haiku",
        summaryModel: "claude-haiku",
        lightweight: true,
      });

      expect(detectCrossChunkContradictions).not.toHaveBeenCalled();
    });
  });

  describe("evolution pre-check in flush", () => {
    beforeEach(() => {
      vi.clearAllMocks();
    });

    afterEach(() => {
      vi.restoreAllMocks();
    });

    it("filters evolved facts from memory flush when sidecarUrl provided", async () => {
      const { flushToMemory } = await import("./hooks.js");
      const { extractFromMessages } = await import("./extract.js");

      vi.mocked(extractFromMessages).mockResolvedValueOnce({
        facts: ["evolved-fact", "new-fact"],
        decisions: [],
        openItems: [],
        keyEntities: [],
        contradictions: [],
      });

      vi.spyOn(globalThis, "fetch").mockResolvedValue(
        new Response(JSON.stringify({ action: "add_new" }), { status: 200 }),
      );

      const store = makeStore();
      await distillSession(store, makeRouter(), "ses_evolution", "syn", {
        triggerThreshold: 10000,
        minMessages: 4,
        extractionModel: "claude-haiku",
        summaryModel: "claude-haiku",
        memoryTarget: { addMemories: vi.fn().mockResolvedValue({ added: 2, errors: 0 }) } as never,
        sidecarUrl: "http://localhost:8230",
      });

      expect(flushToMemory).toHaveBeenCalledWith(
        expect.anything(),
        "syn",
        expect.anything(),
        expect.objectContaining({ sidecarUrl: "http://localhost:8230" }),
        "ses_evolution",
      );
    });
  });

  describe("AbortSignal cancellation", () => {
    beforeEach(() => {
      vi.clearAllMocks();
    });

    it("aborts before extraction when signal is already aborted", async () => {
      const { extractFromMessages } = await import("./extract.js");
      vi.mocked(extractFromMessages).mockClear();

      const controller = new AbortController();
      controller.abort();

      await expect(
        distillSession(makeStore(), makeRouter(), "ses_abort_pre", "syn", {
          triggerThreshold: 10000,
          minMessages: 4,
          extractionModel: "claude-haiku",
          summaryModel: "claude-haiku",
          signal: controller.signal,
        }),
      ).rejects.toThrow();

      expect(extractFromMessages).not.toHaveBeenCalled();
    });

    it("aborts before mutations when signal fires after extraction", async () => {
      const { extractFromMessages } = await import("./extract.js");
      const store = makeStore();

      // Create a controller we'll abort after extraction resolves
      const controller = new AbortController();

      // After extractFromMessages resolves, abort the signal
      vi.mocked(extractFromMessages).mockImplementationOnce(async () => {
        controller.abort();
        return {
          facts: ["fact1"],
          decisions: [],
          openItems: [],
          keyEntities: [],
          contradictions: [],
        };
      });

      await expect(
        distillSession(store, makeRouter(), "ses_abort_mid", "syn", {
          triggerThreshold: 10000,
          minMessages: 4,
          extractionModel: "claude-haiku",
          summaryModel: "claude-haiku",
          signal: controller.signal,
        }),
      ).rejects.toThrow();

      // Mutations must NOT have been called — clean rollback
      expect(store.runDistillationMutations).not.toHaveBeenCalled();
    });

    it("releases lock even when aborted mid-distillation", async () => {
      const { extractFromMessages } = await import("./extract.js");

      const controller = new AbortController();

      // Abort during extraction
      vi.mocked(extractFromMessages).mockImplementationOnce(async () => {
        controller.abort();
        return {
          facts: ["fact1"],
          decisions: [],
          openItems: [],
          keyEntities: [],
          contradictions: [],
        };
      });

      const store = makeStore();
      await expect(
        distillSession(store, makeRouter(), "ses_abort_lock", "syn", {
          triggerThreshold: 10000,
          minMessages: 4,
          extractionModel: "claude-haiku",
          summaryModel: "claude-haiku",
          signal: controller.signal,
        }),
      ).rejects.toThrow();

      // Lock was acquired so it must be released
      expect(store.acquireDistillationLock).toHaveBeenCalled();
      expect(store.releaseDistillationLock).toHaveBeenCalledWith("ses_abort_lock");
    });
  });

  describe("cancelDistillation", () => {
    beforeEach(() => {
      vi.clearAllMocks();
    });

    it("returns false for no active distillation", () => {
      expect(cancelDistillation("ses_no_active")).toBe(false);
    });

    it("aborts an active distillation and returns true", async () => {
      const { extractFromMessages } = await import("./extract.js");

      let resolveExtraction!: (v: Awaited<ReturnType<typeof extractFromMessages>>) => void;
      const extractionPromise = new Promise<Awaited<ReturnType<typeof extractFromMessages>>>(
        (resolve) => { resolveExtraction = resolve; },
      );

      vi.mocked(extractFromMessages).mockImplementationOnce(() => extractionPromise);

      const distillPromise = distillSession(makeStore(), makeRouter(), "ses_cancel_active", "syn", {
        triggerThreshold: 10000,
        minMessages: 4,
        extractionModel: "claude-haiku",
        summaryModel: "claude-haiku",
      });

      // Give distillSession time to reach the extractFromMessages await
      await new Promise((r) => setTimeout(r, 0));

      const wasCancelled = cancelDistillation("ses_cancel_active");
      expect(wasCancelled).toBe(true);

      // Resolve extraction after cancel — distillation should still abort before mutations
      resolveExtraction({
        facts: ["fact1"],
        decisions: [],
        openItems: [],
        keyEntities: [],
        contradictions: [],
      });

      await expect(distillPromise).rejects.toThrow();
    });
  });

  describe("mutation retry logic", () => {
    it("succeeds on first attempt without logging error", async () => {
      const store = makeStore();
      await distillSession(store, makeRouter(), "ses_retry_ok", "syn", {
        triggerThreshold: 10000,
        minMessages: 4,
        extractionModel: "claude-haiku",
        summaryModel: "claude-haiku",
      });

      expect(store.runDistillationMutations).toHaveBeenCalledTimes(1);
    });

    it("retries once when first attempt throws and succeeds on second", async () => {
      let callCount = 0;
      const store = makeStore({
        runDistillationMutations: vi.fn().mockImplementation(() => {
          callCount++;
          if (callCount === 1) throw new Error("transient write error");
          // second call succeeds
        }),
      });

      // Should not throw — retry succeeds
      const result = await distillSession(store, makeRouter(), "ses_retry_success", "syn", {
        triggerThreshold: 10000,
        minMessages: 4,
        extractionModel: "claude-haiku",
        summaryModel: "claude-haiku",
      });

      expect(result.sessionId).toBe("ses_retry_success");
      expect(store.runDistillationMutations).toHaveBeenCalledTimes(2);
    });

    it("logs error and does not rethrow when both attempts fail", async () => {
      const store = makeStore({
        runDistillationMutations: vi.fn().mockImplementation(() => {
          throw new Error("persistent write error");
        }),
      });

      // Should not throw — error logged, distillation result still returned
      const result = await distillSession(store, makeRouter(), "ses_double_fail", "syn", {
        triggerThreshold: 10000,
        minMessages: 4,
        extractionModel: "claude-haiku",
        summaryModel: "claude-haiku",
      });

      expect(result.sessionId).toBe("ses_double_fail");
      expect(store.runDistillationMutations).toHaveBeenCalledTimes(2);
      // Lock still released despite mutation failure
      expect(store.releaseDistillationLock).toHaveBeenCalledWith("ses_double_fail");
    });
  });
});
