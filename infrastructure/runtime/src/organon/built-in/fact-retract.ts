// Memory retraction tool — agents can request fact removal
import { createLogger } from "../../koina/logger.js";
import type { ToolHandler, ToolContext } from "../registry.js";

const log = createLogger("organon.fact-retract");
const SIDECAR_URL = process.env["ALETHEIA_MEMORY_URL"] ?? "http://127.0.0.1:8230";
const USER_ID = process.env["ALETHEIA_MEMORY_USER"] ?? "ck";

export const factRetractTool: ToolHandler = {
  definition: {
    name: "fact_retract",
    description:
      "Request retraction of facts from long-term memory. " +
      "Searches for matching memories and removes them from both vector store and knowledge graph. " +
      "Use when information is outdated, incorrect, or needs to be forgotten.",
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
      const res = await fetch(`${SIDECAR_URL}/retract`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          query,
          user_id: USER_ID,
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
