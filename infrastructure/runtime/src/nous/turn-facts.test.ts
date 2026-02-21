import { describe, it, expect, vi } from "vitest";
import { extractTurnFacts } from "./turn-facts.js";

function mockRouter(response: string) {
  return {
    complete: vi.fn().mockResolvedValue({
      content: [{ type: "text", text: response }],
      usage: { inputTokens: 100, outputTokens: 50 },
    }),
    stream: vi.fn(),
  } as any;
}

describe("extractTurnFacts", () => {
  it("extracts facts from a substantive response", async () => {
    const router = mockRouter('["Pitman arm torque spec is 185 ft-lbs", "Decision: use chrome-tanned leather for belt project"]');
    const result = await extractTurnFacts(
      router,
      "Based on the manual, the pitman arm torque spec is 185 ft-lbs. I also decided to go with chrome-tanned leather for the belt project because of its durability advantages over veg-tan for this particular use case. The grain structure holds up better under daily wear.",
      "",
      "test-model",
    );
    expect(result.facts).toHaveLength(2);
    expect(result.facts[0]).toContain("185 ft-lbs");
    expect(result.facts[1]).toContain("chrome-tanned");
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
    const router = mockRouter('["ok", "yes", "Cody uses Aletheia for distributed cognition across 6 agents"]');
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
    const router = { complete: vi.fn().mockRejectedValue(new Error("timeout")) } as any;
    const result = await extractTurnFacts(router, "x".repeat(200), "", "test-model");
    expect(result.facts).toEqual([]);
  });

  it("includes tool summary in extraction context", async () => {
    const router = mockRouter('["Steering box replacement requires 185 ft-lbs torque on the pitman arm"]');
    const result = await extractTurnFacts(
      router,
      "x".repeat(200),
      'exec(grep "pitman" manual.txt) → Pitman arm: 185 ft-lbs',
      "test-model",
    );
    // Verify tool summary was passed to the router
    const callArgs = router.complete.mock.calls[0][0];
    expect(callArgs.messages[0].content).toContain("Tool Results");
    expect(callArgs.messages[0].content).toContain("pitman");
  });
});
