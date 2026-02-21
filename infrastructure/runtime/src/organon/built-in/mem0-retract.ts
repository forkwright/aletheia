// Memory retraction tool — agents can retract incorrect/stale memories
import { createLogger } from "../../koina/logger.js";
import type { ToolContext, ToolHandler } from "../registry.js";

const log = createLogger("organon.mem0-retract");
const getSidecarUrl = () => process.env["ALETHEIA_MEMORY_URL"] ?? "http://127.0.0.1:8230";
const getUserId = () => process.env["ALETHEIA_MEMORY_USER"] ?? "default";

export const mem0RetractTool: ToolHandler = {
  definition: {
    name: "mem0_retract",
    description:
      "Retract memories matching a semantic query — removes from both vector store and knowledge graph.\n\n" +
      "USE WHEN:\n" +
      "- A stored memory is factually wrong and should be fully removed\n" +
      "- Outdated information keeps surfacing in recall\n" +
      "- You've verified via mem0_audit that specific memories need deletion\n\n" +
      "DO NOT USE WHEN:\n" +
      "- A fact needs correction but not deletion (use memory_correct instead)\n" +
      "- You're unsure what to retract — use mem0_audit first to review\n\n" +
      "TIPS:\n" +
      "- Use dry_run=true first to preview what would be retracted\n" +
      "- cascade=true removes related Neo4j graph nodes\n" +
      "- Only memories scoring >0.75 similarity are matched",
    input_schema: {
      type: "object",
      properties: {
        query: {
          type: "string",
          description: "Semantic description of the memory/memories to retract",
        },
        reason: {
          type: "string",
          description: "Why these memories should be retracted (audit trail)",
        },
        cascade: {
          type: "boolean",
          description: "Also remove related graph nodes in Neo4j (default false)",
        },
        dry_run: {
          type: "boolean",
          description: "Preview what would be retracted without actually doing it (default false)",
        },
      },
      required: ["query", "reason"],
    },
  },
  async execute(
    input: Record<string, unknown>,
    context: ToolContext,
  ): Promise<string> {
    const query = input["query"] as string;
    const reason = input["reason"] as string;
    const cascade = (input["cascade"] as boolean) ?? false;
    const dryRun = (input["dry_run"] as boolean) ?? false;

    log.info(`Retract requested by ${context.nousId}: "${query}" (reason: ${reason}, dry_run: ${dryRun}, cascade: ${cascade})`);

    try {
      const res = await fetch(`${getSidecarUrl()}/retract`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          query,
          reason: `[${context.nousId}] ${reason}`,
          user_id: getUserId(),
          cascade,
          dry_run: dryRun,
        }),
        signal: AbortSignal.timeout(15000),
      });

      if (!res.ok) {
        return JSON.stringify({ error: `Retract failed: HTTP ${res.status}` });
      }

      const data = (await res.json()) as Record<string, unknown>;
      return JSON.stringify(data);
    } catch (err) {
      return JSON.stringify({
        error: err instanceof Error ? err.message : String(err),
      });
    }
  },
};
