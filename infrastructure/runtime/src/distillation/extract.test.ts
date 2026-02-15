// Distillation extraction tests
import { describe, it, expect, vi } from "vitest";
import { extractFromMessages } from "./extract.js";

function mockRouter(responseText: string) {
  return {
    complete: vi.fn().mockResolvedValue({
      content: [{ type: "text", text: responseText }],
      stopReason: "end_turn",
      usage: { inputTokens: 100, outputTokens: 50, cacheReadTokens: 0, cacheWriteTokens: 0 },
      model: "test",
    }),
  } as never;
}

describe("extractFromMessages", () => {
  it("parses JSON from LLM response", async () => {
    const json = JSON.stringify({
      facts: ["fact1"],
      decisions: ["dec1"],
      openItems: ["open1"],
      keyEntities: ["ent1"],
      contradictions: [],
    });
    const router = mockRouter(`Here is the extraction:\n${json}`);
    const result = await extractFromMessages(router, [
      { role: "user", content: "hello" },
    ], "test-model");

    expect(result.facts).toEqual(["fact1"]);
    expect(result.decisions).toEqual(["dec1"]);
    expect(result.openItems).toEqual(["open1"]);
    expect(result.keyEntities).toEqual(["ent1"]);
    expect(result.contradictions).toEqual([]);
  });

  it("returns empty arrays on malformed response", async () => {
    const router = mockRouter("I couldn't parse that, sorry.");
    const result = await extractFromMessages(router, [
      { role: "user", content: "hello" },
    ], "test-model");

    expect(result.facts).toEqual([]);
    expect(result.decisions).toEqual([]);
    expect(result.openItems).toEqual([]);
  });

  it("handles partial JSON (missing fields)", async () => {
    const router = mockRouter('{"facts": ["a"], "decisions": []}');
    const result = await extractFromMessages(router, [
      { role: "user", content: "test" },
    ], "test-model");

    expect(result.facts).toEqual(["a"]);
    expect(result.decisions).toEqual([]);
    // Missing fields should default to empty arrays
    expect(result.openItems).toEqual([]);
  });
});
