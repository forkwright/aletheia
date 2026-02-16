// Mem0 memory search tool — query long-term extracted memories
import type { ToolHandler, ToolContext } from "../registry.js";

const SIDECAR_URL = process.env["ALETHEIA_MEMORY_URL"] ?? "http://127.0.0.1:8230";
const USER_ID = process.env["ALETHEIA_MEMORY_USER"] ?? "ck";

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
    const query = String(input["query"] ?? "");
    const limit = Math.min((input["limit"] as number) ?? 10, 20);

    try {
      const controller = new AbortController();
      const timer = setTimeout(() => controller.abort(), 8000);

      // Try graph-enhanced search first, fall back to standard vector search
      const graphBody = JSON.stringify({
        query,
        user_id: USER_ID,
        agent_id: context.nousId,
        limit: limit * 2,
        graph_weight: 0.3,
        graph_depth: 2,
      });

      const searchBody = (agentId?: string) =>
        JSON.stringify({
          query,
          user_id: USER_ID,
          ...(agentId ? { agent_id: agentId } : {}),
          limit,
        });

      let agentResults: unknown[] = [];
      let globalResults: unknown[] = [];

      const extract = async (res: Response) => {
        if (!res.ok) return [];
        const data = (await res.json()) as Record<string, unknown>;
        const results = (data["results"] as unknown[]) ?? [];
        return Array.isArray(results) ? results : [];
      };

      try {
        // Try graph-enhanced endpoint
        const graphRes = await fetch(`${SIDECAR_URL}/graph_enhanced_search`, {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: graphBody,
          signal: controller.signal,
        });

        if (graphRes.ok) {
          agentResults = await extract(graphRes);
        } else {
          // Fallback to standard search
          const [aRes, gRes] = await Promise.all([
            fetch(`${SIDECAR_URL}/search`, {
              method: "POST",
              headers: { "Content-Type": "application/json" },
              body: searchBody(context.nousId),
              signal: controller.signal,
            }),
            fetch(`${SIDECAR_URL}/search`, {
              method: "POST",
              headers: { "Content-Type": "application/json" },
              body: searchBody(),
              signal: controller.signal,
            }),
          ]);
          agentResults = await extract(aRes);
          globalResults = await extract(gRes);
        }
      } catch {
        // If graph-enhanced fails, try standard
        try {
          const [aRes, gRes] = await Promise.all([
            fetch(`${SIDECAR_URL}/search`, {
              method: "POST",
              headers: { "Content-Type": "application/json" },
              body: searchBody(context.nousId),
              signal: controller.signal,
            }),
            fetch(`${SIDECAR_URL}/search`, {
              method: "POST",
              headers: { "Content-Type": "application/json" },
              body: searchBody(),
              signal: controller.signal,
            }),
          ]);
          agentResults = await extract(aRes);
          globalResults = await extract(gRes);
        } catch {
          // Both failed
        }
      }
      clearTimeout(timer);

      // Merge and deduplicate by memory id, preferring agent-scoped
      const seen = new Set<string>();
      const merged: Record<string, unknown>[] = [];
      const all = [...agentResults, ...globalResults] as Record<string, unknown>[];
      for (let i = 0; i < all.length; i++) {
        const m = all[i]!;
        const id = String(m["id"] ?? `${m["memory"] ?? ""}_${i}`);
        if (!seen.has(id)) {
          seen.add(id);
          merged.push(m);
        }
      }

      // Sort by score descending, take top `limit`
      merged.sort((a, b) => ((b["score"] as number) ?? 0) - ((a["score"] as number) ?? 0));
      const memories = merged.slice(0, limit).map((m) => ({
        memory: m["memory"] ?? m["text"] ?? "",
        score: m["score"] ?? null,
        agent_id: m["agent_id"] ?? null,
      }));

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
