// Web search tool — search the web via DuckDuckGo HTML
import type { ToolHandler } from "../registry.js";

export const webSearchTool: ToolHandler = {
  definition: {
    name: "web_search",
    description:
      "Search the web via DuckDuckGo (no API key needed).\n\n" +
      "USE WHEN:\n" +
      "- Web search when Brave API key is not available\n" +
      "- Quick lookups that don't need API-quality results\n\n" +
      "DO NOT USE WHEN:\n" +
      "- Brave Search is available — it returns richer results\n" +
      "- The answer is in your memory or workspace files\n\n" +
      "TIPS:\n" +
      "- Parses DuckDuckGo HTML — may break if DDG changes format\n" +
      "- Follow up with web_fetch to read full page content",
    input_schema: {
      type: "object",
      properties: {
        query: {
          type: "string",
          description: "Search query",
        },
        maxResults: {
          type: "number",
          description: "Maximum results to return (default: 5)",
        },
      },
      required: ["query"],
    },
  },
  async execute(input: Record<string, unknown>): Promise<string> {
    const query = input["query"] as string;
    const maxResults = (input["maxResults"] as number) ?? 5;

    try {
      const url = `https://html.duckduckgo.com/html/?q=${encodeURIComponent(query)}`;
      const res = await fetch(url, {
        headers: {
          "User-Agent":
            "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36",
        },
        signal: AbortSignal.timeout(10000),
      });

      if (!res.ok) {
        return `Error: Search failed with HTTP ${res.status}`;
      }

      const html = await res.text();
      const results = parseDdgResults(html, maxResults);

      if (results.length === 0) {
        return "No results found";
      }

      return results
        .map((r, i) => `${i + 1}. ${r.title}\n   ${r.url}\n   ${r.snippet}`)
        .join("\n\n");
    } catch (error) {
      const msg = error instanceof Error ? error.message : String(error);
      return `Error: ${msg}`;
    }
  },
};

interface SearchResult {
  title: string;
  url: string;
  snippet: string;
}

function parseDdgResults(html: string, max: number): SearchResult[] {
  const results: SearchResult[] = [];
  const resultPattern =
    /<a[^>]*class="result__a"[^>]*href="([^"]*)"[^>]*>([\s\S]*?)<\/a>/gi;
  const snippetPattern =
    /<a[^>]*class="result__snippet"[^>]*>([\s\S]*?)<\/a>/gi;

  const titles = [...html.matchAll(resultPattern)];
  const snippets = [...html.matchAll(snippetPattern)];

  for (let i = 0; i < Math.min(titles.length, max); i++) {
    const titleMatch = titles[i];
    const snippetMatch = snippets[i];
    if (!titleMatch) continue;

    let url = titleMatch[1] ?? "";
    if (url.startsWith("//duckduckgo.com/l/?uddg=")) {
      const decoded = decodeURIComponent(url.split("uddg=")[1]?.split("&")[0] ?? "");
      if (decoded) url = decoded;
    }

    results.push({
      title: stripTags(titleMatch[2] ?? ""),
      url,
      snippet: snippetMatch ? stripTags(snippetMatch[1] ?? "") : "",
    });
  }

  return results;
}

function stripTags(html: string): string {
  return html
    .replace(/<[^>]+>/g, "")
    .replace(/&amp;/g, "&")
    .replace(/&lt;/g, "<")
    .replace(/&gt;/g, ">")
    .replace(/&quot;/g, '"')
    .replace(/&#39;/g, "'")
    .replace(/&nbsp;/g, " ")
    .trim();
}
