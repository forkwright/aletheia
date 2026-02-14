// Shell execution tool
import { execSync } from "node:child_process";
import type { ToolHandler, ToolContext } from "../registry.js";

export const execTool: ToolHandler = {
  definition: {
    name: "exec",
    description:
      "Execute a shell command and return its output. Use for system operations, file management, and running scripts.",
    input_schema: {
      type: "object",
      properties: {
        command: {
          type: "string",
          description: "The shell command to execute",
        },
        timeout: {
          type: "number",
          description: "Timeout in milliseconds (default 30000)",
        },
      },
      required: ["command"],
    },
  },
  async execute(
    input: Record<string, unknown>,
    context: ToolContext,
  ): Promise<string> {
    const command = input.command as string;
    const timeout = (input.timeout as number) ?? 30000;

    try {
      const output = execSync(command, {
        cwd: context.workspace,
        timeout,
        maxBuffer: 1024 * 1024,
        encoding: "utf-8",
        env: { ...process.env, ALETHEIA_NOUS: context.nousId },
      });
      const trimmed = output.trim();
      if (trimmed.length > 50000) {
        return (
          trimmed.slice(0, 25000) +
          "\n\n... [truncated] ...\n\n" +
          trimmed.slice(-25000)
        );
      }
      return trimmed || "(no output)";
    } catch (error) {
      const msg =
        error instanceof Error ? error.message : String(error);
      return `Error: ${msg}`;
    }
  },
};
