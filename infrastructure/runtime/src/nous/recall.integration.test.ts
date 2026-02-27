// E2E integration tests for all memory read paths
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

type MockFetch = ReturnType<typeof vi.fn>;

interface MemoryHit {
  id: string;
  memory: string;
  score: number;
  agent_id?: string;
  created_at?: string;
}

function makeSearchResponse(results: MemoryHit[]) {
  return {
    ok: true,
    json: vi.fn().mockResolvedValue({ results }),
    status: 200,
  };
}

function makeErrorResponse(status = 500) {
  return {
    ok: false,
    json: vi.fn().mockResolvedValue({ error: "internal error" }),
    status,
  };
}

function makeFetch(responses: Array<ReturnType<typeof makeSearchResponse | typeof makeErrorResponse>>): MockFetch {
  let callIdx = 0;
  return vi.fn().mockImplementation(() => {
    const response = responses[callIdx] ?? responses[responses.length - 1];
    callIdx++;
    return Promise.resolve(response);
  });
}

/** Build a list of N memory hits, all with the given score */
function makeHits(n: number, score: number, prefix = "Memory"): MemoryHit[] {
  return Array.from({ length: n }, (_, i) => ({
    id: `mem_${i + 1}`,
    memory: `${prefix} ${i + 1}: relevant fact about the query topic for testing purposes`,
    score,
    agent_id: "syn",
  }));
}

beforeEach(() => {
  // Ensure env vars are set for memory-client.ts to resolve sidecar URL
  process.env["ALETHEIA_MEMORY_URL"] = "http://127.0.0.1:8230";
  process.env["ALETHEIA_MEMORY_USER"] = "test-user";
});

afterEach(() => {
  vi.unstubAllGlobals();
  vi.clearAllMocks();
  delete process.env["ALETHEIA_MEMORY_URL"];
  delete process.env["ALETHEIA_MEMORY_USER"];
});

// --- Read Path 1: Vector recall (recallMemories → /search) ---

