// Memory correction tool tests
import { describe, expect, it } from "vitest";
import { memoryCorrectTool } from "./memory-correct.js";
import type { ToolContext } from "../registry.js";

const ctx: ToolContext = { nousId: "syl", sessionId: "ses_1", workspace: "/w", allowedRoots: ["/"], depth: 0 };

describe("memoryCorrectTool", () => {
  it("has correct definition", () => {
    expect(memoryCorrectTool.definition.name).toBe("memory_correct");
    expect(memoryCorrectTool.definition.input_schema.required).toContain("query");
    expect(memoryCorrectTool.definition.input_schema.required).toContain("corrected_text");
    expect(memoryCorrectTool.definition.input_schema.required).toContain("reason");
  });

  it("calls sidecar /memory/correct with correct payload", async () => {
    const mockFetch = vi.fn().mockResolvedValue({
      ok: true,
      json: async () => ({ corrected: true, old_id: "m1", new_id: "m2" }),
    });
    vi.stubGlobal("fetch", mockFetch);

    const result = await memoryCorrectTool.execute(
      { query: "old fact", corrected_text: "new fact", reason: "was wrong" },
      ctx,
    );

    expect(mockFetch).toHaveBeenCalledWith(
      expect.stringContaining("/memory/correct"),
      expect.objectContaining({ method: "POST" }),
    );
    const body = JSON.parse((mockFetch.mock.calls[0]![1] as RequestInit).body as string);
    expect(body.query).toBe("old fact");
    expect(body.corrected_text).toBe("new fact");
    expect(body.reason).toContain("[syl]");
    expect(body.agent_id).toBe("syl");

    const parsed = JSON.parse(result);
    expect(parsed.corrected).toBe(true);

    vi.unstubAllGlobals();
  });

  it("returns error JSON on HTTP failure", async () => {
    vi.stubGlobal("fetch", vi.fn().mockResolvedValue({ ok: false, status: 500 }));

    const result = await memoryCorrectTool.execute(
      { query: "q", corrected_text: "c", reason: "r" },
      ctx,
    );

    const parsed = JSON.parse(result);
    expect(parsed.error).toContain("HTTP 500");

    vi.unstubAllGlobals();
  });

  it("returns error JSON on network failure", async () => {
    vi.stubGlobal("fetch", vi.fn().mockRejectedValue(new Error("ECONNREFUSED")));

    const result = await memoryCorrectTool.execute(
      { query: "q", corrected_text: "c", reason: "r" },
      ctx,
    );

    const parsed = JSON.parse(result);
    expect(parsed.error).toContain("ECONNREFUSED");

    vi.unstubAllGlobals();
  });
});
