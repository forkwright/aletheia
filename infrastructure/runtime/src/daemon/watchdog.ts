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

const MAX_CONSECUTIVE_FAILURES = 100;
const RE_ALERT_INTERVAL_MS = 43200_000; // 12 hours

interface ServiceState {
  healthy: boolean;
  lastChange: number;
  consecutiveFailures: number;
  lastAlertedAt: number;
}

export class Watchdog {
  private states = new Map<string, ServiceState>();
  private timer: ReturnType<typeof setTimeout> | null = null;
  private running = false;
  private opts: WatchdogOpts;

  constructor(opts: WatchdogOpts) {
    this.opts = opts;
    for (const svc of opts.services) {
      this.states.set(svc.name, {
        healthy: true,
        lastChange: Date.now(),
        consecutiveFailures: 0,
        lastAlertedAt: 0,
      });
    }
  }

  start(): void {
    if (this.running) return;
    this.running = true;
    log.info(`Watchdog started: ${this.opts.services.length} services, ${this.opts.intervalMs / 1000}s interval`);
    this.timer = setTimeout(() => this.run(), 15000);
  }

  stop(): void {
    this.running = false;
    if (this.timer) {
      clearTimeout(this.timer);
      this.timer = null;
    }
    log.info("Watchdog stopped");
  }

  private async run(): Promise<void> {
    try {
      await this.check();
    } finally {
      if (this.running) {
        this.timer = setTimeout(() => this.run(), this.opts.intervalMs);
      }
    }
  }

  getStatus(): Array<{ name: string; healthy: boolean; since: string }> {
    return Array.from(this.states.entries()).map(([name, state]) => ({
      name,
      healthy: state.healthy,
      since: new Date(state.lastChange).toISOString(),
    }));
  }

  private async check(): Promise<void> {
    const now = Date.now();
    const alerts: string[] = [];

    const probeResults = await Promise.allSettled(
      this.opts.services.map((svc) => probeService(svc).then((healthy) => ({ svc, healthy }))),
    );

    for (const result of probeResults) {
      if (result.status === "rejected") continue;
      const { svc, healthy } = result.value;
      const state = this.states.get(svc.name)!;

      if (healthy && !state.healthy) {
        state.healthy = true;
        state.lastChange = now;
        state.consecutiveFailures = 0;
        state.lastAlertedAt = now;
        alerts.push(`[recovered] ${svc.name} is back up`);
        log.info(`${svc.name} recovered`);
      } else if (!healthy && state.healthy) {
        state.consecutiveFailures = Math.min(state.consecutiveFailures + 1, MAX_CONSECUTIVE_FAILURES);
        if (state.consecutiveFailures >= 2) {
          state.healthy = false;
          state.lastChange = now;
          state.lastAlertedAt = now;
          alerts.push(`[down] ${svc.name} is unreachable (${svc.url})`);
          log.warn(`${svc.name} is DOWN`);
        }
      } else if (!healthy) {
        state.consecutiveFailures = Math.min(state.consecutiveFailures + 1, MAX_CONSECUTIVE_FAILURES);
        if (now - state.lastAlertedAt >= RE_ALERT_INTERVAL_MS) {
          state.lastAlertedAt = now;
          alerts.push(`[still down] ${svc.name} unreachable for ${Math.round((now - state.lastChange) / 60000)}m (${svc.url})`);
          log.warn(`${svc.name} still DOWN, re-alerting`);
        }
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