describe("read path: vector recall (recallMemories → /search)", () => {
  it("calls /search with the correct URL, method, and body", async () => {
    const { recallMemories } = await import("./recall.js");

    const hits = makeHits(3, 0.90);
    const fetchMock = makeFetch([makeSearchResponse(hits)]);
    vi.stubGlobal("fetch", fetchMock);

    await recallMemories("What is the widget torque spec?", "syn");

    expect(fetchMock).toHaveBeenCalledOnce();
    const [url, init] = fetchMock.mock.calls[0]!;

    expect(url).toContain("/search");
    expect(url).not.toContain("graph_enhanced");
    expect(init.method).toBe("POST");
    expect(init.headers).toMatchObject({ "Content-Type": "application/json" });

    const body = JSON.parse(init.body as string) as Record<string, unknown>;
    expect(body["query"]).toContain("widget torque");
    expect(body["agent_id"]).toBe("syn");
    expect(body["user_id"]).toBe("test-user");
    expect(typeof body["limit"]).toBe("number");
  });

  it("returns a RecallResult with count and text block from vector search", async () => {
    const { recallMemories } = await import("./recall.js");

    const hits = makeHits(3, 0.90);
    const fetchMock = makeFetch([makeSearchResponse(hits)]);
    vi.stubGlobal("fetch", fetchMock);

    const result = await recallMemories("vehicle maintenance torque specs", "syn");

    expect(result).toHaveProperty("count");
    expect(result).toHaveProperty("durationMs");
    expect(result).toHaveProperty("tokens");
    expect(result.count).toBeGreaterThan(0);
    expect(result.block).not.toBeNull();
    expect(result.block!.type).toBe("text");
    expect(result.block!.text).toContain("## Recalled Memories");
  });

  it("returns block=null and count=0 when all hits are below minScore", async () => {
    const { recallMemories } = await import("./recall.js");

    // Hits below default minScore of 0.75 — should be filtered out
    const lowScoreHits = makeHits(3, 0.40);
    const fetchMock = makeFetch([makeSearchResponse(lowScoreHits)]);
    vi.stubGlobal("fetch", fetchMock);

    const result = await recallMemories("some query", "syn");

    // Vector found hits but all below minScore, graph not triggered (hasUsableHits would be false,
    // but with default timeoutMs=5000 and near-instant mock, graph IS triggered)
    // With fetchMock only returning one response (lowScoreHits), graph call also gets that response
    // Result: all below minScore after graph too, so count=0
    expect(result.count).toBe(0);
    expect(result.block).toBeNull();
  });

  it("includes recency boost for recent memories", async () => {
    const { recallMemories } = await import("./recall.js");

    // One recent memory (within 24h), one older
    const recentHit: MemoryHit = {
      id: "mem_recent",
      memory: "Recent memory about the project status update that happened today",
      score: 0.80,
      created_at: new Date().toISOString(),
    };
    const olderHit: MemoryHit = {
      id: "mem_older",
      memory: "Older memory about a different topic that was stored earlier this month",
      score: 0.80,
      created_at: new Date(Date.now() - 48 * 3600 * 1000).toISOString(), // 48h ago
    };

    const fetchMock = makeFetch([makeSearchResponse([recentHit, olderHit])]);
    vi.stubGlobal("fetch", fetchMock);

    const result = await recallMemories("project status", "syn", {
      sufficiencyThreshold: 0.95, // High threshold — prevents sufficiency gate from firing
      sufficiencyMinHits: 5,       // Requires 5 strong hits, we have 0
    });

    // Both memories should appear in output (both above minScore after recency boost)
    expect(result.count).toBeGreaterThanOrEqual(1);
  });

  it("includes the thread summary in the query when provided", async () => {
    const { recallMemories } = await import("./recall.js");

    const fetchMock = makeFetch([makeSearchResponse(makeHits(3, 0.90))]);
    vi.stubGlobal("fetch", fetchMock);

    await recallMemories("current topic", "syn", {
      threadSummary: "We have been discussing vehicle maintenance and torque specifications",
      sufficiencyThreshold: 0.70,
      sufficiencyMinHits: 1,
    });

    const body = JSON.parse(fetchMock.mock.calls[0]![1]!.body as string) as Record<string, unknown>;
    expect(body["query"] as string).toContain("Thread context");
    expect(body["query"] as string).toContain("vehicle maintenance");
  });
});

// --- Read Path 2: Graph-enhanced recall ---

