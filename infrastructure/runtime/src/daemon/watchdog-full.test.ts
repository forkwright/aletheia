// Extended watchdog tests — health check logic, alerts, state transitions
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { Watchdog } from "./watchdog.js";

describe("Watchdog health checks", () => {
  beforeEach(() => {
    vi.stubGlobal("fetch", vi.fn());
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("detects service going down after 2 consecutive failures", async () => {
    const alertFn = vi.fn().mockResolvedValue(undefined);
    vi.mocked(fetch).mockRejectedValue(new Error("Connection refused"));

    const wd = new Watchdog({
      services: [{ name: "neo4j", url: "http://localhost:7474" }],
      intervalMs: 1000,
      alertFn,
    });
    wd.start();

    // First check — 1 consecutive failure, not yet marked down
    await vi.advanceTimersByTimeAsync(16000);
    // Second check — 2 consecutive failures, marked down
    await vi.advanceTimersByTimeAsync(1000);

    const status = wd.getStatus();
    expect(status[0]!.healthy).toBe(false);
    expect(alertFn).toHaveBeenCalledWith(expect.stringContaining("[down] neo4j"));
    wd.stop();
  });

  it("detects service recovery", async () => {
    const alertFn = vi.fn().mockResolvedValue(undefined);
    let callCount = 0;
    vi.mocked(fetch).mockImplementation(async () => {
      callCount++;
      if (callCount <= 3) {
        throw new Error("Connection refused");
      }
      return { ok: true } as Response;
    });

    const wd = new Watchdog({
      services: [{ name: "neo4j", url: "http://localhost:7474" }],
      intervalMs: 1000,
      alertFn,
    });
    wd.start();

    // First two checks fail — service goes down
    await vi.advanceTimersByTimeAsync(16000);
    await vi.advanceTimersByTimeAsync(1000);
    expect(wd.getStatus()[0]!.healthy).toBe(false);

    // Third check also fails (still down)
    await vi.advanceTimersByTimeAsync(1000);

    // Fourth check succeeds — recovery
    await vi.advanceTimersByTimeAsync(1000);
    expect(wd.getStatus()[0]!.healthy).toBe(true);
    expect(alertFn).toHaveBeenCalledWith(expect.stringContaining("[recovered]"));
    wd.stop();
  });

  it("stays healthy when service responds ok", async () => {
    const alertFn = vi.fn().mockResolvedValue(undefined);
    vi.mocked(fetch).mockResolvedValue({ ok: true } as Response);

    const wd = new Watchdog({
      services: [{ name: "test", url: "http://localhost:1234" }],
      intervalMs: 1000,
      alertFn,
    });
    wd.start();

    await vi.advanceTimersByTimeAsync(16000);
    await vi.advanceTimersByTimeAsync(1000);

    expect(wd.getStatus()[0]!.healthy).toBe(true);
    expect(alertFn).not.toHaveBeenCalled();
    wd.stop();
  });

  it("probes multiple services independently", async () => {
    const alertFn = vi.fn().mockResolvedValue(undefined);
    vi.mocked(fetch).mockImplementation(async (input: string | URL | Request) => {
      const url = typeof input === "string" ? input : input instanceof URL ? input.toString() : input.url;
      if (url.includes("7474")) throw new Error("refused");
      return { ok: true } as Response;
    });

    const wd = new Watchdog({
      services: [
        { name: "neo4j", url: "http://localhost:7474" },
        { name: "qdrant", url: "http://localhost:6333" },
      ],
      intervalMs: 1000,
      alertFn,
    });
    wd.start();

    // Two checks for consecutive failure threshold
    await vi.advanceTimersByTimeAsync(16000);
    await vi.advanceTimersByTimeAsync(1000);

    const status = wd.getStatus();
    const neo4j = status.find((s) => s.name === "neo4j");
    const qdrant = status.find((s) => s.name === "qdrant");
    expect(neo4j!.healthy).toBe(false);
    expect(qdrant!.healthy).toBe(true);
    wd.stop();
  });

  it("handles alert function failure gracefully", async () => {
    const alertFn = vi.fn().mockRejectedValue(new Error("alert failed"));
    vi.mocked(fetch).mockRejectedValue(new Error("refused"));

    const wd = new Watchdog({
      services: [{ name: "test", url: "http://localhost:1234" }],
      intervalMs: 1000,
      alertFn,
    });
    wd.start();

    // Should not throw even when alertFn rejects
    await vi.advanceTimersByTimeAsync(16000);
    await vi.advanceTimersByTimeAsync(1000);

    expect(alertFn).toHaveBeenCalled();
    wd.stop();
  });

  it("does not alert on single failure (requires 2 consecutive)", async () => {
    const alertFn = vi.fn().mockResolvedValue(undefined);
    let callCount = 0;
    vi.mocked(fetch).mockImplementation(async () => {
      callCount++;
      if (callCount === 1) throw new Error("blip");
      return { ok: true } as Response;
    });

    const wd = new Watchdog({
      services: [{ name: "test", url: "http://localhost:1234" }],
      intervalMs: 1000,
      alertFn,
    });
    wd.start();

    // First check fails, second check succeeds
    await vi.advanceTimersByTimeAsync(16000);
    await vi.advanceTimersByTimeAsync(1000);

    expect(wd.getStatus()[0]!.healthy).toBe(true);
    expect(alertFn).not.toHaveBeenCalled();
    wd.stop();
  });
});
