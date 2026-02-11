// File read tool
import { readFileSync, statSync } from "node:fs";
import { resolve } from "node:path";
import type { ToolHandler, ToolContext } from "../registry.js";

export const readTool: ToolHandler = {
  definition: {
    name: "read",
    description: "Read the contents of a file.",
    input_schema: {
      type: "object",
      properties: {
        path: {
          type: "string",
          description: "File path (absolute or relative to workspace)",
        },
        maxLines: {
          type: "number",
          description: "Maximum lines to return (default: all)",
        },
      },
      required: ["path"],
    },
  },
  async execute(
    input: Record<string, unknown>,
    context: ToolContext,
  ): Promise<string> {
    const filePath = input.path as string;
    const maxLines = input.maxLines as number | undefined;
    const resolved = resolve(context.workspace, filePath);

    try {
      const stat = statSync(resolved);
      if (stat.size > 5 * 1024 * 1024) {
        return `Error: File too large (${(stat.size / 1024 / 1024).toFixed(1)}MB). Use exec with head/tail.`;
      }
      let content = readFileSync(resolved, "utf-8");
      if (maxLines) {
        content = content.split("\n").slice(0, maxLines).join("\n");
      }
      return content;
    } catch (error) {
      const msg =
        error instanceof Error ? error.message : String(error);
      return `Error: ${msg}`;
    }
  },
};