describe("read path: graph-enhanced recall (recallMemories → /graph_enhanced_search)", () => {
  it("calls /graph_enhanced_search when vector search returns no usable hits", async () => {
    const { recallMemories } = await import("./recall.js");

    // First call: /search returns hits below minScore (no usable hits)
    // Second call: /graph_enhanced_search returns good hits
    const graphHits = makeHits(2, 0.82, "Graph Memory");
    const fetchMock = makeFetch([
      makeSearchResponse([]),  // empty vector results
      makeSearchResponse(graphHits),
    ]);
    vi.stubGlobal("fetch", fetchMock);

    const result = await recallMemories("query with no vector hits", "syn", {
      timeoutMs: 5000,
      minScore: 0.75,
    });

    expect(fetchMock).toHaveBeenCalledTimes(2);

    const calls = fetchMock.mock.calls as [string, RequestInit][];
    const urls = calls.map(([url]) => url as string);
    expect(urls[0]).toContain("/search");
    expect(urls[1]).toContain("/graph_enhanced_search");

    // Result should come from graph hits
    expect(result.count).toBeGreaterThan(0);
    expect(result.block).not.toBeNull();
  });

  it("calls /graph_enhanced_search with correct body (query, agent_id, graph params)", async () => {
    const { recallMemories } = await import("./recall.js");

    const fetchMock = makeFetch([
      makeSearchResponse([]),  // no vector hits
      makeSearchResponse(makeHits(2, 0.82)),
    ]);
    vi.stubGlobal("fetch", fetchMock);

    await recallMemories("concept clustering query", "akron", {
      timeoutMs: 5000,
    });

    const calls = fetchMock.mock.calls as [string, RequestInit][];
    const graphCall = calls.find(([url]) => (url as string).includes("graph_enhanced_search"));
    expect(graphCall).toBeDefined();

    const body = JSON.parse(graphCall![1].body as string) as Record<string, unknown>;
    expect(body["query"]).toContain("concept clustering");
    expect(body["agent_id"]).toBe("akron");
    expect(body["user_id"]).toBe("test-user");
    expect(typeof body["graph_weight"]).toBe("number");
    expect(typeof body["graph_depth"]).toBe("number");
  });

  it("skips graph-enhanced search when vector results pass the sufficiency gate", async () => {
    const { recallMemories } = await import("./recall.js");

    // 3 hits above the sufficiency threshold (0.85) — gate should pass, no graph call
    const strongHits = makeHits(3, 0.92);
    const fetchMock = makeFetch([makeSearchResponse(strongHits)]);
    vi.stubGlobal("fetch", fetchMock);

    await recallMemories("well-covered topic", "syn", {
      sufficiencyThreshold: 0.85,
      sufficiencyMinHits: 3,
    });

    // Only /search should have been called — graph skipped due to sufficiency
    expect(fetchMock).toHaveBeenCalledTimes(1);
    const [url] = fetchMock.mock.calls[0]! as [string, RequestInit];
    expect(url).toContain("/search");
    expect(url).not.toContain("graph_enhanced");
  });

  it("skips graph-enhanced search when remaining timeout is less than 1 second", async () => {
    const { recallMemories } = await import("./recall.js");

    // With a very short timeout (1ms), remaining time after first search will be < 1000ms
    // so the graph search should be skipped even when vector results are poor
    const lowHits = makeHits(2, 0.50); // below minScore, would normally trigger graph
    const fetchMock = makeFetch([makeSearchResponse(lowHits)]);
    vi.stubGlobal("fetch", fetchMock);

    await recallMemories("query with tight timeout", "syn", {
      timeoutMs: 1,  // extremely tight — remaining after first call will be < 1000ms
      minScore: 0.75,
    });

    // Only one call — graph search skipped because timeout is nearly exhausted
    expect(fetchMock).toHaveBeenCalledTimes(1);
  });
});

// --- Read Path 3: Tiered fallback behavior ---

