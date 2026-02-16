// Web fetch tool tests
import { describe, it, expect, vi, beforeEach } from "vitest";
import { webFetchTool } from "./web-fetch.js";

// Mock SSRF guard to allow all URLs in tests
vi.mock("./ssrf-guard.js", () => ({
  validateUrl: vi.fn().mockResolvedValue(undefined),
}));

describe("webFetchTool", () => {
  beforeEach(() => {
    vi.stubGlobal("fetch", vi.fn());
  });

  it("has valid definition", () => {
    expect(webFetchTool.definition.name).toBe("web_fetch");
    expect(webFetchTool.definition.input_schema.required).toContain("url");
  });

  it("fetches plain text", async () => {
    (fetch as ReturnType<typeof vi.fn>).mockResolvedValue({
      ok: true,
      headers: new Headers({ "content-type": "text/plain" }),
      text: vi.fn().mockResolvedValue("plain text content"),
    });

    const result = await webFetchTool.execute({ url: "https://example.com/file.txt" });
    expect(result).toBe("plain text content");
  });

  it("strips HTML tags", async () => {
    const html = "<html><body><p>Hello <b>world</b></p><script>evil()</script></body></html>";
    (fetch as ReturnType<typeof vi.fn>).mockResolvedValue({
      ok: true,
      headers: new Headers({ "content-type": "text/html" }),
      text: vi.fn().mockResolvedValue(html),
    });

    const result = await webFetchTool.execute({ url: "https://example.com" });
    expect(result).toContain("Hello");
    expect(result).toContain("world");
    expect(result).not.toContain("<script>");
    expect(result).not.toContain("evil()");
  });

  it("truncates long content", async () => {
    const longText = "a".repeat(200);
    (fetch as ReturnType<typeof vi.fn>).mockResolvedValue({
      ok: true,
      headers: new Headers({ "content-type": "text/plain" }),
      text: vi.fn().mockResolvedValue(longText),
    });

    const result = await webFetchTool.execute({ url: "https://example.com", maxLength: 100 });
    expect(result.length).toBeLessThanOrEqual(120); // 100 + truncation message
    expect(result).toContain("[truncated]");
  });

  it("returns error for HTTP failure", async () => {
    (fetch as ReturnType<typeof vi.fn>).mockResolvedValue({
      ok: false,
      status: 404,
      statusText: "Not Found",
      headers: new Headers(),
    });

    const result = await webFetchTool.execute({ url: "https://example.com/missing" });
    expect(result).toContain("HTTP 404");
  });

  it("returns error for too-large response", async () => {
    (fetch as ReturnType<typeof vi.fn>).mockResolvedValue({
      ok: true,
      headers: new Headers({ "content-type": "text/plain", "content-length": "10000000" }),
      text: vi.fn().mockResolvedValue("big"),
    });

    const result = await webFetchTool.execute({ url: "https://example.com", maxLength: 1000 });
    expect(result).toContain("too large");
  });

  it("returns error for network failure", async () => {
    (fetch as ReturnType<typeof vi.fn>).mockRejectedValue(new Error("Connection refused"));

    const result = await webFetchTool.execute({ url: "https://example.com" });
    expect(result).toContain("Connection refused");
  });

  it("handles HTML entity decoding", async () => {
    const html = "<p>A &amp; B &quot;E&quot; F&#39;s &nbsp;G</p>";
    (fetch as ReturnType<typeof vi.fn>).mockResolvedValue({
      ok: true,
      headers: new Headers({ "content-type": "text/html" }),
      text: vi.fn().mockResolvedValue(html),
    });

    const result = await webFetchTool.execute({ url: "https://example.com" });
    expect(result).toContain("A & B");
    expect(result).toContain('"E"');
    expect(result).toContain("F's");
  });

  it("strips double-encoded script tags for XSS prevention", async () => {
    const html = "<p>safe</p>&lt;script&gt;alert(1)&lt;/script&gt;";
    (fetch as ReturnType<typeof vi.fn>).mockResolvedValue({
      ok: true,
      headers: new Headers({ "content-type": "text/html" }),
      text: vi.fn().mockResolvedValue(html),
    });

    const result = await webFetchTool.execute({ url: "https://example.com" });
    expect(result).toContain("safe");
    expect(result).not.toContain("<script>");
    expect(result).not.toContain("alert(1)");
  });
});
