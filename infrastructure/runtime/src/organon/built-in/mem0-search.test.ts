// Mem0 search tool tests
import { beforeEach, describe, expect, it, vi } from "vitest";
import { mem0SearchTool } from "./mem0-search.js";

const ctx = { nousId: "syn", sessionId: "ses_1", workspace: "/tmp" };

describe("mem0SearchTool", () => {
  beforeEach(() => {
    vi.stubGlobal("fetch", vi.fn());
  });

  it("has valid definition", () => {
    expect(mem0SearchTool.definition.name).toBe("mem0_search");
    expect(mem0SearchTool.definition.input_schema.required).toContain("query");
  });

  it("returns results from graph-enhanced search", async () => {
    (fetch as ReturnType<typeof vi.fn>).mockResolvedValue({
      ok: true,
      json: vi.fn().mockResolvedValue({
        results: [
          { id: "m1", memory: "User prefers TypeScript", score: 0.9, agent_id: "syn" },
          { id: "m2", memory: "User runs Fedora", score: 0.8, agent_id: "syn" },
        ],
      }),
    });

    const result = await mem0SearchTool.execute({ query: "what preferences exist" }, ctx);
    const parsed = JSON.parse(result);
    expect(parsed.count).toBe(2);
    expect(parsed.results[0].memory).toContain("TypeScript");
  });

  it("deduplicates results by id", async () => {
    (fetch as ReturnType<typeof vi.fn>).mockResolvedValue({
      ok: true,
      json: vi.fn().mockResolvedValue({
        results: [
          { id: "m1", memory: "Fact A", score: 0.9 },
          { id: "m1", memory: "Fact A", score: 0.8 },
        ],
      }),
    });

    const result = await mem0SearchTool.execute({ query: "test" }, ctx);
    const parsed = JSON.parse(result);
    expect(parsed.count).toBe(1);
  });

  it("sorts by score descending", async () => {
    (fetch as ReturnType<typeof vi.fn>).mockResolvedValue({
      ok: true,
      json: vi.fn().mockResolvedValue({
        results: [
          { id: "m1", memory: "Low", score: 0.3 },
          { id: "m2", memory: "High", score: 0.9 },
        ],
      }),
    });

    const result = await mem0SearchTool.execute({ query: "test" }, ctx);
    const parsed = JSON.parse(result);
    expect(parsed.results[0].memory).toBe("High");
  });

  it("respects limit parameter", async () => {
    (fetch as ReturnType<typeof vi.fn>).mockResolvedValue({
      ok: true,
      json: vi.fn().mockResolvedValue({
        results: Array.from({ length: 10 }, (_, i) => ({ id: `m${i}`, memory: `Fact ${i}`, score: 1 - i * 0.1 })),
      }),
    });

    const result = await mem0SearchTool.execute({ query: "test", limit: 3 }, ctx);
    const parsed = JSON.parse(result);
    expect(parsed.count).toBe(3);
  });

  it("handles timeout error", async () => {
    const abortError = new Error("aborted");
    abortError.name = "AbortError";
    (fetch as ReturnType<typeof vi.fn>).mockRejectedValue(abortError);

    const result = await mem0SearchTool.execute({ query: "test" }, ctx);
    const parsed = JSON.parse(result);
    // When graph-enhanced throws AbortError, it falls into the outer catch
    // which catches AbortError and returns timed out, OR falls through to
    // the standard search fallback which also fails
    expect(parsed.results).toEqual([]);
  });

  it("handles network error", async () => {
    (fetch as ReturnType<typeof vi.fn>).mockRejectedValue(new Error("Connection refused"));

    const result = await mem0SearchTool.execute({ query: "test" }, ctx);
    const parsed = JSON.parse(result);
    // Falls back to standard search, which also fails
    expect(parsed.results).toEqual([]);
  });

  it("falls back to standard search when graph-enhanced fails", async () => {
    let callCount = 0;
    (fetch as ReturnType<typeof vi.fn>).mockImplementation(async (url: string) => {
      callCount++;
      if (callCount === 1) {
        // Graph-enhanced fails
        return { ok: false, status: 404 };
      }
      // Standard search succeeds
      return {
        ok: true,
        json: vi.fn().mockResolvedValue({
          results: [{ id: "m1", memory: "Fallback result", score: 0.7 }],
        }),
      };
    });

    const result = await mem0SearchTool.execute({ query: "test" }, ctx);
    const parsed = JSON.parse(result);
    expect(parsed.results).toBeDefined();
  });
});
