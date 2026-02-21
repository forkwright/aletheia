// Link preprocessing tests
import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../organon/built-in/ssrf-guard.js", () => ({
  validateUrl: vi.fn().mockResolvedValue(undefined),
}));

import { preprocessLinks } from "./preprocess.js";

describe("preprocessLinks", () => {
  beforeEach(() => {
    vi.restoreAllMocks();
    // Mock global fetch
    vi.stubGlobal("fetch", vi.fn().mockResolvedValue({
      ok: true,
      headers: { get: () => "text/html" },
      text: () => Promise.resolve("<html><head><title>Test Page</title></head><body>Content here</body></html>"),
    }));
  });

  it("returns text unchanged when no URLs present", async () => {
    const result = await preprocessLinks("Hello world");
    expect(result).toBe("Hello world");
  });

  it("appends preview for URLs in text", async () => {
    const result = await preprocessLinks("Check https://example.com");
    expect(result).toContain("https://example.com");
    expect(result).toContain("[Link:");
    expect(result).toContain("Test Page");
  });

  it("limits number of URLs processed", async () => {
    const text = "url1 https://a.com url2 https://b.com url3 https://c.com url4 https://d.com";
    await preprocessLinks(text, 2);
    expect(vi.mocked(fetch)).toHaveBeenCalledTimes(2);
  });

  it("deduplicates URLs", async () => {
    const text = "https://example.com and again https://example.com";
    await preprocessLinks(text);
    expect(vi.mocked(fetch)).toHaveBeenCalledTimes(1);
  });

  it("handles fetch failures gracefully", async () => {
    vi.mocked(fetch).mockRejectedValue(new Error("network error"));
    const result = await preprocessLinks("Check https://example.com");
    expect(result).toBe("Check https://example.com");
  });

  it("skips non-text content types", async () => {
    vi.mocked(fetch).mockResolvedValue({
      ok: true,
      headers: { get: () => "image/png" },
      text: () => Promise.resolve(""),
    } as never);
    const result = await preprocessLinks("Image https://example.com/photo.png");
    expect(result).toBe("Image https://example.com/photo.png");
  });

  it("handles non-ok responses", async () => {
    vi.mocked(fetch).mockResolvedValue({
      ok: false,
      headers: { get: () => "text/html" },
      text: () => Promise.resolve(""),
    } as never);
    const result = await preprocessLinks("Check https://example.com/404");
    expect(result).toBe("Check https://example.com/404");
  });
});
