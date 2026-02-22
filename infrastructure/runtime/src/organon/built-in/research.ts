// Meta-tool — research pipeline: memory → web → synthesize
import type { ToolContext, ToolHandler, ToolRegistry } from "../registry.js";

export function createResearchTool(registry: ToolRegistry): ToolHandler {
  return {
    definition: {
      name: "research",
      description:
        "Multi-step research pipeline: search memory first, then web if needed, return combined findings.\n\n" +
        "USE WHEN:\n" +
        "- Answering a question that may require both memory and web knowledge\n" +
        "- Building comprehensive context on a topic before responding\n" +
        "- You'd otherwise chain mem0_search → web_search → web_fetch manually\n\n" +
        "DO NOT USE WHEN:\n" +
        "- You already know the answer from context\n" +
        "- You only need memory OR only need web — use the specific tool directly\n\n" +
        "TIPS:\n" +
        "- Searches memory first — if enough results, skips web\n" +
        "- Set web=true to always include web results\n" +
        "- Returns structured findings from both sources",
      input_schema: {
        type: "object",
        properties: {
          query: {
            type: "string",
            description: "Research query",
          },
          web: {
            type: "boolean",
            description: "Always include web search (default: auto — only if memory insufficient)",
          },
          memoryLimit: {
            type: "number",
            description: "Max memory results (default: 5)",
          },
          webLimit: {
            type: "number",
            description: "Max web results (default: 3)",
          },
        },
        required: ["query"],
      },
    },
    async execute(
      input: Record<string, unknown>,
      context: ToolContext,
    ): Promise<string> {
      const query = String(input["query"] ?? "");
      const forceWeb = (input["web"] as boolean) ?? false;
      const memoryLimit = (input["memoryLimit"] as number) ?? 5;
      const webLimit = (input["webLimit"] as number) ?? 3;

      const findings: Record<string, unknown> = { query };

      // Step 1: Search memory
      let memoryResults: unknown[] = [];
      try {
        const raw = await registry.execute(
          "mem0_search",
          { query, limit: memoryLimit },
          context,
        );
        const parsed = JSON.parse(raw) as Record<string, unknown>;
        memoryResults = (parsed["results"] as unknown[]) ?? [];
        findings["memory"] = {
          count: memoryResults.length,
          results: memoryResults,
        };
      } catch { /* primary research failed */
        findings["memory"] = { count: 0, error: "memory search unavailable" };
      }

      // Step 2: Web search if memory insufficient or forced
      const memoryInsufficient = memoryResults.length < 2;
      if (forceWeb || memoryInsufficient) {
        try {
          // Try whichever web search tool is registered
          const webToolName = registry.get("brave_search")
            ? "brave_search"
            : "web_search";
          const raw = await registry.execute(
            webToolName,
            { query, maxResults: webLimit },
            context,
          );
          findings["web"] = { source: webToolName, results: raw };
        } catch { /* fallback research failed */
          findings["web"] = { error: "web search unavailable" };
        }
      } else {
        findings["web"] = { skipped: true, reason: "sufficient memory results" };
      }

      // Step 3: Summary metadata
      findings["sources"] = {
        memory: memoryResults.length > 0,
        web: !!(findings["web"] as Record<string, unknown>)?.["results"],
      };

      return JSON.stringify(findings);
    },
  };
}
