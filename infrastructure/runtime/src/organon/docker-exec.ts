// Docker sandbox execution for the exec tool
import { exec, execSync } from "node:child_process";
import { promisify } from "node:util";
import { createLogger } from "../koina/logger.js";
import type { SandboxSettings } from "../taxis/schema.js";

const log = createLogger("sandbox.docker");
const execAsync = promisify(exec);

let dockerChecked = false;
let dockerOk = false;

export function dockerAvailable(): boolean {
  if (dockerChecked) return dockerOk;
  dockerChecked = true;
  try {
    execSync("docker info", { timeout: 5000, stdio: "ignore" });
    dockerOk = true;
    log.info("Docker available for sandbox execution");
  } catch { /* docker not available */
    dockerOk = false;
    log.warn("Docker not available — sandbox will use pattern-only mode");
  }
  return dockerOk;
}

export function resetDockerCheck(): void {
  dockerChecked = false;
  dockerOk = false;
}

export interface DockerExecOpts {
  command: string;
  workspace: string;
  nousId: string;
  timeout: number;
  config: SandboxSettings;
}

export async function execInDocker(opts: DockerExecOpts): Promise<{ stdout: string; stderr: string }> {
  const { command, workspace, nousId, timeout, config } = opts;

  const args: string[] = [
    "docker", "run", "--rm",
    "--read-only",
    "--user", "1000:1000",
    "--memory", config.memoryLimit,
    "--cpus", String(config.cpuLimit),
    "--env", `ALETHEIA_NOUS=${nousId}`,
  ];

  // Network policy
  if (!config.allowNetwork) {
    args.push("--network", "none");
  }

  // Workspace mount
  const mountMode = config.mountWorkspace === "readwrite" ? "rw" : "ro";
  args.push("-v", `${workspace}:${workspace}:${mountMode}`);
  args.push("-w", workspace);

  // Writable tmpdir for programs that need it
  args.push("--tmpfs", "/tmp:rw,noexec,nosuid,size=64m");

  args.push(config.image);
  args.push("sh", "-c", command);

  const dockerCommand = args.map((a) => (a.includes(" ") ? `"${a}"` : a)).join(" ");
  log.debug(`Docker exec: ${dockerCommand.slice(0, 200)}`);

  return execAsync(dockerCommand, {
    timeout,
    maxBuffer: 1024 * 1024,
  });
}
