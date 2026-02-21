// Signal-cli daemon spawn and lifecycle management
import { spawn } from "node:child_process";
import { createLogger } from "../koina/logger.js";
import { TransportError } from "../koina/errors.js";
import type { SignalAccount } from "../taxis/schema.js";

const log = createLogger("semeion:daemon");

export interface DaemonHandle {
  pid: number;
  baseUrl: string;
  healthy: boolean;
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

const MAX_RESTART_ATTEMPTS = 5;
const RESTART_BACKOFF_MS = 2000;

export function spawnDaemon(opts: DaemonOpts): DaemonHandle {
  const cliPath = opts.cliPath ?? "signal-cli";
  const host = opts.httpHost ?? "127.0.0.1";
  const port = opts.httpPort ?? 8080;
  const baseUrl = `http://${host}:${port}`;

  const handle: DaemonHandle = {
    pid: 0,
    baseUrl,
    healthy: false,
    stop: () => {},
  };

  let stopped = false;
  let restartAttempts = 0;

  function buildArgs(): string[] {
    const args = [
      "-a",
      opts.account,
      "daemon",
      "--http",
      `${host}:${port}`,
      "--no-receive-stdout",
    ];

    if (opts.receiveMode) args.push("--receive-mode", opts.receiveMode);
    if (opts.sendReadReceipts) args.push("--send-read-receipts");
    if (opts.ignoreAttachments) args.push("--ignore-attachments");
    if (opts.ignoreStories) args.push("--ignore-stories");

    return args;
  }

  function launch(): void {
    const args = buildArgs();
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
      handle.healthy = false;

      if (stopped) return;

      if (code !== 0 && code !== null) {
        restartAttempts++;
        if (restartAttempts > MAX_RESTART_ATTEMPTS) {
          log.error(`signal-cli daemon exceeded ${MAX_RESTART_ATTEMPTS} restart attempts, giving up`);
          return;
        }
        const delay = RESTART_BACKOFF_MS * restartAttempts;
        log.info(`Restarting signal-cli daemon in ${delay}ms (attempt ${restartAttempts}/${MAX_RESTART_ATTEMPTS})`);
        setTimeout(() => {
          if (!stopped) launch();
        }, delay);
      }
    });

    child.on("error", (err) => {
      log.error(`signal-cli daemon spawn error: ${err.message}`);
      handle.healthy = false;
    });

    if (!child.pid) {
      throw new TransportError("Failed to spawn signal-cli daemon — no PID", { code: "SIGNAL_DAEMON_DOWN" });
    }

    handle.pid = child.pid;
    handle.healthy = true;
    restartAttempts = 0;

    handle.stop = () => {
      stopped = true;
      handle.healthy = false;
      log.info(`Stopping signal-cli daemon pid=${child.pid}`);
      child.kill("SIGTERM");
    };

    log.info(`Daemon spawned pid=${child.pid} at ${baseUrl}`);
  }

  launch();
  return handle;
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

  throw new TransportError(`signal-cli daemon not ready after ${timeoutMs}ms`, {
    code: "SIGNAL_DAEMON_DOWN", recoverable: true, retryAfterMs: 10_000,
    context: { timeoutMs },
  });
}

export function daemonOptsFromConfig(
  accountId: string,
  account: SignalAccount,
): DaemonOpts {
  const opts: DaemonOpts = {
    account: account.account ?? accountId,
    httpHost: account.httpHost,
    httpPort: account.httpPort,
    receiveMode: account.receiveMode,
    sendReadReceipts: account.sendReadReceipts,
  };
  if (account.cliPath) opts.cliPath = account.cliPath;
  return opts;
}
