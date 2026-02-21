// Memory audit tool — agents can review and assess their own memories
import { createLogger } from "../../koina/logger.js";
import type { ToolContext, ToolHandler } from "../registry.js";

const log = createLogger("organon.mem0-audit");
const getSidecarUrl = () => process.env["ALETHEIA_MEMORY_URL"] ?? "http://127.0.0.1:8230";
const getUserId = () => process.env["ALETHEIA_MEMORY_USER"] ?? "default";

export const mem0AuditTool: ToolHandler = {
  definition: {
    name: "mem0_audit",
    description:
      "Review memories stored about a topic or list all memories for self-curation.\n\n" +
      "USE WHEN:\n" +
      "- Checking what you know about a topic before acting on it\n" +
      "- Periodically auditing memory quality during self-evaluation\n" +
      "- Before retracting — verify what exists first\n\n" +
      "DO NOT USE WHEN:\n" +
      "- You need to recall memories for a task (use mem0_search instead)\n" +
      "- You want to add a memory (use the normal memory pipeline)\n\n" +
      "TIPS:\n" +
      "- With query: returns semantically similar memories (scored)\n" +
      "- Without query: returns recent memories for inventory\n" +
      "- Follow up with memory_correct, memory_forget, or mem0_retract as needed",
    input_schema: {
      type: "object",
      properties: {
        query: {
          type: "string",
          description: "Optional topic to audit — returns semantically similar memories. Omit for full inventory.",
        },
        limit: {
          type: "number",
          description: "Maximum memories to return (default 20)",
        },
      },
    },
  },
  async execute(
    input: Record<string, unknown>,
    context: ToolContext,
  ): Promise<string> {
    const query = input["query"] as string | undefined;
    const limit = (input["limit"] as number) ?? 20;

    log.info(`Audit requested by ${context.nousId}: query="${query ?? "(inventory)"}", limit=${limit}`);

    try {
      let data: Record<string, unknown>;

      if (query) {
        const res = await fetch(`${getSidecarUrl()}/search`, {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({
            query,
            user_id: getUserId(),
            agent_id: context.nousId,
            limit,
          }),
          signal: AbortSignal.timeout(15000),
        });
        if (!res.ok) return JSON.stringify({ error: `Audit search failed: HTTP ${res.status}` });
        data = (await res.json()) as Record<string, unknown>;
      } else {
        const params = new URLSearchParams({
          user_id: getUserId(),
          agent_id: context.nousId,
          limit: String(limit),
        });
        const res = await fetch(`${getSidecarUrl()}/memories?${params}`, {
          signal: AbortSignal.timeout(15000),
        });
        if (!res.ok) return JSON.stringify({ error: `Audit list failed: HTTP ${res.status}` });
        data = (await res.json()) as Record<string, unknown>;
      }

      return JSON.stringify({
        ...data,
        instructions: "Review each memory for accuracy. Use memory_correct to fix errors, memory_forget to soft-delete, or mem0_retract for full removal including graph nodes.",
      });
    } catch (err) {
      return JSON.stringify({
        error: err instanceof Error ? err.message : String(err),
      });
    }
  },
};
