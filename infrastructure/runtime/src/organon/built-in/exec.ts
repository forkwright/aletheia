// Shell execution tool
import { exec } from "node:child_process";
import { promisify } from "node:util";
import type { ToolHandler, ToolContext } from "../registry.js";

const execAsync = promisify(exec);

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
      const { stdout } = await execAsync(command, {
        cwd: context.workspace,
        timeout,
        maxBuffer: 1024 * 1024,
        env: { ...process.env, ALETHEIA_NOUS: context.nousId },
      });
      const trimmed = stdout.trim();
      if (trimmed.length > 50000) {
        return (
          trimmed.slice(0, 25000) +
          "\n\n... [truncated] ...\n\n" +
          trimmed.slice(-25000)
        );
      }
      return trimmed || "(no output)";
    } catch (error) {
      const execErr = error as {
        stdout?: string;
        stderr?: string;
        message?: string;
      };
      const output =
        execErr.stdout?.trim() || execErr.stderr?.trim() || execErr.message;
      return `Error: ${output}`;
    }
  },
};
