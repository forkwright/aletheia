// Mem0 memory search tool — query long-term extracted memories
import type { ToolContext, ToolHandler } from "../registry.js";
import { createLogger } from "../../koina/logger.js";

const log = createLogger("tool:mem0-search");

// Lazy reads — env vars may be set by taxis config after module import
const getSidecarUrl = () => process.env["ALETHEIA_MEMORY_URL"] ?? "http://127.0.0.1:8230";
const getUserId = () => process.env["ALETHEIA_MEMORY_USER"] ?? "default";

export const mem0SearchTool: ToolHandler = {
  definition: {
    name: "mem0_search",
    description:
      "Search long-term memory for facts, preferences, and relationships from past conversations.\n\n" +
      "USE WHEN:\n" +
      "- Recalling user preferences, past decisions, or established facts\n" +
      "- Checking what you already know before asking the user\n" +
      "- Finding context from previous sessions or other agents' interactions\n" +
      "- Understanding entity relationships (people, projects, tools)\n\n" +
      "DO NOT USE WHEN:\n" +
      "- Looking for information in the current session — it's already in context\n" +
      "- Searching for files or code — use grep or find instead\n\n" +
      "TIPS:\n" +
      "- Uses semantic search with LLM-powered query rewriting and alias resolution\n" +
      "- Phrase queries naturally — the system rewrites your query into multiple search variants\n" +
      "- Results include score — higher is more relevant\n" +
      "- Searches both agent-scoped and global memories, deduplicates",
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
      const timer = setTimeout(() => controller.abort(), 12000);

      const extract = async (res: Response) => {
        if (!res.ok) return [];
        const data = (await res.json()) as Record<string, unknown>;
        const results = (data["results"] as unknown[]) ?? [];
        return Array.isArray(results) ? results : [];
      };

      let results: unknown[] = [];

      // Tier 1: Enhanced search with query rewriting + alias resolution
      try {
        const enhancedRes = await fetch(`${getSidecarUrl()}/search_enhanced`, {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({
            query,
            user_id: getUserId(),
            agent_id: context.nousId,
            limit: limit * 2,
            rewrite: true,
          }),
          signal: controller.signal,
        });
        if (enhancedRes.ok) {
          results = await extract(enhancedRes);
        }
      } catch (err) {
        log.debug(`Tier 1 (enhanced) failed: ${err instanceof Error ? err.message : err}`);
      }

      // Tier 2: Graph-enhanced search (vector + graph neighbor expansion)
      if (results.length === 0) {
        try {
          const graphRes = await fetch(`${getSidecarUrl()}/graph_enhanced_search`, {
            method: "POST",
            headers: { "Content-Type": "application/json" },
            body: JSON.stringify({
              query,
              user_id: getUserId(),
              agent_id: context.nousId,
              limit: limit * 2,
              graph_weight: 0.3,
              graph_depth: 2,
            }),
            signal: controller.signal,
          });
          if (graphRes.ok) {
            results = await extract(graphRes);
          }
        } catch (err) {
          log.debug(`Tier 2 (graph-enhanced) failed: ${err instanceof Error ? err.message : err}`);
        }
      }

      // Tier 3: Basic parallel search (agent-scoped + global)
      if (results.length === 0) {
        const searchBody = (agentId?: string) =>
          JSON.stringify({
            query,
            user_id: getUserId(),
            ...(agentId ? { agent_id: agentId } : {}),
            limit,
          });
        try {
          const [aRes, gRes] = await Promise.all([
            fetch(`${getSidecarUrl()}/search`, {
              method: "POST",
              headers: { "Content-Type": "application/json" },
              body: searchBody(context.nousId),
              signal: controller.signal,
            }),
            fetch(`${getSidecarUrl()}/search`, {
              method: "POST",
              headers: { "Content-Type": "application/json" },
              body: searchBody(),
              signal: controller.signal,
            }),
          ]);
          const agentResults = await extract(aRes);
          const globalResults = await extract(gRes);
          results = [...agentResults, ...globalResults];
        } catch (err) {
          log.debug(`Tier 3 (basic) failed: ${err instanceof Error ? err.message : err}`);
        }
      }

      clearTimeout(timer);

      // Deduplicate by memory id, sort by score, take top `limit`
      const seen = new Set<string>();
      const merged: Record<string, unknown>[] = [];
      const all = results as Record<string, unknown>[];
      for (let i = 0; i < all.length; i++) {
        const m = all[i]!;
        const id = String(m["id"] ?? `${m["memory"] ?? ""}_${i}`);
        if (!seen.has(id)) {
          seen.add(id);
          merged.push(m);
        }
      }

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
