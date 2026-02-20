// File write tool
import { mkdirSync, writeFileSync } from "node:fs";
import { dirname } from "node:path";
import type { ToolContext, ToolHandler } from "../registry.js";
import { safePath } from "./safe-path.js";
import { trySafe } from "../../koina/safe.js";
import { commitWorkspaceChange } from "../workspace-git.js";

export const writeTool: ToolHandler = {
  definition: {
    name: "write",
    description:
      "Write content to a file, creating parent directories as needed.\n\n" +
      "USE WHEN:\n" +
      "- Creating new files\n" +
      "- Replacing entire file contents\n" +
      "- Appending to logs or data files (set append=true)\n\n" +
      "DO NOT USE WHEN:\n" +
      "- Making surgical edits to existing files (use edit instead)\n" +
      "- You haven't read the file first and need to preserve existing content\n\n" +
      "TIPS:\n" +
      "- Overwrites by default — use append=true to add to end\n" +
      "- Automatically creates missing directories\n" +
      "- Changes are tracked in workspace git",
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
    const filePath = input["path"] as string;
    const content = input["content"] as string;
    const append = (input["append"] as boolean) ?? false;
    const resolved = safePath(context.workspace, filePath, context.allowedRoots);

    try {
      mkdirSync(dirname(resolved), { recursive: true });
      // codeql[js/insecure-temporary-file] - path is workspace-constrained via safePath, not tmpdir
      writeFileSync(resolved, content, {
        flag: append ? "a" : "w",
        encoding: "utf-8",
      });
      trySafe("workspace git commit", () => commitWorkspaceChange(context.workspace, resolved, append ? "append" : "write"), undefined);
      return `Written to ${filePath}`;
    } catch (error) {
      const msg =
        error instanceof Error ? error.message : String(error);
      return `Error: ${msg}`;
    }
  },
};
