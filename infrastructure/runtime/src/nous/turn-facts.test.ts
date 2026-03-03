import { describe, expect, it, vi } from "vitest";
import { extractTurnFacts } from "./turn-facts.js";

function mockRouter(response: string) {
  return {
    complete: vi.fn().mockResolvedValue({
      content: [{ type: "text", text: response }],
      usage: { inputTokens: 100, outputTokens: 50 },
    }),
    stream: vi.fn(),
    // oxlint-disable-next-line typescript/no-explicit-any -- partial mock router, type narrowing not practical
  } as any;
}

describe("extractTurnFacts", () => {
  it("extracts facts from a substantive response", async () => {
    const router = mockRouter('["Widget torque spec is 42 Nm", "Decision: use high-grade polymer for case project"]');
    const result = await extractTurnFacts(
      router,
      "Based on the manual, the widget torque spec is 42 Nm. I also decided to go with high-grade polymer for the case project because of its durability advantages over standard composite for this particular use case. The tensile strength holds up better under daily wear.",
      "",
      "test-model",
    );
    expect(result.facts).toHaveLength(2);
    expect(result.facts[0]).toContain("42 Nm");
    expect(result.facts[1]).toContain("high-grade polymer");
  });

  it("returns empty for short responses", async () => {
    const router = mockRouter('["should not be called"]');
    const result = await extractTurnFacts(router, "Done.", "", "test-model");
    expect(result.facts).toEqual([]);
    expect(router.complete).not.toHaveBeenCalled();
  });

  it("filters noise patterns", async () => {
    const router = mockRouter(JSON.stringify([
      "Uses grep for searching",
      "Familiar with TypeScript",
      "Decision: migrate to Qdrant for vector search because it supports filtering and is lighter than Milvus",
      "The user asked about configuration",
    ]));
    const result = await extractTurnFacts(
      router,
      "x".repeat(200), // Long enough to trigger extraction
      "",
      "test-model",
    );
    // Only the decision should survive noise filtering
    expect(result.facts).toHaveLength(1);
    expect(result.facts[0]).toContain("Qdrant");
  });

  it("filters facts that are too short", async () => {
    const router = mockRouter('["ok", "yes", "Alice uses Aletheia for distributed cognition across 6 agents"]');
    const result = await extractTurnFacts(router, "x".repeat(200), "", "test-model");
    expect(result.facts).toHaveLength(1);
    expect(result.facts[0]).toContain("distributed cognition");
  });

  it("caps at 3 facts", async () => {
    const router = mockRouter(JSON.stringify([
      "Fact one about architecture decisions",
      "Fact two about project planning approach",
      "Fact three about personal preference",
      "Fact four about tool usage patterns",
      "Fact five about deployment strategy",
    ]));
    const result = await extractTurnFacts(router, "x".repeat(200), "", "test-model");
    expect(result.facts.length).toBeLessThanOrEqual(3);
  });

  it("handles markdown-fenced JSON", async () => {
    const router = mockRouter('```json\n["Memory pipeline was completely broken — memoryTarget never wired"]\n```');
    const result = await extractTurnFacts(router, "x".repeat(200), "", "test-model");
    expect(result.facts).toHaveLength(1);
    expect(result.facts[0]).toContain("memoryTarget");
  });

  it("handles malformed output gracefully", async () => {
    const router = mockRouter("I couldn't extract any facts from this.");
    const result = await extractTurnFacts(router, "x".repeat(200), "", "test-model");
    expect(result.facts).toEqual([]);
  });

  it("handles router failure gracefully", async () => {
    // oxlint-disable-next-line typescript/no-explicit-any -- partial mock router
    const router = { complete: vi.fn().mockRejectedValue(new Error("timeout")) } as any;
    const result = await extractTurnFacts(router, "x".repeat(200), "", "test-model");
    expect(result.facts).toEqual([]);
  });

  it("includes tool summary in extraction context", async () => {
    const router = mockRouter('["Motor mount replacement requires 42 Nm torque on the bracket"]');
    await extractTurnFacts(
      router,
      "x".repeat(200),
      'exec(grep "widget" manual.txt) → Widget: 42 Nm',
      "test-model",
    );
    // Verify tool summary was passed to the router
    const callArgs = router.complete.mock.calls[0][0];
    expect(callArgs.messages[0].content).toContain("Tool Results");
    expect(callArgs.messages[0].content).toContain("widget");
  });
});
