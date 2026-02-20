// Watchdog tests
import { beforeEach, describe, expect, it, vi } from "vitest";
import { Watchdog } from "./watchdog.js";

describe("Watchdog", () => {
  beforeEach(() => {
    vi.stubGlobal("fetch", vi.fn());
  });

  it("initializes all services as healthy", () => {
    const wd = new Watchdog({
      services: [
        { name: "neo4j", url: "http://localhost:7474" },
        { name: "qdrant", url: "http://localhost:6333" },
      ],
      intervalMs: 60_000,
      alertFn: vi.fn(),
    });
    const status = wd.getStatus();
    expect(status).toHaveLength(2);
    expect(status.every((s) => s.healthy)).toBe(true);
  });

  it("getStatus returns name and since timestamp", () => {
    const wd = new Watchdog({
      services: [{ name: "test", url: "http://localhost:1234" }],
      intervalMs: 60_000,
      alertFn: vi.fn(),
    });
    const status = wd.getStatus();
    expect(status[0]!.name).toBe("test");
    expect(status[0]!.since).toBeDefined();
  });

  it("start and stop lifecycle", () => {
    const wd = new Watchdog({
      services: [{ name: "test", url: "http://localhost:1234" }],
      intervalMs: 60_000,
      alertFn: vi.fn(),
    });
    wd.start();
    wd.stop();
    // Should not throw
  });

  it("start is idempotent", () => {
    const wd = new Watchdog({
      services: [{ name: "test", url: "http://localhost:1234" }],
      intervalMs: 60_000,
      alertFn: vi.fn(),
    });
    wd.start();
    wd.start(); // second call should be no-op
    wd.stop();
  });
});
