// Shell execution tool
import { exec } from "node:child_process";
import { promisify } from "node:util";
import type { ToolContext, ToolHandler } from "../registry.js";
import { screenCommand } from "../sandbox.js";
import { dockerAvailable, execInDocker } from "../docker-exec.js";
import { createLogger } from "../../koina/logger.js";

const log = createLogger("tool.exec");
const execAsync = promisify(exec);

export const execTool: ToolHandler = {
  definition: {
    name: "exec",
    description:
      "Execute a shell command in your workspace and return stdout/stderr.\n\n" +
      "USE WHEN:\n" +
      "- Running scripts, builds, tests, or system commands\n" +
      "- Installing packages or managing services\n" +
      "- Any operation not covered by dedicated file tools\n\n" +
      "DO NOT USE WHEN:\n" +
      "- Reading files (use read instead)\n" +
      "- Writing files (use write instead)\n" +
      "- Searching files (use grep or find instead)\n\n" +
      "TIPS:\n" +
      "- Working directory is your workspace\n" +
      "- ALETHEIA_NOUS env var is set to your agent ID\n" +
      "- Output truncated at 50KB; use head/tail for large outputs\n" +
      "- Default timeout 30s — increase for long-running commands",
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
    const command = input["command"] as string;
    const timeout = (input["timeout"] as number) ?? 30000;
    const sandbox = context.sandboxConfig;

    // Pre-screen against deny patterns (always runs, even for bypassed agents)
    const extraPatterns = sandbox?.denyPatterns ?? [];
    const screen = screenCommand(command, extraPatterns);
    if (!screen.allowed) {
      if (sandbox?.auditDenied) {
        log.warn(`Denied exec by ${context.nousId}: ${command.slice(0, 200)} (pattern: ${screen.matchedPattern})`);
      }
      return `Error: Command blocked by security policy. Matched pattern: "${screen.matchedPattern}". This command is not allowed.`;
    }

    // Decide execution path: Docker sandbox vs direct
    const useDocker =
      sandbox?.enabled &&
      sandbox.mode === "docker" &&
      !sandbox.bypassFor.includes(context.nousId) &&
      dockerAvailable();

    try {
      let stdout: string;
      let stderr: string;

      if (useDocker) {
        const result = await execInDocker({
          command,
          workspace: context.workspace,
          nousId: context.nousId,
          timeout,
          config: sandbox,
        });
        stdout = result.stdout;
        stderr = result.stderr;
      } else {
        const result = await execAsync(command, {
          cwd: context.workspace,
          timeout,
          maxBuffer: 1024 * 1024,
          env: { ...process.env, ALETHEIA_NOUS: context.nousId },
        });
        stdout = result.stdout;
        stderr = result.stderr;
      }

      let trimmed = stdout.trim();
      if (trimmed.length > 50000) {
        trimmed =
          trimmed.slice(0, 25000) +
          "\n\n... [truncated] ...\n\n" +
          trimmed.slice(-25000);
      }
      const stderrTrimmed = stderr?.trim();
      if (stderrTrimmed) {
        return (trimmed || "(no output)") + `\n[stderr]\n${stderrTrimmed}`;
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
