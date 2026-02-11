// File write tool
import { writeFileSync, mkdirSync } from "node:fs";
import { resolve, dirname } from "node:path";
import type { ToolHandler, ToolContext } from "../registry.js";

export const writeTool: ToolHandler = {
  definition: {
    name: "write",
    description: "Write content to a file, creating directories as needed.",
    input_schema: {
      type: "object",
      properties: {
        path: {
          type: "string",
          description: "File path (absolute or relative to workspace)",
        },
        content: {
          type: "string",
          description: "Content to write",
        },
        append: {
          type: "boolean",
          description: "Append instead of overwrite (default: false)",
        },
      },
      required: ["path", "content"],
    },
  },
  async execute(
    input: Record<string, unknown>,
    context: ToolContext,
  ): Promise<string> {
    const filePath = input.path as string;
    const content = input.content as string;
    const append = (input.append as boolean) ?? false;
    const resolved = resolve(context.workspace, filePath);

    try {
      mkdirSync(dirname(resolved), { recursive: true });
      writeFileSync(resolved, content, {
        flag: append ? "a" : "w",
        encoding: "utf-8",
      });
      return `Written to ${filePath}`;
    } catch (error) {
      const msg =
        error instanceof Error ? error.message : String(error);
      return `Error: ${msg}`;
    }
  },
};
