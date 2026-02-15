// Signal-cli daemon spawn and lifecycle management
import { spawn } from "node:child_process";
import { createLogger } from "../koina/logger.js";
import type { SignalAccount } from "../taxis/schema.js";

const log = createLogger("semeion:daemon");

export interface DaemonHandle {
  pid: number;
  baseUrl: string;
  stop: () => void;
}

export interface DaemonOpts {
  account: string;
  cliPath?: string;
  httpHost?: string;
  httpPort?: number;
  receiveMode?: string;
  sendReadReceipts?: boolean;
  ignoreAttachments?: boolean;
  ignoreStories?: boolean;
}

export function spawnDaemon(opts: DaemonOpts): DaemonHandle {
  const cliPath = opts.cliPath ?? "signal-cli";
  const host = opts.httpHost ?? "127.0.0.1";
  const port = opts.httpPort ?? 8080;

  const args = [
    "-a",
    opts.account,
    "daemon",
    "--http",
    `${host}:${port}`,
    "--no-receive-stdout",
  ];

  if (opts.receiveMode) {
    args.push("--receive-mode", opts.receiveMode);
  }
  if (opts.sendReadReceipts) {
    args.push("--send-read-receipts");
  }
  if (opts.ignoreAttachments) {
    args.push("--ignore-attachments");
  }
  if (opts.ignoreStories) {
    args.push("--ignore-stories");
  }

  log.info(`Spawning signal-cli daemon: ${cliPath} ${args.join(" ")}`);

  const child = spawn(cliPath, args, {
    stdio: ["ignore", "pipe", "pipe"],
    detached: false,
  });

  child.stdout?.on("data", (data: Buffer) => {
    const line = data.toString().trim();
    if (line) log.debug(`[signal-cli] ${line}`);
  });

  child.stderr?.on("data", (data: Buffer) => {
    const line = data.toString().trim();
    if (!line) return;

    if (/error|warn|failed|severe|exception/i.test(line)) {
      log.warn(`[signal-cli] ${line}`);
    } else {
      log.debug(`[signal-cli] ${line}`);
    }
  });

  child.on("exit", (code, signal) => {
    log.info(`signal-cli daemon exited: code=${code} signal=${signal}`);
  });

  child.on("error", (err) => {
    log.error(`signal-cli daemon spawn error: ${err.message}`);
  });

  if (!child.pid) {
    throw new Error("Failed to spawn signal-cli daemon — no PID");
  }

  const baseUrl = `http://${host}:${port}`;
  log.info(`Daemon spawned pid=${child.pid} at ${baseUrl}`);

  return {
    pid: child.pid,
    baseUrl,
    stop: () => {
      log.info(`Stopping signal-cli daemon pid=${child.pid}`);
      child.kill("SIGTERM");
    },
  };
}

export async function waitForReady(
  baseUrl: string,
  timeoutMs = 30000,
  intervalMs = 500,
): Promise<void> {
  const deadline = Date.now() + timeoutMs;
  const healthUrl = `${baseUrl}/api/v1/check`;

  while (Date.now() < deadline) {
    try {
      const res = await fetch(healthUrl, { signal: AbortSignal.timeout(2000) });
      if (res.ok) {
        log.info("signal-cli daemon ready");
        return;
      }
    } catch {
      // not ready yet
    }
    await new Promise((r) => setTimeout(r, intervalMs));
  }

  throw new Error(`signal-cli daemon not ready after ${timeoutMs}ms`);
}

export function daemonOptsFromConfig(
  accountId: string,
  account: SignalAccount,
): DaemonOpts {
  return {
    account: account.account ?? accountId,
    cliPath: account.cliPath,
    httpHost: account.httpHost,
    httpPort: account.httpPort,
    receiveMode: account.receiveMode,
    sendReadReceipts: account.sendReadReceipts,
  };
}
