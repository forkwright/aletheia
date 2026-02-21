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
      facts: ["Pitman arm torque spec is 185 ft-lbs per service manual"],
      decisions: ["Decision: use chrome-tanned leather for belt due to durability"],
      openItems: ["Need to order steering box rebuild kit before Saturday"],
      keyEntities: ["ent1"],
      contradictions: [],
    });
    const router = mockRouter(`Here is the extraction:\n${json}`);
    const result = await extractFromMessages(router, [
      { role: "user", content: "hello" },
    ], "test-model");

    expect(result.facts).toEqual(["Pitman arm torque spec is 185 ft-lbs per service manual"]);
    expect(result.decisions).toEqual(["Decision: use chrome-tanned leather for belt due to durability"]);
    expect(result.openItems).toEqual(["Need to order steering box rebuild kit before Saturday"]);
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
    const router = mockRouter('{"facts": ["ALETHEIA_MEMORY_USER must be set in aletheia.env"], "decisions": []}');
    const result = await extractFromMessages(router, [
      { role: "user", content: "test" },
    ], "test-model");

    expect(result.facts).toEqual(["ALETHEIA_MEMORY_USER must be set in aletheia.env"]);
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

describe("noise filtering", () => {
  it("filters generic patterns from extraction output", async () => {
    const json = JSON.stringify({
      facts: [
        "Uses grep for searching",
        "Familiar with TypeScript and React patterns",
        "Baby #2 due October 2026",
        "Works with a system called Aletheia",
        "The user asked about deployment",
        "Ran git status to check repository state",
        "Prosoche dedup window set to 8 hours to reduce alert fatigue",
      ],
      decisions: [
        "Decision: migrate to Qdrant for vector search due to filtering support",
        "Discussed options for the leather project",
      ],
    });
    const router = mockRouter(json);
    const result = await extractFromMessages(router, [
      { role: "user", content: "hello" },
    ], "test-model");

    // Should keep: Baby #2, Prosoche, Qdrant migration
    // Should filter: Uses grep, Familiar with, Works with, The user asked, Ran git, Discussed
    expect(result.facts).toEqual([
      "Baby #2 due October 2026",
      "Prosoche dedup window set to 8 hours to reduce alert fatigue",
    ]);
    expect(result.decisions).toEqual([
      "Decision: migrate to Qdrant for vector search due to filtering support",
    ]);
  });

  it("filters items that are too short or too long", async () => {
    const json = JSON.stringify({
      facts: [
        "short",
        "x".repeat(301),
        "This is a normal-length fact that should be kept",
      ],
      decisions: [],
    });
    const router = mockRouter(json);
    const result = await extractFromMessages(router, [
      { role: "user", content: "hello" },
    ], "test-model");

    expect(result.facts).toEqual(["This is a normal-length fact that should be kept"]);
  });
});
