// Memory correction tool — agents can correct existing memories
import { createLogger } from "../../koina/logger.js";
import type { ToolContext, ToolHandler } from "../registry.js";

const log = createLogger("organon.memory-correct");
const getSidecarUrl = () => process.env["ALETHEIA_MEMORY_URL"] ?? "http://127.0.0.1:8230";
const getUserId = () => process.env["ALETHEIA_MEMORY_USER"] ?? "default";

export const memoryCorrectTool: ToolHandler = {
  definition: {
    name: "memory_correct",
    description:
      "Correct an existing memory — finds the old version by semantic search, supersedes it, and stores the corrected version.\n\n" +
      "USE WHEN:\n" +
      "- A previously stored fact is now known to be wrong\n" +
      "- Information has been updated (e.g., torque spec changed)\n" +
      "- User explicitly corrects a prior belief\n\n" +
      "DO NOT USE WHEN:\n" +
      "- Adding a new fact (just let turn extraction handle it)\n" +
      "- Removing without replacement (use memory_forget instead)\n\n" +
      "TIPS:\n" +
      "- The query should describe the OLD fact you want to replace\n" +
      "- corrected_text is the NEW, correct version\n" +
      "- Old memory gets confidence dropped to 0.1, new one starts at 0.95",
    input_schema: {
      type: "object",
      properties: {
        query: {
          type: "string",
          description: "Description of the old/incorrect fact to find and correct",
        },
        corrected_text: {
          type: "string",
          description: "The corrected version of the fact",
        },
        reason: {
          type: "string",
          description: "Why this correction is being made (audit trail)",
        },
      },
      required: ["query", "corrected_text", "reason"],
    },
  },
  async execute(
    input: Record<string, unknown>,
    context: ToolContext,
  ): Promise<string> {
    const query = input["query"] as string;
    const correctedText = input["corrected_text"] as string;
    const reason = input["reason"] as string;

    log.info(`Correction requested by ${context.nousId}: "${query}" → "${correctedText.slice(0, 80)}" (reason: ${reason})`);

    try {
      const res = await fetch(`${getSidecarUrl()}/memory/correct`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          query,
          corrected_text: correctedText,
          reason: `[${context.nousId}] ${reason}`,
          user_id: getUserId(),
          agent_id: context.nousId,
        }),
        signal: AbortSignal.timeout(15000),
      });

      if (!res.ok) {
        return JSON.stringify({ error: `Correction failed: HTTP ${res.status}` });
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
