// Distillation summarization tests
import { describe, it, expect, vi } from "vitest";
import { summarizeMessages } from "./summarize.js";

function mockRouter(text: string) {
  return {
    complete: vi.fn().mockResolvedValue({
      content: [{ type: "text", text }],
      stopReason: "end_turn",
      usage: { inputTokens: 100, outputTokens: 50, cacheReadTokens: 0, cacheWriteTokens: 0 },
      model: "test",
    }),
  } as never;
}

const emptyExtraction = {
  facts: [], decisions: [], openItems: [], keyEntities: [], contradictions: [],
};

describe("summarizeMessages", () => {
  it("returns summary text from router", async () => {
    const router = mockRouter("This is a summary of the conversation.");
    const result = await summarizeMessages(
      router,
      [{ role: "user", content: "hello" }],
      emptyExtraction,
      "test-model",
    );
    expect(result).toBe("This is a summary of the conversation.");
  });

  it("includes extraction context when non-empty", async () => {
    const router = mockRouter("Summary with context.");
    await summarizeMessages(
      router,
      [{ role: "user", content: "hello" }],
      { ...emptyExtraction, facts: ["the sky is blue"] },
      "test-model",
    );
    const callArgs = (router.complete as ReturnType<typeof vi.fn>).mock.calls[0]![0];
    expect(JSON.stringify(callArgs)).toContain("the sky is blue");
  });

  it("passes nousId for agent context", async () => {
    const router = mockRouter("Agent-specific summary.");
    await summarizeMessages(
      router,
      [{ role: "user", content: "test" }],
      emptyExtraction,
      "test-model",
      "chiron",
    );
    const callArgs = (router.complete as ReturnType<typeof vi.fn>).mock.calls[0]![0];
    expect(JSON.stringify(callArgs)).toContain("chiron");
  });
});
