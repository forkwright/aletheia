// Cross-chunk contradiction detection tests
import { describe, expect, it, vi } from "vitest";
import { detectCrossChunkContradictions } from "./contradiction-detect.js";

function makeRouter(responseText: string) {
  return {
    complete: vi.fn().mockResolvedValue({
      content: [{ type: "text", text: responseText }],
      usage: { inputTokens: 100, outputTokens: 50, cacheReadTokens: 0, cacheWriteTokens: 0 },
      model: "claude-haiku",
    }),
  } as never;
}

describe("detectCrossChunkContradictions", () => {
  it("returns empty array for fewer than 2 facts — no router call made", async () => {
    const router = makeRouter('{"contradictions": []}');
    const result = await detectCrossChunkContradictions(router, ["single fact"], "claude-haiku");
    expect(result).toEqual([]);
    expect(router.complete).not.toHaveBeenCalled();
  });

  it("returns empty array for zero facts", async () => {
    const router = makeRouter('{"contradictions": []}');
    const result = await detectCrossChunkContradictions(router, [], "claude-haiku");
    expect(result).toEqual([]);
    expect(router.complete).not.toHaveBeenCalled();
  });

  it("detects contradictions from LLM response", async () => {
    const router = makeRouter(
      '{"contradictions": ["User prefers coffee (fact 1) contradicts user dislikes hot beverages (fact 2)"]}',
    );
    const result = await detectCrossChunkContradictions(
      router,
      ["User prefers coffee", "User dislikes all hot beverages"],
      "claude-haiku",
    );
    expect(result).toHaveLength(1);
    expect(result[0]).toContain("contradicts");
    expect(router.complete).toHaveBeenCalledOnce();
  });

  it("returns empty array when LLM finds no contradictions", async () => {
    const router = makeRouter('{"contradictions": []}');
    const result = await detectCrossChunkContradictions(
      router,
      ["User has a dog", "Server runs on port 8080"],
      "claude-haiku",
    );
    expect(result).toEqual([]);
    expect(router.complete).toHaveBeenCalledOnce();
  });

  it("returns empty array on malformed LLM response", async () => {
    const router = makeRouter("I found some contradictions but cannot format them properly");
    const result = await detectCrossChunkContradictions(
      router,
      ["fact one", "fact two"],
      "claude-haiku",
    );
    expect(result).toEqual([]);
  });

  it("returns empty array on router error — no throw propagation", async () => {
    const router = {
      complete: vi.fn().mockRejectedValue(new Error("LLM provider down")),
    } as never;
    const result = await detectCrossChunkContradictions(
      router,
      ["fact one", "fact two"],
      "claude-haiku",
    );
    expect(result).toEqual([]);
  });

  it("handles LLM response wrapped in markdown code fences", async () => {
    const router = makeRouter(
      '```json\n{"contradictions": ["Meeting on Monday (fact 1) contradicts meeting cancelled (fact 2)"]}\n```',
    );
    const result = await detectCrossChunkContradictions(
      router,
      ["Meeting scheduled for Monday", "Meeting was cancelled"],
      "claude-haiku",
    );
    expect(result).toHaveLength(1);
  });

  it("calls router with temperature 0 for deterministic output", async () => {
    const router = makeRouter('{"contradictions": []}');
    await detectCrossChunkContradictions(
      router,
      ["fact one", "fact two"],
      "claude-haiku",
    );
    expect(router.complete).toHaveBeenCalledWith(
      expect.objectContaining({ temperature: 0, maxTokens: 1024 }),
    );
  });
});
