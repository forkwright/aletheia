// Web search tool tests
import { beforeEach, describe, expect, it, vi } from "vitest";
import { webSearchTool } from "./web-search.js";

describe("webSearchTool", () => {
  beforeEach(() => {
    vi.stubGlobal("fetch", vi.fn());
  });

  it("has valid definition", () => {
    expect(webSearchTool.definition.name).toBe("web_search");
    expect(webSearchTool.definition.input_schema.required).toContain("query");
  });

  it("parses DuckDuckGo results", async () => {
    const html = `
      <a class="result__a" href="https://example.com">Example Title</a>
      <a class="result__snippet">Example description here</a>
      <a class="result__a" href="https://other.com">Other Title</a>
      <a class="result__snippet">Other description</a>
    `;
    (fetch as ReturnType<typeof vi.fn>).mockResolvedValue({
      ok: true,
      text: vi.fn().mockResolvedValue(html),
    });

    const result = await webSearchTool.execute({ query: "test query" });
    expect(result).toContain("Example Title");
    expect(result).toContain("https://example.com");
    expect(result).toContain("Example description");
  });

  it("respects maxResults", async () => {
    const html = `
      <a class="result__a" href="https://1.com">One</a>
      <a class="result__snippet">Desc 1</a>
      <a class="result__a" href="https://2.com">Two</a>
      <a class="result__snippet">Desc 2</a>
      <a class="result__a" href="https://3.com">Three</a>
      <a class="result__snippet">Desc 3</a>
    `;
    (fetch as ReturnType<typeof vi.fn>).mockResolvedValue({
      ok: true,
      text: vi.fn().mockResolvedValue(html),
    });

    const result = await webSearchTool.execute({ query: "test", maxResults: 2 });
    expect(result).toContain("One");
    expect(result).toContain("Two");
    expect(result).not.toContain("Three");
  });

  it("returns no results message", async () => {
    (fetch as ReturnType<typeof vi.fn>).mockResolvedValue({
      ok: true,
      text: vi.fn().mockResolvedValue("<html><body></body></html>"),
    });

    const result = await webSearchTool.execute({ query: "impossible query" });
    expect(result).toBe("No results found");
  });

  it("handles HTTP error", async () => {
    (fetch as ReturnType<typeof vi.fn>).mockResolvedValue({
      ok: false,
      status: 429,
    });

    const result = await webSearchTool.execute({ query: "test" });
    expect(result).toContain("HTTP 429");
  });

  it("handles network error", async () => {
    (fetch as ReturnType<typeof vi.fn>).mockRejectedValue(new Error("DNS failure"));

    const result = await webSearchTool.execute({ query: "test" });
    expect(result).toContain("DNS failure");
  });

  it("decodes DuckDuckGo redirect URLs", async () => {
    const html = `
      <a class="result__a" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Freal.com%2Fpage&rut=abc">Title</a>
      <a class="result__snippet">Desc</a>
    `;
    (fetch as ReturnType<typeof vi.fn>).mockResolvedValue({
      ok: true,
      text: vi.fn().mockResolvedValue(html),
    });

    const result = await webSearchTool.execute({ query: "test" });
    expect(result).toContain("https://real.com/page");
  });
});
