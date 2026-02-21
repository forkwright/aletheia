// File edit tool — find and replace
import { readFileSync, writeFileSync } from "node:fs";
import type { ToolContext, ToolHandler } from "../registry.js";
import { safePath } from "./safe-path.js";
import { commitWorkspaceChange } from "../workspace-git.js";
import { trySafe } from "../../koina/safe.js";

export const editTool: ToolHandler = {
  definition: {
    name: "edit",
    description:
      "Replace exact text in a file with new text. Requires a unique match.\n\n" +
      "USE WHEN:\n" +
      "- Modifying specific sections of an existing file\n" +
      "- Updating configuration values, function bodies, or specific lines\n" +
      "- Making targeted changes without rewriting the entire file\n\n" +
      "DO NOT USE WHEN:\n" +
      "- Creating new files (use write instead)\n" +
      "- The old_text appears more than once — include surrounding context to make it unique\n\n" +
      "TIPS:\n" +
      "- old_text must match EXACTLY including whitespace and newlines\n" +
      "- If match is ambiguous, include more surrounding lines\n" +
      "- Read the file first to get exact text to match\n" +
      "- Changes are tracked in workspace git",
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
    const filePath = input["path"] as string;
    const oldText = input["old_text"] as string;
    const newText = input["new_text"] as string;
    const resolved = safePath(context.workspace, filePath, context.allowedRoots);

    if (oldText === "") {
      return "Error: old_text cannot be empty — it would match everywhere";
    }

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

      trySafe("workspace git commit", () => commitWorkspaceChange(context.workspace, resolved, "edit"), undefined);
      return `Edited ${filePath}: replaced ${oldText.length} chars with ${newText.length} chars`;
    } catch (error) {
      const msg = error instanceof Error ? error.message : String(error);
      return `Error: ${msg}`;
    }
  },
};
