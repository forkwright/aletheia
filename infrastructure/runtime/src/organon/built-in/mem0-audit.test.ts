// mem0_audit tool tests
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mem0AuditTool } from "./mem0-audit.js";

const ctx = { nousId: "syn", sessionId: "ses_1", workspace: "/tmp" };

describe("mem0_audit", () => {
  const originalFetch = globalThis.fetch;

  beforeEach(() => {
    vi.stubEnv("ALETHEIA_MEMORY_URL", "http://localhost:9999");
  });

  afterEach(() => {
    globalThis.fetch = originalFetch;
    vi.unstubAllEnvs();
  });

  it("calls /search when query provided", async () => {
    globalThis.fetch = vi.fn().mockResolvedValue({
      ok: true,
      json: async () => ({ results: [{ id: "m1", memory: "test fact", score: 0.9 }] }),
    }) as unknown as typeof fetch;

    const result = JSON.parse(await mem0AuditTool.execute({
      query: "user preferences",
    }, ctx));

    expect(result.results).toHaveLength(1);
    expect(result.instructions).toContain("memory_correct");

    const [url, opts] = (globalThis.fetch as ReturnType<typeof vi.fn>).mock.calls[0]!;
    expect(url).toBe("http://localhost:9999/search");
    const body = JSON.parse((opts as RequestInit).body as string);
    expect(body.query).toBe("user preferences");
    expect(body.agent_id).toBe("syn");
  });

  it("calls /memories when no query provided", async () => {
    globalThis.fetch = vi.fn().mockResolvedValue({
      ok: true,
      json: async () => ({ memories: [], count: 0 }),
    }) as unknown as typeof fetch;

    const result = JSON.parse(await mem0AuditTool.execute({}, ctx));

    expect(result.count).toBe(0);
    expect(result.instructions).toBeDefined();

    const [url] = (globalThis.fetch as ReturnType<typeof vi.fn>).mock.calls[0]!;
    expect(url).toContain("/memories?");
    expect(url).toContain("agent_id=syn");
  });

  it("respects limit parameter", async () => {
    globalThis.fetch = vi.fn().mockResolvedValue({
      ok: true,
      json: async () => ({ results: [] }),
    }) as unknown as typeof fetch;

    await mem0AuditTool.execute({ query: "test", limit: 5 }, ctx);

    const body = JSON.parse(((globalThis.fetch as ReturnType<typeof vi.fn>).mock.calls[0]![1] as RequestInit).body as string);
    expect(body.limit).toBe(5);
  });

  it("returns error on HTTP failure", async () => {
    globalThis.fetch = vi.fn().mockResolvedValue({
      ok: false,
      status: 503,
    }) as unknown as typeof fetch;

    const result = JSON.parse(await mem0AuditTool.execute({
      query: "test",
    }, ctx));

    expect(result.error).toContain("HTTP 503");
  });

  it("returns error on network failure", async () => {
    globalThis.fetch = vi.fn().mockRejectedValue(new Error("ECONNREFUSED")) as unknown as typeof fetch;

    const result = JSON.parse(await mem0AuditTool.execute({}, ctx));

    expect(result.error).toContain("ECONNREFUSED");
  });
});
