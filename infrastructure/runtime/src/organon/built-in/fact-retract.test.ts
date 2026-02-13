// Fact retract tool tests
import { describe, it, expect, vi, beforeEach } from "vitest";
import { factRetractTool } from "./fact-retract.js";

const ctx = { nousId: "syn", sessionId: "ses_1", workspace: "/tmp" };

describe("factRetractTool", () => {
  beforeEach(() => {
    vi.stubGlobal("fetch", vi.fn());
  });

  it("has valid definition", () => {
    expect(factRetractTool.definition.name).toBe("fact_retract");
    expect(factRetractTool.definition.input_schema.required).toContain("query");
    expect(factRetractTool.definition.input_schema.required).toContain("reason");
  });

  it("sends retraction request to sidecar", async () => {
    (fetch as ReturnType<typeof vi.fn>).mockResolvedValue({
      ok: true,
      json: vi.fn().mockResolvedValue({ retracted: 3, ids: ["m1", "m2", "m3"] }),
    });

    const result = await factRetractTool.execute(
      { query: "old fact", reason: "outdated", cascade: false, dry_run: false },
      ctx,
    );
    const parsed = JSON.parse(result);
    expect(parsed.retracted).toBe(3);
    expect(fetch).toHaveBeenCalledWith(
      expect.stringContaining("/retract"),
      expect.objectContaining({ method: "POST" }),
    );
  });

  it("handles HTTP error", async () => {
    (fetch as ReturnType<typeof vi.fn>).mockResolvedValue({
      ok: false,
      status: 500,
    });

    const result = await factRetractTool.execute(
      { query: "something", reason: "test" },
      ctx,
    );
    expect(JSON.parse(result).error).toContain("HTTP 500");
  });

  it("handles network error", async () => {
    (fetch as ReturnType<typeof vi.fn>).mockRejectedValue(new Error("Connection refused"));

    const result = await factRetractTool.execute(
      { query: "something", reason: "test" },
      ctx,
    );
    expect(JSON.parse(result).error).toContain("Connection refused");
  });

  it("includes nous ID in reason prefix", async () => {
    (fetch as ReturnType<typeof vi.fn>).mockResolvedValue({
      ok: true,
      json: vi.fn().mockResolvedValue({ retracted: 0 }),
    });

    await factRetractTool.execute(
      { query: "fact", reason: "wrong info" },
      ctx,
    );

    const body = JSON.parse((fetch as ReturnType<typeof vi.fn>).mock.calls[0]![1].body);
    expect(body.reason).toContain("[syn]");
  });
});
