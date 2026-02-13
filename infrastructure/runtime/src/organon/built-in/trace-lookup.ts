// Introspective debugging — agents inspect their own reasoning provenance
import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";
import type { ToolHandler, ToolContext } from "../registry.js";

export const traceLookupTool: ToolHandler = {
  definition: {
    name: "trace_lookup",
    description:
      "Inspect your own reasoning trace from recent turns. Shows what bootstrap files " +
      "were loaded, which tools were called (with timing), any cross-agent calls, and " +
      "token usage. Use for self-debugging and understanding your own decision provenance.",
    input_schema: {
      type: "object",
      properties: {
        turns_back: {
          type: "number",
          description: "How many turns back to look (default 5, max 20)",
        },
        filter: {
          type: "string",
          description: "Optional filter: 'tools', 'cross-agent', 'usage', 'all' (default: all)",
        },
      },
    },
  },
  async execute(
    input: Record<string, unknown>,
    context: ToolContext,
  ): Promise<string> {
    const turnsBack = Math.min((input["turns_back"] as number) ?? 5, 20);
    const filter = (input["filter"] as string) ?? "all";

    // Find traces file for this agent
    const workspace = context.workspace ?? "";
    const tracesFile = join(workspace, "..", "..", "shared", "traces", `${context.nousId}.jsonl`);

    if (!existsSync(tracesFile)) {
      return JSON.stringify({ error: "No traces found", file: tracesFile });
    }

    const content = readFileSync(tracesFile, "utf-8");
    const lines = content.trim().split("\n").filter(Boolean);
    const recent = lines.slice(-turnsBack);

    const traces = recent.map((line) => {
      try {
        return JSON.parse(line);
      } catch {
        return null;
      }
    }).filter(Boolean);

    if (traces.length === 0) {
      return JSON.stringify({ error: "No valid traces found" });
    }

    // Apply filter
    const filtered = traces.map((t: Record<string, unknown>) => {
      const base = {
        turnSeq: t["turnSeq"],
        model: t["model"],
        timestamp: t["startedAt"],
        durationMs: t["durationMs"],
      };

      if (filter === "tools" || filter === "all") {
        Object.assign(base, {
          toolCalls: (t["toolCalls"] as Record<string, unknown>[])?.map((tc) => ({
            name: tc["name"],
            durationMs: tc["durationMs"],
            isError: tc["isError"],
            inputPreview: String(JSON.stringify(tc["input"]) ?? "").slice(0, 100),
          })),
        });
      }

      if (filter === "cross-agent" || filter === "all") {
        Object.assign(base, {
          crossAgentCalls: t["crossAgentCalls"],
        });
      }

      if (filter === "usage" || filter === "all") {
        Object.assign(base, {
          usage: t["usage"],
          bootstrapFiles: t["bootstrapFiles"],
          degradedServices: t["degradedServices"],
        });
      }

      return base;
    });

    return JSON.stringify({
      nousId: context.nousId,
      tracesFound: filtered.length,
      traces: filtered,
    });
  },
};
