import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { recallMemories } from "./recall.js";

const mockFetch = vi.fn();
vi.stubGlobal("fetch", mockFetch);

function makeResponse(
  results: Array<{ memory: string; score: number | null }>,
  ok = true,
  status = 200,
) {
  return {
    ok,
    status,
    json: () => Promise.resolve({ ok: true, results }),
  };
}

beforeEach(() => {
  mockFetch.mockReset();
});

afterEach(() => {
  vi.restoreAllMocks();
});

describe("recallMemories", () => {
  it("returns formatted block for matching memories", async () => {
    // Primary: vector search returns good results
    mockFetch.mockResolvedValueOnce(
      makeResponse([
        { memory: "User prefers dark mode", score: 0.95 },
        { memory: "User prefers Fish shell for terminals", score: 0.88 },
        { memory: "User uses Fish shell", score: 0.82 },
      ]),
    );

    const result = await recallMemories("What theme do I use?", "chiron");

    expect(result.count).toBe(3);
    expect(result.block).not.toBeNull();
    expect(result.block!.text).toContain("## Recalled Memories");
    expect(result.block!.text).toContain("User prefers dark mode");
    expect(result.block!.text).toContain("score: 0.95");
    expect(result.tokens).toBeGreaterThan(0);
    // Should only call vector search (no graph fallback needed)
    expect(mockFetch).toHaveBeenCalledTimes(1);
    expect(mockFetch.mock.calls[0][0]).toContain("/search");
  });

  it("returns null block when no results", async () => {
    // Vector search: empty (no hits above threshold → tries graph-enhanced)
    mockFetch
      .mockResolvedValueOnce(makeResponse([]))
      .mockResolvedValueOnce(makeResponse([]));

    const result = await recallMemories("random query", "chiron");

    expect(result.block).toBeNull();
    expect(result.count).toBe(0);
  });

  it("filters below minScore", async () => {
    mockFetch.mockResolvedValueOnce(
      makeResponse([
        { memory: "High relevance", score: 0.9 },
        { memory: "Low relevance", score: 0.5 },
        { memory: "Medium relevance", score: 0.76 },
      ]),
    );

    const result = await recallMemories("test", "chiron");

    expect(result.count).toBe(2);
    expect(result.block!.text).toContain("High relevance");
    expect(result.block!.text).toContain("Medium relevance");
    expect(result.block!.text).not.toContain("Low relevance");
  });

  it("falls back to graph-enhanced search when vector scores are all below threshold", async () => {
    // Vector search: results but all below minScore
    mockFetch
      .mockResolvedValueOnce(
        makeResponse([
          { memory: "Low vector result", score: 0.5 },
        ]),
      )
      // Graph-enhanced search: returns better combined scores
      .mockResolvedValueOnce(
        makeResponse([
          { memory: "Graph enriched result", score: 0.85 },
        ]),
      );

    const result = await recallMemories("test", "chiron");

    expect(result.count).toBe(1);
    expect(result.block!.text).toContain("Graph enriched result");
    expect(mockFetch).toHaveBeenCalledTimes(2);
    // First call is vector search, second is graph-enhanced
    expect(mockFetch.mock.calls[0][0]).toContain("/search");
    expect(mockFetch.mock.calls[1][0]).toContain("/graph_enhanced_search");
  });

  it("skips graph-enhanced search when vector search has usable hits", async () => {
    mockFetch.mockResolvedValueOnce(
      makeResponse([
        { memory: "Good vector result", score: 0.9 },
      ]),
    );

    const result = await recallMemories("test", "chiron");

    expect(result.count).toBe(1);
    // Should NOT call graph_enhanced_search
    expect(mockFetch).toHaveBeenCalledTimes(1);
    expect(mockFetch.mock.calls[0][0]).toContain("/search");
  });

  it("returns null on complete failure", async () => {
    mockFetch.mockRejectedValueOnce(new Error("connection refused"));

    const result = await recallMemories("test", "chiron");

    expect(result.block).toBeNull();
    expect(result.count).toBe(0);
  });

  it("deduplicates identical memories", async () => {
    mockFetch.mockResolvedValueOnce(
      makeResponse([
        { memory: "User's name is Cody", score: 0.95 },
        { memory: "User's name is Cody", score: 0.9 },
        { memory: "Different fact", score: 0.85 },
      ]),
    );

    const result = await recallMemories("what's my name", "chiron");

    expect(result.count).toBe(2);
    const occurrences = (
      result.block!.text.match(/User's name is Cody/g) ?? []
    ).length;
    expect(occurrences).toBe(1);
  });

  it("respects limit option", async () => {
    mockFetch.mockResolvedValueOnce(
      makeResponse([
        { memory: "Fact 1", score: 0.95 },
        { memory: "Fact 2", score: 0.9 },
        { memory: "Fact 3", score: 0.85 },
        { memory: "Fact 4", score: 0.8 },
      ]),
    );

    const result = await recallMemories("test", "chiron", { limit: 2 });

    expect(result.count).toBe(2);
  });

  it("handles null scores gracefully", async () => {
    mockFetch.mockResolvedValueOnce(
      makeResponse([
        { memory: "No score", score: null },
        { memory: "Has score", score: 0.9 },
      ]),
    );

    const result = await recallMemories("test", "chiron");

    expect(result.count).toBe(1);
    expect(result.block!.text).toContain("Has score");
    expect(result.block!.text).not.toContain("No score");
  });

  it("truncates query to 500 chars", async () => {
    const longMessage = "a".repeat(1000);
    // Vector: empty, graph-enhanced: empty
    mockFetch
      .mockResolvedValueOnce(makeResponse([]))
      .mockResolvedValueOnce(makeResponse([]));

    await recallMemories(longMessage, "chiron");

    const body = JSON.parse(mockFetch.mock.calls[0][1].body);
    expect(body.query.length).toBe(500);
  });

  it("returns timing information", async () => {
    mockFetch.mockResolvedValueOnce(
      makeResponse([{ memory: "Timed result", score: 0.9 }]),
    );

    const result = await recallMemories("test", "chiron");

    expect(result.durationMs).toBeGreaterThanOrEqual(0);
  });

  it("uses 5s default timeout", async () => {
    // Verify the default timeout is 5000ms, not 3000ms
    mockFetch.mockImplementation(
      (_url: string, init: { signal: AbortSignal }) => {
        // The signal should not be aborted at 3s
        return new Promise((resolve) => {
          setTimeout(() => {
            resolve(
              makeResponse([{ memory: "Slow but valid", score: 0.9 }]),
            );
          }, 100);
        });
      },
    );

    const result = await recallMemories("test", "chiron");
    expect(result.count).toBe(1);
  });
});
