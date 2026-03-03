// Distillation extraction tests
import { describe, expect, it, vi } from "vitest";
import { deduplicateFactsViaSidecar, extractFromMessages, extractJson, findBalancedBraces, repairJson } from "./extract.js";

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
      facts: ["Widget torque spec is 42 Nm per service manual"],
      decisions: ["Decision: use high-grade polymer for bracket due to durability"],
      openItems: ["Need to order motor mount rebuild kit before Saturday"],
      keyEntities: ["ent1"],
      contradictions: [],
    });
    const router = mockRouter(`Here is the extraction:\n${json}`);
    const result = await extractFromMessages(router, [
      { role: "user", content: "hello" },
    ], "test-model");

    expect(result.facts).toEqual(["Widget torque spec is 42 Nm per service manual"]);
    expect(result.decisions).toEqual(["Decision: use high-grade polymer for bracket due to durability"]);
    expect(result.openItems).toEqual(["Need to order motor mount rebuild kit before Saturday"]);
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

describe("object element filtering", () => {
  it("filters non-string elements from LLM arrays", async () => {
    const json = JSON.stringify({
      facts: [
        "Valid fact about deployment process",
        { text: "Structured but wrong format", confidence: 0.9 },
        42,
        "Another valid fact about config changes",
      ],
      decisions: [{ decision: "use Qdrant" }],
      openItems: ["Need to verify integration test results pass"],
      keyEntities: ["Qdrant", { name: "Neo4j" }],
      contradictions: [],
    });
    const router = mockRouter(json);
    const result = await extractFromMessages(router, [
      { role: "user", content: "hello" },
    ], "test-model");

    expect(result.facts).toEqual([
      "Valid fact about deployment process",
      "Another valid fact about config changes",
    ]);
    expect(result.decisions).toEqual([]);
    expect(result.openItems).toEqual(["Need to verify integration test results pass"]);
    expect(result.keyEntities).toEqual(["Qdrant"]);
  });
});

