// Chunked summarization tests
import { describe, it, expect, vi } from "vitest";
import { sanitizeToolResults, summarizeInStages } from "./chunked-summarize.js";

describe("sanitizeToolResults", () => {
  it("passes through normal messages", () => {
    const msgs = [
      { role: "user", content: "hello" },
      { role: "assistant", content: "hi" },
    ];
    const result = sanitizeToolResults(msgs);
    expect(result).toEqual(msgs);
  });

  it("truncates long tool-prefixed messages", () => {
    const msgs = [
      { role: "tool_result", content: "[tool:bash] " + "x".repeat(10000) },
    ];
    const result = sanitizeToolResults(msgs);
    expect(result[0]!.content.length).toBeLessThan(10000);
    expect(result[0]!.content).toContain("truncated");
  });

  it("truncates [tool_result] prefixed messages", () => {
    const msgs = [
      { role: "tool_result", content: "[tool_result]" + "x".repeat(10000) },
    ];
    const result = sanitizeToolResults(msgs);
    expect(result[0]!.content.length).toBeLessThan(10100);
    expect(result[0]!.content).toContain("truncated");
  });

  it("keeps short tool_result messages intact", () => {
    const msgs = [
      { role: "tool_result", content: "short result" },
    ];
    const result = sanitizeToolResults(msgs);
    expect(result[0]!.content).toBe("short result");
  });
});

describe("summarizeInStages", () => {
  it("handles single-pass for small conversations", async () => {
    const router = {
      complete: vi.fn().mockResolvedValue({
        content: [{ type: "text", text: "Summary of conversation." }],
        stopReason: "end_turn",
        usage: { inputTokens: 10, outputTokens: 5, cacheReadTokens: 0, cacheWriteTokens: 0 },
        model: "test",
      }),
    } as never;

    const result = await summarizeInStages(
      router,
      [{ role: "user", content: "hello" }, { role: "assistant", content: "hi" }],
      { facts: [], decisions: [], openItems: [], keyEntities: [], contradictions: [] },
      "test-model",
    );
    expect(result).toBe("Summary of conversation.");
  });

  it("handles multi-stage for large conversations", async () => {
    let callCount = 0;
    const router = {
      complete: vi.fn().mockImplementation(async () => {
        callCount++;
        // First N calls are chunk summarizations (via summarizeMessages),
        // last call is the merge
        return {
          content: [{ type: "text", text: callCount <= 3 ? `Chunk ${callCount} summary.` : "Merged final summary." }],
          stopReason: "end_turn",
          usage: { inputTokens: 10, outputTokens: 5, cacheReadTokens: 0, cacheWriteTokens: 0 },
          model: "test",
        };
      }),
    } as never;

    // Generate enough messages to trigger splitting
    const msgs = Array.from({ length: 100 }, (_, i) => ({
      role: i % 2 === 0 ? "user" : "assistant",
      content: "x".repeat(500),
    }));

    const result = await summarizeInStages(
      router,
      msgs,
      { facts: [], decisions: [], openItems: [], keyEntities: [], contradictions: [] },
      "test-model",
      undefined,
      { maxChunkTokens: 500, minMessagesForSplit: 10 },
    );
    // Should have been called multiple times (chunks + merge)
    expect((router as { complete: ReturnType<typeof vi.fn> }).complete.mock.calls.length).toBeGreaterThan(2);
    expect(result).toBe("Merged final summary.");
  });
});
