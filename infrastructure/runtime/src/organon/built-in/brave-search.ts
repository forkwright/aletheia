// Web search via Brave Search API
import type { ToolHandler } from "../registry.js";

const BRAVE_API_URL = "https://api.search.brave.com/res/v1/web/search";

export const braveSearchTool: ToolHandler = {
  definition: {
    name: "web_search",
    description:
      "Search the web using Brave Search. Returns titles, URLs, descriptions, and age.",
    input_schema: {
      type: "object",
      properties: {
        query: {
          type: "string",
          description: "Search query",
        },
        maxResults: {
          type: "number",
          description: "Maximum results to return (default: 5, max: 20)",
        },
      },
      required: ["query"],
    },
  },
  async execute(input: Record<string, unknown>): Promise<string> {
    const query = String(input.query ?? "");
    const maxResults = Math.min((input.maxResults as number) ?? 5, 20);

    try {
      const apiKey = process.env.BRAVE_API_KEY;
      if (!apiKey) {
        return "Error: BRAVE_API_KEY not set in environment";
      }

      const url = new URL(BRAVE_API_URL);
      url.searchParams.set("q", query);
      url.searchParams.set("count", String(maxResults));

      const res = await fetch(url.toString(), {
        headers: {
          Accept: "application/json",
          "Accept-Encoding": "gzip",
          "X-Subscription-Token": apiKey,
        },
        signal: AbortSignal.timeout(10000),
      });

      if (!res.ok) {
        return `Error: Brave Search returned HTTP ${res.status}`;
      }

      const data = (await res.json()) as BraveResponse;
      const results = data.web?.results ?? [];

      if (results.length === 0) {
        return "No results found";
      }

      return results
        .slice(0, maxResults)
        .map((r, i) => {
          const age = r.age ? ` (${r.age})` : "";
          return `${i + 1}. ${r.title}${age}\n   ${r.url}\n   ${r.description ?? ""}`;
        })
        .join("\n\n");
    } catch (error) {
      const msg = error instanceof Error ? error.message : String(error);
      return `Error: ${msg}`;
    }
  },
};

interface BraveResponse {
  web?: {
    results: Array<{
      title: string;
      url: string;
      description?: string;
      age?: string;
    }>;
  };
}
