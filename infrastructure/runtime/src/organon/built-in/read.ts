// File read tool
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import type { ToolContext, ToolHandler } from "../registry.js";

export const readTool: ToolHandler = {
  definition: {
    name: "read",
    description:
      "Read a file's contents as text.\n\n" +
      "USE WHEN:\n" +
      "- You need to see what's in a file before editing\n" +
      "- Checking configuration, logs, or data files\n" +
      "- Verifying the result of a write or edit\n\n" +
      "DO NOT USE WHEN:\n" +
      "- File is binary (images, executables) — returns error\n" +
      "- File is > 5MB — use exec with head/tail instead\n\n" +
      "TIPS:\n" +
      "- Paths can be absolute or relative to workspace\n" +
      "- Use maxLines to preview large files without loading everything",
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
    const filePath = input["path"] as string;
    const maxLines = input["maxLines"] as number | undefined;
    const resolved = resolve(context.workspace, filePath);

    try {
      // Read atomically — no stat-then-read race
      const buf = readFileSync(resolved);
      if (buf.length > 5 * 1024 * 1024) {
        return `Error: File too large (${(buf.length / 1024 / 1024).toFixed(1)}MB). Use exec with head/tail.`;
      }
      // Check for null bytes (binary indicator)
      for (let i = 0; i < Math.min(buf.length, 8192); i++) {
        if (buf[i] === 0x00) return "Error: Binary file detected — cannot display";
      }
      let content = buf.toString("utf-8");
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
