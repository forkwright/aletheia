// Directory listing tool
import { readdirSync, statSync } from "node:fs";
import { join, resolve } from "node:path";
import type { ToolContext, ToolHandler } from "../registry.js";

export const lsTool: ToolHandler = {
  definition: {
    name: "ls",
    description:
      "List directory contents with size and modification time.\n\n" +
      "USE WHEN:\n" +
      "- Checking what files exist in a directory\n" +
      "- Verifying file sizes or last-modified times\n" +
      "- Getting a quick overview of project structure\n\n" +
      "DO NOT USE WHEN:\n" +
      "- Searching for files by name across directories — use find instead\n" +
      "- Looking at file contents — use read instead\n\n" +
      "TIPS:\n" +
      "- Hidden files excluded by default — set all=true to include\n" +
      "- Directories shown with trailing /\n" +
      "- Path defaults to workspace root",
    input_schema: {
      type: "object",
      properties: {
        path: {
          type: "string",
          description: "Directory to list (default: workspace root)",
        },
        all: {
          type: "boolean",
          description: "Include hidden files (default: false)",
        },
      },
    },
  },
  async execute(
    input: Record<string, unknown>,
    context: ToolContext,
  ): Promise<string> {
    const dirPath = input["path"]
      ? resolve(context.workspace, input["path"] as string)
      : context.workspace;
    const showAll = (input["all"] as boolean) ?? false;

    try {
      const entries = readdirSync(dirPath, { withFileTypes: true });
      const lines: string[] = [];

      for (const entry of entries) {
        if (!showAll && entry.name.startsWith(".")) continue;

        const fullPath = join(dirPath, entry.name);
        try {
          const stat = statSync(fullPath);
          const isDir = entry.isDirectory();
          const size = isDir ? "-" : formatSize(stat.size);
          const modified = stat.mtime.toISOString().slice(0, 16).replace("T", " ");
          const suffix = isDir ? "/" : "";
          lines.push(`${modified}  ${size.padStart(8)}  ${entry.name}${suffix}`);
        } catch {
          lines.push(`${"?".padStart(17)}  ${"?".padStart(8)}  ${entry.name}`);
        }
      }

      return lines.length > 0
        ? lines.join("\n")
        : "(empty directory)";
    } catch (error) {
      const msg = error instanceof Error ? error.message : String(error);
      return `Error: ${msg}`;
    }
  },
};

function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes}B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)}K`;
  if (bytes < 1024 * 1024 * 1024)
    return `${(bytes / (1024 * 1024)).toFixed(1)}M`;
  return `${(bytes / (1024 * 1024 * 1024)).toFixed(1)}G`;
}
