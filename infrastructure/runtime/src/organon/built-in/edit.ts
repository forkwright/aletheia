// File edit tool — find and replace
import { readFileSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import type { ToolHandler, ToolContext } from "../registry.js";

export const editTool: ToolHandler = {
  definition: {
    name: "edit",
    description:
      "Edit a file by replacing exact text. Provide the old text to find and new text to replace it with.",
    input_schema: {
      type: "object",
      properties: {
        path: {
          type: "string",
          description: "File path (absolute or relative to workspace)",
        },
        old_text: {
          type: "string",
          description: "Exact text to find in the file",
        },
        new_text: {
          type: "string",
          description: "Replacement text",
        },
      },
      required: ["path", "old_text", "new_text"],
    },
  },
  async execute(
    input: Record<string, unknown>,
    context: ToolContext,
  ): Promise<string> {
    const filePath = input.path as string;
    const oldText = input.old_text as string;
    const newText = input.new_text as string;
    const resolved = resolve(context.workspace, filePath);

    try {
      const content = readFileSync(resolved, "utf-8");
      const idx = content.indexOf(oldText);

      if (idx === -1) {
        return "Error: old_text not found in file";
      }

      if (content.indexOf(oldText, idx + 1) !== -1) {
        return "Error: old_text matches multiple locations. Provide more context to make it unique.";
      }

      const updated = content.slice(0, idx) + newText + content.slice(idx + oldText.length);
      writeFileSync(resolved, updated, "utf-8");

      return `Edited ${filePath}: replaced ${oldText.length} chars with ${newText.length} chars`;
    } catch (error) {
      const msg = error instanceof Error ? error.message : String(error);
      return `Error: ${msg}`;
    }
  },
};