describe("noise filtering", () => {
  it("filters generic patterns from extraction output", async () => {
    const json = JSON.stringify({
      facts: [
        "Uses grep for searching",
        "Familiar with TypeScript and React patterns",
        "Project Alpha deadline is March 2026",
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

    // Should keep: Project Alpha deadline, Prosoche, Qdrant migration
    // Should filter: Uses grep, Familiar with, Works with, The user asked, Ran git, Discussed
    expect(result.facts).toEqual([
      "Project Alpha deadline is March 2026",
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

  it("filters session and system artifact patterns", async () => {
    const json = JSON.stringify({
      facts: [
        "Session id: abc123-session-started",
        "The conversation started at session created time",
        "Project Alpha deadline is March 2026",
      ],
      decisions: [],
    });
    const router = mockRouter(json);
    const result = await extractFromMessages(router, [{ role: "user", content: "hello" }], "test-model");

    expect(result.facts).toEqual(["Project Alpha deadline is March 2026"]);
  });

  it("filters meta-commentary about the conversation", async () => {
    const json = JSON.stringify({
      facts: [
        "The user mentioned the API is broken",
        "The agent indicated the config path is wrong",
        "The assistant told the user to restart the service",
        "ALETHEIA_MEMORY_USER must be set in aletheia.env or all extractions default to user_id='default'",
      ],
      decisions: [],
    });
    const router = mockRouter(json);
    const result = await extractFromMessages(router, [{ role: "user", content: "hello" }], "test-model");

    expect(result.facts).toEqual([
      "ALETHEIA_MEMORY_USER must be set in aletheia.env or all extractions default to user_id='default'",
    ]);
  });

  it("filters tool/function invocation facts", async () => {
    const json = JSON.stringify({
      facts: [
        "Called tool grep to search for imports",
        "Invoked function deployService with args prod",
        "Executed command npm run build successfully",
        "Widget torque spec is 42 Nm per service manual",
      ],
      decisions: [],
    });
    const router = mockRouter(json);
    const result = await extractFromMessages(router, [{ role: "user", content: "hello" }], "test-model");

    expect(result.facts).toEqual(["Widget torque spec is 42 Nm per service manual"]);
  });

  it("filters acknowledgment phrases", async () => {
    const json = JSON.stringify({
      facts: [
        "Sure, I will help with that",
        "OK, understood the requirements",
        "Sounds good, proceeding with the plan",
        "Got it, will do that right away",
        "Prosoche dedup window set to 8 hours to reduce alert fatigue",
      ],
      decisions: [],
    });
    const router = mockRouter(json);
    const result = await extractFromMessages(router, [{ role: "user", content: "hello" }], "test-model");

    expect(result.facts).toEqual(["Prosoche dedup window set to 8 hours to reduce alert fatigue"]);
  });

  it("filters file path operation artifacts", async () => {
    const json = JSON.stringify({
      facts: [
        "Reading file config.json to load settings",
        "Writing path /etc/aletheia to disk",
        "Opening directory /inst/config",
        "Widget torque spec is 42 Nm per service manual",
      ],
      decisions: [],
    });
    const router = mockRouter(json);
    const result = await extractFromMessages(router, [{ role: "user", content: "hello" }], "test-model");

    expect(result.facts).toEqual(["Widget torque spec is 42 Nm per service manual"]);
  });

  it("filters timestamp-only facts with no content", async () => {
    const json = JSON.stringify({
      facts: [
        "On 3:45 we discussed the project",
        "At 14:00 the meeting occurred",
        "Final project due March 15, needs 3 weeks of work",
      ],
      decisions: [],
    });
    const router = mockRouter(json);
    const result = await extractFromMessages(router, [{ role: "user", content: "hello" }], "test-model");

    expect(result.facts).toEqual(["Final project due March 15, needs 3 weeks of work"]);
  });

  it("passes legitimate short facts above the minimum length threshold", async () => {
    const json = JSON.stringify({
      facts: [
        "Uses Vim",
        "Project Alpha deadline is March 2026",
      ],
      decisions: [],
    });
    const router = mockRouter(json);
    const result = await extractFromMessages(router, [{ role: "user", content: "hello" }], "test-model");

    // "Uses Vim" is filtered by the Uses pattern AND is under 15 chars
    // "Project Alpha deadline is March 2026" is 36 chars and doesn't match noise patterns
    expect(result.facts).toEqual(["Project Alpha deadline is March 2026"]);
  });
});

describe("deduplicateFactsViaSidecar", () => {
  it("returns original for single fact without calling fetch", async () => {
    const fetchSpy = vi.spyOn(globalThis, "fetch");
    const result = await deduplicateFactsViaSidecar(
      ["Only one fact here — no dedup needed"],
      "http://localhost:8230",
    );
    expect(result).toEqual(["Only one fact here — no dedup needed"]);
    expect(fetchSpy).not.toHaveBeenCalled();
    fetchSpy.mockRestore();
  });

  it("returns original for empty array without calling fetch", async () => {
    const fetchSpy = vi.spyOn(globalThis, "fetch");
    const result = await deduplicateFactsViaSidecar([], "http://localhost:8230");
    expect(result).toEqual([]);
    expect(fetchSpy).not.toHaveBeenCalled();
    fetchSpy.mockRestore();
  });

  it("calls sidecar and returns deduplicated facts", async () => {
    const facts = [
      "User prefers high-grade polymer for brackets",
      "User strongly prefers high-grade polymer for brackets",
      "Project Alpha deadline is March 2026",
    ];
    const deduped = [facts[0]!, facts[2]!];

    const fetchSpy = vi.spyOn(globalThis, "fetch").mockResolvedValueOnce(
      new Response(JSON.stringify({ deduplicated: deduped, removed: 1 }), {
        status: 200,
        headers: { "Content-Type": "application/json" },
      }),
    );

    const result = await deduplicateFactsViaSidecar(facts, "http://localhost:8230");

    expect(fetchSpy).toHaveBeenCalledOnce();
    const [url, init] = fetchSpy.mock.calls[0]!;
    expect(url).toBe("http://localhost:8230/dedup/batch");
    expect(JSON.parse((init as RequestInit).body as string)).toMatchObject({
      texts: facts,
      threshold: 0.90,
    });
    expect(result).toEqual(deduped);

    fetchSpy.mockRestore();
  });

  it("falls back to original facts on fetch error (fail-open)", async () => {
    const facts = [
      "Widget torque spec is 42 Nm per service manual",
      "Project Alpha deadline is March 2026",
    ];

    const fetchSpy = vi.spyOn(globalThis, "fetch").mockRejectedValueOnce(
      new Error("ECONNREFUSED"),
    );

    const result = await deduplicateFactsViaSidecar(facts, "http://localhost:8230");

    expect(result).toEqual(facts);
    fetchSpy.mockRestore();
  });

  it("falls back to original facts when sidecar returns non-200 status", async () => {
    const facts = [
      "Widget torque spec is 42 Nm per service manual",
      "Project Alpha deadline is March 2026",
    ];

    const fetchSpy = vi.spyOn(globalThis, "fetch").mockResolvedValueOnce(
      new Response("Service Unavailable", { status: 503 }),
    );

    const result = await deduplicateFactsViaSidecar(facts, "http://localhost:8230");

    expect(result).toEqual(facts);
    fetchSpy.mockRestore();
  });
});
