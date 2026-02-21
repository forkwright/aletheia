// Memory forget tool — agents can soft-delete memories
import { createLogger } from "../../koina/logger.js";
import type { ToolContext, ToolHandler } from "../registry.js";

const log = createLogger("organon.memory-forget");
const getSidecarUrl = () => process.env["ALETHEIA_MEMORY_URL"] ?? "http://127.0.0.1:8230";
const getUserId = () => process.env["ALETHEIA_MEMORY_USER"] ?? "default";

export const memoryForgetTool: ToolHandler = {
  definition: {
    name: "memory_forget",
    description:
      "Soft-delete memories matching a semantic query. Memories are not physically removed — they're flagged as forgotten and excluded from future recall.\n\n" +
      "USE WHEN:\n" +
      "- Information is no longer relevant and should stop appearing in recall\n" +
      "- User explicitly asks to forget something\n" +
      "- Cleaning up noise that wasn't caught by extraction filters\n\n" +
      "DO NOT USE WHEN:\n" +
      "- A fact is wrong but should be replaced (use memory_correct instead)\n" +
      "- You're unsure whether to remove it — verify with the user first\n\n" +
      "TIPS:\n" +
      "- Use dry_run=true first to preview what would be forgotten\n" +
      "- Default min_score 0.85 is strict — only high-confidence matches are forgotten\n" +
      "- Max 3 memories per call (configurable up to 10)",
    input_schema: {
      type: "object",
      properties: {
        query: {
          type: "string",
          description: "Semantic description of the memory/memories to forget",
        },
        reason: {
          type: "string",
          description: "Why this memory should be forgotten (audit trail)",
        },
        dry_run: {
          type: "boolean",
          description: "Preview what would be forgotten without actually doing it",
        },
        max_deletions: {
          type: "number",
          description: "Maximum number of memories to forget (1-10, default 3)",
        },
        min_score: {
          type: "number",
          description: "Minimum similarity score to match (0.5-1.0, default 0.85)",
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
    const dryRun = (input["dry_run"] as boolean) ?? false;
    const maxDeletions = (input["max_deletions"] as number) ?? 3;
    const minScore = (input["min_score"] as number) ?? 0.85;

    log.info(`Forget requested by ${context.nousId}: "${query}" (reason: ${reason}, dry_run: ${dryRun})`);

    try {
      const res = await fetch(`${getSidecarUrl()}/memory/forget`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          query,
          reason: `[${context.nousId}] ${reason}`,
          user_id: getUserId(),
          max_deletions: maxDeletions,
          min_score: minScore,
          dry_run: dryRun,
        }),
        signal: AbortSignal.timeout(15000),
      });

      if (!res.ok) {
        return JSON.stringify({ error: `Forget failed: HTTP ${res.status}` });
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
