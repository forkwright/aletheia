// Memory retraction tool — agents can request fact removal
import { createLogger } from "../../koina/logger.js";
import type { ToolContext, ToolHandler } from "../registry.js";

const log = createLogger("organon.fact-retract");
// Lazy reads — env vars may be set by taxis config after module import
const getSidecarUrl = () => process.env["ALETHEIA_MEMORY_URL"] ?? "http://127.0.0.1:8230";
const getUserId = () => process.env["ALETHEIA_MEMORY_USER"] ?? "default";

export const factRetractTool: ToolHandler = {
  definition: {
    name: "fact_retract",
    description:
      "Remove facts from long-term memory (vector store and knowledge graph).\n\n" +
      "USE WHEN:\n" +
      "- Information is outdated or incorrect and should be forgotten\n" +
      "- User requests deletion of specific memories\n" +
      "- Correcting a fact — retract the old one, let the new one be captured naturally\n\n" +
      "DO NOT USE WHEN:\n" +
      "- You're unsure whether the fact is wrong — verify first\n" +
      "- The fact is just old but still accurate\n\n" +
      "TIPS:\n" +
      "- Use dry_run=true first to preview what would be removed\n" +
      "- cascade=true removes connected graph entities — use carefully\n" +
      "- Reason is required and logged for audit trail\n" +
      "- Retraction is permanent — cannot be undone",
    input_schema: {
      type: "object",
      properties: {
        query: {
          type: "string",
          description: "Description of the fact(s) to retract",
        },
        reason: {
          type: "string",
          description: "Why this fact should be retracted (audit trail)",
        },
        cascade: {
          type: "boolean",
          description: "If true, also remove connected entities in the knowledge graph",
        },
        dry_run: {
          type: "boolean",
          description: "If true, show what would be retracted without actually removing",
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

    log.info(`Retraction requested by ${context.nousId}: "${query}" (reason: ${reason}, dry_run: ${dryRun})`);

    try {
      const res = await fetch(`${getSidecarUrl()}/retract`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          query,
          user_id: getUserId(),
          cascade,
          dry_run: dryRun,
          reason: `[${context.nousId}] ${reason}`,
        }),
        signal: AbortSignal.timeout(15000),
      });

      if (!res.ok) {
        return JSON.stringify({ error: `Retraction failed: HTTP ${res.status}` });
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
