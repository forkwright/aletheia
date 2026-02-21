// mem0_retract tool tests
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mem0RetractTool } from "./mem0-retract.js";

const ctx = { nousId: "syn", sessionId: "ses_1", workspace: "/tmp" };

describe("mem0_retract", () => {
  const originalFetch = globalThis.fetch;

  beforeEach(() => {
    vi.stubEnv("ALETHEIA_MEMORY_URL", "http://localhost:9999");
  });

  afterEach(() => {
    globalThis.fetch = originalFetch;
    vi.unstubAllEnvs();
  });

  it("calls sidecar /retract with correct payload", async () => {
    globalThis.fetch = vi.fn().mockResolvedValue({
      ok: true,
      json: async () => ({ retracted: 2, dry_run: false }),
    }) as unknown as typeof fetch;

    const result = JSON.parse(await mem0RetractTool.execute({
      query: "old address",
      reason: "moved to new house",
    }, ctx));

    expect(result.retracted).toBe(2);
    expect(globalThis.fetch).toHaveBeenCalledOnce();

    const [url, opts] = (globalThis.fetch as ReturnType<typeof vi.fn>).mock.calls[0]!;
    expect(url).toBe("http://localhost:9999/retract");
    const body = JSON.parse((opts as RequestInit).body as string);
    expect(body.query).toBe("old address");
    expect(body.reason).toContain("[syn]");
    expect(body.cascade).toBe(false);
    expect(body.dry_run).toBe(false);
  });

  it("passes cascade and dry_run flags", async () => {
    globalThis.fetch = vi.fn().mockResolvedValue({
      ok: true,
      json: async () => ({ retracted: 0, dry_run: true, preview: [] }),
    }) as unknown as typeof fetch;

    await mem0RetractTool.execute({
      query: "test",
      reason: "audit",
      cascade: true,
      dry_run: true,
    }, ctx);

    const body = JSON.parse(((globalThis.fetch as ReturnType<typeof vi.fn>).mock.calls[0]![1] as RequestInit).body as string);
    expect(body.cascade).toBe(true);
    expect(body.dry_run).toBe(true);
  });

  it("returns error on HTTP failure", async () => {
    globalThis.fetch = vi.fn().mockResolvedValue({
      ok: false,
      status: 500,
    }) as unknown as typeof fetch;

    const result = JSON.parse(await mem0RetractTool.execute({
      query: "test",
      reason: "test",
    }, ctx));

    expect(result.error).toContain("HTTP 500");
  });

  it("returns error on network failure", async () => {
    globalThis.fetch = vi.fn().mockRejectedValue(new Error("Connection refused")) as unknown as typeof fetch;

    const result = JSON.parse(await mem0RetractTool.execute({
      query: "test",
      reason: "test",
    }, ctx));

    expect(result.error).toContain("Connection refused");
  });
});
