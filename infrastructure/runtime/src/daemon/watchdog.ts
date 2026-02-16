// Service health watchdog — probes infrastructure services, alerts on state changes
import { createLogger } from "../koina/logger.js";

const log = createLogger("daemon:watchdog");

export interface ServiceProbe {
  name: string;
  url: string;
  timeoutMs?: number;
}

export interface WatchdogOpts {
  services: ServiceProbe[];
  intervalMs: number;
  alertFn: (message: string) => Promise<void>;
}

interface ServiceState {
  healthy: boolean;
  lastChange: number;
  consecutiveFailures: number;
}

export class Watchdog {
  private states = new Map<string, ServiceState>();
  private timer: ReturnType<typeof setInterval> | null = null;
  private opts: WatchdogOpts;

  constructor(opts: WatchdogOpts) {
    this.opts = opts;
    for (const svc of opts.services) {
      this.states.set(svc.name, {
        healthy: true,
        lastChange: Date.now(),
        consecutiveFailures: 0,
      });
    }
  }

  start(): void {
    if (this.timer) return;
    log.info(`Watchdog started: ${this.opts.services.length} services, ${this.opts.intervalMs / 1000}s interval`);
    this.timer = setInterval(() => this.check(), this.opts.intervalMs);
    // Run first check after a brief delay (let services stabilize on boot)
    setTimeout(() => this.check(), 15000);
  }

  stop(): void {
    if (this.timer) {
      clearInterval(this.timer);
      this.timer = null;
    }
    log.info("Watchdog stopped");
  }

  getStatus(): Array<{ name: string; healthy: boolean; since: string }> {
    return Array.from(this.states.entries()).map(([name, state]) => ({
      name,
      healthy: state.healthy,
      since: new Date(state.lastChange).toISOString(),
    }));
  }

  private async check(): Promise<void> {
    const alerts: string[] = [];

    for (const svc of this.opts.services) {
      const healthy = await probeService(svc);
      const state = this.states.get(svc.name)!;

      if (healthy && !state.healthy) {
        state.healthy = true;
        state.lastChange = Date.now();
        state.consecutiveFailures = 0;
        alerts.push(`[recovered] ${svc.name} is back up`);
        log.info(`${svc.name} recovered`);
      } else if (!healthy && state.healthy) {
        state.consecutiveFailures++;
        // Alert after 2 consecutive failures to avoid transient blips
        if (state.consecutiveFailures >= 2) {
          state.healthy = false;
          state.lastChange = Date.now();
          alerts.push(`[down] ${svc.name} is unreachable (${svc.url})`);
          log.warn(`${svc.name} is DOWN`);
        }
      } else if (!healthy) {
        state.consecutiveFailures++;
      } else {
        state.consecutiveFailures = 0;
      }
    }

    if (alerts.length > 0) {
      const message = `Watchdog Alert\n${alerts.join("\n")}`;
      try {
        await this.opts.alertFn(message);
      } catch (err) {
        log.error(`Failed to send watchdog alert: ${err instanceof Error ? err.message : err}`);
      }
    }
  }
}

async function probeService(svc: ServiceProbe): Promise<boolean> {
  const timeout = svc.timeoutMs ?? 3000;
  try {
    const res = await fetch(svc.url, {
      signal: AbortSignal.timeout(timeout),
    });
    return res.ok;
  } catch {
    return false;
  }
}
