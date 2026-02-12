// Cron scheduler — dispatch timed messages to agents
import { createLogger } from "../koina/logger.js";
import type { NousManager } from "../nous/manager.js";
import type { AletheiaConfig } from "../taxis/schema.js";

const log = createLogger("daemon:cron");

interface CronEntry {
  id: string;
  agentId?: string | undefined;
  sessionKey?: string | undefined;
  model?: string | undefined;
  messageTemplate?: string | undefined;
  schedule: string;
  timeoutSeconds: number;
  lastRun?: number | undefined;
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

  getStatus(): Array<{
    id: string;
    agentId?: string | undefined;
    schedule: string;
    lastRun: string | null;
    nextRun: string;
  }> {
    return this.entries.map((e) => ({
      id: e.id,
      agentId: e.agentId,
      schedule: e.schedule,
      lastRun: e.lastRun ? new Date(e.lastRun).toISOString() : null,
      nextRun: new Date(e.nextRun).toISOString(),
    }));
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

    const dueEntries = this.entries.filter((e) => now >= e.nextRun);
    if (dueEntries.length === 0) return;

    for (const entry of dueEntries) {
      entry.lastRun = now;
      entry.nextRun = computeNextRun(entry.schedule, now);
    }

    const results = await Promise.allSettled(
      dueEntries.map((entry) => {
        log.info(`Cron job ${entry.id} firing`);
        const message =
          entry.messageTemplate ?? `[cron:${entry.id}] Scheduled trigger`;
        const timeoutMs = (entry.timeoutSeconds || 300) * 1000;

        return Promise.race([
          this.manager.handleMessage({
            text: message,
            sessionKey: entry.sessionKey ?? `cron:${entry.id}`,
            channel: "cron",
            peerKind: "system",
            ...(entry.agentId !== undefined && { nousId: entry.agentId }),
            ...(entry.model !== undefined && { model: entry.model }),
          }),
          new Promise<never>((_, reject) =>
            setTimeout(() => reject(new Error(`Timed out after ${timeoutMs / 1000}s`)), timeoutMs),
          ),
        ]);
      }),
    );

    for (let i = 0; i < results.length; i++) {
      const result = results[i];
      const entry = dueEntries[i];
      if (result && entry && result.status === "rejected") {
        const reason = (result as PromiseRejectedResult).reason;
        log.error(
          `Cron job ${entry.id} failed: ${reason instanceof Error ? reason.message : reason}`,
        );
      }
    }
  }
}

function computeNextRun(schedule: string, from?: number): number {
  const now = from ?? Date.now();

  const intervalMatch = schedule.match(/^every\s+(\d+)\s*(m|h|min|hour|s|sec)/i);
  if (intervalMatch) {
    const value = parseInt(intervalMatch[1]!, 10);
    const unit = intervalMatch[2]!.toLowerCase();

    let ms: number;
    if (unit.startsWith("h")) ms = value * 60 * 60 * 1000;
    else if (unit.startsWith("m")) ms = value * 60 * 1000;
    else ms = value * 1000;

    return now + ms;
  }

  const timeMatch = schedule.match(/^at\s+(\d{1,2}):(\d{2})/i);
  if (timeMatch) {
    const hour = parseInt(timeMatch[1]!, 10);
    const minute = parseInt(timeMatch[2]!, 10);
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

function parseCronField(field: string, min: number, max: number): Set<number> | null {
  if (field === "*") return null; // wildcard
  const values = new Set<number>();
  for (const part of field.split(",")) {
    const stepMatch = part.match(/^(\d+|\*)\/(\d+)$/);
    if (stepMatch) {
      const start = stepMatch[1] === "*" ? min : parseInt(stepMatch[1]!, 10);
      const step = parseInt(stepMatch[2]!, 10);
      for (let i = start; i <= max; i += step) values.add(i);
      continue;
    }
    const rangeMatch = part.match(/^(\d+)-(\d+)$/);
    if (rangeMatch) {
      const lo = parseInt(rangeMatch[1]!, 10);
      const hi = parseInt(rangeMatch[2]!, 10);
      for (let i = lo; i <= hi; i++) values.add(i);
      continue;
    }
    values.add(parseInt(part, 10));
  }
  return values;
}

function fieldMatches(field: Set<number> | null, value: number): boolean {
  return field === null || field.has(value);
}

function computeFromCronExpr(parts: string[], from: number): number {
  const [minStr, hourStr, domStr, monStr, dowStr] = parts;
  const minutes = parseCronField(minStr!, 0, 59);
  const hours = parseCronField(hourStr!, 0, 23);
  const doms = parseCronField(domStr!, 1, 31);
  const months = parseCronField(monStr!, 1, 12);
  const dows = parseCronField(dowStr!, 0, 7); // 0=Sun, 7=Sun

  const candidate = new Date(from);
  candidate.setSeconds(0, 0);
  candidate.setMilliseconds(0);

  // Scan forward up to 400 days to find next matching time
  const limit = from + 400 * 24 * 60 * 60 * 1000;
  // Start from next minute
  candidate.setMinutes(candidate.getMinutes() + 1);

  while (candidate.getTime() < limit) {
    const mo = candidate.getMonth() + 1;
    const dom = candidate.getDate();
    const dow = candidate.getDay(); // 0=Sun
    const hr = candidate.getHours();
    const mn = candidate.getMinutes();

    if (!fieldMatches(months, mo)) {
      // Jump to first day of next month
      candidate.setMonth(candidate.getMonth() + 1, 1);
      candidate.setHours(0, 0, 0, 0);
      continue;
    }

    // Day matching: if both dom and dow are specified (non-wildcard), match either (cron standard)
    const domMatch = fieldMatches(doms, dom);
    const dowMatch = fieldMatches(dows, dow) || (dows !== null && dows.has(7) && dow === 0);
    const dayOk = doms !== null && dows !== null
      ? domMatch || dowMatch
      : domMatch && dowMatch;

    if (!dayOk) {
      candidate.setDate(candidate.getDate() + 1);
      candidate.setHours(0, 0, 0, 0);
      continue;
    }

    if (!fieldMatches(hours, hr)) {
      candidate.setHours(candidate.getHours() + 1, 0, 0, 0);
      continue;
    }

    if (!fieldMatches(minutes, mn)) {
      candidate.setMinutes(candidate.getMinutes() + 1, 0, 0);
      continue;
    }

    return candidate.getTime();
  }

  // Fallback if no match found within scan window
  log.warn(`Cron expression ${parts.join(" ")} found no match within 400 days, defaulting to 1h`);
  return from + 60 * 60 * 1000;
}
