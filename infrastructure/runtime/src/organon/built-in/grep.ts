// Content search tool — grep through files
import { execFile } from "node:child_process";
import { promisify } from "node:util";
import type { ToolHandler, ToolContext } from "../registry.js";
import { safePath } from "./safe-path.js";

const execFileAsync = promisify(execFile);

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
          description:
            "Maximum matching lines per file (default: 50). Total output is also capped at this limit.",
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
    const pattern = input["pattern"] as string;
    const searchPath = input["path"]
      ? safePath(context.workspace, input["path"] as string)
      : context.workspace;
    const glob = input["glob"] as string | undefined;
    const maxResults = (input["maxResults"] as number) ?? 50;
    const caseSensitive = (input["caseSensitive"] as boolean) ?? true;

    const args = ["--line-number", "--no-heading", "--color=never"];

    if (!caseSensitive) args.push("-i");
    if (glob) args.push("--glob", glob);
    args.push("--max-count", String(maxResults));
    args.push("--", pattern, searchPath);

    try {
      const { stdout } = await execFileAsync("rg", args, {
        timeout: 10000,
        maxBuffer: 512 * 1024,
      });
      const trimmed = stdout.trim();
      if (!trimmed) return "No matches found";

      const lines = trimmed.split("\n");
      if (lines.length > maxResults) {
        return (
          lines.slice(0, maxResults).join("\n") +
          `\n... (${lines.length - maxResults} more)`
        );
      }
      return trimmed;
    } catch (error) {
      const err = error as { code?: number | string };
      if (err.code === 1) return "No matches found";
      const msg = error instanceof Error ? error.message : String(error);
      return `Error: ${msg}`;
    }
  },
};
