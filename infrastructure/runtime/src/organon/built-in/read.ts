// File read tool
import { readFileSync, statSync, openSync, readSync, closeSync } from "node:fs";
import type { ToolHandler, ToolContext } from "../registry.js";
import { safePath } from "./safe-path.js";

function isBinaryFile(filePath: string): boolean {
  let fd: number | undefined;
  try {
    fd = openSync(filePath, "r");
    const buf = Buffer.alloc(8192);
    const bytesRead = readSync(fd, buf, 0, 8192, 0);
    for (let i = 0; i < bytesRead; i++) {
      if (buf[i] === 0x00) return true;
    }
    return false;
  } catch {
    return false;
  } finally {
    if (fd !== undefined) closeSync(fd);
  }
}

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
    const resolved = safePath(context.workspace, filePath);

    try {
      const stat = statSync(resolved);
      if (stat.size > 5 * 1024 * 1024) {
        return `Error: File too large (${(stat.size / 1024 / 1024).toFixed(1)}MB). Use exec with head/tail.`;
      }
      if (isBinaryFile(resolved)) {
        return "Error: Binary file detected — cannot display";
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
