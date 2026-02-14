// Content search tool — grep through files
import { execSync } from "node:child_process";
import { resolve } from "node:path";
import type { ToolHandler, ToolContext } from "../registry.js";

export const grepTool: ToolHandler = {
  definition: {
    name: "grep",
    description:
      "Search file contents using ripgrep. Returns matching lines with file paths and line numbers.",
    input_schema: {
      type: "object",
      properties: {
        pattern: {
          type: "string",
          description: "Search pattern (regex supported)",
        },
        path: {
          type: "string",
          description: "Directory or file to search (default: workspace root)",
        },
        glob: {
          type: "string",
          description: "File glob filter (e.g., '*.ts', '*.md')",
        },
        maxResults: {
          type: "number",
          description: "Maximum results to return (default: 50)",
        },
        caseSensitive: {
          type: "boolean",
          description: "Case-sensitive search (default: true)",
        },
      },
      required: ["pattern"],
    },
  },
  async execute(
    input: Record<string, unknown>,
    context: ToolContext,
  ): Promise<string> {
    const pattern = input.pattern as string;
    const searchPath = input.path
      ? resolve(context.workspace, input.path as string)
      : context.workspace;
    const glob = input.glob as string | undefined;
    const maxResults = (input.maxResults as number) ?? 50;
    const caseSensitive = (input.caseSensitive as boolean) ?? true;

    const args = ["rg", "--line-number", "--no-heading", "--color=never"];

    if (!caseSensitive) args.push("-i");
    if (glob) args.push("--glob", glob);
    args.push("--max-count", String(maxResults));
    args.push("--", pattern, searchPath);

    try {
      const output = execSync(args.join(" "), {
        timeout: 10000,
        maxBuffer: 512 * 1024,
        encoding: "utf-8",
      });
      const trimmed = output.trim();
      if (!trimmed) return "No matches found";

      const lines = trimmed.split("\n");
      if (lines.length > maxResults) {
        return lines.slice(0, maxResults).join("\n") + `\n... (${lines.length - maxResults} more)`;
      }
      return trimmed;
    } catch (error) {
      if (error && typeof error === "object" && "status" in error && (error as { status: number }).status === 1) {
        return "No matches found";
      }
      const msg = error instanceof Error ? error.message : String(error);
      return `Error: ${msg}`;
    }
  },
};
