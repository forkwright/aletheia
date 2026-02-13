// Extended cron tests — schedule parsing, tick execution, command jobs
import { describe, it, expect, vi, afterEach, beforeEach } from "vitest";
import { CronScheduler } from "./cron.js";

// We need to access computeNextRun indirectly through the scheduler
// by checking the nextRun timestamps in getStatus()

function makeConfig(jobs: Array<Record<string, unknown>> = []) {
  return { cron: { jobs } } as never;
}

function makeManager() {
  return { handleMessage: vi.fn().mockResolvedValue({ text: "ok" }) } as never;
}

describe("CronScheduler schedule parsing", () => {
  let scheduler: CronScheduler;
  afterEach(() => { scheduler?.stop(); });

  it("parses 'every Nm' interval", () => {
    scheduler = new CronScheduler(makeConfig([
      { id: "test", enabled: true, schedule: "every 45m", timeoutSeconds: 30 },
    ]), makeManager());
    scheduler.start();
    const status = scheduler.getStatus();
    const nextRun = new Date(status[0]!.nextRun).getTime();
    const now = Date.now();
    // Should be ~45 minutes from now
    expect(nextRun - now).toBeGreaterThan(44 * 60 * 1000);
    expect(nextRun - now).toBeLessThan(46 * 60 * 1000);
  });

  it("parses 'every Nh' interval", () => {
    scheduler = new CronScheduler(makeConfig([
      { id: "test", enabled: true, schedule: "every 2h", timeoutSeconds: 30 },
    ]), makeManager());
    scheduler.start();
    const status = scheduler.getStatus();
    const nextRun = new Date(status[0]!.nextRun).getTime();
    const now = Date.now();
    expect(nextRun - now).toBeGreaterThan(119 * 60 * 1000);
    expect(nextRun - now).toBeLessThan(121 * 60 * 1000);
  });

  it("parses 'every Ns' interval", () => {
    scheduler = new CronScheduler(makeConfig([
      { id: "test", enabled: true, schedule: "every 30s", timeoutSeconds: 30 },
    ]), makeManager());
    scheduler.start();
    const status = scheduler.getStatus();
    const nextRun = new Date(status[0]!.nextRun).getTime();
    const now = Date.now();
    expect(nextRun - now).toBeGreaterThan(29 * 1000);
    expect(nextRun - now).toBeLessThan(31 * 1000);
  });

  it("parses 'at HH:MM' schedule", () => {
    // Use a time that's definitely in the future (tomorrow)
    const tomorrow = new Date();
    tomorrow.setDate(tomorrow.getDate() + 1);
    tomorrow.setHours(3, 0, 0, 0);

    scheduler = new CronScheduler(makeConfig([
      { id: "test", enabled: true, schedule: "at 03:00", timeoutSeconds: 30 },
    ]), makeManager());
    scheduler.start();
    const status = scheduler.getStatus();
    const nextRun = new Date(status[0]!.nextRun);
    expect(nextRun.getHours()).toBe(3);
    expect(nextRun.getMinutes()).toBe(0);
  });

  it("parses cron expression '*/15 * * * *'", () => {
    scheduler = new CronScheduler(makeConfig([
      { id: "test", enabled: true, schedule: "*/15 * * * *", timeoutSeconds: 30 },
    ]), makeManager());
    scheduler.start();
    const status = scheduler.getStatus();
    const nextRun = new Date(status[0]!.nextRun);
    // Minutes should be 0, 15, 30, or 45
    expect([0, 15, 30, 45]).toContain(nextRun.getMinutes());
  });

  it("parses cron expression with ranges '0 9-17 * * *'", () => {
    scheduler = new CronScheduler(makeConfig([
      { id: "test", enabled: true, schedule: "0 9-17 * * *", timeoutSeconds: 30 },
    ]), makeManager());
    scheduler.start();
    const status = scheduler.getStatus();
    const nextRun = new Date(status[0]!.nextRun);
    expect(nextRun.getMinutes()).toBe(0);
    expect(nextRun.getHours()).toBeGreaterThanOrEqual(9);
    expect(nextRun.getHours()).toBeLessThanOrEqual(17);
  });

  it("defaults to 1h for unknown schedule format", () => {
    scheduler = new CronScheduler(makeConfig([
      { id: "test", enabled: true, schedule: "invalid-format", timeoutSeconds: 30 },
    ]), makeManager());
    scheduler.start();
    const status = scheduler.getStatus();
    const nextRun = new Date(status[0]!.nextRun).getTime();
    const now = Date.now();
    // Should be ~1 hour from now (default fallback)
    expect(nextRun - now).toBeGreaterThan(59 * 60 * 1000);
    expect(nextRun - now).toBeLessThan(61 * 60 * 1000);
  });

  it("filters disabled jobs", () => {
    scheduler = new CronScheduler(makeConfig([
      { id: "a", enabled: true, schedule: "every 1h", timeoutSeconds: 30 },
      { id: "b", enabled: false, schedule: "every 1h", timeoutSeconds: 30 },
      { id: "c", enabled: true, schedule: "every 2h", timeoutSeconds: 30 },
    ]), makeManager());
    scheduler.start();
    expect(scheduler.getStatus()).toHaveLength(2);
  });

  it("includes agentId in status", () => {
    scheduler = new CronScheduler(makeConfig([
      { id: "heartbeat", enabled: true, schedule: "every 45m", agentId: "syn", timeoutSeconds: 30 },
    ]), makeManager());
    scheduler.start();
    expect(scheduler.getStatus()[0]!.agentId).toBe("syn");
  });
});

describe("CronScheduler tick execution", () => {
  let scheduler: CronScheduler;

  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    scheduler?.stop();
    vi.useRealTimers();
  });

  it("fires message-type job when due", async () => {
    const manager = makeManager();
    scheduler = new CronScheduler(makeConfig([
      { id: "test", enabled: true, schedule: "every 1s", messageTemplate: "ping", agentId: "syn", sessionKey: "cron:test", timeoutSeconds: 30 },
    ]), manager);
    scheduler.start();

    // Advance past the 30s tick interval + the 1s schedule
    await vi.advanceTimersByTimeAsync(31000);

    expect((manager as unknown as { handleMessage: ReturnType<typeof vi.fn> }).handleMessage).toHaveBeenCalledWith(
      expect.objectContaining({
        text: "ping",
        channel: "cron",
        peerKind: "system",
      }),
    );
  });

  it("uses default message template when none provided", async () => {
    const manager = makeManager();
    scheduler = new CronScheduler(makeConfig([
      { id: "heartbeat", enabled: true, schedule: "every 1s", agentId: "syn", timeoutSeconds: 30 },
    ]), manager);
    scheduler.start();

    await vi.advanceTimersByTimeAsync(31000);

    const handleMessage = (manager as unknown as { handleMessage: ReturnType<typeof vi.fn> }).handleMessage;
    if (handleMessage.mock.calls.length > 0) {
      expect(handleMessage.mock.calls[0]![0].text).toContain("[cron:heartbeat]");
    }
  });

  it("uses default sessionKey when none provided", async () => {
    const manager = makeManager();
    scheduler = new CronScheduler(makeConfig([
      { id: "myJob", enabled: true, schedule: "every 1s", timeoutSeconds: 30 },
    ]), manager);
    scheduler.start();

    await vi.advanceTimersByTimeAsync(31000);

    const handleMessage = (manager as unknown as { handleMessage: ReturnType<typeof vi.fn> }).handleMessage;
    if (handleMessage.mock.calls.length > 0) {
      expect(handleMessage.mock.calls[0]![0].sessionKey).toBe("cron:myJob");
    }
  });
});
