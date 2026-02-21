// Memory forget tool tests
import { describe, expect, it, vi } from "vitest";
import { memoryForgetTool } from "./memory-forget.js";
import type { ToolContext } from "../registry.js";

const ctx: ToolContext = { nousId: "chiron", sessionId: "ses_2", workspace: "/w", allowedRoots: ["/"], depth: 0 };

describe("memoryForgetTool", () => {
  it("has correct definition", () => {
    expect(memoryForgetTool.definition.name).toBe("memory_forget");
    expect(memoryForgetTool.definition.input_schema.required).toContain("query");
    expect(memoryForgetTool.definition.input_schema.required).toContain("reason");
  });

  it("calls sidecar /memory/forget with correct payload", async () => {
    const mockFetch = vi.fn().mockResolvedValue({
      ok: true,
      json: async () => ({ forgotten: 2, previewed: false }),
    });
    vi.stubGlobal("fetch", mockFetch);

    const result = await memoryForgetTool.execute(
      { query: "old memory", reason: "no longer relevant", dry_run: false, max_deletions: 5, min_score: 0.9 },
      ctx,
    );

    const body = JSON.parse((mockFetch.mock.calls[0]![1] as RequestInit).body as string);
    expect(body.query).toBe("old memory");
    expect(body.reason).toContain("[chiron]");
    expect(body.max_deletions).toBe(5);
    expect(body.min_score).toBe(0.9);
    expect(body.dry_run).toBe(false);

    const parsed = JSON.parse(result);
    expect(parsed.forgotten).toBe(2);

    vi.unstubAllGlobals();
  });

  it("supports dry_run mode", async () => {
    const mockFetch = vi.fn().mockResolvedValue({
      ok: true,
      json: async () => ({ would_forget: 3, previewed: true }),
    });
    vi.stubGlobal("fetch", mockFetch);

    const result = await memoryForgetTool.execute(
      { query: "test", reason: "testing", dry_run: true },
      ctx,
    );

    const body = JSON.parse((mockFetch.mock.calls[0]![1] as RequestInit).body as string);
    expect(body.dry_run).toBe(true);

    const parsed = JSON.parse(result);
    expect(parsed.would_forget).toBe(3);

    vi.unstubAllGlobals();
  });

  it("uses defaults for optional parameters", async () => {
    const mockFetch = vi.fn().mockResolvedValue({
      ok: true,
      json: async () => ({ forgotten: 1 }),
    });
    vi.stubGlobal("fetch", mockFetch);

    await memoryForgetTool.execute({ query: "q", reason: "r" }, ctx);

    const body = JSON.parse((mockFetch.mock.calls[0]![1] as RequestInit).body as string);
    expect(body.max_deletions).toBe(3);
    expect(body.min_score).toBe(0.85);
    expect(body.dry_run).toBe(false);

    vi.unstubAllGlobals();
  });

  it("returns error JSON on HTTP failure", async () => {
    vi.stubGlobal("fetch", vi.fn().mockResolvedValue({ ok: false, status: 503 }));

    const result = await memoryForgetTool.execute({ query: "q", reason: "r" }, ctx);
    const parsed = JSON.parse(result);
    expect(parsed.error).toContain("HTTP 503");

    vi.unstubAllGlobals();
  });

  it("returns error JSON on network failure", async () => {
    vi.stubGlobal("fetch", vi.fn().mockRejectedValue(new Error("timeout")));

    const result = await memoryForgetTool.execute({ query: "q", reason: "r" }, ctx);
    const parsed = JSON.parse(result);
    expect(parsed.error).toContain("timeout");

    vi.unstubAllGlobals();
  });
});
