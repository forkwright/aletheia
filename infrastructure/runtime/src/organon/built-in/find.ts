// File search tool — find files by name pattern
import { execFile } from "node:child_process";
import { promisify } from "node:util";
import type { ToolHandler, ToolContext } from "../registry.js";
import { safePath } from "./safe-path.js";

const execFileAsync = promisify(execFile);

export const findTool: ToolHandler = {
  definition: {
    name: "find",
    description:
      "Find files by name pattern. Uses fd for fast file searching.",
    input_schema: {
      type: "object",
      properties: {
        pattern: {
          type: "string",
          description: "File name pattern (glob or regex)",
        },
        path: {
          type: "string",
          description: "Directory to search (default: workspace root)",
        },
        type: {
          type: "string",
          description: "Filter by type: 'f' for files, 'd' for directories",
        },
        maxDepth: {
          type: "number",
          description: "Maximum directory depth to search",
        },
        maxResults: {
          type: "number",
          description: "Maximum results to return (default: 100)",
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
    const type = input["type"] as string | undefined;
    const maxDepth = input["maxDepth"] as number | undefined;
    const maxResults = (input["maxResults"] as number) ?? 100;

    const args = ["--color=never"];

    if (type === "f") args.push("--type", "f");
    else if (type === "d") args.push("--type", "d");
    if (maxDepth) args.push("--max-depth", String(maxDepth));
    args.push("--max-results", String(maxResults));
    args.push(pattern, searchPath);

    try {
      const { stdout } = await execFileAsync("fd", args, {
        timeout: 10000,
        maxBuffer: 512 * 1024,
      });
      const trimmed = stdout.trim();
      return trimmed || "No files found";
    } catch (error) {
      const err = error as { code?: number | string };
      if (err.code === 1) return "No files found";
      const msg = error instanceof Error ? error.message : String(error);
      return `Error: ${msg}`;
    }
  },
};
