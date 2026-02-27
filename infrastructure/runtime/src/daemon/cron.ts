// Cron scheduler — dispatch timed messages to agents or run shell commands
import { exec as execCb } from "node:child_process";
import { promisify } from "node:util";

const execAsync = promisify(execCb);
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
  command?: string | undefined;
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
  private builtInCommands = new Map<string, () => Promise<string>>();
  private timezone: string;

  constructor(
    private config: AletheiaConfig,
    private manager: NousManager,
  ) {
    this.timezone = config.agents.defaults.userTimezone ?? "UTC";
  }

  /**
   * Register a built-in command that can be referenced from cron jobs.
   * When a cron job's `command` matches the registered name, the callback
   * runs instead of execSync.
   */
  registerCommand(name: string, handler: () => Promise<string>): void {
    this.builtInCommands.set(name, handler);
    log.info(`Registered built-in cron command: ${name}`);
  }

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
        command: j.command,
        schedule: j.schedule,
        timeoutSeconds: j.timeoutSeconds,
        nextRun: computeNextRun(j.schedule, undefined, this.timezone),
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
      entry.nextRun = computeNextRun(entry.schedule, now, this.timezone);
    }

    const results = await Promise.allSettled(
      dueEntries.map((entry) => {
        log.info(`Cron job ${entry.id} firing`);
        const timeoutMs = (entry.timeoutSeconds || 300) * 1000;

        // Command-type jobs: check built-in commands first, then fall through to shell
        if (entry.command) {
          const builtIn = this.builtInCommands.get(entry.command);
          if (builtIn) {
            return Promise.race([
              builtIn().then((stdout) => {
                log.info(`Built-in command ${entry.id} completed: ${stdout.slice(0, 200)}`);
                return { type: "command" as const, stdout };
              }),
              new Promise<never>((_, reject) =>
                setTimeout(() => reject(new Error(`Timed out after ${timeoutMs / 1000}s`)), timeoutMs),
              ),
            ]);
          }

          return execAsync(entry.command!, {
            timeout: timeoutMs,
            encoding: "utf-8",
          }).then(({ stdout }) => {
            const out = String(stdout);
            log.info(`Cron command ${entry.id} completed: ${out.slice(0, 200)}`);
            return { type: "command" as const, stdout: out.slice(0, 2000) };
          });
        }

        // Message-type jobs dispatch to an agent
        const message =
          entry.messageTemplate ?? `[cron:${entry.id}] Scheduled trigger`;

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

/** Get date/time components in the specified timezone using Intl API. */
function tzParts(epochMs: number, tz: string): { year: number; month: number; day: number; dow: number; hour: number; minute: number } {
  const d = new Date(epochMs);
  // Intl.DateTimeFormat with hourCycle: "h23" gives 0-23 hours
  const fmt = new Intl.DateTimeFormat("en-US", {
    timeZone: tz,
    year: "numeric", month: "numeric", day: "numeric",
    hour: "numeric", minute: "numeric",
    weekday: "short",
    hourCycle: "h23",
  });
  const parts = Object.fromEntries(fmt.formatToParts(d).map(p => [p.type, p.value]));
  const dowMap: Record<string, number> = { Sun: 0, Mon: 1, Tue: 2, Wed: 3, Thu: 4, Fri: 5, Sat: 6 };
  return {
    year: parseInt(parts["year"]!, 10),
    month: parseInt(parts["month"]!, 10),
    day: parseInt(parts["day"]!, 10),
    dow: dowMap[parts["weekday"]!] ?? 0,
    hour: parseInt(parts["hour"]!, 10),
    minute: parseInt(parts["minute"]!, 10),
  };
}

function computeNextRun(schedule: string, from?: number, tz = "UTC"): number {
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
    const targetHour = parseInt(timeMatch[1]!, 10);
    const targetMinute = parseInt(timeMatch[2]!, 10);
    // Delegate to cron expression parser which handles timezone correctly
    return computeFromCronExpr([String(targetMinute), String(targetHour), "*", "*", "*"], now, tz);
  }

  const cronParts = schedule.split(/\s+/);
  if (cronParts.length === 5) {
    return computeFromCronExpr(cronParts, now, tz);
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

function computeFromCronExpr(parts: string[], from: number, tz = "UTC"): number {
  const [minStr, hourStr, domStr, monStr, dowStr] = parts;
  const minutes = parseCronField(minStr!, 0, 59);
  const hours = parseCronField(hourStr!, 0, 23);
  const doms = parseCronField(domStr!, 1, 31);
  const months = parseCronField(monStr!, 1, 12);
  const dows = parseCronField(dowStr!, 0, 7); // 0=Sun, 7=Sun

  // Scan forward in 1-minute increments using timezone-aware field extraction.
  // Start from next minute boundary.
  let candidate = from - (from % 60000) + 60000; // next minute boundary
  const limit = from + 400 * 24 * 60 * 60 * 1000;

  while (candidate < limit) {
    const p = tzParts(candidate, tz);
    const mo = p.month;
    const dom = p.day;
    const dow = p.dow;
    const hr = p.hour;
    const mn = p.minute;

    if (!fieldMatches(months, mo)) {
      // Skip forward ~1 day at a time to avoid minute-by-minute scan through wrong months
      candidate += 24 * 60 * 60 * 1000;
      continue;
    }

    // Day matching: if both dom and dow are specified (non-wildcard), match either (cron standard)
    const domMatch = fieldMatches(doms, dom);
    const dowMatch = fieldMatches(dows, dow) || (dows !== null && dows.has(7) && dow === 0);
    const dayOk = doms !== null && dows !== null
      ? domMatch || dowMatch
      : domMatch && dowMatch;

    if (!dayOk) {
      // Skip to next day: advance by enough minutes to reach next midnight in tz
      // Approximate: skip forward by (24 - hr) hours - mn minutes
      const minutesToMidnight = (24 - hr) * 60 - mn;
      candidate += minutesToMidnight * 60000;
      continue;
    }

    if (!fieldMatches(hours, hr)) {
      // Skip to next hour
      candidate += (60 - mn) * 60000;
      continue;
    }

    if (!fieldMatches(minutes, mn)) {
      candidate += 60000; // next minute
      continue;
    }

    return candidate;
  }

  // Fallback if no match found within scan window
  log.warn(`Cron expression ${parts.join(" ")} found no match within 400 days, defaulting to 1h`);
  return from + 60 * 60 * 1000;
}
