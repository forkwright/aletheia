// Distillation pipeline tests
import { describe, it, expect, vi, beforeEach } from "vitest";
import { shouldDistill, distillSession } from "./pipeline.js";

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
    appendMessage: vi.fn(),
    markMessagesDistilled: vi.fn(),
    recordDistillation: vi.fn(),
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
    expect(store.appendMessage).toHaveBeenCalled();
    expect(store.markMessagesDistilled).toHaveBeenCalled();
    expect(store.recordDistillation).toHaveBeenCalled();
  });

  it("prevents concurrent distillation of same session", async () => {
    const store = makeStore();
    const router = makeRouter();
    // Start first distillation (don't await)
    const p1 = distillSession(store, router, "ses_concurrent", "syn", {
      triggerThreshold: 10000,
      minMessages: 4,
      extractionModel: "claude-haiku",
      summaryModel: "claude-haiku",
    });
    // Immediately start second
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
      memoryTarget: { url: "http://localhost:8230/add", userId: "ck" } as never,
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
});
