// Cron scheduler tests
import { afterEach, describe, expect, it, vi } from "vitest";
import { CronScheduler } from "./cron.js";

function makeConfig(jobs: Array<Record<string, unknown>> = []) {
  return {
    cron: { jobs },
  } as never;
}

function makeManager() {
  return {
    handleMessage: vi.fn().mockResolvedValue({ text: "ok" }),
  } as never;
}

describe("CronScheduler", () => {
  let scheduler: CronScheduler;

  afterEach(() => {
    scheduler?.stop();
  });

  it("starts with no jobs configured", () => {
    scheduler = new CronScheduler(makeConfig(), makeManager());
    scheduler.start();
    expect(scheduler.getStatus()).toHaveLength(0);
  });

  it("loads enabled jobs on start", () => {
    scheduler = new CronScheduler(makeConfig([
      { id: "heartbeat", enabled: true, schedule: "every 45m", agentId: "syn", timeoutSeconds: 30 },
      { id: "disabled", enabled: false, schedule: "every 1h", timeoutSeconds: 30 },
    ]), makeManager());
    scheduler.start();
    const status = scheduler.getStatus();
    expect(status).toHaveLength(1);
    expect(status[0]!.id).toBe("heartbeat");
  });

  it("getStatus includes nextRun as ISO string", () => {
    scheduler = new CronScheduler(makeConfig([
      { id: "test", enabled: true, schedule: "every 30m", timeoutSeconds: 60 },
    ]), makeManager());
    scheduler.start();
    const status = scheduler.getStatus();
    expect(status[0]!.nextRun).toMatch(/^\d{4}-\d{2}-\d{2}T/);
    expect(status[0]!.lastRun).toBeNull();
  });

  it("stop clears timer", () => {
    scheduler = new CronScheduler(makeConfig([
      { id: "test", enabled: true, schedule: "every 1m", timeoutSeconds: 30 },
    ]), makeManager());
    scheduler.start();
    scheduler.stop();
    // Should not throw or continue running
  });

  it("start is idempotent", () => {
    scheduler = new CronScheduler(makeConfig([
      { id: "test", enabled: true, schedule: "every 1h", timeoutSeconds: 30 },
    ]), makeManager());
    scheduler.start();
    scheduler.start(); // second call should be no-op
    expect(scheduler.getStatus()).toHaveLength(1);
  });
});
