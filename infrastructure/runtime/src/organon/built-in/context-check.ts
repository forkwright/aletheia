// Meta-tool — context snapshot: session + competence + memory status
import type { ToolContext, ToolHandler, ToolRegistry } from "../registry.js";

export function createContextCheckTool(registry: ToolRegistry): ToolHandler {
  return {
    definition: {
      name: "context_check",
      description:
        "Get a snapshot of your current state: session info, competence, and working context.\n\n" +
        "USE WHEN:\n" +
        "- Starting a new task and need to orient yourself\n" +
        "- Checking available context budget before a long operation\n" +
        "- Building situational awareness across session, competence, and memory\n\n" +
        "DO NOT USE WHEN:\n" +
        "- You only need one specific piece (use session_status or check_calibration directly)\n\n" +
        "TIPS:\n" +
        "- Combines session_status + check_calibration into one call\n" +
        "- Saves tool calls vs running them separately",
      input_schema: {
        type: "object",
        properties: {},
      },
    },
    async execute(
      _input: Record<string, unknown>,
      context: ToolContext,
    ): Promise<string> {
      const results: Record<string, unknown> = {};

      // Session status
      try {
        const sessionResult = await registry.execute("session_status", {}, context);
        results["session"] = JSON.parse(sessionResult);
      } catch { /* memory search failed — skip */
        results["session"] = { error: "unavailable" };
      }

      // Calibration
      try {
        const calResult = await registry.execute("check_calibration", {}, context);
        results["calibration"] = JSON.parse(calResult);
      } catch { /* blackboard read failed — skip */
        results["calibration"] = { error: "unavailable" };
      }

      return JSON.stringify(results);
    },
  };
}
