// Distillation extraction tests
import { describe, expect, it, vi } from "vitest";
import { extractFromMessages, extractJson, findBalancedBraces, repairJson } from "./extract.js";

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
    expect(result.openItems).toEqual([]);
  });
});

describe("findBalancedBraces", () => {
  it("finds balanced JSON object", () => {
    const result = findBalancedBraces('Some text {"key": "value"} more text');
    expect(result).toBe('{"key": "value"}');
  });

  it("handles nested braces", () => {
    const result = findBalancedBraces('{"a": {"b": 1}}');
    expect(result).toBe('{"a": {"b": 1}}');
  });

  it("handles braces inside strings", () => {
    const result = findBalancedBraces('{"text": "has { and } inside"}');
    expect(result).toBe('{"text": "has { and } inside"}');
  });

  it("handles escaped quotes in strings", () => {
    const result = findBalancedBraces('{"text": "has \\"quotes\\""}');
    expect(result).toBe('{"text": "has \\"quotes\\""}');
  });

  it("closes truncated JSON", () => {
    const result = findBalancedBraces('{"facts": ["a", "b"');
    expect(result).not.toBeNull();
    expect(() => JSON.parse(repairJson(result!))).not.toThrow();
  });

  it("returns null when no braces", () => {
    expect(findBalancedBraces("no json here")).toBeNull();
  });
});

describe("repairJson", () => {
  it("removes trailing commas before ]", () => {
    expect(repairJson('{"a": [1, 2, ]}')).toBe('{"a": [1, 2]}');
  });

  it("removes trailing commas before }", () => {
    expect(repairJson('{"a": 1, "b": 2, }')).toBe('{"a": 1, "b": 2}');
  });

  it("handles multiple trailing commas", () => {
    const repaired = repairJson('{"a": [1, ], "b": [2, ]}');
    expect(() => JSON.parse(repaired)).not.toThrow();
  });
});

describe("extractJson", () => {
  it("extracts clean JSON", () => {
    const result = extractJson('{"facts": ["hello"]}');
    expect(result).toEqual({ facts: ["hello"] });
  });

  it("extracts JSON with surrounding text", () => {
    const result = extractJson('Here is the result:\n{"facts": ["test"]}\nDone.');
    expect(result).toEqual({ facts: ["test"] });
  });

  it("repairs trailing commas", () => {
    const result = extractJson('{"facts": ["a", "b", ], "decisions": []}');
    expect(result).not.toBeNull();
    expect(result!["facts"]).toEqual(["a", "b"]);
  });

  it("handles truncated output", () => {
    const result = extractJson('{"facts": ["a", "b"], "decisions": ["d1"');
    expect(result).not.toBeNull();
    expect(result!["facts"]).toEqual(["a", "b"]);
  });

  it("returns null for no JSON", () => {
    expect(extractJson("no json content at all")).toBeNull();
  });
});
