// Cron scheduler — dispatch timed messages to agents
import { createLogger } from "../koina/logger.js";
import type { NousManager } from "../nous/manager.js";
import type { AletheiaConfig } from "../taxis/schema.js";

const log = createLogger("daemon:cron");

interface CronEntry {
  id: string;
  agentId?: string;
  sessionKey?: string;
  model?: string;
  messageTemplate?: string;
  schedule: string;
  timeoutSeconds: number;
  lastRun?: number;
  nextRun: number;
}

export class CronScheduler {
  private entries: CronEntry[] = [];
  private timer: ReturnType<typeof setTimeout> | null = null;
  private running = false;
  private ticking = false;

  constructor(
    private config: AletheiaConfig,
    private manager: NousManager,
  ) {}

  start(): void {
    if (this.running) return;

    this.entries = this.config.cron.jobs
      .filter((j) => j.enabled)
      .map((j) => ({
        id: j.id,
        agentId: j.agentId,
        sessionKey: j.sessionKey,
        model: j.model,
        messageTemplate: j.messageTemplate,
        schedule: j.schedule,
        timeoutSeconds: j.timeoutSeconds,
        nextRun: computeNextRun(j.schedule),
      }));

    if (this.entries.length === 0) {
      log.info("No cron jobs configured");
      return;
    }

    log.info(`Cron scheduler started with ${this.entries.length} jobs`);

    this.running = true;
    this.scheduleTick();
  }

  stop(): void {
    this.running = false;
    if (this.timer) {
      clearTimeout(this.timer);
      this.timer = null;
    }
    log.info("Cron scheduler stopped");
  }

  private scheduleTick(): void {
    if (!this.running) return;
    this.timer = setTimeout(async () => {
      if (this.ticking) {
        this.scheduleTick();
        return;
      }
      this.ticking = true;
      try {
        await this.tick();
      } finally {
        this.ticking = false;
        this.scheduleTick();
      }
    }, 30000);
  }

  private async tick(): Promise<void> {
    const now = Date.now();

    for (const entry of this.entries) {
      if (now < entry.nextRun) continue;

      entry.lastRun = now;
      entry.nextRun = computeNextRun(entry.schedule, now);

      log.info(`Cron job ${entry.id} firing`);

      const message =
        entry.messageTemplate ?? `[cron:${entry.id}] Scheduled trigger`;

      try {
        await this.manager.handleMessage({
          text: message,
          nousId: entry.agentId,
          sessionKey: entry.sessionKey ?? `cron:${entry.id}`,
          channel: "cron",
          peerKind: "system",
        });
      } catch (err) {
        log.error(
          `Cron job ${entry.id} failed: ${err instanceof Error ? err.message : err}`,
        );
      }
    }
  }
}

function computeNextRun(schedule: string, from?: number): number {
  const now = from ?? Date.now();

  const intervalMatch = schedule.match(/^every\s+(\d+)\s*(m|h|min|hour|s|sec)/i);
  if (intervalMatch) {
    const value = parseInt(intervalMatch[1], 10);
    const unit = intervalMatch[2].toLowerCase();

    let ms: number;
    if (unit.startsWith("h")) ms = value * 60 * 60 * 1000;
    else if (unit.startsWith("m")) ms = value * 60 * 1000;
    else ms = value * 1000;

    return now + ms;
  }

  const timeMatch = schedule.match(/^at\s+(\d{1,2}):(\d{2})/i);
  if (timeMatch) {
    const hour = parseInt(timeMatch[1], 10);
    const minute = parseInt(timeMatch[2], 10);
    const date = new Date(now);
    date.setHours(hour, minute, 0, 0);
    if (date.getTime() <= now) {
      date.setDate(date.getDate() + 1);
    }
    return date.getTime();
  }

  const cronParts = schedule.split(/\s+/);
  if (cronParts.length === 5) {
    return computeFromCronExpr(cronParts, now);
  }

  log.warn(`Unknown cron schedule format: ${schedule}, defaulting to 1h`);
  return now + 60 * 60 * 1000;
}

function computeFromCronExpr(parts: string[], from: number): number {
  const [minStr, hourStr] = parts;
  const now = new Date(from);

  const targetMin = minStr === "*" ? now.getMinutes() : parseInt(minStr, 10);
  const targetHour = hourStr === "*" ? now.getHours() : parseInt(hourStr, 10);

  const next = new Date(now);
  next.setSeconds(0, 0);
  next.setMinutes(targetMin);
  next.setHours(targetHour);

  if (next.getTime() <= from) {
    if (hourStr === "*") {
      next.setHours(next.getHours() + 1);
    } else {
      next.setDate(next.getDate() + 1);
    }
  }

  return next.getTime();
}
