// Mem0 memory search tool — query long-term extracted memories
import type { ToolHandler, ToolContext } from "../registry.js";

const SIDECAR_URL = process.env.ALETHEIA_MEMORY_URL || "http://127.0.0.1:8230";
const USER_ID = process.env.ALETHEIA_MEMORY_USER || "ck";

export const mem0SearchTool: ToolHandler = {
  definition: {
    name: "mem0_search",
    description:
      "Search long-term extracted memories from past conversations. " +
      "Returns facts, preferences, and entity relationships that were " +
      "automatically captured. Use for cross-session recall.",
    input_schema: {
      type: "object",
      properties: {
        query: {
          type: "string",
          description: "Semantic search query",
        },
        limit: {
          type: "number",
          description: "Max results (default 10)",
        },
      },
      required: ["query"],
    },
  },
  async execute(
    input: Record<string, unknown>,
    context: ToolContext,
  ): Promise<string> {
    const query = String(input.query ?? "");
    const limit = Math.min((input.limit as number) ?? 10, 20);

    try {
      const controller = new AbortController();
      const timer = setTimeout(() => controller.abort(), 8000);

      const res = await fetch(`${SIDECAR_URL}/search`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          query,
          user_id: USER_ID,
          agent_id: context.nousId,
          limit,
        }),
        signal: controller.signal,
      });

      clearTimeout(timer);

      if (!res.ok) {
        return JSON.stringify({ results: [], error: `mem0 returned ${res.status}` });
      }

      const data = (await res.json()) as Record<string, unknown>;
      const results = (data.results as unknown[]) ?? [];
      const memories = (Array.isArray(results) ? results : []).map(
        (m: Record<string, unknown>) => ({
          memory: m.memory ?? m.text ?? "",
          score: m.score ?? null,
          agent_id: m.agent_id ?? null,
        }),
      );

      return JSON.stringify({ results: memories, count: memories.length });
    } catch (err) {
      if (err instanceof Error && err.name === "AbortError") {
        return JSON.stringify({ results: [], error: "mem0 search timed out" });
      }
      return JSON.stringify({
        results: [],
        error: err instanceof Error ? err.message : String(err),
      });
    }
  },
};