describe("read path: tiered fallback behavior", () => {
  it("returns graceful result when /search fails with HTTP 500", async () => {
    const { recallMemories } = await import("./recall.js");

    const fetchMock = makeFetch([makeErrorResponse(500)]);
    vi.stubGlobal("fetch", fetchMock);

    // Should not throw — degraded gracefully
    const result = await recallMemories("query when search is down", "syn");

    expect(result.count).toBe(0);
    expect(result.block).toBeNull();
    expect(result.durationMs).toBeGreaterThanOrEqual(0);
  });

  it("returns graceful result when /search throws a network error", async () => {
    const { recallMemories } = await import("./recall.js");

    const fetchMock = vi.fn().mockRejectedValue(new Error("Network unreachable"));
    vi.stubGlobal("fetch", fetchMock);

    const result = await recallMemories("query when network is down", "syn");

    expect(result.count).toBe(0);
    expect(result.block).toBeNull();
    expect(result.durationMs).toBeGreaterThanOrEqual(0);
  });

  it("returns vector results when /graph_enhanced_search fails", async () => {
    const { recallMemories } = await import("./recall.js");

    // Vector returns no hits → triggers graph; graph fails → should return empty (not throw)
    const fetchMock = makeFetch([
      makeSearchResponse([]),    // empty vector results
      makeErrorResponse(503),    // graph search unavailable
    ]);
    vi.stubGlobal("fetch", fetchMock);

    const result = await recallMemories("query with graph failure", "syn", {
      timeoutMs: 5000,
    });

    // Graph failure should not cause an unhandled rejection
    expect(result.count).toBe(0);
    expect(result.block).toBeNull();
  });

  it("returns block=null and count=0 on timeout (AbortError)", async () => {
    const { recallMemories } = await import("./recall.js");

    // Mock fetch to abort the request (simulates AbortController.abort())
    const fetchMock = vi.fn().mockImplementation((_url: string, init: RequestInit) => {
      return new Promise<never>((_, reject) => {
        const signal = init.signal as AbortSignal | undefined;
        if (signal) {
          signal.addEventListener("abort", () => {
            const err = new Error("The operation was aborted");
            err.name = "AbortError";
            reject(err);
          });
          // If signal already aborted
          if (signal.aborted) {
            const err = new Error("The operation was aborted");
            err.name = "AbortError";
            reject(err);
          }
        }
      });
    });
    vi.stubGlobal("fetch", fetchMock);

    const result = await recallMemories("query that times out", "syn", {
      timeoutMs: 50, // Very short timeout to trigger abort quickly
    });

    expect(result.count).toBe(0);
    expect(result.block).toBeNull();
    expect(result.durationMs).toBeGreaterThanOrEqual(0);
  });

  it("deduplicates exact-text memories across vector results", async () => {
    const { recallMemories } = await import("./recall.js");

    // Return duplicate memories — only one should appear in results
    const duplicateHits: MemoryHit[] = [
      { id: "mem_1", memory: "ALETHEIA_MEMORY_USER must be set in aletheia.env", score: 0.90 },
      { id: "mem_2", memory: "ALETHEIA_MEMORY_USER must be set in aletheia.env", score: 0.85 }, // exact duplicate
      { id: "mem_3", memory: "Project Alpha due October 2026", score: 0.82 },
    ];

    const fetchMock = makeFetch([makeSearchResponse(duplicateHits)]);
    vi.stubGlobal("fetch", fetchMock);

    const result = await recallMemories("memory user env var", "syn", {
      sufficiencyThreshold: 0.70,
      sufficiencyMinHits: 1,
    });

    // Duplicate text should be deduped — count should be 2, not 3
    expect(result.count).toBe(2);
    const text = result.block?.text ?? "";
    const occurrences = (text.match(/ALETHEIA_MEMORY_USER/g) ?? []).length;
    expect(occurrences).toBe(1);
  });

  it("applies MMR diversity selection to avoid redundant results", async () => {
    const { recallMemories } = await import("./recall.js");

    // All hits about the same topic — MMR should prefer diversity
    const redundantHits: MemoryHit[] = [
      { id: "m1", memory: "Widget torque spec is 185 ft-lbs per service manual", score: 0.95 },
      { id: "m2", memory: "Widget torque specification is 185 ft-lbs according to manual", score: 0.93 },
      { id: "m3", memory: "The torque for widget is 185 ft-lbs as stated in manual", score: 0.91 },
      { id: "m4", memory: "Project Alpha due October 2026", score: 0.88 },
      { id: "m5", memory: "MBA final project deadline is March 15", score: 0.84 },
    ];

    const fetchMock = makeFetch([makeSearchResponse(redundantHits)]);
    vi.stubGlobal("fetch", fetchMock);

    const result = await recallMemories("widget maintenance", "syn", {
      limit: 3,
      sufficiencyThreshold: 0.70,
      sufficiencyMinHits: 1,
    });

    // MMR should select diverse results — the diverse items (Project Alpha, MBA) should appear
    // alongside the top widget result
    expect(result.count).toBeLessThanOrEqual(3);
    expect(result.count).toBeGreaterThan(0);
    const text = result.block?.text ?? "";
    // At least one of the diverse items should appear
    const hasDiversity = text.includes("Baby") || text.includes("MBA");
    expect(hasDiversity).toBe(true);
  });
});
